//! 補助線の推論に使う PC 設定。編集器の複製だけで共有し、文書には保存しない。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(crate) const ID: &str = "sketch.inference";
const FILE: &str = "sketch-inference.json";

#[derive(Default)]
struct Preferences {
    intersections: bool,
    data_dir: Option<PathBuf>,
}

#[derive(Clone, Default)]
pub(crate) struct Store(Arc<Mutex<Preferences>>);

impl Store {
    pub(crate) fn enabled(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .intersections
    }

    pub(crate) fn set_data_dir(&self, dir: Option<&Path>) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.data_dir.as_deref() == dir {
            return;
        }
        state.data_dir = dir.map(Path::to_path_buf);
        state.intersections = false;
        if let Some(dir) = dir {
            let path = dir.join(FILE);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let loaded = serde_json::from_slice(&bytes)
                        .map_err(|e| e.to_string())
                        .and_then(|v| read(&v));
                    match loaded {
                        Ok(on) => state.intersections = on,
                        Err(error) => log::warn!("補助線の設定を読めない: {error}"),
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => log::warn!("補助線の設定を読めない: {error}"),
            }
        }
    }

    pub(crate) fn settings(&self, text: &dyn Fn(&str) -> String) -> kmb_core::Settings {
        kmb_core::Settings {
            id: ID.into(),
            label: text("settings-sketch-inference"),
            group: "cad".into(),
            notes: vec![text("settings-sketch-inference-hint")],
            fields: vec![kmb_core::Field::bool(
                "guide_intersections",
                &text("settings-sketch-guide-intersections"),
            )],
            value: value(self.enabled()),
            default: value(false),
            role: None,
        }
    }

    pub(crate) fn change(&self, next: &serde_json::Value, persist: bool) -> Result<(), String> {
        let on = read(next)?;
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if persist && let Some(dir) = &state.data_dir {
            let mut bytes = serde_json::to_vec_pretty(&value(on)).map_err(|e| e.to_string())?;
            bytes.push(b'\n');
            kmb_session::write_output(&dir.join(FILE), &bytes).map_err(|e| e.to_string())?;
        }
        state.intersections = on;
        Ok(())
    }
}

fn value(on: bool) -> serde_json::Value {
    serde_json::json!({ "guide_intersections": on })
}

fn read(value: &serde_json::Value) -> Result<bool, String> {
    let fields = value
        .as_object()
        .ok_or("補助線の設定は項目の組にしてください")?;
    if fields.keys().any(|key| key != "guide_intersections") {
        return Err("補助線の設定に知らない項目があります".into());
    }
    match fields.get("guide_intersections") {
        None => Ok(false),
        Some(serde_json::Value::Bool(on)) => Ok(*on),
        Some(_) => Err("補助線の交点の設定は真か偽にしてください".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_is_off_by_default_shared_by_tabs_and_rejects_invalid_input() {
        let store = Store::default();
        let tab = store.clone();
        assert!(!store.enabled());
        store.change(&value(true), false).unwrap();
        assert!(tab.enabled());
        assert!(
            store
                .change(&serde_json::json!({"guide_intersections": "yes"}), false)
                .is_err()
        );
        assert!(
            store
                .change(&serde_json::json!({"unknown": true}), false)
                .is_err()
        );
        assert!(store.enabled(), "拒否した値で設定を変えない");
        assert!(
            !Store::default().enabled(),
            "別の組み立てへ設定を漏らさない"
        );
        store.change(&value(false), false).unwrap();
        assert!(!tab.enabled());
    }

    #[test]
    fn persisted_preference_is_reloaded_and_session_only_changes_do_not_write() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("kmb-guide-settings-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let store = Store::default();
        store.set_data_dir(Some(&dir));
        store.change(&value(true), false).unwrap();
        assert!(!dir.join(FILE).exists(), "安全モードで保存しない");
        store.change(&value(true), true).unwrap();
        let saved = std::fs::read(dir.join(FILE)).unwrap();
        let loaded = Store::default();
        loaded.set_data_dir(Some(&dir));
        assert!(loaded.enabled());
        loaded.change(&value(false), false).unwrap();
        assert_eq!(std::fs::read(dir.join(FILE)).unwrap(), saved);
        std::fs::remove_file(dir.join(FILE)).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
