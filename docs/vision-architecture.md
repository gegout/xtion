# Xtion Vision architecture

## Frame ownership and timing

The only adapter that opens USB is `LiveSource` in `vision/src/source.rs`. It checks the native sample units, snapshots acquisition health and copies each contiguous pixel slice once into an owned frame. Recording and perception share that frame through `Arc`. Replay and synthetic sources implement the same pull interface. Recorded monotonic arrival intervals govern replay pacing. Device EOF ticks wrap and can identify repeated or out-of-order observations. Host arrival nanoseconds govern motion and durations.

One acquisition thread owns and stops the sensor. Perception and recording each have their own four-frame queue. Overflow discards the oldest pending item and increments that queue's counter. The renderer reads a single replaceable snapshot. AI has one in-flight call and one replaceable pending observation. Fast headless replay pulls sequentially from disk so that an offline consumer can inspect every frame. The acquisition owner enforces a bounded session deadline independently of window repaints or output consumers. Ending a live session signals stop, joins the sensor owner and closes the queues after flushing. A source failure reaches the viewer or the headless capture-problem record. The viewer marks a retained frame stale after 500 ms.

## Depth and motion

The right-handed optical frame uses metres, with X right, Y down and Z forward. Every frame retains the intrinsics provenance. Nominal pinhole focal lengths use the published horizontal and vertical fields of view and a centred principal point. Projection uses X = (u − cx) Z / fx and Y = (v − cy) Z / fy. Unknown optics preserve axial depth and suppress lateral geometry. A rigid camera pose currently maps the optical frame to itself. The driver exposes no measured distortion coefficients.

The engine samples every fourth row and column. A small median filter includes valid neighbours within 45 mm of the centre. Five valid initial frames learn a background. Subsequent non-foreground samples adapt that background. Recalibration clears tracks and starts learning again. A nearer surface must exceed the configured depth difference and occupy at least six connected samples. Neighbours with a 250 mm jump stay separate. The engine computes centroids from projected samples, bounding extents, axial depth, range, velocities and median surface depth in image thirds.

Greedy association uses image separation and depth difference. Two detections confirm appearance. Hysteresis at 0.10 and 0.05 m/s and consecutive votes limit motion transitions. Tracks survive brief occlusions for 350 ms. Low depth coverage suspends departure inference. IDs identify tracks, individual observations and timestamped events. Crossings, small hands, merged silhouettes and changed backgrounds can still confuse association. Track identities denote regions within a session.

## Interpretation and export

The deterministic description uses measured regions and coverage. An optional worker tests image use with randomized quadrants, selects the fastest passing candidate, and caches selection and structured-output capability for the worker lifetime. It uses Chat Completions by default. A missing Chat route triggers a check of the service’s own OpenAPI declaration before using Responses. It requests JSON schema, JSON object or prompt-only in descending order. Semantic hypotheses use a constrained vocabulary. The validator checks observation IDs, confidence, distance within 0.10 m, direction and named relationships. Accepted output uses local depth. Events and a quantized scene signature schedule interpretation and suppress stale results. All HTTP calls use deadlines. Dropping the worker cancels outstanding work.

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
