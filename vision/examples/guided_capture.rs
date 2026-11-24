//! Local cue-driven data collection. Run inside a visible terminal.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, BufReader, BufWriter, Write},
    path::Path,
    process::Command,
};
use xtion_vision::{
    image,
    source::{FrameSource, LiveSource, Recorder, ReplaySource, Result, SyntheticSource},
};

const SCRIPT: &[(&str, &str, u64)] = &[
    ("empty", "Leave the field of view", 20),
    (
        "enter",
        "Enter from the side, stop in the centre and stand still",
        12,
    ),
    ("lateral", "Walk laterally across the view", 10),
    ("range", "Approach the sensor, then retreat", 12),
    ("arm", "Raise one arm, pause, then lower it", 10),
    ("sit", "Sit down, stay seated, then stand up", 15),
    ("reach", "Reach towards a visible object", 10),
    ("leave", "Leave the field of view", 10),
    (
        "occupied",
        "Stand in view, then move after five seconds",
        12,
    ),
    ("object", "Place the known-size object at two distances", 18),
];

#[derive(Serialize, Deserialize)]
struct Interval {
    cue: String,
    start_frame: u64,
    end_frame_inclusive: u64,
    start_ns: u64,
    end_ns: u64,
    tolerance_ms: u64,
    status: String,
    label: String,
}
fn digest(path: &Path) -> Result<String> {
    let r = Command::new("sha256sum").arg("--").arg(path).output()?;
    if !r.status.success() {
        return Err("checksum failed".into());
    }
    Ok(String::from_utf8(r.stdout)?
        .split_whitespace()
        .next()
        .ok_or("missing checksum")?
        .into())
}
fn save(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut out = BufWriter::new(File::create_new(path)?);
    serde_json::to_writer_pretty(&mut out, value)?;
    out.flush()?;
    Ok(())
}
fn cue(message: &str) {
    println!("\x1b[2J\x1b[H\x1b[1;33m\x1b#3{message}\n\x1b#4{message}\x1b[0m\x07");
    let _ = io::stdout().flush();
}
fn capture(dir: &Path, synthetic: bool) -> Result<()> {
    fs::create_dir(dir)?;
    let mut manifest = Vec::new();
    let mut live = if synthetic {
        None
    } else {
        Some(LiveSource::open()?)
    };
    let result = (|| -> Result<()> {
        for split in ["development", "held-out"] {
            for &(name, message, seconds) in SCRIPT {
                let base = format!("{split}-{name}");
                let file = dir.join(format!("{base}.xtv"));
                let mut writer = Recorder::new(BufWriter::new(File::create_new(&file)?))?;
                let mut sim = SyntheticSource::default();
                let src: &mut dyn FrameSource = match &mut live {
                    Some(s) => s,
                    None => &mut sim,
                };
                // Drain startup data and give time to take the starting position.
                cue(&format!("Prepare: {message}"));
                let prep = if synthetic { 80_000_000 } else { 4_000_000_000 };
                let mut first = None;
                loop {
                    let f = src.next_frame()?.ok_or("source ended during preparation")?;
                    if f.info
                        .arrival_ns
                        .saturating_sub(*first.get_or_insert(f.info.arrival_ns))
                        >= prep
                    {
                        break;
                    }
                }
                cue(message);
                if !synthetic {
                    let _ = Command::new("pw-play")
                        .arg("/usr/share/sounds/freedesktop/stereo/bell.oga")
                        .status();
                }
                let duration = if synthetic {
                    480_000_000
                } else {
                    seconds * 1_000_000_000
                };
                let mut start = None;
                let mut count = 0;
                let mut remaining = None;
                let end = loop {
                    let f = src.next_frame()?.ok_or("source ended during cue")?;
                    let t = f.info.arrival_ns;
                    let elapsed = t.saturating_sub(*start.get_or_insert(t));
                    let left = duration.saturating_sub(elapsed).div_ceil(1_000_000_000);
                    if remaining.replace(left) != Some(left) {
                        println!("{left} s remaining");
                    }
                    writer.frame(&f)?;
                    count += 1;
                    if elapsed >= duration {
                        break t;
                    }
                };
                writer.finish()?;
                let interval = Interval {
                    cue: message.into(),
                    start_frame: 0,
                    end_frame_inclusive: count - 1,
                    start_ns: start.unwrap(),
                    end_ns: end,
                    tolerance_ms: 1000,
                    status: "instructed".into(),
                    label: name.into(),
                };
                save(&dir.join(format!("{base}.cue.json")), &interval)?;
                manifest.push(serde_json::json!({"recording":format!("{base}.xtv"),"split":split,"sha256":digest(&file)?,"cue":format!("{base}.cue.json")}));
            }
        }
        save(
            &dir.join("manifest.json"),
            &serde_json::json!({"schema":"xtion.cues/1","source":if synthetic{"synthetic"}else{"sensor"},"recordings":manifest}),
        )
    })();
    if let Some(s) = live.as_mut() {
        s.stop()?;
    }
    result
}
fn review(path: &Path) -> Result<()> {
    let mut label: Interval =
        serde_json::from_reader(File::open(path.with_extension("cue.json"))?)?;
    let mut src = ReplaySource::new(BufReader::new(File::open(path)?), false)?;
    let dir = path.with_extension("review");
    fs::create_dir_all(&dir)?;
    let mut index = 0usize;
    eprintln!(
        "Cue: {}. Enter steps to the next image. Type accept or correct LABEL START END (inclusive frame indices) after inspecting the interval.",
        label.cue
    );
    let mut stamps = vec![];
    while let Some(f) = src.next_frame()? {
        stamps.push(f.info.arrival_ns);
        if index.is_multiple_of(5) {
            let (w, h, rgb) = image::rgb(&f, 320, 4000);
            let png = dir.join(format!("{index:05}.png"));
            fs::write(&png, image::png(w, h, &rgb)?)?;
        }
        index += 1;
    }
    for i in (0..index)
        .step_by(5)
        .chain(std::iter::repeat(index.saturating_sub(1) / 5 * 5))
    {
        let png = dir.join(format!("{i:05}.png"));
        let mut view = Command::new("ffplay")
            .args(["-loglevel", "error", "-loop", "1", "-i"])
            .arg(&png)
            .spawn()?;
        println!("Frame {i}/{} · {} ns", index - 1, stamps[i]);
        let mut line = String::new();
        let n = io::stdin().read_line(&mut line)?;
        let _ = view.kill();
        let _ = view.wait();
        if n == 0 {
            return Err("review incomplete".into());
        }
        let cmd: Vec<_> = line.split_whitespace().collect();
        match cmd.as_slice() {
            [] => continue,
            ["accept"] => {}
            ["correct", name, start, end] => {
                let (a, b) = (start.parse::<usize>()?, end.parse::<usize>()?);
                if a > b || b >= stamps.len() {
                    return Err("invalid reviewed bounds".into());
                }
                label.label = name.to_string();
                label.start_frame = a as u64;
                label.end_frame_inclusive = b as u64;
                label.start_ns = stamps[a];
                label.end_ns = stamps[b];
            }
            _ => {
                eprintln!("Unrecognised command");
                continue;
            }
        }
        label.status = "reviewed".into();
        return save(&path.with_extension("reviewed.json"), &label);
    }
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, path] if mode == "review" => review(Path::new(path)),
        [mode, path] if mode == "capture" => capture(Path::new(path), false),
        [mode, path] if mode == "synthetic" => capture(Path::new(path), true),
        _ => Err("usage: guided_capture capture|synthetic NEW_DIR, or review FILE.xtv".into()),
    }
}
