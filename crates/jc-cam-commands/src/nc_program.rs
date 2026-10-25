//! NC プログラムのまとめ方の操作: 手動 NC とパターンを足す、一部の加工だけ・全セットアップを
//! 1 本に・工具の順に並べ替えて G-code を書き出す、HTML の加工指示書を書き出す。
//!
//! 既定の「G-code を書き出す」(1 セットアップ = 1 ファイル、タイムラインの順、抑制した加工は
//! 入れない)は commands.rs にあり、ここの操作はそれと別の操作として並べる。画面は操作を並べて
//! 実行するだけ。

use crate::commands::{CamCommands, file_part, last_setup, selected_setup};
use crate::{MANUAL_NC, PATTERN, SETUP};
use jc_core::{Action, Context, FeatureId, Outcome, Place};
use jc_toolpath::Move;
use serde_json::json;

/// 加工の間に手動 NC(停止・コメントなど)を足す
pub const ADD_MANUAL_NC: &str = "cam.add.manual_nc";
/// 選んだ加工のパターンを足す
pub const ADD_PATTERN: &str = "cam.add.pattern";
/// 選んだ加工だけを G-code に書き出す(指定 `operations` で複数)
pub const EXPORT_GCODE_SELECTED: &str = "cam.export_gcode_selected";
/// 全部のセットアップを 1 本の G-code に書き出す
pub const EXPORT_GCODE_ALL: &str = "cam.export_gcode_all";
/// 工具の順に並べ替えて G-code を書き出す
pub const EXPORT_GCODE_BY_TOOL: &str = "cam.export_gcode_by_tool";
/// HTML の加工指示書(上から見た経路の図つき)を書き出す
pub const SETUP_SHEET_HTML: &str = "cam.setup_sheet_html";

/// 操作の並びを置くツールバーのまとまり
const MENU: &str = "NC プログラム";

/// 選んでいる機能が、経路を持つ加工ならその ID。
fn selected_path_op(cx: &Context<'_>) -> Option<FeatureId> {
    let sel = cx.selected?;
    cx.output(sel, "toolpath")?;
    Some(sel)
}

/// 書き出す加工: 指定 `operations`(機能の番号の一覧)か、選んでいる加工か手動 NC。
fn chosen_ops(cx: &Context<'_>) -> Result<Vec<FeatureId>, String> {
    if let Some(list) = cx.arg("operations") {
        let ids: Vec<FeatureId> = serde_json::from_value(list.clone())
            .map_err(|_| "operations は機能の番号の一覧で指定します".to_string())?;
        if ids.is_empty() {
            return Err("書き出す加工がありません".into());
        }
        return Ok(ids);
    }
    let sel = cx.selected.ok_or("加工を選んでいません")?;
    let f = cx.data.feature(sel).ok_or("機能がありません")?;
    if f.type_id == SETUP || !crate::is_operation(cx.registry, &f.type_id) {
        return Err("タイムラインで加工を選んでから使う".into());
    }
    Ok(vec![sel])
}

/// 機能 `id` のセットアップ。
fn setup_of(cx: &Context<'_>, id: FeatureId) -> Option<FeatureId> {
    let f = cx.data.feature(id)?;
    serde_json::from_value(f.data.get("setup")?.clone()).ok()
}

pub(crate) fn actions(cx: &Context<'_>) -> Vec<Action> {
    let setup = selected_setup(cx).is_some();
    let op = cx
        .selected
        .and_then(|s| cx.data.feature(s))
        .is_some_and(|f| f.type_id != SETUP && crate::is_operation(cx.registry, &f.type_id));
    let path_op = selected_path_op(cx).is_some();
    let any_setup = last_setup(cx).is_some();
    let menu_here: &[Place] = if setup { &[Place::FeatureMenu] } else { &[] };
    vec![
        Action::new(ADD_MANUAL_NC, "手動 NC", "cam")
            .menu(MENU)
            .icon("manual_nc")
            .aliases("しゅどう manual nc ていし stop pause m0 m600 くらんぷ clamp きりくず こめんと ていりゅう dwell")
            .hint("加工の間で止める(クランプの付け替え・切りくず払い)。加工を選んでいればその後に入れる。止める前に主軸を止めて退避し、再開の後は主軸を回し直して待つ")
            // NC プログラムの道具は前に出さない(Kie 2026-10-21 の画板)
            .at(if setup {
                &[Place::Menu, Place::FeatureMenu]
            } else {
                &[Place::Menu]
            })
            .disabled_if((!any_setup).then_some("先にセットアップを足す")),
        Action::new(ADD_PATTERN, "加工のパターン", "cam")
            .menu(MENU)
            .icon("campat")
            .aliases("ぱたーん pattern はいれつ array くりかえし みらー mirror かがみ えんけい くけい")
            .hint("選んだ加工の経路を、格子・円・鏡写しで写す(1 枚の板から同じ部品を何個も削る)")
            .at(if path_op {
                &[Place::Menu, Place::FeatureMenu]
            } else {
                &[Place::Menu]
            })
            .disabled_if((!path_op).then_some("タイムラインで経路のできた加工を選んでから使う")),
        Action::new(EXPORT_GCODE_SELECTED, "この加工だけ G-code に書き出す", "cam")
        .arg(jc_core::Arg::list("operations", "書き出す加工(機能の番号の並び)。省くと選んでいる加工"))
        .arg(crate::commands::post_arg())
            .menu("出力")
            .icon("gcodeout")
            .aliases("いちぶ ぶぶん えらんだ selected partial gcode かきだし やりなおし おれた")
            .hint("選んだ加工だけを書き出す(工具が折れた後に 1 つだけやり直す時など)。CLI・MCP では operations で複数を選べる")
            .at(if op { &[Place::FeatureMenu] } else { &[] })
            .disabled_if((!op && cx.arg("operations").is_none()).then_some("タイムラインで加工を選んでから使う")),
        Action::new(EXPORT_GCODE_ALL, "全セットアップを 1 本に書き出す", "cam")
        .arg(crate::commands::post_arg())
            .menu("出力")
            .icon("gcodeout")
            .aliases("ぜんぶ すべて all setups まとめ ひとつ gcode かきだし")
            .hint("セットアップの間に停止と付け替えを促すコメントを入れて、1 本の G-code にする")
            .disabled_if((!any_setup).then_some("先にセットアップを足す")),
        Action::new(EXPORT_GCODE_BY_TOOL, "工具の順に並べ替えて書き出す", "cam")
        .arg(crate::commands::post_arg())
            .menu("出力")
            .icon("gcodesort")
            .aliases("こうぐじゅん ならべかえ tool order sort こうぐこうかん へらす gcode")
            .hint("同じ工具の加工をまとめて工具交換を減らす。残り削り・手動 NC・切り抜きの輪郭が前に動く時は並べ替えずに理由を出す")
            // セットアップを選んでもリボンのボタンがずれないよう、▾ の一覧には常に置く(リボンの
            // 前には出さない)
            .at(&[Place::Menu])
            .at(menu_here)
            .disabled_if((!setup).then_some("タイムラインでセットアップを選んでから使う")),
        Action::new(SETUP_SHEET_HTML, "加工指示書(HTML)を書き出す", "cam")
            .menu("出力")
            .icon("sheethtml")
            .aliases("かこうしじしょ setup sheet html だんどり いんさつ print")
            .hint("段取り表に、加工ごとの送り・切り込み・長さと、上から見た経路の図を付けた HTML を文書の隣に書く。ブラウザで開いて印刷できる")
            // セットアップを選んでもリボンのボタンがずれないよう、▾ の一覧には常に置く(リボンの
            // 前には出さない)
            .at(&[Place::Menu])
            .at(menu_here)
            .disabled_if((!setup).then_some("タイムラインでセットアップを選んでから使う")),
    ]
}

/// 経路の切削の動き(早送りを除く)の XY の幅。
fn cut_extent(moves: &[Move]) -> Option<[f64; 2]> {
    let first = moves.first()?;
    let mut cur = first.to();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for m in &moves[1..] {
        if !matches!(m, Move::Rapid { .. }) {
            for p in m.points(cur, 1.0) {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        cur = m.to();
    }
    (lo[0] <= hi[0]).then(|| [hi[0] - lo[0], hi[1] - lo[1]])
}

fn add_manual_nc(cx: &Context<'_>) -> Result<Outcome, String> {
    let setup = selected_setup(cx)
        .or_else(|| last_setup(cx))
        .ok_or("セットアップがありません")?;
    let mut data = json!({"setup": setup.0, "kind": "stop"});
    if let Some(op) = selected_path_op(cx)
        && setup_of(cx, op) == Some(setup)
    {
        data["after"] = json!(op.0);
    }
    Ok(Outcome::insert(
        "手動 NC を追加",
        MANUAL_NC,
        format!("手動 NC{}", cx.count_of(MANUAL_NC) + 1),
        data,
    ))
}

fn add_pattern(cx: &Context<'_>) -> Result<Outcome, String> {
    let src = selected_path_op(cx).ok_or("経路のできた加工を選んでいません")?;
    let (setup, tp) = cx
        .output(src, "toolpath")
        .and_then(crate::toolpath_of)
        .ok_or("加工の経路が読めません")?;
    let name = cx
        .data
        .feature(src)
        .map_or(String::new(), |f| f.name.clone());
    // 初めの間隔は、削る所の幅 + 5 mm(写しが重ならない)
    let ext = cut_extent(&tp.moves).unwrap_or([25.0, 25.0]);
    let gap = |w: f64| format!("{} mm", (w + 5.0).ceil());
    Ok(Outcome::insert(
        "パターンを追加",
        PATTERN,
        format!("{name} のパターン"),
        json!({
            "setup": setup.0,
            "source": src.0,
            "kind": "rect",
            "count_x": 2,
            "count_y": 1,
            "spacing_x": gap(ext[0]),
            "spacing_y": gap(ext[1]),
        }),
    ))
}

impl CamCommands {
    pub(crate) fn run_nc_program(
        &self,
        id: &str,
        cx: &Context<'_>,
    ) -> Option<Result<Outcome, String>> {
        Some(match id {
            ADD_MANUAL_NC => add_manual_nc(cx),
            ADD_PATTERN => add_pattern(cx),
            EXPORT_GCODE_SELECTED => self.export_selected(cx),
            EXPORT_GCODE_ALL => self.export_all(cx),
            EXPORT_GCODE_BY_TOOL => self.export_by_tool(cx),
            SETUP_SHEET_HTML => self.sheet_html(cx),
            _ => return None,
        })
    }

    /// 対象のセットアップ(選んでいるもの。CLI・MCP で何も選んでいなければ最初のもの)。
    fn target_setup(&self, cx: &Context<'_>) -> Result<FeatureId, String> {
        match cx.selected {
            Some(_) => selected_setup(cx).ok_or_else(|| "セットアップを選んでいません".into()),
            None => cx
                .data
                .features
                .iter()
                .find(|f| f.type_id == SETUP)
                .map(|f| f.id)
                .ok_or_else(|| "セットアップがありません".into()),
        }
    }

    fn file(
        &self,
        suffix: String,
        g: String,
        post: String,
        tools: Vec<u32>,
        extra: serde_json::Value,
    ) -> Outcome {
        let mut info = json!({
            "post": post,
            "lines": g.lines().count(),
            "tools": tools,
        });
        if let (Some(o), Some(e)) = (info.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        Outcome::File {
            suffix,
            contents: g.into_bytes(),
            info,
        }
    }

    fn export_selected(&self, cx: &Context<'_>) -> Result<Outcome, String> {
        let ids = chosen_ops(cx)?;
        let setup = setup_of(cx, ids[0]).ok_or("加工のセットアップが分かりません")?;
        if let Some(other) = ids.iter().find(|i| setup_of(cx, **i) != Some(setup)) {
            return Err(format!(
                "{other} は別のセットアップの加工です。一緒に書き出すのは同じセットアップの加工だけ"
            ));
        }
        let s = self.setup_paths(cx, setup)?.only(&ids)?;
        let (g, post) = self.gcode_text(cx, &s)?;
        let names: Vec<String> = s.paths.iter().map(|p| file_part(&p.name)).collect();
        let tools = s.program().tools().iter().map(|t| t.number).collect();
        let base = self.setup_paths(cx, setup)?.name.clone();
        Ok(self.file(
            format!(
                "{}-{}.{}",
                file_part(&base),
                names.join("-"),
                jc_post::file_extension(&post)
            ),
            g,
            post,
            tools,
            json!({"setup": setup.0, "operations": ids.iter().map(|i| i.0).collect::<Vec<_>>()}),
        ))
    }

    fn export_all(&self, cx: &Context<'_>) -> Result<Outcome, String> {
        let all = self.paths_all(cx);
        let program = crate::chain_program(&all)?;
        let machine = crate::chain_machine(&all)?;
        let (g, post) = self.with_post(cx, &machine, |post, m| {
            post.emit(&program, m).map_err(|e| e.to_string())
        })?;
        let tools = program.tools().iter().map(|t| t.number).collect();
        let setups: Vec<u64> = all
            .iter()
            .filter(|s| !s.paths.is_empty())
            .map(|s| s.setup.0)
            .collect();
        Ok(self.file(
            format!("全セットアップ.{}", jc_post::file_extension(&post)),
            g,
            post,
            tools,
            json!({"setups": setups}),
        ))
    }

    fn export_by_tool(&self, cx: &Context<'_>) -> Result<Outcome, String> {
        let setup = self.target_setup(cx)?;
        let (mut s, before, after) = self.setup_paths(cx, setup)?.by_tool()?;
        s.name = format!("{}。工具交換 {before} → {after} 回", s.name);
        let (g, post) = self.gcode_text(cx, &s)?;
        let tools = s.program().tools().iter().map(|t| t.number).collect();
        Ok(self.file(
            format!(
                "{}-工具の順.{}",
                file_part(&self.setup_paths(cx, setup)?.name),
                jc_post::file_extension(&post)
            ),
            g,
            post,
            tools,
            json!({"setup": setup.0, "tool_changes_before": before, "tool_changes_after": after}),
        ))
    }

    fn sheet_html(&self, cx: &Context<'_>) -> Result<Outcome, String> {
        let setup = self.target_setup(cx)?;
        let s = self.setup_paths(cx, setup)?;
        Ok(Outcome::File {
            suffix: format!("{}-加工指示書.html", file_part(&s.name)),
            contents: crate::setup_sheet_html(&s, &self.machine_for(&s.info)).into_bytes(),
            info: json!({"setup": setup.0}),
        })
    }
}
