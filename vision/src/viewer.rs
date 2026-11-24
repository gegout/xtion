use eframe::egui::{self, Color32, Stroke, Vec2};
use std::{
    collections::VecDeque,
    sync::atomic::Ordering::Relaxed,
    time::{Duration, Instant},
};
use xtion_vision::{
    ai::{Config, Interpreter, Report, Transcript},
    image,
    pipeline::{Input, Pipeline},
    source::Result,
};
const AUBERGINE: Color32 = Color32::from_rgb(0x77, 0x29, 0x53);
const APP_MARK: &[u8] = include_bytes!("../assets/xtion-mark.png");
const RESPONSE_MARK: &[u8] = include_bytes!("../assets/response-mark.png");

fn load_mark(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let icon = eframe::icon_data::from_png_bytes(bytes).expect("bundled logo must decode");
    let pixels = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    ctx.load_texture(name, pixels, egui::TextureOptions::LINEAR)
}
const WARM_PAPER: Color32 = Color32::from_rgb(0xf6, 0xf6, 0xf5);
fn install_theme(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for path in [
        "/usr/share/fonts/truetype/ubuntu/UbuntuSans[wdth,wght].ttf",
        "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("Ubuntu".into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "Ubuntu".into());
            ctx.set_fonts(fonts);
            break;
        }
    }
    ctx.set_theme(egui::Theme::Light);
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = Color32::WHITE;
    visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(0x33);
    visuals.selection.bg_fill = AUBERGINE;
    visuals.selection.stroke.color = Color32::WHITE;
    visuals.hyperlink_color = AUBERGINE;
    visuals.widgets.active.bg_stroke.color = AUBERGINE;
    visuals.widgets.hovered.bg_stroke.color = AUBERGINE;
    ctx.set_visuals(visuals);
}
#[derive(Clone, Copy)]
enum ActionIcon {
    Play,
    Stop,
    Refresh,
}
fn action_button(
    ui: &mut egui::Ui,
    label: &str,
    icon: ActionIcon,
    enabled: bool,
) -> egui::Response {
    let response = ui.add_enabled(enabled, egui::Button::new(format!("  {label}")));
    let c = egui::pos2(response.rect.left() + 10., response.rect.center().y);
    let colour = if enabled { AUBERGINE } else { Color32::GRAY };
    let stroke = Stroke::new(1.4, colour);
    match icon {
        ActionIcon::Play => {
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    c + Vec2::new(-3., -4.),
                    c + Vec2::new(4., 0.),
                    c + Vec2::new(-3., 4.),
                ],
                colour,
                Stroke::NONE,
            ));
        }
        ActionIcon::Stop => {
            ui.painter()
                .rect_filled(egui::Rect::from_center_size(c, Vec2::splat(7.)), 1., colour);
        }
        ActionIcon::Refresh => {
            let arc: Vec<_> = (0..=16)
                .map(|i| c + Vec2::angled(0.4 + i as f32 * 0.31) * 4.)
                .collect();
            let end = *arc.last().unwrap();
            ui.painter().add(egui::Shape::line(arc, stroke));
            ui.painter()
                .line_segment([end, end + Vec2::new(-3., 0.)], stroke);
            ui.painter()
                .line_segment([end, end + Vec2::new(0., -3.)], stroke);
        }
    }
    response
}
pub struct Options {
    pub initial_view: crate::cloud_view::View,
    pub cpu: bool,
    pub benchmark: Option<String>,
    pub input: Input,
    pub capture_fps: u16,
    pub ai: Option<Config>,
    pub threshold: u16,
    pub record: Option<String>,
    pub screenshot: Option<String>,
    pub seconds: Option<f64>,
}
pub fn launch(opts: Options) -> Result<()> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200., 760.])
            .with_icon(eframe::icon_data::from_png_bytes(APP_MARK)?),
        ..Default::default()
    };
    eframe::run_native(
        "Xtion Vision",
        options,
        Box::new(move |cc| {
            // Use the normal window-close path so Pipeline::drop joins the sensor
            // and recorder threads before the process exits on SIGINT or SIGTERM.
            let ctx = cc.egui_ctx.clone();
            ctrlc::set_handler(move || {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                ctx.request_repaint();
            })?;
            install_theme(&cc.egui_ctx);
            Ok(Box::new(Viewer::new(opts, &cc.egui_ctx)))
        }),
    )
    .map_err(|e| e.to_string().into())
}
struct Viewer {
    sidecar: Option<xtion_vision::sidecar::Writer>,
    sidecar_path: Option<String>,
    labels: xtion_vision::semantic::HypothesisStore,
    question: String,
    answer: String,
    selected_entities: Vec<u64>,
    frozen: Option<xtion_vision::pipeline::Snapshot>,
    picks: Vec<([usize; 2], std::collections::VecDeque<u16>)>,
    pick_stamp: Option<u64>,
    units: &'static str,
    filtered: bool,
    overlays: bool,
    export_dir: String,
    export_job: Option<std::thread::JoinHandle<std::result::Result<(), String>>>,
    benchmark: Option<String>,
    cloud: crate::cloud_view::CloudView,
    capture_fps: u16,
    input: Input,
    pipe: Pipeline,
    endpoint: String,
    api_key: String,
    model: String,
    ai: Option<Interpreter>,
    config: Config,
    threshold: u16,
    far: u16,
    record_path: String,
    replay_path: String,
    error: String,
    app_mark: egui::TextureHandle,
    response_mark: egui::TextureHandle,
    texture: Option<egui::TextureHandle>,
    texture_key: (u64, u16),
    shot: Option<String>,
    start: Instant,
    seconds: Option<f64>,
    responses: VecDeque<Report>,
    chat: Transcript,
    show_chat: bool,
    chat_view: crate::chat_view::ChatView,
}
fn api_key_field(ui: &mut egui::Ui, key: &mut String) -> egui::Response {
    let key_field = egui::TextEdit::singleline(&mut *key)
        .password(true)
        .hint_text("Optional for local endpoints")
        .show(ui);
    // egui suppresses copying from password fields. Export only the
    // user's selected characters on an explicit clipboard command.
    if key_field.response.has_focus()
        && ui.input(|i| i.events.contains(&egui::Event::Copy))
        && let Some(range) = key_field.cursor_range.filter(|r| !r.is_empty())
    {
        ui.ctx().copy_text(range.slice_str(key).to_owned());
    }
    key_field.response.response
}
fn response_history(
    ui: &mut egui::Ui,
    responses: &VecDeque<Report>,
) -> egui::scroll_area::ScrollAreaOutput<()> {
    egui::ScrollArea::vertical()
        .id_salt("response_history")
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for r in responses {
                ui.small(format!(
                    "{:.1} s   ·   {}   ·   {}",
                    r.arrival_ns as f64 / 1e9,
                    r.model,
                    r.scene_id
                ));
                ui.add(
                    egui::Label::new(egui::RichText::new(&r.text).size(14.))
                        .selectable(true)
                        .wrap(),
                );
                ui.add_space(3.);
                ui.separator();
            }
        })
}
impl Viewer {
    fn new(o: Options, ctx: &egui::Context) -> Self {
        let mut cfg = o.ai.clone().unwrap_or_else(|| Config::new(String::new()));
        let chat = Transcript::default();
        cfg.transcript = Some(chat.clone());
        let ai = o.ai.map(|_| Interpreter::start(cfg.clone())).transpose();
        let error = ai
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        Self {
            benchmark: o.benchmark,
            sidecar: None,
            sidecar_path: None,
            frozen: None,
            labels: Default::default(),
            question: String::new(),
            answer: String::new(),
            selected_entities: vec![],
            picks: vec![],
            pick_stamp: None,
            units: "m",
            filtered: false,
            overlays: true,
            export_dir: "xtion-snapshot".into(),
            export_job: None,
            cloud: crate::cloud_view::CloudView::new(o.initial_view, o.cpu),
            capture_fps: o.capture_fps,
            pipe: Pipeline::with_capture_rate(
                o.input.clone(),
                o.threshold,
                o.record.clone(),
                o.seconds.map(Duration::from_secs_f64),
                o.capture_fps,
            ),
            input: o.input,
            endpoint: cfg.url.clone(),
            api_key: cfg.api_key.clone().unwrap_or_default(),
            model: cfg.model.clone().unwrap_or_default(),
            ai: ai.ok().flatten(),
            config: cfg,
            threshold: o.threshold,
            far: 4000,
            record_path: o.record.unwrap_or_else(|| "session.xtv".into()),
            replay_path: String::new(),
            error,
            app_mark: load_mark(ctx, "Xtion mark", APP_MARK),
            response_mark: load_mark(ctx, "Response mark", RESPONSE_MARK),
            texture: None,
            texture_key: (0, 0),
            shot: o.screenshot,
            start: Instant::now(),
            seconds: o.seconds,
            responses: VecDeque::new(),
            chat,
            show_chat: true,
            chat_view: Default::default(),
        }
    }
    fn refresh_connection(&mut self) {
        let mut next = self.config.clone();
        next.url = self.endpoint.trim().trim_end_matches('/').to_owned();
        let key = self.api_key.trim();
        next.api_key = (!key.is_empty()).then(|| key.to_owned());
        let model = self.model.trim();
        next.model = (!model.is_empty()).then(|| model.to_owned());
        let applied = match self.ai.as_mut() {
            Some(worker) => worker.reconfigure(next.clone()),
            None => Interpreter::start(next.clone()).map(|worker| self.ai = Some(worker)),
        };
        match applied {
            Ok(()) => {
                self.config = next;
                self.responses.clear();
                self.error.clear();
            }
            Err(e) => self.error = e.to_string(),
        }
    }
    fn depth_view(&mut self, ui: &mut egui::Ui, s: &xtion_vision::pipeline::Snapshot) {
        let health = &s.scene.health;
        ui.small(format!(
            "Intrinsics: {:?} · {:.0}% valid",
            s.scene.optics,
            s.scene.valid_fraction * 100.
        ));
        ui.label(format!("{:.1} processed FPS · gaps {} · incomplete {} · USB errors {} · host drops {} · {} ms since frame",s.recent_fps,health.gaps,health.incomplete,health.errors,health.queue_drops,s.received.elapsed().as_millis()));
        if self.texture_key != (s.processed, self.far) {
            let copy;
            let display = if self.filtered {
                copy = xtion_vision::measurement::filtered(&s.frame);
                &copy
            } else {
                &s.frame
            };
            let (w, h, rgb) = image::rgb(display, s.frame.info.width, self.far);
            let img = egui::ColorImage::from_rgb([w, h], &rgb);
            if let Some(t) = &mut self.texture {
                t.set(img, egui::TextureOptions::NEAREST)
            } else {
                self.texture = Some(ui.ctx().load_texture(
                    "depth",
                    img,
                    egui::TextureOptions::NEAREST,
                ))
            }
            self.texture_key = (s.processed, self.far);
        }
        let Some(texture) = &self.texture else { return };
        let size = texture.size_vec2();
        let scale = (ui.available_width() / size.x)
            .min(ui.available_height() / size.y)
            .max(0.01);
        let response =
            ui.add(egui::Image::new((texture.id(), size * scale)).sense(egui::Sense::click()));
        let canvas = response.rect;
        if response.clicked()
            && let Some(mouse) = response.interact_pointer_pos()
        {
            let at = (mouse - canvas.min) / scale;
            self.pick(&s.frame, [at.x as usize, at.y as usize]);
        }
        for e in &s.graph.entities {
            let at =
                canvas.min + Vec2::new(e.image_bounds[0] as f32, e.image_bounds[1] as f32) * scale;
            self.cloud.paint_entity(ui.painter(), at, e, &s.graph);
        }
        if !self.overlays {
            return;
        }
        let pos = |x: f64, y: f64| canvas.min + Vec2::new(x as f32, y as f32) * scale;
        for o in &s.scene.objects {
            let [x, y, x2, y2] = o.region.bounds;
            let rect = egui::Rect::from_two_pos(pos(x as f64, y as f64), pos(x2 as f64, y2 as f64));
            ui.painter().rect_stroke(
                rect,
                0,
                Stroke::new(1., Color32::WHITE),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.min,
                egui::Align2::LEFT_BOTTOM,
                format!("#{} {:.2} m", o.id, o.region.z),
                egui::FontId::proportional(15.),
                Color32::WHITE,
            );
            if let (Some(v), Some(p)) = (o.velocity, o.region.centroid) {
                let predicted = xtion_vision::geometry::Point3D {
                    x: p.x + v[0] * 0.25,
                    y: p.y + v[1] * 0.25,
                    z: p.z + v[2] * 0.25,
                };
                if let Some((x, y)) = s.frame.info.intrinsics.image(predicted) {
                    let c = pos(o.region.center[0], o.region.center[1]);
                    ui.painter()
                        .arrow(c, pos(x, y) - c, Stroke::new(2., Color32::WHITE));
                }
            }
        }
    }
    fn pick(&mut self, f: &xtion_vision::DepthFrame, pixel: [usize; 2]) {
        if let Err(e) = xtion_vision::measurement::measure(f, pixel, 0., &Default::default()) {
            self.error = e.into();
            return;
        }
        if self.picks.len() == 2 {
            self.picks.clear();
        }
        self.picks.push((pixel, Default::default()));
        self.pick_stamp = None;
        self.error.clear();
    }
    fn spatial_controls(
        &mut self,
        ui: &mut egui::Ui,
        live: Option<&xtion_vision::pipeline::Snapshot>,
    ) {
        let Some(live) = live else { return };
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(if self.frozen.is_some() {
                    "Resume view"
                } else {
                    "Freeze frame"
                })
                .clicked()
            {
                self.frozen = if self.frozen.is_some() {
                    None
                } else {
                    Some(live.clone())
                };
                self.texture_key = (0, 0);
            }
            if ui.checkbox(&mut self.filtered, "Filtered depth").changed() {
                self.texture_key = (0, 0);
            }
        });
        let s = self.frozen.as_ref().unwrap_or(live);
        ui.collapsing("Semantic scene graph", |ui| {
            ui.checkbox(
                &mut self.cloud.entity_marks,
                "Entity marks and visible bounds",
            );
            ui.checkbox(&mut self.cloud.activity_marks, "Activity labels");
            ui.checkbox(&mut self.cloud.relationship_marks, "Spatial relationships");
            for e in &s.graph.entities {
                let selected = self.selected_entities.contains(&e.id);
                if ui
                    .selectable_label(
                        selected,
                        format!(
                            "E{} · {:.2} m forward · quality {:.0}%",
                            e.id,
                            e.median_z_m,
                            e.quality * 100.
                        ),
                    )
                    .clicked()
                {
                    if selected {
                        self.selected_entities.retain(|id| *id != e.id)
                    } else {
                        if self.selected_entities.len() == 2 {
                            self.selected_entities.remove(0);
                        }
                        self.selected_entities.push(e.id);
                    }
                }
                if selected {
                    ui.small(format!(
                        "Sensor XYZ {:?} · {:?} optics · members {:?}",
                        e.position_m, s.graph.optics, e.members
                    ));
                    for h in &e.labels {
                        ui.colored_label(
                            AUBERGINE,
                            format!("AI hypothesis · {} · {:?}", h.label, h.evidence),
                        );
                    }
                }
            }
            for r in s.graph.relationships.iter().take(16) {
                ui.small(format!(
                    "E{} {} {} · {}",
                    r.subject, r.predicate, r.object, r.frame
                ));
            }
        });
        ui.collapsing("Activity timeline", |ui| {
            for a in s.graph.activities.iter().rev().take(16) {
                ui.small(format!(
                    "{:.1} s · E{} {} · {}",
                    a.start_ns as f64 / 1e9,
                    a.entity,
                    a.kind,
                    if a.end_ns.is_none() {
                        "ongoing"
                    } else {
                        "ended"
                    }
                ));
            }
        });
        ui.collapsing("Ask about the scene", |ui| {
            ui.text_edit_singleline(&mut self.question);
            if ui.button("Ask").clicked() {
                let a = xtion_vision::dialogue::answer(
                    &s.graph,
                    &self.question,
                    &self.selected_entities,
                );
                self.answer = format!("{} · {} {}", a.source, a.text, a.evidence.join(", "));
                if self.frozen.is_none()
                    && let Some(worker) = &mut self.ai
                {
                    worker.ask(
                        s.scene.clone(),
                        s.frame.clone(),
                        s.graph.clone(),
                        self.config.refresh,
                        self.question.clone(),
                        self.selected_entities.clone(),
                    );
                }
            }
            ui.label(&self.answer);
        });
        ui.small(format!("Acquisition {:.1} FPS · perception {:.1} FPS · render {:.1} FPS · {:.1} ms processing · {:.1} ms arrival to result · {:.0}% valid",live.acquisition_fps,live.recent_fps,self.cloud.render_fps,live.processing_ms,live.latency_ms,100.*live.scene.valid_fraction));
        ui.checkbox(&mut self.overlays, "Depth tracking overlays");
        ui.collapsing("Measure and inspect",|ui|{
            ui.label("Click two visible points. Live picks follow their source pixels.");
            ui.horizontal(|ui|{for unit in ["mm","cm","m"]{ui.selectable_value(&mut self.units,unit,unit);}if ui.button("Clear").clicked(){self.picks.clear();}});
            let scale=match self.units{"mm"=>1000.,"cm"=>100.,_=>1.};
            let mut measurements=Vec::new();
            for (pixel,series) in &mut self.picks{
                if self.pick_stamp!=Some(s.scene.ns) && let Some(&d)=s.frame.pixels.get(pixel[1]*s.frame.info.width+pixel[0]) && d!=0{
                    if series.len()==12{series.pop_front();}series.push_back(d);
                }
                let mean=series.iter().map(|&v|f64::from(v)).sum::<f64>()/series.len().max(1) as f64;
                let sd=(series.iter().map(|&v|(f64::from(v)-mean).powi(2)).sum::<f64>()/series.len().saturating_sub(1).max(1) as f64).sqrt();
                match xtion_vision::measurement::measure(&s.frame,*pixel,sd,&Default::default()){
                    Ok(m)=>{let p=m.point;let precision=(-(m.sigma_m*scale).log10()).ceil().clamp(0.,3.) as usize;
                        ui.label(format!("Pixel {}, {} · XYZ ({:.precision$}, {:.precision$}, {:.precision$}) {} · sensor distance {:.precision$} ± {:.precision$} {}",pixel[0],pixel[1],p.x*scale,p.y*scale,p.z*scale,self.units,p.x.hypot(p.y).hypot(p.z)*scale,m.sigma_m*scale,self.units));
                        measurements.push(m);
                    }
                    Err(e)=>{ui.label(e);}
                }
            }
            self.pick_stamp=Some(s.scene.ns);
            if let [a,b]=measurements.as_slice(){let (axis,d,u)=a.separation(*b);let precision=(-(u*scale).log10()).ceil().clamp(0.,3.) as usize;
                ui.label(format!("Separation {:.precision$} ± {:.precision$} {} · ΔXYZ ({:.precision$}, {:.precision$}, {:.precision$})",d*scale,u*scale,self.units,axis[0]*scale,axis[1]*scale,axis[2]*scale));}
            if s.scene.optics!=xtion_vision::IntrinsicsSource::Calibrated{ui.small("Lateral dimensions are approximate.");}
            if let Some((pixel,_))=self.picks.last() && let Some(o)=s.scene.objects.iter().find(|o|o.region.contains_pixel(*pixel,s.frame.info.width)){
                let length=o.trajectory.windows(2).map(|pair|{let a=pair[0].1; let b=pair[1].1;(b[0]-a[0]).hypot(b[1]-a[1]).hypot(b[2]-a[2])}).sum::<f64>();
                ui.label(format!("Track {} · median Z {:.2} m · quality {:.0}% · recent path {:.2} m",o.id,o.region.median_z_m,o.region.quality*100.,length));
                if let Some([x,y,z])=o.region.extent{ui.label(format!("Visible XYZ extent {x:.2} × {y:.2} × {z:.2} m"));}
                if let Some([x,y,z])=o.filtered_velocity{ui.label(format!("Velocity ({x:.2}, {y:.2}, {z:.2}) m/s"));}
            }
        });
        ui.collapsing("Export snapshot", |ui| {
            ui.text_edit_singleline(&mut self.export_dir);
            if ui
                .add_enabled(
                    self.export_job.is_none(),
                    egui::Button::new("Save geometry and trajectories"),
                )
                .clicked()
            {
                let snap = s.clone();
                let dir = self.export_dir.clone();
                let pose = self.cloud.pose();
                let settings = self.cloud.settings();
                self.export_job = Some(std::thread::spawn(move || {
                    xtion_vision::spatial_export::export(
                        std::path::Path::new(&dir),
                        &snap.frame,
                        &snap.scene,
                        pose,
                        settings,
                    )
                    .map_err(|e| e.to_string())
                }));
            }
            if ui.button("Save window PNG").clicked() {
                self.shot = Some(format!("{}-view.png", self.export_dir));
            }
        });
        if self.export_job.as_ref().is_some_and(|j| j.is_finished())
            && let Some(j) = self.export_job.take()
        {
            self.error = match j.join() {
                Ok(Ok(())) => format!("Saved {}", self.export_dir),
                Ok(Err(e)) => e,
                Err(_) => "Snapshot export failed".into(),
            };
        }
    }
    fn restart(&mut self, input: Input) {
        if !self.pipe.finished.load(Relaxed) {
            self.error = "Stop the current capture before opening another source.".into();
            return;
        }
        self.frozen = None;
        self.labels = Default::default();
        self.selected_entities.clear();
        self.answer.clear();
        self.picks.clear();
        self.pick_stamp = None;
        self.input = input.clone();
        self.pipe =
            Pipeline::with_capture_rate(input, self.threshold, None, None, self.capture_fps);
        self.texture_key = (0, 0);
        self.responses.clear();
        if self.ai.take().is_some() {
            match Interpreter::start(self.config.clone()) {
                Ok(a) => self.ai = Some(a),
                Err(e) => self.error = e.to_string(),
            }
        }
    }
}
impl eframe::App for Viewer {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if let Some(seconds) = self.seconds {
            let remaining = Duration::from_secs_f64(seconds).saturating_sub(self.start.elapsed());
            if remaining.is_zero() {
                self.pipe.request_stop();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(remaining);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(Duration::from_millis(30));
        let mut snap = self.pipe.latest.lock().unwrap().clone();
        let status = self.pipe.status.lock().unwrap().clone();
        if let Some(s) = &snap
            && s.received.elapsed() < Duration::from_millis(500)
            && !self.pipe.finished.load(Relaxed)
            && let Some(a) = &mut self.ai
        {
            a.submit_graph(
                s.scene.clone(),
                s.frame.clone(),
                self.config.refresh,
                s.graph.clone(),
            );
        }
        let ai = snap
            .as_ref()
            .and_then(|s| self.ai.as_ref()?.latest(&s.scene));
        if let Some(s) = &mut snap {
            self.labels.apply(
                std::sync::Arc::make_mut(&mut s.graph),
                ai.as_ref().map_or(&[], |r| r.label_votes.as_slice()),
            );
        }
        let recording = self.pipe.record.lock().unwrap().clone();
        if recording != self.sidecar_path {
            self.sidecar_path = recording.clone();
            self.sidecar = None;
            if let Some(path) = recording {
                match xtion_vision::sidecar::Writer::new(path.as_ref()) {
                    Ok(w) => self.sidecar = Some(w),
                    Err(e) => self.error = e.to_string(),
                }
            }
        }
        if let (Some(w), Some(s)) = (&mut self.sidecar, &snap) {
            w.push(s.graph.clone(), ai.clone(), self.chat.snapshot());
        }
        if let Some(r) = ai
            && r.success
            && self.responses.back().is_none_or(|old| {
                old.scene_id != r.scene_id
                    && old.text != r.text
                    && (r.history_key.is_none() || old.history_key != r.history_key)
            })
        {
            if self.responses.len() == 100 {
                self.responses.pop_front();
            }
            self.responses.push_back(r);
        }
        egui::Panel::top("masthead")
            .frame(
                egui::Frame::NONE
                    .fill(AUBERGINE)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.;
                    ui.image((self.app_mark.id(), Vec2::splat(38.)));
                    ui.label(
                        egui::RichText::new("Xtion Vision")
                            .color(Color32::WHITE)
                            .size(20.),
                    );
                });
            });
        egui::Panel::right("controls")
            .default_size(300.)
            .frame(egui::Frame::NONE.fill(WARM_PAPER).inner_margin(12))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("control_scroll")
                    .show(ui, |ui| {
                        self.cloud.controls(ui);self.pipe.mount_height_cm.store(self.cloud.mount_height_cm(),Relaxed);
                        if let Some(plane) = snap.as_ref().and_then(|s| s.scene.floor_estimate) {
                            if ui.button("Use this surface as the floor").clicked() {
                                self.cloud.apply_floor(plane);
                            }
                            ui.label(format!(
                                "Support surface, type unknown · {:.2} m below sensor · tilt {:.1}° · {:?} optics",
                                plane.offset,
                                plane.floor_tilt_deg(),
                                plane.source
                            ));
                        }
                        self.spatial_controls(ui, snap.as_ref());
                        ui.heading("Capture controls");
                        ui.horizontal(|ui| {
                            if action_button(ui, "Start sensor", ActionIcon::Play, true).clicked() {
                                self.restart(Input::Live)
                            }
                            if action_button(ui, "Stop", ActionIcon::Stop, true).clicked() {
                                self.pipe.request_stop()
                            }
                        });
                        if matches!(self.input, Input::Live) {
                            ui.label("Sensor mode: VGA at 30 FPS");
                            if ui
                                .add(
                                    egui::Slider::new(&mut self.capture_fps, 5..=30)
                                        .text("App capture FPS"),
                                )
                                .changed()
                            {
                                self.pipe.set_capture_rate(self.capture_fps);
                            }
                        }
                        ui.label("Replay file");
                        ui.text_edit_singleline(&mut self.replay_path);
                        if ui.button("Open replay").clicked() {
                            self.restart(Input::Replay(self.replay_path.clone()))
                        }
                        ui.label("Recording file");
                        ui.text_edit_singleline(&mut self.record_path);
                        ui.horizontal(|ui| {
                            if ui.button("Record").clicked() {
                                *self.pipe.record.lock().unwrap() = Some(self.record_path.clone())
                            }
                            if ui.button("Finish recording").clicked() {
                                *self.pipe.record.lock().unwrap() = None
                            }
                        });
                        if ui
                            .add(
                                egui::Slider::new(&mut self.threshold, 40..=500)
                                    .text("Foreground mm"),
                            )
                            .changed()
                        {
                            self.pipe.threshold.store(self.threshold, Relaxed)
                        }
                        if ui.button("Relearn background").clicked() {
                            self.pipe.recalibrate.store(true, Relaxed)
                        }
                        ui.add(
                            egui::Slider::new(&mut self.far, 1000..=8000).text("Colour range mm"),
                        );
                        if ui.button("Save view as PNG").clicked() {
                            self.shot = Some(format!(
                                "xtion-view-{}.png",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                            ))
                        }
                        ui.separator();
                        ui.heading("Model connection");
                        if ui.checkbox(&mut self.config.allow_remote_frames,"Allow sending depth to remote endpoint").changed() && !self.config.allow_remote_frames {self.ai=None;}
                        ui.label("Endpoint URL");
                        if ui.text_edit_singleline(&mut self.endpoint).changed() {
                            self.config.allow_remote_frames=false;
                            // Never carry an entered credential over to a different endpoint.
                            self.api_key = if self.endpoint.trim().trim_end_matches('/')
                                == "https://api.openai.com/v1"
                            {
                                Config::environment_key().unwrap_or_default()
                            } else {
                                String::new()
                            };
                        }
                        ui.label("API key");
                        api_key_field(ui, &mut self.api_key);
                        ui.label("Model · leave blank to discover");
                        ui.text_edit_singleline(&mut self.model);
                        let mut interval = self.config.refresh.as_secs_f64();
                        if ui
                            .add(
                                egui::DragValue::new(&mut interval)
                                    .range(0.1..=3600.)
                                    .speed(0.2)
                                    .prefix("Minimum AI interval: ")
                                    .suffix(" s"),
                            )
                            .changed()
                        {
                            self.config.refresh = Duration::from_secs_f64(interval);
                        }
                        ui.horizontal(|ui| {
                            ui.label("Response words");
                            let max_min = (self.config.max_words - 6).min(40);
                            ui.add(
                                egui::DragValue::new(&mut self.config.min_words).range(1..=max_min),
                            );
                            ui.label("to");
                            let min_max = (self.config.min_words + 6).max(20);
                            ui.add(
                                egui::DragValue::new(&mut self.config.max_words)
                                    .range(min_max..=120),
                            );
                        });
                        if action_button(
                            ui,
                            "Refresh connection",
                            ActionIcon::Refresh,
                            self.ai.is_some(),
                        )
                        .on_hover_text("Apply the endpoint, model and response length.")
                        .clicked()
                        {
                            self.refresh_connection();
                        }
                        let mut on = self.ai.is_some();
                        if ui.checkbox(&mut on, "Enable AI analysis").changed() {
                            if on {
                                self.refresh_connection();
                            } else {
                                self.ai = None;
                            }
                        }
                        if let Some(a) = &self.ai {
                            ui.small(format!(
                                "Active · {} · {}",
                                self.config.url,
                                self.config
                                    .model
                                    .as_deref()
                                    .unwrap_or("automatic selection")
                            ));
                            ui.small(a.status());
                        }
                        if !self.error.is_empty() {
                            ui.colored_label(Color32::LIGHT_RED, &self.error);
                        }
                    });
            });
        let panel = egui::Panel::bottom("ai_responses")
            .default_size(240.)
            .min_size(110.)
            .resizable(true)
            .frame(
                egui::Frame::NONE
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| {
                ui.take_available_space();
                ui.horizontal(|ui| {
                    ui.image((self.response_mark.id(), Vec2::splat(26.)));
                    ui.label(
                        egui::RichText::new("AI responses")
                            .color(AUBERGINE)
                            .size(19.)
                            .strong(),
                    );
                    ui.selectable_value(&mut self.show_chat, true, "Chat");
                    ui.selectable_value(&mut self.show_chat, false, "Summaries");
                    if self.show_chat {
                        ui.separator();
                        self.chat_view.toggle(ui);
                    }
                });
                ui.separator();
                if self.show_chat {
                    self.chat_view.show(ui, &self.chat);
                } else if !self.responses.is_empty() {
                    response_history(ui, &self.responses);
                } else if let Some(a) = &self.ai {
                    ui.label(a.status());
                } else {
                    ui.label("Enable AI in Controls to see scene interpretations.");
                }
            });
        // The native bottom panel owns the resize gesture along this edge.
        let edge = panel.response.rect.center_top();
        ui.painter().rect_filled(
            egui::Rect::from_center_size(edge, Vec2::new(44., 4.)),
            2.,
            AUBERGINE,
        );
        let displayed = self.frozen.clone().or_else(|| snap.clone());
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(match self.cloud.mode {
                crate::cloud_view::View::Depth => "Depth view",
                crate::cloud_view::View::Points => "3D points",
                crate::cloud_view::View::Surface => "Visible surface",
                crate::cloud_view::View::Split => "Depth and 3D points",
            });
            ui.label(format!(
                "{} · perception queue drops {} · recorder drops {}",
                status,
                self.pipe.drops.load(Relaxed),
                self.pipe.recording_drops.load(Relaxed)
            ));
            if let Some(error) = self.pipe.recording_error.lock().unwrap().as_ref() {
                ui.colored_label(Color32::LIGHT_RED, format!("Recording failed: {error}"));
            }
            if let Some(snapshot) = &displayed {
                match self.cloud.mode {
                    crate::cloud_view::View::Depth => self.depth_view(ui, snapshot),
                    crate::cloud_view::View::Points | crate::cloud_view::View::Surface => {
                        self.cloud.show(ui, snapshot, self.frozen.is_some())
                    }
                    crate::cloud_view::View::Split => ui.columns(2, |cols| {
                        self.depth_view(&mut cols[0], snapshot);
                        self.cloud
                            .show(&mut cols[1], snapshot, self.frozen.is_some());
                    }),
                }
            }
        });
        if let Some(result) = self.cloud.picked.take() {
            match result {
                Ok((frame, pixel)) => self.pick(&frame, pixel),
                Err(e) => self.error = e.into(),
            }
        }
        if snap.is_some()
            && self.cloud.ready()
            && let Some(path) = self.shot.take()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |img| {
                    let rgb = img
                        .pixels
                        .iter()
                        .flat_map(|p| [p.r(), p.g(), p.b()])
                        .collect::<Vec<_>>();
                    if let Err(e) = image::png(img.size[0], img.size[1], &rgb)
                        .and_then(|b| Ok(std::fs::write(path, b)?))
                    {
                        eprintln!("PNG capture failed: {e}")
                    }
                }),
            ));
        }
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        if let Some(job) = self.export_job.take() {
            let _ = job.join();
        }
        if let Some(path) = &self.benchmark {
            let snap = self.pipe.latest.lock().unwrap();
            let result = serde_json::json!({"view":self.cloud.metrics(),"elapsed_s":self.start.elapsed().as_secs_f64(),"processed":snap.as_ref().map(|s|s.processed),"processing_fps":snap.as_ref().map(|s|s.fps),"mean_semantic_ms":snap.as_ref().map(|s|s.mean_semantic_ms),"mean_perception_ms":snap.as_ref().map(|s|s.mean_perception_ms),"latest_latency_ms":snap.as_ref().map(|s|s.latency_ms),"health":snap.as_ref().map(|s|&s.scene.health),"queue_drops":self.pipe.drops.load(Relaxed)});
            if let Err(e) = std::fs::File::create_new(path).and_then(|f| {
                serde_json::to_writer_pretty(f, &result).map_err(std::io::Error::other)
            }) {
                eprintln!("Benchmark output failed: {e}");
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ai")]
    #[test]
    fn refresh_uses_edited_endpoint_and_model_and_preserves_worker_on_invalid_url() {
        use std::{
            io::{BufRead, BufReader, Read, Write},
            net::TcpListener,
            sync::mpsc,
            thread,
        };
        use xtion_vision::{
            perception::Engine,
            source::{FrameSource, SyntheticSource},
        };
        let mut viewer = Viewer::new(
            Options {
                input: Input::Synthetic,
                capture_fps: 10,
                ai: None,
                threshold: 100,
                record: None,
                benchmark: None,
                initial_view: Default::default(),
                cpu: true,
                screenshot: None,
                seconds: Some(0.1),
            },
            &egui::Context::default(),
        );
        let mut src = SyntheticSource::default();
        let f = src.next_frame().unwrap().unwrap();
        let s = Engine::default().observe(&f);
        for model in ["original", "replacement"] {
            let sock = TcpListener::bind("127.0.0.1:0").unwrap();
            viewer.endpoint = format!("http://{}/v1/", sock.local_addr().unwrap());
            viewer.model = format!(" {model} ");
            viewer.api_key = format!("test-credential-{model}");
            viewer.refresh_connection();
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let (mut stream, _) = sock.accept().unwrap();
                let mut reader = BufReader::new(&stream);
                let mut size = 0;
                let mut authorized = false;
                let mut line = String::new();
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if line.trim_end().eq_ignore_ascii_case(&format!(
                        "authorization: Bearer test-credential-{model}"
                    )) {
                        authorized = true;
                    }
                    if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        size = n.trim().parse().unwrap();
                    }
                }
                let mut buf = vec![0; size];
                reader.read_exact(&mut buf).unwrap();
                assert!(authorized);
                let body: serde_json::Value = serde_json::from_slice(&buf).unwrap();
                let reply = r#"{"choices":[{"message":{"content":"{\"claims\":[]}"}}]}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                )
                .unwrap();
                tx.send(body["model"].as_str().unwrap().to_owned()).unwrap();
            });
            viewer
                .ai
                .as_mut()
                .unwrap()
                .submit(s.clone().into(), f.clone().into(), Duration::ZERO);
            assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), model);
        }
        viewer.endpoint = "invalid".into();
        viewer.refresh_connection();
        assert!(viewer.ai.is_some() && !viewer.error.is_empty());
        assert_eq!(viewer.config.model.as_deref(), Some("replacement"));
    }
    #[test]
    fn masked_api_key_pastes_and_copies_only_the_selected_text() {
        use egui::text::{CCursor, CCursorRange};
        let ctx = egui::Context::default();
        let mut key = String::new();
        let mut id = egui::Id::NULL;
        let mut show = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = api_key_field(ui, &mut key);
                    id = response.id;
                    response.request_focus();
                },
            );
            output.textures_delta.clear();
            (id, output.platform_output.commands)
        };
        show(vec![]);
        let (id, commands) = show(vec![egui::Event::Paste("test-only".into())]);
        assert!(
            !commands
                .iter()
                .any(|c| matches!(c, egui::OutputCommand::CopyText(_)))
        );
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap();
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(4))));
        state.store(&ctx, id);
        let (_, commands) = show(vec![egui::Event::Copy]);
        assert!(
            commands
                .iter()
                .any(|c| matches!(c,egui::OutputCommand::CopyText(v) if v=="test"))
        );
        assert_eq!(key, "test-only");
    }
    #[test]
    fn dragging_chat_divider_changes_panel_height() {
        use eframe::App;
        let ctx = egui::Context::default();
        let mut viewer = Viewer::new(
            Options {
                input: Input::Synthetic,
                capture_fps: 10,
                ai: None,
                threshold: 100,
                record: None,
                benchmark: None,
                initial_view: Default::default(),
                cpu: true,
                screenshot: None,
                seconds: Some(0.1),
            },
            &ctx,
        );
        let mut paint = |events| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1200., 760.),
                )),
                events,
                ..Default::default()
            };
            let mut rect = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(input, |ui| {
                let id = ui.scope_id().with("ai_responses");
                viewer.ui(ui, &mut eframe::Frame::_new_kittest());
                rect = egui::containers::panel::PanelState::load(&ctx, id)
                    .unwrap()
                    .outer_rect;
            });
            output.textures_delta.clear();
            rect
        };
        let initial = paint(vec![]);
        let grip = initial.center_top();
        let raised = grip - Vec2::new(0., 90.);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        paint(vec![egui::Event::PointerMoved(grip)]);
        paint(vec![button(grip, true)]);
        paint(vec![egui::Event::PointerMoved(raised)]);
        paint(vec![button(raised, false)]);
        assert!(paint(vec![]).height() > initial.height() + 70.);
    }
    #[test]
    fn history_fits_three_full_responses_and_scrolls_to_recent_entries() {
        let ctx = egui::Context::default();
        install_theme(&ctx);
        let mut reports = VecDeque::new();
        for n in 1..=10 {
            let mut report = Report::default();
            report.text = "A foreground shape moved across the centre of the scene and continued toward the right edge while the camera tracked its position over several seconds of continuous depth measurements. The last observation still placed the shape inside the camera view.".into();
            assert_eq!(report.text.split_whitespace().count(), 40);
            report.scene_id = format!("f{n}");
            reports.push_back(report);
            if n != 3 && n != 10 {
                continue;
            }
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(830., 200.),
                )),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let area = response_history(ui, &reports);
                if n == 3 {
                    assert!(area.content_size.y <= area.inner_rect.height());
                } else {
                    assert!(area.content_size.y > area.inner_rect.height());
                    assert!(area.state.clamped_offset().y > 0.);
                }
            });
            output.textures_delta.clear();
        }
    }
}
