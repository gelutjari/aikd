# AIKD Graph — Motion & Interaction QA

Date: 2026-11-24T07:11:02.751Z  
Environment: headless Chrome CDP (`--headless=new --disable-gpu`, 1440×900), real `Input.dispatchMouseEvent` / `Input.dispatchKeyEvent` events, offline embedded UI at `http://127.0.0.1:9090/ui?tab=graph`.

## Result

- **JS exceptions / console errors: 0** (`JS_ERRORS: []`)
- Console warnings captured: 0
- Benign browser network noise (filtered, not application code): 1 — missing `favicon.ico` only
- Interactions executed: 44
- Graph loaded: 220 nodes / 212 edges (Knowledge Brain preset)
- Views exercised: 2D Galaxy, 3D Force Galaxy, 3D Fibonacci Sphere, 3D Orbital Rings, 3D Semantic Layers, Treemap (2D + 3D City), Circle Packing, Sunburst, Semantic Islands, Miller Columns, Swimlane Timeline.
- View modules registered: 3d-galaxy, 3d-sphere, 3d-orbital, 3d-layers, treemap, packing, sunburst, islands, miller, timeline

## Recording

| Property | Value |
| :--- | :--- |
| Frames | 404 PNG (1440×900, 1× DPR) |
| Capture target | ~120 ms between `Page.captureScreenshot` calls (headless PNG readback is the limiter) |
| Encode rate | 8 fps |
| Video duration | 50.5 s |
| Wall-clock capture | 85.2 s |
| MP4 | `docs/media/aikd-graph-motion-qa.mp4` (3.1 MB) |
| GIF preview | `docs/media/aikd-graph-motion-qa.gif` (960 px wide, 8.0 MB) |

## Tested interactions

| # | At | Interaction |
| :--- | :--- | :--- |
| 1 | 6.7s | 2D Galaxy — fit graph to viewport |
| 2 | 8.3s | 2D Galaxy — hover center to spotlight one hop |
| 3 | 9.1s | 2D Galaxy — wheel zoom in at center |
| 4 | 10.6s | 2D Galaxy — drag pan across the stage |
| 5 | 12.8s | 2D Galaxy — switch preset to Code Map |
| 6 | 18.3s | 2D Galaxy — switch preset back to Knowledge Brain |
| 7 | 23.9s | 3d-galaxy — mount and morph |
| 8 | 25.5s | 3d-galaxy — smooth orbit drag |
| 9 | 27.5s | 3d-galaxy — wheel zoom |
| 10 | 28.6s | 3d-galaxy — Reset Camera |
| 11 | 30.2s | 3d-sphere — mount and morph |
| 12 | 31.8s | 3d-sphere — smooth orbit drag |
| 13 | 33.9s | 3d-sphere — wheel zoom |
| 14 | 35.1s | 3d-sphere — Reset Camera |
| 15 | 36.6s | 3d-orbital — mount and morph |
| 16 | 38.3s | 3d-orbital — smooth orbit drag |
| 17 | 40.1s | 3d-orbital — wheel zoom |
| 18 | 41.3s | 3d-orbital — Reset Camera |
| 19 | 42.8s | 3d-layers — mount and morph |
| 20 | 44.6s | 3d-layers — smooth orbit drag |
| 21 | 46.7s | 3d-layers — wheel zoom |
| 22 | 48.0s | 3d-layers — Reset Camera |
| 23 | 49.5s | treemap — mount and view 2D Treemap |
| 24 | 51.1s | treemap — switch sub-view to 3D City |
| 25 | 52.7s | treemap 3D City — orbit drag |
| 26 | 54.4s | treemap 3D City — click a building for backlink arcs |
| 27 | 56.1s | packing — mount |
| 28 | 57.7s | packing — click a cluster bubble to zoom in |
| 29 | 59.0s | packing — click background to zoom out |
| 30 | 60.2s | sunburst — mount |
| 31 | 61.8s | sunburst — click a ring slice to drill down |
| 32 | 63.1s | sunburst — click centre to go up one level |
| 33 | 64.3s | islands — mount |
| 34 | 65.9s | islands — drag pan |
| 35 | 67.9s | islands — hover an island and inspect |
| 36 | 70.4s | miller — mount |
| 37 | 72.2s | miller — pick a project |
| 38 | 72.9s | miller — pick a type |
| 39 | 73.7s | miller — pick an item and show Preview + Mini Local Graph |
| 40 | 76.1s | timeline — mount |
| 41 | 77.6s | timeline — group by Type |
| 42 | 79.0s | timeline — hover a node for causality arcs |
| 43 | 80.7s | timeline — click a node to pin arcs |
| 44 | 82.2s | timeline — back to Knowledge Brain 2D Galaxy |

### Coverage details

- **2D Galaxy:** hover spotlight, wheel zoom, drag pan, `Code Map` preset → `Knowledge Brain` preset round-trip (both loads confirmed complete before proceeding).
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

Benign network noise excluded from the count:

```json
[
  {
    "source": "log.network",
    "text": "Failed to load resource: the server responded with a status of 404 (Not Found)",
    "url": "http://127.0.0.1:9090/favicon.ico"
  }
]
```

## Notes

- The recording is 100% offline: the UI and all graph modules are served from the embedded Rust binary (`/ui/graph-modules.js`), with no CDN or external assets.
- Frames were captured through `Page.captureScreenshot` while real CDP input events were dispatched, then encoded with `ffmpeg` (`libx264`, `yuv420p`, `+faststart`).
- Headless PNG readback is slower than the 120 ms capture target, so frames are time-compressed at 8 fps; interaction coverage is unaffected.
- Temporary frames and the recording script were removed after encoding.
