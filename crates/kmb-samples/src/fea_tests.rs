//! 強度と熱の解析の通しの試験: スケッチから押し出した棒に研究を付けて解き、寸法を変えると
//! 同じ面に条件が付き直って解き直されること、実行の前後と寸法を変えた後の様子(未実行・済み・古い)、
//! 3D のビューの色分けと凡例が出ること。

use crate::sample::{DocBuilder, SketchBuilder};
use kmb_cad::{Direction, Operation};
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_core::RunTone;
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

/// 長さ len(既定 100 mm)× 10 × 10 mm の棒。
fn bar() -> (Document, FeatureId) {
    let mut b = DocBuilder::new();
    b.param("len", "100 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "len", "10 mm", [0.0, 0.0, 100.0, 10.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "棒の輪郭", s.build());
    let bar = b.feature(
        EXTRUDE,
        "棒",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            direction: Direction::Normal,
            operation: Operation::New,
            ..Default::default()
        },
    );
    (b.finish("棒"), bar)
}

/// 棒の、外向きの法線が `n` の平らな面の名前。
fn face(m: &Modules, doc: &mut Document, body: FeatureId, n: [f64; 3]) -> String {
    let rep = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = Context::new(doc.data(), &rep, &m.registry, &m.kernel);
    let Some(kmb_core::Value::Shape(h)) = cx.output(body, "body") else {
        panic!("棒のボディが無い");
    };
    m.kernel
        .faces(*h)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.planar
                .is_some_and(|p| (0..3).all(|k| (p.normal[k] - n[k]).abs() < 1e-6))
        })
        .expect("面がある")
        .name
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

fn number(r: &SimResult, key: &str) -> f64 {
    let v = &r.numbers.iter().find(|(k, _)| k == key).expect(key).1;
    v.split_whitespace().next().unwrap().parse().unwrap()
}

fn face_ref(body: FeatureId, name: &str) -> serde_json::Value {
    json!({"kind": "face", "feature": body.0, "face": name})
}

#[test]
fn static_study_follows_the_dimension_change() {
    let m = modules();
    let (mut doc, body) = bar();
    let x0 = face(&m, &mut doc, body, [-1.0, 0.0, 0.0]);
    let x1 = face(&m, &mut doc, body, [1.0, 0.0, 0.0]);
    let study = add(
        &mut doc,
        kmb_fea::STATIC,
        "静的応力 1",
        json!({
            "material": "アルミ",
            "fixed": face_ref(body, &x0),
            "loads": [{"faces": face_ref(body, &x1), "kind": "force", "value": "100", "dir": "-z"}],
            "size": "2.5 mm",
        }),
    );
    let r = result(&m, &mut doc, study);
    // 片持ち梁: PL³/(3EI) + せん断の分(アルミ E 69000、ν 0.33)
    let (e, nu, i) = (69_000.0, 0.33, 10.0f64.powi(4) / 12.0);
    let want = |l: f64| {
        100.0 * l.powi(3) / (3.0 * e * i) + 100.0 * l / (5.0 / 6.0 * e / (2.0 * (1.0 + nu)) * 100.0)
    };
    let d100 = number(&r, "最大の変位");
    assert!(
        (d100 - want(100.0)).abs() / want(100.0) < 0.05,
        "変位 {d100} と {}",
        want(100.0)
    );
    assert!(number(&r, "最大のミーゼス応力") > 50.0);
    assert!(r.summary.contains("安全率"));
    // 画面の流れ: 研究は自動では解かず「未実行」。実行すると「済み」になり、色分けが出る
    let wait = json!({"wait": true});
    let tone = |doc: &mut Document| {
        let rep = doc.recompute(&m.registry, m.kernel.as_ref());
        let cx = Context::new(doc.data(), &rep, &m.registry, &m.kernel);
        m.extensions.run_state(&cx, study).expect("研究の様子").tone
    };
    assert_eq!(tone(&mut doc), RunTone::Idle);
    {
        let rep = doc.recompute(&m.registry, m.kernel.as_ref());
        let mut cx = Context::new(doc.data(), &rep, &m.registry, &m.kernel);
        cx.selected = Some(study);
        assert!(
            m.extensions.overlay(&cx).shape.is_none(),
            "解く前は色が無い"
        );
        cx.args = &wait;
        m.extensions.run(kmb_study::RUN, &cx).unwrap();
    }
    assert_eq!(tone(&mut doc), RunTone::Done);
    // 長さを 80 mm にすると、同じ端の面に条件が付き直り、解き直してたわみが減る
    let p = doc.data().param_by_name("len").unwrap().id;
    doc.apply(Transaction::new(
        "長さ",
        vec![Op::SetParamExpr {
            id: p,
            expr: "80 mm".into(),
        }],
    ))
    .unwrap();
    // 寸法を変えると、出している結果は「古い」になる(自動では解き直さない)
    assert_eq!(tone(&mut doc), RunTone::Stale);
    let r80 = result(&m, &mut doc, study);
    let d80 = number(&r80, "最大の変位");
    assert!(
        (d80 - want(80.0)).abs() / want(80.0) < 0.05,
        "変位 {d80} と {}",
        want(80.0)
    );
    // 古い結果は見出しに「古い」と出る。実行し直すと消える
    let rep = doc.recompute(&m.registry, m.kernel.as_ref());
    let mut cx = Context::new(doc.data(), &rep, &m.registry, &m.kernel);
    cx.selected = Some(study);
    let stale = m.extensions.overlay(&cx).shape.expect("色分けの形");
    assert!(stale.label.contains("古い"), "{}", stale.label);
    cx.args = &wait;
    m.extensions.run(kmb_study::RUN, &cx).unwrap();
    // 引数 wait は実行の操作だけが受け取る(ほかの操作に渡すと誤り)
    cx.args = &serde_json::Value::Null;
    // 3D のビューの色分け: 研究を選ぶと、色と凡例が出る
    let o = m.extensions.overlay(&cx);
    let shape = o.shape.expect("色分けの形");
    assert!(!shape.legend.is_empty());
    assert!(
        shape.label.contains("ミーゼス応力") && shape.label.contains("変形 ×"),
        "{}",
        shape.label
    );
    assert!(!shape.label.contains("古い"), "{}", shape.label);
    // 安全率に切り替える
    let acts = m.extensions.actions(&cx);
    let sf = acts
        .iter()
        .find(|a| a.label.contains("安全率"))
        .expect("安全率の操作");
    m.extensions.run(&sf.id, &cx).unwrap();
    let o = m.extensions.overlay(&cx);
    assert!(o.shape.unwrap().label.contains("安全率"));
}

#[test]
fn thermal_modal_and_topology_studies_solve() {
    let m = modules();
    let (mut doc, body) = bar();
    let x0 = face(&m, &mut doc, body, [-1.0, 0.0, 0.0]);
    let x1 = face(&m, &mut doc, body, [1.0, 0.0, 0.0]);
    let thermal = add(
        &mut doc,
        kmb_fea::THERMAL,
        "熱 1",
        json!({
            "material": "アルミ",
            "temps": [{"faces": face_ref(body, &x0), "value": "100"}],
            "rest_h": "50",
            "ambient": "20",
            "size": "2.5 mm",
        }),
    );
    let r = result(&m, &mut doc, thermal);
    assert!((number(&r, "最高の温度") - 100.0).abs() < 1e-6);
    let tmin = number(&r, "最低の温度");
    assert!(tmin > 60.0 && tmin < 80.0, "先の温度 {tmin}");
    let modal = add(
        &mut doc,
        kmb_fea::MODAL,
        "固有振動 1",
        json!({"material": "アルミ", "fixed": face_ref(body, &x0), "modes": "2", "size": "2.5 mm"}),
    );
    let r = result(&m, &mut doc, modal);
    // モードごとに 1 コマ(「1 次 815 Hz」)
    assert_eq!(r.frames.len(), 2);
    let f1: f64 = r.frames[0]
        .label
        .split_whitespace()
        .nth(2)
        .unwrap()
        .parse()
        .unwrap();
    assert!((f1 - 815.0).abs() < 40.0, "1 次 {f1} Hz");
    let topo = add(
        &mut doc,
        kmb_fea::TOPOLOGY,
        "軽い形 1",
        json!({
            "material": "アルミ",
            "fixed": face_ref(body, &x0),
            "loads": [{"faces": face_ref(body, &x1), "kind": "force", "value": "100", "dir": "-z"}],
            "volume": "50",
            "size": "5 mm",
        }),
    );
    let r = result(&m, &mut doc, topo);
    assert!(matches!(&r.parts[0].geom, kmb_study::Geometry::Tets(t) if !t.faces.is_empty()));
    assert!(r.summary.contains("残す体積"));
}

/// 穴・ポケット・ボスのある見本の部品でも網が作れて解ける(網の確かさの数値も出る)。値の確かさは
/// 上の棒と kmb-fea の検算で見るので、ここは網を粗く(4 mm)して解けることだけを確かめる。
#[test]
fn reference_part_static_and_modal_solve() {
    let m = modules();
    let mut doc = crate::reference_document();
    let rep = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = Context::new(doc.data(), &rep, &m.registry, &m.kernel);
    let body = doc
        .data()
        .features
        .iter()
        .rev()
        .find(|f| matches!(cx.output(f.id, "body"), Some(kmb_core::Value::Shape(_))))
        .unwrap()
        .id;
    // 板の +X の横の面を固定し、板の上の面を下へ押す
    let (side, top) = ("f8:side:e6", "f8:end");
    let study = add(
        &mut doc,
        kmb_fea::STATIC,
        "静的応力 1",
        json!({
            "material": "アルミ",
            "fixed": face_ref(body, side),
            "loads": [{"faces": face_ref(body, top), "kind": "force", "value": "100", "dir": "-z"}],
            "size": "4 mm",
        }),
    );
    let r = result(&m, &mut doc, study);
    assert!(number(&r, "最大の変位") > 0.0);
    assert!(number(&r, "最大のミーゼス応力") > 0.0);
    for k in ["網", "体積の差", "表面のずれ", "網の質"] {
        assert!(r.number(k).is_some(), "{k} が無い: {:?}", r.numbers);
    }
    eprintln!("見本の部品の網: {:?}", &r.numbers[r.numbers.len() - 4..]);
    let modal = add(
        &mut doc,
        kmb_fea::MODAL,
        "固有振動 1",
        json!({"material": "アルミ", "fixed": face_ref(body, side), "modes": "2", "size": "4 mm"}),
    );
    let r = result(&m, &mut doc, modal);
    assert_eq!(r.frames.len(), 2);
}
