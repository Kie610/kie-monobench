//! 誤りの種類。研究や操作の機能が `Problem` へ写せるよう、入力・計算・使えないの 3 つに分ける。

use std::fmt;

/// 誤りの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// 手順やデータの値が受け取れない(格子が違う、長さが合わない、知らない名前など)。
    Input,
    /// 計算の途中で値が壊れた(無限大・非数、float16 に収まらない値)。
    Compute,
    /// 形としては正しいが、この道具ではまだ扱えない。
    Unsupported,
}

/// HRTF の処理の誤り。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HrtfError {
    pub kind: ErrorKind,
    pub message: String,
}

impl HrtfError {
    pub fn input(message: impl Into<String>) -> HrtfError {
        HrtfError {
            kind: ErrorKind::Input,
            message: message.into(),
        }
    }

    pub fn compute(message: impl Into<String>) -> HrtfError {
        HrtfError {
            kind: ErrorKind::Compute,
            message: message.into(),
        }
    }

    pub fn unsupported(message: impl Into<String>) -> HrtfError {
        HrtfError {
            kind: ErrorKind::Unsupported,
            message: message.into(),
        }
    }

    /// 文の頭に場所(手順の何段目か)を足す。種類は変えない。
    pub fn at(self, place: &str) -> HrtfError {
        HrtfError {
            kind: self.kind,
            message: format!("{place}: {}", self.message),
        }
    }
}

impl fmt::Display for HrtfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HrtfError {}
