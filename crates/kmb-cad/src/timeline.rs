//! タイムラインの操作(ロールバックの位置を動かす)。画面・CLI・MCP・コマンド欄が同じ宣言と確かめを通る。

use kmb_core::{Action, Arg, Context, Edit, Extension, FeatureId, Outcome};

/// ロールバックの位置を動かす操作。
pub const ROLL_BACK: &str = "cad.timeline.roll_back";
/// 前の版の読み方(読むボディを選んだ時点の形に固定する印)を 1 つの機能で外し、最新の形を読ませる操作。
pub const READ_LATEST_BODY: &str = "cad.timeline.read_latest_body";
/// 前の版の読み方の印を、文書の全部の機能でまとめて外す操作。
pub const READ_LATEST_BODIES: &str = "cad.timeline.read_latest_bodies";

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
            Action::new(READ_LATEST_BODY, "最新の形を読む", "cad")
                .arg(Arg::integer("feature", "前の版の読み方の機能の番号").required())
                .aliases("さいしん ぼでぃ よみかた latest body")
                .hint("前の版の読み方で、選んだ時点のボディを読んでいる機能に、後から足した機能の入った形を読ませる(取り消せる)"),
            Action::new(READ_LATEST_BODIES, "前の版の読み方をまとめて外す", "cad")
                .aliases("さいしん ぼでぃ よみかた まとめて latest bodies")
                .hint("前の版の読み方で、選んだ時点のボディを読んでいる機能の全部に、最新の形を読ませる(1 回で取り消せる)"),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        if id == READ_LATEST_BODY || id == READ_LATEST_BODIES {
            return Some(read_latest(id, cx));
        }
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

/// 読むボディを固定する印を外す変更(1 つの機能か、全部)。印の無い機能を指したら誤り。
fn read_latest(id: &str, cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let pinned: Vec<FeatureId> = if id == READ_LATEST_BODY {
        let n = cx
            .arg("feature")
            .and_then(serde_json::Value::as_u64)
            .ok_or("機能の番号がありません")?;
        let f = cx
            .data
            .feature(FeatureId(n))
            .ok_or_else(|| format!("機能 {n} がありません"))?;
        if !f.pin_bodies {
            return Err(format!("機能「{}」は既に最新の形を読んでいます", f.name).into());
        }
        vec![f.id]
    } else {
        cx.data
            .features
            .iter()
            .filter(|f| f.pin_bodies)
            .map(|f| f.id)
            .collect()
    };
    if pinned.is_empty() {
        return Err("前の版の読み方の機能はありません".into());
    }
    Ok(Outcome::Change {
        label: "最新の形を読む".into(),
        edits: pinned
            .into_iter()
            .map(|id| Edit::SetPinBodies { id, pin: false })
            .collect(),
    })
}
