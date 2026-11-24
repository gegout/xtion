use eframe::egui::{self, Color32, Pos2, Vec2};
use std::{num::NonZeroUsize, sync::Arc};
use xtion_vision::{
    DepthFrame,
    geometry::Point3D,
    point_view::{ColourBy, Orbit, PointBuffer},
};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum View {
    #[default]
    Depth,
    Points,
    Split,
}
#[derive(PartialEq)]
struct RenderSettings {
    camera: Orbit,
    rect: egui::Rect,
    stride: usize,
    radius: i32,
    clip: [f64; 2],
    colour: ColourBy,
    cpu: bool,
}
pub struct CloudView {
    cached: Option<(Arc<DepthFrame>, RenderSettings)>,
    mesh: Arc<egui::Mesh>,
    pub mode: View,
    paints: u64,
    first_paint: Option<std::time::Instant>,
    last_paint: Option<std::time::Instant>,
    camera: Orbit,
    cpu: bool,
    stride: usize,
    radius: i32,
    far: f64,
    near: f64,
    grid: bool,
    colour_by: ColourBy,
    frustum: bool,
    mount: Option<(i32, i32)>,
    points: PointBuffer,
    rgb: Vec<u8>,
    texture: Option<egui::TextureHandle>,
}
impl CloudView {
    pub fn new(mode: View, cpu: bool) -> Self {
        Self {
            mode,
            cached: None,
            mesh: Arc::default(),
            paints: 0,
            first_paint: None,
            last_paint: None,
            cpu,
            camera: Default::default(),
            stride: 4,
            radius: 1,
            far: 4.,
            near: 0.3,
            grid: true,
            colour_by: Default::default(),
            frustum: false,
            mount: None,
            points: Default::default(),
            rgb: vec![],
            texture: None,
        }
    }
    pub fn controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, View::Depth, "Depth");
            ui.selectable_value(&mut self.mode, View::Points, "3D points");
            ui.selectable_value(&mut self.mode, View::Split, "Split");
        });
        if self.mode == View::Depth {
            return;
        }
        ui.checkbox(&mut self.cpu, "CPU rasterizer");
        ui.checkbox(&mut self.camera.orthographic, "Orthographic");
        ui.checkbox(&mut self.grid, "Metric grid and axes");
        ui.checkbox(&mut self.frustum, "Sensor frustum");
        ui.collapsing("Sensor mount", |ui| {
            let mut enabled = self.mount.is_some();
            if ui.checkbox(&mut enabled, "Use manual floor pose").changed() {
                self.mount = enabled.then_some((100, 0));
                if !enabled && matches!(self.colour_by, ColourBy::ManualHeight { .. }) {
                    self.colour_by = ColourBy::Depth;
                }
            }
            if let Some((height, tilt)) = &mut self.mount {
                ui.add(egui::Slider::new(height, 0..=350).text("Height (cm)"));
                ui.add(egui::Slider::new(tilt, -90..=90).text("Downward tilt (°)"));
                if matches!(self.colour_by, ColourBy::ManualHeight { .. }) {
                    self.colour_by = ColourBy::ManualHeight {
                        height_cm: *height,
                        tilt_deg: *tilt,
                    };
                }
            }
        });
        egui::ComboBox::from_label("Colour")
            .selected_text(format!("{:?}", self.colour_by))
            .show_ui(ui, |ui| {
                if let Some((height_cm, tilt_deg)) = self.mount {
                    ui.selectable_value(
                        &mut self.colour_by,
                        ColourBy::ManualHeight {
                            height_cm,
                            tilt_deg,
                        },
                        "Height (manual pose)",
                    );
                }
                for (value, label) in [
                    (ColourBy::Depth, "Depth"),
                    (ColourBy::SensorX, "Sensor X"),
                    (ColourBy::SensorY, "Sensor Y"),
                    (ColourBy::Monochrome, "Monochrome"),
                ] {
                    ui.selectable_value(&mut self.colour_by, value, label);
                }
            });
        ui.add(egui::Slider::new(&mut self.stride, 2..=12).text("Sample stride"));
        ui.add(egui::Slider::new(&mut self.radius, 0..=4).text("Point radius"));
        ui.add(egui::Slider::new(&mut self.near, 0.05..=self.far - 0.01).text("Near clip (m)"));
        ui.add(egui::Slider::new(&mut self.far, self.near + 0.01..=10.).text("Far clip (m)"));
        if ui.button("Reset camera").clicked() {
            self.camera = Orbit::default();
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, frame: &Arc<DepthFrame>) {
        ui.small(format!(
            "{:?} optics · {} · sensor-facing surfaces only",
            frame.info.intrinsics.source,
            if self.cpu {
                "CPU rasterizer"
            } else {
                "egui / glow"
            }
        ));
        ui.small("Drag to orbit · secondary drag to pan · scroll to zoom · grid 0.5 m");
        let Some(cloud) = frame.organised_cloud() else {
            ui.label("Metric depth and known optics required.");
            return;
        };
        let now = std::time::Instant::now();
        self.first_paint.get_or_insert(now);
        self.last_paint = Some(now);
        self.paints += 1;
        let extent = ui
            .available_size()
            .clamp(Vec2::splat(1.), Vec2::splat(2048.));
        let size = [(extent.x as usize).max(1), (extent.y as usize).max(1)];
        let (rect, response) = ui.allocate_exact_size(extent, egui::Sense::drag());
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            self.camera.yaw += f64::from(delta.x) * 0.008;
            self.camera.pitch = (self.camera.pitch + f64::from(delta.y) * 0.008).clamp(-1.55, 1.55);
        } else if response.dragged_by(egui::PointerButton::Secondary) {
            let d = response.drag_delta();
            let scale = self.camera.distance / size[0].min(size[1]) as f64;
            self.camera.pan[0] += f64::from(d.x) * scale;
            self.camera.pan[1] += f64::from(d.y) * scale;
        }
        if response.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta().y);
            self.camera.distance =
                (self.camera.distance * (-f64::from(delta) * 0.003).exp()).clamp(0.1, 15.);
        }
        let settings = RenderSettings {
            camera: self.camera,
            rect,
            stride: self.stride,
            radius: self.radius,
            clip: [self.near, self.far],
            colour: self.colour_by,
            cpu: self.cpu,
        };
        let changed = !self
            .cached
            .as_ref()
            .is_some_and(|(old, key)| Arc::ptr_eq(old, frame) && *key == settings);
        if changed {
            self.cached = Some((Arc::clone(frame), settings));
            self.points.update(
                cloud,
                self.camera,
                size,
                NonZeroUsize::new(self.stride).unwrap(),
                [self.near, self.far],
                self.colour_by,
            );
        }
        let painter = ui.painter_at(rect);
        if self.cpu {
            if changed {
                self.points.rasterize(size, self.radius, &mut self.rgb);
                let image = egui::ColorImage::from_rgb(size, &self.rgb);
                match &mut self.texture {
                    Some(tex) => {
                        tex.set(image, egui::TextureOptions::NEAREST);
                    }
                    None => {
                        self.texture = Some(ui.ctx().load_texture(
                            "point cloud",
                            image,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                }
            }
            let tex = self
                .texture
                .as_ref()
                .expect("CPU texture created on render settings change");
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                Color32::WHITE,
            );
        } else {
            painter.rect_filled(rect, 0., Color32::from_gray(28));
            if changed {
                let mesh = Arc::make_mut(&mut self.mesh);
                mesh.clear();
                for p in &self.points.points {
                    let pos = rect.min
                        + Vec2::new(
                            (p.center[0] - self.radius) as f32,
                            (p.center[1] - self.radius) as f32,
                        );
                    mesh.add_colored_rect(
                        egui::Rect::from_min_size(pos, Vec2::splat((2 * self.radius + 1) as f32)),
                        Color32::from_rgb(p.rgb[0], p.rgb[1], p.rgb[2]),
                    );
                }
            }
            painter.add(egui::Shape::mesh(Arc::clone(&self.mesh)));
        }
        if self.grid || self.frustum {
            let line = |a: [f64; 3], b: [f64; 3], c: Color32| {
                let points = [a, b].map(|[x, y, z]| {
                    self.camera
                        .project(Point3D { x, y, z }, size)
                        .map(|p| rect.min + Vec2::new(p[0] as f32, p[1] as f32))
                });
                if let [Some(a), Some(b)] = points {
                    painter.line_segment([a, b], egui::Stroke::new(1., c));
                }
            };
            if self.frustum {
                let i = &frame.info;
                let corners = [
                    (0, 0),
                    (i.width - 1, 0),
                    (i.width - 1, i.height - 1),
                    (0, i.height - 1),
                ]
                .map(|(u, v)| {
                    let p = i
                        .intrinsics
                        .project(u as f64, v as f64, 0.35)
                        .expect("validated camera optics");
                    [p.x, p.y, p.z]
                });
                for n in 0..4 {
                    line([0.; 3], corners[n], Color32::GOLD);
                    line(corners[n], corners[(n + 1) % 4], Color32::GOLD);
                }
            }
            if !self.grid {
                return;
            }
            for i in -4..=4 {
                let v = f64::from(i) * 0.5;
                let grey = Color32::from_gray(78);
                line([v, 0.5, 0.5], [v, 0.5, 4.5], grey);
                line([-2., 0.5, 2.5 + v], [2., 0.5, 2.5 + v], grey);
            }
            for (end, color) in [
                ([0.5, 0., 1.], Color32::RED),
                ([0., 0.5, 1.], Color32::GREEN),
                ([0., 0., 1.5], Color32::LIGHT_BLUE),
            ] {
                line([0., 0., 1.], end, color);
            }
        }
    }
}

impl Drop for CloudView {
    fn drop(&mut self) {
        if let Some((first, last)) = self.first_paint.zip(self.last_paint) {
            let seconds = last.duration_since(first).as_secs_f64();
            eprintln!(
                "{}",
                serde_json::json!({"type":"point_view_summary","draws":self.paints,"active_seconds":seconds,
                "draws_per_second":if seconds>0. {(self.paints-1) as f64/seconds}else{0.},
                "renderer":if self.cpu {"cpu"} else {"glow"}})
            );
        }
    }
}
