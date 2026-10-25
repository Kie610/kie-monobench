//! 直す道具(角の丸め・トリム・延長と破断): 押した曲線や角を作り直す。

use super::*;

/// 直す道具(角の丸め・トリム・延長と破断)の、このコマの入力を扱う。
pub(super) fn act(ui: &mut egui::Ui, editor: &mut Editor, input: Input<'_>, out: &mut Acted) {
    let Input {
        sketch,
        sol,
        values,
        response,
        t,
        tol,
        cursor,
        ..
    } = input;
    match editor.tool {
        Tool::Fillet => {
            // 角の点を押すか、線分を 2 本押す
            let corner = cursor.filter(|_| response.clicked()).and_then(|w| {
                if let Some(h) = sketch.nearest_point(sol, w[0], w[1], tol) {
                    return Some(h.entity);
                }
                let line = sketch
                    .hits(sol, w[0], w[1], tol)
                    .into_iter()
                    .map(|h| h.entity)
                    .find(|id| matches!(sketch.entity(*id), Some(Entity::Line { .. })))?;
                if !editor.picked.contains(&line) {
                    editor.picked.push(line);
                }
                if editor.picked.len() < 2 {
                    return None;
                }
                let pair = std::mem::take(&mut editor.picked);
                let shared = shared_end(sketch, pair[0], pair[1]);
                if shared.is_none() {
                    editor.notice = Some(
                        "角を丸められません: 端でつながった 2 本の線分を押してください".into(),
                    );
                }
                shared
            });
            if let Some(corner) = corner {
                let value = if !editor.typed.trim().is_empty() {
                    typed_value(&std::mem::take(&mut editor.typed))
                } else if editor.dim_text.trim().is_empty() {
                    default_corner()
                } else {
                    typed_value(&editor.dim_text)
                };
                // 式が数でなければ 2 mm で作り、解く時に寸法の値へ合わせる
                let r = typed_length(&value).unwrap_or(2.0);
                let shown = shown_length(r);
                let mut s = sketch.clone();
                s.adopt_solution(sol);
                if s.fillet_corner(corner, r, &value).is_some() {
                    out.result = Some(s);
                } else {
                    editor.notice = Some(format!(
                        "角を丸められません: 2 本の線分が交わる角を押してください(半径 {shown} が辺より長い時も作れません)"
                    ));
                }
            }
        }
        Tool::Trim => {
            // 押したまま引くと、なぞった区間をまとめて消す
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(p) = ui.input(|i| i.pointer.press_origin())
            {
                editor.trim_path = vec![t.to_world(p)];
            }
            if !editor.trim_path.is_empty()
                && let Some(w) = cursor
                && editor
                    .trim_path
                    .last()
                    .is_some_and(|q| (q[0] - w[0]).hypot(q[1] - w[1]) > tol * 0.25)
            {
                editor.trim_path.push(w);
            }
            if response.drag_stopped() && !editor.trim_path.is_empty() {
                let path = std::mem::take(&mut editor.trim_path);
                out.result = trim_along(sketch, sol, values, &path, tol);
            }
            // 線の端の近くでも、点ではなく曲線を切る(ホバーで予告した区間と同じものを消す)
            if response.clicked()
                && let Some(w) = cursor
                && let Some(h) = sketch
                    .hits(sol, w[0], w[1], tol)
                    .into_iter()
                    .find(|h| !matches!(sketch.entity(h.entity), Some(Entity::Point { .. }) | None))
            {
                let mut s = sketch.clone();
                s.adopt_solution(sol);
                if s.trim(h.entity, w) {
                    out.result = Some(s);
                }
            }
            if let Some(s) = &out.result {
                editor.notice = dropped_dims("トリム", sketch, s);
            }
        }
        Tool::Extend | Tool::Break => {
            // 延長・破断(網羅版 D2): 押した曲線を、ホバーで見せたとおりに延ばす・分ける
            if response.clicked()
                && let Some(w) = cursor
                && let Some(h) = sketch
                    .hits(sol, w[0], w[1], tol)
                    .into_iter()
                    .find(|h| !matches!(sketch.entity(h.entity), Some(Entity::Point { .. }) | None))
            {
                let mut s = sketch.clone();
                s.adopt_solution(sol);
                let done = if editor.tool == Tool::Extend {
                    s.extend(h.entity, w)
                } else {
                    !s.break_at(h.entity, w).is_empty()
                };
                if done {
                    editor.notice = dropped_dims("破断", sketch, &s);
                    out.result = Some(s);
                } else {
                    editor.notice = Some(if editor.tool == Tool::Extend {
                        "延ばせません: 延ばした先に当たる線がありません(スプラインと円錐曲線は延ばせません)".into()
                    } else {
                        "分けられません: ほかの線と交わっている区間を押してください".into()
                    });
                }
            }
        }
        _ => {}
    }
}

/// トリム・分割で外れた寸法の知らせ(下の帯)。外れた物が無ければ `None`。
fn dropped_dims(what: &str, before: &Sketch, after: &Sketch) -> Option<String> {
    let gone = after.dims_dropped_since(before);
    (!gone.is_empty()).then(|| {
        format!(
            "{what}で寸法 {} 個を外しました: {}(消した線の寸法と、残った線に付け替えられない寸法。要るなら付け直してください)",
            gone.len(),
            gone.join(", ")
        )
    })
}
