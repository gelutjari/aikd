/* ============================================================================
 * AIKD Knowledge Graph - Miller Columns View ('miller')
 * ----------------------------------------------------------------------------
 * Finder / Notion style 4-column browser:
 *   Projects -> Types -> Filtered & sortable items -> Preview + Backlinks
 *   + interactive Depth-1 mini local graph.
 * Pure offline Vanilla JS. Registers window.AIKD_GRAPH.views.miller.
 * ========================================================================== */
(function (G) {
  'use strict';

  if (!G) return;
  G.views = G.views || {};

  var STYLE_ID = 'aikd-miller-style';
  var ITEM_LIMIT = 400;
  var MINI_HEIGHT = 150;
  var MINI_MAX_NEIGHBORS = 16;

  var KIND_COLORS = {
    project: '#fbbf24', file: '#38bdf8', symbol: '#60a5fa', note: '#34d399',
    memory: '#c084fc', tag: '#f472b6', agent: '#fb7185', decision: '#34d399',
    constraint: '#fbbf24', fact: '#38bdf8', task: '#60a5fa', prompt: '#c084fc',
    checkpoint: '#94a3b8', daily_log: '#22d3ee', node: '#94a3b8'
  };
  var PROJECT_PALETTE = [
    '#34d399', '#38bdf8', '#c084fc', '#fbbf24', '#f472b6',
    '#60a5fa', '#2dd4bf', '#fb923c', '#a3e635', '#e879f9'
  ];
  var TYPE_ORDER = ['project', 'file', 'symbol', 'note', 'memory', 'tag', 'agent'];
  var UNASSIGNED = 'Unassigned / General';

  var app = null;
  var container = null;
  var root = null;
  var barEl = null;
  var colProjects = null;
  var colTypes = null;
  var colItems = null;
  var colPreview = null;
  var listProjects = null;
  var listTypes = null;
  var listItems = null;
  var previewBody = null;
  var ro = null;
  var miniRaf = 0;
  var resizeRaf = 0;
  var usingWindowResize = false;

  var nodes = [];          // filtered nodes (context)
  var rawNodes = [];       // unfiltered
  var rawEdges = [];
  var nodeById = new Map();
  var projectGroups = [];
  var selectedId = null;
  var previewId = null;
  var lastCtxSelected = null;
  var project = null;
  var kind = null;
  var sortMode = 'degree';

  /* ------------------------------------------------------------------ utils */

  function injectStyle() {
    if (document.getElementById(STYLE_ID)) return;
    var style = document.createElement('style');
    style.id = STYLE_ID;
    style.textContent = [
      '.aikd-miller{position:absolute;inset:0;display:flex;flex-direction:column;min-height:0;',
      'background:radial-gradient(900px 520px at 18% 0%,#0d1725 0%,#070c14 72%);',
      'font-family:"Segoe UI",system-ui,-apple-system,sans-serif;color:#e6eefc;}',
      '.aikd-miller-bar{display:flex;align-items:center;gap:10px;flex-wrap:wrap;padding:8px 12px;border-bottom:1px solid var(--line,#1b2a3d);',
      'background:rgba(10,17,28,.72);backdrop-filter:blur(6px);flex:0 0 auto;}',
      '.aikd-miller-title{font-size:12px;font-weight:700;letter-spacing:.02em;color:#dbe8f7;}',
      '.aikd-miller-title span{color:#34d399;}',
      '.aikd-miller-bar .aikd-miller-hint{font-size:10px;color:#647892;}',
      '.aikd-miller-bar .aikd-miller-count{margin-left:auto;font-family:ui-monospace,Consolas,monospace;font-size:10px;color:#647892;}',
      '.aikd-miller-cols{flex:1 1 auto;display:grid;grid-template-columns:minmax(142px,.85fr) minmax(126px,.72fr) minmax(220px,1.45fr) minmax(270px,1.65fr);',
      'min-height:0;overflow:hidden;}',
      '.aikd-miller-col{display:flex;flex-direction:column;min-height:0;min-width:0;border-right:1px solid var(--line,#1b2a3d);}',
      '.aikd-miller-col:last-child{border-right:0;}',
      '.aikd-miller-head{display:flex;align-items:center;gap:6px;flex:0 0 auto;padding:7px 9px;border-bottom:1px solid rgba(27,42,61,.85);',
      'font-size:9px;letter-spacing:.1em;text-transform:uppercase;color:#647892;background:rgba(13,21,34,.6);}',
      '.aikd-miller-head .aikd-h-count{font-family:ui-monospace,Consolas,monospace;font-size:9px;color:#8397b0;letter-spacing:0;text-transform:none;}',
      '.aikd-miller-sort{margin-left:auto;display:inline-flex;gap:2px;}',
      '.aikd-miller-sort button{padding:2px 6px;border:1px solid transparent;border-radius:5px;background:transparent;color:#647892;',
      'font-size:9px;cursor:pointer;text-transform:none;letter-spacing:0;}',
      '.aikd-miller-sort button:hover{color:#dbe8f7;background:rgba(255,255,255,.05);}',
      '.aikd-miller-sort button.active{color:#a7f3d0;background:rgba(52,211,153,.12);border-color:rgba(52,211,153,.28);}',
      '.aikd-miller-list{flex:1 1 auto;overflow-y:auto;overflow-x:hidden;padding:5px;min-height:0;}',
      '.aikd-miller-row{display:flex;align-items:center;gap:6px;width:100%;height:26px;padding:0 7px;border:1px solid transparent;',
      'border-radius:6px;background:transparent;color:#a9bdd4;font-size:11px;text-align:left;cursor:pointer;min-width:0;}',
      '.aikd-miller-row:hover{background:rgba(255,255,255,.045);color:#f1f7ff;}',
      '.aikd-miller-row.active{background:rgba(52,211,153,.1);border-color:rgba(52,211,153,.28);color:#d1fae5;}',
      '.aikd-miller-row.previewed{background:rgba(56,189,248,.09);border-color:rgba(56,189,248,.26);color:#dff2ff;}',
      '.aikd-miller-row.flash{animation:aikdMillerFlash .9s ease;}',
      '@keyframes aikdMillerFlash{0%{background:rgba(52,211,153,.35);}100%{background:rgba(52,211,153,.1);}}',
      '.aikd-miller-row .aikd-dot{width:7px;height:7px;border-radius:50%;flex:0 0 auto;}',
      '.aikd-miller-row .aikd-row-label{flex:1 1 auto;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}',
      '.aikd-miller-row .aikd-row-count{font-family:ui-monospace,Consolas,monospace;font-size:9px;color:#647892;flex:0 0 auto;}',
      '.aikd-miller-row .aikd-row-kind{font-size:8.5px;text-transform:uppercase;letter-spacing:.05em;color:#647892;flex:0 0 auto;}',
      '.aikd-miller-note{padding:7px 9px;font-size:9.5px;color:#647892;line-height:1.4;}',
      '.aikd-miller-empty{padding:16px 12px;text-align:center;font-size:11px;color:#647892;}',
      '.aikd-miller-preview{flex:1 1 auto;overflow-y:auto;min-height:0;padding:10px 11px 14px;display:flex;flex-direction:column;gap:10px;}',
      '.aikd-miller-pv-head{display:flex;flex-direction:column;gap:4px;}',
      '.aikd-miller-pv-kind{font-size:9px;letter-spacing:.1em;text-transform:uppercase;color:#647892;display:flex;align-items:center;gap:6px;}',
      '.aikd-miller-pv-kind i{width:7px;height:7px;border-radius:50%;display:inline-block;}',
      '.aikd-miller-pv-title{font-size:14px;font-weight:700;line-height:1.3;color:#f1f7ff;word-break:break-word;}',
      '.aikd-miller-pv-meta{font-size:10.5px;color:#8397b0;line-height:1.5;word-break:break-word;}',
      '.aikd-miller-pv-actions{display:flex;gap:6px;flex-wrap:wrap;}',
      '.aikd-miller-btn{padding:3px 8px;border:1px solid var(--line-2,#263a52);border-radius:6px;background:rgba(255,255,255,.03);',
      'color:#a9bdd4;font-size:10px;cursor:pointer;}',
      '.aikd-miller-btn:hover{color:#f1f7ff;border-color:#38bdf8;background:rgba(56,189,248,.1);}',
      '.aikd-miller-btn.primary{color:#a7f3d0;border-color:rgba(52,211,153,.3);background:rgba(52,211,153,.1);}',
      '.aikd-miller-section{display:flex;flex-direction:column;gap:5px;}',
      '.aikd-miller-section-title{display:flex;align-items:center;gap:6px;font-size:9px;letter-spacing:.1em;text-transform:uppercase;color:#647892;}',
      '.aikd-miller-section-title .aikd-h-count{margin-left:auto;font-family:ui-monospace,Consolas,monospace;font-size:9px;letter-spacing:0;text-transform:none;}',
      '.aikd-miller-link{display:flex;align-items:center;gap:6px;width:100%;padding:4px 6px;border:1px solid transparent;border-radius:6px;',
      'background:rgba(255,255,255,.02);color:#a9bdd4;font-size:10.5px;text-align:left;cursor:pointer;}',
      '.aikd-miller-link:hover{background:rgba(56,189,248,.09);border-color:rgba(56,189,248,.24);color:#eaf3ff;}',
      '.aikd-miller-link .aikd-dot{width:6px;height:6px;border-radius:50%;flex:0 0 auto;}',
      '.aikd-miller-link .aikd-link-label{flex:1 1 auto;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}',
      '.aikd-miller-link .aikd-link-kind{font-size:8.5px;color:#647892;flex:0 0 auto;text-transform:uppercase;letter-spacing:.04em;}',
      '.aikd-miller-mini{position:relative;border:1px solid var(--line,#1b2a3d);border-radius:9px;overflow:hidden;',
      'background:radial-gradient(300px 150px at 50% 45%,#0f1c2d 0%,#080d16 75%);}',
      '.aikd-miller-mini canvas{display:block;width:100%;height:' + MINI_HEIGHT + 'px;cursor:pointer;}',
      '.aikd-miller-mini .aikd-mini-hint{position:absolute;left:8px;bottom:6px;font-size:8.5px;color:#647892;pointer-events:none;}',
      '.aikd-miller-tags{display:flex;flex-wrap:wrap;gap:4px;}',
      '.aikd-miller-tag{font-size:9px;padding:1px 6px;border-radius:99px;border:1px solid rgba(56,189,248,.25);',
      'color:#7dd3fc;background:rgba(56,189,248,.08);}'
    ].join('');
    document.head.appendChild(style);
  }

  function el(tag, className, text) {
    var node = document.createElement(tag);
    if (className) node.className = className;
    if (text != null) node.textContent = String(text);
    return node;
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

  function num(v) { var n = Number(v); return isFinite(n) ? n : 0; }

  function kindColor(k) {
    if (app && app.helpers && typeof app.helpers.kindColor === 'function') {
      try { return app.helpers.kindColor(k); } catch (e) { /* fall through */ }
    }
    return KIND_COLORS[String(k || '').toLowerCase()] || KIND_COLORS.node;
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

  function projectColor(key) {
    return key === UNASSIGNED ? '#8397b0' : PROJECT_PALETTE[hashStr(key) % PROJECT_PALETTE.length];
  }

  function tsOf(node) {
    if (!node) return 0;
    var raw = node.updated_at || node.created_at || '';
    var t = Date.parse(String(raw).replace(' ', 'T'));
    return isFinite(t) ? t : 0;
  }

  function fmtTs(ts) {
    if (!ts) return '—';
    try { return new Date(ts).toLocaleString(); } catch (e) { return '—'; }
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

  /* ------------------------------------------------------------------- data */

  function rebuildData(context) {
    nodes = Array.isArray(context.nodes) ? context.nodes : [];
    rawNodes = Array.isArray(context.rawNodes) && context.rawNodes.length ? context.rawNodes : nodes.slice();
    rawEdges = Array.isArray(context.rawEdges) && context.rawEdges.length
      ? context.rawEdges
      : (Array.isArray(context.edges) ? context.edges : []);

    nodeById = new Map();
    rawNodes.forEach(function (n) { if (n && n.id != null) nodeById.set(n.id, n); });
    nodes.forEach(function (n) { if (n && n.id != null) nodeById.set(n.id, n); });

    var groups = new Map();
    nodes.forEach(function (n) {
      var key = projectKey(n);
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(n);
    });
    projectGroups = Array.from(groups.keys()).map(function (key) {
      var members = groups.get(key) || [];
      return { key: key, label: key, count: members.length, nodes: members };
    }).sort(function (a, b) {
      if (a.key === UNASSIGNED) return 1;
      if (b.key === UNASSIGNED) return -1;
      return b.count - a.count || a.label.localeCompare(b.label);
    });

    if (project == null || !groups.has(project)) {
      project = projectGroups.length ? projectGroups[0].key : null;
      kind = null;
    }
    var types = typesFor(project);
    if (kind != null && types.indexOf(kind) === -1) kind = null;
  }

  function typesFor(projectKeyValue) {
    var members = nodes.filter(function (n) { return projectKey(n) === projectKeyValue; });
    var counts = new Map();
    members.forEach(function (n) { counts.set(n.kind, (counts.get(n.kind) || 0) + 1); });
    var kinds = Array.from(counts.keys());
    kinds.sort(function (a, b) {
      var ia = TYPE_ORDER.indexOf(a); var ib = TYPE_ORDER.indexOf(b);
      if (ia === -1) ia = 99;
      if (ib === -1) ib = 99;
      return ia - ib || a.localeCompare(b);
    });
    return kinds;
  }

  function typeCount(projectKeyValue, typeName) {
    return nodes.filter(function (n) {
      return projectKey(n) === projectKeyValue && n.kind === typeName;
    }).length;
  }

  function itemsFor() {
    var list = nodes.filter(function (n) {
      if (projectKey(n) !== project) return false;
      return kind == null || n.kind === kind;
    });
    list.sort(function (a, b) {
      if (sortMode === 'name') return labelOf(a).localeCompare(labelOf(b));
      if (sortMode === 'recent') {
        var d = tsOf(b) - tsOf(a);
        if (d) return d;
        return labelOf(a).localeCompare(labelOf(b));
      }
      return (num(b.degree) - num(a.degree)) || labelOf(a).localeCompare(labelOf(b));
    });
    return list;
  }

  function neighborRefs(nodeId) {
    var seen = new Set();
    var out = [];
    rawEdges.forEach(function (edge) {
      if (!edge) return;
      var other = null;
      if (edge.from === nodeId && edge.to !== nodeId) other = edge.to;
      else if (edge.to === nodeId && edge.from !== nodeId) other = edge.from;
      if (!other || seen.has(other)) return;
      seen.add(other);
      var n = nodeById.get(other);
      out.push({ id: other, node: n || null, edge: edge, direction: edge.from === nodeId ? 'out' : 'in' });
    });
    return out;
  }

  function backlinks(nodeId) {
    return rawEdges.filter(function (e) { return e && e.to === nodeId && e.from !== nodeId; });
  }

  function outlinks(nodeId) {
    return rawEdges.filter(function (e) { return e && e.from === nodeId && e.to !== nodeId; });
  }

  /* ---------------------------------------------------------------- columns */

  function snapshotScroll() {
    return {
      projects: listProjects ? listProjects.scrollTop : 0,
      types: listTypes ? listTypes.scrollTop : 0,
      items: listItems ? listItems.scrollTop : 0,
      preview: previewBody ? previewBody.scrollTop : 0
    };
  }

  function restoreScroll(snap) {
    if (!snap) return;
    if (listProjects) listProjects.scrollTop = snap.projects;
    if (listTypes) listTypes.scrollTop = snap.types;
    if (listItems) listItems.scrollTop = snap.items;
    if (previewBody) previewBody.scrollTop = snap.preview;
  }

  function rowEl(className, color, label, count, extra) {
    var row = el('button', className);
    row.type = 'button';
    var dot = el('i', 'aikd-dot');
    dot.style.background = color;
    dot.style.boxShadow = '0 0 7px ' + rgba(color, 0.65);
    row.appendChild(dot);
    row.appendChild(el('span', 'aikd-row-label', label));
    if (extra) row.appendChild(el('span', extra.cls, extra.text));
    if (count != null) row.appendChild(el('span', 'aikd-row-count', String(count)));
    return row;
  }

  function renderProjects() {
    if (!listProjects) return;
    listProjects.innerHTML = '';
    var headCount = colProjects ? colProjects.querySelector('.aikd-h-count') : null;
    if (headCount) headCount.textContent = String(projectGroups.length) + ' groups';
    if (!projectGroups.length) {
      listProjects.appendChild(el('div', 'aikd-miller-empty', 'No projects in the current filter.'));
      return;
    }
    projectGroups.forEach(function (group) {
      var row = rowEl('aikd-miller-row' + (group.key === project ? ' active' : ''),
        projectColor(group.key), group.label, group.count);
      row.title = group.label + ' · ' + group.count + ' nodes';
      row.dataset.project = group.key;
      listProjects.appendChild(row);
    });
  }

  function renderTypes() {
    if (!listTypes) return;
    listTypes.innerHTML = '';
    var headCount = colTypes ? colTypes.querySelector('.aikd-h-count') : null;
    if (headCount) headCount.textContent = project ? String(typesFor(project).length) + ' types' : '';
    if (!project) {
      listTypes.appendChild(el('div', 'aikd-miller-empty', 'Pick a project.'));
      return;
    }
    var total = nodes.filter(function (n) { return projectKey(n) === project; }).length;
    var all = rowEl('aikd-miller-row' + (kind == null ? ' active' : ''), '#8397b0', 'All types', total);
    all.dataset.type = '';
    listTypes.appendChild(all);

    typesFor(project).forEach(function (type) {
      var row = rowEl('aikd-miller-row' + (kind === type ? ' active' : ''),
        kindColor(type), type, typeCount(project, type));
      row.dataset.type = type;
      listTypes.appendChild(row);
    });
  }

  function renderItems() {
    if (!listItems) return;
    listItems.innerHTML = '';
    var items = itemsFor();
    var headCount = colItems ? colItems.querySelector('.aikd-h-count') : null;
    if (headCount) headCount.textContent = String(items.length) + ' items';
    if (root) {
      var sortButtons = root.querySelectorAll('.aikd-miller-sort button');
      for (var si = 0; si < sortButtons.length; si++) {
        sortButtons[si].classList.toggle('active', sortButtons[si].dataset.sort === sortMode);
      }
    }
    if (!items.length) {
      listItems.appendChild(el('div', 'aikd-miller-empty', 'Nothing here yet — widen the type filters.'));
      return;
    }
    var shown = items.slice(0, ITEM_LIMIT);
    shown.forEach(function (n) {
      var cls = 'aikd-miller-row';
      if (n.id === previewId) cls += ' previewed';
      if (n.id === selectedId) cls += ' active';
      var row = rowEl(cls, kindColor(n.kind), shortLabel(n), num(n.degree));
      row.dataset.node = n.id;
      row.title = labelOf(n) + '\n' + n.kind + ' · degree ' + num(n.degree);
      listItems.appendChild(row);
    });
    if (items.length > shown.length) {
      listItems.appendChild(el('div', 'aikd-miller-note',
        'Showing first ' + shown.length + ' of ' + items.length + ' items. Narrow the filters to see the rest.'));
    }
  }

  function renderBar() {
    if (!barEl) return;
    barEl.innerHTML = '';
    var title = el('div', 'aikd-miller-title');
    title.innerHTML = 'Miller Columns <span>◆</span>';
    barEl.appendChild(title);
    var hint = el('div', 'aikd-miller-hint',
      'Projects → Types → Items → Preview · click a node to inspect');
    barEl.appendChild(hint);
    var count = el('div', 'aikd-miller-count',
      nodes.length + ' visible · ' + rawNodes.length + ' total · ' + rawEdges.length + ' edges');
    barEl.appendChild(count);
  }

  function linkRow(ref) {
    var node = ref.node;
    var color = node ? kindColor(node.kind) : '#8397b0';
    var label = node ? shortLabel(node) : String(ref.id);
    var row = el('button', 'aikd-miller-link');
    row.type = 'button';
    row.dataset.node = node ? node.id : ref.id;
    row.title = (node ? labelOf(node) : ref.id) + ' · ' + (ref.edge.kind || 'link');
    var dot = el('i', 'aikd-dot');
    dot.style.background = color;
    row.appendChild(dot);
    row.appendChild(el('span', 'aikd-link-label', label));
    row.appendChild(el('span', 'aikd-link-kind', ref.edge.kind || 'link'));
    return row;
  }

  function renderPreview() {
    if (!previewBody) return;
    previewBody.innerHTML = '';
    var node = previewId ? nodeById.get(previewId) : null;
    if (!node) {
      previewBody.appendChild(el('div', 'aikd-miller-empty',
        'Select an item to preview its metadata, backlinks and local graph.'));
      return;
    }

    var head = el('div', 'aikd-miller-pv-head');
    var kindLine = el('div', 'aikd-miller-pv-kind');
    var dot = el('i');
    dot.style.background = kindColor(node.kind);
    dot.style.boxShadow = '0 0 8px ' + rgba(kindColor(node.kind), 0.7);
    kindLine.appendChild(dot);
    kindLine.appendChild(el('span', null,
      String(node.kind || 'node') + (node.subtype ? ' · ' + node.subtype : '')));
    head.appendChild(kindLine);
    head.appendChild(el('div', 'aikd-miller-pv-title', labelOf(node)));
    head.appendChild(el('div', 'aikd-miller-pv-meta',
      projectKey(node) + ' · scope ' + String(node.scope || '—') + ' · degree ' + num(node.degree)
      + (node.provenance ? ' · ' + node.provenance : '')));
    var stamp = tsOf(node);
    if (stamp) head.appendChild(el('div', 'aikd-miller-pv-meta', 'updated ' + fmtTs(stamp)));
    if (node.id && String(node.id) !== labelOf(node)) {
      head.appendChild(el('div', 'aikd-miller-pv-meta', String(node.id)));
    }
    previewBody.appendChild(head);

    var actions = el('div', 'aikd-miller-pv-actions');
    var inspectBtn = el('button', 'aikd-miller-btn primary', '◎ Inspect');
    inspectBtn.type = 'button';
    inspectBtn.dataset.action = 'inspect';
    actions.appendChild(inspectBtn);
    var focusBtn = el('button', 'aikd-miller-btn', '⌖ Focus local (depth 1)');
    focusBtn.type = 'button';
    focusBtn.dataset.action = 'focus';
    actions.appendChild(focusBtn);
    previewBody.appendChild(actions);

    // Mini local graph.
    var miniSection = el('div', 'aikd-miller-section');
    var miniTitle = el('div', 'aikd-miller-section-title', 'Mini Local Graph · Depth 1');
    miniTitle.appendChild(el('span', 'aikd-h-count', String(neighborRefs(node.id).length) + ' neighbors'));
    miniSection.appendChild(miniTitle);
    var mini = el('div', 'aikd-miller-mini');
    var miniCanvas = document.createElement('canvas');
    mini.appendChild(miniCanvas);
    mini.appendChild(el('div', 'aikd-mini-hint', 'click a neighbor to inspect'));
    miniSection.appendChild(mini);
    previewBody.appendChild(miniSection);

    miniCanvas.addEventListener('click', onMiniClick);
    if (miniRaf) window.cancelAnimationFrame(miniRaf);
    miniRaf = window.requestAnimationFrame(function () {
      miniRaf = 0;
      if (!miniCanvas.isConnected) return;
      drawMini(miniCanvas, node);
    });

    // Backlinks.
    var incoming = backlinks(node.id);
    var blSection = el('div', 'aikd-miller-section');
    var blTitle = el('div', 'aikd-miller-section-title', 'Backlinks');
    blTitle.appendChild(el('span', 'aikd-h-count', String(incoming.length)));
    blSection.appendChild(blTitle);
    if (!incoming.length) {
      blSection.appendChild(el('div', 'aikd-miller-note', 'No incoming links — this node is a root.'));
    } else {
      incoming.slice(0, 12).forEach(function (edge) {
        var target = nodeById.get(edge.from);
        blSection.appendChild(linkRow({
          id: edge.from,
          node: target || null,
          edge: edge,
          direction: 'in'
        }));
      });
      if (incoming.length > 12) {
        blSection.appendChild(el('div', 'aikd-miller-note', '+ ' + (incoming.length - 12) + ' more backlinks'));
      }
    }
    previewBody.appendChild(blSection);

    // Outgoing.
    var outgoing = outlinks(node.id);
    var outSection = el('div', 'aikd-miller-section');
    var outTitle = el('div', 'aikd-miller-section-title', 'Outgoing');
    outTitle.appendChild(el('span', 'aikd-h-count', String(outgoing.length)));
    outSection.appendChild(outTitle);
    if (!outgoing.length) {
      outSection.appendChild(el('div', 'aikd-miller-note', 'No outgoing links.'));
    } else {
      outgoing.slice(0, 8).forEach(function (edge) {
        var target = nodeById.get(edge.to);
        outSection.appendChild(linkRow({
          id: edge.to,
          node: target || null,
          edge: edge,
          direction: 'out'
        }));
      });
      if (outgoing.length > 8) {
        outSection.appendChild(el('div', 'aikd-miller-note', '+ ' + (outgoing.length - 8) + ' more'));
      }
    }
    previewBody.appendChild(outSection);

    // Tags (when the node carries them).
    var tags = Array.isArray(node.tags) ? node.tags : [];
    if (tags.length) {
      var tagSection = el('div', 'aikd-miller-section');
      tagSection.appendChild(el('div', 'aikd-miller-section-title', 'Tags'));
      var tagWrap = el('div', 'aikd-miller-tags');
      tags.slice(0, 18).forEach(function (tag) {
        tagWrap.appendChild(el('span', 'aikd-miller-tag', '#' + String(tag).replace(/^#+/, '')));
      });
      tagSection.appendChild(tagWrap);
      previewBody.appendChild(tagSection);
    }
  }

  function renderAll(flashNodeId) {
    var snap = snapshotScroll();
    renderBar();
    renderProjects();
    renderTypes();
    renderItems();
    renderPreview();
    restoreScroll(snap);
    if (flashNodeId) scrollToNode(flashNodeId);
  }

  function scrollToNode(nodeId) {
    if (!listItems) return;
    var rows = listItems.querySelectorAll('.aikd-miller-row[data-node]');
    for (var i = 0; i < rows.length; i++) {
      if (rows[i].dataset.node === nodeId) {
        var row = rows[i];
        row.classList.add('flash');
        if (typeof row.scrollIntoView === 'function') {
          try { row.scrollIntoView({ block: 'nearest' }); } catch (e) { row.scrollIntoView(); }
        }
        window.setTimeout(function () { row.classList.remove('flash'); }, 950);
        return;
      }
    }
  }

  /* ----------------------------------------------------------- mini graph */

  function drawMini(canvas, centerNode) {
    var g = canvas.getContext('2d');
    if (!g) return;
    var dpr = Math.min(3, Math.max(1, window.devicePixelRatio || 1));
    var w = Math.max(120, Math.round(canvas.clientWidth || 260));
    var h = MINI_HEIGHT;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);

    var bg = g.createRadialGradient(w / 2, h / 2, 0, w / 2, h / 2, Math.max(w, h) * 0.65);
    bg.addColorStop(0, 'rgba(56,189,248,0.06)');
    bg.addColorStop(1, 'rgba(0,0,0,0)');
    g.fillStyle = bg;
    g.fillRect(0, 0, w, h);

    var refs = neighborRefs(centerNode.id).slice(0, MINI_MAX_NEIGHBORS);
    var cx = w / 2;
    var cy = h / 2;
    var radius = Math.min(w, h) * 0.33;
    var positions = [{ node: centerNode, x: cx, y: cy, r: 7 }];

    refs.forEach(function (ref, i) {
      var angle = (i / Math.max(1, refs.length)) * Math.PI * 2 - Math.PI / 2;
      positions.push({
        node: ref.node || { id: ref.id, label: ref.id, kind: 'node' },
        x: cx + Math.cos(angle) * radius * (w < 260 ? 0.92 : 1),
        y: cy + Math.sin(angle) * radius * 0.82,
        r: 4.5
      });
    });

    // Links.
    refs.forEach(function (ref, i) {
      var p = positions[i + 1];
      g.beginPath();
      g.moveTo(cx, cy);
      g.lineTo(p.x, p.y);
      g.strokeStyle = rgba(kindColor((ref.node && ref.node.kind) || 'node'), 0.45);
      g.lineWidth = 1;
      g.stroke();
    });

    // Neighbor dots.
    positions.forEach(function (p, idx) {
      if (idx === 0) return;
      var color = kindColor(p.node.kind);
      g.fillStyle = color;
      g.beginPath();
      g.arc(p.x, p.y, p.r, 0, Math.PI * 2);
      g.fill();
      g.strokeStyle = rgba(color, 0.45);
      g.lineWidth = 1;
      g.beginPath();
      g.arc(p.x, p.y, p.r + 2.5, 0, Math.PI * 2);
      g.stroke();
    });

    // Center node.
    var centerColor = kindColor(centerNode.kind);
    g.save();
    g.shadowColor = rgba(centerColor, 0.85);
    g.shadowBlur = 14;
    g.fillStyle = centerColor;
    g.beginPath();
    g.arc(cx, cy, 7, 0, Math.PI * 2);
    g.fill();
    g.shadowBlur = 0;
    g.strokeStyle = 'rgba(255,255,255,0.85)';
    g.lineWidth = 1.4;
    g.beginPath();
    g.arc(cx, cy, 10.5, 0, Math.PI * 2);
    g.stroke();
    g.restore();

    // Center label.
    g.font = '600 10px "Segoe UI", system-ui, sans-serif';
    g.textAlign = 'center';
    g.fillStyle = 'rgba(234,243,255,0.92)';
    g.fillText(fitText(g, shortLabel(centerNode), Math.min(w - 20, 180)), cx, h - 8);
    g.textAlign = 'start';

    if (!refs.length) {
      g.fillStyle = 'rgba(100,120,146,0.7)';
      g.font = '10px "Segoe UI", system-ui, sans-serif';
      g.textAlign = 'center';
      g.fillText('no neighbors', cx, cy + 24);
      g.textAlign = 'start';
    }

    canvas._aikdPositions = positions;
  }

  function onMiniClick(ev) {
    var canvas = ev.currentTarget;
    var positions = canvas && canvas._aikdPositions;
    if (!positions) return;
    var rect = canvas.getBoundingClientRect();
    var x = ev.clientX - rect.left;
    var y = ev.clientY - rect.top;
    for (var i = 0; i < positions.length; i++) {
      var p = positions[i];
      var dx = p.x - x;
      var dy = p.y - y;
      if (dx * dx + dy * dy <= (p.r + 5) * (p.r + 5)) {
        if (p.node && p.node.id) selectNodeById(p.node.id, true, true);
        return;
      }
    }
  }

  /* ------------------------------------------------------------- selection */

  function applySelection(node, navigate, notify) {
    if (!node || !node.id) return;
    selectedId = node.id;
    previewId = node.id;
    if (navigate) {
      if (nodeById.has(node.id) || nodes.indexOf(node) !== -1) {
        project = projectKey(node);
        kind = node.kind != null ? node.kind : null;
      }
    }
    renderAll();
    if (notify && app && app.helpers && typeof app.helpers.onSelectNode === 'function') {
      try { app.helpers.onSelectNode(node); } catch (e) { /* ignore */ }
    }
  }

  function selectNodeById(nodeId, navigate, notify) {
    var node = nodeById.get(nodeId);
    if (!node) return;
    applySelection(node, navigate, notify);
  }

  /* --------------------------------------------------------------- events */

  function onClick(ev) {
    var target = ev.target;
    if (!target || !root || !root.contains(target)) return;

    var sortBtn = target.closest ? target.closest('[data-sort]') : null;
    if (sortBtn) {
      ev.preventDefault();
      sortMode = sortBtn.dataset.sort || 'degree';
      renderItems();
      return;
    }

    var action = target.closest ? target.closest('[data-action]') : null;
    if (action) {
      ev.preventDefault();
      var current = previewId ? nodeById.get(previewId) : null;
      if (!current) return;
      if (action.dataset.action === 'inspect') {
        if (app && app.helpers && typeof app.helpers.onSelectNode === 'function') {
          try { app.helpers.onSelectNode(current); } catch (e) { /* ignore */ }
        }
      } else if (action.dataset.action === 'focus') {
        if (app && app.helpers && typeof app.helpers.onFocusLocal === 'function') {
          try { app.helpers.onFocusLocal(current.id); } catch (e) { /* ignore */ }
        }
      }
      return;
    }

    var projRow = target.closest ? target.closest('[data-project]') : null;
    if (projRow) {
      ev.preventDefault();
      project = projRow.dataset.project;
      kind = null;
      renderAll();
      return;
    }

    var typeRow = target.closest ? target.closest('[data-type]') : null;
    if (typeRow) {
      ev.preventDefault();
      var t = typeRow.dataset.type;
      kind = t ? t : null;
      renderAll();
      return;
    }

    var nodeRow = target.closest ? target.closest('[data-node]') : null;
    if (nodeRow) {
      ev.preventDefault();
      selectNodeById(nodeRow.dataset.node, true, true);
      return;
    }
  }

  /* ------------------------------------------------------------- lifecycle */

  function buildDom() {
    root = el('div', 'aikd-miller');

    barEl = el('div', 'aikd-miller-bar');
    root.appendChild(barEl);

    var cols = el('div', 'aikd-miller-cols');

    colProjects = el('div', 'aikd-miller-col');
    var headP = el('div', 'aikd-miller-head');
    headP.appendChild(el('span', null, 'Projects'));
    headP.appendChild(el('span', 'aikd-h-count', ''));
    colProjects.appendChild(headP);
    listProjects = el('div', 'aikd-miller-list');
    colProjects.appendChild(listProjects);
    cols.appendChild(colProjects);

    colTypes = el('div', 'aikd-miller-col');
    var headT = el('div', 'aikd-miller-head');
    headT.appendChild(el('span', null, 'Types'));
    headT.appendChild(el('span', 'aikd-h-count', ''));
    colTypes.appendChild(headT);
    listTypes = el('div', 'aikd-miller-list');
    colTypes.appendChild(listTypes);
    cols.appendChild(colTypes);

    colItems = el('div', 'aikd-miller-col');
    var headI = el('div', 'aikd-miller-head');
    headI.appendChild(el('span', null, 'Items'));
    headI.appendChild(el('span', 'aikd-h-count', ''));
    var sortWrap = el('div', 'aikd-miller-sort');
    [['name', 'Name'], ['degree', 'Degree'], ['recent', 'Recent']].forEach(function (pair) {
      var btn = el('button', sortMode === pair[0] ? 'active' : '', pair[1]);
      btn.type = 'button';
      btn.dataset.sort = pair[0];
      sortWrap.appendChild(btn);
    });
    headI.appendChild(sortWrap);
    colItems.appendChild(headI);
    listItems = el('div', 'aikd-miller-list');
    colItems.appendChild(listItems);
    cols.appendChild(colItems);

    colPreview = el('div', 'aikd-miller-col');
    var headR = el('div', 'aikd-miller-head');
    headR.appendChild(el('span', null, 'Preview'));
    colPreview.appendChild(headR);
    previewBody = el('div', 'aikd-miller-preview');
    colPreview.appendChild(previewBody);
    cols.appendChild(colPreview);

    root.appendChild(cols);
    container.appendChild(root);

    root.addEventListener('click', onClick);
  }

  function resizeMini() {
    if (resizeRaf) return;
    resizeRaf = window.requestAnimationFrame(function () {
      resizeRaf = 0;
      var node = previewId ? nodeById.get(previewId) : null;
      if (!node) return;
      var canvas = root ? root.querySelector('.aikd-miller-mini canvas') : null;
      if (canvas && canvas.isConnected) drawMini(canvas, node);
    });
  }

  function destroy() {
    if (miniRaf) { window.cancelAnimationFrame(miniRaf); miniRaf = 0; }
    if (resizeRaf) { window.cancelAnimationFrame(resizeRaf); resizeRaf = 0; }
    if (ro) { try { ro.disconnect(); } catch (e) { /* ignore */ } ro = null; }
    if (usingWindowResize) {
      window.removeEventListener('resize', resizeMini);
      usingWindowResize = false;
    }
    if (root) {
      root.removeEventListener('click', onClick);
      root.remove();
      root = null;
    }
    barEl = colProjects = colTypes = colItems = colPreview = null;
    listProjects = listTypes = listItems = previewBody = null;
    container = null;
    app = null;
    nodes = [];
    rawNodes = [];
    rawEdges = [];
    nodeById = new Map();
    projectGroups = [];
    selectedId = null;
    previewId = null;
    lastCtxSelected = null;
    project = null;
    kind = null;
  }

  var viewApi = {
    mount: function (context) {
      if (root) destroy();
      app = context || {};
      container = app.container;
      if (!container && typeof document !== 'undefined') {
        container = document.getElementById('graph-alt-stage') || document.getElementById('graph-stage');
      }
      if (!container) return viewApi;

      injectStyle();
      buildDom();
      rebuildData(app);
      selectedId = app.selectedId || null;
      previewId = app.selectedId || null;
      lastCtxSelected = app.selectedId || null;
      if (previewId && nodeById.has(previewId)) {
        project = projectKey(nodeById.get(previewId));
        kind = nodeById.get(previewId).kind;
      }
      renderAll();

      if (typeof ResizeObserver !== 'undefined') {
        ro = new ResizeObserver(resizeMini);
        ro.observe(root);
      } else {
        usingWindowResize = true;
        window.addEventListener('resize', resizeMini);
      }
      return viewApi;
    },

    update: function (context) {
      if (!root) return viewApi.mount(context);
      app = context || app;
      rebuildData(app);

      var incomingSelected = app.selectedId || null;
      if (incomingSelected && incomingSelected !== lastCtxSelected) {
        var selectedNode = nodeById.get(incomingSelected);
        if (selectedNode) {
          project = projectKey(selectedNode);
          kind = selectedNode.kind;
          selectedId = incomingSelected;
          previewId = incomingSelected;
        }
      }
      lastCtxSelected = incomingSelected;

      if (previewId && !nodeById.has(previewId)) previewId = null;
      if (!previewId) {
        var items = itemsFor();
        if (items.length) previewId = items[0].id;
      }
      renderAll();
      return viewApi;
    },

    focusNode: function (nodeId) {
      if (!nodeId) return;
      var node = nodeById.get(nodeId);
      if (!node) return;
      project = projectKey(node);
      kind = node.kind;
      selectedId = node.id;
      previewId = node.id;
      renderAll(node.id);
    },

    destroy: destroy
  };

  G.views.miller = viewApi;
})(window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} });
