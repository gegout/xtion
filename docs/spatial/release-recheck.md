# Spatial release recheck

The short right-to-left passage still reproduces a foreground segmentation regression. The latest candidate is available locally on `spatial/seed-recovery` at `eb2152a`. Production remains at the 0.1 feature level. The two worktrees used the same native driver, camera optics, depth thresholds and XTV recordings.

## Results on recorded sensor data

A dominant region contains at least 80% of the samples assigned to all visible tracks in that frame. This metric can reward a large background surface. The comparisons below also check pixel membership against a later empty-scene reference and the previously drawn silhouettes.

| Replay measure | Production (8258055) | Candidate (eb2152a) |
|---|---:|---:|
| Entry, dominant frames / 114 | 94 | 108 |
| Occupied at startup, dominant frames / 2,681 | 106 | 2,312 |
| Occupied at startup, dominant-ID changes | 143 | 63 |
| Repositioned occupied scene, dominant frames / 1,347 | 1,347 | 1,347 |
| Repositioned occupied scene, dominant-ID changes | 0 | 0 |
| Static interior, motion events / 224 frames | 10 | 7 |
| Confirmed empty scene, motion events / 300 frames | 26 | 6 |
| Confirmed empty scene, frames reporting foreground / 300 | 262 | 174 |
| Short passage, dominant frames / 61 | 56 | 52 |
| Short passage, dominant-ID changes | 3 | 3 |
| Archival clip, frames reporting foreground / 134 | 31 | 41 |

The latest candidate recovered 19,994 of 20,250 labelled person samples on held-out entry frames with 99.82% precision. In the occupied-start holdout it recovered 12,426 of 12,429 with 96.52% precision. Coverage on the occupied-start development outlines was 66.13%. The holdouts each contain three frames from the same recording. The archival clip has no independent object labels.

## What changed

The first integrated candidate (1ba72fd) used depth ordering at a sparsely supported region's left and right boundaries to reject growth onto a background surface. For example, all 5,374 comparable samples in its dominant track on repositioned frame 888 were at least 250 mm nearer than the corresponding empty-scene depths. That version eliminated the occupied interval's eight switches from the earlier experiment. It also missed all the small changes in the archival clip.

A trace on the archival clip showed a connected surface with 37 changed samples among 4,937. Rejecting that entire surface also discarded a compact changed patch. The next candidate (5822774) reconsidered connected groups of measured foreground samples after rejecting a grown surface. A synthetic nine-sample patch on a coplanar surface reproduced the miss before the fix. This recovered compact regions but increased empty-scene motion events from two to sixteen. Requiring consistent depth across successive frames for these recovered groups (eb2152a) reduced that count to six. The temporal check resets on gaps, missing coverage and recalibration.

## Why the release gate remains open

The three integrated variants scored 54, 53 and 52 dominant frames on the short passage, against 56 from production. Four frames that production passed and the latest candidate missed were inspected against the operator-confirmed clear recording. The comparison requires a valid empty-scene measurement in at least 90% of frames. It calls a sample clearly nearer at a depth difference above 250 mm.

* On passage frame 142 (source frame 3992), 702 of 728 comparable samples in the candidate's second-largest region match the clear background within three model standard deviations.
* On frame 146 the candidate split two substantial regions. All 3,310 comparable samples in the first and all 874 in the second were clearly nearer.
* On frame 195 (source frame 4045), production's largest region contained 2,537 clearly nearer samples. The candidate's largest contained only 199. Of 57 comparable samples in its second-largest region, 52 matched the clear background.

This gives reproducible instances of both errors, recruitment of unchanged surfaces and fragmentation of nearer surfaces. Further threshold tuning on this passage would reuse the same evaluation data. Automatic floor pose estimation, occlusion-tested metric tracking, surface meshing, picking and measurement, freeze, additional geometry exports and the expanded AI schema still lack release implementations. The existing 3D point viewer can be exercised independently.

## Reproduce the replay check

Build the `spatial_baseline` example in a checkout of each revision with `cargo build --release --offline --locked -p xtion-vision --example spatial_baseline`. Keep a separate executable for each build. With the original recordings available locally, run the comparison below. The tool records clip hashes and scored intervals, and exits 1 when the passage comparison fails.

```sh
python3 scripts/compare_spatial_replays.py --check \
  /path/to/production/spatial_baseline /path/to/candidate/spatial_baseline \
  recordings/spatial-baseline recordings/repositioned-AW5grrSw \
  --archive /tmp/xtv-verification/final-session.xtv > comparison.json
```

The archived input is optional. Review the aggregate measurements and the four frame comparisons in [the machine-readable results](release-recheck.json). The source recordings stay on the laptop.

To reproduce the depth-evidence check, run the following from the repository root. The helper builds each pinned revision in a temporary worktree with read-only instrumentation for region membership. Its JSON output contains counts for each of the two largest regions.

```sh
rec=recordings/repositioned-AW5grrSw
python3 -m scripts.spatial_review.clear_reference "$rec" passage-before-cue \
  "$rec/clear-fixed.xtv" --frames 142 146 193 195 --revisions 8258055 eb2152a
```

## Live acquisition and desktop checks

Both candidates ran on the same ASUS Xtion PRO, USB 1d27:0601 at port path 1-2 with firmware 5.8.22. Each ran for five minutes in split view with the CPU point renderer, followed by five short glow-viewer acquisitions. AI was disabled. The latest build reopened, delivered 104 processed frames and stopped on each of the five runs. A separate diagnosis after each five-restart series observed idle firmware and zero depth endpoint bytes. No reset or port change was used.

| Five-minute hardware measurement | 1ba72fd | eb2152a |
|---|---:|---:|
| Frames processed | 8,954 | 8,874 |
| Processing FPS | 29.90 | 29.63 |
| Point draw submissions over 300 s, per second | 11.60 | 31.07 |
| Application stage drops | 0 | 0 |
| Source packet gaps / incomplete frames / decoder errors | 21 / 22 / 3 | 79 / 102 / 25 |
| RSS at 10 s / 297 s, MiB | 174.0 / 175.9 | 172.6 / 182.4 |

The first run's first-to-last point draw interval was only 174.04 seconds, and other validation jobs overlapped that run. Its active draw rate of 19.98 per second overstates full-run rendering performance. The latest run's draw interval was 299.42 seconds. Draw submissions measure viewer activity and may repeat a sensor frame. The latest run's RSS grew 9.8 MiB over the sampled interval. A five-minute observation cannot establish long-term memory stability.

The latest candidate passed formatting, warning-denied workspace Clippy, 71 offline Rust tests, the locked release build and offline verification of both workspace packages. Nine opt-in tests were skipped by the default suite. The CPU and glow renderers both placed all three synthetic landmarks with zero pixel-centre error. A headless ASCII PLY export reproduced all 256 fixture points with zero XYZ readback error. The release comparison itself exited 1 on the short passage. No 0.2 version was tagged, packaged or published.
