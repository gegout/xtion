//! Latest-only reconstruction worker and offscreen surface rasterization.
use crate::{
    DepthFrame,
    perception::NoiseModel,
    point_view::{ColourBy, Orbit},
    reconstruction::{MeshSettings, SurfaceMesh},
};
use std::sync::{Arc, Condvar, Mutex};
type Job = (Arc<DepthFrame>, MeshSettings);
type Output = Arc<(Arc<DepthFrame>, SurfaceMesh)>;
#[derive(Default)]
struct State {
    job: Option<Job>,
    output: Option<Output>,
    stop: bool,
}
pub struct SurfaceWorker {
    state: Arc<(Mutex<State>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Default for SurfaceWorker {
    fn default() -> Self {
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let worker = Arc::clone(&state);
        let handle = std::thread::spawn(move || {
            let (lock, wake) = &*worker;
            let mut mesh = SurfaceMesh::default();
            let noise = NoiseModel::default();
            loop {
                let mut s = wake
                    .wait_while(lock.lock().unwrap(), |s| s.job.is_none() && !s.stop)
                    .unwrap();
                if s.stop {
                    return;
                }
                let (frame, settings) = s.job.take().unwrap();
                drop(s);
                mesh.update(&frame, settings, &noise);
                let old = lock.lock().unwrap().output.replace(Arc::new((frame, mesh)));
                mesh = old
                    .and_then(|o| Arc::try_unwrap(o).ok())
                    .map(|(_, m)| m)
                    .unwrap_or_default();
            }
        });
        Self {
            state,
            thread: Some(handle),
        }
    }
}
impl SurfaceWorker {
    pub fn submit(&self, frame: Arc<DepthFrame>, settings: MeshSettings) {
        let (lock, wake) = &*self.state;
        lock.lock().unwrap().job = Some((frame, settings));
        wake.notify_one();
    }
    pub fn latest(&self) -> Option<Output> {
        self.state.0.lock().unwrap().output.clone()
    }
}
impl Drop for SurfaceWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        lock.lock().unwrap().stop = true;
        wake.notify_one();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
#[derive(Clone, Copy)]
pub struct ScreenFace {
    pub coords: [[f64; 3]; 3],
    pub pixels: [[usize; 2]; 3],
    pub rgb: [u8; 3],
}
impl ScreenFace {
    fn weights(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        let [a, b, c] = self.coords;
        let den = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if den.abs() < 1e-8 {
            return None;
        }
        let u = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / den;
        let v = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / den;
        (u >= 0. && v >= 0. && u + v <= 1.).then_some([u, v, 1. - u - v])
    }
}
#[derive(Default)]
pub struct SurfaceBuffer {
    pub faces: Vec<ScreenFace>,
    orthographic: bool,
}
impl SurfaceBuffer {
    pub fn update(
        &mut self,
        mesh: &SurfaceMesh,
        camera: Orbit,
        size: [usize; 2],
        clip: [f64; 2],
        colour: ColourBy,
    ) {
        self.faces.clear();
        self.orthographic = camera.orthographic;
        for t in &mesh.triangles {
            let vertices = t.map(|k| mesh.vertices[k]);
            if vertices
                .iter()
                .any(|v| !(clip[0]..=clip[1]).contains(&v.point.z))
            {
                continue;
            }
            let [Some(a), Some(b), Some(c)] = vertices.map(|v| camera.project(v.point, size))
            else {
                continue;
            };
            let light = 0.35 + 0.65 * vertices[0].normal[2].abs().min(1.);
            self.faces.push(ScreenFace {
                coords: [a, b, c],
                pixels: vertices.map(|v| v.pixel),
                rgb: colour
                    .sample(vertices[0].point, clip[1])
                    .map(|v| (f64::from(v) * light) as u8),
            });
        }
        self.faces.sort_unstable_by(|a, b| {
            b.coords
                .iter()
                .map(|p| p[2])
                .sum::<f64>()
                .total_cmp(&a.coords.iter().map(|p| p[2]).sum())
        });
    }
    pub fn pick(&self, x: f64, y: f64) -> Option<[usize; 2]> {
        self.faces.iter().rev().find_map(|f| {
            let mut w = f.weights(x, y)?;
            if !self.orthographic {
                for (v, p) in w.iter_mut().zip(f.coords) {
                    *v /= p[2];
                }
            }
            let sum = w.iter().sum::<f64>();
            Some(std::array::from_fn(|j| {
                (w.iter()
                    .zip(f.pixels)
                    .map(|(v, p)| v * p[j] as f64)
                    .sum::<f64>()
                    / sum)
                    .round() as usize
            }))
        })
    }
    pub fn rasterize(&self, size: [usize; 2], wire: bool, rgb: &mut Vec<u8>) {
        rgb.resize(size[0] * size[1] * 3, 28);
        rgb.fill(28);
        for f in &self.faces {
            let bounds = |i: usize| {
                let v = f.coords.map(|p| p[i]);
                let lo = v
                    .into_iter()
                    .fold(f64::INFINITY, f64::min)
                    .floor()
                    .clamp(0., size[i] as f64) as usize;
                let hi = v
                    .into_iter()
                    .fold(f64::NEG_INFINITY, f64::max)
                    .ceil()
                    .clamp(0., size[i] as f64) as usize;
                lo..hi
            };
            for y in bounds(1) {
                for x in bounds(0) {
                    if let Some(w) = f.weights(x as f64 + 0.5, y as f64 + 0.5)
                        && (!wire || w.iter().any(|v| *v < 0.15))
                    {
                        let i = (y * size[0] + x) * 3;
                        rgb[i..i + 3].copy_from_slice(&f.rgb);
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_space_refuses_pick_and_visible_triangle_returns_its_source_pixel() {
        let b = SurfaceBuffer {
            faces: vec![ScreenFace {
                coords: [[0., 0., 2.], [10., 0., 2.], [0., 10., 2.]],
                pixels: [[20, 20], [30, 20], [20, 30]],
                rgb: [1, 2, 3],
            }],
            orthographic: false,
        };
        assert_eq!(b.pick(4., 3.), Some([24, 23]));
        assert_eq!(b.pick(9., 9.), None);
        let mut data = vec![];
        b.rasterize([10, 10], false, &mut data);
        assert_eq!(&data[102..105], &[1, 2, 3]);
    }
}
