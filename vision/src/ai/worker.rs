//! Optional Chat Completions worker. A watch channel keeps only the newest pending observation.
use super::history::{Finding, History};
use super::{Claim, Config, Report};
use crate::{DepthFrame, image, perception::SceneState, source::Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::watch;
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    claims: Vec<Claim>,
}
pub fn signature(s: &SceneState) -> String {
    format!(
        "{}|{:?}|{:?}|{:?}",
        s.status,
        s.optics,
        s.sector_z_m.map(|z| z.map(|m| (m * 4.).round() as i32)),
        s.objects
            .iter()
            .map(|o| (
                o.id,
                (o.region.z * 5.).round() as i32,
                (o.region.center[0] / 32.).round() as i32,
                (o.region.center[1] / 32.).round() as i32,
                o.moving,
                o.direction
            ))
            .collect::<Vec<_>>()
    )
}
#[derive(Clone, Debug)]
pub(super) enum Fact {
    Passage {
        event: String,
        track: u64,
        evidence: crate::trajectory::Passage,
    },
    Upright {
        track: u64,
        id: String,
        width: f64,
        height: f64,
        z: f64,
        sector: usize,
    },
    Historical(Finding),
    PastEvent {
        event: String,
        text: String,
        ids: Vec<String>,
    },
    Coverage(f64, &'static str),
    Layout([f64; 3]),
    Sector(usize, f64),
    Region(u64, f64, bool, &'static str, usize, usize),
}
/// Select complete, grounded sentences before publishing to either consumer.
fn narrative(report: &Report, scene: &SceneState, min: usize, max: usize) -> String {
    let mut text = String::new();
    let mut count = 0;
    let mut append = |sentence: &str| {
        let n = sentence.split_whitespace().count();
        if n > 0 && count + n <= max && !text.contains(sentence) {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(sentence);
            count += n;
        }
        count >= min
    };
    // A period followed by whitespace is a sentence boundary; decimal metres stay intact.
    for sentence in report.text.split_inclusive(". ") {
        if append(sentence.trim()) {
            return text;
        }
    }
    if let Some(Evidence {
        fact: Fact::Passage { evidence, .. },
        ..
    }) = &report.evidence
        && append(&format!(
            "The movement lasted {:.1} seconds, during which the camera recorded the same track in {} separate observations.",
            (evidence.end_ns - evidence.start_ns) as f64 / 1e9,
            evidence.samples
        ))
    {
        return text;
    }
    let coverage = (scene.valid_fraction * 100.).round();
    let regions = match scene.objects.len() {
        0 => "No foreground tracks were active.".into(),
        1 => "One foreground track was active.".into(),
        n => format!("{n} foreground tracks were active."),
    };
    if !matches!(
        report.evidence.as_ref().map(|e| &e.fact),
        Some(Fact::Coverage(..))
    ) && append(&format!("Valid depth covered {coverage:.0}% of the view."))
    {
        return text;
    }
    for detail in [
        regions,
        format!("Capture status was {}.", scene.status),
        format!(
            "The observation time was {:.1} seconds.",
            scene.ns as f64 / 1e9
        ),
    ] {
        if append(&detail) {
            return text;
        }
    }
    for (side, depth) in ["left", "central", "right"]
        .into_iter()
        .zip(scene.sector_z_m)
    {
        if let Some(z) = depth
            && append(&format!("The {side} median was {z:.2} metres."))
        {
            return text;
        }
    }
    for detail in [
        format!("USB errors recorded: {}.", scene.health.errors),
        format!("Recorded sequence gaps: {}.", scene.health.gaps),
        format!("Recorded incomplete frames: {}.", scene.health.incomplete),
        format!("Recorded queue drops: {}.", scene.health.queue_drops),
        "Distances use the camera optical frame.".into(),
    ] {
        if append(&detail) {
            break;
        }
    }
    text
}
fn horizontal_third(x: f64, width: usize) -> usize {
    ((x * 3.0 / width.max(1) as f64) as usize).min(2)
}
impl Fact {
    fn text(&self) -> String {
        match *self {
            Self::PastEvent { ref text, .. } => text.clone(),
            Self::Historical(ref f) => f.text.clone(),
            Self::Passage { ref evidence, .. } => {
                format!("A foreground shape {}.", evidence.movement())
            }
            Self::Upright { sector, .. } => format!(
                "An upright foreground shape is in the {} third of the scene.",
                ["left", "centre", "right"][sector]
            ),
            Self::Coverage(v, "insufficient depth coverage") => format!(
                "There is insufficient depth coverage to follow movement. Only {:.0}% of the view returned a reading, so a missing track cannot establish that anything left.",
                v * 100.
            ),
            Self::Coverage(v, status) => format!(
                "{}; valid depth covers {:.0}% of the view.",
                if status == "tracking" {
                    "Tracking is active"
                } else {
                    status
                },
                v * 100.
            ),
            Self::Layout(z) => format!(
                "Median surface depth measures {:.2} m left, {:.2} m centre and {:.2} m right.",
                z[0], z[1], z[2]
            ),
            Self::Sector(i, z) => format!(
                "The {} third has a median surface depth of {z:.2} m.",
                ["left", "centre", "right"][i]
            ),
            Self::Region(_, z, moving, direction, sector, _) => {
                let activity = match (moving, direction) {
                    (false, _) => "stationary",
                    (true, "approaching") => "approaching the sensor",
                    (true, "receding") => "moving away from the sensor",
                    _ => "moving with little change in depth",
                };
                format!(
                    "A foreground shape in the {} third of the view is {activity}, about {z:.1} metres in front of the camera.",
                    ["left", "centre", "right"][sector]
                )
            }
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Evidence {
    status: &'static str,
    optics: crate::IntrinsicsSource,
    sectors: [Option<f64>; 3],
    fact: Fact,
}
impl Evidence {
    fn still_valid(&self, s: &SceneState) -> bool {
        if let Fact::Passage {
            event, evidence, ..
        } = &self.fact
        {
            return s.ns.saturating_sub(evidence.end_ns) <= 12_000_000_000
                && s.status == "tracking"
                && s.events.iter().any(|e| e.id == *event);
        }
        let close = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(x), Some(y)) => (x - y).abs() <= 0.25,
            (None, None) => true,
            _ => false,
        };
        self.status == s.status
            && self.optics == s.optics
            && self
                .sectors
                .into_iter()
                .zip(s.sector_z_m)
                .all(|(a, b)| close(a, b))
            && match self.fact {
                Fact::Passage { .. } => false,
                Fact::Upright { track, z, .. } => s.objects.iter().any(|o| {
                    o.id == track
                        && (o.region.z - z).abs() < 0.20
                        && crate::trajectory::upright_extent(&o.region).is_some()
                }),
                Fact::PastEvent { .. } | Fact::Historical(_) => false, // Historical observations are checked against the capture clock in latest().
                Fact::Layout(depths) => depths
                    .into_iter()
                    .zip(s.sector_z_m)
                    .all(|(a, b)| b.is_some_and(|v| (a - v).abs() <= 0.10)),
                Fact::Coverage(v, _) => (s.valid_fraction - v).abs() < 0.03,
                Fact::Sector(i, z) => s.sector_z_m[i].is_some_and(|v| (v - z).abs() <= 0.10),
                Fact::Region(id, z, m, d, sector, width) => s.objects.iter().any(|o| {
                    o.id == id
                        && horizontal_third(o.region.center[0], width) == sector
                        && o.moving == m
                        && o.direction == d
                        && (o.region.z - z).abs() <= 0.10
                }),
            }
    }
}
type Job = (
    Arc<SceneState>,
    Arc<DepthFrame>,
    Duration,
    History,
    Option<Arc<crate::semantic::SemanticSceneGraph>>,
);
pub struct Interpreter {
    tx: Option<watch::Sender<Option<Job>>>,
    cancel: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
    result: Arc<Mutex<Report>>,
    last: String,
    last_at: Instant,
    event: String,
    history: History,
    last_context_ns: u64,
}
impl Interpreter {
    pub fn start(config: Config) -> Result<Self> {
        let url = reqwest::Url::parse(&config.url)?;
        if !["http", "https"].contains(&url.scheme())
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("invalid AI endpoint URL".into());
        }
        let local = url.host_str().is_some_and(|h| {
            h == "localhost"
                || h.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !local && !config.allow_remote_frames {
            return Err(
                "explicit consent required to send depth data to this remote endpoint".into(),
            );
        }
        config.validate_words()?;
        if config.timeout.is_zero()
            || config.budget.is_zero()
            || config.candidates == 0
            || config.probe_tokens == 0
            || config.tokens == 0
        {
            return Err("invalid AI limits".into());
        }
        let (tx, mut rx) = watch::channel::<Option<Job>>(None);
        let (cancel, mut stop) = watch::channel(false);
        let result = Arc::new(Mutex::new(Report::default()));
        let output = result.clone();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let client = Client::builder()
            .no_proxy()
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let thread = std::thread::spawn(move || {
            rt.block_on(async move {
                let work = async {
                    let mut api = Api {
                        client,
                        responses: Default::default(),
                        c: config,
                        model: None,
                        level: 0,
                        failures: 0,
                        measurements_only: false,
                        history: History::default(),
                        previous: None,
                    };
                    let mut next_start = Instant::now();
                    while rx.changed().await.is_ok() {
                        tokio::time::sleep_until(next_start.into()).await;
                        let Some((s, f, refresh, history, graph)) = rx.borrow_and_update().clone()
                        else {
                            continue;
                        };
                        api.history = history;
                        let sig = signature(&s);
                        let now = Instant::now();
                        next_start = now + refresh;
                        let mut report = match api.interpret(&s, &f, graph.as_deref()).await {
                            Ok(mut r) => {
                                r.text = narrative(&r, &s, api.c.min_words, api.c.max_words);
                                api.failures = 0;
                                r
                            }
                            Err(e) => {
                                api.failures = api.failures.saturating_add(1);
                                if api.failures >= 2 {
                                    api.model = None;
                                    api.level = 0;
                                }
                                Report {
                                    text: e.to_string(),
                                    ..Default::default()
                                }
                            }
                        };
                        report.model = api.model.clone().unwrap_or_default();
                        report.signature = sig;
                        report.scene_id = s.id.clone();
                        report.arrival_ns = s.ns;
                        report.latency_ms = now.elapsed().as_millis() as u64;
                        *output.lock().unwrap() = report;
                    }
                };
                tokio::select! {_=work=>{},_=stop.changed()=>{}}
            })
        });
        Ok(Self {
            tx: Some(tx),
            cancel,
            thread: Some(thread),
            result,
            last: String::new(),
            last_at: Instant::now(),
            event: String::new(),
            history: History::default(),
            last_context_ns: 0,
        })
    }
    pub fn reconfigure(&mut self, config: Config) -> Result<()> {
        let mut next = Self::start(config)?;
        next.history = std::mem::take(&mut self.history);
        *self = next;
        Ok(())
    }

    pub fn submit(&mut self, s: Arc<SceneState>, f: Arc<DepthFrame>, refresh: Duration) {
        self.enqueue(s, f, refresh, None);
    }
    pub fn submit_graph(
        &mut self,
        s: Arc<SceneState>,
        f: Arc<DepthFrame>,
        refresh: Duration,
        graph: Arc<crate::semantic::SemanticSceneGraph>,
    ) {
        self.enqueue(s, f, refresh, Some(graph));
    }
    fn enqueue(
        &mut self,
        s: Arc<SceneState>,
        f: Arc<DepthFrame>,
        refresh: Duration,
        graph: Option<Arc<crate::semantic::SemanticSceneGraph>>,
    ) {
        let sig = signature(&s);
        let sampled = self.history.observe(&s, f.info.width);
        let context_due = sampled && s.ns.abs_diff(self.last_context_ns) >= 7_000_000_000;
        let event = s.events.last().map(|e| e.id.clone()).unwrap_or_default();
        let result = self.result.lock().unwrap();
        let retry = !result.success
            && result.signature == sig
            && self.last_at.elapsed() >= refresh.max(Duration::from_millis(100));
        drop(result);
        if self.last.is_empty() || self.event != event || self.last != sig || retry || context_due {
            self.last_context_ns = s.ns;
            self.last = sig;
            self.event = event;
            self.last_at = Instant::now();
            self.tx.as_ref().unwrap().send_replace(Some((
                s,
                f,
                refresh,
                self.history.clone(),
                graph,
            )));
        }
    }
    pub fn status(&self) -> String {
        let r = self.result.lock().unwrap();
        if r.scene_id.is_empty() {
            return "Waiting for the first model analysis…".into();
        }
        if !r.success {
            return format!("Model request failed: {}", r.text);
        }
        format!(
            "Last analysis: {} · {:.1} s",
            r.scene_id,
            r.latency_ms as f64 / 1000.
        )
    }
    pub fn latest(&self, s: &SceneState) -> Option<Report> {
        let r = self.result.lock().unwrap();
        // A completed sequence is explicitly narrated in the past tense and attributed
        // to its capture time. A later change cannot invalidate that historical observation.
        let current = if r
            .evidence
            .as_ref()
            .is_some_and(|e| matches!(e.fact, Fact::PastEvent { .. } | Fact::Historical(_)))
        {
            s.ns.checked_sub(r.arrival_ns)
                .is_some_and(|age| age <= 30_000_000_000)
                && s.status == "tracking"
        } else if let Some(e) = &r.evidence {
            e.still_valid(s)
        } else {
            r.signature == signature(s)
        };
        current.then(|| r.clone())
    }
}
impl Drop for Interpreter {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
        self.tx.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
struct Api {
    responses: std::cell::Cell<Option<bool>>,
    client: Client,
    c: Config,
    model: Option<String>,
    level: u8,
    failures: u8,
    measurements_only: bool,
    history: History,
    previous: Option<(String, Value, Value)>,
}
fn image_part(data: Vec<u8>) -> Value {
    json!({"type":"image_url","image_url":{"url":format!("data:image/png;base64,{}",STANDARD.encode(data))}})
}
impl Api {
    async fn call(&self, path: &str, body: Option<Value>) -> Result<(StatusCode, Value)> {
        let url = format!("{}/{}", self.c.url.trim_end_matches('/'), path);
        let redact = |s: String| match self.c.api_key.as_deref().filter(|k| !k.is_empty()) {
            Some(key) => s.replace(key, "[redacted]"),
            None => s,
        };
        let mut exchange = self.c.transcript.as_ref().map(|log| {
            let mut texts = Vec::new();
            if let Some(b) = body.as_ref() {
                if let Some(messages) = b["messages"].as_array() {
                    for message in messages {
                        if let Some(s) = message["content"].as_str() {
                            texts.push(s);
                        } else if let Some(parts) = message["content"].as_array() {
                            texts.extend(parts.iter().filter_map(|p| p["text"].as_str()));
                        }
                    }
                } else if let Some(parts) = b["input"].as_array() {
                    texts.extend(parts.iter().filter_map(|p| p["text"].as_str()));
                }
            }
            let method = if body.is_some() { "POST" } else { "GET" };
            let model = body
                .as_ref()
                .and_then(|b| b["model"].as_str())
                .unwrap_or("discovery");
            log.begin(
                redact(format!("{model}   ·   {method} {url}")),
                redact(texts.join("\n\n")),
                redact(
                    body.as_ref()
                        .map(|b| serde_json::to_string_pretty(b).unwrap())
                        .unwrap_or_default(),
                ),
            )
        });
        let mut r = if let Some(b) = body {
            self.client.post(url).json(&b)
        } else {
            self.client.get(url)
        };
        if let Some(key) = self.c.api_key.as_deref().filter(|k| !k.is_empty()) {
            r = r.bearer_auth(key);
        }
        let received: Result<_> = async {
            let mut response = r.send().await.map_err(|e| {
                if e.is_timeout() {
                    "AI request timed out"
                } else {
                    "AI endpoint unreachable"
                }
            })?;
            let status = response.status();
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "AI response interrupted")?
            {
                if bytes.len() + chunk.len() > 1_048_576 {
                    return Err("oversized AI response".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok((status, bytes))
        }
        .await;
        let (status, bytes) = match received {
            Ok(data) => data,
            Err(e) => {
                if let Some(entry) = &mut exchange {
                    entry.finish(&e.to_string(), None, None);
                }
                return Err(e);
            }
        };
        if let Some(entry) = &mut exchange {
            let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            let answer = value["choices"][0]["message"]["content"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| {
                    value["output"].as_array().map(|items| {
                        items
                            .iter()
                            .filter_map(|i| i["content"].as_array())
                            .flatten()
                            .filter_map(|p| p["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                });
            entry.finish(
                &format!("HTTP {status}"),
                Some(redact(String::from_utf8_lossy(&bytes).into_owned())),
                answer.map(redact),
            );
        }
        if status == StatusCode::UNAUTHORIZED {
            return Err("HTTP 401: enter a valid API key in Model connection and refresh, or set OPENAI_API_KEY before launch".into());
        }
        if !status.is_success() {
            return Ok((
                status,
                serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            ));
        }
        Ok((status, serde_json::from_slice(&bytes)?))
    }
    // A missing Chat route alone provides no evidence that Responses exists.
    // Consult the configured service's own OpenAPI declaration before fallback.
    async fn completion(&self, body: Value) -> Result<(StatusCode, Value)> {
        if self.responses.get() != Some(true) {
            let chat = self.call("chat/completions", Some(body.clone())).await?;
            if !matches!(chat.0.as_u16(), 404 | 405) || self.responses.get() == Some(false) {
                return Ok(chat);
            }
            let (status, spec) = self.call("openapi.json", None).await?;
            let prefix = reqwest::Url::parse(&self.c.url)?
                .path()
                .trim_end_matches('/')
                .to_owned();
            let supported = status.is_success()
                && ["/responses".to_string(), format!("{prefix}/responses")]
                    .iter()
                    .any(|p| spec["paths"][p]["post"].is_object());
            self.responses.set(Some(supported));
            if !supported {
                return Ok(chat);
            }
        }
        let content = body["messages"][0]["content"]
            .as_array()
            .ok_or("missing model inputs")?
            .iter()
            .map(|part| {
                if part["type"] == "text" {
                    json!({"type":"input_text","text":part["text"]})
                } else {
                    json!({"type":"input_image","image_url":part["image_url"]["url"]})
                }
            })
            .collect::<Vec<_>>();
        let mut request = json!({"model":body["model"],"max_output_tokens":body["max_tokens"],"input":[{"role":"user","content":content}]});
        if let Some(kind) = body["response_format"]["type"].as_str() {
            let mut format = if kind == "json_schema" {
                body["response_format"]["json_schema"].clone()
            } else {
                json!({})
            };
            format["type"] = json!(kind);
            request["text"] = json!({"format":format});
        }
        let (status, value) = self.call("responses", Some(request)).await?;
        let text = value["output"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["content"].as_array())
            .flatten()
            .filter(|p| p["type"] == "output_text")
            .filter_map(|p| p["text"].as_str())
            .collect::<String>();
        Ok((
            status,
            json!({"choices":[{"message":{"content":text}}],"usage":value["usage"]}),
        ))
    }
    async fn select(&self) -> Result<String> {
        if let Some(m) = &self.c.model {
            return Ok(m.clone());
        }
        let discovery = Instant::now();
        let (status, models) = tokio::time::timeout(self.c.budget, self.call("models", None))
            .await
            .map_err(|_| "model listing deadline expired")??;
        let ids = models["data"]
            .as_array()
            .filter(|_| status.is_success())
            .ok_or("model listing unavailable; configure --ai-model")?;
        let mut best = None;
        for m in ids
            .iter()
            .filter_map(|v| v["id"].as_str())
            .filter(|m| {
                !["embed", "audio", "moderation", "whisper", "tts"]
                    .iter()
                    .any(|x| m.to_lowercase().contains(x))
            })
            .take(self.c.candidates)
        {
            let start = Instant::now();
            let remaining = self.c.budget.saturating_sub(discovery.elapsed());
            if remaining.is_zero() {
                break;
            }
            let passed = matches!(
                tokio::time::timeout(remaining.min(self.c.timeout), self.probe(m)).await,
                Ok(Ok(true))
            );
            if passed && best.as_ref().is_none_or(|(d, _)| start.elapsed() < *d) {
                best = Some((start.elapsed(), m.to_owned()))
            }
        }
        best.map(|(_, m)| m)
            .ok_or_else(|| "no candidate passed the image probe".into())
    }
    async fn probe(&self, m: &str) -> Result<bool> {
        let mut seed = [0u8; 1];
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(&mut seed)?;
        for offset in 0..4 {
            let q = (seed[0] as usize + offset) % 4;
            let mut pix = vec![0; 32 * 32 * 3];
            for y in 0..32 {
                for x in 0..32 {
                    if (y / 16) * 2 + x / 16 == q {
                        pix[(y * 32 + x) * 3..][..3].fill(255)
                    }
                }
            }
            let parts = json!([{"type":"text","text":"Which quadrant is white? Reply only TL, TR, BL or BR."},image_part(image::png(32,32,&pix)?)]);
            let res=self.completion(json!({"model":m,"messages":[{"role":"user","content":parts}],"max_tokens":self.c.probe_tokens})).await;
            if !matches!(res,Ok((s,ref v)) if s.is_success() && v["choices"][0]["message"]["content"].as_str().unwrap_or("").trim()==["TL","TR","BL","BR"][q])
            {
                return Ok(false);
            }
        }

        let swap = seed[0] & 1 == 1;
        let mut pixels = vec![0; 64 * 32 * 3];
        for y in 4..28 {
            for x in 2..62 {
                let left = x < 32;
                let j = (y * 64 + x) * 3;
                pixels[j..j + 3].copy_from_slice(if left == swap {
                    &[255, 0, 0]
                } else {
                    &[0, 0, 255]
                });
            }
        }
        let (left, right) = if seed[0] & 2 == 0 {
            ("A", "B")
        } else {
            ("B", "A")
        };
        super::marked::mark(&mut pixels, 64, 32, 8, 12, left);
        super::marked::mark(&mut pixels, 64, 32, 44, 12, right);
        let parts = json!([{"type":"text","text":"Quality probe. Red is near and blue far. Return count, nearest letter, then letters left-to-right, separated by spaces. Example format: 3 C CAB."},image_part(image::png(64,32,&pixels)?)]);
        let (status, v) = self
            .completion(
                json!({"model":m,"messages":[{"role":"user","content":parts}],"max_tokens":12}),
            )
            .await?;
        Ok(status.is_success()
            && v["choices"][0]["message"]["content"]
                .as_str()
                .is_some_and(|s| {
                    s.trim() == format!("2 {} {left}{right}", if swap { left } else { right })
                }))
    }
    async fn semantic_report(
        &mut self,
        s: &SceneState,
        width_from_frame: usize,
        fact: Fact,
    ) -> Result<Report> {
        let (observation, ids, action, body) = match &fact {
            Fact::Passage {
                event,
                track,
                evidence: p,
            } => (
                json!({"track":track,"first_x":p.start_x,"last_x":p.end_x,"duration_s":(p.end_ns-p.start_ns) as f64/1e9,"consistent_samples":p.samples,"upright_width_height_m":p.upright_extent_m}),
                vec![event.clone(), p.start_id.clone(), p.end_id.clone()],
                p.movement(),
                p.upright_extent_m.is_some(),
            ),
            Fact::Upright {
                track,
                id,
                width,
                height,
                z,
                sector,
            } => (
                json!({"track":track,"upright_width_height_m":[width,height],"forward_m":z,"image_third":(["left","centre","right"][*sector])}),
                vec![id.clone()],
                "is present",
                true,
            ),
            _ => return Err("missing semantic evidence".into()),
        };
        let prompt = format!(
            "Describe this depth-camera observation. Infer whether the subject could be a person from an upright human-sized silhouette. Use possible person when the supplied upright dimensions fit a human silhouette. This is a shape hypothesis; a tall object may look similar. Choose unknown shape when size evidence is absent. Camera x increases from 0 at left to 1 at right. Infer motion from ordered positions of the same track. Left-to-right or right-to-left means traversing both outer thirds; otherwise use rightward or leftward. Without a trajectory use is present. Do not infer identity, intention or colour. The measured action is {action}. Use this action exactly. Return JSON fields subject and action. Evidence: {observation}. Measured spatial context: {}",
            super::spatial::observations(s)
        );
        let subjects = if body {
            vec!["possible person", "unknown shape"]
        } else {
            vec!["unknown shape"]
        };
        let schema = json!({"type":"object","properties":{"subject":{"type":"string","enum":subjects},"action":{"type":"string","enum":[action]}},"required":["subject","action"],"additionalProperties":false});
        let mut tries = 0;
        loop {
            let mut request = json!({"model":self.model,"messages":[{"role":"user","content":format!("{prompt} Context: {}. Allowed values: {schema}",self.history.prompt(s))}],"temperature":0,"max_tokens":self.c.tokens.min(80)});
            if self.level == 0 {
                request["response_format"] = json!({"type":"json_schema","json_schema":{"name":"semantic_observation","schema":schema,"strict":true}})
            } else if self.level == 1 {
                request["response_format"] = json!({"type":"json_object"})
            }
            let (status, v) = self.call("chat/completions", Some(request)).await?;
            if [400, 422].contains(&status.as_u16()) && self.level < 2 {
                self.level += 1;
                continue;
            }
            if !status.is_success() {
                return Err(format!("Semantic request returned HTTP {status}").into());
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Interpretation {
                subject: String,
                action: String,
            }
            let raw = v["choices"][0]["message"]["content"].as_str().unwrap_or("");
            if let Ok(answer) = serde_json::from_str::<Interpretation>(raw.trim())
                && subjects.contains(&answer.subject.as_str())
                && answer.action == action
            {
                let noun = if answer.subject == "possible person" {
                    "A person-like figure"
                } else {
                    "A foreground shape"
                };
                let mut text = match &fact {
                    Fact::Upright { sector, .. } => format!(
                        "{noun} is visible in the {} third of the scene.",
                        ["left", "centre", "right"][*sector]
                    ),
                    _ => format!("{noun} {action}."),
                };
                let mut ids = ids;
                let mut history_key = Some(format!("{}-{action}-{noun}", ids[0]));
                if let Fact::Upright { track, .. } = &fact {
                    history_key = Some(format!("upright-{track}-{noun}"));
                    if let Some(t) = self
                        .history
                        .findings(s, width_from_frame)
                        .into_iter()
                        .find(|f| f.track == *track)
                    {
                        text = t.text.replacen("A foreground shape", noun, 1).replacen(
                            "A moving shape",
                            &format!("{noun} that had been moving"),
                            1,
                        );
                        ids.extend(t.ids);
                        history_key = Some(format!("{}-{noun}", t.key));
                    }
                }
                return Ok(Report {
                    success: true,
                    text,
                    supporting_ids: ids,
                    history_key,
                    usage: v["usage"].clone(),
                    evidence: Some(Evidence {
                        status: s.status,
                        optics: s.optics,
                        sectors: s.sector_z_m,
                        fact,
                    }),
                    ..Default::default()
                });
            }
            tries += 1;
            if tries >= 2 {
                return Err("Semantic reply did not match the observed shape or trajectory".into());
            }
        }
    }
    /// Text-only endpoints rank measured observations. The displayed fact is
    /// rendered locally, so the model cannot invent geometry or visual labels.
    async fn measurements(&mut self, s: &SceneState, width: usize) -> Result<Report> {
        if s.status == "tracking" {
            if let Some((event, track, p)) = s
                .events
                .iter()
                .rev()
                .find_map(|e| Some((e, e.object?, e.passage.as_ref()?)))
                .filter(|(_, _, p)| s.ns.saturating_sub(p.end_ns) <= 10_000_000_000)
            {
                return self
                    .semantic_report(
                        s,
                        width,
                        Fact::Passage {
                            event: event.id.clone(),
                            track,
                            evidence: p.clone(),
                        },
                    )
                    .await;
            }
            if let Some((o, [w, h])) = s
                .objects
                .iter()
                .filter_map(|o| Some((o, crate::trajectory::upright_extent(&o.region)?)))
                .max_by_key(|(o, _)| o.region.samples)
            {
                return self
                    .semantic_report(
                        s,
                        width,
                        Fact::Upright {
                            track: o.id,
                            id: o.observation_id.clone(),
                            width: w,
                            height: h,
                            z: o.region.z,
                            sector: horizontal_third(o.region.center[0], width),
                        },
                    )
                    .await;
            }
        }
        let mut facts = vec![Fact::Coverage(s.valid_fraction, s.status)];
        if s.status == "tracking" {
            for (i, z) in s.sector_z_m.into_iter().enumerate() {
                if let Some(z) = z {
                    facts.push(Fact::Sector(i, z))
                }
            }
            if let [Some(l), Some(c), Some(r)] = s.sector_z_m {
                facts.push(Fact::Layout([l, c, r]));
            }
            let mut regions: Vec<_> = s.objects.iter().collect();
            regions.sort_by_key(|o| (!o.moving, std::cmp::Reverse(o.region.samples)));
            for o in regions.into_iter().take(8) {
                facts.push(Fact::Region(
                    o.id,
                    o.region.z,
                    o.moving,
                    o.direction,
                    horizontal_third(o.region.center[0], width),
                    width,
                ));
            }
        }
        let trends = self.history.findings(s, width);
        let trends_absent = trends.is_empty();
        if !trends_absent {
            facts = trends.into_iter().map(Fact::Historical).collect();
            facts.sort_by_key(|f| matches!(f,Fact::Historical(t) if t.key.contains("-still-")));
        }
        if s.status == "tracking"
            && let Some(end) =
                s.events.iter().rev().find(|e| {
                    e.kind == "disappeared" && s.ns.saturating_sub(e.ns) <= 30_000_000_000
                })
            && let Some(id) = end.object
            && !s.objects.iter().any(|o| o.id == id)
            && let Some(before) = s.events.iter().rev().find(|e| {
                e.object == Some(id)
                    && e.ns < end.ns
                    && s.ns.saturating_sub(e.ns) <= 30_000_000_000
                    && (e.passage.is_some() || e.kind == "appeared")
            })
        {
            let action = before
                .passage
                .as_ref()
                .map(|p| p.movement())
                .unwrap_or("appeared in the foreground");
            let text = format!(
                "A shape {action} before the camera lost track of it. The depth readings alone cannot tell whether it left the scene or became obscured."
            );
            facts.insert(
                usize::from(!facts.is_empty() && matches!(facts[0], Fact::Historical(_))),
                Fact::PastEvent {
                    event: end.id.clone(),
                    text,
                    ids: vec![before.id.clone(), end.id.clone()],
                },
            );
        }
        if trends_absent && let Some((key, text)) = self.history.tracking_pattern(s) {
            facts.insert(
                0,
                Fact::PastEvent {
                    event: key,
                    text,
                    ids: vec![s.id.clone()],
                },
            );
        }
        // Keep the model focused when the camera measures a motion transition.
        if facts
            .iter()
            .any(|f| matches!(f, Fact::Region(_, _, true, _, _, _)))
        {
            facts.retain(|f| {
                matches!(
                    f,
                    Fact::Region(_, _, true, _, _, _) | Fact::PastEvent { .. }
                )
            });
        } else if s.status == "tracking"
            && s.objects.is_empty()
            && facts.iter().any(|f| matches!(f, Fact::Layout(_)))
        {
            facts.retain(|f| matches!(f, Fact::Layout(_) | Fact::PastEvent { .. }));
        }
        let numbered = facts
            .iter()
            .enumerate()
            .map(|(i, f)| format!("{i}: {}", f.text()))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "You narrate a fixed ASUS Xtion depth camera for someone who cannot see the display. You receive measurements only. Left and right are from the camera; Z is forward distance in metres. A region is a tracked patch of depth, with unknown identity. Zero means missing. Never infer a person, colour or activity from appearance. If tracking is impaired, report coverage and withhold motion claims. Otherwise prioritize a moving region and its direction; next prefer a stationary foreground region; for an empty foreground describe the left/centre/right depth layout. Choose the one candidate that best explains meaningful change over time. Prefer a motion transition to a steady region. Return only JSON with its integer index.\n{}\nSpatial evidence: {}\nCandidates:\n{numbered}",
            self.history.prompt(s),
            super::spatial::observations(s)
        );
        let mut attempts = 0;
        loop {
            let mut body = json!({"model":self.model,"messages":[{"role":"user","content":prompt}],"temperature":0,"max_tokens":self.c.tokens.min(48)});
            if self.level == 0 {
                body["response_format"] = json!({"type":"json_schema","json_schema":{"name":"depth_fact","strict":true,"schema":{"type":"object","properties":{"index":{"type":"integer","enum":(0..facts.len()).collect::<Vec<_>>()}},"required":["index"],"additionalProperties":false}}})
            } else if self.level == 1 {
                body["response_format"] = json!({"type":"json_object"})
            }
            let (status, v) = self.call("chat/completions", Some(body)).await?;
            if [400, 422].contains(&status.as_u16()) && self.level < 2 {
                self.level += 1;
                continue;
            }
            if !status.is_success() {
                return Err(format!("AI measurement request returned HTTP {status}").into());
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Choice {
                index: usize,
            }
            let raw = v["choices"][0]["message"]["content"].as_str().unwrap_or("");
            let choice = raw
                .find('{')
                .zip(raw.rfind('}'))
                .and_then(|(a, b)| raw.get(a..=b))
                .and_then(|text| serde_json::from_str::<Choice>(text).ok());
            if let Some(fact) = choice.and_then(|c| facts.get(c.index)) {
                return Ok(Report {
                    success: true,
                    text: fact.text(),
                    supporting_ids: match fact {
                        Fact::PastEvent { ids, .. } => ids.clone(),
                        Fact::Historical(f) => f.ids.clone(),
                        _ => vec![s.id.clone()],
                    },
                    history_key: match fact {
                        Fact::PastEvent { event, .. } => Some(event.clone()),
                        Fact::Historical(f) => Some(f.key.clone()),
                        Fact::Region(id, _, false, _, sector, _) => {
                            Some(format!("snapshot-{id}-still-{sector}"))
                        }
                        _ => None,
                    },
                    evidence: Some(Evidence {
                        status: s.status,
                        optics: s.optics,
                        sectors: s.sector_z_m,
                        fact: fact.clone(),
                    }),
                    usage: v["usage"].clone(),
                    ..Default::default()
                });
            }
            attempts += 1;
            if attempts >= 2 {
                return Err("AI did not select a valid measured fact".into());
            }
        }
    }
    async fn interpret(
        &mut self,
        s: &SceneState,
        f: &DepthFrame,
        graph: Option<&crate::semantic::SemanticSceneGraph>,
    ) -> Result<Report> {
        if self.model.is_none() {
            self.model = Some(self.select().await?);
        }
        if self.measurements_only {
            return self.measurements(s, f.info.width).await;
        }
        let (packet, img, surface) = if let Some(g) = graph {
            let (p, d, m) = super::marked::package(f, s, g)?;
            (p, image_part(d), Some(image_part(m)))
        } else {
            let (w, h, rgb) = image::rgb(f, 224, 4000);
            (
                super::spatial::observations(s),
                image_part(image::png(w, h, &rgb)?),
                None,
            )
        };
        let schema = schema();
        let text = format!(
            "Depth only. Colours encode distance, not appearance. Red near, blue 4 m or farther, black invalid. Do not infer identity, colour, texture or text. Use recent lateral movement events for the direction of travel of the same track. A human-sized upright silhouette can support a tentative person-like label. Supply shape hypotheses only. Use current observation IDs, local Z and direction. Use an empty claims array when uncertain. Return JSON matching {schema}. Context: {}. Current scene: {}",
            self.history.prompt(s),
            packet
        );
        let mut parts = vec![json!({"type":"text","text":text})];
        if s.objects.iter().any(|o| o.moving)
            && let Some((id, image, history)) = &self.previous
        {
            parts.push(json!({"type":"text","text":format!("Previous keyframe {id}: {history}")}));
            parts.push(image.clone())
        }
        parts.push(json!({"type":"text","text":"Current keyframe"}));
        parts.push(img.clone());
        if let Some(surface) = surface {
            parts.push(json!({"type":"text","text":"Sensor-facing shaded visible surfaces. E marks identify the same entities across images and JSON. Cite their supplied observation IDs. Use only JSON for measurements. Hidden geometry, anatomy, posture and intent are unknown."}));
            parts.push(surface);
        }
        let mut tries = 0;
        loop {
            let mut body = json!({"model":self.model,"messages":[{"role":"user","content":parts}],"max_tokens":self.c.tokens});
            if self.level == 0 {
                body["response_format"] = json!({"type":"json_schema","json_schema":{"name":"depth_claims","strict":true,"schema":schema}})
            } else if self.level == 1 {
                body["response_format"] = json!({"type":"json_object"})
            }
            let (status, v) = self.completion(body).await?;
            if self.c.model.is_some()
                && !status.is_success()
                && v["error"]["message"].as_str().is_some_and(|m| {
                    m.to_ascii_lowercase()
                        .contains("image input is not supported")
                })
            {
                self.measurements_only = true;
                return self.measurements(s, f.info.width).await;
            }
            if [400, 422].contains(&status.as_u16()) && self.level < 2 {
                self.level += 1;
                continue;
            }
            if !status.is_success() {
                return Err(format!("AI endpoint returned HTTP {status}").into());
            }
            let raw = v["choices"][0]["message"]["content"].as_str().unwrap_or("");
            let parsed = raw
                .find('{')
                .zip(raw.rfind('}'))
                .and_then(|(a, b)| raw.get(a..=b))
                .and_then(|s| serde_json::from_str::<Reply>(s).ok());
            if let Some(mut reply) = parsed
                && valid(s, &reply.claims)
            {
                for claim in &mut reply.claims {
                    if let Some(o) = s
                        .objects
                        .iter()
                        .find(|o| o.observation_id == claim.observation_id)
                    {
                        claim.z_m = o.region.z;
                    }
                }
                let current = json!(s.objects);
                let change = if let Some((_, _, previous)) = &self.previous {
                    let old = previous.as_array().map(Vec::as_slice).unwrap_or_default();
                    let mut changes = Vec::new();
                    for o in &s.objects {
                        if let Some(p) = old.iter().find(|p| p["id"].as_u64() == Some(o.id)) {
                            let dz = o.region.z - p["region"]["z"].as_f64().unwrap_or(o.region.z);
                            if dz.abs() > 0.08 {
                                changes.push(format!(
                                    "Region {} is {:.2} m {} than before",
                                    o.id,
                                    dz.abs(),
                                    if dz < 0. { "nearer" } else { "farther" }
                                ));
                            }
                            if p["moving"].as_bool() != Some(o.moving) {
                                changes.push(format!("Region {} changed motion state", o.id));
                            }
                        } else {
                            changes.push(format!("Region {} is newly visible", o.id));
                        }
                    }
                    for p in old {
                        if let Some(id) = p["id"].as_u64()
                            && !s.objects.iter().any(|o| o.id == id)
                        {
                            changes.push(format!("Region {id} is no longer visible"));
                        }
                    }
                    if changes.is_empty() {
                        "No significant depth or motion change".to_owned()
                    } else {
                        changes.join(". ")
                    }
                } else {
                    "First interpretation".to_owned()
                };
                let trends = self.history.findings(s, f.info.width);
                let main_trend = trends.iter().find(|t| {
                    reply.claims.is_empty()
                        || reply
                            .claims
                            .iter()
                            .any(|c| t.ids.last() == Some(&c.observation_id))
                });
                let text = if let Some(t) = main_trend {
                    let subject = reply
                        .claims
                        .iter()
                        .find(|c| t.ids.last() == Some(&c.observation_id))
                        .map(|c| c.hypothesis.as_str());
                    match subject {
                        Some("person-like shape") => t
                            .text
                            .replace("A foreground shape", "A person-like figure")
                            .replace("A moving shape", "A moving person-like figure"),
                        Some("hand-like shape") => {
                            t.text.replace("A foreground shape", "A hand-like shape")
                        }
                        _ => t.text.clone(),
                    }
                } else if reply.claims.is_empty() {
                    format!("{} {change}.", crate::describe::describe(s))
                } else {
                    reply
                        .claims
                        .iter()
                        .map(|claim| {
                            let obj = s
                                .objects
                                .iter()
                                .find(|o| o.observation_id == claim.observation_id)
                                .unwrap();
                            let subject = match claim.hypothesis.as_str() {
                                "person-like shape" => "A person-like figure",
                                "hand-like shape" => "A hand-like shape",
                                _ => "A foreground shape",
                            };
                            let passage = s
                                .events
                                .iter()
                                .rev()
                                .find(|e| {
                                    e.object == Some(obj.id)
                                        && e.passage.is_some()
                                        && s.ns.saturating_sub(e.ns) <= 10_000_000_000
                                })
                                .and_then(|e| e.passage.as_ref());
                            if let Some(p) = passage {
                                format!("{subject} {}.", p.movement())
                            } else {
                                format!("{subject} is visible {:.2} m forward.", obj.region.z)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                self.previous = Some((s.id.clone(), img, current));
                return Ok(Report {
                    success: true,
                    history_key: main_trend.map(|t| {
                        format!(
                            "{}-{:?}",
                            t.key,
                            reply.claims.first().map(|c| &c.hypothesis)
                        )
                    }),
                    supporting_ids: main_trend.map(|t| t.ids.clone()).unwrap_or_default(),
                    claims: reply.claims,
                    text,
                    usage: v["usage"].clone(),
                    ..Default::default()
                });
            }
            tries += 1;
            if tries >= 2 {
                return Err("AI reply failed schema or measurement checks".into());
            }
        }
    }
}
fn schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["claims"],"properties":{"claims":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["observation_id","hypothesis","activity","relative_to","relationship","z_m","confidence"],"properties":{"observation_id":{"type":"string"},"hypothesis":{"type":"string","enum":["person-like shape","hand-like shape","unclassified surface"]},"activity":{"type":"string","enum":["approaching","receding","steady"]},"relative_to":{"type":"string"},"relationship":{"type":"string","enum":["none","left","right","nearer","farther"]},"z_m":{"type":"number"},"confidence":{"type":"number"}}}}}})
}
fn valid(s: &SceneState, claims: &[Claim]) -> bool {
    let mut ids = HashSet::new();
    claims.iter().all(|c| {
        let Some(o) = s
            .objects
            .iter()
            .find(|o| o.observation_id == c.observation_id)
        else {
            return false;
        };
        ids.insert(&c.observation_id)
            && [
                "person-like shape",
                "hand-like shape",
                "unclassified surface",
            ]
            .contains(&c.hypothesis.as_str())
            && (0.0..=1.).contains(&c.confidence)
            && (o.region.z - c.z_m).abs() <= 0.1
            && o.direction == c.activity
            && if c.relationship == "none" {
                c.relative_to.is_empty()
            } else {
                s.objects
                    .iter()
                    .find(|t| t.observation_id == c.relative_to && t.id != o.id)
                    .is_some_and(|t| match c.relationship.as_str() {
                        "left" => o.region.center[0] < t.region.center[0],
                        "right" => o.region.center[0] > t.region.center[0],
                        "nearer" => o.region.z < t.region.z,
                        "farther" => o.region.z > t.region.z,
                        _ => false,
                    })
            }
    })
}
