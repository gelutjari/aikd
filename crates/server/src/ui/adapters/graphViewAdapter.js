/* ============================================================
   AIKD Knowledge Graph — shared graph view adapter
   Pure, deterministic, DOM-free transforms used by every view.
   Registers: window.AIKD_GRAPH.adapter
   ============================================================ */
(function (G) {
  'use strict';

  var GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
  var TAU = Math.PI * 2;
  var FALLBACK_GROUP = 'Unassigned / General';

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
  var PALETTE = ['#38bdf8', '#f472b6', '#fbbf24', '#34d399', '#c084fc', '#fb7185', '#60a5fa', '#2dd4bf'];
  var TYPE_ORDER = ['project', 'file', 'symbol', 'note', 'memory', 'tag', 'agent'];
  var LAYER_ORDER = ['project', 'agent', 'note', 'memory', 'tag', 'file', 'symbol'];

  /* ------------------------------------------------------------------ */
  /* Small deterministic utilities                                       */
  /* ------------------------------------------------------------------ */

  function toNum(value, fallback) {
    var n = Number(value);
    return Number.isFinite(n) ? n : (fallback || 0);
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

  function unitVector(seed) {
    var rand = mulberry32(seed >>> 0);
    var y = rand() * 2 - 1;
    var angle = rand() * TAU;
    var ring = Math.sqrt(Math.max(0, 1 - y * y));
    return { x: Math.cos(angle) * ring, y: y, z: Math.sin(angle) * ring };
  }

  function kindColor(kind) {
    return KIND_COLORS[String(kind || '').toLowerCase()] || KIND_COLORS.node;
  }

  function edgeColor(kind) {
    return EDGE_COLORS[String(kind || '').toLowerCase()] || EDGE_COLORS.link;
  }

  function basename(path) {
    var text = String(path === null || path === undefined ? '' : path).replace(/\\/g, '/');
    var parts = text.split('/').filter(Boolean);
    return parts.length ? parts[parts.length - 1] : text;
  }

  function clampText(value, max) {
    var text = String(value === null || value === undefined ? '' : value);
    var limit = Math.max(2, toNum(max, 36));
    if (text.length <= limit) return text;
    return text.slice(0, limit - 1).trim() + '…';
  }

  function esc(value) {
    if (value === null || value === undefined) return '';
    return String(value)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  function degreeOf(node) {
    return Math.max(0, toNum(node && node.degree, 0));
  }

  function compareIds(a, b) {
    var left = String(a.id || a.label || '');
    var right = String(b.id || b.label || '');
    return left < right ? -1 : (left > right ? 1 : 0);
  }

  function compareNodes(a, b) {
    var delta = degreeOf(b) - degreeOf(a);
    if (delta) return delta;
    return compareIds(a, b);
  }

  function projectKey(node) {
    if (!node) return '';
    if (node.project) return 'p:' + String(node.project);
    if (node.kind === 'project') return 'p:' + String(node.label || node.id || 'project');
    return '';
  }

  function typeRank(kind) {
    var index = TYPE_ORDER.indexOf(String(kind || '').toLowerCase());
    return index === -1 ? TYPE_ORDER.length : index;
  }

  function parseTime(value) {
    if (value === null || value === undefined || value === '') return null;
    if (typeof value === 'number' && Number.isFinite(value)) {
      if (value > 1e12) return value;
      if (value > 1e9) return value * 1000;
      return null;
    }
    var text = String(value).trim();
    if (!text) return null;
    if (/^\d+$/.test(text)) {
      var numeric = Number(text);
      if (numeric > 1e12) return numeric;
      if (numeric > 1e9) return numeric * 1000;
      return null;
    }
    var parsed = Date.parse(text);
    return Number.isFinite(parsed) ? parsed : null;
  }

  function nodeTime(node) {
    if (!node) return null;
    var created = parseTime(node.created_at);
    if (created !== null) return created;
    return parseTime(node.updated_at);
  }

  function formatDay(ms) {
    var months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
    var date = new Date(ms);
    if (!Number.isFinite(date.getTime())) return '';
    var day = date.getDate() < 10 ? '0' + date.getDate() : String(date.getDate());
    return months[date.getMonth()] + ' ' + day;
  }

  /* ------------------------------------------------------------------ */
  /* 3D spatial layouts                                                  */
  /* ------------------------------------------------------------------ */

  function relaxGalaxy(layout, nodes, edges, hubFor) {
    var node, point, hub, dx, dy, dz, dist, force;
    for (var pass = 0; pass < 22; pass += 1) {
      for (var edgeIndex = 0; edgeIndex < edges.length; edgeIndex += 1) {
        var a = layout.get(edges[edgeIndex].from);
        var b = layout.get(edges[edgeIndex].to);
        if (!a || !b) continue;
        dx = b.x - a.x;
        dy = b.y - a.y;
        dz = b.z - a.z;
        dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
        if (dist < 0.001 || dist > 900) continue;
        force = ((dist - 108) / dist) * 0.03;
        a.x += dx * force;
        a.y += dy * force;
        a.z += dz * force;
        b.x -= dx * force;
        b.y -= dy * force;
        b.z -= dz * force;
      }
      for (var nodeIndex = 0; nodeIndex < nodes.length; nodeIndex += 1) {
        node = nodes[nodeIndex];
        point = layout.get(node.id);
        if (!point) continue;
        hub = hubFor ? hubFor(node) : null;
        if (hub) {
          point.x += (hub.x - point.x) * 0.018;
          point.y += (hub.y - point.y) * 0.018;
          point.z += (hub.z - point.z) * 0.018;
        }
      }
    }
    var maxRadius = 0;
    layout.forEach(function (p) {
      maxRadius = Math.max(maxRadius, Math.sqrt(p.x * p.x + p.y * p.y + p.z * p.z));
    });
    if (maxRadius > 560) {
      var scale = 560 / maxRadius;
      layout.forEach(function (p) {
        p.x *= scale;
        p.y *= scale;
        p.z *= scale;
      });
    }
  }

  function galaxyLayout(nodes, edges) {
    var layout = new Map();
    if (!nodes.length) return layout;

    var groups = new Map();
    nodes.forEach(function (node) {
      var key = projectKey(node);
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(node);
    });

    var projectKeys = Array.from(groups.keys()).filter(Boolean).sort();
    var hubs = new Map();
    var spread = 430;
    projectKeys.forEach(function (key, index) {
      var y = projectKeys.length === 1 ? 0 : 1 - (index / (projectKeys.length - 1)) * 2;
      var ring = Math.sqrt(Math.max(0, 1 - y * y));
      var angle = GOLDEN_ANGLE * index + (hashString(key) % 720) / 720 * Math.PI;
      hubs.set(key, {
        x: Math.cos(angle) * ring * spread * 0.6,
        y: y * spread * 0.42,
        z: Math.sin(angle) * ring * spread * 0.6
      });
    });

    var orphans = (groups.get('') || []).slice().sort(compareNodes);
    orphans.forEach(function (node, index) {
      var t = orphans.length === 1 ? 0.5 : index / (orphans.length - 1);
      var y = 1 - t * 2;
      var ring = Math.sqrt(Math.max(0, 1 - y * y));
      var angle = GOLDEN_ANGLE * index + 0.4;
      layout.set(node.id, {
        x: Math.cos(angle) * ring * spread * 1.04,
        y: y * spread * 0.96,
        z: Math.sin(angle) * ring * spread * 1.04
      });
    });

    groups.forEach(function (members, key) {
      if (!key) return;
      var hub = hubs.get(key) || { x: 0, y: 0, z: 0 };
      members.slice().sort(compareNodes).forEach(function (node, index) {
        var direction = unitVector(hashString(node.id + '#galaxy'));
        var radius = 30 + Math.sqrt(index + 1) * 15 + (hashString(node.id + '#r') % 100) / 100 * 16;
        if (node.kind === 'project') radius *= 0.08;
        layout.set(node.id, {
          x: hub.x + direction.x * radius,
          y: hub.y + direction.y * radius * 0.7,
          z: hub.z + direction.z * radius
        });
      });
    });

    relaxGalaxy(layout, nodes, edges, function (node) {
      if (!node.project) return null;
      return hubs.get(projectKey(node)) || null;
    });
    return layout;
  }

  function sphereLayout(nodes) {
    var layout = new Map();
    if (!nodes.length) return layout;

    var sorted = nodes.slice().sort(function (a, b) {
      var keyA = projectKey(a);
      var keyB = projectKey(b);
      if (keyA !== keyB) return keyA < keyB ? -1 : 1;
      var rankA = typeRank(a.kind);
      var rankB = typeRank(b.kind);
      if (rankA !== rankB) return rankA - rankB;
      return compareNodes(a, b);
    });

    var radius = 340;
    var phases = new Map();
    var total = sorted.length;
    sorted.forEach(function (node, index) {
      var key = projectKey(node);
      if (!phases.has(key)) phases.set(key, (hashString(key) % 360) / 360 * TAU);
      var t = total === 1 ? 0.5 : index / (total - 1);
      var y = 1 - t * 2;
      var ring = Math.sqrt(Math.max(0, 1 - y * y));
      var angle = index * GOLDEN_ANGLE + phases.get(key);
      var jitter = 0.94 + (hashString(node.id + '#sphere') % 60) / 1000;
      layout.set(node.id, {
        x: Math.cos(angle) * ring * radius * jitter,
        y: y * radius,
        z: Math.sin(angle) * ring * radius * jitter
      });
    });
    return layout;
  }

  function orbitalLayout(nodes) {
    var layout = new Map();
    if (!nodes.length) return layout;

    var hubs = nodes.filter(function (node) {
      return node.kind === 'project' || node.kind === 'agent';
    }).slice().sort(compareNodes);
    if (!hubs.length) hubs = nodes.slice().sort(compareNodes).slice(0, 3);
    if (!hubs.length) return layout;

    var hubIndexByKey = new Map();
    hubs.forEach(function (hub, index) {
      if (hub.project) hubIndexByKey.set(hub.project, index);
      hubIndexByKey.set(hub.label, index);
      hubIndexByKey.set(hub.id, index);
    });

    hubs.forEach(function (hub, index) {
      var direction = unitVector(hashString(hub.id + '#core'));
      var radius = 34 + 52 * Math.sqrt((index + 0.5) / hubs.length);
      layout.set(hub.id, {
        x: direction.x * radius,
        y: direction.y * radius * 0.6,
        z: direction.z * radius
      });
    });

    var derivedByHub = new Map();
    nodes.forEach(function (node) {
      if (layout.has(node.id)) return;
      var hubIndex = -1;
      if (node.project && hubIndexByKey.has(node.project)) hubIndex = hubIndexByKey.get(node.project);
      if (hubIndex < 0 && node.kind === 'project' && hubIndexByKey.has(node.label)) hubIndex = hubIndexByKey.get(node.label);
      if (hubIndex < 0) hubIndex = hashString(node.id + '#orbit') % hubs.length;
      if (!derivedByHub.has(hubIndex)) derivedByHub.set(hubIndex, []);
      derivedByHub.get(hubIndex).push(node);
    });

    var elevations = [0.62, 0.02, -0.58];
    derivedByHub.forEach(function (members, hubIndex) {
      members.sort(compareNodes);
      var ringIndex = hubIndex % 3;
      var ringRadius = 150 + ringIndex * 74;
      var elevation = elevations[ringIndex] + 0.1 * Math.sin(hubIndex * 1.7);
      var tilt = hubIndex * 0.9;
      var horizontal = Math.cos(elevation);
      var vertical = Math.sin(elevation);
      members.forEach(function (node, index) {
        var angle = tilt + (index + 0.5) / members.length * TAU;
        var radius = ringRadius * (0.88 + (hashString(node.id + '#ring') % 120) / 1000);
        layout.set(node.id, {
          x: Math.cos(angle) * radius * horizontal,
          y: vertical * radius,
          z: Math.sin(angle) * radius * horizontal
        });
      });
    });
    return layout;
  }

  function layersLayout(nodes) {
    var layout = new Map();
    if (!nodes.length) return layout;

    var byKind = new Map();
    nodes.forEach(function (node) {
      var kind = String(node.kind || 'node').toLowerCase();
      if (!byKind.has(kind)) byKind.set(kind, []);
      byKind.get(kind).push(node);
    });

    var kinds = LAYER_ORDER.filter(function (kind) { return byKind.has(kind); });
    if (kinds.length < byKind.size) {
      var extra = Array.from(byKind.keys()).filter(function (kind) {
        return LAYER_ORDER.indexOf(kind) === -1;
      }).sort();
      kinds = kinds.concat(extra);
    }

    var step = kinds.length > 1 ? Math.min(150, 900 / (kinds.length - 1)) : 0;
    var top = (kinds.length - 1) * step / 2;
    kinds.forEach(function (kind, layer) {
      var y = top - layer * step;
      byKind.get(kind).slice().sort(compareNodes).forEach(function (node, index) {
        var angle = index * GOLDEN_ANGLE + layer * 0.4;
        var radius = Math.min(430, 22 + Math.sqrt(index + 1) * 36);
        layout.set(node.id, {
          x: Math.cos(angle) * radius,
          y: y,
          z: Math.sin(angle) * radius
        });
      });
    });
    return layout;
  }

  function toSpatialGraph(nodes, edges) {
    var list = Array.isArray(nodes) ? nodes : [];
    var links = Array.isArray(edges) ? edges : [];
    var adj = new Map();
    var degreeMap = new Map();
    var validEdges = [];

    list.forEach(function (node) {
      adj.set(node.id, []);
      degreeMap.set(node.id, 0);
    });

    links.forEach(function (edge) {
      if (!edge || !adj.has(edge.from) || !adj.has(edge.to) || edge.from === edge.to) return;
      adj.get(edge.from).push(edge.to);
      adj.get(edge.to).push(edge.from);
      degreeMap.set(edge.from, degreeMap.get(edge.from) + 1);
      degreeMap.set(edge.to, degreeMap.get(edge.to) + 1);
      validEdges.push(edge);
    });

    list.forEach(function (node) {
      degreeMap.set(node.id, Math.max(degreeMap.get(node.id) || 0, degreeOf(node)));
    });

    var maxDegree = 1;
    degreeMap.forEach(function (value) {
      if (value > maxDegree) maxDegree = value;
    });

    return {
      nodes: list,
      edges: validEdges,
      adj: adj,
      degreeMap: degreeMap,
      maxDegree: maxDegree,
      layouts: {
        galaxy3d: galaxyLayout(list, validEdges),
        sphere3d: sphereLayout(list),
        orbital3d: orbitalLayout(list),
        layers3d: layersLayout(list)
      }
    };
  }

  /* ------------------------------------------------------------------ */
  /* Hierarchy tree (Project -> Type -> Item)                            */
  /* ------------------------------------------------------------------ */

  function toHierarchyTree(nodes, edges) {
    var list = Array.isArray(nodes) ? nodes : [];
    var groups = new Map();

    list.forEach(function (node) {
      var key = projectKey(node) || '';
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(node);
    });

    var hasOrphans = (groups.get('') || []).length > 0;
    if (!hasOrphans && groups.size === 0) groups.set('', []);

    var projects = [];
    groups.forEach(function (members, key) {
      var projectName = key ? key.slice(2) : FALLBACK_GROUP;
      var typeGroups = new Map();
      members.forEach(function (node) {
        var kind = String(node.kind || 'node').toLowerCase();
        if (!typeGroups.has(kind)) typeGroups.set(kind, []);
        typeGroups.get(kind).push(node);
      });

      var children = Array.from(typeGroups.keys())
        .sort(function (a, b) { return typeRank(a) - typeRank(b) || a.localeCompare(b); })
        .map(function (kind) {
          var items = typeGroups.get(kind).slice().sort(compareNodes);
          return {
            name: kind,
            id: 'type:' + projectName + ':' + kind,
            kind: kind,
            value: items.length,
            children: items.map(function (node) {
              return {
                name: node.label || node.id,
                id: node.id,
                kind: String(node.kind || 'node').toLowerCase(),
                value: Math.max(1, degreeOf(node)),
                degree: degreeOf(node),
                node: node
              };
            })
          };
        });

      projects.push({
        name: projectName === FALLBACK_GROUP ? FALLBACK_GROUP : basename(projectName),
        id: 'project:' + (key || FALLBACK_GROUP),
        kind: 'project',
        value: members.length,
        project: key ? key.slice(2) : '',
        children: children
      });
    });

    projects.sort(function (a, b) {
      var delta = b.value - a.value;
      if (delta) return delta;
      return a.name < b.name ? -1 : (a.name > b.name ? 1 : 0);
    });

    return {
      name: 'AIKD',
      id: 'root:AIKD',
      kind: 'root',
      value: list.length,
      children: projects
    };
  }

  /* ------------------------------------------------------------------ */
  /* Semantic islands                                                    */
  /* ------------------------------------------------------------------ */

  function clusterKey(node) {
    var project = projectKey(node);
    if (project) return project;
    var match = String(node.label || '').match(/#([\w.-]+)/);
    if (match) return 't:' + match[1].toLowerCase();
    return '';
  }

  function toSemanticIslands(nodes, edges) {
    var list = Array.isArray(nodes) ? nodes : [];
    var links = Array.isArray(edges) ? edges : [];
    var nodeCoords = new Map();
    if (!list.length) return { islands: [], nodeCoords: nodeCoords };

    var nodeMap = new Map();
    list.forEach(function (node) { nodeMap.set(node.id, node); });

    var clusters = new Map();
    var keyById = new Map();
    list.forEach(function (node) {
      var key = clusterKey(node) || '*';
      keyById.set(node.id, key);
      if (!clusters.has(key)) clusters.set(key, []);
      clusters.get(key).push(node);
    });

    var crossLinks = new Map();
    links.forEach(function (edge) {
      var fromKey = keyById.get(edge.from);
      var toKey = keyById.get(edge.to);
      if (!fromKey || !toKey || fromKey === toKey) return;
      var pair = fromKey < toKey ? fromKey + '\u0000' + toKey : toKey + '\u0000' + fromKey;
      crossLinks.set(pair, (crossLinks.get(pair) || 0) + 1);
    });

    var mergedInto = new Map();
    clusters.forEach(function (members, key) {
      if (key === '*' || members.length > 2) return;
      var bestKey = '';
      var bestWeight = 0;
      clusters.forEach(function (otherMembers, otherKey) {
        if (otherKey === key) return;
        var a = key < otherKey ? key : otherKey;
        var b = key < otherKey ? otherKey : key;
        var weight = crossLinks.get(a + '\u0000' + b) || 0;
        if (weight > bestWeight) { bestWeight = weight; bestKey = otherKey; }
      });
      if (bestKey && bestWeight > 0) mergedInto.set(key, bestKey);
    });

    mergedInto.forEach(function (target, source) {
      var sourceMembers = clusters.get(source) || [];
      if (!clusters.has(target)) clusters.set(target, []);
      sourceMembers.forEach(function (node) {
        clusters.get(target).push(node);
        keyById.set(node.id, target);
      });
      clusters.delete(source);
    });

    var entries = Array.from(clusters.entries()).map(function (entry) {
      return { key: entry[0], nodes: entry[1] };
    }).filter(function (entry) {
      return entry.nodes.length > 0;
    });
    entries.sort(function (a, b) {
      var delta = b.nodes.length - a.nodes.length;
      if (delta) return delta;
      return a.key < b.key ? -1 : (a.key > b.key ? 1 : 0);
    });

    var total = list.length;
    var count = entries.length;
    var spread = 120 + 22 * Math.sqrt(total);
    var centers = entries.map(function (entry, index) {
      if (count === 1) return { x: 0, y: 0 };
      var angle = index * GOLDEN_ANGLE + 0.7;
      var distance = spread * Math.sqrt((index + 0.7) / count);
      return { x: Math.cos(angle) * distance, y: Math.sin(angle) * distance * 0.86 };
    });

    var radii = entries.map(function (entry) {
      return 46 + 11 * Math.sqrt(entry.nodes.length);
    });

    for (var pass = 0; pass < 70; pass += 1) {
      for (var i = 0; i < centers.length; i += 1) {
        for (var j = i + 1; j < centers.length; j += 1) {
          var dx = centers[j].x - centers[i].x;
          var dy = centers[j].y - centers[i].y;
          var dist = Math.sqrt(dx * dx + dy * dy) || 0.001;
          var minDist = radii[i] + radii[j] + 26;
          if (dist < minDist) {
            var push = (minDist - dist) / dist * 0.5;
            centers[i].x -= dx * push;
            centers[i].y -= dy * push;
            centers[j].x += dx * push;
            centers[j].y += dy * push;
          }
        }
      }
    }

    var islands = entries.map(function (entry, index) {
      var members = entry.nodes;
      var center = centers[index];
      var reach = radii[index];
      var rand = mulberry32(hashString(entry.key));
      var placed = [];
      var phase = rand() * TAU;
      members.slice().sort(compareNodes).forEach(function (node, itemIndex) {
        var t = members.length === 1 ? 0 : Math.sqrt(itemIndex / Math.max(1, members.length - 1));
        var angle = itemIndex * GOLDEN_ANGLE + phase;
        var distance = reach * 0.68 * t;
        var x = center.x + Math.cos(angle) * distance;
        var y = center.y + Math.sin(angle) * distance * 0.78;
        placed.push({ node: node, x: x, y: y });
        nodeCoords.set(node.id, { x: x, y: y });
      });

      var minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
      placed.forEach(function (point) {
        minX = Math.min(minX, point.x);
        maxX = Math.max(maxX, point.x);
        minY = Math.min(minY, point.y);
        maxY = Math.max(maxY, point.y);
      });
      var cx = (minX + maxX) / 2;
      var cy = (minY + maxY) / 2;
      var rx = Math.max(38, (maxX - minX) / 2 + 40);
      var ry = Math.max(30, (maxY - minY) / 2 + 32);

      var wobble = rand() * TAU;
      var points = [];
      for (var step = 0; step < 28; step += 1) {
        var theta = step / 28 * TAU;
        var swell = 1 + 0.06 * Math.sin(theta * 3 + wobble) + 0.035 * Math.sin(theta * 5 + wobble * 1.7);
        points.push({
          x: cx + Math.cos(theta) * rx * swell,
          y: cy + Math.sin(theta) * ry * swell
        });
      }

      var label;
      if (entry.key.indexOf('p:') === 0) label = basename(entry.key.slice(2));
      else if (entry.key.indexOf('t:') === 0) label = '#' + entry.key.slice(2);
      else label = FALLBACK_GROUP;

      return {
        id: entry.key,
        label: label,
        color: PALETTE[hashString(entry.key) % PALETTE.length],
        cx: cx,
        cy: cy,
        rx: rx,
        ry: ry,
        points: points,
        nodes: members
      };
    });

    return { islands: islands, nodeCoords: nodeCoords };
  }

  /* ------------------------------------------------------------------ */
  /* Swimlane timeline                                                   */
  /* ------------------------------------------------------------------ */

  function laneKey(node, groupBy) {
    if (groupBy === 'kind') return String(node.kind || 'node');
    if (groupBy === 'agent') {
      return String(
        node.agent ||
        (node.kind === 'agent' ? node.label : '') ||
        node.provenance ||
        FALLBACK_GROUP
      );
    }
    var project = node.project || (node.kind === 'project' ? node.label : '');
    return project ? String(project) : FALLBACK_GROUP;
  }

  function laneLabel(key, groupBy) {
    if (groupBy === 'project' && key !== FALLBACK_GROUP) return basename(key);
    return key;
  }

  function toTimelineLanes(nodes, edges, groupBy) {
    var list = Array.isArray(nodes) ? nodes : [];
    var mode = groupBy === 'kind' || groupBy === 'agent' ? groupBy : 'project';
    var lanesByKey = new Map();

    list.forEach(function (node) {
      var key = laneKey(node, mode);
      if (!lanesByKey.has(key)) lanesByKey.set(key, []);
      lanesByKey.get(key).push(node);
    });

    var timed = [];
    var hasTimestamps = false;
    lanesByKey.forEach(function (members) {
      members.forEach(function (node) {
        var time = nodeTime(node);
        if (time !== null) {
          hasTimestamps = true;
          timed.push(time);
        }
      });
    });

    var minTime = Infinity;
    var maxTime = -Infinity;
    timed.forEach(function (time) {
      minTime = Math.min(minTime, time);
      maxTime = Math.max(maxTime, time);
    });
    var span = maxTime - minTime;

    var lanes = [];
    lanesByKey.forEach(function (members, key) {
      var sorted = members.slice().sort(function (a, b) {
        if (hasTimestamps) {
          var timeA = nodeTime(a);
          var timeB = nodeTime(b);
          if (timeA !== null && timeB !== null && timeA !== timeB) return timeA - timeB;
          if (timeA !== null && timeB === null) return -1;
          if (timeA === null && timeB !== null) return 1;
          if (timeA === null && timeB === null) return compareIds(a, b);
          return compareIds(a, b);
        }
        return compareIds(a, b);
      });

      var items = sorted.map(function (node, index) {
        var time = nodeTime(node);
        var xNorm;
        if (time !== null && hasTimestamps && span > 0) {
          xNorm = Math.max(0, Math.min(1, (time - minTime) / span));
        } else if (sorted.length > 1) {
          xNorm = index / (sorted.length - 1);
        } else {
          xNorm = 0.5;
        }
        return {
          node: node,
          xNorm: xNorm,
          tsLabel: time !== null ? formatDay(time) : '#' + (index + 1),
          idx: index
        };
      });

      lanes.push({
        id: key,
        label: laneLabel(key, mode),
        color: PALETTE[hashString(key) % PALETTE.length],
        items: items
      });
    });

    lanes.sort(function (a, b) {
      var delta = b.items.length - a.items.length;
      if (delta) return delta;
      return a.label < b.label ? -1 : (a.label > b.label ? 1 : 0);
    });

    return { lanes: lanes, hasTimestamps: hasTimestamps };
  }

  /* ------------------------------------------------------------------ */

  G.adapter = {
    version: '1.0.0',
    FALLBACK_GROUP: FALLBACK_GROUP,
    LAYOUT_KEYS: {
      '3d-galaxy': 'galaxy3d',
      '3d-sphere': 'sphere3d',
      '3d-orbital': 'orbital3d',
      '3d-layers': 'layers3d'
    },
    KIND_COLORS: KIND_COLORS,
    PALETTE: PALETTE,
    kindColor: kindColor,
    edgeColor: edgeColor,
    basename: basename,
    clampText: clampText,
    esc: esc,
    toNum: toNum,
    hashString: hashString,
    mulberry32: mulberry32,
    degreeOf: degreeOf,
    toSpatialGraph: toSpatialGraph,
    toHierarchyTree: toHierarchyTree,
    toSemanticIslands: toSemanticIslands,
    toTimelineLanes: toTimelineLanes
  };
})(window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} });
