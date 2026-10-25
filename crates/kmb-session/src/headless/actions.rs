//! モジュールの操作を探して実行する(操作の文脈を作り、結果のファイルを書く)。

use super::{Handled, Headless, Picks, Ran};
use kmb_core::{Action, Context, FeatureId};
use serde_json::{Value as Json, json};
use std::path::{Path, PathBuf};

impl Headless {
    pub(super) fn context<'a>(
        &'a self,
        selected: Option<FeatureId>,
        file: Option<&'a Path>,
        args: &'a Json,
    ) -> Context<'a> {
        Context {
            selected,
            file,
            args,
            doc_path: self.path.as_deref(),
            ..Context::new(
                self.doc.data(),
                &self.report,
                &self.modules.registry,
                &self.modules.kernel,
            )
        }
    }

    /// 選んでいる物 `picks` での文脈(画面と同じ物を操作に渡す)。
    pub fn context_with<'a>(
        &'a self,
        picks: &Picks<'a>,
        file: Option<&'a Path>,
        args: &'a Json,
    ) -> Context<'a> {
        Context {
            selected: picks.selected,
            picked: picks.picked,
            also_picked: picks.also_picked,
            edges: picks.edges,
            vertices: picks.vertices,
            also_selected: picks.also_selected,
            bodies_picked: picks.bodies_picked,
            doc: picks.doc,
            files: picks.files,
            ..self.context(picks.selected, file, args)
        }
    }

    /// 今の文書で使える操作(`selected` を選んでいるとした時)。
    pub fn actions(&self, selected: Option<FeatureId>) -> Vec<Action> {
        self.modules
            .extensions
            .actions(&self.context(selected, None, &Json::Null))
    }

    /// 操作 `id` が引数 `key` を受け取るか([`Action::args`] の宣言。宣言に無い鍵を渡すと誤り)。
    pub fn declares(&self, id: &str, key: &str) -> bool {
        self.actions(None)
            .iter()
            .any(|a| a.id == id && a.args.iter().any(|d| d.key == key))
    }

    /// 初めのセットアップ(CLI・MCP でセットアップを省いた時に使う)。画面と同じく、セットアップの操作は
    /// セットアップを選ばないと使えないので、省いた時はここで選んでから [`Headless::run`] に渡す。
    pub fn first_setup(&self) -> Option<FeatureId> {
        self.status()["setups"][0]["id"].as_u64().map(FeatureId)
    }

    /// 役割の印 `role` を持つ操作の名前(CLI・MCP は機能の名前を書かず、役割で探す)。
    pub fn action_for_role(&self, role: &kmb_core::ActionRole) -> Result<String, String> {
        self.actions(None)
            .into_iter()
            .find(|a| a.role.as_ref() == Some(role))
            .map(|a| a.id)
            .ok_or_else(|| format!("役割 {} の操作を持つモジュールがありません", role.0))
    }

    /// 拡張子(`out` の)で書き出す操作を選ぶ。ボディを書き出す役割([`kmb_interchange::role::action::EXPORT_BODIES`])の
    /// 操作のうち、その拡張子を書けて今使える物の最初(同じモジュールの中では並べた順。STL なら
    /// ボディごとでない方、DXF なら隠れ線なしの方)。`action` を渡すと、その操作を使う(役割を
    /// 持たない書き出し、例えばスケッチの DXF も選べる)。
    /// 役割を見ずに拡張子だけで選ぶと、組み立ての並びで先に来るスケッチの DXF・SVG や部品表の
    /// CSV を選んでしまう。
    pub fn save_action_for(&self, out: &Path, action: Option<&str>) -> Result<String, String> {
        let ext = out
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        let actions = self.actions(None);
        let writes = |a: &Action| match &a.input {
            kmb_core::Input::SaveFile { extensions, .. } => {
                extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext))
            }
            _ => false,
        };
        if let Some(id) = action {
            let a = actions.iter().find(|a| a.id == id).ok_or_else(|| {
                let near = crate::near::near_names(id, actions.iter().map(|a| a.id.as_str()));
                format!(
                    "操作 {id} はありません(actions で一覧){}",
                    crate::near::hint(&near)
                )
            })?;
            if !writes(a) {
                return Err(format!("操作 {id} は .{ext} を書き出しません"));
            }
            if let Some(why) = &a.disabled {
                return Err(why.clone());
            }
            return Ok(a.id.clone());
        }
        let bodies: Vec<&Action> = actions
            .iter()
            .filter(|a| a.role == Some(kmb_interchange::role::action::EXPORT_BODIES))
            .collect();
        let candidates: Vec<&&Action> = bodies.iter().filter(|a| writes(a)).collect();
        if let Some(a) = candidates.iter().find(|a| a.disabled.is_none()) {
            return Ok(a.id.clone());
        }
        if let Some(why) = candidates.first().and_then(|a| a.disabled.clone()) {
            return Err(why);
        }
        let mut known: Vec<String> = Vec::new();
        for a in &bodies {
            if let kmb_core::Input::SaveFile { extensions, .. } = &a.input {
                for e in extensions {
                    let e = format!(".{e}");
                    if !known.contains(&e) {
                        known.push(e);
                    }
                }
            }
        }
        Err(format!(
            "ボディの書き出しの拡張子は {} のどれかです(ほかの書き出しは操作の ID を指定する)",
            known.join("、")
        ))
    }

    /// 書き出しの操作 `action` で `out` へ書く時の引数の始め。形式を拡張子で選ぶ書き出し(動画の
    /// .mp4・.gif・.png)には拡張子を `format` で渡す。ほかの指定は呼ぶ側が足す。
    pub fn export_args(&self, action: &str, out: &Path) -> Json {
        let mut args = json!({});
        if let Some(format) = out.extension().and_then(|e| e.to_str())
            && self.declares(action, "format")
        {
            args["format"] = json!(format);
        }
        args
    }

    /// 操作 `id` を実行する。`file` は読み込むファイル、`args` は操作ごとの指定
    /// (例: G-code の `post`、シミュレーションの `cell`)。時間のかかる仕事は終わるまで待つ。
    pub fn run(
        &mut self,
        id: &str,
        selected: Option<FeatureId>,
        file: Option<&Path>,
        args: &Json,
    ) -> Result<Ran, kmb_core::Problem> {
        let picks = Picks {
            selected,
            ..Picks::default()
        };
        Ok(self.run_with(id, &picks, file, args)?.into_ran())
    }

    /// 操作 `id` を、画面で選んでいる物 `picks` で実行する。使えない操作は断り、文書の変更は
    /// ここで適用して再計算する。ほかの結果(ファイル・知らせ・表・時間のかかる仕事)は扱わずに
    /// 返す(画面は窓に出し、仕事は裏で走らせる。CLI・MCP は [`Self::run`] が文にして待つ)。
    pub fn run_with(
        &mut self,
        id: &str,
        picks: &Picks<'_>,
        file: Option<&Path>,
        args: &Json,
    ) -> Result<Handled, kmb_core::Problem> {
        let cx = self.context_with(picks, file, args);
        // 使えない操作はここで断る(画面のボタンを押せなくするだけでは、CLI・MCP から走ってしまう)
        if let Some(why) = self
            .modules
            .extensions
            .action(id, &cx)
            .and_then(|a| a.disabled)
        {
            return Err(kmb_core::Problem::new(
                kmb_core::ProblemKind::Unavailable,
                format!("操作 {id} は今使えません: {why}"),
            ));
        }
        let outcome = self.modules.extensions.run(id, &cx).map_err(|mut e| {
            // 知らない操作なら、打ち間違いに近い ID を添える(10-24 m1)
            if self.modules.extensions.action(id, &cx).is_some() {
                return e;
            }
            let all = self.actions(picks.selected);
            let near = crate::near::near_names(id, all.iter().map(|a| a.id.as_str()));
            e.message.push_str(&crate::near::hint(&near));
            e
        })?;
        Ok(match outcome {
            kmb_core::Outcome::Change { label, edits } => {
                let added = self
                    .doc
                    .apply_edits(&label, edits)
                    .map_err(|e| e.to_string())?;
                self.recompute();
                Handled::Changed(added)
            }
            other => Handled::Other(Box::new(other)),
        })
    }

    /// [`Ran::Files`] を `base` の名前に `-<名前の後ろ>` を付けて書く。書いた場所を返す。
    pub fn write_files(base: &Path, files: &[(String, Vec<u8>)]) -> Result<Vec<PathBuf>, String> {
        files
            .iter()
            .map(|(tail, contents)| {
                let out = kmb_core::sibling_path(base, tail);
                crate::write_output(&out, contents)?;
                Ok(out)
            })
            .collect()
    }
}

impl Handled {
    /// 画面の扱わない結果を CLI・MCP の形にする(時間のかかる仕事は終わるまで待つ)。
    /// MCP は許したフォルダを `Picks::files` に入れて [`Headless::run_with`] を呼び、これで受ける。
    pub fn into_ran(self) -> Ran {
        match self {
            Handled::Changed(added) => Ran::Changed(added),
            Handled::Other(outcome) => match *outcome {
                kmb_core::Outcome::Change { .. } => {
                    unreachable!("文書の変更は run_with が適用する")
                }
                kmb_core::Outcome::File {
                    suffix,
                    contents,
                    info,
                } => Ran::File {
                    suffix,
                    contents,
                    info,
                },
                kmb_core::Outcome::Files { files, info } => Ran::Files { files, info },
                kmb_core::Outcome::Message(m) => Ran::Message(m),
                kmb_core::Outcome::Text { title, body } => Ran::Message(format!("{title}\n{body}")),
                kmb_core::Outcome::Table { title, table } => {
                    Ran::Message(format!("{title}\n{}", table.text()))
                }
                kmb_core::Outcome::Job(job) => Ran::Report(job(&kmb_core::JobControl::new())),
            },
        }
    }
}
