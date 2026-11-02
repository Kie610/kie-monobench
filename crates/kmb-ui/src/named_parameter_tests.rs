//! 確かめること: 数の欄の名前付きの式と、編集画面を開いた直後の候補とEsc。

use crate::*;
use egui_kittest::{Harness, kittest::Queryable};
use kmb_core::{Document, FeatureEntry, FeatureId, Op, Param, Transaction};
use serde_json::json;

fn fixture(existing_name: bool) -> (Workspace, FeatureId) {
    let mut doc = kmb_samples::sample_document();
    let sketch = doc.data().features[0].id;
    if existing_name {
        for (name, expr) in [("width", "10 mm"), ("width_extra", "5 mm")] {
            let id = doc.alloc_param_id();
            doc.apply(Transaction::new("パラメータ", vec![Op::AddParam {
                index:None, param:Param::new(id,name,expr)
            }])).unwrap();
        }
    }
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new("板",vec![Op::InsertFeature {index:None,entry:FeatureEntry {
        id,type_id:"solid.extrude@1".into(),name:"板".into(),data:json!({
            "profile":sketch.0,"distance":if existing_name {"width"}else{"10 mm"},
            "operation":"new","direction":"normal"
        }),suppressed:false,pin_bodies:false
    }}])).unwrap();
    let mut ws = Workspace::new(doc,None,crate::tests::reg());
    ws.open_edit(id);
    (ws,id)
}

#[test]
fn named_parameter_opening_an_edit_does_not_open_candidates_or_steal_escape() {
    let (ws,id)=fixture(true);
    let mut h=Harness::builder().with_size(egui::vec2(1400.0,800.0))
        .build_ui_state(|ui,ws:&mut Workspace|ws.ui(ui),ws);
    h.run();
    assert!(h.state().is_editing(id));
    assert!(h.query_by_label("width_extra").is_none(),"候補を勝手に開かない");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().is_editing(id),"Esc1回で編集を閉じる");
}

#[test]
fn named_parameter_typing_in_the_distance_field_changes_both_values_with_one_undo() {
    let (ws,id)=fixture(false);
    let mut h=Harness::builder().with_size(egui::vec2(1400.0,800.0))
        .build_ui_state(|ui,ws:&mut Workspace|ws.ui(ui),ws);
    h.run();
    let before=h.state().document().undo_depth();
    let key=crate::doc_id(&h.ctx,("field",id,"distance"));
    h.ctx.memory_mut(|m|m.request_focus(key.with("edit")));
    h.run();
    for pressed in [true,false] {
        h.event(egui::Event::Key {key:egui::Key::A,physical_key:None,pressed,repeat:false,modifiers:egui::Modifiers::CTRL});
        h.run();
    }
    h.event(egui::Event::Text("w=20".into()));
    h.run();
    assert_eq!(h.ctx.data(|d|d.get_temp::<String>(key)).as_deref(),Some("w=20"),"欄に式を打つ準備ができた");
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(h.state().document().data().param_by_name("w").unwrap().expr,"20 mm");
    assert_eq!(h.state().document().data().feature(id).unwrap().data["distance"],"w");
    assert_eq!(h.state().document().undo_depth(),before+1);
    h.state_mut().undo();
    h.run();
    assert!(h.state().document().data().param_by_name("w").is_none());
    assert_eq!(h.state().document().data().feature(id).unwrap().data["distance"],"10 mm");
}
