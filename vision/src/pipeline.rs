//! Acquisition is single-owner. Processing drains a bounded drop-oldest queue.
//! The UI reads a single replaceable snapshot and never holds a sensor handle.
use crate::{DepthFrame, perception::*, source::*};
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufReader, BufWriter},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering::*},
    },
    thread,
    time::{Duration, Instant},
};
#[derive(Clone)]
pub enum Input {
    Live,
    Replay(String),
    Synthetic,
}
#[derive(Clone)]
pub struct Snapshot {
    pub scene: Arc<SceneState>,
    pub frame: Arc<DepthFrame>,
    pub received: Instant,
    pub processed: u64,
    pub fps: f64,
}
struct Queue {
    frames: VecDeque<(Instant, DepthFrame)>,
    ended: bool,
}
pub struct Pipeline {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
    pub latest: Arc<Mutex<Option<Snapshot>>>,
    pub status: Arc<Mutex<String>>,
    pub drops: Arc<AtomicU64>,
    pub recalibrate: Arc<AtomicBool>,
    pub threshold: Arc<AtomicU16>,
    pub record: Arc<Mutex<Option<String>>>,
    pub finished: Arc<AtomicBool>,
}
impl Pipeline {
    pub fn start(input: Input, threshold: u16, record: Option<String>) -> Self {
        let mut p = Self {
            stop: Default::default(),
            threads: vec![],
            latest: Default::default(),
            status: Arc::new(Mutex::new("opening".into())),
            drops: Default::default(),
            recalibrate: Default::default(),
            threshold: Arc::new(AtomicU16::new(threshold)),
            record: Arc::new(Mutex::new(record)),
            finished: Default::default(),
        };
        let q = Arc::new((
            Mutex::new(Queue {
                frames: VecDeque::new(),
                ended: false,
            }),
            Condvar::new(),
        ));
        let (stop, drops, status, queue) =
            (p.stop.clone(), p.drops.clone(), p.status.clone(), q.clone());
        p.threads.push(thread::spawn(move || {
            let run = || -> Result<()> {
                let paced = !matches!(input, Input::Live);
                let mut src: Box<dyn FrameSource> = match input {
                    Input::Live => Box::new(LiveSource::open()?),
                    Input::Replay(path) => {
                        Box::new(ReplaySource::new(BufReader::new(File::open(path)?), false)?)
                    }
                    Input::Synthetic => Box::new(SyntheticSource::default()),
                };
                *status.lock().unwrap() = "receiving".into();
                let mut last: Option<u64> = None;
                let mut due = Instant::now();
                while !stop.load(Relaxed) {
                    let Some(f) = src.next_frame()? else { break };
                    if paced {
                        if let Some(t) = last {
                            due += Duration::from_nanos(f.info.arrival_ns.saturating_sub(t));
                            while !stop.load(Relaxed) && Instant::now() < due {
                                thread::sleep(
                                    due.saturating_duration_since(Instant::now())
                                        .min(Duration::from_millis(10)),
                                )
                            }
                        }
                        last = Some(f.info.arrival_ns)
                    }
                    let mut q = queue.0.lock().unwrap();
                    if q.frames.len() == 4 {
                        q.frames.pop_front();
                        drops.fetch_add(1, Relaxed);
                    }
                    q.frames.push_back((Instant::now(), f));
                    queue.1.notify_one();
                }
                src.stop()?;
                Ok(())
            };
            let result = run();
            *status.lock().unwrap() = match result {
                Ok(()) => "stopped".into(),
                Err(e) => format!("capture problem: {e}"),
            };
            queue.0.lock().unwrap().ended = true;
            queue.1.notify_all();
        }));
        let (latest, threshold, recalibrate, record, status, finished) = (
            p.latest.clone(),
            p.threshold.clone(),
            p.recalibrate.clone(),
            p.record.clone(),
            p.status.clone(),
            p.finished.clone(),
        );
        p.threads.push(thread::spawn(move || {
            let mut engine = Engine::default();
            let mut writer: Option<(String, Recorder<BufWriter<File>>)> = None;
            let mut processed = 0;
            let mut first = None;
            loop {
                let mut guard = q.0.lock().unwrap();
                while guard.frames.is_empty() && !guard.ended {
                    guard = q.1.wait(guard).unwrap()
                }
                let Some((received, f)) = guard.frames.pop_front() else {
                    break;
                };
                drop(guard);
                let path = record.lock().unwrap().clone();
                let recording = (|| -> Result<()> {
                    if writer.as_ref().map(|(p, _)| p) != path.as_ref() {
                        if let Some((_, w)) = writer.take() {
                            w.finish()?;
                        }
                        if let Some(p) = path {
                            writer = Some((
                                p.clone(),
                                Recorder::new(BufWriter::new(File::create_new(p)?))?,
                            ))
                        }
                    }
                    if let Some((_, w)) = &mut writer {
                        w.frame(&f)?
                    }
                    Ok(())
                })();
                if let Err(e) = recording {
                    *status.lock().unwrap() = format!("recording failed: {e}");
                    *record.lock().unwrap() = None;
                    writer = None;
                }
                if recalibrate.swap(false, Relaxed) {
                    engine.recalibrate()
                }
                engine.threshold_mm = threshold.load(Relaxed);
                let scene = engine.observe(&f);
                processed += 1;
                let delta = f
                    .info
                    .arrival_ns
                    .saturating_sub(*first.get_or_insert(f.info.arrival_ns));
                let fps = if delta > 0 {
                    (processed - 1) as f64 * 1e9 / delta as f64
                } else {
                    0.
                };
                *latest.lock().unwrap() = Some(Snapshot {
                    scene: Arc::new(scene),
                    frame: Arc::new(f),
                    received,
                    processed,
                    fps,
                });
            }
            if let Some((_, w)) = writer
                && let Err(e) = w.finish()
            {
                *status.lock().unwrap() = format!("recording flush failed: {e}")
            }
            finished.store(true, Release);
        }));
        p
    }
    pub fn request_stop(&self) {
        self.stop.store(true, Release)
    }
}
impl Drop for Pipeline {
    fn drop(&mut self) {
        self.request_stop();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}
