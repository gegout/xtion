# Xtion Vision architecture

## Frame ownership and timing

The only adapter that opens USB is `LiveSource` in `vision/src/source.rs`. It checks the native sample units, snapshots acquisition health and copies each contiguous pixel slice once into an owned frame. Recording and perception share that frame through `Arc`. Replay and synthetic sources implement the same pull interface. Recorded monotonic arrival intervals govern replay pacing. Device EOF ticks wrap and can identify repeated or out-of-order observations. Host arrival nanoseconds govern motion and durations.

One acquisition thread owns and stops the sensor. Perception and recording each have their own four-frame queue. Overflow discards the oldest pending item and increments that queue's counter. The renderer reads a single replaceable snapshot. AI has one in-flight call and one replaceable pending observation. Fast headless replay pulls sequentially from disk so that an offline consumer can inspect every frame. The acquisition owner enforces a bounded session deadline independently of window repaints or output consumers. Ending a live session signals stop, joins the sensor owner and closes the queues after flushing. A source failure reaches the viewer or the headless capture-problem record. The viewer marks a retained frame stale after 500 ms.

Live sampling bins monotonic USB arrival times before frames enter the recording and perception queues. Each occupied bin contributes one complete frame. VGA/30 passes every complete frame. The desktop slider starts a fresh bin sequence on a rate change. The UI reports processed FPS over the most recent second and the native stream mode separately. Firmware 5.8.22 returned 30 FPS for resolution 1 (VGA) in the read-only CMOS preset query (opcode 36, CMOS ID 1). OpenNI2 lists VGA/25 only for firmware 5.2 through versions below 5.6. [Upstream depth mode table](https://github.com/OpenNI/OpenNI2/blob/115cf06c6efea32304182d293eca16ca883c9150/Source/Drivers/PS1080/Sensor/XnHostProtocol.cpp#L487-L500).

## Depth and motion

The right-handed optical frame uses metres, with X right, Y down and Z forward. Every frame retains the intrinsics provenance. Nominal pinhole focal lengths use the published horizontal and vertical fields of view and a centred principal point. Projection uses X = (u − cx) Z / fx and Y = (v − cy) Z / fy. Unknown optics preserve axial depth and suppress lateral geometry. A rigid camera pose currently maps the optical frame to itself. The driver exposes no measured distortion coefficients.

The engine samples every fourth row and column. A small median filter includes valid neighbours within 45 mm of the centre. Five valid initial frames learn a background. Subsequent non-foreground samples adapt that background. Recalibration clears tracks and starts learning again. A nearer surface must exceed the configured depth difference and occupy at least six connected samples. Neighbours with a 250 mm jump stay separate. The engine computes centroids from projected samples, bounding extents, axial depth, range, velocities and median surface depth in image thirds.

Greedy association uses image separation and depth difference. Two detections confirm appearance. Hysteresis at 0.10 and 0.05 m/s and consecutive votes limit motion transitions. Tracks survive brief occlusions for 350 ms. Low depth coverage suspends departure inference. IDs identify tracks, individual observations and timestamped events. Crossings, small hands, merged silhouettes and changed backgrounds can still confuse association. Track identities denote regions within a session.

Each track retains up to five seconds of horizontal centroid history. Directional movement requires at least four observations over 300 ms, net travel of at least 22% of the image width and at least 80% agreement between net displacement and path length. A full crossing starts and ends in opposite outer thirds. A gap over 350 ms breaks the path. A text model may propose a person-like figure when an upright region has human-scale metric dimensions in at least three quarters of the passage samples, including the last one. Furniture can satisfy this heuristic. Passage reports cite the event and both endpoint observations.

## Interpretation and export

The deterministic description uses measured regions and coverage. An optional worker tests image use with randomized quadrants, selects the fastest passing candidate, and caches selection and structured-output capability for the worker lifetime. It uses Chat Completions by default. A missing Chat route triggers a check of the service’s own OpenAPI declaration before using Responses. It requests JSON schema, JSON object or prompt-only in descending order. Semantic hypotheses use a constrained vocabulary. The validator checks observation IDs, confidence, distance within 0.10 m, direction and named relationships. Accepted output uses local depth. Events and a quantized scene signature schedule interpretation and suppress stale results. All HTTP calls use deadlines. Dropping the worker cancels outstanding work.

The AI worker samples the four largest tracks once per second and keeps up to 30 seconds of capture time. A clock reversal or a gap over two seconds restarts that history. Each interpretation receives a compact chronological selection and up to ten recent tracking events. The text endpoint selects evidence-backed accounts of motion, settling, steady depth, or track loss. Historical reports refer to a timestamped observation window. The desktop suppresses consecutive repetitions of the same steady-track finding.

The cloud exporter consumes points with pose, frame of reference, timestamp and optics provenance. PLY writes visible samples in camera coordinates, with metres and timing in its header. A future renderer can consume the cloud directly. Future pose estimation, multi-view fusion and reconstruction can own their own transform history and persistent surface representation downstream of the frame source. Further file formats can implement the exporter trait.

## Rust consumer

```rust
use xtion_vision::{describe::describe, perception::Engine, source::{FrameSource, ReplaySource, Result}};
use std::{fs::File, io::BufReader};

fn analyse(path: &str) -> Result<()> {
    let mut source = ReplaySource::new(BufReader::new(File::open(path)?), false)?;
    let mut engine = Engine::default();
    engine.threshold_mm = 100;
    while let Some(frame) = source.next_frame()? {
        let scene = engine.observe(&frame);
        println!("{}", describe(&scene));
        for region in scene.objects {
            println!("{}: Z = {:.3} m, centroid = {:?}", region.id, region.region.z, region.region.centroid);
        }
    }
    source.stop()
}
```

An organised cloud provides zero-copy pixel-addressable geometry over immutable depth frames. The virtual camera projects those samples for CPU rasterization or the egui glow mesh. Sensor mount coordinates, optical coordinates and virtual camera coordinates remain separate. See the [point-view architecture and rendering decision](spatial/rendering.md).
