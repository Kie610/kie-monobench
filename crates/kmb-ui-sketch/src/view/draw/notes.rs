//! 2D スケッチのビューの、拘束の印と寸法(線・矢印・値の札)、自動拘束の予告、寸法の道具が
//! 置こうとしている寸法、札の上で値を打つ欄、重なりの一覧。

use super::*;

/// 拘束の印と寸法を描き、札の上の欄を出す。欄で値を決めたら、直したスケッチを返す。
#[allow(clippy::too_many_arguments)]
pub(super) fn notes(
    ui: &mut egui::Ui,
    state: &mut ViewState,
    editor: &mut Editor,
    painter: &egui::Painter,
    l: &kmb_ui::theme::Look,
    t: &Transform,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
    cursor: Option<[f64; 2]>,
    rect: Rect,
    areas: &[crate::annot::Area],
    mark_hover: Option<kmb_sketch::ConstraintId>,
    pick_color: Color32,
    long_id: egui::Id,
    long_fired: bool,
) -> Option<Sketch> {
    let mut result = None;
    // 拘束の印(水平・平行・一致など)。自動で付いたものも含めて出す
    state.marks = if editor.show_marks {
        crate::marks::draw(
            painter,
            t,
            sketch,
            sol,
            mark_hover,
            editor.picked_constraint,
            l.pal.ink2,
            l.pal.panel,
            pick_color,
            // 寸法の札(前のコマの位置)に重ねない
            &state.labels.iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        )
    } else {
        Vec::new()
    };

    // 寸法: 形から離した寸法線と補助の線、矢印、値の札(試作の「幅 = 100」)。
    // 寸法線は形の真ん中から離れる側に出す
    let away = {
        let pts: Vec<Pos2> = sol.points.values().map(|p| t.to_screen(*p)).collect();
        if pts.is_empty() {
            rect.center()
        } else {
            let sum = pts.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2());
            (sum / pts.len() as f32).to_pos2()
        }
    };
    let taken = std::cell::RefCell::new(state.marks.iter().map(|m| m.rect).collect());
    let mut labels = Vec::new();
    let fmt = editor.fmt;
    for c in sketch.constraints.iter().filter(|_| editor.display.dims) {
        let Some(value) = c.kind.value() else {
            continue;
        };
        // 参照寸法は括弧つきで、今の形で測った値を出す(形を決めない)
        // 式がパラメータなら「式 = 値」(試作の「幅 = 100」)
        // 値は設定の桁数と単位で出す(形の計算は丸めない)
        let shown = sol
            .measured
            .get(&c.id)
            .or_else(|| values.and_then(|v| v.get(&c.id)))
            .map(|m| fmt.value(&c.kind, *m));
        let plain = typed_length(value).is_some() || value.trim().ends_with("deg");
        let body = match shown {
            Some(m) if c.reference || plain => m,
            Some(m) => format!("{value} = {m}"),
            None => value.to_string(),
        };
        let body = match c.kind {
            ConstraintKind::Radius { .. } => format!("R{body}"),
            ConstraintKind::Diameter { .. } | ConstraintKind::LinearDiameter { .. } => {
                format!("Ø{body}")
            }
            _ => body,
        };
        let (text, look) = if c.reference {
            (
                format!("({body})"),
                crate::annot::DimLook {
                    line: l.pal.ink3,
                    text: l.pal.ink3,
                    dashed: true,
                    away,
                    toward: None,
                    taken: &taken,
                },
            )
        } else {
            (
                body,
                crate::annot::DimLook {
                    line: l.pal.ink2,
                    text: l.pal.ink,
                    dashed: false,
                    away,
                    toward: None,
                    taken: &taken,
                },
            )
        };
        // 掴んで動かしている札はカーソルの側へ。選んだ寸法は選んだ色で描く
        let dragged = (editor.dim_drag == Some(c.id))
            .then(|| ui.input(|i| i.pointer.latest_pos()))
            .flatten()
            .map(|p| p + editor.dim_grab);
        let picked = editor.picked_constraint == Some(c.id);
        let look = crate::annot::DimLook {
            toward: dragged.or_else(|| sketch.label_at(c, sol).map(|p| t.to_screen(p))),
            line: if picked { pick_color } else { look.line },
            text: if picked { pick_color } else { look.text },
            ..look
        };
        if let Some(r) = crate::annot::dimension(painter, t, sketch, sol, &c.kind, text, &look) {
            labels.push((c.id, r));
        }
    }
    // 自動拘束の予告(K18): 付ける候補を拘束の道具の色で描く(写した形の紫・選んだ橙と見分ける)
    if let Some(st) = &editor.auto {
        let ink = l.pal.inks.sketch_constraint;
        let picks = st.picks();
        let mut preview = st.sketch.clone();
        preview.constraints.clear();
        for p in &picks {
            preview.add_constraint(p.kind.clone());
        }
        let avoid: Vec<Rect> = state
            .marks
            .iter()
            .map(|m| m.rect)
            .chain(labels.iter().map(|(_, r)| *r))
            .collect();
        crate::marks::draw(
            painter,
            t,
            &preview,
            &st.sol,
            None,
            None,
            ink,
            l.pal.panel,
            ink,
            &avoid,
        );
        for p in picks.iter().filter(|p| p.kind.value().is_some()) {
            let body = p.kind.value().unwrap_or_default().to_string();
            let body = match p.kind {
                ConstraintKind::Radius { .. } => format!("R{body}"),
                ConstraintKind::Diameter { .. } => format!("Ø{body}"),
                _ => body,
            };
            let look = crate::annot::DimLook {
                line: ink,
                text: ink,
                dashed: false,
                away,
                toward: None,
                taken: &taken,
            };
            crate::annot::dimension(painter, t, &st.sketch, &st.sol, &p.kind, body, &look);
        }
    }
    // 寸法を選べない時は値の札を押せなくする(ダブルクリックで直さない)
    state.labels = if editor.filter.dims {
        labels
    } else {
        Vec::new()
    };
    // 寸法の道具: 置こうとしている寸法を、カーソル(置いた後はその位置)の側に描く
    if editor.tool == Tool::Dimension {
        let at = editor
            .placing
            .as_ref()
            .map(|p| (p.kind.clone(), p.at, p.measured.clone()))
            .or_else(|| {
                editor
                    .ghost
                    .clone()
                    .zip(cursor)
                    .map(|((k, text), w)| (k, w, text))
            });
        if let Some((kind, w, text)) = at {
            let look = crate::annot::DimLook {
                line: pick_color,
                text: pick_color,
                dashed: false,
                away,
                toward: Some(t.to_screen(w)),
                taken: &std::cell::RefCell::new(Vec::new()),
            };
            let shown = if editor.placing.is_some() {
                String::from("     ")
            } else {
                input_shown(&fmt, &kind, &text)
            };
            let r = crate::annot::dimension(painter, t, sketch, sol, &kind, shown, &look);
            if let (Some(p), Some(r)) = (editor.placing.as_mut(), r) {
                match place_dimension_inline(ui, values, sketch, sol, p, r) {
                    Inline::Typing => {}
                    Inline::Cancel => editor.placing = None,
                    Inline::Failed(e) => editor.dim_error = Some(e),
                    Inline::Placed(s) => {
                        result = Some(*s);
                        editor.placing = None;
                        editor.ghost = None;
                        // 測った値で埋めたパネルの「寸法の値」も空に戻す(残ると次の道具が使う)
                        if editor.dim_kind.take().is_some() {
                            editor.dim_text.clear();
                        }
                        editor.dim_error = None;
                        editor.clear_selection();
                    }
                }
            }
        }
    }
    if let Some(s) = edit_dimension_inline(ui, state, editor, sketch, sol) {
        result = Some(s);
    }
    let alt_opened = ui
        .ctx()
        .data_mut(|m| m.remove_temp::<bool>(long_id.with("alt")))
        .unwrap_or(false);
    others_popup(ui, editor, sketch, areas, long_fired || alt_opened);
    result
}
