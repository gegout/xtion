//! Triangulation of visible depth samples in the camera optical frame (metres).
use crate::{DepthFrame, geometry::Point3D, perception::NoiseModel};
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct MeshSettings {
    pub stride: usize,
    pub erode: usize,
    pub k: f64,
    pub max_slope: f64,
    pub max_edge_m: f64,
}
impl Default for MeshSettings {
    fn default() -> Self {
        Self {
            stride: 4,
            erode: 1,
            k: 3.,
            max_slope: 2.,
            max_edge_m: 0.15,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Vertex {
    pub point: Point3D,
    pub pixel: [usize; 2],
    pub normal: [f64; 3],
}
#[derive(Default)]
pub struct SurfaceMesh {
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<[usize; 3]>,
    valid: Vec<bool>,
    indices: Vec<usize>,
}
pub(crate) fn distance(a: Point3D, b: Point3D) -> f64 {
    (a.x - b.x).hypot(a.y - b.y).hypot(a.z - b.z)
}
impl SurfaceMesh {
    /// Reuses scratch storage. Each decimated quad must have continuous native
    /// edges throughout its footprint, including samples between its vertices.
    pub fn update(&mut self, f: &DepthFrame, s: MeshSettings, noise: &NoiseModel) {
        self.vertices.clear();
        self.triangles.clear();
        let Some(cloud) = f.organised_cloud() else {
            return;
        };
        let (w, h) = (f.info.width, f.info.height);
        let stride = s.stride.max(1);
        if stride >= w || stride >= h {
            return;
        }
        let gw = w.div_ceil(stride);
        let gh = h.div_ceil(stride);
        self.indices.clear();
        self.indices.resize(gw * gh, usize::MAX);
        self.valid.clear();
        self.valid.resize(w * h, false);
        let focal = f.info.intrinsics.fx.min(f.info.intrinsics.fy);
        for v in 0..h - 1 {
            for u in 0..w - 1 {
                let i = v * w + u;
                let base = f.pixels[i];
                let adjacent = [f.pixels[i + 1], f.pixels[i + w], f.pixels[i + w + 1]];
                let low = *adjacent.iter().min().unwrap_or(&0);
                let low = low.min(base);
                let tolerance = s.k * noise.sigma_mm(f64::from(low))
                    + s.max_slope * f64::from(low) / focal * std::f64::consts::SQRT_2;
                self.valid[i] = low != 0
                    && adjacent
                        .into_iter()
                        .all(|a| f64::from(base.abs_diff(a)) <= tolerance);
            }
        }
        for gy in 0..gh - 1 {
            for gx in 0..gw - 1 {
                let (u, v) = (gx * stride, gy * stride);
                let right = (u + stride).saturating_add(s.erode).min(w - 1);
                let down = (v + stride).saturating_add(s.erode).min(h - 1);
                if !(v.saturating_sub(s.erode)..down).all(|y| {
                    self.valid[y * w + u.saturating_sub(s.erode)..y * w + right]
                        .iter()
                        .all(|&ok| ok)
                }) {
                    continue;
                }
                let coords = [
                    (u, v),
                    (u + stride, v),
                    (u, v + stride),
                    (u + stride, v + stride),
                ];
                let pts = coords.map(|(x, y)| cloud.point(x, y).expect("validated native depth"));
                if [(0, 1), (0, 2), (1, 2), (1, 3), (2, 3)]
                    .into_iter()
                    .any(|(a, b)| distance(pts[a], pts[b]) > s.max_edge_m)
                {
                    continue;
                }
                let ids = std::array::from_fn::<_, 4, _>(|i| {
                    let (x, y) = coords[i];
                    let key = y / stride * gw + x / stride;
                    if self.indices[key] == usize::MAX {
                        self.indices[key] = self.vertices.len();
                        self.vertices.push(Vertex {
                            point: pts[i],
                            pixel: [x, y],
                            normal: [0.; 3],
                        });
                    }
                    self.indices[key]
                });
                for tri in [[ids[0], ids[2], ids[1]], [ids[1], ids[2], ids[3]]] {
                    let [a, b, c] = tri.map(|idx| self.vertices[idx].point);
                    let e = [b.x - a.x, b.y - a.y, b.z - a.z];
                    let d = [c.x - a.x, c.y - a.y, c.z - a.z];
                    let n = [
                        e[1] * d[2] - e[2] * d[1],
                        e[2] * d[0] - e[0] * d[2],
                        e[0] * d[1] - e[1] * d[0],
                    ];
                    for idx in tri {
                        for (v, add) in self.vertices[idx].normal.iter_mut().zip(n) {
                            *v += add;
                        }
                    }
                    self.triangles.push(tri);
                }
            }
        }
        for v in &mut self.vertices {
            let [x, y, z] = v.normal;
            let len = x.hypot(y).hypot(z);
            if len > 0. {
                v.normal = v.normal.map(|c| c / len);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{FrameSource, SyntheticSource};
    #[test]
    fn decimated_plane_has_camera_facing_normals_and_never_bridges_steps_or_native_holes() {
        let mut f = SyntheticSource::default().next_frame().unwrap().unwrap();
        let (w, h) = (f.info.width, f.info.height);
        f.pixels.fill(1000);
        let mut mesh = SurfaceMesh::default();
        let settings = MeshSettings {
            stride: 4,
            erode: 0,
            max_edge_m: 1.,
            ..Default::default()
        };
        let noise = NoiseModel::default();
        mesh.update(&f, settings, &noise);
        let expected = 2 * ((w - 1) / 4) * ((h - 1) / 4);
        assert_eq!(mesh.triangles.len(), expected);
        assert!(mesh.vertices.iter().all(|v| v.normal[2] < -0.999));
        f.pixels[w + 1] = 0;
        mesh.update(&f, settings, &noise);
        assert_eq!(mesh.triangles.len(), expected - 2);
        for row in f.pixels.chunks_exact_mut(w) {
            row[10..].fill(2200);
        }
        mesh.update(&f, settings, &noise);
        assert!(mesh.triangles.iter().all(|t| {
            let z = t.map(|i| mesh.vertices[i].point.z);
            z[0] == z[1] && z[1] == z[2]
        }));
    }
}
