//! ほかのタブの重い計算が形状カーネルを使っている間も、このタブの画面が待たずに描けるか。
//! 形状カーネル(SMLib)の錠はすべての文書で 1 つなので、[`kmb_bundle::occupy_kernel`] でほかの文書の
//! 計算を作り、錠を持っている間に戻ったか([`kmb_bundle::KernelHeld::still_held`])で確かめる。

use super::*;
use egui_kittest::Harness;
use kmb_samples::reference_document;
use std::time::Duration;

/// 錠を持ち続ける上限(速さの確かめではない。待ってしまった時に試験が止まらないように放す)。
const LIMIT: Duration = Duration::from_secs(60);

fn settled() -> Workspace {
    let mut ws = Workspace::open(reference_document(), None, true, kmb_bundle::modules());
    ws.wait();
    assert!(!ws.cur.scene.bodies.is_empty());
    ws
}

fn edge_count(ws: &mut Workspace) -> usize {
    ws.scene_edges().iter().map(|(_, e)| e.len()).sum()
}

#[test]
fn the_view_keeps_its_meshes_and_edges_while_another_tab_computes() {
    let mut ws = settled();
    let shown = ws.cur.scene.bodies.len();
    let kernel = ws.cur.work.kernel().clone();
    assert!(edge_count(&mut ws) > 0);
    let busy = kmb_bundle::occupy_kernel(LIMIT);
    // 網の細かさを替える(網を作り直す)・辺を問い合わせ直す・ボディの種類を確かめる
    ws.cur.scene.set_tolerance(kernel.as_ref(), 0.2);
    ws.cur.edge_cache = Default::default();
    assert_eq!(edge_count(&mut ws), 0, "辺は控えが無いので、まだ出せない");
    ws.cur.scene.set_body_kinds(kernel.as_ref());
    assert!(busy.still_held(), "ほかのタブの計算が終わるのを待った");
    assert!(ws.cur.scene.is_pending(), "作り直しを待つ");
    assert_eq!(ws.cur.scene.bodies.len(), shown, "前の網のまま描く");
    // 計算が終われば作り直す
    drop(busy);
    assert!(ws.cur.scene.retry(kernel.as_ref()));
    assert!(!ws.cur.scene.is_pending());
    assert_eq!(ws.cur.scene.bodies.len(), shown);
    assert!(edge_count(&mut ws) > 0, "辺を問い合わせ直した");
}

#[test]
fn frames_of_this_tab_are_drawn_while_another_tab_computes() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), settled());
    h.run();
    let busy = kmb_bundle::occupy_kernel(LIMIT);
    // 描画の設定で網の細かさを替え(こまの中で網を作り直す)、こまを描く
    let shown = h.state().cur.scene.bodies.len();
    h.state_mut().win.layout.render.mesh = kmb_ui_view3d::render::MeshDetail::Coarse;
    for _ in 0..3 {
        h.step();
    }
    assert!(busy.still_held(), "ほかのタブの計算が終わるのを待った");
    assert!(h.state().cur.scene.is_pending(), "作り直しを待つ");
    assert_eq!(h.state().cur.scene.bodies.len(), shown, "前の網のまま描く");
    drop(busy);
    h.run();
    assert!(
        !h.state().cur.scene.is_pending(),
        "計算が終わったら作り直す"
    );
}
