//! 編集パネルの欄の名前と説明。記述子は日本語の既定を保ち、描く時だけ訳す。

use crate::window_state::Words;
use kmb_core::{Field, FieldKind};

/// 専用の編集器で記述子の欄を描く時に、名前と説明を今の表示の言語にする。返した欄は描画用で、記述子や
/// 文書には書き戻さない。汎用の編集パネルも同じ文言の口を通る。
/// 行の中の欄は親も含む完全な鍵を渡す(`edges.radius`、`edges.0.radius` など)。
/// 訳の無い拡張の欄は、渡された日本語の既定で出す。
pub fn translated_field(ctx: &egui::Context, type_id: &str, field: &Field) -> Field {
    translate(&Words::of(ctx), type_id, field, "")
}

pub(crate) fn translate(words: &Words, type_id: &str, field: &Field, parent: &str) -> Field {
    let mut field = field.clone();
    let key = if parent.is_empty() {
        field.key.clone()
    } else {
        format!("{parent}.{}", field.key)
    };
    // 行番号は保存先の位置で、文言は同じ列の全行で共用する。
    let path = key.split('.').filter(|s| s.parse::<usize>().is_err()).collect::<Vec<_>>().join(".");
    let id = kmb_i18n::field_id(type_id, &path);
    if !field.label.is_empty() {
        field.label = words.find(&id).unwrap_or(field.label);
    }
    if !field.help.is_empty() {
        field.help = words.find(&format!("{id}-help")).unwrap_or(field.help);
    }
    if let FieldKind::Rows { fields } = &mut field.kind {
        for sub in fields {
            *sub = translate(words, type_id, sub, &key);
        }
    }
    field
}
