//! パラメータ式の構文解析と評価。
//!
//! 内部の単位は長さが mm、角度が rad に固定である。式の中では `10 mm`、`2 cm`、
//! `30 deg` のように単位を書ける。単位は直前の掛け算・割り算の並び全体に掛かる
//! (`PI/4 rad`、`1/2 in`)。大かっこで囲んだ単位は直前の 1 つだけに掛かる
//! (`1/2 [in]` は 1 ÷ 2 in、`(1/2) [in]` は 12.7 mm)。単位を書かない数値は無次元として扱う。
//! 定数は `PI`・`E`、関数の引数は `pow(a; b)` のように `;` でも区切れる。
//! 文字の中に値を入れる差し込みの文字は [`Template`](`M3-{長さ}`)。

mod ast;
mod lexer;
mod parser;
mod quantity;
mod template;
mod units;

pub use ast::{BinOp, Expr};
pub use quantity::{Dim, MAX_ANGLE_DEG, MAX_LENGTH_MM, Quantity};
pub use template::{
    Template, Value, escape_template, format_number, inline_in_template, rename_in_template,
};
pub use units::{LengthUnit, display_unit, set_display_unit};

use std::fmt;

/// 式の誤り。`pos` は入力文字列の中のバイト位置。
#[derive(Debug, Clone, PartialEq)]
pub enum ExprError {
    Syntax {
        pos: usize,
        message: String,
    },
    UnknownName(String),
    UnknownFunction(String),
    Arity {
        function: String,
        expected: usize,
        got: usize,
    },
    DimensionMismatch {
        op: String,
        left: Dim,
        right: Dim,
    },
    Domain(String),
    /// 数の式の中に文字のパラメータを書いた(名前)
    TextParam(String),
    /// 差し込みの文字の誤り(文はそのまま見せる)
    Text(String),
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExprError::Syntax { pos, message } => {
                write!(f, "構文の誤り({pos} 文字目付近): {message}")
            }
            ExprError::UnknownName(n) => write!(f, "未定義の名前: {n}"),
            ExprError::UnknownFunction(n) => write!(f, "未定義の関数: {n}"),
            ExprError::Arity {
                function,
                expected,
                got,
            } => {
                write!(
                    f,
                    "{function} の引数は {expected} 個ですが {got} 個あります"
                )
            }
            ExprError::DimensionMismatch { op, left, right } => {
                write!(f, "単位の次元が合いません: {left} {op} {right}")
            }
            ExprError::Domain(m) => write!(f, "値の範囲の誤り: {m}"),
            ExprError::TextParam(n) => write!(f, "文字のパラメータ {n} は計算に使えません"),
            ExprError::Text(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ExprError {}

/// 単位として予約された名前。パラメータ名には使えない。単位を足すと、その名前のパラメータを
/// 持つ前の文書は、開く時に「単位の名前と同じ」の誤りで止まる(黙って単位として読まない)。
pub const UNIT_NAMES: &[&str] = &["mm", "cm", "m", "in", "ft", "deg", "rad"];
/// 組み込み関数と定数の名前。パラメータ名には使えない。
pub const BUILTIN_NAMES: &[&str] = &[
    "pi", "PI", "π", "E", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh",
    "tanh", "asinh", "acosh", "atanh", "sqrt", "abs", "min", "max", "floor", "ceil", "round",
    "exp", "ln", "log", "pow", "sign",
];

/// パラメータ名として使えるかを返す。
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_alphabetic() || first == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && !UNIT_NAMES.contains(&name)
        && !BUILTIN_NAMES.contains(&name)
}

/// 文字列を構文解析する。
pub fn parse(src: &str) -> Result<Expr, ExprError> {
    let tokens = lexer::lex(src)?;
    parser::Parser::new(&tokens, src.len()).parse()
}

/// 式の中のパラメータ名 `old` を `new` に置き換える。空白や書式はそのまま残す。
pub fn rename_reference(src: &str, old: &str, new: &str) -> Result<String, ExprError> {
    let tokens = lexer::lex(src)?;
    let mut out = String::with_capacity(src.len());
    let mut last = 0;
    for (i, (tok, pos)) in tokens.iter().enumerate() {
        let is_call = matches!(tokens.get(i + 1), Some((lexer::Tok::LParen, _)));
        if let lexer::Tok::Ident(name) = tok
            && name == old
            && !is_call
        {
            out.push_str(&src[last..*pos]);
            out.push_str(new);
            last = pos + name.len();
        }
    }
    out.push_str(&src[last..]);
    Ok(out)
}

/// 文字列を構文解析して評価する。`lookup` はパラメータ名から値を返す。
pub fn eval_str(
    src: &str,
    lookup: &dyn Fn(&str) -> Option<Quantity>,
) -> Result<Quantity, ExprError> {
    parse(src)?.eval(lookup)
}

#[cfg(test)]
mod tests;
