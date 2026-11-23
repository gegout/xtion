mod viewer;
use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use xtion_vision::{
    DepthFrame,
    ai::{Config, Interpreter},
    describe::describe,
    geometry::{CloudExport, Ply},
    perception::{Engine, SceneState},
    pipeline::{Input, Pipeline},
    source::*,
};
const HELP: &str = "xtion-vision [--headless] [--replay PATH | --synthetic] [--fast] [--seconds N]
    --record NEW.xtv    Save raw depth, clocks and health
    --png VIEW.png      Save desktop window
    --ply NEW.ply       Export first metric cloud in headless mode
    --threshold-mm N    Minimum foreground depth difference (40..65535)
    --ai-url BASE       Enable AI at this Chat Completions endpoint
    --ai-model NAME     Skip model discovery
    --ai-refresh-ms N --ai-timeout-ms N --ai-budget-ms N --ai-candidates N --ai-probe-tokens N --ai-tokens N";
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
struct Output {
    ai: Option<Interpreter>,
    refresh: Duration,
    last_event: Option<String>,
    ply: Option<String>,
    stdout: io::Stdout,
}
impl Output {
    fn emit(&mut self, s: Arc<SceneState>, f: Arc<DepthFrame>) -> Result<()> {
        if self.ply.is_some()
            && let Some(cloud) = f.point_cloud()
        {
            let mut out = BufWriter::new(File::create_new(self.ply.take().unwrap())?);
            Ply.export(&cloud, &mut out)?;
            out.flush()?
        }
        let report = self.ai.as_mut().and_then(|a| {
            a.submit(s.clone(), f, self.refresh);
            a.latest(&s)
        });
        let mut w = self.stdout.lock();
        let offset = self
            .last_event
            .as_ref()
            .and_then(|id| s.events.iter().position(|e| &e.id == id))
            .map_or(0, |k| k + 1);
        for e in &s.events[offset..] {
            writeln!(w, "{}", serde_json::json!({"type":"event","event":e}))?
        }
        self.last_event = s.events.last().map(|e| e.id.clone());
        writeln!(
            w,
            "{}",
            serde_json::json!({"type":"observation","description":describe(&s),"scene":*s,"ai":report})
        )?;
        Ok(())
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|v| v == "--help" || v == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--headless" | "--fast" | "--synthetic" => {}
            "--replay" | "--record" | "--png" | "--ply" | "--seconds" | "--threshold-mm"
            | "--ai-url" | "--ai-model" | "--ai-refresh-ms" | "--ai-timeout-ms"
            | "--ai-budget-ms" | "--ai-candidates" | "--ai-tokens" | "--ai-probe-tokens" => {
                idx += 1;
                if args
                    .get(idx)
                    .is_none_or(|v| v.starts_with("--") || v.is_empty())
                {
                    return Err("missing flag value".into());
                }
            }
            v => return Err(format!("unknown option {v}; see --help").into()),
        }
        idx += 1
    }
    let val = |k: &str| args.windows(2).find(|a| a[0] == k).map(|a| a[1].as_str());
    let flag = |k: &str| args.iter().any(|a| a == k);
    if flag("--replay") && flag("--synthetic") {
        return Err("choose one frame source".into());
    }
    let input = if let Some(p) = val("--replay") {
        Input::Replay(p.into())
    } else if flag("--synthetic") {
        Input::Synthetic
    } else {
        Input::Live
    };
    let threshold = val("--threshold-mm").unwrap_or("100").parse()?;
    if threshold < 40 {
        return Err("threshold must be at least 40 mm".into());
    }
    let seconds: Option<f64> = val("--seconds").map(str::parse).transpose()?;
    if seconds.is_some_and(|s| !s.is_finite() || s <= 0. || s >= u64::MAX as f64) {
        return Err("seconds must be positive and finite".into());
    }
    let record = val("--record").map(str::to_owned);
    let mut config = val("--ai-url").map(|s| Config::new(s.into()));
    if let Some(c) = &mut config {
        c.model = val("--ai-model").map(str::to_owned);
        c.candidates = val("--ai-candidates").unwrap_or("4").parse()?;
        c.timeout = Duration::from_millis(val("--ai-timeout-ms").unwrap_or("8000").parse()?);
        c.budget = Duration::from_millis(val("--ai-budget-ms").unwrap_or("30000").parse()?);
        c.probe_tokens = val("--ai-probe-tokens").unwrap_or("4").parse()?;
        c.tokens = val("--ai-tokens").unwrap_or("400").parse()?;
        c.refresh = Duration::from_millis(val("--ai-refresh-ms").unwrap_or("2000").parse()?);
    } else if flag("--ai-model") {
        return Err("--ai-model requires --ai-url".into());
    }
    if !flag("--headless") {
        if flag("--fast") || flag("--ply") {
            return Err("--fast and --ply require --headless".into());
        }
        if std::env::var("LIBGL_ALWAYS_SOFTWARE").as_deref() != Ok("1") {
            use std::os::unix::process::CommandExt;
            return Err(std::process::Command::new(std::env::current_exe()?)
                .args(&args)
                .env("LIBGL_ALWAYS_SOFTWARE", "1")
                .exec()
                .into());
        }
        return viewer::launch(viewer::Options {
            input,
            ai: config,
            threshold,
            record,
            screenshot: val("--png").map(str::to_owned),
            seconds,
        });
    }
    if flag("--png") {
        return Err("--png requires the desktop viewer".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag_stop = stop.clone();
    ctrlc::set_handler(move || flag_stop.store(true, Ordering::Release))?;
    let start = Instant::now();
    let done = || {
        stop.load(Ordering::Acquire) || seconds.is_some_and(|s| start.elapsed().as_secs_f64() >= s)
    };
    let mut out = Output {
        refresh: config.as_ref().map(|c| c.refresh).unwrap_or_default(),
        ai: config.map(Interpreter::start).transpose()?,
        ply: val("--ply").map(str::to_owned),
        last_event: None,
        stdout: io::stdout(),
    };
    if matches!(input, Input::Live) || matches!(input, Input::Replay(_)) && !flag("--fast") {
        let pipe = Pipeline::bounded(
            input,
            threshold,
            record,
            seconds.map(Duration::from_secs_f64),
        );
        let mut seen = 0;
        let mut stale = false;
        loop {
            let snap = pipe.latest.lock().unwrap().clone();
            if let Some(s) = snap {
                if s.processed != seen {
                    seen = s.processed;
                    out.emit(s.scene, s.frame)?;
                    stale = false
                } else if !stale && s.received.elapsed() > Duration::from_millis(500) {
                    writeln!(
                        out.stdout,
                        "{}",
                        serde_json::json!({"type":"capture_problem","status":"stale","last_frame":seen})
                    )?;
                    stale = true
                }
            }
            if done() {
                pipe.request_stop()
            }
            if pipe.finished.load(Ordering::Acquire) {
                break;
            }
            thread::sleep(Duration::from_millis(4))
        }
        if let Some(e) = pipe.recording_error.lock().unwrap().as_ref() {
            return Err(format!("recording failed: {e}").into());
        }
        let status = pipe.status.lock().unwrap().clone();
        if status != "stopped" {
            writeln!(
                out.stdout,
                "{}",
                serde_json::json!({"type":"capture_problem","status":"stale","message":status})
            )?;
            return Err(status.into());
        }
    } else {
        let mut src: Box<dyn FrameSource> = match input {
            Input::Synthetic => Box::new(SyntheticSource::default()),
            Input::Replay(p) => Box::new(ReplaySource::new(BufReader::new(File::open(p)?), false)?),
            Input::Live => unreachable!(),
        };
        let mut writer = record
            .map(|p| Recorder::new(BufWriter::new(File::create_new(p)?)))
            .transpose()?;
        let mut engine = Engine::default();
        engine.threshold_mm = threshold;
        while !done() {
            let Some(f) = src.next_frame()? else { break };
            if let Some(w) = &mut writer {
                w.frame(&f)?
            }
            out.emit(Arc::new(engine.observe(&f)), Arc::new(f))?
        }
        src.stop()?;
        if let Some(w) = writer {
            w.finish()?;
        }
    }
    Ok(())
}
