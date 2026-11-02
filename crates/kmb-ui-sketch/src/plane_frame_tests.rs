//! 確かめること: 出現の回転を持つ平面と、正面へ戻る視点の X・Y の向き。
use crate::SketchEditor;
use crate::tests::{editor, reg, ws_of};
use egui_kittest::Harness;
use kmb_core::{Edit, FeatureId, FeatureStatus, Plane, Profile};
use kmb_sketch::{FaceOrigin, Sketch, SketchPlane};
use kmb_ui::{Workspace, view3d::Camera};
use serde_json::json;

fn sketch_on(feature: FeatureId, face: &str) -> Sketch {
    let mut s = Sketch {
        plane: SketchPlane::Face {
            feature,
            face: face.into(),
            origin: FaceOrigin::World,
        },
        ..Default::default()
    };
    let ids = [[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]].map(|p| s.add_point(p[0], p[1]));
    for i in 0..4 {
        s.add_line(ids[i], ids[(i + 1) % 4]);
    }
    s
}

fn plane_of(ws: &Workspace, id: FeatureId) -> Plane {
    let Some(FeatureStatus::Ok { outputs, .. }) = ws.report().feature(id) else {
        panic!(
            "平面を持つスケッチを計算する: {:?}",
            ws.report().feature(id)
        );
    };
    outputs
        .get("profile")
        .and_then(Profile::from_value)
        .unwrap()
        .plane
}

fn goal_camera(cam: Camera, rect: egui::Rect) -> Camera {
    let (center, yaw, pitch, height) = cam.snapshot(rect);
    let roll = cam.tilt_deg();
    let mut goal = cam;
    goal.restore_now(center, yaw, pitch, roll, height, rect);
    goal
}

#[test]
fn looking_front_on_a_rotated_occurrence_keeps_sketch_x_right_and_y_up() {
    let part = kmb_samples::reference_part();
    let root = part.plate;
    let mut doc = part.doc;
    let face = format!("f{}:end", root.0);
    let base = doc
        .apply_edits(
            "元の平面",
            vec![Edit::Insert {
                type_id: kmb_sketch::SKETCH.into(),
                name: "元の面の輪郭".into(),
                data: serde_json::to_value(sketch_on(root, &face)).unwrap(),
            }],
        )
        .unwrap()[0];
    let (st, ct) = 37.0f64.to_radians().sin_cos();
    let (sp, cp) = 28.0f64.to_radians().sin_cos();
    let x0 = [ct, st, 0.0];
    let y0 = [-st * cp, ct * cp, sp];
    let (sr, cr) = 41.0f64.to_radians().sin_cos();
    let x = [0, 1, 2].map(|i| x0[i] * cr + y0[i] * sr);
    let y = [0, 1, 2].map(|i| -x0[i] * sr + y0[i] * cr);
    let z = [st * sp, -ct * sp, cp];
    let matrix = [
        x[0], y[0], z[0], 80.0, x[1], y[1], z[1], 0.0, x[2], y[2], z[2], 20.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let occurrence = doc
        .apply_edits(
            "回した出現",
            vec![Edit::Insert {
                type_id: "asm.occurrence@1".into(),
                name: "回した部品".into(),
                data: json!({"of":root.0,"place":matrix}),
            }],
        )
        .unwrap()[0];
    let id = doc
        .apply_edits(
            "出現の面のスケッチ",
            vec![Edit::Insert {
                type_id: kmb_sketch::SKETCH.into(),
                name: "回した面の輪郭".into(),
                data: serde_json::to_value(sketch_on(occurrence, &face)).unwrap(),
            }],
        )
        .unwrap()[0];
    let mut ws = ws_of(doc, None, reg());
    ws.open_edit(id);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.run();
    assert_eq!(h.state().report().error_count(), 0);
    let before = plane_of(h.state(), base);
    let plane = plane_of(h.state(), id);
    let rotate = |p: [f64; 3]| [0, 1, 2].map(|i| x[i] * p[0] + y[i] * p[1] + z[i] * p[2]);
    let actual = plane.normal();
    let expected = rotate(before.normal());
    assert!(
        (0..3).all(|i| (actual[i] - expected[i]).abs() < 1e-8),
        "出現の回転が平面の法線に入る: {actual:?} / {expected:?}"
    );
    assert_front(&mut h, plane);
}

fn assert_front(h: &mut Harness<'static, Workspace>, plane: Plane) {
    let e = h.state().editor::<SketchEditor>().unwrap();
    assert!(e.view.space.is_none(), "開いた時は平らな図");
    let t = e.view.last.as_ref().unwrap();
    let o = t.to_screen([0.0, 0.0]);
    let x = t.to_screen([10.0, 0.0]) - o;
    let y = t.to_screen([0.0, 10.0]) - o;
    assert!(
        x.x > 0.0 && x.y.abs() < 0.01 && y.x.abs() < 0.01 && y.y < 0.0,
        "開いた図のXは右、Yは上"
    );
    let c = editor(h.state_mut())
        .view
        .last
        .as_ref()
        .unwrap()
        .center_screen;
    crate::space_view_tests::right_drag(h, c, c + egui::vec2(90.0, 50.0));
    let batch = h.state_mut().run_script("click \"正面から見る\"").unwrap();
    let start = std::time::Instant::now();
    let mut checked = false;
    loop {
        h.step();
        let e = h.state().editor::<SketchEditor>().unwrap();
        if !checked
            && e.view.to_front
            && let Some(cam) = e.view.space
        {
            let rect = e.view.seen.unwrap().1;
            let goal = goal_camera(cam, rect);
            let (right, up, _) = goal.basis_pub();
            let dot =
                |a: [f32; 3], b: [f64; 3]| (0..3).map(|i| f64::from(a[i]) * b[i]).sum::<f64>();
            assert!(
                dot(right, plane.x_axis) > 0.99999,
                "正面の画面の右がスケッチのXに合う: {right:?} / {:?}",
                plane.x_axis
            );
            assert!(
                dot(up, plane.y_axis) > 0.99999,
                "正面の画面の上がスケッチのYに合う"
            );
            checked = true;
        }
        if let Some(r) = h.state_mut().script_result(batch) {
            assert!(r.ok, "{}", r.report());
            break;
        }
        assert!(start.elapsed().as_secs() < 15, "視点の台本の時間切れ");
    }
    assert!(checked, "正面へ戻る途中の視点を確認する");
}

#[test]
fn looking_front_on_a_tilted_datum_plane_keeps_its_declared_x_and_y() {
    let (st, ct) = 37.0f64.to_radians().sin_cos();
    let (sp, cp) = 28.0f64.to_radians().sin_cos();
    let (sr, cr) = 41.0f64.to_radians().sin_cos();
    let x0 = [ct, st, 0.0];
    let y0 = [-st * cp, ct * cp, sp];
    let x = [0, 1, 2].map(|i| x0[i] * cr + y0[i] * sr);
    let y = [0, 1, 2].map(|i| -x0[i] * sr + y0[i] * cr);
    let origin = [3.0, 4.0, 5.0];
    let px = [0, 1, 2].map(|i| origin[i] + 20.0 * x[i]);
    let py = [0, 1, 2].map(|i| origin[i] + 20.0 * y[i]);
    let mut doc = kmb_core::Document::new();
    let datum=doc.apply_edits("斜めの作業平面",vec![Edit::Insert {type_id:"cad.datum_plane@1".into(),name:"Xも回した平面".into(),data:json!({"method":"three_points","points":[{"kind":"at","at":origin},{"kind":"at","at":px},{"kind":"at","at":py}]})}]).unwrap()[0];
    let mut sketch = sketch_on(FeatureId(0), "unused");
    sketch.plane = SketchPlane::Datum { feature: datum };
    let id = doc
        .apply_edits(
            "平面のスケッチ",
            vec![Edit::Insert {
                type_id: kmb_sketch::SKETCH.into(),
                name: "斜めの平面の輪郭".into(),
                data: serde_json::to_value(sketch).unwrap(),
            }],
        )
        .unwrap()[0];
    let mut ws = ws_of(doc, None, reg());
    ws.open_edit(id);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.run();
    assert_eq!(h.state().report().error_count(), 0);
    let plane = plane_of(h.state(), id);
    assert_front(&mut h, plane);
}
