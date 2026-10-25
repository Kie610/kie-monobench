//! 寸法の値をその場で打つ欄と、重なった物を選ぶ一覧。

use super::*;

/// 値の欄から入力が離れた時の扱い。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Leave {
    /// 離れていない
    Stay,
    /// 決める: Enter・Tab・欄の外を左クリック(マウスだけで決められる。Kie のレビュー K1)
    Commit,
    /// やめる: Esc(ほかの理由で離れた時も)
    Cancel,
    /// 欄を開いたまま入力を戻す: 欄の外を右・中ボタンで押した(ビューを回す・動かす間も打ち続けられる)
    Keep,
}

/// 値の欄 `resp` から入力が離れたか、離れたならどう扱うか。欄の外のクリックで入力が離れるのは
/// ボタンを離したフレーム(egui の既定)。egui は離れた知らせを 2 フレーム続けて出すので、
/// 離したフレーム以外の知らせは「キャンセル」に数える(決めた欄はその時に閉じるか空になるので、
/// 2 回目は何もしない)。
pub(crate) fn leave_field(ui: &egui::Ui, resp: &egui::Response) -> Leave {
    if !resp.lost_focus() {
        return Leave::Stay;
    }
    ui.input(|i| {
        if i.key_pressed(egui::Key::Escape) {
            Leave::Cancel
        } else if i.key_pressed(egui::Key::Enter)
            || i.key_pressed(egui::Key::Tab)
            || i.pointer.button_released(egui::PointerButton::Primary)
        {
            Leave::Commit
        } else if i.pointer.button_released(egui::PointerButton::Secondary)
            || i.pointer.button_released(egui::PointerButton::Middle)
        {
            Leave::Keep
        } else {
            Leave::Cancel
        }
    })
}

/// その場で値を打つ欄の結果。
pub(crate) enum Inline {
    Typing,
    Cancel,
    Failed(String),
    Placed(Box<Sketch>),
}

/// 置いた寸法の値の札の上に欄を出す。Enter か欄の外を左クリックで寸法を付け、Esc でやめる。
/// 数だけを打てば mm(角度は deg)を補う。`p.ready` なら欄を出さずに、打った値ですぐ付ける。
pub(crate) fn place_dimension_inline(
    ui: &egui::Ui,
    values: Option<&DimValues>,
    sketch: &Sketch,
    sol: &Solution,
    p: &mut Placing,
    label: Rect,
) -> Inline {
    if p.ready {
        return place_now(values, sketch, sol, p);
    }
    let fid = egui::Id::new("sketch_dim_place");
    // 打っている名前の候補(K19): このスケッチの寸法の名前と、文書のパラメータ
    let extra = dim_candidates(sketch, None);
    let resp = egui::Area::new(egui::Id::new("sketch_dim_place_area"))
        .order(egui::Order::Foreground)
        .fixed_pos(label.left_top())
        .show(ui.ctx(), |ui| {
            let c = kmb_ui::complete::begin(ui, fid);
            let r = ui.add(c.filter(
                egui::TextEdit::singleline(&mut p.text).desired_width(label.width().max(90.0)),
            ));
            c.end(ui, &r, &mut p.text, &extra);
            r
        })
        .inner;
    // 置いたクリックのフレームで入力を移すと、そのクリックが「欄の外を押した」と見なされて
    // すぐ外れるので、次のフレームで移す
    if p.fresh && ui.input(|i| i.pointer.any_released() || i.pointer.any_down()) {
        ui.ctx().request_repaint();
        return Inline::Typing;
    }
    if std::mem::take(&mut p.fresh) {
        resp.request_focus();
        if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), fid) {
            let all = egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(p.text.chars().count()),
            );
            st.cursor.set_char_range(Some(all));
            st.store(ui.ctx(), fid);
        }
        return Inline::Typing;
    }
    match leave_field(ui, &resp) {
        Leave::Stay => return Inline::Typing,
        Leave::Keep => {
            resp.request_focus();
            return Inline::Typing;
        }
        Leave::Cancel => return Inline::Cancel,
        Leave::Commit => {}
    }
    place_now(values, sketch, sol, p)
}

/// 置いている寸法 `p` を、打った値で付ける。
pub(crate) fn place_now(
    values: Option<&DimValues>,
    sketch: &Sketch,
    sol: &Solution,
    p: &mut Placing,
) -> Inline {
    let Some(values) = values else {
        return Inline::Failed("寸法の値が計算できないので置けません".into());
    };
    let mut k = p.kind.clone();
    if let Some(v) = k.value_mut() {
        *v = kmb_ui::with_unit_of(p.text.trim(), &p.measured);
    }
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    match s.add_dimension(k, values) {
        Ok((id, _)) => {
            // 札は図形に対する相対の位置で覚える(図形を動かしても付いて来る)
            s.place_label(id, p.at, sol);
            if let Some(c) = s.constraints.iter_mut().find(|c| c.id == id) {
                // 右クリックで被駆動を選んだ寸法は、形を決めずに測るだけ
                c.reference |= p.driven;
            }
            Inline::Placed(Box::new(s))
        }
        Err(e) => {
            // 欄を開き直して直せるようにする
            p.fresh = true;
            p.ready = false;
            Inline::Failed(e)
        }
    }
}

/// 寸法の道具で、選んだ要素に付ける寸法。線と 2 点は、カーソル `w` の位置で向きを選ぶ:
/// 2 点の横の範囲の上下に出すと横の距離、縦の範囲の左右に出すと縦の距離、それ以外は
/// 2 点を結ぶ向きの長さ(水平・垂直な線は、いつも線に沿う長さ)。
pub(crate) fn dimension_at(
    sketch: &Sketch,
    sol: &Solution,
    sel: &[EntityId],
    w: [f64; 2],
) -> Option<ConstraintKind> {
    let kind = kmb_sketch::dimension_for(sketch, sol, sel)?;
    if let ConstraintKind::Angle { a, b, value } = &kind {
        return Some(angle_toward(sketch, sol, *a, *b, value, w).unwrap_or(kind));
    }
    let (a, b) = match &kind {
        ConstraintKind::Length { line, .. } => match sketch.entity(*line)? {
            Entity::Line { a, b, .. } => (*a, *b),
            _ => return Some(kind),
        },
        ConstraintKind::Distance { a, b, .. } => (*a, *b),
        _ => return Some(kind),
    };
    let (p, q) = (sol.points.get(&a)?, sol.points.get(&b)?);
    let (dx, dy) = ((q[0] - p[0]).abs(), (q[1] - p[1]).abs());
    let len = dx.hypot(dy).max(1e-9);
    // ほぼ水平・垂直なら、斜めの長さと横・縦の距離は同じ。線に沿う長さにする
    if dx / len < 1e-3 || dy / len < 1e-3 {
        return Some(kind);
    }
    let in_x = (p[0].min(q[0])..=p[0].max(q[0])).contains(&w[0]);
    let in_y = (p[1].min(q[1])..=p[1].max(q[1])).contains(&w[1]);
    let value = String::new();
    Some(match (in_x, in_y) {
        (true, false) => ConstraintKind::HorizontalDistance { a, b, value },
        (false, true) => ConstraintKind::VerticalDistance { a, b, value },
        _ => kind,
    })
}

/// 角度の寸法を、カーソル `w` のある扇の角にする(交点の周りの 4 つの扇の
/// どれにカーソルを出すかで、角かその補角かを選ぶ)。角度の値は 1 本目から 2 本目へ左回りに
/// 測るので、補角の扇なら 2 本の順を入れ替える。
pub(crate) fn angle_toward(
    sketch: &Sketch,
    sol: &Solution,
    a: EntityId,
    b: EntityId,
    value: &str,
    w: [f64; 2],
) -> Option<ConstraintKind> {
    use std::f64::consts::PI;
    let ends = |id: EntityId| match sketch.entity(id)? {
        Entity::Line { a, b, .. } => Some((*sol.points.get(a)?, *sol.points.get(b)?)),
        _ => None,
    };
    let ((p, q), (r, u)) = (ends(a)?, ends(b)?);
    let x = kmb_sketch::line_crossing(p, q, r, u)?;
    let ta = (q[1] - p[1]).atan2(q[0] - p[0]);
    let v = ((u[1] - r[1]).atan2(u[0] - r[0]) - ta).rem_euclid(PI);
    let rel = ((w[1] - x[1]).atan2(w[0] - x[0]) - ta).rem_euclid(PI);
    let (a, b) = if rel <= v { (a, b) } else { (b, a) };
    Some(ConstraintKind::Angle {
        a,
        b,
        value: value.to_string(),
    })
}

/// 点 `w` に重なっている点・線・円・円弧と、`w` を含む閉じた領域(内側から)。
pub(crate) fn others_at(sketch: &Sketch, sol: &Solution, w: [f64; 2], tol: f64) -> Vec<Other> {
    let mut v: Vec<Other> = sketch
        .hits(sol, w[0], w[1], tol)
        .into_iter()
        .map(|h| Other::Entity(h.entity))
        .collect();
    let mut areas: Vec<(f64, String)> = crate::annot::regions(sketch, sol)
        .into_iter()
        .filter(|a| crate::annot::contains(&a.loops, w))
        .map(|a| (crate::annot::area_of(&a.loops), a.key))
        .collect();
    areas.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.extend(areas.into_iter().map(|(_, k)| Other::Region(k)));
    v
}

/// 一覧に出す名前(種類と番号)。
pub(crate) fn other_label(sketch: &Sketch, o: &Other, areas: &[crate::annot::Area]) -> String {
    match o {
        Other::Region(k) => {
            let n = areas.iter().position(|a| a.key == *k).map_or(0, |i| i + 1);
            format!("閉じた領域 {n}")
        }
        Other::Entity(id) => {
            let what = match sketch.entity(*id) {
                Some(Entity::Point { .. }) => "点",
                Some(Entity::Line {
                    construction: true, ..
                }) => "補助線",
                Some(Entity::Line { .. }) => "線分",
                Some(Entity::Circle { .. }) => "円",
                Some(Entity::Arc { .. }) => "円弧",
                Some(Entity::Ellipse { ends: None, .. }) => "楕円",
                Some(Entity::Ellipse { .. }) => "楕円弧",
                Some(Entity::Spline { .. }) => "スプライン",
                Some(Entity::Conic { .. }) => "円錐曲線",
                None => "要素",
            };
            let from = if sketch.is_projected(*id) {
                "(写した形)"
            } else {
                ""
            };
            format!("{what} {id}{from}")
        }
    }
}

/// 長押しで出す、重なったものの一覧。指を置いた候補は図で光らせ、押すとそれだけを選ぶ
/// (Shift・Ctrl なら足す)。外を押すか Esc で閉じる。
pub(crate) fn others_popup(
    ui: &egui::Ui,
    editor: &mut Editor,
    sketch: &Sketch,
    areas: &[crate::annot::Area],
    just_opened: bool,
) {
    let Some((pos, list)) = editor.others.clone() else {
        return;
    };
    let mut chosen = None;
    let mut hover = None;
    let r = egui::Area::new(egui::Id::new("sketch_others"))
        .fixed_pos(pos + egui::vec2(10.0, 10.0))
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.weak("重なっているもの");
                for o in &list {
                    let b = ui.add(egui::Button::selectable(
                        false,
                        other_label(sketch, o, areas),
                    ));
                    if b.hovered() {
                        hover = Some(o.clone());
                    }
                    if b.clicked() {
                        chosen = Some(o.clone());
                    }
                }
            });
        });
    editor.others_hover = hover;
    if let Some(o) = chosen {
        if !ui.input(|i| i.modifiers.shift || i.modifiers.command) {
            editor.selection.clear();
            editor.regions.clear();
        }
        match o {
            Other::Entity(id) => {
                editor.selection.insert(id);
            }
            Other::Region(k) => {
                if !editor.regions.contains(&k) {
                    editor.regions.push(k);
                }
            }
        }
        editor.others = None;
    } else if r.response.clicked_elsewhere() && !just_opened {
        // 長押しを離した時のクリックでは閉じない
        editor.others = None;
    }
    if editor.others.is_none() {
        editor.others_hover = None;
    }
}

/// ビューを描き、操作で変わったスケッチがあれば返す(呼び出し側が文書へ反映する)。
/// 値の札の上で寸法の値を直す欄の ID。
pub(crate) const DIM_INLINE: &str = "sketch_dim_inline";

/// 値の札の上に欄を出し、打った値で寸法を直す。Enter か欄の外を左クリックで決め、Esc でやめる。
/// 数だけを打てば mm(角度は deg)を補う。
pub(crate) fn edit_dimension_inline(
    ui: &egui::Ui,
    state: &ViewState,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
) -> Option<Sketch> {
    let (id, mut text) = editor.edit_dim.clone()?;
    let (Some(r), Some(c)) = (
        state.labels.iter().find(|(i, _)| *i == id).map(|(_, r)| *r),
        sketch.constraints.iter().find(|c| c.id == id),
    ) else {
        editor.edit_dim = None;
        return None;
    };
    let fid = egui::Id::new(DIM_INLINE);
    let name = sketch.dim_name(id);
    // 打っている名前の候補(K19): このスケッチのほかの寸法の名前と、文書のパラメータ
    let extra = dim_candidates(sketch, Some(id));
    let resp = egui::Area::new(egui::Id::new("sketch_dim_inline_area"))
        .order(egui::Order::Foreground)
        .fixed_pos(r.left_top())
        .show(ui.ctx(), |ui| {
            // 下の札や線と重ならないよう、地の色を敷く
            egui::Frame::popup(ui.style())
                .inner_margin(egui::Margin::same(3))
                .show(ui, |ui| {
            ui.horizontal(|ui| {
                // 寸法の名前(ほかの寸法の式から、この名前で参照できる)
                if let Some(n) = name {
                    ui.label(egui::RichText::new(format!("{n} =")).strong())
                        .on_hover_text(format!(
                            "この寸法の名前。ほかの寸法やパラメータの式に {n} と打つと、この値を使えます"
                        ));
                }
                let c = kmb_ui::complete::begin(ui, fid).except(name);
                let resp = ui.add(c.filter(
                    egui::TextEdit::singleline(&mut text).desired_width(r.width().max(90.0)),
                ));
                c.end(ui, &resp, &mut text, &extra);
                resp.on_hover_text(
                    "式も打てます(例: d1 * 2、(PI/4) [rad]、50 / 2)。数だけなら mm(角度は deg)。\
                     単位をはっきり書く時は 5 [cm] のように大かっこで囲みます(直前の 1 つに掛かる)。\
                     別の寸法の札をクリックすると、その名前が入ります",
                )
            })
            .inner
                })
                .inner
        })
        .inner;
    // 出した最初のフレームで入力を移し、全部選ぶ(そのまま打ち直せる)。名前を入れた後は
    // その後ろにカーソルを置く
    if std::mem::take(&mut editor.edit_dim_fresh) {
        resp.request_focus();
        if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), fid) {
            let range = match editor.edit_dim_caret.take() {
                Some(c) => egui::text::CCursorRange::one(egui::text::CCursor::new(c)),
                None => egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(text.chars().count()),
                ),
            };
            st.cursor.set_char_range(Some(range));
            st.store(ui.ctx(), fid);
        }
    }
    let leave = leave_field(ui, &resp);
    // 直している間に別の寸法の札をクリック: その寸法の名前を式に入れて続ける
    if leave == Leave::Commit
        && !ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Tab))
        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
        && let Some(other) = state
            .labels
            .iter()
            .find(|(i, r)| *i != id && r.contains(p))
            .and_then(|(i, _)| sketch.dim_name(*i))
    {
        let range = egui::TextEdit::load_state(ui.ctx(), fid)
            .and_then(|st| st.cursor.char_range())
            .map(|r| {
                let r = r.as_sorted_char_range();
                r.start.0..r.end.0
            })
            .unwrap_or(0..0);
        let (text, caret) = insert_name(&text, range, other);
        editor.edit_dim = Some((id, text));
        editor.edit_dim_fresh = true;
        editor.edit_dim_caret = Some(caret);
        return None;
    }
    if leave == Leave::Keep {
        resp.request_focus();
    } else if leave != Leave::Stay {
        editor.edit_dim = None;
        // Esc で離れた: やめる。Enter と欄の外の左クリックは決める
        if leave == Leave::Cancel {
            return None;
        }
        if c.reference {
            editor.dim_error =
                Some("参照寸法は値を決めません。パネルの「駆動にする」で形を決める寸法にしてから直してください".into());
            return None;
        }
        let unit = if c.kind.is_angle() { "0 deg" } else { "0 mm" };
        let value = kmb_ui::with_unit_of(text.trim(), unit);
        // 直さずに離れた時は変更にしない(取り消しの履歴を増やさない)
        if value.is_empty() || c.kind.value() == Some(value.as_str()) {
            return None;
        }
        let mut s = sketch.clone();
        s.adopt_solution(sol);
        if let Some(v) = s
            .constraints
            .iter_mut()
            .find(|k| k.id == id)
            .and_then(|k| k.kind.value_mut())
        {
            *v = value;
        }
        return Some(s);
    }
    editor.edit_dim = Some((id, text));
    None
}

/// スケッチの寸法の名前(式の欄の候補)。`except` は今直している寸法(自分を指す式は作れない)。
fn dim_candidates(
    sketch: &Sketch,
    except: Option<kmb_sketch::ConstraintId>,
) -> Vec<kmb_ui::complete::Candidate> {
    sketch
        .constraints
        .iter()
        .filter(|c| Some(c.id) != except)
        .filter_map(|c| {
            let name = sketch.dim_name(c.id)?;
            let note = c
                .kind
                .value()
                .map_or_else(|| "寸法".into(), |v| format!("寸法 {v}"));
            Some(kmb_ui::complete::Candidate::new(name, note))
        })
        .collect()
}

/// 式の `range`(字の位置)を名前 `name` に置き換える(選んでいなければカーソルの位置に入れる)。
/// 新しい式と、名前の後ろのカーソルの位置を返す。
pub(crate) fn insert_name(
    text: &str,
    range: std::ops::Range<usize>,
    name: &str,
) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (a, b) = (range.start.min(chars.len()), range.end.min(chars.len()));
    let mut out: String = chars[..a].iter().collect();
    out.push_str(name);
    let caret = out.chars().count();
    out.extend(&chars[b..]);
    (out, caret)
}
