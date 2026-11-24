/* ============================================================
   AIKD Knowledge Graph — 3D spatial views
   Registers: 3d-galaxy, 3d-sphere, 3d-orbital, 3d-layers
   Pure offline Canvas 2D projection with orbit / pan / zoom,
   HiDPI rendering, morphing layouts and full cleanup.
   ============================================================ */
(function (G) {
  'use strict';

  G.views = G.views || {};

  var TAU = Math.PI * 2;
  var GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
  var FOV = Math.PI / 3.35;
  var STYLE_ID = 'aikd-spatial-3d-styles';

  var LAYOUT_KEY = {
    '3d-galaxy': 'galaxy3d',
    '3d-sphere': 'sphere3d',
    '3d-orbital': 'orbital3d',
    '3d-layers': 'layers3d'
  };
  var MODE_LABEL = {
    '3d-galaxy': '3D Force Galaxy',
    '3d-sphere': '3D Fibonacci Sphere',
    '3d-orbital': '3D Orbital Rings',
    '3d-layers': '3D Semantic Layers'
  };
  var KIND_COLORS = {
    project: '#fbbf24',
    file: '#38bdf8',
    symbol: '#60a5fa',
    note: '#34d399',
    memory: '#c084fc',
    tag: '#f472b6',
    agent: '#fb7185',
    node: '#94a3b8'
  };
  var EDGE_COLORS = {
    imports: '#38bdf8',
    contains: '#647892',
    wikilink: '#34d399',
    tag: '#f472b6',
    mentions: '#fbbf24',
    relates: '#c084fc',
    references: '#60a5fa',
    link: '#2b435f'
  };

  /* Shared across the four 3D view instances so switching modes
     keeps the camera and morphs node positions instead of snapping. */
  var shared = {
    positions: null,
    camera: null
  };

  function ensureStyle() {
    if (document.getElementById(STYLE_ID)) return;
    var style = document.createElement('style');
    style.id = STYLE_ID;
    style.textContent = [
      '.aikd3d-host{overflow:hidden;}',
      '.aikd3d-canvas{position:absolute;inset:0;width:100%;height:100%;display:block;',
      'cursor:grab;touch-action:none;z-index:1;}',
      '.aikd3d-canvas.is-dragging{cursor:grabbing;}',
      '.aikd3d-toolbar{position:absolute;right:10px;top:10px;z-index:4;display:flex;',
      'align-items:center;gap:6px;pointer-events:none;}',
      '.aikd3d-toolbar > *{pointer-events:auto;}',
      '.aikd3d-chip{font:600 10px/1.2 inherit;letter-spacing:.04em;color:#94a3b8;',
      'background:rgba(6,12,22,.78);border:1px solid rgba(148,163,184,.18);',
      'border-radius:999px;padding:6px 10px;white-space:nowrap;backdrop-filter:blur(6px);}',
      '.aikd3d-btn{appearance:none;-webkit-appearance:none;font:600 10px/1.2 inherit;',
      'color:#c7d2fe;background:rgba(10,18,32,.86);border:1px solid rgba(129,140,248,.32);',
      'border-radius:8px;padding:6px 10px;cursor:pointer;white-space:nowrap;',
      'transition:color .15s ease,border-color .15s ease,background .15s ease,transform .15s ease;}',
      '.aikd3d-btn:hover{color:#f8fafc;border-color:rgba(34,211,238,.55);',
      'background:rgba(20,32,52,.94);transform:translateY(-1px);}',
      '.aikd3d-btn:active{transform:translateY(0);}',
      '.aikd3d-btn:focus-visible{outline:2px solid #38bdf8;outline-offset:1px;}',
      '.aikd3d-tooltip{position:absolute;z-index:5;max-width:270px;padding:8px 10px;',
      'border-radius:9px;border:1px solid rgba(56,189,248,.28);',
      'background:rgba(5,11,20,.95);color:#e2e8f0;font:11px/1.4 inherit;',
      'box-shadow:0 12px 34px rgba(0,0,0,.55);pointer-events:none;opacity:0;',
      'transform:translateY(4px);transition:opacity .12s ease,transform .12s ease;}',
      '.aikd3d-tooltip.is-visible{opacity:1;transform:none;}',
      '.aikd3d-tip-title{font-weight:700;font-size:11.5px;color:#f8fafc;',
      'white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:244px;}',
      '.aikd3d-tip-meta{margin-top:3px;display:flex;align-items:center;gap:5px;',
      'color:#94a3b8;font-size:10px;}',
      '.aikd3d-dot{width:7px;height:7px;border-radius:50%;display:inline-block;',
      'box-shadow:0 0 8px currentColor;flex:0 0 auto;}'
    ].join('');
    document.head.appendChild(style);
  }

  function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
  }

  function toNum(value, fallback) {
    var n = Number(value);
    return Number.isFinite(n) ? n : fallback;
  }

  function hashString(input) {
    var text = String(input === null || input === undefined ? '' : input);
    var hash = 2166136261 >>> 0;
    for (var i = 0; i < text.length; i += 1) {
      hash ^= text.charCodeAt(i);
      hash = Math.imul(hash, 16777619);
    }
    return hash >>> 0;
  }

  function mulberry32(seed) {
    var state = seed >>> 0;
    return function () {
      state = (state + 0x6D2B79F5) | 0;
      var t = Math.imul(state ^ (state >>> 15), 1 | state);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  function shortestAngle(from, to) {
    var delta = (to - from) % TAU;
    if (delta > Math.PI) delta -= TAU;
    if (delta < -Math.PI) delta += TAU;
    return delta;
  }

  function adapter() {
    return G.adapter || null;
  }

  function kindColorOf(node, ctx) {
    var map = adapter();
    if (map && typeof map.kindColor === 'function') return map.kindColor(node.kind);
    if (ctx && ctx.helpers && typeof ctx.helpers.kindColor === 'function') return ctx.helpers.kindColor(node.kind);
    return KIND_COLORS[String(node.kind || '').toLowerCase()] || KIND_COLORS.node;
  }

  function edgeColorOf(kind) {
    var map = adapter();
    if (map && typeof map.edgeColor === 'function') return map.edgeColor(kind);
    return EDGE_COLORS[String(kind || '').toLowerCase()] || EDGE_COLORS.link;
  }

  function basenameOf(path) {
    var map = adapter();
    if (map && typeof map.basename === 'function') return map.basename(path);
    var text = String(path === null || path === undefined ? '' : path).replace(/\\/g, '/');
    var parts = text.split('/').filter(Boolean);
    return parts.length ? parts[parts.length - 1] : text;
  }

  function clampTextOf(value, max) {
    var map = adapter();
    if (map && typeof map.clampText === 'function') return map.clampText(value, max);
    var text = String(value === null || value === undefined ? '' : value);
    return text.length > max ? text.slice(0, max - 1).trim() + '…' : text;
  }

  function escOf(value, ctx) {
    var map = adapter();
    if (map && typeof map.esc === 'function') return map.esc(value);
    if (ctx && ctx.helpers && typeof ctx.helpers.esc === 'function') return ctx.helpers.esc(value);
    return String(value === null || value === undefined ? '' : value)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  function worldRadius(node, maxDegree) {
    var degree = Math.max(0, toNum(node.degree, 0));
    var radius = 2.9 + Math.sqrt(degree) * 1.2;
    if (node.kind === 'project') radius += 3;
    else if (node.kind === 'agent' || node.kind === 'tag') radius += 1.3;
    if (maxDegree > 0 && degree >= maxDegree * 0.8) radius += 0.8;
    return radius;
  }

  function fallbackLayouts(nodes, edges) {
    var layouts = {
      galaxy3d: new Map(),
      sphere3d: new Map(),
      orbital3d: new Map(),
      layers3d: new Map()
    };
    var adj = new Map();
    nodes.forEach(function (node) { adj.set(node.id, []); });
    var validEdges = [];
    (edges || []).forEach(function (edge) {
      if (!adj.has(edge.from) || !adj.has(edge.to) || edge.from === edge.to) return;
      adj.get(edge.from).push(edge.to);
      adj.get(edge.to).push(edge.from);
      validEdges.push(edge);
    });

    var total = nodes.length;
    var radius = 320;
    nodes.forEach(function (node, index) {
      var t = total === 1 ? 0.5 : index / (total - 1);
      var y = 1 - t * 2;
      var ring = Math.sqrt(Math.max(0, 1 - y * y));
      var angle = index * GOLDEN_ANGLE;
      var point = {
        x: Math.cos(angle) * ring * radius,
        y: y * radius,
        z: Math.sin(angle) * ring * radius
      };
      layouts.galaxy3d.set(node.id, { x: point.x, y: point.y, z: point.z });
      layouts.sphere3d.set(node.id, { x: point.x, y: point.y, z: point.z });
      layouts.orbital3d.set(node.id, { x: point.x * 0.8, y: point.y * 0.6, z: point.z * 0.8 });
      layouts.layers3d.set(node.id, {
        x: Math.cos(angle) * radius * 0.8,
        y: ((index % 5) - 2) * 96,
        z: Math.sin(angle) * radius * 0.8
      });
    });
    return { layouts: layouts, adj: adj, edges: validEdges };
  }

  function createSpatialView(modeKey) {
    var layoutKey = LAYOUT_KEY[modeKey] || 'galaxy3d';
    var ctx = null;
    var container = null;
    var canvas = null;
    var g2d = null;
    var toolbar = null;
    var chipEl = null;
    var resetBtn = null;
    var tooltipEl = null;
    var destroyed = true;
    var raf = 0;
    var resizeObserver = null;
    var listeners = [];
    var stars = [];

    var state = {
      nodes: [],
      edges: [],
      nodeMap: new Map(),
      adj: new Map(),
      cur: new Map(),
      target: new Map(),
      topLabels: new Set(),
      screenItems: [],
      maxDegree: 1,
      boundsRadius: 380,
      hoverId: null,
      selectedId: null,
      tooltipId: null,
      width: 0,
      height: 0,
      dpr: 1,
      time: 0,
      lastTs: 0,
      cameraReady: false,
      pointerInside: false,
      pointer: { x: 0, y: 0 },
      drag: null,
      fly: null,
      camera: { yaw: 0.66, pitch: 0.3, dist: 1100, target: { x: 0, y: 0, z: 0 } }
    };

    /* ---------------------------------------------------------------- */
    /* DOM + lifecycle                                                   */
    /* ---------------------------------------------------------------- */

    function on(target, type, handler, options) {
      target.addEventListener(type, handler, options);
      listeners.push({ target: target, type: type, handler: handler, options: options });
    }

    function buildDom() {
      canvas = document.createElement('canvas');
      canvas.className = 'aikd3d-canvas';
      canvas.setAttribute('aria-label', MODE_LABEL[modeKey] || '3D graph');
      canvas.style.width = '100%';
      canvas.style.height = '100%';
      container.appendChild(canvas);
      g2d = canvas.getContext('2d');

      toolbar = document.createElement('div');
      toolbar.className = 'aikd3d-toolbar';

      chipEl = document.createElement('span');
      chipEl.className = 'aikd3d-chip';
      chipEl.textContent = (MODE_LABEL[modeKey] || '3D view') + ' · 0 nodes';
      toolbar.appendChild(chipEl);

      resetBtn = document.createElement('button');
      resetBtn.type = 'button';
      resetBtn.className = 'aikd3d-btn';
      resetBtn.textContent = '⟲ Reset Camera';
      resetBtn.title = 'Reset the 3D camera to fit every node';
      toolbar.appendChild(resetBtn);
      container.appendChild(toolbar);

      tooltipEl = document.createElement('div');
      tooltipEl.className = 'aikd3d-tooltip';
      container.appendChild(tooltipEl);
    }

    function bindEvents() {
      on(canvas, 'pointerdown', onPointerDown);
      on(canvas, 'pointermove', onPointerMove);
      on(canvas, 'pointerup', onPointerUp);
      on(canvas, 'pointercancel', onPointerUp);
      on(canvas, 'pointerleave', onPointerLeave);
      on(canvas, 'wheel', onWheel, { passive: false });
      on(canvas, 'contextmenu', onContextMenu);
      on(canvas, 'dblclick', onDoubleClick);
      on(resetBtn, 'click', onResetCamera);
      on(window, 'resize', resize);

      if (typeof ResizeObserver !== 'undefined') {
        resizeObserver = new ResizeObserver(function () { resize(); });
        resizeObserver.observe(container);
      }
    }

    function resetState() {
      state.nodes = [];
      state.edges = [];
      state.nodeMap = new Map();
      state.adj = new Map();
      state.target = new Map();
      state.topLabels = new Set();
      state.screenItems = [];
      state.maxDegree = 1;
      state.boundsRadius = 380;
      state.hoverId = null;
      state.selectedId = null;
      state.tooltipId = null;
      state.time = 0;
      state.lastTs = 0;
      state.cameraReady = false;
      state.pointerInside = false;
      state.drag = null;
      state.fly = null;
      state.cur = new Map();
      if (shared.positions) {
        Object.keys(shared.positions).forEach(function (id) {
          var point = shared.positions[id];
          if (point) state.cur.set(id, { x: point.x, y: point.y, z: point.z });
        });
      }
      if (shared.camera) {
        copyCamera(shared.camera);
        state.cameraReady = true;
      } else {
        state.camera = { yaw: 0.66, pitch: 0.3, dist: 1100, target: { x: 0, y: 0, z: 0 } };
      }
    }

    function mount(nextCtx) {
      if (!nextCtx || !nextCtx.container) return;
      if (!destroyed) destroy();
      ensureStyle();
      destroyed = false;
      resetState();
      listeners = [];

      ctx = nextCtx;
      container = nextCtx.container;
      container.classList.add('aikd3d-host');
      if (window.getComputedStyle && window.getComputedStyle(container).position === 'static') {
        container.style.position = 'relative';
      }

      buildDom();
      bindEvents();
      resize();
      syncData(nextCtx);
      if (!state.cameraReady) {
        fitCamera(true);
        state.cameraReady = true;
      }
      startLoop();
    }

    function update(nextCtx) {
      if (destroyed || !nextCtx) return;
      syncData(nextCtx);
    }

    function destroy() {
      if (destroyed) return;
      destroyed = true;

      if (raf) {
        cancelAnimationFrame(raf);
        raf = 0;
      }
      if (resizeObserver) {
        resizeObserver.disconnect();
        resizeObserver = null;
      }
      listeners.forEach(function (entry) {
        try {
          entry.target.removeEventListener(entry.type, entry.handler, entry.options);
        } catch (error) { /* noop */ }
      });
      listeners = [];

      if (state.cur.size) {
        var snapshot = {};
        state.cur.forEach(function (point, id) {
          snapshot[id] = { x: point.x, y: point.y, z: point.z };
        });
        shared.positions = snapshot;
      }
      shared.camera = {
        yaw: state.camera.yaw,
        pitch: state.camera.pitch,
        dist: state.camera.dist,
        target: { x: state.camera.target.x, y: state.camera.target.y, z: state.camera.target.z }
      };

      [canvas, toolbar, tooltipEl].forEach(function (el) {
        if (el && el.parentNode) el.parentNode.removeChild(el);
      });
      if (container) container.classList.remove('aikd3d-host');

      canvas = null;
      g2d = null;
      toolbar = null;
      chipEl = null;
      resetBtn = null;
      tooltipEl = null;
      container = null;
      ctx = null;
      stars = [];
      state.drag = null;
      state.fly = null;
      state.screenItems = [];
      state.cameraReady = false;
    }

    /* ---------------------------------------------------------------- */
    /* Data sync + camera                                                */
    /* ---------------------------------------------------------------- */

    function copyCamera(source) {
      state.camera.yaw = toNum(source.yaw, 0.66);
      state.camera.pitch = clamp(toNum(source.pitch, 0.3), -1.42, 1.42);
      state.camera.dist = clamp(toNum(source.dist, 1100), 140, 5200);
      state.camera.target = {
        x: toNum(source.target && source.target.x, 0),
        y: toNum(source.target && source.target.y, 0),
        z: toNum(source.target && source.target.z, 0)
      };
    }

    function syncData(nextCtx) {
      ctx = nextCtx;
      var nodes = Array.isArray(ctx.nodes) ? ctx.nodes : [];
      var edges = Array.isArray(ctx.edges) ? ctx.edges : [];
      if (Object.prototype.hasOwnProperty.call(ctx, 'selectedId')) {
        state.selectedId = ctx.selectedId || null;
      }

      var map = adapter();
      var spatial = map && typeof map.toSpatialGraph === 'function'
        ? map.toSpatialGraph(nodes, edges)
        : fallbackLayouts(nodes, edges);
      var layout = spatial.layouts && spatial.layouts[layoutKey] ? spatial.layouts[layoutKey] : null;
      if (!layout) layout = fallbackLayouts(nodes, edges).layouts[layoutKey];

      state.nodes = nodes;
      state.edges = spatial.edges || edges;
      state.nodeMap = new Map(nodes.map(function (node) { return [node.id, node]; }));
      state.adj = spatial.adj || new Map();
      state.maxDegree = Math.max(1, toNum(spatial.maxDegree, 1));

      var nextTarget = new Map();
      var nextCur = new Map();
      nodes.forEach(function (node) {
        var point = layout.get(node.id);
        if (!point) return;
        nextTarget.set(node.id, { x: point.x, y: point.y, z: point.z });
        var current = state.cur.get(node.id);
        if (!current) {
          current = { x: point.x, y: point.y, z: point.z };
        }
        nextCur.set(node.id, current);
      });
      state.target = nextTarget;
      state.cur = nextCur;

      var labelled = nodes.slice().sort(function (a, b) {
        return toNum(b.degree, 0) - toNum(a.degree, 0);
      }).slice(0, 16);
      state.topLabels = new Set(labelled.map(function (node) { return node.id; }));

      var maxRadius = 120;
      nextTarget.forEach(function (point) {
        maxRadius = Math.max(maxRadius, Math.sqrt(point.x * point.x + point.y * point.y + point.z * point.z));
      });
      state.boundsRadius = maxRadius;

      if (chipEl) {
        chipEl.textContent = (MODE_LABEL[modeKey] || '3D view') + ' · ' +
          nodes.length + ' nodes · ' + state.edges.length + ' edges';
      }

      if (state.hoverId && !state.nodeMap.has(state.hoverId)) state.hoverId = null;
      if (state.selectedId && !state.nodeMap.has(state.selectedId)) state.selectedId = null;

      if (!state.cameraReady) {
        fitCamera(true);
        state.cameraReady = true;
      }
    }

    function fitCamera(instant) {
      var dist = clamp(state.boundsRadius * 2.4, 320, 5200);
      var goal = { yaw: 0.66, pitch: 0.3, dist: dist, target: { x: 0, y: 0, z: 0 } };
      if (instant) {
        state.camera.yaw = goal.yaw;
        state.camera.pitch = goal.pitch;
        state.camera.dist = goal.dist;
        state.camera.target = { x: 0, y: 0, z: 0 };
        state.fly = null;
      } else {
        state.fly = goal;
      }
    }

    function focusNode(nodeId) {
      if (destroyed || !nodeId) return false;
      var point = state.target.get(nodeId) || state.cur.get(nodeId);
      if (!point) return false;
      state.selectedId = nodeId;
      state.fly = {
        yaw: state.camera.yaw,
        pitch: state.camera.pitch,
        dist: clamp(Math.min(state.camera.dist, state.boundsRadius * 1.15), 180, 2600),
        target: { x: point.x, y: point.y, z: point.z }
      };
      return true;
    }

    function resetCamera() {
      fitCamera(false);
    }

    /* ---------------------------------------------------------------- */
    /* Loop, resize and interactions                                     */
    /* ---------------------------------------------------------------- */

    function startLoop() {
      state.lastTs = 0;
      if (raf) cancelAnimationFrame(raf);
      var frame = function (timestamp) {
        if (destroyed) { raf = 0; return; }
        var now = toNum(timestamp, 0);
        var dt = state.lastTs ? Math.min(0.05, Math.max(0, (now - state.lastTs) / 1000)) : 1 / 60;
        state.lastTs = now;
        state.time += dt;
        step(dt);
        draw();
        raf = requestAnimationFrame(frame);
      };
      raf = requestAnimationFrame(frame);
    }

    function step(dt) {
      var morph = 1 - Math.exp(-dt * 9);
      state.cur.forEach(function (current, id) {
        var target = state.target.get(id);
        if (!target) return;
        current.x += (target.x - current.x) * morph;
        current.y += (target.y - current.y) * morph;
        current.z += (target.z - current.z) * morph;
      });

      if (!state.fly) return;
      var fly = state.fly;
      var camera = state.camera;
      var ease = 1 - Math.exp(-dt * 6);
      var deltaYaw = shortestAngle(camera.yaw, fly.yaw);
      camera.yaw += deltaYaw * ease;
      camera.pitch += (fly.pitch - camera.pitch) * ease;
      camera.dist += (fly.dist - camera.dist) * ease;
      camera.target.x += (fly.target.x - camera.target.x) * ease;
      camera.target.y += (fly.target.y - camera.target.y) * ease;
      camera.target.z += (fly.target.z - camera.target.z) * ease;

      var targetGap = Math.abs(fly.target.x - camera.target.x) +
        Math.abs(fly.target.y - camera.target.y) +
        Math.abs(fly.target.z - camera.target.z);
      if (Math.abs(deltaYaw) < 0.002 && Math.abs(fly.dist - camera.dist) < 1.2 && targetGap < 0.8) {
        camera.yaw = fly.yaw;
        camera.pitch = fly.pitch;
        camera.dist = fly.dist;
        camera.target.x = fly.target.x;
        camera.target.y = fly.target.y;
        camera.target.z = fly.target.z;
        state.fly = null;
      }
    }

    function resize() {
      if (!canvas || !container) return;
      var rect = container.getBoundingClientRect();
      var width = Math.max(160, Math.floor(rect.width || container.clientWidth || 640));
      var height = Math.max(140, Math.floor(rect.height || container.clientHeight || 420));
      var dpr = Math.min(2, Math.max(1, window.devicePixelRatio || 1));
      if (width === state.width && height === state.height && dpr === state.dpr) {
        buildStars(width, height);
        return;
      }
      state.width = width;
      state.height = height;
      state.dpr = dpr;
      canvas.width = Math.max(1, Math.floor(width * dpr));
      canvas.height = Math.max(1, Math.floor(height * dpr));
      buildStars(width, height);
    }

    function buildStars(width, height) {
      stars = [];
      var total = Math.max(70, Math.min(190, Math.floor((width * height) / 9000)));
      var rand = mulberry32(0xA1C0DE);
      for (var i = 0; i < total; i += 1) {
        stars.push({
          x: rand() * width,
          y: rand() * height,
          r: 0.4 + rand() * 1.1,
          a: 0.06 + rand() * 0.24,
          phase: rand() * TAU
        });
      }
    }

    function localPoint(event) {
      var rect = canvas.getBoundingClientRect();
      return { x: event.clientX - rect.left, y: event.clientY - rect.top };
    }

    function onPointerDown(event) {
      if (event.button === 2 || (event.button === 0 && event.shiftKey)) {
        state.drag = { mode: 'pan', x: event.clientX, y: event.clientY, moved: false };
      } else if (event.button === 0) {
        state.drag = { mode: 'orbit', x: event.clientX, y: event.clientY, moved: false };
      } else {
        return;
      }
      state.fly = null;
      state.pointerInside = true;
      if (canvas.setPointerCapture) {
        try { canvas.setPointerCapture(event.pointerId); } catch (error) { /* noop */ }
      }
      canvas.classList.add('is-dragging');
    }

    function onPointerMove(event) {
      var point = localPoint(event);
      state.pointer = point;
      state.pointerInside = true;

      if (!state.drag) return;
      var dx = event.clientX - state.drag.x;
      var dy = event.clientY - state.drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 2) state.drag.moved = true;
      state.drag.x = event.clientX;
      state.drag.y = event.clientY;

      if (state.drag.mode === 'orbit') {
        state.camera.yaw += dx * 0.0062;
        state.camera.pitch = clamp(state.camera.pitch + dy * 0.0055, -1.42, 1.42);
      } else {
        panBy(dx, dy);
      }
    }

    function onPointerUp(event) {
      if (!state.drag) return;
      var drag = state.drag;
      state.drag = null;
      canvas.classList.remove('is-dragging');
      if (canvas.releasePointerCapture) {
        try { canvas.releasePointerCapture(event.pointerId); } catch (error) { /* noop */ }
      }
      if (drag.moved || drag.mode !== 'orbit' || event.button !== 0) return;
      var hit = pickAt(localPoint(event));
      if (hit) selectNode(hit.node);
    }

    function onPointerLeave() {
      if (state.drag) return;
      state.pointerInside = false;
      state.hoverId = null;
    }

    function onWheel(event) {
      event.preventDefault();
      var delta = event.deltaMode === 1 ? event.deltaY * 18 : event.deltaY;
      delta = clamp(delta, -260, 260);
      state.fly = null;
      state.camera.dist = clamp(state.camera.dist * Math.exp(delta * 0.0016), 140, 5200);
    }

    function onContextMenu(event) {
      event.preventDefault();
    }

    function onDoubleClick(event) {
      var hit = pickAt(localPoint(event));
      if (!hit) return;
      selectNode(hit.node);
      invoke(ctx, 'onFocusLocal', hit.node.id);
    }

    function onResetCamera(event) {
      if (event) event.preventDefault();
      resetCamera();
    }

    function panBy(dx, dy) {
      var camera = state.camera;
      var cosYaw = Math.cos(camera.yaw);
      var sinYaw = Math.sin(camera.yaw);
      var cosPitch = Math.cos(camera.pitch);
      var sinPitch = Math.sin(camera.pitch);
      var right = { x: cosYaw, y: 0, z: -sinYaw };
      var up = { x: -sinYaw * sinPitch, y: cosPitch, z: -cosYaw * sinPitch };
      var scale = (2 * camera.dist * Math.tan(FOV / 2)) / Math.max(1, state.height);
      camera.target.x += (-right.x * dx + up.x * dy) * scale;
      camera.target.y += (-right.y * dx + up.y * dy) * scale;
      camera.target.z += (-right.z * dx + up.z * dy) * scale;
    }

    /* ---------------------------------------------------------------- */
    /* Selection helpers                                                 */
    /* ---------------------------------------------------------------- */

    function invoke(targetCtx, name, value) {
      if (!targetCtx) return false;
      if (targetCtx.helpers && typeof targetCtx.helpers[name] === 'function') {
        targetCtx.helpers[name](value);
        return true;
      }
      if (typeof targetCtx[name] === 'function') {
        targetCtx[name](value);
        return true;
      }
      return false;
    }

    function selectNode(node) {
      if (!node) return;
      state.selectedId = node.id;
      invoke(ctx, 'onSelectNode', node);
    }

    function pickAt(point) {
      var best = null;
      var bestDepth = Infinity;
      var items = state.screenItems || [];
      for (var i = 0; i < items.length; i += 1) {
        var item = items[i];
        var dx = point.x - item.x;
        var dy = point.y - item.y;
        var reach = Math.max(9, item.r + 7);
        if (dx * dx + dy * dy <= reach * reach && item.z < bestDepth) {
          best = item;
          bestDepth = item.z;
        }
      }
      return best;
    }

    function focusSetFor(id) {
      if (!id || !state.adj.has(id)) return null;
      var set = new Set([id]);
      var neighbors = state.adj.get(id);
      if (neighbors) neighbors.forEach(function (neighbor) { set.add(neighbor); });
      return set;
    }

    /* ---------------------------------------------------------------- */
    /* Rendering                                                         */
    /* ---------------------------------------------------------------- */

    function makeProjector(camera, width, height) {
      var cosYaw = Math.cos(camera.yaw);
      var sinYaw = Math.sin(camera.yaw);
      var cosPitch = Math.cos(camera.pitch);
      var sinPitch = Math.sin(camera.pitch);
      var focal = (height * 0.5) / Math.tan(FOV / 2);
      var tx = camera.target.x;
      var ty = camera.target.y;
      var tz = camera.target.z;
      var dist = camera.dist;

      return function (point) {
        var dx = point.x - tx;
        var dy = point.y - ty;
        var dz = point.z - tz;
        var x1 = dx * cosYaw - dz * sinYaw;
        var z1 = dx * sinYaw + dz * cosYaw;
        var y1 = dy * cosPitch - z1 * sinPitch;
        var z2 = dy * sinPitch + z1 * cosPitch + dist;
        if (z2 < 16) return null;
        var scale = focal / z2;
        return {
          x: width / 2 + x1 * scale,
          y: height / 2 + y1 * scale,
          scale: scale,
          z: z2
        };
      };
    }

    function drawBackdrop(width, height) {
      var gradient = g2d.createRadialGradient(
        width * 0.5, height * 0.42, 12,
        width * 0.5, height * 0.5, Math.max(width, height) * 0.72
      );
      gradient.addColorStop(0, 'rgba(20,35,58,0.44)');
      gradient.addColorStop(0.55, 'rgba(9,16,28,0.16)');
      gradient.addColorStop(1, 'rgba(4,8,14,0.52)');
      g2d.fillStyle = gradient;
      g2d.fillRect(0, 0, width, height);

      for (var i = 0; i < stars.length; i += 1) {
        var star = stars[i];
        var twinkle = 0.72 + 0.28 * Math.sin(state.time * 1.4 + star.phase);
        g2d.globalAlpha = star.a * twinkle;
        g2d.fillStyle = '#c7dcff';
        g2d.fillRect(star.x, star.y, star.r, star.r);
      }
      g2d.globalAlpha = 1;
    }

    function drawEmpty(width, height) {
      g2d.save();
      g2d.textAlign = 'center';
      g2d.textBaseline = 'middle';
      g2d.fillStyle = 'rgba(148,163,184,0.9)';
      g2d.font = '600 14px ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif';
      g2d.fillText('No nodes match the current filters', width / 2, height / 2 - 8);
      g2d.fillStyle = 'rgba(100,116,139,0.85)';
      g2d.font = '400 11px ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif';
      g2d.fillText('Loosen the TYPES chips, scope or search to bring the constellation back.', width / 2, height / 2 + 12);
      g2d.restore();
    }

    function drawEdges(screenMap, focusSet) {
      var edges = state.edges;
      var cap = Math.min(edges.length, 6000);
      for (var i = 0; i < cap; i += 1) {
        var edge = edges[i];
        var from = screenMap.get(edge.from);
        var to = screenMap.get(edge.to);
        if (!from || !to) continue;
        var strong = !focusSet || (focusSet.has(edge.from) && focusSet.has(edge.to));
        var alpha = focusSet ? (strong ? 0.52 : 0.045) : 0.17;
        g2d.beginPath();
        g2d.moveTo(from.x, from.y);
        g2d.lineTo(to.x, to.y);
        g2d.strokeStyle = edgeColorOf(edge.kind);
        g2d.globalAlpha = alpha;
        g2d.lineWidth = strong && focusSet ? 1.35 : 0.75;
        g2d.stroke();
      }
      g2d.globalAlpha = 1;
    }

    function drawNodes(items, focusSet) {
      for (var i = 0; i < items.length; i += 1) {
        var item = items[i];
        var node = item.node;
        var dim = focusSet ? !focusSet.has(node.id) : false;
        var alpha = dim ? 0.16 : 1;
        var color = kindColorOf(node, ctx);
        var selected = node.id === state.selectedId;
        var hovered = node.id === state.hoverId;
        var hub = node.kind === 'project' || node.kind === 'agent' || toNum(node.degree, 0) >= state.maxDegree * 0.55;

        g2d.globalAlpha = alpha;

        if (hub) {
          var pulse = 0.16 + 0.06 * Math.sin(state.time * 1.8 + (hashString(node.id) % 100) / 15);
          g2d.beginPath();
          g2d.arc(item.x, item.y, item.r * 2.05, 0, TAU);
          g2d.strokeStyle = color;
          g2d.globalAlpha = alpha * pulse;
          g2d.lineWidth = 1.1;
          g2d.stroke();
          g2d.globalAlpha = alpha;
        }

        if (selected || hovered) {
          g2d.beginPath();
          g2d.arc(item.x, item.y, item.r + 4, 0, TAU);
          g2d.fillStyle = color;
          g2d.globalAlpha = alpha * 0.14;
          g2d.fill();
          g2d.globalAlpha = alpha;
        }

        g2d.beginPath();
        g2d.arc(item.x, item.y, item.r, 0, TAU);
        g2d.fillStyle = color;
        g2d.fill();

        g2d.beginPath();
        g2d.arc(item.x - item.r * 0.18, item.y - item.r * 0.2, Math.max(0.55, item.r * 0.38), 0, TAU);
        g2d.fillStyle = 'rgba(255,255,255,0.42)';
        g2d.fill();

        if (selected || hovered) {
          g2d.beginPath();
          g2d.arc(item.x, item.y, item.r + 2.8, 0, TAU);
          g2d.strokeStyle = selected ? '#f8fafc' : color;
          g2d.lineWidth = selected ? 1.7 : 1.3;
          g2d.stroke();
        }
      }
      g2d.globalAlpha = 1;
    }

    function drawLabels(items, focusSet) {
      g2d.save();
      g2d.font = '600 10.5px ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif';
      g2d.textAlign = 'left';
      g2d.textBaseline = 'middle';
      for (var i = 0; i < items.length; i += 1) {
        var item = items[i];
        var node = item.node;
        var selected = node.id === state.selectedId;
        var hovered = node.id === state.hoverId;
        if (!selected && !hovered && !state.topLabels.has(node.id)) continue;
        var dim = focusSet ? !focusSet.has(node.id) : false;
        g2d.globalAlpha = dim ? 0.3 : 0.94;
        g2d.fillStyle = selected || hovered ? '#f8fafc' : '#cbd5e1';
        var text = clampTextOf(node.label || node.id, 26);
        g2d.fillText(text, item.x + item.r + 5, item.y, 156);
      }
      g2d.restore();
      g2d.globalAlpha = 1;
    }

    function updateTooltip(item) {
      if (!tooltipEl) return;
      if (!item) {
        if (state.tooltipId !== null) {
          tooltipEl.classList.remove('is-visible');
          state.tooltipId = null;
        }
        return;
      }
      var node = item.node;
      if (state.tooltipId !== node.id) {
        var color = kindColorOf(node, ctx);
        var degree = toNum(node.degree, 0);
        var project = node.project ? basenameOf(node.project) : '';
        tooltipEl.innerHTML =
          '<div class="aikd3d-tip-title">' + escOf(node.label || node.id, ctx) + '</div>' +
          '<div class="aikd3d-tip-meta"><span class="aikd3d-dot" style="background:' + escOf(color, ctx) +
          ';color:' + escOf(color, ctx) + '"></span>' + escOf(node.kind || 'node', ctx) +
          (node.subtype ? ' · ' + escOf(node.subtype, ctx) : '') + '</div>' +
          '<div class="aikd3d-tip-meta">' + degree + ' links' +
          (project ? ' · ' + escOf(project, ctx) : '') + '</div>';
        state.tooltipId = node.id;
      }
      tooltipEl.classList.add('is-visible');

      var left = item.x + 14;
      var top = item.y + 12;
      var boxWidth = tooltipEl.offsetWidth || 220;
      var boxHeight = tooltipEl.offsetHeight || 60;
      if (left + boxWidth > state.width - 8) left = Math.max(8, item.x - boxWidth - 14);
      if (top + boxHeight > state.height - 8) top = Math.max(8, item.y - boxHeight - 12);
      tooltipEl.style.left = left + 'px';
      tooltipEl.style.top = top + 'px';
    }

    function draw() {
      if (!g2d || !state.width || !state.height) return;
      var width = state.width;
      var height = state.height;
      g2d.setTransform(state.dpr, 0, 0, state.dpr, 0, 0);
      g2d.clearRect(0, 0, width, height);
      drawBackdrop(width, height);

      if (!state.nodes.length) {
        state.screenItems = [];
        drawEmpty(width, height);
        updateTooltip(null);
        return;
      }

      var project = makeProjector(state.camera, width, height);
      var screenItems = [];
      var screenMap = new Map();
      for (var i = 0; i < state.nodes.length; i += 1) {
        var node = state.nodes[i];
        var point = state.cur.get(node.id);
        if (!point) continue;
        var projected = project(point);
        if (!projected) continue;
        var radius = clamp(worldRadius(node, state.maxDegree) * projected.scale, 1.4, 44);
        var item = {
          node: node,
          id: node.id,
          x: projected.x,
          y: projected.y,
          r: radius,
          z: projected.z
        };
        screenItems.push(item);
        screenMap.set(node.id, item);
      }
      state.screenItems = screenItems;

      var hovered = null;
      if (state.pointerInside && !state.drag) {
        hovered = pickAt(state.pointer);
      }
      state.hoverId = hovered ? hovered.node.id : null;

      var focusId = state.hoverId || state.selectedId;
      var focusSet = focusId ? focusSetFor(focusId) : null;

      screenItems.sort(function (a, b) { return b.z - a.z; });

      drawEdges(screenMap, focusSet);
      drawNodes(screenItems, focusSet);
      drawLabels(screenItems, focusSet);
      updateTooltip(hovered);
    }

    return {
      key: modeKey,
      mount: mount,
      update: update,
      focusNode: focusNode,
      destroy: destroy,
      resetCamera: resetCamera
    };
  }

  G.views['3d-galaxy'] = createSpatialView('3d-galaxy');
  G.views['3d-sphere'] = createSpatialView('3d-sphere');
  G.views['3d-orbital'] = createSpatialView('3d-orbital');
  G.views['3d-layers'] = createSpatialView('3d-layers');
})(window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} });
