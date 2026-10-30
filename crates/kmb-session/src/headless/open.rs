//! ファイルを、新しい文書として開く準備(読み取れた時だけ画面のタブへ渡す)。

use super::{Handled, Headless};
use kmb_core::{Document, DocumentData, Input, Modules, Problem, ProblemKind};
use std::path::{Path, PathBuf};

/// 読み取れた文書と、その開き方。まだカーネルの再計算や、今の文書の入れ替えはしない。
pub struct OpenedDocument {
    /// 読み取った文書。挿入から作った時は、その変更を 1 回で取り消せる。
    pub document: Document,
    /// 文書の保存先。外の図面から作った新しい設計は、名前を付けて保存する。
    pub path: Option<PathBuf>,
    /// 保存先の無い文書の表示名(元のファイル名から拡張子を除いたもの)。
    pub label: Option<String>,
    /// 新しい設計に読み込む前の保存基準。文書をそのまま開いた時は None で、
    /// 版の移行や寸法名の整備後の文書を保存基準にする。
    pub saved: Option<DocumentData>,
}

impl Headless {
    /// ファイルから新しい文書を作る。DXF は、その挿入操作と同じ宣言・確かめ・変更を通し、
    /// `blank`(新しい文書の既定の属性を持つ空文書)へ読み込む。文書の形式なら従来通り読む。
    /// 読み取りが失敗しても今の文書と形を変えないので、呼ぶ側は成功した後でタブを作れる。
    pub fn prepare_open(
        modules: Modules,
        path: &Path,
        blank: Document,
    ) -> Result<OpenedDocument, Problem> {
        let dxf = path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x.eq_ignore_ascii_case("dxf"));
        if !dxf {
            return Ok(OpenedDocument {
                document: crate::load(path)?,
                path: Some(path.to_path_buf()),
                label: None,
                saved: None,
            });
        }
        if !blank.data().features.is_empty()
            || !blank.data().params.is_empty()
            || blank.can_undo()
            || blank.can_redo()
        {
            return Err(Problem::new(
                ProblemKind::Input,
                "新しい設計に読み込むには、空の文書を指定してください",
            ));
        }
        let saved = blank.data().clone();
        // 空文書を計算すると、共有カーネルが今の文書の形まで捨てる。読み取る段階では
        // 計算を後へ回し、通常の操作を通した変更だけを取り出す(ここでは解除しない)。
        let mut work = Self::uncomputed(modules, blank, None);
        work.hold_recompute(true);
        let action = work
            .action_for_role(&kmb_interchange::role::action::INSERT_DXF)
            .map_err(|why| Problem::new(ProblemKind::Unavailable, why))?;
        let declaration = work
            .actions(None)
            .into_iter()
            .find(|a| a.id == action)
            .ok_or_else(|| {
                Problem::new(ProblemKind::Unavailable, "DXF の挿入操作が見つかりません")
            })?;
        if !matches!(&declaration.input, Input::OpenFile { extensions, .. } if extensions.iter().any(|x| x.eq_ignore_ascii_case("dxf")))
        {
            return Err(Problem::new(
                ProblemKind::Unavailable,
                "DXF の挿入操作が、この形式の読み込みを宣言していません",
            ));
        }
        let result = work.run_with(
            &action,
            &super::Picks::default(),
            Some(path),
            &serde_json::Value::Null,
        )?;
        if !matches!(result, Handled::Changed(ref added) if !added.is_empty()) {
            return Err(Problem::new(
                ProblemKind::Input,
                "DXF に新しい設計へ読み込める形がありません",
            ));
        }
        Ok(OpenedDocument {
            document: work.doc,
            path: None,
            label: path.file_stem().map(|n| n.to_string_lossy().into_owned()),
            saved: Some(saved),
        })
    }
}
