//! 部品を動かした後と、分かれた面が 1 枚に戻った後も、付けた物が付いて来る。
//! - 部品の中の作業平面・作業軸は、部品を動かすと一緒に動く(動かした後に置いたスケッチ・回転も)
//! - 溝で 2 つに分かれた面の片方に置いたスケッチは、溝を消すと戻った 1 枚の面に残る

use super::*;
use kmb_asm::{AsmCommands, MOVE_PART, Position};
use kmb_cad_datum::{AxisMethod, Base, DATUM_AXIS, DATUM_PLANE, DatumAxisFeature, Method};
use kmb_core::{
    Component, Context, Document, Edit, Extension, FeatureId, FeatureStatus, Kernel, Op, Outcome,
    Payload, Registry, Transaction, body_root,
};
use kmb_interchange::{DATUM_OUTPUT, DatumAxis, DatumPlane};
use kmb_kernel_smlib::SmlibKernel;
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use serde_json::json;
use std::sync::Arc;

fn reg() -> Registry {
    let mut r = Registry::new();
    kmb_sketch::register(&mut r);
    register(&mut r);
    kmb_cad_datum::register(&mut r);
    r
}

struct Bench {
    doc: Document,
    reg: Registry,
    k: Arc<dyn Kernel>,
    report: kmb_core::RecomputeReport,
}

impl Bench {
    fn new(doc: Document) -> Self {
        let mut t = Bench {
            doc,
            reg: reg(),
            k: Arc::new(SmlibKernel::new()),
            report: Default::default(),
        };
        t.recompute();
        t
    }

    fn recompute(&mut self) {
        self.report = self.doc.recompute(&self.reg, &*self.k);
        crate::tests::assert_ok(&self.report);
    }

    fn insert(&mut self, type_id: &str, data: impl serde::Serialize) -> FeatureId {
        let id = self
            .doc
            .apply_edits(
                "機能",
                vec![Edit::Insert {
                    type_id: type_id.into(),
                    name: "試験".into(),
                    data: serde_json::to_value(data).unwrap(),
                }],
            )
            .unwrap()[0];
        self.recompute();
        id
    }

    fn outputs(&self, id: FeatureId) -> &kmb_core::Outputs {
        match self.report.feature(id) {
            Some(FeatureStatus::Ok { outputs, .. }) => outputs,
            other => panic!("{id}: {other:?}"),
        }
    }

    fn bounds(&self, id: FeatureId) -> ([f64; 3], [f64; 3]) {
        match self.outputs(id)["body"] {
            kmb_core::Value::Shape(h) => self.k.bounds(h).unwrap(),
            ref v => panic!("{v:?}"),
        }
    }

    fn datum<T: Payload>(&self, id: FeatureId) -> T {
        T::from_value(&self.outputs(id)[DATUM_OUTPUT]).expect("作業物")
    }

    /// ボディ `last` を部品にし、部品を移動の操作で X に `dx` mm 動かす(位置の機能を足す)。
    fn move_part(&mut self, last: FeatureId, dx: f64) -> FeatureId {
        self.move_part_with(last, |p| p.x = format!("{dx} mm"))
    }

    /// ボディ `last` を部品にし、部品を移動の操作で足した位置の機能を `set` で直して足す。
    fn move_part_with(&mut self, last: FeatureId, set: impl FnOnce(&mut Position)) -> FeatureId {
        let root = body_root(self.doc.data(), &self.reg, last);
        if self.doc.data().component_of(root).is_none() {
            let c = self.doc.alloc_group_id();
            self.doc
                .apply(Transaction::new(
                    "部品",
                    vec![Op::SetComponents {
                        components: vec![Component {
                            id: c,
                            name: "部品".into(),
                            parent: None,
                            bodies: vec![root],
                        }],
                    }],
                ))
                .unwrap();
            self.recompute();
        }
        let args = json!({"bodies": [root.0]});
        let cx = Context {
            args: &args,
            ..Context::new(self.doc.data(), &self.report, &self.reg, &self.k)
        };
        let Some(Ok(Outcome::Change { edits, .. })) = AsmCommands.run(MOVE_PART, &cx) else {
            panic!("部品を動かせない");
        };
        let [Edit::Insert { type_id, data, .. }] = edits.as_slice() else {
            panic!("{edits:?}");
        };
        let mut p: Position = serde_json::from_value(data.clone()).unwrap();
        set(&mut p);
        let type_id = type_id.clone();
        self.insert(&type_id, p)
    }
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3)
}

/// 長方形 (x, y, w, h) を左下を固定して描いたスケッチ。
fn rect(plane: SketchPlane, [x, y, w, h]: [f64; 4]) -> kmb_sketch::Sketch {
    let mut s = SketchBuilder::new(plane);
    let o = s.fixed_point(x, y);
    s.rectangle(
        o,
        &format!("{w} mm"),
        &format!("{h} mm"),
        [x, y, x + w, y + h],
    );
    s.build()
}

/// 箱(30 × 20 × 10)。押し出しの機能を返す。
fn boxed(b: &mut DocBuilder) -> FeatureId {
    let s = b.feature(
        kmb_sketch::SKETCH,
        "箱の形",
        rect(SketchPlane::Xy, [0.0, 0.0, 30.0, 20.0]),
    );
    b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(s),
            distance: "10 mm".into(),
            ..Default::default()
        },
    )
}

fn join(profile: FeatureId, target: FeatureId, distance: &str, direction: Direction) -> Extrude {
    Extrude {
        profile: Some(profile),
        distance: distance.into(),
        direction,
        operation: Operation::Join,
        target: Some(target),
        ..Default::default()
    }
}

/// 箱の上面から 5 mm 浮かせた作業平面に置いた 6 × 6 の四角を、下へ 7 mm 押し出して箱に足す。
/// 部品を X に 50 mm 動かすのが、作業平面を作った後の、スケッチの前(`before` = 1)・押し出しの前(2)・
/// 全部の後(3)のどれでも、足した形は同じ所(動かした箱の上)に出る。作業平面の四角も動いた先に出る。
/// (作業平面より前に動かした時は、動いた後の面から作るので、スケッチは動いた先で描く。比べない)
#[test]
fn a_sketch_on_a_datum_plane_of_a_moved_part_moves_with_the_part() {
    let mut want = None;
    for before in 1..4 {
        let mut b = DocBuilder::new();
        let e = boxed(&mut b);
        let mut t = Bench::new(b.finish("箱"));
        let mut moved = None;
        let mut step = |t: &mut Bench, at: usize| {
            if at == before {
                moved = Some(t.move_part(e, 50.0));
            }
        };
        let d = t.insert(
            DATUM_PLANE,
            kmb_cad_datum::DatumPlaneFeature {
                method: Method::Offset,
                base: Some(Base::Face {
                    feature: e,
                    face: format!("f{}:end", e.0),
                }),
                distance: "5 mm".into(),
                ..Default::default()
            },
        );
        step(&mut t, 1);
        let s = t.insert(
            kmb_sketch::SKETCH,
            rect(SketchPlane::Datum { feature: d }, [5.0, 5.0, 6.0, 6.0]),
        );
        step(&mut t, 2);
        let x = t.insert(EXTRUDE, join(s, e, "7 mm", Direction::Reverse));
        step(&mut t, 3);
        assert!(moved.is_some());
        // 動かした後の形は、押し出しの結果の並びにも入る(後に動かした時も)
        let got = t.bounds(x);
        let plane: DatumPlane = t.datum(d);
        assert!(
            got.0[0] > 49.9 && got.1[0] < 80.1 && (got.1[2] - 15.0).abs() < 1e-3,
            "動かす位置 {before}: 足した形が動いた箱の上に無い {got:?}"
        );
        assert!(
            plane.plane.origin[0] > 49.9 && (plane.plane.origin[2] - 15.0).abs() < 1e-6,
            "動かす位置 {before}: 作業平面が動いた先に無い {:?}",
            plane.plane.origin
        );
        let profile = kmb_core::Profile::from_value(&t.outputs(s)["profile"]).expect("輪郭");
        assert!(
            profile.plane.origin[0] > 49.9,
            "動かす位置 {before}: スケッチが動いた先に無い {:?}",
            profile.plane.origin
        );
        match want {
            None => want = Some(got),
            Some(w) => assert!(
                near(got.0, w.0) && near(got.1, w.1),
                "{before}: {got:?} {w:?}"
            ),
        }
    }
}

/// 箱の右手前の縦の辺の作業軸で、ZX 平面のスケッチ(軸から 2〜6 mm 離れた四角)を 90° 回す。
/// 部品を X に 50 mm 動かすのが作業軸の前でも後でも、回した形は動いた辺のまわりにできる。
#[test]
fn a_revolve_about_a_datum_axis_of_a_moved_part_turns_about_the_moved_edge() {
    for axis_first in [false, true] {
        let mut b = DocBuilder::new();
        let e = boxed(&mut b);
        let mut t = Bench::new(b.finish("箱"));
        if !axis_first {
            t.move_part(e, 50.0);
        }
        // 動かす前の箱で辺の名前を探す(名前は動かしても変わらない)
        let probe = {
            let mut b = DocBuilder::new();
            let e2 = boxed(&mut b);
            let p = Bench::new(b.finish("確かめ"));
            let h = match p.outputs(e2)["body"] {
                kmb_core::Value::Shape(h) => h,
                _ => unreachable!(),
            };
            p.k.edges(h).unwrap()
        };
        let faces = probe
            .into_iter()
            .find(|g| {
                g.arc.is_none()
                    && [g.a, g.b]
                        .iter()
                        .all(|p| near([p[0], p[1], 0.0], [30.0, 0.0, 0.0]))
            })
            .expect("右手前の縦の辺")
            .faces;
        let axis = t.insert(
            DATUM_AXIS,
            DatumAxisFeature {
                method: AxisMethod::Edge,
                edge: Some(kmb_cad_datum::axis::EdgeRef { feature: e, faces }),
                ..Default::default()
            },
        );
        if axis_first {
            t.move_part(e, 50.0);
        }
        let a: DatumAxis = t.datum(axis);
        assert!(
            near([a.at[0], a.at[1], 0.0], [80.0, 0.0, 0.0]),
            "軸を作るのが動かす{}: 作業軸が動いた辺に無い {a:?}",
            if axis_first { "前" } else { "後" }
        );
        // ZX 平面(Y = 0)のスケッチ。スケッチの X は世界の Z、Y は世界の X
        let s = t.insert(
            kmb_sketch::SKETCH,
            rect(SketchPlane::Zx, [0.0, 82.0, 5.0, 4.0]),
        );
        let r = t.insert(
            REVOLVE,
            Revolve {
                profile: Some(s),
                axis: Axis::Datum { feature: axis },
                angle: "90 deg".into(),
                operation: Operation::New,
                ..Default::default()
            },
        );
        let (lo, hi) = t.bounds(r);
        assert!(
            lo[0] > 79.9 && hi[0] < 86.1 && (hi[1] - lo[1] - 6.0).abs() < 1e-3,
            "軸を作るのが動かす{}: 回した形が動いた辺のまわりに無い {lo:?} {hi:?}",
            if axis_first { "前" } else { "後" }
        );
    }
}

/// 板(30 × 20 × 10)の上面を溝(y 8〜12)で 2 つに分け、奥の片方に 4 × 4 の四角を置いて 3 mm 押し出す。
/// 溝を消すと上面は 1 枚に戻り、スケッチと押し出しは戻った面の同じ所に残る(誤りにしない)。戻った面を
/// 指していると注意で知らせる。
#[test]
fn a_sketch_on_one_half_of_a_split_face_stays_when_the_face_is_whole_again() {
    let mut b = DocBuilder::new();
    let plate = boxed(&mut b);
    let groove_sk = b.feature(
        kmb_sketch::SKETCH,
        "溝の形",
        rect(SketchPlane::Xy, [-1.0, 8.0, 32.0, 4.0]),
    );
    let groove = b.feature(
        EXTRUDE,
        "溝",
        Extrude {
            profile: Some(groove_sk),
            distance: "10 mm".into(),
            start: Start::Offset,
            start_offset: Some("5 mm".into()),
            operation: Operation::Cut,
            target: Some(plate),
            ..Default::default()
        },
    );
    let mut t = Bench::new(b.finish("板"));
    let body = match t.outputs(groove)["body"] {
        kmb_core::Value::Shape(h) => h,
        _ => unreachable!(),
    };
    // 奥の切れ端(y 12〜20)の名前(辺の端が y > 12 にある方)
    let edges = t.k.edges(body).unwrap();
    let back = ["#0", "#1"]
        .map(|k| format!("f{}:end{k}", plate.0))
        .into_iter()
        .find(|n| {
            edges
                .iter()
                .filter(|e| e.faces.contains(n))
                .all(|e| e.a[1] > 11.9 && e.b[1] > 11.9)
        })
        .expect("奥の切れ端");
    let s = t.insert(
        kmb_sketch::SKETCH,
        rect(
            SketchPlane::Face {
                feature: groove,
                face: back.clone(),
                origin: kmb_sketch::FaceOrigin::World,
            },
            [5.0, 14.0, 4.0, 4.0],
        ),
    );
    let up = t.insert(EXTRUDE, join(s, groove, "3 mm", Direction::Normal));
    let before = t.bounds(up);
    // 溝を止める(消すのと同じく、上面は 1 枚に戻る)
    t.doc
        .apply(Transaction::new(
            "溝を止める",
            vec![Op::SetSuppressed {
                id: groove,
                suppressed: true,
            }],
        ))
        .unwrap();
    t.report = t.doc.recompute(&t.reg, &*t.k);
    let st = t.report.feature(s).cloned();
    let Some(FeatureStatus::Ok { outputs, .. }) = &st else {
        panic!("1 枚に戻った面のスケッチが誤りになった: {st:?}");
    };
    let warnings = kmb_core::warnings_of(outputs);
    assert!(
        warnings.iter().any(|w| w.contains(&back)),
        "戻った面を指している注意が無い: {warnings:?}"
    );
    let after = t.bounds(up);
    assert!(
        near(after.0, before.0) && near(after.1, before.1),
        "足した形の位置が変わった {before:?} {after:?}"
    );
}

/// 回して置いた部品の、動かす前に作った作業平面からオフセット平面を作ると、回した後の向きの平面になる。
/// 回した部品の面から作っても同じ。箱の右の側面(法線 +X)は、Z のまわりに 90° 回すと法線 +Y になる。
#[test]
fn an_offset_plane_from_a_turned_part_uses_the_turned_plane() {
    let mut b = DocBuilder::new();
    let e = boxed(&mut b);
    let mut t = Bench::new(b.finish("箱"));
    let right =
        t.k.faces(match t.outputs(e)["body"] {
            kmb_core::Value::Shape(h) => h,
            _ => unreachable!(),
        })
        .unwrap()
        .into_iter()
        .find(|f| f.planar.as_ref().is_some_and(|p| p.normal[0] > 0.99))
        .expect("右の側面")
        .name;
    let side = Base::Face {
        feature: e,
        face: right,
    };
    let offset = |base: Base| kmb_cad_datum::DatumPlaneFeature {
        method: Method::Offset,
        base: Some(base),
        distance: "5 mm".into(),
        ..Default::default()
    };
    let inner = t.insert(DATUM_PLANE, offset(side.clone()));
    t.move_part_with(e, |p| p.rz = "90 deg".into());
    let from_datum = t.insert(DATUM_PLANE, offset(Base::Datum { feature: inner }));
    let from_face = t.insert(DATUM_PLANE, offset(side));
    for (what, id, dist) in [
        ("動かす前の作業平面", inner, 5.0),
        ("作業平面から", from_datum, 10.0),
        ("回した面から", from_face, 5.0),
    ] {
        let p: DatumPlane = t.datum(id);
        let n = p.plane.normal();
        assert!(n[1] > 0.99, "{what}: 回した後の向き(+Y)でない {n:?}");
        // 箱の中心 (15, 10) のまわりに回すと、右の側面は y = 10 + 15 = 25 に来る
        assert!(
            (p.plane.origin[1] - (25.0 + dist)).abs() < 1e-6,
            "{what}: {:?}",
            p.plane.origin
        );
    }
}

/// 動かしたボディの辺を回転の軸にすると、動いた先の辺のまわりに回る(ボディの移動でも、部品の移動でも)。
#[test]
fn a_revolve_about_an_edge_of_a_moved_body_turns_about_the_moved_edge() {
    for by_part in [false, true] {
        let mut b = DocBuilder::new();
        let e = boxed(&mut b);
        let mut t = Bench::new(b.finish("箱"));
        let faces = {
            let h = match t.outputs(e)["body"] {
                kmb_core::Value::Shape(h) => h,
                _ => unreachable!(),
            };
            t.k.edges(h)
                .unwrap()
                .into_iter()
                .find(|g| {
                    g.arc.is_none()
                        && [g.a, g.b]
                            .iter()
                            .all(|p| near([p[0], p[1], 0.0], [30.0, 0.0, 0.0]))
                })
                .expect("右手前の縦の辺")
                .faces
        };
        if by_part {
            t.move_part(e, 50.0);
        } else {
            t.insert(kmb_cad_modify::MOVE, json!({"body": e.0, "x": "50 mm"}));
        }
        let s = t.insert(
            kmb_sketch::SKETCH,
            rect(SketchPlane::Zx, [0.0, 82.0, 5.0, 4.0]),
        );
        let r = t.insert(
            REVOLVE,
            Revolve {
                profile: Some(s),
                axis: Axis::Edge { feature: e, faces },
                angle: "90 deg".into(),
                operation: Operation::New,
                ..Default::default()
            },
        );
        let (lo, hi) = t.bounds(r);
        assert!(
            lo[0] > 79.9 && hi[0] < 86.1 && (hi[1] - lo[1] - 6.0).abs() < 1e-3,
            "{}: 回した形が動いた辺のまわりに無い {lo:?} {hi:?}",
            if by_part {
                "部品の移動"
            } else {
                "ボディの移動"
            }
        );
    }
}

/// ボディを写して動かした後、写しの上面に描いたスケッチを押し出して足す。元の箱の幅を変えて作り直しても、
/// 写しは置いた所(X に 50 mm)に残り、足した形も写しの上の同じ所に残る。
#[test]
fn a_sketch_on_a_moved_copy_stays_on_the_copy_when_a_size_changes() {
    let mut b = DocBuilder::new();
    b.param("w", "30 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "w", "20 mm", [0.0, 0.0, 30.0, 20.0]);
    let sk = b.sketch("箱の形", s);
    let e = b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let copy = b.feature(
        kmb_cad_modify::MOVE,
        "写して動かす",
        json!({"body": e.0, "x": "50 mm", "copy": true}),
    );
    let mut t = Bench::new(b.finish("箱"));
    let s = t.insert(
        kmb_sketch::SKETCH,
        rect(
            SketchPlane::Face {
                feature: copy,
                face: format!("f{}:end", e.0),
                origin: kmb_sketch::FaceOrigin::World,
            },
            [55.0, 5.0, 4.0, 4.0],
        ),
    );
    let up = t.insert(EXTRUDE, join(s, copy, "3 mm", Direction::Normal));
    for (w, right) in [("30 mm", 80.0), ("40 mm", 90.0)] {
        let id = t.doc.data().param_by_name("w").unwrap().id;
        t.doc
            .apply(Transaction::new(
                "幅",
                vec![Op::SetParamExpr { id, expr: w.into() }],
            ))
            .unwrap();
        t.recompute();
        let (lo, hi) = t.bounds(up);
        assert!(
            near(lo, [50.0, 0.0, 0.0]) && near(hi, [right, 20.0, 13.0]),
            "幅 {w}: 写しと足した形が置いた所に無い {lo:?} {hi:?}"
        );
        assert!(near(t.bounds(e).0, [0.0; 3]), "元の箱は動かない");
    }
}

/// ボディ `id` の、法線 `n` の向きの平らな面の名前(そのような面のうち `pick` で選ぶ物)。
fn face_toward(
    t: &Bench,
    id: FeatureId,
    n: [f64; 3],
    pick: impl Fn(&kmb_core::FaceInfo) -> bool,
) -> String {
    let h = match t.outputs(id)["body"] {
        kmb_core::Value::Shape(h) => h,
        ref v => panic!("{v:?}"),
    };
    let faces = t.k.faces(h).unwrap();
    faces
        .iter()
        .find(|f| {
            f.planar
                .as_ref()
                .is_some_and(|p| (0..3).all(|i| (p.normal[i] - n[i]).abs() < 1e-6))
                && pick(f)
        })
        .unwrap_or_else(|| {
            panic!(
                "法線 {n:?} の面: {:?}",
                faces.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        })
        .name
        .clone()
}

fn set_param(t: &mut Bench, name: &str, expr: &str) {
    let id = t.doc.data().param_by_name(name).unwrap().id;
    t.doc
        .apply(Transaction::new(
            name,
            vec![Op::SetParamExpr {
                id,
                expr: expr.into(),
            }],
        ))
        .unwrap();
    t.recompute();
}

/// 高さ `h` の箱(30 × 20)。
fn tall_box(b: &mut DocBuilder) -> FeatureId {
    b.param("h", "10 mm");
    let s = b.feature(
        kmb_sketch::SKETCH,
        "箱の形",
        rect(SketchPlane::Xy, [0.0, 0.0, 30.0, 20.0]),
    );
    b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(s),
            distance: "h".into(),
            ..Default::default()
        },
    )
}

/// 上の手前の辺を丸めた箱の手前の面に四角を置いて 3 mm 押し出す。箱を高くしても、押し出しは手前の面の
/// 同じ所に残る(丸めた辺に替わった面の名前で別の面へ移らない)。
#[test]
fn a_sketch_beside_a_rounded_edge_stays_on_its_face_when_the_size_changes() {
    let mut b = DocBuilder::new();
    let e = tall_box(&mut b);
    let mut t = Bench::new(b.finish("箱"));
    let front = face_toward(&t, e, [0.0, -1.0, 0.0], |_| true);
    let round = t.insert(
        FILLET,
        json!({"target": e.0, "edges": [[format!("f{}:end", e.0), front.clone()]], "radius": "2 mm"}),
    );
    // 手前の面(法線 -Y)の平面の座標は、X が世界の X、Y が世界の Z
    let s = t.insert(
        kmb_sketch::SKETCH,
        rect(
            SketchPlane::Face {
                feature: round,
                face: front,
                origin: kmb_sketch::FaceOrigin::World,
            },
            [5.0, 2.0, 4.0, 3.0],
        ),
    );
    let up = t.insert(
        EXTRUDE,
        Extrude {
            profile: Some(s),
            distance: "3 mm".into(),
            ..Default::default()
        },
    );
    for h in ["10 mm", "14 mm"] {
        set_param(&mut t, "h", h);
        let (lo, hi) = t.bounds(up);
        assert!(
            near(lo, [5.0, -3.0, 2.0]) && near(hi, [9.0, 0.0, 5.0]),
            "高さ {h}: 押し出しが手前の面の同じ所に無い {lo:?} {hi:?}"
        );
    }
}

/// 両側に切り取った溝(ZX 平面から前後に 25 mm ずつ、x 10〜20、z 5 より上)の底に四角を置いて 2 mm
/// 押し出す。箱を高くしても、押し出しは溝の底の同じ所に残る。
#[test]
fn a_sketch_on_the_floor_of_a_two_sided_cut_stays_when_the_size_changes() {
    let mut b = DocBuilder::new();
    let e = tall_box(&mut b);
    // ZX 平面のスケッチの X は世界の Z、Y は世界の X
    let slot = b.feature(
        kmb_sketch::SKETCH,
        "溝の形",
        rect(SketchPlane::Zx, [5.0, 10.0, 20.0, 10.0]),
    );
    let cut = b.feature(
        EXTRUDE,
        "溝",
        Extrude {
            profile: Some(slot),
            distance: "50 mm".into(),
            direction: Direction::Symmetric,
            operation: Operation::Cut,
            target: Some(e),
            ..Default::default()
        },
    );
    let mut t = Bench::new(b.finish("箱"));
    let floor = face_toward(&t, cut, [0.0, 0.0, 1.0], |f| {
        f.planar.as_ref().is_some_and(|p| p.point[2] < 9.0)
    });
    let s = t.insert(
        kmb_sketch::SKETCH,
        rect(
            SketchPlane::Face {
                feature: cut,
                face: floor,
                origin: kmb_sketch::FaceOrigin::World,
            },
            [12.0, 5.0, 4.0, 4.0],
        ),
    );
    let up = t.insert(EXTRUDE, join(s, cut, "2 mm", Direction::Normal));
    let before = t.bounds(up);
    for h in ["10 mm", "14 mm"] {
        set_param(&mut t, "h", h);
        let (lo, hi) = t.bounds(up);
        assert!(near(lo, before.0), "高さ {h}: {lo:?} {:?}", before.0);
        let tall: f64 = h.trim_end_matches(" mm").parse().unwrap();
        assert!(
            near([hi[0], hi[1], 0.0], [before.1[0], before.1[1], 0.0])
                && (hi[2] - tall).abs() < 1e-3,
            "高さ {h}: 溝の底の押し出しがずれた {hi:?}"
        );
    }
}

/// 別のスケッチの線(X = `ax` の縦の線)を回転の軸にする。`ax` を変えると軸の線が動き、回した形も
/// その線のまわりに付いて来る。
#[test]
fn a_revolve_about_a_line_of_another_sketch_follows_the_line() {
    let mut b = DocBuilder::new();
    b.param("ax", "40 mm");
    // ZX 平面: スケッチの X は世界の Z、Y は世界の X。線は世界の X = ax の縦の線
    let mut s = SketchBuilder::new(SketchPlane::Zx);
    let o = s.fixed_point(0.0, 0.0);
    let a = s.point_from(o, "0 mm", "ax", [0.0, 40.0]);
    let c = s.point_from(a, "10 mm", "0 mm", [10.0, 40.0]);
    s.line(a, c);
    let line = b.sketch("軸の線", s);
    let profile = b.feature(
        kmb_sketch::SKETCH,
        "回す形",
        rect(SketchPlane::Zx, [0.0, 2.0, 5.0, 4.0]),
    );
    let r = b.feature(
        REVOLVE,
        "回す",
        Revolve {
            profile: Some(profile),
            axis: Axis::Points {
                sketch: Some(line),
                a: Some(kmb_core::PointId(a.0)),
                b: Some(kmb_core::PointId(c.0)),
            },
            angle: "360 deg".into(),
            ..Default::default()
        },
    );
    let mut t = Bench::new(b.finish("回す"));
    for (ax, x) in [("40 mm", 40.0), ("30 mm", 30.0)] {
        set_param(&mut t, "ax", ax);
        let (lo, hi) = t.bounds(r);
        // 四角は世界の X 2〜6。軸からの距離 x - 6 〜 x - 2 の輪になる
        assert!(
            ((lo[0] + hi[0]) / 2.0 - x).abs() < 1e-3 && ((lo[1] + hi[1]) / 2.0).abs() < 1e-3,
            "軸 {ax}: 回した形が軸の線のまわりに無い {lo:?} {hi:?}"
        );
    }
}

/// 箱(部品)の上面から、上に離して置いた板(部品に入れないボディ、z 20〜22)の下面まで押し出して箱に
/// 足す。箱の部品を X に 50 mm 動かすと、足した形は伸び縮みせず(高さ 20 のまま)箱と一緒に動く。
#[test]
fn an_extrude_up_to_another_body_moves_with_its_part_without_stretching() {
    let mut b = DocBuilder::new();
    let e = boxed(&mut b);
    let ps = b.feature(
        kmb_sketch::SKETCH,
        "板の形",
        rect(SketchPlane::Xy, [-10.0, -10.0, 50.0, 40.0]),
    );
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(ps),
            distance: "2 mm".into(),
            start: Start::Offset,
            start_offset: Some("20 mm".into()),
            ..Default::default()
        },
    );
    let mut t = Bench::new(b.finish("箱と板"));
    let bottom = face_toward(&t, plate, [0.0, 0.0, -1.0], |_| true);
    let s = t.insert(
        kmb_sketch::SKETCH,
        rect(
            SketchPlane::Face {
                feature: e,
                face: format!("f{}:end", e.0),
                origin: kmb_sketch::FaceOrigin::World,
            },
            [5.0, 5.0, 6.0, 6.0],
        ),
    );
    let up = t.insert(
        EXTRUDE,
        Extrude {
            extent: Extent::ToFace,
            to_face: Some(ToFace {
                feature: plate,
                face: bottom,
                vertex: None,
                datum: false,
            }),
            ..join(s, e, "1 mm", Direction::Normal)
        },
    );
    let (lo, hi) = t.bounds(up);
    assert!(
        near(lo, [0.0; 3]) && near(hi, [30.0, 20.0, 20.0]),
        "{lo:?} {hi:?}"
    );
    t.move_part(up, 50.0);
    let (lo, hi) = t.bounds(up);
    assert!(
        near(lo, [50.0, 0.0, 0.0]) && near(hi, [80.0, 20.0, 20.0]),
        "足した形が箱と一緒に動いていない {lo:?} {hi:?}"
    );
    let (plo, _) = t.bounds(plate);
    assert!(near(plo, [-10.0, -10.0, 20.0]), "板は動かない {plo:?}");
}
