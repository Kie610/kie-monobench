//! 図の右クリックのメニュー(放射状にはしない)。
//! 道具の中・何も選んでいない時・選んでいる時で、出す項目を分ける(網羅版 B6〜B8)。
//! メニューは押された項目([`Ask`])を返すだけで、形を変えるのは view.rs。

use crate::view::{DimValues, Editor, Tool, ViewState};
use jc_sketch::{ConstraintKind, Entity, EntityId, Sketch, Solution};

/// メニューで押した項目。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Ask {
    /// 道具を終える(描いた所までは残す。打った値があればそれで作ってから)
    Ok,
    /// 描きかけを捨てて道具を終える
    Cancel,
    /// 道具を選ぶ(繰り返し・最近の道具)
    Tool(Tool),
    /// 選んだ要素に寸法を付ける(寸法の道具へ。選びは残す)
    Dimension,
    Undo,
    Redo,
    /// 全体が収まるように表示する
    Fit,
    Delete,
    Deselect,
    /// 図の印で選んだ拘束を外す
    Unconstrain,
    /// 選んだ半径の寸法を直径に、直径を半径にする
    RoundDim,
    /// 補助線の切り替え(X)
    Construction,
    /// 中心線の切り替え(線分だけ)
    Centerline,
    /// 固定する(真)か、固定を外す(偽)
    Fix(bool),
    /// 円の中心の点だけを固定する・外す
    FixCenter(bool),
    Horizontal,
    Vertical,
    /// 選んだ 2 つに拘束を付ける(帯のボタンの名前。K20)
    Constrain(&'static str),
    /// 選んだ線のオフセットの拘束を外す(写しを元とは別に動かせるようにする。K28)
    BreakOffset,
    /// 選んだ点(線の端も)の一致を外す
    Uncoincide,
    /// 選んだ線をオフセットの道具で写す(O)
    Offset,
    /// オフセットの向き(片側・両側・対称)
    OffsetMode(crate::offset_tool::Mode),
    /// オフセットの角を丸める(真)か、辺を延ばして角を作る(偽)
    OffsetRound(bool),
    /// 寸法の道具で、ついてくる寸法の種類を選び直す
    DimChoice(crate::dim_choice::DimChoice),
    /// 選んだ要素を動かす道具へ
    Move,
    Copy,
    Cut,
    Paste,
    /// 次に描く線を補助線にする(描く前に線種を選ぶ。もう一度で戻す)
    DrawConstruction,
    /// 押し出しへ(E)
    Extrude,
    /// パラメータの一覧を開く
    Params,
    /// モジュールの操作を役割で探し、直すのを終えずに実行する(DXF として保存)
    Run(jc_core::ActionRole),
}

/// メニューを出す時の様子(開いた時に覚えておく)。
#[derive(Clone, Debug)]
pub(crate) struct Context {
    pub tool: Tool,
    pub selection: Vec<EntityId>,
    pub last_tool: Option<Tool>,
    pub recent: Vec<Tool>,
    pub can_paste: bool,
    /// 図の印で選んだ拘束の名前
    pub picked_constraint: Option<&'static str>,
    /// 選んだ寸法が直径なら真、半径なら偽(半径・直径の寸法でなければ `None`)
    pub round_dim: Option<bool>,
    pub drawing_construction: bool,
    /// 描いている途中か
    pub busy: bool,
    /// オフセットの道具の中なら、今の向き
    /// オフセットの道具の中なら、ずらす向きと角を丸めるか
    pub offset_mode: Option<(crate::offset_tool::Mode, bool)>,
    /// 寸法の道具で、ついてくる寸法の選び直し(名前と選び直し)
    pub dim_options: Vec<(&'static str, crate::dim_choice::DimChoice)>,
}

/// 何かの線(点でない要素)を選んでいるか。
fn has_curves(sketch: &Sketch, sel: &[EntityId]) -> bool {
    sel.iter()
        .any(|id| !matches!(sketch.entity(*id), Some(Entity::Point { .. }) | None))
}

/// 選んだ要素の点(点そのもの、線の端、円の中心など)。
pub(crate) fn points_of(sketch: &Sketch, sel: &[EntityId]) -> Vec<EntityId> {
    let mut out = Vec::new();
    for id in sel {
        let pts = match sketch.entity(*id) {
            Some(Entity::Point { id, .. }) => vec![*id],
            Some(e) => e.points(),
            None => vec![],
        };
        for p in pts {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// 円か。
fn is_circle(sketch: &Sketch, id: EntityId) -> bool {
    matches!(sketch.entity(id), Some(Entity::Circle { .. }))
}

/// 固定の拘束を付ける先: 円は円そのもの(中心は別に固定する)、ほかは点(線の端など)。
fn fix_targets(sketch: &Sketch, sel: &[EntityId]) -> Vec<EntityId> {
    let (circles, others): (Vec<EntityId>, Vec<EntityId>) =
        sel.iter().partition(|id| is_circle(sketch, **id));
    let mut out = points_of(sketch, &others);
    out.extend(circles);
    out
}

/// 選んだ要素に、固定の拘束が付いているか(円は円そのものの固定だけを見る。中心の固定は別)。
fn has_fix(sketch: &Sketch, sel: &[EntityId]) -> bool {
    let pts = fix_targets(sketch, sel);
    sketch
        .constraints
        .iter()
        .any(|c| matches!(c.kind, ConstraintKind::Fix { point } if pts.contains(&point)))
}

/// 選んだ要素が関わるオフセットの拘束。
pub(crate) fn offsets_of(sketch: &Sketch, sel: &[EntityId]) -> Vec<jc_sketch::ConstraintId> {
    sketch
        .constraints
        .iter()
        .filter(|c| match &c.kind {
            ConstraintKind::Offset { parts } => parts
                .iter()
                .any(|p| p.refs().iter().any(|r| sel.contains(r))),
            _ => false,
        })
        .map(|c| c.id)
        .collect()
}

/// 拘束の名前(帯のボタン)に付ける、メニューの文。
fn constrain_label(name: &str, as_name: &str) -> String {
    match (name, as_name) {
        ("一致", "線上") => "線上に置く".into(),
        ("一致", "円周上") => "円周上に置く".into(),
        ("一致", "曲線上") => "曲線上に置く".into(),
        ("一致", _) => "一致させる".into(),
        ("平行", _) => "平行にする".into(),
        ("直交", _) => "直交させる".into(),
        ("等しい", _) => "等しくする".into(),
        ("接線", _) => "接線にする".into(),
        ("同心円", _) => "同心円にする".into(),
        ("同一直線上", _) => "同一直線上にする".into(),
        _ => format!("{as_name}にする"),
    }
}

/// 選んだ点(線の端など)に、一致の拘束が付いているか。
fn has_coincident(sketch: &Sketch, sel: &[EntityId]) -> bool {
    let pts = points_of(sketch, sel);
    sketch.constraints.iter().any(|c| match c.kind {
        ConstraintKind::Coincident { a, b } => pts.contains(&a) || pts.contains(&b),
        ConstraintKind::PointOnLine { point, .. } | ConstraintKind::PointOnCircle { point, .. } => {
            pts.contains(&point)
        }
        _ => false,
    })
}

fn item(ui: &mut egui::Ui, text: &str, key: &str, out: &mut Option<Ask>, ask: Ask) {
    let label = if key.is_empty() {
        text.to_string()
    } else {
        format!("{text}({key})")
    };
    if ui.button(label).clicked() {
        *out = Some(ask);
        ui.close();
    }
}

/// 右クリックのメニューの中身。押した項目を返す。
pub(crate) fn show(ui: &mut egui::Ui, cx: &Context, sketch: &Sketch) -> Option<Ask> {
    let mut out = None;
    let sel = &cx.selection;
    if cx.tool != Tool::Select {
        // 道具の中
        ui.weak(crate::tool_label(cx.tool));
        item(ui, "OK", "Enter", &mut out, Ask::Ok);
        item(ui, "キャンセル", "Esc", &mut out, Ask::Cancel);
        if crate::view::draws(cx.tool) {
            let text = if cx.drawing_construction {
                "補助線で描くのをやめる"
            } else {
                "補助線で描く"
            };
            item(ui, text, "X", &mut out, Ask::DrawConstruction);
        }
        // 寸法の道具: 置く前に寸法の種類を選び直す
        if !cx.dim_options.is_empty() {
            ui.separator();
            for (label, c) in &cx.dim_options {
                item(ui, label, "", &mut out, Ask::DimChoice(*c));
            }
        }
        if let Some((mode, round)) = cx.offset_mode {
            ui.separator();
            for m in [
                crate::offset_tool::Mode::One,
                crate::offset_tool::Mode::Both,
                crate::offset_tool::Mode::Symmetric,
            ] {
                if ui.radio(m == mode, m.label()).clicked() {
                    out = Some(Ask::OffsetMode(m));
                    ui.close();
                }
            }
            ui.separator();
            for (r, label) in crate::offset_tool::CORNERS {
                if ui.radio(r == round, label).clicked() {
                    out = Some(Ask::OffsetRound(r));
                    ui.close();
                }
            }
        }
        if !sel.is_empty() && !cx.busy {
            ui.separator();
            item(ui, "削除", "Delete", &mut out, Ask::Delete);
            item(ui, "移動/コピー", "", &mut out, Ask::Move);
        }
    } else {
        match cx.last_tool {
            Some(t) => item(
                ui,
                &format!("繰り返し: {}", crate::tool_label(t)),
                "",
                &mut out,
                Ask::Tool(t),
            ),
            None => {
                ui.label("繰り返す道具はまだありません");
            }
        }
        if let Some(diameter) = cx.round_dim {
            let label = if diameter {
                "半径の寸法にする"
            } else {
                "直径の寸法にする"
            };
            item(ui, label, "", &mut out, Ask::RoundDim);
        }
        if let Some(name) = cx.picked_constraint {
            item(
                ui,
                &format!("{name}の拘束を外す"),
                "Delete",
                &mut out,
                Ask::Unconstrain,
            );
        }
        if sel.is_empty() {
            item(ui, "元に戻す", "Ctrl+Z", &mut out, Ask::Undo);
            item(ui, "やり直し", "Ctrl+Y", &mut out, Ask::Redo);
            if cx.can_paste {
                item(ui, "貼り付け", "Ctrl+V", &mut out, Ask::Paste);
            }
            ui.separator();
            item(ui, "すべて表示", "ホイール二度押し", &mut out, Ask::Fit);
            item(ui, "パラメータを変更", "", &mut out, Ask::Params);
            item(
                ui,
                "DXF として保存…",
                "",
                &mut out,
                Ask::Run(jc_interchange::role::action::EXPORT_SKETCH_DXF),
            );
            item(
                ui,
                "押し出し",
                &jc_ui::keys::label("extrude"),
                &mut out,
                Ask::Extrude,
            );
            let recent: Vec<Tool> = if cx.recent.is_empty() {
                vec![Tool::Line, Tool::Rectangle, Tool::Circle]
            } else {
                cx.recent.clone()
            };
            ui.separator();
            ui.weak("最近の道具");
            for t in recent {
                let key = crate::view::tool_key(t).unwrap_or_default();
                item(ui, crate::tool_label(t), &key, &mut out, Ask::Tool(t));
            }
        } else {
            // 選んだ要素
            item(
                ui,
                "スケッチ寸法",
                &jc_ui::keys::label("sketch_dimension"),
                &mut out,
                Ask::Dimension,
            );
            if has_curves(sketch, sel) {
                item(ui, "標準/補助線", "X", &mut out, Ask::Construction);
            }
            if sel
                .iter()
                .any(|id| matches!(sketch.entity(*id), Some(Entity::Line { .. })))
            {
                item(ui, "標準/中心線", "", &mut out, Ask::Centerline);
            }
            if has_fix(sketch, sel) {
                item(ui, "固定解除", "", &mut out, Ask::Fix(false));
            } else {
                item(ui, "固定する", "", &mut out, Ask::Fix(true));
            }
            // 円は中心だけも固定できる(大きさは動く)
            if let [id] = sel.as_slice()
                && let Some(Entity::Circle { center, .. }) = sketch.entity(*id)
            {
                if has_fix(sketch, &[*center]) {
                    item(ui, "中心の固定を解除", "", &mut out, Ask::FixCenter(false));
                } else {
                    item(ui, "中心だけ固定する", "", &mut out, Ask::FixCenter(true));
                }
            }
            if let [id] = sel.as_slice()
                && matches!(sketch.entity(*id), Some(Entity::Line { .. }))
            {
                item(ui, "水平にする", "", &mut out, Ask::Horizontal);
                item(ui, "垂直にする", "", &mut out, Ask::Vertical);
            }
            // 2 つを選んだら、そのまま付けられる拘束(帯の拘束と同じ。K20)
            for (name, as_name) in crate::constraint_tool::fitting(sketch, sel) {
                item(
                    ui,
                    &constrain_label(name, as_name),
                    "",
                    &mut out,
                    Ask::Constrain(name),
                );
            }
            if has_coincident(sketch, sel) {
                item(ui, "一致を解除", "", &mut out, Ask::Uncoincide);
            }
            if has_curves(sketch, sel) {
                item(ui, "オフセット", "O", &mut out, Ask::Offset);
            }
            if !offsets_of(sketch, sel).is_empty() {
                item(ui, "オフセットの連動を外す", "", &mut out, Ask::BreakOffset);
            }
            item(ui, "移動/コピー", "", &mut out, Ask::Move);
            ui.separator();
            item(ui, "切り取り", "Ctrl+X", &mut out, Ask::Cut);
            item(ui, "コピー", "Ctrl+C", &mut out, Ask::Copy);
            if cx.can_paste {
                item(ui, "貼り付け", "Ctrl+V", &mut out, Ask::Paste);
            }
            item(ui, "削除", "Delete", &mut out, Ask::Delete);
            item(ui, "選択を外す", "Esc", &mut out, Ask::Deselect);
        }
    }
    out
}

/// 選んだ要素の一致を外したスケッチ(線上・円周上も外す)。外した数も返す。
pub(crate) fn uncoincide(sketch: &Sketch, sel: &[EntityId]) -> (Sketch, usize) {
    let pts = points_of(sketch, sel);
    let mut s = sketch.clone();
    let before = s.constraints.len();
    s.constraints.retain(|c| match c.kind {
        ConstraintKind::Coincident { a, b } => !(pts.contains(&a) || pts.contains(&b)),
        ConstraintKind::PointOnLine { point, .. } | ConstraintKind::PointOnCircle { point, .. } => {
            !pts.contains(&point)
        }
        _ => true,
    });
    let n = before - s.constraints.len();
    (s, n)
}

/// 選んだ要素を固定する(点を固定)か、固定を外す。円は円そのものを固定する(中心と大きさが
/// 止まる)。円の中心の点は別に固定・解除でき、円の固定を外しても中心の
/// 固定は残る(逆も同じ)。
pub(crate) fn set_fixed(sketch: &Sketch, sel: &[EntityId], on: bool) -> Sketch {
    let pts = fix_targets(sketch, sel);
    let mut s = sketch.clone();
    if on {
        let (projected, projected_circles) = s.projected_parts();
        for p in pts {
            let has = s
                .constraints
                .iter()
                .any(|c| matches!(c.kind, ConstraintKind::Fix { point } if point == p));
            if !has && !projected.contains(&p) && !projected_circles.contains(&p) {
                s.add_constraint(ConstraintKind::Fix { point: p });
            }
        }
    } else {
        s.constraints
            .retain(|c| !matches!(c.kind, ConstraintKind::Fix { point } if pts.contains(&point)));
    }
    s
}

/// 拘束を足す。ほかの拘束で既に決まっていれば足さず、何で決まっているかを知らせる。
fn add_checked(
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
    label: &str,
    k: ConstraintKind,
) -> Option<Sketch> {
    if let Some(v) = values
        && let Some(by) = jc_sketch::redundant_with(sketch, v, &k)
    {
        let names: Vec<String> = by
            .iter()
            .filter_map(|id| sketch.constraints.iter().find(|c| c.id == *id))
            .map(jc_sketch::describe)
            .collect();
        editor.notice = Some(if names.is_empty() {
            format!("{label}: 写した形などで既に決まっているので付けません")
        } else {
            format!(
                "{label}: {} で既に決まっているので付けません(付けると過剰な拘束になります)",
                names.join("、")
            )
        });
        return None;
    }
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    s.add_constraint(k);
    Some(s)
}

/// メニューで押した項目を行う。形が変われば新しいスケッチを返す。
pub(crate) fn apply(
    ctx: &egui::Context,
    state: &mut ViewState,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
    ask: Ask,
) -> Option<Sketch> {
    let cx = editor.menu.take();
    let sel: Vec<EntityId> = cx.map(|c| c.selection).unwrap_or_default();
    match ask {
        Ask::Ok => {
            if editor.hud.pinned() || (editor.tool == Tool::Offset && editor.pending.is_some()) {
                // 次のコマで Enter と同じに形を作ってから終える
                editor.ok = true;
            } else {
                editor.finish(editor.tool);
            }
            None
        }
        Ask::Cancel => {
            editor.finish(editor.tool);
            None
        }
        Ask::Tool(t) => {
            editor.set_tool(t);
            None
        }
        Ask::Dimension => {
            editor.set_tool(Tool::Dimension);
            None
        }
        Ask::Undo => {
            editor.app_ask = Some(jc_ui::APP_UNDO);
            None
        }
        Ask::Redo => {
            editor.app_ask = Some(jc_ui::APP_REDO);
            None
        }
        Ask::Params => {
            editor.app_ask = Some(jc_ui::APP_PARAMS);
            None
        }
        Ask::Run(a) => {
            editor.run_action = Some(a);
            None
        }
        Ask::Extrude => {
            editor.wants_extrude = true;
            None
        }
        Ask::Fit => {
            state.fit();
            None
        }
        Ask::Delete => crate::constraints_ui::selection_action(editor, sketch, sol, "delete"),
        Ask::Deselect => {
            editor.selection.clear();
            editor.regions.clear();
            None
        }
        Ask::Unconstrain => crate::view::remove_picked_constraint(editor, sketch, sol),
        Ask::RoundDim => {
            let id = editor.picked_constraint?;
            let mut s = sketch.clone();
            s.adopt_solution(sol);
            s.toggle_radius(id).then_some(s)
        }
        Ask::Construction => {
            crate::constraints_ui::selection_action(editor, sketch, sol, "construction")
        }
        Ask::DimChoice(c) => {
            editor.dim_choice = c;
            // ついてくる寸法を作り直す(値の欄も測り直す)
            editor.ghost = None;
            None
        }
        Ask::Centerline => {
            crate::constraints_ui::selection_action(editor, sketch, sol, "centerline")
        }
        Ask::Fix(on) => {
            let mut s = set_fixed(sketch, &sel, on);
            s.adopt_solution(sol);
            Some(s)
        }
        Ask::FixCenter(on) => {
            let centers: Vec<EntityId> = sel
                .iter()
                .filter_map(|id| match sketch.entity(*id) {
                    Some(Entity::Circle { center, .. }) => Some(*center),
                    _ => None,
                })
                .collect();
            let mut s = set_fixed(sketch, &centers, on);
            s.adopt_solution(sol);
            Some(s)
        }
        Ask::Horizontal | Ask::Vertical => {
            let [line] = sel.as_slice() else {
                return None;
            };
            let (label, k) = if ask == Ask::Horizontal {
                ("水平", ConstraintKind::Horizontal { line: *line })
            } else {
                ("垂直", ConstraintKind::Vertical { line: *line })
            };
            add_checked(editor, sketch, sol, values, label, k)
        }
        Ask::Constrain(name) => {
            crate::constraint_tool::apply_selected(name, editor, sketch, sol, values, &sel)
        }
        Ask::BreakOffset => {
            let ids = offsets_of(sketch, &sel);
            if ids.is_empty() {
                return None;
            }
            log::info!("オフセットの連動を外す: 拘束 {} 個", ids.len());
            let mut s = sketch.clone();
            s.adopt_solution(sol);
            for id in ids {
                s.remove_constraint(id);
            }
            editor.notice = Some(
                "オフセットの連動を外しました(写しは元とは別に動かせます。距離の寸法は残ります)"
                    .into(),
            );
            Some(s)
        }
        Ask::Uncoincide => {
            let (mut s, n) = uncoincide(sketch, &sel);
            if n == 0 {
                editor.notice = Some("外す一致の拘束がありません".into());
                return None;
            }
            s.adopt_solution(sol);
            Some(s)
        }
        Ask::Offset => {
            editor.set_tool(Tool::Offset);
            None
        }
        Ask::OffsetMode(m) => {
            editor.offset.mode = m;
            None
        }
        Ask::OffsetRound(r) => {
            editor.offset.round = r;
            None
        }
        Ask::Move => {
            editor.set_tool(Tool::Move);
            None
        }
        Ask::Copy => {
            crate::clipboard::copy(ctx, editor, sketch, sol);
            None
        }
        Ask::Cut => crate::clipboard::cut(ctx, editor, sketch, sol),
        Ask::Paste => crate::clipboard::paste(ctx, editor, sketch, sol),
        Ask::DrawConstruction => {
            editor.draw_construction = !editor.draw_construction;
            editor.draw_centerline = false;
            None
        }
    }
}

#[cfg(test)]
#[path = "menu_tests.rs"]
pub(crate) mod tests;
