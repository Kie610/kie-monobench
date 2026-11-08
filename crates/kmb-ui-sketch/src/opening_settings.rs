//! スケッチを開く時の PC 設定。編集器の複製だけで共有し、文書には保存しない。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(crate) const ID: &str = "sketch.opening_view";
const FILE: &str = "sketch-opening-view.json";

struct Preferences {
    front: bool,
    data_dir: Option<PathBuf>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            front: true,
            data_dir: None,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Store(Arc<Mutex<Preferences>>);

impl Store {
    pub(crate) fn front(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).front
    }

    pub(crate) fn set_data_dir(&self, dir: Option<&Path>) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.data_dir.as_deref() == dir {
            return;
        }
        state.data_dir = dir.map(Path::to_path_buf);
        state.front = true;
        if let Some(dir) = dir {
            let path = dir.join(FILE);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let loaded = serde_json::from_slice(&bytes)
                        .map_err(|e| e.to_string())
                        .and_then(|v| read(&v));
                    match loaded {
                        Ok(on) => state.front = on,
                        Err(error) => log::warn!("開く時の設定を読めない: {error}"),
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => log::warn!("開く時の設定を読めない: {error}"),
            }
        }
    }

    pub(crate) fn settings(&self, text: &dyn Fn(&str) -> String) -> kmb_core::Settings {
        kmb_core::Settings {
            id: ID.into(),
            label: text("settings-sketch-opening-view"),
            group: "cad".into(),
            notes: vec![text("settings-sketch-opening-view-hint")],
            fields: vec![kmb_core::Field::bool(
                "look_front_on_open",
                &text("settings-sketch-opening-front"),
            )],
            value: value(self.front()),
            default: value(true),
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
        state.front = on;
        Ok(())
    }
}

fn value(on: bool) -> serde_json::Value {
    serde_json::json!({ "look_front_on_open": on })
}

fn read(value: &serde_json::Value) -> Result<bool, String> {
    let fields = value
        .as_object()
        .ok_or("開く時の設定は項目の組にしてください")?;
    if fields.keys().any(|key| key != "look_front_on_open") {
        return Err("開く時の設定に知らない項目があります".into());
    }
    match fields.get("look_front_on_open") {
        None => Ok(true),
        Some(serde_json::Value::Bool(on)) => Ok(*on),
        Some(_) => Err("開く時の向きの設定は真か偽にしてください".into()),
    }
}
