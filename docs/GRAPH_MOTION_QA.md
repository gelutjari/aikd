# AIKD Graph — Motion & Interaction QA

Date: 2026-11-24T06:59:25.869Z  
Environment: headless Chrome CDP (`--headless=new`, 1440×900), real `Input.dispatchMouseEvent` / `Input.dispatchKeyEvent` events, offline embedded UI at `http://127.0.0.1:9090/ui?tab=graph`.

## Result

- **JS exceptions / console errors: 0** (`JS_ERRORS: []`)
- Console warnings captured: 0
- Benign browser network noise (filtered): 0 — `favicon.ico` 404 only
- Interactions executed: 43
- Views exercised: 2D Galaxy, 3D Force Galaxy, 3D Fibonacci Sphere, 3D Orbital Rings, 3D Semantic Layers, Treemap (2D + 3D City), Circle Packing, Sunburst, Semantic Islands, Miller Columns, Swimlane Timeline.
- View modules registered: 3d-galaxy, 3d-sphere, 3d-orbital, 3d-layers, treemap, packing, sunburst, islands, miller, timeline

## Recording

| Property | Value |
| :--- | :--- |
| Frames | 135 PNG (1440×900, HiDPI 1×) |
| Frame cadence | ~120 ms, encoded at 8 fps |
| Duration | 16.9 s |
| Wall-clock capture | 88.6 s |
| MP4 | `docs/media/aikd-graph-motion-qa.mp4` (0.7 MB) |
| GIF preview | `docs/media/aikd-graph-motion-qa.gif` (960 px wide, 2.6 MB) |

## Tested interactions

| # | At | Interaction |
| :--- | :--- | :--- |
| 1 | 11.4s | 2D Galaxy — hover center to spotlight one hop |
| 2 | 12.1s | 2D Galaxy — wheel zoom in at center |
| 3 | 13.9s | 2D Galaxy — drag pan across the stage |
| 4 | 16.1s | 2D Galaxy — switch preset to Code Map |
| 5 | 18.2s | 2D Galaxy — switch preset back to Knowledge Brain |
| 6 | 19.8s | 3d-galaxy — mount and morph |
| 7 | 21.8s | 3d-galaxy — smooth orbit drag |
| 8 | 24.4s | 3d-galaxy — wheel zoom |
| 9 | 25.8s | 3d-galaxy — Reset Camera |
| 10 | 27.3s | 3d-sphere — mount and morph |
| 11 | 29.2s | 3d-sphere — smooth orbit drag |
| 12 | 31.4s | 3d-sphere — wheel zoom |
| 13 | 32.4s | 3d-sphere — Reset Camera |
| 14 | 34.3s | 3d-orbital — mount and morph |
| 15 | 36.4s | 3d-orbital — smooth orbit drag |
| 16 | 38.7s | 3d-orbital — wheel zoom |
| 17 | 40.3s | 3d-orbital — Reset Camera |
| 18 | 42.2s | 3d-layers — mount and morph |
| 19 | 44.1s | 3d-layers — smooth orbit drag |
| 20 | 46.6s | 3d-layers — wheel zoom |
| 21 | 47.7s | 3d-layers — Reset Camera |
| 22 | 49.7s | treemap — mount and view 2D Treemap |
| 23 | 51.3s | treemap — switch sub-view to 3D City |
| 24 | 53.0s | treemap 3D City — orbit drag |
| 25 | 55.0s | treemap 3D City — click a building for backlink arcs |
| 26 | 56.7s | packing — mount |
| 27 | 58.6s | packing — click a cluster bubble to zoom in |
| 28 | 60.1s | packing — click background to zoom out |
| 29 | 61.4s | sunburst — mount |
| 30 | 63.3s | sunburst — click a ring slice to drill down |
| 31 | 64.7s | sunburst — click centre to go up one level |
| 32 | 66.1s | islands — mount |
| 33 | 67.9s | islands — drag pan |
| 34 | 70.7s | islands — hover an island and inspect |
| 35 | 73.5s | miller — mount |
| 36 | 75.4s | miller — pick a project |
| 37 | 76.2s | miller — pick a type |
| 38 | 77.1s | miller — pick an item and show Preview + Mini Local Graph |
| 39 | 79.6s | timeline — mount |
| 40 | 81.4s | timeline — group by Type |
| 41 | 82.7s | timeline — hover a node for causality arcs |
| 42 | 84.5s | timeline — click a node to pin arcs |
| 43 | 86.0s | timeline — back to Knowledge Brain 2D Galaxy |

### Coverage details

- **2D Galaxy:** hover spotlight, wheel zoom, drag pan, `Code Map` preset → `Knowledge Brain` preset round-trip.
- **3D Spatial:** each of `3d-galaxy`, `3d-sphere`, `3d-orbital`, `3d-layers` mounted with smooth morph capture, 15-step orbit drag, wheel zoom, and **Reset Camera**.
- **Treemap / 3D City:** `treemap` mounted, `2D Treemap` → `3D City` sub-toggle, orbit drag, building click for glowing backlink arcs.
- **Circle Packing:** cluster bubble click to zoom in, background click to zoom out.
- **Sunburst:** ring-slice click to drill down, centre click to go up one level.
- **Semantic Islands:** drag pan, hover + click island.
- **Miller Columns:** project → type → item navigation, Preview and Mini Local Graph rendered.
- **Swimlane Timeline:** group-by switched to Type, hovered nodes (causality arcs), clicked a node to pin selection.

## Error log

```json
[]
```

Benign network noise excluded from the count (missing browser favicon, not application code):

```json
[]
```

## Notes

- The recording is 100% offline: the UI and all graph modules are served from the embedded Rust binary (`/ui/graph-modules.js`), no CDN or external assets.
- Frames were captured through `Page.captureScreenshot` while real CDP input events were dispatched, then encoded with `ffmpeg` (`libx264`, `yuv420p`, `+faststart`).
- Temporary frames and the recording script were removed after encoding.
