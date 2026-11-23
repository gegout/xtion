//! Optional Chat Completions worker. A watch channel keeps only the newest pending observation.
use crate::{DepthFrame, image, perception::SceneState, source::Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::watch;
#[derive(Clone)]
pub struct Config {
    pub url: String,
    pub model: Option<String>,
    pub candidates: usize,
    pub timeout: Duration,
    pub budget: Duration,
    pub tokens: u32,
    pub refresh: Duration,
}
impl Config {
    pub fn new(url: String) -> Self {
        Self {
            url,
            model: None,
            candidates: 4,
            timeout: Duration::from_secs(8),
            budget: Duration::from_secs(30),
            tokens: 400,
            refresh: Duration::from_secs(2),
        }
    }
}
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub observation_id: String,
    pub hypothesis: String,
    pub activity: String,
    pub relative_to: String,
    pub relationship: String,
    pub z_m: f64,
    pub confidence: f64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    claims: Vec<Claim>,
}
#[derive(Clone, Debug, Serialize, Default)]
pub struct Report {
    pub model: String,
    pub scene_id: String,
    pub text: String,
    pub claims: Vec<Claim>,
    pub usage: Value,
    pub latency_ms: u64,
    pub success: bool,
    pub signature: String,
}
pub fn signature(s: &SceneState) -> String {
    format!(
        "{}|{:?}|{:?}",
        s.status,
        s.optics,
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
type Job = (Arc<SceneState>, Arc<DepthFrame>);
pub struct Interpreter {
    tx: Option<watch::Sender<Option<Job>>>,
    cancel: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
    result: Arc<Mutex<Report>>,
    last: String,
    last_at: Instant,
    event: String,
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
        if config.timeout.is_zero()
            || config.budget.is_zero()
            || config.candidates == 0
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
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let thread = std::thread::spawn(move || {
            rt.block_on(async move {
                let work = async {
                    let mut api = Api {
                        client,
                        c: config,
                        model: None,
                        level: 0,
                        failures: 0,
                        key: std::env::var("OPENAI_API_KEY").ok(),
                        previous: None,
                    };
                    while rx.changed().await.is_ok() {
                        let Some((s, f)) = rx.borrow_and_update().clone() else {
                            continue;
                        };
                        let sig = signature(&s);
                        let now = Instant::now();
                        let mut report = match api.interpret(&s, &f).await {
                            Ok(r) => {
                                api.failures = 0;
                                r
                            }
                            Err(e) => {
                                api.failures += 1;
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
        })
    }
    pub fn submit(&mut self, s: Arc<SceneState>, f: Arc<DepthFrame>, refresh: Duration) {
        let sig = signature(&s);
        let event = s.events.last().map(|e| e.id.clone()).unwrap_or_default();
        let retry = !self.result.lock().unwrap().success
            && self.last_at.elapsed() >= refresh.max(Duration::from_millis(100));
        if self.last.is_empty()
            || self.event != event
            || (self.last != sig && self.last_at.elapsed() >= refresh)
            || retry
        {
            self.last = sig;
            self.event = event;
            self.last_at = Instant::now();
            self.tx.as_ref().unwrap().send_replace(Some((s, f)));
        }
    }
    pub fn latest(&self, s: &SceneState) -> Option<Report> {
        let r = self.result.lock().unwrap();
        (r.signature == signature(s)).then(|| r.clone())
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
    client: Client,
    c: Config,
    model: Option<String>,
    level: u8,
    failures: u8,
    key: Option<String>,
    previous: Option<(String, Value, Value)>,
}
fn image_part(data: Vec<u8>) -> Value {
    json!({"type":"image_url","image_url":{"url":format!("data:image/png;base64,{}",STANDARD.encode(data))}})
}
impl Api {
    async fn call(&self, path: &str, body: Option<Value>) -> Result<(StatusCode, Value)> {
        let url = format!("{}/{}", self.c.url.trim_end_matches('/'), path);
        let mut r = if let Some(b) = body {
            self.client.post(url).json(&b)
        } else {
            self.client.get(url)
        };
        if let Some(key) = &self.key {
            r = r.bearer_auth(key)
        }
        let mut response = r.send().await.map_err(|e| {
            if e.is_timeout() {
                "AI request timed out"
            } else {
                "AI endpoint unreachable"
            }
        })?;
        let status = response.status();
        if !status.is_success() {
            return Ok((status, Value::Null));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > 1_048_576 {
                return Err("oversized AI response".into());
            }
            bytes.extend_from_slice(&chunk)
        }
        Ok((status, serde_json::from_slice(&bytes)?))
    }
    async fn select(&self) -> Result<String> {
        if let Some(m) = &self.c.model {
            return Ok(m.clone());
        }
        let (status, models) = self.call("models", None).await?;
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
            let mut passed = true;
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
                let res=self.call("chat/completions",Some(json!({"model":m,"messages":[{"role":"user","content":parts}],"max_tokens":4}))).await;
                if !matches!(res,Ok((s,ref v)) if s.is_success() && v["choices"][0]["message"]["content"].as_str().unwrap_or("").trim()==["TL","TR","BL","BR"][q])
                {
                    passed = false;
                    break;
                }
            }
            if passed && best.as_ref().is_none_or(|(d, _)| start.elapsed() < *d) {
                best = Some((start.elapsed(), m.to_owned()))
            }
        }
        best.map(|(_, m)| m)
            .ok_or_else(|| "no candidate passed the image probe".into())
    }
    async fn interpret(&mut self, s: &SceneState, f: &DepthFrame) -> Result<Report> {
        if self.model.is_none() {
            self.model = Some(
                tokio::time::timeout(self.c.budget, self.select())
                    .await
                    .map_err(|_| "model discovery deadline expired")??,
            )
        }
        let (w, h, rgb) = image::rgb(f, 224, 4000);
        let img = image_part(image::png(w, h, &rgb)?);
        let schema = schema();
        let text = format!(
            "Depth only. Colours encode distance, not appearance. Red near, blue 4 m or farther, black invalid. Do not infer identity, colour, texture or text. Supply shape hypotheses only. Use current observation IDs, local Z and direction. Use an empty claims array when uncertain. Return JSON matching {schema}. Current scene: {}",
            serde_json::to_string(s)?
        );
        let mut parts = vec![json!({"type":"text","text":text})];
        if s.objects.iter().any(|o| o.moving) {
            if let Some((id, image, history)) = &self.previous {
                parts.push(
                    json!({"type":"text","text":format!("Previous keyframe {id}: {history}")}),
                );
                parts.push(image.clone())
            }
        }
        parts.push(json!({"type":"text","text":"Current keyframe"}));
        parts.push(img.clone());
        let mut tries = 0;
        loop {
            let mut body = json!({"model":self.model,"messages":[{"role":"user","content":parts}],"max_tokens":self.c.tokens});
            if self.level == 0 {
                body["response_format"] = json!({"type":"json_schema","json_schema":{"name":"depth_claims","strict":true,"schema":schema}})
            } else if self.level == 1 {
                body["response_format"] = json!({"type":"json_object"})
            }
            let (status, v) = self.call("chat/completions", Some(body)).await?;
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
            if let Some(reply) = parsed
                && valid(s, &reply.claims)
            {
                let prev = self.previous.as_ref().map(|p| &p.2);
                let current = json!(s.objects);
                let change = if prev == Some(&current) {
                    "Measurements unchanged"
                } else {
                    "Measurements updated"
                };
                let text = format!(
                    "{} {}. {}",
                    crate::describe::describe(s),
                    change,
                    reply
                        .claims
                        .iter()
                        .map(|c| format!("Possible {} ({})", c.hypothesis, c.observation_id))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                self.previous = Some((s.id.clone(), img, current));
                return Ok(Report {
                    success: true,
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
    json!({"type":"object","additionalProperties":false,"required":["claims"],"properties":{"claims":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["observation_id","hypothesis","activity","relative_to","relationship","z_m","confidence"],"properties":{"observation_id":{"type":"string"},"hypothesis":{"enum":["person-like shape","hand-like shape","unclassified surface"]},"activity":{"enum":["approaching","receding","steady"]},"relative_to":{"type":"string"},"relationship":{"enum":["none","left","right","nearer","farther"]},"z_m":{"type":"number"},"confidence":{"type":"number"}}}}}})
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
