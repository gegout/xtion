//! Opt-in hardware checks. The default test suite never opens USB.
use std::{
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};
use xtion_vision::pipeline::{Input, Pipeline};

fn run_for(seconds: u64) {
    let duration = Duration::from_secs(seconds);
    let pipeline = Pipeline::bounded(Input::Live, 100, None, Some(duration));
    let started = Instant::now();
    while !pipeline.finished.load(Ordering::Acquire) {
        assert!(
            started.elapsed() < duration + Duration::from_secs(15),
            "source exceeded shutdown budget"
        );
        thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(&*pipeline.status.lock().unwrap(), "stopped");
    let snapshot = pipeline
        .latest
        .lock()
        .unwrap()
        .clone()
        .expect("no complete frames");
    assert!(snapshot.frame.info.metric);
    assert!(snapshot.processed >= seconds * 15 && snapshot.fps >= 25.);
    assert_eq!(pipeline.drops.load(Ordering::Relaxed), 0);
}
#[test]
#[ignore = "opens and streams from the physical Xtion five times"]
fn five_clean_restarts() {
    for _ in 0..5 {
        run_for(5)
    }
}

#[test]
#[ignore = "requires exclusive access to the physical sensor for ten minutes"]
fn ten_minute_perception_soak() {
    run_for(603)
}
