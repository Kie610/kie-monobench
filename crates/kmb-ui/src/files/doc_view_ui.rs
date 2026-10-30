//! 文書ごとの見え方(ブラウザの木の開け閉め・畳んだグループ)を覚え、文書を開き直した時に戻す
//! (10-25 u1)。置き場所はデータフォルダ(`kmb_session::doc_view`)。目の開け閉めは文書の区画に
//! 保存するので、ここでは扱わない。安全モードとデータフォルダの無い時は読みも書きもしない。

use crate::Workspace;
use kmb_session::doc_view::DocView;

impl Workspace {
    /// 今の文書の見え方。
    pub(crate) fn doc_view(&self) -> DocView {
        let c = &self.cur;
        let mut collapsed: Vec<u64> = c.collapsed.iter().copied().collect();
        collapsed.sort();
        // ひとりでに開いた枝は閉じたまま覚える(開き直しても開かない。覚え直しも要らない)
        let mut open = c.tree_open.clone();
        for k in &c.tree_auto {
            open.insert(k.clone(), false);
        }
        let mut view = c.work.doc_view().clone();
        view.open = open;
        view.collapsed = collapsed;
        view
    }

    /// 文書を入れ替えた後に、その文書の前の見え方に戻す(覚えていなければ既定)。
    pub(crate) fn restore_doc_view(&mut self) {
        let directory = self.win.data_dir.clone().filter(|_| !self.win.safe_mode);
        self.cur.work.set_view_directory(directory);
        let v = self.cur.work.doc_view().clone();
        self.apply_doc_view(v);
    }

    /// 退避した文書へ戻す時は、未保存の見え方もそのまま戻す。
    pub(crate) fn apply_doc_view(&mut self, v: DocView) {
        self.cur.work.sync_doc_view(v.clone());
        self.cur.tree_open = v.open.clone();
        self.cur.tree_auto.clear();
        self.cur.collapsed = v.collapsed.iter().copied().collect();
        self.refresh_hidden();
        self.refresh_body_colors();
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
        if self.win.safe_mode || self.win.data_dir.is_none() || self.cur.work.path().is_none() {
            return;
        }
        let now = self.doc_view();
        if now == self.cur.view_saved {
            return;
        }
        if let Err(e) = self.cur.work.save_doc_view(now.clone()) {
            self.win.fail(format!("文書の見え方を書けません: {e}"));
            return;
        }
        self.cur.view_saved = now;
    }
}
