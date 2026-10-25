use crate::{Descriptor, FeatureId, Kernel, Quantity, ShapeHandle};
use kmb_expr::ExprError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

/// 機能の出力値。CAM の経路などコアが知らない型は `Custom` で運ぶ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Scalar(Quantity),
    Shape(ShapeHandle),
    Custom {
        type_key: String,
        data: serde_json::Value,
    },
}

/// 出力の名前(スロット)と値。
pub type Outputs = BTreeMap<String, Value>;

/// 警告の出力スロット。評価はできたが、利用者に知らせたいこと(例: 材料に合う切削条件がない)。
pub const WARNINGS: &str = "warnings";

/// 警告の一覧を出力に入れる(空なら何もしない)。
pub fn add_warnings(out: &mut Outputs, list: Vec<String>) {
    if list.is_empty() {
        return;
    }
    let mut all = warnings_of(out);
    all.extend(list);
    out.insert(
        WARNINGS.into(),
        crate::Payload::to_value(&crate::Warnings(all)),
    );
}

/// 出力の中の警告。
pub fn warnings_of(out: &Outputs) -> Vec<String> {
    out.get(WARNINGS)
        .and_then(<crate::Warnings as crate::Payload>::from_value)
        .map(|w| w.0)
        .unwrap_or_default()
}

/// 機能が宣言する依存先。宣言していない入力は評価中に読めない。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputRef {
    Param { name: String },
    Output { feature: FeatureId, slot: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    /// 機能のデータが不正(必須項目がない、型が違う など)
    InvalidData(String),
    /// 宣言していない入力を読もうとした(契約違反)
    Undeclared(String),
    Expr(ExprError),
    Kernel(String),
    Other(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::InvalidData(m) => write!(f, "データが不正です: {m}"),
            EvalError::Undeclared(m) => write!(f, "宣言していない入力を読みました: {m}"),
            EvalError::Expr(e) => write!(f, "式の誤り: {e}"),
            EvalError::Kernel(m) => write!(f, "形状カーネルの誤り: {m}"),
            EvalError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for EvalError {}

impl From<ExprError> for EvalError {
    fn from(e: ExprError) -> Self {
        EvalError::Expr(e)
    }
}

/// タイムラインの 1 項目の振る舞い。CAD の操作も CAM の操作もこれを実装する。
pub trait Feature {
    /// 依存先を宣言する。再計算はこれで依存を調べ、評価の順序とキャッシュを決める。
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError>;
    /// 入力から出力を作る。入力とカーネルには `ctx` からのみ触る。
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError>;
    /// パラメータ名の変更に合わせて、式を書き換えたデータを返す。`old` が自分の持つ
    /// モデルのパラメータ([`Feature::model_params`])の名前なら、その名前も `new` にする。
    /// 式を持たない機能、または `old` を参照しない機能は `None` を返す。
    fn rename_param(&self, _old: &str, _new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        Ok(None)
    }
    /// 機能が持つ、名前の付いた式(スケッチの寸法の `d1` など)。
    /// 名前は文書の中で一意で、ほかの機能の式やパラメータから参照できる。式は形に依らず、
    /// 再計算の初めにパラメータと一緒に計算する。
    fn model_params(&self) -> Vec<ModelParam> {
        Vec::new()
    }
    /// 名前の無い式に名前を付け、文書の中でほかと重なる名前を付け直したデータを返す
    /// (変えなければ `None`)。名前は `names` から取る。
    fn name_model_params(&self, _names: &mut Names) -> Option<serde_json::Value> {
        None
    }
    /// モデルのパラメータ `name` の式を `expr` にしたデータを返す(持っていなければ `None`)。
    fn set_model_param(&self, _name: &str, _expr: &str) -> Option<serde_json::Value> {
        None
    }
    /// 抑制した時に、そのまま後ろへ渡す出力(出力の名前と、その元)。
    /// 例えば切り取りの押し出しは、抑制すると対象のボディを `body` として渡す。
    /// 何も渡さない機能を抑制すると、それを使う後ろの機能は止まる。
    fn pass_through(&self) -> Vec<(String, InputRef)> {
        Vec::new()
    }
    /// 読むが使い切らないボディ。ここに挙げた機能のボディは、
    /// この機能が読んでも、見えるボディに残る。
    fn keeps(&self) -> Vec<FeatureId> {
        Vec::new()
    }
}

/// 機能が持つ、名前の付いた式([`Feature::model_params`])。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelParam {
    /// 機能の中で変わらない鍵(スケッチの寸法なら拘束の ID)。名前を変えても変わらないので、
    /// パラメータの表([`crate::ParamKey::Model`])はこれで指す
    #[serde(default)]
    pub key: String,
    pub name: String,
    pub expr: String,
    /// 何の値か(例: 「距離 C3」)。パラメータの一覧に出す
    pub label: String,
}

/// モデルのパラメータの名前の帳面。使った名前を覚え、新しい名前(`d1`、`d2`…)を出す。
#[derive(Debug, Clone, Default)]
pub struct Names {
    taken: std::collections::HashSet<String>,
    next: u64,
}

impl Names {
    /// 使っている名前から帳面を作る。新しい名前は `d` + 使っている最大の番号の次から。
    pub fn new<'a>(taken: impl IntoIterator<Item = &'a str>) -> Self {
        let mut n = Names::default();
        for t in taken {
            n.mark(t);
        }
        n
    }

    fn mark(&mut self, name: &str) {
        if let Some(k) = name.strip_prefix('d').and_then(|d| d.parse::<u64>().ok()) {
            self.next = self.next.max(k);
        }
        self.taken.insert(name.to_string());
    }

    /// `name` がまだ使われていなければ自分のものにして true。使われていれば false。
    pub fn claim(&mut self, name: &str) -> bool {
        if self.taken.contains(name) || !kmb_expr::is_valid_name(name) {
            return false;
        }
        self.mark(name);
        true
    }

    /// 新しい名前(`d` + 番号)を出して使ったことにする。
    pub fn fresh(&mut self) -> String {
        loop {
            self.next += 1;
            let name = format!("d{}", self.next);
            if self.claim(&name) {
                return name;
            }
        }
    }
}

/// 保存されたデータから機能を組み立てる関数。
pub type Factory = fn(&serde_json::Value) -> Result<Box<dyn Feature>, EvalError>;

/// [`Registry::mark`] の印のうち、コアが読むもの: 形を作らず、カーネルの形を読むだけの種類
/// (加工の経路など)。再計算は、互いに依存しないこの印の機能を、別々のスレッドで同時に計算する
/// (形を作る機能は、形の番号の付く順を毎回同じにするため、1 つずつ順に計算する)。
pub const CONCURRENT: &str = "concurrent";

/// 機能の種類 ID(例: `"cad.extrude@1"`)と組み立て関数の対応表。
#[derive(Default)]
pub struct Registry {
    factories: BTreeMap<String, Factory>,
    descriptors: BTreeMap<String, Descriptor>,
    marks: BTreeMap<String, BTreeSet<&'static str>>,
    /// モジュールが名乗る文書の区画(鍵 → 名前。[`crate::section`])
    sections: BTreeMap<&'static str, &'static str>,
    /// モジュールが組み立ての時に置く共有の値([`Registry::provide`])。型で引く
    services: std::collections::HashMap<
        std::any::TypeId,
        std::sync::Arc<dyn std::any::Any + Send + Sync>,
    >,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 種類を登録する。同じ ID を二度登録するのはプログラムの誤りとして panic する。
    pub fn register(&mut self, type_id: &str, factory: Factory) {
        let prev = self.factories.insert(type_id.to_string(), factory);
        assert!(
            prev.is_none(),
            "機能の種類 {type_id} が二重に登録されました"
        );
    }

    /// 説明つきで種類を登録する。
    pub fn register_described(&mut self, desc: Descriptor, factory: Factory) {
        self.register(&desc.type_id.clone(), factory);
        self.descriptors.insert(desc.type_id.clone(), desc);
    }

    /// 種類の説明。説明なしで登録された種類は区分 `other` の既定の説明になる。
    pub fn descriptor(&self, type_id: &str) -> Option<Descriptor> {
        if !self.contains(type_id) {
            return None;
        }
        Some(
            self.descriptors
                .get(type_id)
                .cloned()
                .unwrap_or_else(|| Descriptor::unknown(type_id)),
        )
    }

    /// 種類の区分(登録されていなければ `None`)。
    pub fn category(&self, type_id: &str) -> Option<&str> {
        if !self.contains(type_id) {
            return None;
        }
        Some(
            self.descriptors
                .get(type_id)
                .map_or(crate::category::OTHER, |d| d.category.as_str()),
        )
    }

    /// 区分 `category` の種類が `type_id` か。
    pub fn is(&self, type_id: &str, category: &str) -> bool {
        self.category(type_id) == Some(category)
    }

    /// 区分 `category` の種類の一覧(登録順ではなく ID の順)。
    pub fn in_category<'a>(&'a self, category: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.type_ids().filter(move |t| self.is(t, category))
    }

    /// すべての種類の説明(ID の順)。
    pub fn descriptors(&self) -> Vec<Descriptor> {
        self.type_ids().filter_map(|t| self.descriptor(t)).collect()
    }

    pub fn contains(&self, type_id: &str) -> bool {
        self.factories.contains_key(type_id)
    }

    /// 種類に印を付ける(例: 加工の「荒削りの格子で削る」)。印の意味は、付けるモジュールと
    /// 読むモジュールが決める(コアが読むのは [`CONCURRENT`] だけ)。
    pub fn mark(&mut self, type_id: &str, mark: &'static str) {
        self.marks
            .entry(type_id.to_string())
            .or_default()
            .insert(mark);
    }

    /// 種類 `type_id` に印 `mark` が付いているか。
    pub fn has_mark(&self, type_id: &str, mark: &str) -> bool {
        self.marks.get(type_id).is_some_and(|m| m.contains(mark))
    }

    pub fn type_ids(&self) -> impl Iterator<Item = &str> {
        self.factories.keys().map(String::as_str)
    }

    /// モジュールの共有の値を置く(例: マシンライブラリ、PC 全体の設定の入れ物)。組み立ての時に
    /// 種類の登録と一緒に置き、ほかのモジュールは [`Registry::service`] で型を名指しして読む。
    /// 設定や控えをプロセス全体の static に置かず、組み立てた物(文書を開く単位)ごとに持つため。
    /// 同じ型を二度置くのはプログラムの誤りとして panic する(黙って片方を捨てない)。
    pub fn provide<T: std::any::Any + Send + Sync>(&mut self, value: std::sync::Arc<T>) {
        let prev = self.services.insert(std::any::TypeId::of::<T>(), value);
        assert!(
            prev.is_none(),
            "共有の値 {} が二重に置かれました",
            std::any::type_name::<T>()
        );
    }

    /// 文書の区画 `T` を名乗る。同じ鍵を二度名乗るのはプログラムの誤りとして panic する
    /// (2 つのモジュールが同じ区画を書き合わないため)。
    pub fn declare_section<T: crate::SectionType>(&mut self) {
        crate::section::check_key(T::KEY).expect("区画の鍵");
        let prev = self.sections.insert(T::KEY, T::LABEL);
        assert!(
            prev.is_none(),
            "文書の区画 {} が二重に名乗られました",
            T::KEY
        );
    }

    /// 名乗られた文書の区画(鍵と名前、鍵の順)。
    pub fn sections(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        self.sections.iter().map(|(k, v)| (*k, *v))
    }

    /// [`Registry::provide`] で置いた値(置いたモジュールが組み立てに無ければ `None`)。
    pub fn service<T: std::any::Any + Send + Sync>(&self) -> Option<std::sync::Arc<T>> {
        self.services
            .get(&std::any::TypeId::of::<T>())
            .cloned()
            .and_then(|v| v.downcast::<T>().ok())
    }

    pub fn instantiate(
        &self,
        type_id: &str,
        data: &serde_json::Value,
    ) -> Option<Result<Box<dyn Feature>, EvalError>> {
        self.factories.get(type_id).map(|f| f(data))
    }
}

/// 式の文字列から、参照するパラメータを入力として列挙する。
pub fn expr_inputs(src: &str) -> Result<Vec<InputRef>, EvalError> {
    Ok(kmb_expr::parse(src)?
        .references()
        .into_iter()
        .map(|name| InputRef::Param { name })
        .collect())
}

/// 評価中の機能に渡す文脈。宣言済みの入力だけを返す。
pub struct EvalCtx<'a> {
    pub(crate) id: FeatureId,
    pub(crate) declared: &'a [InputRef],
    pub(crate) params: &'a HashMap<String, Quantity>,
    pub(crate) outputs: &'a HashMap<FeatureId, Outputs>,
    pub(crate) kernel: &'a dyn Kernel,
    pub(crate) registry: &'a Registry,
}

impl EvalCtx<'_> {
    /// モジュールの共有の値([`Registry::service`])。
    pub fn service<T: std::any::Any + Send + Sync>(&self) -> Option<std::sync::Arc<T>> {
        self.registry.service::<T>()
    }

    /// 評価している機能の ID。面の名前の接頭辞に使う。
    pub fn feature_id(&self) -> FeatureId {
        self.id
    }

    fn is_param_declared(&self, name: &str) -> bool {
        self.declared
            .iter()
            .any(|i| matches!(i, InputRef::Param { name: n } if n == name))
    }

    pub fn param(&self, name: &str) -> Result<Quantity, EvalError> {
        if !self.is_param_declared(name) {
            return Err(EvalError::Undeclared(format!("パラメータ {name}")));
        }
        // 宣言済みの入力は、再計算が評価の前に存在と成否を確かめている
        self.params
            .get(name)
            .copied()
            .ok_or_else(|| EvalError::Other(format!("パラメータ {name} がありません")))
    }

    /// 式を評価する。式が参照するパラメータはすべて宣言済みでなければならない。
    pub fn expr(&self, src: &str) -> Result<Quantity, EvalError> {
        let e = kmb_expr::parse(src)?;
        for name in e.references() {
            if !self.is_param_declared(&name) {
                return Err(EvalError::Undeclared(format!("パラメータ {name}")));
            }
        }
        Ok(e.eval(&|n| self.params.get(n).copied())?)
    }

    pub fn output(&self, feature: FeatureId, slot: &str) -> Result<&Value, EvalError> {
        let declared = self.declared.iter().any(
            |i| matches!(i, InputRef::Output { feature: f, slot: s } if *f == feature && s == slot),
        );
        if !declared {
            return Err(EvalError::Undeclared(format!("{feature} の出力 {slot}")));
        }
        self.outputs
            .get(&feature)
            .and_then(|o| o.get(slot))
            .ok_or_else(|| EvalError::Other(format!("{feature} の出力 {slot} がありません")))
    }

    pub fn kernel(&self) -> &dyn Kernel {
        self.kernel
    }

    /// 機能の種類の登録(入れ子に計算した文書のボディを調べるのに使う)。
    pub fn registry(&self) -> &Registry {
        self.registry
    }

    /// 別の文書 `data` を、この計算の中で同じカーネルと機能の種類で計算する(リンクで組み込んだ部品)。
    /// 途中の形は、外の再計算の終わりに捨てられる(結果に残した形だけが残る)。入れ子は
    /// [`crate::NESTED_DEPTH`] 段まで。
    pub fn recompute_nested(
        &self,
        data: &crate::DocumentData,
    ) -> Result<crate::RecomputeReport, EvalError> {
        crate::recompute::recompute_nested(data, self.registry, self.kernel)
    }
}
