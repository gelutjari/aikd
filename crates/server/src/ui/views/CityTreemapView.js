/* AIKD Graph — City / Treemap view (2D squarified treemap + isometric 3D city).
 * Pure offline vanilla JS. Registers window.AIKD_GRAPH.views.treemap.
 */
(function (G) {
  'use strict';

  G.views = G.views || {};

  var COS = Math.cos(Math.PI / 6);
  var SIN = Math.sin(Math.PI / 6);
  var TAU = Math.PI * 2;

  var PALETTE = {
    project: '#38bdf8',
    agent: '#a78bfa',
    memory: '#34d399',
    file: '#fbbf24',
    note: '#f472b6',
    symbol: '#22d3ee',
    tag: '#facc15',
    skill: '#f97316',
    task: '#60a5fa',
    default: '#94a3b8'
  };
  var PROJECT_COLORS = ['#38bdf8', '#a78bfa', '#34d399', '#fbbf24', '#f472b6', '#22d3ee',
    '#facc15', '#fb7185', '#4ade80', '#c084fc', '#fcd34d', '#67e8f9'];

  var S = null;

  /* ------------------------------------------------------------------ utils */

  function num(v, d) {
    var n = Number(v);
    return isFinite(n) ? n : (d || 0);
  }

  function clamp(v, lo, hi) {
    return v < lo ? lo : (v > hi ? hi : v);
  }

  function esc(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) {
      return ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c];
    });
  }

  function toRgb(c) {
    c = String(c || '#94a3b8').trim();
    if (c.charAt(0) === '#') {
      var h = c.slice(1);
      if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
      var n = parseInt(h, 16);
      if (isFinite(n)) return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    }
    var m = c.match(/(\d+(?:\.\d+)?)/g);
    if (m && m.length >= 3) return [+m[0], +m[1], +m[2]];
    return [148, 163, 184];
  }

  function rgba(c, a) {
    var r = toRgb(c);
    return 'rgba(' + (r[0] | 0) + ',' + (r[1] | 0) + ',' + (r[2] | 0) + ',' + a + ')';
  }

  function lighten(c, t) {
    var r = toRgb(c);
    return 'rgb(' + Math.round(r[0] + (255 - r[0]) * t) + ',' +
      Math.round(r[1] + (255 - r[1]) * t) + ',' + Math.round(r[2] + (255 - r[2]) * t) + ')';
  }

  function darken(c, t) {
    var r = toRgb(c);
    return 'rgb(' + Math.round(r[0] * (1 - t)) + ',' +
      Math.round(r[1] * (1 - t)) + ',' + Math.round(r[2] * (1 - t)) + ')';
  }

  function colorOf(kind, helpers) {
    if (helpers && typeof helpers.kindColor === 'function') {
      try {
        var v = helpers.kindColor(kind);
        if (v) return v;
      } catch (e) { /* ignore */ }
    }
    return PALETTE[kind] || PALETTE.default;
  }

  function projectColor(name) {
    var s = String(name || '');
    var h = 0;
    for (var i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
    return PROJECT_COLORS[Math.abs(h) % PROJECT_COLORS.length];
  }

  function labelOf(node, helpers) {
    if (!node) return '';
    if (helpers && typeof helpers.graphLabel === 'function') {
      try {
        var v = helpers.graphLabel(node);
        if (v) return String(v);
      } catch (e) { /* ignore */ }
    }
    return String(node.label || node.name || node.id || '');
  }

  function clampText(s, n) {
    s = String(s == null ? '' : s);
    if (s.length <= n) return s;
    return s.slice(0, Math.max(1, n - 1)) + '…';
  }

  function idOf(leaf) {
    if (!leaf) return null;
    return leaf.id || (leaf.node && leaf.node.id) || null;
  }

  function leafWeight(leaf) {
    var d = leaf.degree != null ? num(leaf.degree) : (leaf.node ? num(leaf.node.degree) : 0);
    if (d > 0) return 1 + d;
    var v = num(leaf.value);
    return v > 0 ? v : 1;
  }

  function pathRoundRect(c, x, y, w, h, r) {
    r = Math.max(0, Math.min(r, w / 2, h / 2));
    c.beginPath();
    c.moveTo(x + r, y);
    c.lineTo(x + w - r, y);
    c.arcTo(x + w, y, x + w, y + r, r);
    c.lineTo(x + w, y + h - r);
    c.arcTo(x + w, y + h, x + w - r, y + h, r);
    c.lineTo(x + r, y + h);
    c.arcTo(x, y + h, x, y + h - r, r);
    c.lineTo(x, y + r);
    c.arcTo(x, y, x + r, y, r);
    c.closePath();
  }

  function pointInPoly(px, py, pts) {
    var inside = false;
    for (var i = 0, j = pts.length - 1; i < pts.length; j = i++) {
      var xi = pts[i].x, yi = pts[i].y;
      var xj = pts[j].x, yj = pts[j].y;
      if (((yi > py) !== (yj > py)) && (px < (xj - xi) * (py - yi) / ((yj - yi) || 1e-6) + xi)) {
        inside = !inside;
      }
    }
    return inside;
  }

  /* ---------------------------------------------------------------- hierarchy */

  function fallbackTree(nodes) {
    var projects = Object.create(null);
    (nodes || []).forEach(function (n) {
      if (!n) return;
      var pName = n.project || 'Unassigned / General';
      var kind = n.kind || 'node';
      var p = projects[pName] || (projects[pName] = { name: pName, kind: 'project', children: Object.create(null) });
      var t = p.children[kind] || (p.children[kind] = { name: kind, kind: kind, children: [] });
      t.children.push({
        name: n.label || n.id,
        id: n.id,
        kind: n.kind || 'node',
        value: 1,
        degree: num(n.degree),
        node: n
      });
    });
    var children = Object.keys(projects).map(function (k) {
      var p = projects[k];
      var types = Object.keys(p.children).map(function (tk) { return p.children[tk]; });
      return { name: p.name, kind: 'project', children: types };
    });
    return { name: 'AIKD', kind: 'root', children: children };
  }

  function getTree(ctx) {
    var a = G.adapter;
    if (a && typeof a.toHierarchyTree === 'function') {
      try {
        var t = a.toHierarchyTree(ctx.nodes || [], ctx.edges || []);
        if (t && t.children && t.children.length) return t;
      } catch (e) { /* fall back */ }
    }
    return fallbackTree(ctx.nodes || []);
  }

  function normalizeTree(tree) {
    var groups = [];
    ((tree && tree.children) || []).forEach(function (g, gi) {
      if (!g) return;
      var types = [];
      var direct = [];
      (g.children || []).forEach(function (child) {
        if (!child) return;
        if (child.node) { direct.push(child); return; }
        var leaves = [];
        (child.children || []).forEach(function (l) {
          if (l && (l.node || l.id)) leaves.push(l);
        });
        if (leaves.length) {
          types.push({
            name: child.name || child.kind || 'items',
            kind: child.kind || 'type',
            leaves: leaves
          });
        }
      });
      if (direct.length) types.push({ name: 'General', kind: 'general', leaves: direct });

      var group = {
        name: g.name || g.label || ('Group ' + (gi + 1)),
        kind: g.kind || 'project',
        id: g.id || null,
        types: [],
        weight: 0
      };
      types.forEach(function (t) {
        var tw = 0;
        t.leaves.forEach(function (l) {
          l.weight = leafWeight(l);
          tw += l.weight;
        });
        if (!t.leaves.length) return;
        t.weight = tw;
        t.key = group.name + '::' + t.name;
        group.types.push(t);
        group.weight += tw;
      });
      if (!group.types.length) return;
      group.key = group.name + '#' + gi;
      groups.push(group);
    });
    groups.sort(function (a, b) { return b.weight - a.weight; });
    return groups;
  }

  function buildAdj(ctx) {
    var map = Object.create(null);
    function add(a, b) {
      if (!a || !b) return;
      var list = map[a] || (map[a] = []);
      if (list.indexOf(b) < 0) list.push(b);
    }
    var edges = (ctx.rawEdges && ctx.rawEdges.length) ? ctx.rawEdges : (ctx.edges || []);
    edges.forEach(function (e) {
      if (!e) return;
      add(e.from, e.to);
      add(e.to, e.from);
    });
    return map;
  }

  function signature(ctx) {
    var h = 0;
    var nodes = ctx.nodes || [];
    for (var i = 0; i < nodes.length; i++) {
      var id = String(nodes[i] && nodes[i].id || '');
      for (var j = 0; j < id.length; j++) h = (h * 31 + id.charCodeAt(j)) | 0;
      h = (h * 31 + num(nodes[i] && nodes[i].degree)) | 0;
    }
    return nodes.length + ':' + h;
  }

  /* ---------------------------------------------------------------- treemap */

  function aspect(a, b) {
    if (a <= 0.001 || b <= 0.001) return Infinity;
    return Math.max(a / b, b / a);
  }

  function splitRect(list, x, y, w, h) {
    var n = list.length;
    if (!n) return;
    if (n === 1) {
      list[0].rect = { x: x, y: y, w: w, h: h };
      return;
    }
    if (w < 2 || h < 2) {
      var cw = w / n;
      for (var q = 0; q < n; q++) list[q].rect = { x: x + q * cw, y: y, w: cw, h: h };
      return;
    }
    var total = 0, i;
    for (i = 0; i < n; i++) total += list[i].weight;
    if (total <= 0) {
      var cw2 = w / n;
      for (i = 0; i < n; i++) list[i].rect = { x: x + i * cw2, y: y, w: cw2, h: h };
      return;
    }
    var best = null;
    var acc = 0;
    for (var k = 1; k < n; k++) {
      acc += list[k - 1].weight;
      var f = clamp(acc / total, 0.03, 0.97);
      var vs = Math.max(aspect(w * f, h), aspect(w * (1 - f), h));
      var hs = Math.max(aspect(w, h * f), aspect(w, h * (1 - f)));
      var score = Math.min(vs, hs);
      if (!best || score < best.score) {
        best = { k: k, score: score, vertical: vs <= hs };
      }
    }
    if (!best) {
      list[0].rect = { x: x, y: y, w: w, h: h };
      return;
    }
    var acc2 = 0;
    for (i = 0; i < best.k; i++) acc2 += list[i].weight;
    var f2 = clamp(acc2 / total, 0.03, 0.97);
    if (best.vertical) {
      splitRect(list.slice(0, best.k), x, y, w * f2, h);
      splitRect(list.slice(best.k), x + w * f2, y, w - w * f2, h);
    } else {
      splitRect(list.slice(0, best.k), x, y, w, h * f2);
      splitRect(list.slice(best.k), x, y + h * f2, w, h - h * f2);
    }
  }

  function layoutRects(items, x, y, w, h) {
    var list = items.slice().sort(function (a, b) { return b.weight - a.weight; });
    splitRect(list, x, y, w, h);
    return list;
  }

  function buildLayout(W, H, pad) {
    pad = pad == null ? 8 : pad;
    var area = {
      x: pad,
      y: pad,
      w: Math.max(24, W - pad * 2),
      h: Math.max(24, H - pad * 2)
    };
    var groups = S.groups.map(function (g) { return { weight: g.weight, group: g }; });
    layoutRects(groups, area.x, area.y, area.w, area.h);

    var out = [];
    groups.forEach(function (gc) {
      var g = gc.group;
      var r = gc.rect;
      var proj = {
        group: g,
        rect: r,
        color: projectColor(g.name),
        labelBand: (r.w > 88 && r.h > 46) ? 15 : 0,
        types: []
      };
      var types = g.types.map(function (t) { return { weight: t.weight, type: t }; });
      layoutRects(types, r.x + 2, r.y + 2 + proj.labelBand,
        Math.max(4, r.w - 4), Math.max(4, r.h - 4 - proj.labelBand));
      types.forEach(function (tc) {
        var tr = tc.rect;
        var leaves = tc.type.leaves.map(function (l) { return { weight: l.weight, leaf: l }; });
        layoutRects(leaves, tr.x + 1, tr.y + 1, Math.max(2, tr.w - 2), Math.max(2, tr.h - 2));
        proj.types.push({
          type: tc.type,
          rect: tr,
          items: leaves.map(function (lc) { return { leaf: lc.leaf, rect: lc.rect }; })
        });
      });
      out.push(proj);
    });
    return out;
  }

  function buildCity() {
    var VW = 1400, VH = 1000, pad = 30;
    var layout = buildLayout(VW, VH, pad);
    var city = {
      districts: [],
      blocks: [],
      buildings: [],
      byId: Object.create(null),
      minX: Infinity,
      minY: Infinity,
      maxX: -Infinity,
      maxY: -Infinity,
      midX: 0,
      midY: 0,
      fit: 1
    };

    function track(x, y, z) {
      var nx = (x - y) * COS;
      var ny = (x + y) * SIN - z;
      if (nx < city.minX) city.minX = nx;
      if (nx > city.maxX) city.maxX = nx;
      if (ny < city.minY) city.minY = ny;
      if (ny > city.maxY) city.maxY = ny;
    }

    layout.forEach(function (proj) {
      var r = proj.rect;
      city.districts.push({ x: r.x, y: r.y, w: r.w, h: r.h, color: proj.color, name: proj.group.name });
      track(r.x, r.y, 0); track(r.x + r.w, r.y, 0);
      track(r.x, r.y + r.h, 0); track(r.x + r.w, r.y + r.h, 0);

      proj.types.forEach(function (ty) {
        var tr = ty.rect;
        var tcolor = colorOf(ty.type.kind, S.ctx.helpers);
        city.blocks.push({
          x: tr.x, y: tr.y, w: tr.w, h: tr.h,
          color: tcolor, name: ty.type.name
        });
        track(tr.x, tr.y, 0); track(tr.x + tr.w, tr.y, 0);
        track(tr.x, tr.y + tr.h, 0); track(tr.x + tr.w, tr.y + tr.h, 0);

        var items = ty.items;
        var cnt = items.length;
        if (!cnt) return;
        var cols = Math.max(1, Math.min(cnt, Math.round(Math.sqrt(cnt * tr.w / Math.max(1, tr.h)))));
        var rows = Math.ceil(cnt / cols);
        var cw = tr.w / cols;
        var ch = tr.h / rows;
        items.forEach(function (it, i) {
          var ci = i % cols;
          var ri = Math.floor(i / cols);
          var bw = Math.max(2.4, cw * 0.6);
          var bd = Math.max(2.4, ch * 0.6);
          var bx = tr.x + ci * cw + (cw - bw) / 2;
          var by = tr.y + ri * ch + (ch - bd) / 2;
          var ht = 12 + Math.min(155, Math.sqrt(Math.max(1, it.leaf.weight)) * 15);
          var b = {
            x: bx, y: by, w: bw, d: bd, h: ht,
            leaf: it.leaf,
            node: it.leaf.node,
            color: colorOf(it.leaf.kind, S.ctx.helpers),
            proj: proj.group,
            type: ty.type
          };
          city.buildings.push(b);
          var id = idOf(it.leaf);
          if (id) city.byId[id] = b;
          track(bx, by, ht); track(bx + bw, by, ht);
          track(bx, by + bd, ht); track(bx + bw, by + bd, ht);
          track(bx, by, 0); track(bx + bw, by + bd, 0);
        });
      });
    });

    city.buildings.sort(function (a, b) {
      return (a.x + a.y) - (b.x + b.y);
    });

    if (!isFinite(city.minX)) {
      city.minX = -1; city.maxX = 1; city.minY = -1; city.maxY = 1;
    }
    city.midX = (city.minX + city.maxX) / 2;
    city.midY = (city.minY + city.maxY) / 2;
    return city;
  }

  function computeCityFit() {
    if (!S || !S.city) return;
    var w = S.size.w, h = S.size.h, pad = 34;
    var bw = Math.max(1, S.city.maxX - S.city.minX);
    var bh = Math.max(1, S.city.maxY - S.city.minY);
    var fit = Math.min((w - pad * 2) / bw, (h - pad * 2) / bh);
    S.city.fit = (isFinite(fit) && fit > 0) ? fit : 1;
  }

  /* -------------------------------------------------------------------- DOM */

  function pill(text) {
    var b = document.createElement('button');
    b.type = 'button';
    b.textContent = text;
    b.style.cssText = 'appearance:none;border:0;background:transparent;color:inherit;font:inherit;' +
      'font-weight:600;letter-spacing:.02em;padding:5px 11px;border-radius:999px;cursor:pointer;' +
      'transition:background .15s,color .15s,box-shadow .15s;';
    return b;
  }

  function buildHeader() {
    var bar = document.createElement('div');
    bar.style.cssText = 'position:relative;z-index:3;display:flex;align-items:center;gap:10px;' +
      'padding:8px 12px;flex:0 0 auto;font:12px/1 ui-sans-serif,system-ui,-apple-system,' +
      'Segoe UI,sans-serif;color:#9fb3c8;';

    var seg = document.createElement('div');
    seg.style.cssText = 'display:inline-flex;padding:2px;border-radius:999px;' +
      'background:rgba(255,255,255,.05);border:1px solid rgba(120,200,255,.18);';
    var b2 = pill('2D Treemap');
    var b3 = pill('3D City');
    seg.appendChild(b2);
    seg.appendChild(b3);

    var fit = pill('Fit');
    fit.style.marginLeft = '2px';

    var hint = document.createElement('span');
    hint.textContent = 'Click to inspect · drag to pan · wheel to zoom';
    hint.style.cssText = 'margin-left:auto;opacity:.55;font-size:11px;white-space:nowrap;' +
      'overflow:hidden;text-overflow:ellipsis;';

    bar.appendChild(seg);
    bar.appendChild(fit);
    bar.appendChild(hint);
    return { bar: bar, b2: b2, b3: b3, fit: fit };
  }

  function createTooltip(host) {
    var el = document.createElement('div');
    el.style.cssText = 'position:absolute;pointer-events:none;z-index:20;max-width:300px;' +
      'padding:8px 10px;border-radius:8px;background:rgba(8,12,20,.94);' +
      'border:1px solid rgba(120,200,255,.25);box-shadow:0 8px 24px rgba(0,0,0,.45);' +
      'color:#dbeafe;font:12px/1.45 ui-sans-serif,system-ui,sans-serif;opacity:0;' +
      'transform:translate(-50%,-118%);transition:opacity .12s;white-space:nowrap;';
    host.appendChild(el);
    return {
      el: el,
      show: function (html, x, y) {
        el.innerHTML = html;
        el.style.left = x + 'px';
        el.style.top = y + 'px';
        el.style.opacity = '1';
      },
      hide: function () { el.style.opacity = '0'; },
      destroy: function () {
        if (el.parentNode) el.parentNode.removeChild(el);
      }
    };
  }

  /* ------------------------------------------------------------------ mount */

  function mount(ctx) {
    destroy();
    ctx = ctx || {};
    if (!ctx.container) return;

    var root = document.createElement('div');
    root.className = 'aikd-view aikd-treemap';
    root.style.cssText = 'position:absolute;inset:0;display:flex;flex-direction:column;' +
      'overflow:hidden;background:transparent;';

    var ui = buildHeader();
    root.appendChild(ui.bar);

    var wrap = document.createElement('div');
    wrap.style.cssText = 'position:relative;flex:1 1 auto;min-height:0;overflow:hidden;';
    root.appendChild(wrap);

    var canvas = document.createElement('canvas');
    canvas.style.cssText = 'position:absolute;inset:0;display:block;width:100%;height:100%;' +
      'touch-action:none;cursor:grab;';
    wrap.appendChild(canvas);

    var g2d = canvas.getContext('2d');
    var tip = createTooltip(wrap);

    S = {
      ctx: ctx,
      root: root,
      wrap: wrap,
      canvas: canvas,
      g: g2d,
      tip: tip,
      ui: ui,
      sub: '2d',
      size: { w: 1, h: 1, dpr: 1 },
      groups: [],
      layout2d: [],
      city: null,
      hits2d: [],
      hoverId: null,
      selectedId: ctx.selectedId || null,
      adj: buildAdj(ctx),
      cam: { k: 1, kT: 1, x: 0, y: 0, xT: 0, yT: 0 },
      drag: null,
      raf: 0,
      dirty: true,
      ro: null,
      sig: ''
    };

    S.groups = normalizeTree(getTree(ctx));
    S.sig = signature(ctx);
    S.layout2d = buildLayout(S.size.w, S.size.h, 8);
    S.city = buildCity();

    bind();
    ctx.container.appendChild(root);
    resize();
    loop();
    syncPills();
  }

  function bind() {
    var canvas = S.canvas;

    S.onDown = function (ev) {
      if (ev.button !== 0) return;
      S.drag = { sx: ev.clientX, sy: ev.clientY, cx: S.cam.x, cy: S.cam.y, moved: false };
      canvas.style.cursor = 'grabbing';
      S.tip.hide();
    };
    S.onWheel = function (ev) {
      ev.preventDefault();
      var p = localPoint(ev);
      var cam = S.cam;
      var factor = Math.exp(-ev.deltaY * 0.0012);
      var kNew = clamp(cam.kT * factor, 0.35, 9);
      var kOld = cam.kT;
      if (Math.abs(kNew - kOld) < 1e-6) return;
      if (S.sub === '3d' && S.city) {
        var sOld = S.city.fit * kOld;
        var sNew = S.city.fit * kNew;
        var ox = p.x - S.size.w / 2 - cam.xT;
        var oy = p.y - S.size.h / 2 - cam.yT;
        cam.xT = p.x - S.size.w / 2 - ox * (sNew / sOld);
        cam.yT = p.y - S.size.h / 2 - oy * (sNew / sOld);
      } else {
        cam.xT = p.x - (p.x - cam.xT) * (kNew / kOld);
        cam.yT = p.y - (p.y - cam.yT) * (kNew / kOld);
      }
      cam.kT = kNew;
      S.dirty = true;
    };
    S.onMove = function (ev) {
      if (!S) return;
      var p = localPoint(ev);
      var inside = p.x >= 0 && p.y >= 0 && p.x <= S.size.w && p.y <= S.size.h;

      if (S.drag) {
        var dx = ev.clientX - S.drag.sx;
        var dy = ev.clientY - S.drag.sy;
        if (Math.abs(dx) + Math.abs(dy) > 3) S.drag.moved = true;
        S.cam.x = S.drag.cx + dx;
        S.cam.y = S.drag.cy + dy;
        S.cam.xT = S.cam.x;
        S.cam.yT = S.cam.y;
        S.dirty = true;
        S.tip.hide();
        return;
      }
      if (!inside) {
        if (S.hoverId) { S.hoverId = null; S.dirty = true; }
        S.tip.hide();
        canvas.style.cursor = 'grab';
        return;
      }
      var hit = S.sub === '3d' ? hit3D(p) : hit2D(p);
      var id = hit ? idOf(hit.leaf) : null;
      if (id !== S.hoverId) {
        S.hoverId = id;
        S.dirty = true;
      }
      if (hit) {
        canvas.style.cursor = 'pointer';
        S.tip.show(tipHtml(hit.leaf), p.x, p.y);
      } else {
        canvas.style.cursor = 'grab';
        S.tip.hide();
      }
    };
    S.onUp = function (ev) {
      if (!S || !S.drag) return;
      var drag = S.drag;
      S.drag = null;
      canvas.style.cursor = 'grab';
      if (drag.moved) return;
      var p = localPoint(ev);
      var hit = S.sub === '3d' ? hit3D(p) : hit2D(p);
      if (hit) selectLeaf(hit.leaf);
    };
    S.onLeave = function () {
      if (!S) return;
      S.hoverId = null;
      S.tip.hide();
      S.dirty = true;
    };

    canvas.addEventListener('pointerdown', S.onDown);
    canvas.addEventListener('wheel', S.onWheel, { passive: false });
    canvas.addEventListener('pointerleave', S.onLeave);
    window.addEventListener('pointermove', S.onMove);
    window.addEventListener('pointerup', S.onUp);

    S.onResize = function () { if (S) resize(); };
    if (typeof ResizeObserver !== 'undefined') {
      S.ro = new ResizeObserver(S.onResize);
      S.ro.observe(S.wrap);
    } else {
      window.addEventListener('resize', S.onResize);
    }

    S.ui.b2.addEventListener('click', function () { setSub('2d'); });
    S.ui.b3.addEventListener('click', function () { setSub('3d'); });
    S.ui.fit.addEventListener('click', resetCam);
  }

  function resize() {
    if (!S) return;
    var rect = S.wrap.getBoundingClientRect();
    var w = Math.max(1, Math.round(rect.width));
    var h = Math.max(1, Math.round(rect.height));
    var dpr = clamp(window.devicePixelRatio || 1, 1, 3);
    S.size = { w: w, h: h, dpr: dpr };
    S.canvas.width = Math.round(w * dpr);
    S.canvas.height = Math.round(h * dpr);
    S.layout2d = buildLayout(w, h, 8);
    computeCityFit();
    S.dirty = true;
  }

  function setSub(sub) {
    if (!S || S.sub === sub) return;
    S.sub = sub;
    S.hoverId = null;
    S.tip.hide();
    S.cam.kT = 1;
    S.cam.xT = 0;
    S.cam.yT = 0;
    S.dirty = true;
    syncPills();
  }

  function syncPills() {
    if (!S) return;
    [[S.ui.b2, '2d'], [S.ui.b3, '3d']].forEach(function (pair) {
      var on = S.sub === pair[1];
      pair[0].style.background = on ? 'rgba(56,189,248,.18)' : 'transparent';
      pair[0].style.color = on ? '#7dd3fc' : '#9fb3c8';
      pair[0].style.boxShadow = on ? 'inset 0 0 0 1px rgba(56,189,248,.45)' : 'none';
    });
  }

  function resetCam() {
    if (!S) return;
    S.cam.kT = 1;
    S.cam.xT = 0;
    S.cam.yT = 0;
    S.dirty = true;
  }

  function localPoint(ev) {
    var rect = S.wrap.getBoundingClientRect();
    return { x: ev.clientX - rect.left, y: ev.clientY - rect.top };
  }

  function tipHtml(leaf) {
    var node = leaf.node || leaf;
    var label = labelOf(node, S.ctx.helpers);
    var kind = node.kind || leaf.kind || 'node';
    var proj = node.project || '';
    var deg = num(node.degree);
    return '<div style="font-weight:700;color:#e6f2ff;margin-bottom:2px">' +
      esc(clampText(label, 64)) + '</div><div style="opacity:.75">' + esc(kind) +
      (proj ? ' · ' + esc(proj) : '') + (deg ? ' · ' + deg + ' links' : '') + '</div>';
  }

  function selectLeaf(leaf) {
    if (!S || !leaf) return;
    var node = leaf.node || leaf;
    S.selectedId = node.id || leaf.id || S.selectedId;
    S.dirty = true;
    if (S.ctx.helpers && typeof S.ctx.helpers.onSelectNode === 'function') {
      try { S.ctx.helpers.onSelectNode(node); } catch (e) { /* ignore */ }
    }
  }

  /* ------------------------------------------------------------------ draw */

  function screenRect2D(r) {
    var c = S.cam;
    return { x: r.x * c.k + c.x, y: r.y * c.k + c.y, w: r.w * c.k, h: r.h * c.k };
  }

  function draw2D(time) {
    var g = S.g, w = S.size.w, h = S.size.h;
    g.setTransform(S.size.dpr, 0, 0, S.size.dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    S.hits2d = [];

    S.layout2d.forEach(function (proj) {
      var pr = screenRect2D(proj.rect);
      if (pr.w < 2 || pr.h < 2) return;
      pathRoundRect(g, pr.x, pr.y, pr.w, pr.h, 8);
      g.fillStyle = rgba(proj.color, 0.07);
      g.fill();
      g.strokeStyle = rgba(proj.color, 0.32);
      g.lineWidth = 1;
      g.stroke();

      if (pr.w > 54 && pr.h > 18) {
        g.save();
        g.beginPath();
        g.rect(pr.x + 1, pr.y + 1, pr.w - 2, pr.h - 2);
        g.clip();
        g.fillStyle = rgba(lighten(proj.color, 0.35), 0.9);
        g.font = '700 11px ui-sans-serif,system-ui,sans-serif';
        g.textBaseline = 'top';
        g.fillText(clampText(proj.group.name, Math.floor((pr.w - 10) / 6.4)), pr.x + 6, pr.y + 5);
        g.restore();
      }

      proj.types.forEach(function (ty) {
        var tr = screenRect2D(ty.rect);
        if (tr.w < 2 || tr.h < 2) return;

        pathRoundRect(g, tr.x, tr.y, tr.w, tr.h, 4);
        g.fillStyle = rgba(colorOf(ty.type.kind, S.ctx.helpers), 0.08);
        g.fill();
        g.strokeStyle = rgba(colorOf(ty.type.kind, S.ctx.helpers), 0.22);
        g.lineWidth = 1;
        g.stroke();

        ty.items.forEach(function (it) {
          var r = screenRect2D(it.rect);
          if (r.w < 0.6 || r.h < 0.4) return;
          var color = colorOf(it.leaf.kind, S.ctx.helpers);
          var id = idOf(it.leaf);
          var sel = id !== null && id === S.selectedId;
          var hov = id !== null && id === S.hoverId;

          pathRoundRect(g, r.x + 0.5, r.y + 0.5, Math.max(0.8, r.w - 1), Math.max(0.8, r.h - 1),
            Math.min(4, r.w / 4, r.h / 4));
          g.fillStyle = rgba(color, sel ? 0.95 : (hov ? 0.82 : 0.5));
          g.fill();

          if (sel || hov) {
            g.strokeStyle = sel ? '#ffffff' : rgba(lighten(color, 0.7), 0.95);
            g.lineWidth = sel ? 1.8 : 1.2;
            g.stroke();
          } else if (r.w > 20 && r.h > 12) {
            g.strokeStyle = 'rgba(4,8,14,.35)';
            g.lineWidth = 1;
            g.stroke();
          }

          if (r.w > 68 && r.h > 22) {
            g.save();
            g.beginPath();
            g.rect(r.x + 1, r.y + 1, r.w - 2, r.h - 2);
            g.clip();
            g.fillStyle = 'rgba(240,248,255,.92)';
            g.font = '600 10px ui-sans-serif,system-ui,sans-serif';
            g.textBaseline = 'middle';
            g.fillText(clampText(labelOf(it.leaf.node || it.leaf, S.ctx.helpers),
              Math.floor((r.w - 10) / 5.8)), r.x + 5, r.y + Math.min(r.h - 8, 12));
            g.restore();
          }

          S.hits2d.push({
            leaf: it.leaf,
            x: r.x, y: r.y, w: r.w, h: r.h
          });
        });
      });
    });

    if (S.selectedId) {
      for (var i = 0; i < S.hits2d.length; i++) {
        var hit = S.hits2d[i];
        if (idOf(hit.leaf) !== S.selectedId) continue;
        var pulse = 0.5 + 0.5 * Math.sin(time * 3);
        g.save();
        g.strokeStyle = 'rgba(255,255,255,' + (0.45 + 0.4 * pulse) + ')';
        g.lineWidth = 2;
        g.shadowColor = '#7dd3fc';
        g.shadowBlur = 10 + 8 * pulse;
        pathRoundRect(g, hit.x - 2, hit.y - 2, hit.w + 4, hit.h + 4, 4);
        g.stroke();
        g.restore();
        break;
      }
    }
  }

  function makeProjector() {
    var city = S.city;
    var w = S.size.w, h = S.size.h;
    var s = city.fit * S.cam.k;
    return function (x, y, z) {
      var nx = (x - y) * COS;
      var ny = (x + y) * SIN - z;
      return {
        x: w / 2 + (nx - city.midX) * s + S.cam.x,
        y: h / 2 + (ny - city.midY) * s + S.cam.y
      };
    };
  }

  function drawQuad(g, pts, fill, stroke, lw) {
    g.beginPath();
    g.moveTo(pts[0].x, pts[0].y);
    for (var i = 1; i < pts.length; i++) g.lineTo(pts[i].x, pts[i].y);
    g.closePath();
    if (fill) { g.fillStyle = fill; g.fill(); }
    if (stroke) { g.strokeStyle = stroke; g.lineWidth = lw || 1; g.stroke(); }
  }

  function draw3D(time) {
    var g = S.g, w = S.size.w, h = S.size.h;
    g.setTransform(S.size.dpr, 0, 0, S.size.dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    if (!S.city) return;
    var city = S.city;
    var pr = makeProjector();

    city.districts.forEach(function (d) {
      var pts = [pr(d.x, d.y, 0), pr(d.x + d.w, d.y, 0), pr(d.x + d.w, d.y + d.h, 0), pr(d.x, d.y + d.h, 0)];
      drawQuad(g, pts, rgba(d.color, 0.1), rgba(d.color, 0.3), 1);
    });

    city.blocks.forEach(function (b) {
      var pts = [pr(b.x, b.y, 0), pr(b.x + b.w, b.y, 0), pr(b.x + b.w, b.y + b.h, 0), pr(b.x, b.y + b.h, 0)];
      drawQuad(g, pts, rgba(b.color, 0.12), rgba(b.color, 0.26), 1);
    });

    var selId = S.selectedId;
    var lit = null;
    if (selId) {
      lit = Object.create(null);
      lit[selId] = true;
      var nbs = S.adj[selId] || [];
      for (var ni = 0; ni < nbs.length; ni++) lit[nbs[ni]] = true;
    }

    city.buildings.forEach(function (b) {
      var x0 = b.x, y0 = b.y, x1 = b.x + b.w, y1 = b.y + b.d, z = b.h;
      var t1 = pr(x0, y0, z), t2 = pr(x1, y0, z), t3 = pr(x1, y1, z), t4 = pr(x0, y1, z);
      var b2 = pr(x1, y0, 0), b3 = pr(x1, y1, 0), b4 = pr(x0, y1, 0);
      var id = idOf(b.leaf);
      var sel = id !== null && id === S.selectedId;
      var hov = id !== null && id === S.hoverId;
      var dim = (lit && !lit[id]) ? 0.3 : 1;

      if (sel || hov) {
        g.save();
        g.shadowColor = b.color;
        g.shadowBlur = sel ? 24 : 13;
      }
      drawQuad(g, [t2, t3, b3, b2], rgba(darken(b.color, 0.26), 0.94 * dim), 'rgba(2,6,12,.4)', 0.6);
      drawQuad(g, [t4, t3, b3, b4], rgba(darken(b.color, 0.46), 0.95 * dim), 'rgba(2,6,12,.4)', 0.6);
      drawQuad(g, [t1, t2, t3, t4],
        sel ? lighten(b.color, 0.55) : (hov ? lighten(b.color, 0.28) : lighten(b.color, 0.1)),
        'rgba(2,6,12,.35)', 0.6);
      if (sel || hov) g.restore();

      if (sel) {
        var mid = pr(x0 + b.w / 2, y0 + b.d / 2, z);
        var pulse = 0.5 + 0.5 * Math.sin(time * 3);
        g.save();
        g.strokeStyle = 'rgba(255,255,255,' + (0.5 + 0.35 * pulse) + ')';
        g.lineWidth = 1.6;
        g.beginPath();
        g.arc(mid.x, mid.y, 5 + 4 * pulse, 0, TAU);
        g.stroke();
        g.restore();

        var lbl = clampText(labelOf(b.node || b.leaf, S.ctx.helpers), 26);
        g.save();
        g.font = '600 11px ui-sans-serif,system-ui,sans-serif';
        var tw = g.measureText(lbl).width;
        var lx = mid.x - tw / 2 - 8;
        var ly = mid.y - 30;
        g.fillStyle = 'rgba(5,10,18,.86)';
        g.strokeStyle = rgba(b.color, 0.7);
        g.lineWidth = 1;
        pathRoundRect(g, lx, ly, tw + 16, 20, 6);
        g.fill();
        g.stroke();
        g.fillStyle = '#eaf4ff';
        g.textAlign = 'center';
        g.textBaseline = 'middle';
        g.fillText(lbl, mid.x, ly + 10.5);
        g.restore();
      }
    });

    city.districts.forEach(function (d) {
      var pw = Math.abs(pr(d.x + d.w, d.y, 0).x - pr(d.x, d.y, 0).x);
      if (pw < 64) return;
      var p = pr(d.x + 4, d.y + 4, 0);
      var text = clampText(d.name, Math.max(4, Math.floor(pw / 7)));
      g.font = '600 11px ui-sans-serif,system-ui,sans-serif';
      var tw = g.measureText(text).width;
      g.fillStyle = 'rgba(5,10,18,.74)';
      g.strokeStyle = rgba(d.color, 0.55);
      g.lineWidth = 1;
      pathRoundRect(g, p.x, p.y, tw + 14, 18, 6);
      g.fill();
      g.stroke();
      g.fillStyle = rgba(lighten(d.color, 0.35), 0.95);
      g.textAlign = 'left';
      g.textBaseline = 'middle';
      g.fillText(text, p.x + 7, p.y + 9.5);
    });

    drawArcs(g, pr, time);
  }

  function drawArcs(g, pr, time) {
    if (!S.selectedId || !S.city) return;
    var fromB = S.city.byId[S.selectedId];
    if (!fromB) return;
    var from = pr(fromB.x + fromB.w / 2, fromB.y + fromB.d / 2, fromB.h);
    var ids = S.adj[S.selectedId] || [];
    if (!ids.length) return;

    ids.forEach(function (id, i) {
      var toB = S.city.byId[id];
      if (!toB) return;
      var to = pr(toB.x + toB.w / 2, toB.y + toB.d / 2, toB.h);
      var dx = to.x - from.x;
      var dy = to.y - from.y;
      var dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < 2 || dist > 4000) return;
      var lift = 26 + Math.min(150, dist * 0.32);
      var alpha = 0.55 + 0.3 * Math.sin(time * 3 + i * 0.9);
      g.save();
      g.strokeStyle = rgba(lighten(toB.color, 0.35), alpha);
      g.lineWidth = 1.9;
      g.shadowColor = toB.color;
      g.shadowBlur = 15;
      g.beginPath();
      g.moveTo(from.x, from.y);
      g.quadraticCurveTo((from.x + to.x) / 2, (from.y + to.y) / 2 - lift, to.x, to.y);
      g.stroke();
      g.restore();

      g.save();
      g.fillStyle = rgba(lighten(toB.color, 0.35), 0.85);
      g.beginPath();
      g.arc(to.x, to.y, 2.2, 0, TAU);
      g.fill();
      g.restore();
    });
  }

  /* ------------------------------------------------------------------- hit */

  function hit2D(p) {
    for (var i = S.hits2d.length - 1; i >= 0; i--) {
      var r = S.hits2d[i];
      if (p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h) return r;
    }
    return null;
  }

  function hit3D(p) {
    if (!S.city) return null;
    var pr = makeProjector();
    var list = S.city.buildings;
    for (var i = list.length - 1; i >= 0; i--) {
      var b = list[i];
      var x0 = b.x, y0 = b.y, x1 = b.x + b.w, y1 = b.y + b.d, z = b.h;
      var t1 = pr(x0, y0, z), t2 = pr(x1, y0, z), t3 = pr(x1, y1, z), t4 = pr(x0, y1, z);
      var minX = Math.min(t1.x, t2.x, t3.x, t4.x);
      var maxX = Math.max(t1.x, t2.x, t3.x, t4.x);
      var minY = Math.min(t1.y, t2.y, t3.y, t4.y);
      var maxY = Math.max(t1.y, t2.y, t3.y, t4.y);
      if (p.x < minX - 2 || p.x > maxX + 2 || p.y < minY - 2 || p.y > maxY + 40) continue;

      if (pointInPoly(p.x, p.y, [t1, t2, t3, t4])) return { leaf: b.leaf };
      var b3 = pr(x1, y1, 0), b4 = pr(x0, y1, 0);
      if (pointInPoly(p.x, p.y, [t4, t3, b3, b4])) return { leaf: b.leaf };
      var b2 = pr(x1, y0, 0);
      if (pointInPoly(p.x, p.y, [t2, t3, b3, b2])) return { leaf: b.leaf };
    }
    return null;
  }

  /* ----------------------------------------------------------------- update */

  function update(ctx) {
    if (!S) return;
    if (ctx) S.ctx = ctx;
    S.selectedId = S.ctx.selectedId || null;
    S.adj = buildAdj(S.ctx);
    S.hoverId = null;
    S.tip.hide();

    var sig = signature(S.ctx);
    if (sig !== S.sig) {
      S.sig = sig;
      S.groups = normalizeTree(getTree(S.ctx));
      S.layout2d = buildLayout(S.size.w, S.size.h, 8);
      S.city = buildCity();
      computeCityFit();
      S.hits2d = [];
      S.cam.k = 1;
      S.cam.kT = 1;
      S.cam.x = 0;
      S.cam.xT = 0;
      S.cam.y = 0;
      S.cam.yT = 0;
    }
    S.dirty = true;
  }

  function findItemRect2D(id) {
    for (var i = 0; i < S.layout2d.length; i++) {
      var proj = S.layout2d[i];
      for (var j = 0; j < proj.types.length; j++) {
        var items = proj.types[j].items;
        for (var k = 0; k < items.length; k++) {
          if (idOf(items[k].leaf) === id) return items[k].rect;
        }
      }
    }
    return null;
  }

  function focusNode(nodeId) {
    if (!S || !nodeId) return;
    S.selectedId = nodeId;

    if (S.sub === '2d') {
      var rect = findItemRect2D(nodeId);
      if (!rect) return;
      var kT = Math.max(S.cam.k, 1.9);
      S.cam.kT = kT;
      S.cam.xT = S.size.w / 2 - (rect.x + rect.w / 2) * kT;
      S.cam.yT = S.size.h / 2 - (rect.y + rect.h / 2) * kT;
      S.dirty = true;
      return;
    }

    var b = S.city && S.city.byId[nodeId];
    if (!b) return;
    var city = S.city;
    var kT2 = Math.max(S.cam.k, 1.5);
    var s = city.fit * kT2;
    var cx = b.x + b.w / 2;
    var cy = b.y + b.d / 2;
    var nx = (cx - cy) * COS;
    var ny = (cx + cy) * SIN - b.h;
    S.cam.kT = kT2;
    S.cam.xT = -(nx - city.midX) * s;
    S.cam.yT = -(ny - city.midY) * s;
    S.dirty = true;
  }

  /* ------------------------------------------------------------------ loop */

  function loop() {
    if (!S) return;
    S.raf = requestAnimationFrame(frame);
  }

  function frame(ts) {
    if (!S) return;
    S.raf = requestAnimationFrame(frame);

    var cam = S.cam;
    var moving = Math.abs(cam.k - cam.kT) > 0.0006 ||
      Math.abs(cam.x - cam.xT) > 0.4 || Math.abs(cam.y - cam.yT) > 0.4;
    if (moving) {
      cam.k += (cam.kT - cam.k) * 0.2;
      cam.x += (cam.xT - cam.x) * 0.22;
      cam.y += (cam.yT - cam.y) * 0.22;
      if (Math.abs(cam.k - cam.kT) < 0.0006) cam.k = cam.kT;
      if (Math.abs(cam.x - cam.xT) < 0.4) cam.x = cam.xT;
      if (Math.abs(cam.y - cam.yT) < 0.4) cam.y = cam.yT;
      S.dirty = true;
    }

    var pulsing = !!(S.selectedId || S.hoverId);
    if (!S.dirty && !moving && !pulsing) return;
    S.dirty = false;
    if (S.sub === '3d') draw3D(ts / 1000);
    else draw2D(ts / 1000);
  }

  /* ---------------------------------------------------------------- destroy */

  function destroy() {
    if (!S) return;
    if (S.raf) cancelAnimationFrame(S.raf);
    if (S.ro) {
      try { S.ro.disconnect(); } catch (e) { /* ignore */ }
    } else if (S.onResize) {
      window.removeEventListener('resize', S.onResize);
    }
    window.removeEventListener('pointermove', S.onMove);
    window.removeEventListener('pointerup', S.onUp);
    S.tip.destroy();
    if (S.root && S.root.parentNode) S.root.parentNode.removeChild(S.root);
    if (S.root) S.root.innerHTML = '';
    S = null;
  }

  G.views.treemap = {
    mount: mount,
    update: update,
    focusNode: focusNode,
    destroy: destroy
  };
})(window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} });
