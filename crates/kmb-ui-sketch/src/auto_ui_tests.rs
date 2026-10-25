//! 自動拘束(K18)を人の操作の順で確かめる: 拘束 ▾ の「自動拘束」→ 予告の欄 → OK か Esc。

use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_sketch::SolveStatus;
use kmb_ui::Workspace;

fn open_auto(h: &mut Harness<'static, Workspace>) {
    h.get_by_label("拘束 ▾").click();
    h.run();
    // ▾ の一覧は巻物なので、下の方の行は送ってから押す
    h.get_by_label("自動拘束").scroll_to_me();
    for _ in 0..3 {
        h.run();
    }
    h.get_by_label("自動拘束").click();
    h.run();
    h.run();
    for n in h.query_all_by_label_contains("自動") {
        eprintln!("ラベル: {:?}", n);
    }
}

// 確かめること:
// - 描いた長方形(位置と大きさの 4 つが動く)に、寸法と原点からの位置を付けて完全拘束にする
// - 付けるのは 1 回の取り消しで戻る
#[test]
fn auto_constrain_previews_and_fully_constrains_a_drawn_rectangle() {
    let mut h = sketch_with_rectangle();
    let before = current_sketch(h.state()).constraints.len();
    assert!(matches!(
        solved(h.state()).status,
        SolveStatus::UnderConstrained { dof: 4 }
    ));
    open_auto(&mut h);
    h.get_by_label_contains("全部を付けると自由度 4 → 0");
    h.get_by_label("直径・半径・長さ(2)");
    h.get_by_label("基準の点からの位置(2)");
    assert!(
        current_sketch(h.state()).constraints.len() == before,
        "予告の間は付けない"
    );
    h.get_by_label("OK(4 個を付ける)").click();
    h.run();
    h.run();
    assert_eq!(solved(h.state()).status, SolveStatus::WellConstrained);
    assert!(h.query_by_label("OK(4 個を付ける)").is_none(), "欄を閉じる");
    h.state_mut().undo();
    h.run();
    assert_eq!(current_sketch(h.state()).constraints.len(), before);
}

#[test]
fn unchecking_dimensions_and_esc_leave_the_sketch_as_is() {
    let mut h = sketch_with_rectangle();
    let before = current_sketch(h.state());
    open_auto(&mut h);
    h.get_by_label("寸法も付ける").click();
    h.run();
    h.run();
    // 描いた長方形は一致・水平・垂直が付いているので、寸法を外すと候補は無い
    h.get_by_label_contains("候補 0 個");
    press_key(&mut h, egui::Key::Escape);
    h.run();
    assert!(
        h.query_by_label_contains("候補 0 個").is_none(),
        "Esc でやめる"
    );
    assert_eq!(current_sketch(h.state()), before);
}
