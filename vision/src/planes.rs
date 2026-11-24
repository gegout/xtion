//! Connected planar cells of an organised depth image. All distances are metres.
use crate::{DepthFrame, IntrinsicsSource, geometry::OrganisedCloud, perception::NoiseModel};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Plane {
    /// Unit normal oriented so that dot(normal, point) = offset is positive.
    pub normal: [f64; 3],
    pub offset: f64,
    pub cells: usize,
    pub source: IntrinsicsSource,
}
impl Plane {
    pub fn floor_tilt_deg(self) -> f64 {
        self.normal[2].asin().to_degrees()
    }
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
const CELL: usize = 24;

fn fit(cloud: OrganisedCloud<'_>, x: usize, y: usize, noise: &NoiseModel) -> Option<Plane> {
    // Least-squares fit of inverse depth to image coordinates on a symmetric grid.
    // Missing samples invalidate this cell; no extrapolation across depth holes.
    let info = &cloud.frame().info;
    let k = info.intrinsics;
    let mut values = [0.; 9];
    let mut mean = 0.;
    let mut dx = 0.;
    let mut dy = 0.;
    for (i, v) in values.iter_mut().enumerate() {
        let u = x + 4 + (i % 3) * 8;
        let row = y + 4 + (i / 3) * 8;
        let p = cloud.point(u, row)?;
        if !(noise.min_range_mm as f64..=noise.max_range_mm as f64).contains(&(p.z * 1000.)) {
            return None;
        }
        *v = 1. / p.z;
        mean += *v / 9.;
        dx += *v * ((i % 3) as f64 - 1.) / 48.;
        dy += *v * ((i / 3) as f64 - 1.) / 48.;
    }
    let normal = [
        dx * k.fx,
        dy * k.fy,
        mean + dx * (k.cx - (x + 12) as f64) + dy * (k.cy - (y + 12) as f64),
    ];
    let magnitude = dot(normal, normal).sqrt();
    if magnitude < 1e-9 {
        return None;
    }
    for (i, &v) in values.iter().enumerate() {
        let predicted = mean + dx * 8. * ((i % 3) as f64 - 1.) + dy * 8. * ((i / 3) as f64 - 1.);
        if (predicted.recip() - v.recip()).abs() * 1000.
            > noise.k_filter * noise.sigma_mm(1000. / v)
        {
            return None;
        }
    }
    Some(Plane {
        normal: normal.map(|n| n / magnitude),
        offset: 1. / magnitude,
        cells: 1,
        source: k.source,
    })
}

/// Extract connected floor/wall candidates. A near-horizontal plane below the
/// camera can also be a tabletop; its mount estimate needs operator review.
pub fn structural_planes(frame: &DepthFrame, noise: &NoiseModel) -> Vec<Plane> {
    let Some(cloud) = frame.organised_cloud() else {
        return vec![];
    };
    let (w, h) = (frame.info.width / CELL, frame.info.height / CELL);
    let mut cells: Vec<_> = (0..w * h)
        .map(|i| fit(cloud, (i % w) * CELL, (i / w) * CELL, noise))
        .collect();
    let mut result = vec![];
    for i in 0..cells.len() {
        let Some(seed) = cells[i].take() else {
            continue;
        };
        let mut todo = vec![i];
        let mut normal = seed.normal;
        let mut offset = seed.offset;
        let mut count = 1;
        while let Some(j) = todo.pop() {
            for n in [
                (j % w > 0).then(|| j - 1),
                (j % w + 1 < w).then_some(j + 1),
                j.checked_sub(w),
                (j + w < cells.len()).then_some(j + w),
            ]
            .into_iter()
            .flatten()
            {
                let Some(other) = cells[n] else { continue };
                if dot(seed.normal, other.normal) > 0.98
                    && (seed.offset - other.offset).abs() * 1000.
                        <= noise.k_connect * noise.sigma_mm(1000. * seed.offset)
                {
                    cells[n] = None;
                    todo.push(n);
                    for (sum, v) in normal.iter_mut().zip(other.normal) {
                        *sum += v;
                    }
                    offset += other.offset;
                    count += 1;
                }
            }
        }
        if count >= 6 && count * 20 >= w * h {
            let length = dot(normal, normal).sqrt();
            result.push(Plane {
                normal: normal.map(|n| n / length),
                offset: offset / count as f64,
                cells: count,
                source: seed.source,
            });
        }
    }
    result.sort_by_key(|p| std::cmp::Reverse(p.cells));
    result
}
pub fn floor_candidate(planes: &[Plane]) -> Option<Plane> {
    support_candidate(planes, None)
}
pub fn support_candidate(planes: &[Plane], known_mount_m: Option<f64>) -> Option<Plane> {
    planes
        .iter()
        .copied()
        .filter(|p| p.normal[1] > 0.8)
        .filter(|p| known_mount_m.is_none_or(|h| (h - p.offset).abs() < 0.20))
        .max_by(|a, b| {
            known_mount_m
                .map_or_else(
                    || a.offset.total_cmp(&b.offset),
                    |h| (h - b.offset).abs().total_cmp(&(h - a.offset).abs()),
                )
                .then(a.cells.cmp(&b.cells))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CameraIntrinsics,
        source::{FrameSource, SyntheticSource},
    };
    #[test]
    fn tilted_floor_recovers_offset_and_orientation_but_vertical_wall_has_no_height() {
        let mut source = SyntheticSource::default();
        let mut frame = source.next_frame().unwrap().unwrap();
        frame.info.width = 320;
        frame.info.height = 240;
        let k = CameraIntrinsics::nominal(320, 240);
        frame.info.intrinsics = k;
        let tilt = 25f64.to_radians();
        frame.pixels = (0..320 * 240)
            .map(|i| {
                let denom = tilt.cos() * ((i / 320) as f64 - k.cy) / k.fy + tilt.sin();
                if denom > 0. {
                    (1000. / denom).round() as u16
                } else {
                    0
                }
            })
            .collect();
        let noise = NoiseModel::default();
        let floor = floor_candidate(&structural_planes(&frame, &noise)).unwrap();
        assert!((floor.offset - 1.).abs() < 0.01 && (floor.floor_tilt_deg() - 25.).abs() < 1.);
        frame.pixels.fill(1800);
        let walls = structural_planes(&frame, &noise);
        assert_eq!(walls.len(), 1);
        assert!(floor_candidate(&walls).is_none());
    }
}
