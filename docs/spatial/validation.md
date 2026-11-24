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
