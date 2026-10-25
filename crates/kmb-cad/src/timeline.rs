//! タイムラインの操作(ロールバックの位置を動かす)。画面・CLI・MCP・コマンド欄が同じ宣言と確かめを通る。

use kmb_core::{Action, Arg, Context, Edit, Extension, Outcome};

/// ロールバックの位置を動かす操作。
pub const ROLL_BACK: &str = "cad.timeline.roll_back";

/// タイムラインの操作。
pub struct TimelineCommands;

/// 引数 `position` を、文書の機能の数 `len` に収まるロールバックの位置にする(`None` なら最後まで)。
/// 負の数と機能の数より後ろは、範囲を書いた誤りにする。
fn position(cx: &Context<'_>, len: usize) -> Result<Option<usize>, String> {
    let Some(v) = cx.arg("position").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let n = v
        .as_i64()
        .ok_or_else(|| format!("巻き戻しの位置は 0 から {len} の整数です: {v}"))?;
    if n < 0 || n as u64 > len as u64 {
        return Err(format!(
            "巻き戻しの位置 {n} が範囲の外です(0 から {len}。省くと最後まで進める)"
        ));
    }
    let n = n as usize;
    Ok((n < len).then_some(n))
}

impl Extension for TimelineCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        // 画面はタイムラインの印と右クリックで動かすので、ツールバーには出さない(コマンド検索には出る)
        vec![
            Action::new(ROLL_BACK, "ロールバックの位置", "cad")
                .role(kmb_interchange::role::action::ROLL_BACK)
                .arg(Arg::integer(
                    "position",
                    "先頭から何個の機能を効かせるか(0 から機能の数。省くと最後まで進める)",
                ))
                .aliases("ろーるばっく まきもどし もどす rollback timeline")
                .hint("タイムラインの印を動かし、その後ろの機能を計算から外す(取り消せる)"),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        if id != ROLL_BACK {
            return None;
        }
        let len = cx.data.features.len();
        Some(
            position(cx, len)
                .map(|rollback| {
                    let label = match rollback {
                        Some(_) => "ここまで戻す",
                        None => "最後まで進める",
                    };
                    Outcome::Change {
                        label: label.into(),
                        edits: vec![Edit::SetRollback { rollback }],
                    }
                })
                .map_err(Into::into),
        )
    }
}
