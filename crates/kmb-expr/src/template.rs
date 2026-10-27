//! 差し込みの文字(文字の中の `{式}`・`{式; 桁}` に値を入れる)。`{{`・`}}` は字の `{`・`}`。
//! 全角の `｛｝` は字のまま。中身がパラメータの名前 1 つで、その値が文字なら、その文字を入れる。
//! ほかは数の式として評価し、[`format_number`] で文字にする。

use crate::{Dim, Expr, ExprError, LengthUnit, Quantity};
use std::ops::Range;

/// 差し込みに入れる値(数か文字)。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(Quantity),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Lit(String),
    Slot {
        expr: Expr,
        /// 式の部分の、元の文字の中の位置(名前の書き換えに使う)
        span: Range<usize>,
        digits: Option<u8>,
    },
}

/// 差し込みの文字を読んだもの。
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pieces: Vec<Piece>,
}

/// 桁に書ける最大。
const MAX_DIGITS: u8 = 6;

fn syntax(pos: usize, message: impl Into<String>) -> ExprError {
    ExprError::Syntax {
        pos,
        message: message.into(),
    }
}

/// 中身の最後の、かっこの外の `;` の位置(関数の引数の `;` はかっこの中)。
fn digits_split(inner: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut at = None;
    for (i, c) in inner.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ';' if depth == 0 => at = Some(i),
            _ => {}
        }
    }
    at
}

impl Template {
    pub fn parse(src: &str) -> Result<Template, ExprError> {
        let mut pieces = Vec::new();
        let mut lit = String::new();
        let mut it = src.char_indices().peekable();
        while let Some((i, c)) = it.next() {
            match c {
                '{' if it.peek().is_some_and(|&(_, d)| d == '{') => {
                    it.next();
                    lit.push('{');
                }
                '}' if it.peek().is_some_and(|&(_, d)| d == '}') => {
                    it.next();
                    lit.push('}');
                }
                '}' => {
                    return Err(syntax(
                        i,
                        "対になる { がありません(} を字として書くには }})",
                    ));
                }
                '{' => {
                    let start = i + 1;
                    let Some(len) = src[start..].find('}') else {
                        return Err(syntax(
                            i,
                            "対になる } がありません({ を字として書くには {{)",
                        ));
                    };
                    let inner = &src[start..start + len];
                    if let Some(j) = inner.find('{') {
                        return Err(syntax(
                            start + j,
                            "差し込みの中に { は書けません({ を字として書くには {{)",
                        ));
                    }
                    if inner.trim().is_empty() {
                        return Err(syntax(i, "差し込み {} の中が空です"));
                    }
                    let (expr_src, digits) = match digits_split(inner) {
                        Some(k) => {
                            let d = inner[k + 1..].trim();
                            match d.parse::<u8>() {
                                Ok(n) if n <= MAX_DIGITS => (&inner[..k], Some(n)),
                                _ => {
                                    return Err(syntax(
                                        start + k + 1,
                                        format!("桁は 0〜{MAX_DIGITS} の整数で書いてください"),
                                    ));
                                }
                            }
                        }
                        None => (inner, None),
                    };
                    let expr = crate::parse(expr_src).map_err(|e| match e {
                        ExprError::Syntax { pos, message } => syntax(start + pos, message),
                        e => e,
                    })?;
                    if !lit.is_empty() {
                        pieces.push(Piece::Lit(std::mem::take(&mut lit)));
                    }
                    pieces.push(Piece::Slot {
                        expr,
                        span: start..start + expr_src.len(),
                        digits,
                    });
                    // 閉じの `}` まで進める
                    while it.next_if(|&(j, _)| j < start + len + 1).is_some() {}
                }
                c => lit.push(c),
            }
        }
        if !lit.is_empty() {
            pieces.push(Piece::Lit(lit));
        }
        Ok(Template { pieces })
    }

    fn slots(&self) -> impl Iterator<Item = (&Expr, &Range<usize>, Option<u8>)> {
        self.pieces.iter().filter_map(|p| match p {
            Piece::Slot { expr, span, digits } => Some((expr, span, *digits)),
            Piece::Lit(_) => None,
        })
    }

    /// 差し込みがあるか。
    pub fn has_slots(&self) -> bool {
        self.slots().next().is_some()
    }

    /// 差し込みが参照するパラメータ名(出現順・重複なし)。
    pub fn references(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (e, _, _) in self.slots() {
            for n in e.references() {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
        out
    }

    /// 数の差し込みがあるか(あれば文書の長さの単位で文字が変わる)。名前 1 つだけの差し込みは、
    /// `is_text` がその名前を文字のパラメータと答えた時だけ数に数えない。
    pub fn has_numbers(&self, is_text: &dyn Fn(&str) -> bool) -> bool {
        self.slots().any(|(e, _, _)| match e {
            Expr::Ref(n) => !is_text(n),
            _ => true,
        })
    }

    /// 値を入れた文字。`lookup` は名前から数か文字を返す。長さは `unit` で書く。
    pub fn render(
        &self,
        lookup: &dyn Fn(&str) -> Option<Value>,
        unit: LengthUnit,
    ) -> Result<String, ExprError> {
        let mut out = String::new();
        for p in &self.pieces {
            match p {
                Piece::Lit(s) => out.push_str(s),
                Piece::Slot { expr, digits, .. } => {
                    if let Expr::Ref(n) = expr
                        && let Some(Value::Text(t)) = lookup(n)
                    {
                        if digits.is_some() {
                            return Err(ExprError::Text(format!(
                                "文字のパラメータ {n} に桁は書けません"
                            )));
                        }
                        out.push_str(&t);
                        continue;
                    }
                    for n in expr.references() {
                        if let Some(Value::Text(_)) = lookup(&n) {
                            return Err(ExprError::TextParam(n));
                        }
                    }
                    let q = expr.eval(&|n| match lookup(n) {
                        Some(Value::Num(q)) => Some(q),
                        _ => None,
                    })?;
                    out.push_str(&format_number(q, *digits, unit)?);
                }
            }
        }
        Ok(out)
    }
}

/// 数を文字にする(差し込みの決まり)。長さは文書の長さの単位 `unit` で書き、単位の字を付ける
/// (`25 mm`、in の文書なら `0.984 in`。長さの n 乗は `12 mm^2`)。角度は `30 deg`、単位なしは数だけ。
/// 桁が無ければ単位の画面の桁(角度は 2、単位なしは 6)で丸めて末尾の 0 と小数点を落とし、
/// 桁があればその桁で固定する。
pub fn format_number(
    q: Quantity,
    digits: Option<u8>,
    unit: LengthUnit,
) -> Result<String, ExprError> {
    if !q.value.is_finite() {
        return Err(ExprError::Text(
            "数になっていない値は文字にできません".into(),
        ));
    }
    let Dim { length, angle } = q.dim;
    let (v, suffix, default) = match (length, angle) {
        (0, 0) => (q.value, String::new(), 6),
        (0, a) => (
            q.value.to_degrees() * (180f64 / std::f64::consts::PI).powi(i32::from(a) - 1),
            power_suffix("deg", a),
            2,
        ),
        (l, 0) => (
            q.value / unit.mm().powi(i32::from(l)),
            power_suffix(unit.name(), l),
            unit.digits(),
        ),
        _ => {
            return Err(ExprError::Text(
                "長さと角度を掛けた値は文字にできません".into(),
            ));
        }
    };
    let s = match digits {
        Some(d) => format!("{v:.*}", usize::from(d)),
        None => {
            let s = format!("{v:.default$}");
            if s.contains('.') {
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            } else {
                s
            }
        }
    };
    let s = match s.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => s,
    };
    Ok(format!("{s}{suffix}"))
}

fn power_suffix(name: &str, n: i8) -> String {
    if n == 1 {
        format!(" {name}")
    } else {
        format!(" {name}^{n}")
    }
}

/// 字の `{`・`}` を `{{`・`}}` にする(前の形の文字を差し込みの文字にする時)。
pub fn escape_template(s: &str) -> String {
    s.replace('{', "{{").replace('}', "}}")
}

/// 差し込みの式 1 つ(読んだ式と元の字)から、書き換えた字(変えなければ `None`)を返す。
type SlotEdit<'a> = dyn FnMut(&Expr, &str) -> Result<Option<String>, ExprError> + 'a;

/// 差し込みの式の部分を `f` で書き換える(字の所と桁はそのまま)。
fn map_slots(src: &str, f: &mut SlotEdit) -> Result<String, ExprError> {
    let t = Template::parse(src)?;
    let mut out = String::with_capacity(src.len());
    let mut last = 0;
    for (e, span, _) in t.slots() {
        if let Some(new) = f(e, &src[span.clone()])? {
            out.push_str(&src[last..span.start]);
            out.push_str(&new);
            last = span.end;
        }
    }
    out.push_str(&src[last..]);
    Ok(out)
}

/// 差し込みの中のパラメータ名 `old` を `new` に替える(字の所はそのまま)。
pub fn rename_in_template(src: &str, old: &str, new: &str) -> Result<String, ExprError> {
    map_slots(src, &mut |e, s| {
        Ok(if e.references().iter().any(|n| n == old) {
            Some(crate::rename_reference(s, old, new)?)
        } else {
            None
        })
    })
}

/// 差し込み `{old}` を、消したパラメータの値 `with` に置き換える。`with_is_text` なら `with` は
/// 差し込みの文字で、`{old}` をそのまま `with` にする。数なら、式の中の `old` を `(with)` にする
/// (数のパラメータを消した時と同じ。数だけなら括弧を付けない)。
pub fn inline_in_template(
    src: &str,
    old: &str,
    with: &str,
    with_is_text: bool,
) -> Result<String, ExprError> {
    if with_is_text {
        let t = Template::parse(src)?;
        let mut out = String::with_capacity(src.len());
        let mut last = 0;
        for (e, span, _) in t.slots() {
            if matches!(e, Expr::Ref(n) if n == old) {
                // `{` と `}` も含めて置き換える(桁は文字のパラメータには書けない)
                let open = src[..span.start].rfind('{').unwrap_or(span.start);
                let close = span.end + src[span.end..].find('}').map_or(0, |k| k + 1);
                out.push_str(&src[last..open]);
                out.push_str(with);
                last = close;
            }
        }
        out.push_str(&src[last..]);
        return Ok(out);
    }
    let with = match crate::parse(with) {
        Ok(Expr::Num(_)) => with.trim().to_string(),
        _ => format!("({})", with.trim()),
    };
    rename_in_template(src, old, &with)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(n: &str) -> Option<Value> {
        match n {
            "長さ" => Some(Value::Num(Quantity::mm(25.0))),
            "角" => Some(Value::Num(Quantity::deg(30.0))),
            "数" => Some(Value::Num(Quantity::scalar(3.0))),
            "型番" => Some(Value::Text("M3".into())),
            _ => None,
        }
    }

    fn render(src: &str) -> Result<String, ExprError> {
        Template::parse(src)?.render(&look, LengthUnit::Mm)
    }

    #[test]
    fn slots_take_numbers_and_texts() {
        assert_eq!(render("{型番}-{長さ}").unwrap(), "M3-25 mm");
        assert_eq!(render("L={長さ; 1}").unwrap(), "L=25.0 mm");
        assert_eq!(render("{長さ / 1 mm}").unwrap(), "25");
        assert_eq!(render("{角}・{数 * 2}").unwrap(), "30 deg・6");
        assert_eq!(render("{{型番}} ｛型番｝").unwrap(), "{型番} ｛型番｝");
        assert_eq!(render("{pow(数; 2); 0}").unwrap(), "9");
        assert_eq!(render("面積 {長さ * 長さ}").unwrap(), "面積 625 mm^2");
        let inch = Template::parse("{長さ}").unwrap();
        assert_eq!(inch.render(&look, LengthUnit::In).unwrap(), "0.984 in");
        let t = Template::parse("{型番}-{長さ}-{型番}").unwrap();
        assert_eq!(t.references(), vec!["型番", "長さ"]);
        assert!(t.has_numbers(&|n| n == "型番"));
        assert!(
            !Template::parse("{型番}")
                .unwrap()
                .has_numbers(&|n| n == "型番")
        );
        assert!(!Template::parse("字だけ {{").unwrap().has_slots());
    }

    #[test]
    fn bad_templates_are_errors() {
        for (src, pos) in [("M3-{長さ", 3), ("a}", 1), ("{}", 0), ("{ }", 0)] {
            match Template::parse(src) {
                Err(ExprError::Syntax { pos: p, .. }) => assert_eq!(p, pos, "{src}"),
                other => panic!("{src}: {other:?}"),
            }
        }
        assert!(Template::parse("{長さ; 7}").is_err());
        assert!(Template::parse("{長さ; 1.5}").is_err());
        assert!(Template::parse("{長さ +}").is_err());
        assert_eq!(
            render("{型番 + 1}").unwrap_err(),
            ExprError::TextParam("型番".into())
        );
        assert!(matches!(render("{型番; 1}"), Err(ExprError::Text(_))));
        assert!(matches!(render("{長さ * 角}"), Err(ExprError::Text(_))));
        assert!(matches!(render("{無い}"), Err(ExprError::UnknownName(_))));
    }

    #[test]
    fn numbers_round_like_the_screen() {
        let mm = LengthUnit::Mm;
        assert_eq!(
            format_number(Quantity::mm(12.3456), None, mm).unwrap(),
            "12.35 mm"
        );
        assert_eq!(
            format_number(Quantity::mm(-0.001), None, mm).unwrap(),
            "0 mm"
        );
        assert_eq!(
            format_number(Quantity::mm(-0.001), Some(1), mm).unwrap(),
            "0.0 mm"
        );
        assert_eq!(
            format_number(Quantity::mm(304.8), None, LengthUnit::Ft).unwrap(),
            "1 ft"
        );
        assert_eq!(
            format_number(Quantity::scalar(1.0 / 3.0), None, mm).unwrap(),
            "0.333333"
        );
        assert_eq!(
            format_number(Quantity::scalar(120.0), None, mm).unwrap(),
            "120"
        );
        assert!(format_number(Quantity::scalar(f64::NAN), None, mm).is_err());
    }

    #[test]
    fn rename_and_inline_touch_only_slots() {
        assert_eq!(
            rename_in_template("長さ={長さ; 1} {長さ*2}", "長さ", "L").unwrap(),
            "長さ={L; 1} {L*2}"
        );
        assert_eq!(
            inline_in_template("{型番}-{長さ}", "型番", "M{数}", true).unwrap(),
            "M{数}-{長さ}"
        );
        assert_eq!(
            inline_in_template("{長さ * 2; 0}", "長さ", "a + 1 mm", false).unwrap(),
            "{(a + 1 mm) * 2; 0}"
        );
        assert_eq!(
            inline_in_template("{長さ}", "長さ", "25 mm", false).unwrap(),
            "{25 mm}"
        );
        assert_eq!(escape_template("a{b}"), "a{{b}}");
        let s = "x{y}{{z}}";
        assert_eq!(
            Template::parse(&escape_template(s))
                .unwrap()
                .render(&look, LengthUnit::Mm)
                .unwrap(),
            s
        );
    }
}
