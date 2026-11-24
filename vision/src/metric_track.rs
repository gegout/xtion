//! Constant-velocity filter in camera metres, updated only from observed regions.
//! Synthetic observations during reacquisition stay internal to the filter.
use crate::geometry::Point3D;
use std::collections::VecDeque;

#[derive(Default)]
pub(crate) struct MetricTrack {
    pos: [f64; 3],
    vel: [f64; 3],
    cov: [f64; 3], // Shared position variance, cross covariance, velocity variance.
    history: VecDeque<(u64, [f64; 3])>,
    cadence: f64,
}
fn norm(v: [f64; 3]) -> f64 {
    v[0].hypot(v[1]).hypot(v[2])
}
fn xyz(p: Point3D) -> [f64; 3] {
    [p.x, p.y, p.z]
}
impl MetricTrack {
    pub fn history(&self) -> Vec<(u64, [f64; 3])> {
        self.history.iter().copied().collect()
    }
    pub fn cost(&self, p: Point3D, ns: u64, sigma: f64, extent: f64) -> Option<f64> {
        let Some(&(last_ns, last)) = self.history.back() else {
            return Some(0.);
        };
        let dt = ns.saturating_sub(last_ns) as f64 * 1e-9;
        let point = xyz(p);
        let residual = norm(std::array::from_fn(|j| {
            point[j] - self.pos[j] - dt * self.vel[j]
        }));
        let variance = self.cov[0] + 2. * dt * self.cov[1] + dt * dt * self.cov[2] + sigma * sigma;
        let gate = 4. * variance.max(0.).sqrt() + extent + 3. * dt;
        if residual > gate {
            return None;
        }
        let direction = if self.history.len() >= 3 {
            let past = self.history[self.history.len() - 3].1;
            let prev: [f64; 3] = std::array::from_fn(|j| last[j] - past[j]);
            let next: [f64; 3] = std::array::from_fn(|j| point[j] - last[j]);
            if norm(prev) > 2. * sigma && norm(next) > 2. * sigma {
                1. - (prev.iter().zip(next).map(|(a, b)| a * b).sum::<f64>()
                    / (norm(prev) * norm(next)))
                .clamp(-1., 1.)
            } else {
                0.
            }
        } else {
            0.
        };
        Some(residual / gate.max(1e-6) + 0.25 * direction)
    }
    pub fn observe(&mut self, p: Point3D, ns: u64, sigma: f64) -> Option<[f64; 3]> {
        let measured = xyz(p);
        if let Some(&(last_ns, last)) = self.history.back() {
            let dt = ns.checked_sub(last_ns)? as f64 * 1e-9;
            if dt <= 0. {
                return None;
            }
            if self.history.len() == 1 {
                self.vel = std::array::from_fn(|j| (measured[j] - last[j]) / dt);
                self.cadence = dt;
            }
            let steps = (dt / self.cadence.max(1e-4)).round().clamp(1., 10.) as usize;
            let delta = dt / steps as f64;
            let r = sigma.max(0.002).powi(2);
            for i in 1..=steps {
                let [a, b, c] = self.cov;
                let pos = a + 2. * delta * b + delta * delta * c + 2.25 * delta.powi(4);
                let cross = b + delta * c + 4.5 * delta.powi(3);
                let velocity = c + 9. * delta * delta;
                let innovation = pos + r;
                for j in 0..3 {
                    let z = last[j] + (measured[j] - last[j]) * i as f64 / steps as f64;
                    self.pos[j] += delta * self.vel[j];
                    let e = z - self.pos[j];
                    self.pos[j] += pos / innovation * e;
                    self.vel[j] += cross / innovation * e;
                }
                self.cov = [
                    pos * r / innovation,
                    cross * r / innovation,
                    (velocity - cross * cross / innovation).max(0.),
                ];
            }
            if steps == 1 {
                self.cadence = 0.9 * self.cadence + 0.1 * dt;
            }
        } else {
            self.pos = measured;
            self.cov = [sigma * sigma, 0., 1.];
        }
        if self.history.len() == 32 {
            self.history.pop_front();
        }
        self.history.push_back((ns, measured));
        (self.history.len() > 1).then_some(self.vel)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metric_velocity_after_occlusion_uses_real_elapsed_time() {
        let mut track = MetricTrack::default();
        for i in (0..30u64).filter(|i| !(10..15).contains(i)) {
            let t = i as f64 * 0.04;
            let p = Point3D {
                x: 0.4 * t,
                y: -0.1 * t,
                z: 2. - 0.2 * t,
            };
            assert!(track.cost(p, i * 40_000_000, 0.01, 0.2).is_some());
            if let Some(v) = track.observe(p, i * 40_000_000, 0.01) {
                assert!(norm([v[0] - 0.4, v[1] + 0.1, v[2] + 0.2]) < 1e-7);
            }
        }
        assert_eq!(track.history().len(), 25);
    }
}
