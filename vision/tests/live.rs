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

#[test]
#[ignore = "requires exclusive access to the physical sensor"]
fn adjust_app_capture_rate_without_reopening() {
    let pipeline =
        Pipeline::with_capture_rate(Input::Live, 100, None, Some(Duration::from_secs(15)), 5);
    for fps in [5, 15, 30] {
        pipeline.set_capture_rate(fps);
        let start = Instant::now();
        thread::sleep(Duration::from_millis(1600));
        loop {
            if let Some(s) = pipeline.latest.lock().unwrap().as_ref()
                && s.processed > 2
                && (s.recent_fps - f64::from(fps)).abs() < 1.5
            {
                println!("target {fps}, measured {:.2}", s.recent_fps);
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(4),
                "rate change did not settle"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
    pipeline.request_stop();
    let stop = Instant::now();
    while !pipeline.finished.load(Ordering::Acquire) {
        assert!(stop.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(&*pipeline.status.lock().unwrap(), "stopped");
}
