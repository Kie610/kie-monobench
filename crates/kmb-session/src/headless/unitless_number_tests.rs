//! 確かめること: 長さの欄とパラメータに打った、単位の無い数を含む式の読み方。数だけの式は全体を
//! 文書の単位で読み、mm でない文書で単位の無い数を長さと足し引きする式は断る。機能の欄(set_value)と
//! パラメータ(set_param)は同じ規則で、保存済みの文書の式は開いても値を変えない。

use super::*;
use serde_json::json;

fn doc(unit: kmb_core::LengthUnit) -> (Headless, FeatureId) {
    let mut work = Headless::new(kmb_bundle::modules());
    let sketch = work
        .add_feature(
            "cad.sketch@1",
            Some("輪郭"),
            json!({
                "entities":[
                    {"type":"point","id":1,"x":0.0,"y":0.0},
                    {"type":"point","id":2,"x":20.0,"y":0.0},
                    {"type":"point","id":3,"x":20.0,"y":10.0},
                    {"type":"point","id":4,"x":0.0,"y":10.0},
                    {"type":"line","id":5,"a":1,"b":2},
                    {"type":"line","id":6,"a":2,"b":3},
                    {"type":"line","id":7,"a":3,"b":4},
                    {"type":"line","id":8,"a":4,"b":1}
                ],"constraints":[]
            }),
        )
        .unwrap();
    let feature = work
        .add_feature(
            "solid.extrude@1",
            Some("板"),
            json!({"profile":sketch.0,"distance":"10 mm","operation":"new","direction":"normal"}),
        )
        .unwrap();
    let mut properties = work.doc().data().properties.clone();
    properties.units = unit;
    work.apply(Transaction::new(
        "単位",
        vec![kmb_core::Op::SetProperties {
            properties: Box::new(properties),
        }],
    ))
    .unwrap();
    work.add_param_info(
        "w",
        "4 mm",
        &crate::edit::ParamInfo {
            unit: Some(kmb_core::ParamUnit::Length),
            ..Default::default()
        },
    )
    .unwrap();
    (work, feature)
}

fn eval(work: &Headless, src: &str) -> Result<kmb_core::Quantity, String> {
    let values = work.report().param_values();
    kmb_core::eval_str(src, &|n| values.get(n).copied()).map_err(|e| e.to_string())
}

fn approx(q: Result<kmb_core::Quantity, String>, mm: f64) -> bool {
    q.is_ok_and(|q| (q.value - mm).abs() < 1e-9)
}

/// 入る式と値(mm)。
type Entered = (&'static str, f64);

/// 打つ式・mm の文書で入る式と値・インチの文書で入る式と値(断るなら None)。
const CASES: &[(&str, Entered, Option<Entered>)] = &[
    ("10", ("10 mm", 10.0), Some(("10 in", 254.0))),
    (
        "10 + 5",
        ("(10 + 5) mm", 15.0),
        Some(("(10 + 5) in", 381.0)),
    ),
    (
        "(2 + 3) * 2",
        ("((2 + 3) * 2) mm", 10.0),
        Some(("((2 + 3) * 2) in", 254.0)),
    ),
    ("10 + 5 mm", ("10 + 5 mm", 15.0), None),
    (
        "pi mm",
        ("pi mm", std::f64::consts::PI),
        Some(("pi mm", std::f64::consts::PI)),
    ),
    ("2 * 5 mm", ("2 * 5 mm", 10.0), Some(("2 * 5 mm", 10.0))),
    ("w + 1", ("w + 1", 5.0), None),
    ("w * 2", ("w * 2", 8.0), Some(("w * 2", 8.0))),
];

#[test]
fn unitless_numbers_in_a_length_field_are_read_the_same_from_every_entry() {
    for unit in [kmb_core::LengthUnit::Mm, kmb_core::LengthUnit::In] {
        for (typed, mm, inch) in CASES {
            let want = if unit.is_mm() { Some(*mm) } else { *inch };
            // 機能の欄(画面の編集パネル・MCP とコマンドの set_value)
            let (mut work, f) = doc(unit);
            let depth = work.doc().undo_depth();
            let r = work.set_value(f, "distance", typed);
            let stored = work.doc().data().feature(f).unwrap().data["distance"]
                .as_str()
                .unwrap()
                .to_string();
            // 長さのパラメータ(画面のパラメータの表・CLI と MCP の set_param)
            let (mut w2, _) = doc(unit);
            w2.add_param_info(
                "h",
                "1 mm",
                &crate::edit::ParamInfo {
                    unit: Some(kmb_core::ParamUnit::Length),
                    ..Default::default()
                },
            )
            .unwrap();
            let r2 = w2.set_param("h", typed);
            let p = w2.doc().data().param_by_name("h").unwrap().expr.clone();
            match want {
                Some((expr, value)) => {
                    assert_eq!(
                        (r.clone(), stored.as_str()),
                        (Ok(()), expr),
                        "{unit:?} {typed}"
                    );
                    assert!(approx(eval(&work, &stored), value), "{unit:?} {typed}");
                    assert_eq!((r2, p.as_str()), (Ok(()), expr), "{unit:?} {typed} param");
                    assert!(approx(eval(&w2, &p), value), "{unit:?} {typed} param");
                }
                None => {
                    let why = crate::params::unitless_reason(unit);
                    assert_eq!(r, Err(why.clone()), "{unit:?} {typed}");
                    assert_eq!(stored, "10 mm", "断った式は入れない");
                    assert_eq!(work.doc().undo_depth(), depth, "断った時は履歴を増やさない");
                    assert_eq!(r2, Err(why), "{unit:?} {typed} param");
                    assert_eq!(p, "1 mm");
                }
            }
        }
    }
}

#[test]
fn unitless_numbers_in_saved_expressions_keep_their_values_when_opened() {
    // インチの文書に、打ち直す前の書き方の式を入れたまま保存する(ファイルの中身と同じ道)
    let (mut work, f) = doc(kmb_core::LengthUnit::In);
    for (expr, mm) in [("10 + 5", 15.0), ("10 + 5 mm", 15.0), ("w + 1", 5.0)] {
        let mut data = work.doc().data().feature(f).unwrap().data.clone();
        data["distance"] = json!(expr);
        let tx = crate::edit::set_feature_data(work.doc().data(), f, data);
        work.apply(tx).unwrap();
        let json = serde_json::to_string(work.doc().data()).unwrap();
        let opened = Headless::with_document(
            kmb_bundle::modules(),
            kmb_core::Document::from_json(&json).unwrap(),
            None,
        );
        assert_eq!(opened.report().error_count(), 0, "{expr}");
        let v = opened.doc().data().feature(f).unwrap().data["distance"].clone();
        assert_eq!(v, json!(expr), "開いても式を書き換えない");
        assert!(approx(eval(&opened, expr), mm), "{expr}");
        // 値は形にも同じく効く(押し出しの高さ)
        let shown = crate::params::feature_values(
            opened.doc().data(),
            &opened.modules.registry,
            opened.report(),
        )
        .into_iter()
        .find(|v| v.feature == f && v.key == "distance")
        .unwrap();
        assert!(
            matches!(shown.status, kmb_core::ParamStatus::Ok { value } if (value.value - mm).abs() < 1e-9),
            "{expr}"
        );
    }
}

#[test]
fn a_named_parameter_typed_in_a_field_follows_the_same_rule() {
    let (mut work, f) = doc(kmb_core::LengthUnit::In);
    work.set_value(f, "distance", "t=10 + 5").unwrap();
    assert_eq!(
        work.doc().data().param_by_name("t").unwrap().expr,
        "(10 + 5) in"
    );
    let r = work.set_value(f, "distance", "u=10 + 5 mm");
    assert!(r.is_err_and(|e| e.contains("単位を付けて")));
    assert!(work.doc().data().param_by_name("u").is_none());
}
