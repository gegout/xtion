//! Configuration and grounded results shared by the desktop and headless consumers.
mod transcript;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
pub use transcript::{Exchange, Transcript};
#[derive(Clone)]
pub struct Config {
    pub url: String,
    /// Session-only bearer credential for this endpoint.
    pub api_key: Option<String>,
    pub transcript: Option<Transcript>,
    pub model: Option<String>,
    pub candidates: usize,
    pub timeout: Duration,
    pub budget: Duration,
    pub tokens: u32,
    pub probe_tokens: u32,
    pub refresh: Duration,
    pub min_words: usize,
    pub max_words: usize,
}
impl Config {
    pub fn validate_words(&self) -> crate::source::Result<()> {
        if !(1..=40).contains(&self.min_words)
            || !(20..=120).contains(&self.max_words)
            || self.max_words < self.min_words + 6
        {
            return Err("choose a minimum of 1–40 words and maximum of 20–120, at least six above the minimum".into());
        }
        Ok(())
    }
    pub fn environment_key() -> Option<String> {
        std::env::var("OPENAI_API_KEY")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    }
    pub fn new(url: String) -> Self {
        Self {
            api_key: if url.is_empty() {
                None
            } else {
                Self::environment_key()
            },
            url,
            transcript: None,
            model: None,
            candidates: 4,
            timeout: Duration::from_secs(20),
            budget: Duration::from_secs(30),
            tokens: 400,
            probe_tokens: 4,
            refresh: Duration::from_secs(7),
            min_words: 20,
            max_words: 40,
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
#[derive(Clone, Debug, Serialize, Default)]
pub struct Report {
    pub model: String,
    pub scene_id: String,
    pub text: String,
    #[serde(skip)]
    pub history_key: Option<String>,
    pub claims: Vec<Claim>,
    pub supporting_ids: Vec<String>,
    pub usage: Value,
    pub latency_ms: u64,
    pub arrival_ns: u64,
    pub success: bool,
    pub signature: String,
    #[cfg(feature = "ai")]
    #[serde(skip)]
    evidence: Option<worker::Evidence>,
}

#[cfg(feature = "ai")]
mod history;
#[cfg(feature = "ai")]
mod spatial;
#[cfg(feature = "ai")]
mod worker;
#[cfg(feature = "ai")]
pub use worker::Interpreter;

#[cfg(not(feature = "ai"))]
pub struct Interpreter(std::convert::Infallible);
#[cfg(not(feature = "ai"))]
impl Interpreter {
    pub fn start(_: Config) -> crate::source::Result<Self> {
        Err("AI support was omitted at build time".into())
    }
    pub fn reconfigure(&mut self, _: Config) -> crate::source::Result<()> {
        match self.0 {}
    }
    pub fn submit(
        &mut self,
        _: std::sync::Arc<crate::perception::SceneState>,
        _: std::sync::Arc<crate::DepthFrame>,
        _: Duration,
    ) {
        match self.0 {}
    }
    pub fn status(&self) -> String {
        match self.0 {}
    }
    pub fn latest(&self, _: &crate::perception::SceneState) -> Option<Report> {
        match self.0 {}
    }
}
