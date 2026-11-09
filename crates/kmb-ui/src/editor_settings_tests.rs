//! 編集器が持つ複数の PC 設定を、文書の設定と混ぜずに扱う契約。

use super::*;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct PreferencesEditor {
    values: Arc<Mutex<[bool; 2]>>,
    exposes_settings: bool,
}

impl FeatureEditor for PreferencesEditor {
    fn handles(&self, _type_id: &str) -> bool {
        false
    }
    fn fresh(&self) -> Box<dyn FeatureEditor> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn settings(&self, _text: &dyn Fn(&str) -> String) -> Vec<kmb_core::Settings> {
        if !self.exposes_settings {
            return Vec::new();
        }
        ["test.editor.first", "test.editor.second"]
            .into_iter()
            .enumerate()
            .map(|(index, id)| kmb_core::Settings {
                id: id.into(),
                label: id.into(),
                group: "cad".into(),
                notes: vec![],
                fields: vec![kmb_core::Field::bool("enabled", "有効")],
                value: serde_json::json!({"enabled":self.values.lock().unwrap()[index]}),
                default: serde_json::json!({"enabled":false}),
                role: None,
            })
            .collect()
    }
    fn change_settings(
        &mut self,
        id: &str,
        value: &serde_json::Value,
        _persist: bool,
    ) -> Option<Result<(), String>> {
        let index = match id {
            "test.editor.first" => 0,
            "test.editor.second" => 1,
            _ => return None,
        };
        Some(
            match value.get("enabled").and_then(serde_json::Value::as_bool) {
                Some(on) => {
                    self.values.lock().unwrap()[index] = on;
                    Ok(())
                }
                None => Err("有効は真か偽にしてください".into()),
            },
        )
    }
}

#[test]
fn multiple_editor_setting_groups_apply_independently_and_empty_settings_are_unchanged() {
    let values = Arc::new(Mutex::new([false, false]));
    let mut ws = Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules());
    let existing = ws.all_settings();
    ws = ws.with_editor(PreferencesEditor {
        values: values.clone(),
        exposes_settings: false,
    });
    assert_eq!(
        ws.all_settings(),
        existing,
        "設定を出さない編集器は既存一覧を変えない"
    );
    ws.cur.editors.clear();
    ws = ws.with_editor(PreferencesEditor {
        values: values.clone(),
        exposes_settings: true,
    });
    let groups = ws.all_settings();
    assert_eq!(groups.len(), existing.len() + 2);
    assert!(ws.change_settings("test.editor.first", &serde_json::json!({"enabled":true})));
    assert_eq!(*values.lock().unwrap(), [true, false]);
    assert!(ws.change_settings("test.editor.second", &serde_json::json!({"enabled":true})));
    assert_eq!(*values.lock().unwrap(), [true, true]);
    assert!(!ws.change_settings("test.editor.second", &serde_json::json!({"enabled":"bad"})));
    assert_eq!(*values.lock().unwrap(), [true, true]);
    let reset = ws.reset_module_settings();
    assert!(
        reset.contains(&"test.editor.first".into()) && reset.contains(&"test.editor.second".into())
    );
    assert_eq!(*values.lock().unwrap(), [false, false]);
}
