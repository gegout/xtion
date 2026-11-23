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
#[derive(Clone, Copy, Debug)]
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
    pub fn point_cloud(&self) -> Option<PointCloud> {
        let i = &self.info;
        if !i.metric || i.intrinsics.source == IntrinsicsSource::Unknown {
            return None;
        }
        let points = self
            .pixels
            .iter()
            .enumerate()
            .filter_map(|(n, &d)| {
                i.intrinsics
                    .project((n % i.width) as f64, (n / i.width) as f64, d as f64 / 1000.)
            })
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
