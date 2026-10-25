//! 3D スケッチの寸法の名前(`d1` など)と式。名前の付け方と式の計算は 2D のスケッチと同じ
//! 部品([`jc_sketch::dims`])を使う。

use crate::{Constraint3d, Sketch3d};
use jc_core::{ModelParam, Names};
use jc_sketch::ConstraintId;
use jc_sketch::dims::{self, DimConstraint, DimExpr, rename_in};

impl Sketch3d {
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
                    label: format!("{} {}(3D)", c.kind.label(), c.id),
                })
            })
            .collect()
    }

    /// 寸法に名前を付け、ほかと重なる名前を付け直したスケッチ(変えなければ `None`)。
    pub fn with_dim_names(&self, book: &mut Names) -> Option<Sketch3d> {
        let mut s = self.clone();
        dims::name_dims(&mut s.dim_names, &mut s.constraints, book).then_some(s)
    }

    /// 名前 `old` を `new` にしたスケッチ(式の参照と寸法の名前。変えなければ `None`)。
    pub fn renamed(&self, old: &str, new: &str) -> Result<Option<Sketch3d>, jc_core::EvalError> {
        let mut s = self.clone();
        let values: Vec<&mut String> = s
            .constraints
            .iter_mut()
            .filter_map(|c| c.kind.value_mut())
            .collect();
        let changed = rename_in(&mut s.dim_names, values, old, new)?;
        Ok(changed.then_some(s))
    }

    /// 寸法 `name` の式を `expr` にしたスケッチ(その名前の形を決める寸法が無ければ `None`)。
    pub fn with_dim_value(&self, name: &str, expr: &str) -> Option<Sketch3d> {
        let mut s = self.clone();
        dims::set_dim_value(&s.dim_names, &mut s.constraints, name, expr).then_some(s)
    }

    pub(crate) fn is_own_name(&self, name: &str) -> bool {
        dims::is_own_name(&self.dim_names, &self.constraints, name)
    }

    pub(crate) fn dim_exprs(&self) -> Vec<DimExpr<'_>> {
        dims::dim_exprs(&self.dim_names, &self.constraints)
    }
}

impl DimConstraint for Constraint3d {
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
