//! アプリが描く窓の帯(まとめてレビュー N4)。Windows の標準の帯をやめ、上の帯(印・ファイル・
//! 保存・文書の名前・コマンド検索・設定・ヘルプ)の右端に、最小化・最大化・閉じるを置く。
//!
//! 窓の操作は OS に任せる: 帯の空いた所を引くと OS の窓の移動(画面の端へ寄せると半分・全体に
//! する Aero Snap も OS が行う)、二度押しで最大化と戻す、窓の縁を引くと OS の大きさ変え。
//! 設定の「画面の配置」で Windows の標準の帯に戻せる(安全モードは標準の帯)。

use crate::{Workspace, theme};
use egui::{Rect, Sense, Vec2, ViewportCommand};

/// 窓の縁で大きさを変えられる幅(論理の px)。
const EDGE: f32 = 5.0;

/// 窓のボタン。
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum WinButton {
    Minimize,
    Maximize,
    Close,
}

impl Workspace {
    /// アプリが描く帯を使うか(使わない時は Windows の標準の帯)。
    pub(crate) fn custom_title_bar(&self) -> bool {
        !self.win.layout.system_title_bar && !self.win.safe_mode
    }

    /// 標準の帯の有り無しを窓へ伝える(設定を変えた時と、初めの描画の時)。
    pub(crate) fn sync_decorations(&mut self, ctx: &egui::Context) {
        let want = !self.custom_title_bar();
        if self.win.decorations_sent != Some(want) {
            ctx.send_viewport_cmd(ViewportCommand::Decorations(want));
            self.win.decorations_sent = Some(want);
        }
    }

    /// 帯の空いた所の操作(引くと窓を動かす、二度押しで最大化と戻す)。
    pub(crate) fn title_drag(&self, ui: &egui::Ui, r: &egui::Response) {
        if !self.custom_title_bar() {
            return;
        }
        if r.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
        } else if r.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
    }

    /// 上の帯の右端の、最小化・最大化(戻す)・閉じる。右から左へ並べる所で呼ぶ。
    pub(crate) fn window_buttons(&mut self, ui: &mut egui::Ui) {
        if !self.custom_title_bar() {
            return;
        }
        let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        for b in [WinButton::Close, WinButton::Maximize, WinButton::Minimize] {
            let name = self.win.t(match b {
                WinButton::Minimize => "frame-window-minimize",
                WinButton::Maximize if max => "frame-window-restore",
                WinButton::Maximize => "frame-window-maximize",
                WinButton::Close => "frame-window-close",
            });
            if window_button(ui, b, max, &name).clicked() {
                let cmd = match b {
                    WinButton::Minimize => ViewportCommand::Minimized(true),
                    WinButton::Maximize => ViewportCommand::Maximized(!max),
                    // 閉じる前の確かめ(変更の保存)は、標準の帯の閉じると同じ所で行う
                    WinButton::Close => ViewportCommand::Close,
                };
                ui.ctx().send_viewport_cmd(cmd);
            }
        }
    }

    /// 窓の縁を引いて大きさを変える(最大化・全画面の時はしない)。画面の部品より後に呼び、縁を優先する。
    pub(crate) fn window_edges(&self, ui: &mut egui::Ui) {
        if !self.custom_title_bar() {
            return;
        }
        let (max, full) = ui.ctx().input(|i| {
            (
                i.viewport().maximized.unwrap_or(false),
                i.viewport().fullscreen.unwrap_or(false),
            )
        });
        if max || full {
            return;
        }
        use egui::ResizeDirection as D;
        let s = ui.ctx().content_rect();
        let (l, r, t, b) = (s.left(), s.right(), s.top(), s.bottom());
        let c = EDGE * 2.0;
        let zones = [
            (
                D::NorthWest,
                Rect::from_min_max(s.min, egui::pos2(l + c, t + c)),
            ),
            (
                D::NorthEast,
                Rect::from_min_max(egui::pos2(r - c, t), egui::pos2(r, t + c)),
            ),
            (
                D::SouthWest,
                Rect::from_min_max(egui::pos2(l, b - c), egui::pos2(l + c, b)),
            ),
            (
                D::SouthEast,
                Rect::from_min_max(egui::pos2(r - c, b - c), s.max),
            ),
            (D::North, Rect::from_min_max(s.min, egui::pos2(r, t + EDGE))),
            (D::South, Rect::from_min_max(egui::pos2(l, b - EDGE), s.max)),
            (D::West, Rect::from_min_max(s.min, egui::pos2(l + EDGE, b))),
            (D::East, Rect::from_min_max(egui::pos2(r - EDGE, t), s.max)),
        ];
        for (dir, rect) in zones {
            let resp = ui.interact(
                rect,
                egui::Id::new(("window_edge", dir as u8)),
                Sense::drag(),
            );
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(cursor(dir));
            }
            if resp.drag_started_by(egui::PointerButton::Primary) {
                ui.ctx()
                    .send_viewport_cmd(ViewportCommand::BeginResize(dir));
            }
        }
    }
}

fn cursor(d: egui::ResizeDirection) -> egui::CursorIcon {
    use egui::{CursorIcon as C, ResizeDirection as D};
    match d {
        D::North => C::ResizeNorth,
        D::South => C::ResizeSouth,
        D::East => C::ResizeEast,
        D::West => C::ResizeWest,
        D::NorthEast => C::ResizeNorthEast,
        D::NorthWest => C::ResizeNorthWest,
        D::SouthEast => C::ResizeSouthEast,
        D::SouthWest => C::ResizeSouthWest,
    }
}

/// 窓のボタン 1 つ(帯の高さいっぱい。閉じるは乗せると赤)。`name` は今の言語のボタンの名前。
pub(crate) fn window_button(
    ui: &mut egui::Ui,
    b: WinButton,
    maximized: bool,
    name: &str,
) -> egui::Response {
    let l = theme::look(ui.ctx());
    let h = ui.available_height();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(40.0 * theme::band(), h), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    let p = ui.painter();
    let (fill, ink) = match (b, resp.hovered(), resp.is_pointer_button_down_on()) {
        (WinButton::Close, true, _) => (egui::Color32::from_rgb(196, 43, 28), egui::Color32::WHITE),
        (_, true, true) => (l.pal.edge, l.pal.ink),
        (_, true, false) => (l.pal.chrome, l.pal.ink),
        _ => (egui::Color32::TRANSPARENT, l.pal.ink2),
    };
    p.rect_filled(rect, 0.0, fill);
    let c = rect.center();
    let k = 5.0 * theme::band();
    let st = egui::Stroke::new(1.0, ink);
    match b {
        WinButton::Minimize => {
            p.hline(c.x - k..=c.x + k, c.y, st);
        }
        WinButton::Maximize if maximized => {
            // 重なった 2 つの四角
            let back = Rect::from_center_size(c + Vec2::new(1.5, -1.5), Vec2::splat(2.0 * k - 3.0));
            let front =
                Rect::from_center_size(c + Vec2::new(-1.5, 1.5), Vec2::splat(2.0 * k - 3.0));
            p.line_segment([back.left_top(), back.right_top()], st);
            p.line_segment([back.right_top(), back.right_bottom()], st);
            p.rect_filled(front, 0.0, fill);
            p.rect_stroke(front, 0.0, st, egui::StrokeKind::Middle);
        }
        WinButton::Maximize => {
            p.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(2.0 * k)),
                0.0,
                st,
                egui::StrokeKind::Middle,
            );
        }
        WinButton::Close => {
            p.line_segment([c + Vec2::new(-k, -k), c + Vec2::new(k, k)], st);
            p.line_segment([c + Vec2::new(-k, k), c + Vec2::new(k, -k)], st);
        }
    }
    resp.on_hover_text(name)
}
