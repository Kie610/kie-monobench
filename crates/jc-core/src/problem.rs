//! 誤りの種類。画面の無い道(CLI・MCP)が、誤りを文の字で見分けずに種類で扱えるようにする
//! (CLI の終了の番号も、壊れた入力と加工の食い込みを分けて返せる)。
//!
//! 拡張点の操作は今も `Result<_, String>` で返す。種類は画面の無い道の入り口(jc-session の
//! `Headless::run`・`write_output`、CLI)で決め、文字列から作った物は [`ProblemKind::Input`] にする。

use serde::{Deserialize, Serialize};

/// 誤りの種類。CLI の終了の番号と MCP の答えの `kind` になる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// 入力の誤り(引数・式・文書の中身・知らない操作)。終了の番号 1
    Input,
    /// 再計算が誤りで止まった(誤りの機能がある)。終了の番号 2
    Compute,
    /// 確かめで問題が見つかった(シミュレーションの食い込みなど)。終了の番号 3
    Found,
    /// 今の文脈では使えない操作(画面ならボタンを押せない物)。終了の番号 4
    Unavailable,
    /// ファイルを読めない・書けない。終了の番号 5
    File,
}

impl ProblemKind {
    /// CLI の終了の番号。
    pub fn exit_code(self) -> u8 {
        match self {
            ProblemKind::Input => 1,
            ProblemKind::Compute => 2,
            ProblemKind::Found => 3,
            ProblemKind::Unavailable => 4,
            ProblemKind::File => 5,
        }
    }
}

/// 種類つきの誤り。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub kind: ProblemKind,
    pub message: String,
}

impl Problem {
    pub fn new(kind: ProblemKind, message: impl Into<String>) -> Self {
        Problem {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Problem {}

/// 種類の分からない文字列の誤りは入力の誤り。
impl From<String> for Problem {
    fn from(message: String) -> Self {
        Problem::new(ProblemKind::Input, message)
    }
}

impl From<&str> for Problem {
    fn from(message: &str) -> Self {
        Problem::new(ProblemKind::Input, message)
    }
}

/// 文字列で返す所(拡張点・画面)へは文だけを渡す。
impl From<Problem> for String {
    fn from(p: Problem) -> Self {
        p.message
    }
}
