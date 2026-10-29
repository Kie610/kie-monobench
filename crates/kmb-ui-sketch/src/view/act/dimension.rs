//! 寸法の道具: 要素を押して寸法の種類を決め、置く位置を押して付ける。

use super::*;

/// 寸法の道具の、このコマの入力を扱う。
pub(super) fn act(ui: &mut egui::Ui, editor: &mut Editor, input: Input<'_>, out: &mut Acted) {
    let Input {
        sketch,
        sol,
        base_sketch,
        base_sol,
        aug,
        values,
        response,
        tol,
        cursor,
        free_keys,
        ..
    } = input;
    // 要素を選ぶと寸法がカーソルについてくる。何もない所をクリックするとそこに置き、
    // その場に値を打つ欄を出す。線と 2 点は、カーソルを横へ
    // 出すと縦の距離、上下へ出すと横の距離、斜めの線の脇なら線に沿う長さになる
    // カーソルが図の外(パネルの値の欄など)にある間は、選びが同じなら最後の種類を覚えておく
    let sel = crate::constraints_ui::selected(editor, base_sketch);
    if editor.dim_choice_sel != sel {
        editor.dim_choice = Default::default();
        editor.dim_choice_sel = sel.clone();
    }
    let kind = match cursor {
        Some(w) => dimension_at(base_sketch, base_sol, &sel, w)
            .map(|k| crate::dim_choice::apply(k, &editor.dim_choice, base_sketch, &sel)),
        None if editor.ghost_sel == sel => editor.ghost.as_ref().map(|g| g.0.clone()),
        None => None,
    };
    match (&kind, values) {
        (Some(k), Some(v)) if editor.ghost.as_ref().is_none_or(|g| g.0 != *k) => {
            editor.ghost = kmb_sketch::preview_dimension(base_sketch, v, k)
                .ok()
                .map(|p| (k.clone(), p.text));
            editor.ghost_sel = sel;
        }
        (None, _) if editor.placing.is_none() => editor.ghost = None,
        _ => {}
    }
    // 1 文字のキーは道具の切り替えに使うので、パネルの値の欄へは数字から打ち始めた時だけ
    // 入力を移す(網羅版 F8。式は欄を押して打つ)
    if free_keys
        && editor.placing.is_none()
        && editor.ghost.is_some()
        && let Some(t) = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Text(t)
                    if t.starts_with(|c: char| c.is_ascii_digit() || ".-(".contains(c)) =>
                {
                    Some(t.clone())
                }
                _ => None,
            })
        })
    {
        editor.dim_text = t;
        editor.dim_dirty = true;
        let id = egui::Id::new(crate::constraints_ui::DIM_FIELD);
        ui.ctx().memory_mut(|m| m.request_focus(id));
        let mut st = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        let end = egui::text::CCursor::new(editor.dim_text.chars().count());
        st.cursor
            .set_char_range(Some(egui::text::CCursorRange::one(end)));
        st.store(ui.ctx(), id);
    }
    let empty = cursor.is_some_and(|w| sketch.hit_test(sol, w[0], w[1], tol).is_none());
    if response.clicked()
        && (empty || editor.dim_typed_click)
        && editor.placing.is_none()
        && let (Some(w), Some((k, text))) = (cursor, editor.ghost.clone())
    {
        // パネルの「寸法の値」に打ってから図を押した: 押した所に、打った値ですぐ付ける
        // (Enter と同じ。欄は出さない)
        let typed = std::mem::take(&mut editor.dim_typed_click);
        editor.placing = Some(Placing {
            kind: k,
            at: w,
            text: if typed {
                editor.dim_text.clone()
            } else {
                text.clone()
            },
            measured: text,
            fresh: true,
            driven: editor.dim_choice.driven,
            ready: typed,
        });
    } else if response.clicked()
        && let Some(p) = editor.placing.as_mut()
    {
        // 値の欄を出している間に図をクリック: Enter と同じく、打った値で付ける
        // (欄は図より後に描くので、欄の側で付ける)
        p.ready = true;
    } else if response.clicked()
        && let Some(w) = cursor
    {
        editor.placing = None;
        editor.dim_typed_click = false;
        match sketch.hit_test(sol, w[0], w[1], tol) {
            Some(h) => {
                if editor.selection.len() >= 2 {
                    editor.clear_selection();
                }
                if aug.kind(h.entity).is_some() {
                    out.keep.push(h.entity);
                    editor.selection.insert(h.entity);
                } else if !editor.selection.remove(&h.entity) {
                    editor.selection.insert(h.entity);
                }
            }
            None => editor.clear_selection(),
        }
    }
    // パネルの欄から離れたのが図のクリックでなかった(リボンなど、パネルと図の外を押した):
    // 打った値で次の図のクリックに付ける約束は捨てる(捨てないと、後で図を押した時に
    // 前に打った値で寸法が付く)
    if !response.clicked() {
        editor.dim_typed_click = false;
    }
}
