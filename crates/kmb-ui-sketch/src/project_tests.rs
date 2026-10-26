//! 投影の道具(網羅版 K1)、選ぶだけでは写さない(G10)、元が見つからない投影(K4)を、
//! 人の操作(クリック)で確かめる。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::Document;
use kmb_sketch::{ConstraintKind, RefShape, Sketch};
use kmb_ui::Workspace;

/// 基準部品の見本の最後(ボディの後)に足したスケッチを開いた作業場。ボディの辺を写せる。
/// `edit` でスケッチを作ってから開く(引数は板のボディの機能の番号)。
pub(crate) fn later_sketch(edit: impl Fn(&mut Sketch, u64)) -> Harness<'static, Workspace> {
    let part = kmb_samples::reference_part();
    let mut v: serde_json::Value = serde_json::from_str(&part.doc.to_json()).unwrap();
    let body = part.plate.0;
    let mut sk = Sketch::default();
    edit(&mut sk, body);
    let id = v["next_id"].as_u64().unwrap();
    v["next_id"] = (id + 1).into();
    v["features"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": id,
            "type_id": kmb_sketch::SKETCH,
            "name": "後のスケッチ",
            "data": serde_json::to_value(&sk).unwrap(),
        }));
    let doc = Document::from_json(&v.to_string()).unwrap();
    let mut ws = ws_of(doc, None, reg());
    ws.open_edit(kmb_core::FeatureId(id));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.run();
    h
}

/// 写していないボディのまっすぐな辺のうち、`n` 番目の真ん中(スケッチの座標)。
fn edge_mid(h: &mut Harness<'static, Workspace>, n: usize) -> [f64; 2] {
    let s = current_sketch(h.state());
    let refs = editor(h.state_mut()).refs.as_ref().unwrap().1.clone();
    refs.iter()
        .filter(|r| !s.projections.iter().any(|p| p.source.same(&r.source)))
        .filter_map(|r| match r.shape {
            RefShape::Line(a, b) => Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]),
            _ => None,
        })
        .nth(n)
        .expect("ボディの辺")
}

fn projections(h: &Harness<'static, Workspace>) -> usize {
    current_sketch(h.state()).projections.len()
}

#[test]
fn selecting_a_body_edge_does_not_project_it_until_it_is_used() {
    let mut h = later_sketch(|s, _| {
        // 板の外に斜めの線分を 1 本
        let a = s.add_point(-30.0, -20.0);
        let b = s.add_point(-10.0, -5.0);
        s.add_line(a, b);
    });
    let before = projections(&h);
    let undo = h.state().document().undo_labels().len();
    let p = edge_mid(&mut h, 0);
    click_at(&mut h, p);
    h.run();
    assert_eq!(
        editor(h.state_mut()).editor.selection.len(),
        1,
        "辺を選べる"
    );
    assert_eq!(projections(&h), before, "選んだだけでは写さない");
    assert_eq!(
        h.state().document().undo_labels().len(),
        undo,
        "取り消しの段も増えない"
    );
    // 描いた線分を Shift で足して、パネルの「角度」の寸法を押すと、その時に辺を写して付ける
    crate::menu::tests::click_with(&mut h, [-20.0, -12.5], egui::Modifiers::SHIFT);
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 2);
    h.get_by_label("角度").click();
    h.run();
    h.run();
    assert!(projections(&h) > before, "使った時に写す");
    assert!(
        current_sketch(h.state())
            .constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::Angle { .. }))
    );
}

#[test]
fn the_project_tool_copies_an_edge_or_a_whole_body() {
    let mut h = later_sketch(|_, _| {});
    let before = projections(&h);
    editor(h.state_mut()).set_tool(Tool::Project);
    h.run();
    let p = edge_mid(&mut h, 0);
    click_at(&mut h, p);
    h.run();
    // 辺と、その両端の角
    let one = projections(&h);
    assert!(one > before, "{one}");
    // ボディの辺と角すべて: 押したボディの辺と角をまとめて
    editor(h.state_mut()).editor.project_pick = crate::view::ProjectPick::Body;
    let p = edge_mid(&mut h, 0);
    click_at(&mut h, p);
    h.run();
    assert!(projections(&h) > one + 4, "{}", projections(&h));
}

#[test]
fn projecting_without_a_link_makes_a_fixed_plain_line() {
    let mut h = later_sketch(|_, _| {});
    let before = projections(&h);
    let lines = count_of(&current_sketch(h.state()), |e| {
        matches!(e, kmb_sketch::Entity::Line { .. })
    });
    editor(h.state_mut()).set_tool(Tool::Project);
    editor(h.state_mut()).editor.project_link = false;
    h.run();
    let p = edge_mid(&mut h, 0);
    click_at(&mut h, p);
    h.run();
    let s = current_sketch(h.state());
    assert_eq!(projections(&h), before, "つながりは残さない");
    // 写した線は補助線のまま(投影と同じ見え方)、元とのつながりだけを切る
    assert_eq!(
        count_of(&s, |e| matches!(e, kmb_sketch::Entity::Line { .. })),
        lines + 1
    );
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::Fix { .. }))
    );
}

#[test]
fn lost_projections_are_listed_and_can_be_released_or_deleted() {
    // 元の面の名前が無い投影を 1 つ足しておく(ボディが変わって辺が無くなった時と同じ)
    let lost = |s: &mut Sketch, body: u64| {
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(10.0, 0.0);
        let line = s.add_line(a, b);
        s.projections.push(kmb_sketch::Projection {
            entity: line,
            source: kmb_sketch::ProjectionSource::Edge {
                feature: kmb_core::FeatureId(body),
                faces: ["無い面".into(), "もう無い面".into()],
            },
        });
    };
    let mut h = later_sketch(lost);
    assert_eq!(projections(&h), 1);
    h.get_by_label("投影をやめる").click();
    h.run();
    h.run();
    assert_eq!(projections(&h), 0);

    let mut h = later_sketch(lost);
    let n = current_sketch(h.state()).entities.len();
    h.get_by_label("削除").click();
    h.run();
    h.run();
    assert_eq!(projections(&h), 0);
    assert!(current_sketch(h.state()).entities.len() < n);
}

/// 写した物のうち、線分と円の数。
fn projected_kinds(h: &Harness<'static, Workspace>) -> (usize, usize) {
    let s = current_sketch(h.state());
    let of = |want: fn(&kmb_sketch::Entity) -> bool| {
        s.projections
            .iter()
            .filter(|p| s.entity(p.entity).is_some_and(want))
            .count()
    };
    (
        of(|e| matches!(e, kmb_sketch::Entity::Line { .. })),
        of(|e| matches!(e, kmb_sketch::Entity::Circle { .. })),
    )
}

#[test]
fn clicking_inside_a_face_projects_its_edges() {
    // 板の下面(z = 0)のスケッチ。上から見て、ポケットとボスの無い板の上面の所を押す
    let mut h = later_sketch(|_, _| {});
    editor(h.state_mut()).set_tool(Tool::Project);
    h.run();
    assert_eq!(projected_kinds(&h), (0, 0));
    click_at(&mut h, [30.0, 34.0]);
    h.run();
    h.run();
    let (lines, circles) = projected_kinds(&h);
    if on_truck() {
        // truck は円の辺を返さず、ポケットの縁は別の面(ポケットの始まりの面)の辺になる
        assert_eq!(lines, 4, "外形の 4 辺");
    } else {
        // 上面の縁: 外形の 4 辺とポケットの縁 4 辺、真ん中と四隅の穴、ボスの根元の円
        assert_eq!(lines, 8, "線 {lines}");
        assert_eq!(circles, 6, "円 {circles}");
    }
    assert!(
        editor(h.state_mut()).editor.notice.is_none(),
        "{:?}",
        editor(h.state_mut()).editor.notice
    );
    // 元が見つからない投影は無い(平面から遠い面の辺も取り直せる)
    assert!(h.query_by_label_contains("元が見つからない投影").is_none());
}

#[test]
fn the_silhouette_option_projects_only_the_outline_of_the_body() {
    let mut h = later_sketch(|_, _| {});
    editor(h.state_mut()).set_tool(Tool::Project);
    h.run();
    // パネルで「ボディのシルエット」を選んでから、ボディの面を押す
    h.get_by_label("ボディのシルエット(押したボディの外形)")
        .click();
    h.run();
    h.run();
    click_at(&mut h, [30.0, 34.0]);
    h.run();
    h.run();
    // 外形の 4 辺と、抜けた穴(真ん中と四隅)。ポケットとボスは外形の内側なので写さない
    assert_eq!(projected_kinds(&h), (4, 5));
    let s = current_sketch(h.state());
    // truck は円の辺を返さないので、穴の縁は網から当てはめた線(Silhouette)になる
    assert!(
        s.projections
            .iter()
            .filter(|p| !matches!(s.entity(p.entity), Some(kmb_sketch::Entity::Point { .. })))
            .filter(|p| !on_truck()
                || matches!(s.entity(p.entity), Some(kmb_sketch::Entity::Line { .. })))
            .all(|p| matches!(p.source, kmb_sketch::ProjectionSource::Edge { .. })),
        "外形に乗る辺は辺として写す(元の辺に付いて動く)"
    );
    assert!(h.query_by_label_contains("元が見つからない投影").is_none());
}

fn on_truck() -> bool {
    std::env::var("JC_KERNEL").is_ok_and(|v| v.trim() == "truck")
}

#[test]
fn a_body_hidden_with_the_eye_is_not_a_reference_in_the_sketch() {
    // まとめてレビュー R1: ボディの線(吸い付く先)がブラウザの目と関係なく出ていた。
    // 隠したボディは描かず、吸い付かない。出し直すと戻る
    let mut h = later_sketch(|_, _| {});
    let refs =
        |h: &mut Harness<'static, Workspace>| editor(h.state_mut()).refs.as_ref().unwrap().1.len();
    assert!(refs(&mut h) > 1, "原点とボディの角・辺");
    let body = h.state().visible_bodies()[0].0;
    h.state_mut().set_hidden(body, true);
    h.run();
    h.run();
    assert_eq!(refs(&mut h), 1, "原点だけ");
    h.state_mut().set_hidden(body, false);
    h.run();
    h.run();
    assert!(refs(&mut h) > 1, "出すと戻る");
}

#[test]
fn a_body_hidden_with_the_browser_body_eye_is_not_a_reference_in_the_sketch() {
    // ブラウザのボディの行の目(最初に作った機能で覚える)で隠したボディも、スケッチの中で
    // 描かず吸い付かない。行の目だけを見ていた時は、スケッチの中にだけ出ていた
    let mut h = later_sketch(|_, _| {});
    let refs =
        |h: &mut Harness<'static, Workspace>| editor(h.state_mut()).refs.as_ref().unwrap().1.len();
    assert!(refs(&mut h) > 1, "原点とボディの角・辺");
    let root = h.state().body_roots()[0].1;
    h.state_mut().set_body_hidden(root, true);
    h.run();
    h.run();
    assert_eq!(refs(&mut h), 1, "原点だけ");
    h.state_mut().set_body_hidden(root, false);
    h.run();
    h.run();
    assert!(refs(&mut h) > 1, "出すと戻る");
}
