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
