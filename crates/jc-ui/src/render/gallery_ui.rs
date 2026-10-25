//! レンダリング ギャラリー。レンダリングの作業空間の下の帯(設計の
//! タイムラインの所)に、描いた静止画を描いた時の版と一緒に並べる。
//! 「レンダリング…」で描き終えると、書き出したファイルとは別に文書の版の履歴の横へ残る
//! (jc-session の `gallery`)。絵を押すと大きく見る窓、右クリックで書き出す・版を開く・消す。
//! 版の履歴の窓からも、その版の絵だけを出せる。

use crate::Workspace;
use crate::layout::Space;
use crate::theme;
use jc_session::gallery::{self, Render};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// ギャラリーの状態。
#[derive(Default)]
pub(crate) struct GalleryPage {
    /// 読んだ一覧と、その文書(足した・消した時は `None` にして読み直す)
    list: Option<(PathBuf, Vec<Render>)>,
    /// 出す版の番号(`None` はすべて)
    pub version: Option<u32>,
    /// 大きく見ている絵の番号と絵
    viewing: Option<(u32, Option<egui::TextureHandle>)>,
    /// 縮小図の絵の控え(文書と絵の番号ごと。絵は変わらないので 1 回だけ読む)
    thumbs: HashMap<(PathBuf, u32), Option<egui::TextureHandle>>,
}

/// 版の印(「版 3」。保存の後に変えてから描いた絵は「版 3 +変更」)。
pub(crate) fn version_label(r: &Render) -> String {
    match (r.version_number, r.changed) {
        (Some(n), false) => format!("版 {n}"),
        (Some(n), true) => format!("版 {n} +変更"),
        (None, _) => "保存前".into(),
    }
}

fn texture(ctx: &egui::Context, name: String, rgba: (u32, u32, Vec<u8>)) -> egui::TextureHandle {
    let (w, h, px) = rgba;
    let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
    ctx.load_texture(name, img, egui::TextureOptions::LINEAR)
}

impl Workspace {
    /// 今の文書のギャラリーの絵(新しい順。保存していなければ空)。
    pub fn gallery(&self) -> Vec<Render> {
        self.cur.work.path().map(gallery::list).unwrap_or_default()
    }

    /// 書き出した静止画 `image` を、文書 `doc` のギャラリーに残す(描き終えた時)。
    /// `changed` は描き始めた時に保存の後の変更があったか。`keep` は残す枚数(超えた古い絵は消す)。
    /// 残せた番号を返す。
    pub(crate) fn add_to_gallery(
        &mut self,
        doc: &Path,
        image: &Path,
        changed: bool,
        note: &str,
        keep: usize,
    ) -> Option<u32> {
        let r = std::fs::read(image)
            .map_err(|e| format!("{} を読めません: {e}", image.display()))
            .and_then(|png| gallery::add(doc, &png, changed, note, keep));
        self.cur.gallery.list = None;
        match r {
            Ok(r) => Some(r.number),
            Err(e) => {
                log::warn!("ギャラリーに残せません: {e}");
                None
            }
        }
    }

    /// 絵 `number` を大きく見る窓を開く。
    pub fn view_render(&mut self, number: u32) {
        log::info!("ギャラリーの絵 {number} を大きく見る");
        self.cur.gallery.viewing = Some((number, None));
    }

    /// 大きく見ている絵の番号。
    pub fn viewing_render(&self) -> Option<u32> {
        self.cur.gallery.viewing.as_ref().map(|(n, _)| *n)
    }

    /// 絵 `number` を消す。
    pub fn remove_render(&mut self, number: u32) -> bool {
        let Some(path) = self.cur.work.path().map(std::path::Path::to_path_buf) else {
            return false;
        };
        self.cur.gallery.list = None;
        match gallery::remove(&path, number) {
            Ok(()) => {
                if self.viewing_render() == Some(number) {
                    self.cur.gallery.viewing = None;
                }
                self.win.message = Some(format!("ギャラリーの絵 {number} を消しました"));
                true
            }
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    /// 絵 `number` を、選んだファイルへ書き出す。
    fn export_render(&mut self, number: u32) {
        let Some(path) = self.cur.work.path().map(std::path::Path::to_path_buf) else {
            return;
        };
        let Some(png) = gallery::image(&path, number) else {
            self.win.fail(format!("絵 {number} がありません"));
            return;
        };
        let stem = path
            .file_stem()
            .map_or(String::new(), |s| s.to_string_lossy().to_string());
        let name = format!("{stem}-レンダリング{number}.png");
        match self
            .win
            .dialogs
            .save_file("絵を書き出す", "PNG", &["png"], &name)
        {
            Some(out) => match jc_session::write_output(&out, &png) {
                Ok(()) => self.win.message = Some(format!("{} に書き出しました", out.display())),
                Err(e) => self.win.fail(format!("書き出せません: {e}")),
            },
            None => {
                if let Some(why) = self.win.dialogs.unavailable() {
                    self.win.fail(format!("保存の窓を出せません({why})"));
                }
            }
        }
    }

    /// 絵 `number` を描いた版を別のタブに開く。
    fn open_render_version(&mut self, number: u32) {
        let id = self
            .gallery()
            .into_iter()
            .find(|r| r.number == number)
            .and_then(|r| r.version);
        match id {
            Some(id) => {
                self.open_version(&id);
            }
            None => self
                .win
                .fail("この絵は版を残す前に描いたので、開く版がありません"),
        }
    }

    /// 版 `number` の絵だけをギャラリーに出す(レンダリングの作業空間へ移り、下の帯を出す)。
    pub fn show_gallery_of_version(&mut self, number: Option<u32>) {
        if self.win.layout.space != Space::Render {
            self.switch_space(Space::Render);
        }
        self.win.layout.timeline_open = true;
        self.cur.gallery.version = number;
    }

    fn gallery_list(&mut self) -> Vec<Render> {
        let Some(p) = self.cur.work.path().map(std::path::Path::to_path_buf) else {
            return Vec::new();
        };
        if self.cur.gallery.list.as_ref().map(|(q, _)| q) != Some(&p) {
            self.cur.gallery.list = Some((p.clone(), gallery::list(&p)));
        }
        self.cur
            .gallery
            .list
            .as_ref()
            .map(|(_, l)| l.clone())
            .unwrap_or_default()
    }

    fn gallery_thumb(&mut self, ctx: &egui::Context, number: u32) -> Option<egui::TextureHandle> {
        let p = self.cur.work.path().map(std::path::Path::to_path_buf)?;
        let key = (p.clone(), number);
        if let Some(h) = self.cur.gallery.thumbs.get(&key) {
            return h.clone();
        }
        let h = gallery::thumbnail(&p, number)
            .and_then(|png| jc_session::history::decode_thumbnail(&png))
            .map(|(w, h, rgb)| {
                let px = rgb
                    .chunks(3)
                    .flat_map(|c| [c[0], c[1], c[2], 255])
                    .collect();
                texture(ctx, format!("gallery:{}:{number}", p.display()), (w, h, px))
            });
        self.cur.gallery.thumbs.insert(key, h.clone());
        h
    }

    /// 下の帯のレンダリング ギャラリー。
    pub(crate) fn gallery_ui(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        let list = self.gallery_list();
        let mut versions: Vec<u32> = list.iter().filter_map(|r| r.version_number).collect();
        versions.sort_unstable_by(|a, b| b.cmp(a));
        versions.dedup();
        if self
            .cur
            .gallery
            .version
            .is_some_and(|v| !versions.contains(&v))
        {
            self.cur.gallery.version = None;
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.strong("レンダリング ギャラリー");
            if self.cur.work.path().is_none() {
                ui.weak("保存した文書だけ、描いた静止画が版と一緒にここへ並びます");
                return;
            }
            let shown = |v: Option<u32>| match v {
                Some(n) => format!("版 {n}"),
                None => "すべての版".into(),
            };
            egui::ComboBox::from_id_salt("gallery_version")
                .selected_text(shown(self.cur.gallery.version))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.cur.gallery.version, None, shown(None));
                    for v in &versions {
                        ui.selectable_value(
                            &mut self.cur.gallery.version,
                            Some(*v),
                            shown(Some(*v)),
                        );
                    }
                })
                .response
                .on_hover_text("この版から描いた絵だけを出す");
            let n = list
                .iter()
                .filter(|r| {
                    self.cur
                        .gallery
                        .version
                        .is_none_or(|v| r.version_number == Some(v))
                })
                .count();
            ui.weak(format!("{n} 枚"));
        });
        if self.cur.work.path().is_none() {
            return;
        }
        if list.is_empty() {
            ui.weak("まだ絵がありません。「レンダリング…」で描いた静止画が、描いた時の版と一緒にここへ並びます");
            return;
        }
        let h = (ui.available_height() - 22.0).clamp(36.0, 160.0);
        let w = h * 4.0 / 3.0;
        let mut act: Option<(u8, u32)> = None;
        egui::ScrollArea::horizontal()
            .id_salt("gallery_strip")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for r in &list {
                        if self
                            .cur.gallery
                            .version
                            .is_some_and(|v| r.version_number != Some(v))
                        {
                            continue;
                        }
                        ui.vertical(|ui| {
                            ui.set_width(w);
                            let (rect, resp) =
                                ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
                            let name = format!("絵 {}", r.number);
                            resp.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name)
                            });
                            ui.painter()
                                .rect_filled(rect, theme::corner(4.0), l.pal.canvas);
                            if let Some(t) = self.gallery_thumb(ui.ctx(), r.number) {
                                crate::paint_fit(ui.painter(), rect, &t);
                            }
                            let on = self.viewing_render() == Some(r.number);
                            let stroke = if on || resp.hovered() {
                                egui::Stroke::new(2.0, l.ws())
                            } else {
                                egui::Stroke::new(1.0, l.pal.edge)
                            };
                            ui.painter().rect_stroke(
                                rect,
                                theme::corner(4.0),
                                stroke,
                                egui::StrokeKind::Inside,
                            );
                            let resp = resp
                                .on_hover_text(format!(
                                    "絵 {}・{}・{}×{} 画素{}{}\n押すと大きく見る。右クリックで書き出す・版を開く・消す",
                                    r.number,
                                    crate::versions_ui::ago_secs(r.saved_at),
                                    r.width,
                                    r.height,
                                    if r.note.is_empty() {
                                        String::new()
                                    } else {
                                        format!("・{}", r.note)
                                    },
                                    if r.changed {
                                        "\n保存の後に変えてから描いた(この版とは少し違う形)"
                                    } else {
                                        ""
                                    }
                                ));
                            if resp.clicked() {
                                act = Some((0, r.number));
                            }
                            resp.context_menu(|ui| {
                                for (k, name) in
                                    [(0, "大きく見る"), (1, "書き出す…"), (2, "描いた版を開く"), (3, "削除")]
                                {
                                    if ui.button(name).clicked() {
                                        act = Some((k, r.number));
                                        ui.close();
                                    }
                                }
                            });
                            ui.label(
                                egui::RichText::new(version_label(r))
                                    .size(11.0)
                                    .color(l.pal.ink2),
                            );
                        });
                    }
                });
            });
        self.gallery_act(act);
    }

    fn gallery_act(&mut self, act: Option<(u8, u32)>) {
        match act {
            Some((0, n)) => self.view_render(n),
            Some((1, n)) => self.export_render(n),
            Some((2, n)) => self.open_render_version(n),
            Some((3, n)) => {
                self.remove_render(n);
            }
            _ => {}
        }
    }

    /// 大きく見る窓(ギャラリーの絵を押した時)。
    pub(crate) fn render_viewer_window(&mut self, ctx: &egui::Context) {
        let Some((number, _)) = self.cur.gallery.viewing else {
            return;
        };
        let Some(path) = self.cur.work.path().map(std::path::Path::to_path_buf) else {
            self.cur.gallery.viewing = None;
            return;
        };
        let Some(r) = self.gallery_list().into_iter().find(|r| r.number == number) else {
            self.cur.gallery.viewing = None;
            return;
        };
        if self
            .cur
            .gallery
            .viewing
            .as_ref()
            .is_some_and(|(_, t)| t.is_none())
        {
            let t = gallery::image(&path, number)
                .and_then(|png| gallery::decode(&png))
                .map(|px| texture(ctx, format!("gallery-view:{number}"), px));
            self.cur.gallery.viewing = Some((number, t));
        }
        let tex = self
            .cur
            .gallery
            .viewing
            .as_ref()
            .and_then(|(_, t)| t.clone());
        let mut open = true;
        let mut act = None;
        egui::Window::new(format!("レンダリングの絵 {number}"))
            .id(egui::Id::new("render_viewer"))
            .open(&mut open)
            .default_size([720.0, 520.0])
            .default_pos([300.0, 120.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(version_label(&r));
                    ui.weak(format!(
                        "{}・{}×{} 画素{}",
                        crate::versions_ui::ago_secs(r.saved_at),
                        r.width,
                        r.height,
                        if r.note.is_empty() {
                            String::new()
                        } else {
                            format!("・{}", r.note)
                        }
                    ));
                });
                ui.horizontal(|ui| {
                    if ui.button("書き出す…").clicked() {
                        act = Some((1, number));
                    }
                    if ui
                        .add_enabled(r.version.is_some(), egui::Button::new("描いた版を開く"))
                        .on_hover_text("この絵を描いた版を別のタブに写しとして開く")
                        .clicked()
                    {
                        act = Some((2, number));
                    }
                    if ui.button("削除").clicked() {
                        act = Some((3, number));
                    }
                });
                let size = ui.available_size().max(egui::vec2(64.0, 48.0));
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let l = theme::look(ui.ctx());
                ui.painter()
                    .rect_filled(rect, theme::corner(4.0), l.pal.canvas);
                match &tex {
                    Some(t) => crate::paint_fit(ui.painter(), rect, t),
                    None => {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "絵を読めません",
                            theme::prop(12.0),
                            l.pal.ink2,
                        );
                    }
                }
            });
        if !open {
            self.cur.gallery.viewing = None;
        }
        self.gallery_act(act);
    }
}
