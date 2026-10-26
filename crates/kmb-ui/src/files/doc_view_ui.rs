//! 文書ごとの見え方(ブラウザの木の開け閉め・原点とまとまりの目・行とボディの目・畳んだグループ)を
//! 覚え、文書を開き直した時に戻す(10-25 u1)。置き場所はデータフォルダ(`kmb_session::doc_view`)。
//! 安全モードとデータフォルダの無い時は読みも書きもしない。

use crate::{Workspace, browser_ui};
use kmb_core::FeatureId;
use kmb_session::doc_view::{DocView, DocViews};
use std::collections::HashSet;

/// 覚えるまとまりの目
const FOLDERS: [browser_ui::Kind; 2] = [browser_ui::Kind::Sketch, browser_ui::Kind::Body];

fn sorted<'a>(ids: impl Iterator<Item = &'a FeatureId>) -> Vec<FeatureId> {
    let mut v: Vec<FeatureId> = ids.copied().collect();
    v.sort();
    v
}

impl Workspace {
    /// 今の文書の見え方。
    pub(crate) fn doc_view(&self) -> DocView {
        let c = &self.cur;
        let mut collapsed: Vec<u64> = c.collapsed.iter().copied().collect();
        collapsed.sort();
        DocView {
            open: c.tree_open.clone(),
            origin: c.origin_shown,
            hidden_folders: FOLDERS
                .iter()
                .filter(|k| c.hidden_folders.contains(k))
                .map(|k| format!("{k:?}"))
                .collect(),
            hidden: sorted(c.hidden.difference(&c.auto_hidden)),
            shown: sorted(c.used_sketches.difference(&c.hidden)),
            hidden_bodies: sorted(c.hidden_bodies.iter()),
            collapsed,
        }
    }

    /// 文書を入れ替えた後に、その文書の前の見え方に戻す(覚えていなければ既定)。
    pub(crate) fn restore_doc_view(&mut self) {
        let v = match (
            self.win.data_dir.as_ref().filter(|_| !self.win.safe_mode),
            self.cur.work.path(),
        ) {
            (Some(dir), Some(path)) => DocViews::load(dir).get(path).cloned().unwrap_or_default(),
            _ => DocView::default(),
        };
        self.cur.tree_open = v.open.clone();
        self.cur.origin_shown = v.origin;
        self.cur.hidden_folders = FOLDERS
            .into_iter()
            .filter(|k| v.hidden_folders.contains(&format!("{k:?}")))
            .collect();
        self.cur.hidden_bodies = v.hidden_bodies.iter().copied().collect();
        self.cur.collapsed = v.collapsed.iter().copied().collect();
        // 使われたスケッチを先に自動で隠し、目を開けていた物だけ出し直す
        self.hide_used_sketches();
        let shown: HashSet<FeatureId> = v.shown.iter().copied().collect();
        self.cur.auto_hidden.retain(|id| !shown.contains(id));
        let mut hidden = self.cur.hidden.clone();
        hidden.extend(v.hidden.iter().copied());
        hidden.retain(|id| !shown.contains(id));
        self.set_all_hidden(hidden);
        self.cur.view_saved = self.doc_view();
    }

    /// 見え方が変わっていればデータフォルダへ書く(保存先のある文書だけ。ボタンを離している時に)。
    pub(crate) fn save_doc_view_if_changed(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        self.save_doc_view();
    }

    /// 見え方が変わっていればデータフォルダへ書く。
    pub(crate) fn save_doc_view(&mut self) {
        let (Some(dir), Some(path)) = (
            self.win.data_dir.clone().filter(|_| !self.win.safe_mode),
            self.cur.work.path().map(std::path::Path::to_path_buf),
        ) else {
            return;
        };
        let now = self.doc_view();
        if now == self.cur.view_saved {
            return;
        }
        let mut all = DocViews::load(&dir);
        all.set(&path, now.clone());
        if let Err(e) = all.save(&dir) {
            self.win.fail(format!("文書の見え方を書けません: {e}"));
        }
        self.cur.view_saved = now;
    }
}
