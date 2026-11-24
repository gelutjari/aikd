/* ============================================================================
 * AIKD Knowledge Graph - Swimlane Timeline View ('timeline')
 * ----------------------------------------------------------------------------
 * Horizontal swimlanes grouped by Project / Type / Agent, sequence or
 * timestamp ordered, with bezier causality arcs on hover/select.
 * Pure offline Vanilla JS, HiDPI canvas, focusNode() fly-to, destroy() cleanup.
 * Registers window.AIKD_GRAPH.views.timeline.
 * ========================================================================== */
(function (G) {
  'use strict';

  if (!G) return;
  G.views = G.views || {};

  var STYLE_ID = 'aikd-timeline-style';
  var AXIS_H = 34;
  var MAX_LANES = 18;
  var GROUP_LABELS = { project: 'Project', kind: 'Type', agent: 'Agent' };

  var KIND_COLORS = {
    project: '#fbbf24', file: '#38bdf8', symbol: '#60a5fa', note: '#34d399',
    memory: '#c084fc', tag: '#f472b6', agent: '#fb7185', decision: '#34d399',
    constraint: '#fbbf24', fact: '#38bdf8', task: '#60a5fa', prompt: '#c084fc',
    checkpoint: '#94a3b8', daily_log: '#22d3ee', node: '#94a3b8'
  };
  var LANE_PALETTE = [
    '#34d399', '#38bdf8', '#c084fc', '#fbbf24', '#f472b6', '#60a5fa',
    '#2dd4bf', '#fb923c', '#a3e635', '#e879f9', '#22d3ee', '#facc15'
  ];
  var EDGE_COLORS = {
    imports: '#38bdf8', contains: '#647892', wikilink: '#34d399',
    tag: '#f472b6', mentions: '#fbbf24', relates: '#c084fc',
    references: '#60a5fa', link: '#2b435f'
  };
  var UNASSIGNED = 'Unassigned / General';

  var app = null;
  var container = null;
  var root = null;
  var canvas = null;
  var g2d = null;
  var tip = null;
  var barEl = null;
  var statusEl = null;
  var ro = null;
  var raf = 0;
  var usingWindowResize = false;
  var destroyed = false;

  var nodes = [];
  var rawNodes = [];
  var rawEdges = [];
  var nodeById = new Map();
  var adj = new Map();
  var model = { lanes: [], hasTimestamps: false, signature: '' };
  var xSorted = [];
  var selectedId = null;
  var hoverId = null;
  var hoverLaneIndex = -1;
  var lastCtxSelected = null;
  var groupBy = 'project';

  var view = { zoom: 1, panX: 0 };
  var target = null;
  var drag = null;
  var lastPoints = new Map();
  var lastLayout = null;

  var dpr = 1;
  var W = 0;
  var H = 0;

  /* ------------------------------------------------------------------ utils */

  function injectStyle() {
    if (document.getElementById(STYLE_ID)) return;
    var style = document.createElement('style');
    style.id = STYLE_ID;
    style.textContent = [
      '.aikd-timeline{position:absolute;inset:0;display:flex;flex-direction:column;min-height:0;',
      'background:radial-gradient(900px 520px at 30% 0%,#0c1a2a 0%,#070c14 72%);',
      'font-family:"Segoe UI",system-ui,-apple-system,sans-serif;color:#e6eefc;}',
      '.aikd-timeline-bar{display:flex;align-items:center;gap:9px;flex-wrap:wrap;padding:8px 12px;flex:0 0 auto;',
      'border-bottom:1px solid var(--line,#1b2a3d);background:rgba(10,17,28,.72);backdrop-filter:blur(6px);}',
      '.aikd-timeline-title{font-size:12px;font-weight:700;color:#dbe8f7;}',
      '.aikd-timeline-title span{color:#34d399;}',
      '.aikd-timeline-group{display:inline-flex;gap:2px;padding:2px;border:1px solid var(--line,#1b2a3d);border-radius:8px;background:rgba(0,0,0,.25);}',
      '.aikd-timeline-group button{padding:3px 9px;border:1px solid transparent;border-radius:6px;background:transparent;',
      'color:#8397b0;font-size:10.5px;cursor:pointer;}',
      '.aikd-timeline-group button:hover{color:#eaf3ff;background:rgba(255,255,255,.05);}',
      '.aikd-timeline-group button.active{color:#a7f3d0;background:rgba(52,211,153,.13);border-color:rgba(52,211,153,.3);}',
      '.aikd-timeline-reset{padding:3px 9px;border:1px solid var(--line-2,#263a52);border-radius:6px;background:rgba(255,255,255,.03);',
      'color:#a9bdd4;font-size:10px;cursor:pointer;}',
      '.aikd-timeline-reset:hover{color:#eaf3ff;border-color:#38bdf8;background:rgba(56,189,248,.1);}',
      '.aikd-timeline-status{margin-left:auto;display:flex;align-items:center;gap:9px;font-size:10px;color:#647892;}',
      '.aikd-timeline-status .aikd-tl-pill{padding:2px 7px;border-radius:99px;border:1px solid rgba(52,211,153,.25);',
      'color:#a7f3d0;background:rgba(52,211,153,.08);font-family:ui-monospace,Consolas,monospace;font-size:9px;}',
      '.aikd-timeline-status .aikd-tl-pill.seq{border-color:rgba(148,163,184,.25);color:#94a3b8;background:rgba(148,163,184,.08);}',
      '.aikd-timeline-stage{position:relative;flex:1 1 auto;min-height:0;overflow:hidden;}',
      '.aikd-timeline-stage canvas{position:absolute;inset:0;width:100%;height:100%;display:block;cursor:crosshair;touch-action:none;}',
      '.aikd-timeline-stage canvas.dragging{cursor:grabbing;}',
      '.aikd-timeline-stage canvas.pointing{cursor:pointer;}',
      '.aikd-timeline-hint{position:absolute;right:12px;bottom:10px;font-size:9.5px;color:#647892;pointer-events:none;',
      'background:rgba(7,12,20,.7);border:1px solid rgba(38,58,82,.85);border-radius:6px;padding:3px 8px;}',
      '.aikd-timeline-tip{position:absolute;z-index:6;max-width:290px;padding:8px 10px;border:1px solid rgba(56,189,248,.36);',
      'border-radius:9px;background:rgba(6,11,19,.95);box-shadow:0 14px 34px rgba(0,0,0,.5);pointer-events:none;',
      'font-size:11px;line-height:1.45;color:#dbe8f7;opacity:0;transform:translateY(4px);transition:opacity .1s ease;}',
      '.aikd-timeline-tip.show{opacity:1;transform:translateY(0);}',
      '.aikd-timeline-tip .aikd-tip-kind{font-size:9px;letter-spacing:.08em;text-transform:uppercase;color:#647892;}',
      '.aikd-timeline-tip .aikd-tip-label{font-weight:600;font-size:12px;color:#f1f7ff;margin:2px 0 4px;word-break:break-word;}',
      '.aikd-timeline-tip .aikd-tip-meta{color:#8397b0;font-size:10.5px;}',
      '.aikd-timeline-tip .aikd-tip-hint{color:#4f6a88;font-size:9.5px;margin-top:5px;}'
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

  function num(v, fallback) {
    var n = Number(v);
    return isFinite(n) ? n : (fallback == null ? 0 : fallback);
  }

  function clamp(v, lo, hi) { return v < lo ? lo : (v > hi ? hi : v); }

  function kindColor(kind) {
    if (app && app.helpers && typeof app.helpers.kindColor === 'function') {
      try { return app.helpers.kindColor(kind); } catch (e) { /* fall through */ }
    }
    return KIND_COLORS[String(kind || '').toLowerCase()] || KIND_COLORS.node;
  }

  function labelOf(node) {
    if (!node) return '(unknown)';
    return String(node.label || node.id || '(unknown)');
  }

  function shortLabel(node) {
    if (app && app.helpers && typeof app.helpers.graphLabel === 'function') {
      try { return app.helpers.graphLabel(node) || labelOf(node); } catch (e) { /* fall through */ }
    }
    return labelOf(node);
  }

  function projectKey(node) {
    return (node && node.project) ? String(node.project) : UNASSIGNED;
  }

  function tsOf(node) {
    if (!node) return 0;
    var raw = node.updated_at || node.created_at || '';
    var t = Date.parse(String(raw).replace(' ', 'T'));
    return isFinite(t) ? t : 0;
  }

  function shortTs(ts) {
    if (!ts) return '';
    try {
      return new Date(ts).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
    } catch (e) { return ''; }
  }

  function laneColor(key) {
    return LANE_PALETTE[hashStr(key) % LANE_PALETTE.length];
  }

  function rgba(hex, alpha) {
    var h = String(hex || '').replace('#', '');
    if (h.length === 3) h = h.split('').map(function (c) { return c + c; }).join('');
    if (!/^[0-9a-f]{6}$/i.test(h)) return 'rgba(148,163,184,' + alpha + ')';
    var n = parseInt(h, 16);
    return 'rgba(' + ((n >> 16) & 255) + ',' + ((n >> 8) & 255) + ',' + (n & 255) + ',' + alpha + ')';
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

  /* ------------------------------------------------------------- data model */

  function edgeList() {
    return Array.isArray(app && app.edges) ? app.edges : [];
  }

  function buildAdjacency(edgeSet) {
    var map = new Map();
    edgeSet.forEach(function (e) {
      if (!e || !e.from || !e.to || e.from === e.to) return;
      if (!map.has(e.from)) map.set(e.from, new Set());
      if (!map.has(e.to)) map.set(e.to, new Set());
      map.get(e.from).add(e.to);
      map.get(e.to).add(e.from);
    });
    return map;
  }

  function groupKeyFor(node) {
    if (groupBy === 'kind') return String(node.kind || 'node');
    if (groupBy === 'agent') {
      return String(node.agent || (node.kind === 'agent' ? labelOf(node) : 'Agent / Unassigned'));
    }
    return projectKey(node);
  }

  function fallbackLanes() {
    var groups = new Map();
    nodes.forEach(function (n) {
      var key = groupKeyFor(n);
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(n);
    });

    var timed = [];
    groups.forEach(function (members) {
      members.forEach(function (n) { if (tsOf(n)) timed.push(n); });
    });
    var hasTs = timed.length > 0;
    var minTs = Infinity, maxTs = -Infinity;
    timed.forEach(function (n) {
      var t = tsOf(n);
      if (t < minTs) minTs = t;
      if (t > maxTs) maxTs = t;
    });

    var lanes = [];
    groups.forEach(function (members, key) {
      var sorted = members.slice().sort(function (a, b) {
        if (hasTs) {
          var d = tsOf(a) - tsOf(b);
          if (d) return d;
        }
        return labelOf(a).localeCompare(labelOf(b));
      });
      var items = sorted.map(function (n, i) {
        var xNorm = sorted.length > 1 ? i / (sorted.length - 1) : 0.5;
        var t = tsOf(n);
        if (hasTs && t && maxTs > minTs) xNorm = (t - minTs) / (maxTs - minTs);
        return {
          node: n,
          xNorm: xNorm,
          tsLabel: hasTs && t ? shortTs(t) : '#' + (i + 1),
          idx: i
        };
      });
      lanes.push({ id: key, label: key, color: laneColor(key), items: items });
    });

    lanes.sort(function (a, b) { return b.items.length - a.items.length; });
    return { lanes: lanes, hasTimestamps: hasTs };
  }

  function normalizeLanes(raw) {
    var lanes = [];
    var rawLanes = raw && Array.isArray(raw.lanes) ? raw.lanes : [];
    rawLanes.forEach(function (lane, li) {
      if (!lane) return;
      var items = [];
      var rawItems = Array.isArray(lane.items) ? lane.items : [];
      rawItems.forEach(function (it, i) {
        if (!it) return;
        var node = it.node ? it.node : (it.id ? it : null);
        if (!node || !node.id) return;
        var resolved = nodeById.get(node.id) || node;
        var xNorm = num(it.xNorm, NaN);
        if (!isFinite(xNorm)) xNorm = i;
        items.push({
          node: resolved,
          xNorm: xNorm,
          tsLabel: it.tsLabel != null ? String(it.tsLabel) : '',
          idx: num(it.idx, i)
        });
      });
      if (!items.length) return;
      items.sort(function (a, b) { return a.xNorm - b.xNorm; });
      var key = String(lane.id != null ? lane.id : lane.label || ('lane-' + li));
      var color = typeof lane.color === 'string' && /^#[0-9a-f]{3,8}$/i.test(lane.color)
        ? lane.color : laneColor(key);
      lanes.push({
        id: key,
        label: String(lane.label || key),
        color: color,
        items: items
      });
    });

    // Normalize xNorm if the adapter returned indices instead of 0..1.
    var maxX = 0;
    lanes.forEach(function (lane) {
      lane.items.forEach(function (it) { if (it.xNorm > maxX) maxX = it.xNorm; });
    });
    if (maxX > 1.001) {
      lanes.forEach(function (lane) {
        lane.items.forEach(function (it) { it.xNorm = clamp(it.xNorm / maxX, 0, 1); });
      });
    }

    // Keep the biggest lanes when a workspace has an unreasonable number.
    if (lanes.length > MAX_LANES) {
      var keep = lanes.slice().sort(function (a, b) { return b.items.length - a.items.length; }).slice(0, MAX_LANES - 1);
      var keepIds = new Set(keep.map(function (l) { return l.id; }));
      var restItems = [];
      lanes.forEach(function (lane) {
        if (keepIds.has(lane.id)) return;
        lane.items.forEach(function (it) { restItems.push(it); });
      });
      if (restItems.length) {
        restItems.sort(function (a, b) { return a.xNorm - b.xNorm; });
        keep.push({
          id: '__other',
          label: 'Other lanes',
          color: '#8397b0',
          items: restItems
        });
      }
      lanes = keep.sort(function (a, b) { return b.items.length - a.items.length; });
    }

    var hasTs = raw && typeof raw.hasTimestamps === 'boolean' ? raw.hasTimestamps
      : lanes.some(function (lane) {
        return lane.items.some(function (it) { return /\d{4}|\d{1,2}:\d{2}|[A-Za-z]{3}\s+\d/.test(it.tsLabel); });
      });

    return { lanes: lanes, hasTimestamps: hasTs };
  }

  function buildModel() {
    var raw = null;
    var adapter = G.adapter;
    if (adapter && typeof adapter.toTimelineLanes === 'function') {
      try { raw = adapter.toTimelineLanes(nodes, edgeList(), groupBy); } catch (e) { raw = null; }
    }
    if (!raw || !Array.isArray(raw.lanes)) raw = fallbackLanes();
    model = normalizeLanes(raw);
    model.signature = groupBy + '|' + model.lanes.map(function (l) { return l.id + ':' + l.items.length; }).join('|');

    xSorted = [];
    model.lanes.forEach(function (lane) {
      lane.items.forEach(function (it) {
        xSorted.push({ xNorm: it.xNorm, item: it, lane: lane });
      });
    });
    xSorted.sort(function (a, b) { return a.xNorm - b.xNorm; });
    updateStatus();
  }

  function setData(context) {
    nodes = Array.isArray(context.nodes) ? context.nodes : [];
    rawNodes = Array.isArray(context.rawNodes) && context.rawNodes.length ? context.rawNodes : nodes.slice();
    rawEdges = Array.isArray(context.rawEdges) && context.rawEdges.length
      ? context.rawEdges
      : (Array.isArray(context.edges) ? context.edges : []);

    nodeById = new Map();
    rawNodes.forEach(function (n) { if (n && n.id != null) nodeById.set(n.id, n); });
    nodes.forEach(function (n) { if (n && n.id != null) nodeById.set(n.id, n); });
    adj = buildAdjacency(rawEdges.length ? rawEdges : edgeList());
    buildModel();
  }

  /* ----------------------------------------------------------------- layout */

  function computeLayout() {
    var gutter = clamp(W * 0.2, 118, 190);
    var contentW = Math.max(60, W - gutter - 22);
    var availH = Math.max(30, H - AXIS_H - 10);
    var laneCount = Math.max(1, model.lanes.length);
    var laneH = clamp(availH / laneCount, 16, 200);
    var totalH = laneH * model.lanes.length;
    var top = AXIS_H + Math.max(0, (availH - totalH) / 2);
    return { gutter: gutter, contentW: contentW, availH: availH, laneH: laneH, top: top };
  }

  function xOf(xNorm, L) {
    return L.gutter + xNorm * L.contentW * view.zoom + view.panX;
  }

  function clampPan(pan, L, zoom) {
    var min = -(L.contentW * (zoom - 1)) - 30;
    var max = 30;
    return clamp(pan, Math.min(min, 0), Math.max(max, 0));
  }

  function computePoints(L) {
    var points = new Map();
    lastPoints = points;
    lastLayout = L;
    model.lanes.forEach(function (lane, li) {
      var laneTop = L.top + li * L.laneH;
      var slots = clamp(Math.floor((L.laneH - 8) / 11), 1, 4);
      var lastX = [];
      for (var s = 0; s < slots; s++) lastX.push(-1e9);
      var yStep = slots > 1 ? Math.min(11, (L.laneH - 12) / (slots - 1)) : 0;

      lane.items.forEach(function (item) {
        var x = xOf(item.xNorm, L);
        var slot = 0;
        while (slot < slots - 1 && x - lastX[slot] < 16) slot++;
        lastX[slot] = x;
        var y = laneTop + L.laneH / 2 + (slot - (slots - 1) / 2) * yStep;
        points.set(item.node.id, {
          x: x, y: y, laneIndex: li, lane: lane, item: item
        });
      });
    });
    return points;
  }

  /* ------------------------------------------------------------------- draw */

  function drawBackground(g) {
    var bg = g.createRadialGradient(W * 0.35, 0, 0, W * 0.35, 0, Math.max(W, H) * 0.75);
    bg.addColorStop(0, 'rgba(56,189,248,0.055)');
    bg.addColorStop(0.5, 'rgba(52,211,153,0.02)');
    bg.addColorStop(1, 'rgba(0,0,0,0)');
    g.fillStyle = bg;
    g.fillRect(0, 0, W, H);
  }

  function drawLanes(g, L) {
    model.lanes.forEach(function (lane, li) {
      var laneTop = L.top + li * L.laneH;
      var hovered = hoverLaneIndex === li;
      g.fillStyle = hovered ? 'rgba(56,189,248,0.07)'
        : (li % 2 ? 'rgba(255,255,255,0.018)' : 'rgba(255,255,255,0.038)');
      g.fillRect(L.gutter, laneTop, W - L.gutter, L.laneH);

      g.strokeStyle = 'rgba(27,42,61,0.7)';
      g.lineWidth = 1;
      g.beginPath();
      g.moveTo(L.gutter, laneTop);
      g.lineTo(W, laneTop);
      g.stroke();

      // Gutter band.
      g.fillStyle = hovered ? 'rgba(56,189,248,0.08)' : 'rgba(8,13,22,0.6)';
      g.fillRect(0, laneTop, L.gutter, L.laneH);
      g.strokeStyle = 'rgba(27,42,61,0.85)';
      g.beginPath();
      g.moveTo(L.gutter, laneTop);
      g.lineTo(L.gutter, laneTop + L.laneH);
      g.stroke();

      // Lane label.
      if (L.laneH >= 14) {
        g.fillStyle = lane.color;
        g.beginPath();
        g.arc(12, laneTop + L.laneH / 2, 3, 0, Math.PI * 2);
        g.fill();
        g.save();
        g.shadowColor = rgba(lane.color, 0.6);
        g.shadowBlur = 8;
        g.beginPath();
        g.arc(12, laneTop + L.laneH / 2, 3, 0, Math.PI * 2);
        g.fill();
        g.restore();

        g.font = (L.laneH >= 26 ? '600 10.5px' : '9.5px') + ' "Segoe UI", system-ui, sans-serif';
        g.fillStyle = '#c3d4e7';
        g.textBaseline = 'middle';
        var maxLabelW = Math.max(30, L.gutter - 58);
        var label = fitText(g, lane.label, maxLabelW);
        g.fillText(label, 21, laneTop + L.laneH / 2);
        if (L.laneH >= 26) {
          g.font = '9px ui-monospace, Consolas, monospace';
          g.fillStyle = '#647892';
          g.textAlign = 'right';
          g.fillText(String(lane.items.length), L.gutter - 8, laneTop + L.laneH / 2);
          g.textAlign = 'start';
        }
        g.textBaseline = 'alphabetic';
      }
    });

    // Bottom border of last lane.
    var bottom = L.top + model.lanes.length * L.laneH;
    g.strokeStyle = 'rgba(27,42,61,0.8)';
    g.beginPath();
    g.moveTo(0, bottom);
    g.lineTo(W, bottom);
    g.stroke();
  }

  function nearestXLabel(xNorm) {
    if (!xSorted.length) return '';
    var lo = 0;
    var hi = xSorted.length - 1;
    while (lo < hi) {
      var mid = (lo + hi) >> 1;
      if (xSorted[mid].xNorm < xNorm) lo = mid + 1; else hi = mid;
    }
    var candidates = [{ entry: xSorted[lo], index: lo }];
    if (lo > 0) candidates.push({ entry: xSorted[lo - 1], index: lo - 1 });
    var best = candidates[0];
    candidates.forEach(function (c) {
      if (!c.entry) return;
      if (Math.abs(c.entry.xNorm - xNorm) < Math.abs(best.entry.xNorm - xNorm)) best = c;
    });
    if (model.hasTimestamps && best.entry.item.tsLabel) return best.entry.item.tsLabel;
    return '#' + (best.index + 1);
  }

  function drawAxis(g, L) {
    g.save();
    g.strokeStyle = 'rgba(27,42,61,0.9)';
    g.lineWidth = 1;
    g.beginPath();
    g.moveTo(0, AXIS_H);
    g.lineTo(W, AXIS_H);
    g.stroke();

    g.font = '9.5px "Segoe UI", system-ui, sans-serif';
    g.fillStyle = '#647892';
    g.textBaseline = 'middle';
    g.fillText(model.hasTimestamps ? 'Time' : 'Sequence', 10, 16);

    var startX = L.gutter + 26;
    var step = 118;
    for (var sx = startX; sx < W - 12; sx += step) {
      var xNorm = (sx - L.gutter - view.panX) / (L.contentW * view.zoom);
      if (xNorm < -0.02 || xNorm > 1.02) continue;
      g.strokeStyle = 'rgba(38,58,82,0.5)';
      g.setLineDash([3, 5]);
      g.beginPath();
      g.moveTo(sx, AXIS_H);
      g.lineTo(sx, Math.max(AXIS_H, H - 8));
      g.stroke();
      g.setLineDash([]);
      g.fillStyle = 'rgba(100,120,146,0.9)';
      g.fillText(nearestXLabel(clamp(xNorm, 0, 1)), sx + 4, 16);
    }
    g.restore();
  }

  function bezierPath(g, p1, p2) {
    g.beginPath();
    g.moveTo(p1.x, p1.y);
    if (Math.abs(p2.y - p1.y) < 2) {
      var lift = Math.min(74, 26 + Math.abs(p2.x - p1.x) * 0.14);
      g.bezierCurveTo(p1.x, p1.y - lift, p2.x, p2.y - lift, p2.x, p2.y);
      return { cp2x: p2.x, cp2y: p2.y - lift };
    }
    var dx = Math.max(18, Math.abs(p2.x - p1.x) * 0.4);
    g.bezierCurveTo(p1.x + dx, p1.y, p2.x - dx, p2.y, p2.x, p2.y);
    return { cp2x: p2.x - dx, cp2y: p2.y };
  }

  function drawArrow(g, cp2x, cp2y, p2, color) {
    var a = Math.atan2(p2.y - cp2y, p2.x - cp2x);
    var size = 5;
    g.beginPath();
    g.moveTo(p2.x, p2.y);
    g.lineTo(p2.x - Math.cos(a - 0.45) * size, p2.y - Math.sin(a - 0.45) * size);
    g.lineTo(p2.x - Math.cos(a + 0.45) * size, p2.y - Math.sin(a + 0.45) * size);
    g.closePath();
    g.fillStyle = color;
    g.fill();
  }

  function drawArcs(g, points, focusId) {
    if (!focusId) return;
    var focusSet = new Set([focusId]);
    var links = [];
    rawEdges.forEach(function (edge) {
      if (!edge || !edge.from || !edge.to) return;
      if (edge.from !== focusId && edge.to !== focusId) return;
      var p1 = points.get(edge.from);
      var p2 = points.get(edge.to);
      if (!p1 || !p2) return;
      links.push({ edge: edge, p1: p1, p2: p2 });
    });
    if (!links.length) return;

    g.save();
    links.forEach(function (link) {
      var color = EDGE_COLORS[link.edge.kind] || '#38bdf8';
      focusSet.add(link.edge.from);
      focusSet.add(link.edge.to);
      g.strokeStyle = rgba(color, 0.62);
      g.fillStyle = rgba(color, 0.7);
      g.lineWidth = 1.35;
      g.shadowColor = rgba(color, 0.45);
      g.shadowBlur = 9;
      var cp = bezierPath(g, link.p1, link.p2);
      g.stroke();
      g.shadowBlur = 0;
      drawArrow(g, cp.cp2x, cp.cp2y, link.p2, rgba(color, 0.8));
    });
    g.restore();

    // Rings on connected nodes.
    links.forEach(function (link) {
      [link.p1, link.p2].forEach(function (p) {
        g.strokeStyle = 'rgba(56,189,248,0.35)';
        g.lineWidth = 1;
        g.beginPath();
        g.arc(p.x, p.y, 7.5, 0, Math.PI * 2);
        g.stroke();
      });
    });
    return focusSet;
  }

  function draw(g, now) {
    if (!g2d || !W || !H) return;
    drawBackground(g);

    if (!model.lanes.length) {
      g.fillStyle = 'rgba(131,151,176,0.75)';
      g.font = '600 13px "Segoe UI", system-ui, sans-serif';
      g.textAlign = 'center';
      g.fillText('No nodes for the timeline yet', W / 2, H / 2 - 8);
      g.fillStyle = 'rgba(100,120,146,0.65)';
      g.font = '11px "Segoe UI", system-ui, sans-serif';
      g.fillText('Scan a workspace or widen the type filters.', W / 2, H / 2 + 14);
      g.textAlign = 'start';
      return;
    }

    var L = computeLayout();
    drawLanes(g, L);
    drawAxis(g, L);
    var points = computePoints(L);

    var focusId = hoverId || selectedId;
    var focusSet = null;
    if (focusId) {
      var res = drawArcs(g, points, focusId);
      if (res) focusSet = res;
      if (adj.has(focusId)) {
        if (!focusSet) focusSet = new Set([focusId]);
        adj.get(focusId).forEach(function (id) { focusSet.add(id); });
      }
    }

    var pulse = 0.5 + 0.5 * Math.sin(now / 420);

    // Items.
    model.lanes.forEach(function (lane, li) {
      lane.items.forEach(function (item) {
        var p = points.get(item.node.id);
        if (!p) return;
        var n = item.node;
        var color = kindColor(n.kind);
        var dim = focusSet ? !focusSet.has(n.id) : false;
        var r = clamp(2.6 + Math.sqrt(num(n.degree)) * 0.62, 2.8, 8);
        var isSel = n.id === selectedId;
        var isHover = n.id === hoverId;
        if (p.x < L.gutter - 6 || p.x > W + 12) return;

        g.save();
        g.globalAlpha = dim ? 0.25 : 0.96;
        if (isSel || isHover) {
          g.shadowColor = rgba(color, 0.9);
          g.shadowBlur = isSel ? 14 + pulse * 10 : 10;
        }
        g.fillStyle = color;
        g.beginPath();
        g.arc(p.x, p.y, r, 0, Math.PI * 2);
        g.fill();
        g.shadowBlur = 0;
        if (isSel) {
          g.strokeStyle = '#ffffff';
          g.lineWidth = 1.5;
          g.beginPath();
          g.arc(p.x, p.y, r + 3 + pulse * 2, 0, Math.PI * 2);
          g.stroke();
        } else if (isHover) {
          g.strokeStyle = rgba(color, 0.95);
          g.lineWidth = 1.2;
          g.beginPath();
          g.arc(p.x, p.y, r + 2.2, 0, Math.PI * 2);
          g.stroke();
        }
        g.restore();
      });
    });

    // Callout for the focused item.
    var calloutId = hoverId || selectedId;
    if (calloutId) {
      var cp = points.get(calloutId);
      var calloutNode = nodeById.get(calloutId);
      if (cp && calloutNode) {
        g.save();
        g.font = '600 11px "Segoe UI", system-ui, sans-serif';
        var text = fitText(g, shortLabel(calloutNode), 230);
        var tw = g.measureText(text).width;
        var sub = '';
        if (model.hasTimestamps) {
          var item = null;
          model.lanes.forEach(function (lane) {
            lane.items.forEach(function (it) { if (it.node.id === calloutId) item = it; });
          });
          if (item && item.tsLabel) sub = item.tsLabel;
        }
        var boxW = Math.max(tw, sub ? g.measureText(sub).width : 0) + 18;
        var boxH = sub ? 36 : 22;
        var bx = clamp(cp.x + 12, 8, Math.max(8, W - boxW - 8));
        var by = clamp(cp.y - boxH - 8, AXIS_H + 4, H - boxH - 6);
        roundRectPath(g, bx, by, boxW, boxH, 7);
        g.fillStyle = 'rgba(6,11,19,0.94)';
        g.fill();
        g.strokeStyle = rgba(kindColor(calloutNode.kind), 0.5);
        g.lineWidth = 1;
        g.stroke();
        g.fillStyle = '#eaf3ff';
        g.textBaseline = 'middle';
        g.fillText(text, bx + 9, by + (sub ? 12 : 11));
        if (sub) {
          g.font = '9.5px "Segoe UI", system-ui, sans-serif';
          g.fillStyle = 'rgba(131,151,176,0.9)';
          g.fillText(sub, bx + 9, by + 25);
        }
        g.restore();
        g.textBaseline = 'alphabetic';
      }
    }
  }

  /* ------------------------------------------------------------- interaction */

  function showTip(node, mx, my) {
    if (!tip) return;
    var laneLabel = '';
    var item = null;
    model.lanes.forEach(function (lane) {
      lane.items.forEach(function (it) {
        if (it.node.id === node.id) { laneLabel = lane.label; item = it; }
      });
    });
    var degree = num(node.degree);
    var neighbors = adj.has(node.id) ? adj.get(node.id).size : 0;
    var meta = (node.kind || 'node') + ' · ' + (node.project || UNASSIGNED) + ' · degree ' + degree;
    tip.innerHTML = ''
      + '<div class="aikd-tip-kind">' + esc(laneLabel || 'timeline') + '</div>'
      + '<div class="aikd-tip-label">' + esc(labelOf(node)) + '</div>'
      + '<div class="aikd-tip-meta">' + esc(meta) + '</div>'
      + (item && item.tsLabel ? '<div class="aikd-tip-meta">' + esc(item.tsLabel) + '</div>' : '')
      + '<div class="aikd-tip-meta">' + neighbors + ' causal neighbor' + (neighbors === 1 ? '' : 's') + '</div>'
      + '<div class="aikd-tip-hint">click inspect · double-click focus local</div>';
    tip.classList.add('show');
    positionTip(mx, my);
  }

  function showLaneTip(lane, mx, my) {
    if (!tip) return;
    var kinds = {};
    lane.items.forEach(function (it) { kinds[it.node.kind] = (kinds[it.node.kind] || 0) + 1; });
    var parts = Object.keys(kinds).sort(function (a, b) { return kinds[b] - kinds[a]; }).slice(0, 4)
      .map(function (k) { return k + ' ' + kinds[k]; }).join(' · ');
    tip.innerHTML = ''
      + '<div class="aikd-tip-kind">swimlane</div>'
      + '<div class="aikd-tip-label">' + esc(lane.label) + '</div>'
      + '<div class="aikd-tip-meta">' + lane.items.length + ' items · ' + esc(parts) + '</div>';
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

  function hitPoint(mx, my) {
    var best = null;
    var bestD = Infinity;
    lastPoints.forEach(function (p) {
      var dx = p.x - mx;
      var dy = p.y - my;
      var d = dx * dx + dy * dy;
      if (d <= 9 * 9 && d < bestD) { best = p; bestD = d; }
    });
    return best;
  }

  function laneAt(mx, my) {
    if (!lastLayout || mx > lastLayout.gutter) return -1;
    var idx = Math.floor((my - lastLayout.top) / Math.max(1, lastLayout.laneH));
    if (idx < 0 || idx >= model.lanes.length) return -1;
    return idx;
  }

  function onPointerDown(ev) {
    if (ev.button !== undefined && ev.button !== 0) return;
    var p = localXY(ev);
    drag = { x: p.x, pan: view.panX, moved: false };
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
      if (Math.abs(dx) > 4) drag.moved = true;
      if (drag.moved) {
        var L = computeLayout();
        view.panX = clampPan(drag.pan + dx, L, view.zoom);
        target = null;
        hideTip();
      }
      return;
    }

    var hit = hitPoint(p.x, p.y);
    if (hit) {
      hoverId = hit.item.node.id;
      hoverLaneIndex = hit.laneIndex;
      canvas.classList.add('pointing');
      var full = nodeById.get(hoverId) || hit.item.node;
      showTip(full, p.x, p.y);
      return;
    }
    var li = laneAt(p.x, p.y);
    if (li >= 0) {
      hoverId = null;
      if (hoverLaneIndex !== li) {
        hoverLaneIndex = li;
        showLaneTip(model.lanes[li], p.x, p.y);
      } else {
        positionTip(p.x, p.y);
      }
      canvas.classList.remove('pointing');
      return;
    }
    hoverId = null;
    hoverLaneIndex = -1;
    canvas.classList.remove('pointing');
    hideTip();
  }

  function onPointerUp(ev) {
    if (!drag) return;
    var moved = drag.moved;
    drag = null;
    canvas.classList.remove('dragging');
    if (canvas.releasePointerCapture && ev.pointerId != null) {
      try { canvas.releasePointerCapture(ev.pointerId); } catch (e) { /* ignore */ }
    }
    if (moved) return;
    var p = localXY(ev);
    var hit = hitPoint(p.x, p.y);
    if (!hit) return;
    var node = nodeById.get(hit.item.node.id) || hit.item.node;
    selectedId = node.id;
    hoverId = node.id;
    if (app && app.helpers && typeof app.helpers.onSelectNode === 'function') {
      try { app.helpers.onSelectNode(node); } catch (e) { /* ignore */ }
    }
  }

  function onPointerLeave() {
    drag = null;
    if (canvas) canvas.classList.remove('dragging', 'pointing');
    hoverId = null;
    hoverLaneIndex = -1;
    hideTip();
  }

  function onWheel(ev) {
    ev.preventDefault();
    if (!W) return;
    var L = computeLayout();
    var p = localXY(ev);
    if (Math.abs(ev.deltaX) > Math.abs(ev.deltaY)) {
      view.panX = clampPan(view.panX - ev.deltaX, L, view.zoom);
      target = null;
      return;
    }
    var anchorNorm = (p.x - L.gutter - view.panX) / Math.max(1, L.contentW * view.zoom);
    var nextZoom = clamp(view.zoom * Math.exp(-ev.deltaY * 0.0014), 1, 12);
    if (nextZoom === view.zoom) return;
    var pan = p.x - L.gutter - anchorNorm * L.contentW * nextZoom;
    view.zoom = nextZoom;
    view.panX = clampPan(pan, L, nextZoom);
    target = null;
  }

  function onDblClick(ev) {
    var p = localXY(ev);
    var hit = hitPoint(p.x, p.y);
    if (!hit) return;
    if (app && app.helpers && typeof app.helpers.onFocusLocal === 'function') {
      try { app.helpers.onFocusLocal(hit.item.node.id); } catch (e) { /* ignore */ }
    }
  }

  /* ----------------------------------------------------------------- chrome */

  function updateStatus() {
    if (!statusEl) return;
    statusEl.innerHTML = '';
    var pill = document.createElement('span');
    pill.className = 'aikd-tl-pill' + (model.hasTimestamps ? '' : ' seq');
    pill.textContent = model.hasTimestamps ? 'timestamps' : 'sequence order';
    statusEl.appendChild(pill);
    var counts = document.createElement('span');
    counts.textContent = model.lanes.length + ' lanes · ' + nodes.length + ' nodes';
    statusEl.appendChild(counts);
  }

  function syncGroupButtons() {
    if (!barEl) return;
    var buttons = barEl.querySelectorAll('.aikd-timeline-group button');
    for (var i = 0; i < buttons.length; i++) {
      buttons[i].classList.toggle('active', buttons[i].dataset.group === groupBy);
    }
  }

  function buildBar() {
    barEl = document.createElement('div');
    barEl.className = 'aikd-timeline-bar';

    var title = document.createElement('div');
    title.className = 'aikd-timeline-title';
    title.innerHTML = 'Swimlane Timeline <span>⏱</span>';
    barEl.appendChild(title);

    var group = document.createElement('div');
    group.className = 'aikd-timeline-group';
    ['project', 'kind', 'agent'].forEach(function (key) {
      var btn = document.createElement('button');
      btn.type = 'button';
      btn.dataset.group = key;
      btn.textContent = GROUP_LABELS[key];
      btn.addEventListener('click', function () {
        if (groupBy === key) return;
        groupBy = key;
        syncGroupButtons();
        hoverId = null;
        hoverLaneIndex = -1;
        setData(app || {});
        resetView();
      });
      group.appendChild(btn);
    });
    barEl.appendChild(group);

    var reset = document.createElement('button');
    reset.type = 'button';
    reset.className = 'aikd-timeline-reset';
    reset.textContent = '⛶ Fit';
    reset.addEventListener('click', function () { resetView(); });
    barEl.appendChild(reset);

    statusEl = document.createElement('div');
    statusEl.className = 'aikd-timeline-status';
    barEl.appendChild(statusEl);

    root.appendChild(barEl);
    syncGroupButtons();
  }

  function resetView() {
    view = { zoom: 1, panX: 0 };
    target = null;
  }

  function resize() {
    if (!root || !canvas) return;
    var rect = root.getBoundingClientRect();
    W = Math.max(0, Math.round(rect.width));
    H = Math.max(0, Math.round(rect.height));
    dpr = Math.min(3, Math.max(1, window.devicePixelRatio || 1));
    canvas.width = Math.max(1, Math.round(W * dpr));
    canvas.height = Math.max(1, Math.round(H * dpr));
    var L = computeLayout();
    view.panX = clampPan(view.panX, L, view.zoom);
  }

  /* ------------------------------------------------------------------- loop */

  function frame(now) {
    if (destroyed) return;
    raf = window.requestAnimationFrame(frame);
    if (target) {
      var ease = 0.2;
      view.zoom += (target.zoom - view.zoom) * ease;
      view.panX += (target.panX - view.panX) * ease;
      if (Math.abs(target.zoom - view.zoom) < 0.004 && Math.abs(target.panX - view.panX) < 0.5) {
        view.zoom = target.zoom;
        view.panX = target.panX;
        target = null;
      }
    }
    if (!g2d) return;
    g2d.setTransform(dpr, 0, 0, dpr, 0, 0);
    g2d.clearRect(0, 0, W, H);
    draw(g2d, now);
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
    barEl = null;
    statusEl = null;
    container = null;
    app = null;
    nodes = [];
    rawNodes = [];
    rawEdges = [];
    nodeById = new Map();
    adj = new Map();
    model = { lanes: [], hasTimestamps: false, signature: '' };
    xSorted = [];
    selectedId = null;
    hoverId = null;
    hoverLaneIndex = -1;
    lastCtxSelected = null;
    target = null;
    drag = null;
    lastPoints = new Map();
    lastLayout = null;
    W = 0;
    H = 0;
    view = { zoom: 1, panX: 0 };
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
      root.className = 'aikd-timeline';
      buildBar();

      var stage = document.createElement('div');
      stage.className = 'aikd-timeline-stage';
      canvas = document.createElement('canvas');
      stage.appendChild(canvas);
      var hint = document.createElement('div');
      hint.className = 'aikd-timeline-hint';
      hint.textContent = 'wheel zoom · drag pan · hover for causality arcs';
      stage.appendChild(hint);
      tip = document.createElement('div');
      tip.className = 'aikd-timeline-tip';
      stage.appendChild(tip);
      root.appendChild(stage);
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
      lastCtxSelected = selectedId;
      setData(app);
      syncGroupButtons();

      if (typeof ResizeObserver !== 'undefined') {
        ro = new ResizeObserver(function () { resize(); });
        ro.observe(root);
      } else {
        usingWindowResize = true;
        window.addEventListener('resize', resize);
      }
      resize();
      raf = window.requestAnimationFrame(frame);
      return viewApi;
    },

    update: function (context) {
      if (!root) return viewApi.mount(context);
      app = context || app;
      selectedId = app.selectedId || null;
      lastCtxSelected = selectedId;
      setData(app);
      hoverId = null;
      hoverLaneIndex = -1;
      hideTip();
      var L = computeLayout();
      view.panX = clampPan(view.panX, L, view.zoom);
      return viewApi;
    },

    focusNode: function (nodeId) {
      if (!nodeId) return;
      var found = null;
      model.lanes.forEach(function (lane) {
        lane.items.forEach(function (it) {
          if (it.node.id === nodeId) found = { lane: lane, item: it };
        });
      });
      if (!found) return;
      selectedId = nodeId;
      var L = computeLayout();
      var z = clamp(Math.max(view.zoom, 3), 1, 12);
      var desired = L.gutter + L.contentW * 0.44;
      var pan = desired - L.gutter - found.item.xNorm * L.contentW * z;
      target = { zoom: z, panX: clampPan(pan, L, z) };
    },

    destroy: destroy
  };

  G.views.timeline = viewApi;
})(window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} });
