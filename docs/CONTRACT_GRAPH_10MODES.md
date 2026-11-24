# AIKD Knowledge Graph 10-Mode Upgrade Contract (`CONTRACT_GRAPH_10MODES.md`)

> **Stack:** Pure offline Vanilla JS (no React/npm/CDN), embedded into Rust `aikd-server` (`crates/server/src/rest.rs`) and mounted inside `crates/server/src/ui.html`.
> **Rule:** Preserve 100% of existing features (`Knowledge Brain` / `Code Map` / `All`, `Global` / `Local`, `DEPTH`, `Hide orphans`, `Find node...`, `TYPES` filter chips & counters, Inspector/Backlinks panel, 2D Galaxy & Hierarchy Tree).

---

## 1. File Structure & Strict Worker Ownership

| Worker | Owned Files |
| :--- | :--- |
| `worker-1` (`t27`) | `crates/server/src/ui/adapters/graphViewAdapter.js`, `crates/server/src/ui/components/ViewModeSwitcher.js`, `crates/server/src/ui/views/SpatialGraph3DView.js` |
| `worker-2` (`t28`) | `crates/server/src/ui/views/CityTreemapView.js`, `crates/server/src/ui/views/CirclePackingView.js`, `crates/server/src/ui/views/SunburstView.js` |
| `worker-3` (`t29`) | `crates/server/src/ui/views/SemanticIslandsView.js`, `crates/server/src/ui/views/MillerColumnsView.js`, `crates/server/src/ui/views/SwimlaneTimelineView.js` |
| `worker-4` (`t2b`) | `crates/server/src/rest.rs`, `crates/server/src/ui.html` |

---

## 2. Shared Global Namespace (`window.AIKD_GRAPH`)

Every module registers onto `window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} };` (wrapped in an IIFE `(function (G) { ... })(window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} });`).

### 2.1 Shared Context Passed by `ui.html` to Views (`ctx`)
```js
{
  container: HTMLElement,     // #graph-alt-stage (or #graph-stage host)
  nodes: Array<Node>,         // filtered nodes from state.graph.nodes
  edges: Array<Edge>,         // filtered edges from state.graph.edges
  rawNodes: Array<Node>,      // unfiltered nodes for reference
  rawEdges: Array<Edge>,      // unfiltered edges for backlinks
  selectedId: string | null,  // state.graph.selected
  depth: number,              // state.graph.depth (1 or 2)
  mode: string,               // 'global' | 'local'
  helpers: {
    kindColor(kind),
    graphLabel(node),
    basename(path),
    clampText(s, n),
    esc(s),
    onSelectNode(node),       // triggers selectGraphNode(node) & syncs Inspector/Backlinks
    onFocusLocal(nodeId),     // triggers focusLocal(nodeId)
  }
}
```
Each `Node` has: `{ id, kind, subtype, label, project, scope, provenance, degree, updated_at?, created_at? }`.
Each `Edge` has: `{ from, to, kind, provenance }`.

### 2.2 `graphViewAdapter` (`window.AIKD_GRAPH.adapter`)
Pure utility functions (zero DOM side effects):
- `toSpatialGraph(nodes, edges)` → `{ nodes, edges, adj, degreeMap, maxDegree, layouts: { galaxy3d: Map<id,{x,y,z}>, sphere3d: Map<id,{x,y,z}>, orbital3d: Map<id,{x,y,z}>, layers3d: Map<id,{x,y,z}> } }`
  - `galaxy3d`: deterministic seeded/force 3D layout around project hubs
  - `sphere3d`: Golden Spiral Fibonacci sphere (`y = 1 - (i/(n-1))*2`, grouped by project/kind angle)
  - `orbital3d`: hubs (`project`/`agent`) at inner core, derived nodes (`memory`, `file`, `note`, `symbol`, `tag`) on concentric 3D rings with distinct elevation angles
  - `layers3d`: horizontal Y planes separated by `kind` (`project` top -> `agent` -> `note` -> `memory` -> `tag` -> `file` -> `symbol` bottom)
- `toHierarchyTree(nodes, edges)` → `{ name: 'AIKD', kind: 'root', value, children: [ { name, id, kind, value, project, children: [ { name: typeName, kind, value, children: [ { name, id, kind, value, degree, node } ] } ] } ] }`
  - Always provides fallback group `"Unassigned / General"` for orphan/global nodes without a project!
- `toSemanticIslands(nodes, edges)` → `{ islands: [ { id, label, color, cx, cy, rx, ry, points, nodes } ], nodeCoords: Map<id,{x,y}> }`
  - Clusters by project + shared `#tag`/neighbor Jaccard similarity; computes 2D island hull/contour & dominant topic label.
- `toTimelineLanes(nodes, edges, groupBy)` (`groupBy`: `'project' | 'kind' | 'agent'`) → `{ lanes: [ { id, label, color, items: [ { node, xNorm, tsLabel, idx } ] } ], hasTimestamps: bool }`
  - Uses `created_at` / `updated_at` if present, gracefully falling back to index/ID ordering with sequence indicator.

### 2.3 `ViewModeSwitcher` (`window.AIKD_GRAPH.ViewModeSwitcher`)
- `mount(hostEl, activeMode, onChange)` → renders dark neon pill segmented control with 2 clean groups:
  - **2D & Structured:** `galaxy` (`2D Galaxy`), `tree` (`Hierarchy Tree`), `treemap` (`Treemap / 3D City`), `packing` (`Circle Packing`), `sunburst` (`Sunburst`), `islands` (`Semantic Islands`), `miller` (`Miller Columns`), `timeline` (`Swimlane Timeline`)
  - **3D Spatial:** `3d-galaxy` (`3D Force Galaxy`), `3d-sphere` (`3D Fibonacci Sphere`), `3d-orbital` (`3D Orbital Rings`), `3d-layers` (`3D Semantic Layers`)
- `setActive(mode)` → updates active pill highlight.

### 2.4 View Interface (`window.AIKD_GRAPH.views[key]`)
Every view module registers an object implementing:
```js
{
  mount(ctx),          // initializes DOM/Canvas, observers, animation loop
  update(ctx),         // updates data/selection/depth without tearing down camera if same mode
  focusNode(nodeId),   // smoothly flies/zooms/scrolls to nodeId (called by Find node... Enter)
  destroy(),           // REQUIRED: cancels rAF, disconnects ResizeObserver, removes listeners, clears container
}
```
Keys:
- `SpatialGraph3DView`: registers `'3d-galaxy'`, `'3d-sphere'`, `'3d-orbital'`, `'3d-layers'` (supports smooth lerp morphing when switching between the 4 3D modes, orbit drag, pan right/Shift+drag, wheel zoom, Reset Camera button, auto-fly on `focusNode`, HiDPI Canvas 3D projection at 60 FPS with zero external deps!).
- `CityTreemapView`: registers `'treemap'` (with `[2D Treemap | 3D City]` sub-toggle; clicking a 3D City building highlights it + draws glowing 3D arcs to all its backlinks!).
- `CirclePackingView`: registers `'packing'` (zoomable nested bubbles with breadcrumb/click-outside zoom-out).
- `SunburstView`: registers `'sunburst'` (concentric radial rings with % proportion + click slice to drill down + click center to go up 1 level).
- `SemanticIslandsView`: registers `'islands'` (2D topographic glowing islands with cluster labels, HiDPI pan/zoom, hover tooltip).
- `MillerColumnsView`: registers `'miller'` (4-column Finder/Notion browser: Projects → Types → Sorted Items → Preview/Backlinks + interactive Mini Local Graph Depth 1).
- `SwimlaneTimelineView`: registers `'timeline'` (`Group by: Project | Type | Agent` selector, horizontal lanes, bezier causality arcs shown on hover/select).
