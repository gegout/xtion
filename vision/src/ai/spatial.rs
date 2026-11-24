use crate::perception::SceneState;
use serde_json::json;
const LIMIT: usize = 8192;
pub(super) fn observations(s: &SceneState) -> String {
    let mut tracks = Vec::new();
    let mut relations = Vec::new();
    for o in s.objects.iter().take(6) {
        let trail = o.trajectory.iter().rev().take(4).rev().collect::<Vec<_>>();
        tracks.push(json!({"id":o.id,"observation_id":o.observation_id,"centroid_m":o.region.centroid,"visible_extent_m":o.region.extent,"median_z_m":o.region.median_z_m,"distance_m":o.distance_m,"velocity_m_s":o.filtered_velocity,"direction":o.direction,"quality":o.region.quality,"clipped":o.region.clipped,"recent_positions":trail}));
        if let (Some(a), Some(b)) = (s.objects.first(), o.region.centroid)
            && a.id != o.id
            && let Some(p) = a.region.centroid
        {
            relations.push(json!({"from":a.observation_id,"to":o.observation_id,"delta_m":[b.x-p.x,b.y-p.y,b.z-p.z]}));
        }
    }
    let mut events = s.events.iter().rev().take(6).rev().collect::<Vec<_>>();
    loop {
        let data=json!({"schema":"xtion.observations/2","id":s.id,"ns":s.ns,"ticks":s.ticks,"optics":s.optics,"status":s.status,"health":s.health,"valid_fraction":s.valid_fraction,"sector_z_m":s.sector_z_m,"objects":tracks,"relationships":relations,"events":events}).to_string();
        if data.len() <= LIMIT {
            return data;
        }
        if tracks.pop().is_some() {
            relations.pop();
        } else if events.pop().is_none() {
            return json!({"schema":"xtion.observations/2","status":"observations exceeded byte limit"}).to_string();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        perception::Engine,
        source::{FrameSource, SyntheticSource},
    };
    #[test]
    fn spatial_packet_is_bounded_and_keeps_calibration_provenance() {
        let mut e = Engine::default();
        let mut source = SyntheticSource::default();
        let mut s = (0..20)
            .map(|_| e.observe(&source.next_frame().unwrap().unwrap()))
            .last()
            .unwrap();
        s.objects = vec![s.objects[0].clone(); 200];
        let packet = observations(&s);
        assert!(packet.len() <= LIMIT);
        let v: serde_json::Value = serde_json::from_str(&packet).unwrap();
        assert_eq!(v["schema"], "xtion.observations/2");
        assert_eq!(v["optics"], "Nominal");
        assert!(
            v["objects"][0]["recent_positions"]
                .as_array()
                .unwrap()
                .len()
                <= 4
        );
    }
}
