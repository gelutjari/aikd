/* AIKD Graph — Sunburst view (concentric radial hierarchy with drill-down).
 * Pure offline vanilla JS. Registers window.AIKD_GRAPH.views.sunburst.
 */
(function (G) {
  'use strict';

  G.views = G.views || {};

  var TAU = Math.PI * 2;
  var START = -Math.PI / 2;

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

  function pctText(pct) {
    var p = pct * 100;
    if (p >= 10) return Math.round(p) + '%';
    if (p >= 1) return p.toFixed(1) + '%';
    if (p >= 0.1) return p.toFixed(1) + '%';
    return p > 0 ? '<0.1%' : '0%';
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
          types.push({
            name: child.name || child.kind || 'items',
            kind: child.kind || 'type',
            id: child.id || null,
            leaves: leaves
          });
        }
      });
      if (direct.length) types.push({ name: 'General', kind: 'general', id: null, leaves: direct });

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
    title.textContent = 'Sunburst';
    title.style.cssText = 'font-weight:700;color:#dbeafe;letter-spacing:.02em;';
    var crumbs = document.createElement('div');
    crumbs.style.cssText = 'display:flex;align-items:center;gap:2px;overflow:hidden;white-space:nowrap;';
    var hint = document.createElement('span');
    hint.textContent = 'Click a ring slice to drill down · click the centre to go up';
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
    root.className = 'aikd-view aikd-sunburst';
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
      entries: [],
      index: Object.create(null),
      nodeGroups: Object.create(null),
      nodeTypes: Object.create(null),
      path: [],
      focus: null,
      arcs: [],
      prevArcs: null,
      reveal: 1,
      centerR: 30,
      totalWeight: 1,
      maxDepth: 3,
      hoverKey: null,
      selectedId: ctx.selectedId || null,
      flashKey: null,
      sig: '',
      lastTs: 0,
      dirty: true,
      raf: 0,
      ro: null
    };

    S.groups = normalizeTree(getTree(ctx));
    S.sig = signature(ctx);
    rebuild();
    bind();
    ctx.container.appendChild(root);
    resize();
    loop();
  }

  function bind() {
    var canvas = S.canvas;

    S.onMove = function (ev) {
      if (!S) return;
      var p = localPoint(ev);
      var inside = p.x >= 0 && p.y >= 0 && p.x <= S.size.w && p.y <= S.size.h;
      if (!inside) {
        if (S.hoverKey) { S.hoverKey = null; S.dirty = true; }
        S.tip.hide();
        return;
      }
      var hit = hitTest(p);
      var key = hit ? (hit.kind === 'center' ? '@center' : hit.arc.key) : null;
      if (key !== S.hoverKey) {
        S.hoverKey = key;
        S.dirty = true;
      }
      if (hit) {
        canvas.style.cursor = hit.kind === 'center' && !S.path.length ? 'default' : 'pointer';
        S.tip.show(tipHtml(hit), p.x, p.y);
      } else {
        canvas.style.cursor = 'default';
        S.tip.hide();
      }
    };
    S.onUp = function (ev) {
      if (!S) return;
      var p = localPoint(ev);
      if (p.x < 0 || p.y < 0 || p.x > S.size.w || p.y > S.size.h) return;
      var hit = hitTest(p);
      if (!hit) return;
      if (hit.kind === 'center') {
        if (S.path.length) setPath(S.path.slice(0, -1));
        return;
      }
      var entry = hit.arc.entry;
      if (entry.children && entry.children.length) {
        if (entry.type) setPath([entry.projectKey, entry.key]);
        else setPath([entry.key]);
        return;
      }
      selectLeaf(entry.leaf);
    };
    S.onLeave = function () {
      if (!S) return;
      S.hoverKey = null;
      S.tip.hide();
      S.dirty = true;
    };
    S.onResize = function () { if (S) resize(); };

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
    S.prevArcs = null;
    S.reveal = 1;
    buildLayout();
    S.dirty = true;
  }

  /* ---------------------------------------------------------------- model */

  function buildModel() {
    var entries = [];
    var index = Object.create(null);
    var nodeGroups = Object.create(null);
    var nodeTypes = Object.create(null);

    S.groups.forEach(function (g) {
      var projectEntry = {
        key: 'p:' + g.key,
        label: g.name,
        kind: 'project',
        color: projectColor(g.name),
        weight: g.weight,
        group: g,
        type: null,
        projectKey: null,
        leaf: null,
        node: null,
        children: []
      };
      projectEntry.projectKey = projectEntry.key;

      g.types.forEach(function (t) {
        var typeEntry = {
          key: 't:' + g.key + '::' + t.name,
          label: t.name,
          kind: t.kind,
          color: colorOf(t.kind, S.ctx.helpers),
          weight: t.weight,
          group: g,
          type: t,
          projectKey: projectEntry.key,
          leaf: null,
          node: null,
          children: []
        };
        if (t.id) nodeTypes[t.id] = typeEntry;

        t.leaves.forEach(function (leaf) {
          var id = idOf(leaf);
          var itemEntry = {
            key: 'i:' + (id != null ? id : ('x' + Math.random())),
            label: labelOf(leaf.node || leaf, S.ctx.helpers),
            kind: leaf.kind,
            color: colorOf(leaf.kind, S.ctx.helpers),
            weight: leaf.weight,
            group: g,
            type: t,
            projectKey: projectEntry.key,
            leaf: leaf,
            node: leaf.node || null,
            children: null
          };
          typeEntry.children.push(itemEntry);
          if (id != null) {
            index[id] = {
              group: g,
              type: t,
              entry: itemEntry,
              projKey: projectEntry.key,
              typeKey: typeEntry.key
            };
          }
        });

        projectEntry.children.push(typeEntry);
      });

      entries.push(projectEntry);
      if (g.id) nodeGroups[g.id] = projectEntry;
    });

    S.entries = entries;
    S.index = index;
    S.nodeGroups = nodeGroups;
    S.nodeTypes = nodeTypes;
  }

  function findEntryByKey(list, key) {
    for (var i = 0; i < list.length; i++) {
      if (list[i].key === key) return list[i];
    }
    return null;
  }

  function getFocus() {
    if (!S.path.length) {
      return { entries: S.entries, maxDepth: 3, group: null, type: null, projectEntry: null, typeEntry: null };
    }
    var projectEntry = findEntryByKey(S.entries, S.path[0]);
    if (!projectEntry) {
      return { entries: S.entries, maxDepth: 3, group: null, type: null, projectEntry: null, typeEntry: null };
    }
    if (S.path.length < 2) {
      return {
        entries: projectEntry.children,
        maxDepth: 2,
        group: projectEntry.group,
        type: null,
        projectEntry: projectEntry,
        typeEntry: null
      };
    }
    var typeEntry = findEntryByKey(projectEntry.children, S.path[1]);
    if (!typeEntry) {
      return {
        entries: projectEntry.children,
        maxDepth: 2,
        group: projectEntry.group,
        type: null,
        projectEntry: projectEntry,
        typeEntry: null
      };
    }
    return {
      entries: typeEntry.children,
      maxDepth: 1,
      group: projectEntry.group,
      type: typeEntry.type,
      projectEntry: projectEntry,
      typeEntry: typeEntry
    };
  }

  function buildLayout() {
    if (!S) return;
    var w = S.size.w, h = S.size.h;
    var R = Math.min(w, h) * 0.455;
    var centerR = Math.max(26, R * 0.16);
    var focus = getFocus();
    var maxDepth = focus.maxDepth;
    var gap = 1.5;
    var span = R - (centerR + gap);
    var th = Math.max(8, (span - gap * (maxDepth - 1)) / maxDepth);

    var arcs = [];
    var total = 0;
    focus.entries.forEach(function (e) { total += Math.max(0.0001, e.weight); });
    if (total <= 0) total = 1;

    function addLevel(entries, a0, a1, depth) {
      var sum = 0;
      entries.forEach(function (e) { sum += Math.max(0.0001, e.weight); });
      if (sum <= 0) return;

      var r0 = centerR + gap + (depth - 1) * (th + gap);
      var r1 = r0 + th;
      var angle = a0;

      entries.forEach(function (e) {
        var w2 = Math.max(0.0001, e.weight);
        var spanA = (w2 / sum) * (a1 - a0);
        if (spanA <= 0) return;
        var arc = {
          key: depth + '|' + e.key,
          entry: e,
          depth: depth,
          a0: angle,
          a1: angle + spanA,
          r0: r0,
          r1: r1,
          color: e.color,
          pct: w2 / total
        };
        arcs.push(arc);
        if (e.children && e.children.length && depth < maxDepth) {
          addLevel(e.children, arc.a0, arc.a1, depth + 1);
        }
        angle += spanA;
      });
    }

    addLevel(focus.entries, START, START + TAU, 1);

    S.focus = focus;
    S.arcs = arcs;
    S.centerR = centerR;
    S.outerR = centerR + gap + maxDepth * th + Math.max(0, maxDepth - 1) * gap;
    S.totalWeight = total;
    S.maxDepth = maxDepth;
  }

  function rebuild() {
    buildModel();

    if (S.path.length) {
      var projectEntry = findEntryByKey(S.entries, S.path[0]);
      if (!projectEntry) S.path = [];
      else if (S.path.length > 1 && !findEntryByKey(projectEntry.children, S.path[1])) {
        S.path = [S.path[0]];
      }
    }

    buildLayout();
    updateCrumbs();
    S.hoverKey = null;
    S.prevArcs = null;
    S.reveal = 1;
    S.dirty = true;
  }

  function setPath(next) {
    if (!S) return;
    var path = next || [];
    if (path.length === S.path.length) {
      var same = true;
      for (var i = 0; i < path.length; i++) {
        if (path[i] !== S.path[i]) { same = false; break; }
      }
      if (same) return;
    }
    S.prevArcs = S.arcs && S.arcs.length ? S.arcs.slice() : null;
    S.reveal = S.prevArcs ? 0 : 1;
    S.path = path;
    S.flashKey = null;
    S.hoverKey = null;
    S.tip.hide();
    buildLayout();
    updateCrumbs();
    S.dirty = true;
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
    }

    crumb('AIKD', function () { setPath([]); }, !S.path.length);

    var group = null;
    var type = null;
    if (S.focus) {
      group = S.focus.group;
      type = S.focus.type;
    }

    if (group) {
      var sep = document.createElement('span');
      sep.textContent = '/';
      sep.style.cssText = 'opacity:.35;padding:0 2px;';
      host.appendChild(sep);
      if (S.focus.projectEntry) {
        var pKey = S.focus.projectEntry.key;
        crumb(clampText(group.name, 24), function () { setPath([pKey]); }, !type);
      }
    }
    if (type && S.focus && S.focus.projectEntry && S.focus.typeEntry) {
      var sep2 = document.createElement('span');
      sep2.textContent = '/';
      sep2.style.cssText = 'opacity:.35;padding:0 2px;';
      host.appendChild(sep2);
      var pk = S.focus.projectEntry.key;
      var tk = S.focus.typeEntry.key;
      crumb(clampText(type.name, 24), function () { setPath([pk, tk]); }, true);
    }
  }

  function selectLeaf(leaf) {
    if (!S || !leaf) return;
    var node = leaf.node || leaf;
    S.selectedId = idOf(leaf);
    S.dirty = true;
    if (S.ctx.helpers && typeof S.ctx.helpers.onSelectNode === 'function') {
      try { S.ctx.helpers.onSelectNode(node); } catch (e) { /* ignore */ }
    }
  }

  function localPoint(ev) {
    var rect = S.wrap.getBoundingClientRect();
    return { x: ev.clientX - rect.left, y: ev.clientY - rect.top };
  }

  function tipHtml(hit) {
    if (hit.kind === 'center') {
      var label = S.focus && S.focus.type ? S.focus.type.name :
        (S.focus && S.focus.group ? S.focus.group.name : 'AIKD');
      var count = S.focus ? S.focus.entries.length : S.entries.length;
      return '<div style="font-weight:700;color:#e6f2ff;margin-bottom:2px">' +
        esc(clampText(label, 60)) + '</div><div style="opacity:.75">' + count + ' ' +
        (S.path.length >= 2 ? 'items' : S.path.length === 1 ? 'types' : 'projects') + '</div>' +
        (S.path.length ? '<div style="opacity:.5;margin-top:2px">Click to go up one level</div>' : '');
    }
    var arc = hit.arc;
    var e = arc.entry;
    if (e.leaf) {
      var node = e.node || e.leaf;
      return '<div style="font-weight:700;color:#e6f2ff;margin-bottom:2px">' +
        esc(clampText(labelOf(node, S.ctx.helpers), 60)) + '</div>' +
        '<div style="opacity:.75">' + esc(node.kind || e.kind || 'node') +
        (num(node.degree) ? ' · ' + num(node.degree) + ' links' : '') + '</div>' +
        '<div style="opacity:.6">' + pctText(arc.pct) + ' of ' +
        (S.path.length >= 2 ? 'this type' : S.path.length === 1 ? 'this project' : 'all nodes') +
        '</div><div style="opacity:.5;margin-top:2px">Click to select</div>';
    }
    return '<div style="font-weight:700;color:#e6f2ff;margin-bottom:2px">' +
      esc(clampText(e.label, 60)) + '</div><div style="opacity:.75">' + esc(e.kind) +
      ' · ' + (e.children ? e.children.length : 0) + ' children · ' + pctText(arc.pct) + '</div>' +
      '<div style="opacity:.5;margin-top:2px">Click to drill down</div>';
  }

  /* ------------------------------------------------------------------ hit */

  function hitTest(p) {
    var cx = S.size.w / 2;
    var cy = S.size.h / 2;
    var dx = p.x - cx;
    var dy = p.y - cy;
    var dist = Math.sqrt(dx * dx + dy * dy);
    if (dist <= S.centerR) return { kind: 'center' };

    var a = Math.atan2(dy, dx);
    if (a < START) a += TAU;
    if (a > START + TAU) a -= TAU;

    var arcs = S.arcs;
    for (var i = 0; i < arcs.length; i++) {
      var arc = arcs[i];
      if (dist >= arc.r0 && dist <= arc.r1 && a >= arc.a0 && a <= arc.a1) {
        return { kind: 'arc', arc: arc };
      }
    }
    return null;
  }

  /* ----------------------------------------------------------------- draw */

  function strokeArc(g, arc, extra, cx, cy) {
    g.beginPath();
    g.arc(cx, cy, Math.max(1, arc.r1 + extra), arc.a0, arc.a1);
    g.arc(cx, cy, Math.max(1, arc.r0 - extra * 0.5), arc.a1, arc.a0, true);
    g.closePath();
    g.stroke();
  }

  function drawArcSet(g, arcs, alpha, cx, cy, live, time) {
    if (!arcs || alpha <= 0.01) return;
    var hoverKey = live ? S.hoverKey : null;

    arcs.forEach(function (arc) {
      var isHover = live && hoverKey === arc.key;
      var r1 = arc.r1 + (isHover ? 3 : 0);
      var baseAlpha = arc.depth === 1 ? 0.6 : (arc.depth === 2 ? 0.45 : 0.34);
      var fillA = clamp(baseAlpha + (isHover ? 0.24 : 0), 0, 0.95) * alpha;

      g.beginPath();
      g.arc(cx, cy, r1, arc.a0, arc.a1);
      g.arc(cx, cy, arc.r0, arc.a1, arc.a0, true);
      g.closePath();
      g.fillStyle = rgba(isHover ? lighten(arc.color, 0.25) : arc.color, fillA);
      g.fill();

      if (isHover) {
        g.strokeStyle = rgba(lighten(arc.color, 0.55), 0.9 * alpha);
        g.lineWidth = 1.4;
      } else {
        g.strokeStyle = 'rgba(3,7,14,' + (0.5 * alpha) + ')';
        g.lineWidth = 0.7;
      }
      g.stroke();

      var spanA = arc.a1 - arc.a0;
      var thickness = arc.r1 - arc.r0;
      if (alpha > 0.7 && spanA > 0.055 && thickness > 11) {
        var mid = (arc.a0 + arc.a1) / 2;
        var midR = (arc.r0 + arc.r1) / 2;
        var flipped = Math.cos(mid) < 0;
        var txt = pctText(arc.pct);
        g.save();
        g.translate(cx, cy);
        g.rotate(flipped ? mid + Math.PI : mid);
        g.textAlign = 'center';
        g.textBaseline = 'middle';
        g.fillStyle = 'rgba(240,248,255,' + (0.9 * alpha) + ')';
        g.font = '600 ' + (spanA > 0.3 ? 11 : 10) + 'px ui-sans-serif,system-ui,sans-serif';
        var x = flipped ? -midR : midR;
        g.fillText(txt, x, 0);
        if (spanA > 0.55 && thickness > 26 && arc.entry.children) {
          g.fillStyle = 'rgba(219,234,254,' + (0.78 * alpha) + ')';
          g.font = '500 10px ui-sans-serif,system-ui,sans-serif';
          g.fillText(clampText(arc.entry.label, Math.floor(thickness / 5.5)), x, -11);
        }
        g.restore();
      }
    });

    if (!live) return;

    var selArc = null;
    if (S.selectedId) {
      for (var i = 0; i < arcs.length; i++) {
        if (arcs[i].entry.leaf && idOf(arcs[i].entry.leaf) === S.selectedId) {
          selArc = arcs[i];
          break;
        }
      }
    }
    if (selArc) {
      var pulse = 0.5 + 0.5 * Math.sin(time * 3);
      g.save();
      g.strokeStyle = 'rgba(255,255,255,' + (0.6 + 0.3 * pulse) + ')';
      g.lineWidth = 1.7;
      g.shadowColor = selArc.color;
      g.shadowBlur = 12;
      strokeArc(g, selArc, 1.5, cx, cy);
      g.stroke();
      g.restore();
    }

    if (S.flashKey) {
      var flash = null;
      for (var j = 0; j < arcs.length; j++) {
        if (arcs[j].entry.key === S.flashKey) { flash = arcs[j]; break; }
      }
      if (flash) {
        var pulse2 = 0.5 + 0.5 * Math.sin(time * 5);
        g.save();
        g.strokeStyle = 'rgba(255,255,255,' + (0.5 + 0.45 * pulse2) + ')';
        g.lineWidth = 2;
        g.shadowColor = '#ffffff';
        g.shadowBlur = 16;
        strokeArc(g, flash, 2.5, cx, cy);
        g.stroke();
        g.restore();
      }
    }
  }

  function drawCenter(g, time) {
    var cx = S.size.w / 2;
    var cy = S.size.h / 2;
    if (S.focus) {
      var r = S.centerR;
      var focusColor = S.focus.type ? colorOf(S.focus.type.kind, S.ctx.helpers) :
        (S.focus.group ? projectColor(S.focus.group.name) : '#7dd3fc');
      var label = S.focus.type ? S.focus.type.name :
        (S.focus.group ? S.focus.group.name : 'AIKD');

      g.beginPath();
      g.arc(cx, cy, r, 0, TAU);
      g.fillStyle = 'rgba(6,11,20,.92)';
      g.fill();
      g.strokeStyle = rgba(focusColor, 0.7);
      g.lineWidth = 1.5;
      g.stroke();

      if (S.path.length) {
        g.beginPath();
        g.arc(cx, cy, Math.max(4, r * 0.74), 0, TAU);
        g.strokeStyle = rgba(focusColor, 0.28);
        g.lineWidth = 1;
        g.stroke();
      }

      g.textAlign = 'center';
      g.textBaseline = 'middle';
      g.fillStyle = '#e6f2ff';
      g.font = '700 ' + clamp(Math.round(r * 0.19), 12, 17) + 'px ui-sans-serif,system-ui,sans-serif';
      g.fillText(clampText(label, Math.max(8, Math.floor(r / 4.5))), cx, cy - Math.max(5, r * 0.14));

      g.fillStyle = 'rgba(159,179,200,.85)';
      g.font = '500 10px ui-sans-serif,system-ui,sans-serif';
      var count = S.path.length >= 2 ? S.focus.entries.length : S.focus.entries.length;
      var unit = S.path.length >= 2 ? 'items' : (S.path.length === 1 ? 'types' : 'projects');
      g.fillText(count + ' ' + unit, cx, cy + Math.max(9, r * 0.22));
    }

    if (S.selectedId && S.arcs.length) {
      for (var i = 0; i < S.arcs.length; i++) {
        var arc = S.arcs[i];
        if (!arc.entry.leaf || idOf(arc.entry.leaf) !== S.selectedId) continue;
        var pulse = 0.5 + 0.5 * Math.sin(time * 3);
        var mid = (arc.a0 + arc.a1) / 2;
        var midR = (arc.r0 + arc.r1) / 2;
        g.beginPath();
        g.arc(cx + Math.cos(mid) * midR, cy + Math.sin(mid) * midR, 3 + 2 * pulse, 0, TAU);
        g.fillStyle = 'rgba(255,255,255,.95)';
        g.fill();
        break;
      }
    }
  }

  function draw(time) {
    var g = S.g, w = S.size.w, h = S.size.h;
    g.setTransform(S.size.dpr, 0, 0, S.size.dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    var cx = w / 2;
    var cy = h / 2;

    if (S.prevArcs && S.reveal < 1) {
      drawArcSet(g, S.prevArcs, 1 - S.reveal, cx, cy, false, time);
    }
    drawArcSet(g, S.arcs, S.reveal, cx, cy, true, time);
    drawCenter(g, time);
  }

  /* ------------------------------------------------------------------ loop */

  function loop() {
    if (!S) return;
    S.raf = requestAnimationFrame(frame);
  }

  function frame(ts) {
    if (!S) return;
    S.raf = requestAnimationFrame(frame);

    var dt = S.lastTs ? Math.min(0.05, (ts - S.lastTs) / 1000) : 0;
    S.lastTs = ts;

    if (S.reveal < 1) {
      S.reveal = Math.min(1, S.reveal + dt * 4.5);
      S.dirty = true;
      if (S.reveal >= 1) S.prevArcs = null;
    }

    var pulsing = !!(S.selectedId || S.flashKey || S.hoverKey);
    if (!S.dirty && !pulsing) return;
    S.dirty = false;
    draw(ts / 1000);
  }

  /* ----------------------------------------------------------------- update */

  function update(ctx) {
    if (!S) return;
    if (ctx) S.ctx = ctx;
    S.selectedId = S.ctx.selectedId || null;
    S.flashKey = null;

    var sig = signature(S.ctx);
    if (sig !== S.sig) {
      S.sig = sig;
      S.groups = normalizeTree(getTree(S.ctx));
      rebuild();
    } else {
      S.dirty = true;
    }
    S.hoverKey = null;
    S.tip.hide();
  }

  function focusNode(nodeId) {
    if (!S || !nodeId) return;
    var loc = S.index[nodeId];
    var next = null;
    var flash = null;

    if (loc) {
      next = [loc.projKey, loc.typeKey];
      flash = loc.entry.key;
    } else if (S.nodeGroups[nodeId]) {
      next = [];
      flash = S.nodeGroups[nodeId].key;
    } else if (S.nodeTypes[nodeId]) {
      var tEntry = S.nodeTypes[nodeId];
      next = [tEntry.projectKey];
      flash = tEntry.key;
    }

    if (next) setPath(next);
    S.selectedId = nodeId;
    S.flashKey = flash;
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

  G.views.sunburst = {
    mount: mount,
    update: update,
    focusNode: focusNode,
    destroy: destroy
  };
})(window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} });
