//! 自動拘束の欄と予告。
//! 拘束の「自動拘束」を押すと、足りない拘束と寸法の候補を図に別の色で出し、パネルで種類ごとに
//! 外せる。OK で 1 回の取り消しで付け、キャンセルか Esc でやめる。描いている最中や引いている
//! 最中には走らせない(独立の命令。docs/避ける形.md の自動の拘束 (a))。
//!
//! 対象は選んだ線(無ければ全部)。点を 1 つだけ選んで押すと、その点を位置の寸法の基準にする。
//! 選ばなければ原点。

use crate::view::{DimValues, Editor};
use jc_sketch::{AutoGroup, AutoOptions, AutoResult, Entity, EntityId, Sketch, Solution};
use std::collections::BTreeSet;

/// 予告の状態(文書には入れない)。
pub(crate) struct State {
    /// 押した時のスケッチ(文書のまま。予告の間に図が変わったら予告をやめる)
    pub from: Sketch,
    /// 候補を作ったスケッチ(原点を写した物)と解
    pub sketch: Sketch,
    pub sol: Solution,
    pub result: AutoResult,
    /// 外した種類
    pub off: BTreeSet<AutoGroup>,
    /// 寸法も付ける
    pub dims: bool,
    /// 基準にした点(選んだ点。`None` なら原点)
    pub datum: Option<EntityId>,
    /// 対象の要素(空なら全部)
    pub only: BTreeSet<EntityId>,
}

impl State {
    /// 付ける候補(外した種類を除く)。
    pub fn picks(&self) -> Vec<jc_sketch::Suggestion> {
        self.result
            .suggestions
            .iter()
            .filter(|s| !self.off.contains(&s.group))
            .cloned()
            .collect()
    }
}

/// 候補を作る。`dims` が偽なら寸法を作らない。
fn compute(
    sketch: &Sketch,
    sol: &Solution,
    values: &DimValues,
    dims: bool,
    datum: Option<EntityId>,
    only: BTreeSet<EntityId>,
) -> Result<State, String> {
    let (mut s, mut sol) = (sketch.clone(), sol.clone());
    let base = match datum {
        Some(p) => p,
        None if dims => jc_sketch::with_origin(&mut s, &mut sol),
        None => EntityId(0),
    };
    let opts = AutoOptions {
        dimensions: dims,
        datum: dims.then_some(base),
        only: only.clone(),
        ..Default::default()
    };
    let result = jc_sketch::auto_constrain(&s, &sol, values, &opts).map_err(|e| e.to_string())?;
    Ok(State {
        from: sketch.clone(),
        sketch: s,
        sol,
        result,
        off: BTreeSet::new(),
        dims,
        datum,
        only,
    })
}

/// 「自動拘束」を押した: 選びから対象と基準の点を決めて、候補を作る。
pub(crate) fn start(
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
) {
    let Some(values) = values else {
        editor.notice = Some("寸法の式が計算できないので、自動拘束は使えません".into());
        return;
    };
    let sel: Vec<EntityId> = editor.selection.iter().copied().collect();
    let one_point =
        matches!(sel.as_slice(), [p] if matches!(sketch.entity(*p), Some(Entity::Point { .. })));
    let (datum, only) = if one_point {
        (Some(sel[0]), BTreeSet::new())
    } else {
        (None, sel.into_iter().collect())
    };
    match compute(sketch, sol, values, true, datum, only) {
        Ok(st) if st.result.suggestions.is_empty() => {
            editor.notice = Some(format!(
                "付けられる拘束がありません(自由度 {}。今の位置で成り立つ物は既に決まっています)",
                st.result.dof_before
            ));
        }
        Ok(st) => {
            editor.set_tool(crate::view::Tool::Select);
            editor.auto = Some(st);
        }
        Err(e) => editor.notice = Some(format!("自動拘束: {e}")),
    }
}

/// 予告の欄。OK を押したら付けたスケッチを返す。
pub(crate) fn panel(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    current: &Sketch,
    values: Option<&DimValues>,
) -> Option<Sketch> {
    if editor.auto.as_ref().is_some_and(|st| st.from != *current) {
        editor.auto = None;
        editor.notice = Some("スケッチが変わったので、自動拘束の予告をやめました".into());
        return None;
    }
    let st = editor.auto.as_mut()?;
    let mut out = None;
    let mut close = false;
    let mut redo: Option<bool> = None;
    ui.strong("自動拘束");
    let all = st.result.suggestions.len();
    ui.label(format!(
        "候補 {all} 個。全部を付けると自由度 {} → {}",
        st.result.dof_before, st.result.dof_after
    ))
    .on_hover_text("今の位置で既に成り立っている拘束と、今の形で測った寸法だけ。決まった順に試し、付けると自由度が減る物だけを残した(等しいは推測しない)");
    ui.label(match st.datum {
        Some(p) => format!("位置の寸法の基準: 選んだ点 {p}"),
        None => "位置の寸法の基準: 原点".into(),
    });
    if !st.only.is_empty() {
        ui.label(format!("対象: 選んだ要素 {} 個", st.only.len()));
    }
    for g in AutoGroup::ALL {
        let n = st
            .result
            .suggestions
            .iter()
            .filter(|s| s.group == g)
            .count();
        if n == 0 {
            continue;
        }
        let mut on = !st.off.contains(&g);
        if ui
            .checkbox(&mut on, format!("{}({n})", g.label()))
            .changed()
        {
            if on {
                st.off.remove(&g);
            } else {
                st.off.insert(g);
            }
        }
    }
    let mut dims = st.dims;
    if ui
        .checkbox(&mut dims, "寸法も付ける")
        .on_hover_text("直径・半径・長さと、基準の点からの横と縦の距離")
        .changed()
    {
        redo = Some(dims);
    }
    ui.horizontal(|ui| {
        let n = st.picks().len();
        if ui
            .add_enabled(n > 0, egui::Button::new(format!("OK({n} 個を付ける)")))
            .clicked()
        {
            out = Some(jc_sketch::apply_suggestions(
                &st.sketch,
                &st.sol,
                &st.picks(),
            ));
            close = true;
        }
        if ui.button("キャンセル").clicked() {
            close = true;
        }
    });
    ui.separator();
    if let (Some(dims), Some(values)) = (redo, values) {
        let (sketch, sol, datum, only, off) = (
            st.from.clone(),
            st.sol.clone(),
            st.datum,
            st.only.clone(),
            st.off.clone(),
        );
        match compute(&sketch, &sol, values, dims, datum, only) {
            Ok(mut next) => {
                next.off = off;
                *st = next;
            }
            Err(e) => editor.notice = Some(format!("自動拘束: {e}")),
        }
    }
    if close {
        editor.auto = None;
    }
    out
}
