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

#[test]
fn separates_adjacent_objects_across_a_depth_step_and_marks_missing_pixels() {
    let mut source = SyntheticSource::default();
    let mut engine = Engine::default();
    for t in 0..20 {
        let mut frame = source.next_frame().unwrap().unwrap();
        let w = frame.info.width;
        frame.pixels.fill(3000);
        if t >= 6 {
            for y in 12..48 {
                frame.pixels[y * w + 16..y * w + 40].fill(1300);
                frame.pixels[y * w + 40..y * w + 64].fill(1700);
            }
            frame.pixels[20 * w + 24] = 0;
        }
        let state = engine.observe(&frame);
        if t > 7 {
            assert_eq!(state.objects.len(), 2);
            assert_eq!(engine.quality[20 * w + 24], 1);
            assert_ne!(engine.quality[24 * w + 39] & 4, 0);
            assert!((state.objects[0].region.z - state.objects[1].region.z).abs() > 0.35);
            assert!(state.objects.iter().all(|o| !o.moving));
        }
    }
}

#[test]
fn a_step_in_a_tracked_silhouette_preserves_its_id_but_separate_surfaces_stay_separate() {
    for initially_separate in [false, true] {
        let mut source = SyntheticSource::default();
        let mut engine = Engine::default();
        let mut initial_ids = Vec::new();
        for t in 0..18 {
            let mut frame = source.next_frame().unwrap().unwrap();
            let w = frame.info.width;
            frame.pixels.fill(3000);
            if t >= 6 {
                for row in frame.pixels.chunks_exact_mut(w).take(48).skip(12) {
                    row[16..40].fill(1400);
                    row[40..64].fill(if t < 12 && !initially_separate {
                        1400
                    } else {
                        1560
                    });
                }
            }
            let scene = engine.observe(&frame);
            let ids = scene.objects.iter().map(|o| o.id).collect::<Vec<_>>();
            if t == 10 {
                assert_eq!(ids.len(), if initially_separate { 2 } else { 1 });
                initial_ids = ids.clone();
            }
            if t >= 12 {
                assert_eq!(ids, initial_ids);
                assert_eq!(
                    scene
                        .objects
                        .iter()
                        .map(|o| o.region.samples)
                        .sum::<usize>(),
                    108
                );
            }
        }
    }
}

#[test]
fn one_invalid_native_column_preserves_established_surface_without_filling_pixels() {
    let mut source = SyntheticSource::default();
    let mut engine = Engine::default();
    let mut id = None;
    for t in 0..18 {
        let mut frame = source.next_frame().unwrap().unwrap();
        let w = frame.info.width;
        frame.pixels.fill(3000);
        if t >= 6 {
            for row in frame.pixels.chunks_exact_mut(w).take(48).skip(12) {
                row[16..64].fill(1400);
                if t >= 12 {
                    row[40] = 0;
                }
            }
        }
        let state = engine.observe(&frame);
        if t == 10 {
            id = Some(state.objects[0].id);
        }
        if t >= 12 {
            assert_eq!(state.objects.len(), 1);
            assert_eq!(Some(state.objects[0].id), id);
            assert_eq!(state.objects[0].region.samples, 99);
            assert_eq!(engine.quality[20 * w + 40], 1);
        }
    }
}

#[test]
fn sparse_depth_changes_only_recruit_a_surface_with_foreground_boundaries() {
    for surround in [1000, 3000] {
        let mut source = SyntheticSource::default();
        let mut engine = Engine::default();
        for t in 0..10 {
            let mut f = source.next_frame().unwrap().unwrap();
            let w = f.info.width;
            f.pixels.fill(surround);
            for row in f.pixels.chunks_exact_mut(w).take(48).skip(12) {
                row[24..72].fill(1800);
                if t < 6 {
                    row[32..40].fill(2600);
                }
            }
            let scene = engine.observe(&f);
            if t >= 8 {
                if surround == 3000 {
                    assert_eq!(scene.objects.len(), 1);
                    assert_eq!(scene.objects[0].region.samples, 108);
                } else {
                    assert!(scene.objects.iter().all(|o| o.region.samples < 108));
                }
            }
        }
    }
}

#[test]
fn compact_foreground_at_vga_resolution_keeps_the_original_six_sample_sensitivity() {
    let mut source = SyntheticSource::default();
    let mut engine = Engine::default();
    for t in 0..12 {
        let mut frame = source.next_frame().unwrap().unwrap();
        frame.info.width = 640;
        frame.info.height = 480;
        frame.info.intrinsics = xtion_vision::CameraIntrinsics::nominal(640, 480);
        frame.pixels = vec![2800; 640 * 480];
        if t >= 6 {
            for row in frame
                .pixels
                .as_chunks_mut::<640>()
                .0
                .iter_mut()
                .skip(200)
                .take(12)
            {
                row[320..332].fill(1400);
            }
        }
        let scene = engine.observe(&frame);
        if t >= 8 {
            assert_eq!(scene.objects.len(), 1);
            assert_eq!(scene.objects[0].region.samples, 9);
        }
    }
}

#[test]
fn a_compact_change_on_a_large_coplanar_surface_retains_only_measured_seeds() {
    let mut source = SyntheticSource::default();
    let mut engine = Engine::default();
    for t in 0..12 {
        let mut f = source.next_frame().unwrap().unwrap();
        let w = f.info.width;
        f.pixels.fill(1800);
        if t < 6 {
            for row in f.pixels.chunks_exact_mut(w).skip(16).take(12) {
                row[32..44].fill(2800);
            }
        }
        let scene = engine.observe(&f);
        if t >= 8 {
            assert_eq!(scene.objects.len(), 1);
            assert_eq!(scene.objects[0].region.samples, 9);
        }
    }
}

#[test]
fn sparse_unstable_depth_cannot_confirm_a_compact_track() {
    let mut source = SyntheticSource::default();
    let mut engine = Engine::default();
    for t in 0..18 {
        let mut f = source.next_frame().unwrap().unwrap();
        let w = f.info.width;
        // Both values remain close enough to grow into the surrounding plane.
        // The local patch has a large foreground residual after warmup, but its
        // alternating depths disagree by more than the temporal noise bound.
        f.pixels.fill(if t % 2 == 0 { 1800 } else { 1850 });
        if t < 6 {
            for row in f.pixels.chunks_exact_mut(w).skip(16).take(12) {
                row[32..44].fill(2800);
            }
        }
        let scene = engine.observe(&f);
        assert!(scene.objects.is_empty());
    }
}

#[test]
fn opposite_depth_order_on_the_two_sides_does_not_promote_a_background_strip() {
    let mut source = SyntheticSource::default();
    let mut engine = Engine::default();
    for t in 0..12 {
        let mut f = source.next_frame().unwrap().unwrap();
        let w = f.info.width;
        for row in f.pixels.chunks_exact_mut(w) {
            row[..24].fill(3000);
            row[24..72].fill(1800);
            row[72..].fill(1000);
        }
        if t < 6 {
            for row in f.pixels.chunks_exact_mut(w).skip(16).take(12) {
                row[32..44].fill(2800);
            }
        }
        let scene = engine.observe(&f);
        assert!(scene.objects.iter().all(|o| o.region.samples <= 9));
    }
}

#[test]
fn established_foreground_keeps_its_measured_extent_when_it_meets_a_static_surface() {
    let mut source = SyntheticSource::default();
    let mut engine = Engine::default();
    let mut initial = None;
    for t in 0..16 {
        let mut f = source.next_frame().unwrap().unwrap();
        let w = f.info.width;
        f.pixels.fill(3000);
        for row in f.pixels.chunks_exact_mut(w) {
            row[..40].fill(1800);
        }
        if t >= 6 {
            let x = if t < 12 { 44 } else { 40 };
            for row in f.pixels.chunks_exact_mut(w).skip(16).take(20) {
                row[x..x + 24].fill(1800);
            }
        }
        let scene = engine.observe(&f);
        if t == 10 {
            initial = Some(scene.objects[0].id);
        }
        if t >= 12 {
            assert_eq!(scene.objects.len(), 1);
            assert_eq!(Some(scene.objects[0].id), initial);
            assert_eq!(scene.objects[0].region.samples, 30);
        }
    }
}

#[test]
fn sparse_coplanar_change_does_not_label_a_learned_wall_as_a_moving_region() {
    let mut source = SyntheticSource::default();
    let mut engine = Engine::default();
    for t in 0..14 {
        let mut frame = source.next_frame().unwrap().unwrap();
        let (w, h) = (320, 240);
        frame.info.width = w;
        frame.info.height = h;
        frame.info.intrinsics = xtion_vision::CameraIntrinsics::nominal(w, h);
        frame.pixels = vec![3000; w * h];
        for row in frame.pixels.chunks_exact_mut(w).skip(24).take(192) {
            row[48..272].fill(1800);
        }
        for row in frame.pixels.chunks_exact_mut(w).skip(80).take(32) {
            row[120..152].fill(if t < 6 {
                2800
            } else if t < 10 {
                1800
            } else {
                // Step toward the camera by five centimetres per frame.
                1800 - 50 * (t - 9)
            });
        }
        let state = engine.observe(&frame);
        if t >= 8 {
            assert_eq!(state.objects.len(), 1, "frame {t}");
            assert_eq!(state.objects[0].region.samples, 64);
        }
    }
}

#[test]
fn fresh_motion_events_require_continuous_capture() {
    let mut source = SyntheticSource::default();
    let mut engine = Engine::default();
    while let Some(mut frame) = source.next_frame().unwrap() {
        frame.info.health.gaps = frame.info.health.frames;
        let scene = engine.observe(&frame);
        assert!(
            !scene
                .events
                .iter()
                .any(|e| e.ns == scene.ns && e.kind == "moving")
        );
    }
}
