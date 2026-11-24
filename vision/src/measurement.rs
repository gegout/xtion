//! Source-pixel measurements from unmodified depth samples.
use crate::{DepthFrame, IntrinsicsSource, geometry::Point3D, perception::NoiseModel};
#[derive(Clone, Copy, Debug)]
pub struct Measurement {
    pub point: Point3D,
    pub sigma_m: f64,
    pub approximate_xy: bool,
}
pub fn measure(
    frame: &DepthFrame,
    pixel: [usize; 2],
    temporal_mm: f64,
    noise: &NoiseModel,
) -> Result<Measurement, &'static str> {
    let cloud = frame
        .organised_cloud()
        .ok_or("Metric depth and known intrinsics required")?;
    let [x, y] = pixel;
    let centre = cloud
        .point(x, y)
        .ok_or("No reading at the selected source pixel")?;
    let d = centre.z * 1000.;
    let sigma = noise.sigma_mm(d);
    let mut depths = [0u16; 9];
    let mut n = 0;
    for v in y.saturating_sub(1)..=(y + 1).min(frame.info.height - 1) {
        for u in x.saturating_sub(1)..=(x + 1).min(frame.info.width - 1) {
            let z = frame.pixels[v * frame.info.width + u];
            if z > 0 && (f64::from(z) - d).abs() <= noise.k_filter * sigma {
                depths[n] = z;
                n += 1;
            }
        }
    }
    let mm = *depths[..n].select_nth_unstable(n / 2).1;
    let point = frame
        .info
        .intrinsics
        .project(x as f64, y as f64, f64::from(mm) / 1000.)
        .ok_or("Unable to project selected point")?;
    Ok(Measurement {
        point,
        sigma_m: sigma.hypot(temporal_mm) / 1000.,
        approximate_xy: frame.info.intrinsics.source != IntrinsicsSource::Calibrated,
    })
}
impl Measurement {
    pub fn separation(self, b: Self) -> ([f64; 3], f64, f64) {
        let a = self.point;
        let p = b.point;
        (
            [p.x - a.x, p.y - a.y, p.z - a.z],
            crate::reconstruction::distance(a, p),
            self.sigma_m.hypot(b.sigma_m),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{FrameSource, SyntheticSource};
    #[test]
    fn picks_use_original_depth_and_reject_holes() {
        let mut f = SyntheticSource::default().next_frame().unwrap().unwrap();
        f.pixels.fill(1000);
        let n = NoiseModel::default();
        let a = measure(&f, [10, 10], 2., &n).unwrap();
        let b = measure(&f, [20, 10], 3., &n).unwrap();
        let (delta, d, s) = a.separation(b);
        assert!((d - 10. / f.info.intrinsics.fx).abs() < 1e-12);
        assert_eq!(delta[2], 0.);
        assert!(a.approximate_xy && s > a.sigma_m);
        f.pixels[10 * f.info.width + 10] = 0;
        assert!(measure(&f, [10, 10], 0., &n).is_err());
    }
}
