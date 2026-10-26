//! 左: 始め方(新規デザイン・開く…・プロジェクトを追加)と場所(ホームの 3 つ、プロジェクトの
//! フォルダの木)。木の行は押すとそのフォルダを開き、デザインを引いて離すとそこへ移す。

use crate::middle::drop_target;
use crate::{Library, Place, big_button, file_name, heading, place_row};
use kmb_session::projects::Folder;
use kmb_ui::theme;
use kmb_ui::theme::RightMenu as _;
use kmb_ui::{HomeHost, HomeRequest};
use std::path::{Path, PathBuf};

impl Library {
    /// 左: 始め方と場所。
    pub(crate) fn left(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        ui.add_space(16.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        if big_button(ui, "plus", "新規デザイン", true).clicked() {
            host.request(HomeRequest::New);
        }
        if big_button(ui, "folder", "開く…", false).clicked() {
            host.request(HomeRequest::OpenDialog);
        }
        if big_button(ui, "lib", "プロジェクトを追加", false)
            .on_hover_text("PC のフォルダをプロジェクトにして、中のデザインを探せるようにします")
            .clicked()
        {
            host.request(HomeRequest::AddProject);
        }
        ui.spacing_mut().item_spacing.y = 2.0;
        heading(ui, "ホーム");
        let pinned = host.recent.iter().filter(|r| r.pinned).count();
        for (place, icon, n) in [
            (Place::Recent, "time", Some(host.recent.len())),
            (Place::Pinned, "pin", Some(pinned)),
            (Place::Samples, "box", None),
        ] {
            if place_row(ui, icon, &place.label(), n, self.place == place).clicked() {
                self.go(place);
            }
        }
        heading(ui, "プロジェクト");
        let l = theme::look(ui.ctx());
        if host.projects.is_empty() {
            ui.label(
                egui::RichText::new("まだありません")
                    .size(theme::fs(11.5))
                    .color(l.pal.ink3),
            );
        }
        for p in host.projects.clone() {
            // 木はプロジェクトを開いた時だけ読む(開いていない物は名前だけ)
            let open = self.expanded.contains(&p);
            let tree = if open {
                self.trees.get(ui.ctx(), &p).map(|t| t.root.clone())
            } else {
                None
            };
            match tree {
                Some(root) => self.tree_rows(ui, host, &p, &root, 0),
                None => {
                    let r = self.tree_row(ui, host, &p, &p, file_name(&p), None, 0, false);
                    if open && r.is_some() {
                        ui.label(
                            egui::RichText::new("読んでいます…")
                                .size(theme::fs(11.0))
                                .color(l.pal.ink3),
                        );
                    }
                }
            }
        }
        if let Place::Folder { project, folder } = self.place.clone() {
            ui.add_space(6.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.new_folder)
                    .hint_text("新しいフォルダの名前")
                    .desired_width(f32::INFINITY),
            );
            ui.ctx()
                .accesskit_node_builder(r.id, |b| b.set_label("新しいフォルダの名前"));
            let r = r.on_hover_text(format!(
                "Enter で「{}」の中に作ります(先頭に / を付けるとプロジェクトのいちばん上に)",
                file_name(&folder)
            ));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let name = self.new_folder.trim().to_string();
                let (parent, name) = match name.strip_prefix('/') {
                    Some(n) => (project.clone(), n.trim().to_string()),
                    None => (folder.clone(), name),
                };
                if !name.is_empty() {
                    host.request(HomeRequest::NewFolder { parent, name });
                    self.new_folder.clear();
                }
            }
        }
        self.left_tidy(ui, host);
        ui.add_space(12.0);
    }

    /// フォルダ `f` の行と、開いていれば下のフォルダの行。
    fn tree_rows(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut HomeHost<'_>,
        project: &Path,
        f: &Folder,
        depth: usize,
    ) {
        let has_children = !f.folders.is_empty();
        self.tree_row(
            ui,
            host,
            project,
            &f.path,
            f.name.clone(),
            Some(f.model_count()),
            depth,
            has_children,
        );
        if (depth == 0 || has_children) && self.expanded.contains(&f.path) {
            for c in &f.folders {
                self.tree_rows(ui, host, project, c, depth + 1);
            }
        }
    }

    /// 木の行 1 つ(押すと開く。三角で下を開閉。右クリックはフォルダの操作)。
    /// 返すのは行の反応(押した時は場所を替えた後)。
    #[allow(clippy::too_many_arguments)]
    fn tree_row(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut HomeHost<'_>,
        project: &Path,
        path: &Path,
        name: String,
        count: Option<usize>,
        depth: usize,
        has_children: bool,
    ) -> Option<egui::Response> {
        let on = matches!(&self.place, Place::Folder { folder, .. } if folder == path);
        let root = depth == 0;
        let indent = depth as f32 * 14.0;
        let open = self.expanded.contains(path);
        let r = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(indent);
                // 三角(プロジェクトはいつも、フォルダは下がある時だけ)
                let (tri, tr) = ui.allocate_exact_size(
                    egui::vec2(14.0, 26.0 * theme::band()),
                    egui::Sense::click(),
                );
                if root || has_children {
                    let l = theme::look(ui.ctx());
                    let c = tri.center();
                    let s = 3.5;
                    let pts = if open {
                        vec![
                            c + egui::vec2(-s, -s * 0.5),
                            c + egui::vec2(s, -s * 0.5),
                            c + egui::vec2(0.0, s * 0.7),
                        ]
                    } else {
                        vec![
                            c + egui::vec2(-s * 0.5, -s),
                            c + egui::vec2(s * 0.7, 0.0),
                            c + egui::vec2(-s * 0.5, s),
                        ]
                    };
                    ui.painter().add(egui::Shape::convex_polygon(
                        pts,
                        l.pal.ink2,
                        egui::Stroke::NONE,
                    ));
                    if tr
                        .on_hover_text(if open { "閉じる" } else { "開く" })
                        .clicked()
                    {
                        if open {
                            self.expanded.remove(path);
                        } else {
                            self.expanded.insert(path.to_path_buf());
                        }
                    }
                }
                place_row(ui, if root { "lib" } else { "folder" }, &name, count, on)
            })
            .inner;
        let n = count.unwrap_or(0);
        if let Some(designs) = drop_target(ui, &r, path, n) {
            host.request(HomeRequest::Move {
                designs,
                to: path.to_path_buf(),
            });
        }
        let r = r.on_hover_text(path.display().to_string());
        r.right_menu(|ui| self.folder_menu(ui, host, project, path));
        if r.clicked() {
            self.go(Place::Folder {
                project: project.to_path_buf(),
                folder: path.to_path_buf(),
            });
            return None;
        }
        Some(r)
    }
}

/// フォルダの木を、`path` から下の深さつきの一覧にする(「移す」の行き先の一覧に使う)。
pub(crate) fn flat(f: &Folder, depth: usize, out: &mut Vec<(PathBuf, String, usize)>) {
    out.push((f.path.clone(), f.name.clone(), depth));
    for c in &f.folders {
        flat(c, depth + 1, out);
    }
}
