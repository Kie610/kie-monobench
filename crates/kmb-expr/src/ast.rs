use crate::{Dim, ExprError, Quantity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(Quantity),
    Ref(String),
    Neg(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

impl Expr {
    /// 式が参照するパラメータ名を、出現順に重複なく返す。
    pub fn references(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_refs(&mut out);
        out
    }

    fn collect_refs(&self, out: &mut Vec<String>) {
        match self {
            Expr::Num(_) => {}
            Expr::Ref(n) => {
                if constant(n).is_none() && !out.contains(n) {
                    out.push(n.clone());
                }
            }
            Expr::Neg(e) => e.collect_refs(out),
            Expr::Bin(_, a, b) => {
                a.collect_refs(out);
                b.collect_refs(out);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.collect_refs(out)),
        }
    }

    pub fn eval(&self, lookup: &dyn Fn(&str) -> Option<Quantity>) -> Result<Quantity, ExprError> {
        match self {
            Expr::Num(q) => Ok(*q),
            Expr::Ref(n) if constant(n).is_some() => {
                Ok(Quantity::scalar(constant(n).expect("定数")))
            }
            Expr::Ref(n) => lookup(n).ok_or_else(|| ExprError::UnknownName(n.clone())),
            Expr::Neg(e) => {
                let q = e.eval(lookup)?;
                Ok(Quantity::new(-q.value, q.dim))
            }
            Expr::Bin(op, a, b) => binary(*op, a.eval(lookup)?, b.eval(lookup)?),
            Expr::Call(name, args) => {
                let vals = args
                    .iter()
                    .map(|a| a.eval(lookup))
                    .collect::<Result<Vec<_>, _>>()?;
                call(name, &vals)
            }
        }
    }
}

/// 組み込みの定数(`pi` と `π` も円周率)。
pub(crate) fn constant(name: &str) -> Option<f64> {
    match name {
        "pi" | "PI" | "π" => Some(std::f64::consts::PI),
        "E" => Some(std::f64::consts::E),
        _ => None,
    }
}

fn mismatch(op: &str, a: Quantity, b: Quantity) -> ExprError {
    ExprError::DimensionMismatch {
        op: op.into(),
        left: a.dim,
        right: b.dim,
    }
}

fn binary(op: BinOp, a: Quantity, b: Quantity) -> Result<Quantity, ExprError> {
    match op {
        BinOp::Add | BinOp::Sub => {
            let (a, b) = (a.like(b), b.like(a));
            if a.dim != b.dim {
                return Err(mismatch(if op == BinOp::Add { "+" } else { "-" }, a, b));
            }
            let v = if op == BinOp::Add {
                a.value + b.value
            } else {
                a.value - b.value
            };
            Ok(Quantity::new(v, a.dim))
        }
        BinOp::Mul => Ok(Quantity::new(
            a.value * b.value,
            Dim {
                length: a.dim.length + b.dim.length,
                angle: a.dim.angle + b.dim.angle,
            },
        )),
        BinOp::Div => {
            if b.value == 0.0 {
                return Err(ExprError::Domain("0 で割っています".into()));
            }
            Ok(Quantity::new(
                a.value / b.value,
                Dim {
                    length: a.dim.length - b.dim.length,
                    angle: a.dim.angle - b.dim.angle,
                },
            ))
        }
        BinOp::Pow => power(a, b),
    }
}

/// `a ^ b`。指数は無次元。単位つきの値は 8 以下の整数乗だけ。
fn power(a: Quantity, b: Quantity) -> Result<Quantity, ExprError> {
    if !b.dim.is_none() {
        return Err(mismatch("^", a, b));
    }
    if a.dim.is_none() {
        let v = a.value.powf(b.value);
        if v.is_nan() {
            return Err(ExprError::Domain("負の数の小数乗です".into()));
        }
        return Ok(Quantity::scalar(v));
    }
    // 次元を持つ値は整数乗だけを許す
    let n = b.value;
    if n.fract() != 0.0 || n.abs() > 8.0 {
        return Err(ExprError::Domain(
            "単位つきの値は 8 以下の整数乗のみです".into(),
        ));
    }
    let n = n as i8;
    Ok(Quantity::new(
        a.value.powi(n as i32),
        Dim {
            length: a.dim.length * n,
            angle: a.dim.angle * n,
        },
    ))
}

/// 無次元の値(そうでなければ誤り)。
fn unitless(name: &str, a: Quantity) -> Result<f64, ExprError> {
    if a.dim.is_none() {
        Ok(a.value)
    } else {
        Err(ExprError::DimensionMismatch {
            op: name.into(),
            left: a.dim,
            right: Dim::NONE,
        })
    }
}

fn arity(name: &str, args: &[Quantity], n: usize) -> Result<(), ExprError> {
    if args.len() == n {
        Ok(())
    } else {
        Err(ExprError::Arity {
            function: name.into(),
            expected: n,
            got: args.len(),
        })
    }
}

fn call(name: &str, args: &[Quantity]) -> Result<Quantity, ExprError> {
    match name {
        // 角度(か無次元)を受けて無次元を返す。無次元はラジアンとして読む
        "sin" | "cos" | "tan" | "sinh" | "cosh" | "tanh" => {
            arity(name, args, 1)?;
            let a = args[0];
            if !(a.dim.is_none() || a.dim == Dim::ANGLE) {
                return Err(ExprError::DimensionMismatch {
                    op: name.into(),
                    left: a.dim,
                    right: Dim::ANGLE,
                });
            }
            let v = match name {
                "sin" => a.value.sin(),
                "cos" => a.value.cos(),
                "tan" => a.value.tan(),
                "sinh" => a.value.sinh(),
                "cosh" => a.value.cosh(),
                _ => a.value.tanh(),
            };
            Ok(Quantity::scalar(v))
        }
        // 無次元を受けて角度を返す
        "asin" | "acos" | "atan" | "asinh" | "acosh" | "atanh" => {
            arity(name, args, 1)?;
            let a = unitless(name, args[0])?;
            let v = match name {
                "asin" => a.asin(),
                "acos" => a.acos(),
                "atan" => a.atan(),
                "asinh" => a.asinh(),
                "acosh" => a.acosh(),
                _ => a.atanh(),
            };
            if !v.is_finite() {
                return Err(ExprError::Domain(format!("{name} の引数が範囲外です")));
            }
            Ok(Quantity::new(v, Dim::ANGLE))
        }
        // 無次元を受けて無次元を返す。ln は自然対数、log は常用対数
        "exp" | "ln" | "log" => {
            arity(name, args, 1)?;
            let a = unitless(name, args[0])?;
            if name != "exp" && a <= 0.0 {
                return Err(ExprError::Domain(format!("{name} の引数は正の数です")));
            }
            let v = match name {
                "exp" => a.exp(),
                "ln" => a.ln(),
                _ => a.log10(),
            };
            if !v.is_finite() {
                return Err(ExprError::Domain(format!("{name} の値が大きすぎます")));
            }
            Ok(Quantity::scalar(v))
        }
        "pow" => {
            arity(name, args, 2)?;
            power(args[0], args[1])
        }
        // 符号(-1・0・1)。無次元を返す
        "sign" => {
            arity(name, args, 1)?;
            let v = args[0].value;
            Ok(Quantity::scalar(if v > 0.0 {
                1.0
            } else if v < 0.0 {
                -1.0
            } else {
                0.0
            }))
        }
        "atan2" => {
            arity(name, args, 2)?;
            if args[0].dim != args[1].dim {
                return Err(mismatch("atan2", args[0], args[1]));
            }
            Ok(Quantity::new(
                args[0].value.atan2(args[1].value),
                Dim::ANGLE,
            ))
        }
        "sqrt" => {
            arity(name, args, 1)?;
            let a = args[0];
            if a.dim.length % 2 != 0 || a.dim.angle % 2 != 0 {
                return Err(ExprError::Domain(
                    "sqrt の引数の次元が偶数乗ではありません".into(),
                ));
            }
            if a.value < 0.0 {
                return Err(ExprError::Domain("負の数の sqrt です".into()));
            }
            Ok(Quantity::new(
                a.value.sqrt(),
                Dim {
                    length: a.dim.length / 2,
                    angle: a.dim.angle / 2,
                },
            ))
        }
        "abs" | "floor" | "ceil" | "round" => {
            arity(name, args, 1)?;
            let a = args[0];
            let v = match name {
                "abs" => a.value.abs(),
                "floor" => a.value.floor(),
                "ceil" => a.value.ceil(),
                _ => a.value.round(),
            };
            Ok(Quantity::new(v, a.dim))
        }
        "min" | "max" => {
            if args.is_empty() {
                return Err(ExprError::Arity {
                    function: name.into(),
                    expected: 1,
                    got: 0,
                });
            }
            let mut acc = args[0];
            for &b in &args[1..] {
                let (b, a) = (b.like(acc), acc.like(b));
                acc = a;
                if b.dim != acc.dim {
                    return Err(mismatch(name, acc, b));
                }
                let pick = if name == "min" {
                    b.value < acc.value
                } else {
                    b.value > acc.value
                };
                if pick {
                    acc = b;
                }
            }
            Ok(acc)
        }
        _ => Err(ExprError::UnknownFunction(name.into())),
    }
}
