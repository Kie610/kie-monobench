//! 右の編集パネル(閉じなくてよい)。選んだ機能の値を見せ、直すのは開いた時だけ:
//! - 選んだだけ: 値を見せるだけ(灰色)。「編集」・ダブルクリック・Enter で直し始める。
//! - 直している: 直すとその場で形に出て、「OK」でそのまま決め、「キャンセル」で直し始めた時へ戻す。
//! - 作っている途中(ツールバーで足した直後): 「OK」で決め、「キャンセル」で作る前へ戻す。
//!
//! 加工の操作を Ctrl で複数選び「まとめて編集」を押すと、共通の欄をまとめて直せる(値が違う欄は「—」)。

use crate::{Workspace, icons, theme};
use kmb_core::{FeatureEntry, Field, FieldKind};
use kmb_session::edit;
use serde_json::Value as Json;

/// 値が違う欄に出す印。
pub(crate) const MIXED: &str = "—";

/// スケッチを直している間の決めるボタン(パネルの下と帯の右)。
pub(crate) const SKETCH_FINISH: &str = "✓  スケッチを終了";

impl Workspace {
    /// 右の編集パネルの枠(試作の `.rpanel`): 見出しの帯、中身、下に OK とキャンセル。
    pub(crate) fn panel_frame(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let (title, icon) = self.panel_title();
        let w = ui.available_width();
        // 見出しの高さ・アイコン・閉じるは字とアイコンの倍率に合わせる(1 倍で 32・14・20)。
        // (高さを固定にすると、250 % で倍率を掛けたアイコンと題が重なる)
        let icon_w = crate::theme::icon_px(14.0);
        let title_font = crate::theme::prop(12.5);
        let title_h = ui.fonts_mut(|f| f.row_height(&title_font));
        let head_h = 32.0_f32.max(title_h + 12.0).max(icon_w + 18.0);
        let close_w = 20.0 * theme::band();
        let (head, _) = ui.allocate_exact_size(egui::vec2(w, head_h), egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(head, crate::theme::corner(0.0), l.ws_soft());
        p.hline(
            head.x_range(),
            head.bottom() - 0.5,
            egui::Stroke::new(1.0, l.pal.edge),
        );
        icons::paint_exact(
            p,
            egui::Rect::from_center_size(
                egui::pos2(head.left() + 12.0 + icon_w / 2.0, head.center().y),
                egui::vec2(icon_w, icon_w),
            ),
            &icon,
            l.ws(),
            1.6,
        );
        let g = p.layout_no_wrap(title.clone(), title_font, l.ws());
        let tp = egui::pos2(
            head.left() + 12.0 + icon_w + 8.0,
            head.center().y - g.size().y / 2.0,
        );
        let title_right = tp.x + g.size().x;
        p.galley(tp, g.clone(), l.ws());
        p.galley(tp + egui::vec2(0.5, 0.0), g, l.ws());
        // スケッチを直している間は、見出しの右に「スケッチ中」の札を出す(設計の機能の編集と
        // 見分ける。見出しの帯の色もスケッチの水色になる。レビュー K15)
        if l.sketch {
            let g = p.layout_no_wrap(
                "スケッチ中".to_string(),
                crate::theme::prop(10.5),
                l.pal.chrome,
            );
            // 札の高さは字に合わせる(1 倍で 18)
            let bh = 18.0_f32.max(g.size().y + 4.0);
            let r = egui::Rect::from_min_size(
                egui::pos2(
                    (head.right() - 8.0 - close_w - 12.0 - g.size().x - 14.0)
                        .max(title_right + 8.0),
                    head.center().y - bh / 2.0,
                ),
                egui::vec2(g.size().x + 14.0, bh),
            );
            p.rect_filled(r, crate::theme::corner(bh / 2.0), l.ws());
            p.galley(r.center() - g.size() / 2.0, g, l.pal.chrome);
        }
        let x = egui::Rect::from_center_size(
            egui::pos2(head.right() - 8.0 - close_w / 2.0, head.center().y),
            egui::vec2(close_w, close_w),
        );
        let xr = ui
            .interact(x, ui.id().with("panel_close"), egui::Sense::click())
            .on_hover_text(self.panel_close_hint());
        xr.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "編集パネルを隠す")
        });
        if xr.hovered() {
            ui.painter().rect_filled(
                x,
                crate::theme::corner(3.0),
                theme::mix(l.ws_soft(), l.ws(), 0.15),
            );
        }
        icons::paint_exact(
            ui.painter(),
            x.shrink(4.0 * theme::band()),
            "x",
            l.pal.ink2,
            1.6,
        );
        if xr.clicked() {
            self.hide_pane(crate::dock_ui::Pane::Panel);
        }
        let rest = ui.available_rect_before_wrap();
        let footer = self.panel_footer_kind();
        // スケッチを直している間は「スケッチを終了」が長いので、計算の札を上の段に分ける。
        // 字が大きくてボタンと札が 1 段に入らない時も分ける
        let two_rows = matches!(footer, Some(Footer::Edit | Footer::Create | Footer::View))
            && (self.view_editor_type().is_some()
                || footer.is_some_and(|k| self.footer_width(ui, k) > rest.width() - 24.0));
        // 段の高さはボタンの高さ(字の倍率)に合わせる(1 倍なら 1 段 46、2 段 72)
        let row_h = crate::theme::fs(28.0) + 18.0;
        let foot_h = match (footer.is_some(), two_rows) {
            (false, _) => 0.0,
            (true, false) => row_h,
            (true, true) => row_h + crate::theme::fs(26.0),
        };
        let body = egui::Rect::from_min_max(rest.min, egui::pos2(rest.max.x, rest.max.y - foot_h));
        let mut b = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(&mut b, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 7.0);
                        self.panel_ui(ui)
                    });
            });
        if let Some(kind) = footer {
            let fr =
                egui::Rect::from_min_max(egui::pos2(rest.min.x, rest.max.y - foot_h), rest.max);
            ui.painter().hline(
                fr.x_range(),
                fr.top() + 0.5,
                egui::Stroke::new(1.0, l.pal.edge),
            );
            let inner = fr.shrink2(egui::vec2(12.0, 0.0));
            let (top_rect, button_rect) = if two_rows {
                let mid = inner.top() + crate::theme::fs(28.0);
                (
                    Some(egui::Rect::from_min_max(
                        inner.min + egui::vec2(0.0, 4.0),
                        egui::pos2(inner.right(), mid),
                    )),
                    egui::Rect::from_min_max(egui::pos2(inner.left(), mid), inner.max),
                )
            } else {
                (None, inner)
            };
            let mut top = top_rect.map(|r| {
                let mut t = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(r)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                t.spacing_mut().item_spacing.x = 8.0;
                t
            });
            let mut f = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(button_rect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            f.spacing_mut().item_spacing.x = 8.0;
            self.panel_footer(&mut f, kind, top.as_mut());
        }
        ui.allocate_rect(rest, egui::Sense::hover());
    }

    /// 見出しの文字とアイコン。
    fn panel_title(&self) -> (String, String) {
        if let Some(g) = &self.cur.gesture {
            return (g.label.clone(), self.action_icon(&g.action));
        }
        if self.win.show_params {
            return ("パラメータ".into(), "param".into());
        }
        let sel = self.selection();
        if sel.len() >= 2 {
            let t = if self.cur.multi_edit {
                format!("{} 個をまとめて編集", sel.len())
            } else {
                format!("{} 個を選択中", sel.len())
            };
            return (t, "copy".into());
        }
        match self
            .cur
            .selected
            .and_then(|id| self.cur.work.doc().data().feature(id))
        {
            // 3D スケッチは作ってすぐ直し始めるので、作っている途中も編集器の見出しにする
            Some(f) if self.creating() == Some(f.id) && self.view_editor_type().is_none() => {
                let icon = self
                    .win
                    .registry
                    .descriptor(&f.type_id)
                    .map(|d| d.icon)
                    .filter(|i| !i.is_empty())
                    .unwrap_or_else(|| "box".into());
                (format!("{} を作る", f.name), icon)
            }
            Some(f) => {
                let icon = self
                    .win
                    .registry
                    .descriptor(&f.type_id)
                    .map(|d| d.icon)
                    .filter(|i| !i.is_empty())
                    .unwrap_or_else(|| "box".into());
                // 専用の編集器が見出しを決める種類(スケッチの「拘束と寸法」)
                let title = self
                    .cur
                    .editors
                    .iter()
                    .find(|ed| ed.handles(&f.type_id))
                    .and_then(|ed| ed.panel_title());
                match title {
                    Some((t, i)) if self.is_editing(f.id) => (t.into(), i.into()),
                    _ if self.is_editing(f.id) => (format!("{} を編集", f.name), icon),
                    _ => (f.name.clone(), icon),
                }
            }
            None => ("編集パネル".into(), "display".into()),
        }
    }

    /// 操作のアイコン(操作の一覧から探す)。
    fn action_icon(&self, id: &str) -> String {
        self.actions()
            .into_iter()
            .find(|a| a.id == id)
            .map(|a| a.icon)
            .unwrap_or_default()
    }

    /// 下の欄に何を出すか(クリックで作る流れか、機能の編集か)。
    fn panel_footer_kind(&self) -> Option<Footer> {
        if self.cur.gesture.is_some() {
            return Some(Footer::Gesture);
        }
        if self.win.show_params || self.selection().len() >= 2 {
            return None;
        }
        let id = self
            .cur
            .selected
            .filter(|id| self.cur.work.doc().data().feature(*id).is_some())?;
        Some(if self.creating() == Some(id) {
            Footer::Create
        } else if self.is_editing(id) {
            Footer::Edit
        } else {
            Footer::View
        })
    }

    /// 下の欄を 1 段に並べた時の幅(ボタンと計算の札。字の大きさで変わる)。
    fn footer_width(&self, ui: &egui::Ui, kind: Footer) -> f32 {
        let w = |t: &str, f| {
            ui.painter()
                .layout_no_wrap(t.to_string(), f, egui::Color32::WHITE)
                .size()
                .x
        };
        let pad = 2.0 * ui.spacing().button_padding.x + 8.0;
        let button = |t: &str, key: &str| {
            w(t, crate::theme::prop(12.5)) + 6.0 + w(key, crate::theme::mono(10.0)) + pad
        };
        let chip = |t: &str| w(t, crate::theme::prop(11.0)) + 16.0 + 8.0;
        match kind {
            Footer::Gesture => button("OK", "Enter") + button("キャンセル", "Esc"),
            Footer::Create => {
                button("OK", "Enter") + button("キャンセル", "Esc") + chip("まだ決めていません")
            }
            Footer::View => button("編集", "Enter") + chip("ロールバックの後"),
            Footer::Edit => {
                button("OK", "Enter")
                    + button("キャンセル", "Esc")
                    + chip("ロールバックの後")
                    + w("99 回直した", crate::theme::prop(11.0))
            }
        }
    }

    /// 下の欄。`top` があれば、計算の札などをボタンの段でなくそちらに出す(2 段の時)。
    fn panel_footer(&mut self, ui: &mut egui::Ui, kind: Footer, top: Option<&mut egui::Ui>) {
        let l = theme::look(ui.ctx());
        match kind {
            Footer::Gesture => {
                // Enter・右クリックの OK と同じ(省ける手順は値なしで次へ進める)
                let ready = self.ok_blocked().is_none();
                if theme::primary(ui, "OK", "Enter", ready)
                    .on_hover_text("決めた値で作る")
                    .clicked()
                {
                    self.gesture_enter();
                }
                if theme::secondary(ui, "キャンセル", "Esc", true).clicked() {
                    self.cur.gesture = None;
                }
            }
            Footer::Create => {
                let sketch = self.view_editor_type().is_some();
                let (ok_key, cancel_key) = if sketch { ("", "") } else { ("Enter", "Esc") };
                // スケッチは「スケッチを終了」で決める
                // (設計の機能の OK と見分ける。レビュー K15)
                let (ok, ok_hint) = if sketch {
                    (SKETCH_FINISH, "スケッチを決めて 3D のビューに戻る")
                } else {
                    ("OK", "この値で作る")
                };
                if theme::primary(ui, ok, ok_key, true)
                    .on_hover_text(ok_hint)
                    .clicked()
                {
                    self.finish_edit();
                }
                if theme::secondary(ui, "キャンセル", cancel_key, true)
                    .on_hover_text("作らずにやめる(作る前の状態へ戻す)")
                    .clicked()
                {
                    self.cancel_creation();
                }
                let status = |ui: &mut egui::Ui| {
                    theme::chip(ui, "まだ決めていません", Some(l.ws()));
                };
                match top {
                    Some(t) => status(t),
                    None => {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), status);
                    }
                }
            }
            Footer::View => {
                let Some(id) = self.cur.selected else { return };
                if theme::primary(ui, "編集", "Enter", true)
                    .on_hover_text("この機能を直す(ダブルクリックでも開く)")
                    .clicked()
                {
                    self.open_edit(id);
                }
                match top {
                    Some(t) => self.status_chip(t, id),
                    None => {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            self.status_chip(ui, id);
                        });
                    }
                }
            }
            Footer::Edit => {
                let Some(id) = self.cur.selected else { return };
                // スケッチを直している間は Enter と Esc を作図に使うので、押せないキーを書かない(網羅版 L5)
                let (ok_key, cancel_key) = if self.view_editor_type().is_some() {
                    ("", "")
                } else {
                    ("Enter", "Esc")
                };
                let now = self.cur.work.doc().undo_labels().len();
                let start = self
                    .cur
                    .edit_start
                    .filter(|(i, _)| *i == id)
                    .map_or(now, |s| s.1);
                let changed = now != start;
                let (ok, ok_hint) = if self.view_editor_type().is_some() {
                    (
                        SKETCH_FINISH,
                        "スケッチを決めて 3D のビューに戻る(スケッチは選んだまま)",
                    )
                } else {
                    ("OK", "直したところを決めて、直すのを終える")
                };
                if theme::primary(ui, ok, ok_key, true)
                    .on_hover_text(ok_hint)
                    .clicked()
                {
                    self.finish_edit();
                }
                if theme::secondary(ui, "キャンセル", cancel_key, true)
                    .on_hover_text("直したところを捨てて、直し始めた時の状態へ戻す")
                    .clicked()
                {
                    self.cancel_edit();
                }
                let status = |ui: &mut egui::Ui| {
                    self.status_chip(ui, id);
                    if changed {
                        ui.label(
                            egui::RichText::new(format!("{} 回直した", now.saturating_sub(start)))
                                .size(crate::theme::fs(11.0))
                                .color(l.pal.ink3),
                        );
                    }
                };
                match top {
                    Some(t) => status(t),
                    None => {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), status);
                    }
                }
            }
        }
    }

    /// 計算の状態の札(計算できた・注意あり など)。
    fn status_chip(&self, ui: &mut egui::Ui, id: kmb_core::FeatureId) {
        let l = theme::look(ui.ctx());
        let (mark, detail) = self.status_line(id);
        let (text, c) = match mark {
            "✔" => ("計算できた", l.pal.ok),
            "⚠" => ("注意あり", l.pal.warn),
            "✖" | "⏸" => ("計算できない", l.pal.err),
            "–" => ("抑制中", l.pal.ink3),
            "…" => ("ロールバックの後", l.pal.ink3),
            _ => ("", l.pal.ink3),
        };
        if !text.is_empty() {
            let r = theme::chip(ui, text, Some(c));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
            if !detail.is_empty() {
                r.on_hover_text(detail);
            }
        }
    }

    pub(crate) fn panel_ui(&mut self, ui: &mut egui::Ui) {
        // 前のフレームまでに別の欄(設定の窓など)で押した Enter は使わない
        ui.data_mut(|d| d.remove::<bool>(crate::feature_ui::enter_id()));
        if let Some(g) = &self.cur.gesture {
            // 手順の一覧(済んだものは消し線、今のものは作業空間の色)
            let l = theme::look(ui.ctx());
            for (i, h) in g.handles.iter().enumerate() {
                let t = egui::RichText::new(format!("{}. {}", i + 1, h.label))
                    .size(crate::theme::fs(11.5));
                let t = if i < g.step {
                    t.color(l.pal.ink2).strikethrough()
                } else if i == g.step {
                    t.color(l.ws()).strong()
                } else {
                    t.color(l.pal.ink3)
                };
                ui.label(t);
            }
            // 最後の手順を決めると、そのまま作る(残りの値は作っている途中の欄で直す)
            ui.label(
                egui::RichText::new("最後の手順を数で打って Enter なら、作って決める。クリックで決めると作っている途中になり、残りの値を欄で直して OK か Enter で決める")
                    .size(crate::theme::fs(11.5))
                    .color(l.pal.ink3),
            );
            if let (Some(v), Some(h)) = (&g.live, g.current()) {
                ui.label(
                    egui::RichText::new(format!("今の値: {}", crate::gesture::describe_for(h, v)))
                        .color(l.pal.ink2),
                );
            }
            if let (false, Some(h)) = (g.typed.is_empty(), g.current()) {
                ui.label(
                    egui::RichText::new(format!(
                        "打った値: {}",
                        crate::gesture::typed_text(h, &g.typed)
                    ))
                    .color(l.ws()),
                );
            }
            let typing = g.current().and_then(crate::gesture::typing_hint);
            ui.label(
                egui::RichText::new(format!(
                    "{}ボタンを押したまま動かす操作はありません(誤って形が変わらないように)。",
                    typing.map(|t| format!("{t}。")).unwrap_or_default()
                ))
                .size(crate::theme::fs(11.5))
                .color(l.pal.ink2),
            );
            return;
        }
        if self.win.show_params {
            self.params_ui(ui);
            return;
        }
        let sel = self.selection();
        if sel.len() >= 2 {
            self.multi_panel(ui, &sel);
            return;
        }
        let Some(entry) = self
            .cur
            .selected
            .and_then(|id| self.cur.work.doc().data().feature(id).cloned())
        else {
            ui.weak(
                "タイムラインかブラウザで機能を選ぶと、ここに値が出ます。直すときはダブルクリックするか「編集」を押します。",
            );
            return;
        };
        if !self.is_editing(entry.id) {
            // 選んだだけ: 値を見せるだけにする(誤って既存の機能を直さないように)
            let l = theme::look(ui.ctx());
            ui.label(
                egui::RichText::new(
                    "選んでいるだけです(灰色の欄は直せません)。直すときは下の「編集」を押すか、タイムライン・ブラウザでダブルクリックします。ボタンの操作はこのまま押せます。",
                )
                .size(crate::theme::fs(11.5))
                .color(l.pal.ink2),
            );
            crate::feature_ui::feature_panel(ui, self, &entry, false);
            return;
        }
        if let Some(data) = crate::feature_ui::feature_panel(ui, self, &entry, true) {
            self.set_panel_data(&entry, data);
            // JSON の欄は新しいデータで作り直す
            self.cur.data_edit = None;
        }
        // 値の欄で打って Enter: 値を入れてコマンドを決める(
        // 作っている途中も、直している途中も。値だけ決めて続けるのは Tab。2026-10-18 Kie)。
        // スケッチを直している間は、Enter を作図に使うので決めない
        let enter = ui.data_mut(|d| d.remove_temp::<bool>(crate::feature_ui::enter_id()));
        if enter == Some(true)
            && self.view_editor_type().is_none()
            && (self.cur.creating.is_some_and(|(id, _)| id == entry.id)
                || self.is_editing(entry.id))
        {
            self.finish_edit();
        }
    }

    /// 編集の欄で直したデータを入れる。欄を決まった値にした時に続けて始める操作
    /// ([`kmb_core::Field::pick`]。範囲を「対象まで」にしたら面を選ぶ、など)があれば、
    /// 直している途中のまま始める。
    pub(crate) fn set_panel_data(
        &mut self,
        entry: &kmb_core::FeatureEntry,
        data: serde_json::Value,
    ) {
        let mut data = data;
        // 手で替えたら印を付ける欄([`kmb_core::Field::marks`]。押し出しの演算を手で選んだら、
        // もう自動で替えない)
        if let Some(d) = self.win.registry.descriptor(&entry.type_id) {
            for f in &d.fields {
                if let Some(mark) = &f.marks
                    && f.get(&data) != f.get(&entry.data)
                {
                    kmb_core::Field::bool(mark, "")
                        .set(&mut data, Some(serde_json::Value::Bool(true)));
                }
            }
        }
        let picks: Vec<String> = self
            .win
            .registry
            .descriptor(&entry.type_id)
            .map(|d| {
                d.fields
                    .iter()
                    .filter_map(|f| {
                        let p = f.pick.as_ref()?;
                        let now = f.get(&data).and_then(|v| v.as_str());
                        let was = f.get(&entry.data).and_then(|v| v.as_str());
                        (now == Some(p.value.as_str()) && was != now).then(|| p.action.clone())
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.set_feature_data(entry.id, data);
        if let Some(a) = picks.first() {
            self.begin_pick(a);
        }
    }

    /// 選んでいる機能(2 つ以上)をまとめて直せるか(共通の欄がある)。
    pub(crate) fn can_multi_edit(&self, sel: &[kmb_core::FeatureId]) -> bool {
        let entries: Vec<FeatureEntry> = sel
            .iter()
            .filter_map(|id| self.cur.work.doc().data().feature(*id).cloned())
            .collect();
        entries.len() >= 2 && !self.common_fields(&entries).is_empty()
    }

    /// 複数の機能の、共通の欄(同じ名前と種類の欄)。
    fn common_fields(&self, entries: &[FeatureEntry]) -> Vec<Field> {
        let descs: Vec<_> = entries
            .iter()
            .filter_map(|e| self.win.registry.descriptor(&e.type_id))
            .collect();
        if descs.len() != entries.len() {
            return Vec::new();
        }
        descs[0]
            .fields
            .iter()
            .filter(|f| {
                !matches!(
                    f.kind,
                    FieldKind::Output { .. }
                        | FieldKind::Info
                        | FieldKind::Pick { .. }
                        | FieldKind::Rows { .. }
                )
            })
            .filter(|f| {
                descs[1..].iter().all(|d| {
                    d.fields
                        .iter()
                        .any(|g| g.key == f.key && same_kind(&g.kind, &f.kind))
                })
            })
            .filter(|f| entries.iter().all(|e| f.applies(&e.data)))
            .cloned()
            .collect()
    }

    fn multi_panel(&mut self, ui: &mut egui::Ui, ids: &[kmb_core::FeatureId]) {
        let entries: Vec<FeatureEntry> = ids
            .iter()
            .filter_map(|id| self.cur.work.doc().data().feature(*id).cloned())
            .collect();
        ui.strong(format!("{} 個を選んでいます", entries.len()));
        ui.weak(
            entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>()
                .join("、"),
        );
        let fields = self.common_fields(&entries);
        if fields.is_empty() {
            ui.weak("まとめて直せる共通の欄がありません(同じ種類の加工の操作を選ぶと直せます)。");
            return;
        }
        let editing = self.cur.multi_edit;
        if editing {
            ui.weak(format!("値が違う欄は「{MIXED}」。直すと選んだ全部に入る"));
        } else {
            ui.horizontal(|ui| {
                if ui
                    .button("まとめて編集")
                    .on_hover_text("共通の欄を、選んだ全部に入れて直す")
                    .clicked()
                {
                    self.cur.multi_edit = true;
                }
                ui.weak("選んでいるだけです");
            });
        }
        let mut change = None;
        ui.add_enabled_ui(editing, |ui| {
            egui::Grid::new("multi_form").num_columns(2).show(ui, |ui| {
                for f in &fields {
                    let vals: Vec<Option<&Json>> = entries.iter().map(|e| f.get(&e.data)).collect();
                    let same = vals.windows(2).all(|w| w[0] == w[1]);
                    // 見せるデータ: 同じならその値、違えば「—」
                    let mut shown = entries[0].data.clone();
                    if !same {
                        f.set(&mut shown, Some(Json::String(MIXED.into())));
                    }
                    let before = f.get(&shown).cloned();
                    crate::feature_ui::field_ui(ui, self, &entries[0], f, &mut shown, false);
                    let after = f.get(&shown).cloned();
                    if after != before {
                        change = Some((f.key.clone(), after));
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((key, v)) = change {
            match edit::set_field(self.cur.work.doc().data(), &self.win.registry, ids, &key, v) {
                Ok(tx) if !tx.ops.is_empty() => {
                    self.apply(tx);
                }
                Ok(_) => {}
                Err(e) => self.win.fail(e),
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Footer {
    Gesture,
    /// 作っている途中(OK で決め、キャンセルで作る前へ)
    Create,
    /// 直している(OK で終え、キャンセルで直し始めた時へ)
    Edit,
    /// 選んだだけ(「編集」で直し始める)
    View,
}

fn same_kind(a: &FieldKind, b: &FieldKind) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}
