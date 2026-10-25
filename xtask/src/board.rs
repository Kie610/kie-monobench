//! 画板の並びの点検: 画板「JC の画面」のツールバーと実物の既知の差の表
//! (`crates/kmb-ui/board/known-diffs.txt`)は、減る向きにしか変えない(2026-10-22 Kie)。
//!
//! 実物と画板の比べ方は kmb-ui の `board` と、その試験(設計・加工は kmb-ui、スケッチは kmb-ui-sketch)。
//! ここでは分かれ目の版と比べ、差の行が増えていたら落とす。画板のデータ
//! (`crates/kmb-ui/board/toolbar.json`)を直した時だけは、新しく決めた物を差に書いてよい。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub const BOARD: &str = "crates/kmb-ui/board/toolbar.json";
pub const KNOWN: &str = "crates/kmb-ui/board/known-diffs.txt";

/// 表の差の行(「差 # 理由」の差の所)。
fn rows(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" # ").next().unwrap_or(l).trim().to_string())
        .collect()
}

/// `read` は分かれ目の版のファイルの中身(無ければ `None`)。
pub fn check(root: &Path, read: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let lf = |s: String| s.replace("\r\n", "\n");
    let now_board = fs::read_to_string(root.join(BOARD)).ok().map(lf);
    if read(BOARD).map(lf) != now_board {
        return vec![];
    }
    let old = rows(&read(KNOWN).map(lf).unwrap_or_default());
    let now = rows(
        &fs::read_to_string(root.join(KNOWN))
            .map(lf)
            .unwrap_or_default(),
    );
    now.difference(&old)
        .map(|r| {
            format!(
                "{KNOWN} に差「{r}」が増えた。既知の差の表は減らすだけ(画板のデータ {BOARD} を\
                 直す時のほかは増やさない)。実物を画板に合わせる"
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(base_board: &str, base_known: &str, now_board: &str, now_known: &str) -> usize {
        let dir = std::env::temp_dir().join(format!("kmb-board-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("crates/kmb-ui/board")).unwrap();
        fs::write(dir.join(BOARD), now_board).unwrap();
        fs::write(dir.join(KNOWN), now_known).unwrap();
        let (bb, bk) = (base_board.to_string(), base_known.to_string());
        let read = move |f: &str| Some(if f == BOARD { bb.clone() } else { bk.clone() });
        let n = check(&dir, &read).len();
        let _ = fs::remove_dir_all(&dir);
        n
    }

    #[test]
    fn the_known_differences_only_shrink() {
        let known = "# 見出し\n設計/挿入: A # 理由\n設計/作成: B # 理由\n";
        // 減らすのはよい。理由だけ直すのもよい
        assert_eq!(run("{}", known, "{}", "設計/挿入: A # 別の理由\n"), 0);
        // 増やすと落ちる(CRLF でも同じ)
        assert_eq!(
            run(
                "{}",
                known,
                "{}",
                &format!("{known}加工/2D: C # 理由\n").replace('\n', "\r\n")
            ),
            1
        );
        // 画板のデータを直した時は増やしてよい
        assert_eq!(
            run("{}", known, "{\"v\":2}", &format!("{known}加工/2D: C\n")),
            0
        );
    }
}
