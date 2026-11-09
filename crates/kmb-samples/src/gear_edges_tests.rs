//! 確かめること: 歯車(モジュール 1・20 歯・幅 10)のボディで、画面が辺の当たり判定に使う 3 種の辺
//! (まっすぐな辺・円の辺・曲がった辺)をカーネルに問い合わせた時間と数を出す。時間の上限では落とさない
//! (速さは機械で変わる)。2 回目は形ごとの控えを使い、1 回目と同じ辺を返すことを確かめる。
//! 測った値は `cargo test -p kmb-samples gear_edges -- --nocapture` で見る。

use crate::sample::DocBuilder;
use kmb_core::{EdgeInfo, Kernel, ShapeHandle};
use serde_json::json;
use std::time::Instant;

fn timed(
    what: &str,
    f: impl Fn() -> Result<Vec<EdgeInfo>, kmb_core::KernelError>,
) -> Vec<EdgeInfo> {
    let began = Instant::now();
    let edges = f().expect(what);
    eprintln!(
        "{what}: {} 本、{:.3} 秒",
        edges.len(),
        began.elapsed().as_secs_f64()
    );
    edges
}

/// 3 種の辺を画面と同じ順に問い合わせる。
fn all_edges(kernel: &dyn Kernel, h: ShapeHandle, round: &str) -> Vec<Vec<EdgeInfo>> {
    let began = Instant::now();
    let lists = vec![
        timed(&format!("{round} まっすぐな辺"), || kernel.edges(h)),
        timed(&format!("{round} 円の辺"), || kernel.arc_edges(h)),
        timed(&format!("{round} 曲がった辺"), || {
            kernel.curve_edges(h)
        }),
    ];
    eprintln!("{round} 合計: {:.3} 秒", began.elapsed().as_secs_f64());
    lists
}

#[test]
fn gear_edges_are_listed_once_and_reused() {
    let mut b = DocBuilder::new();
    let gear = b.feature(
        "cad.gear@1",
        "歯車の輪郭",
        json!({"module":"1 mm", "teeth":"20"}),
    );
    b.feature(
        "cad.gear_body@1",
        "歯車のボディ",
        json!({"gear":gear, "width":"10 mm"}),
    );
    let mut doc = b.finish("歯車の辺");
    let registry = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let report = doc.recompute(&registry, kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let bodies = kmb_core::visible_bodies(doc.data(), &registry, &report);
    let [(_, h)] = bodies[..] else {
        panic!("ボディが 1 つ: {bodies:?}")
    };
    let first = all_edges(kernel.as_ref(), h, "1 回目");
    // 歯 1 枚に上下の円の辺と縦の辺があるので、歯の数より多い(選べる辺が減っていない)
    let total: usize = first.iter().map(Vec::len).sum();
    assert!(first[1].len() >= 40 && total >= 120, "辺の数 {total}");
    let second = all_edges(kernel.as_ref(), h, "2 回目");
    let same = |a: &[EdgeInfo], b: &[EdgeInfo]| {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(x, y)| x.faces == y.faces && x.a == y.a && x.b == y.b)
    };
    assert!(
        first.iter().zip(&second).all(|(a, b)| same(a, b)),
        "控えは同じ辺を返す"
    );
}
