// Compiled only in a temporary checkout with component membership instrumentation.
use std::{collections::BTreeSet, fs::File, io::BufReader};
use xtion_vision::{
    perception::Engine,
    source::{FrameSource, ReplaySource, Result},
};
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut replay = ReplaySource::new(BufReader::new(File::open(args.next().unwrap())?), false)?;
    let indices: BTreeSet<usize> = args.map(|s| s.parse().unwrap()).collect();
    let mut engine = Engine::default();
    engine.threshold_mm = 100;
    for index in 0..=*indices.last().unwrap() {
        let frame = replay.next_frame()?.ok_or("recording too short")?;
        let scene = engine.observe(&frame);
        if indices.contains(&index) {
            let objects: Vec<_> = scene.objects.iter().map(|object| {
                assert_eq!(object.region.members.len(), object.region.samples);
                (object.id, &object.region.members)
            }).collect();
            println!("{}", serde_json::json!([index, objects]));
        }
    }
    Ok(())
}
