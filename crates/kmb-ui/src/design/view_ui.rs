//! 中央のビュー: スケッチの編集器か 3D のビュー。3D のビューの出来事(クリック、範囲、長押し、
//! 右クリック)を選びに直し、「クリックで作る」流れの値を決める。

use crate::select_ui::Hit;
use crate::theme::RightMenu as _;
use crate::view3d::{self, Pick};
use crate::{MenuKind, PickFilter, Workspace, gesture};
use egui::{Pos2, Rect};
use kmb_core::{FeatureId, RefKind};
use kmb_session::edit;

mod edges;
mod making;
mod menus;
mod overlays;
mod pick;

/// 3D のビューの右クリックと長押しのメニューの状態(画面の上の位置を持つ。タブを替えると閉じる)。
pub(crate) struct ViewMenu {
    /// 右クリックの時の修飾キー(メニューの種類を決める)
    pub(crate) kind: MenuKind,
    /// 長押しで出す、重なった面の候補(位置、候補)
    pub(crate) candidates: Option<(egui::Pos2, Vec<view3d::Pick>)>,
    /// 右クリックした所に重なっている面(「ほかを選ぶ」の一覧。手前から)
    pub(crate) context_others: (egui::Pos2, Vec<view3d::Pick>),
}

impl Workspace {
    /// 計算に失敗して前の形を出し続けている間、ビューの上に知らせを出す(M27)。
    fn held_banner(&self, ui: &egui::Ui, rect: egui::Rect) {
        if self.cur.held.is_empty() {
            return;
        }
        let names = kmb_session::edit::names(self.cur.work.doc().data(), &self.cur.held);
        let text = format!("{names}の計算に失敗したので、前に計算できた形を出しています");
        let l = &self.win.look;
        let p = ui.painter_at(rect);
        let g = p.layout_no_wrap(text, crate::theme::prop(12.0), l.pal.ink);
        let size = g.size() + egui::vec2(20.0, 10.0);
        let r = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 14.0 + size.y / 2.0),
            size,
        );
        p.rect(
            r,
            crate::theme::corner(5.0),
            l.pal.panel,
            egui::Stroke::new(1.0, l.pal.warn),
            egui::StrokeKind::Inside,
        );
        p.galley(r.min + egui::vec2(10.0, 5.0), g, l.pal.ink);
    }

    /// スケッチの編集器が中央を使っている時の機能。
    /// 選んだだけでは使わず、直している時(ダブルクリックや「編集」で開いた時)だけ。
    pub(crate) fn view_editor_type(&self) -> Option<String> {
        let id = self.cur.selected.filter(|id| self.is_editing(*id))?;
        let e = self.cur.work.doc().data().feature(id)?;
        self.cur
            .editors
            .iter()
            .any(|ed| ed.handles(&e.type_id) && ed.takes_view())
            .then(|| e.type_id.clone())
    }

    /// 部品の出し入れ(編集パネル・ブラウザ)で 3D のビューの左右の片側だけが動いた時、形を画面の
    /// 同じ所に留める。投影の中心は四角の真ん中なので、そのままだと動いた幅の半分だけ形が跳ぶ。
    /// 引いて幅を変えた時と窓の大きさを変えた時は今のまま(並びに置く部品が変わった時だけ)。
    pub(crate) fn keep_shape_in_place(&mut self, next: egui::Rect) {
        let panes: Vec<_> = crate::dock_ui::Pane::HIDABLE
            .into_iter()
            .filter(|p| self.pane_docked(*p))
            .collect();
        let Some(before) = self.win.view_panes.replace(panes.clone()) else {
            return;
        };
        if before == panes {
            return;
        }
        let old = self.win.view_rect;
        let same = |a: f32, b: f32| (a - b).abs() < 0.5;
        if old.width() > 1.0
            && next.width() > 1.0
            && same(old.top(), next.top())
            && same(old.bottom(), next.bottom())
            && (same(old.left(), next.left()) != same(old.right(), next.right()))
        {
            self.cur.camera.pan.x += old.center().x - next.center().x;
        }
    }

    /// 中央のビューを持つ編集器(スケッチ)を終えた最初のコマに、その編集器で見ていた向き・真ん中・
    /// 大きさを 3D のビューへ移す(入る前の向きへ飛ばない。10-23 u1)。
    fn take_editor_view(&mut self) {
        let Some(t) = self.cur.view_editor_was.take() else {
            return;
        };
        let Some((cam, rect)) = self
            .cur
            .editors
            .iter()
            .find(|ed| ed.handles(&t) && ed.takes_view())
            .and_then(|ed| ed.view_camera())
        else {
            return;
        };
        let (center, yaw, pitch, height) = cam.snapshot(rect);
        log::info!(
            "スケッチで見ていた向きのまま 3D のビューへ戻る(ヨー {yaw:.1}°・ピッチ {pitch:.1}°)"
        );
        self.cur
            .camera
            .restore_now(center, yaw, pitch, cam.tilt_deg(), height, rect);
    }

    pub(crate) fn view_ui(&mut self, ui: &mut egui::Ui) {
        let entry = self
            .cur
            .selected
            .filter(|id| self.is_editing(*id))
            .and_then(|id| self.cur.work.doc().data().feature(id).cloned());
        // 専用の編集器が中央のビューを持つ種類(スケッチ)は、直している時だけそれに描かせる
        if let Some(e) = &entry {
            let changed = self.with_editors(|eds, host| {
                eds.iter_mut()
                    .find(|ed| ed.handles(&e.type_id) && ed.takes_view())
                    .map(|ed| (ed.view(ui, host, e), ed.edit_label()))
            });
            if let Some((data, label)) = changed {
                self.cur.view_editor_was = Some(e.type_id.clone());
                if let Some(data) = data {
                    let mut tx = edit::set_feature_data(self.cur.work.doc().data(), e.id, data);
                    tx.label = format!("{}({})", label, e.name);
                    self.apply(tx);
                }
                return;
            }
        }
        self.take_editor_view();
        // 選ぶ欄が受けている間は、その機能の手前の形を出す
        self.sync_scene_stage();
        // 再生している間は、描く前に時刻を進める
        self.play_tick(ui.ctx());
        self.frames_tick(ui.ctx());
        let mut overlay = self.overlay();
        self.play_filter_lines(&mut overlay.lines);
        // 隠したスケッチ(や機能)を選んでいる時は、その線を重ねない
        if self.cur.selected.is_some_and(|id| self.is_hidden(id)) {
            overlay.lines.clear();
        }
        overlay.lines.extend(self.sketch_lines());
        overlay.pictures.extend(self.pictures(ui.ctx()));
        overlay.lines.extend(self.lamp_lines());
        overlay.lines.extend(self.trail_lines());
        // クリックで作る流れを始めたら、測る道具は閉じる
        if self.cur.gesture.is_some() {
            self.cur.measure = None;
        }
        let measuring = self.is_measuring();
        let picks = if measuring {
            self.measure_faces()
        } else {
            self.highlighted_faces()
        };
        let controls = view3d::Controls {
            middle_orbits: self.win.layout.middle_drag == crate::MiddleDrag::Orbit,
            left: self.win.left_drag,
            // 部品を引いている間は、カーソルの下の面を色付けしない(下の別の部品が動くように見える)
            no_face_hover: if measuring {
                !self.measure_hovers_face()
            } else {
                self.no_face_hover() || self.joint_dragging()
            },
            no_guide_planes: self
                .cur
                .gesture
                .as_ref()
                .and_then(|g| g.current())
                .is_some_and(|h| matches!(h.gesture, kmb_core::Gesture::Choice)),
            // 部品を引いてジョイントを動かしている間は、範囲で選ばない
            no_box: self.joint_dragging(),
            free_orbit: self.win.layout.free_orbit,
            see_through: self.win.layout.see_through,
        };
        let colors = view3d::Colors::from_look(&self.win.look);
        self.sync_ai_glow(ui.ctx());
        let mut ghosts = self.tool_ghosts(&colors);
        ghosts.extend(self.compare_ghosts());
        ghosts.extend(self.table_ghosts());
        self.cur.camera.perspective = self.win.layout.projection.perspective_for(&self.cur.camera);
        self.keep_shape_in_place(ui.available_rect_before_wrap());
        let (ev, resp, rect) = view3d::view3d(
            ui,
            &mut self.cur.camera,
            &self.cur.scene,
            &picks,
            &overlay,
            &ghosts,
            controls,
            &colors,
            self.win.layout.shading,
            view3d::Guides {
                grid: self.win.layout.grid,
                axes: self.win.layout.origin,
            },
        );
        self.win.view_rect = rect;
        self.joint_drag_events(ui, &resp, rect);
        self.canvas_render_ui(ui, rect, &colors);
        self.held_banner(ui, rect);
        self.auto_explode_ui(ui, rect);
        // ブラウザの「原点」の目を開けている間は、原点の平面をいつも出す(空の文書では view3d が出す)
        let drawn_empty =
            self.cur.scene.is_empty() && overlay.lines.is_empty() && self.win.layout.origin;
        // 原点の平面を選ぶ欄のある機能(ミラーの対称の平面・ボディの分割の割る平面)を直している
        // 間は、原点の平面を出し、選んでいる平面を色付けする
        let plane_field = self.origin_plane_field();
        if let Some(marked) = plane_field
            && !drawn_empty
            && !controls.no_guide_planes
        {
            let pick = self.win.look.ws();
            view3d::origin_planes_marked(
                &ui.painter_at(rect),
                &self.cur.camera,
                rect,
                colors.machine,
                marked.map(|k| (k, pick)),
            );
        } else if self.cur.visibility.origin_shown && !drawn_empty && !controls.no_guide_planes {
            view3d::origin_planes(&ui.painter_at(rect), &self.cur.camera, rect, colors.machine);
        }
        let shape = overlay.shape.as_ref().map(|s| s.label.clone());
        if !measuring {
            let hover = ev.hover.and_then(|p| self.hover_hit(rect, p));
            self.cur.hover_kind = hover.as_ref().map(Hit::kind);
            self.cur.hover_feature = hover.as_ref().map(Hit::feature);
            self.draw_edges(ui, rect, hover.as_ref());
            self.draw_vertices(ui, rect, hover.as_ref());
            self.draw_pick_field_hint(ui, rect, ev.hover);
        }
        let taken = self.view_overlays(ui, rect, shape);
        self.isolation_banner(ui, rect);
        self.moved_ui(ui, rect);
        let legend = overlay
            .shape
            .as_ref()
            .map(|s| s.legend.clone())
            .unwrap_or_default();
        if self.win.layout.space == crate::layout::Space::Simulation {
            // 解析の結果: 見出し付きの凡例を重ね(表面と流線など)、解き方の札と時刻の帯を出す
            if let Some(s) = &overlay.shape {
                let title = s.label.split('・').next().unwrap_or_default().to_string();
                let mut groups = vec![(title, s.legend.clone())];
                groups.extend(s.more_legends.iter().cloned());
                // 左上の案内と右上の軸の玉の下に置く(案内が折り返しても、札が長くても隠さない)
                let top = (taken.left + 8.0).max(rect.top() + 80.0);
                crate::result_ui::legends_ui(ui, rect, top, &groups, &self.win.look);
                crate::result_ui::badge_ui(ui, rect, taken.right + 8.0, &s.badge, &self.win.look);
            }
            self.playback_ui(ui, rect, &[]);
            self.frames_ui(ui, rect);
        } else {
            self.playback_ui(ui, rect, &legend);
        }
        marks_legend(ui, rect, &overlay.marks, &self.win.look);
        self.draw_editing_profile(ui, rect);
        if self.cur.gesture.is_none() && !measuring {
            self.edge_value_boxes(ui, rect);
        }
        if self.cur.gesture.is_some() {
            self.gesture_events(&ev, rect);
            self.draw_gesture(ui, rect);
        } else if measuring {
            self.measure_events(&ev, rect);
            self.draw_measure(ui, rect);
            self.measure_panel(ui, rect);
        } else {
            self.pick_events(ui, &ev, rect);
        }
        if !measuring {
            self.review_panel(ui, rect);
        }
        self.section_panel(ui, rect);
        self.problem_badge(ui, rect);
        self.fps_badge(ui, rect);
        self.candidates_popup(ui);
        if ev.context.is_some() {
            let m = ui.input(|i| i.modifiers);
            self.view_menu.kind = if m.shift {
                MenuKind::Create
            } else if m.command {
                MenuKind::Filter
            } else {
                MenuKind::Actions
            };
            self.cur.context_hit = None;
            self.view_menu.context_others.1.clear();
            if self.view_menu.kind == MenuKind::Actions
                && self.cur.gesture.is_none()
                && let Some(p) = ev.context
            {
                self.context_pick(rect, p);
            }
        }
        resp.right_menu(|ui| match self.view_menu.kind {
            MenuKind::Actions => self.view_menu(ui),
            MenuKind::Create => self.create_menu(ui),
            MenuKind::Filter => self.filter_menu(ui),
        });
    }

    /// 画面の速さ(設定の「画面の速さを出す」)。右下に、直近 1 秒の fps と 1 回の処理の時間。
    fn fps_badge(&mut self, ui: &egui::Ui, rect: Rect) {
        if !self.win.layout.show_fps {
            return;
        }
        // 止まっている間も数え直す(描き直さないと前の値のまま残る)
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
        let Some(text) = self.win.perf.text() else {
            return;
        };
        let l = self.win.look;
        let font = crate::theme::mono(11.0);
        let g = ui.fonts_mut(|f| f.layout_no_wrap(text, font, l.pal.ink2));
        // 右下の「注意」の札(problem_badge)の上に置く
        let r = Rect::from_min_size(
            rect.right_bottom() - g.size() - egui::vec2(22.0, 52.0),
            g.size() + egui::vec2(12.0, 6.0),
        );
        let p = ui.painter_at(rect);
        p.rect(
            r,
            4.0,
            l.pal.panel,
            egui::Stroke::new(1.0, l.pal.edge),
            egui::StrokeKind::Inside,
        );
        p.galley(r.min + egui::vec2(6.0, 3.0), g, l.pal.ink2);
    }
}

/// ブラウザを隠した時の左の端のつまみの字(縦に 1 字ずつ)。
const GRIP_TEXT: &str = "ブラウザ";

/// シミュレーションの印(赤の点・紫の点)の凡例。印を出している時だけ、ビューの左下に出す
/// (Z22: 凡例が無いと、「G-code で確かめる」の結果の紫の点が何の印か分からない)。
fn marks_legend(
    ui: &mut egui::Ui,
    rect: Rect,
    marks: &[([f64; 3], kmb_core::MarkKind, f64)],
    l: &crate::theme::Look,
) {
    if marks.is_empty() {
        return;
    }
    let gouge = marks
        .iter()
        .filter(|m| m.1 == kmb_interchange::ink::mark::GOUGE)
        .count();
    let left = marks.len() - gouge;
    let at = Rect::from_min_size(
        egui::pos2(rect.left() + 12.0, rect.bottom() - 96.0),
        egui::vec2(260.0, 52.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
        egui::Frame::new()
            .fill(l.pal.panel)
            .stroke(egui::Stroke::new(1.0, l.pal.edge))
            .corner_radius(crate::theme::corner(4.0))
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let row = |ui: &mut egui::Ui, color: egui::Color32, text: String| {
                    ui.horizontal(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, color);
                        ui.label(
                            egui::RichText::new(text)
                                .size(crate::theme::fs(11.0))
                                .color(l.pal.ink2),
                        );
                    });
                };
                if gouge > 0 {
                    row(
                        ui,
                        l.pal.inks.gouge,
                        format!("赤の点: 食い込み・工具の当たり {gouge} か所"),
                    );
                }
                if left > 0 {
                    row(
                        ui,
                        l.pal.inks.leftover,
                        format!("紫の点: 削り残し {left} か所(大きいほど厚い)"),
                    );
                }
            });
    });
}

/// 押し出す輪郭を塗る半透明の青。
fn profile_fill(l: &crate::theme::Look) -> egui::Color32 {
    view3d::with_alpha(l.pal.inks.profile, view3d::PROFILE_ALPHA)
}
