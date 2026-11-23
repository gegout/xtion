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
    pub frame: Arc<DepthFrame>,
    pub received: Instant,
    pub processed: u64,
    pub fps: f64,
}
struct Queue<T> {
    state: Mutex<(VecDeque<T>, bool)>,
    wake: Condvar,
    drops: Arc<AtomicU64>,
}
impl<T> Queue<T> {
    fn new(drops: Arc<AtomicU64>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::default(),
            wake: Condvar::new(),
            drops,
        })
    }
    fn push(&self, item: T) {
        let mut state = self.state.lock().unwrap();
        if state.0.len() >= 4 {
            state.0.pop_front();
            self.drops.fetch_add(1, Relaxed);
        }
        state.0.push_back(item);
        self.wake.notify_one();
    }
    fn pop(&self) -> Option<T> {
        let guard = self.state.lock().unwrap();
        let mut state = self
            .wake
            .wait_while(guard, |(queue, closed)| queue.is_empty() && !*closed)
            .unwrap();
        state.0.pop_front()
    }
    fn close(&self) {
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
        let mut p = Self {
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
                while !stopping() {
                    let Some(f) = src.next_frame()? else { break };
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
            let mut processed = 0;
            while let Some((received, frame)) = frames.pop() {
                if recal.swap(false, Relaxed) {
                    engine.recalibrate()
                }
                engine.threshold_mm = threshold.load(Relaxed);
                let scene = Arc::new(engine.observe(&frame));
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
                *latest.lock().unwrap() = Some(Snapshot {
                    scene,
                    frame,
                    received,
                    processed,
                    fps,
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
        if let Some(s) = self.latest.lock().unwrap().as_ref() {
            eprintln!(
                "{}",
                serde_json::json!({"type":"capture_summary","processed":s.processed,"fps":s.fps,"stage_drops":self.drops.load(Relaxed),"recording_drops":self.recording_drops.load(Relaxed),"health":s.scene.health,"status":*self.status.lock().unwrap()})
            );
        }
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
