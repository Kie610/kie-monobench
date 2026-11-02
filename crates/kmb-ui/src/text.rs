//! 専用編集器が、窓の表示言語で案内や断りの文を作るための口。

use crate::window_state::Words;

/// 文言のIDと、文章に差し込む表示済みの値を、現在の表示言語の文にする。
/// 訳が無い時は、画面の文言と同じく英語、日本語の順で探す。
pub fn translated_text(ctx: &egui::Context, id: &str, args: &[(&str, &str)]) -> String {
    let args: Vec<_> = args
        .iter()
        .map(|(name, value)| (*name, kmb_i18n::Arg::Text((*value).to_owned())))
        .collect();
    Words::of(ctx).t_with(id, &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workspace;

    #[test]
    fn editor_text_uses_the_current_shared_language_and_arguments() {
        let ctx = egui::Context::default();
        let mut ws = Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules());
        for (lang, start) in [
            ("en", "The selected corners cannot be processed."),
            ("ko", "The selected corners cannot be processed."),
            ("ja", "選んだ角を処理できません。"),
        ] {
            ws.win.layout.appearance.language = lang.into();
            ws.win.words().share(&ctx);
            let text = translated_text(
                &ctx,
                "view-sketch-corner-cannot-fit",
                &[("maximum", "1.25 mm")],
            );
            assert!(text.starts_with(start), "{lang}: {text}");
            assert!(text.contains("1.25 mm"), "{lang}: {text}");
            assert!(!text.contains("$maximum"), "{lang}: {text}");
            let invalid = translated_text(&ctx, "view-sketch-corner-invalid", &[]);
            assert!(invalid.starts_with(start), "{lang}: {invalid}");
        }
    }
}
