//! 文書ごとの見え方(ブラウザの木の開け閉め・畳んだグループ)を覚え、文書を開き直した時に戻す
//! (10-25 u1)。置き場所はデータフォルダ(`kmb_session::doc_view`)。目の開け閉めは文書の区画に
//! 保存するので、ここでは扱わない。安全モードとデータフォルダの無い時は読みも書きもしない。

use crate::Workspace;
use kmb_session::doc_view::{DocView, DocViews};

impl Workspace {
    /// 今の文書の見え方。
    pub(crate) fn doc_view(&self) -> DocView {
        let c = &self.cur;
        let mut collapsed: Vec<u64> = c.collapsed.iter().copied().collect();
        collapsed.sort();
        DocView {
            open: c.tree_open.clone(),
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
        self.cur.collapsed = v.collapsed.iter().copied().collect();
        self.refresh_hidden();
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
