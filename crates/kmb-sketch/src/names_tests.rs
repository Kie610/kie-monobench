//! 寸法の名前と、寸法の式(Kie のレビュー 2026-10-16 の K10)。

use crate::tests::{close, doc_with_rectangle, e, rectangle, registry, solved};
use crate::*;
use kmb_core::{
    Document, FeatureEntry, FeatureId, FeatureStatus, NullKernel, Op, Param, ParamStatus, Quantity,
    Transaction,
};

fn sketch_of(doc: &Document, f: FeatureId) -> Sketch {
    serde_json::from_value(doc.data().feature(f).unwrap().data.clone()).unwrap()
}

/// 寸法 `id` の式を `v` にする(1 回の取り消し単位)。
fn set_dim(doc: &mut Document, f: FeatureId, id: u32, v: &str) {
    let mut s = sketch_of(doc, f);
    *s.constraints
        .iter_mut()
        .find(|c| c.id == ConstraintId(id))
        .unwrap()
        .kind
        .value_mut()
        .unwrap() = v.into();
    let data = serde_json::to_value(&s).unwrap();
    doc.apply(Transaction::new(
        "寸法",
        vec![Op::SetFeatureData { id: f, data }],
    ))
    .unwrap();
}

fn add_param(doc: &mut Document, name: &str, expr: &str) {
    let id = doc.alloc_param_id();
    doc.apply(Transaction::new(
        "パラメータ",
        vec![Op::AddParam {
            index: None,
            param: Param::new(id, name, expr),
        }],
    ))
    .unwrap();
}

fn value_of(r: &kmb_core::RecomputeReport, name: &str) -> Quantity {
    match r.param(name) {
        Some(ParamStatus::Ok { value }) => *value,
        other => panic!("{name} が計算できません: {other:?}"),
    }
}

#[test]
fn driving_dimensions_get_names_d1_d2() {
    let (mut doc, f) = doc_with_rectangle();
    let r = doc.recompute(&registry(), &NullKernel);
    let s = sketch_of(&doc, f);
    assert_eq!(s.dim_name(ConstraintId(6)), Some("d1"));
    assert_eq!(s.dim_name(ConstraintId(7)), Some("d2"));
    // 拘束(水平など)は名前を持たない
    assert_eq!(s.dim_name(ConstraintId(2)), None);
    let names: Vec<&str> = r.model_params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["d1", "d2"]);
    assert_eq!(value_of(&r, "d2"), Quantity::mm(20.0));
    // 名前を付けたのは直前の変更と同じ取り消し単位(取り消すと一緒に戻る)
    assert_eq!(doc.undo_labels().len(), 1);
}

#[test]
fn a_dimension_can_refer_to_another_by_name() {
    let (mut doc, f) = doc_with_rectangle();
    doc.recompute(&registry(), &NullKernel);
    set_dim(&mut doc, f, 7, "d1 / 4");
    let sol = solved(&mut doc, f);
    assert!(close(sol.points[&e(3)], [40.0, 10.0]));
    // 寸法の名前は同じスケッチの中で解くので、入力には出ない
    let inputs = kmb_core::Feature::inputs(&sketch_of(&doc, f)).unwrap();
    assert!(
        !inputs
            .iter()
            .any(|i| matches!(i, kmb_core::InputRef::Param { name } if name == "d1"))
    );
}

#[test]
fn parameters_and_other_features_can_use_dimension_names() {
    let (mut doc, f) = doc_with_rectangle();
    doc.recompute(&registry(), &NullKernel);
    add_param(&mut doc, "twice", "d1 * 2");
    let r = doc.recompute(&registry(), &NullKernel);
    assert_eq!(value_of(&r, "twice"), Quantity::mm(80.0));
    // 別のスケッチの寸法から参照する(名前は文書の中で一意)
    let g = doc.alloc_feature_id();
    let data = serde_json::to_value(rectangle("d1 + 5 mm", "d2")).unwrap();
    doc.apply(Transaction::new(
        "スケッチ2",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id: g,
                type_id: SKETCH.into(),
                name: "スケッチ2".into(),
                data,
                suppressed: false,
            },
        }],
    ))
    .unwrap();
    let sol = solved(&mut doc, g);
    assert!(close(sol.points[&e(3)], [45.0, 20.0]));
    let s2 = sketch_of(&doc, g);
    assert_eq!(s2.dim_name(ConstraintId(6)), Some("d3"));
    assert_eq!(s2.dim_name(ConstraintId(7)), Some("d4"));
    // 元の寸法を変えると、参照した側も付いて来る
    set_dim(&mut doc, f, 6, "50 mm");
    let sol = solved(&mut doc, g);
    // 高さは d2(= width / 2)のまま
    assert!(close(sol.points[&e(3)], [55.0, 20.0]));
}

#[test]
fn renaming_a_dimension_rewrites_every_reference_in_one_undo() {
    let (mut doc, f) = doc_with_rectangle();
    doc.recompute(&registry(), &NullKernel);
    set_dim(&mut doc, f, 7, "d1 / 4");
    add_param(&mut doc, "twice", "d1 * 2");
    doc.recompute(&registry(), &NullKernel);
    doc.rename_model_param(&registry(), "d1", "w").unwrap();
    let s = sketch_of(&doc, f);
    assert_eq!(s.dim_name(ConstraintId(6)), Some("w"));
    assert_eq!(s.constraints[6].kind.value(), Some("w / 4"));
    assert_eq!(doc.data().param_by_name("twice").unwrap().expr, "w * 2");
    let r = doc.recompute(&registry(), &NullKernel);
    assert_eq!(value_of(&r, "twice"), Quantity::mm(80.0));
    // 使っている名前・使えない名前にはできない
    assert!(doc.rename_model_param(&registry(), "w", "d2").is_err());
    assert!(doc.rename_model_param(&registry(), "w", "twice").is_err());
    assert!(doc.rename_model_param(&registry(), "w", "mm").is_err());
    assert!(doc.undo());
    let s = sketch_of(&doc, f);
    assert_eq!(s.dim_name(ConstraintId(6)), Some("d1"));
    assert_eq!(s.constraints[6].kind.value(), Some("d1 / 4"));
    assert_eq!(doc.data().param_by_name("twice").unwrap().expr, "d1 * 2");
    // パラメータの名前を寸法の名前にもできない
    let id = doc.data().param_by_name("twice").unwrap().id;
    assert!(doc.rename_param(&registry(), id, "d2").is_err());
}

#[test]
fn deleting_a_referenced_dimension_inlines_its_expression() {
    let (mut doc, f) = doc_with_rectangle();
    doc.recompute(&registry(), &NullKernel);
    set_dim(&mut doc, f, 7, "d1 / 4");
    add_param(&mut doc, "twice", "d1 * 2");
    doc.recompute(&registry(), &NullKernel);
    // 幅の寸法 d1 を消す(式は width)
    let mut s = sketch_of(&doc, f);
    s.constraints.retain(|c| c.id != ConstraintId(6));
    let data = serde_json::to_value(&s).unwrap();
    doc.apply(Transaction::new(
        "消す",
        vec![Op::SetFeatureData { id: f, data }],
    ))
    .unwrap();
    let r = doc.recompute(&registry(), &NullKernel);
    let s = sketch_of(&doc, f);
    assert_eq!(s.constraints[5].kind.value(), Some("(width) / 4"));
    assert_eq!(s.dim_name(ConstraintId(6)), None);
    assert_eq!(
        doc.data().param_by_name("twice").unwrap().expr,
        "(width) * 2"
    );
    assert_eq!(value_of(&r, "twice"), Quantity::mm(80.0));
    assert!(matches!(r.feature(f), Some(FeatureStatus::Ok { .. })));
    // 消すのと置き換えは 1 回の取り消しで戻る
    assert!(doc.undo());
    let s = sketch_of(&doc, f);
    assert_eq!(s.dim_name(ConstraintId(6)), Some("d1"));
    assert_eq!(s.constraints[6].kind.value(), Some("d1 / 4"));
    assert_eq!(doc.data().param_by_name("twice").unwrap().expr, "d1 * 2");
    // やり直すと置き換えも戻る
    assert!(doc.redo());
    let r = doc.recompute(&registry(), &NullKernel);
    assert_eq!(
        doc.data().param_by_name("twice").unwrap().expr,
        "(width) * 2"
    );
    assert_eq!(value_of(&r, "twice"), Quantity::mm(80.0));
}

#[test]
fn a_reference_cycle_is_an_error() {
    let (mut doc, f) = doc_with_rectangle();
    doc.recompute(&registry(), &NullKernel);
    set_dim(&mut doc, f, 6, "d2 * 2");
    set_dim(&mut doc, f, 7, "d1 / 2");
    let s = sketch_of(&doc, f);
    let e = dimension_values(&s, &|_| None).unwrap_err();
    assert!(e.contains("循環"), "{e}");
    let r = doc.recompute(&registry(), &NullKernel);
    assert!(matches!(r.feature(f), Some(FeatureStatus::Error { .. })));
    // 別のパラメータを通した輪も誤り
    let (mut doc, f) = doc_with_rectangle();
    doc.recompute(&registry(), &NullKernel);
    add_param(&mut doc, "p", "d1");
    set_dim(&mut doc, f, 6, "p");
    let r = doc.recompute(&registry(), &NullKernel);
    assert!(matches!(r.param("p"), Some(ParamStatus::Error { .. })));
    assert!(!matches!(r.feature(f), Some(FeatureStatus::Ok { .. })));
}

#[test]
fn a_copied_sketch_gets_its_own_names() {
    let (mut doc, f) = doc_with_rectangle();
    doc.recompute(&registry(), &NullKernel);
    set_dim(&mut doc, f, 7, "d1 / 4");
    doc.recompute(&registry(), &NullKernel);
    // 名前ごと写したスケッチ(機能の複製・貼り付け)
    let g = doc.alloc_feature_id();
    let data = doc.data().feature(f).unwrap().data.clone();
    doc.apply(Transaction::new(
        "複製",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id: g,
                type_id: SKETCH.into(),
                name: "スケッチ2".into(),
                data,
                suppressed: false,
            },
        }],
    ))
    .unwrap();
    let r = doc.recompute(&registry(), &NullKernel);
    let s2 = sketch_of(&doc, g);
    assert_eq!(s2.dim_name(ConstraintId(6)), Some("d3"));
    assert_eq!(s2.dim_name(ConstraintId(7)), Some("d4"));
    // 写した側の中の参照は、写した側の寸法へ
    assert_eq!(s2.constraints[6].kind.value(), Some("d3 / 4"));
    assert_eq!(r.error_count(), 0);
    // 元は変わらない
    assert_eq!(sketch_of(&doc, f).dim_name(ConstraintId(6)), Some("d1"));
}

#[test]
fn unitless_values_take_the_dimension_unit_and_pi_works_for_angles() {
    let mut s = rectangle("50 / 2", "d1 - 5");
    let b = rectangle("2.5 [cm]", "d1 - 5 [mm]");
    let mut bb = kmb_core::Names::default();
    let vb = dimension_values(&b.with_dim_names(&mut bb).unwrap(), &|_| None).unwrap();
    assert!((vb[&ConstraintId(6)] - 25.0).abs() < 1e-9);
    assert!((vb[&ConstraintId(7)] - 20.0).abs() < 1e-9);
    let mut book = kmb_core::Names::default();
    s = s.with_dim_names(&mut book).unwrap();
    let v = dimension_values(&s, &|_| None).unwrap();
    assert!((v[&ConstraintId(6)] - 25.0).abs() < 1e-9);
    assert!((v[&ConstraintId(7)] - 20.0).abs() < 1e-9);
    // 角度の寸法: 数だけは deg、PI/4 rad はラジアン
    use ConstraintKind as K;
    for (src, want) in [
        ("45", 45.0),
        ("PI/4 rad", 45.0),
        ("(PI/4) [rad]", 45.0),
        ("30 [deg] + 15", 45.0),
        ("90 / 2", 45.0),
        ("atan(1)", 45.0),
    ] {
        let mut a = s.clone();
        a.constraints.push(Constraint::new(
            ConstraintId(20),
            K::Angle {
                a: e(11),
                b: e(12),
                value: src.into(),
            },
        ));
        let v = dimension_values(&a, &|_| None).unwrap();
        assert!(
            (v[&ConstraintId(20)].to_degrees() - want).abs() < 1e-9,
            "{src}"
        );
    }
}

#[test]
fn reference_dimensions_are_not_named_and_names_of_removed_ones_are_dropped() {
    let mut s = rectangle("40 mm", "20 mm");
    s.constraints[6].reference = true;
    let mut book = kmb_core::Names::default();
    let s = s.with_dim_names(&mut book).unwrap();
    assert_eq!(s.dim_name(ConstraintId(6)), Some("d1"));
    assert_eq!(s.dim_name(ConstraintId(7)), None);
    let mut t = s.clone();
    t.constraints.retain(|c| c.id != ConstraintId(6));
    let t = t.with_dim_names(&mut kmb_core::Names::default()).unwrap();
    assert!(t.dim_names.is_empty());
    // 名前を持つ寸法を保存して読み直しても同じ
    let back: Sketch = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert_eq!(back, s);
    assert_eq!(s.model_params()[0].label, "長さ C6");
}
