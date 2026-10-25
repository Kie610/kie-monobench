//! 画面の枠: 上のメニューの行(ファイル、作業空間の切替、保存・取り消し)、タブ付きツールバー、
//! 下の状態の欄、キーの割り当て、配置を覚えること。

use crate::layout::Space;
use crate::ribbon::{self, Item, PinEdit};
use crate::{MiddleDrag, Tone, Workspace, theme};
use kmb_core::Place;

/// 再計算がこれより長くかかった時だけ、下の帯に「再計算中」の札を出す。
pub(crate) const BUSY_BADGE_DELAY: std::time::Duration = std::time::Duration::from_millis(300);

/// 再計算中の札を出すか(走っているか `busy`、走っている時間 `busy_for`)と、まだ出さない時に描き直す
/// までの時間。
pub(crate) fn busy_badge(
    busy: bool,
    busy_for: Option<std::time::Duration>,
) -> (bool, Option<std::time::Duration>) {
    let wait = busy_for.map(|d| BUSY_BADGE_DELAY.saturating_sub(d));
    match wait {
        _ if !busy => (false, None),
        Some(w) if !w.is_zero() => (false, Some(w)),
        _ => (true, None),
    }
}

mod ribbon_band;
mod settings_pages;
mod sketch_band;

impl Workspace {
    /// 上の帯(試作の `.appbar`): 印、ファイル、保存・取り消し・やり直し・履歴、文書の名前、
    /// コマンド検索の欄、知らせ、設定、ヘルプ。
    pub(crate) fn menubar(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // 印(アプリが描く窓の帯の時は、ここを引いても窓が動く)。窓・exe と同じアプリのアイコンを
            // 描く。字とアイコンの倍率で箱も広げる
            let (r, logo) = ui.allocate_exact_size(
                egui::vec2(22.0, 22.0) * theme::band(),
                egui::Sense::click_and_drag(),
            );
            self.title_drag(ui, &logo);
            crate::app_icon::paint(ui, r);
            ui.add_space(6.0);
            // データパネルを出し入れ(ホームを出している間は文書へ戻ると出る)
            let data = self.win.layout.data_open;
            if theme::icon_button(ui, "apps", "データパネル", true, data)
                .on_hover_text(if data {
                    "データパネルを隠す"
                } else {
                    "データパネルを表示: 最近・プロジェクトのデザインを左に並べ、開く・3D のビューへ引いてリンクで挿入する"
                })
                .clicked()
            {
                self.win.layout.data_open = !data;
            }
            self.file_menu(ui);
            if theme::icon_button(ui, "save", "保存", true, false)
                .on_hover_text("保存(Ctrl+S)")
                .clicked()
            {
                self.save();
            }
            if theme::icon_button(
                ui,
                "undo",
                "元に戻す",
                self.cur.work.doc().can_undo(),
                false,
            )
            .on_hover_text("取り消し(Ctrl+Z)")
            .clicked()
            {
                self.undo();
            }
            if theme::icon_button(
                ui,
                "redo",
                "やり直し",
                self.cur.work.doc().can_redo(),
                false,
            )
            .on_hover_text("やり直し(Ctrl+Y)")
            .clicked()
            {
                self.redo();
            }
            if theme::icon_button(
                ui,
                "hist",
                "取り消しの履歴",
                true,
                self.win.undo_history_open,
            )
            .on_hover_text("取り消しの履歴: 済んだ変更の一覧。行を押すと、その時点へ戻る")
            .clicked()
            {
                self.toggle_undo_history();
            }
            // 右から並べる(右端から: ヘルプ、設定、区切り、知らせ、コマンド検索)。残りに文書のタブ
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                // アプリが描く窓の帯の時は、右端に最小化・最大化・閉じる
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    self.window_buttons(ui);
                });
                self.help_menu(ui);
                if theme::icon_button(ui, "gear", "設定", true, self.settings.open)
                    .on_hover_text("設定: マウス、画面の配置、機械など、この PC 全体の設定")
                    .clicked()
                {
                    self.settings.open = !self.settings.open;
                }
                let (sep, _) = ui.allocate_exact_size(egui::vec2(13.0, 18.0), egui::Sense::hover());
                ui.painter().vline(
                    sep.center().x,
                    sep.y_range(),
                    egui::Stroke::new(1.0, l.pal.edge),
                );
                self.notices_button(ui);
                self.search_box(ui);
                // 文書のタブ(候補 20。残りの幅。空いた所を引くと窓が動く)
                self.doc_tabs_bar(ui);
            });
        });
    }

    /// コマンド検索の欄の形をしたボタン(押すとコマンド検索を開く)。
    fn search_box(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        // 字が大きい時は「コマンドを探す」と「Ctrl+K」が重ならない幅にする(1 倍なら 250)
        let name = ui.painter().layout_no_wrap(
            "コマンドを探す".into(),
            crate::theme::prop(12.5),
            l.pal.ink3,
        );
        let key =
            ui.painter()
                .layout_no_wrap("Ctrl+K".into(), crate::theme::mono(10.5), l.pal.ink3);
        let icon = crate::theme::icon_px(14.0);
        let need = 8.0 + icon + 6.0 + name.size().x + 16.0 + key.size().x + 8.0;
        let (rect, r) = ui.allocate_exact_size(
            egui::vec2(need.max(250.0), crate::theme::fs(24.0).max(icon + 8.0)),
            egui::Sense::click(),
        );
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "コマンド検索"));
        let p = ui.painter();
        p.rect(
            rect,
            4.0,
            l.pal.panel,
            egui::Stroke::new(1.0, if r.hovered() { l.ws() } else { l.pal.edge }),
            egui::StrokeKind::Inside,
        );
        let ic = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 8.0 + icon / 2.0, rect.center().y),
            egui::Vec2::splat(icon),
        );
        crate::icons::paint_exact(p, ic, "search", l.pal.ink3, 1.6);
        p.galley(
            egui::pos2(ic.right() + 6.0, rect.center().y - name.size().y / 2.0),
            name,
            l.pal.ink3,
        );
        p.galley(
            rect.right_center() - egui::vec2(8.0 + key.size().x, key.size().y / 2.0),
            key,
            l.pal.ink3,
        );
        if r.on_hover_text("名前や読みでコマンドを探して実行する(Ctrl+K)")
            .clicked()
        {
            self.toggle_command_palette();
        }
    }

    /// 知らせ(鐘と、まだ見ていない数)。押すと一覧を出す。
    fn notices_button(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        let unseen = self.win.history.len().saturating_sub(self.win.notices_seen);
        // 鐘と数は字とアイコンの倍率で広げる(固定の幅だと、大きくした時に隣の検索の欄に食い込む)
        let k = theme::band();
        let w = (if unseen > 0 { 48.0 } else { 30.0 }) * k;
        let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 26.0 * k), egui::Sense::click());
        let label = format!("知らせ({})", self.win.history.len());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&r));
        if r.hovered() || open {
            ui.painter()
                .rect_filled(rect, crate::theme::corner(4.0), l.pal.edge);
        }
        let ic = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 15.0 * k, rect.center().y),
            egui::Vec2::splat(theme::icon_px(16.0)),
        );
        crate::icons::paint_exact(ui.painter(), ic, "bell", l.pal.ink, 1.6);
        if unseen > 0 {
            let c = egui::pos2(rect.left() + 32.0 * k, rect.center().y);
            let t = unseen.min(99).to_string();
            let bw = theme::fs(16.0).max(theme::fs(8.0 + 6.0 * t.len() as f32));
            ui.painter().rect_filled(
                egui::Rect::from_center_size(c, egui::vec2(bw, theme::fs(16.0))),
                theme::fs(8.0),
                l.pal.warn,
            );
            ui.painter().text(
                c,
                egui::Align2::CENTER_CENTER,
                t,
                crate::theme::prop(10.0),
                l.pal.chrome,
            );
        }
        let r = r.on_hover_text("知らせ: 計算の注意、保存、書き出しの結果");
        egui::Popup::menu(&r)
            .width(360.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                self.win.notices_seen = self.win.history.len();
                ui.label(
                    egui::RichText::new(format!("知らせ ・ {} 件", self.win.history.len()))
                        .size(crate::theme::fs(10.5))
                        .color(l.pal.ink3),
                );
                if self.win.history.is_empty() {
                    ui.weak("知らせはありません");
                }
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        let rows = self
                            .win
                            .history
                            .iter()
                            .zip(&self.win.history_at)
                            .enumerate()
                            .rev();
                        for (i, (m, t)) in rows {
                            let tone = self.win.history_tone.get(i).copied().unwrap_or_default();
                            ui.horizontal(|ui| {
                                let c = match tone {
                                    Tone::Error => l.pal.err,
                                    Tone::Warn => l.pal.warn,
                                    Tone::Info => l.pal.ink3,
                                };
                                let (d, _) = ui.allocate_exact_size(
                                    egui::vec2(8.0, 16.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(d.center(), 4.0, c);
                                ui.label(
                                    egui::RichText::new(crate::help_ui::ago(*t))
                                        .size(crate::theme::fs(10.5))
                                        .color(l.pal.ink3),
                                );
                                if ui
                                    .small_button("写す")
                                    .on_hover_text("この知らせをクリップボードへ写す")
                                    .clicked()
                                {
                                    ui.ctx().copy_text(m.clone());
                                }
                                ui.add(egui::Label::new(m.as_str()).wrap());
                            });
                        }
                    });
                ui.separator();
                if !self.win.history.is_empty()
                    && ui
                        .button("全部を写す")
                        .on_hover_text("知らせを古い順に、時刻つきでクリップボードへ写す")
                        .clicked()
                {
                    ui.ctx()
                        .copy_text(notices_text(&self.win.history, &self.win.history_at));
                }
                if ui.button("履歴を消す").clicked() {
                    self.win.history.clear();
                    self.win.history_at.clear();
                    self.win.history_tone.clear();
                    self.win.notices_seen = 0;
                    self.win.message = None;
                    ui.close();
                }
            });
    }

    /// 作業空間を替える。選んでいる機能は残す。
    pub fn switch_space(&mut self, s: Space) {
        if self.win.layout.space != s {
            self.win.layout.space = s;
            self.win.tab.clear();
            self.cur.gesture = None;
        }
    }

    /// 中央のビューを持つ編集器が、下の帯に出す操作(なければ空)。
    fn editor_status_hints(&self) -> Vec<(String, String)> {
        let Some(type_id) = self.view_editor_type() else {
            return Vec::new();
        };
        self.cur
            .editors
            .iter()
            .find(|ed| ed.handles(&type_id) && ed.takes_view())
            .map(|ed| ed.status_hints())
            .unwrap_or_default()
    }

    /// 下の帯(試作の `.statusbar`): 今できるマウスとキーの操作、右に修飾キーと単位。
    /// 知らせがあればそれを先に出す(× で消す)。
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let info = self.selection_info();
            // 右の端(単位・選んだ物の数値・修飾キー)を先に置き、左の案内は残りの幅に収める
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 今の文書の長さの単位(候補 19)。押すと設定の「単位」を開く
                if ui
                    .add(
                        egui::Label::new(
                            egui::RichText::new(format!("単位 {}", self.length_unit().name()))
                                .size(crate::theme::fs(11.5))
                                .color(l.pal.ink2),
                        )
                        .sense(egui::Sense::click()),
                    )
                    .on_hover_text("この文書の長さの単位。押すと設定の「単位」で変えられる")
                    .clicked()
                {
                    self.open_settings_at("general/units");
                }
                ui.add_space(12.0);
                self.live_badge(ui);
                // 選んでいる物の数値(詳しくは測る道具)
                if let Some(t) = &info {
                    ui.label(
                        egui::RichText::new(t)
                            .size(crate::theme::fs(11.5))
                            .color(l.pal.ink),
                    );
                    ui.add_space(12.0);
                }
                let m = ui.input(|i| i.modifiers);
                theme::modifier(ui, "Alt", m.alt);
                theme::modifier(ui, "Ctrl", m.ctrl);
                theme::modifier(ui, "Shift", m.shift);
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                    // すぐ終わる計算では札を出さない(寸法を打つたび・スライダを引くたびに一瞬ずつ
                    // 出たり消えたりしてちらつく。10-22 u11)。出す時刻に描き直しを頼む
                    let (show, wait) = busy_badge(self.is_busy(), self.busy_for());
                    if let Some(w) = wait {
                        ui.ctx().request_repaint_after(w);
                    }
                    if show {
                        ui.spinner();
                        // 長くかかる時は経った秒数も出す(画面は止めずに計算している)
                        let text = match self.busy_for().map(|d| d.as_secs()) {
                            Some(s) if s >= 1 => format!("再計算中 {s} 秒"),
                            _ => "再計算中".to_string(),
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .size(crate::theme::fs(11.5))
                                .color(l.pal.ink2),
                        )
                        .on_hover_text(
                            "計算の間もビューを回したり選んだりできる。終わると形を差し替える",
                        );
                        ui.add_space(12.0);
                    }
                    if self.is_running() {
                        match self.task_progress() {
                            // 進みを知らせる仕事(静止画のレンダリングなど): 進みの帯・経過と残り・中止
                            Some(p) => self.task_progress_ui(ui, &p),
                            None => {
                                ui.spinner();
                                // シミュレーションなどの仕事も、経った秒数を出す(U66。止まっていないと分かる)
                                let text = match self.task_for().map(|d| d.as_secs()) {
                                    Some(s) if s >= 1 => format!("計算中 {s} 秒"),
                                    _ => "計算中".to_string(),
                                };
                                ui.label(
                                    egui::RichText::new(text)
                                        .size(crate::theme::fs(11.5))
                                        .color(l.pal.ink2),
                                )
                                .on_hover_text(
                                    "計算の間もビューを回したり選んだりできる。終わると結果を知らせる",
                                );
                            }
                        }
                        ui.add_space(12.0);
                    }
                    // 入りきらない案内は途中で切らず、そこから後ろを出さない(字が大きい時)
                    let mut full = false;
                    let mut hint = |ui: &mut egui::Ui, key: &str, text: &str| {
                        let w = |t: &str, f| {
                            ui.painter()
                                .layout_no_wrap(t.into(), f, l.pal.ink2)
                                .size()
                                .x
                        };
                        let need = w(key, crate::theme::prop(10.5))
                            + 10.0
                            + 4.0
                            + w(text, crate::theme::prop(11.5));
                        if full || ui.available_width() < need {
                            full = true;
                            return;
                        }
                        theme::kbd(ui, key);
                        ui.label(
                            egui::RichText::new(text)
                                .size(crate::theme::fs(11.5))
                                .color(l.pal.ink2),
                        );
                        ui.add_space(12.0);
                    };
                    if let Some(g) = &self.cur.gesture {
                        let ready = g.ready();
                        if ready {
                            hint(ui, "Enter", "OK");
                        }
                        hint(ui, "左", "クリックで決める");
                        hint(ui, "数字", "値を打つ");
                        hint(ui, "Esc", "キャンセル");
                    } else if let Some(m) = self.win.message.clone() {
                        ui.label(
                            egui::RichText::new(&m)
                                .size(crate::theme::fs(11.5))
                                .color(l.pal.warn),
                        );
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("×").size(crate::theme::fs(11.5)),
                                )
                                .frame(false),
                            )
                            .on_hover_text("この知らせを消す(鐘の一覧には残る)")
                            .clicked()
                        {
                            self.win.message = None;
                        }
                    } else if let hints = self.editor_status_hints()
                        && !hints.is_empty()
                    {
                        // スケッチを直している間は、編集器が道具に合わせた操作を出す(3D の操作は出さない)
                        for (key, text) in &hints {
                            hint(ui, key, text);
                        }
                    } else if self.selection().len() >= 2 {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} 個を選んでいます",
                                self.selection().len()
                            ))
                            .size(crate::theme::fs(11.5))
                            .color(l.pal.ink2),
                        );
                        ui.add_space(12.0);
                        hint(ui, "Ctrl+G", "グループ");
                        hint(ui, "Delete", "削除");
                    } else {
                        hint(ui, "左", "選ぶ ・ ドラッグで範囲 ・ 長押しで重なり");
                        hint(ui, "右", "メニュー ・ ドラッグで回す");
                        let mid = match self.win.layout.middle_drag {
                            MiddleDrag::Pan => "動かす ・ 二度押しでそこを回す中心に",
                            MiddleDrag::Orbit => "回す ・ 二度押しでそこを回す中心に",
                        };
                        hint(ui, "ホイール押し", mid);
                        hint(ui, "ホイール", "拡大");
                    }
                });
            });
        });
    }

    /// 配置が変わっていれば書く(データフォルダがある時だけ。ボタンを離している時に)。
    pub(crate) fn save_layout_if_changed(&mut self, ctx: &egui::Context) {
        if self.win.layout == self.win.layout_saved || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        if let Some(dir) = self.win.data_dir.as_ref().filter(|_| !self.win.safe_mode)
            && let Err(e) = self.win.layout.save(dir)
        {
            self.win.fail(format!("画面の配置を書けません: {e}"));
        }
        self.win.layout_saved = self.win.layout.clone();
    }

    /// 画面のキー(文字の欄を打っていない時だけ)。
    pub(crate) fn view_keys(&mut self, ctx: &egui::Context) {
        // クリックで作る流れが Enter・Esc を使ったフレームでは、下の Enter・Esc を重ねて使わない
        let had_gesture = self.cur.gesture.is_some();
        self.gesture_keys(ctx);
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::Key;
        if let Some(row) = self.pressed_key(ctx, crate::keys::Scope::View) {
            self.run_key(ctx, row);
        }
        // スケッチの編集器は F・Esc・Delete を自分で使う
        if self.view_editor_type().is_some() {
            return;
        }
        if had_gesture || self.cur.gesture.is_some() {
            // クリックで決める手順の途中の Delete は黙って捨てず、やめ方を知らせる(10-22 a4)
            if ctx.input(|i| i.key_pressed(Key::Delete)) {
                self.win.message =
                    Some("手順の途中は消せません。Esc で手順をやめてから Delete で消します".into());
            }
            return;
        }
        self.reuse_keys(ctx);
        self.feature_clip_keys(ctx);
        // 1 文字のキーでよく使うコマンドを始める(キーの一覧は keys::KEYS)。測っている間は
        // 測る・隠す・検索のキーだけ(ほかの道具を重ねて始めない)
        if let Some(row) = self.pressed_key(ctx, crate::keys::Scope::Model) {
            use crate::keys::{Cmd, Does};
            let ok = self.cur.measure.is_none()
                || matches!(
                    row.does,
                    Does::Run(Cmd::Measure | Cmd::ToggleVisible | Cmd::SearchHere)
                );
            if ok {
                self.run_key(ctx, row);
                return;
            }
        }
        let (f, home, esc, del, enter) = ctx.input(|i| {
            (
                i.key_pressed(Key::F) && i.modifiers.is_none(),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::Delete),
                i.key_pressed(Key::Enter) && i.modifiers.is_none(),
            )
        });
        // 欄の一覧(ドロップダウン)やメニューが開いている時の Esc は、egui がそれを閉じるだけにする
        // (Z2: 一覧を Esc で閉じると、作りかけのセットアップや加工ごと消えていた)
        let esc = esc && !egui::Popup::is_any_open(ctx);
        // 自動分解の小さな道具を出している間: Enter で OK、Esc でやめる
        if self.cur.anim.auto.is_some() && self.search.text.is_none() {
            if enter {
                self.finish_auto_explode();
                return;
            }
            if esc {
                self.cancel_auto_explode();
                return;
            }
        }
        // Enter: 作っている途中・直している途中なら OK、選んでいるだけなら直し始める
        if enter && self.search.text.is_none() {
            if self.cur.creating.is_some() || self.cur.editing.is_some() {
                self.finish_edit();
            } else if let [id] = self.selection()[..] {
                self.open_edit(id);
            }
        }
        if f {
            self.focus_selected();
        }
        if home {
            self.cur.camera.refit();
        }
        // Esc は内側から 1 段ずつ戻す: 左ドラッグの働き → 測る道具 → 候補の一覧 → 作る途中・直す途中をやめる
        // → 選んだ面・辺 → 選んだ機能
        if esc {
            if self.win.left_drag != crate::view3d::LeftDrag::Select {
                self.win.left_drag = crate::view3d::LeftDrag::Select;
            } else if self.cur.measure.is_some() {
                self.cur.measure = None;
            } else if self.is_sectioning() {
                self.close_section();
            } else if self.view_menu.candidates.is_some() {
                self.view_menu.candidates = None;
            } else if self.leave_pick_field() {
                // 選ぶ欄が受けているのをやめる(作る・直すのは続ける)
            } else if self.cur.creating.is_some()
                || self.cur.editing.is_some()
                || self.cur.multi_edit
            {
                self.cancel_edit();
            } else if self.cur.picked.is_some()
                || !self.cur.also_picked.is_empty()
                || !self.cur.picked_edges.is_empty()
                || !self.cur.picked_vertices.is_empty()
            {
                self.cur.picked = None;
                self.cur.also_picked.clear();
                self.cur.picked_edges.clear();
                self.cur.picked_vertices.clear();
            } else {
                self.clear_selection();
            }
        }
        if del {
            let sel = self.selection();
            if !sel.is_empty() {
                self.ask_remove_features(&sel);
            } else if self.cur.picked.is_some()
                || !self.cur.also_picked.is_empty()
                || !self.cur.picked_edges.is_empty()
                || !self.cur.picked_vertices.is_empty()
            {
                // 面・辺・角は形の一部なので消せない。黙って何もしないと押し損じに見えるので、
                // 消し方を知らせる
                self.win.message = Some(
                    "面・辺・角は消せません。消すには、作った機能をタイムラインかブラウザで選んで Delete"
                        .into(),
                );
            }
        }
    }

    /// 下の帯: 走っている仕事の進みの帯、段の説明・経過・残りの見込み、「仕上げる」「中止」。
    fn task_progress_ui(&mut self, ui: &mut egui::Ui, p: &kmb_core::JobProgress) {
        let l = self.win.look;
        let f = p.fraction.clamp(0.0, 1.0);
        let elapsed = self.task_for().map_or(0.0, |d| d.as_secs_f64());
        ui.add(
            egui::ProgressBar::new(f)
                .desired_width(120.0)
                .desired_height(10.0)
                .fill(l.ws()),
        )
        .on_hover_text(format!("{:.0}%", f * 100.0));
        // 残りは、ここまでの速さが続くとした見込み(はじめの少しは揺れるので出さない)
        let left = (0.03..1.0)
            .contains(&f)
            .then(|| elapsed * (1.0 - f as f64) / f as f64);
        let text = format!(
            "{} · 経過 {}{}",
            p.note,
            clock(elapsed),
            left.map(|s| format!(" · 残り 約 {}", clock(s)))
                .unwrap_or_default()
        );
        ui.label(
            egui::RichText::new(text)
                .size(crate::theme::fs(11.5))
                .color(l.pal.ink2),
        )
        .on_hover_text("計算の間もビューを回したり選んだりできる。終わると結果を知らせる");
        let stopping = self.task_stopping();
        if let Some(name) = &p.finish
            && ui
                .add_enabled(!stopping, egui::Button::new(name.as_str()).small())
                .on_hover_text("ここまでの回数で絵を仕上げてファイルに書く(粗さが残ることがある)")
                .clicked()
        {
            self.finish_task();
        }
        if ui
            .add_enabled(!stopping, egui::Button::new("中止").small())
            .on_hover_text("途中でやめる。ファイルは書かない")
            .clicked()
        {
            self.cancel_task();
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// タブ(試作の `.tab`)。`mode` はスケッチなど、作業空間の色で塗るタブ。
fn tab_button(ui: &mut egui::Ui, name: &str, on: bool, mode: bool) -> egui::Response {
    let l = theme::look(ui.ctx());
    let g = ui
        .painter()
        .layout_no_wrap(name.to_string(), crate::theme::prop(12.5), l.pal.ink2);
    let size = egui::vec2(g.size().x + 28.0, crate::theme::fs(26.0));
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
    let p = ui.painter();
    let round = egui::CornerRadius {
        nw: 5,
        ne: 5,
        sw: 0,
        se: 0,
    };
    let fg = if mode {
        p.rect_filled(rect, round, l.ws());
        l.pal.chrome
    } else if on {
        p.rect_filled(rect, round, l.pal.panel);
        p.hline(
            egui::Rangef::new(rect.left() + 2.0, rect.right() - 2.0),
            rect.top() + 1.0,
            egui::Stroke::new(2.0, l.ws()),
        );
        l.ws()
    } else {
        if r.hovered() {
            p.rect_filled(rect, round, l.pal.chrome2);
        }
        l.pal.ink2
    };
    let pos = rect.center() - g.size() / 2.0;
    p.galley_with_override_text_color(pos, g.clone(), fg);
    if on || mode {
        p.galley_with_override_text_color(pos + egui::vec2(0.5, 0.0), g, fg);
    }
    r
}

/// 秒を「1:05」の形にする(1 時間を超えると「1:02:05」)。
pub(crate) fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// 知らせの一覧を写す文(古い順に 1 件 1 行、頭に UTC の時刻。複数行の知らせは続きの行を字下げする)。
pub(crate) fn notices_text(history: &[String], at: &[u64]) -> String {
    history
        .iter()
        .zip(at)
        .map(|(m, t)| {
            let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(*t);
            format!(
                "{} {}",
                kmb_session::logging::utc(when),
                m.replace('\n', "\n  ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
