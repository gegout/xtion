//! Validate entity interpretations against the exact bounded observation sent to the endpoint.
use crate::{ai::Claim, semantic::SemanticSceneGraph};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityInterpretation {
    pub entity: String,
    pub evidence: String,
    pub label: String,
    pub posture: String,
    pub activity: String,
    pub relationship: String,
    pub other: String,
    pub z_m: f64,
    pub uncertainty: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Interpretation {
    pub snapshot: String,
    pub claims: Vec<EntityInterpretation>,
    pub change: String,
    pub description: String,
}

pub(super) fn schema() -> Value {
    let mut fields = json!({});
    for k in [
        "entity",
        "evidence",
        "label",
        "posture",
        "activity",
        "relationship",
        "other",
        "uncertainty",
    ] {
        fields[k] = json!({"type":"string"});
    }
    fields["z_m"] = json!({"type":"number"});
    let required = fields.as_object().unwrap().keys().collect::<Vec<_>>();
    json!({"type":"object","additionalProperties":false,"required":["snapshot","claims","change","description"],"properties":{"snapshot":{"type":"string"},"change":{"type":"string"},"description":{"type":"string"},"claims":{"type":"array","items":{"type":"object","additionalProperties":false,"required":required,"properties":fields}}}})
}
pub(super) fn validate(
    raw: &str,
    packet: &Value,
    g: &SemanticSceneGraph,
) -> Result<(Interpretation, Vec<Claim>), &'static str> {
    let r: Interpretation = serde_json::from_str(raw).map_err(|_| "schema")?;
    if r.snapshot != g.snapshot {
        return Err("stale_snapshot");
    }
    if r.description != "visible surfaces" && r.description != "insufficient evidence" {
        return Err("unsupported_description");
    }
    let activities = packet["activities"].as_array().ok_or("packet")?;
    if r.change != "unknown" && !activities.iter().any(|a| a["id"] == r.change) {
        return Err("unknown_change");
    }
    let mut claims = vec![];
    let mut ids = std::collections::HashSet::new();
    for c in &r.claims {
        if !ids.insert(&c.entity) {
            return Err("duplicate_entity");
        }
        let e = packet["entities"]
            .as_array()
            .ok_or("packet")?
            .iter()
            .find(|e| e["mark"] == c.entity)
            .ok_or("unknown_entity")?;
        if !e["observations"]
            .as_array()
            .is_some_and(|v| v.iter().any(|v| v == &c.evidence))
        {
            return Err("unsupported_evidence");
        }
        if ![
            "unknown",
            "person-like shape",
            "hand-like shape",
            "chair-like",
            "table-like",
            "box-like",
        ]
        .contains(&c.label.as_str())
        {
            return Err("unsupported_label");
        }
        if c.posture != "unknown" {
            return Err("unsupported_posture");
        }
        if !["uncertain", "partial visibility", "geometry only"].contains(&c.uncertainty.as_str()) {
            return Err("unsupported_uncertainty");
        }
        if (c.z_m - e["z_m"].as_f64().ok_or("missing_measurement")?).abs() > 0.005 {
            return Err("contradictory_number");
        }
        let id = c
            .entity
            .strip_prefix('E')
            .and_then(|x| x.parse::<u64>().ok())
            .ok_or("unknown_entity")?;
        if c.activity != "unknown"
            && !activities
                .iter()
                .any(|a| a["id"] == c.activity && a["entity"] == id && a["end_ns"].is_null())
        {
            return Err("contradictory_activity");
        }
        if c.relationship != "unknown" {
            if !packet["relationships"].as_array().is_some_and(|list| {
                list.iter().any(|v| {
                    v["subject"] == id && v["predicate"] == c.relationship && v["object"] == c.other
                })
            }) {
                return Err("contradictory_relationship");
            }
        } else if !c.other.is_empty() {
            return Err("unknown_entity");
        }
        claims.push(Claim {
            observation_id: c.evidence.clone(),
            hypothesis: c.label.clone(),
            activity: c.activity.clone(),
            relative_to: c.other.clone(),
            relationship: c.relationship.clone(),
            z_m: e["z_m"].as_f64().unwrap(),
            confidence: 0.,
        });
    }
    Ok((r, claims))
}
