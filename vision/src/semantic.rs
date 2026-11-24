//! Session-scoped depth semantics. All geometry uses optical metres (right, down, forward).
use crate::perception::{SceneState, TrackedObject};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provenance {
    Measured,
    Derived,
    LocalInference,
    AiHypothesis,
    Operator,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ObservationEvidence {
    pub frame: String,
    pub observation: String,
    pub ns: u64,
    pub provenance: Provenance,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SemanticHypothesis {
    pub label: String,
    pub evidence: Vec<String>,
    pub provenance: Provenance,
    pub expires_ns: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SemanticEntity {
    pub id: u64,
    pub members: Vec<u64>,
    pub member_history: Vec<u64>,
    pub first_ns: u64,
    pub last_ns: u64,
    pub position_m: Option<[f64; 3]>,
    pub bounds_m: Option<[[f64; 3]; 2]>,
    pub median_z_m: f64,
    pub velocity_m_s: Option<[f64; 3]>,
    pub image_bounds: [usize; 4],
    pub quality: f64,
    pub clipped: bool,
    pub moving: bool,
    pub labels: Vec<SemanticHypothesis>,
    pub evidence: Vec<ObservationEvidence>,
    pub provenance: Provenance,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpatialRelationship {
    pub subject: u64,
    pub predicate: String,
    pub object: String,
    pub frame: String,
    pub evidence: Vec<String>,
    pub provenance: Provenance,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SemanticSceneGraph {
    pub schema: String,
    pub session: String,
    pub snapshot: String,
    pub ns: u64,
    pub status: String,
    pub optics: crate::IntrinsicsSource,
    pub entities: Vec<SemanticEntity>,
    pub relationships: Vec<SpatialRelationship>,
    pub expired: Vec<u64>,
    pub activities: Vec<crate::activity::ActivityHypothesis>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphDelta {
    pub base: String,
    pub snapshot: String,
    pub upsert: Vec<SemanticEntity>,
    pub removed: Vec<u64>,
    pub relationships: Vec<SpatialRelationship>,
}
impl SemanticSceneGraph {
    pub fn delta(&self, previous: &Self) -> GraphDelta {
        GraphDelta {
            base: previous.snapshot.clone(),
            snapshot: self.snapshot.clone(),
            upsert: self
                .entities
                .iter()
                .filter(|e| !previous.entities.contains(e))
                .cloned()
                .collect(),
            removed: previous
                .entities
                .iter()
                .filter(|old| !self.entities.iter().any(|e| e.id == old.id))
                .map(|e| e.id)
                .collect(),
            relationships: self.relationships.clone(),
        }
    }
}
pub struct SemanticEngine {
    session: String,
    next: u64,
    active: BTreeMap<u64, SemanticEntity>,
    pairs: BTreeMap<(u64, u64), u8>,
    previous: BTreeSet<(u64, u64, String)>,
    pub timeout_ns: u64,
    pub activity: crate::activity::ActivityEngine,
}
impl Default for SemanticEngine {
    fn default() -> Self {
        Self::new("capture")
    }
}
impl SemanticEngine {
    pub fn new(session: impl Into<String>) -> Self {
        Self {
            session: session.into(),
            next: 0,
            active: BTreeMap::new(),
            pairs: BTreeMap::new(),
            previous: BTreeSet::new(),
            timeout_ns: 750_000_000,
            activity: Default::default(),
        }
    }
    pub fn observe(&mut self, s: &SceneState) -> SemanticSceneGraph {
        let mut expired = vec![];
        self.active.retain(|id, e| {
            let keep = s.ns.saturating_sub(e.last_ns) <= self.timeout_ns;
            if !keep {
                expired.push(*id)
            }
            keep
        });
        let mut graph = SemanticSceneGraph {
            schema: "xtion.semantic/1".into(),
            session: self.session.clone(),
            snapshot: s.id.clone(),
            ns: s.ns,
            status: s.status.into(),
            optics: s.optics,
            entities: vec![],
            relationships: vec![],
            expired,
            activities: vec![],
        };
        if s.status != "tracking" {
            self.pairs.clear();
            self.previous.clear();
            graph.activities = self.activity.observe(&graph);
            return graph;
        }
        let tracks: Vec<_> = s.objects.iter().take(16).collect();
        let mut groups: Vec<Vec<&TrackedObject>> = vec![];
        let mut votes = BTreeMap::new();
        for t in &tracks {
            for u in &tracks {
                if t.id >= u.id {
                    continue;
                }
                let key = (t.id, u.id);
                if coherent(t, u) {
                    votes.insert(
                        key,
                        self.pairs.get(&key).copied().unwrap_or(0).saturating_add(1),
                    );
                }
            }
        }
        self.pairs = votes;
        for t in tracks {
            if let Some(g) = groups.iter_mut().find(|g| {
                g.iter().all(|u| {
                    self.pairs
                        .get(&(u.id.min(t.id), u.id.max(t.id)))
                        .copied()
                        .unwrap_or(0)
                        >= 3
                })
            }) {
                g.push(t)
            } else {
                groups.push(vec![t])
            }
        }
        let mut used = BTreeSet::new();
        for group in groups {
            let main = *group.iter().max_by_key(|t| t.region.samples).unwrap();
            let members = group.iter().map(|t| t.id).collect::<Vec<_>>();
            let id = self
                .active
                .values()
                .filter(|e| !used.contains(&e.id))
                .filter_map(|e| {
                    let overlap = members.iter().filter(|m| e.members.contains(m)).count();
                    (overlap > 0).then_some((overlap, e.id))
                })
                .max_by_key(|&(n, id)| (n, std::cmp::Reverse(id)))
                .map(|(_, id)| id)
                .unwrap_or_else(|| {
                    self.next += 1;
                    self.next
                });
            used.insert(id);
            let old = self.active.get(&id);
            let mut history = old.map(|e| e.member_history.clone()).unwrap_or_default();
            for &m in &members {
                if !history.contains(&m) {
                    history.push(m)
                }
            }
            if history.len() > 32 {
                history.drain(..history.len() - 32);
            }
            let mut b = main.region.bounds_m;
            let mut image = main.region.bounds;
            for t in &group {
                if let (Some(bounds), Some(new)) = (&mut b, t.region.bounds_m) {
                    for k in 0..3 {
                        bounds[0][k] = bounds[0][k].min(new[0][k]);
                        bounds[1][k] = bounds[1][k].max(new[1][k]);
                    }
                }
                for k in 0..2 {
                    image[k] = image[k].min(t.region.bounds[k]);
                    image[k + 2] = image[k + 2].max(t.region.bounds[k + 2]);
                }
            }
            let e = SemanticEntity {
                id,
                members,
                member_history: history,
                first_ns: old.map_or(s.ns, |e| e.first_ns),
                last_ns: s.ns,
                position_m: main.region.centroid.map(|p| [p.x, p.y, p.z]),
                bounds_m: b,
                image_bounds: image,
                median_z_m: main.region.median_z_m,
                quality: group.iter().map(|t| t.region.quality).fold(1., f64::min),
                clipped: group.iter().any(|t| t.region.clipped),
                velocity_m_s: main.filtered_velocity,
                moving: main.moving,
                labels: vec![],
                evidence: group
                    .iter()
                    .map(|t| ObservationEvidence {
                        frame: s.id.clone(),
                        observation: t.observation_id.clone(),
                        ns: s.ns,
                        provenance: Provenance::Measured,
                    })
                    .collect(),
                provenance: Provenance::Derived,
            };
            self.active.insert(id, e.clone());
            graph.entities.push(e);
        }
        // Bound retained occlusions under adversarial churn. Visible entities take precedence.
        while self.active.len() > 64 {
            let Some(id) = self
                .active
                .values()
                .filter(|e| !used.contains(&e.id))
                .min_by_key(|e| e.last_ns)
                .map(|e| e.id)
            else {
                break;
            };
            self.active.remove(&id);
            graph.expired.push(id);
        }
        let mut current = BTreeSet::new();
        for (i, a) in graph.entities.iter().enumerate() {
            for b in &graph.entities[i + 1..] {
                let (mut a, mut b) = (a, b);
                if a.id > b.id {
                    std::mem::swap(&mut a, &mut b)
                }
                let (Some(p), Some(q)) = (a.position_m, b.position_m) else {
                    continue;
                };
                let mut add = |kind: &str| {
                    current.insert((a.id, b.id, kind.to_owned()));
                    graph.relationships.push(SpatialRelationship {
                        subject: a.id,
                        predicate: kind.into(),
                        object: format!("entity-{}", b.id),
                        frame: "sensor".into(),
                        evidence: vec![
                            a.evidence[0].observation.clone(),
                            b.evidence[0].observation.clone(),
                        ],
                        provenance: Provenance::Derived,
                    });
                };
                for (k, pos, neg) in [
                    (0, "right of", "left of"),
                    (1, "below", "above"),
                    (2, "behind", "in front of"),
                ] {
                    let d = p[k] - q[k];
                    let kind = if d > 0. { pos } else { neg };
                    let band = if self.previous.contains(&(a.id, b.id, kind.into())) {
                        0.03
                    } else {
                        0.06
                    };
                    if d.abs() > band {
                        add(kind)
                    }
                }
                let d = dist(p, q);
                if d < 0.7 || d < 0.8 && self.previous.contains(&(a.id, b.id, "near".into())) {
                    add("near")
                } else if d > 1.3 || d > 1.2 && self.previous.contains(&(a.id, b.id, "far".into()))
                {
                    add("far")
                }
                if (0..2).all(|k| {
                    a.image_bounds[k] <= b.image_bounds[k + 2]
                        && b.image_bounds[k] <= a.image_bounds[k + 2]
                }) && (p[2] - q[2]).abs() > 0.06
                {
                    add(if p[2] < q[2] {
                        "overlaps in front"
                    } else {
                        "overlaps behind"
                    })
                }
                if let (Some(v), Some(w)) = (a.velocity_m_s, b.velocity_m_s)
                    && d > 0.05
                {
                    let rate = (0..3).map(|k| (p[k] - q[k]) * (v[k] - w[k])).sum::<f64>() / d;
                    if rate.abs() > 0.15 {
                        add(if rate < 0. { "approaching" } else { "receding" })
                    }
                }
            }
            if let (Some(plane), Some(bounds), Some(p)) =
                (s.floor_estimate, a.bounds_m, a.position_m)
            {
                let d = plane.normal[0] * p[0]
                    + plane.normal[1] * bounds[1][1]
                    + plane.normal[2] * p[2]
                    - plane.offset;
                if d.abs() < 0.03 {
                    graph.relationships.push(SpatialRelationship {
                        subject: a.id,
                        predicate: "touches support-plane estimate".into(),
                        object: "support-plane".into(),
                        frame: "sensor".into(),
                        evidence: vec![a.evidence[0].observation.clone(), s.id.clone()],
                        provenance: Provenance::Derived,
                    });
                }
            }
        }
        self.previous = current;
        graph.activities = self.activity.observe(&graph);
        graph
    }
}
fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2])
}
fn coherent(a: &TrackedObject, b: &TrackedObject) -> bool {
    let (Some(x), Some(y), Some(v), Some(w)) = (
        a.region.bounds_m,
        b.region.bounds_m,
        a.filtered_velocity,
        b.filtered_velocity,
    ) else {
        return false;
    };
    let gap = std::array::from_fn(|k| (x[0][k] - y[1][k]).max(y[0][k] - x[1][k]).max(0.));
    a.moving
        && b.moving
        && dist(gap, [0.; 3]) < 0.06
        && dist(v, w) < 0.10
        && dist(v, [0.; 3]) > 0.12
        && v.iter().zip(w).map(|(a, b)| a * b).sum::<f64>() > 0.
}
