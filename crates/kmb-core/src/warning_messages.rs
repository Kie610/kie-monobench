//! 翻訳を持たない計算側から、表示側へ渡す型付きの注意。

use crate::{Outputs, Payload, Value, WARNINGS, Warnings};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 型付きの注意を運ぶ出力スロット。
pub const WARNING_MESSAGES: &str = "warning_messages";

/// 翻訳へ差し込む文字または数。単位と丸めは注意を作る側が決める。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WarningArg {
    Text(String),
    Number(f64),
}

/// 訳の鍵、型付き引数、訳がなくても読める説明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WarningMessage {
    pub id: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, WarningArg>,
    pub fallback: String,
}
impl WarningMessage {
    /// 空の説明や不正な数値を、読める注意として扱わない。
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.fallback.trim().is_empty() {
            return Err("注意の鍵と代わりの説明が必要です".into());
        }
        if self.args.iter().any(|(key, value)| {
            key.trim().is_empty() || matches!(value, WarningArg::Number(n) if !n.is_finite())
        }) {
            return Err("注意の引数の名前と数値を確かめてください".into());
        }
        Ok(())
    }
}

/// 言語に依存しない注意の一覧。文書の保存形式とは独立した派生出力。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WarningMessages(pub Vec<WarningMessage>);
impl Payload for WarningMessages {
    const TYPE: &'static str = "core.warning_messages@1";
}

fn invalid(reason: &str) -> WarningMessage {
    WarningMessage {
        id: "core-invalid-warning-message".into(),
        args: BTreeMap::new(),
        fallback: format!("注意のデータが不正です: {reason}"),
    }
}

/// 旧形式の文字列だけを読む。型付きの説明を旧形式に重ねないための口。
pub fn legacy_warnings_of(out: &Outputs) -> Vec<String> {
    out.get(WARNINGS)
        .and_then(Warnings::from_value)
        .map(|w| w.0)
        .unwrap_or_default()
}

/// 型付きの注意。不正なペイロードは黙って捨てず、読める診断を返す。
pub fn warning_messages_of(out: &Outputs) -> Vec<WarningMessage> {
    let Some(value) = out.get(WARNING_MESSAGES) else {
        return vec![];
    };
    let Some(messages) = WarningMessages::from_value(value) else {
        return vec![invalid("注意の一覧を読めません")];
    };
    messages
        .0
        .into_iter()
        .map(|m| match m.validate() {
            Ok(()) => m,
            Err(reason) => invalid(&reason),
        })
        .collect()
}

/// 注意を追加する。旧形式の注意には手を付けない。
pub fn add_warning_messages(out: &mut Outputs, added: Vec<WarningMessage>) {
    if added.is_empty() {
        return;
    }
    let mut all = warning_messages_of(out);
    all.extend(added);
    out.insert(WARNING_MESSAGES.into(), WarningMessages(all).to_value());
}

/// 訳が作れなかった時は必ず説明へ戻す。空の訳も失敗として扱う。
pub fn warnings_with(
    out: &Outputs,
    format: impl Fn(&WarningMessage) -> Option<String>,
) -> Vec<String> {
    let mut all = legacy_warnings_of(out);
    all.extend(warning_messages_of(out).iter().map(|m| {
        format(m)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| m.fallback.clone())
    }));
    all
}

/// status と書き出しの補足へ、旧形式と型付きの注意を別々に運ぶ。
pub fn warning_info(out: &Outputs) -> serde_json::Map<String, serde_json::Value> {
    let mut info = serde_json::Map::new();
    let legacy = legacy_warnings_of(out);
    if !legacy.is_empty() {
        info.insert(WARNINGS.into(), serde_json::json!(legacy));
    }
    let typed = warning_messages_of(out);
    if !typed.is_empty() {
        info.insert(WARNING_MESSAGES.into(), serde_json::json!(typed));
    }
    info
}

/// status・書き出しの補足を同じ規則で表示する。
pub fn warnings_from_info_with(
    info: &serde_json::Value,
    format: impl Fn(&WarningMessage) -> Option<String>,
) -> Vec<String> {
    let mut out = Outputs::new();
    if let Some(value) = info.get(WARNINGS) {
        out.insert(
            WARNINGS.into(),
            Value::Custom {
                type_key: Warnings::TYPE.into(),
                data: value.clone(),
            },
        );
    }
    if let Some(value) = info.get(WARNING_MESSAGES) {
        out.insert(
            WARNING_MESSAGES.into(),
            Value::Custom {
                type_key: WarningMessages::TYPE.into(),
                data: value.clone(),
            },
        );
    }
    warnings_with(&out, format)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn notice() -> WarningMessage {
        WarningMessage {
            id: "unknown-notice".into(),
            args: [("count".into(), WarningArg::Number(20.0))].into(),
            fallback: "説明".into(),
        }
    }
    #[test]
    fn typed_and_legacy_round_trip_without_duplicate_fallbacks() {
        for typed_first in [false, true] {
            let mut out = Outputs::new();
            if typed_first {
                add_warning_messages(&mut out, vec![notice()]);
            }
            crate::add_warnings(&mut out, vec!["古い注意".into()]);
            if !typed_first {
                add_warning_messages(&mut out, vec![notice()]);
            }
            let saved = serde_json::to_value(&out).unwrap();
            let restored: Outputs = serde_json::from_value(saved).unwrap();
            assert_eq!(crate::warnings_of(&restored), ["古い注意", "説明"]);
            assert_eq!(
                warnings_with(&restored, |_| Some("translated".into())),
                ["古い注意", "translated"]
            );
            assert_eq!(
                warnings_with(&restored, |_| Some("".into())),
                ["古い注意", "説明"]
            );
            assert_eq!(
                warnings_from_info_with(
                    &serde_json::Value::Object(warning_info(&restored)),
                    |_| None
                ),
                ["古い注意", "説明"]
            );
            assert_eq!(warning_messages_of(&restored), vec![notice()]);
        }
        assert!(warning_info(&Outputs::new()).is_empty());
        assert_eq!(
            serde_json::to_value(Warnings(vec!["old".into()])).unwrap(),
            serde_json::json!(["old"])
        );
    }
    #[test]
    fn malformed_notice_never_silently_becomes_an_empty_display() {
        let mut m = notice();
        m.fallback.clear();
        assert!(m.validate().is_err());
        let mut out = Outputs::new();
        add_warning_messages(&mut out, vec![m]);
        assert!(crate::warnings_of(&out)[0].contains("不正"));
        out.insert(
            WARNING_MESSAGES.into(),
            Value::Custom {
                type_key: WarningMessages::TYPE.into(),
                data: serde_json::json!([{"id":"x","args":{"x":true}}]),
            },
        );
        assert!(crate::warnings_of(&out)[0].contains("不正"));
        let mut m = notice();
        m.args.insert("n".into(), WarningArg::Number(f64::NAN));
        assert!(m.validate().is_err());
    }
}
