//! まとめて実行(`batch`)。道具の呼び出しを順に動かし、途中で失敗したら始めの形まで全部戻す。
//! 通った時は、文書を変えた手を 1 回の取り消し単位にまとめる(`undo` 1 回で batch の前に戻る)。
//!
//! 先に全部の手の引数を読み、知らない道具・入れられない道具・引数の誤りがあれば、何もせずに断る。
//! 文書を入れ替える・保存する・取り消す・設定を変える道具は入れられない(戻せないか、戻し方と
//! ぶつかるため)。書き出したファイルは戻さない。
//! `screenshot` の絵は結果に入れない(説明だけ。絵は `out` で書き出す)。

use crate::server::{batch_call, batch_refusal};
use crate::session::{Reply, Session};
use kmb_core::Problem;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value as Json, json};

/// 1 回に入れられる手の数。
pub const MAX_STEPS: usize = 200;

/// batch の 1 手。
#[derive(Deserialize, JsonSchema)]
pub struct Step {
    /// 道具の名前(例: "set_param")
    pub tool: String,
    /// その道具の引数(道具を 1 つずつ呼ぶ時と同じ形。引数の無い道具は省く)
    #[serde(default)]
    pub args: Json,
}

/// 読み終えた 1 手。中身は道具の表(server.rs の `tools!`)が、道具を 1 つずつ呼ぶ時と同じに作る。
pub struct Call {
    only_sets_values: bool,
    run: Box<dyn FnOnce(&mut Session) -> Reply>,
}

pub(crate) fn args<T: DeserializeOwned>(tool: &str, a: Json) -> Result<T, String> {
    let a = if a.is_null() { json!({}) } else { a };
    serde_json::from_value(a).map_err(|e| format!("{tool} の引数を読めません: {e}"))
}

/// 道具の名前と引数を読む。知らない道具、入れられない道具、引数の誤りは理由を返す。
pub fn parse(tool: &str, a: Json) -> Result<Call, String> {
    if let Some(why) = batch_refusal(tool) {
        return Err(format!("{tool}: {why}"));
    }
    if let Some(call) = batch_call(tool, a) {
        return call;
    }
    // 打ち間違いに近い、batch に入れられる道具の名前を添える(コマンド欄と同じ案内)
    let names: Vec<String> = crate::Server::tools()
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| batch_refusal(n).is_none())
        .collect();
    let near = kmb_session::near::near_names(tool, names.iter().map(String::as_str));
    Err(format!(
        "知らない道具です: {tool}{}",
        kmb_session::near::hint(&near)
    ))
}

impl Call {
    pub(crate) fn new(only_sets_values: bool, run: Box<dyn FnOnce(&mut Session) -> Reply>) -> Self {
        Self {
            only_sets_values,
            run,
        }
    }

    /// パラメータと機能の値の式を直すだけの手か。続けても前の手の作り直しの結果を読まない
    /// (名前の一覧は変わらない)ので、作り直しを後へ回せる。
    fn only_sets_values(&self) -> bool {
        self.only_sets_values
    }

    /// 道具を 1 つずつ呼んだ時と同じ中身を動かす。
    pub fn run(self, s: &mut Session) -> Reply {
        (self.run)(s)
    }
}

/// 1 手の結果を短くする。文書の要約(status)は最後に 1 回だけ返すので、手ごとには
/// 要約の外の項目(足した機能の `added`、グループの `group` など)だけを残す。
fn brief(tool: &str, r: Json) -> Json {
    let is_status = r.get("features").is_some() && r.get("params").is_some();
    if !is_status {
        return json!({"tool": tool, "result": r});
    }
    let mut out = json!({"tool": tool});
    for k in ["added", "group"] {
        if let Some(v) = r.get(k) {
            out[k] = v.clone();
        }
    }
    out
}

impl Session {
    /// 手を順に動かす。途中で失敗したら始めの形まで戻し、何手目の、どの道具が、なぜ失敗したかを
    /// 返す(JSON の文字列)。通ったら、文書を変えた手を 1 回の取り消し単位にまとめる。
    pub fn batch(&mut self, steps: Vec<Step>) -> Reply {
        if steps.is_empty() {
            return Err("手がありません(steps に 1 つ以上)".into());
        }
        if steps.len() > MAX_STEPS {
            return Err(format!("手が多すぎます({} 手。{MAX_STEPS} 手まで)", steps.len()).into());
        }
        // 先に全部読み、読めない手があれば何もしない
        let mut calls = Vec::with_capacity(steps.len());
        for (i, st) in steps.into_iter().enumerate() {
            match parse(&st.tool, st.args) {
                Ok(c) => calls.push((st.tool, c)),
                Err(e) => {
                    return Err(failure(
                        i,
                        &st.tool,
                        &e.into(),
                        0,
                        "読めない手があるので、何もしていません",
                    ));
                }
            }
        }
        let n = calls.len();
        let mark = self.mark();
        let start = self.recomputes();
        log::info!("まとめて実行: {n} 手");
        let mut results = Vec::with_capacity(n);
        for (i, (tool, call)) in calls.into_iter().enumerate() {
            // 値を直す手が続く間は作り直しを後へ回し、ほかの手の前で作り直す
            self.hold_recompute(call.only_sets_values());
            match call.run(self) {
                Ok(r) => results.push(brief(&tool, r)),
                Err(e) => {
                    // 戻してから 1 回だけ作り直す
                    let back = self.rollback_to(mark);
                    self.hold_recompute(false);
                    log::warn!("まとめて実行の {} 手目({tool})で失敗: {e}", i + 1);
                    return Err(failure(
                        i,
                        &tool,
                        &e,
                        back,
                        "失敗したので、batch の前の形に戻しました",
                    ));
                }
            }
        }
        self.hold_recompute(false);
        log::info!("まとめて実行の作り直し: {} 回", self.recomputes() - start);
        let changed = self.squash_since(mark, &format!("まとめて実行({n} 手)"));
        Ok(json!({"ok": true, "steps": results, "changes": changed, "status": self.status()}))
    }
}

/// 失敗の知らせ(文は JSON)。`index` は 0 から、`step` は 1 から数える。種類は失敗した手の種類。
fn failure(index: usize, tool: &str, error: &Problem, rolled_back: usize, note: &str) -> Problem {
    let v = json!({
        "ok": false,
        "failed": {"step": index + 1, "index": index, "tool": tool, "error": error.message, "kind": error.kind},
        "rolled_back": rolled_back,
        "message": format!("{} 手目の {tool} で失敗しました: {error}。{note}", index + 1),
    });
    Problem::new(
        error.kind,
        serde_json::to_string_pretty(&v).expect("JSON にできる"),
    )
}
