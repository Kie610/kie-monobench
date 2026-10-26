//! 2D スケッチのビューで、今の道具の操作(クリック・引く・Enter)を形の変更にする。
//! 視点とキーの扱いは `sketch_view`、描くのは `draw.rs`。

use super::*;

mod dimension;
mod modify;
mod select;
mod shapes;

/// 道具の操作に要る、このコマの入力(`sketch_view` が視点とキーを済ませてから渡す)。
#[derive(Clone, Copy)]
pub(super) struct Input<'a> {
    pub(super) sketch: &'a Sketch,
    pub(super) sol: &'a Solution,
    pub(super) base_sketch: &'a Sketch,
    pub(super) base_sol: &'a Solution,
    pub(super) aug: &'a crate::snap::Augmented,
    pub(super) values: Option<&'a DimValues>,
    pub(super) refs: &'a [kmb_sketch::RefItem],
    pub(super) sections: &'a [Section],
    pub(super) bodies: &'a [BodyMesh],
    pub(super) origins: &'a [([f64; 2], kmb_sketch::FaceOrigin)],
    pub(super) response: &'a egui::Response,
    pub(super) t: &'a Transform,
    pub(super) tol: f64,
    pub(super) cursor: Option<[f64; 2]>,
    pub(super) ctrl: bool,
    pub(super) shift: bool,
    pub(super) enter: bool,
    pub(super) enter_empty: bool,
    pub(super) raw_enter: bool,
    pub(super) free_keys: bool,
    pub(super) primary_drag: bool,
    pub(super) long_id: egui::Id,
    pub(super) long_state: u8,
    pub(super) long_fired: bool,
}

impl Input<'_> {
    /// 押した位置(近くに点があればその点)。
    fn pick(&self, p: [f64; 2]) -> Pick {
        Pick {
            point: self
                .sketch
                .nearest_point(self.sol, p[0], p[1], self.tol)
                .map(|h| h.entity),
            pos: p,
        }
    }

    /// 固定した寸法があれば、その値に合わせた位置(吸着はしない)。なければ押した所。
    fn aim_pick(&self, e: &Editor, w: [f64; 2]) -> Pick {
        if e.hud.pinned() {
            Pick {
                point: None,
                pos: aimed(e, w),
            }
        } else {
            self.pick(w)
        }
    }
}

/// 道具の操作で決まった物(描画の段へ渡す)。
pub(super) struct Acted {
    /// 道具が直したスケッチ
    pub(super) result: Option<Sketch>,
    /// 描いている途中の見本の解(引いている点など)
    pub(super) preview: Option<Solution>,
    /// 選ぶ・寸法の道具で、仮の角や辺をクリックしたら文書に写して選ぶ物
    pub(super) keep: Vec<EntityId>,
    /// 投影・交差の道具で、カーソルの所を押すと写す物(太く描いて見せる)
    pub(super) to_project: Vec<kmb_sketch::RefItem>,
    /// 原点を選ぶ道具で、カーソルの所を押すと原点になる角
    pub(super) origin_hover: Option<[f64; 2]>,
}

/// 今の道具でこのコマの入力を扱い、決まった物を返す。
pub(super) fn act(
    ui: &mut egui::Ui,
    state: &mut ViewState,
    editor: &mut Editor,
    input: Input<'_>,
) -> Acted {
    let Input {
        sketch,
        sol,
        base_sketch,
        base_sol,
        aug,
        values,
        refs,
        sections,
        bodies,
        origins,
        response,
        tol,
        cursor,
        shift,
        enter,
        enter_empty,
        raw_enter,
        ..
    } = input;
    let mut out = Acted {
        result: None,
        preview: None,
        keep: Vec::new(),
        to_project: Vec::new(),
        origin_hover: None,
    };
    // 寸法の道具の間に、付いた寸法の値の札を押す: 道具を抜けずに、その場で値を直す
    // (選ぶ道具ではダブルクリック。続けて寸法を入れている途中に前の寸法を直せる)
    if editor.tool == Tool::Dimension
        && response.clicked()
        && editor.placing.is_none()
        && editor.edit_dim.is_none()
        && editor.filter.dims
        && let Some(p) = response.interact_pointer_pos()
        && let Some(id) = state
            .labels
            .iter()
            .find(|(_, r)| r.contains(p))
            .map(|(id, _)| *id)
        && let Some(value) = base_sketch
            .constraints
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.kind.value())
    {
        editor.edit_dim = Some((id, value.to_string()));
        editor.edit_dim_fresh = true;
        editor.dim_error = None;
        editor.selection.clear();
        editor.ghost = None;
        return out;
    }
    match editor.tool {
        Tool::Dimension => dimension::act(ui, editor, input, &mut out),
        Tool::Select => select::act(ui, state, editor, input, &mut out),
        Tool::Line
        | Tool::Circle
        | Tool::Point
        | Tool::Text
        | Tool::Rectangle
        | Tool::CenterRectangle
        | Tool::RoundedRectangle
        | Tool::Arc
        | Tool::TangentArc
        | Tool::Slot
        | Tool::Polygon => shapes::act(ui, editor, input, &mut out),
        Tool::Fillet | Tool::Trim | Tool::Extend | Tool::Break => {
            modify::act(ui, editor, input, &mut out)
        }
        Tool::ThreePointRectangle
        | Tool::TwoPointCircle
        | Tool::ThreePointCircle
        | Tool::TangentCircle
        | Tool::TwoTangentCircle
        | Tool::MidpointLine
        | Tool::EdgePolygon
        | Tool::ThreePointArcSlot
        | Tool::Ellipse
        | Tool::FitSpline
        | Tool::ControlSpline
        | Tool::Conic
        | Tool::Blend
        | Tool::ThreePointArc
        | Tool::OuterPolygon
        | Tool::OverallSlot
        | Tool::CenterSlot
        | Tool::ArcSlot
        | Tool::Chamfer
        | Tool::LinearPattern
        | Tool::CircularPattern
        | Tool::Move
        | Tool::Rotate
        | Tool::Scale => {
            let input = crate::more_tools::Input {
                sketch,
                sol,
                cursor,
                clicked: response.clicked(),
                enter: enter
                    || ((editor.choosing
                        || matches!(editor.tool, Tool::FitSpline | Tool::ControlSpline))
                        && raw_enter),
                tol,
                shift,
            };
            out.result = crate::more_tools::interact(editor, &input);
        }
        Tool::Constraint(name) => {
            if response.clicked()
                && let Some(w) = cursor
            {
                let hit = sketch.hit_test(sol, w[0], w[1], tol).map(|h| h.entity);
                let sel: Vec<EntityId> = editor.selection.iter().copied().collect();
                // 写した辺や角をクリックしたら、文書に写して使う
                if let Some(h) = hit
                    && aug.kind(h).is_some()
                    && crate::constraint_tool::accepts(name, sketch, &sel, h)
                {
                    out.keep.push(h);
                }
                out.result = crate::constraint_tool::pick(name, editor, sketch, sol, values, hit);
            }
        }
        Tool::Intersect => {
            // 断面をクリック: 押した線(ボディ全体なら、そのボディの断面すべて)を写す。
            // 線は輪郭に使えるよう補助線にしない。投影リンクがあれば、ボディの形が変わると
            // 付いて行き、外せば固定した普通の線になる(下の「投影リンクを外して写した」)
            let items = cursor
                .map(|w| intersect_items(sections, editor.intersect_body, w, tol))
                .unwrap_or_default();
            if response.clicked() && cursor.is_some() {
                if items.is_empty() {
                    editor.notice = Some(
                        "断面がありません: スケッチの平面を通る、前にできたボディの断面(色の付いた線)を押してください"
                            .into(),
                    );
                } else {
                    let mut s = sketch.clone();
                    s.adopt_solution(sol);
                    let before = s.projections.len();
                    // 投影リンクを外して写す時は、同じ形の線が既にあれば写さない
                    let fresh =
                        unlinked_fresh(!editor.intersect_link, base_sketch, base_sol, &items);
                    let ids = add_items(&mut s, &fresh, false);
                    if fresh.is_empty() || s.projections.len() == before {
                        editor.notice = Some("この断面はもう写してあります".into());
                    } else {
                        editor.selection = ids.iter().copied().collect();
                        out.keep.extend(ids);
                        editor.notice = None;
                        out.result = Some(s);
                    }
                }
            }
            out.to_project = items;
        }
        Tool::Project => {
            // 角・辺・円の中心はそれを、面の中なら面の縁を写す。シルエットなら押したボディの
            // 外形、「ボディの辺と角すべて」ならそのボディの辺と角をまとめて
            if let Some(w) = cursor {
                let hits: Vec<EntityId> = sketch
                    .hits(sol, w[0], w[1], tol)
                    .into_iter()
                    .map(|h| h.entity)
                    .collect();
                let part = hits.iter().copied().find(|id| aug.kind(*id).is_some());
                let feature = part.into_iter().chain(hits.iter().copied()).find_map(|id| {
                    sketch
                        .projections
                        .iter()
                        .find(|p| p.entity == id)
                        .and_then(|p| p.source.feature())
                });
                let items = project_items(
                    editor.project_pick,
                    refs,
                    bodies,
                    feature.map(|f| (f, part.is_some())),
                    w,
                );
                if !response.clicked() {
                    out.to_project = items;
                } else if !items.is_empty() {
                    let mut s = sketch.clone();
                    s.adopt_solution(sol);
                    let before = s.projections.len();
                    // 投影リンクを外して写す時は、同じ形の線が既にあれば写さない
                    let items = unlinked_fresh(!editor.project_link, base_sketch, base_sol, &items);
                    let ids = add_items(&mut s, &items, true);
                    let fresh = ids.iter().any(|id| aug.kind(*id).is_some());
                    if items.is_empty() || (s.projections.len() == before && !fresh) {
                        editor.notice = Some("もう写してあります".into());
                    } else {
                        out.keep.extend(ids);
                        editor.notice = None;
                        out.result = Some(s);
                    }
                } else if let Some(id) = part.filter(|_| editor.project_pick == ProjectPick::Parts)
                {
                    out.keep.push(id);
                    editor.notice = None;
                } else {
                    editor.notice = Some(match editor.project_pick {
                        ProjectPick::Parts => "写せる物がありません: 前にできたボディの辺・角・円の中心・面か原点を押してください",
                        _ => "写せる物がありません: 前にできたボディ(辺か面)を押してください",
                    }
                    .into());
                }
            }
        }
        Tool::Origin => {
            let near = cursor.and_then(|w| {
                origins
                    .iter()
                    .map(|(at, o)| ((at[0] - w[0]).hypot(at[1] - w[1]), at, o))
                    .filter(|(d, _, _)| *d <= tol * 1.5)
                    .min_by(|a, b| a.0.total_cmp(&b.0))
            });
            out.origin_hover = near.map(|(_, at, _)| *at);
            if response.clicked() {
                match near {
                    Some((_, at, o)) => {
                        let mut s = base_sketch.clone();
                        s.adopt_solution(base_sol);
                        out.result = s.with_origin(o.clone(), *at);
                        state.follow([-at[0], -at[1]]);
                        editor.finish(Tool::Origin);
                    }
                    None => {
                        editor.notice = Some(
                            "原点にできる所がありません: 面の角(色の付いた丸)を押してください"
                                .into(),
                        )
                    }
                }
            }
        }
        Tool::Measure => {
            if response.clicked()
                && let Some(w) = cursor
            {
                match base_sketch.hit_test(base_sol, w[0], w[1], tol) {
                    Some(h) => {
                        if editor.selection.len() >= 2 {
                            editor.selection.clear();
                        }
                        if !editor.selection.remove(&h.entity) {
                            editor.selection.insert(h.entity);
                        }
                    }
                    None => editor.selection.clear(),
                }
            }
            editor.measured = measure_text(base_sketch, base_sol, values, editor);
        }
        Tool::Offset => {
            out.result = crate::offset_tool::interact(
                editor,
                sketch,
                sol,
                cursor,
                response.clicked(),
                enter || enter_empty,
                tol,
            );
            if out.result.is_some() {
                // 「内側は作れませんでした」などの知らせは、道具を終えても残す
                let notice = editor.notice.take();
                editor.finish(Tool::Offset);
                editor.notice = notice;
            }
        }
        Tool::MirrorAxis => {
            let hit = cursor
                .and_then(|w| sketch.hit_test(sol, w[0], w[1], tol))
                .map(|h| h.entity);
            // 選び直し: 何も選ばずに始めた時はクリックで選び、Shift+クリックはいつでも足し外し
            let axis = if response.clicked() && (editor.choosing || shift) {
                if let Some(id) = hit
                    && !editor.selection.remove(&id)
                {
                    editor.selection.insert(id);
                }
                None
            } else if editor.choosing {
                if raw_enter && !editor.selection.is_empty() {
                    editor.choosing = false;
                }
                None
            } else if response.clicked() {
                hit.filter(|id| matches!(sketch.entity(*id), Some(Entity::Line { .. })))
            } else if raw_enter {
                // Enter: 選んでいない中心線を対称の線にする
                sketch
                    .centerlines
                    .iter()
                    .copied()
                    .find(|c| !editor.selection.contains(c))
            } else {
                None
            };
            if let Some(axis) = axis {
                let mut s = sketch.clone();
                s.adopt_solution(sol);
                let ids: Vec<EntityId> = editor.selection.iter().copied().collect();
                let new = s.mirror(&ids, axis);
                if !new.is_empty() {
                    editor.selection = new.into_iter().collect();
                    out.result = Some(s);
                }
                editor.finish(Tool::MirrorAxis);
            }
        }
    }
    out
}
