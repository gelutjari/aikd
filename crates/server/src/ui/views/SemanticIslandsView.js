/* ============================================================================
 * AIKD Knowledge Graph - Semantic Islands View ('islands')
 * ----------------------------------------------------------------------------
 * 2D topographic "islands" of related nodes (project + tag/Jaccard clusters).
 * Pure offline Vanilla JS, HiDPI canvas, pan / zoom, hover tooltip, spotlight,
 * smooth focusNode() fly-to, and full destroy() cleanup.
 * Registers window.AIKD_GRAPH.views.islands.
 * ========================================================================== */
(function (G) {
  'use strict';

  if (!G) return;
  G.views = G.views || {};

  var STYLE_ID = 'aikd-islands-style';

  var KIND_COLORS = {
    project: '#fbbf24', file: '#38bdf8', symbol: '#60a5fa', note: '#34d399',
    memory: '#c084fc', tag: '#f472b6', agent: '#fb7185', decision: '#34d399',
    constraint: '#fbbf24', fact: '#38bdf8', task: '#60a5fa', prompt: '#c084fc',
    checkpoint: '#94a3b8', daily_log: '#22d3ee', node: '#94a3b8'
  };

  var ISLAND_PALETTE = [
    '#34d399', '#38bdf8', '#c084fc', '#fbbf24', '#f472b6', '#60a5fa',
    '#2dd4bf', '#fb923c', '#a3e635', '#e879f9', '#22d3ee', '#facc15'
  ];

  var app = null;            // shared ui.html context
  var container = null;
  var root = null;
  var canvas = null;
  var g2d = null;
  var tip = null;
  var legendEl = null;
  var hintEl = null;
  var fitBtn = null;
  var ro = null;
  var raf = 0;

  var nodes = [];            // filtered nodes
  var edges = [];            // filtered edges
  var nodeById = new Map();
  var adj = new Map();
  var model = { islands: [], nodes: [], bounds: null, signature: '' };
  var screenNodes = [];
  var screenIslands = [];
  var selectedId = null;
  var hoverNodeId = null;
  var hoverIslandId = null;
  var focusSet = null;

  var view = { cx: 0, cy: 0, zoom: 1 };
  var fitZoom = 1;
  var target = null;
  var fitted = false;

  var dpr = 1;
  var W = 0;
  var H = 0;

  var drag = null;
  var pointerDirty = false;
  var destroyed = false;
  var usingWindowResize = false;

  /* ------------------------------------------------------------------ utils */

  function injectStyle() {
    if (document.getElementById(STYLE_ID)) return;
    var style = document.createElement('style');
    style.id = STYLE_ID;
    style.textContent = [
      '.aikd-islands{position:absolute;inset:0;overflow:hidden;background:radial-gradient(900px 560px at 46% 44%,#0d1a2b 0%,#070c14 70%);',
      'font-family:"Segoe UI",system-ui,-apple-system,sans-serif;color:#e6eefc;}',
      '.aikd-islands canvas{position:absolute;inset:0;width:100%;height:100%;display:block;cursor:grab;touch-action:none;}',
      '.aikd-islands canvas.dragging{cursor:grabbing;}',
      '.aikd-islands canvas.pointing{cursor:pointer;}',
      '.aikd-islands-legend{position:absolute;left:12px;top:12px;display:flex;flex-direction:column;gap:5px;max-width:230px;',
      'max-height:46%;overflow:hidden;padding:9px 11px;border:1px solid rgba(38,58,82,.9);border-radius:10px;',
      'background:rgba(7,12,20,.78);backdrop-filter:blur(7px);pointer-events:none;}',
      '.aikd-islands-legend .aikd-isl-title{font-size:9px;letter-spacing:.11em;text-transform:uppercase;color:#647892;}',
      '.aikd-islands-legend .aikd-isl-row{display:flex;align-items:center;gap:6px;font-size:10.5px;color:#8397b0;min-width:0;}',
      '.aikd-islands-legend .aikd-isl-dot{width:7px;height:7px;border-radius:50%;flex:0 0 auto;}',
      '.aikd-islands-legend .aikd-isl-name{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:140px;}',
      '.aikd-islands-legend .aikd-isl-count{margin-left:auto;font-family:ui-monospace,Consolas,monospace;font-size:9.5px;color:#647892;}',
      '.aikd-islands-hint{position:absolute;right:12px;bottom:12px;font-size:10px;color:#647892;background:rgba(7,12,20,.72);',
      'border:1px solid rgba(38,58,82,.9);border-radius:7px;padding:4px 8px;pointer-events:none;}',
      '.aikd-islands-fit{position:absolute;right:12px;top:12px;padding:4px 10px;font-size:10.5px;color:#a7f3d0;',
      'background:rgba(7,12,20,.8);border:1px solid rgba(52,211,153,.3);border-radius:7px;cursor:pointer;}',
      '.aikd-islands-fit:hover{border-color:rgba(52,211,153,.6);background:rgba(52,211,153,.12);}',
      '.aikd-islands-tip{position:absolute;z-index:6;max-width:270px;padding:8px 10px;border:1px solid rgba(56,189,248,.35);',
      'border-radius:9px;background:rgba(6,11,19,.95);box-shadow:0 14px 34px rgba(0,0,0,.5);pointer-events:none;',
      'font-size:11px;line-height:1.45;color:#dbe8f7;opacity:0;transform:translateY(4px);transition:opacity .1s ease;}',
      '.aikd-islands-tip.show{opacity:1;transform:translateY(0);}',
      '.aikd-islands-tip .aikd-tip-kind{font-size:9px;letter-spacing:.08em;text-transform:uppercase;color:#647892;}',
      '.aikd-islands-tip .aikd-tip-label{font-weight:600;font-size:12px;color:#f1f7ff;margin:2px 0 4px;}',
      '.aikd-islands-tip .aikd-tip-meta{color:#8397b0;font-size:10.5px;}',
      '.aikd-islands-tip .aikd-tip-hint{color:#4f6a88;font-size:9.5px;margin-top:5px;}'
    ].join('');
    document.head.appendChild(style);
  }

  function esc(s) {
    if (app && app.helpers && typeof app.helpers.esc === 'function') return app.helpers.esc(s);
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }

  function hashStr(s) {
    var h = 2166136261;
    var str = String(s == null ? '' : s);
    for (var i = 0; i < str.length; i++) {
      h ^= str.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    return h >>> 0;
  }

  function clamp(v, lo, hi) { return v < lo ? lo : (v > hi ? hi : v); }

  function num(v) { var n = Number(v); return isFinite(n) ? n : 0; }

  function kindColor(kind) {
    if (app && app.helpers && typeof app.helpers.kindColor === 'function') {
      try { return app.helpers.kindColor(kind); } catch (e) { /* fall through */ }
    }
    return KIND_COLORS[String(kind || '').toLowerCase()] || KIND_COLORS.node;
  }

  function nodeLabel(node) {
    if (!node) return '(unknown)';
    if (app && app.helpers && typeof app.helpers.graphLabel === 'function') {
      try { return app.helpers.graphLabel(node) || node.id; } catch (e) { /* fall through */ }
    }
    return String(node.label || node.id || '(unknown)');
  }

  function rgba(hex, alpha) {
    var h = String(hex || '').replace('#', '');
    if (h.length === 3) h = h.split('').map(function (c) { return c + c; }).join('');
    if (!/^[0-9a-f]{6}$/i.test(h)) return 'rgba(148,163,184,' + alpha + ')';
    var n = parseInt(h, 16);
    return 'rgba(' + ((n >> 16) & 255) + ',' + ((n >> 8) & 255) + ',' + (n & 255) + ',' + alpha + ')';
  }

  function isValidColor(c) {
    return typeof c === 'string' && (/^#[0-9a-f]{3,8}$/i.test(c) || /^rgba?\(/i.test(c));
  }

  function paletteFor(key) {
    return ISLAND_PALETTE[hashStr(key) % ISLAND_PALETTE.length];
  }

  function fitText(g, text, maxW) {
    var t = String(text || '');
    if (g.measureText(t).width <= maxW) return t;
    while (t.length > 3 && g.measureText(t + '…').width > maxW) t = t.slice(0, -1);
    return t + '…';
  }

  function roundRectPath(g, x, y, w, h, r) {
    var rr = Math.min(r, h / 2, w / 2);
    g.beginPath();
    g.moveTo(x + rr, y);
    g.lineTo(x + w - rr, y);
    g.quadraticCurveTo(x + w, y, x + w, y + rr);
    g.lineTo(x + w, y + h - rr);
    g.quadraticCurveTo(x + w, y + h, x + w - rr, y + h);
    g.lineTo(x + rr, y + h);
    g.quadraticCurveTo(x, y + h, x, y + h - rr);
    g.lineTo(x, y + rr);
    g.quadraticCurveTo(x, y, x + rr, y);
    g.closePath();
  }

  function coordOf(value) {
    if (!value) return null;
    if (Array.isArray(value) && value.length >= 2) {
      var ax = Number(value[0]); var ay = Number(value[1]);
      return isFinite(ax) && isFinite(ay) ? { x: ax, y: ay } : null;
    }
    if (typeof value === 'object') {
      var x = Number(value.x); var y = Number(value.y);
      return isFinite(x) && isFinite(y) ? { x: x, y: y } : null;
    }
    return null;
  }

  /* ------------------------------------------------------------ data model */

  function buildAdjacency(list) {
    var map = new Map();
    list.forEach(function (e) {
      if (!e || !e.from || !e.to) return;
      if (!map.has(e.from)) map.set(e.from, new Set());
      if (!map.has(e.to)) map.set(e.to, new Set());
      map.get(e.from).add(e.to);
      map.get(e.to).add(e.from);
    });
    return map;
  }

  function fallbackIslands(list, edgeList) {
    var groups = new Map();
    var byId = new Map();
    list.forEach(function (n) { byId.set(n.id, n); });
    var adjLocal = buildAdjacency(edgeList || []);

    function groupKey(n) {
      if (n.project) return 'project:' + n.project;
      if (n.scope && n.scope !== 'global') return 'scope:' + n.scope;
      return 'general';
    }

    list.forEach(function (n) {
      var key = groupKey(n);
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(n);
    });

    // Merge tiny orphan groups into the general island.
    var keys = Array.from(groups.keys());
    if (keys.length > 1) {
      var general = groups.get('general') || [];
      keys.forEach(function (key) {
        if (key === 'general') return;
        var members = groups.get(key);
        if (members && members.length <= 1) {
          general = general.concat(members);
          groups.delete(key);
        }
      });
      if (general.length) groups.set('general', general);
    }
    if (!groups.size && list.length) groups.set('general', list.slice());

    var islandList = [];
    var coords = new Map();
    var keys2 = Array.from(groups.keys());
    var cols = Math.max(1, Math.ceil(Math.sqrt(keys2.length)));
    var cell = 320;

    keys2.forEach(function (key, gi) {
      var members = groups.get(key) || [];
      var label = key === 'general'
        ? 'Unassigned / General'
        : (key.indexOf('project:') === 0 ? key.slice(8) : key.slice(6));
      var gx = (gi % cols) * cell;
      var gy = Math.floor(gi / cols) * cell;
      var color = paletteFor(key);
      var cx = gx + cell / 2;
      var cy = gy + cell / 2;
      var pts = [];

      members.forEach(function (n, i) {
        var a = i * 2.399963229728653 + hashStr(n.id) % 100 / 100;
        var r = 14 + 11 * Math.sqrt(i + 1);
        var p = { x: cx + Math.cos(a) * r, y: cy + Math.sin(a) * r * 0.82 };
        coords.set(n.id, p);
        pts.push(p);
      });

      islandList.push({
        id: key, label: label, color: color, cx: cx, cy: cy,
        rx: 60, ry: 50, nodes: members, points: pts
      });
    });

    return { islands: islandList, nodeCoords: coords };
  }

  function normalizeModel(raw) {
    raw = raw || {};
    var rawIslands = Array.isArray(raw.islands) ? raw.islands : [];
    var rawCoords = raw.nodeCoords;
    var coords = new Map();
    if (rawCoords instanceof Map) {
      rawCoords.forEach(function (v, k) { var c = coordOf(v); if (c) coords.set(k, c); });
    } else if (rawCoords && typeof rawCoords === 'object') {
      Object.keys(rawCoords).forEach(function (k) {
        var c = coordOf(rawCoords[k]);
        if (c) coords.set(k, c);
      });
    }

    var seenIds = new Set();
    var islands = [];
    rawIslands.forEach(function (island, idx) {
      if (!island) return;
      var members = [];
      var rawMembers = Array.isArray(island.nodes) ? island.nodes : [];
      rawMembers.forEach(function (m) {
        var node = (typeof m === 'string') ? nodeById.get(m) : (m && m.id ? (nodeById.get(m.id) || m) : null);
        if (node && node.id && !seenIds.has(node.id)) {
          seenIds.add(node.id);
          members.push(node);
        }
      });
      if (!members.length) return;

      var sumX = 0, sumY = 0, found = 0;
      members.forEach(function (n) {
        var p = coords.get(n.id);
        if (p) { sumX += p.x; sumY += p.y; found++; }
      });
      var cx = Number(island.cx);
      var cy = Number(island.cy);
      if (!isFinite(cx) || !isFinite(cy)) {
        cx = found ? sumX / found : idx * 280;
        cy = found ? sumY / found : 0;
      }

      // Fill missing member coordinates deterministically around the island.
      members.forEach(function (n, i) {
        if (coords.has(n.id)) return;
        var a = i * 2.399963229728653;
        var r = 12 + 10 * Math.sqrt(i + 1);
        coords.set(n.id, { x: cx + Math.cos(a) * r, y: cy + Math.sin(a) * r * 0.82 });
      });

      var maxDx = 18, maxDy = 16;
      members.forEach(function (n) {
        var p = coords.get(n.id);
        if (!p) return;
        maxDx = Math.max(maxDx, Math.abs(p.x - cx));
        maxDy = Math.max(maxDy, Math.abs(p.y - cy));
      });

      var color = isValidColor(island.color) ? island.color : paletteFor(island.id || island.label || idx);
      islands.push({
        id: String(island.id || island.label || ('island-' + idx)),
        label: String(island.label || island.id || 'Island ' + (idx + 1)),
        color: color,
        cx: cx, cy: cy,
        rx: Math.max(num(island.rx), maxDx * 1.42 + 16),
        ry: Math.max(num(island.ry), maxDy * 1.55 + 16),
        seed: hashStr(island.id || island.label || idx) % 1000,
        nodes: members,
        count: members.length
      });
    });

    // Any node the adapter missed still needs a home.
    var leftovers = nodes.filter(function (n) { return !seenIds.has(n.id); });
    if (leftovers.length) {
      var maxY = 0;
      islands.forEach(function (is) { maxY = Math.max(maxY, is.cy + is.ry); });
      var lcx = 0;
      var lcy = islands.length ? maxY + 220 : 0;
      var pts = [];
      leftovers.forEach(function (n, i) {
        var a = i * 2.399963229728653;
        var r = 14 + 11 * Math.sqrt(i + 1);
        var p = { x: lcx + Math.cos(a) * r, y: lcy + Math.sin(a) * r * 0.82 };
        coords.set(n.id, p);
        pts.push(p);
        seenIds.add(n.id);
      });
      islands.push({
        id: '__leftover', label: 'Unassigned / General', color: paletteFor('leftover'),
        cx: lcx, cy: lcy, rx: 110, ry: 84, seed: 421,
        nodes: leftovers, points: pts, count: leftovers.length
      });
    }

    var viewNodes = [];
    var minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    nodes.forEach(function (n) {
      var p = coords.get(n.id);
      if (!p) return;
      viewNodes.push({ node: n, x: p.x, y: p.y });
      if (p.x < minX) minX = p.x;
      if (p.x > maxX) maxX = p.x;
      if (p.y < minY) minY = p.y;
      if (p.y > maxY) maxY = p.y;
    });
    islands.forEach(function (is) {
      minX = Math.min(minX, is.cx - is.rx);
      maxX = Math.max(maxX, is.cx + is.rx);
      minY = Math.min(minY, is.cy - is.ry);
      maxY = Math.max(maxY, is.cy + is.ry);
    });
    if (!isFinite(minX)) { minX = -100; maxX = 100; minY = -100; maxY = 100; }

    var pad = 40;
    var bounds = { minX: minX - pad, minY: minY - pad, maxX: maxX + pad, maxY: maxY + pad };
    var signature = islands.map(function (is) { return is.id + ':' + is.count; }).sort().join('|');
    var colorByNode = new Map();
    islands.forEach(function (is) {
      is.nodes.forEach(function (n) { colorByNode.set(n.id, is.color); });
    });

    return {
      islands: islands,
      nodes: viewNodes,
      bounds: bounds,
      signature: signature,
      colorByNode: colorByNode
    };
  }

  function setData(context) {
    nodes = Array.isArray(context.nodes) ? context.nodes : [];
    edges = Array.isArray(context.edges) ? context.edges : [];
    nodeById = new Map();
    (Array.isArray(context.rawNodes) ? context.rawNodes : []).forEach(function (n) {
      if (n && n.id != null) nodeById.set(n.id, n);
    });
    nodes.forEach(function (n) { if (n && n.id != null) nodeById.set(n.id, n); });
    adj = buildAdjacency(Array.isArray(context.rawEdges) ? context.rawEdges : edges);

    var raw = null;
    var adapter = G.adapter;
    if (adapter && typeof adapter.toSemanticIslands === 'function') {
      try { raw = adapter.toSemanticIslands(nodes, edges); } catch (e) { raw = null; }
    }
    if (!raw) raw = fallbackIslands(nodes, edges);

    var previous = model.signature;
    model = normalizeModel(raw);
    renderLegend();

    if (!root) return;
    if (previous !== model.signature) {
      fitted = false;
      refit();
    }
    pointerDirty = true;
  }

  /* ------------------------------------------------------------- viewport */

  function boundsSpan() {
    var b = model.bounds;
    if (!b) return { cx: 0, cy: 0, w: 1, h: 1 };
    return {
      cx: (b.minX + b.maxX) / 2,
      cy: (b.minY + b.maxY) / 2,
      w: Math.max(1, b.maxX - b.minX),
      h: Math.max(1, b.maxY - b.minY)
    };
  }

  function refit() {
    if (!W || !H) { fitted = false; return; }
    var s = boundsSpan();
    fitZoom = Math.min((W * 0.88) / s.w, (H * 0.84) / s.h);
    if (!isFinite(fitZoom) || fitZoom <= 0) fitZoom = 1;
    view.zoom = fitZoom;
    view.cx = s.cx;
    view.cy = s.cy;
    target = null;
    fitted = true;
    pointerDirty = true;
  }

  function toScreen(x, y) {
    return {
      x: (x - view.cx) * view.zoom + W / 2,
      y: (y - view.cy) * view.zoom + H / 2
    };
  }

  function toWorld(sx, sy) {
    return {
      x: (sx - W / 2) / Math.max(1e-6, view.zoom) + view.cx,
      y: (sy - H / 2) / Math.max(1e-6, view.zoom) + view.cy
    };
  }

  function clampZoom(z) {
    return clamp(z, fitZoom * 0.12, fitZoom * 45);
  }

  /* ------------------------------------------------------------------- draw */

  function islandPath(g, sx, sy, rx, ry, seed, scale) {
    var N = 84;
    g.beginPath();
    for (var i = 0; i <= N; i++) {
      var a = (i / N) * Math.PI * 2;
      var wobble = 1
        + 0.075 * Math.sin(a * 3 + seed)
        + 0.045 * Math.sin(a * 5 - seed * 1.7)
        + 0.028 * Math.sin(a * 7 + seed * 0.6);
      var rr = scale * wobble;
      var x = sx + Math.cos(a) * rx * rr;
      var y = sy + Math.sin(a) * ry * rr;
      if (i === 0) g.moveTo(x, y); else g.lineTo(x, y);
    }
    g.closePath();
  }

  function focusNodeId() {
    return hoverNodeId || selectedId;
  }

  function draw(now) {
    if (!g2d || !W || !H) return;
    var g = g2d;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, W, H);

    // Ambient core glow.
    var glow = g.createRadialGradient(W / 2, H / 2, 0, W / 2, H / 2, Math.max(W, H) * 0.6);
    glow.addColorStop(0, 'rgba(56,189,248,0.05)');
    glow.addColorStop(0.55, 'rgba(52,211,153,0.025)');
    glow.addColorStop(1, 'rgba(0,0,0,0)');
    g.fillStyle = glow;
    g.fillRect(0, 0, W, H);

    screenNodes = [];
    screenIslands = [];
    if (!model.nodes.length) {
      g.fillStyle = 'rgba(131,151,176,0.7)';
      g.font = '600 13px "Segoe UI", system-ui, sans-serif';
      g.textAlign = 'center';
      g.textBaseline = 'middle';
      g.fillText('No nodes for Semantic Islands yet', W / 2, H / 2 - 8);
      g.fillStyle = 'rgba(100,120,146,0.65)';
      g.font = '11px "Segoe UI", system-ui, sans-serif';
      g.fillText('Scan a workspace or adjust the type filters.', W / 2, H / 2 + 14);
      g.textAlign = 'start';
      g.textBaseline = 'alphabetic';
      return;
    }

    var focusId = focusNodeId();
    focusSet = null;
    var focusIslandId = null;
    if (focusId && adj.has(focusId)) {
      focusSet = new Set([focusId]);
      adj.get(focusId).forEach(function (id) { focusSet.add(id); });
    }
    if (focusId) {
      for (var fi = 0; fi < model.islands.length; fi++) {
        if (model.islands[fi].nodes.some(function (n) { return n.id === focusId; })) {
          focusIslandId = model.islands[fi].id;
          break;
        }
      }
    }

    var pulse = 0.5 + 0.5 * Math.sin(now / 430);

    // 1) Island bodies (fills + topographic contour rings).
    model.islands.forEach(function (is) {
      var p = toScreen(is.cx, is.cy);
      var rx = is.rx * view.zoom;
      var ry = is.ry * view.zoom;
      if (rx < 2 || ry < 2) return;
      if (p.x + rx < -80 || p.x - rx > W + 80 || p.y + ry < -80 || p.y - ry > H + 80) return;

      var active = !focusId || focusIslandId === is.id;
      screenIslands.push({ island: is, x: p.x, y: p.y, rx: rx, ry: ry });

      g.save();
      g.globalAlpha = active ? 1 : 0.34;

      // Outer glow.
      g.shadowColor = rgba(is.color, active ? 0.35 : 0.12);
      g.shadowBlur = Math.min(46, 18 + rx * 0.2);
      islandPath(g, p.x, p.y, rx, ry, is.seed, 1);
      var grad = g.createRadialGradient(p.x, p.y, 0, p.x, p.y, Math.max(rx, ry));
      grad.addColorStop(0, rgba(is.color, 0.16));
      grad.addColorStop(0.62, rgba(is.color, 0.075));
      grad.addColorStop(1, rgba(is.color, 0.012));
      g.fillStyle = grad;
      g.fill();
      g.shadowBlur = 0;

      // Contour rings.
      var rings = [
        { s: 1, a: 0.42, w: 1.3 },
        { s: 0.78, a: 0.24, w: 1.0 },
        { s: 0.56, a: 0.16, w: 0.9 },
        { s: 0.35, a: 0.1, w: 0.8 }
      ];
      rings.forEach(function (ring) {
        islandPath(g, p.x, p.y, rx, ry, is.seed, ring.s);
        g.strokeStyle = rgba(is.color, ring.a);
        g.lineWidth = ring.w;
        g.stroke();
      });
      g.restore();

      // Island caption pill.
      if (rx > 52 && ry > 34) {
        var cap = String(is.label || '');
        g.font = '600 10.5px "Segoe UI", system-ui, sans-serif';
        var label = fitText(g, cap, Math.max(60, rx * 1.7));
        var countText = is.count + ' node' + (is.count === 1 ? '' : 's');
        var tw = Math.max(g.measureText(label).width, 9 + g.measureText(countText).width);
        var pillW = tw + 22;
        var pillH = 30;
        var px = p.x - pillW / 2;
        var py = p.y - ry - 12;
        g.save();
        g.globalAlpha = active ? 0.95 : 0.4;
        roundRectPath(g, px, py, pillW, pillH, 8);
        g.fillStyle = 'rgba(6,11,19,0.82)';
        g.fill();
        g.strokeStyle = rgba(is.color, 0.38);
        g.lineWidth = 1;
        g.stroke();
        g.fillStyle = is.color;
        g.beginPath();
        g.arc(px + 9, py + 10, 2.6, 0, Math.PI * 2);
        g.fill();
        g.fillStyle = '#dbe8f7';
        g.textBaseline = 'middle';
        g.fillText(label, px + 15, py + 10.5);
        g.fillStyle = 'rgba(131,151,176,0.85)';
        g.font = '9.5px "Segoe UI", system-ui, sans-serif';
        g.fillText(countText, px + 16, py + 22);
        g.restore();
        g.textBaseline = 'alphabetic';
      }
    });

    // 2) Edges (faint ambient, brighter around focus).
    var positions = new Map();
    model.nodes.forEach(function (entry) {
      var p = toScreen(entry.x, entry.y);
      positions.set(entry.node.id, p);
    });
    var edgeLimit = 2600;
    var seenEdge = 0;
    for (var ei = 0; ei < edges.length && seenEdge < edgeLimit; ei++) {
      var e = edges[ei];
      if (!e || !e.from || !e.to) continue;
      var pa = positions.get(e.from);
      var pb = positions.get(e.to);
      if (!pa || !pb) continue;
      seenEdge++;
      var inFocus = !focusSet || (focusSet.has(e.from) && focusSet.has(e.to));
      if (inFocus) continue; // focused edges are drawn later, on top
      g.beginPath();
      g.moveTo(pa.x, pa.y);
      g.lineTo(pb.x, pb.y);
      g.strokeStyle = rgba(islandColorForNode(e.from, '#647892'), 0.09);
      g.lineWidth = 0.7;
      g.stroke();
    }
    if (focusSet) {
      for (var ej = 0; ej < edges.length && ej < 9000; ej++) {
        var fe = edges[ej];
        if (!fe || !focusSet.has(fe.from) && !focusSet.has(fe.to)) continue;
        var fa = positions.get(fe.from);
        var fb = positions.get(fe.to);
        if (!fa || !fb) continue;
        g.beginPath();
        g.moveTo(fa.x, fa.y);
        g.lineTo(fb.x, fb.y);
        g.strokeStyle = rgba(islandColorForNode(fe.from, '#38bdf8'), 0.5);
        g.lineWidth = 1.15;
        g.stroke();
      }
    }

    // 3) Node dots.
    model.nodes.forEach(function (entry) {
      var n = entry.node;
      var p = positions.get(n.id) || toScreen(entry.x, entry.y);
      if (p.x < -24 || p.x > W + 24 || p.y < -24 || p.y > H + 24) return;
      var deg = num(n.degree);
      var r = clamp(2.6 + Math.sqrt(deg) * 0.72, 2.8, 8.5);
      if (view.zoom > fitZoom * 2.2) r *= 1.06;
      var isSel = n.id === selectedId;
      var isHover = n.id === hoverNodeId;
      var dim = focusSet ? !focusSet.has(n.id) : false;
      var color = kindColor(n.kind);

      screenNodes.push({ node: n, x: p.x, y: p.y, r: Math.max(r, 3) });

      g.save();
      g.globalAlpha = dim ? 0.14 : 0.96;
      if (isSel) {
        g.shadowColor = rgba(color, 0.9);
        g.shadowBlur = 12 + pulse * 12;
      } else if (isHover) {
        g.shadowColor = rgba(color, 0.7);
        g.shadowBlur = 10;
      }
      g.fillStyle = color;
      g.beginPath();
      g.arc(p.x, p.y, r, 0, Math.PI * 2);
      g.fill();
      g.shadowBlur = 0;

      if (isSel || (focusSet && focusSet.has(n.id) && n.id !== selectedId)) {
        g.strokeStyle = isSel ? '#ffffff' : rgba(color, 0.8);
        g.lineWidth = isSel ? 1.6 : 1;
        g.beginPath();
        g.arc(p.x, p.y, r + (isSel ? 4 + pulse * 2 : 2.4), 0, Math.PI * 2);
        g.stroke();
      }
      g.restore();
    });

    // 4) Hover / selection callout label.
    var calloutId = hoverNodeId || selectedId;
    if (calloutId) {
      var calloutNode = nodeById.get(calloutId);
      var cp = positions.get(calloutId);
      if (calloutNode && cp) {
        g.save();
        g.font = '600 11px "Segoe UI", system-ui, sans-serif';
        var lbl = fitText(g, nodeLabel(calloutNode), 220);
        var tw2 = g.measureText(lbl).width;
        var bx = clamp(cp.x + 12, 8, Math.max(8, W - tw2 - 28));
        var by = clamp(cp.y - 26, 8, H - 32);
        roundRectPath(g, bx, by, tw2 + 16, 22, 6);
        g.fillStyle = 'rgba(6,11,19,0.92)';
        g.fill();
        g.strokeStyle = rgba(kindColor(calloutNode.kind), 0.5);
        g.lineWidth = 1;
        g.stroke();
        g.fillStyle = '#eaf3ff';
        g.textBaseline = 'middle';
        g.fillText(lbl, bx + 8, by + 11.5);
        g.restore();
        g.textBaseline = 'alphabetic';
      }
    }

    // 5) Label the strongest node of each island (single, unobtrusive callout).
    if (!focusSet && model.nodes.length <= 160) {
      g.save();
      g.font = '10px "Segoe UI", system-ui, sans-serif';
      g.textAlign = 'center';
      model.islands.forEach(function (is) {
        var top = is.nodes.slice().sort(function (a, b) {
          return num(b.degree) - num(a.degree);
        })[0];
        if (!top || num(top.degree) < 2) return;
        var p = positions.get(top.id);
        if (!p) return;
        var text = fitText(g, nodeLabel(top), 130);
        var tw = g.measureText(text).width;
        var bx = clamp(p.x - tw / 2 - 5, 4, Math.max(4, W - tw - 14));
        var by = p.y + 14;
        roundRectPath(g, bx, by, tw + 10, 15, 4);
        g.fillStyle = 'rgba(6,11,19,0.72)';
        g.fill();
        g.fillStyle = 'rgba(219,232,247,0.82)';
        g.textBaseline = 'middle';
        g.fillText(text, bx + tw / 2 + 5, by + 7.5);
      });
      g.textAlign = 'start';
      g.restore();
      g.textBaseline = 'alphabetic';
    }

    // 6) Alignment pulses near the selected node.
    if (selectedId) {
      var sp = positions.get(selectedId);
      if (sp) {
        g.save();
        g.strokeStyle = 'rgba(52,211,153,' + (0.28 + pulse * 0.25) + ')';
        g.lineWidth = 1;
        g.beginPath();
        g.arc(sp.x, sp.y, 12 + pulse * 6, 0, Math.PI * 2);
        g.stroke();
        g.restore();
      }
    }
  }

  function islandColorForNode(nodeId, fallback) {
    if (model.colorByNode && model.colorByNode.has(nodeId)) return model.colorByNode.get(nodeId);
    return fallback;
  }

  /* ------------------------------------------------------------- interaction */

  function hitNode(mx, my) {
    var best = null;
    var bestD = Infinity;
    for (var i = 0; i < screenNodes.length; i++) {
      var s = screenNodes[i];
      var dx = s.x - mx;
      var dy = s.y - my;
      var d = dx * dx + dy * dy;
      var hitR = Math.max(6, s.r + 3);
      if (d <= hitR * hitR && d < bestD) {
        best = s.node;
        bestD = d;
      }
    }
    return best;
  }

  function hitIsland(mx, my) {
    for (var i = screenIslands.length - 1; i >= 0; i--) {
      var s = screenIslands[i];
      if (s.rx < 2 || s.ry < 2) continue;
      var nx = (mx - s.x) / s.rx;
      var ny = (my - s.y) / s.ry;
      if (nx * nx + ny * ny <= 1.05) return s.island;
    }
    return null;
  }

  function showTip(node, mx, my) {
    if (!tip || !node) return;
    var project = node.project || 'Unassigned / General';
    tip.innerHTML = ''
      + '<div class="aikd-tip-kind">' + esc(node.kind || 'node')
      + (node.subtype ? ' · ' + esc(node.subtype) : '') + '</div>'
      + '<div class="aikd-tip-label">' + esc(String(node.label || node.id)) + '</div>'
      + '<div class="aikd-tip-meta">' + esc(project) + ' · degree ' + num(node.degree) + '</div>'
      + '<div class="aikd-tip-hint">click inspect · double-click focus local</div>';
    tip.classList.add('show');
    positionTip(mx, my);
  }

  function showIslandTip(island, mx, my) {
    if (!tip || !island) return;
    var kinds = {};
    island.nodes.forEach(function (n) { kinds[n.kind] = (kinds[n.kind] || 0) + 1; });
    var parts = Object.keys(kinds).sort(function (a, b) { return kinds[b] - kinds[a]; }).slice(0, 4)
      .map(function (k) { return k + ' ' + kinds[k]; }).join(' · ');
    tip.innerHTML = ''
      + '<div class="aikd-tip-kind">semantic island</div>'
      + '<div class="aikd-tip-label">' + esc(island.label) + '</div>'
      + '<div class="aikd-tip-meta">' + island.count + ' nodes · ' + esc(parts) + '</div>'
      + '<div class="aikd-tip-hint">click to zoom into this island</div>';
    tip.classList.add('show');
    positionTip(mx, my);
  }

  function positionTip(mx, my) {
    if (!tip || !W) return;
    var tw = tip.offsetWidth || 220;
    var th = tip.offsetHeight || 70;
    var x = mx + 16;
    var y = my + 16;
    if (x + tw > W - 10) x = mx - tw - 14;
    if (y + th > H - 10) y = my - th - 14;
    tip.style.left = Math.max(6, x) + 'px';
    tip.style.top = Math.max(6, y) + 'px';
  }

  function hideTip() {
    if (tip) tip.classList.remove('show');
  }

  function localXY(ev) {
    var rect = canvas.getBoundingClientRect();
    return { x: ev.clientX - rect.left, y: ev.clientY - rect.top };
  }

  function onPointerDown(ev) {
    if (ev.button !== undefined && ev.button !== 0) return;
    var p = localXY(ev);
    drag = { x: p.x, y: p.y, cx: view.cx, cy: view.cy, moved: false };
    canvas.classList.add('dragging');
    if (canvas.setPointerCapture && ev.pointerId != null) {
      try { canvas.setPointerCapture(ev.pointerId); } catch (e) { /* ignore */ }
    }
    target = null;
  }

  function onPointerMove(ev) {
    var p = localXY(ev);

    if (drag) {
      var dx = p.x - drag.x;
      var dy = p.y - drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
      if (drag.moved) {
        view.cx = drag.cx - dx / Math.max(1e-6, view.zoom);
        view.cy = drag.cy - dy / Math.max(1e-6, view.zoom);
        target = null;
        pointerDirty = true;
        hideTip();
      }
      return;
    }

    var node = hitNode(p.x, p.y);
    if (node) {
      hoverNodeId = node.id;
      hoverIslandId = null;
      canvas.classList.add('pointing');
      showTip(nodeById.get(node.id) || node, p.x, p.y);
      pointerDirty = true;
      return;
    }
    var island = hitIsland(p.x, p.y);
    if (island) {
      hoverIslandId = island.id;
      hoverNodeId = null;
      canvas.classList.remove('pointing');
      showIslandTip(island, p.x, p.y);
      pointerDirty = true;
      return;
    }
    if (hoverNodeId || hoverIslandId) pointerDirty = true;
    hoverNodeId = null;
    hoverIslandId = null;
    canvas.classList.remove('pointing');
    hideTip();
  }

  function onPointerUp(ev) {
    if (!drag) return;
    var wasDrag = drag.moved;
    var start = { x: drag.x, y: drag.y };
    drag = null;
    canvas.classList.remove('dragging');
    if (canvas.releasePointerCapture && ev.pointerId != null) {
      try { canvas.releasePointerCapture(ev.pointerId); } catch (e) { /* ignore */ }
    }
    if (wasDrag) return;

    var p = localXY(ev);
    var node = hitNode(p.x, p.y);
    if (node) {
      selectedId = node.id;
      hoverNodeId = node.id;
      if (app && app.helpers && typeof app.helpers.onSelectNode === 'function') {
        try { app.helpers.onSelectNode(nodeById.get(node.id) || node); } catch (e) { /* ignore */ }
      }
      pointerDirty = true;
      return;
    }
    var island = hitIsland(p.x, p.y);
    if (island) {
      zoomToIsland(island);
      return;
    }
    if (start) { /* empty-space clicks keep the current selection */ }
  }

  function onPointerLeave() {
    drag = null;
    if (canvas) canvas.classList.remove('dragging', 'pointing');
    hoverNodeId = null;
    hoverIslandId = null;
    hideTip();
    pointerDirty = true;
  }

  function onWheel(ev) {
    if (!W) return;
    ev.preventDefault();
    var p = localXY(ev);
    var world = toWorld(p.x, p.y);
    var factor = Math.exp(-ev.deltaY * 0.0016);
    var next = clampZoom(view.zoom * factor);
    if (next === view.zoom) return;
    view.zoom = next;
    // Keep the world point under the cursor anchored.
    view.cx = world.x - (p.x - W / 2) / view.zoom;
    view.cy = world.y - (p.y - H / 2) / view.zoom;
    target = null;
    pointerDirty = true;
  }

  function onDblClick(ev) {
    var p = localXY(ev);
    var node = hitNode(p.x, p.y);
    if (!node) return;
    if (app && app.helpers && typeof app.helpers.onFocusLocal === 'function') {
      try { app.helpers.onFocusLocal(node.id); } catch (e) { /* ignore */ }
    }
  }

  function zoomToIsland(island) {
    var s = boundsSpan();
    var zx = (W * 0.7) / Math.max(40, island.rx * 2);
    var zy = (H * 0.7) / Math.max(32, island.ry * 2);
    target = {
      cx: island.cx,
      cy: island.cy,
      zoom: clamp(Math.min(zx, zy), fitZoom, fitZoom * 12)
    };
    hideTip();
    pointerDirty = true;
  }

  /* ------------------------------------------------------------------ chrome */

  function renderLegend() {
    if (!legendEl) return;
    legendEl.innerHTML = '';
    var title = document.createElement('div');
    title.className = 'aikd-isl-title';
    title.textContent = 'Semantic Islands · ' + model.islands.length;
    legendEl.appendChild(title);
    model.islands.slice().sort(function (a, b) { return b.count - a.count; }).slice(0, 8)
      .forEach(function (is) {
        var row = document.createElement('div');
        row.className = 'aikd-isl-row';
        var dot = document.createElement('i');
        dot.className = 'aikd-isl-dot';
        dot.style.background = is.color;
        dot.style.boxShadow = '0 0 8px ' + rgba(is.color, 0.7);
        var name = document.createElement('span');
        name.className = 'aikd-isl-name';
        name.textContent = is.label;
        var count = document.createElement('span');
        count.className = 'aikd-isl-count';
        count.textContent = String(is.count);
        row.appendChild(dot);
        row.appendChild(name);
        row.appendChild(count);
        legendEl.appendChild(row);
      });
  }

  function resize() {
    if (!root || !canvas) return;
    var rect = root.getBoundingClientRect();
    W = Math.max(0, Math.round(rect.width));
    H = Math.max(0, Math.round(rect.height));
    dpr = Math.min(3, Math.max(1, window.devicePixelRatio || 1));
    canvas.width = Math.max(1, Math.round(W * dpr));
    canvas.height = Math.max(1, Math.round(H * dpr));
    if (!fitted) refit();
    pointerDirty = true;
  }

  /* ------------------------------------------------------------------- loop */

  function frame(now) {
    if (destroyed) return;
    raf = window.requestAnimationFrame(frame);
    if (target) {
      var ease = 0.2;
      view.cx += (target.cx - view.cx) * ease;
      view.cy += (target.cy - view.cy) * ease;
      view.zoom += (target.zoom - view.zoom) * ease;
      if (Math.abs(target.cx - view.cx) * view.zoom < 0.35
        && Math.abs(target.cy - view.cy) * view.zoom < 0.35
        && Math.abs(target.zoom - view.zoom) < 0.002) {
        view.cx = target.cx;
        view.cy = target.cy;
        view.zoom = target.zoom;
        target = null;
      }
      pointerDirty = true;
    }
    draw(now);
    pointerDirty = false;
  }

  /* --------------------------------------------------------------- lifecycle */

  function destroy() {
    destroyed = true;
    if (raf) { window.cancelAnimationFrame(raf); raf = 0; }
    if (ro) { try { ro.disconnect(); } catch (e) { /* ignore */ } ro = null; }
    if (usingWindowResize) {
      window.removeEventListener('resize', resize);
      usingWindowResize = false;
    }
    if (root) {
      root.remove();
      root = null;
    }
    canvas = null;
    g2d = null;
    tip = null;
    legendEl = null;
    hintEl = null;
    fitBtn = null;
    container = null;
    app = null;
    model = { islands: [], nodes: [], bounds: null, signature: '' };
    screenNodes = [];
    screenIslands = [];
    selectedId = null;
    hoverNodeId = null;
    hoverIslandId = null;
    focusSet = null;
    target = null;
    drag = null;
    W = 0;
    H = 0;
    fitZoom = 1;
    fitted = false;
    view = { cx: 0, cy: 0, zoom: 1 };
  }

  var viewApi = {
    mount: function (context) {
      if (root) destroy();
      destroyed = false;
      app = context || {};
      container = app.container;
      if (!container && typeof document !== 'undefined') {
        container = document.getElementById('graph-alt-stage') || document.getElementById('graph-stage');
      }
      if (!container) return viewApi;

      injectStyle();

      root = document.createElement('div');
      root.className = 'aikd-islands';

      canvas = document.createElement('canvas');
      canvas.className = 'aikd-islands-canvas';
      root.appendChild(canvas);

      legendEl = document.createElement('div');
      legendEl.className = 'aikd-islands-legend';
      root.appendChild(legendEl);

      fitBtn = document.createElement('button');
      fitBtn.type = 'button';
      fitBtn.className = 'aikd-islands-fit';
      fitBtn.textContent = '⛶ Fit';
      fitBtn.addEventListener('click', function (e) {
        e.preventDefault();
        e.stopPropagation();
        refit();
      });
      root.appendChild(fitBtn);

      hintEl = document.createElement('div');
      hintEl.className = 'aikd-islands-hint';
      hintEl.textContent = 'drag pan · wheel zoom · hover spotlight · click inspect · dbl-click local';
      root.appendChild(hintEl);

      tip = document.createElement('div');
      tip.className = 'aikd-islands-tip';
      root.appendChild(tip);

      container.appendChild(root);

      g2d = canvas.getContext('2d');
      if (!g2d) { destroy(); return viewApi; }

      canvas.addEventListener('pointerdown', onPointerDown);
      canvas.addEventListener('pointermove', onPointerMove);
      canvas.addEventListener('pointerup', onPointerUp);
      canvas.addEventListener('pointercancel', onPointerUp);
      canvas.addEventListener('pointerleave', onPointerLeave);
      canvas.addEventListener('wheel', onWheel, { passive: false });
      canvas.addEventListener('dblclick', onDblClick);

      selectedId = app.selectedId || null;
      setData(app);

      if (typeof ResizeObserver !== 'undefined') {
        ro = new ResizeObserver(function () { resize(); });
        ro.observe(root);
      } else {
        usingWindowResize = true;
        window.addEventListener('resize', resize);
      }
      resize();
      pointerDirty = true;
      raf = window.requestAnimationFrame(frame);
      return viewApi;
    },

    update: function (context) {
      if (!root) return viewApi.mount(context);
      app = context || app;
      selectedId = app.selectedId || null;
      setData(app);
      if (!W) resize();
      pointerDirty = true;
      return viewApi;
    },

    focusNode: function (nodeId) {
      if (!nodeId) return;
      var entry = null;
      for (var i = 0; i < model.nodes.length; i++) {
        if (model.nodes[i].node.id === nodeId) { entry = model.nodes[i]; break; }
      }
      if (!entry) return;
      selectedId = nodeId;
      target = {
        cx: entry.x,
        cy: entry.y,
        zoom: clampZoom(Math.max(view.zoom, fitZoom * 3.2))
      };
      pointerDirty = true;
    },

    destroy: destroy
  };

  G.views.islands = viewApi;
})(window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} });
