//! 後から直せるパターンのパネル。
//! パターンの元か写しを選ぶと、数・角度・距離などの欄と、写しの抑制・解除のボタンを出す。

use crate::view::{DimValues, Editor};
use kmb_sketch::{
    AngularSpacing, DistanceType, EntityId, PatternAxis, PatternDef, PatternId, Sketch, Solution,
};

/// 選んだ要素が属するパターン(最初に見つかったもの)。
pub(crate) fn selected_pattern(editor: &Editor, sketch: &Sketch) -> Option<PatternId> {
    // 並びの拘束の印を選んだ時も
    let picked = editor.picked_constraint.and_then(|id| {
        sketch.constraints.iter().find_map(|c| match c.kind {
            kmb_sketch::ConstraintKind::Pattern { pattern } if c.id == id => Some(pattern),
            _ => None,
        })
    });
    picked.or_else(|| {
        editor
            .selection
            .iter()
            .find_map(|id| sketch.pattern_of(*id).map(|(p, _)| p))
    })
}

/// 欄を直し終えた(引いている途中は反映しない。取り消しの段を 1 つにする)。
fn done(r: &egui::Response) -> bool {
    r.drag_stopped() || (r.changed() && !r.dragged())
}

/// パターンのパネル。直したスケッチを返す。欄を引いている間は文書を変えず、引いている途中の
/// 値で作り直した見本を `editor.pattern_ghost` に置く(ビューが文書の代わりに描く)。
pub(crate) fn panel(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
) -> Option<Sketch> {
    let Some((id, pattern)) =
        selected_pattern(editor, sketch).and_then(|id| Some((id, sketch.pattern(id)?)))
    else {
        editor.pattern_ghost = None;
        return None;
    };
    // 引いている途中は、前のコマまでに引いた値から続ける(欄はマウスの動いたコマでしか
    // 引いた値を返さないので、文書の値から始めると止まったコマで元へ戻る)
    let mut def = match &editor.pattern_ghost {
        Some((d, ..)) => d.clone(),
        None => pattern.def.clone(),
    };
    let mut changed = false;
    // 欄を引いている途中
    let mut dragging = false;
    let mut remove: Option<bool> = None;
    let title = match &def {
        PatternDef::Circular(_) => "円形状パターン",
        PatternDef::Rectangular(_) => "矩形状パターン",
    };
    ui.strong(format!("{title}({id})"));
    match &mut def {
        PatternDef::Circular(c) => {
            ui.horizontal(|ui| {
                let l = ui.label("数");
                let r = ui.add(egui::DragValue::new(&mut c.count).range(1..=360));
                dragging |= r.dragged();
                changed |= done(&r.labelled_by(l.id));
                egui::ComboBox::from_id_salt("pattern_spacing")
                    .selected_text(match c.spacing {
                        AngularSpacing::Total => "全体",
                        AngularSpacing::Step => "角度ごと",
                        AngularSpacing::Symmetric => "対称",
                    })
                    .show_ui(ui, |ui| {
                        for (v, l) in [
                            (AngularSpacing::Total, "全体"),
                            (AngularSpacing::Step, "角度ごと"),
                            (AngularSpacing::Symmetric, "対称"),
                        ] {
                            changed |= ui.selectable_value(&mut c.spacing, v, l).changed();
                        }
                    });
            });
            ui.horizontal(|ui| {
                let l = ui.label("角度");
                let mut deg = c.angle.to_degrees();
                let r = ui
                    .add(
                        egui::DragValue::new(&mut deg)
                            .range(-360.0..=360.0)
                            .speed(1.0)
                            .suffix(" deg"),
                    )
                    .labelled_by(l.id);
                dragging |= r.dragged();
                if (done(&r) || r.dragged()) && deg.abs() > 1e-9 {
                    c.angle = deg.to_radians();
                    changed |= done(&r);
                }
            });
        }
        PatternDef::Rectangular(r) => {
            for (k, d) in r.directions.iter_mut().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("方向 {}", k + 1));
                    let l = ui.label("数");
                    let r = ui.add(egui::DragValue::new(&mut d.count).range(1..=200));
                    dragging |= r.dragged();
                    changed |= done(&r.labelled_by(l.id));
                    let l = ui.label("距離");
                    let resp = ui
                        .add(
                            egui::DragValue::new(&mut d.distance)
                                .speed(0.5)
                                .suffix(" mm"),
                        )
                        .labelled_by(l.id);
                    dragging |= resp.dragged();
                    changed |= done(&resp);
                    egui::ComboBox::from_id_salt(("pattern_dist", k))
                        .selected_text(match d.distance_type {
                            DistanceType::Spacing => "間隔",
                            DistanceType::Extent => "範囲",
                        })
                        .show_ui(ui, |ui| {
                            for (v, l) in [
                                (DistanceType::Spacing, "間隔"),
                                (DistanceType::Extent, "範囲"),
                            ] {
                                changed |=
                                    ui.selectable_value(&mut d.distance_type, v, l).changed();
                            }
                        });
                    changed |= ui.checkbox(&mut d.symmetric, "対称").changed();
                    if let PatternAxis::Angle(a) = &mut d.axis {
                        let mut deg = a.to_degrees();
                        ui.label("向き");
                        let resp = ui.add(egui::DragValue::new(&mut deg).speed(1.0).suffix(" deg"));
                        dragging |= resp.dragged();
                        if done(&resp) || resp.dragged() {
                            *a = deg.to_radians();
                            changed |= done(&resp);
                        }
                    }
                });
            }
        }
    }
    // 選んだ写しを抑制する(作らない)
    let copies: Vec<[i32; 2]> = editor
        .selection
        .iter()
        .filter_map(|e| match sketch.pattern_of(*e) {
            Some((p, Some(ix))) if p == id => Some(ix),
            _ => None,
        })
        .collect();
    ui.horizontal_wrapped(|ui| {
        if !copies.is_empty() && ui.button("選んだ写しを抑制").clicked() {
            match &mut def {
                PatternDef::Circular(c) => {
                    c.suppressed.extend(copies.iter().map(|ix| ix[0]));
                }
                PatternDef::Rectangular(r) => r.suppressed.extend(copies.iter().copied()),
            }
            changed = true;
        }
        let any = match &def {
            PatternDef::Circular(c) => !c.suppressed.is_empty(),
            PatternDef::Rectangular(r) => !r.suppressed.is_empty(),
        };
        if any && ui.button("抑制を全部戻す").clicked() {
            match &mut def {
                PatternDef::Circular(c) => c.suppressed.clear(),
                PatternDef::Rectangular(r) => r.suppressed.clear(),
            }
            changed = true;
        }
        if ui
            .button("パターンを解除")
            .on_hover_text("写しは残し、パターンの定義だけを消す(写しは普通の要素になる)")
            .clicked()
        {
            remove = Some(true);
        }
        if ui
            .button("パターンを消す")
            .on_hover_text("写しも消す(元は残す)")
            .clicked()
        {
            remove = Some(false);
        }
    });
    ui.separator();
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    ghost(editor, &s, id, &def, dragging && def != pattern.def, values);
    if let Some(keep) = remove {
        if s.remove_pattern(id, keep) {
            editor.selection.retain(|e| s.entity(*e).is_some());
            return Some(s);
        }
        return None;
    }
    if !changed {
        return None;
    }
    if s.edit_pattern(id, def) {
        editor
            .selection
            .retain(|e: &EntityId| s.entity(*e).is_some());
        editor.dim_error = None;
        Some(s)
    } else {
        editor.dim_error =
            Some("この値ではパターンを作れません(数が 0、角度や距離が 0 など)".into());
        None
    }
}

/// 欄を引いている間の見本を作り直す(値が変わった時だけ解く)。引いていないか、値が元のままなら消す。
fn ghost(
    editor: &mut Editor,
    s: &Sketch,
    id: PatternId,
    def: &PatternDef,
    dragging: bool,
    values: Option<&DimValues>,
) {
    if !dragging {
        editor.pattern_ghost = None;
        return;
    }
    if editor
        .pattern_ghost
        .as_ref()
        .is_some_and(|(d, ..)| d == def)
    {
        return;
    }
    let mut p = s.clone();
    let empty = DimValues::default();
    editor.pattern_ghost = p
        .edit_pattern(id, def.clone())
        .then(|| kmb_sketch::solve(&p, values.unwrap_or(&empty)).ok())
        .flatten()
        .map(|sol| (def.clone(), p, sol));
}
