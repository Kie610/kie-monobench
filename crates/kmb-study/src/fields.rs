//! 研究の欄(式の文字)の読み方。強度・熱(kmb-fea)と流れ(kmb-flow)が同じ物を使う。

use kmb_core::{Dim, EvalCtx, EvalError, FeatureId, InputRef, expr_inputs};

/// 欄が空(空白だけ)か。空の欄は「自動」や既定の値を表す。
pub fn is_empty(s: &str) -> bool {
    s.trim().is_empty()
}

/// 単位の無い数の欄(単位は欄の名前にある)。
pub fn number(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if q.dim != Dim::NONE {
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は数だけで書いてください(単位は欄の名前のとおり。値: {q})"
        )));
    }
    Ok(q.value)
}

/// 長さの欄(mm)。単位の無い値は mm。
pub fn length(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if q.dim != Dim::NONE && q.dim != Dim::LENGTH {
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は長さにしてください(値: {q})"
        )));
    }
    Ok(q.value)
}

/// 式の欄の並び `exprs` の中の名前 `old` を `new` に替える(空の欄は飛ばす)。1 つでも変われば真。
/// 研究の種類の `Feature::rename_param` が使う。
pub fn rename_exprs<'a>(
    exprs: impl IntoIterator<Item = &'a mut String>,
    old: &str,
    new: &str,
) -> Result<bool, EvalError> {
    let mut changed = false;
    for e in exprs {
        if is_empty(e) {
            continue;
        }
        let r = kmb_core::rename_reference(e, old, new)?;
        if r != *e {
            *e = r;
            changed = true;
        }
    }
    Ok(changed)
}

/// 研究の入力: 選んだボディの出力 `body` と、式の欄 `exprs` が使う値(空の欄は飛ばす、重ねない)。
/// 研究の種類の `Feature::inputs` が使う。
pub fn inputs<'a>(
    bodies: impl IntoIterator<Item = FeatureId>,
    exprs: impl IntoIterator<Item = &'a String>,
) -> Result<Vec<InputRef>, EvalError> {
    let mut v: Vec<InputRef> = bodies
        .into_iter()
        .map(|feature| InputRef::Output {
            feature,
            slot: "body".into(),
        })
        .collect();
    for e in exprs {
        if is_empty(e) {
            continue;
        }
        for i in expr_inputs(e)? {
            if !v.contains(&i) {
                v.push(i);
            }
        }
    }
    Ok(v)
}
