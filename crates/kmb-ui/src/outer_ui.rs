//! 部品をアプリの外の窓へ出す(Unity と同じく、別のモニターへも動かせる。N4 の続き)。
//!
//! 部品(ブラウザ・編集パネル・タイムライン)の右クリックの「アプリの外の窓に出す」で、OS の別の窓に
//! する。その窓を閉じると、並びの元の場所へ戻る。出した窓の場所と大きさは `layout.json` の `outer` に
//! 書き、開き直しても同じ所に出す。
//!
//! 描き直す時は本体の窓と分ける(egui の deferred の viewport)。外の窓は、その窓を操作した時と、
//! 本体の窓でクリックやキーの操作があった時に描き直す。本体の窓が動き続けている間(3D のビューを
//! 回す・動きを見せるなど)は、外の窓を 1 秒に 5 回までにする。何もしていない間は描き直さない。
//! 描き直すたびに GPU を使うので、動きの要らない部品で本体と同じ速さで描かない(Kie の要望)。
//!
//! 外の窓の中身は本体の `Workspace` を使って描くので、アプリは `Workspace::into_shared` で
//! `Arc<Mutex<Workspace>>` にして持つ。共有していない時(試験)や、窓を分けられない時は、
//! 本体の窓の中の窓(egui の Window)で出す。

use crate::theme::RightMenu as _;
use crate::dock_ui::{DockAction, Pane, pane_menu};
use crate::titlebar_ui::{WinButton, window_button};
use crate::{Workspace, theme};
use egui::{ViewportBuilder, ViewportId};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// 本体の窓が動き続けている間に、外の窓を描き直す間隔(1 秒に 5 回)。
const BUSY_REFRESH: Duration = Duration::from_millis(200);

/// 外の窓の大きさの下限(論理の px)。
const MIN_SIZE: [f32; 2] = [220.0, 160.0];

/// 覚えていた場所が、今のモニターの並びで見えそうか。外したモニターの上だった時などは、本体の窓の
/// 近くへ出し直す(見えない所に窓が開いて戻せなくなるのを防ぐ)。egui は本体の窓のモニターの大きさしか
/// 分からないので、そのモニターの左右 2 枚・上下 1 枚分までを見える所とみなす。
fn on_screen(ctx: &egui::Context, r: [f32; 4]) -> [f32; 4] {
    let (main, monitor) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().monitor_size));
    let (Some(main), Some(m)) = (main, monitor) else {
        return r;
    };
    let near = egui::Rect::from_min_max(
        main.min - egui::vec2(2.0 * m.x, m.y),
        main.min + egui::vec2(3.0 * m.x, 2.0 * m.y),
    );
    if near.contains(egui::pos2(r[0] + 40.0, r[1] + 10.0)) {
        r
    } else {
        log::warn!("外の窓の覚えていた場所が見えない所なので、本体の窓の近くへ出す: {r:?}");
        [main.min.x + 60.0, main.min.y + 60.0, r[2], r[3]]
    }
}

fn outer_id(pane: Pane) -> ViewportId {
    ViewportId::from_hash_of(("jc_outer", pane))
}

/// 描き直しが要る操作か(ボタン・キー・文字・ホイール。カーソルを動かしただけは入れない)。
fn discrete_input(i: &egui::InputState) -> bool {
    i.events.iter().any(|e| {
        matches!(
            e,
            egui::Event::PointerButton { .. }
                | egui::Event::Key { .. }
                | egui::Event::Text(_)
                | egui::Event::Paste(_)
                | egui::Event::Copy
                | egui::Event::Cut
                | egui::Event::MouseWheel { .. }
        )
    })
}

impl Workspace {
    /// 画面のアプリが持つ形にする(外の窓が、本体と別の時に中身を描けるように)。
    pub fn into_shared(self) -> Arc<Mutex<Workspace>> {
        let shared = Arc::new(Mutex::new(self));
        shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .win
            .shared = Some(Arc::downgrade(&shared));
        shared
    }

    /// 部品をアプリの外の窓へ出す(今の場所と大きさのまま、少しずらして)。
    pub(crate) fn pop_out(&mut self, ctx: &egui::Context, pane: Pane) {
        let origin = ctx
            .input(|i| i.viewport().inner_rect)
            .map(|r| r.min)
            .unwrap_or(egui::Pos2::ZERO);
        let rect = self
            .docking
            .tree
            .as_ref()
            .and_then(|d| {
                let path = d.find_tab(&pane)?;
                d[path.surface][path.node].rect()
            })
            .unwrap_or(egui::Rect::from_min_size(
                egui::pos2(80.0, 80.0),
                egui::vec2(320.0, 480.0),
            ));
        let at = origin + rect.min.to_vec2() + egui::vec2(24.0, 24.0);
        let size = rect.size().max(egui::vec2(MIN_SIZE[0], MIN_SIZE[1]));
        log::info!("部品を外の窓へ出す: {pane:?}({at:?}、{size:?})");
        self.win
            .layout
            .outer
            .insert(pane, [at.x, at.y, size.x, size.y]);
    }

    /// 部品を引いて本体の窓の外で離した時に、離した所(本体の窓の中の座標)へ外の窓を出す。
    pub(crate) fn pop_out_at(
        &mut self,
        ctx: &egui::Context,
        pane: Pane,
        pos: egui::Pos2,
        size: egui::Vec2,
    ) {
        let origin = ctx
            .input(|i| i.viewport().inner_rect)
            .map(|r| r.min)
            .unwrap_or(egui::Pos2::ZERO);
        // 帯の名前のあたりをつかんでいるように、少し左上へずらす
        let at = origin + pos.to_vec2() - egui::vec2(size.x.min(240.0) / 2.0, 16.0);
        let size = size.max(egui::vec2(MIN_SIZE[0], MIN_SIZE[1]));
        log::info!("部品を本体の窓の外で離したので外の窓へ出す: {pane:?}({at:?}、{size:?})");
        self.win
            .layout
            .outer
            .insert(pane, [at.x, at.y, size.x, size.y]);
    }

    /// 外の窓へ出した部品を並びへ戻す(外の窓を閉じた時)。
    pub(crate) fn pop_in(&mut self, pane: Pane) {
        log::info!("外の窓を閉じたので並びへ戻す: {pane:?}");
        self.win.layout.outer.remove(&pane);
        self.docking.outer_start.remove(&pane);
    }

    /// 外へ出した部品の窓を出す(本体の窓の描画ごとに呼ぶ。呼ばない描画では窓が閉じる)。
    pub(crate) fn outer_windows(&mut self, ctx: &egui::Context) {
        let panes: Vec<Pane> = self
            .win
            .layout
            .outer
            .keys()
            .copied()
            .filter(|p| *p != Pane::View && self.pane_open(*p))
            .collect();
        let shared = self.win.shared.clone().filter(|_| !ctx.embed_viewports());
        let discrete = ctx.input(discrete_input);
        for pane in panes {
            let Some(shared) = shared.clone() else {
                self.outer_embedded(ctx, pane);
                continue;
            };
            // 窓を作る時の場所と大きさ(作った後は利用者が動かすので、同じ値を渡し続ける)
            let saved = self.win.layout.outer[&pane];
            let [x, y, w, h] = *self
                .docking
                .outer_start
                .entry(pane)
                .or_insert_with(|| on_screen(ctx, saved));
            let builder = ViewportBuilder::default()
                .with_title(pane.label(self.win.layout.space))
                .with_position([x, y])
                .with_inner_size([w, h])
                .with_min_inner_size(MIN_SIZE)
                // 本体と同じく、アプリが描く帯にする(設定で標準の帯にした時は標準の帯)
                .with_decorations(!self.custom_title_bar());
            let id = outer_id(pane);
            ctx.show_viewport_deferred(id, builder, move |ui, _class| {
                let Some(ws) = shared.upgrade() else {
                    return;
                };
                // 本体の窓を描いている間は待つ(同じスレッドなので、ふつうは空いている)
                let Ok(mut ws) = ws.try_lock() else {
                    ui.ctx().request_repaint_after(BUSY_REFRESH);
                    return;
                };
                ws.outer_body(ui, pane);
            });
            if discrete {
                ctx.request_repaint_of(id);
            } else {
                ctx.request_repaint_after_for(BUSY_REFRESH, id);
            }
        }
    }

    /// 外の窓の中身(その窓の描き直しの時に呼ばれる)。
    fn outer_body(&mut self, ui: &mut egui::Ui, pane: Pane) {
        log::trace!("外の窓を描く: {pane:?}");
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) {
            self.pop_in(pane);
            ctx.request_repaint_of(ViewportId::ROOT);
            return;
        }
        // 動かした・大きさを変えた所を覚える(開き直した時にそこへ出す)
        if let Some((outer, inner)) =
            ctx.input(|i| Some((i.viewport().outer_rect?, i.viewport().inner_rect?)))
            && let Some(r) = self.win.layout.outer.get_mut(&pane)
        {
            *r = [outer.min.x, outer.min.y, inner.width(), inner.height()];
        }
        if self.custom_title_bar() {
            let look = theme::look(&ctx);
            egui::Panel::top(egui::Id::new(("jc_outer_bar", pane)))
                .exact_size(34.0 * theme::band())
                .frame(
                    egui::Frame::new()
                        .fill(look.pal.chrome2)
                        .inner_margin(egui::Margin {
                            left: 10,
                            right: 0,
                            top: 0,
                            bottom: 0,
                        }),
                )
                .show(ui, |ui| {
                    let mut act = None;
                    self.pane_bar(ui, pane, true, &mut act);
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| self.pane_ui(ui, pane));
        self.window_edges(ui);
        // 外の窓で操作したら、本体の窓にも映す(選んだ物の強調など)
        if ctx.input(discrete_input) {
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }

    /// 外の窓と、本体の中に浮かせた窓の上の帯(どちらも同じ見た目。本体の上の帯と同じ色と高さ)。
    /// 真ん中の部品の名前を引くと窓が動く。右端は、外の窓なら最小化・最大化・閉じる、浮かせた窓なら
    /// 閉じる(どちらも閉じると並びの元の場所へ戻る)。浮かせた窓は、名前の右クリックで部品のメニュー。
    pub(crate) fn pane_bar(
        &mut self,
        ui: &mut egui::Ui,
        pane: Pane,
        outer: bool,
        act: &mut Option<DockAction>,
    ) -> BarOut {
        let l = theme::look(ui.ctx());
        let mut out = BarOut::default();
        if !outer {
            ui.painter().rect_filled(ui.max_rect(), 0.0, l.pal.chrome2);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if outer {
                    self.window_buttons(ui);
                } else if window_button(ui, WinButton::Close, false).clicked() {
                    *act = Some(DockAction::Return(pane));
                }
            });
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let r = ui.add_sized(
                    ui.available_size(),
                    egui::Label::new(
                        egui::RichText::new(pane.label(self.win.layout.space))
                            .font(theme::prop(13.0))
                            .color(l.pal.ink),
                    )
                    .selectable(false)
                    .sense(egui::Sense::click_and_drag()),
                );
                if outer {
                    self.title_drag(ui, &r);
                } else {
                    if r.dragged() {
                        out.drag = Some(r.drag_delta());
                    }
                    // 離した所(窓の外ではカーソルの位置が分からなくなることがあるので、引いている間の最後の位置)
                    let last = egui::Id::new(("float_bar_last", pane));
                    if r.dragged()
                        && let Some(p) = ui.input(|i| i.pointer.latest_pos())
                    {
                        ui.ctx().data_mut(|d| d.insert_temp(last, p));
                    }
                    if r.drag_stopped() {
                        out.dropped = ui
                            .input(|i| i.pointer.latest_pos())
                            .or_else(|| ui.ctx().data(|d| d.get_temp(last)));
                        log::debug!("浮かせた窓の帯を離した: {pane:?} {:?}", out.dropped);
                    }
                    r.right_menu(|ui| pane_menu(ui, pane, act));
                }
            });
        });
        out
    }

    /// 窓を分けられない時は、本体の窓の中の窓で出す(閉じると並びへ戻る)。
    fn outer_embedded(&mut self, ctx: &egui::Context, pane: Pane) {
        let [x, y, w, h] = self.win.layout.outer[&pane];
        let mut open = true;
        egui::Window::new(pane.label(self.win.layout.space))
            .id(egui::Id::new(("jc_outer", pane)))
            .default_pos([x, y])
            .default_size([w, h])
            .open(&mut open)
            .show(ctx, |ui| self.pane_ui(ui, pane));
        if !open {
            self.pop_in(pane);
        }
    }
}

/// 浮かせた窓の帯でしたこと。
#[derive(Default)]
pub(crate) struct BarOut {
    /// 名前を引いた量(窓を動かす)
    pub drag: Option<egui::Vec2>,
    /// 引いて離した所(本体の窓の外なら外の窓に出す)
    pub dropped: Option<egui::Pos2>,
}
