# Silhouette investigation

The occupied-start failure is visible in the depth samples. Production learns the stationary subject during initialization and subsequently labels only small changes. A different failure occurs during entry. At frame 300 a surface belonging to the same reviewed outline develops an 80–110 mm step across neighbouring native pixels. Surface growth gives a fragment its own track. Simply following the intermediate raw pixels (083b36d) produced identical development-mask scores to 18cd346.

The retained-support candidate (32e4085) allows a wider noise-scaled depth step only where both cells were previously assigned to the same established track and both currently show foreground evidence. Independently established neighbours keep the original connectivity threshold. The focused synthetic test exercises both histories with the same final 160 mm depth step. At occupied-start frame 30 no region has been acquired. Occupancy requires independent evidence before it can be described.

## Independent replay checks

Outlines were drawn from depth images before inspecting tracker memberships. Predictions use actual flood-fill membership on the four-pixel grid. Exclude invalid samples and a four-cell band on either side of the outline. Green marks confident target interior; amber marks uncertain boundaries in the reference panels. The largest live track is selected without consulting the reference. On the result panels green means covered target, orange means missed target and pink means tracked background. In the two-column guide, the operator confirmed that the green interiors belong to the person in entry frame 285 and occupied-scene frame 450. The amber boundaries and the remaining reference frames have no operator review.

The development portions end before frame 315 (entry) and 1600 (occupied); replay runs from frame zero in both cases. Three later frames in each clip were reserved before tuning the new candidates. These are six held-out outlines from the same two previously benchmarked recordings, not independent sessions. Scores pool labelled cells, and uncertainty bands exclude the most difficult boundary pixels.

| Held-out result | Production | Retained support |
|---|---:|---:|
| Entry person coverage | 94.70% | 98.74% |
| Entry precision | 99.83% | 99.82% |
| Occupied person coverage | 1.42% | 99.98% |
| Occupied precision | 100% (177 labelled target cells) | 96.52% (12,426 labelled target cells) |
| Entry dominant region, all 114 scored frames | 94 | 76 |
| Occupied dominant region, all 2,681 frames | 106 | 2,386 |
| Changes of dominant ID, entry / occupied | 1 / 143 | 0 / 21 |
| False moving events, 224 empty frames | 10 | 2 |

The candidate recovers more of the labelled person. It includes 448 confident-background samples in the occupied holdout, compared with zero from a production track covering very little of the person. Entry misses the original joint dominance gate. The candidate stays on spatial/reopened-phase1. Fmt, warning-denied workspace Clippy, 62 offline tests and a workspace release build passed. Nine opt-in tests were skipped. No new hardware validation was performed.

## Reproduce

Run from the workspace root with NumPy and Pillow installed. The extraction script instruments historical checkouts with component membership and compiles only a replay example against each candidate. Raw frames, review images and exact membership lists remain under the ignored recordings directory. JSON references use zero-based frame and grid coordinates. See masks-development.json and masks-holdout.json for all intermediate component counts.

```sh
python3 -m unittest discover -s scripts/spatial_review
python3 scripts/spatial_review/extract.py recordings/spatial-baseline recordings/spatial-baseline/review
python3 scripts/spatial_review/score.py recordings/spatial-baseline recordings/spatial-baseline/review development
python3 scripts/spatial_review/score.py recordings/spatial-baseline recordings/spatial-baseline/review holdout
```

The third reopened experiment (de7f8e4) reconnects an established region across a single missing native pixel when both ends and every intervening valid sample agree in depth. Its regression test preserves the missing pixel and the track ID. The entry score remained 76/114. The occupied score reached 2,387/2,681. Empty-scene motion events remained at two. The experimental workspace passed all 63 offline tests, formatting, Clippy and its release build. See bridge-*.json. The three-attempt stop rule closes this experiment. Raw camera-frame geometry can be validated independently; object-dependent measurements still require segmentation validation.
