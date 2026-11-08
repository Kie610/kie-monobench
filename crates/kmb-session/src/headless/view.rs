//! 文書を変更せず、文書ごとの見え方を同じ宣言から切り替える。

use super::{Handled, Headless, Picks};
use crate::doc_view::{DocView, DocViews};
use kmb_core::{Context, Extension, Extensions, Outcome, Problem, ProblemKind};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct Commands(Mutex<bool>);

impl Extension for Commands {
    fn actions(&self, _: &Context<'_>) -> Vec<kmb_core::Action> {
        crate::doc_view::actions()
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        if !crate::doc_view::is_action(id) {
            return None;
        }
        let mut on = self.0.lock().expect("見え方の値");
        *on = cx.arg("on").and_then(|v| v.as_bool()).unwrap_or(!*on);
        Some(Ok(Outcome::Message(if *on {
            "部品ごとの色を入れました".into()
        } else {
            "部品ごとの色を切りました".into()
        })))
    }
}

impl Headless {
    /// 今の文書の見え方。文書データや取り消しの履歴には含めない。
    pub fn doc_view(&self) -> &DocView {
        &self.view
    }

    /// 見え方の保存先。画面は安全モードやデータフォルダの無い時に None を渡す。
    pub fn set_view_directory(&mut self, directory: Option<PathBuf>) {
        self.view_directory = Some(directory);
        self.restore_view();
    }

    /// ブラウザなどの現在の見え方を同期する。ここでは保存しない。
    pub fn sync_doc_view(&mut self, view: DocView) {
        self.view = view;
    }

    /// 見え方を保存する。保存に失敗した時は、現在の値も変えない。
    pub fn save_doc_view(&mut self, view: DocView) -> Result<(), String> {
        if let (Some(Some(dir)), Some(path)) = (&self.view_directory, &self.path) {
            let mut all = DocViews::load(dir);
            all.set(path, view.clone());
            all.save(dir)?;
        }
        self.view = view;
        Ok(())
    }

    pub(super) fn restore_view(&mut self) {
        self.view = match (&self.view_directory, &self.path) {
            (Some(Some(dir)), Some(path)) => {
                DocViews::load(dir).get(path).cloned().unwrap_or_default()
            }
            _ => DocView::default(),
        };
    }

    pub(super) fn run_view(
        &mut self,
        id: &str,
        picks: &Picks<'_>,
        args: &serde_json::Value,
    ) -> Result<Handled, Problem> {
        // 画面は保存先を明示する。CLI・MCP は最初の表示操作で PC の保存先を使う。
        if self.view_directory.is_none() {
            self.set_view_directory(crate::default_data_dir());
        }
        let command = Arc::new(Commands(Mutex::new(self.view.part_hues)));
        let extensions = Extensions(vec![command.clone()]);
        let outcome = extensions.run(id, &self.context_with(picks, None, args))?;
        let mut view = self.view.clone();
        view.part_hues = *command.0.lock().expect("見え方の値");
        self.save_doc_view(view)
            .map_err(|why| Problem::new(ProblemKind::File, why))?;
        Ok(Handled::Other(Box::new(outcome)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::{Document, Modules, NullKernel};
    use serde_json::json;

    fn work(path: Option<PathBuf>) -> Headless {
        Headless::with_document(
            Modules::assemble(vec![], Arc::new(NullKernel)),
            Document::new(),
            path,
        )
    }

    #[test]
    fn view_action_shares_argument_checks_and_leaves_the_document_alone() {
        let mut h = work(None);
        h.set_view_directory(None);
        let before = h.doc().to_json();
        let count = h.recomputes();
        let action = crate::doc_view::PART_HUES;
        assert!(h.actions(None).iter().any(|a| a.id == action));
        h.run(action, None, None, &json!({"on":true})).unwrap();
        assert!(h.doc_view().part_hues);
        for args in [json!({"unknown":true}), json!({"on":"true"})] {
            assert_eq!(
                h.run(action, None, None, &args).unwrap_err().kind,
                ProblemKind::Input
            );
            assert!(h.doc_view().part_hues);
        }
        h.run(action, None, None, &json!({})).unwrap();
        assert!(!h.doc_view().part_hues);
        assert_eq!(h.doc().to_json(), before);
        assert!(!h.doc().can_undo());
        assert_eq!(h.recomputes(), count);
    }

    #[test]
    fn view_action_keeps_unknown_fields_and_separate_document_paths() {
        let dir = std::env::temp_dir().join(format!("kmb-part-hues-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.kmb"), dir.join("b.kmb"));
        let old = json!({"future_root":3,"docs":[{"path":a,"future_entry":4,"collapsed":[7]}]});
        std::fs::write(dir.join("doc-views.json"), old.to_string()).unwrap();
        let mut h = work(Some(a.clone()));
        h.set_view_directory(Some(dir.clone()));
        h.run(crate::doc_view::PART_HUES, None, None, &json!({"on":true}))
            .unwrap();
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("doc-views.json")).unwrap())
                .unwrap();
        assert_eq!(back["future_root"], 3);
        assert_eq!(back["docs"][0]["future_entry"], 4);
        assert_eq!(h.doc_view().collapsed, [7]);
        let mut again = work(Some(a));
        again.set_view_directory(Some(dir.clone()));
        assert!(again.doc_view().part_hues);
        again.replace(Document::new(), Some(b));
        assert!(!again.doc_view().part_hues);
        let mut unsaved = work(None);
        unsaved.set_view_directory(Some(dir.clone()));
        unsaved
            .run(crate::doc_view::PART_HUES, None, None, &json!({"on":true}))
            .unwrap();
        unsaved.set_path(Some(dir.join("new.kmb")));
        let saved = DocViews::load(&dir);
        assert!(saved.get(&dir.join("new.kmb")).unwrap().part_hues);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
