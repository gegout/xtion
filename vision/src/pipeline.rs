//! One sensor owner, independently scheduled perception and disk writing,
//! and a latest-only snapshot for rendering or JSON output.
use crate::{
    DepthFrame,
    perception::{Engine, SceneState},
    source::*,
};
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
    pub graph: Arc<crate::semantic::SemanticSceneGraph>,
    pub frame: Arc<DepthFrame>,
    pub received: Instant,
    pub processed: u64,
    pub fps: f64,
    pub recent_fps: f64,
    pub acquisition_fps: f64,
    pub processing_ms: f64,
    pub latency_ms: f64,
    pub quality: Arc<[u8]>,
}
/// Select at most one complete frame per arrival-time bin. A rate change
/// starts a fresh bin sequence; native VGA/30 passes every complete frame.
#[derive(Default)]
struct FrameSampler {
    previous: Option<(u16, u128)>,
}
impl FrameSampler {
    fn keep(&mut self, fps: u16, arrival_ns: u64) -> bool {
        let key = (
            fps,
            u128::from(arrival_ns) * u128::from(fps) / 1_000_000_000,
        );
        self.previous.replace(key) != Some(key) || fps == 30
    }
}
pub(crate) struct Queue<T> {
    state: Mutex<(VecDeque<T>, bool)>,
    wake: Condvar,
    drops: Arc<AtomicU64>,
}
impl<T> Queue<T> {
    pub(crate) fn new(drops: Arc<AtomicU64>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::default(),
            wake: Condvar::new(),
            drops,
        })
    }
    pub(crate) fn push(&self, item: T) {
        let mut state = self.state.lock().unwrap();
        if state.0.len() >= 4 {
            state.0.pop_front();
            self.drops.fetch_add(1, Relaxed);
        }
        state.0.push_back(item);
        self.wake.notify_one();
    }
    pub(crate) fn pop(&self) -> Option<T> {
        let guard = self.state.lock().unwrap();
        let mut state = self
            .wake
            .wait_while(guard, |(queue, closed)| queue.is_empty() && !*closed)
            .unwrap();
        state.0.pop_front()
    }
    pub(crate) fn close(&self) {
        self.state.lock().unwrap().1 = true;
        self.wake.notify_all();
    }
}
type RecordingJob = Option<(String, Arc<DepthFrame>)>;
fn record_loop(queue: Arc<Queue<RecordingJob>>) -> Result<()> {
    let mut writer: Option<(String, Recorder<BufWriter<File>>)> = None;
    while let Some(job) = queue.pop() {
        let path = job.as_ref().map(|(p, _)| p);
        if writer.as_ref().map(|(p, _)| p) != path {
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
        if let (Some((_, w)), Some((_, frame))) = (&mut writer, job) {
            w.frame(&frame)?
        }
    }
    if let Some((_, w)) = writer {
        w.finish()?;
    }
    Ok(())
}
pub struct Pipeline {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
    pub latest: Arc<Mutex<Option<Snapshot>>>,
    pub status: Arc<Mutex<String>>,
    pub drops: Arc<AtomicU64>,
    pub recording_drops: Arc<AtomicU64>,
    pub recording_error: Arc<Mutex<Option<String>>>,
    pub recalibrate: Arc<AtomicBool>,
    pub threshold: Arc<AtomicU16>,
    pub record: Arc<Mutex<Option<String>>>,
    pub finished: Arc<AtomicBool>,
    capture_fps: Arc<AtomicU16>,
    pub sampled_out: Arc<AtomicU64>,
}
impl Pipeline {
    pub fn start(input: Input, threshold: u16, record: Option<String>) -> Self {
        Self::bounded(input, threshold, record, None)
    }
    pub fn bounded(
        input: Input,
        threshold: u16,
        record: Option<String>,
        limit: Option<Duration>,
    ) -> Self {
        Self::with_capture_rate(input, threshold, record, limit, 30)
    }
    pub fn set_capture_rate(&self, fps: u16) {
        self.capture_fps.store(fps.clamp(5, 30), Relaxed);
    }
    pub fn with_capture_rate(
        input: Input,
        threshold: u16,
        record: Option<String>,
        limit: Option<Duration>,
        fps: u16,
    ) -> Self {
        let mut p = Self {
            capture_fps: Arc::new(AtomicU16::new(fps.clamp(5, 30))),
            sampled_out: Default::default(),
            stop: Default::default(),
            threads: vec![],
            latest: Default::default(),
            status: Arc::new(Mutex::new("opening".into())),
            drops: Default::default(),
            recording_drops: Default::default(),
            recording_error: Default::default(),
            recalibrate: Default::default(),
            threshold: Arc::new(AtomicU16::new(threshold)),
            record: Arc::new(Mutex::new(record)),
            finished: Default::default(),
        };
        let frames = Queue::new(p.drops.clone());
        let disk = Queue::new(p.recording_drops.clone());
        let (rec_queue, rec_error) = (disk.clone(), p.recording_error.clone());
        let recorder = thread::spawn(move || {
            if let Err(e) = record_loop(rec_queue) {
                *rec_error.lock().unwrap() = Some(e.to_string());
            }
        });
        let (stop, status, out, record_path, record_error) = (
            p.stop.clone(),
            p.status.clone(),
            frames.clone(),
            p.record.clone(),
            p.recording_error.clone(),
        );
        let rate = p.capture_fps.clone();
        let skipped = p.sampled_out.clone();
        p.threads.push(thread::spawn(move || {
            let start = Instant::now();
            let stopping = || stop.load(Relaxed) || limit.is_some_and(|d| start.elapsed() >= d);
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
                let mut last = None;
                let mut due = Instant::now();
                let mut recording = false;
                let mut sampler = FrameSampler::default();
                while !stopping() {
                    let Some(f) = src.next_frame()? else { break };
                    if !paced && !sampler.keep(rate.load(Relaxed), f.info.arrival_ns) {
                        skipped.fetch_add(1, Relaxed);
                        continue;
                    }
                    if paced {
                        if let Some(t) = last {
                            due += Duration::from_nanos(f.info.arrival_ns.saturating_sub(t));
                            while !stopping() && Instant::now() < due {
                                thread::sleep(
                                    due.saturating_duration_since(Instant::now())
                                        .min(Duration::from_millis(10)),
                                )
                            }
                        }
                        last = Some(f.info.arrival_ns)
                    }
                    let frame = Arc::new(f);
                    out.push((Instant::now(), frame.clone()));
                    if let Some(path) = record_path.lock().unwrap().clone()
                        && record_error.lock().unwrap().is_none()
                    {
                        disk.push(Some((path, frame)));
                        recording = true;
                    } else if recording {
                        disk.push(None);
                        recording = false;
                    }
                }
                src.stop()?;
                Ok(())
            };
            let result = run();
            *status.lock().unwrap() = match result {
                Ok(()) => "stopped".into(),
                Err(e) => format!("capture problem: {e}"),
            };
            disk.close();
            let _ = recorder.join();
            out.close();
        }));
        let (latest, threshold, recal, finished) = (
            p.latest.clone(),
            p.threshold.clone(),
            p.recalibrate.clone(),
            p.finished.clone(),
        );
        p.threads.push(thread::spawn(move || {
            let mut engine = Engine::default();
            let mut first = None;
            let mut semantics = crate::semantic::SemanticEngine::default();
            let mut processed = 0;
            let mut recent = VecDeque::new();
            while let Some((received, frame)) = frames.pop() {
                if recal.swap(false, Relaxed) {
                    engine.recalibrate()
                }
                engine.threshold_mm = threshold.load(Relaxed);
                let began = Instant::now();
                let scene = Arc::new(engine.observe(&frame));
                let graph = Arc::new(semantics.observe(&scene));
                let processing_ms = began.elapsed().as_secs_f64() * 1000.;
                processed += 1;
                let delta = frame
                    .info
                    .arrival_ns
                    .saturating_sub(*first.get_or_insert(frame.info.arrival_ns));
                let fps = if delta > 0 {
                    (processed - 1) as f64 * 1e9 / delta as f64
                } else {
                    0.
                };
                let now = frame.info.arrival_ns;
                recent.push_back((now, frame.info.health.frames));
                while recent.len() > 120
                    || recent
                        .front()
                        .is_some_and(|t| now.saturating_sub(t.0) > 1_000_000_000)
                {
                    recent.pop_front();
                }
                let span = now - recent[0].0;
                let acquisition_fps = if span > 0 {
                    frame.info.health.frames.saturating_sub(recent[0].1) as f64 * 1e9 / span as f64
                } else {
                    0.
                };
                let recent_fps = if span > 0 {
                    (recent.len() - 1) as f64 * 1e9 / span as f64
                } else {
                    0.
                };
                *latest.lock().unwrap() = Some(Snapshot {
                    graph,
                    scene,
                    frame,
                    received,
                    processed,
                    fps,
                    recent_fps,
                    acquisition_fps,
                    processing_ms,
                    latency_ms: received.elapsed().as_secs_f64() * 1000.,
                    quality: engine.quality.as_slice().into(),
                });
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
        let latest = self.latest.lock().unwrap();
        let snapshot = latest.as_ref();
        eprintln!(
            "{}",
            serde_json::json!({
                "type":"capture_summary", "processed":snapshot.map_or(0, |s| s.processed),
                "fps":snapshot.map(|s| s.fps), "sampled_out":self.sampled_out.load(Relaxed),
                "stage_drops":self.drops.load(Relaxed), "recording_drops":self.recording_drops.load(Relaxed),
                "health":snapshot.map(|s| &s.scene.health), "status":*self.status.lock().unwrap()
            })
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn congested_queue_discards_only_oldest() {
        let q = Queue::new(Default::default());
        for v in 0..10 {
            q.push(v);
        }
        q.close();
        assert_eq!(q.drops.load(Relaxed), 6);
        assert_eq!(
            std::iter::from_fn(|| q.pop()).collect::<Vec<_>>(),
            [6, 7, 8, 9]
        );
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    #[test]
    fn source_stops_without_any_consumer_or_repaints() {
        let p = Pipeline::bounded(
            Input::Synthetic,
            100,
            None,
            Some(Duration::from_millis(120)),
        );
        thread::sleep(Duration::from_millis(350));
        assert!(p.finished.load(Acquire));
        assert_eq!(&*p.status.lock().unwrap(), "stopped");
        assert!(p.latest.lock().unwrap().as_ref().unwrap().processed < 10);
    }
}

#[cfg(test)]
mod rate_tests {
    use super::*;
    #[test]
    fn selection_tracks_arrival_time_and_changes_immediately() {
        for fps in [5, 10, 15, 23, 30] {
            let mut s = FrameSampler::default();
            let kept = (0..300)
                .filter(|i| s.keep(fps, i * 1_000_000_000 / 30))
                .count();
            assert_eq!(kept, fps as usize * 10);
        }
        let mut s = FrameSampler::default();
        assert!(s.keep(5, 0));
        assert!(!s.keep(5, 35_000_000));
        assert!(s.keep(15, 40_000_000));
        assert!(s.keep(5, 2_000_000_000));
        assert!(!s.keep(5, 2_000_000_001));
    }
}
