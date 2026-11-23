mod viewer;
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
    let mut ai_config = val("--ai-url").map(|url| xtion_vision::ai::Config::new(url.to_owned()));
    if let Some(c) = &mut ai_config {
        c.model = val("--ai-model").map(str::to_owned);
        c.candidates = val("--ai-candidates").unwrap_or("4").parse()?;
        c.timeout =
            std::time::Duration::from_millis(val("--ai-timeout-ms").unwrap_or("8000").parse()?);
        c.budget =
            std::time::Duration::from_millis(val("--ai-budget-ms").unwrap_or("30000").parse()?);
        c.tokens = val("--ai-tokens").unwrap_or("400").parse()?;
        c.refresh =
            std::time::Duration::from_millis(val("--ai-refresh-ms").unwrap_or("2000").parse()?);
    }
    if !flag("--headless") {
        if std::env::var("LIBGL_ALWAYS_SOFTWARE").as_deref() != Ok("1") {
            use std::os::unix::process::CommandExt;
            return Err(std::process::Command::new(std::env::current_exe()?)
                .args(&args)
                .env("LIBGL_ALWAYS_SOFTWARE", "1")
                .exec()
                .into());
        }
        let input = if let Some(p) = val("--replay") {
            xtion_vision::pipeline::Input::Replay(p.into())
        } else if flag("--synthetic") {
            xtion_vision::pipeline::Input::Synthetic
        } else {
            xtion_vision::pipeline::Input::Live
        };
        return viewer::launch(viewer::Options {
            input,
            ai: ai_config,
            threshold: val("--threshold-mm").unwrap_or("100").parse()?,
            record: val("--record").map(str::to_owned),
            screenshot: val("--png").map(str::to_owned),
            seconds: val("--seconds").map(str::parse).transpose()?,
        });
    }
    let refresh = ai_config.as_ref().map(|c| c.refresh).unwrap_or_default();
    let mut ai = ai_config
        .map(xtion_vision::ai::Interpreter::start)
        .transpose()?;
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
        let s = std::sync::Arc::new(engine.observe(&frame));
        let report = ai.as_mut().and_then(|a| {
            a.submit(s.clone(), std::sync::Arc::new(frame), refresh);
            a.latest(&s)
        });
        println!(
            "{}",
            serde_json::json!({"type":"observation","description":describe(&s),"scene":*s,"ai":report})
        );
    }
    src.stop()?;
    if let Some(w) = writer {
        w.finish()?;
    }
    Ok(())
}
