//! Configuration and grounded results shared by the desktop and headless consumers.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
#[derive(Clone)]
pub struct Config {
    pub url: String,
    pub model: Option<String>,
    pub candidates: usize,
    pub timeout: Duration,
    pub budget: Duration,
    pub tokens: u32,
    pub probe_tokens: u32,
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
            probe_tokens: 4,
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
#[derive(Clone, Debug, Serialize, Default)]
pub struct Report {
    pub model: String,
    pub scene_id: String,
    pub text: String,
    pub claims: Vec<Claim>,
    pub usage: Value,
    pub latency_ms: u64,
    pub arrival_ns: u64,
    pub success: bool,
    pub signature: String,
}

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
    pub fn submit(
        &mut self,
        _: std::sync::Arc<crate::perception::SceneState>,
        _: std::sync::Arc<crate::DepthFrame>,
        _: Duration,
    ) {
        match self.0 {}
    }
    pub fn latest(&self, _: &crate::perception::SceneState) -> Option<Report> {
        match self.0 {}
    }
}
