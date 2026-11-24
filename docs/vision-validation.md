# Xtion Vision V0.1 validation

Ubuntu 26.04.1 LTS, ASUS Xtion PRO 1d27:0601, direct USB, CPU rendering. The driver exposes metric Z when its calibration succeeds. Vision uses **Nominal** horizontal and vertical intrinsics. All live images, recordings and per-frame observations stayed on the validation host.

| Capability | Status | Method and evidence |
|---|---|---|
| Native frame input and recording | Verified | Captured 15 VGA frames, verified normal stop, replayed and compared all pixels, both clocks, intrinsics and health. |
| Projection and PLY | Verified on synthetic geometry | Projection/inverse on a 2 m plane. Independent plyfile reader imported 256 vertices with Z exactly 2 m. Unknown intrinsics suppress lateral output. Absolute X/Y accuracy remains unverified. |
| Region tracking and local descriptions | Verified on synthetic data | Approach and retreat at 0.25 m/s, stable ID across one missing observation, debounced transitions, coverage loss and noise. Two recorded fixtures give byte-identical JSON in repeated fast replay. |
| Desktop and PNG | Verified | Displayed live and replay on the Ubuntu desktop and captured 1200×760 full-window PNGs. The live view contained a depth silhouette, missing pixels, timeline, controls and nominal optics label. |
| Live processing and memory | Verified for measured run | 18,025 frames at 29.918 processed FPS. RSS sampled every second from 60–600 s ranged from 12.70 to 12.84 MiB, ending at 12.84 MiB. |
| Acquisition health | Partly verified | Driver counters over the full run including startup: 23 sequence gaps, 31 incomplete frames, 9 decoder errors, 0 host queue drops, 0 USB errors. Vision perception queue drops: 0. |
| Normal stop and reuse | Verified | Five consecutive desktop cycles initially. Five more release runs after the final pipeline changes, with desktop and headless clients on the same port. See cycle results below. |
| AI scheduling, grounding and cancellation | Verified with mock HTTP | Discovery uses image contents, explicit model bypasses discovery, timeout, outage after success, retry, fallback, ID/distance checks and stale results tested. Desktop with a 400 ms mock delay processed 100/100 paced synthetic frames at 25 FPS and made nine local requests in the measured run. Real VGA replay with 400 ms mock replies processed all 134 frames at 29.97 FPS with 12 loopback calls and zero stage drops. |
| Deployed model semantics | Not verified | No live model service was selected for the test. The measured geometry and local description operate independently. |
| USB and driver regression | Verified | Offline workspace suite passed on a clean file export: 47 passed, five explicitly opt-in. A separate AI-free build passed all ten applicable tests. Existing private packed and unpacked capture regression also passed locally with four transfer chunk sizes. |
| Guided physical movement | Not verified | See the live checklist below. |

## Repeatability

The [machine-readable benchmark](vision-benchmark.json) includes native acquisition counters, stage drops and sampled resident memory.

| Final run | Processed frames | FPS | Perception drops | Stop result |
|---|---:|---:|---:|---|
| 1 | 134 | 29.968 | 0 | stopped |
| 2 | 134 | 29.970 | 0 | stopped |
| 3 | 131 | 29.291 | 0 | stopped |
| 4 | 134 | 29.966 | 0 | stopped |
| 5 | 133 | 29.743 | 0 | stopped |

The first desktop soak revealed that a deadline tied to repaints could overrun while the compositor suspended rendering. The process was terminated after 664 s. Desktop RSS ranged from 166.25 to 166.68 MiB during minutes 1–10. No final frame count or shutdown success was inferred from that attempt. The existing driver reconciled the orphan on bus 1, port 2 in one open attempt with zero resets. The revised acquisition worker enforces the deadline without a window or output consumer. The GUI also handles the close deadline in its logic callback. Two overlapping synthetic windows exited normally in a follow-up test.

The completed ten-minute run used headless mode with the revised pipeline. FPS uses processed frame intervals over monotonic sensor-host arrivals. RSS covers the headless process during that run. Desktop RSS and the five reuse runs are complementary measurements. The first final desktop cycle also recorded 134 frames, all recovered on fast replay. A finite soak cannot establish indefinite stability.

## Live checklist

The application reopened the sensor after the soak. It processed 2,599 frames at 29.96 FPS while the empty-scene instruction was pending and stopped cleanly on SIGINT. The observed session lasted 86.7 s. No operator confirmation arrived during this session. Spontaneous depth changes cannot establish which action caused them.

| Requested physical condition | Expected observation | Status |
|---|---|---|
| Empty field of view | Settled background with no persistent new foreground | Not verified, scene occupancy unconfirmed |
| Enter from left and stop centrally | Appeared region, increasing image X, then stationary | Not verified |
| Approach then retreat | Negative then positive axial velocity on the same track | Not verified |
| Hand moves left to right | Localized lateral motion | Not verified |
| Leave the field | Disappearance after occlusion grace period | Not verified |
| Briefly cover sensor | Reduced valid coverage or near-field occlusion followed by restored signal | Not verified |

The remaining instructions depend on the confirmed empty baseline and were deferred. A previous driver-only movement observation is documented in [native validation](validation.md). It does not verify this application’s guided checklist.

An opt-in reuse test launched before the checklist process had finished releasing USB received the expected “Resource busy” error. After release, the same test passed all five cycles in 28.16 s. The test checks metric frames, at least 25 processed FPS, zero perception drops and a successful stop on each cycle.

## Adjustable application capture and local model interpretation

A read-only firmware query on the attached 5.8.22 device returned 30 FPS for VGA. The upstream 25 FPS preset is gated to older firmware. Xtion Vision can retain fewer frames before processing and recording while the native USB acquisition continues. Short live runs targeting 5, 15 and 30 retained FPS measured 5.15, 14.98 and 29.97 FPS respectively.

With the localhost SmolLM3-3B endpoint and a target of 10 FPS, a 36-second session processed 355 frames at 10.01 FPS. Sampling intentionally omitted 707 frames. Both perception and recording queues reported zero overflow drops. Replay recovered all 355 frames. Two current model selections cited the same layout at distinct source frames (f103 and f243). In each case the recorded median depths matched the displayed 2.86 m left, 3.50 m centre and 2.33 m right within rounding.

The opt-in `local_model_prioritizes_depth_evidence` test supplied four controlled observations to the actual model service. It selected approach for an approaching patch, retreat for a receding patch, the three-sector depth profile for a static scene, and insufficient coverage for a fully masked scene. These are semantic selection checks using synthetic inputs. Object identity and human activity were not established.
