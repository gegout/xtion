# Spatial Vision 0.2 gate results

Phase 0 passed. Phase 1 failed its joint replay gate after three changes. The production package remains at 0.1. The three candidate implementations are committed on the local spatial/segmentation-experiments branch. The recordings are local, and their SHA-256 digests are listed in recording-checksums.txt.

| Recorded outcome | 0.1 | Noise-aware connectivity | Area-aware association | Seeded surface growth |
|---|---:|---:|---:|---:|
| Entry, dominant region / 114 frames | 94 | 94 | 98 | 59 |
| Entry, changes of dominant ID | 1 | 1 | 0 | 0 |
| Entry, mean tracked regions | 7.13 | 8.61 | 4.28 | 4.29 |
| Already occupied, dominant region / 2,681 frames | 106 | 96 | 595 | 2,367 |
| Already occupied, changes of dominant ID | 143 | 99 | 28 | 23 |
| Empty, motion events / 224 frames | 10 | 8 | 1 | 2 |
| Empty, changes of dominant ID | 13 | 7 | 0 | 0 |

All columns score exactly the same frame intervals. Dominance requires at least 80% of tracked samples in one component. It measures fragmentation; it does not establish anatomical accuracy. The 114-frame subset includes an approach and clipping at the image boundary. The 89.46-second sequence starts occupied, allowing the five-frame initializer to learn the stationary silhouette as background. Captured foreground changes can then originate from only a part of the silhouette. Independent image review could establish occupancy; the prompted crossing direction could not be verified.

| Capability | Status | Method and evidence |
|---|---|---|
| Baseline, adapter, static calibration | Verified | See baseline.md, JSON distributions, acquisition statistics and rendering backend inventory. In-range data cover a limited depth interval. |
| Depth-dependent connectivity | Partly verified | b992145. Weighted thresholds reduce empty-scene motion but do not unify the occupied silhouette. |
| ID stability and sensitivity | Partly verified | fde92fd. Area continuity and minimum support reduce ID switches but the occupied clip still has a dominant region in only 22.2% of all frames. In the separate 134-frame archived replay, this candidate also removes all previously reported small regions. |
| Surface growth, quality, nearby objects | Partly verified | 18cd346. Extend a foreground seed through adjacent depth-consistent samples, protect accepted components during background updates and flag invalid, range, depth edge and temporal variance. Nearby synthetic surfaces with a 400 mm depth step stay distinct. Surface growth produces dominance in 88.3% of occupied-start frames and just 51.8% of entry frames. |
| Default compatibility | Verified for tested inputs | Original implementation remains on main. Workspace tests, format, warning-denied Clippy and release build checked. |
| Plane extraction, floor pose, organised geometry, 3D viewer, mesh, metric tracking, measurement and exports | Not verified | Dependent phases did not pass the segmentation gate. |
| AI geometry inputs, interactive performance, user measurements and five-minute 3D run | Not verified | Require the later geometry and viewer gates. |

The two depth clips exercise opposing errors. A narrow support mask fragments an object learned as background. Unrestricted region growth can enlarge competing components on entry. Choosing one candidate based on a single clip would conceal the regression on the other. The next investigation needs source-pixel masks of the visible subject and competing surfaces across both initial conditions to distinguish leakage from fragmentation.

## Replay

Read baseline.md for clip indices. In each candidate checkout, run the same commands with the recording directory set to its absolute path. Application capture, processing thresholds and optics must match across runs.

```sh
cargo run --locked --release -p xtion-vision --example spatial_baseline -- "$recording_dir/entry.xtv" 265
cargo run --locked --release -p xtion-vision --example spatial_baseline -- "$recording_dir/crossing.xtv"
cargo run --locked --release -p xtion-vision --example spatial_baseline -- "$recording_dir/static-interior.xtv"
```

The experimental workspace passed 61 tests; the production workspace passed 60. Each run skipped nine opt-in tests. Rebuilding the production example reproduced every non-timing baseline field on all three local recordings.

The [reopened Phase 1 investigation](phase1-reopened.md) adds silhouette-reference comparisons, held-out frames and a candidate that retains track membership across a changing depth boundary.

## Independent geometry and point-view validation

The production branch now includes camera-frame geometry and interactive point viewing. These operate directly on raw frames and have no dependency on a tracked silhouette. The segmentation experiment remains on its own branch.

| Capability | Status | Method and measurement |
|---|---|---|
| Camera geometry and organised cloud | Verified offline | Source-pixel projection, missing data, sphere residual within 0.6 mm on quantised synthetic input, rectangular plane width and height, manual mount and rigid transform tests. Ten library tests pass without desktop or AI features. |
| Depth model on the recorded wall | Partly verified | Axial interpretation has 11.61 mm RMS plane residual, radial interpretation 14.87 mm. Nominal optics. See axial-check.json. |
| CPU and glow point drawing | Verified on offscreen fixtures | Three coloured landmarks had zero pixel centre error in each PNG. Both paths ran on live input. See point-projection-check.json and points-restarts.json. |
| Five-minute live split view | Verified on this laptop | CPU point rasterizer plus depth view, 8,817 processed frames, 29.44 processed FPS and 30.05 draw submissions/s. RSS at 10 and 300 seconds was 172.0 and 173.1 MiB. The run preceded the final redraw-cache optimization. See points-live.json. |
| Acquisition health | Partly verified | Zero application and recorder queue drops. Native counters include 134 packet gaps, 159 incomplete frames and 31 decoder errors during the five-minute run. Root cause unverified. |
| Restart | Verified on hardware | Five successive desktop starts after the long run produced 104, 101, 103, 102 and 101 processed frames and stopped cleanly. The test alternated glow and CPU rasterization. |
| Orbit, pan and zoom controls | Partly verified | Desktop controls and synthetic camera projections implemented. Operator gesture confirmation is outstanding. |
| Floor estimation, 3D object tracking and dependent release work | Blocked | Phase 1 entry dominance was 76/114 on the third reopened attempt versus 94/114 in production. Visible-surface meshes, measurement tools, extended exports and AI observations have not shipped. |

The locked offline workspace suite passed 64 tests (nine opt-in tests skipped), together with formatting, warning-denied Clippy and a release build. No firmware, USB driver or kernel settings were changed. The live screenshot and raw recordings stay local.
