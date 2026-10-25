//! 版の見比べ。保存した版と今の文書の違いを、パラメータ・機能・属性の表と、
//! 版の形を 3D のビューに透かして重ねる絵で見せる。合流(マージ)は作らない
//! (docs/避ける形.md)。
//! 版の形は jc-session の [`history::version_shape`] が別のカーネルで計算し、ここは別のスレッドで
//! 待つだけ(今の文書の形とキャッシュに触らない)。

use crate::{Workspace, view3d};
use jc_session::history::{self, Change, VersionShape};
use std::sync::mpsc;

/// 見比べている版。
pub(crate) struct Comparison {
    pub number: u32,
    old: jc_core::Document,
    /// 版の形を重ねる
    pub overlay: bool,
    shape: Option<VersionShape>,
    /// 計算している間
    pending: Option<mpsc::Receiver<Option<VersionShape>>>,
    /// 形を出せない時の理由
    no_shape: Option<String>,
}

impl Comparison {
    /// 計算している間か。
    pub fn computing(&self) -> bool {
        self.pending.is_some()
    }

    /// 形の計算を待つ(試験用)。
    #[cfg(test)]
    pub fn wait(&mut self) {
        if let Some(rx) = self.pending.take() {
            self.shape = rx.recv().ok().flatten();
        }
    }

    /// 重ねる形の三角形の数(計算していなければ 0)。
    #[cfg(test)]
    pub fn shape_triangles(&self) -> usize {
        self.shape.as_ref().map_or(0, |s| s.tris.len())
    }

    pub(crate) fn poll(&mut self) {
        let Some(rx) = &self.pending else {
            return;
        };
        match rx.try_recv() {
            Ok(s) => {
                if s.is_none() {
                    self.no_shape = Some("この版には形がありません".into());
                }
                self.shape = s;
                self.pending = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.no_shape = Some("この版の形を計算できませんでした".into());
                self.pending = None;
            }
        }
    }
}

/// 重ねる版の形の色(まとまりの 1 番目の色。色覚の型のテーマでも見分けられる)。
fn compare_ink(l: &crate::theme::Look) -> egui::Color32 {
    l.pal.inks.group[0]
}

impl Workspace {
    /// 版 `id` を今の文書と見比べる(表を出し、形を別のスレッドで計算し始める)。
    pub fn compare_version(&mut self, id: &str) -> bool {
        let Some(path) = self.cur.work.path().map(std::path::Path::to_path_buf) else {
            return false;
        };
        let Some(v) = history::list(&path).into_iter().find(|v| v.id == id) else {
            return false;
        };
        let mut old = match history::read(&path, id) {
            Ok(d) => d,
            Err(e) => {
                self.win.fail(format!("版 {} を読めません: {e}", v.number));
                return false;
            }
        };
        // 寸法の名前は計算の時にそろう物なので、版の側もそろえてから比べる(違いに出さない)
        old.settle_names(&self.win.registry);
        let mut c = Comparison {
            number: v.number,
            old,
            overlay: true,
            shape: None,
            pending: None,
            no_shape: None,
        };
        // 形はカーネルを分けられる時だけ(分け合うと今の文書の形が捨てられる)
        match &self.win.kernel_factory {
            Some(make) => {
                let (tx, rx) = mpsc::channel();
                let (make, reg) = (make.clone(), self.win.registry.clone());
                let doc = c.old.data().clone();
                let tol = self.win.layout.render.mesh.tolerance();
                std::thread::spawn(move || {
                    let k = make();
                    let _ = tx.send(history::version_shape(&doc, &reg, &*k, tol));
                });
                c.pending = Some(rx);
            }
            None => c.no_shape = Some("この画面では版の形を重ねられません".into()),
        }
        log::info!("版の見比べ: 版 {} と今の文書", v.number);
        self.cur.versions.compare = Some(c);
        true
    }

    /// 見比べている版と今の文書の違い(版 → 今)。
    pub fn compared_changes(&self) -> Vec<Change> {
        self.cur
            .versions
            .compare
            .as_ref()
            .map(|c| history::compare(&c.old, self.cur.work.doc()))
            .unwrap_or_default()
    }

    /// 見比べている版の形を、3D のビューに透かして重ねる物。
    pub(crate) fn compare_ghosts(&self) -> Vec<view3d::Ghost> {
        let Some(c) = self.cur.versions.compare.as_ref().filter(|c| c.overlay) else {
            return Vec::new();
        };
        let Some(s) = &c.shape else {
            return Vec::new();
        };
        let ink = compare_ink(&self.win.look);
        vec![view3d::Ghost {
            tris: s.tris.clone(),
            edges: s.edges.clone(),
            color: egui::Color32::from_rgba_unmultiplied(
                ink.r(),
                ink.g(),
                ink.b(),
                view3d::GHOST_CUT_ALPHA,
            ),
            edge_color: ink,
            layer: view3d::GhostLayer::Over,
        }]
    }

    /// 版の見比べの窓。
    pub(crate) fn compare_window(&mut self, ctx: &egui::Context) {
        let Some(c) = self.cur.versions.compare.as_mut() else {
            return;
        };
        c.poll();
        if c.computing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let number = c.number;
        let changes = self.compared_changes();
        let l = crate::theme::look(ctx);
        let mut open = true;
        let mut close = false;
        egui::Window::new("版の見比べ")
            .id(egui::Id::new(("compare_window", self.tab_uid())))
            .open(&mut open)
            .default_size([560.0, 360.0])
            // 重ねた形を隠さないよう、3D のビューの左下に出す
            .pivot(egui::Align2::LEFT_BOTTOM)
            .default_pos([262.0, ctx.content_rect().bottom() - 110.0])
            .collapsible(true)
            .show(ctx, |ui| {
                let Some(c) = self.cur.versions.compare.as_mut() else {
                    return;
                };
                ui.label(format!("版 {number} から今の文書までの違い"));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut c.overlay, format!("版 {number} の形を重ねる"));
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 3.0, compare_ink(&l));
                    if c.computing() {
                        ui.spinner();
                        ui.weak("形を計算しています");
                    } else if let Some(why) = &c.no_shape {
                        ui.weak(why);
                    } else {
                        ui.weak("透けた色の形が版の形、灰色が今の形");
                    }
                });
                ui.separator();
                if changes.is_empty() {
                    ui.weak("パラメータ・機能・属性に違いはありません");
                }
                egui::ScrollArea::both()
                    .id_salt("compare_rows")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Grid::new("compare_grid")
                            .striped(true)
                            .num_columns(4)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                ui.strong("種類");
                                ui.strong("名前");
                                ui.strong(format!("版 {number}"));
                                ui.strong("今");
                                ui.end_row();
                                let none = |ui: &mut egui::Ui, s: &str, mark: egui::Color32| {
                                    if s.is_empty() {
                                        ui.weak("(無い)");
                                    } else {
                                        ui.colored_label(mark, s);
                                    }
                                };
                                for ch in &changes {
                                    ui.label(ch.what);
                                    ui.label(&ch.name);
                                    none(ui, &ch.before, l.pal.ink2);
                                    none(ui, &ch.after, l.pal.ink);
                                    ui.end_row();
                                }
                            });
                    });
                ui.separator();
                if ui.button("閉じる").clicked() {
                    close = true;
                }
            });
        if !open || close {
            self.cur.versions.compare = None;
        }
    }
}
