use eframe::egui::{self, Color32, Stroke, Vec2};
use std::{
    sync::atomic::Ordering::Relaxed,
    time::{Duration, Instant},
};
use xtion_vision::{
    ai::{Config, Interpreter},
    describe::describe,
    image,
    pipeline::{Input, Pipeline},
    source::Result,
};
pub struct Options {
    pub input: Input,
    pub ai: Option<Config>,
    pub threshold: u16,
    pub record: Option<String>,
    pub screenshot: Option<String>,
    pub seconds: Option<f64>,
}
pub fn launch(opts: Options) -> Result<()> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default().with_inner_size([1200., 760.]),
        ..Default::default()
    };
    eframe::run_native(
        "Xtion Vision",
        options,
        Box::new(move |_| Ok(Box::new(Viewer::new(opts)))),
    )
    .map_err(|e| e.to_string().into())
}
struct Viewer {
    input: Input,
    pipe: Pipeline,
    endpoint: String,
    model: String,
    ai: Option<Interpreter>,
    config: Config,
    threshold: u16,
    far: u16,
    record_path: String,
    replay_path: String,
    error: String,
    texture: Option<egui::TextureHandle>,
    texture_key: (u64, u16),
    shot: Option<String>,
    start: Instant,
    seconds: Option<f64>,
    timeline: std::collections::VecDeque<String>,
    last_ai: String,
    last_status: String,
}
impl Viewer {
    fn new(o: Options) -> Self {
        let cfg = o.ai.clone().unwrap_or_else(|| Config::new(String::new()));
        let ai = o.ai.map(Interpreter::start).transpose();
        let error = ai
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        Self {
            pipe: Pipeline::start(o.input.clone(), o.threshold, o.record.clone()),
            input: o.input,
            endpoint: cfg.url.clone(),
            model: cfg.model.clone().unwrap_or_default(),
            ai: ai.ok().flatten(),
            config: cfg,
            threshold: o.threshold,
            far: 4000,
            record_path: o.record.unwrap_or_else(|| "session.xtv".into()),
            replay_path: String::new(),
            error,
            texture: None,
            texture_key: (0, 0),
            shot: o.screenshot,
            start: Instant::now(),
            seconds: o.seconds,
            timeline: Default::default(),
            last_ai: String::new(),
            last_status: String::new(),
        }
    }
    fn log(&mut self, s: String) {
        if self.timeline.len() == 32 {
            self.timeline.pop_front();
        }
        self.timeline
            .push_back(format!("{:.1}s  {s}", self.start.elapsed().as_secs_f64()));
    }
    fn restart(&mut self, input: Input) {
        if !self.pipe.finished.load(Relaxed) {
            self.error = "Stop the current capture before opening another source.".into();
            return;
        }
        self.input = input.clone();
        self.pipe = Pipeline::start(input, self.threshold, None);
        self.texture_key = (0, 0);
        self.last_ai.clear();
    }
}
impl eframe::App for Viewer {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(Duration::from_millis(30));
        let snap = self.pipe.latest.lock().unwrap().clone();
        let status = self.pipe.status.lock().unwrap().clone();
        if self.last_status != status {
            self.log(status.clone());
            self.last_status = status.clone()
        }
        if let Some(s) = &snap
            && let Some(a) = &mut self.ai
        {
            a.submit(s.scene.clone(), s.frame.clone(), self.config.refresh);
        }
        let ai = snap
            .as_ref()
            .and_then(|s| self.ai.as_ref()?.latest(&s.scene));
        if let Some(r) = &ai
            && r.scene_id != self.last_ai
        {
            self.log(format!("AI {} · {}", r.model, r.text));
            self.last_ai = r.scene_id.clone()
        }
        egui::Panel::right("controls")
            .default_size(300.)
            .show(ui, |ui| {
                ui.heading("Controls");
                ui.horizontal(|ui| {
                    if ui.button("Start sensor").clicked() {
                        self.restart(Input::Live)
                    }
                    if ui.button("Stop").clicked() {
                        self.pipe.request_stop()
                    }
                });
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
                    .add(egui::Slider::new(&mut self.threshold, 40..=500).text("Foreground mm"))
                    .changed()
                {
                    self.pipe.threshold.store(self.threshold, Relaxed)
                }
                if ui.button("Relearn background").clicked() {
                    self.pipe.recalibrate.store(true, Relaxed)
                }
                ui.add(egui::Slider::new(&mut self.far, 1000..=8000).text("Colour range mm"));
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
                ui.heading("AI interpretation");
                ui.label("Endpoint");
                ui.text_edit_singleline(&mut self.endpoint);
                ui.label("Model (blank for discovery)");
                ui.text_edit_singleline(&mut self.model);
                let mut on = self.ai.is_some();
                if ui
                    .checkbox(&mut on, "Enable AI and send depth views")
                    .changed()
                {
                    self.ai = None;
                    if on {
                        self.config.url = self.endpoint.clone();
                        self.config.model = (!self.model.is_empty()).then(|| self.model.clone());
                        match Interpreter::start(self.config.clone()) {
                            Ok(a) => self.ai = Some(a),
                            Err(e) => self.error = e.to_string(),
                        }
                    }
                }
                if !self.error.is_empty() {
                    ui.colored_label(Color32::LIGHT_RED, &self.error);
                }
                ui.separator();
                ui.heading("Scene description");
                if let Some(s) = &snap {
                    let stale = s.received.elapsed() > Duration::from_millis(500)
                        || self.pipe.finished.load(Relaxed);
                    if stale {
                        ui.colored_label(Color32::YELLOW, "Last observation · capture inactive");
                    }
                    ui.label(describe(&s.scene));
                    ui.label(format!(
                        "Intrinsics: {:?} · {:.0}% valid",
                        s.scene.optics,
                        s.scene.valid_fraction * 100.
                    ));
                    if let Some(r) = &ai {
                        ui.label(format!("{} · {} ms · {}", r.model, r.latency_ms, r.text));
                        for c in &r.claims {
                            ui.label(format!(
                                "{} · {:.0}% confidence",
                                c.observation_id,
                                c.confidence * 100.
                            ));
                        }
                    }
                }
            });
        egui::Panel::bottom("timeline")
            .default_size(145.)
            .show(ui, |ui| {
                ui.heading("Timeline");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for l in self.timeline.iter().rev().take(3) {
                        ui.label(l);
                    }
                    if let Some(s) = &snap {
                        for e in s.scene.events.iter().rev() {
                            ui.label(format!(
                                "{:.2}s  {}  {}  {:?}",
                                e.ns as f64 * 1e-9,
                                e.id,
                                e.kind,
                                e.object
                            ));
                        }
                    }
                });
            });
        egui::CentralPanel::default().show(ui,|ui|{ui.heading("Depth viewer");ui.label(format!("{} · perception queue drops {}",status,self.pipe.drops.load(Relaxed)));
if let Some(s)=&snap{let f=&s.frame;ui.label(format!("{:.1} FPS · gaps {} · incomplete {} · USB errors {} · host queue drops {} · {} ms since frame",s.fps,s.scene.health.gaps,s.scene.health.incomplete,s.scene.health.errors,s.scene.health.queue_drops,s.received.elapsed().as_millis()));
if self.texture_key!=(s.processed,self.far){let (w,h,rgb)=image::rgb(f,f.info.width,self.far);let img=egui::ColorImage::from_rgb([w,h],&rgb);if let Some(t)=&mut self.texture{t.set(img,egui::TextureOptions::NEAREST)}else{self.texture=Some(ctx.load_texture("depth",img,egui::TextureOptions::NEAREST))}self.texture_key=(s.processed,self.far)}
if let Some(t)=&self.texture{let scale=(ui.available_width()/t.size_vec2().x).min(ui.available_height()/t.size_vec2().y).max(0.01);let r=ui.image((t.id(),t.size_vec2()*scale)).rect;let pos=|x:f64,y:f64|r.min+Vec2::new(x as f32,y as f32)*scale;
for o in &s.scene.objects{let [x,y,x2,y2]=o.region.bounds;let rect=egui::Rect::from_two_pos(pos(x as f64,y as f64),pos(x2 as f64,y2 as f64));ui.painter().rect_stroke(rect,0,Stroke::new(1.,Color32::WHITE),egui::StrokeKind::Inside);ui.painter().text(rect.min,egui::Align2::LEFT_BOTTOM,format!("#{} {:.2} m",o.id,o.region.z),egui::FontId::proportional(15.),Color32::WHITE);if let (Some(v),Some(p))=(o.velocity,o.region.centroid){let k=f.info.intrinsics;let q=xtion_vision::geometry::Point3D{x:p.x+v[0]*0.25,y:p.y+v[1]*0.25,z:p.z+v[2]*0.25};if let Some((x,y))=k.image(q){let c=pos(o.region.center[0],o.region.center[1]);ui.painter().arrow(c,pos(x,y)-c,Stroke::new(2.,Color32::WHITE));}}}}}
});
        if snap.is_some()
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
        if self
            .seconds
            .is_some_and(|s| self.start.elapsed().as_secs_f64() >= s)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
