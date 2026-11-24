//! Optional session configuration, installed before worker startup.
use serde::Deserialize;
use std::{
    path::Path,
    sync::{LazyLock, OnceLock},
};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub occlusion_ms: u64,
    pub activity_debounce_ms: u64,
    pub history_windows_s: [u64; 3],
    pub label_lifetime_s: u64,
    pub spatial_budget_bytes: usize,
    pub labels: Vec<String>,
}
static CONFIG: OnceLock<Settings> = OnceLock::new();
impl Settings {
    pub fn get() -> &'static Self {
        static DEFAULT: LazyLock<Settings> = LazyLock::new(|| {
            serde_json::from_str(include_str!("../config/semantics.json"))
                .expect("built-in settings")
        });
        CONFIG.get().unwrap_or(&DEFAULT)
    }
    pub fn configure(path: &Path) -> crate::source::Result<()> {
        let value: Self = serde_json::from_reader(std::fs::File::open(path)?)?;
        let allowed = [
            "unknown",
            "person-like shape",
            "hand-like shape",
            "chair-like",
            "table-like",
            "box-like",
        ];
        if !(50..=5000).contains(&value.occlusion_ms)
            || !(50..=5000).contains(&value.activity_debounce_ms)
            || !(1..=120).contains(&value.label_lifetime_s)
            || !(2048..=8192).contains(&value.spatial_budget_bytes)
            || value.history_windows_s[0] == 0
            || value.history_windows_s[2] > 300
            || value.history_windows_s.windows(2).any(|a| a[0] >= a[1])
            || !value.labels.iter().any(|l| l == "unknown")
            || value.labels.len() > allowed.len()
            || value.labels.iter().any(|l| !allowed.contains(&l.as_str()))
        {
            return Err("invalid semantic durations, vocabulary or byte budget".into());
        }
        CONFIG
            .set(value)
            .map_err(|_| "semantics already configured".into())
    }
}
