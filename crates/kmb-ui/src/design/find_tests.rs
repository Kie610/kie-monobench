//! 形から機能を探す・選択表示・右クリックの「スケッチを編集」・消す前の確かめ・計算時間の試験。

use crate::select_ui::Hit;
use crate::{Found, Workspace};
use kmb_core::{EdgePick, FeatureId};
use kmb_session::edit;

fn reg() -> kmb_core::Modules {
    kmb_bundle::modules()
}

/// ボディの面の名前のうち、`pred` に合う最初のもの。
fn face_where(ws: &Workspace, body: FeatureId, pred: impl Fn(&str) -> bool) -> String {
    ws.cur
        .scene
        .bodies
        .iter()
        .find(|b| b.feature == body)
        .and_then(|b| b.mesh.face_names.iter().find(|n| pred(n)).cloned())
        .expect("面がある")
}

#[test]
fn a_face_is_looked_at_and_the_feature_that_made_it_is_found_in_the_timeline() {
    // 1 つの部品で順に確かめる:
    // - 面の右クリックの「正面」は選びを変えない
    //   (元 a_right_clicked_face_is_looked_at_from_the_front_without_changing_the_selection)
    // - 面・辺を作った機能をタイムラインとブラウザで探す(元 the_feature_that_made_a_face_or_edge_is_found_in_the_timeline)
    let part = kmb_samples::reference_part();
    let (plate, pocket) = (part.plate, part.pocket);
    let mut ws = Workspace::new(part.doc, None, reg());
    // 1 本目の「正面」(下の帯)を面の右クリックからも使う。選びは変えない
    ws.clear_selection();
    ws.cur.camera.set_view(-60.0, 30.0);
    let b = &ws.cur.scene.bodies[0];
    let face = b
        .mesh
        .face_names
        .iter()
        .find(|f| {
            crate::view3d::face_normal(&ws.cur.scene, f)
                .is_some_and(|m| (m[0]).abs() < 1e-3 && (m[1] + 1.0).abs() < 1e-3)
        })
        .expect("前の面がある")
        .clone();
    let pick = crate::view3d::Pick {
        feature: b.feature,
        face,
    };
    ws.look_at_face(&pick);
    assert!(ws.cur.camera.is_view(-90.0, 0.0), "前の面を正面から見る");
    assert!(ws.picked_faces().is_empty() && ws.selection().is_empty());

    // 「この面はどの機能で作ったか」をタイムラインで探せる(U32)
    let last = ws.visible_bodies()[0].0;
    let pocket_face = face_where(&ws, last, |n| n.starts_with(&format!("f{}:", pocket.0)));
    let plate_face = face_where(&ws, last, |n| n.starts_with(&format!("f{}:", plate.0)));
    let face = |f: &str| {
        Hit::Face(crate::view3d::Pick {
            feature: last,
            face: f.to_string(),
        })
    };
    assert_eq!(ws.creator_of(&face(&pocket_face)), Some(pocket));
    assert_eq!(ws.creator_of(&face(&plate_face)), Some(plate));
    // 辺は両側の面のうち、後で作った方の機能
    let edge = Hit::Edge(EdgePick {
        feature: last,
        faces: [plate_face.clone(), pocket_face.clone()],
    });
    assert_eq!(ws.creator_of(&edge), Some(pocket));
    // 探すと光らせ、見える所まで送る(選びは変えない)
    ws.clear_selection();
    ws.find_in_timeline(pocket);
    assert!(matches!(ws.cur.found, Some((Found::Timeline(id), _)) if id == pocket));
    assert_eq!(ws.cur.found_scroll, Some(Found::Timeline(pocket)));
    assert!(ws.selection().is_empty());
    let ctx = egui::Context::default();
    assert!(ws.found_glow(&ctx, Found::Timeline(pocket)).is_some());
    assert!(ws.found_glow(&ctx, Found::Timeline(plate)).is_none());
    // ブラウザで探すと、ボディ(最初に作った機能)を光らせる
    ws.find_in_browser(last);
    assert_eq!(ws.cur.found_scroll, Some(Found::Browser(plate)));
}

#[test]
fn isolating_a_body_hides_the_others_until_it_ends_and_keeps_the_eyes() {
    // まとめて隠し、まとめて戻せる(目で 1 つずつ隠して戻さずに済む。U21)
    let mut b = kmb_samples::sample::DocBuilder::new();
    let mut block = |name: &str, x: f64| {
        let mut s = kmb_samples::sample::SketchBuilder::new(kmb_sketch::SketchPlane::Xy);
        let o = s.fixed_point(0.0, 0.0);
        let c = s.point_from(o, &format!("{x} mm"), "0 mm", [x, 0.0]);
        s.rectangle(c, "10 mm", "10 mm", [x, 0.0, 10.0, 10.0]);
        let sk = b.feature(kmb_sketch::SKETCH, &format!("{name}のスケッチ"), s.build());
        b.feature(
            kmb_cad_extrude::EXTRUDE,
            name,
            kmb_cad_extrude::Extrude {
                profile: Some(sk),
                distance: "5 mm".into(),
                ..Default::default()
            },
        )
    };
    let (a, c, d) = (block("A", 0.0), block("B", 20.0), block("C", 40.0));
    let mut ws = Workspace::new(b.finish("3 つの箱"), None, reg());
    ws.set_body_hidden(d, true);
    assert!(ws.is_hidden(d) && !ws.is_hidden(c));
    ws.isolate_bodies(vec![a]);
    assert_eq!(ws.isolated_bodies(), [a]);
    assert!(!ws.is_hidden(a) && ws.is_hidden(c) && ws.is_hidden(d));
    assert!(ws.any_body_hidden());
    // やめると前の見え方(C は目で隠したまま)
    ws.end_isolation();
    assert!(!ws.is_hidden(a) && !ws.is_hidden(c) && ws.is_hidden(d));
    // すべて表示は選択表示をやめ、目で隠したボディも出す
    ws.isolate_bodies(vec![c]);
    ws.show_all_bodies();
    assert!(ws.isolated_bodies().is_empty());
    assert!(!ws.is_hidden(a) && !ws.is_hidden(c) && !ws.is_hidden(d));
    assert!(!ws.any_body_hidden());
    // 一緒に選んでいるボディもまとめて選択表示できる
    ws.select(a);
    ws.toggle_select(c);
    assert_eq!(ws.bodies_to_isolate(a), [a, c]);
    ws.isolate_bodies(ws.bodies_to_isolate(a));
    assert!(!ws.is_hidden(a) && !ws.is_hidden(c) && ws.is_hidden(d));
    // 選択表示していたボディを消すと外し、残りは選択表示のまま。全部無くなればやめる
    ws.clear_selection();
    assert!(ws.remove_feature(c));
    assert_eq!(ws.isolated_bodies(), [a]);
    assert!(ws.remove_feature(a));
    assert!(ws.isolated_bodies().is_empty());
}

#[test]
fn what_a_removal_takes_along_is_named_with_the_groups_it_empties() {
    // 1 つの部品で順に確かめる:
    // - U43: 押し出しの元のスケッチ。U33: 消す前に、直接使う機能と、それを通して使う機能を分けて出す
    //   (元 the_sketch_a_feature_uses_and_what_a_removal_takes_along_are_named)
    // - 機能が全部消えるグループも、消える物として出す(元 a_group_whose_features_are_all_removed_is_named_as_going_too)
    let part = kmb_samples::reference_part();
    let (sketch, plate, pocket, holes, pattern) = (
        part.plate_sketch,
        part.plate,
        part.pocket,
        part.holes,
        part.hole_pattern,
    );
    let mut ws = Workspace::new(part.doc, None, reg());
    let data = ws.cur.work.doc().data();
    assert_eq!(
        edit::used_features(data, &ws.win.registry, plate),
        vec![sketch]
    );
    let direct = edit::users(data, &ws.win.registry, &[plate]);
    let all = edit::dependents(data, &ws.win.registry, &[plate]);
    assert!(direct.contains(&pocket), "{direct:?}");
    assert!(all.contains(&holes) && all.contains(&pattern), "{all:?}");
    assert!(
        !direct.contains(&pattern),
        "パターンは穴を通して使う: {direct:?}"
    );
    assert!(edit::groups_emptied(data, &[plate]).is_empty());

    // 穴とパターンをグループにまとめる
    let g = ws
        .group_features(&[holes, pattern], edit::GroupMode::IncludeGaps)
        .expect("まとめられる");
    let data = ws.cur.work.doc().data().clone();
    let gone: Vec<u64> = edit::groups_emptied(&data, &[holes, pattern])
        .iter()
        .map(|x| x.id)
        .collect();
    assert_eq!(gone, vec![g]);
    assert!(edit::groups_emptied(&data, &[holes]).is_empty());
    // 誰も使っていなければすぐ消し、知らせにグループも出す
    let ids: Vec<FeatureId> = data
        .group_range(data.groups.iter().find(|x| x.id == g).unwrap())
        .map(|(a, b)| data.features[a..=b].iter().map(|f| f.id).collect())
        .unwrap();
    assert!(ws.ask_remove_features(&ids));
    let m = ws.win.message.clone().unwrap_or_default();
    assert!(
        m.contains("グループ") && m.contains("もなくなりました"),
        "{m}"
    );
}

#[test]
fn each_feature_carries_its_compute_time_and_the_chip_tip_shows_it() {
    // 吹き出しに計算の時間も出し、重い機能が分かる(U44)
    let part = kmb_samples::reference_part();
    let plate = part.plate;
    let mut ws = Workspace::new(part.doc, None, reg());
    let first = ws.report().time_ms(plate).expect("計算した時間");
    assert!(first >= 0.0);
    // 使い回した機能も、その結果を計算した時の時間を出す
    ws.recompute();
    assert!(matches!(
        ws.report().feature(plate),
        Some(kmb_core::FeatureStatus::Ok { reused: true, .. })
    ));
    assert_eq!(ws.report().time_ms(plate), Some(first));
    let before = ws.report().clone();
    ws.recompute();
    assert_eq!(*ws.report(), before, "時間は比べる時に見ない");
    let tip =
        crate::timeline_ui::chip_tip("板", &crate::timeline_ui::Health::Ok, Some(12.34), true);
    assert_eq!(
        tip,
        "板\n計算 12 ms(前の結果を使い回し)\nダブルクリックで編集・ドラッグで並べ替え・右クリックでほかの操作"
    );
    assert_eq!(crate::timeline_ui::time_text(0.42), "0.4 ms");
    assert_eq!(crate::timeline_ui::time_text(2345.0), "2.3 秒");
}

/// 「正面から見る」は面のほか、選んだスケッチの平面とブラウザで選んだ原点の平面にも効き、
/// 回し終えるとボディとスケッチの全体が画面に収まる(スケッチの「正面から見る」と同じ)
#[test]
fn look_at_works_on_a_sketch_and_an_origin_plane_and_fits_the_whole_shape() {
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.win.view_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let (lo, hi) = ws.cur.scene.bounds();
    let center = [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0);
    let settle = |ws: &mut Workspace| {
        ws.cur.camera.step_turn(0.0);
        ws.cur.camera.step_turn(10.0);
    };
    // 選ぶ物が無ければ理由を出す
    ws.clear_selection();
    assert!(ws.look_at_target().is_err());
    // XY 平面のスケッチを選ぶと、上から見て形全体を収める
    let sketch = ws
        .cur
        .work
        .doc()
        .data()
        .features
        .iter()
        .map(|f| f.id)
        .find(|id| ws.sketch_plane(*id).is_some())
        .expect("スケッチがある");
    let n = ws.sketch_plane(sketch).unwrap().normal();
    ws.select(sketch);
    ws.cur.camera.set_view(-60.0, 30.0);
    ws.cur.camera.target = [500.0, 0.0, 0.0];
    ws.look_at_selected();
    settle(&mut ws);
    let (_, _, eye) = ws.cur.camera.basis_pub();
    let d: f64 = (0..3).map(|i| eye[i] as f64 * n[i]).sum();
    assert!(
        d.abs() > 0.999,
        "スケッチの平面を正面から見る: {eye:?} {n:?}"
    );
    let t = ws.cur.camera.target;
    assert!(
        (0..3).all(|i| (t[i] - center[i]).abs() < 1e-3),
        "形全体の真ん中へ移る: {t:?} {center:?}"
    );
    // 原点の YZ 平面(ブラウザの行)を選ぶと、右から見る。もう一度で裏から
    ws.clear_selection();
    ws.cur.origin_pick = Some(1);
    assert_eq!(ws.look_at_target(), Ok([1.0, 0.0, 0.0]));
    ws.look_at_selected();
    settle(&mut ws);
    let (_, _, eye) = ws.cur.camera.basis_pub();
    assert!(eye[0] > 0.999, "右から: {eye:?}");
    ws.look_at_selected();
    settle(&mut ws);
    let (_, _, eye) = ws.cur.camera.basis_pub();
    assert!(eye[0] < -0.999, "もう一度で裏から: {eye:?}");
    // 機能を選ぶと原点の平面の選びは外れる
    ws.select(sketch);
    assert_eq!(ws.cur.origin_pick, None);
}

#[test]
fn removing_a_cut_names_the_fillet_on_its_faces_before_removing() {
    // 採用分 10-20 u7(試験だけ): 切り取りで出来た面の辺に付けた丸めも、切り取りを消す前の確かめに出る。
    // 確かめは形を計算しない(固まらない)。出た通りに、消すと丸めは計算できなくなる。ポケットは上から
    // 切るので、底の面は始めの面(start)
    let part = kmb_samples::reference_part();
    let pocket = part.pocket;
    let mut ws = Workspace::new(part.doc, None, reg());
    let (tip, _) = ws.visible_bodies()[0];
    let pocket_face = |kind: &str| {
        let p = format!("f{}:{kind}", pocket.0);
        face_where(&ws, tip, |n| n.starts_with(&p))
    };
    let (floor, wall) = (pocket_face("start"), pocket_face("side:"));
    let fillet = ws
        .cur
        .work
        .add_feature(
            kmb_cad_edge::FILLET,
            None,
            serde_json::json!({"target": tip.0, "edges": [[floor, wall]], "radius": "1 mm"}),
        )
        .expect("足せる");
    ws.recompute();
    assert!(
        matches!(
            ws.report().feature(fillet),
            Some(kmb_core::FeatureStatus::Ok { .. })
        ),
        "{:?}",
        ws.report().feature(fillet)
    );
    // ポケットを消そうとすると、すぐには消さず丸めを並べて確かめる
    assert!(!ws.ask_remove_features(&[pocket]));
    let (_, users) = ws.cur.remove_ask.clone().expect("確かめを出す");
    assert!(users.contains(&fillet), "{users:?}");
    // 確かめの通り、ポケットだけ消すと丸めは計算できない
    assert!(ws.remove_feature(pocket));
    ws.recompute();
    assert!(
        !matches!(
            ws.report().feature(fillet),
            Some(kmb_core::FeatureStatus::Ok { .. })
        ),
        "{:?}",
        ws.report().feature(fillet)
    );
}
