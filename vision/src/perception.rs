//! Fixed-camera depth segmentation, tracking and bounded history.
use crate::{CaptureHealth, DepthFrame, IntrinsicsSource, geometry::Point3D};
use serde::Serialize;
use std::collections::VecDeque;
const STEP: usize = 4;
#[derive(Clone, Debug, Serialize)]
pub struct SpatialRegion {
    pub bounds: [usize; 4],
    pub center: [f64; 2],
    pub z: f64,
    pub centroid: Option<Point3D>,
    pub extent: Option<[f64; 3]>,
    pub samples: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct TrackedObject {
    pub id: u64,
    pub observation_id: String,
    pub region: SpatialRegion,
    pub velocity: Option<[f64; 3]>,
    pub speed: Option<f64>,
    pub radial_velocity: f64,
    pub direction: &'static str,
    pub moving: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct SceneEvent {
    pub id: String,
    pub ns: u64,
    pub kind: String,
    pub object: Option<u64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct SceneState {
    pub id: String,
    pub ns: u64,
    pub ticks: u32,
    pub optics: IntrinsicsSource,
    pub valid_fraction: f64,
    pub status: &'static str,
    pub health: CaptureHealth,
    pub objects: Vec<TrackedObject>,
    pub events: Vec<SceneEvent>,
}
struct Track {
    obj: TrackedObject,
    seen: u64,
    hits: u32,
    votes: i8,
    active: bool,
}
#[derive(Default)]
pub struct Engine {
    background: Vec<f64>,
    size: (usize, usize),
    tracks: Vec<Track>,
    next: u64,
    serial: u64,
    event_id: u64,
    history: VecDeque<SceneEvent>,
    last: Option<(u32, u64, CaptureHealth)>,
    warmup: u8,
    pub threshold_mm: u16,
}
impl Engine {
    pub fn recalibrate(&mut self) {
        self.background.clear();
        self.tracks.clear();
        self.warmup = 0;
    }
    fn event(&mut self, ns: u64, kind: &str, object: Option<u64>) {
        self.event_id += 1;
        if self.history.len() == 64 {
            self.history.pop_front();
        }
        self.history.push_back(SceneEvent {
            id: format!("e{}", self.event_id),
            ns,
            kind: kind.to_string(),
            object,
        });
    }
    pub fn observe(&mut self, f: &DepthFrame) -> SceneState {
        let i = &f.info;
        let (w, h) = (i.width, i.height);
        let (gw, gh) = (w.div_ceil(STEP), h.div_ceil(STEP));
        let ns = i.arrival_ns;
        self.serial += 1;
        if self.size != (w, h) {
            self.recalibrate();
            self.size = (w, h)
        }
        if self.background.is_empty() {
            self.background.resize(gw * gh, 0.);
        }
        let mut status = if i.metric { "tracking" } else { "nonmetric" };
        if let Some((ticks, prev, last)) = &self.last {
            let delta = i.ticks.wrapping_sub(*ticks);
            if ns.saturating_sub(*prev) > 250_000_000
                || delta == 0
                || delta > i32::MAX as u32
                || i.health.gaps > last.gaps
                || i.health.queue_drops > last.queue_drops
                || i.health.errors > last.errors
                || i.health.incomplete > last.incomplete
            {
                status = "capture gap";
                self.event(ns, status, None);
            }
        }
        self.last = Some((i.ticks, ns, i.health.clone()));
        let mut grid = vec![0u16; gw * gh];
        let mut valid = 0;
        for gy in 0..gh {
            for gx in 0..gw {
                let x = gx * STEP;
                let y = gy * STEP;
                let k = gy * gw + gx;
                let d = f.pixels[y * w + x];
                if d == 0 {
                    continue;
                }
                valid += 1;
                if !i.metric {
                    continue;
                }
                let mut neighbors = [d; 3];
                for (j, index) in [y * w + (x + 1).min(w - 1), (y + 1).min(h - 1) * w + x]
                    .into_iter()
                    .enumerate()
                {
                    let a = f.pixels[index];
                    if a > 0 && a.abs_diff(d) < 45 {
                        neighbors[j] = a
                    }
                }
                neighbors.sort_unstable();
                let d = neighbors[1];
                let bg = &mut self.background[k];
                if *bg == 0. || self.warmup < 5 {
                    *bg = if *bg == 0. {
                        d as f64
                    } else {
                        0.5 * (*bg + d as f64)
                    }
                } else if *bg - d as f64 > self.threshold_mm.max(40) as f64 {
                    grid[k] = d
                } else {
                    *bg += 0.02 * (d as f64 - *bg);
                }
            }
        }
        if self.warmup < 5 && valid > 0 && i.metric {
            self.warmup += 1;
            status = "learning background"
        }
        if valid == 0 {
            status = "no valid depth"
        }
        let mut regions = vec![];
        for start in 0..grid.len() {
            if grid[start] == 0 {
                continue;
            }
            let mut todo = vec![(start, grid[start])];
            grid[start] = 0;
            let mut count = 0;
            let mut sum = [0.; 3];
            let mut xyz = [0.; 3];
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            let mut bounds = [w, h, 0, 0];
            while let Some((k, d)) = todo.pop() {
                let (x, y) = (k % gw, k / gw);
                let u = x * STEP;
                let v = y * STEP;
                count += 1;
                let z = d as f64 / 1000.;
                sum[0] += u as f64;
                sum[1] += v as f64;
                sum[2] += z;
                bounds = [
                    bounds[0].min(u),
                    bounds[1].min(v),
                    bounds[2].max(u),
                    bounds[3].max(v),
                ];
                if let Some(p) = i.intrinsics.project(u as f64, v as f64, z) {
                    for (j, a) in [p.x, p.y, p.z].into_iter().enumerate() {
                        xyz[j] += a;
                        lo[j] = lo[j].min(a);
                        hi[j] = hi[j].max(a)
                    }
                }
                for n in [
                    (x > 0).then(|| k - 1),
                    (x + 1 < gw).then_some(k + 1),
                    (y > 0).then(|| k - gw),
                    (y + 1 < gh).then_some(k + gw),
                ]
                .into_iter()
                .flatten()
                {
                    let a = grid[n];
                    if a != 0 && a.abs_diff(d) < 250 {
                        todo.push((n, a));
                        grid[n] = 0
                    }
                }
            }
            if count >= 6 {
                let n = count as f64;
                regions.push(SpatialRegion {
                    bounds,
                    center: [sum[0] / n, sum[1] / n],
                    z: sum[2] / n,
                    centroid: lo[0].is_finite().then_some(Point3D {
                        x: xyz[0] / n,
                        y: xyz[1] / n,
                        z: xyz[2] / n,
                    }),
                    extent: lo[0]
                        .is_finite()
                        .then(|| std::array::from_fn(|j| hi[j] - lo[j])),
                    samples: count,
                });
            }
        }
        let mut events = vec![];
        self.tracks.retain(|t| {
            if ns.saturating_sub(t.seen) > 350_000_000 {
                if t.active {
                    events.push(("disappeared", t.obj.id))
                }
                false
            } else {
                true
            }
        });
        let mut matched = vec![false; self.tracks.len()];
        let mut visible = vec![];
        for region in regions {
            let choice = self
                .tracks
                .iter()
                .enumerate()
                .filter(|(k, _)| !matched[*k])
                .filter_map(|(k, t)| {
                    let r = &t.obj.region;
                    let s = (region.center[0] - r.center[0]).hypot(region.center[1] - r.center[1]);
                    let dz = (region.z - r.z).abs();
                    (s < 80. && dz < 0.65).then_some((k, s + 60. * dz))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let idx = if let Some((k, _)) = choice {
                k
            } else {
                self.next += 1;
                let obj = TrackedObject {
                    id: self.next,
                    observation_id: String::new(),
                    region: region.clone(),
                    velocity: None,
                    speed: None,
                    radial_velocity: 0.,
                    direction: "steady",
                    moving: false,
                };
                matched.push(false);
                self.tracks.push(Track {
                    obj,
                    seen: ns,
                    hits: 0,
                    votes: 0,
                    active: false,
                });
                self.tracks.len() - 1
            };
            matched[idx] = true;
            let t = &mut self.tracks[idx];
            let dt = ns.saturating_sub(t.seen) as f64 * 1e-9;
            if dt > 0. {
                let dz = (region.z - t.obj.region.z) / dt;
                t.obj.radial_velocity = dz;
                t.obj.velocity = region
                    .centroid
                    .zip(t.obj.region.centroid)
                    .map(|(a, b)| [(a.x - b.x) / dt, (a.y - b.y) / dt, dz]);
                t.obj.speed = t.obj.velocity.map(|v| v[0].hypot(v[1]).hypot(v[2]));
                let speed = t.obj.speed.unwrap_or(dz.abs());
                t.votes = if speed > 0.10 {
                    (t.votes + 1).min(2)
                } else if speed < 0.05 {
                    (t.votes - 1).max(-2)
                } else {
                    t.votes
                };
                if t.active && t.obj.moving != (t.votes > 0) && t.votes.abs() == 2 {
                    t.obj.moving = t.votes > 0;
                    events.push((if t.obj.moving { "moving" } else { "stationary" }, t.obj.id))
                }
                t.obj.direction = if dz < -0.08 {
                    "approaching"
                } else if dz > 0.08 {
                    "receding"
                } else {
                    "steady"
                };
            }
            t.seen = ns;
            t.hits += 1;
            t.obj.region = region;
            t.obj.observation_id = format!("f{}-o{}", self.serial, t.obj.id);
            if !t.active && t.hits >= 2 {
                t.active = true;
                events.push(("appeared", t.obj.id))
            }
            if t.active {
                visible.push(t.obj.clone())
            }
        }
        for (kind, id) in events {
            self.event(ns, kind, Some(id))
        }
        SceneState {
            id: format!("f{}", self.serial),
            ns,
            ticks: i.ticks,
            optics: i.intrinsics.source,
            valid_fraction: valid as f64 / grid.len() as f64,
            status,
            health: i.health.clone(),
            objects: visible,
            events: self.history.iter().cloned().collect(),
        }
    }
}
