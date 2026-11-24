//! Bounded depth observations, in capture time. Each sample retains the four largest tracks.
use crate::perception::SceneState;

use std::collections::VecDeque;

const SECOND: u64 = 1_000_000_000;
const BRIEF: &str = include_str!("scene-prompt.txt");

#[derive(Clone)]
struct Track {
    id: u64,
    obs: String,
    sector: usize,
    x: f64,
    z: f64,
    moving: bool,
}
#[derive(Clone)]
struct Sample {
    ns: u64,
    status: &'static str,
    tracks: Vec<Track>,
}
#[derive(Default, Clone)]
pub(super) struct History {
    samples: VecDeque<Sample>,
}
#[derive(Clone, Debug)]
pub(super) struct Finding {
    pub key: String,
    pub text: String,
    pub ids: Vec<String>,
    pub track: u64,
}
impl History {
    pub fn observe(&mut self, s: &SceneState, width: usize) -> bool {
        if let Some(last) = self.samples.back() {
            if s.ns < last.ns || s.ns - last.ns > 2 * SECOND {
                self.samples.clear();
            } else if s.ns - last.ns < SECOND {
                return false;
            }
        }
        while self
            .samples
            .front()
            .is_some_and(|p| s.ns - p.ns > 30 * SECOND)
        {
            self.samples.pop_front();
        }
        let mut regions: Vec<_> = s.objects.iter().collect();
        regions.sort_by_key(|r| std::cmp::Reverse(r.region.samples));
        let tracks = regions
            .into_iter()
            .take(4)
            .map(|o| {
                let x = o.region.center[0] / width.max(1) as f64;
                Track {
                    id: o.id,
                    obs: o.observation_id.clone(),
                    sector: (x * 3.) as usize,
                    x,
                    z: o.region.z,
                    moving: o.moving,
                }
            })
            .collect();
        self.samples.push_back(Sample {
            ns: s.ns,
            status: s.status,
            tracks,
        });
        true
    }

    pub fn findings(&self, s: &SceneState, width: usize) -> Vec<Finding> {
        let mut out = Vec::new();
        if s.status != "tracking" {
            return out;
        }
        for o in &s.objects {
            let mut path = Vec::new();
            for sample in self.samples.iter().rev() {
                if sample.status != "tracking" || s.ns.saturating_sub(sample.ns) > 30 * SECOND {
                    break;
                }
                let Some(t) = sample.tracks.iter().find(|t| t.id == o.id) else {
                    break;
                };
                path.push((sample.ns, t));
            }
            let Some(&(start, first)) = path.last() else {
                continue;
            };
            let secs = (s.ns - start) / SECOND;
            if secs < 3 {
                continue;
            }
            let x = o.region.center[0] / width.max(1) as f64;
            let sector = ((x * 3.) as usize).min(2);
            let location = ["on the left", "in the centre", "on the right"][sector];
            let z = o.region.z;
            let dz = z - first.z;
            let stayed = !o.moving
                && path.iter().all(|(_, p)| {
                    !p.moving
                        && p.sector == sector
                        && (p.x - x).abs() < 0.07
                        && (p.z - z).abs() < 0.10
                });
            let settled = !o.moving
                && path.iter().take(2).all(|(_, p)| !p.moving)
                && path.iter().any(|(_, p)| p.moving);
            let (kind, text) = if settled && dz.abs() >= 0.20 {
                let (kind, direction) = if dz < 0. {
                    ("approached then settled", "closer to")
                } else {
                    ("receded then settled", "farther from")
                };
                (
                    kind,
                    format!(
                        "A foreground shape moved {direction} the camera, then settled {location} of the view. Its last measured distance was about {z:.1} metres."
                    ),
                )
            } else if dz.abs() >= 0.20 {
                let dir = if dz < 0. { "closer to" } else { "farther from" };
                (
                    dir,
                    format!(
                        "A foreground shape ended up {:.1} metres {dir} the camera over {secs} seconds. It was last visible {location} of the view.",
                        dz.abs()
                    ),
                )
            } else if settled {
                (
                    "settled",
                    format!(
                        "A moving shape settled {location} of the view. The last observations placed it about {z:.1} metres in front of the camera."
                    ),
                )
            } else if stayed {
                (
                    "still",
                    format!(
                        "A foreground shape stayed {location} of the view for {secs} seconds. Its distance held near {z:.1} metres with little change between observations."
                    ),
                )
            } else {
                continue;
            };
            out.push(Finding {
                key: format!("{}-{kind}-{sector}", o.id),
                text,
                ids: vec![first.obs.clone(), o.observation_id.clone()],
                track: o.id,
            });
        }
        out
    }
    /// Explain empty or fragmented foreground when no individual track has a useful path.
    pub fn tracking_pattern(&self, s: &SceneState) -> Option<(String, String)> {
        let samples: Vec<_> = self
            .samples
            .iter()
            .rev()
            .take_while(|p| p.status == "tracking" && s.ns.saturating_sub(p.ns) <= 30 * SECOND)
            .collect();
        let oldest = samples.last()?;
        let secs = (s.ns - oldest.ns) / SECOND;
        if secs < 5 || s.status != "tracking" {
            return None;
        }
        if s.objects.is_empty() && samples.iter().all(|p| p.tracks.is_empty()) {
            return Some((
                "clear".into(),
                format!(
                    "Across {secs} seconds of depth observations, the camera picked up the background without finding a persistent foreground shape moving through the view."
                ),
            ));
        }
        let ids: std::collections::HashSet<_> = samples
            .iter()
            .flat_map(|p| p.tracks.iter().map(|t| t.id))
            .collect();
        if ids.len() >= 3
            && ids.iter().all(|id| {
                samples
                    .iter()
                    .filter(|p| p.tracks.iter().any(|t| t.id == *id))
                    .count()
                    <= 2
            })
        {
            return Some((
                "fragmented".into(),
                format!(
                    "Several shapes registered briefly over {secs} seconds. The camera could not follow any one of them consistently enough to establish a direction of travel."
                ),
            ));
        }
        None
    }
    pub fn prompt(&self, s: &SceneState) -> String {
        let now = s.ns;
        let mut samples = Vec::new();
        for (i, p) in self
            .samples
            .iter()
            .filter(|p| now.saturating_sub(p.ns) <= 30 * SECOND)
            .enumerate()
        {
            if i % 6 == 0 || Some(p.ns) == self.samples.back().map(|s| s.ns) {
                let tracks: Vec<_> = p
                    .tracks
                    .iter()
                    .map(|t| {
                        serde_json::json!([
                            t.id,
                            t.obs,
                            (t.x * 100.).round() as i32,
                            (t.z * 100.).round() as i32,
                            t.moving
                        ])
                    })
                    .collect();
                samples.push(serde_json::json!([(now - p.ns) / SECOND, p.status, tracks]));
            }
        }
        let mut events:Vec<_>=s.events.iter().rev().filter(|e| e.ns<=now && now-e.ns<=30*SECOND).take(10).map(|e| serde_json::json!({"id":e.id,"seconds_ago":(now-e.ns)/SECOND,"track":e.object,"event":e.kind,"direction":e.passage.as_ref().map(|p|p.movement())})).collect();
        events.reverse();
        format!(
            "{BRIEF}\nSamples oldest first: [seconds ago, status, tracks]. Track: [id, observation, horizontal percent from left, depth cm, moving]. {}. Recent tracking events: {}",
            serde_json::to_string(&samples).unwrap(),
            serde_json::to_string(&events).unwrap()
        )
    }
}
