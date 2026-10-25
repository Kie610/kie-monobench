use crate::ExprError;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
    /// 単位をはっきり書く大かっこ(`5 [mm]`)
    LBracket,
    RBracket,
    Comma,
}

/// 全角の数字・小数点・符号を半角に読む(日本語の入力のまま打った数も普通の数として受ける)。
/// 名前の文字は変えない(全角の名前のパラメータは名前のまま)。
pub(crate) fn half_width(c: char) -> char {
    match c {
        '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
        '．' => '.',
        '＋' => '+',
        '－' | '−' => '-',
        _ => c,
    }
}

/// `c` が先頭の小数のカンマ(`,5`)か。すぐ後ろが数字で、前が値の終わり(数・名前・閉じかっこ)で
/// ない時。小数は点で書く決まりなので、0.5 と読まずに誤りにする(`1,5` が誤りなのとそろえる)。
/// `max(1,5)` のカンマは引数の区切りのまま。
fn leading_comma(before: &[(Tok, usize)], rest: &str, c: char) -> bool {
    c == ','
        && rest.starts_with(|d: char| half_width(d).is_ascii_digit())
        && !matches!(
            before.last(),
            Some((Tok::Num(_) | Tok::Ident(_) | Tok::RParen | Tok::RBracket, _))
        )
}

/// トークンと、その開始位置(バイト)。
pub(crate) fn lex(src: &str) -> Result<Vec<(Tok, usize)>, ExprError> {
    let mut out = Vec::new();
    let mut it = src
        .char_indices()
        .map(|(i, c)| (i, half_width(c)))
        .peekable();
    while let Some(&(i, c)) = it.peek() {
        let raw = src[i..].chars().next().map_or(1, char::len_utf8);
        if c.is_whitespace() {
            it.next();
        } else if leading_comma(&out, &src[i + raw..], c) {
            return Err(ExprError::Syntax {
                pos: i,
                message: "小数は点で書いてください(0.5)".into(),
            });
        } else if c.is_ascii_digit() || c == '.' {
            let mut text = String::new();
            let mut seen_exp = false;
            while let Some(&(_, d)) = it.peek() {
                let exp_sign = seen_exp && (d == '+' || d == '-') && text.ends_with(['e', 'E']);
                if d.is_ascii_digit() || d == '.' || exp_sign {
                    text.push(d);
                    it.next();
                } else if (d == 'e' || d == 'E') && !seen_exp && {
                    let mut ahead = it.clone();
                    ahead.next();
                    exponent_follows(ahead.map(|(_, c)| c))
                } {
                    seen_exp = true;
                    text.push(d);
                    it.next();
                } else {
                    break;
                }
            }
            let v: f64 = text.parse().map_err(|_| ExprError::Syntax {
                pos: i,
                message: format!("数値として読めません: {text}"),
            })?;
            out.push((Tok::Num(v), i));
        } else if c.is_alphabetic() || c == '_' {
            let mut end = i;
            while let Some(&(j, d)) = it.peek() {
                if d.is_alphanumeric() || d == '_' {
                    end = j + src[j..].chars().next().map_or(1, char::len_utf8);
                    it.next();
                } else {
                    break;
                }
            }
            out.push((Tok::Ident(src[i..end].to_string()), i));
        } else {
            let tok = match c {
                '+' | '-' | '*' | '/' | '^' => Tok::Op(c),
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                '[' => Tok::LBracket,
                ']' => Tok::RBracket,
                // 引数は `;` でも区切れる(`pow(3; 2)`)。`,` も使える
                ',' | ';' => Tok::Comma,
                _ => {
                    return Err(ExprError::Syntax {
                        pos: i,
                        message: format!("使えない文字: {c}"),
                    });
                }
            };
            out.push((tok, i));
            it.next();
        }
    }
    Ok(out)
}

/// `e` の後ろが指数部(数字、または符号と数字)か。`2em` のような単位の誤読を避ける。
fn exponent_follows(mut cs: impl Iterator<Item = char>) -> bool {
    match cs.next() {
        Some(d) if d.is_ascii_digit() => true,
        Some('+' | '-') => cs.next().is_some_and(|d| d.is_ascii_digit()),
        _ => false,
    }
}
