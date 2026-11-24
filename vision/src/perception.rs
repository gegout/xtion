//! Fixed-camera depth segmentation, tracking and bounded history.
use crate::{CaptureHealth, DepthFrame, IntrinsicsSource, geometry::Point3D};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
const STEP: usize = 4;
#[derive(Debug, Deserialize)]
pub struct NoiseModel {
    pub source: String,
    pub a_mm: f64,
    pub b_mm_per_m2: f64,
    pub min_range_mm: u16,
    pub max_range_mm: u16,
    pub k_connect: f64,
    pub k_retained: f64,
    pub k_filter: f64,
    pub k_unstable: f64,
    pub min_seed_fraction: f64,
    pub edge_probe_cells: usize,
}
impl Default for NoiseModel {
    fn default() -> Self {
        serde_json::from_str(include_str!("../config/spatial-noise.json"))
            .expect("valid built-in noise model")
    }
}
impl NoiseModel {
    pub fn sigma_mm(&self, depth_mm: f64) -> f64 {
        self.a_mm + self.b_mm_per_m2 * (depth_mm / 1000.).powi(2)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct SpatialRegion {
    pub bounds: [usize; 4],
    pub center: [f64; 2],
    pub z: f64,
    pub centroid: Option<Point3D>,
    pub extent: Option<[f64; 3]>,
    pub samples: usize,
    /// Fraction of visible samples with no range, edge or repeatability warning.
    pub quality: f64,
    pub median_z_m: f64,
    pub bounds_m: Option<[[f64; 3]; 2]>,
    pub clipped: bool,
    #[serde(skip)]
    support: std::sync::Arc<[usize]>,
}
impl SpatialRegion {
    pub fn contains_pixel(&self, [u, v]: [usize; 2], width: usize) -> bool {
        self.support
            .contains(&(v / STEP * width.div_ceil(STEP) + u / STEP))
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct TrackedObject {
    pub id: u64,
    pub observation_id: String,
    pub region: SpatialRegion,
    /// Euclidean distance to centroid when lateral geometry is available.
    pub distance_m: Option<f64>,
    pub velocity: Option<[f64; 3]>,
    pub speed: Option<f64>,
    pub radial_velocity: f64,
    pub direction: &'static str,
    pub moving: bool,
    pub filtered_velocity: Option<[f64; 3]>,
    pub trajectory: Vec<(u64, [f64; 3])>,
    pub visible_seconds: f64,
    pub stationary_seconds: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct SceneEvent {
    pub id: String,
    pub ns: u64,
    pub kind: String,
    pub object: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passage: Option<crate::trajectory::Passage>,
}
#[derive(Clone, Debug, Serialize)]
pub struct SceneState {
    pub id: String,
    pub ns: u64,
    pub ticks: u32,
    pub optics: IntrinsicsSource,
    pub floor_estimate: Option<crate::planes::Plane>,
    pub valid_fraction: f64,
    /// Median valid axial depth in left, centre and right image thirds, in metres.
    pub sector_z_m: [Option<f64>; 3],
    pub status: &'static str,
    pub health: CaptureHealth,
    pub objects: Vec<TrackedObject>,
    pub events: Vec<SceneEvent>,
}
struct Track {
    obj: TrackedObject,
    filter: crate::metric_track::MetricTrack,
    seen: u64,
    hits: u32,
    votes: i8,
    active: bool,
    path: crate::trajectory::Trajectory,
}
#[derive(Default)]
pub struct Engine {
    background: Vec<f64>,
    floor: Option<crate::planes::Plane>,
    size: (usize, usize),
    tracks: Vec<Track>,
    next: u64,
    serial: u64,
    event_id: u64,
    history: VecDeque<SceneEvent>,
    last: Option<(u32, u64, CaptureHealth)>,
    warmup: u8,
    obscured: bool,
    pub noise: NoiseModel,
    pub threshold_mm: u16,
    temporal: Vec<(f32, f32)>,
    /// Quality bits in source pixel order, including invalid readings.
    pub quality: Vec<u8>,
    protected: Vec<bool>,
    direct_track: Vec<bool>,
    previous_seeds: Vec<u16>,
    prior_ids: Vec<u64>,
}
/// Test whether visible row boundaries place a sparsely supported surface in
/// front of the surrounding scene. Missing or out-of-image depths cast no vote.
fn occluding_surface(
    frame: &DepthFrame,
    support: &[usize],
    bounds: [usize; 4],
    model: &NoiseModel,
) -> bool {
    let w = frame.info.width;
    let gw = w.div_ceil(STEP);
    let top = bounds[1] / STEP;
    let mut edges = vec![(usize::MAX, 0); bounds[3] / STEP - top + 1];
    for &cell in support {
        let x = (cell % gw) * STEP;
        let edge = &mut edges[cell / gw - top];
        edge.0 = edge.0.min(x);
        edge.1 = edge.1.max(x);
    }
    let mut votes = [(0, 0); 2];
    for (j, (left, right)) in edges.into_iter().enumerate() {
        if left == usize::MAX {
            continue;
        }
        let row = (top + j) * STEP * w;
        let offset = model.edge_probe_cells.saturating_mul(STEP);
        for (side, (edge, other)) in [
            (left, left.checked_sub(offset)),
            (right, right.checked_add(offset).filter(|x| *x < w)),
        ]
        .into_iter()
        .enumerate()
        {
            let Some(other) = other else { continue };
            let d = frame.pixels[row + edge];
            let surrounding = frame.pixels[row + other];
            if d == 0 || surrounding == 0 {
                continue;
            }
            let delta = f64::from(surrounding) - f64::from(d);
            let tolerance = model.k_connect * model.sigma_mm(d.min(surrounding).into());
            if delta > tolerance {
                votes[side].0 += 1
            } else if delta < -tolerance {
                votes[side].1 += 1
            }
        }
    }
    // A depth ramp can face nearer samples on one side and farther
    // samples on the other. Pooling both sides mistakes it for a silhouette.
    votes
        .iter()
        .any(|&(front, behind)| front >= 3 && front > behind)
        && votes
            .iter()
            .all(|&(front, behind)| behind < 3 || front >= behind)
}
impl Engine {
    pub fn recalibrate(&mut self) {
        self.background.clear();
        self.floor = None;
        self.temporal.clear();
        self.protected.clear();
        self.direct_track.clear();
        self.previous_seeds.clear();
        self.prior_ids.clear();
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
            passage: None,
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
            self.protected.resize(gw * gh, false);
            self.direct_track.resize(gw * gh, false);
            self.previous_seeds.resize(gw * gh, 0);
            self.prior_ids.resize(gw * gh, 0);
            self.temporal.resize(w * h, (0., 0.));
        }
        if self.serial % 30 == 1 {
            self.floor =
                crate::planes::floor_candidate(&crate::planes::structural_planes(f, &self.noise));
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
                self.previous_seeds.fill(0);
                self.direct_track.fill(false);
                status = "capture gap";
                self.event(ns, status, None);
            }
        }
        self.last = Some((i.ticks, ns, i.health.clone()));
        let mut grid = vec![0u16; gw * gh];
        self.quality.resize(w * h, 0);
        for (idx, (&d, flag)) in f.pixels.iter().zip(&mut self.quality).enumerate() {
            *flag = 0;
            if d == 0 || !i.metric {
                *flag = 1;
                self.temporal[idx] = (0., 0.);
                continue;
            }
            if !(self.noise.min_range_mm..=self.noise.max_range_mm).contains(&d) {
                *flag |= 2;
            }
            let sigma = self.noise.sigma_mm(d.into()) as f32;
            let (mean, variance) = &mut self.temporal[idx];
            if *mean == 0. {
                *mean = d as f32;
            }
            let delta = d as f32 - *mean;
            *mean += 0.125 * delta;
            *variance = 0.875 * (*variance + 0.125 * delta * delta);
            if *variance > (self.noise.k_unstable as f32 * sigma).powi(2) {
                *flag |= 8;
            }
            let right = (idx % w + 1 < w).then(|| f.pixels[idx + 1]);
            let below = f.pixels.get(idx + w).copied();
            if [right, below].into_iter().flatten().any(|other| {
                other == 0
                    || f64::from(other.abs_diff(d))
                        > self.noise.k_connect * self.noise.sigma_mm(d.into())
            }) {
                *flag |= 4;
            }
        }
        let mut surface = vec![0u16; gw * gh];
        let mut valid = 0;
        let mut sectors: [Vec<u16>; 3] =
            std::array::from_fn(|_| Vec::with_capacity(grid.len() / 3 + 1));
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
                    if a > 0
                        && f64::from(a.abs_diff(d))
                            <= self.noise.k_filter * self.noise.sigma_mm(d.into())
                    {
                        neighbors[j] = a
                    }
                }
                neighbors.sort_unstable();
                let d = neighbors[1];
                surface[k] = d;
                sectors[x * 3 / w].push(d);
                let bg = &mut self.background[k];
                if *bg == 0. || self.warmup < 5 {
                    *bg = if *bg == 0. {
                        d as f64
                    } else {
                        0.5 * (*bg + d as f64)
                    }
                } else if d <= self.noise.max_range_mm
                    && *bg - d as f64
                        > (f64::from(self.threshold_mm.max(40)) / self.noise.sigma_mm(1500.))
                            * self.noise.sigma_mm(*bg)
                {
                    grid[k] = d
                } else if !self.protected[k] {
                    *bg += 0.02 * (d as f64 - *bg);
                }
            }
        }
        let sector_z_m = sectors.each_mut().map(|values| {
            let middle = values.len() / 2;
            if values.is_empty() {
                None
            } else {
                Some(f64::from(*values.select_nth_unstable(middle).1) / 1000.)
            }
        });
        if self.warmup < 5 && valid > 0 && i.metric {
            self.warmup += 1;
            status = "learning background"
        }
        if valid == 0 {
            status = "no valid depth"
        }
        // Missing coverage carries no evidence of a departure. Resume tracking
        // after visibility returns without deriving velocities across the gap.
        let obscured = valid * 20 < grid.len() || !i.metric;
        if obscured || self.obscured {
            if !i.metric {
                status = "nonmetric"
            } else if obscured {
                status = "insufficient depth coverage"
            }
            if obscured != self.obscured {
                self.event(
                    ns,
                    if obscured {
                        "depth unavailable"
                    } else {
                        "depth restored"
                    },
                    None,
                );
            }
            for t in &mut self.tracks {
                t.seen = ns;
                t.votes = 0;
                t.path = Default::default();
                t.filter = Default::default();
            }
            self.obscured = obscured;
            if obscured {
                self.previous_seeds.fill(0);
                self.direct_track.fill(false);
                return SceneState {
                    id: format!("f{}", self.serial),
                    ns,
                    ticks: i.ticks,
                    optics: i.intrinsics.source,
                    floor_estimate: self.floor,
                    valid_fraction: valid as f64 / grid.len() as f64,
                    sector_z_m,
                    status,
                    health: i.health.clone(),
                    objects: vec![],
                    events: self.history.iter().cloned().collect(),
                };
            }
        }
        self.protected.fill(false);
        let mut regions = vec![];
        let previous_direct = std::mem::replace(&mut self.direct_track, vec![false; gw * gh]);
        let mut independent = vec![false; grid.len()];
        for seed_only in [false, true] {
            if seed_only {
                // A changed patch can meet an unchanged surface at the same depth.
                // If the grown surface was rejected, retain connected measured
                // changes. Never re-emit samples of an accepted surface.
                for (k, d) in surface.iter_mut().enumerate() {
                    let seed = grid[k];
                    let previous = self.previous_seeds[k];
                    let consistent = previous > 0
                        && seed > 0
                        && f64::from(seed.abs_diff(previous))
                            <= self.noise.k_filter * self.noise.sigma_mm(seed.into());
                    *d = if !self.protected[k] && (consistent || independent[k]) {
                        seed
                    } else {
                        0
                    };
                }
            }
            for start in 0..grid.len() {
                if grid[start] == 0 || surface[start] == 0 {
                    continue;
                }
                let mut todo = vec![(start, grid[start])];
                surface[start] = 0;
                let mut component = Vec::new();
                let mut seeds = 0;
                let mut clean = 0;
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
                    clean += usize::from(self.quality[v * w + u] == 0);
                    seeds += usize::from(grid[k] != 0);
                    component.push(k);
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
                    // A single missing native pixel can remove an entire sampled grid
                    // column. Reconnect only an already established surface. Every
                    // intervening valid measurement must agree with both endpoints.
                    for bridge in [
                        (x >= 2).then(|| (k - 2, 1)),
                        (x + 2 < gw).then(|| (k + 2, 1)),
                        (y >= 2).then(|| (k - 2 * gw, w)),
                        (y + 2 < gh).then(|| (k + 2 * gw, w)),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let (n, stride) = bridge;
                        let other = surface[n];
                        if other == 0
                            || self.prior_ids[k] == 0
                            || self.prior_ids[n] != self.prior_ids[k]
                        {
                            continue;
                        }
                        let base = (k.min(n) / gw * w + k.min(n) % gw) * STEP;
                        if f.pixels[base + STEP * stride] != 0 {
                            continue;
                        }
                        let tolerance =
                            self.noise.k_filter * self.noise.sigma_mm(d.min(other).into());
                        if f64::from(d.abs_diff(other)) > tolerance {
                            continue;
                        }
                        let continuous =
                            (1..2 * STEP)
                                .filter(|&offset| offset != STEP)
                                .all(|offset| {
                                    let value = f.pixels[base + stride * offset];
                                    value != 0
                                        && f64::from(value.abs_diff(d).max(value.abs_diff(other)))
                                            <= tolerance
                                });
                        if continuous {
                            todo.push((n, other));
                            surface[n] = 0;
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
                        let a = surface[n];
                        let same_tracked_surface = self.prior_ids[n] != 0
                            && self.prior_ids[n] == self.prior_ids[k]
                            && grid[k] != 0
                            && grid[n] != 0;
                        // The calibrated interval excludes near-field samples. Keep the
                        // wider continuity allowance for directly observed foreground
                        // there; the range-quality flag still applies to every sample.
                        let k_limit = if same_tracked_surface
                            || (d.max(a) < self.noise.min_range_mm && grid[k] > 0 && grid[n] > 0)
                        {
                            self.noise.k_retained
                        } else {
                            self.noise.k_connect
                        };
                        if a != 0
                            && f64::from(a.abs_diff(d))
                                <= k_limit * self.noise.sigma_mm(d.min(a).into())
                        {
                            todo.push((n, a));
                            surface[n] = 0
                        }
                    }
                }
                // When a large directly observed patch meets a still larger
                // unchanged surface, segment its measured foreground independently.
                if !seed_only
                    && seeds >= (gw * gh / 100).max(6)
                    && seeds * 2 < count
                    && component
                        .iter()
                        .filter(|&&k| grid[k] > 0 && previous_direct[k])
                        .count()
                        * 2
                        > seeds
                    && self.tracks.iter().any(|track| {
                        let last = &track.obj.region;
                        track.active
                            && ns.saturating_sub(track.seen) < 250_000_000
                            && count > last.samples * 2
                            && seeds.abs_diff(last.samples) < last.samples / 2
                            && component
                                .iter()
                                .filter(|&&k| grid[k] != 0 && self.prior_ids[k] == track.obj.id)
                                .count()
                                * 2
                                > seeds
                    })
                {
                    for &k in &component {
                        independent[k] = grid[k] > 0;
                    }
                    continue;
                }
                if seeds >= 6
                    && (count >= (gw * gh / 700).max(6) || seeds == count)
                    && seeds as f64 / count as f64 >= self.noise.min_seed_fraction
                    && (seeds * 2 >= count || occluding_surface(f, &component, bounds, &self.noise))
                {
                    for &k in &component {
                        self.protected[k] = true;
                        self.direct_track[k] = seeds * 2 >= count;
                    }
                    let n = count as f64;
                    let mut depths: Vec<_> = component
                        .iter()
                        .map(|&cell| f.pixels[(cell / gw) * STEP * w + cell % gw * STEP])
                        .collect();
                    let mid = depths.len() / 2;
                    let median_z_m = f64::from(*depths.select_nth_unstable(mid).1) / 1000.;
                    regions.push(SpatialRegion {
                        median_z_m,
                        bounds_m: lo[0].is_finite().then_some([lo, hi]),
                        clipped: bounds[0] == 0
                            || bounds[1] == 0
                            || bounds[2] / STEP + 1 == gw
                            || bounds[3] / STEP + 1 == gh,
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
                        quality: clean as f64 / n,
                        support: component.into(),
                    });
                }
            }
        }
        self.previous_seeds.copy_from_slice(&grid);
        let mut events = vec![];
        let mut passages = vec![];
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
        regions.sort_by_key(|r| std::cmp::Reverse(r.samples));
        self.prior_ids.fill(0);
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
                    // Preserve the established silhouette when a small nearby fragment
                    // has a closer image centroid than the main body.
                    let area_ratio = (region.samples as f64 / r.samples as f64).ln().abs();
                    let metric_cost = if let Some(p) = region.centroid {
                        t.filter.cost(
                            p,
                            ns,
                            self.noise.sigma_mm(p.z * 1000.) / 1000.,
                            region.extent.map_or(0., |v| v[0].hypot(v[1]).hypot(v[2])),
                        )?
                    } else {
                        0.
                    };
                    (s < 80. && dz < 0.65)
                        .then_some((k, s + 60. * dz + 32. * area_ratio + metric_cost))
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
                    distance_m: None,
                    velocity: None,
                    speed: None,
                    radial_velocity: 0.,
                    direction: "steady",
                    moving: false,
                    filtered_velocity: None,
                    trajectory: vec![],
                    visible_seconds: 0.,
                    stationary_seconds: 0.,
                };
                matched.push(false);
                self.tracks.push(Track {
                    obj,
                    filter: Default::default(),
                    seen: ns,
                    hits: 0,
                    votes: 0,
                    active: false,
                    path: Default::default(),
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
            if let Some(p) = region.centroid {
                t.obj.filtered_velocity =
                    t.filter
                        .observe(p, ns, self.noise.sigma_mm(p.z * 1000.) / 1000.);
                t.obj.trajectory = t.filter.history();
            }
            if dt <= 0.1 {
                t.obj.visible_seconds += dt;
                if t.obj.moving {
                    t.obj.stationary_seconds = 0.;
                } else {
                    t.obj.stationary_seconds += dt;
                }
            }
            t.seen = ns;
            t.hits += 1;
            t.obj.distance_m = region.centroid.map(|p| p.x.hypot(p.y).hypot(p.z));
            t.obj.region = region;
            t.obj.observation_id = format!("f{}-o{}", self.serial, t.obj.id);
            if !t.active && t.hits >= 2 {
                t.active = true;
                events.push(("appeared", t.obj.id))
            }
            if status == "tracking" {
                if let Some(p) = t.path.observe(&t.obj.observation_id, ns, &t.obj.region, w) {
                    passages.push((t.obj.id, p));
                }
            } else {
                t.path = Default::default();
            }
            if t.active {
                for &cell in t.obj.region.support.iter() {
                    self.prior_ids[cell] = t.obj.id;
                }
                visible.push(t.obj.clone())
            }
        }
        for (kind, id) in events {
            self.event(ns, kind, Some(id))
        }
        for (track, passage) in passages {
            self.event(ns, "lateral movement", Some(track));
            self.history.back_mut().unwrap().passage = Some(passage);
        }
        SceneState {
            id: format!("f{}", self.serial),
            ns,
            ticks: i.ticks,
            optics: i.intrinsics.source,
            floor_estimate: self.floor,
            valid_fraction: valid as f64 / grid.len() as f64,
            sector_z_m,
            status,
            health: i.health.clone(),
            objects: visible,
            events: self.history.iter().cloned().collect(),
        }
    }
}
