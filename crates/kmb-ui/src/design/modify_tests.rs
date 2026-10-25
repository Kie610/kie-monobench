//! 画面の試験: ボディを分割すると残りの側も同じ取り消し単位で入り、直すのは分割の方。
//! 移動/コピーは打つまで動かない。

use crate::Workspace;
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use serde_json::json;

/// 30×20×10 の板。
fn plate() -> Workspace {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "30 mm", "20 mm", [0.0, 0.0, 30.0, 20.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "板のスケッチ", s.build());
    b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut ws = Workspace::new(b.finish("板"), None, kmb_bundle::modules());
    ws.wait();
    ws
}

#[test]
fn splitting_adds_the_other_side_too_and_a_move_waits_for_a_value() {
    // 1 つの板で順に確かめる:
    // - 分割は残りの側も同じ取り消し単位で入れ、直すのは分割の方(元 splitting_adds_the_other_side_too_and_edits_the_split)
    // - 移動/コピーは打つまで動かない(元 a_move_does_nothing_until_a_value_is_typed)
    let mut ws = plate();
    let n = ws.cur.work.doc().data().features.len();
    let id = ws.add_described(kmb_cad_modify::SPLIT).expect("足せる");
    ws.wait();
    let data = ws.cur.work.doc().data().clone();
    assert_eq!(data.features.len(), n + 2, "分割と残りの側");
    let rest = data.features.last().unwrap();
    assert_eq!(rest.type_id, kmb_cad_modify::SPLIT_PART);
    assert_eq!(rest.data, json!({"of": id.0}));
    assert_eq!(ws.creating(), Some(id), "直すのは分割の方");
    // 板はただ 1 つのボディなので、ボディの欄に入っている。厚みの半分で 2 つに割れる
    assert_eq!(ws.visible_bodies().len(), 2);
    // キャンセルすると両方とも消える(1 回の取り消し単位)
    ws.cancel_edit();
    assert_eq!(ws.cur.work.doc().data().features.len(), n);

    // キャンセルした後の 1 つのボディの板で、移動を足す
    let (_, h) = ws.visible_bodies()[0];
    let before = ws.cur.work.kernel().bounds(h).unwrap();
    let id = ws.add_described(kmb_cad_modify::MOVE).expect("足せる");
    ws.wait();
    let (_, h) = ws.visible_bodies()[0];
    assert_eq!(
        ws.cur.work.kernel().bounds(h).unwrap(),
        before,
        "選んだだけでは跳ねない"
    );
    let mut data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    data["x"] = json!("12 mm");
    assert!(ws.set_feature_data(id, data));
    ws.wait();
    let (_, h) = ws.visible_bodies()[0];
    let after = ws.cur.work.kernel().bounds(h).unwrap();
    assert!((after.0[0] - before.0[0] - 12.0).abs() < 1e-6);
}

#[test]
fn both_halves_of_a_split_keep_the_edges_on_the_cut() {
    // M8: 割った所の辺(厚みの半分 z = 5 の外周 4 本)は、両方のボディの網に入っていて描かれる
    let mut ws = plate();
    ws.add_described(kmb_cad_modify::SPLIT).expect("足せる");
    ws.wait();
    ws.finish_edit();
    ws.wait();
    assert_eq!(ws.cur.scene.bodies.len(), 2);
    for b in &ws.cur.scene.bodies {
        let on_cut = b
            .edge_segments()
            .iter()
            .filter(|[p, q]| (p[2] - 5.0).abs() < 1e-3 && (q[2] - 5.0).abs() < 1e-3)
            .count();
        assert_eq!(on_cut, 4, "{:?}", b.feature);
    }
}

#[test]
fn a_section_paints_each_body_its_own_striped_color() {
    // M11: 断面の切り口は既定でボディごとの色で、斜めの縞が入る。外側も同じ色にできる
    use crate::view3d;
    let mut ws = plate();
    ws.add_described(kmb_cad_modify::SPLIT).expect("足せる");
    ws.wait();
    ws.finish_edit();
    ws.wait();
    ws.clear_selection();
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(320.0, 240.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let colors = view3d::Colors::from_look(&crate::theme::Look::default());
    ws.toggle_section();
    let s = ws.cur.section.clone().unwrap();
    assert!(s.by_body && !s.outside, "既定は切り口だけ色分け");
    ws.cur.scene.section = Some(s.section());
    let render = |scene: &view3d::Scene| {
        view3d::render(
            scene,
            &ws.cur.camera,
            [320, 240],
            1.0,
            &[],
            None,
            &colors,
            crate::Shading::Shaded,
        )
    };
    let count =
        |img: &egui::ColorImage, c: egui::Color32| img.pixels.iter().filter(|p| **p == c).count();
    let caps = [0, 1].map(|i| ws.cur.scene.cap_color(i, &colors));
    assert_ne!(caps[0], caps[1], "ボディごとに違う色");
    let img = render(&ws.cur.scene);
    for c in caps {
        assert!(count(&img, c) > 100, "切り口の地の色 {c:?}");
        assert!(count(&img, view3d::hatch_color(c)) > 10, "縞 {c:?}");
    }
    // 色分けを外すと、どちらの切り口も同じ色
    let mut plain = s.clone();
    plain.by_body = false;
    ws.cur.scene.section = Some(plain.section());
    assert_eq!(
        ws.cur.scene.cap_color(0, &colors),
        ws.cur.scene.cap_color(1, &colors)
    );
    assert_eq!(
        ws.cur.scene.cap_color(0, &colors),
        view3d::cap_color(&colors)
    );
    // 外側も塗ると、ボディの色が切り口と同じになる
    let mut outside = s;
    outside.outside = true;
    ws.cur.scene.section = Some(outside.section());
    for (i, cap) in caps.iter().enumerate() {
        assert_eq!(ws.cur.scene.body_color(i, &colors), *cap);
    }
}

#[test]
fn point_to_point_takes_the_points_from_vertex_clicks() {
    // 点から点の移動の「元の点」「先の点」を、3D のビューの頂点のクリックで埋める。
    // 元の点を入れたら先の点へ進み、両方入ると受けるのをやめる。頂点の無い所は何も変えない
    let mut ws = plate();
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(640.0, 480.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let id = ws.add_described(kmb_cad_modify::MOVE).expect("足せる");
    ws.wait();
    assert!(ws.is_editing(id));
    let mut data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    data["mode"] = json!("point_to_point");
    assert!(ws.set_feature_data(id, data));
    ws.wait();
    ws.toggle_point_pick(id, "from_point");
    assert!(ws.is_point_pick(id, "from_point"));
    let at = |ws: &Workspace, p: [f64; 3]| ws.cur.camera.project(p.map(|v| v as f32), rect).0;
    // 頂点の無い所(上の面の真ん中)
    let mid = at(&ws, [15.0, 10.0, 10.0]);
    ws.point_pick_click(rect, mid);
    assert!(ws.message().is_some_and(|m| m.contains("頂点")));
    assert!(ws.is_point_pick(id, "from_point"), "外れたら留まる");
    let corner = at(&ws, [30.0, 20.0, 10.0]);
    ws.point_pick_click(rect, corner);
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["from_point"], json!([30.0, 20.0, 10.0]), "{data}");
    assert!(ws.is_point_pick(id, "to_point"), "先の点へ進む");
    let origin = at(&ws, [0.0, 0.0, 10.0]);
    ws.point_pick_click(rect, origin);
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["to_point"], json!([0.0, 0.0, 10.0]), "{data}");
    assert!(ws.active_point_pick().is_none(), "両方入ったらやめる");
    ws.wait();
    let (_, h) = ws.visible_bodies()[0];
    let (lo, _) = ws.cur.work.kernel().bounds(h).unwrap();
    assert!(
        (lo[0] + 30.0).abs() < 1e-3 && (lo[1] + 20.0).abs() < 1e-3,
        "{lo:?}"
    );
}

/// 採用分 10-24 u3(意図だけ取り込む): 移動の量を数で打つ人が、X 距離に打って Tab で Y 距離・Z 距離へ
/// 順に進める(欄の間のボタンで止まらない)。Enter で決めると 3 つとも入る。
#[test]
fn tab_walks_the_move_fields_from_x_to_y_to_z() {
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{Queryable, by};
    let mut ws = plate();
    let id = ws.add_described(kmb_cad_modify::MOVE).expect("足せる");
    ws.wait();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run_steps(3);
    // 人の手順: X 距離の欄を押して 5 と打ち、Tab、6、Tab、7、Enter
    let y = h.get_by_label("X 距離").rect().center().y;
    let x_field = h
        .query_all(by().role(Role::TextInput))
        .find(|n| n.rect().top() <= y && y <= n.rect().bottom())
        .expect("X 距離の欄")
        .rect();
    h.event(egui::Event::PointerMoved(x_field.center()));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: x_field.center(),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
    for (i, v) in ["5", "6", "7"].into_iter().enumerate() {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text(v.into()));
        h.run_steps(2);
        h.key_press(if i < 2 {
            egui::Key::Tab
        } else {
            egui::Key::Enter
        });
        h.run_steps(3);
    }
    let data = h
        .state()
        .cur
        .work
        .doc()
        .data()
        .feature(id)
        .unwrap()
        .data
        .clone();
    let num = |k: &str| {
        let s = data[k].as_str().unwrap_or("").to_string();
        s.split_whitespace().next().unwrap_or("").to_string()
    };
    assert_eq!(
        [num("x"), num("y"), num("z")],
        ["5", "6", "7"].map(String::from),
        "{data}"
    );
}
