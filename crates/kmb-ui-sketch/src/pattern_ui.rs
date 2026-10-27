//! 後から直せるパターンのパネル。
//! パターンの元か写しを選ぶと、数・角度・距離などの欄と、写しの抑制・解除のボタンを出す。

use crate::view::Editor;
use kmb_sketch::{
    AngularSpacing, DistanceType, EntityId, PatternDef, PatternExpressions, PatternId, Sketch,
    Solution,
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

/// 寸法と同じ文字の欄。単位は数値だけを打った時に付け、式の参照はそのまま残す。
fn expression(
    ui: &mut egui::Ui,
    label: &str,
    text: &mut String,
    unit: Option<&str>,
) -> (bool, bool) {
    let l = ui.label(label);
    let id = ui.next_auto_id();
    let completion = kmb_ui::complete::begin(ui, id);
    let r = ui
        .add(completion.filter(egui::TextEdit::singleline(text).id(id).desired_width(100.0)))
        .labelled_by(l.id);
    let completed = completion.end(ui, &r, text, &[]);
    let editing = r.has_focus() || r.changed() || completed;
    let commit = r.lost_focus() && ui.input(|i| !i.key_pressed(egui::Key::Escape));
    if commit {
        *text = match unit {
            Some("length") => crate::view::typed_value(text),
            Some(u) => kmb_ui::with_unit_of(text, &format!("0 {u}")),
            None => text.trim().to_string(),
        };
    }
    if let Some(unit) = unit {
        ui.weak(if unit == "length" {
            kmb_core::display_unit().name()
        } else {
            unit
        });
    }
    (editing, commit)
}

/// パターンのパネル。入力中は文書を変えず、評価できた式の見本を出す。
pub(crate) fn panel(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    lookup: &dyn Fn(&str) -> Option<kmb_core::Quantity>,
) -> Option<Sketch> {
    let Some((id, pattern)) =
        selected_pattern(editor, sketch).and_then(|id| Some((id, sketch.pattern(id)?)))
    else {
        editor.pattern_ghost = None;
        editor.pattern_input = None;
        return None;
    };
    let mut def = pattern.def.clone();
    let mut expressions = editor
        .pattern_input
        .as_ref()
        .filter(|(p, _)| *p == id)
        .map(|(_, e)| e.clone())
        .or_else(|| pattern.expressions.clone())
        .unwrap_or_else(|| PatternExpressions::from_def(&def));
    let mut changed = false;
    let mut editing = false;
    let mut remove: Option<bool> = None;
    let title = match &def {
        PatternDef::Circular(_) => "円形状パターン",
        PatternDef::Rectangular(_) => "矩形状パターン",
    };
    ui.strong(format!("{title}({id})"));
    match (&mut def, &mut expressions) {
        (PatternDef::Circular(c), PatternExpressions::Circular { count, angle }) => {
            ui.horizontal(|ui| {
                let (active, commit) = expression(ui, "数", count, None);
                editing |= active;
                changed |= commit;
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
                let (active, commit) = expression(ui, "角度", angle, Some("deg"));
                editing |= active;
                changed |= commit;
            });
        }
        (PatternDef::Rectangular(r), PatternExpressions::Rectangular { directions }) => {
            for (k, (d, e)) in r.directions.iter_mut().zip(directions).enumerate() {
                ui.push_id(k, |ui| {
                    ui.label(format!("方向 {}", k + 1));
                    ui.horizontal(|ui| {
                        let (active, commit) = expression(ui, "数", &mut e.count, None);
                        editing |= active;
                        changed |= commit;
                    });
                    ui.horizontal(|ui| {
                        let (active, commit) =
                            expression(ui, "距離", &mut e.distance, Some("length"));
                        editing |= active;
                        changed |= commit;
                    });
                    ui.horizontal(|ui| {
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
                    });
                    if let Some(a) = &mut e.angle {
                        ui.horizontal(|ui| {
                            let (active, commit) = expression(ui, "向き", a, Some("deg"));
                            editing |= active;
                            changed |= commit;
                        });
                    }
                });
            }
        }
        _ => {
            editor.dim_error = Some("パターンの式と種類が合いません".into());
            return None;
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

    if let Some(keep) = remove {
        if s.remove_pattern(id, keep) {
            editor.selection.retain(|e| s.entity(*e).is_some());
            return Some(s);
        }
        return None;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        editor.pattern_input = None;
        editor.pattern_ghost = None;
        editor.dim_error = None;
        return None;
    }
    let dirty = def != pattern.def || pattern.expressions.as_ref() != Some(&expressions);
    if !editing && !changed {
        editor.pattern_ghost = None;
        return None;
    }
    editor.pattern_input = Some((id, expressions.clone()));
    let p = s.patterns.iter_mut().find(|p| p.id == id)?;
    p.def = def;
    // 見本にも欄の既定の単位を使う。入力中の文字は補完で書き換えない。
    let mut normalized = expressions;
    match &mut normalized {
        PatternExpressions::Circular { angle, .. } => *angle = kmb_ui::with_unit_of(angle, "0 deg"),
        PatternExpressions::Rectangular { directions } => {
            for d in directions {
                d.distance = crate::view::typed_value(&d.distance);
                if let Some(a) = &mut d.angle {
                    *a = kmb_ui::with_unit_of(a, "0 deg");
                }
            }
        }
    }
    p.expressions = Some(normalized);
    let evaluated = kmb_sketch::dimension_values(&s, lookup).and_then(|values| {
        let s = s.evaluated_patterns(&values)?;
        let sol = kmb_sketch::solve(&s, &values).map_err(|e| e.to_string())?;
        Ok((s, sol))
    });
    match evaluated {
        Ok((s, sol)) => {
            editor.dim_error = None;
            if changed && dirty {
                editor.pattern_input = None;
                editor.pattern_ghost = None;
                editor
                    .selection
                    .retain(|e: &EntityId| s.entity(*e).is_some());
                Some(s)
            } else {
                editor.pattern_ghost = dirty.then(|| (s.pattern(id).unwrap().def.clone(), s, sol));
                None
            }
        }
        Err(e) => {
            editor.dim_error = Some(e);
            editor.pattern_ghost = None;
            None
        }
    }
}
