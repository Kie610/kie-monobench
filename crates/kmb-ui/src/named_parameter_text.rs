//! 名前付きの式を受け付けられない理由を、現在の表示言語で知らせる。

use crate::window_state::Words;
use kmb_session::params::NamedParamError;

/// 数の欄とスケッチの寸法で共有する、名前付きの式の誤りの文言。
pub fn named_parameter_error(ctx: &egui::Context, error: &NamedParamError) -> String {
    text(&Words::of(ctx), error)
}

pub(crate) fn text(words: &Words, error: &NamedParamError) -> String {
    use kmb_i18n::Arg;
    match error {
        NamedParamError::Syntax => words.t("common-named-parameter-syntax"),
        NamedParamError::InvalidName(name) => words.t_with(
            "common-named-parameter-invalid-name",
            &[("name", Arg::from(name.as_str()))],
        ),
        NamedParamError::Duplicate { name, dimension } => words.t_with(
            if *dimension {
                "common-named-parameter-duplicate-dimension"
            } else {
                "common-named-parameter-duplicate"
            },
            &[("name", Arg::from(name.as_str()))],
        ),
        NamedParamError::Expression { expr, reason } => words.t_with(
            "common-named-parameter-expression",
            &[
                ("expr", Arg::from(expr.as_str())),
                ("reason", Arg::from(reason.as_str())),
            ],
        ),
        NamedParamError::UnitMismatch { length: true } => words.t("common-named-parameter-length"),
        NamedParamError::UnitMismatch { length: false } => words.t("common-named-parameter-angle"),
    }
}
