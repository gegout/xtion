//! Deterministic scene queries. Every numerical answer is formatted from the local graph.
use crate::semantic::{SemanticEntity, SemanticSceneGraph};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Answer {
    pub source: String,
    pub text: String,
    pub evidence: Vec<String>,
}
pub fn answer(g: &SemanticSceneGraph, question: &str, selected: &[u64]) -> Answer {
    let q = question.trim().to_lowercase();
    let respond = |source: &str, text: String, evidence: Vec<String>| Answer {
        source: source.into(),
        text,
        evidence,
    };
    let unknown = || {
        respond(
            "unknown",
            "There is insufficient evidence for that question.".into(),
            vec![],
        )
    };
    if q.contains("recent") || q.contains("happened") || q.contains("history") {
        let windows = crate::semantic_config::Settings::get().history_windows_s;
        let window = q
            .split_whitespace()
            .find_map(|s| s.parse::<u64>().ok())
            .unwrap_or(windows[1]);
        if !windows.contains(&window) {
            return unknown();
        }
        return respond(
            "historical",
            crate::activity::ActivityEngine::narrative(&g.activities, g.ns, window),
            g.activities
                .iter()
                .rev()
                .filter(|a| g.ns.saturating_sub(a.end_ns.unwrap_or(g.ns)) <= window * 1_000_000_000)
                .take(5)
                .flat_map(|a| a.evidence.clone())
                .collect(),
        );
    }
    if g.status != "tracking" {
        return unknown();
    }
    let ids = q
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u64>().ok())
        .collect::<Vec<_>>();
    let ids = if ids.is_empty() { selected } else { &ids };
    let ev = |e: &SemanticEntity| {
        e.evidence
            .iter()
            .map(|v| v.observation.clone())
            .collect::<Vec<_>>()
    };
    if q.contains("person") || q.contains("people") || q.contains("hand") || q.contains("posture") {
        return unknown();
    }
    if q.contains("count") || q.contains("how many") {
        if !["entit", "object", "region", "track"]
            .iter()
            .any(|k| q.contains(k))
            && !matches!(q.as_str(), "count" | "how many?")
        {
            return unknown();
        }
        return respond(
            "measured now",
            format!("{} visible tracked entities.", g.entities.len()),
            vec![g.snapshot.clone()],
        );
    }
    if q.contains("nearest") {
        return g
            .entities
            .iter()
            .min_by(|a, b| a.median_z_m.total_cmp(&b.median_z_m))
            .map_or_else(unknown, |e| {
                respond(
                    "measured now",
                    format!(
                        "Entity {} has the nearest median axial depth, {:.2} m.",
                        e.id, e.median_z_m
                    ),
                    ev(e),
                )
            });
    }
    let chosen = ids
        .iter()
        .map(|id| g.entities.iter().find(|e| e.id == *id))
        .collect::<Option<Vec<_>>>();
    let Some(chosen) = chosen else {
        return unknown();
    };
    if let [a, b] = chosen.as_slice()
        && (q.contains("distance") || q.contains("apart"))
        && let (Some(x), Some(y)) = (a.position_m, b.position_m)
    {
        let d = (x[0] - y[0]).hypot(x[1] - y[1]).hypot(x[2] - y[2]);
        let mut evidence = ev(a);
        evidence.extend(ev(b));
        return respond(
            "measured now",
            format!(
                "Centroid separation is {d:.2} m with {:?} optics.",
                g.optics
            ),
            evidence,
        );
    }
    if let [e] = chosen.as_slice() {
        if q.contains("distance")
            && let Some([x, y, z]) = e.position_m
        {
            return respond(
                "measured now",
                format!(
                    "Entity {} centroid range is {:.2} m with {:?} optics.",
                    e.id,
                    x.hypot(y).hypot(z),
                    g.optics
                ),
                ev(e),
            );
        }
        if q.contains("depth") || q.contains("distance") {
            return respond(
                "measured now",
                format!("Entity {} is {:.2} m forward.", e.id, e.median_z_m),
                ev(e),
            );
        }
        if (q.contains("position") || q.contains("where"))
            && let Some([x, y, z]) = e.position_m
        {
            return respond(
                "measured now",
                format!(
                    "Sensor XYZ ({x:.2}, {y:.2}, {z:.2}) m with {:?} optics.",
                    g.optics
                ),
                ev(e),
            );
        }
        if q.contains("label")
            && let Some(h) = e.labels.first()
        {
            return respond(
                "AI interpretation",
                format!("{} hypothesis", h.label),
                h.evidence.clone(),
            );
        }
    }
    unknown()
}
