use super::*;
use kmb_core::{Document, FeatureStatus, Kernel, Op, Registry};
use kmb_kernel_smlib::SmlibKernel;
use kmb_kernel_truck::TruckKernel;
use kmb_samples::sample::{DocBuilder, SketchBuilder, reference_part, reference_values};
use kmb_sketch::SketchPlane;

/// 試験に使うカーネル。既定は SMLib(組み立ての既定と同じ)、環境変数 `JC_KERNEL=truck` で truck。
pub(crate) enum TestKernel {
    Truck(TruckKernel),
    Smlib(SmlibKernel),
}

impl TestKernel {
    pub(crate) fn new() -> Self {
        match std::env::var("JC_KERNEL").as_deref() {
            Ok("truck") => TestKernel::Truck(TruckKernel::new()),
            _ => TestKernel::Smlib(SmlibKernel::new()),
        }
    }

    pub(crate) fn is_truck(&self) -> bool {
        matches!(self, TestKernel::Truck(_))
    }

    fn shape_count(&self) -> usize {
        match self {
            TestKernel::Truck(k) => k.shape_count(),
            TestKernel::Smlib(k) => k.shape_count(),
        }
    }
}

impl std::ops::Deref for TestKernel {
    type Target = dyn Kernel;

    fn deref(&self) -> &Self::Target {
        match self {
            TestKernel::Truck(k) => k,
            TestKernel::Smlib(k) => k,
        }
    }
}

pub(crate) fn registry() -> Registry {
    let mut r = Registry::new();
    kmb_sketch::register(&mut r);
    kmb_text::register(&mut r);
    register(&mut r);
    r
}

pub(crate) fn assert_ok(report: &kmb_core::RecomputeReport) {
    for f in &report.features {
        assert!(
            matches!(f.status, FeatureStatus::Ok { .. }),
            "{} ({}): {:?}",
            f.name,
            f.id,
            f.status
        );
    }
}

pub(crate) fn final_mass(doc: &mut Document, k: &TestKernel) -> kmb_core::MassProps {
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    assert_ok(&report);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    k.mass(bodies[0].1).unwrap()
}

pub(crate) fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

#[test]
fn reference_part_matches_theory() {
    let k = TestKernel::new();
    let mut part = reference_part();
    let m = final_mass(&mut part.doc, &k);
    let (v, a, size) = reference_values(60.0);
    assert!(close(m.volume, v, 2e-4), "体積 {} 理論値 {v}", m.volume);
    assert!(close(m.area, a, 2e-3), "表面積 {} 理論値 {a}", m.area);
    for (i, s) in size.iter().enumerate() {
        assert!(
            close(m.max[i] - m.min[i], *s, 1e-6),
            "外形 {:?} {:?}",
            m.min,
            m.max
        );
    }
}

#[test]
fn changing_a_dimension_keeps_downstream_references() {
    let k = TestKernel::new();
    let mut part = reference_part();
    final_mass(&mut part.doc, &k);
    let width = part.doc.data().param_by_name("width").unwrap().id;
    part.doc
        .apply(kmb_core::Transaction::new(
            "幅",
            vec![Op::SetParamExpr {
                id: width,
                expr: "70 mm".into(),
            }],
        ))
        .unwrap();
    let m = final_mass(&mut part.doc, &k);
    let (v, _, size) = reference_values(70.0);
    assert!(close(m.volume, v, 2e-4), "体積 {} 理論値 {v}", m.volume);
    assert!(close(m.max[0] - m.min[0], size[0], 1e-6));
    // 面の名前で参照したスケッチは、板の上面に乗ったまま
    let reg = registry();
    let report = part.doc.recompute(&reg, &*k);
    let FeatureStatus::Ok { outputs, .. } = report.feature(part.top_sketch).unwrap() else {
        panic!()
    };
    let p = kmb_sketch::profile_value(&outputs["profile"]).unwrap();
    assert!(close(p.plane.origin[2], 10.0, 1e-9));
}

#[test]
fn only_the_last_body_is_visible_and_kernel_drops_old_shapes() {
    let k = TestKernel::new();
    let mut part = reference_part();
    let reg = registry();
    let report = part.doc.recompute(&reg, &*k);
    let bodies = visible_bodies(part.doc.data(), &reg, &report);
    assert_eq!(
        bodies.iter().map(|b| b.0).collect::<Vec<_>>(),
        vec![part.hole_pattern]
    );
    let before = k.shape_count();
    // 同じ文書を解き直しても、形状は増えない
    part.doc.recompute(&reg, &*k);
    assert_eq!(k.shape_count(), before);
}

pub(crate) fn rect_sketch(
    b: &mut DocBuilder,
    plane: SketchPlane,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> kmb_core::FeatureId {
    let mut s = SketchBuilder::new(plane);
    let o = s.fixed_point(0.0, 0.0);
    let c = s.point_from(
        o,
        &format!("{} mm", x.abs()),
        &format!("{} mm", y.abs()),
        [x, y],
    );
    s.rectangle(c, &format!("{w} mm"), &format!("{h} mm"), [x, y, w, h]);
    b.feature(kmb_sketch::SKETCH, "長方形", s.build())
}

#[test]
fn revolve_and_circular_pattern() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    // ZX 平面: スケッチの x が世界の z、y が世界の x。半径 10〜15、高さ 0〜8 の輪
    let s = rect_sketch(&mut b, SketchPlane::Zx, 0.0, 10.0, 8.0, 5.0);
    b.feature(
        REVOLVE,
        "輪",
        Revolve {
            profile: s,
            regions: Vec::new(),
            axis: Axis::SketchX,
            angle: "360 deg".into(),
            operation: Operation::New,
            target: None,
            direction: Direction::Normal,
            angle2: None,
            extent: RevolveExtent::Angle,
            to_face: None,
        },
    );
    let mut doc = b.finish("回転");
    let m = final_mass(&mut doc, &k);
    let v = std::f64::consts::PI * (225.0 - 100.0) * 8.0;
    assert!(close(m.volume, v, 2e-3), "{} vs {v}", m.volume);

    // 角柱 1 本を z 軸まわりに 4 本並べる
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 10.0, -2.0, 6.0, 4.0);
    let bar = b.feature(
        EXTRUDE,
        "柱",
        Extrude {
            profile: Some(s),
            regions: Vec::new(),
            distance: "3 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    b.feature(
        CIRCULAR_PATTERN,
        "並び",
        CircularPattern {
            feature: vec![bar],
            body: None,
            axis_ref: Default::default(),
            origin: Some([0.0; 3]),
            axis: [0.0, 0.0, 1.0],
            count: "4".into(),
            angle: "360 deg".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("円形パターン");
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, 4.0 * 6.0 * 4.0 * 3.0, 1e-6), "{}", m.volume);
    assert!(close(m.min[0], -16.0, 1e-6) && close(m.max[1], 16.0, 1e-6));
}

#[test]
fn boolean_intersect_and_symmetric_extrude() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s1 = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 20.0, 20.0);
    let a = b.feature(
        EXTRUDE,
        "a",
        Extrude {
            profile: Some(s1),
            regions: Vec::new(),
            distance: "10 mm".into(),
            direction: Direction::Symmetric,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    let s2 = rect_sketch(&mut b, SketchPlane::Xy, 10.0, 10.0, 20.0, 20.0);
    let t = b.feature(
        EXTRUDE,
        "b",
        Extrude {
            profile: Some(s2),
            regions: Vec::new(),
            distance: "4 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    b.feature(
        BOOLEAN,
        "共通",
        BooleanFeature {
            target: a,
            tool: vec![t],
            operation: Operation::Intersect,
            keep_tools: false,
        },
    );
    let mut doc = b.finish("交差");
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, 10.0 * 10.0 * 4.0, 1e-6), "{}", m.volume);
    assert!(close(m.min[2], 0.0, 1e-6) && close(m.max[2], 4.0, 1e-6));
}

// 確かめること: 「ボディN」の N は文書でボディを作った順で、ほかのボディを結合で使って
// 見えなくなっても変わらない(前は見えているボディの中の順で、ボディ3 がボディ2 になっていた)
#[test]
fn body_numbers_stay_when_another_body_is_combined_away() {
    let reg = registry();
    let mut b = DocBuilder::new();
    let block = |b: &mut DocBuilder, name: &str, x: f64| {
        let s = rect_sketch(b, SketchPlane::Xy, x, 0.0, 10.0, 10.0);
        extrude_with(
            b,
            name,
            s,
            "5 mm",
            Direction::Normal,
            None,
            kmb_cad::Extent::Distance,
            Operation::New,
            None,
        )
    };
    let a = block(&mut b, "a", 0.0);
    let t = block(&mut b, "b", 5.0);
    let c = block(&mut b, "c", 40.0);
    let j = b.feature(
        BOOLEAN,
        "結合",
        BooleanFeature {
            target: a,
            tool: vec![t],
            operation: Operation::Join,
            keep_tools: false,
        },
    );
    let doc = b.finish("番号");
    assert_eq!(
        kmb_core::body_labels(doc.data(), &reg, &[j, c]),
        vec!["ボディ1", "ボディ3"]
    );
}

// 確かめること: どこにも当たらない交差は、ブーリアンの機能でも押し出しの交差でも
// 「交差すると何も残りません」で止まり、対象のボディは消えずに残る
#[test]
fn an_intersection_that_leaves_nothing_stops_and_keeps_the_body() {
    let k = TestKernel::new();
    let reg = registry();
    let ext = |profile, op, target| Extrude {
        profile: Some(profile),
        regions: Vec::new(),
        distance: "4 mm".into(),
        direction: Direction::Normal,
        distance2: None,
        extent: Default::default(),
        to_face: None,
        to_offset: None,
        operation: op,
        target,
        ..Default::default()
    };
    for by_boolean in [true, false] {
        let mut b = DocBuilder::new();
        let s1 = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 20.0, 20.0);
        let a = b.feature(EXTRUDE, "a", ext(s1, Operation::New, None));
        let s2 = rect_sketch(&mut b, SketchPlane::Xy, 100.0, 100.0, 120.0, 120.0);
        let last = if by_boolean {
            let t = b.feature(EXTRUDE, "b", ext(s2, Operation::New, None));
            b.feature(
                BOOLEAN,
                "共通",
                BooleanFeature {
                    target: a,
                    tool: vec![t],
                    operation: Operation::Intersect,
                    keep_tools: false,
                },
            )
        } else {
            b.feature(EXTRUDE, "交差", ext(s2, Operation::Intersect, Some(a)))
        };
        let mut doc = b.finish("当たらない交差");
        let report = doc.recompute(&reg, &*k);
        match report.feature(last).unwrap() {
            FeatureStatus::Error { message } => {
                assert!(message.contains("何も残りません"), "{message}")
            }
            other => panic!("{other:?}"),
        }
        let bodies = visible_bodies(doc.data(), &reg, &report);
        assert!(!bodies.is_empty(), "対象のボディが消えた");
    }
}

// 確かめること: 道具をいくつも当てる交差で、当たらない道具があれば、その道具のボディの名前と
// 入っている部品の名前を誤りの文で言う(当たった道具の名前は言わない)
#[test]
fn an_intersection_that_misses_names_the_tool_body_and_its_component() {
    let k = TestKernel::new();
    if k.is_truck() {
        // 対象と当たる道具は上下の面が同じ高さで、truck は外れた道具まで行く前のブーリアンで落ちる
        kmb_contract::skip!(
            "truck は同じ平面で接する形のブーリアンに失敗する(対象と道具の上下の面が同じ高さ)"
        );
    }
    let reg = registry();
    let ext = |b: &mut DocBuilder, name: &str, x: f64| {
        let s = rect_sketch(b, SketchPlane::Xy, x, 0.0, 20.0, 20.0);
        b.feature(
            EXTRUDE,
            name,
            Extrude {
                profile: Some(s),
                distance: "4 mm".into(),
                operation: Operation::New,
                ..Default::default()
            },
        )
    };
    let mut b = DocBuilder::new();
    let a = ext(&mut b, "a", 0.0);
    let hit = ext(&mut b, "当たる", 10.0);
    let miss = ext(&mut b, "外れる", 100.0);
    let last = b.feature(
        BOOLEAN,
        "共通",
        BooleanFeature {
            target: a,
            tool: vec![hit, miss],
            operation: Operation::Intersect,
            keep_tools: false,
        },
    );
    let mut doc = b.finish("外れた道具の名前");
    let id = doc.alloc_group_id();
    doc.apply(kmb_core::Transaction::new(
        "部品",
        vec![Op::SetComponents {
            components: vec![kmb_core::Component {
                id,
                name: "ふた".into(),
                parent: None,
                bodies: vec![miss],
            }],
        }],
    ))
    .unwrap();
    let report = doc.recompute(&reg, &*k);
    match report.feature(last).unwrap() {
        FeatureStatus::Error { message } => {
            assert!(
                message.contains("道具のボディ「ボディ3」(部品「ふた」)が対象に当たっていません"),
                "{message}"
            );
            assert!(!message.contains("ボディ2"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn errors_are_reported_not_panicked() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let p = s.fixed_point(0.0, 0.0);
    let q = s.point(10.0, 0.0);
    s.line(p, q);
    let open = b.feature(kmb_sketch::SKETCH, "開いた線", s.build());
    let e1 = b.feature(
        EXTRUDE,
        "押し出し",
        Extrude {
            profile: Some(open),
            regions: Vec::new(),
            distance: "5 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    let e2 = b.feature(
        EXTRUDE,
        "対象なし",
        Extrude {
            profile: Some(open),
            regions: Vec::new(),
            distance: "5 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::Cut,
            target: None,
            ..Default::default()
        },
    );
    // 単位の誤りは、閉じた輪郭で確かめる(開いた線だと輪郭の誤りが先に出る)
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let c = s.fixed_point(0.0, 0.0);
    s.rectangle(c, "10 mm", "10 mm", [0.0, 0.0, 10.0, 10.0]);
    let closed = b.feature(kmb_sketch::SKETCH, "四角", s.build());
    let e3 = b.feature(
        EXTRUDE,
        "単位違い",
        Extrude {
            profile: Some(closed),
            regions: Vec::new(),
            distance: "5 deg".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    let mut doc = b.finish("誤り");
    let report = doc.recompute(&registry(), &*k);
    let msg = |id| match report.feature(id).unwrap() {
        FeatureStatus::Error { message } => message.clone(),
        other => panic!("{other:?}"),
    };
    assert!(msg(e1).contains("閉じた輪郭"), "{}", msg(e1));
    assert!(msg(e2).contains("target"), "{}", msg(e2));
    assert!(msg(e3).contains("長さ"), "{}", msg(e3));
}

#[test]
fn rename_param_rewrites_solid_expressions() {
    let mut part = reference_part();
    let id = part.doc.data().param_by_name("pocket_depth").unwrap().id;
    part.doc.rename_param(&registry(), id, "depth").unwrap();
    let e = part.doc.data().feature(part.pocket).unwrap();
    assert_eq!(e.data["distance"], "depth");
}

#[test]
fn a_suppressed_cut_passes_its_target_body_on() {
    let k = TestKernel::new();
    let reg = registry();
    let mut part = reference_part();
    let before = final_mass(&mut part.doc, &k).volume;
    part.doc
        .apply(kmb_core::Transaction::new(
            "抑制",
            vec![Op::SetSuppressed {
                id: part.pocket,
                suppressed: true,
            }],
        ))
        .unwrap();
    let report = part.doc.recompute(&reg, &*k);
    for f in &report.features {
        let want_suppressed = f.id == part.pocket;
        assert_eq!(
            f.status == FeatureStatus::Suppressed,
            want_suppressed,
            "{}: {:?}",
            f.name,
            f.status
        );
        if !want_suppressed {
            assert!(
                matches!(f.status, FeatureStatus::Ok { .. }),
                "{}: {:?}",
                f.name,
                f.status
            );
        }
    }
    let bodies = visible_bodies(part.doc.data(), &reg, &report);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    assert_eq!(bodies[0].0, part.hole_pattern);
    let after = k.mass(bodies[0].1).unwrap().volume;
    // ポケット(15 × 12、深さ 4)の分だけ埋まる(truck は穴の網の分だけ 0.01 ほどずれる)
    assert!(
        (after - before - 720.0).abs() < 0.05,
        "ポケットが埋まるはず: {before} → {after}"
    );
}

/// 最後の機能(四隅の穴のパターン)を抑制しても、その前のボディが残る(渡す先が無いまま
/// 元のボディを使い切ったことにして、ボディが 1 つも無くならない)。
#[test]
fn suppressing_the_last_feature_leaves_the_body_before_it() {
    let k = TestKernel::new();
    let reg = registry();
    let mut part = reference_part();
    part.doc
        .apply(kmb_core::Transaction::new(
            "抑制",
            vec![Op::SetSuppressed {
                id: part.hole_pattern,
                suppressed: true,
            }],
        ))
        .unwrap();
    let report = part.doc.recompute(&reg, &*k);
    assert_eq!(report.error_count(), 0);
    let bodies = visible_bodies(part.doc.data(), &reg, &report);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    assert_eq!(bodies[0].0, part.holes);
    // 抑制した機能の前で止めて描く時も同じ
    let before =
        kmb_core::visible_bodies_before(part.doc.data(), &reg, &report, Some(part.hole_pattern));
    assert_eq!(before.iter().map(|b| b.0).collect::<Vec<_>>(), [part.holes]);
}

/// 基準部品の最後に機能を足して、最後のボディの体積を返す。
pub(crate) fn with_edge_feature(
    type_id: &str,
    mut data: serde_json::Value,
) -> (f64, f64, Document) {
    let k = TestKernel::new();
    let mut part = reference_part();
    data["target"] = serde_json::json!(part.hole_pattern.0);
    let before = final_mass(&mut part.doc, &k).volume;
    let id = part.doc.alloc_feature_id();
    part.doc
        .apply(kmb_core::Transaction::new(
            "辺",
            vec![Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id,
                    type_id: type_id.into(),
                    name: "辺".into(),
                    data,
                    suppressed: false,
                },
            }],
        ))
        .unwrap();
    let after = final_mass(&mut part.doc, &k).volume;
    (before, after, part.doc)
}

/// 基準部品の最後に機能を足して再計算し、その機能の状態を返す。
pub(crate) fn edge_feature_status(type_id: &str, mut data: serde_json::Value) -> FeatureStatus {
    let k = TestKernel::new();
    let mut part = reference_part();
    data["target"] = serde_json::json!(part.hole_pattern.0);
    let id = part.doc.alloc_feature_id();
    part.doc
        .apply(kmb_core::Transaction::new(
            "辺",
            vec![Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id,
                    type_id: type_id.into(),
                    name: "辺".into(),
                    data,
                    suppressed: false,
                },
            }],
        ))
        .unwrap();
    let report = part.doc.recompute(&registry(), &*k);
    report.feature(id).unwrap().clone()
}

/// 最後の辺の機能だけを入れ替えて試す台。文書とカーネルは 1 回だけ作り、前の機能の結果は
/// 文書の再計算の控えをそのまま使う([`with_edge_feature`] を何度も呼ぶより速い)。
pub(crate) struct EdgeBench {
    pub(crate) k: TestKernel,
    pub(crate) doc: Document,
    /// 辺の機能の `target` に入れる機能
    pub(crate) target: kmb_core::FeatureId,
    /// 辺の機能を足す前の体積
    pub(crate) before: f64,
    /// 今入っている辺の機能と、その種類
    edge: Option<(kmb_core::FeatureId, String)>,
}

impl EdgeBench {
    /// 基準部品の最後(穴のパターン)に足す台
    pub(crate) fn reference() -> Self {
        let part = reference_part();
        Self::new(part.doc, part.hole_pattern)
    }

    pub(crate) fn new(mut doc: Document, target: kmb_core::FeatureId) -> Self {
        let k = TestKernel::new();
        let before = final_mass(&mut doc, &k).volume;
        EdgeBench {
            k,
            doc,
            target,
            before,
            edge: None,
        }
    }

    /// 最後の辺の機能を `type_id` の `data`(`target` は自動で入れる)に替え、その機能の番号を返す。
    /// 同じ種類ならデータだけを替え、違う種類なら前のを消して足す。
    pub(crate) fn swap(
        &mut self,
        type_id: &str,
        mut data: serde_json::Value,
    ) -> kmb_core::FeatureId {
        data["target"] = serde_json::json!(self.target.0);
        let (id, ops) = match self.edge.take() {
            Some((id, t)) if t == type_id => (id, vec![Op::SetFeatureData { id, data }]),
            old => {
                let id = self.doc.alloc_feature_id();
                let insert = Op::InsertFeature {
                    index: None,
                    entry: kmb_core::FeatureEntry {
                        id,
                        type_id: type_id.into(),
                        name: "辺".into(),
                        data,
                        suppressed: false,
                    },
                };
                let ops = match old {
                    Some((old, _)) => vec![Op::RemoveFeature { id: old }, insert],
                    None => vec![insert],
                };
                (id, ops)
            }
        };
        self.edge = Some((id, type_id.into()));
        self.doc
            .apply(kmb_core::Transaction::new("辺", ops))
            .unwrap();
        id
    }

    /// 入れ替えて、最後のボディの体積を返す
    pub(crate) fn after(&mut self, type_id: &str, data: serde_json::Value) -> f64 {
        self.swap(type_id, data);
        final_mass(&mut self.doc, &self.k).volume
    }

    /// 入れ替えて再計算し、その機能の状態を返す
    pub(crate) fn status(&mut self, type_id: &str, data: serde_json::Value) -> FeatureStatus {
        let id = self.swap(type_id, data);
        let report = self.doc.recompute(&registry(), &*self.k);
        report.feature(id).unwrap().clone()
    }
}

#[test]
fn a_mirrored_feature_keeps_its_extrude_direction() {
    let k = TestKernel::new();
    // x 10〜16 の柱を x = 0 の平面で写す
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 10.0, -2.0, 6.0, 4.0);
    let bar = b.feature(
        EXTRUDE,
        "柱",
        Extrude {
            profile: Some(s),
            regions: Vec::new(),
            distance: "3 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    b.feature(
        MIRROR,
        "ミラー",
        Mirror {
            feature: vec![bar],
            body: None,
            origin: Some([0.0; 3]),
            normal: [1.0, 0.0, 0.0],
            ..Default::default()
        },
    );
    let mut doc = b.finish("ミラー");
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, 2.0 * 6.0 * 4.0 * 3.0, 1e-6), "{}", m.volume);
    assert!(close(m.min[0], -16.0, 1e-6) && close(m.max[0], 16.0, 1e-6));
    assert!(
        close(m.min[2], 0.0, 1e-6) && close(m.max[2], 3.0, 1e-6),
        "{m:?}"
    );

    // 円弧のある輪郭(長穴)を y = 0 の平面で写す
    let mut b = DocBuilder::new();
    let mut sk = kmb_sketch::Sketch::default();
    sk.add_slot((None, [0.0, 10.0]), (None, [20.0, 10.0]), 3.0)
        .unwrap();
    let s = b.feature(kmb_sketch::SKETCH, "長穴", sk);
    let slot = b.feature(
        EXTRUDE,
        "長穴",
        Extrude {
            profile: Some(s),
            regions: Vec::new(),
            distance: "2 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    b.feature(
        MIRROR,
        "ミラー",
        Mirror {
            feature: vec![slot],
            body: None,
            origin: Some([0.0; 3]),
            normal: [0.0, 1.0, 0.0],
            ..Default::default()
        },
    );
    let mut doc = b.finish("長穴のミラー");
    let m = final_mass(&mut doc, &k);
    let one = 20.0 * 6.0 * 2.0 + std::f64::consts::PI * 9.0 * 2.0;
    assert!(
        close(m.volume, 2.0 * one, 2e-3),
        "{} vs {}",
        m.volume,
        2.0 * one
    );
    assert!(
        close(m.min[1], -13.0, 1e-6) && close(m.max[1], 13.0, 1e-6),
        "{m:?}"
    );
    assert!(
        close(m.min[2], 0.0, 1e-6) && close(m.max[2], 2.0, 1e-6),
        "{m:?}"
    );
}

/// 板(x 0〜30、y 0〜20、厚さ 10)の上面に、`points` の位置で穴 `hole` をあける。
/// 穴をパターンで 10 mm 右へもう 1 つ並べるなら `pattern`。
pub(crate) fn plate_with_hole(
    points: &[[f64; 2]],
    hole: impl Fn(kmb_core::FeatureId, kmb_core::FeatureId, Vec<kmb_core::PointId>) -> Hole,
    pattern: bool,
) -> Document {
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 20.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            regions: Vec::new(),
            distance: "10 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = s.fixed_point(0.0, 0.0);
    let ids: Vec<kmb_core::PointId> = points
        .iter()
        .map(|p| {
            let c = s.point_from(o, &format!("{} mm", p[0]), &format!("{} mm", p[1]), *p);
            kmb_core::PointId(c.0)
        })
        .collect();
    let hs = b.feature(kmb_sketch::SKETCH, "穴の位置", s.build());
    let hole = b.feature(HOLE, "穴", hole(plate, hs, ids));
    if pattern {
        b.feature(
            crate::LINEAR_PATTERN,
            "穴の並び",
            LinearPattern {
                feature: vec![hole],
                body: None,
                direction_ref: Default::default(),
                direction: [1.0, 0.0, 0.0],
                spacing: "10 mm".into(),
                count: "2".into(),
                ..Default::default()
            },
        );
    }
    b.finish("板と穴")
}

/// 板と穴の取り除いた体積。
pub(crate) fn removed(doc: &mut Document, k: &TestKernel) -> f64 {
    30.0 * 20.0 * 10.0 - final_mass(doc, k).volume
}

/// [`removed`] と同じ。ただし truck で穴がカーネルの誤りになった時は、誤りとして知らせて
/// いることを確かめて `None` を返す(SMLib では必ず通す)。
pub(crate) fn removed_or_reported(doc: &mut Document, k: &TestKernel) -> Option<f64> {
    if k.is_truck() {
        let reg = registry();
        let report = doc.recompute(&reg, &**k);
        let st = &report
            .features
            .iter()
            .find(|f| f.name == "穴")
            .unwrap()
            .status;
        if let FeatureStatus::Error { message } = st {
            assert!(message.contains("形状カーネル"), "{message}");
            return None;
        }
    }
    Some(removed(doc, k))
}

/// 前方後円墳: 円柱(直径 20、高さ 10)に、長さ 25・幅 `w` の直方体を半分重ねて和算する。
/// `edges` があれば、上面の辺(面の名前の組)を半径 2 で丸める。
pub(crate) fn keyhole(w: f64, edges: &[[&str; 2]]) -> Document {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.circle(o, "20 mm", 10.0);
    let cs = b.feature(kmb_sketch::SKETCH, "円", s.build());
    let cyl = b.feature(
        EXTRUDE,
        "円柱",
        Extrude {
            profile: Some(cs),
            distance: "10 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            regions: Vec::new(),
            ..Default::default()
        },
    );
    let rs = rect_sketch(&mut b, SketchPlane::Xy, 0.0, -w / 2.0, 25.0, w);
    let bx = b.feature(
        EXTRUDE,
        "直方体",
        Extrude {
            profile: Some(rs),
            distance: "10 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::Join,
            target: Some(cyl),
            regions: Vec::new(),
            ..Default::default()
        },
    );
    if !edges.is_empty() {
        let name = |f: &str| {
            f.replace("{c}", &cyl.0.to_string())
                .replace("{b}", &bx.0.to_string())
        };
        b.feature(
            FILLET,
            "上面の丸め",
            Fillet::one(
                bx,
                EdgeSelection {
                    edges: edges.iter().map(|[a, c]| [name(a), name(c)]).collect(),
                    faces: vec![],
                    arcs: false,
                },
                "2 mm",
                false,
                Default::default(),
            ),
        );
    }
    b.finish("前方後円墳")
}

#[test]
fn a_union_of_same_height_bodies_keeps_the_first_bodys_top_face_name() {
    let k = TestKernel::new();
    if k.is_truck() {
        // truck は同じ高さの面が重なる和算に失敗する(SMLib が既定)
        kmb_contract::skip!("truck は同じ高さの面が重なる和算に失敗する");
    }
    let reg = registry();
    for _ in 0..3 {
        let mut doc = keyhole(10.0, &[]);
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let bodies = visible_bodies(doc.data(), &reg, &report);
        let names: Vec<String> = k
            .faces(bodies[0].1)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        // 円柱の上面の名前を引き継ぐ(実行ごとに円柱か直方体のどちらかに変わらない)
        assert!(names.contains(&"f2:end".to_string()), "{names:?}");
        assert!(!names.contains(&"f4:end".to_string()), "{names:?}");
    }
}

#[test]
fn revolve_auto_axis_uses_the_axis_the_profile_sits_on() {
    // 半断面を X 軸の上(と Y 軸の上)に描いた長方形。軸に触れているだけなら
    // 「またいでいます」と言わない
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 8.0, 5.0);
    b.feature(
        REVOLVE,
        "円柱",
        Revolve {
            profile: s,
            regions: Vec::new(),
            axis: Axis::Auto,
            angle: "360 deg".into(),
            operation: Operation::New,
            target: None,
            direction: Direction::Normal,
            angle2: None,
            extent: RevolveExtent::Angle,
            to_face: None,
        },
    );
    let mut doc = b.finish("自動の軸");
    let m = final_mass(&mut doc, &k);
    // X 軸まわり: 半径 5、長さ 8
    let v = std::f64::consts::PI * 25.0 * 8.0;
    assert!(close(m.volume, v, 2e-3), "{} vs {v}", m.volume);
}

#[test]
fn revolve_axis_kind_points_without_points_still_reads() {
    // 欄で軸を「スケッチの線」にしただけ(点はまだ無い)でも読める。
    // 読めないと(「missing field `a`」)ボディが消え、線を選ぶ操作も出なくなる
    let r: Revolve = serde_json::from_value(serde_json::json!({
        "profile": 1, "axis": {"kind": "points"}, "angle": "360 deg"
    }))
    .expect("点の無い軸も読める");
    assert_eq!(
        r.axis,
        Axis::Points {
            sketch: None,
            a: None,
            b: None
        }
    );
    // 点のある軸は、前と同じ形で書く
    let r = Revolve {
        axis: Axis::Points {
            sketch: None,
            a: Some(kmb_core::PointId(2)),
            b: Some(kmb_core::PointId(3)),
        },
        ..r
    };
    let v = serde_json::to_value(&r).expect("JSON にできる");
    assert_eq!(
        v["axis"],
        serde_json::json!({"kind": "points", "a": 2, "b": 3})
    );
}

#[test]
fn revolve_and_patterns_take_origin_axes_and_sketch_lines() {
    // 原点の X・Y・Z 軸と、別のスケッチの線も軸にできる。
    // (元 revolve_axis_must_lie_on_the_profile_plane)軸は輪郭と同じ平面に置く(2026-10-18 Kie)。
    // 平面に沿っていても、平面から外れた線では回さない
    let k = TestKernel::new();
    let ring = std::f64::consts::PI * (225.0 - 100.0) * 8.0;
    // ZX 平面の輪(半径 10〜15、高さ 0〜8)を回す。文書は 1 回だけ作り、軸だけを入れ替える
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Zx, 0.0, 10.0, 8.0, 5.0);
    // 別のスケッチの線: ZX 平面の (0, 0)〜(10, 0)(世界の Z 軸に沿う)
    let mut l = SketchBuilder::new(SketchPlane::Zx);
    let a = l.fixed_point(0.0, 0.0);
    let c = l.fixed_point(10.0, 0.0);
    l.line(a, c);
    let other = b.feature(kmb_sketch::SKETCH, "軸の線", l.build());
    let on_plane = Axis::Points {
        sketch: Some(other),
        a: Some(kmb_core::PointId(a.0)),
        b: Some(kmb_core::PointId(c.0)),
    };
    // XY 平面の y = 30 の線(向きは輪郭の平面に沿うが、平面の上に無い)
    let mut l = SketchBuilder::new(SketchPlane::Xy);
    let a = l.fixed_point(-5.0, 30.0);
    let c = l.fixed_point(25.0, 30.0);
    l.line(a, c);
    let off = b.feature(kmb_sketch::SKETCH, "平面の外の線", l.build());
    let off_plane = Axis::Points {
        sketch: Some(off),
        a: Some(kmb_core::PointId(a.0)),
        b: Some(kmb_core::PointId(c.0)),
    };
    let revolve = |axis: Axis| Revolve {
        profile: s,
        regions: Vec::new(),
        axis,
        angle: "360 deg".into(),
        operation: Operation::New,
        target: None,
        direction: Direction::Normal,
        angle2: None,
        extent: RevolveExtent::Angle,
        to_face: None,
    };
    let id = b.feature(REVOLVE, "輪", revolve(Axis::OriginZ));
    let mut doc = b.finish("回転");
    let set_axis = |doc: &mut Document, axis: &Axis| {
        let data = serde_json::to_value(revolve(axis.clone())).unwrap();
        doc.apply(kmb_core::Transaction::new(
            "軸",
            vec![Op::SetFeatureData { id, data }],
        ))
        .unwrap();
    };
    // 原点の Z 軸(スケッチの X 軸と同じ)と、別のスケッチの線
    for axis in [Axis::OriginZ, on_plane] {
        set_axis(&mut doc, &axis);
        let m = final_mass(&mut doc, &k);
        assert!(close(m.volume, ring, 2e-3), "{axis:?}: {}", m.volume);
    }
    // 輪郭の平面(ZX)に垂直な原点の Y 軸と、平面から外れた線では回せない
    for (axis, want) in [(Axis::OriginY, "垂直"), (off_plane, "平面の上にありません")] {
        set_axis(&mut doc, &axis);
        let report = doc.recompute(&registry(), &*k);
        let Some(FeatureStatus::Error { message }) = report.feature(id) else {
            panic!("{axis:?}: {:?}", report.feature(id));
        };
        assert!(message.contains(want), "{axis:?}: {message}");
    }
    // 保存の形: 原点の軸は {"kind": "origin_z"}、別のスケッチの線は sketch を書く
    assert_eq!(
        serde_json::to_value(Axis::OriginZ).unwrap(),
        serde_json::json!({"kind": "origin_z"})
    );

    // 角柱 1 本を原点の Z 軸まわりに 4 本、原点の Y 軸の向きに 2 本並べる
    let pattern = |circular: bool| {
        let mut b = DocBuilder::new();
        let s = rect_sketch(&mut b, SketchPlane::Xy, 10.0, -2.0, 6.0, 4.0);
        let bar = b.feature(
            EXTRUDE,
            "柱",
            Extrude {
                profile: Some(s),
                regions: Vec::new(),
                distance: "3 mm".into(),
                direction: Direction::Normal,
                distance2: None,
                extent: Default::default(),
                to_face: None,
                to_offset: None,
                operation: Operation::New,
                target: None,
                ..Default::default()
            },
        );
        if circular {
            b.feature(
                CIRCULAR_PATTERN,
                "並び",
                serde_json::json!({
                    "feature": bar, "axis_ref": {"kind": "origin_z"},
                    "count": "4", "angle": "360 deg"
                }),
            );
        } else {
            b.feature(
                LINEAR_PATTERN,
                "並び",
                serde_json::json!({
                    "feature": bar, "direction_ref": {"kind": "origin_y"},
                    "spacing": "10 mm", "count": "2"
                }),
            );
        }
        b.finish("パターン")
    };
    let m = final_mass(&mut pattern(true), &k);
    assert!(close(m.volume, 4.0 * 6.0 * 4.0 * 3.0, 1e-6), "{}", m.volume);
    // 軸の通る点は原点(省いた時の外形の中心ではない)
    assert!(close(m.min[0], -16.0, 1e-6) && close(m.max[1], 16.0, 1e-6));
    let m = final_mass(&mut pattern(false), &k);
    assert!(close(m.volume, 2.0 * 6.0 * 4.0 * 3.0, 1e-6), "{}", m.volume);
    assert!(close(m.max[1], 12.0, 1e-6), "{:?}", m.max);
}

#[test]
fn a_pattern_can_be_mirrored_and_the_default_plane_is_the_body_center() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    // 板 30×10×2 に、y にはみ出す柱(x 2〜6、z -3〜3)を足す
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 10.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            regions: Vec::new(),
            distance: "2 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    let s = rect_sketch(&mut b, SketchPlane::Xy, 2.0, -3.0, 4.0, 16.0);
    let bar = b.feature(
        EXTRUDE,
        "柱",
        Extrude {
            profile: Some(s),
            regions: Vec::new(),
            distance: "6 mm".into(),
            direction: Direction::Symmetric,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::Join,
            target: Some(plate),
            ..Default::default()
        },
    );
    // 柱を x 方向に 8 mm ずらして 2 本(x 2〜6、10〜14)
    let pat = b.feature(
        LINEAR_PATTERN,
        "並び",
        LinearPattern {
            feature: vec![bar],
            body: None,
            direction_ref: Default::default(),
            direction: [1.0, 0.0, 0.0],
            spacing: "8 mm".into(),
            count: "2".into(),
            ..Default::default()
        },
    );
    // パターンをさらにミラー。平面の点を省くと形の外形の中心(板の中央 x = 15)なので、
    // 柱は x 16〜20、24〜28 に写る
    b.feature(
        MIRROR,
        "写し",
        Mirror {
            feature: vec![pat],
            body: None,
            origin: None,
            normal: [1.0, 0.0, 0.0],
            ..Default::default()
        },
    );
    let mut doc = b.finish("パターンのミラー");
    let m = final_mass(&mut doc, &k);
    // 柱 1 本は 4×16×6 = 384 のうち板と重なる 4×10×2 = 80 を除く 304
    let v = 30.0 * 10.0 * 2.0 + 4.0 * 304.0;
    assert!(close(m.volume, v, 1e-6), "{} vs {v}", m.volume);
    assert!(close(m.min[0], 0.0, 1e-6) && close(m.max[0], 30.0, 1e-6));
}

/// 押し出しを足す(テスト用の短い書き方)。
#[allow(clippy::too_many_arguments)]
fn extrude_with(
    b: &mut DocBuilder,
    name: &str,
    profile: kmb_core::FeatureId,
    distance: &str,
    direction: Direction,
    distance2: Option<&str>,
    extent: kmb_cad::Extent,
    operation: Operation,
    target: Option<kmb_core::FeatureId>,
) -> kmb_core::FeatureId {
    b.feature(
        EXTRUDE,
        name,
        Extrude {
            profile: Some(profile),
            regions: Vec::new(),
            distance: distance.into(),
            direction,
            distance2: distance2.map(String::from),
            extent,
            to_face: None,
            to_offset: None,
            operation,
            target,
            ..Default::default()
        },
    )
}

#[test]
fn extrude_two_sides_takes_a_distance_for_each_side() {
    // 2 つの側面: 表と裏へ別の距離で押し出す
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 10.0, 10.0);
    extrude_with(
        &mut b,
        "両側",
        s,
        "4 mm",
        Direction::TwoSides,
        Some("6 mm"),
        kmb_cad::Extent::Distance,
        Operation::New,
        None,
    );
    let mut doc = b.finish("両側");
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, 10.0 * 10.0 * 10.0, 1e-6), "{}", m.volume);
    assert!(close(m.min[2], -6.0, 1e-6) && close(m.max[2], 4.0, 1e-6));
}

#[test]
fn extrude_all_cuts_through_the_whole_body() {
    // 範囲「すべて」: 距離を打たずにボディを貫く(ボディを厚くしても貫いたまま)
    let k = TestKernel::new();
    for thick in [10.0, 37.5] {
        let mut b = DocBuilder::new();
        let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 20.0, 20.0);
        let block = extrude_with(
            &mut b,
            "台",
            s,
            &format!("{thick} mm"),
            Direction::Normal,
            None,
            kmb_cad::Extent::Distance,
            Operation::New,
            None,
        );
        let hole = rect_sketch(&mut b, SketchPlane::Xy, 5.0, 5.0, 5.0, 5.0);
        extrude_with(
            &mut b,
            "貫通",
            hole,
            "1 mm",
            Direction::Normal,
            None,
            kmb_cad::Extent::All,
            Operation::Cut,
            Some(block),
        );
        let mut doc = b.finish("すべて");
        let m = final_mass(&mut doc, &k);
        let v = (20.0 * 20.0 - 5.0 * 5.0) * thick;
        assert!(
            close(m.volume, v, 1e-6),
            "厚さ {thick}: {} vs {v}",
            m.volume
        );
    }
    // 向きの先にボディがなければ、理由を出す(黙って何も切らないのではない)
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 20.0, 20.0);
    let block = extrude_with(
        &mut b,
        "台",
        s,
        "10 mm",
        Direction::Normal,
        None,
        kmb_cad::Extent::Distance,
        Operation::New,
        None,
    );
    let hole = rect_sketch(&mut b, SketchPlane::Xy, 5.0, 5.0, 5.0, 5.0);
    let cut = extrude_with(
        &mut b,
        "逆向き",
        hole,
        "1 mm",
        Direction::Reverse,
        None,
        kmb_cad::Extent::All,
        Operation::Cut,
        Some(block),
    );
    let mut doc = b.finish("先にない");
    let report = doc.recompute(&registry(), &*k);
    match report.feature(cut).unwrap() {
        FeatureStatus::Error { message } => assert!(message.contains("向きの先"), "{message}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn two_sides_take_their_own_extent() {
    // 両方向: 表は「すべて」で貫き、裏は打った距離だけ(裏の距離が黙って効かないことがない)。
    // 裏側の範囲を書いていない前の版の「両方向・すべて」は、前と同じく両側とも貫く
    use kmb_cad_extrude::BackExtent;
    let k = TestKernel::new();
    let cases = [
        (kmb_cad::Extent::All, Some(BackExtent::Distance), 14.0),
        (kmb_cad::Extent::Distance, Some(BackExtent::All), 13.0),
        (kmb_cad::Extent::All, Some(BackExtent::All), 20.0),
        (kmb_cad::Extent::All, None, 20.0),
        (kmb_cad::Extent::Distance, None, 7.0),
    ];
    for (extent, extent2, depth) in cases {
        let mut b = DocBuilder::new();
        let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 20.0, 20.0);
        let block = extrude_with(
            &mut b,
            "台",
            s,
            "10 mm",
            Direction::TwoSides,
            Some("10 mm"),
            kmb_cad::Extent::Distance,
            Operation::New,
            None,
        );
        let hole = rect_sketch(&mut b, SketchPlane::Xy, 5.0, 5.0, 5.0, 5.0);
        b.feature(
            EXTRUDE,
            "穴",
            Extrude {
                profile: Some(hole),
                distance: "3 mm".into(),
                direction: Direction::TwoSides,
                distance2: Some("4 mm".into()),
                extent,
                extent2,
                operation: Operation::Cut,
                target: Some(block),
                ..Default::default()
            },
        );
        let mut doc = b.finish("両方向の範囲");
        let m = final_mass(&mut doc, &k);
        let v = 20.0 * 20.0 * 20.0 - 5.0 * 5.0 * depth;
        assert!(
            close(m.volume, v, 1e-6),
            "{extent:?} {extent2:?}: {} vs {v}",
            m.volume
        );
    }
    // 欄に出す前の形の直し: 前の版の「すべて」は裏側も「すべて」と書く(距離はそのまま)
    let d = registry().descriptor(EXTRUDE).unwrap();
    let up = d.upgraded(
        &serde_json::json!({"distance": "1 mm", "extent": "all", "direction": "two_sides"}),
    );
    assert_eq!(up["extent2"], "all");
    let up = d.upgraded(&serde_json::json!({"distance": "1 mm", "direction": "two_sides"}));
    assert!(up.get("extent2").is_none());
}

#[test]
fn a_sketch_offset_from_its_plane_extrudes_from_there() {
    // XY から 5 mm 上に描いて 3 mm 押し出す
    let k = TestKernel::new();
    for (offset, lo) in [("5 mm", 5.0), ("-2.5 mm", -2.5)] {
        let mut b = DocBuilder::new();
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(0.0, 0.0);
        let c = s.point_from(o, "0 mm", "0 mm", [0.0, 0.0]);
        s.rectangle(c, "10 mm", "10 mm", [0.0, 0.0, 10.0, 10.0]);
        let mut sk = s.build();
        sk.offset = Some(offset.into());
        let s = b.feature(kmb_sketch::SKETCH, "浮いた長方形", sk);
        extrude_with(
            &mut b,
            "柱",
            s,
            "3 mm",
            Direction::Normal,
            None,
            kmb_cad::Extent::Distance,
            Operation::New,
            None,
        );
        let mut doc = b.finish("オフセット");
        let m = final_mass(&mut doc, &k);
        assert!(close(m.min[2], lo, 1e-6), "{offset}: {}", m.min[2]);
        assert!(close(m.max[2], lo + 3.0, 1e-6), "{offset}: {}", m.max[2]);
        assert!(close(m.volume, 300.0, 1e-6));
    }
}

#[test]
fn extrude_to_a_face_stops_at_the_face_or_offset_from_it() {
    // 高さ 25 の柱の上面まで、隣の輪郭を押し出す。文書は 1 回だけ作る
    // - 上面の高さで止まり、横の面(押し出す向きと平行)は理由つきで断る
    // - 上面から手前へ 5 mm(負)・先へ 3 mm(正)ずらして止め、面より手前へ出すずらしは断る
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 10.0, 10.0);
    let post = extrude_with(
        &mut b,
        "柱",
        s,
        "25 mm",
        Direction::Normal,
        None,
        kmb_cad::Extent::Distance,
        Operation::New,
        None,
    );
    let s2 = rect_sketch(&mut b, SketchPlane::Xy, 20.0, 0.0, 10.0, 10.0);
    let top = format!("f{}:end", post.0);
    let to_face = |face: &str, off: Option<&str>| Extrude {
        profile: Some(s2),
        regions: Vec::new(),
        distance: "1 mm".into(),
        direction: Direction::Normal,
        distance2: None,
        extent: kmb_cad::Extent::ToFace,
        to_face: Some(ToFace {
            feature: post,
            face: face.into(),
            vertex: None,
        }),
        to_offset: off.map(|o| o.into()),
        operation: Operation::Join,
        target: Some(post),
        ..Default::default()
    };
    let e = b.feature(EXTRUDE, "上面まで", to_face(&top, None));
    let mut doc = b.finish("対象まで");
    let m = final_mass(&mut doc, &k);
    // 距離の 1 mm は使わず、上面の高さ 25 まで
    assert!(close(m.volume, 2.0 * 100.0 * 25.0, 1e-3), "{}", m.volume);
    let reg = registry();
    let set = |doc: &mut Document, data: Extrude| {
        let data = serde_json::to_value(data).unwrap();
        doc.apply(kmb_core::Transaction::new(
            "対象",
            vec![Op::SetFeatureData { id: e, data }],
        ))
        .unwrap();
    };
    for (off, want) in [
        ("-5 mm", Some(100.0 * 25.0 + 100.0 * 20.0)),
        ("3 mm", Some(100.0 * 25.0 + 100.0 * 28.0)),
        ("-25 mm", None),
    ] {
        set(&mut doc, to_face(&top, Some(off)));
        match want {
            Some(v) => {
                let m = final_mass(&mut doc, &k);
                assert!(close(m.volume, v, 1e-3), "{off}: {}", m.volume);
            }
            // 面まで 25 mm なのに手前へ 25 mm は、理由つきで断る
            None => match doc.recompute(&reg, &*k).feature(e).unwrap() {
                FeatureStatus::Error { message } => {
                    assert!(message.contains("ずらし"), "{message}")
                }
                s => panic!("断るはず: {s:?}"),
            },
        }
    }
    // 横の面(押し出す向きと平行で、届かない)は、理由つきで断る
    let report = doc.recompute(&reg, &*k);
    let FeatureStatus::Ok { outputs, .. } = report.feature(post).unwrap() else {
        panic!()
    };
    let kmb_core::Value::Shape(h) = &outputs["body"] else {
        panic!()
    };
    let side = k
        .faces(*h)
        .unwrap()
        .into_iter()
        .find(|f| f.planar.is_some_and(|p| p.normal[2].abs() < 1e-6))
        .expect("横の面")
        .name;
    set(&mut doc, to_face(&side, None));
    let report = doc.recompute(&reg, &*k);
    match report.feature(e).unwrap() {
        FeatureStatus::Error { message } => assert!(message.contains("平行"), "{message}"),
        s => panic!("断るはず: {s:?}"),
    }
}

/// 平面からずらしたスケッチのずらしの式が参照するパラメータを
/// 変えると、そのスケッチから作った形も動く。JC は式の参照を入力として宣言しないと評価で誤りになるので、黙って古いまま
/// にはならない。
#[test]
fn changing_the_parameter_of_a_sketch_offset_moves_what_is_made_from_it() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    b.param("d", "30 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "20 mm", "10 mm", [0.0, 0.0, 20.0, 10.0]);
    let mut sketch = s.build();
    sketch.offset = Some("d".into());
    let sk = b.feature(kmb_sketch::SKETCH, "ずらした輪郭", sketch);
    b.feature(
        kmb_cad_extrude::EXTRUDE,
        "柱",
        Extrude {
            profile: Some(sk),
            distance: "5 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("ずらし");
    let m = final_mass(&mut doc, &k);
    assert!((m.min[2] - 30.0).abs() < 1e-6, "{:?}", m.min);
    let d = doc.data().param_by_name("d").unwrap().id;
    doc.apply(kmb_core::Transaction::new(
        "ずらしを変える",
        vec![Op::SetParamExpr {
            id: d,
            expr: "50 mm".into(),
        }],
    ))
    .unwrap();
    let m = final_mass(&mut doc, &k);
    assert!((m.min[2] - 50.0).abs() < 1e-6, "{:?}", m.min);
}
