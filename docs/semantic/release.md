# Xtion Vision 0.3.0 release

This release adds session-scoped entities, geometric relationships, temporal activity intervals, evidence-linked local dialogue and optional validated model interpretations. It preserves XTV1 replay and the native driver at 0.1.0. Recording writes a local semantic sidecar. The Linux release bundle includes the viewer, USB CLI and guided capture tool.

| Capability | Status | Method and evidence |
|---|---|---|
| Spatial compatibility | Verified on available recordings | Seven identical recording checksums and all protected counts matched V0.2. Development and historical within-recording holdout outline scores matched exactly. [Joint gate](joint-replay.json) · [Outlines](mask-regression.json) |
| Semantic entities and relationships | Verified on fixtures | Synthetic fragments merged after consistent motion, split on divergence, preserved identity across a short gap and stayed separate as stationary neighbours. Sensor-frame relationships carry observation IDs. |
| Motion and dialogue | Partly verified | Synthetic intervals debounced and closed on lost coverage. Local and tool-enabled mock dialogue returned the same measured answer. Segment precision, recall and human detection delay require independently reviewed sensor recordings. |
| Guided capture and review | Verified on synthetic input | Two splits of ten XTV files passed checksum checks. The exercise saved per-clip semantic sidecars and a corrected interval. Human instructed files 0, reviewed files 0. [Protocol](dataset.md) |
| Grounded AI and scheduling | Verified with mocks | Nine intentionally unsupported single-claim payloads were rejected on both attempts. One valid control passed. These are 18 rejected claims out of 19 crafted attempts, with two each for nine rejection categories. This measures validation behavior, not model accuracy. Discovery, tool fallback, timeout, cancellation, rate limiting and endpoint loss have offline tests. |
| Live model validation and five-way input comparison | Blocked | No endpoint or consent was supplied. No depth was transmitted to a model. [Evaluation matrix](evaluation.md) |
| Human posture, extremities and person-specific activity | Not verified | Zero independently reviewed human holdout intervals. No local human perception level qualified. Arm, sitting, standing-up, reaching and possible human interaction remain unsupported. [Human validation](human-validation.md) |
| Calibration and room pose | Partly verified | Calibrated axial depth, nominal lateral optics. A synthetic known mount rejects an inconsistent tabletop. Physical known-object error at two distances is unverified. |
| Recording and replay | Verified on fixtures | Saved graph deltas reconstructed exact floating-point observations offline. Released-source split replay with saved semantics displayed 13 distinct synthetic frames in 74 draw submissions. |
| Build and packaging | Verified | Workspace formatting, warning-denied Clippy, 94 offline tests, locked release build and offline workspace package verification passed. Nine opt-in tests skipped. The library also passed 16 tests without default features. |
| Ten-minute live semantics | Verified on qualification build | Directly connected ASUS Xtion PRO, USB 1d27:0601, firmware 5.8.22. In 600.02 s the viewer processed 17,948 frames at 29.94 FPS. Geometry averaged 8.54 ms and semantics 0.39 ms per frame. Zero application-stage drops. [Soak and resource samples](live-soak.json) |
| Surface display | Partly verified | Glow submitted 19,754 draws, including 17,717 distinct frames, over 599.38 active seconds. This gives 32.96 draw submissions/s and 29.56 distinct frame submissions/s. Physical compositor presentations were uninstrumented. |
| Capture health and memory | Partly verified | 17 sequence gaps, 18 incomplete frames and 2 decode errors. RSS at 12 s was 149.3 MiB, ending at 159.9 MiB with a 160.8 MiB peak. This window does not establish long-term memory behavior or a USB root cause. |
| Final application restart and local dialogue | Verified on hardware | Three independent nine-second sessions each delivered 254 graphs and 254 local answers. Each capture stopped. A separate firmware probe reported idle with zero endpoint bytes. Entity and relationship counts are algorithm outputs with no operator ground truth. [Restart checks](restarts.json) |

The ten-minute run used the 0.3 qualification build. The packaged binary then processed 1,333 frames at 29.97 FPS over 45.04 s and returned to firmware idle. [Packaged binary verification](release-surface.json) No known-size object was available for a lateral accuracy assessment. Shader submissions and reported source counters remain visible in the measurements.

## Running the Linux bundle

Extract the archive into a new directory. Run `./xtion probe` for discovery, `./xtion-vision --view split --capture-fps 30` to inspect live surfaces or `./xtion-guided capture NEW_DIRECTORY` in a visible terminal to start the cue script. Keep personal recordings and their sidecars local. [Architecture](architecture.md) · [Retrospective](retrospective.md) · [Operator guide](dataset.md)
