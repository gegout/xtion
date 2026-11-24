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
    Surface,
}
#[derive(PartialEq)]
struct RenderSettings {
    camera: Orbit,
    rect: egui::Rect,
    stride: usize,
    radius: i32,
    clip: [f64; 2],
    colour: ColourBy,
    surface: bool,
    wire: bool,
    dots: bool,
    cpu: bool,
}
pub struct CloudView {
    auto_density: bool,
    load_stride: usize,
    sample_since: Option<std::time::Instant>,
    sample_draws: u64,
    pub render_fps: f64,
    pub trails: bool,
    surface_worker: Option<xtion_vision::surface_view::SurfaceWorker>,
    surface_buffer: xtion_vision::surface_view::SurfaceBuffer,
    submitted: Option<(Arc<DepthFrame>, usize, usize)>,
    surface_in_split: bool,
    wire: bool,
    dots: bool,
    erosion: usize,
    pub picked: Option<Result<[usize; 2], &'static str>>,
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
            auto_density: true,
            load_stride: 0,
            sample_since: None,
            sample_draws: 0,
            render_fps: 0.,
            trails: false,
            surface_worker: None,
            surface_buffer: Default::default(),
            submitted: None,
            surface_in_split: false,
            wire: false,
            dots: false,
            erosion: 1,
            picked: None,
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
            ui.selectable_value(&mut self.mode, View::Surface, "Surface");
            ui.selectable_value(&mut self.mode, View::Split, "Split");
        });
        if self.mode == View::Depth {
            return;
        }
        if self.mode == View::Split {
            ui.checkbox(&mut self.surface_in_split, "Surface in split view");
        }
        if self.surface_active() {
            ui.checkbox(&mut self.wire, "Wireframe");
            ui.checkbox(&mut self.dots, "Show mesh vertices");
            ui.add(egui::Slider::new(&mut self.erosion, 0..=2).text("Border erosion (px)"));
        }
        ui.checkbox(&mut self.cpu, "CPU rasterizer");
        ui.checkbox(&mut self.camera.orthographic, "Orthographic");
        ui.checkbox(&mut self.auto_density, "Adaptive display density");
        if self.auto_density && self.load_stride > 0 {
            ui.small(format!(
                "Display stride raised by {} under load",
                self.load_stride
            ));
        }
        ui.checkbox(&mut self.trails, "3D trajectories");
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
    pub fn ready(&self) -> bool {
        !self.surface_active() || !self.surface_buffer.faces.is_empty()
    }
    fn surface_active(&self) -> bool {
        self.mode == View::Surface || self.mode == View::Split && self.surface_in_split
    }
    pub fn apply_floor(&mut self, plane: xtion_vision::planes::Plane) {
        self.mount = Some((
            (plane.offset * 100.).round() as i32,
            plane.floor_tilt_deg().round() as i32,
        ));
    }
    pub fn metrics(&self) -> serde_json::Value {
        let seconds = self
            .first_paint
            .zip(self.last_paint)
            .map_or(0., |(a, b)| b.duration_since(a).as_secs_f64());
        serde_json::json!({"renderer":if self.cpu{"cpu"}else{"glow"},"draws":self.paints,"draw_interval_s":seconds,"draws_per_second":if seconds>0.{self.paints.saturating_sub(1) as f64/seconds}else{0.},"density_stride_extra":self.load_stride})
    }
    pub fn pose(&self) -> xtion_vision::geometry::Transform {
        self.mount
            .and_then(|(h, t)| {
                xtion_vision::geometry::Pose::manual_mount(
                    f64::from(h) / 100.,
                    f64::from(t).to_radians(),
                )
            })
            .map_or(xtion_vision::geometry::Transform::IDENTITY, |p| p.transform)
    }
    pub fn settings(&self) -> xtion_vision::reconstruction::MeshSettings {
        xtion_vision::reconstruction::MeshSettings {
            stride: self.stride,
            erode: self.erosion,
            ..Default::default()
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &xtion_vision::pipeline::Snapshot,
        frozen: bool,
    ) {
        let input = &snapshot.frame;
        let effective_stride = self.stride
            + if self.auto_density {
                self.load_stride
            } else {
                0
            };
        let surface = self.surface_active();
        let output = if surface {
            let settings = xtion_vision::reconstruction::MeshSettings {
                stride: effective_stride,
                ..self.settings()
            };
            let worker = self.surface_worker.get_or_insert_with(Default::default);
            if self.submitted.as_ref().is_none_or(|(old, step, erode)| {
                !Arc::ptr_eq(input, old) || *step != settings.stride || *erode != settings.erode
            }) {
                worker.submit(input.clone(), settings);
                self.submitted = Some((input.clone(), settings.stride, settings.erode));
            }
            worker.latest()
        } else {
            None
        };
        if surface
            && output
                .as_ref()
                .is_none_or(|o| frozen && !Arc::ptr_eq(&o.0, input))
        {
            ui.label("Reconstructing visible surfaces…");
            return;
        }
        let frame = output.as_ref().map_or(input, |o| &o.0);
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
        self.sample_draws += 1;
        let start = *self.sample_since.get_or_insert(now);
        let elapsed = now.duration_since(start).as_secs_f64();
        if elapsed >= 2. {
            self.render_fps = self.sample_draws as f64 / elapsed;
            if self.auto_density && !frozen {
                if self.render_fps < 15. {
                    self.load_stride = (self.load_stride + 2).min(8);
                } else if self.render_fps > 26. {
                    self.load_stride = self.load_stride.saturating_sub(1);
                }
            }
            self.sample_since = Some(now);
            self.sample_draws = 0;
        }
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
            stride: effective_stride,
            radius: self.radius,
            clip: [self.near, self.far],
            colour: self.colour_by,
            cpu: self.cpu,
            surface,
            wire: self.wire,
            dots: self.dots,
        };
        let changed = !self
            .cached
            .as_ref()
            .is_some_and(|(old, key)| Arc::ptr_eq(old, frame) && *key == settings);
        if changed {
            self.cached = Some((Arc::clone(frame), settings));
            if let Some(output) = &output {
                self.surface_buffer.update(
                    &output.1,
                    self.camera,
                    size,
                    [self.near, self.far],
                    self.colour_by,
                );
            } else {
                self.points.update(
                    cloud,
                    self.camera,
                    size,
                    NonZeroUsize::new(effective_stride).unwrap(),
                    [self.near, self.far],
                    self.colour_by,
                );
            }
        }
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let pos = pos - rect.min;
            self.picked = Some(
                if surface {
                    self.surface_buffer.pick(pos.x.into(), pos.y.into())
                } else {
                    self.points.pick([pos.x as i32, pos.y as i32], self.radius)
                }
                .ok_or("No visible geometry under pointer"),
            );
        }
        let painter = ui.painter_at(rect);
        if self.cpu {
            if changed {
                if surface {
                    self.surface_buffer
                        .rasterize(size, self.wire, &mut self.rgb);
                    if self.dots {
                        for f in &self.surface_buffer.faces {
                            for [x, y, _] in f.coords {
                                if x >= 0. && y >= 0. && x < size[0] as f64 && y < size[1] as f64 {
                                    let i = (y as usize * size[0] + x as usize) * 3;
                                    self.rgb[i..i + 3].fill(235);
                                }
                            }
                        }
                    }
                } else {
                    self.points.rasterize(size, self.radius, &mut self.rgb);
                }
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
                if surface {
                    for f in &self.surface_buffer.faces {
                        let points = f
                            .coords
                            .map(|p| rect.min + Vec2::new(p[0] as f32, p[1] as f32));
                        let c = Color32::from_rgb(f.rgb[0], f.rgb[1], f.rgb[2]);
                        if !self.wire {
                            let base = mesh.vertices.len() as u32;
                            for pos in points {
                                mesh.colored_vertex(pos, c);
                            }
                            mesh.add_triangle(base, base + 1, base + 2);
                        }
                        if self.dots {
                            for pos in points {
                                mesh.add_colored_rect(
                                    egui::Rect::from_center_size(pos, Vec2::splat(1.5)),
                                    Color32::WHITE,
                                );
                            }
                        }
                    }
                } else {
                    for p in &self.points.points {
                        let pos = rect.min
                            + Vec2::new(
                                (p.center[0] - self.radius) as f32,
                                (p.center[1] - self.radius) as f32,
                            );
                        mesh.add_colored_rect(
                            egui::Rect::from_min_size(
                                pos,
                                Vec2::splat((2 * self.radius + 1) as f32),
                            ),
                            Color32::from_rgb(p.rgb[0], p.rgb[1], p.rgb[2]),
                        );
                    }
                }
            }
            painter.add(egui::Shape::mesh(Arc::clone(&self.mesh)));
            if surface && self.wire {
                for f in &self.surface_buffer.faces {
                    let points = f
                        .coords
                        .map(|p| rect.min + Vec2::new(p[0] as f32, p[1] as f32));
                    for j in 0..3 {
                        painter.line_segment(
                            [points[j], points[(j + 1) % 3]],
                            egui::Stroke::new(1., Color32::from_rgb(f.rgb[0], f.rgb[1], f.rgb[2])),
                        );
                    }
                }
            }
        }
        if self.trails && Arc::ptr_eq(frame, input) {
            let screen = |p: [f64; 3]| {
                self.camera
                    .project(
                        Point3D {
                            x: p[0],
                            y: p[1],
                            z: p[2],
                        },
                        size,
                    )
                    .map(|v| rect.min + Vec2::new(v[0] as f32, v[1] as f32))
            };
            for track in &snapshot.scene.objects {
                for pair in track.trajectory.windows(2) {
                    if let (Some(a), Some(b)) = (screen(pair[0].1), screen(pair[1].1)) {
                        painter.line_segment([a, b], egui::Stroke::new(1.5, Color32::GOLD));
                    }
                }
                if let Some(p) = track.region.centroid
                    && let Some(at) = screen([p.x, p.y, p.z])
                {
                    painter.text(
                        at,
                        egui::Align2::LEFT_BOTTOM,
                        format!("#{}", track.id),
                        egui::FontId::proportional(13.),
                        Color32::WHITE,
                    );
                }
            }
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
