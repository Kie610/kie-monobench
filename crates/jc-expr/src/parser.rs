use crate::ast::{BinOp, Expr};
use crate::lexer::Tok;
use crate::{Dim, ExprError, Quantity};

pub(crate) struct Parser<'a> {
    toks: &'a [(Tok, usize)],
    pos: usize,
    src_len: usize,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(toks: &'a [(Tok, usize)], src_len: usize) -> Self {
        Parser {
            toks,
            pos: 0,
            src_len,
        }
    }

    pub(crate) fn parse(mut self) -> Result<Expr, ExprError> {
        if self.toks.is_empty() {
            return Err(ExprError::Syntax {
                pos: 0,
                message: "式が空です".into(),
            });
        }
        let e = self.additive()?;
        if self.pos < self.toks.len() {
            return Err(self.err("式の終わりに余分な記号があります"));
        }
        Ok(e)
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|(t, _)| t)
    }

    fn err(&self, message: &str) -> ExprError {
        let pos = self.toks.get(self.pos).map_or(self.src_len, |(_, p)| *p);
        ExprError::Syntax {
            pos,
            message: message.into(),
        }
    }

    fn additive(&mut self) -> Result<Expr, ExprError> {
        let mut lhs = self.multiplicative()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek() {
            let op = if *c == '+' { BinOp::Add } else { BinOp::Sub };
            self.pos += 1;
            let rhs = self.multiplicative()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    /// 掛け算と割り算の並び。単位は、直前に続く数だけの部分(数・定数・それらの式)に掛かる
    /// (`PI/4 rad` は 45 度、`1/2 in` は 12.7 mm、`(1 + 2) mm` は 3 mm。
    /// `width / 2 mm` は width ÷ 2 mm、`90 deg / 90 deg` は 1)。`5 mm^2` の `^` は単位だけに掛かる。
    fn multiplicative(&mut self) -> Result<Expr, ExprError> {
        let mut items: Vec<(BinOp, Expr)> = vec![(BinOp::Mul, self.unary()?)];
        self.units(&mut items)?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek() {
            let op = if *c == '*' { BinOp::Mul } else { BinOp::Div };
            self.pos += 1;
            items.push((op, self.unary()?));
            self.units(&mut items)?;
        }
        let mut it = items.into_iter();
        let (_, mut lhs) = it.next().expect("並びは 1 つ以上");
        for (op, rhs) in it {
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    /// 並びの後ろの単位(`mm`、`deg` など。`mm^2` のように累乗も)を掛ける。
    fn units(&mut self, items: &mut Vec<(BinOp, Expr)>) -> Result<(), ExprError> {
        while let Some(Tok::Ident(u)) = self.peek()
            && let Some(mut q) = with_unit(1.0, u)
        {
            self.pos += 1;
            if let Some(Tok::Op('^')) = self.peek() {
                self.pos += 1;
                let n = match self.peek() {
                    Some(Tok::Num(n)) if n.fract() == 0.0 && *n >= 1.0 && *n <= 8.0 => *n as i8,
                    _ => return Err(self.err("単位の累乗は 1 から 8 の整数です")),
                };
                self.pos += 1;
                q = Quantity::new(
                    q.value.powi(n as i32),
                    Dim {
                        length: q.dim.length * n,
                        angle: q.dim.angle * n,
                    },
                );
            }
            // 後ろから続く、数だけの部分(なければ最後の 1 つ)
            let mut k = items.len();
            while k > 0 && is_numeric(&items[k - 1].1) {
                k -= 1;
            }
            let k = k.min(items.len() - 1);
            let mut run = items.split_off(k).into_iter();
            let (op, mut sub) = run.next().expect("1 つ以上");
            for (o, e) in run {
                sub = Expr::Bin(o, Box::new(sub), Box::new(e));
            }
            let sub = match sub {
                // 数と単位だけなら 1 つの値にまとめる(`10 mm` は値 1 つ)
                Expr::Num(v) if v.dim.is_none() => {
                    Expr::Num(Quantity::new(v.value * q.value, q.dim))
                }
                e => Expr::Bin(BinOp::Mul, Box::new(e), Box::new(Expr::Num(q))),
            };
            items.push((op, sub));
        }
        Ok(())
    }

    fn unary(&mut self) -> Result<Expr, ExprError> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    /// `^` は右結合で、単項マイナスより強い(`-2^2` は -4)。
    fn power(&mut self) -> Result<Expr, ExprError> {
        let base = self.primary()?;
        let base = self.bracket_unit(base)?;
        if let Some(Tok::Op('^')) = self.peek() {
            self.pos += 1;
            let exp = self.unary()?;
            return Ok(Expr::Bin(BinOp::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    /// 大かっこの単位(`5 [mm]`、`(1/2) [in]`、`3 [mm^2]`、`2 [mm*deg]`)。直前の 1 つ
    /// (数・名前・かっこ・関数)だけに掛かり、`^` より強い(`2 [mm]^2` は 4 mm²)。
    /// 大かっこの無い単位は、今までどおり掛け算の並びで読む([`Self::units`])。
    fn bracket_unit(&mut self, base: Expr) -> Result<Expr, ExprError> {
        if !matches!(self.peek(), Some(Tok::LBracket)) {
            return Ok(base);
        }
        self.pos += 1;
        let mut q = self.bracket_factor()?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek() {
            let div = *c == '/';
            self.pos += 1;
            let f = self.bracket_factor()?;
            q = if div {
                Quantity::new(
                    q.value / f.value,
                    Dim {
                        length: q.dim.length - f.dim.length,
                        angle: q.dim.angle - f.dim.angle,
                    },
                )
            } else {
                Quantity::new(
                    q.value * f.value,
                    Dim {
                        length: q.dim.length + f.dim.length,
                        angle: q.dim.angle + f.dim.angle,
                    },
                )
            };
        }
        if !matches!(self.peek(), Some(Tok::RBracket)) {
            return Err(self.err("']' が必要です"));
        }
        self.pos += 1;
        Ok(match base {
            Expr::Num(v) if v.dim.is_none() => Expr::Num(Quantity::new(v.value * q.value, q.dim)),
            e => Expr::Bin(BinOp::Mul, Box::new(e), Box::new(Expr::Num(q))),
        })
    }

    /// 大かっこの中の単位 1 つ(累乗も)。
    fn bracket_factor(&mut self) -> Result<Quantity, ExprError> {
        let q = match self.peek() {
            Some(Tok::Ident(u)) => with_unit(1.0, u),
            _ => None,
        };
        let Some(mut q) = q else {
            return Err(self.err("大かっこの中は単位です(mm・cm・m・in・ft・deg・rad)"));
        };
        self.pos += 1;
        if let Some(Tok::Op('^')) = self.peek() {
            self.pos += 1;
            let n = match self.peek() {
                Some(Tok::Num(n)) if n.fract() == 0.0 && *n >= 1.0 && *n <= 8.0 => *n as i8,
                _ => return Err(self.err("単位の累乗は 1 から 8 の整数です")),
            };
            self.pos += 1;
            q = Quantity::new(
                q.value.powi(n as i32),
                Dim {
                    length: q.dim.length * n,
                    angle: q.dim.angle * n,
                },
            );
        }
        Ok(q)
    }

    fn primary(&mut self) -> Result<Expr, ExprError> {
        match self.peek().cloned() {
            Some(Tok::Num(v)) => {
                self.pos += 1;
                // 後ろの単位は掛け算の並びで読む([`Self::units`])
                Ok(Expr::Num(Quantity::scalar(v)))
            }
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                if crate::UNIT_NAMES.contains(&name.as_str()) {
                    self.pos -= 1;
                    return Err(self.err("単位の前に数値がありません"));
                }
                if let Some(Tok::LParen) = self.peek() {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::RParen)) {
                        loop {
                            args.push(self.additive()?);
                            match self.peek() {
                                Some(Tok::Comma) => self.pos += 1,
                                Some(Tok::RParen) => break,
                                _ => return Err(self.err("',' か ')' が必要です")),
                            }
                        }
                    }
                    self.pos += 1; // ')'
                    return Ok(Expr::Call(name, args));
                }
                Ok(Expr::Ref(name))
            }
            Some(Tok::LParen) => {
                self.pos += 1;
                let e = self.additive()?;
                if !matches!(self.peek(), Some(Tok::RParen)) {
                    return Err(self.err("')' が必要です"));
                }
                self.pos += 1;
                Ok(e)
            }
            _ => Err(self.err("数値、名前、'(' のいずれかが必要です")),
        }
    }
}

/// 単位も名前も含まない、数だけの式か(定数 `PI` などは数)。
fn is_numeric(e: &Expr) -> bool {
    match e {
        Expr::Num(q) => q.dim.is_none(),
        Expr::Ref(n) => crate::ast::constant(n).is_some(),
        Expr::Neg(a) => is_numeric(a),
        Expr::Bin(_, a, b) => is_numeric(a) && is_numeric(b),
        Expr::Call(_, args) => args.iter().all(is_numeric),
    }
}

fn with_unit(v: f64, unit: &str) -> Option<Quantity> {
    let q = match unit {
        "mm" => Quantity::new(v, Dim::LENGTH),
        "cm" => Quantity::new(v * 10.0, Dim::LENGTH),
        "m" => Quantity::new(v * 1000.0, Dim::LENGTH),
        "in" => Quantity::new(v * 25.4, Dim::LENGTH),
        "ft" => Quantity::new(v * 304.8, Dim::LENGTH),
        "deg" => Quantity::deg(v),
        "rad" => Quantity::new(v, Dim::ANGLE),
        _ => return None,
    };
    Some(q)
}
