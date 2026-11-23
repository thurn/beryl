use super::*;
use crate::pose::Segment;

fn local_ease() -> HermiteCurve {
    HermiteCurve {
        keys: [
            HermiteKey {
                time: 0.0,
                value: 0.0,
                in_slope: 0.0,
                out_slope: 0.0,
            },
            HermiteKey {
                time: 0.57859,
                value: 0.8,
                in_slope: 0.0,
                out_slope: 0.0,
            },
            HermiteKey {
                time: 0.70068,
                value: 0.8,
                in_slope: 0.0,
                out_slope: 0.0,
            },
            HermiteKey {
                time: 0.94951,
                value: 1.0,
                in_slope: 0.0,
                out_slope: 0.0,
            },
        ],
        len: 4,
    }
}

fn opponent_ease() -> HermiteCurve {
    HermiteCurve {
        keys: [
            HermiteKey {
                time: 0.0,
                value: 0.0,
                in_slope: 0.0,
                out_slope: 0.0,
            },
            HermiteKey {
                time: 0.04998,
                value: 0.0,
                in_slope: 0.0,
                out_slope: 1.03272,
            },
            HermiteKey {
                time: 1.0,
                value: 1.0,
                in_slope: 0.0,
                out_slope: 0.0,
            },
            HermiteKey::default(),
        ],
        len: 3,
    }
}

#[test]
fn segment_stays_within_value_size_budget() {
    assert!(
        std::mem::size_of::<Segment>() <= 256,
        "Segment grew to {} bytes",
        std::mem::size_of::<Segment>()
    );
}

#[test]
fn hermite_local_draw_holds_flat_plateau_and_settles_at_one() {
    let curve = local_ease();
    assert!((curve.evaluate(0.0) - 0.0).abs() < 1e-6);
    assert!((curve.evaluate(0.57859) - 0.8).abs() < 1e-6);
    assert!((curve.evaluate(0.64) - 0.8).abs() < 1e-6);
    assert!((curve.evaluate(0.70068) - 0.8).abs() < 1e-6);
    assert!((curve.evaluate(0.94951) - 1.0).abs() < 1e-6);
    assert!((curve.evaluate(1.0) - 1.0).abs() < 1e-6);
}

#[test]
fn hermite_opponent_draw_pauses_then_launches_monotonically() {
    let curve = opponent_ease();
    assert!((curve.evaluate(0.0) - 0.0).abs() < 1e-6);
    assert!((curve.evaluate(0.04998) - 0.0).abs() < 1e-6);
    let early = curve.evaluate(0.15);
    assert!(early > 0.1, "sharp launch after pause: {early}");
    assert!((curve.evaluate(1.0) - 1.0).abs() < 1e-6);
}

#[test]
fn spline_interpolates_control_nodes_and_preserves_transform_across_flip() {
    let eye = Vec3::new(0.0, 0.0, 1200.0);
    let from = Pose {
        position: Vec3::new(-300.0, -200.0, 0.0),
        rotation: Quat::IDENTITY,
        scale: 0.625,
        turn: 1.0,
        ..Pose::default()
    };
    let to = Pose {
        position: Vec3::new(0.0, -150.0, 900.0),
        rotation: Quat::IDENTITY,
        scale: 0.25,
        turn: 0.0,
        ..Pose::default()
    };
    let hold = Vec3::new(200.0, -50.0, 950.0);
    let nodes = [
        SplineNode {
            position: Vec3::new(-150.0, -80.0, 300.0),
            rotation: Quat::from_rotation_x(PI * 1.15),
            seen_scale: 0.00055,
            weight: 1.192,
        },
        SplineNode {
            position: Vec3::new(40.0, 10.0, 600.0),
            rotation: Quat::from_rotation_x(-0.6),
            seen_scale: 0.00065,
            weight: 1.114,
        },
        SplineNode {
            position: Vec3::new(140.0, 20.0, 850.0),
            rotation: Quat::from_euler(glam::EulerRot::YXZ, 0.3, -0.2, -0.35),
            seen_scale: 0.00080,
            weight: 0.992,
        },
        SplineNode {
            position: hold,
            rotation: Quat::IDENTITY,
            seen_scale: 0.00108,
            weight: 0.771,
        },
    ];
    let spline = Spline::new(
        &nodes,
        1.239,
        &to,
        local_ease(),
        (true, false),
        (eye, Vec3::Z),
    );

    // At hold plateau (u = 0.8), card rests at interior node 3 (node 4 of 6).
    let at_hold = spline.pose_at(&from, &to, 0.64);
    assert!(at_hold.position.distance(hold) < 0.5);
    assert!(at_hold.turn < 0.5);

    // Before edge-on it is face down (turn == 1.0); after edge-on it is face up (turn == 0.0).
    let early = spline.pose_at(&from, &to, 0.10);
    assert!(early.turn > 0.5);
    let late = spline.pose_at(&from, &to, 0.45);
    assert!(late.turn < 0.5);

    // Mid-flight resize continuity (`through`) hits target exactly and lands at `to`.
    let mid_t = 0.35;
    let current = spline.pose_at(&from, &to, mid_t);
    let shifted_to = Pose {
        position: Vec3::new(20.0, -140.0, 910.0),
        scale: 0.28,
        ..to
    };
    let mut adjusted = spline;
    let mut adjusted_from = from;
    adjusted.through(&mut adjusted_from, mid_t, current);
    let kept = adjusted.pose_at(&adjusted_from, &shifted_to, mid_t);
    assert!(kept.position.distance(current.position) < 1e-2);
    assert!((kept.scale - current.scale).abs() < 1e-3);
    let eff_kept = effective_rotation(&kept);
    let eff_cur = effective_rotation(&current);
    assert!(eff_kept.dot(eff_cur).abs() > 1.0 - 1e-4);

    let end = adjusted.pose_at(&adjusted_from, &shifted_to, 1.0);
    assert!(end.position.distance(shifted_to.position) < 1e-2);
    assert!((end.scale - shifted_to.scale).abs() < 1e-3);
}

#[test]
fn packed_hermite_preserves_endpoint_and_interior_slopes() {
    let curves = [
        // 3-key curve with fast launch out_slope and brisk arrival in_slope.
        HermiteCurve {
            keys: [
                HermiteKey {
                    time: 0.0,
                    value: 0.0,
                    in_slope: 0.0,
                    out_slope: 1.75352,
                },
                HermiteKey {
                    time: 0.91507,
                    value: 1.0,
                    in_slope: 1.57319,
                    out_slope: 0.0,
                },
                HermiteKey {
                    time: 1.0,
                    value: 1.0,
                    in_slope: 0.0,
                    out_slope: 0.0,
                },
                HermiteKey::default(),
            ],
            len: 3,
        },
        // 4-key curve with non-zero interior and final slopes.
        HermiteCurve {
            keys: [
                HermiteKey {
                    time: 0.0,
                    value: 0.0,
                    in_slope: 0.0,
                    out_slope: 0.0,
                },
                HermiteKey {
                    time: 0.04896,
                    value: 0.0,
                    in_slope: 0.0,
                    out_slope: 0.0,
                },
                HermiteKey {
                    time: 0.53541,
                    value: 0.52125,
                    in_slope: 0.61037,
                    out_slope: 0.55090,
                },
                HermiteKey {
                    time: 0.93205,
                    value: 1.0,
                    in_slope: 1.92952,
                    out_slope: 0.0,
                },
            ],
            len: 4,
        },
    ];
    for curve in curves {
        let (packed, four) = PackedHermite::pack(curve);
        let unpacked = packed.unpack(four);
        for i in 0..=20 {
            #[expect(clippy::cast_precision_loss, reason = "i is 0..=20")]
            let t = i as f32 / 20.0;
            let expected = curve.evaluate(t);
            let actual = unpacked.evaluate(t);
            assert!(
                (expected - actual).abs() < 0.005,
                "at t={t}: expected {expected}, got {actual}"
            );
        }
    }
}

#[test]
fn zero_interior_node_spline_interpolates_smoothly_with_wide_weights() {
    let eye = Vec3::new(0.0, 0.0, 1200.0);
    let from = Pose {
        position: Vec3::new(-200.0, -300.0, 800.0),
        rotation: Quat::IDENTITY,
        scale: 0.4,
        ..Pose::default()
    };
    let to = Pose {
        position: Vec3::new(200.0, 100.0, 0.0),
        rotation: Quat::IDENTITY,
        scale: 0.8,
        ..Pose::default()
    };
    let ease = HermiteCurve {
        keys: [
            HermiteKey {
                time: 0.0,
                value: 0.0,
                in_slope: 0.0,
                out_slope: 0.0,
            },
            HermiteKey {
                time: 0.94951,
                value: 1.0,
                in_slope: 2.0,
                out_slope: 0.0,
            },
            HermiteKey {
                time: 1.0,
                value: 1.0,
                in_slope: 0.0,
                out_slope: 0.0,
            },
            HermiteKey::default(),
        ],
        len: 3,
    };
    let spline = Spline::new(&[], 2.33, &to, ease, (false, false), (eye, Vec3::Z));
    let start = spline.pose_at(&from, &to, 0.0);
    let mid = spline.pose_at(&from, &to, 0.5);
    let end = spline.pose_at(&from, &to, 1.0);
    assert!(start.position.distance(from.position) < 1e-2);
    assert!(end.position.distance(to.position) < 1e-2);
    // Ease-in curve stays closer to start at t = 0.5.
    assert!(mid.position.distance(from.position) < mid.position.distance(to.position));

    // Log2 weight packing round-trips exact 1.0 and wide weights in [0.25, 4.0].
    let weights = pack_weights([0.44, 1.0, 1.64, 2.33, 0.25]);
    assert!((unpack_weight(weights, 0) - 0.44).abs() < 0.02);
    assert!((unpack_weight(weights, 1) - 1.0).abs() < 1e-6);
    assert!((unpack_weight(weights, 2) - 1.64).abs() < 0.05);
    assert!((unpack_weight(weights, 3) - 2.33).abs() < 0.07);
    assert!((unpack_weight(weights, 4) - 0.25).abs() < 0.01);
}
