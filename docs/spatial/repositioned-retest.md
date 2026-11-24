# Repositioned Xtion release check

The connected ASUS sensor enumerated as 1d27:0601 at 001/056 during this run. The live program used the main branch at 9d6ce3b, split depth and CPU point views, a 30 FPS capture setting and AI disabled. The candidate at de7f8e4 ran offline on the same recorded bytes. The new camera position is specific to this session. Optics remain nominal.

## Spatial checks

| Scored interval | Production | Experimental candidate |
|---|---:|---:|
| Single-person interval, dominant region on frames 134–1480 inclusive | 1,347 / 1,347 | 1,229 / 1,347 |
| Changes of the largest region ID in that interval | 0 | 8 |
| Mean number of visible tracked regions | 3.68 | 2.06 |
| Confirmed clear scene, motion events over frames 4172–4471 | 26 | 0 |
| Clear-scene frames reporting foreground | 262 / 300 | 44 / 300 |
| Previous entry clip, unchanged replay gate | 94 / 114 | 76 / 114 |

The largest-region metric can switch to a competing surface while the original track survives. At new frame 888 production retained the person’s box. The candidate switched its largest ID from 2 to 51; the new box extended to the right image boundary and its mean Z was 2.32 m. At frame 888, 5,446 of the candidate’s 5,694 comparable largest-region samples had depths within three model standard deviations of their later clear-scene medians. All 5,374 comparable production samples were over 250 mm nearer than their clear-scene medians. ID 2 later became largest again. The local track-comparison.png overlays both boxes. The operator confirmed that the view was clear immediately before source frame 4172. A proposed earlier window was rejected on image inspection because a subject passed through it.

For the clear interval, 285,055 pixels met the temporal coverage criterion. Across occupied depth bins the axial noise fit had R² 0.886 and RMS residual 2.00 mm. Its fitted intercept was negative. This run used the existing configuration throughout.

## Build, export and scope

Workspace formatting, warning-denied Clippy, 64 offline tests and the locked release build passed; nine opt-in tests were skipped. Both Xvfb point rendering paths placed all three landmarks at the expected pixels. Single-frame ASCII PLY readback yielded 284,730 vertices, exactly one per valid source pixel; every exported Z matched its recorded millimetres converted to metres. Standalone offline Cargo packaging of xtion-vision failed to resolve xtion in the local crates.io index. Packaging the full workspace together with Cargo verified both 0.1.0 source packages offline. No package was published.

The manifest still declares 0.1.0. Floor pose estimation, the visible-surface mesh, measurement tools, freeze and the additional geometry exports, validated 3D tracking, and the extended AI observation schema remain outstanding. The experimental segmentation still misses the previous entry gate. Moving the sensor supplied independent validation data. These results do not qualify a 0.2 release.

## Capture termination and recovery

The original five-minute command exited with status 143 (SIGTERM). Its file contained 8,812 complete VGA frames over 294.41 seconds of sensor arrivals (29.93 recorded FPS), with zero trailing bytes. Its final recorded health counters included 13 incomplete frames, 11 gaps and three decode errors. The desktop did not emit a final shutdown summary. The source of SIGTERM was not established. A follow-up glow viewer produced no recording. Native diagnosis then observed active depth traffic. The existing device-scoped recover command verified idle in one open attempt without resetting the sensor. A subsequent eight-second glow test processed 224 live frames, drew at 32.97 draws/s and reported a clean stop.

The desktop now sends a window-close request on SIGINT or SIGTERM and lets the existing acquisition and recorder threads finish. Pipeline shutdown emits a diagnostic summary even when the first frame never arrived. The opt-in signal regression runs both signals and decodes the final recording boundary on each invocation. Both live signals produced a stopped status and complete recordings, with no recovery command between the two opens. A separate diagnostic reported idle firmware and zero endpoint bytes. Synthetic signal tests and a missing replay input also passed.

Run `python3 scripts/check_viewer_shutdown.py` after a release build with a desktop display. Add `--live` for the hardware regression. All private images and frames from this session remain under the ignored recordings directory.

The [machine-readable results](repositioned-results.json) record the source frame ranges, tracking metrics, noise fit, signal checks and USB counters. The local session is recordings/repositioned-AW5grrSw. Raw footage and mask membership lists were excluded from the report.

An isolated X11 display exercised the desktop gesture handlers on three synthetic landmarks. Both CPU and glow produced the same screen coordinates after primary drag (orbit), secondary drag (pan) and wheel zoom. For the nearest landmark the sequence was (362,250), (329,252), (358,259), (293,214) in pixels. This checks actual pointer input and redraw. Human confirmation of mesh rendering remains inapplicable until a mesh view exists.

A separate 61-frame passage before the crossing cue is visible from image right to left. The median horizontal coordinate of nearer foreground samples moved from 0.89 to 0.33 of image width between source frames 3990 and 4050. This was a pre-cue observation. The later requested left-to-right crossing was not verified in the recording.

## Final five-minute verification

Build 22cc11a ran the live sensor in split view on an isolated X11 display with the CPU point renderer. It processed 8,928 frames at 29.81 FPS and submitted 31.73 draws/s. No application queue drops were recorded. At the source, 34 packet gaps, 48 incomplete frames and 16 decoder errors were reported. RSS from the first sample after 10 seconds to the final sample was 136.2 to 136.3 MiB. Workspace package verification overlapped part of this run. The viewer exited 0 with status stopped. A separate diagnosis returned idle and 0 depth endpoint bytes.
