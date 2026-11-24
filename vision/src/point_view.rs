//! Virtual camera and point rasterization. Input positions remain in the optical frame.
use crate::{
    geometry::{OrganisedCloud, Point3D},
    image::colour,
};
use std::num::NonZeroUsize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    pub pan: [f64; 2],
    pub orthographic: bool,
}
impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: 0.,
            pitch: 0.,
            distance: 2.,
            pan: [0.; 2],
            orthographic: false,
        }
    }
}
impl Orbit {
    /// Orbit about a point two metres along the physical sensor axis.
    pub fn project(self, p: Point3D, size: [usize; 2]) -> Option<[f64; 3]> {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let z = p.z - 2.;
        let x = cy * p.x + sy * z + self.pan[0];
        let rotated_z = cy * z - sy * p.x;
        let y = cp * p.y - sp * rotated_z + self.pan[1];
        let depth = sp * p.y + cp * rotated_z + self.distance;
        if depth <= 0.03 {
            return None;
        }
        let scale = size[0].min(size[1]) as f64
            / if self.orthographic {
                self.distance
            } else {
                depth
            };
        let view = [
            size[0] as f64 / 2. + x * scale,
            size[1] as f64 / 2. + y * scale,
            depth,
        ];
        view.iter().all(|v| v.is_finite()).then_some(view)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColourBy {
    #[default]
    Depth,
    SensorX,
    SensorY,
    Monochrome,
    Region,
    Motion,
    Quality,
    ManualHeight {
        height_cm: i32,
        tilt_deg: i32,
    },
}
impl ColourBy {
    pub(crate) fn sample(self, p: Point3D, far: f64) -> [u8; 3] {
        match self {
            Self::Depth => colour((p.z * 1000.).round() as u16, (far * 1000.) as u16),
            Self::SensorX | Self::SensorY => {
                let v = if self == Self::SensorX { p.x } else { p.y };
                let t = (v / far).clamp(-1., 1.);
                [
                    (230. - 195. * (-t).max(0.)) as u8,
                    (230. - 155. * t.abs()) as u8,
                    (230. - 195. * t.max(0.)) as u8,
                ]
            }
            Self::ManualHeight {
                height_cm,
                tilt_deg,
            } => {
                let angle = f64::from(tilt_deg).to_radians();
                let h = f64::from(height_cm) / 100. - angle.cos() * p.y - angle.sin() * p.z;
                colour((h.max(0.) * 1000.).round() as u16, (far * 1000.) as u16)
            }
            Self::Monochrome | Self::Region | Self::Motion | Self::Quality => [223; 3],
        }
    }
}
#[derive(Clone, Copy)]
pub struct Splat {
    pub center: [i32; 2],
    pub pixel: [usize; 2],
    pub depth: f64,
    pub rgb: [u8; 3],
}
#[derive(Default)]
pub struct PointBuffer {
    pub points: Vec<Splat>,
}
impl PointBuffer {
    pub fn update(
        &mut self,
        cloud: OrganisedCloud,
        camera: Orbit,
        size: [usize; 2],
        stride: NonZeroUsize,
        clip: [f64; 2],
        colour_by: ColourBy,
    ) {
        self.points.clear();
        for ((u, v), p) in cloud.samples(stride) {
            if !(clip[0]..=clip[1]).contains(&p.z) {
                continue;
            }
            let Some([x, y, depth]) = camera.project(p, size) else {
                continue;
            };
            if x < 0. || y < 0. || x >= size[0] as f64 || y >= size[1] as f64 {
                continue;
            }
            let rgb = colour_by.sample(p, clip[1]);
            self.points.push(Splat {
                center: [x as i32, y as i32],
                pixel: [u, v],
                depth,
                rgb,
            });
        }
        self.points
            .sort_unstable_by(|a, b| b.depth.total_cmp(&a.depth));
    }
    /// Same footprint and near-over-far ordering as the displayed points.
    pub fn pick(&self, at: [i32; 2], radius: i32) -> Option<[usize; 2]> {
        self.points
            .iter()
            .rev()
            .find(|p| {
                p.center[0].abs_diff(at[0]) <= radius as u32
                    && p.center[1].abs_diff(at[1]) <= radius as u32
            })
            .map(|p| p.pixel)
    }
    /// Painter order is shared with the GL mesh path so nearer points win.
    pub fn rasterize(&self, size: [usize; 2], radius: i32, rgb: &mut Vec<u8>) {
        let [w, h] = size;
        rgb.resize(w * h * 3, 0);
        rgb.fill(28);
        for p in &self.points {
            let [x, y] = p.center;
            for py in (y - radius).max(0)..=(y + radius).min(h as i32 - 1) {
                for px in (x - radius).max(0)..=(x + radius).min(w as i32 - 1) {
                    let offset = (py as usize * w + px as usize) * 3;
                    rgb[offset..offset + 3].copy_from_slice(&p.rgb);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn centre_perspective_orthographic_and_occlusion() {
        let mut camera = Orbit::default();
        let p = Point3D {
            x: 0.5,
            y: 0.,
            z: 4.,
        };
        assert_eq!(camera.project(p, [200, 100]), Some([112.5, 50., 4.]));
        camera.orthographic = true;
        assert_eq!(camera.project(p, [200, 100]), Some([125., 50., 4.]));
        let mut buf = PointBuffer::default();
        for (depth, rgb) in [(3., [0, 0, 255]), (2., [255, 0, 0])] {
            buf.points.push(Splat {
                center: [2, 2],
                pixel: [2, 2],
                depth,
                rgb,
            });
        }
        let mut image = vec![];
        buf.rasterize([5, 5], 1, &mut image);
        assert_eq!(&image[36..39], &[255, 0, 0]);
        assert_eq!(image[0], 28);
    }
}
