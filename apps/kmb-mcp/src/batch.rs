//! まとめて実行(`batch`)。道具の呼び出しを順に動かし、途中で失敗したら始めの形まで全部戻す。
//! 通った時は、文書を変えた手を 1 回の取り消し単位にまとめる(`undo` 1 回で batch の前に戻る)。
//!
//! 先に全部の手の引数を読み、知らない道具・入れられない道具・引数の誤りがあれば、何もせずに断る。
//! 文書を入れ替える・保存する・取り消す・設定を変える道具は入れられない(戻せないか、戻し方と
//! ぶつかるため)。書き出したファイルは戻さない。
//! `screenshot` の絵は結果に入れない(説明だけ。絵は `out` で書き出す)。

use crate::server::{
    ActionsArg, ActivateComponentArg, AddFeatureArg, CrashArg, EditViewArg, ExportArg, GcodeArg,
    GroupArg, IdArg, ImportArg, MaterialArg, MeasureArg, MoveBodiesArg, MoveComponentArg,
    MoveFeaturesArg, ParamArg, PasteFeaturesArg, PropertiesArg, RenameArg, RenameViewArg,
    RollBackArg, RunActionArg, SaveViewArg, ScreenshotArg, SetFeatureArg, SimArg, SuppressArg,
    TopologyArg, ValueArg, ViewsArg,
};
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

/// 読み終えた 1 手。
pub enum Call {
    Status,
    SetParam(ParamArg),
    RenameParam(RenameArg),
    SetValue(ValueArg),
    ParamViews(ViewsArg),
    EditParamView(EditViewArg),
    Types,
    GetFeature(IdArg),
    AddFeature(AddFeatureArg),
    ImportStep(ImportArg),
    RollBack(RollBackArg),
    Properties(PropertiesArg),
    SetFeature(SetFeatureArg),
    Suppress(SuppressArg),
    RemoveFeature(IdArg),
    MoveFeatures(MoveFeaturesArg),
    PasteFeatures(PasteFeaturesArg),
    MoveComponent(MoveComponentArg),
    MoveBodies(MoveBodiesArg),
    ActivateComponent(ActivateComponentArg),
    DuplicateComponent(IdArg),
    NamedViews,
    SaveNamedView(SaveViewArg),
    RenameNamedView(RenameViewArg),
    RemoveNamedView(IdArg),
    Group(GroupArg),
    Ungroup(IdArg),
    History,
    Settings,
    Actions(ActionsArg),
    RunAction(RunActionArg),
    Mass,
    SetMaterial(MaterialArg),
    Topology(TopologyArg),
    Measure(MeasureArg),
    Screenshot(ScreenshotArg),
    Export(ExportArg),
    Gcode(GcodeArg),
    Simulate(SimArg),
    CrashReports(CrashArg),
}

/// batch に入れられない道具と、その理由。
fn refused(tool: &str) -> Option<&'static str> {
    Some(match tool {
        "open" | "sample" => "文書を入れ替える道具は、戻せないので batch に入れられません",
        "save" => "保存は batch の外で(途中の形を保存しないため)",
        "app" => "画面の道具は、画面の取り消しの履歴に入るので batch の外で使います",
        "versions" | "restore_version" => {
            "版の履歴は保存したファイルの物なので、batch の外で使います"
        }
        "undo" | "redo" => {
            "取り消し・やり直しは batch に入れられません(失敗した時は batch が戻します)"
        }
        "set_settings" => "設定の変更は戻せないので batch に入れられません",
        "batch" => "batch の中に batch は入れられません",
        _ => return None,
    })
}

fn args<T: DeserializeOwned>(tool: &str, a: Json) -> Result<T, String> {
    let a = if a.is_null() { json!({}) } else { a };
    serde_json::from_value(a).map_err(|e| format!("{tool} の引数を読めません: {e}"))
}

/// 道具の名前と引数を読む。知らない道具、入れられない道具、引数の誤りは理由を返す。
pub fn parse(tool: &str, a: Json) -> Result<Call, String> {
    if let Some(why) = refused(tool) {
        return Err(format!("{tool}: {why}"));
    }
    Ok(match tool {
        "status" => Call::Status,
        "set_param" => Call::SetParam(args(tool, a)?),
        "rename_param" => Call::RenameParam(args(tool, a)?),
        "set_value" => Call::SetValue(args(tool, a)?),
        "param_views" => Call::ParamViews(args(tool, a)?),
        "edit_param_view" => Call::EditParamView(args(tool, a)?),
        "types" => Call::Types,
        "get_feature" => Call::GetFeature(args(tool, a)?),
        "add_feature" => Call::AddFeature(args(tool, a)?),
        "import_step" => Call::ImportStep(args(tool, a)?),
        "roll_back" => Call::RollBack(args(tool, a)?),
        "properties" => Call::Properties(args(tool, a)?),
        "set_feature" => Call::SetFeature(args(tool, a)?),
        "suppress" => Call::Suppress(args(tool, a)?),
        "remove_feature" => Call::RemoveFeature(args(tool, a)?),
        "move_features" => Call::MoveFeatures(args(tool, a)?),
        "paste_features" => Call::PasteFeatures(args(tool, a)?),
        "move_component" => Call::MoveComponent(args(tool, a)?),
        "move_bodies" => Call::MoveBodies(args(tool, a)?),
        "activate_component" => Call::ActivateComponent(args(tool, a)?),
        "duplicate_component" => Call::DuplicateComponent(args(tool, a)?),
        "named_views" => Call::NamedViews,
        "save_named_view" => Call::SaveNamedView(args(tool, a)?),
        "rename_named_view" => Call::RenameNamedView(args(tool, a)?),
        "remove_named_view" => Call::RemoveNamedView(args(tool, a)?),
        "group" => Call::Group(args(tool, a)?),
        "ungroup" => Call::Ungroup(args(tool, a)?),
        "history" => Call::History,
        "settings" => Call::Settings,
        "actions" => Call::Actions(args(tool, a)?),
        "run_action" => Call::RunAction(args(tool, a)?),
        "mass" => Call::Mass,
        "set_material" => Call::SetMaterial(args(tool, a)?),
        "topology" => Call::Topology(args(tool, a)?),
        "measure" => Call::Measure(args(tool, a)?),
        "screenshot" => Call::Screenshot(args(tool, a)?),
        "export" => Call::Export(args(tool, a)?),
        "gcode" => Call::Gcode(args(tool, a)?),
        "simulate" => Call::Simulate(args(tool, a)?),
        "crash_reports" => Call::CrashReports(args(tool, a)?),
        t => {
            // 打ち間違いに近い、batch に入れられる道具の名前を添える(コマンド欄と同じ案内)
            let names: Vec<String> = crate::Server::tools()
                .into_iter()
                .map(|(n, _)| n)
                .filter(|n| refused(n).is_none())
                .collect();
            let near = kmb_session::near::near_names(t, names.iter().map(String::as_str));
            return Err(format!(
                "知らない道具です: {t}{}",
                kmb_session::near::hint(&near)
            ));
        }
    })
}

impl Call {
    /// 道具を 1 つずつ呼んだ時と同じ中身を動かす。
    /// パラメータと機能の値の式を直すだけの手か。続けても前の手の作り直しの結果を読まない
    /// (名前の一覧は変わらない)ので、作り直しを後へ回せる。
    fn only_sets_values(&self) -> bool {
        matches!(self, Call::SetParam(_) | Call::SetValue(_))
    }

    pub fn run(self, s: &mut Session) -> Reply {
        match self {
            Call::Status => Ok(s.status()),
            Call::SetParam(a) => {
                s.set_param_info(&a.name, &a.expr, a.comment.as_deref(), a.unit.as_deref())
            }
            Call::RenameParam(a) => s.rename_param(&a.name, &a.new_name),
            Call::SetValue(a) => s.set_value(a.feature, &a.key, &a.expr),
            Call::ParamViews(a) => s.param_views(a.id),
            Call::EditParamView(a) => s.edit_param_view(&a),
            Call::Types => Ok(s.types()),
            Call::GetFeature(a) => s.feature_data(a.id),
            Call::AddFeature(a) => s.add_feature(&a.type_id, a.name.as_deref(), a.data),
            Call::ImportStep(a) => s.import_step(&a.path, a.stand_up),
            Call::RollBack(a) => s.roll_back(a.position),
            Call::Properties(a) => s.properties(&a.set),
            Call::SetFeature(a) => s.set_feature(a.id, a.data),
            Call::Suppress(a) => s.suppress(a.id, a.suppressed),
            Call::RemoveFeature(a) => s.remove_feature(a.id),
            Call::MoveFeatures(a) => s.move_features(&a.ids, a.target, a.after),
            Call::PasteFeatures(a) => s.paste_features(&a.ids),
            Call::MoveComponent(a) => s.move_component(&a),
            Call::MoveBodies(a) => s.move_bodies(&a.ids, a.component),
            Call::ActivateComponent(a) => s.activate_component(a.component),
            Call::DuplicateComponent(a) => s.duplicate_component(a.id),
            Call::NamedViews => Ok(s.named_views()),
            Call::SaveNamedView(a) => s.save_named_view(&a),
            Call::RenameNamedView(a) => s.rename_named_view(a.id, &a.name),
            Call::RemoveNamedView(a) => s.remove_named_view(a.id),
            Call::Group(a) => s.group(&a.ids, &a.name, &a.mode),
            Call::Ungroup(a) => s.ungroup(a.id),
            Call::History => Ok(s.history()),
            Call::Settings => Ok(s.settings()),
            Call::Actions(a) => Ok(s.actions(a.selected)),
            Call::RunAction(a) => s.run_action_at(
                &a.id,
                a.selected,
                a.file.as_deref(),
                &a.args,
                a.out.as_deref(),
            ),
            Call::Mass => s.mass(),
            Call::SetMaterial(a) => s.set_material(a.body, &a.material),
            Call::Topology(a) => s.topology(a.body),
            Call::Measure(a) => s.measure(&a.targets, a.basis.as_deref()),
            // 絵は batch の結果に入れない(大きいので)。out で書き出す
            Call::Screenshot(a) => s.screenshot(&a).map(|(_, info)| info).map_err(Into::into),
            Call::Export(a) => s.export(
                &a.path,
                a.action.as_deref(),
                a.view.as_ref(),
                a.tolerance,
                a.each,
            ),
            Call::Gcode(a) => s.gcode(a.setup, a.out.as_deref(), a.post.as_deref()),
            Call::Simulate(a) => s.simulate(a.setup, a.cell),
            Call::CrashReports(a) => s.crash_reports(a.name.as_deref()),
        }
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
