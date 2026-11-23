use std::{
    fs::File,
    io::{BufReader, BufWriter},
    time::Instant,
};
use xtion_vision::{describe::describe, perception::Engine, source::*};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let val = |k: &str| args.windows(2).find(|a| a[0] == k).map(|a| a[1].as_str());
    let flag = |k: &str| args.iter().any(|v| v == k);
    let mut src: Box<dyn FrameSource> = if let Some(path) = val("--replay") {
        Box::new(ReplaySource::new(
            BufReader::new(File::open(path)?),
            !flag("--fast"),
        )?)
    } else if flag("--synthetic") {
        Box::new(SyntheticSource::default())
    } else {
        Box::new(LiveSource::open()?)
    };
    let mut writer = val("--record")
        .map(|path| Recorder::new(BufWriter::new(File::create_new(path)?)))
        .transpose()?;
    let mut engine = Engine::default();
    engine.threshold_mm = val("--threshold-mm").unwrap_or("100").parse()?;
    let start = Instant::now();
    let seconds: f64 = val("--seconds").unwrap_or("1e12").parse()?;
    while start.elapsed().as_secs_f64() < seconds {
        let frame = match src.next_frame() {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                println!(
                    "{}",
                    serde_json::json!({"type":"capture_problem","status":"stale","message":e.to_string()})
                );
                return Err(e);
            }
        };
        if let Some(w) = writer.as_mut() {
            w.frame(&frame)?
        }
        let s = engine.observe(&frame);
        println!(
            "{}",
            serde_json::json!({"type":"observation","description":describe(&s),"scene":s})
        );
    }
    src.stop()?;
    if let Some(w) = writer {
        w.finish()?;
    }
    Ok(())
}
