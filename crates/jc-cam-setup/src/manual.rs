//! 手動 NC(`cam.manual_nc@1`)。加工の操作の間に、停止・任意停止・コメント・停留・知らせを入れる。
//!
//! 途中でクランプを付け替える、切りくずを払う、のために使う。タイムラインで加工の操作と同じく
//! セットアップの下に並び、G-code ではタイムラインの位置(前後の加工の間)に入る。
//! 停止の前で主軸を止めて退避し、再開の後で主軸を回し直して待つ組み立ては
//! [`jc_toolpath::Program::build_with`] が行う。この機能は命令を出力するだけで、経路は持たない。

use crate::{SetupInfo, invalid, output, parse};
use jc_core::{
    Descriptor, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Registry,
    category,
};
use jc_toolpath::Manual;
use serde::{Deserialize, Serialize};

pub const MANUAL_NC: &str = "cam.manual_nc@1";
/// 出力 `manual_nc` の型
pub const MANUAL_NC_TYPE: &str = "cam.manual_nc@1";

/// 手動 NC の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualKind {
    /// 止まって再開を待つ(プログラム停止)
    #[default]
    Stop,
    /// 任意停止(機械によっては必ず止まる)
    OptionalStop,
    /// G-code のコメントだけ
    Comment,
    /// その場で決めた秒だけ待つ
    Dwell,
    /// 操作する人への知らせ(画面のない機械ではコメント)
    Message,
}

/// 手動 NC の機能のデータ。例: `{"setup": 3, "kind": "stop", "text": "クランプを付け替える"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualNc {
    pub setup: FeatureId,
    #[serde(default)]
    pub kind: ManualKind,
    /// 停止の理由・コメント・知らせの文
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// 停留の秒
    #[serde(default = "five", skip_serializing_if = "is_five")]
    pub seconds: f64,
    /// この加工(同じセットアップ)の後に入れる。空ならタイムラインの位置。
    /// 評価には使わない(G-code を組み立てる時に位置を決める)ので、入力として宣言しない
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<FeatureId>,
}

fn five() -> f64 {
    5.0
}

fn is_five(v: &f64) -> bool {
    *v == 5.0
}

/// 出力 `manual_nc`: どのセットアップの、どの命令か。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualOutput {
    pub setup: FeatureId,
    pub manual: Manual,
}

impl jc_core::Payload for ManualOutput {
    const TYPE: &'static str = MANUAL_NC_TYPE;
}

/// 出力の値から、セットアップと手動 NC の命令を読む。
pub fn manual_of(v: &jc_core::Value) -> Option<(FeatureId, Manual)> {
    <ManualOutput as jc_core::Payload>::from_value(v).map(|m| (m.setup, m.manual))
}

/// 停留の秒の上限(打ち間違いで機械が長く止まらないように)
const MAX_DWELL: f64 = 600.0;

impl ManualNc {
    /// 命令にする。文は 1 行にまとめる(コメントは改行を持てない)。
    pub fn manual(&self) -> Result<Manual, EvalError> {
        let text: String = self.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let need_text = || {
            if text.is_empty() {
                Err(invalid("文を入れてください"))
            } else {
                Ok(text.clone())
            }
        };
        Ok(match self.kind {
            ManualKind::Stop => Manual::Stop {
                optional: false,
                text: text.clone(),
            },
            ManualKind::OptionalStop => Manual::Stop {
                optional: true,
                text: text.clone(),
            },
            ManualKind::Comment => Manual::Comment { text: need_text()? },
            ManualKind::Message => Manual::Message { text: need_text()? },
            ManualKind::Dwell => {
                if !(self.seconds > 0.0 && self.seconds <= MAX_DWELL) {
                    return Err(invalid(format!(
                        "停留の秒は 0 より大きく {MAX_DWELL} 秒以下にしてください"
                    )));
                }
                Manual::Dwell {
                    seconds: self.seconds,
                }
            }
        })
    }
}

impl Feature for ManualNc {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![output(self.setup, "setup")])
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        SetupInfo::from_value(ctx.output(self.setup, "setup")?)
            .ok_or_else(|| invalid(format!("{} はセットアップではありません", self.setup)))?;
        let mut out = Outputs::new();
        out.insert(
            "manual_nc".into(),
            jc_core::Payload::to_value(&ManualOutput {
                setup: self.setup,
                manual: self.manual()?,
            }),
        );
        Ok(out)
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        Descriptor::new(MANUAL_NC, category::OPERATION, "手動 NC")
            .parent("setup")
            .icon("manual_nc")
            .aliases("しゅどう manual nc ていし stop m0 pause ちゅうだん くらんぷ clamp こめんと comment")
            .hint("加工の間で止める(クランプの付け替え・切りくず払い)、コメント・停留・知らせを入れる。タイムラインの位置で G-code に入る")
            .field(Field::output("setup", "セットアップ", "setup"))
            .field(
                Field::choice(
                    "kind",
                    "種類",
                    &[
                        ("stop", "停止(再開を待つ)"),
                        ("optional_stop", "任意停止"),
                        ("comment", "コメント"),
                        ("dwell", "停留(秒だけ待つ)"),
                        ("message", "知らせ"),
                    ],
                )
                .help("停止の前は退避の高さへ上げて主軸を止め、再開の後は主軸を回し直して待ってから動く。停止の命令は機械で変わる(Carvera Air は M600、汎用は M0。任意停止を書けない機械では止まる停止にする)。止まっている間に原点復帰すると再開できない"),
            )
            .field(
                Field::text("text", "文")
                    .help("停止の理由・コメント・知らせの文。G-code のコメントに書く")
                    .unless("kind", "dwell"),
            )
            .field(Field::opt_output("after", "この加工の後に入れる", "toolpath").help(
                "空ならタイムラインの位置(前の加工と次の加工の間)に入れる",
            ))
            .field(
                Field::number("seconds", "停留時間(秒)").plain("秒")
                    .help("主軸を回したまま、前の加工の最後の安全高さで待つ")
                    .only("kind", "dwell", "stop"),
            ),
        |d| Ok(Box::new(parse::<ManualNc>(d)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_decides_the_command() {
        let m = |v: serde_json::Value| parse::<ManualNc>(&v).unwrap().manual();
        assert_eq!(
            m(serde_json::json!({"setup": 1})).unwrap(),
            Manual::Stop {
                optional: false,
                text: String::new()
            }
        );
        assert_eq!(
            m(serde_json::json!({"setup": 1, "kind": "optional_stop", "text": " 確かめる\n 2 回目 "}))
                .unwrap(),
            Manual::Stop {
                optional: true,
                text: "確かめる 2 回目".into()
            }
        );
        assert_eq!(
            m(serde_json::json!({"setup": 1, "kind": "dwell"})).unwrap(),
            Manual::Dwell { seconds: 5.0 }
        );
        assert!(m(serde_json::json!({"setup": 1, "kind": "dwell", "seconds": 0})).is_err());
        assert!(m(serde_json::json!({"setup": 1, "kind": "comment"})).is_err());
        assert!(parse::<ManualNc>(&serde_json::json!({"setup": 1, "kind": "m0"})).is_err());
    }
}
