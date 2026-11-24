//! Replays every frame through the production engine without acquisition or rendering.
use serde_json::json;
use std::{collections::HashSet, fs::File, io::BufReader, time::Instant};
use xtion_vision::{
    perception::Engine,
    source::{FrameSource, ReplaySource, Result},
};

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: spatial_baseline FILE.xtv")?;
    let score_from = std::env::args()
        .nth(2)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    let mut index = 0;
    let mut source = ReplaySource::new(BufReader::new(File::open(&path)?), false)?;
    let mut engine = Engine::default();
    engine.threshold_mm = 100;
    let mut counts = Vec::new();
    let mut times = Vec::new();
    let mut first = None;
    let mut last = 0;
    let mut previous = HashSet::new();
    let mut largest = None;
    let mut switches = 0;
    let mut losses = 0;
    let mut dominant = 0;
    let mut movement = HashSet::new();
    while let Some(f) = source.next_frame()? {
        let now = Instant::now();
        let scene = engine.observe(&f);
        index += 1;
        if index <= score_from {
            continue;
        }
        times.push(now.elapsed().as_secs_f64() * 1000.);
        first.get_or_insert(scene.ns);
        last = scene.ns;
        counts.push(scene.objects.len());
        let ids = scene.objects.iter().map(|o| o.id).collect::<HashSet<_>>();
        losses += usize::from(!previous.is_subset(&ids));
        previous = ids;
        let max = scene.objects.iter().max_by_key(|o| o.region.samples);
        if let Some(m) = max {
            if std::env::var_os("XTION_TRACE_REGIONS").is_some() {
                eprintln!(
                    "{} {} {:?} {:.3} {} {:?}",
                    index - 1,
                    m.id,
                    m.region.bounds,
                    m.region.z,
                    m.region.samples,
                    m.region.center
                );
            }
            dominant += usize::from(
                5 * m.region.samples
                    >= 4 * scene
                        .objects
                        .iter()
                        .map(|o| o.region.samples)
                        .sum::<usize>(),
            );
            switches += usize::from(largest.is_some_and(|old| old != m.id));
        }
        largest = max.map(|o| o.id);
        for e in scene
            .events
            .iter()
            .filter(|e| e.kind == "moving" && e.ns >= first.unwrap())
        {
            movement.insert(e.id.clone());
        }
    }
    let frames = times.len();
    let seconds = last
        .checked_sub(first.ok_or("empty input")?)
        .ok_or("clock reversal")? as f64
        / 1e9;
    times.sort_by(f64::total_cmp);
    counts.sort_unstable();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "score_from_frame":score_from,"input":path,"frames":frames,"seconds":seconds,
            "regions_mean":counts.iter().sum::<usize>() as f64/frames as f64,
            "regions_p95":counts[(frames-1)*95/100],"frames_with_foreground":counts.iter().filter(|n|**n>0).count(),
            "frames_with_80pct_dominant_region":dominant,"largest_id_switches_in_adjacent_nonempty_frames":switches,
            "largest_id_switches_per_minute":(seconds>0.).then(||60.*switches as f64/seconds),
            "frames_losing_any_visible_id":losses,"moving_events":movement.len(),
            "perception_mean_ms":times.iter().sum::<f64>()/frames as f64,"perception_p95_ms":times[(frames-1)*95/100]
        }))?
    );
    Ok(())
}
