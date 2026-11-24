/* AIKD Graph — Circle Packing view (zoomable Project -> Type -> Item bubbles).
 * Pure offline vanilla JS. Registers window.AIKD_GRAPH.views.packing.
 */
(function (G) {
  'use strict';

  G.views = G.views || {};

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
      return {
        name: p.name,
        kind: 'project',
        children: Object.keys(p.children).map(function (tk) { return p.children[tk]; })
      };
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
          types.push({ name: child.name || child.kind || 'items', kind: child.kind || 'type', leaves: leaves });
        }
      });
      if (direct.length) types.push({ name: 'General', kind: 'general', leaves: direct });

      var group = { name: g.name || g.label || ('Group ' + (gi + 1)), kind: g.kind || 'project', types: [], weight: 0 };
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

  /* -------------------------------------------------------------- packing */

  /* Greedy circle packing: place each circle tangent to an already placed one,
   * picking the position closest to the parent centre that has no overlap. */
  function packChildren(children, pad) {
    var list = children.slice().sort(function (a, b) { return b.r - a.r; });
    var placed = [];

    list.forEach(function (c) {
      if (!placed.length) {
        c.lx = 0;
        c.ly = 0;
      } else {
        var best = null;
        var bestD = Infinity;
        for (var pi = 0; pi < placed.length; pi++) {
          var p = placed[pi];
          var rad = p.r + c.r + pad;
          for (var s = 0; s < 32; s++) {
            var a = (s / 32) * TAU + pi * 0.19;
            var nx = p.lx + Math.cos(a) * rad;
            var ny = p.ly + Math.sin(a) * rad;
            var ok = true;
            for (var qi = 0; qi < placed.length; qi++) {
              var q = placed[qi];
              var need = q.r + c.r + pad;
              var dx = nx - q.lx;
              var dy = ny - q.ly;
              if (dx * dx + dy * dy < need * need - 0.01) { ok = false; break; }
            }
            if (ok) {
              var d = nx * nx + ny * ny;
              if (d < bestD) {
                bestD = d;
                best = { x: nx, y: ny };
              }
            }
          }
        }
        if (best) {
          c.lx = best.x;
          c.ly = best.y;
        } else {
          var ang = 0;
          var rad2 = c.r + pad;
          for (var t = 0; t < 900; t++) {
            var px = Math.cos(ang) * rad2;
            var py = Math.sin(ang) * rad2;
            var free = true;
            for (var zi = 0; zi < placed.length; zi++) {
              var z = placed[zi];
              var need2 = z.r + c.r + pad;
              if ((px - z.lx) * (px - z.lx) + (py - z.ly) * (py - z.ly) < need2 * need2 - 0.01) {
                free = false;
                break;
              }
            }
            if (free) {
              c.lx = px;
              c.ly = py;
              break;
            }
            ang += 0.4;
            rad2 += 0.8;
          }
        }
      }
      placed.push(c);
    });

    var maxD = 0;
    var sw = 0;
    var sx = 0;
    var sy = 0;
    placed.forEach(function (c) {
      var wgt = c.r * c.r;
      sw += wgt;
      sx += c.lx * wgt;
      sy += c.ly * wgt;
    });
    if (sw > 0) {
      var mcx = sx / sw;
      var mcy = sy / sw;
      placed.forEach(function (c) {
        c.lx -= mcx;
        c.ly -= mcy;
      });
    }
    placed.forEach(function (c) {
      var d = Math.sqrt(c.lx * c.lx + c.ly * c.ly) + c.r;
      if (d > maxD) maxD = d;
    });
    return maxD + pad;
  }

  function absolutize(node, ox, oy) {
    node.x = ox + (node.lx || 0);
    node.y = oy + (node.ly || 0);
    if (node.children) {
      node.children.forEach(function (c) { absolutize(c, node.x, node.y); });
    }
  }

  function buildPacking() {
    var root = { key: 'root', label: 'AIKD', kind: 'root', children: [], r: 0, lx: 0, ly: 0, x: 0, y: 0 };
    var byProjKey = Object.create(null);
    var byTypeKey = Object.create(null);
    var index = Object.create(null);

    var projects = [];
    S.groups.forEach(function (g) {
      var proj = {
        key: 'p:' + g.key,
        label: g.name,
        kind: 'project',
        color: projectColor(g.name),
        group: g,
        children: [],
        r: 0, lx: 0, ly: 0, x: 0, y: 0,
        count: 0
      };
      byProjKey[proj.key] = proj;

      g.types.forEach(function (t, ti) {
        var type = {
          key: 't:' + g.key + '::' + t.name,
          label: t.name,
          kind: t.kind,
          color: colorOf(t.kind, S.ctx.helpers),
          group: g,
          type: t,
          parent: proj,
          children: [],
          r: 0, lx: 0, ly: 0, x: 0, y: 0
        };
        byTypeKey[type.key] = type;

        var maxR = 2;
        var items = t.leaves.map(function (leaf) {
          var r = 2.2 + Math.sqrt(Math.max(1, leaf.weight)) * 1.1;
          if (r > maxR) maxR = r;
          return {
            key: 'i:' + (idOf(leaf) || 'x'),
            label: labelOf(leaf.node || leaf, S.ctx.helpers),
            kind: leaf.kind,
            color: colorOf(leaf.kind, S.ctx.helpers),
            leaf: leaf,
            node: leaf.node || null,
            parent: type,
            r: r,
            lx: 0, ly: 0, x: 0, y: 0
          };
        });
        items.sort(function (a, b) { return b.r - a.r; });

        var step = maxR * 1.8;
        items.forEach(function (it, i) {
          var rr = step * Math.sqrt(i + 0.42);
          var a = i * 2.399963229728653 + (ti + 1) * 1.3;
          it.lx = Math.cos(a) * rr;
          it.ly = Math.sin(a) * rr;
        });

        type.r = step * Math.sqrt(Math.max(1, items.length - 0.45)) + maxR + 3;
        type.r = Math.max(type.r, maxR * 2.4 + 4);
        type.children = items;
        type.count = items.length;

        items.forEach(function (it) {
          var id = idOf(it.leaf);
          if (id) index[id] = { projNode: proj, typeNode: type, itemNode: it };
        });

        proj.children.push(type);
        proj.count += items.length;
      });

      if (!proj.children.length) return;
      proj.r = Math.max(22, packChildren(proj.children, 5));
      projects.push(proj);
    });

    root.children = projects;
    root.r = Math.max(40, packChildren(projects, 12));
    absolutize(root, 0, 0);

    return {
      root: root,
      projects: projects,
      byProjKey: byProjKey,
      byTypeKey: byTypeKey,
      index: index
    };
  }

  /* -------------------------------------------------------------------- DOM */

  function pill(text) {
    var b = document.createElement('button');
    b.type = 'button';
    b.textContent = text;
    b.style.cssText = 'appearance:none;border:0;background:transparent;color:inherit;font:inherit;' +
      'font-weight:600;padding:5px 10px;border-radius:999px;cursor:pointer;' +
      'transition:background .15s,color .15s;';
    return b;
  }

  function buildHeader() {
    var bar = document.createElement('div');
    bar.style.cssText = 'position:relative;z-index:3;display:flex;align-items:center;gap:10px;' +
      'padding:8px 12px;flex:0 0 auto;font:12px/1 ui-sans-serif,system-ui,-apple-system,' +
      'Segoe UI,sans-serif;color:#9fb3c8;';
    var title = document.createElement('span');
    title.textContent = 'Circle Packing';
    title.style.cssText = 'font-weight:700;color:#dbeafe;letter-spacing:.02em;';
    var crumbs = document.createElement('div');
    crumbs.style.cssText = 'display:flex;align-items:center;gap:2px;overflow:hidden;' +
      'white-space:nowrap;';
    var hint = document.createElement('span');
    hint.textContent = 'Click bubbles to zoom in · click background to zoom out';
    hint.style.cssText = 'margin-left:auto;opacity:.55;font-size:11px;overflow:hidden;' +
      'text-overflow:ellipsis;white-space:nowrap;';
    bar.appendChild(title);
    bar.appendChild(crumbs);
    bar.appendChild(hint);
    return { bar: bar, crumbs: crumbs };
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
      show: function (html, x, y) {
        el.innerHTML = html;
        el.style.left = x + 'px';
        el.style.top = y + 'px';
        el.style.opacity = '1';
      },
      hide: function () { el.style.opacity = '0'; },
      destroy: function () { if (el.parentNode) el.parentNode.removeChild(el); }
    };
  }

  /* ------------------------------------------------------------------ mount */

  function mount(ctx) {
    destroy();
    ctx = ctx || {};
    if (!ctx.container) return;

    var root = document.createElement('div');
    root.className = 'aikd-view aikd-packing';
    root.style.cssText = 'position:absolute;inset:0;display:flex;flex-direction:column;' +
      'overflow:hidden;background:transparent;';

    var ui = buildHeader();
    root.appendChild(ui.bar);

    var wrap = document.createElement('div');
    wrap.style.cssText = 'position:relative;flex:1 1 auto;min-height:0;overflow:hidden;';
    root.appendChild(wrap);

    var canvas = document.createElement('canvas');
    canvas.style.cssText = 'position:absolute;inset:0;display:block;width:100%;height:100%;' +
      'touch-action:none;cursor:pointer;';
    wrap.appendChild(canvas);

    S = {
      ctx: ctx,
      root: root,
      wrap: wrap,
      canvas: canvas,
      g: canvas.getContext('2d'),
      tip: createTooltip(wrap),
      ui: ui,
      size: { w: 1, h: 1, dpr: 1 },
      groups: [],
      rootNode: null,
      projects: [],
      byProjKey: Object.create(null),
      byTypeKey: Object.create(null),
      index: Object.create(null),
      focus: { projKey: null, typeKey: null },
      cam: { s: 1, x: 0, y: 0, sT: 1, xT: 0, yT: 0 },
      vis: [],
      hoverId: null,
      selectedId: ctx.selectedId || null,
      blinkId: null,
      sig: '',
      dirty: true,
      raf: 0,
      ro: null
    };

    S.groups = normalizeTree(getTree(ctx));
    S.sig = signature(ctx);
    rebuild(true);

    bind();
    ctx.container.appendChild(root);
    resize();
    loop();
  }

  function bind() {
    var canvas = S.canvas;

    S.onDown = function () {
      S.tip.hide();
    };
    S.onMove = function (ev) {
      if (!S) return;
      var p = localPoint(ev);
      var inside = p.x >= 0 && p.y >= 0 && p.x <= S.size.w && p.y <= S.size.h;
      if (!inside) {
        if (S.hoverId) { S.hoverId = null; S.dirty = true; }
        S.tip.hide();
        canvas.style.cursor = 'pointer';
        return;
      }
      var hit = hitTest(p);
      var id = hit ? hitKey(hit) : null;
      if (id !== S.hoverId) {
        S.hoverId = id;
        S.dirty = true;
      }
      if (hit) {
        canvas.style.cursor = hit.level === 'item' ? 'pointer' : 'zoom-in';
        S.tip.show(tipHtml(hit), p.x, p.y);
      } else {
        canvas.style.cursor = 'zoom-out';
        S.tip.hide();
      }
    };
    S.onUp = function (ev) {
      if (!S) return;
      var p = localPoint(ev);
      if (p.x < 0 || p.y < 0 || p.x > S.size.w || p.y > S.size.h) return;
      var hit = hitTest(p);
      if (!hit) { zoomOut(); return; }
      if (hit.level === 'project') zoomToProject(hit.node);
      else if (hit.level === 'type') zoomToType(hit.node);
      else {
        selectItem(hit.node);
        zoomToType(hit.node.parent);
      }
    };
    S.onLeave = function () {
      if (!S) return;
      S.hoverId = null;
      S.tip.hide();
      S.dirty = true;
    };
    S.onResize = function () { if (S) resize(); };

    canvas.addEventListener('pointerdown', S.onDown);
    canvas.addEventListener('pointerup', S.onUp);
    canvas.addEventListener('pointerleave', S.onLeave);
    window.addEventListener('pointermove', S.onMove);

    if (typeof ResizeObserver !== 'undefined') {
      S.ro = new ResizeObserver(S.onResize);
      S.ro.observe(S.wrap);
    } else {
      window.addEventListener('resize', S.onResize);
    }
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
    computeTargets();
    S.cam.s = S.cam.sT;
    S.cam.x = S.cam.xT;
    S.cam.y = S.cam.yT;
    S.dirty = true;
  }

  function rebuild(snap) {
    var built = buildPacking();
    S.rootNode = built.root;
    S.projects = built.projects;
    S.byProjKey = built.byProjKey;
    S.byTypeKey = built.byTypeKey;
    S.index = built.index;

    if (S.focus.projKey && !S.byProjKey[S.focus.projKey]) S.focus.projKey = null;
    if (S.focus.typeKey && !S.byTypeKey[S.focus.typeKey]) S.focus.typeKey = null;

    computeTargets();
    if (snap) {
      S.cam.s = S.cam.sT;
      S.cam.x = S.cam.xT;
      S.cam.y = S.cam.yT;
    }
    updateCrumbs();
    S.dirty = true;
  }

  function fitScale(node) {
    return Math.min(S.size.w, S.size.h) * 0.42 / Math.max(1, node.r * 1.08);
  }

  function computeTargets() {
    if (!S || !S.rootNode) return;
    var cam = S.cam;
    var fType = S.focus.typeKey ? S.byTypeKey[S.focus.typeKey] : null;
    var fProj = S.focus.projKey ? S.byProjKey[S.focus.projKey] : null;

    if (S.blinkId && S.index[S.blinkId]) {
      var loc = S.index[S.blinkId];
      cam.xT = loc.itemNode.x;
      cam.yT = loc.itemNode.y;
      cam.sT = fitScale(loc.typeNode) * 1.55;
      return;
    }
    if (fType) {
      cam.xT = fType.x;
      cam.yT = fType.y;
      cam.sT = fitScale(fType);
      return;
    }
    if (fProj) {
      cam.xT = fProj.x;
      cam.yT = fProj.y;
      cam.sT = fitScale(fProj);
      return;
    }
    cam.xT = 0;
    cam.yT = 0;
    cam.sT = fitScale(S.rootNode);
  }

  function updateCrumbs() {
    if (!S || !S.ui) return;
    var host = S.ui.crumbs;
    host.innerHTML = '';

    function crumb(label, onClick, active) {
      var b = pill((active ? '● ' : '') + label);
      b.style.color = active ? '#7dd3fc' : '#9fb3c8';
      b.addEventListener('click', function (ev) {
        ev.stopPropagation();
        onClick();
      });
      host.appendChild(b);
      return b;
    }

    var fProj = S.focus.projKey ? S.byProjKey[S.focus.projKey] : null;
    var fType = S.focus.typeKey ? S.byTypeKey[S.focus.typeKey] : null;

    crumb('AIKD', function () {
      S.focus.projKey = null;
      S.focus.typeKey = null;
      S.blinkId = null;
      computeTargets();
      updateCrumbs();
      S.dirty = true;
    }, !fProj);

    if (fProj) {
      var sep = document.createElement('span');
      sep.textContent = '/';
      sep.style.cssText = 'opacity:.35;padding:0 2px;';
      host.appendChild(sep);
      crumb(clampText(fProj.label, 26), function () {
        S.focus.projKey = fProj.key;
        S.focus.typeKey = null;
        S.blinkId = null;
        computeTargets();
        updateCrumbs();
        S.dirty = true;
      }, !fType);
    }
    if (fType) {
      var sep2 = document.createElement('span');
      sep2.textContent = '/';
      sep2.style.cssText = 'opacity:.35;padding:0 2px;';
      host.appendChild(sep2);
      crumb(clampText(fType.label, 26), function () {
        S.focus.projKey = fType.parent.key;
        S.focus.typeKey = fType.key;
        S.blinkId = null;
        computeTargets();
        updateCrumbs();
        S.dirty = true;
      }, true);
    }
  }

  function zoomToProject(proj) {
    if (!S || !proj) return;
    S.focus.projKey = proj.key;
    S.focus.typeKey = null;
    S.blinkId = null;
    computeTargets();
    updateCrumbs();
    S.dirty = true;
  }

  function zoomToType(type) {
    if (!S || !type) return;
    S.focus.projKey = type.parent.key;
    S.focus.typeKey = type.key;
    S.blinkId = null;
    computeTargets();
    updateCrumbs();
    S.dirty = true;
  }

  function zoomOut() {
    if (!S) return;
    S.blinkId = null;
    if (S.focus.typeKey) S.focus.typeKey = null;
    else if (S.focus.projKey) S.focus.projKey = null;
    else return;
    computeTargets();
    updateCrumbs();
    S.dirty = true;
  }

  function selectItem(itemNode) {
    if (!S || !itemNode) return;
    var node = itemNode.node || itemNode.leaf;
    var id = idOf(itemNode.leaf);
    S.selectedId = id;
    S.dirty = true;
    if (S.ctx.helpers && typeof S.ctx.helpers.onSelectNode === 'function') {
      try { S.ctx.helpers.onSelectNode(node); } catch (e) { /* ignore */ }
    }
  }

  function localPoint(ev) {
    var rect = S.wrap.getBoundingClientRect();
    return { x: ev.clientX - rect.left, y: ev.clientY - rect.top };
  }

  function hitKey(hit) {
    if (!hit) return null;
    return hit.node.key;
  }

  function tipHtml(hit) {
    var n = hit.node;
    if (hit.level === 'project') {
      var projItems = 0;
      n.children.forEach(function (t) { projItems += t.children.length; });
      return '<div style="font-weight:700;color:#e6f2ff;margin-bottom:2px">' +
        esc(clampText(n.label, 64)) + '</div><div style="opacity:.75">project · ' +
        n.children.length + ' types · ' + projItems + ' items</div>' +
        '<div style="opacity:.5;margin-top:2px">Click to zoom in</div>';
    }
    if (hit.level === 'type') {
      return '<div style="font-weight:700;color:#e6f2ff;margin-bottom:2px">' +
        esc(clampText(n.label, 64)) + '</div><div style="opacity:.75">' + esc(n.kind) +
        ' · ' + n.children.length + ' items</div>' +
        '<div style="opacity:.5;margin-top:2px">Click to zoom in</div>';
    }
    var node = n.node || n.leaf || {};
    return '<div style="font-weight:700;color:#e6f2ff;margin-bottom:2px">' +
      esc(clampText(n.label, 64)) + '</div><div style="opacity:.75">' +
      esc(node.kind || n.kind || 'node') + (num(node.degree) ? ' · ' + num(node.degree) + ' links' : '') +
      '</div><div style="opacity:.5;margin-top:2px">Click to select</div>';
  }

  /* ------------------------------------------------------------------- hit */

  function hitTest(p) {
    var world = {
      x: (p.x - S.size.w / 2) / S.cam.s + S.cam.x,
      y: (p.y - S.size.h / 2) / S.cam.s + S.cam.y
    };
    var best = null;
    for (var i = 0; i < S.vis.length; i++) {
      var v = S.vis[i];
      if (v.alpha < 0.45) continue;
      var node = v.node;
      if (node.r * S.cam.s < 1.4) continue;
      var dx = world.x - node.x;
      var dy = world.y - node.y;
      if (dx * dx + dy * dy <= node.r * node.r) {
        if (!best || node.r < best.node.r) best = v;
      }
    }
    return best;
  }

  /* ------------------------------------------------------------------ draw */

  function draw(time) {
    var g = S.g, w = S.size.w, h = S.size.h;
    g.setTransform(S.size.dpr, 0, 0, S.size.dpr, 0, 0);
    g.clearRect(0, 0, w, h);

    var cam = S.cam;
    var fProj = S.focus.projKey ? S.byProjKey[S.focus.projKey] : null;
    var fType = S.focus.typeKey ? S.byTypeKey[S.focus.typeKey] : null;
    S.vis = [];

    function screen(node) {
      return {
        x: w / 2 + (node.x - cam.x) * cam.s,
        y: h / 2 + (node.y - cam.y) * cam.s
      };
    }

    /* projects */
    S.projects.forEach(function (p) {
      var alpha;
      if (!fProj) alpha = 1;
      else alpha = p === fProj ? 1 : 0.1;

      var c = screen(p);
      var sr = p.r * cam.s;
      if (sr < 1.5) return;
      var hovP = S.hoverId === p.key;

      g.beginPath();
      g.arc(c.x, c.y, sr, 0, TAU);
      g.fillStyle = rgba(p.color, 0.06 * alpha + (p === fProj ? 0.025 : 0) + (hovP ? 0.04 : 0));
      g.fill();
      g.strokeStyle = rgba(p.color, (hovP ? 0.85 : (p === fProj ? 0.6 : 0.4)) * Math.max(alpha, 0.2));
      g.lineWidth = hovP ? 2 : (p === fProj ? 1.7 : 1.1);
      g.stroke();

      if (sr > 34 && alpha > 0.5 && (!fProj || p !== fProj)) {
        g.fillStyle = rgba(lighten(p.color, 0.4), 0.9);
        g.font = '700 ' + clamp(Math.round(sr * 0.09), 11, 15) + 'px ui-sans-serif,system-ui,sans-serif';
        g.textAlign = 'center';
        g.textBaseline = 'middle';
        g.fillText(clampText(p.label, Math.max(6, Math.floor(sr / 7))), c.x, c.y - sr * 0.82);
      }

      S.vis.push({ level: 'project', node: p, alpha: alpha });
    });

    /* types */
    S.projects.forEach(function (p) {
      p.children.forEach(function (t) {
        var alpha;
        if (fType) alpha = t === fType ? 1 : (fProj && t.parent === fProj ? 0.16 : 0.06);
        else if (fProj) alpha = t.parent === fProj ? 0.95 : 0.06;
        else {
          var sr0 = t.r * cam.s;
          alpha = sr0 > 34 ? 0.55 : (sr0 > 12 ? 0.32 : 0);
        }
        if (alpha <= 0.02) return;

        var c = screen(t);
        var sr = t.r * cam.s;
        if (sr < 1.6) return;
        if (c.x + sr < -20 || c.x - sr > w + 20 || c.y + sr < -20 || c.y - sr > h + 20) return;
        var hovT = S.hoverId === t.key;

        g.beginPath();
        g.arc(c.x, c.y, sr, 0, TAU);
        g.fillStyle = rgba(t.color, 0.1 * alpha + 0.02 + (hovT ? 0.05 : 0));
        g.fill();
        g.strokeStyle = rgba(t.color, (hovT ? 0.9 : 0.45) * alpha);
        g.lineWidth = hovT ? 2 : (t === fType ? 1.5 : 1);
        g.stroke();

        if (sr > 26 && alpha > 0.55) {
          g.fillStyle = rgba(lighten(t.color, 0.35), 0.88 * alpha);
          g.font = '600 ' + clamp(Math.round(sr * 0.08), 10, 13) + 'px ui-sans-serif,system-ui,sans-serif';
          g.textAlign = 'center';
          g.textBaseline = 'middle';
          g.fillText(clampText(t.label, Math.max(4, Math.floor(sr / 7))), c.x, c.y - sr * 0.8);
        }

        S.vis.push({ level: 'type', node: t, alpha: alpha });
      });
    });

    /* items */
    var selectedItem = null;
    S.projects.forEach(function (p) {
      p.children.forEach(function (t) {
        var tScreenR = t.r * cam.s;
        var alpha;
        if (fType) alpha = t === fType ? 1 : 0;
        else if (fProj) alpha = t.parent === fProj ? (tScreenR > 18 ? 0.9 : 0.35) : 0;
        else if (tScreenR > 46) alpha = 0.85;
        else if (tScreenR > 22) alpha = 0.32;
        else alpha = 0;

        t.children.forEach(function (it) {
          var id = idOf(it.leaf);
          var sel = id !== null && id === S.selectedId;
          var hov = S.hoverId === it.key;
          var a = alpha;
          if (sel || hov) a = Math.max(a, 0.95);
          if (a <= 0.02) return;

          var c = screen(it);
          var sr = Math.max(0.8, it.r * cam.s);
          if (c.x + sr < -20 || c.x - sr > w + 20 || c.y + sr < -20 || c.y - sr > h + 20) return;
          if (sr < 0.9 && !sel) return;

          g.beginPath();
          g.arc(c.x, c.y, sr, 0, TAU);
          g.fillStyle = rgba(it.color, (sel ? 0.98 : (hov ? 0.95 : 0.78)) * Math.max(a, 0.35));
          g.fill();

          if (sr > 4 && (sel || hov)) {
            g.strokeStyle = sel ? '#ffffff' : 'rgba(255,255,255,.6)';
            g.lineWidth = 1.2;
            g.stroke();
          }
          if (sel) selectedItem = { node: it, screen: c, r: sr };

          S.vis.push({ level: 'item', node: it, alpha: a });
        });
      });
    });

    /* selection halo */
    if (selectedItem) {
      var pulse = 0.5 + 0.5 * Math.sin(time * 3);
      g.save();
      g.strokeStyle = 'rgba(255,255,255,' + (0.45 + 0.35 * pulse) + ')';
      g.lineWidth = 1.6;
      g.shadowColor = selectedItem.node.color;
      g.shadowBlur = 10 + 8 * pulse;
      g.beginPath();
      g.arc(selectedItem.screen.x, selectedItem.screen.y, selectedItem.r + 4 + 3 * pulse, 0, TAU);
      g.stroke();
      g.restore();

      if (S.focus.typeKey) {
        var lbl = clampText(selectedItem.node.label, 42);
        g.font = '600 11px ui-sans-serif,system-ui,sans-serif';
        var tw = g.measureText(lbl).width;
        var bx = selectedItem.screen.x - tw / 2 - 8;
        var by = selectedItem.screen.y - selectedItem.r - 30;
        g.fillStyle = 'rgba(8,12,20,.85)';
        g.strokeStyle = 'rgba(120,200,255,.3)';
        g.lineWidth = 1;
        if (g.roundRect) {
          g.beginPath();
          g.roundRect(bx, by, tw + 16, 20, 6);
          g.fill();
          g.stroke();
        } else {
          g.fillRect(bx, by, tw + 16, 20);
        }
        g.fillStyle = '#dbeafe';
        g.textAlign = 'center';
        g.textBaseline = 'middle';
        g.fillText(lbl, selectedItem.screen.x, by + 10);
      }
    }
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
    var moving = Math.abs(cam.s - cam.sT) > Math.max(0.0005, cam.sT * 0.0008) ||
      Math.abs(cam.x - cam.xT) > 0.001 || Math.abs(cam.y - cam.yT) > 0.001;
    if (moving) {
      cam.s += (cam.sT - cam.s) * 0.16;
      cam.x += (cam.xT - cam.x) * 0.18;
      cam.y += (cam.yT - cam.y) * 0.18;
      if (Math.abs(cam.s - cam.sT) < Math.max(0.0005, cam.sT * 0.0008)) cam.s = cam.sT;
      if (Math.abs(cam.x - cam.xT) < 0.001) cam.x = cam.xT;
      if (Math.abs(cam.y - cam.yT) < 0.001) cam.y = cam.yT;
      S.dirty = true;
    }

    var pulsing = !!(S.selectedId || S.hoverId);
    if (!S.dirty && !moving && !pulsing) return;
    S.dirty = false;
    draw(ts / 1000);
  }

  /* ----------------------------------------------------------------- update */

  function update(ctx) {
    if (!S) return;
    if (ctx) S.ctx = ctx;
    S.selectedId = S.ctx.selectedId || null;

    var sig = signature(S.ctx);
    if (sig !== S.sig) {
      S.sig = sig;
      S.groups = normalizeTree(getTree(S.ctx));
      S.blinkId = null;
      rebuild(false);
    } else {
      computeTargets();
    }
    S.hoverId = null;
    S.tip.hide();
    S.dirty = true;
  }

  function focusNode(nodeId) {
    if (!S || !nodeId) return;
    var loc = S.index[nodeId];
    if (!loc) return;
    S.selectedId = nodeId;
    S.focus.projKey = loc.projNode.key;
    S.focus.typeKey = loc.typeNode.key;
    S.blinkId = nodeId;
    computeTargets();
    updateCrumbs();
    S.dirty = true;
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
    S.tip.destroy();
    if (S.root && S.root.parentNode) S.root.parentNode.removeChild(S.root);
    if (S.root) S.root.innerHTML = '';
    S = null;
  }

  G.views.packing = {
    mount: mount,
    update: update,
    focusNode: focusNode,
    destroy: destroy
  };
})(window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} });
