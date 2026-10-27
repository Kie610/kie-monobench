//! 出現(同じ形を別の位置に置いたボディ。コアの `Feature::instance_of` と `Kernel::instance`)の
//! 読み方。出現の機能は kmb-asm の物の代わりに、ここで試験だけの物を作る。

use super::*;
use crate::tests::{TestKernel, assert_ok, rect_sketch, registry};
use kmb_cad_modify::MOVE;
use kmb_core::kernel::translation;
use kmb_core::{
    Document, EvalCtx, EvalError, Feature, FeatureId, FeatureStatus, InputRef, Kernel, Outputs,
    RecomputeReport, Registry, ShapeHandle, Value, body_root, visible_bodies, warnings_of,
};
use kmb_samples::sample::DocBuilder;
use kmb_sketch::SketchPlane;
use serde::Deserialize;
use serde_json::json;

const OCCURRENCE: &str = "test.occurrence@1";

/// 元のボディ `of` を x 方向に `dx` ずらして置く出現。
#[derive(Deserialize)]
struct Occurrence {
    of: u64,
    dx: f64,
}

impl Feature for Occurrence {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![kmb_cad::output(FeatureId(self.of), "body")])
    }

    fn instance_of(&self) -> Option<InputRef> {
        Some(kmb_cad::output(FeatureId(self.of), "body"))
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let src = kmb_cad::body_of(ctx, FeatureId(self.of))?;
        let h = ctx
            .kernel()
            .instance(src, &translation([self.dx, 0.0, 0.0]))
            .map_err(|e| EvalError::Kernel(e.to_string()))?;
        Ok([("body".to_string(), Value::Shape(h))].into())
    }
}

fn occurrence_registry() -> Registry {
    let mut r = registry();
    r.register(OCCURRENCE, |v| {
        serde_json::from_value::<Occurrence>(v.clone())
            .map(|f| Box::new(f) as Box<dyn Feature>)
            .map_err(|e| EvalError::Other(e.to_string()))
    });
    r
}

/// 10 mm 角の立方体(原点から)。押し出しの機能を返す。
fn cube(b: &mut DocBuilder) -> FeatureId {
    let s = rect_sketch(b, SketchPlane::Xy, 0.0, 0.0, 10.0, 10.0);
    b.feature(
        EXTRUDE,
        "立方体",
        Extrude {
            profile: Some(s),
            distance: "10 mm".into(),
            ..Default::default()
        },
    )
}

/// 縦の辺 1 本(x と y が 10 の角)を半径 3 mm で丸める。
fn round_corner(
    b: &mut DocBuilder,
    k: &dyn Kernel,
    target: FeatureId,
    shape: ShapeHandle,
) -> FeatureId {
    let edge = k
        .edges(shape)
        .unwrap()
        .into_iter()
        .find(|e| {
            (e.a[0] - 10.0).abs() < 1e-6
                && (e.b[0] - 10.0).abs() < 1e-6
                && (e.a[1] - 10.0).abs() < 1e-6
                && (e.b[1] - 10.0).abs() < 1e-6
        })
        .expect("縦の辺");
    b.feature(
        FILLET,
        "丸め",
        json!({"target": target.0, "edges": [edge.faces], "radius": "3 mm"}),
    )
}

fn shape_of(
    doc: &Document,
    reg: &Registry,
    report: &RecomputeReport,
    root: FeatureId,
) -> ShapeHandle {
    let data = doc.data();
    visible_bodies(data, reg, report)
        .into_iter()
        .find(|(id, _)| body_root(data, reg, *id) == root)
        .map(|(_, h)| h)
        .expect("見えているボディ")
}

fn recompute(doc: &mut Document, reg: &Registry, k: &dyn Kernel) -> RecomputeReport {
    doc.recompute(reg, k)
}

/// 箱の 1 つ目の立方体の形(丸めの辺を探すのに使う)。
fn cube_shape(k: &dyn Kernel) -> ShapeHandle {
    let reg = occurrence_registry();
    let mut b = DocBuilder::new();
    let a = cube(&mut b);
    let mut doc = b.finish("立方体");
    let r = recompute(&mut doc, &reg, k);
    shape_of(&doc, &reg, &r, a)
}

#[test]
fn an_occurrence_follows_a_fillet_added_to_its_source_later() {
    let k = TestKernel::new();
    let reg = occurrence_registry();
    let first = cube_shape(&*k);
    let mut b = DocBuilder::new();
    let a = cube(&mut b);
    let o = b.feature(OCCURRENCE, "立方体:2", json!({"of": a.0, "dx": 50.0}));
    round_corner(&mut b, &*k, a, first);
    // 丸めの後で出現を動かす機能は、丸めた形を出現の位置で読む
    b.feature(MOVE, "出現を上げる", json!({"body": o.0, "z": "20 mm"}));
    let mut doc = b.finish("出現");
    let r = recompute(&mut doc, &reg, &*k);
    assert_ok(&r);
    let (sa, so) = (shape_of(&doc, &reg, &r, a), shape_of(&doc, &reg, &r, o));
    let (ma, mo) = (k.mass(sa).unwrap(), k.mass(so).unwrap());
    assert!(ma.volume < 1000.0 - 1.0, "丸めた: {}", ma.volume);
    // (truck は体積を網で求めるので、置いた位置で少し違う)
    assert!(
        (mo.volume - ma.volume).abs() < 1e-5 * ma.volume,
        "{} vs {}",
        mo.volume,
        ma.volume
    );
    assert!(
        (mo.min[0] - 50.0).abs() < 1e-6 && (mo.min[2] - 20.0).abs() < 1e-6,
        "{:?}",
        mo.min
    );
    assert!(ma.min[2].abs() < 1e-6, "{:?}", ma.min);
    // 形は写さない: 出現は元の形を置いた物
    let base = |h| k.instance_of(h).map_or(h, |(b, _)| b);
    assert_eq!(base(so), base(sa));
    // 2 回目は全部使い回す
    let again = recompute(&mut doc, &reg, &*k);
    assert_eq!(again.evaluated, 0, "{again:?}");
}

#[test]
fn moving_the_source_does_not_move_its_occurrence_and_back() {
    let k = TestKernel::new();
    let reg = occurrence_registry();
    let mut b = DocBuilder::new();
    let a = cube(&mut b);
    b.feature(MOVE, "元を上げる", json!({"body": a.0, "z": "20 mm"}));
    let o = b.feature(OCCURRENCE, "立方体:2", json!({"of": a.0, "dx": 50.0}));
    b.feature(MOVE, "元をずらす", json!({"body": a.0, "y": "30 mm"}));
    b.feature(MOVE, "出現を下げる", json!({"body": o.0, "z": "-5 mm"}));
    let mut doc = b.finish("出現");
    let r = recompute(&mut doc, &reg, &*k);
    assert_ok(&r);
    let ma = k.mass(shape_of(&doc, &reg, &r, a)).unwrap();
    let mo = k.mass(shape_of(&doc, &reg, &r, o)).unwrap();
    // 出現は元の定義の座標(動かす前)から置く。元の移動も、出現の移動も、互いに移らない
    assert!(
        (ma.min[0]).abs() < 1e-6
            && (ma.min[1] - 30.0).abs() < 1e-6
            && (ma.min[2] - 20.0).abs() < 1e-6,
        "{:?}",
        ma.min
    );
    assert!(
        (mo.min[0] - 50.0).abs() < 1e-6 && mo.min[1].abs() < 1e-6 && (mo.min[2] + 5.0).abs() < 1e-6,
        "{:?}",
        mo.min
    );
}

#[test]
fn changing_the_shape_of_an_occurrence_is_an_error_that_points_to_the_source() {
    let k = TestKernel::new();
    let reg = occurrence_registry();
    let first = cube_shape(&*k);
    let mut b = DocBuilder::new();
    let a = cube(&mut b);
    let o = b.feature(OCCURRENCE, "立方体:2", json!({"of": a.0, "dx": 0.0}));
    let f = round_corner(&mut b, &*k, o, first);
    let mut doc = b.finish("出現");
    let r = recompute(&mut doc, &reg, &*k);
    let st = &r.features.iter().find(|x| x.id == f).unwrap().status;
    let FeatureStatus::Error { message } = st else {
        panic!("誤りになるはず: {st:?}")
    };
    assert!(
        message.contains("元の部品") && message.contains("出現"),
        "{message}"
    );
}

#[test]
fn an_occurrence_of_a_source_joined_away_keeps_the_shape_before_and_warns() {
    let k = TestKernel::new();
    let reg = occurrence_registry();
    let mut b = DocBuilder::new();
    let a = cube(&mut b);
    let o = b.feature(OCCURRENCE, "立方体:2", json!({"of": a.0, "dx": 50.0}));
    // 立方体を横に突き抜ける板(面が同じ平面で接しないよう、上げて幅を狭める)
    let s = rect_sketch(&mut b, SketchPlane::Xy, 5.0, 2.0, 20.0, 6.0);
    let other = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            distance: "4 mm".into(),
            ..Default::default()
        },
    );
    b.feature(MOVE, "板を上げる", json!({"body": other.0, "z": "3 mm"}));
    b.feature(
        BOOLEAN,
        "足す",
        json!({"target": other.0, "tool": a.0, "operation": "join"}),
    );
    let mut doc = b.finish("出現");
    let r = recompute(&mut doc, &reg, &*k);
    assert_ok(&r);
    let mo = k.mass(shape_of(&doc, &reg, &r, o)).unwrap();
    assert!((mo.volume - 1000.0).abs() < 1e-2, "{}", mo.volume);
    let FeatureStatus::Ok { outputs, .. } = &r.features.iter().find(|x| x.id == o).unwrap().status
    else {
        unreachable!()
    };
    let w = warnings_of(outputs);
    assert!(w.iter().any(|m| m.contains("足す")), "{w:?}");
}

#[test]
fn a_joint_on_the_source_or_the_occurrence_moves_only_that_one() {
    let k = TestKernel::new();
    let reg = occurrence_registry();
    let turn = |root: FeatureId| {
        json!({"kind": "revolute", "moving": {"kind": "body", "root": root.0}, "bodies": [root.0],
            "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "angle": "90 deg"})
    };
    let mut b = DocBuilder::new();
    let a = cube(&mut b);
    let o = b.feature(OCCURRENCE, "立方体:2", json!({"of": a.0, "dx": 50.0}));
    b.feature(kmb_asm::JOINT, "元を回す", turn(a));
    b.feature(kmb_asm::JOINT, "出現を回す", turn(o));
    let mut doc = b.finish("出現");
    let r = recompute(&mut doc, &reg, &*k);
    assert_ok(&r);
    let ma = k.mass(shape_of(&doc, &reg, &r, a)).unwrap();
    let mo = k.mass(shape_of(&doc, &reg, &r, o)).unwrap();
    // 原点まわりに 90° 回すと、(x, y) は (-y, x) へ。元と出現は自分のジョイントだけで回る
    let near = |p: [f64; 3], q: [f64; 2]| (p[0] - q[0]).abs() < 1e-6 && (p[1] - q[1]).abs() < 1e-6;
    assert!(near(ma.min, [-10.0, 0.0]), "{:?}", ma.min);
    assert!(near(mo.min, [-10.0, 50.0]), "{:?}", mo.min);
}
