//! 寸法の名前と、寸法の式の計算。
//!
//! 形を決める寸法には文書の中で一意な名前が付き([`kmb_core::Feature::name_model_params`])、
//! ほかの寸法・機能の欄・パラメータの式から名前で参照できる(モデルのパラメータ)。
//! 同じスケッチの寸法の名前は、スケッチの中でその場で解く(循環は誤り)。
//! 名前は拘束とは別に持つ(写しや貼り付けで作った寸法は、新しい名前になる)。

use crate::model::{Constraint, ConstraintId, Sketch};
use kmb_core::{Dim, ModelParam, Names, Quantity};
use std::collections::{BTreeMap, HashMap};

/// 計算する寸法 1 つ。
pub struct DimExpr<'a> {
    pub id: ConstraintId,
    pub name: Option<&'a str>,
    /// 誤りに出す欄名。式から参照できる名前とは別に持つ。
    pub label: Option<&'a str>,
    pub expr: &'a str,
    pub angle: bool,
    /// 個数は単位を付けず、整数と上限を確かめる。
    pub count_max: Option<usize>,
    /// パターンの距離は負で向きを反転できる。
    pub signed: bool,
}

impl DimExpr<'_> {
    fn description(&self) -> String {
        self.label
            .map_or_else(|| dimension_description(self.id, self.name), str::to_string)
    }

    pub(crate) fn checked(&self, q: Quantity) -> Result<f64, String> {
        checked_value(&self.description(), self.expr, q, self.angle, self.signed)
    }
}

fn dimension_description(id: ConstraintId, name: Option<&str>) -> String {
    let who = name.map_or_else(|| id.to_string(), str::to_string);
    format!("{who} の寸法")
}

/// 寸法の式を計算する。同じスケッチの寸法の名前はその場で解き(循環は誤り)、ほかの名前は
/// `lookup` で引く。単位の無い値は mm(角度の寸法は deg)として読む(数だけの
/// 式は欄の単位。`50 / 2` は 25 mm)。
pub fn eval_dims(
    dims: &[DimExpr<'_>],
    lookup: &dyn Fn(&str) -> Option<Quantity>,
) -> Result<HashMap<ConstraintId, Quantity>, String> {
    let local: HashMap<&str, usize> = dims
        .iter()
        .enumerate()
        .filter_map(|(i, d)| Some((d.name?, i)))
        .collect();
    let mut done: Vec<Option<Quantity>> = vec![None; dims.len()];
    let mut visiting = vec![false; dims.len()];

    fn visit(
        i: usize,
        dims: &[DimExpr<'_>],
        local: &HashMap<&str, usize>,
        lookup: &dyn Fn(&str) -> Option<Quantity>,
        done: &mut Vec<Option<Quantity>>,
        visiting: &mut Vec<bool>,
    ) -> Result<Quantity, String> {
        if let Some(q) = done[i] {
            return Ok(q);
        }
        let d = &dims[i];
        let who = d.name.map_or_else(|| d.id.to_string(), str::to_string);
        if visiting[i] {
            return Err(format!("寸法 {who} の参照が循環しています"));
        }
        visiting[i] = true;
        let description = d.description();
        let e =
            kmb_core::parse_expr(d.expr).map_err(|e| format!("{description} `{}`: {e}", d.expr))?;
        for r in e.references() {
            if let Some(&j) = local.get(r.as_str()) {
                visit(j, dims, local, lookup, done, visiting)?;
            }
        }
        let get = |n: &str| match local.get(n) {
            Some(&j) => done[j],
            None => lookup(n),
        };
        let q = e
            .eval(&get)
            .map_err(|e| format!("{description} `{}`: {e}", d.expr))?;
        let q = match (q.dim.is_none() && d.count_max.is_none(), d.angle) {
            (true, true) => Quantity::deg(q.value),
            (true, false) => Quantity::mm(q.value),
            _ => q,
        };
        visiting[i] = false;
        done[i] = Some(q);
        Ok(q)
    }

    let mut out = HashMap::new();
    for i in 0..dims.len() {
        let q = visit(i, dims, &local, lookup, &mut done, &mut visiting)?;
        out.insert(dims[i].id, q);
    }
    Ok(out)
}

/// 計算した寸法の値を、ソルバに渡す値(長さは mm、角度は rad)にする。単位の次元が違う式や、
/// 負の長さ(`signed` でないもの)は誤りにする。
pub fn checked(
    id: ConstraintId,
    name: Option<&str>,
    src: &str,
    q: Quantity,
    angle: bool,
    signed: bool,
) -> Result<f64, String> {
    checked_value(&dimension_description(id, name), src, q, angle, signed)
}

fn checked_value(
    description: &str,
    src: &str,
    q: Quantity,
    angle: bool,
    signed: bool,
) -> Result<f64, String> {
    let (want, unit) = if angle {
        (Dim::ANGLE, "角度")
    } else {
        (Dim::LENGTH, "長さ")
    };
    if q.dim != want {
        return Err(format!(
            "{description} `{src}` は{unit}の単位が必要です(値: {q})"
        ));
    }
    if !angle && !signed && q.value < 0.0 {
        return Err(format!("{description} `{src}` が負です"));
    }
    // 桁違いの寸法は解く前に止める(壊れたファイルや AI の打ち込み)。
    if let Some(why) = q.size_problem() {
        return Err(format!("{description} `{src}` が{why}"));
    }
    Ok(q.value)
}

/// 名前を付ける・付け直す。`dims` は寸法(拘束の番号、形を決めるか)。
/// 名前の無い形を決める寸法に新しい名前を付け、ほかと重なる名前は付け直す(付け直した名前を
/// 返す)。もう無い寸法の名前は消す。変えたら true。
fn settle(
    names: &mut BTreeMap<ConstraintId, String>,
    dims: &[(ConstraintId, bool)],
    book: &mut Names,
) -> (bool, Vec<(String, String)>) {
    let before = names.len();
    names.retain(|id, _| dims.iter().any(|(d, _)| d == id));
    let mut changed = names.len() != before;
    let mut renamed = Vec::new();
    for &(id, driving) in dims {
        match names.get(&id) {
            Some(n) => {
                if !book.claim(n) {
                    let fresh = book.fresh();
                    renamed.push((n.clone(), fresh.clone()));
                    names.insert(id, fresh);
                    changed = true;
                }
            }
            // 参照寸法(形を決めない)はパラメータにしない
            None if driving => {
                names.insert(id, book.fresh());
                changed = true;
            }
            None => {}
        }
    }
    (changed, renamed)
}

/// 名前 `old` を `new` にする(式の参照と、名前そのもの)。変えたら true。
pub fn rename_in(
    names: &mut BTreeMap<ConstraintId, String>,
    values: Vec<&mut String>,
    old: &str,
    new: &str,
) -> Result<bool, kmb_core::EvalError> {
    let mut changed = false;
    for v in values {
        let r = kmb_core::rename_reference(v, old, new).map_err(kmb_core::EvalError::Expr)?;
        if r != *v {
            *v = r;
            changed = true;
        }
    }
    // 名前そのもの(名前に使えない文字への置き換え、つまり消えた寸法の置き換えでは変えない)
    if kmb_core::is_valid_name(new) {
        for n in names.values_mut() {
            if n == old {
                *n = new.to_string();
                changed = true;
            }
        }
    }
    Ok(changed)
}

/// 寸法を持てる拘束(2D の [`Constraint`] と 3D スケッチの拘束)。名前の付け方と式の計算を
/// 2D と 3D で 1 つにするための形。
pub trait DimConstraint {
    fn id(&self) -> ConstraintId;
    /// 参照寸法(形を決めない)か
    fn reference(&self) -> bool;
    /// 寸法の式(寸法でなければ `None`)
    fn value(&self) -> Option<&str>;
    fn value_mut(&mut self) -> Option<&mut String>;
    fn is_angle(&self) -> bool;
}

impl DimConstraint for Constraint {
    fn id(&self) -> ConstraintId {
        self.id
    }
    fn reference(&self) -> bool {
        self.reference
    }
    fn value(&self) -> Option<&str> {
        self.kind.value()
    }
    fn value_mut(&mut self) -> Option<&mut String> {
        self.kind.value_mut()
    }
    fn is_angle(&self) -> bool {
        self.kind.is_angle()
    }
}

/// 寸法に名前を付け、ほかと重なる名前を付け直す(写したスケッチの中の参照も新しい名前へ)。
/// 変えたら true。
pub fn name_dims<C: DimConstraint>(
    names: &mut BTreeMap<ConstraintId, String>,
    constraints: &mut [C],
    book: &mut Names,
) -> bool {
    let dims: Vec<(ConstraintId, bool)> = constraints
        .iter()
        .filter(|c| c.value().is_some())
        .map(|c| (c.id(), !c.reference()))
        .collect();
    let (changed, renamed) = settle(names, &dims, book);
    for (old, new) in &renamed {
        let values = constraints
            .iter_mut()
            .filter_map(|c| c.value_mut())
            .collect();
        // 名前そのものは settle で付け直したので、式だけ書き換える
        let _ = rename_in(&mut BTreeMap::new(), values, old, new);
    }
    changed
}

/// 名前 `name` の形を決める寸法の式を `expr` にする(その寸法が無ければ false)。
pub fn set_dim_value<C: DimConstraint>(
    names: &BTreeMap<ConstraintId, String>,
    constraints: &mut [C],
    name: &str,
    expr: &str,
) -> bool {
    let Some((&id, _)) = names.iter().find(|(_, n)| *n == name) else {
        return false;
    };
    match constraints
        .iter_mut()
        .find(|c| c.id() == id && !c.reference())
        .and_then(|c| c.value_mut())
    {
        Some(v) => {
            *v = expr.to_string();
            true
        }
        None => false,
    }
}

/// 同じスケッチの中で解く名前(形を決める寸法の名前)か。
pub fn is_own_name<C: DimConstraint>(
    names: &BTreeMap<ConstraintId, String>,
    constraints: &[C],
    name: &str,
) -> bool {
    constraints
        .iter()
        .filter(|c| !c.reference() && c.value().is_some())
        .any(|c| names.get(&c.id()).map(String::as_str) == Some(name))
}

/// 形を決める寸法の式(計算する寸法の並び)。
pub fn dim_exprs<'a, C: DimConstraint>(
    names: &'a BTreeMap<ConstraintId, String>,
    constraints: &'a [C],
) -> Vec<DimExpr<'a>> {
    constraints
        .iter()
        .filter(|c| !c.reference())
        .filter_map(|c| {
            Some(DimExpr {
                id: c.id(),
                name: names.get(&c.id()).map(String::as_str),
                label: None,
                expr: c.value()?,
                angle: c.is_angle(),
                count_max: None,
                signed: false,
            })
        })
        .collect()
}

impl Sketch {
    /// 寸法の名前(`d1` など)。
    pub fn dim_name(&self, id: ConstraintId) -> Option<&str> {
        self.dim_names.get(&id).map(String::as_str)
    }

    /// 形を決める寸法の名前と式(モデルのパラメータ)。
    pub fn model_params(&self) -> Vec<ModelParam> {
        self.constraints
            .iter()
            .filter(|c| !c.reference)
            .filter_map(|c| {
                Some(ModelParam {
                    key: c.id.to_string(),
                    name: self.dim_name(c.id)?.to_string(),
                    expr: c.kind.value()?.to_string(),
                    label: format!("{} {}", crate::dimension::kind_word(&c.kind), c.id),
                })
            })
            .collect()
    }

    /// 寸法に名前を付け、ほかと重なる名前を付け直したスケッチ(変えなければ `None`)。
    pub fn with_dim_names(&self, book: &mut Names) -> Option<Sketch> {
        let mut s = self.clone();
        name_dims(&mut s.dim_names, &mut s.constraints, book).then_some(s)
    }

    /// 名前 `old` を `new` にしたスケッチ(式の参照と寸法の名前。変えなければ `None`)。
    pub fn renamed(&self, old: &str, new: &str) -> Result<Option<Sketch>, kmb_core::EvalError> {
        let mut s = self.clone();
        let mut values: Vec<&mut String> = s
            .constraints
            .iter_mut()
            .filter_map(|c| c.kind.value_mut())
            .collect();
        values.extend(s.offset.as_mut());
        for p in &mut s.patterns {
            if let Some(e) = &mut p.expressions {
                values.extend(e.strings_mut());
            }
        }
        let changed = rename_in(&mut s.dim_names, values, old, new)?;
        Ok(changed.then_some(s))
    }

    /// 寸法 `name` の式を `expr` にしたスケッチ(その名前の形を決める寸法が無ければ `None`)。
    pub fn with_dim_value(&self, name: &str, expr: &str) -> Option<Sketch> {
        let mut s = self.clone();
        set_dim_value(&s.dim_names, &mut s.constraints, name, expr).then_some(s)
    }

    /// 同じスケッチの中で解く名前(形を決める寸法の名前)か。
    pub(crate) fn is_own_name(&self, name: &str) -> bool {
        is_own_name(&self.dim_names, &self.constraints, name)
    }

    pub(crate) fn dim_exprs(&self) -> Result<Vec<DimExpr<'_>>, String> {
        let mut dims = dim_exprs(&self.dim_names, &self.constraints);
        dims.extend(self.pattern_exprs()?);
        Ok(dims)
    }
}
