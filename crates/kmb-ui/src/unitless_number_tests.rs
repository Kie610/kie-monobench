//! 確かめること: 編集パネルの長さの欄に、単位の無い数を含む式を 1 字ずつ打って Enter で決めた時の
//! 値と、断った時の印。定数だけ(`pi`)を打った Enter で編集を決められる。

use crate::*;
use egui_kittest::{Harness, kittest::Queryable};
use kmb_core::{FeatureEntry, FeatureId, LengthUnit, Op, Transaction};
use serde_json::json;

fn fixture(unit: LengthUnit) -> (Workspace, FeatureId) {
    let mut doc = kmb_samples::sample_document();
    let sketch = doc.data().features[0].id;
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "板",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: "solid.extrude@1".into(),
                name: "板".into(),
                data: json!({
                    "profile":sketch.0,"distance":"10 mm","operation":"new","direction":"normal"
                }),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let mut ws = Workspace::new(doc, None, crate::tests::reg());
    let mut p = ws.document().data().properties.clone();
    p.units = unit;
    assert!(ws.set_properties(p));
    ws.open_edit(id);
    (ws, id)
}

/// 距離の欄を全部選んで `typed` を打ち(`one_by_one` なら 1 字ずつ、でなければ貼り付けのように 1 回で)、
/// Enter を 1 回押す。
fn type_distance(
    unit: LengthUnit,
    typed: &str,
    one_by_one: bool,
) -> (Harness<'static, Workspace>, FeatureId) {
    let (ws, id) = fixture(unit);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    let _ = h.try_run();
    let key = crate::doc_id(&h.ctx, ("field", id, "distance"));
    h.ctx.memory_mut(|m| m.request_focus(key.with("edit")));
    let _ = h.try_run();
    for pressed in [true, false] {
        h.event(egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                command: true,
                ..egui::Modifiers::NONE
            },
        });
        let _ = h.try_run();
    }
    if one_by_one {
        for c in typed.chars() {
            h.event(egui::Event::Text(c.to_string()));
            let _ = h.try_run();
        }
    } else {
        h.event(egui::Event::Text(typed.into()));
        let _ = h.try_run();
    }
    h.key_press(egui::Key::Enter);
    let _ = h.try_run();
    (h, id)
}

fn distance(h: &Harness<'static, Workspace>, id: FeatureId) -> String {
    h.state().document().data().feature(id).unwrap().data["distance"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn typing_unitless_expressions_in_the_distance_field() {
    for (unit, typed, want) in [
        (LengthUnit::Mm, "10 + 5", "(10 + 5) mm"),
        (LengthUnit::In, "10", "10 in"),
        (LengthUnit::In, "10 + 5", "(10 + 5) in"),
        (LengthUnit::In, "pi mm", "pi mm"),
        (LengthUnit::In, "2 * 5 mm", "2 * 5 mm"),
        (LengthUnit::Mm, "10 + 5 mm", "10 + 5 mm"),
    ] {
        let (h, id) = type_distance(unit, typed, true);
        assert_eq!(distance(&h, id), want, "{unit:?} {typed}");
        assert!(
            !h.state().is_editing(id),
            "Enter で決める: {unit:?} {typed}"
        );
    }
    // インチの文書で単位の無い数を長さと足し引きすると、入れずに印と理由を出し、編集を続ける
    // (1 字ずつ打つと途中の「10 + 5」が形の予告に入るので、1 回で打つ)
    let (h, id) = type_distance(LengthUnit::In, "10 + 5 mm", false);
    assert_eq!(distance(&h, id), "10 mm");
    assert!(h.state().is_editing(id));
    assert!(h.query_by_label("単位が要る").is_some(), "断った印が出る");
    // 定数だけを打った Enter も、候補を選ぶのでなく編集を決める
    let (h, id) = type_distance(LengthUnit::Mm, "pi", true);
    assert_eq!(distance(&h, id), "(pi) mm");
    assert!(!h.state().is_editing(id), "pi の後の Enter で決める");
}
