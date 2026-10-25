//! まとまりの拘束のパネル: オフセットの道具の向きと角の作り方、多角形の道具の辺の数、
//! 作った多角形(ポリゴンの拘束)の辺の数の直し(レビューの K8・K11)。

use crate::view::{Editor, Tool};
use kmb_sketch::{ConstraintId, ConstraintKind, Sketch, Solution};

/// オフセットの道具の欄(右クリックと同じ選び)。
pub(crate) fn offset_tool(ui: &mut egui::Ui, editor: &mut Editor) {
    if editor.tool != Tool::Offset {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("向き");
        for m in [
            crate::offset_tool::Mode::One,
            crate::offset_tool::Mode::Both,
            crate::offset_tool::Mode::Symmetric,
        ] {
            ui.radio_value(&mut editor.offset.mode, m, m.label());
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (r, label) in crate::offset_tool::CORNERS {
            ui.radio_value(&mut editor.offset.round, r, label);
        }
    });
    // オフセットの「チェーン選択」
    let mut chain = !editor.offset.single;
    if ui
        .checkbox(&mut chain, "チェーン選択")
        .on_hover_text("オンは押した線と端でつながった全体をずらす。オフは押した線だけをずらす")
        .changed()
    {
        editor.offset.single = !chain;
        editor.offset.rechain = true;
    }
    ui.separator();
}

/// 多角形の道具なら、辺の数の欄(描く前に決める。描いている途中の欄の値とそろえる)。
pub(crate) fn polygon_tool(ui: &mut egui::Ui, editor: &mut Editor) {
    if !matches!(
        editor.tool,
        Tool::Polygon | Tool::OuterPolygon | Tool::EdgePolygon
    ) {
        return;
    }
    let mut n = crate::view::polygon_sides(editor);
    let changed = ui
        .horizontal(|ui| {
            let l = ui.label("辺の数");
            ui.add(egui::DragValue::new(&mut n).range(3..=64))
                .labelled_by(l.id)
                .changed()
        })
        .inner;
    if changed {
        editor.polygon_n = n;
        editor.polygon_typed = false;
        if let Some(f) = editor.hud.fields.iter_mut().find(|f| f.label == "辺の数") {
            f.text = n.to_string();
            f.locked = true;
        }
    }
    ui.separator();
}

/// 選んだ多角形(辺か、ポリゴンの拘束の印)のポリゴンの拘束。
pub(crate) fn selected_polygon(editor: &Editor, sketch: &Sketch) -> Option<ConstraintId> {
    let picked = editor.picked_constraint.filter(|id| {
        sketch
            .constraints
            .iter()
            .any(|c| c.id == *id && matches!(c.kind, ConstraintKind::Polygon { .. }))
    });
    picked.or_else(|| {
        editor
            .selection
            .iter()
            .find_map(|id| sketch.polygon_of(*id))
    })
}

/// 作った多角形の欄(辺の数を直す)。直したスケッチを返す。
pub(crate) fn polygon_panel(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
) -> Option<Sketch> {
    let id = selected_polygon(editor, sketch)?;
    let Some(ConstraintKind::Polygon { sides, .. }) = sketch
        .constraints
        .iter()
        .find(|c| c.id == id)
        .map(|c| &c.kind)
    else {
        return None;
    };
    let mut n = sides.len();
    ui.strong(format!("ポリゴン({id})"));
    let r = ui
        .horizontal(|ui| {
            let l = ui.label("辺の数");
            ui.add(egui::DragValue::new(&mut n).range(3..=64))
                .labelled_by(l.id)
        })
        .inner;
    ui.label(
        egui::RichText::new("最初の辺と、それに付けた寸法・拘束は残し、ほかの辺を作り直します")
            .size(kmb_ui::theme::fs(11.0))
            .weak(),
    );
    ui.separator();
    // 引いている途中は反映しない(取り消しの段を 1 つにする)
    let done = r.drag_stopped() || (r.changed() && !r.dragged());
    if !done || n == sides.len() {
        return None;
    }
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    if !s.set_polygon_sides(id, n) {
        return None;
    }
    editor.selection.retain(|e| s.entity(*e).is_some());
    Some(s)
}
