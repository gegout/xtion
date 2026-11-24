//! Camera optical coordinates in metres: X right, Y down, Z forward.
use crate::{CameraIntrinsics, DepthFrame, IntrinsicsSource};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl CameraIntrinsics {
    pub fn project(&self, u: f64, v: f64, z: f64) -> Option<Point3D> {
        if self.source == IntrinsicsSource::Unknown || !(self.fx > 0. && self.fy > 0. && z > 0.) {
            return None;
        }
        let p = Point3D {
            x: (u - self.cx) * z / self.fx,
            y: (v - self.cy) * z / self.fy,
            z,
        };
        [p.x, p.y, p.z].iter().all(|a| a.is_finite()).then_some(p)
    }
    pub fn image(&self, p: Point3D) -> Option<(f64, f64)> {
        self.project(self.cx, self.cy, p.z)?;
        let uv = (self.fx * p.x / p.z + self.cx, self.fy * p.y / p.z + self.cy);
        (uv.0.is_finite() && uv.1.is_finite()).then_some(uv)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Transform {
    pub rotation: [[f64; 3]; 3],
    pub translation: [f64; 3],
}
impl Transform {
    pub const IDENTITY: Self = Self {
        rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        translation: [0.; 3],
    };
}
impl Transform {
    /// Map a point from child coordinates into the parent frame.
    pub fn apply(&self, p: Point3D) -> Point3D {
        let xyz = self.rotation.map(|r| r[0] * p.x + r[1] * p.y + r[2] * p.z);
        Point3D {
            x: xyz[0] + self.translation[0],
            y: xyz[1] + self.translation[1],
            z: xyz[2] + self.translation[2],
        }
    }
    /// Invert a rigid transform whose rotation is orthonormal.
    pub fn inverse(&self) -> Self {
        let rotation: [[f64; 3]; 3] =
            std::array::from_fn(|r| std::array::from_fn(|c| self.rotation[c][r]));
        let [x, y, z] = self.translation;
        Self {
            rotation,
            translation: rotation.map(|r| -(r[0] * x + r[1] * y + r[2] * z)),
        }
    }
}

/// An immutable organised cloud in the camera optical frame. The backing raw
/// frame retains missing pixels, calibration provenance and both timestamps.
#[derive(Clone, Copy)]
pub struct OrganisedCloud<'a> {
    frame: &'a DepthFrame,
}
impl<'a> OrganisedCloud<'a> {
    pub fn frame(self) -> &'a DepthFrame {
        self.frame
    }
    pub fn point(self, u: usize, v: usize) -> Option<Point3D> {
        let i = &self.frame.info;
        if u >= i.width || v >= i.height {
            return None;
        }
        let mm = self.frame.pixels[v * i.width + u];
        i.intrinsics
            .project(u as f64, v as f64, f64::from(mm) / 1000.)
    }
    pub fn samples(
        self,
        stride: std::num::NonZeroUsize,
    ) -> impl Iterator<Item = ((usize, usize), Point3D)> + 'a {
        let step = stride.get();
        (0..self.frame.info.height)
            .step_by(step)
            .flat_map(move |v| {
                (0..self.frame.info.width)
                    .step_by(step)
                    .filter_map(move |u| self.point(u, v).map(|p| ((u, v), p)))
            })
    }
}
pub struct Pose {
    pub parent: &'static str,
    pub child: &'static str,
    pub transform: Transform,
}
pub struct PointCloud {
    pub pose: Pose,
    pub arrival_ns: u64,
    pub source: IntrinsicsSource,
    pub points: Vec<Point3D>,
}
impl DepthFrame {
    pub fn organised_cloud(&self) -> Option<OrganisedCloud<'_>> {
        let i = &self.info;
        (i.metric
            && i.width > 0
            && i.height > 0
            && i.width.checked_mul(i.height) == Some(self.pixels.len())
            && i.intrinsics.project(0., 0., 1.).is_some())
        .then_some(OrganisedCloud { frame: self })
    }

    pub fn point_cloud(&self) -> Option<PointCloud> {
        let i = &self.info;
        let points = self
            .organised_cloud()?
            .samples(std::num::NonZeroUsize::MIN)
            .map(|(_, p)| p)
            .collect();
        Some(PointCloud {
            pose: Pose {
                parent: "camera",
                child: "camera",
                transform: Transform::IDENTITY,
            },
            arrival_ns: i.arrival_ns,
            source: i.intrinsics.source,
            points,
        })
    }
}
pub trait CloudExport {
    fn export(&self, cloud: &PointCloud, out: &mut dyn Write) -> io::Result<()>;
}
pub struct Ply;
impl CloudExport for Ply {
    fn export(&self, c: &PointCloud, out: &mut dyn Write) -> io::Result<()> {
        writeln!(
            out,
            "ply\nformat ascii 1.0\ncomment camera optical metres {:?} arrival_ns {}\nelement vertex {}\nproperty double x\nproperty double y\nproperty double z\nend_header",
            c.source,
            c.arrival_ns,
            c.points.len()
        )?;
        for p in &c.points {
            writeln!(out, "{} {} {}", p.x, p.y, p.z)?
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CaptureHealth, FrameInfo};
    #[test]
    fn plane_roundtrip() {
        let k = CameraIntrinsics::nominal(3, 3);
        for v in 0..3 {
            for u in 0..3 {
                let p = k.project(u as f64, v as f64, 2.).unwrap();
                assert_eq!(k.image(p), Some((u as f64, v as f64)));
                assert_eq!(p.z, 2.)
            }
        }
        assert!(k.project(1., 1., 0.).is_none());
        assert!(
            CameraIntrinsics {
                source: IntrinsicsSource::Unknown,
                ..k
            }
            .project(1., 1., 2.)
            .is_none()
        );
    }
    #[test]
    fn export_plane() {
        let f = DepthFrame {
            info: FrameInfo {
                width: 2,
                height: 2,
                metric: true,
                ticks: 1,
                arrival_ns: 9,
                intrinsics: CameraIntrinsics::nominal(2, 2),
                health: CaptureHealth::default(),
            },
            pixels: vec![1000, 0, 1000, 1000],
        };
        let c = f.point_cloud().unwrap();
        let mut out = vec![];
        Ply.export(&c, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let (h, body) = text.split_once("end_header\n").unwrap();
        assert!(h.contains("element vertex 3"));
        for l in body.lines() {
            let xyz: Vec<f64> = l.split_whitespace().map(|v| v.parse().unwrap()).collect();
            assert_eq!(xyz.len(), 3);
            assert_eq!(xyz[2], 1.)
        }
        assert_eq!(body.lines().count(), 3);
    }
}

#[cfg(test)]
mod spatial_tests {
    use super::*;
    #[test]
    fn rigid_transform_round_trip() {
        let t = Transform {
            rotation: [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]],
            translation: [1., 2., 3.],
        };
        let p = Point3D {
            x: 2.,
            y: 4.,
            z: 1.,
        };
        assert_eq!(t.inverse().apply(t.apply(p)), p);
    }

    #[test]
    fn synthetic_box_and_sphere_roundtrip_and_invalid_pixel() {
        use crate::source::{FrameSource, SyntheticSource};
        let mut f = SyntheticSource::default().next_frame().unwrap().unwrap();
        let i = &f.info;
        let (w, h) = (i.width, i.height);
        for y in 0..h {
            for x in 0..w {
                let ray = i.intrinsics.project(x as f64, y as f64, 1.).unwrap();
                let norm = ray.x * ray.x + ray.y * ray.y + 1.;
                // Front intersection with a 0.5 m radius sphere centred at (0,0,2).
                let disc = 4. - norm * 3.75;
                let z = if disc >= 0. {
                    (2. - disc.sqrt()) / norm
                } else {
                    3.
                };
                f.pixels[y * w + x] = (1000. * z).round() as u16;
            }
        }
        f.pixels[0] = 0;
        let cloud = f.organised_cloud().unwrap();
        assert!(cloud.point(0, 0).is_none() && cloud.point(w, 0).is_none());
        for ((u, v), p) in cloud.samples(std::num::NonZeroUsize::MIN) {
            let (x, y) = i.intrinsics.image(p).unwrap();
            assert!((x - u as f64).abs() < 1e-10 && (y - v as f64).abs() < 1e-10);
            if p.z < 3. {
                assert!((p.x.hypot(p.y).hypot(p.z - 2.) - 0.5).abs() < 0.0006);
            }
        }
        // Four pixel-grid corners of a rectangular face at 3 metres.
        let a = cloud.point(1, 1).unwrap();
        let b = cloud.point(w - 2, h - 2).unwrap();
        assert!((b.x - a.x - 3. * (w - 3) as f64 / i.intrinsics.fx).abs() < 1e-10);
        assert!((b.y - a.y - 3. * (h - 3) as f64 / i.intrinsics.fy).abs() < 1e-10);
    }
}
