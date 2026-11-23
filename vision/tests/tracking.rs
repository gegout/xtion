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

#[test]
fn noise_and_missing_pixels_do_not_generate_motion() {
    let mut engine = Engine::default();
    let mut f = SyntheticSource::default().next_frame().unwrap().unwrap();
    for t in 0..100 {
        for (n, d) in f.pixels.iter_mut().enumerate() {
            *d = if n % 17 == 0 {
                0
            } else {
                2000 + ((n + 3 * t) % 19) as u16
            };
        }
        f.info.arrival_ns = t as u64 * 33_333_333;
        f.info.ticks = t as u32;
        let s = engine.observe(&f);
        assert!(s.objects.is_empty());
        assert!(s.events.is_empty());
    }
}
#[test]
fn signal_loss_is_distinct_from_departure_and_unknown_optics_keep_z() {
    use xtion_vision::IntrinsicsSource;
    let mut s = SyntheticSource::default();
    let mut e = Engine::default();
    let mut t = 0;
    while let Some(mut f) = s.next_frame().unwrap() {
        f.info.intrinsics.source = IntrinsicsSource::Unknown;
        if (20..40).contains(&t) {
            f.pixels.fill(0);
        }
        let scene = e.observe(&f);
        if t == 15 {
            assert_eq!(scene.objects.len(), 1);
            let o = &scene.objects[0];
            assert!(o.region.z > 1.);
            assert!(o.region.centroid.is_none());
            assert!(o.velocity.is_none());
        }
        if t == 39 {
            assert_eq!(scene.status, "insufficient depth coverage");
            assert!(scene.events.iter().any(|v| v.kind == "depth unavailable"));
            assert!(!scene.events.iter().any(|v| v.kind == "disappeared"))
        }
        t += 1;
    }
}

#[test]
fn raw_shifts_are_never_measured_as_metres() {
    let mut e = Engine::default();
    let mut source = SyntheticSource::default();
    while let Some(mut frame) = source.next_frame().unwrap() {
        frame.info.metric = false;
        assert!(frame.point_cloud().is_none());
        let s = e.observe(&frame);
        assert_eq!(s.status, "nonmetric");
        assert!(s.objects.is_empty());
        assert_eq!(s.sector_z_m, [None; 3]);
    }
}
