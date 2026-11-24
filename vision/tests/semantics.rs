use xtion_vision::{
    perception::{Engine, SceneState},
    semantic::SemanticEngine,
    source::{FrameSource, SyntheticSource},
};
fn fixture() -> SceneState {
    let mut e = Engine::default();
    let mut src = SyntheticSource::default();
    let mut s = (0..18)
        .map(|_| e.observe(&src.next_frame().unwrap().unwrap()))
        .last()
        .unwrap();
    let mut t = s.objects[0].clone();
    t.id += 1;
    t.observation_id = "fragment".into();
    for o in &mut s.objects {
        o.moving = true;
        o.filtered_velocity = Some([0.3, 0., 0.]);
    }
    t.moving = true;
    t.filtered_velocity = Some([0.3, 0., 0.]);
    s.objects.push(t);
    s
}
#[test]
fn fragments_require_consistent_motion_and_split_again() {
    let mut s = fixture();
    let mut engine = SemanticEngine::default();
    for i in 0..4 {
        s.ns += 40_000_000;
        s.id = format!("f{i}");
        let g = engine.observe(&s);
        assert_eq!(g.entities.len(), if i >= 2 { 1 } else { 2 });
    }
    s.objects[1].filtered_velocity = Some([-0.3, 0., 0.]);
    s.ns += 40_000_000;
    let g = engine.observe(&s);
    assert_eq!(g.entities.len(), 2);
    assert_ne!(g.entities[0].id, g.entities[1].id);
}
#[test]
fn stationary_neighbors_stay_separate_and_relate_in_sensor_frame() {
    let mut s = fixture();
    s.objects[1].moving = false;
    s.objects[1].region.centroid.as_mut().unwrap().x += 0.4;
    let mut engine = SemanticEngine::default();
    let g = engine.observe(&s);
    assert_eq!(g.entities.len(), 2);
    assert!(
        g.relationships
            .iter()
            .any(|r| r.predicate == "left of" && r.frame == "sensor" && r.evidence.len() == 2)
    );
    s.objects.clear();
    s.ns += 900_000_000;
    let gone = engine.observe(&s);
    assert_eq!(gone.expired.len(), 2);
    assert_eq!(gone.delta(&g).removed.len(), 2);
}
