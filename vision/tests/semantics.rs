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
#[test]
fn sustained_lateral_motion_has_one_interval_and_gaps_close_it() {
    let mut scene = fixture();
    scene.objects.truncate(1);
    let mut engine = SemanticEngine::default();
    for _ in 0..16 {
        scene.ns += 40_000_000;
        engine.observe(&scene);
    }
    let g = engine.observe(&scene);
    let active = g
        .activities
        .iter()
        .filter(|a| a.end_ns.is_none())
        .collect::<Vec<_>>();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].kind, "moving right");
    scene.status = "capture gap";
    scene.ns += 40_000_000;
    let g = engine.observe(&scene);
    assert!(g.activities.iter().all(|a| a.end_ns.is_some()));
    assert!(
        !g.activities
            .iter()
            .any(|a| a.kind == "left visible tracking")
    );
}
#[test]
fn local_questions_use_only_visible_measurements() {
    let g = SemanticEngine::default().observe(&fixture());
    let ask = |q| xtion_vision::dialogue::answer(&g, q, &[]);
    assert_eq!(
        ask("how many entities?").text,
        "2 visible tracked entities."
    );
    assert_eq!(ask("how many people?").source, "unknown");
    assert!(
        ask("nearest entity?")
            .text
            .contains(&format!("{:.2} m", g.entities[0].median_z_m))
    );
    assert_eq!(ask("distance to E999").source, "unknown");
    assert_eq!(ask("recent events").source, "historical");
}
#[test]
fn ai_labels_require_three_distinct_observations_and_expire_on_split() {
    use xtion_vision::semantic::{HypothesisStore, LabelVote, Provenance};
    let mut s = fixture();
    s.objects.truncate(1);
    let mut engine = SemanticEngine::default();
    let mut store = HypothesisStore::default();
    for i in 0..3 {
        s.ns += 1_000_000_000;
        s.objects[0].region.quality = 1.;
        let mut g = engine.observe(&s);
        // Keep the track alive across the inter-request interval.
        engine.timeout_ns = 2_000_000_000;
        let e = &g.entities[0];
        let vote = LabelVote {
            entity: e.id,
            members: e.members.clone(),
            ns: g.ns,
            evidence: format!("obs{i}"),
            label: "box-like".into(),
        };
        for _ in 0..2 {
            store.apply(&mut g, std::slice::from_ref(&vote));
        }
        assert_eq!(g.entities[0].labels.len(), usize::from(i == 2));
        if i == 2 {
            assert_eq!(g.entities[0].labels[0].provenance, Provenance::AiHypothesis);
            g.entities[0].members.push(2);
            store.apply(&mut g, &[]);
            assert!(g.entities[0].labels.is_empty());
        }
    }
}
#[test]
fn known_mount_rejects_an_incompatible_tabletop() {
    use xtion_vision::planes::{Plane, support_candidate};
    let plane = |offset| Plane {
        offset,
        normal: [0., 1., 0.],
        cells: 20,
        source: xtion_vision::IntrinsicsSource::Nominal,
    };
    let surfaces = [plane(0.6), plane(1.2)];
    assert_eq!(
        support_candidate(&surfaces, Some(1.18)).unwrap().offset,
        1.2
    );
    assert!(support_candidate(&surfaces[..1], Some(1.18)).is_none());
}
#[test]
fn observation_motion_reconnects_new_fragment_id_after_brief_loss() {
    let mut s = fixture();
    s.objects.truncate(1);
    let mut e = SemanticEngine::default();
    let a = e.observe(&s);
    let obj = s.objects.pop().unwrap();
    s.ns += 100_000_000;
    e.observe(&s);
    s.ns += 100_000_000;
    s.objects.push(obj);
    s.objects[0].id = 99;
    s.objects[0].region.centroid.as_mut().unwrap().x += 0.06;
    let b = e.observe(&s);
    assert_eq!(a.entities[0].id, b.entities[0].id);
    assert_eq!(b.entities[0].member_history.len(), 2);
}
