//! 交差の道具とスライスを、人の操作で確かめる。

use crate::Tool;
use crate::project_tests::later_sketch;
use crate::tests::*;
use egui_kittest::kittest::Queryable;
use kmb_sketch::{ConstraintKind, Entity};

/// 板(60 × 40 × 10 mm、真ん中と四隅に穴)の厚みの途中(底から 3 mm)のスケッチ。
fn mid_plate() -> egui_kittest::Harness<'static, kmb_ui::Workspace> {
    later_sketch(|s, _| s.offset = Some("3 mm".into()))
}

/// 作成 ▾ の「投影/取り込み ▸」から交差の道具を選ぶ(人の操作)。
fn pick_intersect(h: &mut egui_kittest::Harness<'static, kmb_ui::Workspace>) {
    h.get_by_label("作成 ▾").click();
    h.run();
    // 道具が多いので、巻物を下へ送ってから、「投影/取り込み ▸」に指を置いて子の一覧から押す
    h.get_by_label("投影/取り込み").scroll_to_me();
    for _ in 0..5 {
        h.run();
    }
    h.get_by_label("投影/取り込み").hover();
    for _ in 0..3 {
        h.run();
    }
    h.get_by_label("交差").click();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Intersect);
    h.run();
    assert!(
        !editor(h.state_mut())
            .sections
            .as_ref()
            .unwrap()
            .1
            .is_empty(),
        "断面を作った"
    );
}

fn lines_of(s: &kmb_sketch::Sketch) -> usize {
    s.entities
        .iter()
        .filter(|e| matches!(e, Entity::Line { .. }))
        .count()
}

fn radii(s: &kmb_sketch::Sketch) -> Vec<f64> {
    s.entities
        .iter()
        .filter_map(|e| match e {
            Entity::Circle { radius, .. } => Some(*radius),
            _ => None,
        })
        .collect()
}

fn fixes(s: &kmb_sketch::Sketch) -> usize {
    s.constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Fix { .. }))
        .count()
}

#[test]
fn clicking_a_section_line_copies_just_that_line_with_a_link() {
    let mut h = mid_plate();
    pick_intersect(&mut h);
    // 板の手前の辺(y = 0)の真ん中を押す: その線だけを写す
    click_at(&mut h, [30.0, 0.0]);
    h.run();
    let s = current_sketch(h.state());
    assert_eq!(lines_of(&s), 1, "押した線だけ");
    assert!(radii(&s).is_empty());
    let line = s
        .entities
        .iter()
        .find(|e| matches!(e, Entity::Line { .. }))
        .unwrap()
        .id();
    assert!(
        s.projections.iter().any(|p| p.entity == line
            && matches!(
                &p.source,
                kmb_sketch::ProjectionSource::Section { index: 0, .. }
            )),
        "元の面とつながる: {:?}",
        s.projections
    );
    assert!(
        !s.entity(line).unwrap().is_construction(),
        "輪郭に使えるよう補助線にしない"
    );
    assert_eq!(fixes(&s), 0, "固定ではなく元に付いて動く");
    // もう一度押しても重ねない
    click_at(&mut h, [30.0, 0.0]);
    h.run();
    assert_eq!(lines_of(&current_sketch(h.state())), 1);
}

#[test]
fn the_whole_body_section_is_a_profile_that_follows_the_body_when_its_size_changes() {
    // 「ボディ全体」は押したボディの断面をすべて写し、動かない領域(押し出しに使える)になる
    // 元とつないだ断面は、板の幅のパラメータを変えると付いて行く
    let mut h = mid_plate();
    pick_intersect(&mut h);
    h.get_by_label("ボディ全体(押したボディの断面すべて)")
        .click();
    h.run();
    click_at(&mut h, [30.0, 0.0]);
    h.run();
    let s = current_sketch(h.state());
    assert_eq!(lines_of(&s), 4, "外形の 4 辺");
    let r = radii(&s);
    assert!(
        r.iter().any(|r| (r - 4.0).abs() < 0.02),
        "真ん中の 8 mm の穴: {r:?}"
    );
    assert_eq!(r.len(), 5, "真ん中と四隅の穴: {r:?}");
    assert_eq!(fixes(&s), 0);
    // 写した線は動かない(元の形から位置が決まる)
    let sol = solved(h.state());
    assert_eq!(sol.status, kmb_sketch::SolveStatus::WellConstrained);
    // 領域になる(押し出しに使える)
    let id = editor(h.state_mut()).editor.feature.unwrap();
    assert!(has_profile(h.state(), id));
    // 計算した形(元の形から取り直した位置で解いたもの)の右端
    let right = |h: &egui_kittest::Harness<'static, kmb_ui::Workspace>| {
        let f = h.state().report().features.last().unwrap();
        let kmb_core::FeatureStatus::Ok { outputs, .. } = &f.status else {
            panic!("{:?}", f.status)
        };
        kmb_sketch::solution_of(&outputs["sketch"])
            .unwrap()
            .points
            .values()
            .map(|p| p[0])
            .fold(f64::MIN, f64::max)
    };
    assert!((right(&h) - 60.0).abs() < 1e-6, "{}", right(&h));
    // 板の幅のパラメータを 60 → 80 mm にする
    let id = h
        .state()
        .document()
        .data()
        .param_by_name("width")
        .unwrap()
        .id;
    assert!(h.state_mut().set_param_expr(id, "80 mm"));
    h.run();
    h.run();
    assert!(
        (right(&h) - 80.0).abs() < 1e-6,
        "断面が付いて行く: {}",
        right(&h)
    );
    assert!(h.query_by_label_contains("元が見つからない投影").is_none());
}

#[test]
fn an_unlinked_section_is_fixed_where_it_was_and_not_copied_twice() {
    // リンクを外した交差を同じ所でもう一度押しても、線を重ねない。
    // リンクを外した断面はつながりを残さず、円の大きさまで固定する
    let mut h = mid_plate();
    pick_intersect(&mut h);
    h.get_by_label("投影リンク(ボディの形が変わると付いて行く)")
        .click();
    h.run();
    assert!(!editor(h.state_mut()).editor.intersect_link);
    click_at(&mut h, [30.0, 0.0]);
    h.run();
    assert_eq!(lines_of(&current_sketch(h.state())), 1);
    let undo = h.state().document().undo_labels().len();
    click_at(&mut h, [30.0, 0.0]);
    h.run();
    assert_eq!(lines_of(&current_sketch(h.state())), 1, "重ねない");
    assert_eq!(
        h.state().document().undo_labels().len(),
        undo,
        "何も変えない"
    );
    assert!(
        editor(h.state_mut())
            .editor
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("もう写してあります"))
    );
    // ボディ全体にすると、まだ写していない残りの線と円だけを足す
    h.get_by_label("ボディ全体(押したボディの断面すべて)")
        .click();
    h.run();
    click_at(&mut h, [30.0, 0.0]);
    h.run();
    let s = current_sketch(h.state());
    assert_eq!(lines_of(&s), 4, "外形の 4 辺(重ならない)");
    assert_eq!(radii(&s).len(), 5);
    assert!(s.projections.is_empty(), "つながりは残さない");
    assert!(fixes(&s) > 0, "固定する");
    assert_eq!(
        solved(h.state()).status,
        kmb_sketch::SolveStatus::WellConstrained,
        "円も大きさまで固定"
    );
}

#[test]
fn clicking_away_from_a_section_adds_nothing_and_says_so() {
    let mut h = mid_plate();
    editor(h.state_mut()).set_tool(Tool::Intersect);
    h.run();
    let before = current_sketch(h.state()).entities.len();
    click_at(&mut h, [-6.0, -6.0]);
    h.run();
    assert_eq!(current_sketch(h.state()).entities.len(), before);
    assert!(
        editor(h.state_mut())
            .editor
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("断面がありません"))
    );
}

#[test]
fn slice_shows_the_section_without_adding_anything() {
    let mut h = mid_plate();
    assert!(editor(h.state_mut()).sections.is_none(), "既定は作らない");
    // パレットの下の方にあるので、見える所まで送ってから開く
    h.get_by_label("スケッチの表示").scroll_to_me();
    h.run();
    h.get_by_label("スケッチの表示").click();
    h.run();
    h.get_by_label("スライス").scroll_to_me();
    h.run();
    h.get_by_label("スライス").click();
    h.run();
    h.run();
    assert!(editor(h.state_mut()).editor.display.slice);
    assert!(
        !editor(h.state_mut())
            .sections
            .as_ref()
            .unwrap()
            .1
            .is_empty()
    );
    assert!(current_sketch(h.state()).entities.is_empty());
}
