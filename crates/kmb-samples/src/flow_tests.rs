//! 流れと空力の通しの試験: スケッチから押し出した板の翼・円柱・箱に研究を付けて解き、寸法を
//! 変えると解き直されること、3D のビューの色分けと凡例、流れの時刻のコマ・表面の圧力・流線が出ること。

use crate::sample::{DocBuilder, SketchBuilder};
use kmb_cad::{Direction, Operation};
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_core::{Context, Document, FeatureId, Modules, Op, Transaction};
use kmb_sketch::SketchPlane;
use kmb_study::SimResult;
use serde_json::json;

thread_local! {
    static MODULES: Modules = kmb_bundle::modules();
}

fn modules() -> Modules {
    MODULES.with(|m| m.clone())
}

/// 長方形 (x, y, 幅 w, 奥 d) を厚さ t で押し出した板を足す。
#[allow(clippy::too_many_arguments)]
fn plate(
    b: &mut DocBuilder,
    name: &str,
    x: f64,
    y: f64,
    w: &str,
    d: &str,
    guess: [f64; 2],
    t: &str,
) -> FeatureId {
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(x, y);
    s.rectangle(o, w, d, [x, y, guess[0], guess[1]]);
    let sk = b.feature(kmb_sketch::SKETCH, &format!("{name}の輪郭"), s.build());
    b.feature(
        EXTRUDE,
        name,
        Extrude {
            profile: Some(sk),
            distance: t.into(),
            direction: Direction::Normal,
            operation: Operation::New,
            ..Default::default()
        },
    )
}

fn add(doc: &mut Document, type_id: &str, name: &str, data: serde_json::Value) -> FeatureId {
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        name,
        vec![Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: type_id.into(),
                name: name.into(),
                data,
                suppressed: false,
            },
        }],
    ))
    .unwrap();
    id
}

/// 研究を解いた結果(評価は解く準備だけをするので、準備した仕事をここで解く)。
fn result(m: &Modules, doc: &mut Document, id: FeatureId) -> SimResult {
    let rep = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = Context::new(doc.data(), &rep, &m.registry, &m.kernel);
    kmb_study::solve_now(&cx, id).unwrap_or_else(|e| panic!("研究が解けない: {e}"))
}

/// 画面の「実行」と同じ操作で解く(終わるまで待つ)。結果は画面の色分けに残る。
fn run(m: &Modules, doc: &mut Document, id: FeatureId) {
    let rep = doc.recompute(&m.registry, m.kernel.as_ref());
    let mut cx = Context::new(doc.data(), &rep, &m.registry, &m.kernel);
    cx.selected = Some(id);
    let wait = json!({"wait": true});
    cx.args = &wait;
    m.extensions.run(kmb_study::RUN, &cx).unwrap();
}

/// 色分けの形の、灰色(色なし)の面の番号(いつも最後)。
fn gray(s: &kmb_core::OverlayShape) -> u32 {
    s.face_colors.len() as u32 - 1
}

/// 結果の値(最後のコマ、量 `q`)。
fn values(r: &SimResult, q: usize) -> Vec<f32> {
    let v = r.values(r.frames.len() - 1, q).expect("値");
    (0..v.len()).map(|i| v.at(i)).collect()
}

fn number(r: &SimResult, key: &str) -> f64 {
    let v = &r
        .numbers
        .iter()
        .find(|(k, _)| k == key)
        .or_else(|| r.numbers.iter().find(|(k, _)| k.starts_with(key)))
        .unwrap_or_else(|| panic!("{key} が無い: {:?}", r.numbers))
        .1;
    v.split(|c: char| c.is_whitespace() || c == '(' || c == '%')
        .find_map(|w| w.parse().ok())
        .unwrap_or_else(|| panic!("{key} の数が無い: {v}"))
}

fn body(id: FeatureId) -> serde_json::Value {
    json!({"kind": "body", "feature": id.0})
}

#[test]
fn wing_study_gives_lift_and_the_tail_moves_the_neutral_point() {
    let m = modules();
    let mut b = DocBuilder::new();
    b.param("chord", "200 mm");
    // 主翼: 翼弦 200 mm(X)× 翼幅 1000 mm(Y)、厚さ 4 mm。尾翼は 500 mm 後ろ
    let wing = plate(
        &mut b,
        "主翼",
        0.0,
        -500.0,
        "chord",
        "1000 mm",
        [200.0, 1000.0],
        "4 mm",
    );
    let tail = plate(
        &mut b,
        "尾翼",
        700.0,
        -200.0,
        "80 mm",
        "400 mm",
        [80.0, 400.0],
        "3 mm",
    );
    let mut doc = b.finish("翼");
    let alone = add(
        &mut doc,
        kmb_flow::WING,
        "翼の揚力 1",
        json!({"bodies": [body(wing)], "dir": "+x", "alpha": "4", "speed": "10"}),
    );
    let r = result(&m, &mut doc, alone);
    // 翼幅の揚力係数は、どの帯も 0〜CL の 1.5 倍ほど(端の帯の幅が負になって飛び抜けない)
    let span_cl = &values(&r, 1);
    assert!(
        span_cl.iter().all(|v| (0.0..0.5).contains(v)),
        "翼幅の揚力係数 {:?}",
        span_cl
            .iter()
            .cloned()
            .fold((f32::MAX, f32::MIN), |a, v| (a.0.min(v), a.1.max(v)))
    );
    // 縦横比 5 の長方形の翼: 揚力傾斜は Helmbold の式で約 4.25 /rad、4° で CL 約 0.29
    let cl = number(&r, "揚力係数");
    assert!((0.26..0.31).contains(&cl), "CL {cl}");
    let np = number(&r, "中立点");
    assert!((40.0..60.0).contains(&np), "主翼だけの中立点 X = {np} mm");
    // 尾翼を足すと中立点が後ろへ動き、重心を入れると静安定余裕が出る
    let both = add(
        &mut doc,
        kmb_flow::WING,
        "翼の揚力 2",
        json!({"bodies": [body(wing), body(tail)], "dir": "+x", "alpha": "4", "speed": "10", "cg": "60 mm"}),
    );
    let r2 = result(&m, &mut doc, both);
    let np2 = number(&r2, "中立点");
    assert!(np2 > np + 20.0, "尾翼つきの中立点 {np2}(主翼だけ {np})");
    assert!(number(&r2, "静安定余裕") > 0.0);
    // 翼弦を 250 mm にすると解き直し、揚力が増える
    let p = doc.data().param_by_name("chord").unwrap().id;
    doc.apply(Transaction::new(
        "翼弦",
        vec![Op::SetParamExpr {
            id: p,
            expr: "250 mm".into(),
        }],
    ))
    .unwrap();
    let r3 = result(&m, &mut doc, alone);
    let lift = |r: &SimResult| number(r, "揚力");
    assert!(lift(&r3) > lift(&r) * 1.1, "{} と {}", lift(&r3), lift(&r));
    // 3D のビュー: 実行すると、研究を選んだ時に揚力の分布の色と凡例が出る
    run(&m, &mut doc, alone);
    run(&m, &mut doc, both);
    let rep = doc.recompute(&m.registry, m.kernel.as_ref());
    let mut cx = Context::new(doc.data(), &rep, &m.registry, &m.kernel);
    cx.selected = Some(alone);
    let o = m.extensions.overlay(&cx);
    let shape = o.shape.expect("色分けの形");
    assert!(!shape.legend.is_empty());
    assert!(shape.label.contains("揚力の分布"), "{}", shape.label);
    // 解いていない尾翼も灰色(色なし)で描く(結果を出している間もクリックして足せるように)
    let g = gray(&shape);
    let grays = shape
        .mesh
        .triangle_faces
        .iter()
        .filter(|f| **f == g)
        .count();
    assert!(grays >= 12, "灰色の三角形 {grays}");
    cx.selected = Some(both);
    let shape2 = m.extensions.overlay(&cx).shape.expect("両方の形");
    let g2 = gray(&shape2);
    assert!(
        !shape2.mesh.triangle_faces.contains(&g2),
        "両方を解いた時は灰色のボディはない"
    );
    cx.selected = Some(alone);
    let acts = m.extensions.actions(&cx);
    assert!(acts.iter().any(|a| a.label.contains("翼幅の揚力係数")));
    let text = m.extensions.summary(&cx, alone).unwrap();
    assert!(text.contains("渦格子法"), "{text}");
}

#[test]
fn section_flow_around_a_cylinder_shows_speed_and_drag() {
    let m = modules();
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let c = s.fixed_point(0.0, 0.0);
    s.circle(c, "20 mm", 10.0);
    let sk = b.feature(kmb_sketch::SKETCH, "円", s.build());
    let cyl = b.feature(
        EXTRUDE,
        "円柱",
        Extrude {
            profile: Some(sk),
            distance: "30 mm".into(),
            direction: Direction::Normal,
            operation: Operation::New,
            ..Default::default()
        },
    );
    let mut doc = b.finish("円柱");
    // 空気 0.1 m/s・直径 20 mm で Re 約 130(渦が出始める)
    let study = add(
        &mut doc,
        kmb_flow::SECTION,
        "2D の流れ 1",
        json!({"bodies": [body(cyl)], "plane": "xy", "dir": "+x", "speed": "0.1", "fluid": "air", "size": "1.5 mm"}),
    );
    let r = result(&m, &mut doc, study);
    let cd = number(&r, "抗力係数");
    assert!((1.0..2.2).contains(&cd), "円柱の抗力係数 {cd}");
    // 時刻を進めて解き、コマを残す(時刻は増えていく)
    assert!(r.frames.len() >= 10, "コマ {}", r.frames.len());
    assert!(r.frames.windows(2).all(|w| w[0].time < w[1].time));
    assert!(
        !r.series.is_empty() && r.series[0].points.len() > 3,
        "抗力の推移"
    );
    run(&m, &mut doc, study);
    let rep = doc.recompute(&m.registry, m.kernel.as_ref());
    let mut cx = Context::new(doc.data(), &rep, &m.registry, &m.kernel);
    cx.selected = Some(study);
    let shape = m.extensions.overlay(&cx).shape.expect("色分けの形");
    assert!(shape.label.contains("速さ"), "{}", shape.label);
    assert!(shape.badge.starts_with("2D・格子 1.5"), "{}", shape.badge);
    // ボディは灰色(色なし)で出る
    assert!(shape.mesh.triangle_faces.contains(&gray(&shape)));
    // 時刻の帯: コマの数だけ並び、最初は最後のコマ(落ち着いた流れ)
    let bar = m.extensions.frames(&cx).expect("時刻の帯");
    assert_eq!(bar.labels.len(), r.frames.len());
    assert_eq!(bar.at, bar.labels.len() - 1);
    assert!(bar.playable);
    // 最初のコマへ送ると、見出しの時刻が変わる
    m.extensions.run(&format!("{}0", bar.seek), &cx).unwrap();
    let first = m.extensions.overlay(&cx).shape.expect("最初のコマ");
    assert!(first.label.contains(&bar.labels[0]), "{}", first.label);
    // 流線を出すと、流線の色の凡例が足される
    let acts = m.extensions.actions(&cx);
    let on = acts
        .iter()
        .find(|a| a.label == "流線を出す")
        .expect("流線の操作");
    m.extensions.run(&on.id, &cx).unwrap();
    let lines = m.extensions.overlay(&cx).shape.expect("流線");
    assert!(
        lines.more_legends.iter().any(|(t, _)| t.contains("流線")),
        "{:?}",
        lines.more_legends.iter().map(|l| &l.0).collect::<Vec<_>>()
    );
}

#[test]
fn volume_flow_around_a_box_gives_a_drag_coefficient() {
    let m = modules();
    let mut b = DocBuilder::new();
    let boxy = plate(
        &mut b,
        "箱",
        -10.0,
        -10.0,
        "20 mm",
        "20 mm",
        [20.0, 20.0],
        "20 mm",
    );
    let mut doc = b.finish("箱");
    let study = add(
        &mut doc,
        kmb_flow::VOLUME,
        "3D の流れ 1",
        json!({"bodies": [body(boxy)], "plane": "xz", "dir": "+x", "speed": "1", "fluid": "air", "size": "3.3 mm"}),
    );
    let r = result(&m, &mut doc, study);
    // 立方体の抗力係数は高い Re で約 1.05(計算は低い Re に下げるので大きめ)
    let cd = number(&r, "抗力係数");
    assert!((0.8..2.5).contains(&cd), "立方体の抗力係数 {cd}");
    assert!(r.summary.contains("抗力係数"));
    // 表面の圧力: 正面(-X の面)は押され(正)、横や後ろは吸われる(負)
    let Some(kmb_study::Geometry::Surface { points, .. }) = r.parts.first().map(|p| &p.geom) else {
        panic!("表面の網が無い")
    };
    let p = values(&r, 0);
    let front: Vec<f32> = points
        .iter()
        .zip(&p)
        .filter(|(q, v)| {
            q[0] < -9.9 && q[1].abs() < 6.0 && q[2] > 4.0 && q[2] < 16.0 && v.is_finite()
        })
        .map(|(_, v)| *v)
        .collect();
    assert!(!front.is_empty());
    let mean = front.iter().sum::<f32>() / front.len() as f32;
    assert!(mean > 0.0, "正面の圧力 {mean} Pa");
    // 表面の圧力は、どのコマでも流れの止まる所の圧力(動圧 ½ρV² ≈ 0.60 Pa、Cp = 1)ほどまで。
    // 一度に風を当てたり出口で音の波が跳ね返ったりすると、その何倍にもなる
    let q = 0.5 * 1.204 * 1.0 * 1.0;
    for f in 0..r.frames.len() {
        let v = r.values(f, 0).unwrap();
        let top = (0..v.len())
            .map(|i| v.at(i))
            .filter(|x| x.is_finite())
            .fold(f32::MIN, f32::max);
        assert!(top < 1.3 * q as f32, "コマ {f} の表面の圧力の上限 {top} Pa");
    }
    // 断面の向きと位置は見せ方なので、直しても解き直さない(結果は古くならない)
    run(&m, &mut doc, study);
    let mut data = doc.data().feature(study).unwrap().data.clone();
    data["plane"] = json!("xy");
    data["at"] = json!("5 mm");
    doc.apply(Transaction::new(
        "断面",
        vec![Op::SetFeatureData { id: study, data }],
    ))
    .unwrap();
    let rep = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = Context::new(doc.data(), &rep, &m.registry, &m.kernel);
    let st = m.extensions.run_state(&cx, study).expect("様子");
    assert_eq!(st.tone, kmb_core::RunTone::Done, "{}", st.badge);
}
