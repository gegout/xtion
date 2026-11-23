use xtion_vision::{perception::*, source::*};
#[test]
fn tracks_mover_across_occlusion() {
    let mut s = SyntheticSource::default();
    let mut e = Engine::default();
    let mut kinds = vec![];
    let mut seen = 0;
    while let Some(f) = s.next_frame().unwrap() {
        let scene = e.observe(&f);
        if let Some(o) = scene.objects.first() {
            assert_eq!(o.id, 1);
            seen += 1;
            if f.info.health.frames == 20 {
                assert_eq!(o.direction, "approaching");
                assert!((o.radial_velocity + 0.25).abs() < 1e-8);
                assert!(o.speed.unwrap() >= 0.25 - 1e-8)
            }
            if f.info.health.frames == 52 {
                assert_eq!(o.direction, "receding")
            }
        }
        kinds = scene.events.into_iter().map(|v| v.kind).collect();
    }
    assert!(seen > 50);
    assert_eq!(
        kinds,
        ["appeared", "moving", "stationary", "moving", "disappeared"]
    );
}
