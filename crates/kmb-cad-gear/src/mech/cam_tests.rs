//! 確かめること: 4法則の正しい端値・微分、区間の読み取り、閉じた輪郭、基礎円への接触、注意と諸元の互換。

use super::*;

fn program(law: MotionLaw) -> Vec<CamSegment> {
    vec![
        CamSegment {
            span: 2.0 * PI / 3.0,
            lift: 10.0,
            law,
        },
        CamSegment {
            span: PI / 3.0,
            lift: 0.0,
            law: MotionLaw::Uniform,
        },
        CamSegment {
            span: 2.0 * PI / 3.0,
            lift: -10.0,
            law,
        },
        CamSegment {
            span: PI / 3.0,
            lift: 0.0,
            law: MotionLaw::Uniform,
        },
    ]
}
fn input(follower: Follower, tip: FollowerTip) -> ShapeInput {
    ShapeInput {
        base: 20.0,
        roller: 2.0,
        offset: 0.0,
        follower,
        tip,
        arm: 30.0,
        pivot: if tip == FollowerTip::Flat {
            [60.0, 0.0]
        } else {
            [40.0, 0.0]
        },
        rotation: Rotation::Ccw,
        segments: if follower == Follower::Oscillating {
            program(MotionLaw::Cycloidal)
                .into_iter()
                .map(|s| CamSegment {
                    lift: s.lift.to_radians(),
                    ..s
                })
                .collect()
        } else {
            program(MotionLaw::Cycloidal)
        },
        rest: 0.0,
    }
    .prepare()
    .unwrap()
}
#[test]
fn laws_keep_their_endpoint_values_and_physical_endpoint_speeds() {
    for law in [
        MotionLaw::Uniform,
        MotionLaw::SimpleHarmonic,
        MotionLaw::Cycloidal,
        MotionLaw::Polynomial345,
    ] {
        assert_eq!(law.ratio(0.0), 0.0);
        assert!((law.ratio(1.0) - 1.0).abs() < 1e-12);
        let expected = if law == MotionLaw::Uniform { 1.0 } else { 0.0 };
        assert!((derivative(law, 0.0) - expected).abs() < 1e-12);
        assert!((derivative(law, 1.0) - expected).abs() < 1e-12);
        for u in [0.2, 0.5, 0.8] {
            let h = 1e-5;
            let d = (law.ratio(u + h) - law.ratio(u - h)) / (2.0 * h);
            assert!((d - derivative(law, u)).abs() < 1e-7);
        }
    }
    assert!(second(MotionLaw::Cycloidal, 0.0).abs() < 1e-12);
    assert!(second(MotionLaw::Cycloidal, 1.0).abs() < 1e-12);
}
#[test]
fn a_diagram_requires_known_laws_a_full_turn_and_a_return_to_zero() {
    let parsed = terms(&diagram_default()).unwrap();
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0].span, "120 deg");
    assert_eq!(parsed[0].lift.as_deref(), Some("10 mm"));
    assert_eq!(parsed[2].direction, -1);
    assert!(parsed[2].lift.is_none());
    assert_eq!(parsed[2].render(), "下がり 120 deg 単弦");
    assert!(terms("上がり 120 deg 10 mm unknown").is_err());
    assert!(terms("上がり 120 10 サイクロイド").is_err());
    assert!(validate_program(&program(MotionLaw::Cycloidal)).is_ok());
    let mut bad = program(MotionLaw::Cycloidal);
    bad[0].span = 1.0;
    assert!(validate_program(&bad).is_err());
    let mut bad = program(MotionLaw::Cycloidal);
    bad[2].lift = 0.0;
    assert!(validate_program(&bad).is_err());
}
#[test]
fn all_follower_tips_start_on_the_base_circle() {
    for follower in [Follower::Translating, Follower::Oscillating] {
        for tip in [FollowerTip::Knife, FollowerTip::Roller, FollowerTip::Flat] {
            let p = input(follower, tip);
            assert!(
                (norm(p.point(0.0)) - 20.0).abs() < 2e-5,
                "{follower:?} {tip:?} {:?}",
                p.point(0.0)
            );
            for t in [0.0, PI / 3.0, PI, 5.0 * PI / 3.0] {
                assert!(p.point(t).iter().all(|v| v.is_finite()));
            }
        }
    }
}
#[test]
fn the_biarc_cam_and_the_follower_are_closed_and_have_positive_areas() {
    for tip in [FollowerTip::Knife, FollowerTip::Roller, FollowerTip::Flat] {
        let p = input(Follower::Translating, tip);
        let (cam, follower, _) = outlines(&p).unwrap();
        for ring in [&cam, &follower] {
            assert!(ring.signed_area() > 0.0);
            for i in 0..ring.edges.len() {
                assert!(
                    norm(sub(
                        ring.edges[i].to_point(),
                        ring.edges[(i + 1) % ring.edges.len()].from_point()
                    )) < 1e-6,
                    "{tip:?} {i}"
                );
            }
        }
    }
}
#[test]
fn a_steep_translating_roller_reports_pressure_angle_and_uniform_reports_speed_jumps() {
    let mut p = input(Follower::Translating, FollowerTip::Roller);
    p.segments[0].span = PI / 6.0;
    p.segments[1].span = 5.0 * PI / 6.0;
    validate_program(&p.segments).unwrap();
    let (_, _, warnings) = outlines(&p).unwrap();
    assert!(
        warnings.iter().any(|s| s.contains("圧力角")),
        "{warnings:?}"
    );
    let mut p = input(Follower::Translating, FollowerTip::Knife);
    p.segments = program(MotionLaw::Uniform);
    let (_, _, warnings) = outlines(&p).unwrap();
    assert!(warnings.iter().any(|s| s.contains("速度")), "{warnings:?}");
}
#[test]
fn clockwise_mirrors_a_centered_cam_without_changing_lift() {
    let a = input(Follower::Translating, FollowerTip::Roller);
    let b = ShapeInput {
        rotation: Rotation::Cw,
        ..a.clone()
    };
    for t in [0.0, 0.5, 2.0, 4.0] {
        let p = a.point(t);
        let q = b.point(t);
        assert!((p[0] + q[0]).abs() < 1e-6);
        assert!((p[1] - q[1]).abs() < 1e-6);
    }
}
#[test]
fn serialized_defaults_use_xy_and_parameter_renaming_rewrites_diagram_expressions() {
    let c: Cam = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(c.plane, kmb_cad::plane::PlaneRef::OriginXy);
    let c=Cam{diagram:"上がり 120 deg rise mm サイクロイド, 止まり 60 deg, 下がり 120 deg rise mm 単弦, 止まり 60 deg".into(),..Default::default()};
    assert!(c.inputs().unwrap().contains(&InputRef::Param {
        name: "rise".into()
    }));
    let value = c.rename_param("rise", "stroke").unwrap().unwrap();
    let updated: Cam = serde_json::from_value(value).unwrap();
    assert!(updated.diagram.contains("stroke mm"));
    assert!(!updated.diagram.contains("rise mm"));
    assert!(serde_json::from_value::<Cam>(serde_json::json!({"base_raduis":"20 mm"})).is_err());
}

#[test]
fn the_existing_cam_spec_keeps_all_one_degree_displacements_in_both_directions() {
    let program = program(MotionLaw::Cycloidal);
    for rotation in [Rotation::Ccw, Rotation::Cw] {
        let spec = CamSpec {
            follower: Follower::Translating,
            tip: FollowerTip::Roller,
            base_radius: 20.0,
            roller_radius: 2.0,
            offset: 0.0,
            arm: None,
            segments: sampled_segments(&program, rotation),
            axis: Default::default(),
        };
        assert_eq!(spec.segments.len(), 360);
        for degree in 0..=360 {
            let theta = (degree as f64).to_radians();
            assert!(
                (spec.displacement(theta) - displacement(&program, theta * rotation.sign())).abs()
                    < 1e-8
            );
        }
        let value = spec.to_value();
        assert_eq!(CamSpec::from_value(&value).unwrap(), spec);
    }
}

#[test]
fn cam_action_uses_one_declared_operation_for_both_bodies_and_planar_placement() {
    use kmb_core::{Context, Document, Edit, Extensions, Outcome};
    use std::sync::Arc;
    let mut reg = Registry::new();
    register(&mut reg);
    let mut doc = Document::new();
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(kmb_core::NullKernel);
    let report = doc.recompute(&reg, &*kernel);
    let args =
        serde_json::json!({"plane":"yz","point":[5.0,10.0,20.0],"width":4.0,"follower_width":3.0});
    let mut cx = Context::new(doc.data(), &report, &reg, &kernel);
    cx.args = &args;
    let ext = Extensions(vec![Arc::new(CamCommands)]);
    let Outcome::Change { edits, .. } = ext.run(CAM_ADD, &cx).unwrap() else {
        panic!("カムと従節を追加")
    };
    assert_eq!(edits.len(), 2);
    let Edit::Insert { type_id, data, .. } = &edits[0] else {
        panic!("カム")
    };
    assert_eq!(type_id, CAM);
    let cam: Cam = serde_json::from_value(data.clone()).unwrap();
    assert_eq!(cam.plane, kmb_cad::plane::PlaneRef::OriginYz);
    assert_eq!(cam.x, "10 mm");
    assert_eq!(cam.y, "20 mm");
    assert_eq!(cam.width, "4 mm");
    let Edit::Insert { type_id, data, .. } = &edits[1] else {
        panic!("従節")
    };
    assert_eq!(type_id, FOLLOWER_BODY);
    assert_eq!(data["width"], "3 mm");
    assert_eq!(data["cam"], serde_json::json!({"$new":0}));
}
#[test]
fn cam_action_refuses_unknown_arguments_and_nonpositive_dimensions() {
    use kmb_core::{Context, Document, Extensions};
    use std::sync::Arc;
    let mut reg = Registry::new();
    register(&mut reg);
    let mut doc = Document::new();
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(kmb_core::NullKernel);
    let report = doc.recompute(&reg, &*kernel);
    let ext = Extensions(vec![Arc::new(CamCommands)]);
    for args in [
        serde_json::json!({"bas_radius":20}),
        serde_json::json!({"base_radius":0}),
        serde_json::json!({"width":-1}),
        serde_json::json!({"follower_width":0}),
        serde_json::json!({"diagram":"上がり 120 deg 10 mm unknown"}),
    ] {
        let mut cx = Context::new(doc.data(), &report, &reg, &kernel);
        cx.args = &args;
        assert!(ext.run(CAM_ADD, &cx).is_err(), "{args}");
    }
}
