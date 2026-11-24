//! Interval reasoning uses host monotonic time. A coverage gap interrupts every ongoing interval.
use crate::semantic::{Provenance, SemanticSceneGraph};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActivityHypothesis {
    pub id: String,
    pub entity: u64,
    pub kind: String,
    pub start_ns: u64,
    pub end_ns: Option<u64>,
    pub evidence: Vec<String>,
    pub quality: f64,
    pub provenance: Provenance,
    pub alternative: String,
}
pub type TemporalEvent = ActivityHypothesis;
struct Candidate {
    kind: &'static str,
    since: u64,
    evidence: String,
    last_seen: u64,
    active: Option<ActivityHypothesis>,
}
pub struct ActivityEngine {
    states: BTreeMap<u64, Candidate>,
    history: VecDeque<ActivityHypothesis>,
    serial: u64,
    pub debounce_ns: u64,
    pub windows_s: [u64; 3],
}
impl Default for ActivityEngine {
    fn default() -> Self {
        Self {
            states: BTreeMap::new(),
            history: VecDeque::new(),
            serial: 0,
            debounce_ns: 400_000_000,
            windows_s: [2, 10, 60],
        }
    }
}
impl ActivityEngine {
    fn remember(&mut self, a: ActivityHypothesis) {
        if self.history.len() == 128 {
            self.history.pop_front();
        }
        self.history.push_back(a);
    }
    pub fn observe(&mut self, g: &SemanticSceneGraph) -> Vec<ActivityHypothesis> {
        let mut closed = vec![];
        self.states.retain(|id, s| {
            let visible = g.entities.iter().any(|e| e.id == *id);
            if !visible && let Some(mut a) = s.active.take() {
                a.end_ns = Some(s.last_seen);
                closed.push(a);
            }
            if !visible {
                s.kind = "";
                s.since = g.ns;
            }
            let expired = g.expired.contains(id);
            if expired && g.status == "tracking" {
                closed.push(ActivityHypothesis {
                    id: format!("left-{id}-{}", s.last_seen),
                    entity: *id,
                    kind: "left visible tracking".into(),
                    start_ns: s.last_seen,
                    end_ns: Some(g.ns),
                    evidence: vec![s.evidence.clone(), g.snapshot.clone()],
                    quality: 0.,
                    provenance: Provenance::LocalInference,
                    alternative: "Occlusion or tracking failure".into(),
                });
            }
            !expired && g.status == "tracking"
        });
        for a in closed {
            self.remember(a);
        }
        for e in &g.entities {
            let ev = &e.evidence[0].observation;
            if !self.states.contains_key(&e.id) {
                self.remember(ActivityHypothesis {
                    id: format!("enter-{}-{}", e.id, g.ns),
                    entity: e.id,
                    kind: "entered visible tracking".into(),
                    start_ns: g.ns,
                    end_ns: Some(g.ns),
                    evidence: vec![ev.clone()],
                    quality: e.quality,
                    provenance: Provenance::LocalInference,
                    alternative: "Previously hidden or fragmented surface".into(),
                });
            }
            let s = self.states.entry(e.id).or_insert_with(|| Candidate {
                kind: "",
                since: g.ns,
                evidence: ev.clone(),
                last_seen: g.ns,
                active: None,
            });
            s.last_seen = g.ns;
            let k = match e.velocity_m_s {
                Some([x, _, z]) if e.moving && x.abs() > 0.12 && x.abs() > z.abs() => {
                    if x > 0. {
                        "moving right"
                    } else {
                        "moving left"
                    }
                }
                Some([_, _, z]) if e.moving && z.abs() > 0.12 => {
                    if z < 0. {
                        "approaching"
                    } else {
                        "receding"
                    }
                }
                _ if !e.moving => "stationary surface",
                _ => "unresolved motion",
            };
            if s.kind != k {
                s.kind = k;
                s.since = g.ns;
                s.evidence = ev.clone();
            }
            if g.ns.saturating_sub(s.since) >= self.debounce_ns
                && s.active.as_ref().is_none_or(|a| a.kind != k)
            {
                self.serial += 1;
                let a = ActivityHypothesis {
                    id: format!("activity-{}", self.serial),
                    entity: e.id,
                    kind: k.into(),
                    start_ns: s.since,
                    end_ns: None,
                    evidence: vec![s.evidence.clone(), ev.clone()],
                    quality: e.quality,
                    provenance: Provenance::Derived,
                    alternative: "Centroid change from partial visibility".into(),
                };
                if let Some(mut old) = s.active.replace(a) {
                    old.end_ns = Some(s.since);
                    self.remember(old);
                }
            }
        }
        let horizon = self
            .windows_s
            .iter()
            .max()
            .copied()
            .unwrap_or(60)
            .saturating_mul(1_000_000_000);
        self.history
            .retain(|a| g.ns.saturating_sub(a.end_ns.unwrap_or(g.ns)) <= horizon);
        self.history
            .iter()
            .cloned()
            .chain(self.states.values().filter_map(|s| s.active.clone()))
            .collect()
    }
    pub fn narrative(events: &[ActivityHypothesis], now: u64, window_s: u64) -> String {
        let recent = events
            .iter()
            .filter(|e| {
                now.saturating_sub(e.end_ns.unwrap_or(now))
                    <= window_s.saturating_mul(1_000_000_000)
            })
            .rev()
            .take(5)
            .map(|e| {
                format!(
                    "Entity {} {}{}",
                    e.entity,
                    e.kind,
                    if e.end_ns.is_none() { " (ongoing)" } else { "" }
                )
            })
            .collect::<Vec<_>>();
        if recent.is_empty() {
            "No evidenced activity in this window.".into()
        } else {
            recent.join(". ")
        }
    }
}
