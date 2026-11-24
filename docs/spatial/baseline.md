# Spatial Vision baseline

## Inventory and gate

| Area | Present in 0.1 | Spatial upgrade |
|---|---|---|
| Acquisition, AI, recording | Live, replay, synthetic frame sources, bounded queues, XTV1 raw u16 metric flag, clock pair, health, grounded AI, headless JSON, depth PNG and desktop chat | Preserve and exercise each entry point. Extend AI observations. |
| Geometry | Axial pinhole projection and inverse; camera frame in metres, X right, Y down, Z forward. Per-frame nominal or unknown optics. Identity pose, compact cloud, ASCII PLY | Extend with organised pixels, usable transforms, boxes, snapshots, sensor tilt and physical validation. |
| Segmentation | Five-frame background, adaptive update, four-connected 4-pixel grid, fixed depth thresholds, greedy position association and two-vote transitions | Extend segmentation and tracking. Noise calibration, planes, quality flags, occlusion repair and variance-aware thresholds are new. |
| Viewer | eframe 0.37 using glow, CPU-generated depth textures, overlays and capture controls | Orbit, clipping, grid, mesh, picking, freeze, inspector and offscreen 3D rendering are new. |
| Reconstruction and exchange | No triangles. Cloud export and depth PNG exist. | New visible-surface reconstruction, normals, OBJ, binary PLY, measurement, metadata and trajectory export. |
| Validation | Cargo tests for projections, occlusion, replay, endpoints, CLI and viewer; opt-in hardware tests | Add labelled replay comparisons, metric geometry fixtures, render checks, timed performance and hardware measurements. EdgeTAM is optional. |

The i915 Intel Raptor Lake-P adapter exposes renderD128. PCI enumeration also found an NVIDIA GB207M. The existing desktop startup requests software GL. In this application's environment glxinfo reports llvmpipe, LLVM 21.1.8, 256 bits, acceleration disabled. Physical GPU presence alone does not verify a hardware rendering path.

## Measurement

Run the release example spatial_baseline with an XTV path and optional first scored frame. Replay always starts at frame zero so background learning stays identical. Wall time covers Engine::observe. A switch counts a change of the largest region ID on consecutive nonempty frames. A lost-visible-ID frame counts a missing ID even when it will return. Dominance means one region contains at least 80% of tracked samples. These proxies require checking the actual depth silhouettes.

The sensor delivered 464 VGA frames at 29.97 FPS with zero application and recording drops in static.xtv. Inspection showed the sensor view shifting early and a person entering near the end. The reproducible static clip contains source frames [85,309). The entry clip contains [85,464); score its last 114 frames from index 265 with the preceding empty background retained. See the adjoining JSON reports. The subsequent crossing recording already contained a foreground subject at startup. The operator confirmed crossing after that recording closed; the direction of that instructed crossing is unverified.

Run scripts/spatial_noise.py on the static clip for per-pixel temporal sample SD and depth-bin medians. Coverage must reach 90%. The full-range quadratic fit has a negative intercept, RMSE 31.94 mm and R² 0.501. At 3.86 m the median SD was 139.5 mm. The conservative configuration uses 3 + 1.4 z² mm, above all five in-range bin medians (2.47–3.36 m). Near-field accuracy remains unmeasured. Stable horizontal-coordinate edges yielded 0.48 px jitter at 1.5–2.5 m and 0.47 px at 2.5–3.5 m. See noise-static-interior.json for vertical-coordinate edges and counts. The mixed recording fit is retained as a rejected control.

Nguyen et al. measured Kinect plane and edge residuals with angle dependence. Our two-term fit measures temporal repeatability at fixed pixels. [Original paper](https://users.cecs.anu.edu.au/~nguyen/papers/conferences/Nguyen2012-ModelingKinectSensorNoise.pdf).

## Reproduction

Use scripts/spatial_clip.py SOURCE OUTPUT FIRST END to make byte-preserving intervals. Raw recordings stay local under recordings/spatial-baseline. The baseline Rust application sources and assets were present as working-tree changes before this investigation. Initial debug tests found three viewer harnesses dropping unconsumed egui texture output. Consuming the output in the test harness restored the full offline suite. Workspace formatting, warning-denied Clippy and the locked offline release build passed.
