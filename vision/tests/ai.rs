#![cfg(feature = "ai")]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use xtion_vision::{ai::*, perception::*, source::*};
struct Mock {
    url: String,
    n: Arc<AtomicUsize>,
}
fn mock(reply: impl Fn(&str, Value) -> (u16, Value) + Send + 'static) -> Mock {
    let sock = TcpListener::bind("127.0.0.1:0").unwrap();
    sock.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", sock.local_addr().unwrap());
    let n = Arc::new(AtomicUsize::default());
    let counter = Arc::downgrade(&n);
    thread::spawn(move || {
        while let Some(count) = counter.upgrade() {
            let Ok((mut stream, _)) = sock.accept() else {
                drop(count);
                thread::sleep(Duration::from_millis(1));
                continue;
            };
            count.fetch_add(1, Ordering::SeqCst);
            let mut r = BufReader::new(&stream);
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap().to_string();
            let mut len = 0;
            loop {
                line.clear();
                r.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
                    len = n.trim().parse().unwrap()
                }
            }
            let mut b = vec![0; len];
            r.read_exact(&mut b).unwrap();
            let (status, v) = reply(&path, serde_json::from_slice(&b).unwrap_or(Value::Null));
            if status == 0 {
                continue;
            }
            let data = v.to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                data.len(),
                data
            );
        }
    });
    Mock { url, n }
}
fn chat(text: &str) -> (u16, Value) {
    (
        200,
        json!({"choices":[{"message":{"content":text}}],"usage":{"total_tokens":3}}),
    )
}
fn job() -> (Arc<SceneState>, Arc<xtion_vision::DepthFrame>) {
    let mut e = Engine::default();
    let mut src = SyntheticSource::default();
    loop {
        let f = src.next_frame().unwrap().unwrap();
        let s = e.observe(&f);
        if s.objects.len() == 1 {
            return (Arc::new(s), Arc::new(f));
        }
    }
}
fn worker(m: &Mock, explicit: bool, timeout: u64) -> Interpreter {
    let mut c = Config::new(m.url.clone());
    c.model = explicit.then(|| "vision".into());
    c.timeout = Duration::from_millis(timeout);
    Interpreter::start(c).unwrap()
}
fn poll(a: &Interpreter, s: &SceneState) -> Report {
    let start = Instant::now();
    loop {
        if let Some(r) = a.latest(s) {
            return r;
        }
        assert!(start.elapsed() < Duration::from_secs(6));
        thread::sleep(Duration::from_millis(5))
    }
}
fn run(m: &Mock, explicit: bool, timeout: u64) -> Report {
    let mut a = worker(m, explicit, timeout);
    let (s, f) = job();
    a.submit(s.clone(), f, Duration::ZERO);
    poll(&a, &s)
}
#[test]
fn discovery_and_known_answer() {
    let m = mock(|p, b| {
        if p.ends_with("/models") {
            return (
                200,
                json!({"data":[{"id":"embedding"},{"id":"guesser"},{"id":"vision"}]}),
            );
        }
        assert_ne!(b["model"], "embedding");
        if b["max_tokens"] == 4 {
            if b["model"] == "guesser" {
                return chat("TL");
            }
            let url = b["messages"][0]["content"][1]["image_url"]["url"]
                .as_str()
                .unwrap();
            let bytes = STANDARD.decode(url.split_once(',').unwrap().1).unwrap();
            let mut r = png::Decoder::new(std::io::Cursor::new(bytes))
                .read_info()
                .unwrap();
            let mut rgb = vec![0; r.output_buffer_size().unwrap()];
            r.next_frame(&mut rgb).unwrap();
            let q = [0, 16, 512, 528]
                .into_iter()
                .position(|k| rgb[k * 3] == 255)
                .unwrap();
            return chat(["TL", "TR", "BL", "BR"][q]);
        }
        chat("{\"claims\":[]}")
    });
    let r = run(&m, false, 500);
    assert!(r.success);
    assert_eq!(r.model, "vision");
}
#[test]
fn missing_listing_and_blind_model() {
    let m = mock(|_, _| (404, Value::Null));
    assert!(run(&m, false, 500).text.contains("listing"));
    let m = mock(|p, _| {
        if p.ends_with("models") {
            (200, json!({"data":[{"id":"blind"}]}))
        } else {
            chat("TL")
        }
    });
    assert!(!run(&m, false, 500).success);
}
#[test]
fn malformed_unknown_contradiction() {
    let (s, _) = job();
    for (id, z) in [
        ("bad", 1.5),
        (s.objects[0].observation_id.as_str(), 99.),
        ("", 0.),
    ] {
        let body = if id.is_empty() {
            "malformed".into()
        } else {
            json!({"claims":[{"observation_id":id,"z_m":z,"activity":"steady","confidence":0.5,"hypothesis":"unclassified surface","relative_to":"","relationship":"none"}]}).to_string()
        };
        let m = mock(move |p, _| {
            assert!(p.ends_with("completions"));
            chat(&body)
        });
        assert!(!run(&m, true, 500).success);
        assert_eq!(m.n.load(Ordering::SeqCst), 2)
    }
}
#[test]
fn timeout_stale_and_endpoint_loss() {
    let m = mock(|_, _| {
        thread::sleep(Duration::from_millis(180));
        chat("{}")
    });
    assert!(run(&m, true, 40).text.contains("timed out"));
    let m = mock(|_, _| (0, Value::Null));
    assert!(!run(&m, true, 400).success);
    let m = mock(|_, _| chat("{\"claims\":[]}"));
    let mut a = worker(&m, true, 500);
    let (s, f) = job();
    a.submit(s.clone(), f, Duration::ZERO);
    assert!(poll(&a, &s).success);
    let mut other = (*s).clone();
    other.objects.clear();
    assert!(a.latest(&other).is_none());
}
#[test]
fn explicit_mode_ladder_and_disabled() {
    let m = mock(|p, b| {
        assert!(p.ends_with("completions"));
        if !b["response_format"].is_null() {
            (400, Value::Null)
        } else {
            chat("Here is JSON: {\"claims\":[]}")
        }
    });
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xtion-vision"))
        .env("OPENAI_BASE_URL", &m.url)
        .args(["--headless", "--synthetic"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(m.n.load(Ordering::SeqCst), 0);
    assert!(run(&m, true, 500).success);
    assert_eq!(m.n.load(Ordering::SeqCst), 3);
}

#[test]
fn endpoint_dies_after_success_then_recovers_without_blocking_capture() {
    let calls = AtomicUsize::new(0);
    let m = mock(move |_, _| {
        if calls.fetch_add(1, Ordering::SeqCst) == 1 {
            (0, Value::Null)
        } else {
            chat("{\"claims\":[]}")
        }
    });
    let mut a = worker(&m, true, 500);
    let (s, f) = job();
    a.submit(s.clone(), f.clone(), Duration::ZERO);
    assert!(poll(&a, &s).success);
    let mut changed = (*s).clone();
    changed.objects[0].region.z += 1.;
    let changed = Arc::new(changed);
    a.submit(changed.clone(), f.clone(), Duration::ZERO);
    assert!(!poll(&a, &changed).success);
    let mut next = (*changed).clone();
    next.objects.clear();
    let next = Arc::new(next);
    a.submit(next.clone(), f, Duration::ZERO);
    assert!(poll(&a, &next).success);
}

#[test]
fn responses_requires_an_advertised_operation() {
    let m = mock(|p, b| match p {
        "/v1/chat/completions" => (405, Value::Null),
        "/v1/openapi.json" => (200, json!({"paths":{"/v1/responses":{"post":{}}}})),
        "/v1/responses" => {
            assert_eq!(b["input"][0]["content"][0]["type"], "input_text");
            assert_eq!(b["text"]["format"]["type"], "json_schema");
            (
                200,
                json!({"output":[{"content":[{"type":"output_text","text":"{\"claims\":[]}"}]}]}),
            )
        }
        _ => panic!("unexpected route"),
    });
    assert!(run(&m, true, 500).success);
    let unsupported = mock(|p, _| {
        assert!(!p.ends_with("/responses"));
        (404, Value::Null)
    });
    assert!(!run(&unsupported, true, 500).success);
}

#[test]
fn explicit_text_model_selects_grounded_measurements_after_image_rejection() {
    let calls = AtomicUsize::new(0);
    let m = mock(move |_, body| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            assert!(body["messages"][0]["content"].is_array());
            return (
                500,
                json!({"error":{"message":"image input is not supported - hint: missing projector"}}),
            );
        }
        assert!(body["messages"][0]["content"].is_string());
        let choices =
            &body["response_format"]["json_schema"]["schema"]["properties"]["index"]["enum"];
        chat(if choices.as_array().unwrap().len() == 1 {
            r#"{"index":0}"#
        } else {
            r#"{"index":1}"#
        })
    });
    let transcript = Transcript::default();
    let mut c = Config::new(m.url.clone());
    c.model = Some("vision".into());
    c.transcript = Some(transcript.clone());
    let mut a = Interpreter::start(c).unwrap();
    let (s, f) = job();
    a.submit(s.clone(), f.clone(), Duration::ZERO);
    let r = poll(&a, &s);
    let exchanges = transcript.snapshot();
    assert_eq!(exchanges.len(), 2);
    assert!(exchanges[0].status.contains("500"));
    assert!(
        exchanges[0]
            .response
            .as_ref()
            .unwrap()
            .contains("missing projector")
    );
    assert!(exchanges[0].request.contains("data:image/png;base64,"));
    let sent: Value = serde_json::from_str(&exchanges[1].request).unwrap();
    assert_eq!(
        exchanges[1].prompt.as_ref(),
        sent["messages"][0]["content"].as_str().unwrap()
    );
    assert!(
        exchanges[1].prompt.contains("30 seconds") && exchanges[1].prompt.contains("Candidates:")
    );
    let raw: Value = serde_json::from_str(exchanges[1].response.as_ref().unwrap()).unwrap();
    assert_eq!(
        exchanges[1].answer.as_deref(),
        raw["choices"][0]["message"]["content"].as_str()
    );
    assert!(exchanges[1].status.contains("200"));
    assert!(r.success);
    assert!((20..=40).contains(&r.text.split_whitespace().count()));
    assert_eq!(r.supporting_ids.as_slice(), std::slice::from_ref(&s.id));
    assert!(r.claims.is_empty());
    let mut next = (*s).clone();
    next.objects.clear();
    assert!(
        a.latest(&next).is_some(),
        "an unrelated track does not invalidate a sector measurement"
    );
    next.sector_z_m[0] = Some(0.5);
    assert!(a.latest(&next).is_none(), "the selected surface has moved");
    let next = Arc::new(next);
    a.submit(next.clone(), f, Duration::ZERO);
    assert!(poll(&a, &next).success);
    assert_eq!(m.n.load(Ordering::SeqCst), 3);
}
#[test]
fn event_bursts_wait_for_refresh_and_replace_pending_frames() {
    let m = mock(|_, _| chat("{\"claims\":[]}"));
    let mut a = worker(&m, true, 500);
    let (s, f) = job();
    let refresh = Duration::from_millis(350);
    a.submit(s.clone(), f.clone(), refresh);
    assert!(poll(&a, &s).success);
    let mut next = (*s).clone();
    for k in 1..6 {
        next.id = format!("updated{k}");
        next.objects[0].region.z += 0.3;
        next.events.last_mut().unwrap().id = format!("event{k}");
        a.submit(Arc::new(next.clone()), f.clone(), refresh);
    }
    thread::sleep(Duration::from_millis(80));
    assert_eq!(m.n.load(Ordering::SeqCst), 1);
    assert_eq!(poll(&a, &next).scene_id, "updated5");
    assert_eq!(m.n.load(Ordering::SeqCst), 2);
}

#[test]
#[ignore = "requires the local smollm3-3b Chat endpoint on port 8366; uses synthetic observations"]
fn local_model_prioritizes_depth_evidence() {
    let (scene, frame) = job();
    let mut c = Config::new("http://localhost:8366/v1".into());
    c.model = Some("smollm3-3b".into());
    c.timeout = Duration::from_secs(25);
    let mut ai = Interpreter::start(c).unwrap();
    for (id, expected) in [
        ("approach", "approaching the sensor"),
        ("retreat", "moving away from the sensor"),
        ("layout", "Median surface depth"),
        ("occluded", "insufficient depth coverage"),
    ] {
        let mut s = (*scene).clone();
        s.id = id.into();
        s.status = "tracking";
        s.sector_z_m = [Some(2.8), Some(1.4), Some(3.2)];
        match id {
            "layout" => s.objects.clear(),
            "occluded" => {
                s.objects.clear();
                s.status = "insufficient depth coverage";
                s.valid_fraction = 0.;
                s.sector_z_m = [None; 3]
            }
            _ => {
                let o = &mut s.objects[0];
                o.moving = true;
                o.direction = if id == "approach" {
                    "approaching"
                } else {
                    "receding"
                };
                o.region.z = 1.4;
            }
        }
        let s = Arc::new(s);
        ai.submit(s.clone(), frame.clone(), Duration::ZERO);
        let start = Instant::now();
        loop {
            if let Some(r) = ai.latest(&s)
                && r.scene_id == id
            {
                println!("{} ms: {}", r.latency_ms, r.text);
                assert!(r.success && r.text.contains(expected));
                assert!((20..=40).contains(&r.text.split_whitespace().count()));
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(60), "{}", ai.status());
            thread::sleep(Duration::from_millis(50));
        }
    }
}

fn crossing(reverse: bool, small: bool) -> (Arc<SceneState>, Arc<xtion_vision::DepthFrame>) {
    let mut frame = SyntheticSource::default().next_frame().unwrap().unwrap();
    let (w, h) = (160, 120);
    frame.info.width = w;
    frame.info.height = h;
    frame.info.intrinsics = xtion_vision::CameraIntrinsics::nominal(w, h);
    frame.pixels = vec![4000; w * h];
    let mut engine = Engine::default();
    for step in 0..65 {
        frame.info.ticks = step;
        frame.info.arrival_ns = u64::from(step) * 100_000_000;
        frame.pixels.fill(4000);
        let mut x = 24 + (step as usize).saturating_sub(10) * 2;
        x = if reverse { w - x } else { x };
        if step >= 8 {
            for y in 8..112 {
                for u in x - 16..=x + 16 {
                    let dx = u.abs_diff(x);
                    let inside = if small {
                        (50..66).contains(&y) && dx < 7
                    } else {
                        match y {
                            8..25 => dx < 8,
                            25..74 => dx < 16,
                            _ => (2..13).contains(&dx),
                        }
                    };
                    if inside {
                        frame.pixels[y * w + u] = 2000;
                    }
                }
            }
        }
        let scene = engine.observe(&frame);
        if scene.events.iter().any(|e| {
            e.passage.as_ref().is_some_and(|p| {
                p.movement()
                    == if reverse {
                        "moved from right to left"
                    } else {
                        "moved from left to right"
                    }
            })
        }) {
            return (Arc::new(scene), Arc::new(frame));
        }
    }
    panic!("no crossing was found");
}
#[test]
fn semantic_claims_require_matching_direction_and_shape_evidence() {
    for (reverse, small, subject, action, accept) in [
        (
            false,
            false,
            "possible person",
            "moved from left to right",
            true,
        ),
        (
            true,
            false,
            "unknown shape",
            "moved from left to right",
            false,
        ),
        (
            false,
            true,
            "possible person",
            "moved from left to right",
            false,
        ),
    ] {
        let (s, f) = crossing(reverse, small);
        let m = mock(move |_, b| {
            if b["messages"][0]["content"].is_array() {
                (
                    500,
                    json!({"error":{"message":"image input is not supported"}}),
                )
            } else {
                chat(&json!({"subject":subject,"action":action}).to_string())
            }
        });
        let mut a = worker(&m, true, 500);
        a.submit(s.clone(), f, Duration::ZERO);
        let r = poll(&a, &s);
        assert_eq!(r.success, accept);
        if accept {
            assert!(r.text.starts_with("A person-like figure moved from left to right. The movement lasted 4.4 seconds, during which the camera recorded the same track in 45 separate observations."));
            assert!((20..=40).contains(&r.text.split_whitespace().count()));
            assert_eq!(r.supporting_ids.len(), 3);
        }
    }
}
#[test]
fn configured_response_lengths_apply_to_published_reports() {
    let (scene, frame) = job();
    let m = mock(|_, _| chat("{\"claims\":[]}"));
    for (min, max) in [(8, 20), (16, 29), (32, 38), (40, 46)] {
        let mut c = Config::new(m.url.clone());
        assert_eq!(c.refresh, Duration::from_secs(7));
        c.model = Some("vision".into());
        c.min_words = min;
        c.max_words = max;
        let mut a = Interpreter::start(c).unwrap();
        a.submit(scene.clone(), frame.clone(), Duration::ZERO);
        let r = poll(&a, &scene);
        assert!(r.success);
        assert!(
            (min..=max).contains(&r.text.split_whitespace().count()),
            "{}",
            r.text
        );
        assert!(r.text.ends_with('.'));
    }
}
#[test]
#[ignore = "uses synthetic crossings and the local SmolLM3 endpoint"]
fn local_semantic_crossings() {
    for (reverse, small) in [(false, false), (true, false), (false, true)] {
        let (s, f) = crossing(reverse, small);
        let mut c = Config::new("http://localhost:8366/v1".into());
        c.model = Some("smollm3-3b".into());
        let mut a = Interpreter::start(c).unwrap();
        a.submit(s.clone(), f, Duration::ZERO);
        let start = Instant::now();
        loop {
            if let Some(r) = a.latest(&s) {
                println!("{} ms: {} {:?}", r.latency_ms, r.text, r.supporting_ids);
                assert!(r.success);
                assert!(r.text.contains(if reverse {
                    "from right to left"
                } else {
                    "from left to right"
                }));
                if small {
                    assert!(!r.text.contains("person"));
                }
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(50), "{}", a.status());
            thread::sleep(Duration::from_millis(50));
        }
    }
}

fn stationary_window(c: Config) -> Report {
    let (sample, frame) = job();
    let mut worker = Interpreter::start(c).unwrap();
    let mut latest = sample.clone();
    for second in 0..=36 {
        let mut s = (*sample).clone();
        s.ns = second * 1_000_000_000;
        s.id = format!("window-{second}");
        s.objects[0].observation_id = format!("obs-{second}");
        s.objects[0].moving = false;
        s.objects[0].region.z = 0.67 + (second % 2) as f64 * 0.01;
        s.objects[0].region.center[0] = frame.info.width as f64 / 2.;
        s.events.clear();
        latest = Arc::new(s);
        // Deliver new evidence at the window boundary even if the scene barely moves.
        worker.submit(latest.clone(), frame.clone(), Duration::ZERO);
    }
    let start = Instant::now();
    loop {
        if let Some(r) = worker.latest(&latest)
            && r.scene_id == "window-35"
        {
            return r;
        }
        assert!(
            start.elapsed() < Duration::from_secs(50),
            "{}",
            worker.status()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn temporal_request_excludes_expired_evidence_and_explains_jitter() {
    let m = mock(|_, body| {
        if body["messages"][0]["content"].is_array() {
            return (
                400,
                json!({"error":{"message":"image input is not supported"}}),
            );
        }
        let p = body["messages"][0]["content"].as_str().unwrap();
        if p.contains("obs-35\"") {
            assert!(p.contains("obs-5\"") && !p.contains("obs-4\""));
            assert!(p.contains("Treat centimetre jitter"));
        }
        chat("{\"index\":0}")
    });
    let mut c = Config::new(m.url.clone());
    c.model = Some("text".into());
    let r = stationary_window(c);
    assert!(
        r.success && r.text.contains("stayed in the centre") && r.text.contains("30 seconds"),
        "{}",
        r.text
    );
    assert_eq!(r.supporting_ids, ["obs-5", "obs-35"]);
    assert!((20..=40).contains(&r.text.split_whitespace().count()));
}

#[test]
#[ignore = "requires the local SmolLM3 endpoint"]
fn local_stationary_context() {
    let mut c = Config::new("http://localhost:8366/v1".into());
    c.model = Some("smollm3-3b".into());
    let r = stationary_window(c);
    println!("{}", r.text);
    assert!(r.success && r.text.contains("30 seconds"), "{}", r.text);
}

#[test]
fn rejected_bearer_is_redacted_from_the_transcript() {
    let secret = "example-test-only-credential";
    let server = mock(move |_, _| {
        (
            401,
            json!({"error":{"message":format!("Rejected key {secret}")}}),
        )
    });
    let transcript = Transcript::default();
    let mut config = Config::new(server.url.clone());
    config.model = Some("test".into());
    config.api_key = Some(secret.to_owned());
    config.transcript = Some(transcript.clone());
    let mut worker = Interpreter::start(config).unwrap();
    let (scene, frame) = job();
    worker.submit(scene.clone(), frame, Duration::ZERO);
    let report = poll(&worker, &scene);
    assert!(!report.success && report.text.contains("401") && !report.text.contains(secret));
    let log = transcript.snapshot();
    assert_eq!(log.len(), 1);
    assert!(!log[0].request.contains(secret));
    assert!(log[0].response.as_deref().unwrap().contains("[redacted]"));
    assert!(!log[0].response.as_deref().unwrap().contains(secret));
}
