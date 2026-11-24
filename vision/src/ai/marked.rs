//! Prepare bounded, sensor-facing observations on the AI worker only.
use crate::{
    DepthFrame, image, perception::SceneState, semantic::SemanticSceneGraph, source::Result,
};
use font8x8::{BASIC_FONTS, UnicodeFonts};
use serde_json::json;
pub(super) fn mark(rgb: &mut [u8], w: usize, h: usize, x: usize, y: usize, label: &str) {
    let x = x.min(w.saturating_sub(label.len() * 8));
    let y = y.min(h.saturating_sub(8));
    for (i, c) in label.chars().enumerate() {
        if let Some(g) = BASIC_FONTS.get(c) {
            for (row, bits) in g.iter().enumerate() {
                for col in 0..8 {
                    let (px, py) = (x + i * 8 + col, y + row);
                    if px < w && py < h {
                        let j = (py * w + px) * 3;
                        rgb[j..j + 3].fill(if bits & (1 << col) != 0 { 255 } else { 0 });
                    }
                }
            }
        }
    }
}
pub(super) fn package(
    f: &DepthFrame,
    s: &SceneState,
    g: &SemanticSceneGraph,
) -> Result<(String, Vec<u8>, Vec<u8>)> {
    let (w, h, mut depth) = image::rgb(f, 224, 4000);
    let rows = h - 12;
    let mut shaded = vec![0; 3 * w * rows];
    if let Some(cloud) = f.organised_cloud() {
        for y in 0..rows {
            for x in 0..w {
                let (u, v) = (x * f.info.width / w, y * f.info.height / rows);
                let (Some(p), Some(q), Some(r)) = (
                    cloud.point(u, v),
                    cloud.point(u + 1, v),
                    cloud.point(u, v + 1),
                ) else {
                    continue;
                };
                if (p.z - q.z).abs().max((p.z - r.z).abs()) > 0.04 {
                    continue;
                }
                let a = [q.x - p.x, q.y - p.y, q.z - p.z];
                let b = [r.x - p.x, r.y - p.y, r.z - p.z];
                let n = [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ];
                let len = n[0].hypot(n[1]).hypot(n[2]);
                if len > 0. {
                    let light = ((n[0] * 0.36 + n[1] * 0.48 + n[2] * 0.8) / len).abs();
                    let j = 3 * (y * w + x);
                    shaded[j..j + 3].fill((35. + 210. * light) as u8);
                }
            }
        }
    }
    let mut entities = vec![];
    for e in &g.entities {
        let mark_id = format!("E{}", e.id);
        let x = (e.image_bounds[0] + e.image_bounds[2]) / 2 * w / f.info.width;
        let y = (e.image_bounds[1] + e.image_bounds[3]) / 2 * rows / f.info.height;
        mark(&mut depth, w, rows, x, y, &mark_id);
        mark(&mut shaded, w, rows, x, y, &mark_id);
        let trajectory = s
            .objects
            .iter()
            .find(|o| e.members.contains(&o.id))
            .map(|o| o.trajectory.iter().rev().take(4).rev().collect::<Vec<_>>());
        entities.push(json!({"mark":mark_id,"trajectory":trajectory,"axial_sigma_m":e.axial_sigma_m,"previous_labels":e.labels,"observations":e.evidence.iter().map(|v|&v.observation).collect::<Vec<_>>(),"members":e.members,"position_m":e.position_m,"bounds_m":e.bounds_m,"z_m":e.median_z_m,"velocity_m_s":e.velocity_m_s,"quality":e.quality,"moving":e.moving}));
    }
    let mut relations = g.relationships.iter().take(12).collect::<Vec<_>>();
    let mut activity = g.activities.iter().rev().take(6).collect::<Vec<_>>();
    let packet = loop {
        let p=json!({"schema":"xtion.observations/3","id":s.id,"ns":s.ns,"allowed_labels":crate::semantic_config::Settings::get().labels,"ticks":s.ticks,"optics":s.optics,"frame":"sensor","health":s.health,"status":s.status,"support_plane":s.floor_estimate,"entities":entities,"relationships":relations,"activities":activity}).to_string();
        if p.len() <= crate::semantic_config::Settings::get().spatial_budget_bytes {
            break p;
        }
        if relations.pop().is_none() && activity.pop().is_none() && entities.pop().is_none() {
            return Err("spatial budget exhausted".into());
        }
    };
    Ok((
        packet,
        image::png(w, h, &depth)?,
        image::png(w, rows, &shaded)?,
    ))
}
