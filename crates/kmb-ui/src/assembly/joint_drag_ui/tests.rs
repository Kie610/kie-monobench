use super::*;

#[test]
fn only_plain_numbers_are_dragged() {
    assert!(plain_number("30 deg", "deg"));
    assert!(plain_number("-12.5 mm", "mm"));
    assert!(plain_number("0", "deg"));
    assert!(!plain_number("hinge", "deg"));
    assert!(!plain_number("hinge + 5 deg", "deg"));
}

#[test]
fn angles_wrap_into_a_half_turn() {
    assert_eq!(wrap(190.0), -170.0);
    assert_eq!(wrap(-190.0), 170.0);
    assert_eq!(wrap(180.0), 180.0);
    assert_eq!(value_text(29.96, "deg"), "30 deg");
    assert_eq!(value_text(-0.04, "mm"), "0 mm");
    assert_eq!(offset_text([12.34, 0.0, -4.0]), "X 12.3  Y 0  Z -4 mm");
}

/// 画面の角度: 右から下へ(画面の y は下向き)が正。半周を越える前に符号が変わる。
#[test]
fn screen_turns_follow_the_cursor_around_the_pivot() {
    let c = Pos2::new(100.0, 100.0);
    let right = Pos2::new(150.0, 100.0);
    assert!((screen_turn(c, right, Pos2::new(100.0, 150.0)) - 90.0).abs() < 1e-6);
    assert!((screen_turn(c, right, Pos2::new(100.0, 50.0)) + 90.0).abs() < 1e-6);
    // 円を一周すると、コマごとの差の合計が 360 度(大きさは関係ない)
    let mut sum = 0.0;
    let mut last = right;
    for i in 1..=36 {
        let a = (i as f32 * 10.0).to_radians();
        let p = c + egui::vec2(a.cos(), a.sin()) * (30.0 + i as f32);
        sum += screen_turn(c, last, p);
        last = p;
    }
    assert!((sum - 360.0).abs() < 1e-3, "{sum}");
}

use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use serde_json::json;

/// 30×20 の板を 2 枚(高さ 10 と、x = 40 の所に高さ 2)。
fn two_plates() -> Workspace {
    let mut b = DocBuilder::new();
    for (x, t, name) in [(0.0, "10 mm", "箱"), (40.0, "2 mm", "蓋")] {
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(x, 0.0);
        s.rectangle(o, "30 mm", "20 mm", [x, 0.0, x + 30.0, 20.0]);
        let sk = b.feature(kmb_sketch::SKETCH, &format!("{name}のスケッチ"), s.build());
        b.feature(
            EXTRUDE,
            name,
            Extrude {
                profile: Some(sk),
                distance: t.into(),
                ..Default::default()
            },
        );
    }
    let mut ws = Workspace::new(b.finish("板"), None, kmb_bundle::modules());
    ws.wait();
    ws
}

/// ジョイントの無い部品を引いて動かす: 部品の木の部品は自由に引け(部品に入っていないボディは引けない)、
/// 引くと「位置」の機能が入って帯が出る。決めずにコマンドを始めると止めて聞き、「元に戻す」なら
/// 引く前に戻してからそのコマンドを始める。「位置を残す」なら機能が残る。取り消せば帯は消える。
#[test]
fn a_dragged_component_asks_to_keep_or_revert_before_the_next_command() {
    let mut ws = two_plates();
    let roots = ws.body_roots();
    let (boxed, lid) = (roots[0].1, roots[1].1);
    crate::assembly::components_tests::make_component(&mut ws, &[lid], "蓋の部品", None)
        .expect("作れる");
    ws.wait();
    let face = |f| kmb_core::FacePick {
        feature: f,
        face: "f".into(),
    };
    let drag_of = |ws: &Workspace, f| ws.win.extensions.drag(&ws.context(), &face(f));
    assert!(
        drag_of(&ws, boxed).is_none(),
        "部品に入っていないボディは範囲で選ぶ"
    );
    let positions = |ws: &Workspace| {
        ws.cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .filter(|e| e.type_id == "asm.position@1")
            .count()
    };
    let drag = |ws: &mut Workspace, offset: [f64; 3]| {
        let d = drag_of(ws, lid).expect("蓋の部品は引ける");
        assert_eq!(d.motion, kmb_core::DragMotion::Free);
        let mut j = ws.free_drag(d, [0.0; 3], egui::Pos2::ZERO, [0.0, 0.0, -1.0]);
        // 2 コマ引いて(コマごとに戻してから入れ直す)離す
        j.offset = [1.0, 0.0, 0.0];
        ws.cur.joint_drag = Some(j);
        ws.joint_drag_apply();
        ws.cur.joint_drag.as_mut().unwrap().offset = offset;
        ws.joint_drag_apply();
        ws.joint_drag_end();
        ws.wait();
    };
    let depth0 = ws.cur.work.doc().undo_depth();
    drag(&mut ws, [10.0, 0.0, 0.0]);
    assert_eq!(positions(&ws), 1);
    assert_eq!(
        ws.cur.work.doc().undo_depth(),
        depth0 + 1,
        "1 回引くと 1 つの取り消し単位"
    );
    assert!(ws.moved_pending());
    // 決めずに干渉の確認を始めると、止めて聞く
    assert!(ws.start_action("asm.interference").is_none());
    assert_eq!(
        ws.cur.moved_ask,
        Some(Held::Action("asm.interference".into()))
    );
    assert_eq!(positions(&ws), 1, "聞いている間は変えない");
    // 「やめる」: 何もしない
    ws.answer_moved(None);
    assert!(ws.cur.moved_ask.is_none() && ws.moved_pending());
    // 「元に戻す」: 引く前に戻してから始める
    ws.start_action("asm.interference");
    ws.answer_moved(Some(false));
    ws.wait();
    assert_eq!(positions(&ws), 0);
    assert_eq!(ws.cur.work.doc().undo_depth(), depth0);
    assert!(!ws.moved_pending());
    // 2 回続けて引くと 1 つの位置の機能にまとまり、「位置を残す」で残る
    drag(&mut ws, [10.0, 0.0, 0.0]);
    drag(&mut ws, [0.0, 5.0, 0.0]);
    assert_eq!(positions(&ws), 1);
    let p = ws
        .cur
        .work
        .doc()
        .data()
        .features
        .iter()
        .find(|e| e.type_id == "asm.position@1")
        .unwrap()
        .data
        .clone();
    assert_eq!((&p["x"], &p["y"]), (&json!("10 mm"), &json!("5 mm")));
    ws.settle_moved(true);
    assert!(!ws.moved_pending());
    assert_eq!(positions(&ws), 1);
    // もう一度引いて、取り消すと帯は消える(ほかの変更をしたら残したとみなす)
    drag(&mut ws, [0.0, 0.0, 3.0]);
    assert!(ws.moved_pending());
    ws.undo();
    ws.wait();
    assert!(!ws.moved_pending());
    // 機能を直し始める時(二度押し)も先に聞き、残せば直し始める
    drag(&mut ws, [0.0, 0.0, 3.0]);
    let first = ws.cur.work.doc().data().features[0].id;
    ws.open_edit(first);
    assert_eq!(ws.cur.moved_ask, Some(Held::Edit(first)));
    assert_ne!(ws.cur.editing, Some(first));
    ws.answer_moved(Some(true));
    assert_eq!(ws.cur.editing, Some(first));
    assert!(!ws.moved_pending());
    assert!(
        ws.win.message.is_none(),
        "引いても下の帯の文は出さない: {:?}",
        ws.win.message
    );
}

/// 箱と、円柱のジョイント(軸は Z、蓋の真ん中 (55, 10) を通る)でつないだ蓋。
fn cylinder(extra: serde_json::Value) -> (Workspace, kmb_core::FeatureId, kmb_core::FeatureId) {
    let mut b = DocBuilder::new();
    let mut ids = Vec::new();
    for (x, t, name) in [(0.0, "10 mm", "箱"), (40.0, "2 mm", "蓋")] {
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(x, 0.0);
        s.rectangle(o, "30 mm", "20 mm", [x, 0.0, x + 30.0, 20.0]);
        let sk = b.feature(kmb_sketch::SKETCH, &format!("{name}のスケッチ"), s.build());
        ids.push(b.feature(
            EXTRUDE,
            name,
            Extrude {
                profile: Some(sk),
                distance: t.into(),
                ..Default::default()
            },
        ));
    }
    let mut joint = json!({
        "kind": "cylindrical",
        "moving": {"kind": "body", "root": ids[1].0},
        "base": {"kind": "body", "root": ids[0].0},
        "bodies": [ids[1].0],
        "origin": [55.0, 10.0, 0.0],
        "axis": [0.0, 0.0, 1.0],
    });
    for (k, v) in extra.as_object().unwrap() {
        joint[k] = v.clone();
    }
    b.feature("asm.joint@1", "ジョイント1", joint);
    let mut ws = Workspace::new(b.finish("円柱"), None, kmb_bundle::modules());
    ws.wait();
    let j = ws.cur.work.doc().data().features.last().unwrap().id;
    (ws, j, ids[1])
}

/// 円柱のジョイントの部品を引くと、押した点がカーソルに付いて、回りながら軸の向きにも動く。
/// 下限・上限で止まり、式で決めた欄は変えず、離すと戻る位置に戻る。
/// 1 回引くと 1 つの取り消し単位。
#[test]
fn a_cylindrical_part_turns_and_slides_with_the_cursor() {
    let field = |ws: &Workspace, j, k: &str| {
        ws.cur.work.doc().data().feature(j).unwrap().data[k]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    // 蓋の右の縁の点 (70, 10, 2) を、軸のまわりに 90 度回して 5 mm 上げた (55, 25, 7) へ、斜めの視線で
    let grab = [70.0, 10.0, 2.0];
    let dir = k::normalize([-0.5, -1.0, -0.7]);
    let path = |(z0, z1): (f64, f64)| -> Vec<([f64; 3], [f64; 3])> {
        (1..=12)
            .map(|i| {
                let t = f64::from(i) / 12.0;
                let a = t * std::f64::consts::FRAC_PI_2;
                let p = [
                    55.0 + 15.0 * a.cos(),
                    10.0 + 15.0 * a.sin(),
                    z0 + t * (z1 - z0),
                ];
                (k::sub(p, k::scale(dir, 200.0)), dir)
            })
            .collect()
    };
    let drag_of = |ws: &Workspace, lid| {
        ws.win
            .extensions
            .drag(
                &ws.context(),
                &kmb_core::FacePick {
                    feature: lid,
                    face: "f".into(),
                },
            )
            .expect("蓋は引ける")
    };
    let (mut ws, j, lid) = cylinder(json!({}));
    let d = drag_of(&ws, lid);
    assert_eq!(d.motion, DragMotion::Follow);
    let depth = ws.cur.work.doc().undo_depth();
    assert!(ws.drag_follow_to(d, grab, &path((2.0, 7.0))));
    ws.wait();
    assert_eq!(
        (field(&ws, j, "angle"), field(&ws, j, "distance")),
        ("90 deg".into(), "5 mm".into())
    );
    assert_eq!(
        ws.cur.work.doc().undo_depth(),
        depth + 1,
        "1 回引くと 1 つの取り消し単位"
    );
    assert!(ws.moved_pending());
    // 距離の上限 3 mm で止まる
    let (mut ws, j, lid) = cylinder(json!({"distance_max": "3 mm"}));
    let d = drag_of(&ws, lid);
    ws.drag_follow_to(d, grab, &path((2.0, 7.0)));
    ws.wait();
    assert_eq!(field(&ws, j, "distance"), "3 mm");
    assert_eq!(field(&ws, j, "angle"), "90 deg");
    // 距離を式で決めていれば距離は変えず、角度だけ付いてくる
    let (mut ws, j, lid) = cylinder(json!({"distance": "1 mm + 1 mm"}));
    let d = drag_of(&ws, lid);
    ws.drag_follow_to(d, [70.0, 10.0, 4.0], &path((4.0, 4.0)));
    ws.wait();
    assert_eq!(field(&ws, j, "distance"), "1 mm + 1 mm");
    assert_eq!(field(&ws, j, "angle"), "90 deg");
    // 距離の戻る位置 0 mm: 離すと距離だけ戻る
    let (mut ws, j, lid) = cylinder(json!({"distance_rest": "0 mm"}));
    let d = drag_of(&ws, lid);
    ws.drag_follow_to(d, grab, &path((2.0, 7.0)));
    ws.wait();
    assert_eq!(
        (field(&ws, j, "angle"), field(&ws, j, "distance")),
        ("90 deg".into(), "0 mm".into())
    );
}

/// 回転の回し方: 軸を横から見ている時(蝶番を正面から)は、円の手前の点の動く向きへ引いた長さで
/// 回し(-90 度を越えても伸び続ける)、押した点がカーソルについて来る向き。軸がこちらを向いている
/// 時は、軸の点のまわりにカーソルを回した角度で回す。
#[test]
fn the_turn_follows_the_view_of_the_axis() {
    let mut ws = two_plates();
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
    let (origin, axis) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let ray_dir = |ws: &Workspace, p: Pos2| k::normalize(f64s(ws.cur.camera.ray(p, rect).1));
    let project = |ws: &Workspace, p: [f64; 3]| ws.cur.camera.project(p.map(|c| c as f32), rect).0;
    ws.cur.camera.set_pivot([0.0, 0.0, 0.0]);
    // 正面(-Y の側)から見る: X の軸は画面の中、円は縦の線に潰れる
    ws.cur.camera.face([0.0, -1.0, 0.0]);
    for grab in [[0.0, -20.0, 0.0], [0.0, 20.0, 0.0], [0.0, 0.0, 20.0]] {
        let press = project(&ws, grab);
        let turn = ws.turn_for(origin, axis, grab, ray_dir(&ws, press), press, rect);
        let Turn::Along { per_rad } = turn else {
            panic!("{turn:?}")
        };
        // 20 mm の半径の 1 ラジアン分は、画面で 20 mm 分の画素
        let px_per_mm = (project(&ws, [0.0, 0.0, 1.0]) - project(&ws, [0.0, 0.0, 0.0])).length();
        assert!(
            (per_rad.length() / (20.0 * px_per_mm) - 1.0).abs() < 0.05,
            "{per_rad:?} {px_per_mm}"
        );
        // 押した点を少し正に回すと、per_rad の向きへ動く(手前でも奥でも、カーソルについて来る)
        let r = k::rotation(origin, axis, 0.1);
        let step = project(&ws, k::transform_point(&r, grab)) - press;
        if step.length() > 0.5 {
            assert!(step.dot(per_rad) > 0.0, "{grab:?} {step:?} {per_rad:?}");
        }
    }
    // 軸の向き(+X の側)から見る: 軸の点のまわりに回す
    ws.cur.camera.face([1.0, 0.0, 0.0]);
    let grab = [0.0, -20.0, 0.0];
    let press = project(&ws, grab);
    let turn = ws.turn_for(origin, axis, grab, ray_dir(&ws, press), press, rect);
    let Turn::Around { pivot, .. } = turn else {
        panic!("{turn:?}")
    };
    assert!((pivot - project(&ws, origin)).length() < 1.0);
    // 軸のすぐ近くを押した時は、引いた長さで回す
    let grab = [0.0, -0.5, 0.0];
    let press = project(&ws, grab);
    let turn = ws.turn_for(origin, axis, grab, ray_dir(&ws, press), press, rect);
    assert!(matches!(turn, Turn::Along { .. }), "{turn:?}");
}

/// 回転のジョイントを引く間は形を作り直さず、3D のビューのボディだけが動く。離すと 1 回だけ欄を直し、
/// 計算が終わると見せていた動きを外す(10-23 a1)。
#[test]
fn turning_a_joint_moves_only_the_view_until_released() {
    let (mut ws, j, lid) = cylinder(json!({"kind": "revolute"}));
    let d = ws
        .win
        .extensions
        .drag(
            &ws.context(),
            &kmb_core::FacePick {
                feature: lid,
                face: "f".into(),
            },
        )
        .expect("蓋は引ける");
    assert!(matches!(d.motion, DragMotion::Rotate { .. }));
    let depth = ws.cur.work.doc().undo_depth();
    let stamp = ws.report().stamp();
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut jd = ws
        .field_drag(d, [70.0, 10.0, 2.0], Pos2::new(400.0, 300.0), rect)
        .expect("引ける");
    for v in [10.0, 30.0, 45.0] {
        jd.value = v;
        ws.cur.joint_drag = Some(jd);
        ws.joint_drag_show();
        ws.sync_drag_pose();
        jd = ws.cur.joint_drag.take().unwrap();
        assert_eq!(
            ws.cur.work.doc().undo_depth(),
            depth,
            "引く間は文書を変えない"
        );
        assert_eq!(ws.report().stamp(), stamp, "引く間は計算し直さない");
        assert!(ws.cur.scene.is_posed(), "ビューのボディは動く");
    }
    ws.cur.joint_drag = Some(jd);
    ws.joint_drag_end();
    ws.wait();
    ws.sync_drag_pose();
    assert_eq!(
        ws.cur.work.doc().data().feature(j).unwrap().data["angle"],
        json!("45 deg")
    );
    assert_eq!(
        ws.cur.work.doc().undo_depth(),
        depth + 1,
        "1 つの取り消し単位"
    );
    assert!(!ws.cur.scene.is_posed(), "形が直ったら見せていた動きを外す");
    assert!(ws.cur.drag_pose.is_none());
}
