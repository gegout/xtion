//! Short, continuous centroid paths in normalized camera-image coordinates.
use crate::perception::SpatialRegion;
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Clone, Debug, Serialize)]
pub struct Passage {
    pub start_id: String,
    pub end_id: String,
    pub start_ns: u64,
    pub end_ns: u64,
    pub start_x: f64,
    pub end_x: f64,
    pub samples: usize,
    pub upright_extent_m: Option<[f64; 2]>,
}
impl Passage {
    pub fn movement(&self) -> &'static str {
        match (self.start_x, self.end_x) {
            (a, b) if a < 1. / 3. && b >= 2. / 3. => "moved from left to right",
            (a, b) if a >= 2. / 3. && b < 1. / 3. => "moved from right to left",
            (a, b) if a < b => "moved rightward",
            _ => "moved leftward",
        }
    }
}
/// A broad size/shape gate for allowing a person hypothesis. Furniture can fit it too.
pub(crate) fn upright_extent(region: &SpatialRegion) -> Option<[f64; 2]> {
    let [w, h, _] = region.extent?;
    (region.samples >= 30
        && (0.25..=1.05).contains(&w)
        && (0.85..=2.25).contains(&h)
        && h >= 1.55 * w)
        .then_some([w, h])
}
struct Sample {
    id: String,
    ns: u64,
    x: f64,
    extent: Option<[f64; 2]>,
}
#[derive(Default)]
pub(crate) struct Trajectory {
    points: VecDeque<Sample>,
    last: Option<&'static str>,
}
impl Trajectory {
    pub fn observe(
        &mut self,
        id: &str,
        ns: u64,
        region: &SpatialRegion,
        width: usize,
    ) -> Option<Passage> {
        if self
            .points
            .back()
            .is_some_and(|p| ns <= p.ns || ns - p.ns > 350_000_000)
        {
            *self = Self::default();
        }
        while self
            .points
            .front()
            .is_some_and(|p| ns - p.ns > 5_000_000_000)
            || self.points.len() >= 160
        {
            self.points.pop_front();
        }
        let x = region.center[0] / width as f64;
        let extent = upright_extent(region);
        self.points.push_back(Sample {
            id: id.into(),
            ns,
            x,
            extent,
        });
        let mut previous = x;
        let mut path = 0.;
        let mut candidate = None;
        let mut body_samples = 0;
        for (k, p) in self.points.iter().rev().enumerate() {
            path += (previous - p.x).abs();
            previous = p.x;
            body_samples += usize::from(p.extent.is_some());
            let dx = (x - p.x).abs();
            if k >= 3 && ns - p.ns >= 300_000_000 && dx >= 0.22 && dx >= 0.8 * path {
                candidate = Some((p, k + 1, body_samples));
            }
        }
        let (start, count, bodies) = candidate?;
        let passage = Passage {
            start_id: start.id.clone(),
            end_id: id.into(),
            start_ns: start.ns,
            end_ns: ns,
            start_x: start.x,
            end_x: x,
            samples: count,
            upright_extent_m: extent.filter(|_| bodies * 4 >= count * 3),
        };
        let movement = passage.movement();
        if self.last == Some(movement)
            || matches!(
                (self.last, movement),
                (Some("moved from left to right"), "moved rightward")
                    | (Some("moved from right to left"), "moved leftward")
            )
        {
            return None;
        }
        self.last = Some(movement);
        Some(passage)
    }
}
