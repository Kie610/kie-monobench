//! 機能の道具を動かして作り直す機能: 直線のパターン、円形のパターン、ミラー。
//! 道具の作り方([`Recipe`])を持つ機能(押し出し、回転、穴、パターン・ミラー自身)に使える。
//! 機能はいくつでも選べる。例: `"feature": [4, 6]`
//! (1 つの時は前と同じく番号だけで書く)。並べた形を足すボディは `body`(画面は、機能の面を
//! クリックした時のそのボディを入れる)。省くと並べる機能のうち最後のもの(タイムラインの順。
//! 画面はその順に並べて持つ)の後のボディ。
//!
//! 種類を「ボディ」(`"kind": "bodies"`)にすると、選んだボディ(`bodies`)をそのまま動かして写し、
//! 元と足し合わせて 1 つのボディにする(フィレットした後の形も写せる)。
//! 離れた写しは 1 つのボディの中の別々の塊になる。「写しを別のボディにする」(`separate`)なら元を残し、
//! 写し 1 をこの機能のボディにする。写し 2 からは、写しを分ける操作が写しのボディの機能
//! ([`PATTERN_COPY`])を 1 つずつ足す(1 つの機能が 1 つのボディなので。足すだけでパターンは書き換えない)。
//! まだボディになっていない写し(個数を増やした分)と、個数が減って止まった写しのボディの機能は、
//! パターンの注意で知らせる。
//!
//! 切り取りの機能を並べる時は、写した道具の形を `tool_preview` に出す(直している間、画面が
//! 赤い半透明で描く。押し出しの切り取りと同じ)。
pub use kmb_cad::plane::PlaneRef;
use kmb_cad::*;
use kmb_core::kernel::{self as k, P3};
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, RefKind, Registry, Value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const LINEAR_PATTERN: &str = "solid.linear_pattern@1";
pub const CIRCULAR_PATTERN: &str = "solid.circular_pattern@1";
pub const MIRROR: &str = "solid.mirror@1";
/// ボディのパターン・ミラーの写しを別のボディとして出す機能。
pub const PATTERN_COPY: &str = "solid.pattern_copy@1";
/// 選んでいるボディのパターン・ミラーの写しを、1 つずつ別のボディに分ける操作。
pub const SPLIT_COPIES: &str = "cad.pattern_split_copies";
/// 選んだ面でミラーする操作。
pub const MIRROR_ON_FACE: &str = "cad.mirror_on_face";
/// パターンの軸(向き)を、原点の軸かスケッチの線のクリックで選ぶ操作。
pub const PATTERN_AXIS: &str = "cad.pattern_axis";
/// 直線のパターンの方向 2 を、原点の軸かスケッチの線のクリックで選ぶ操作。
pub const PATTERN_AXIS2: &str = "cad.pattern_axis2";
/// ミラーの対称の平面を、原点の平面か平らな面のクリックで選ぶ操作。
pub const MIRROR_PLANE: &str = "cad.mirror_plane";

/// 並べる物の種類。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    /// 機能の道具を作り直す(前からの形)
    #[default]
    Features,
    /// ボディをそのまま動かして写す
    Bodies,
}

impl PatternKind {
    fn is_features(&self) -> bool {
        *self == PatternKind::Features
    }
}

/// 並べる物(パターン・ミラーで共通)。
fn source_inputs(
    kind: PatternKind,
    features: &[FeatureId],
    bodies: &[FeatureId],
    body: Option<FeatureId>,
) -> Result<Vec<InputRef>, EvalError> {
    match kind {
        PatternKind::Features => pattern_inputs(features, body),
        PatternKind::Bodies => {
            if bodies.is_empty() {
                return Err(EvalError::InvalidData("写すボディを選んでください".into()));
            }
            Ok(bodies.iter().map(|b| output(*b, "body")).collect())
        }
    }
}

/// 抑制した時に後ろへ渡すボディ。
fn source_pass(
    kind: PatternKind,
    features: &[FeatureId],
    bodies: &[FeatureId],
    body: Option<FeatureId>,
) -> Vec<(String, InputRef)> {
    match kind {
        PatternKind::Features => pass_base(features, body),
        PatternKind::Bodies => bodies
            .first()
            .map(|b| vec![("body".into(), output(*b, "body"))])
            .unwrap_or_default(),
    }
}

/// 写しを別のボディにする時に、元のボディを残す(後ろの機能が使い切らない)。
fn kept(kind: PatternKind, separate: bool, bodies: &[FeatureId]) -> Vec<FeatureId> {
    if kind == PatternKind::Bodies && separate {
        bodies.to_vec()
    } else {
        Vec::new()
    }
}

/// 写しの番号の出力(`copy.1` から)。写しのボディの機能が読む。
fn copy_slot(i: usize) -> String {
    format!("copy.{i}")
}

/// 写しの数の出力(種類がボディの時。写しのボディの機能を足す操作が読む)。
const COPIES: &str = "copies";

/// ボディ `bodies` を `transforms` の位置へ写す。面の名前には写しの番号を足す。
/// `separate` でなければ、写しを元と足し合わせて 1 つのボディにする(ボディが 2 つ以上なら、
/// それも足し合わせる)。`separate` なら元は残し、写し 1 をこの機能のボディにする。写しは
/// どちらでも `copy.N` に出す(写しの順はボディごと、位置の順。写し 2 からは写しのボディの機能が読む)。
fn copy_bodies(
    ctx: &EvalCtx<'_>,
    bodies: &[FeatureId],
    transforms: &[k::Matrix4],
    separate: bool,
) -> Result<Outputs, EvalError> {
    let kernel = ctx.kernel();
    let tag = tag(ctx);
    let mut out: Option<kmb_core::ShapeHandle> = None;
    let add = |out: &mut Option<kmb_core::ShapeHandle>, h| -> Result<(), EvalError> {
        *out = Some(match out.take() {
            Some(b) => kernel
                .boolean(kmb_core::BooleanOp::Union, b, h)
                .map_err(kernel_err)?,
            None => h,
        });
        Ok(())
    };
    let mut copies = Vec::new();
    for b in bodies {
        let src = body_of(ctx, *b)?;
        if !separate {
            add(&mut out, src)?;
        }
        for (i, m) in transforms.iter().enumerate() {
            let copy = kernel
                .transform(src, m, &format!("@{tag}.{}", i + 1))
                .map_err(|e| EvalError::Kernel(format!("ボディを写せませんでした({e})")))?;
            copies.push(copy);
        }
    }
    if separate && copies.is_empty() {
        return Err(EvalError::InvalidData(
            "写しがありません。個数を 2 以上にしてください".into(),
        ));
    }
    let mut outputs = Outputs::new();
    for (i, h) in copies.iter().enumerate() {
        if !separate || i == 0 {
            add(&mut out, *h)?;
        }
        outputs.insert(copy_slot(i + 1), Value::Shape(*h));
    }
    let body = out.ok_or_else(|| EvalError::InvalidData("写すボディを選んでください".into()))?;
    outputs.insert("body".to_string(), Value::Shape(body));
    outputs.insert(
        COPIES.to_string(),
        Value::Scalar(kmb_core::Quantity::scalar(copies.len() as f64)),
    );
    Ok(outputs)
}

/// 写しのボディ。ボディのパターン・ミラー `of` の写し `index`(2 から)を、別のボディとして出す。
/// 写しを足す操作([`SPLIT_COPIES`])が、写しの数だけ足す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternCopy {
    pub of: FeatureId,
    pub index: usize,
}

impl Feature for PatternCopy {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![output(self.of, &copy_slot(self.index))])
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        match ctx.output(self.of, &copy_slot(self.index)) {
            Ok(Value::Shape(h)) => Ok([("body".to_string(), Value::Shape(*h))].into()),
            _ => Err(EvalError::InvalidData(format!(
                "写し {} はもうありません(パターンの個数が減りました)。個数を戻すか、この機能を消してください",
                self.index
            ))),
        }
    }
}

/// 予告の三角形の数の上限(超えた分は描かない。結果に JSON で残るので重くしない)。
const PREVIEW_TRIANGLES: usize = 20_000;
/// 予告の網の粗さ(mm)
const PREVIEW_TOLERANCE: f64 = 0.1;

/// 切り取り(交差)の機能を並べた時の、写した道具の形(`tool_preview`)。元の道具の網を 1 回だけ
/// 作り、写しの位置へ動かす(裏返す写しは三角形の向きも直す)。結合は結果の形が予告になるので出さない。
fn copies_preview(
    ctx: &EvalCtx<'_>,
    recipes: &[(Recipe, Operation)],
    transforms: &[k::Matrix4],
) -> Option<kmb_core::ToolPreview> {
    let op = match recipes.first()?.1 {
        Operation::Cut => kmb_core::ToolOp::Cut,
        Operation::Intersect => kmb_core::ToolOp::Intersect,
        Operation::New | Operation::Join => return None,
    };
    let kernel = ctx.kernel();
    let mut faces = Vec::new();
    let mut lines = Vec::new();
    for (recipe, _) in recipes {
        let tool = recipe.moved(&k::IDENTITY, Operation::New);
        let h = apply_recipe(ctx, &tool, None, &format!("{}:preview", tag(ctx))).ok()?;
        let mesh = kernel.mesh(h, PREVIEW_TOLERANCE).ok()?;
        let at = |i: u32| mesh.positions[i as usize].map(f64::from);
        for m in transforms {
            if faces.len() + mesh.triangles.len() > PREVIEW_TRIANGLES {
                break;
            }
            let flip = det3(m) < 0.0;
            let mv = |p: P3| k::transform_point(m, p);
            faces.extend(mesh.triangles.iter().map(|t| {
                let (a, b, c) = (mv(at(t[0])), mv(at(t[1])), mv(at(t[2])));
                if flip { [a, c, b] } else { [a, b, c] }
            }));
            lines.extend(
                mesh.edges
                    .iter()
                    .map(|l| l.iter().map(|p| mv(p.map(f64::from))).collect()),
            );
        }
    }
    Some(kmb_core::ToolPreview {
        op,
        lines,
        faces,
        profile: Vec::new(),
        arrows: Vec::new(),
    })
}

/// 行列の回す部分の行列式(負なら裏返す写し)。
fn det3(m: &k::Matrix4) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn pattern_inputs(
    features: &[FeatureId],
    body: Option<FeatureId>,
) -> Result<Vec<InputRef>, EvalError> {
    if features.is_empty() {
        return Err(EvalError::InvalidData("並べる機能を選んでください".into()));
    }
    Ok(features
        .iter()
        .flat_map(|f| [output(*f, "body"), output(*f, "recipe")])
        .chain(body.map(|b| output(b, "body")))
        .collect())
}

/// 並べた形を足すボディ(`body`。無ければ並べる機能のうち最後のものの後のボディ)。
fn base(features: &[FeatureId], body: Option<FeatureId>) -> FeatureId {
    body.unwrap_or_else(|| *features.last().expect("inputs で確かめている"))
}

fn pass_base(features: &[FeatureId], body: Option<FeatureId>) -> Vec<(String, InputRef)> {
    body.or(features.last().copied())
        .map(|f| vec![("body".into(), output(f, "body"))])
        .unwrap_or_default()
}

/// 足すボディの欄(空なら最後の機能の後のボディ)。
fn body_field() -> Field {
    Field::pick("body", "足すボディ", &[RefKind::Body], 0, Some(1))
        .help(
            "並べた形を足すボディ。機能の面をクリックすると、そのボディが入る。空なら、最後の機能の後のボディ",
        )
        .only("kind", "features", "features")
}

/// 機能の欄(3D のビューで面をクリックすると、その面を作った機能が入る)。
fn feature_field(label: &str) -> Field {
    Field::pick("feature", label, &[RefKind::Feature], 1, None)
        .help(
            "押し出し・回転・穴・パターン・ミラーの機能(いくつでも)。3D のビューで、その機能が作った面をクリックして選ぶ。もう一度クリックすると外す",
        )
        .only("kind", "features", "features")
}

/// 種類の欄(機能か、ボディか)。
fn kind_field() -> Field {
    Field::choice("kind", "種類", &[("features", "機能"), ("bodies", "ボディ")]).help(
        "機能: 押し出しや穴などの機能の形を作り直して並べる。ボディ: ボディをそのまま写し、元と足し合わせて 1 つのボディにする(フィレットした後の形も写せる)。先にボディを選んでから始めるとボディになる",
    )
}

/// 写すボディの欄(種類がボディの時)。
fn bodies_field() -> Field {
    Field::pick("bodies", "写すボディ", &[RefKind::Body], 1, Some(1))
        .help("写すボディ。3D のビューでボディをクリックして選ぶ")
        .only("kind", "bodies", "features")
}

/// 写しを別のボディにする欄(種類がボディの時)。
fn separate_field() -> Field {
    Field::bool("separate", "写しを別のボディにする")
        .help("元を残し、写しを元と足し合わせない。写し 1 はこの機能のボディ。写し 2 から先は、右クリックの「写しを別のボディに分ける」で 1 つずつボディになる")
        .only("kind", "bodies", "features")
}

/// 機能の道具の作り方。
fn recipe_of(ctx: &EvalCtx<'_>, feature: FeatureId) -> Result<Recipe, EvalError> {
    <Recipe as kmb_core::Payload>::from_value(ctx.output(feature, "recipe")?).ok_or_else(|| {
        EvalError::InvalidData(format!(
            "{feature} はパターンやミラーにできる機能ではありません"
        ))
    })
}

/// 複製に効かせる演算(新しいボディは結合にする)。
fn pattern_operation(recipe: &Recipe) -> Result<Operation, EvalError> {
    match recipe.operation() {
        Operation::New | Operation::Join => Ok(Operation::Join),
        Operation::Cut => Ok(Operation::Cut),
        Operation::Intersect => Err(EvalError::InvalidData(
            "交差の機能はパターンやミラーにできません".into(),
        )),
    }
}

/// 機能の道具を `transforms` の位置で作り直し、同じ演算をくり返す。
/// 新しいボディを作る機能なら、複製を結合する。
fn repeat(
    ctx: &EvalCtx<'_>,
    features: &[FeatureId],
    body: Option<FeatureId>,
    transforms: &[k::Matrix4],
) -> Result<Outputs, EvalError> {
    let mut body = body_of(ctx, base(features, body))?;
    let mut recipes = Vec::new();
    for f in features {
        let recipe = recipe_of(ctx, *f)?;
        let op = pattern_operation(&recipe)?;
        recipes.push((*f, recipe, op));
    }
    let mut parts: Vec<Recipe> = recipes
        .iter()
        .map(|(_, r, op)| r.moved(&k::IDENTITY, *op))
        .collect();
    let mut misses = Misses::default();
    for (i, m) in transforms.iter().enumerate() {
        for (f, recipe, op) in &recipes {
            let r = recipe.moved(m, *op);
            body = misses.apply(ctx, &r, *op, body, &format!("f{}@{}", f.0, i + 1))?;
            parts.push(r);
        }
    }
    let preview: Vec<(Recipe, Operation)> = recipes.into_iter().map(|(_, r, op)| (r, op)).collect();
    let mut out = outputs(body, parts);
    with_preview(&mut out, copies_preview(ctx, &preview, transforms));
    misses.warn(&mut out);
    Ok(out)
}

/// 切り取りの写しのうち、本体に当たらず何も削らなかった数(本体の外に並んだ穴に気付けるように)。
#[derive(Default)]
struct Misses {
    cuts: usize,
    missed: usize,
}

impl Misses {
    /// 写し `r` を `body` に効かせる。切り取りなら体積が減ったかを見る(体積を測れなければ数えない)。
    fn apply(
        &mut self,
        ctx: &EvalCtx<'_>,
        r: &Recipe,
        op: Operation,
        body: kmb_core::ShapeHandle,
        tag: &str,
    ) -> Result<kmb_core::ShapeHandle, EvalError> {
        if op != Operation::Cut {
            return apply_recipe(ctx, r, Some(body), tag);
        }
        let before = ctx.kernel().mass(body).ok().map(|m| m.volume);
        let after = apply_recipe(ctx, r, Some(body), tag)?;
        if let (Some(b), Ok(m)) = (before, ctx.kernel().mass(after)) {
            self.cuts += 1;
            // 体積の誤差より小さい変化は削っていないとみなす
            if b - m.volume <= 1e-6 * b.abs().max(1.0) {
                self.missed += 1;
            }
        }
        Ok(after)
    }

    /// 当たらなかった写しがあれば注意を足す。
    fn warn(&self, out: &mut Outputs) {
        if self.missed > 0 {
            kmb_core::add_warnings(
                out,
                vec![format!(
                    "{} 個の写しが本体に当たっていません(切り取りの写し {} 個のうち。間隔・個数・向きを確かめてください)",
                    self.missed, self.cuts
                )],
            );
        }
    }
}

/// 予告の形があれば出力に足す。
fn with_preview(out: &mut Outputs, preview: Option<kmb_core::ToolPreview>) {
    if let Some(p) = preview {
        out.insert(kmb_core::TOOL_PREVIEW.into(), kmb_core::Payload::to_value(&p));
    }
}

/// パターン・ミラーの出力。道具の作り方(元と写しのすべて)も出し、続けて別のパターンや
/// ミラーに使えるようにする(並べた形をさらにミラーする、など)。
fn outputs(body: kmb_core::ShapeHandle, parts: Vec<Recipe>) -> Outputs {
    let parts = parts
        .into_iter()
        .flat_map(|p| match p {
            Recipe::Many { parts } => parts,
            p => vec![p],
        })
        .collect();
    [
        ("body".to_string(), Value::Shape(body)),
        (
            "recipe".to_string(),
            kmb_core::Payload::to_value(&Recipe::Many { parts }),
        ),
    ]
    .into()
}

/// 直線に並べる。例: `{"feature": 4, "direction": [1, 0, 0], "spacing": "10 mm", "count": "5"}`
/// `count` は元を含む個数。向きは `direction_ref` で原点の軸やスケッチの線にもできる
/// (例: `"direction_ref": {"kind": "origin_y"}`。省くと `direction`)。
/// 方向 2 の `count2`(既定 1)を 2 以上にすると格子に並べる。方向 2 の向きは
/// `direction2_ref`・`direction2` で、省くと方向 1 に直角な XY 平面の中の向き。
/// 方向ごとに、距離 `spacing` を写し同士の間隔にするか全体の幅にするか(`distance_type`・
/// `distance_type2`)、元を中心に両側へ並べるか(`symmetric`・`symmetric2`。個数は片側の数)を選べる
/// (スケッチの矩形状パターンと同じ決まり)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearPattern {
    #[serde(default, skip_serializing_if = "PatternKind::is_features")]
    pub kind: PatternKind,
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub feature: Vec<FeatureId>,
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub bodies: Vec<FeatureId>,
    /// 写しを元と別のボディにする(種類がボディの時)。元はそのまま残り、写し 1 はこの機能の
    /// ボディ、写し 2 からは写しのボディの機能([`PATTERN_COPY`])のボディになる
    #[serde(default, skip_serializing_if = "is_false")]
    pub separate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<FeatureId>,
    #[serde(default, skip_serializing_if = "AxisRef::is_vector")]
    pub direction_ref: AxisRef,
    #[serde(default = "x_dir")]
    pub direction: P3,
    pub spacing: String,
    pub count: String,
    #[serde(default, skip_serializing_if = "DistanceType::is_spacing")]
    pub distance_type: DistanceType,
    #[serde(default, skip_serializing_if = "is_false")]
    pub symmetric: bool,
    #[serde(default, skip_serializing_if = "AxisRef::is_vector")]
    pub direction2_ref: AxisRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction2: Option<P3>,
    #[serde(default = "spacing2")]
    pub spacing2: String,
    #[serde(default = "one")]
    pub count2: String,
    #[serde(default, skip_serializing_if = "DistanceType::is_spacing")]
    pub distance_type2: DistanceType,
    #[serde(default, skip_serializing_if = "is_false")]
    pub symmetric2: bool,
}

/// 直線のパターンの距離の決め方。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistanceType {
    /// 写し同士の間隔
    #[default]
    Spacing,
    /// 両端の写しの間の全体の幅
    Extent,
}

impl DistanceType {
    fn is_spacing(&self) -> bool {
        *self == DistanceType::Spacing
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 1 方向の並びの位置(元からの距離。元の 0 は除く)。`n` は元を含む数(対称なら片側の数)。
fn offsets(n: usize, dist: f64, kind: DistanceType, symmetric: bool) -> Vec<f64> {
    if n < 2 {
        return Vec::new();
    }
    let m = (n - 1) as f64;
    let step = match (kind, symmetric) {
        (DistanceType::Spacing, _) => dist,
        (DistanceType::Extent, false) => dist / m,
        (DistanceType::Extent, true) => dist / (2.0 * m),
    };
    let n = n as i64;
    let from = if symmetric { -(n - 1) } else { 0 };
    (from..n).map(|i| step * i as f64).collect()
}

impl Default for LinearPattern {
    fn default() -> Self {
        LinearPattern {
            kind: PatternKind::Features,
            feature: Vec::new(),
            bodies: Vec::new(),
            separate: false,
            body: None,
            direction_ref: AxisRef::Vector,
            direction: x_dir(),
            spacing: "10 mm".into(),
            count: "3".into(),
            direction2_ref: AxisRef::Vector,
            direction2: None,
            spacing2: spacing2(),
            count2: one(),
            distance_type: DistanceType::Spacing,
            symmetric: false,
            distance_type2: DistanceType::Spacing,
            symmetric2: false,
        }
    }
}

fn spacing2() -> String {
    "10 mm".into()
}

/// 方向 2 を持たない前のデータに、既定の間隔 2 と個数 2 を入れる(欄が空に見えないように)。
fn fill_grid(d: &mut serde_json::Value) {
    if let Some(o) = d.as_object_mut() {
        o.entry("spacing2").or_insert_with(|| json!(spacing2()));
        o.entry("count2").or_insert_with(|| json!(one()));
    }
}

fn one() -> String {
    "1".into()
}

/// 方向 1 に直角な、XY 平面の中の向き(方向 1 が Z 軸に沿う時は X)。成分の大きい方を正にする。
fn across(d: P3) -> P3 {
    let c = k::cross([0.0, 0.0, 1.0], d);
    if k::dot(c, c) < 1e-12 {
        return [1.0, 0.0, 0.0];
    }
    let c = k::normalize(c);
    let big = if c[0].abs() >= c[1].abs() { c[0] } else { c[1] };
    if big < 0.0 { k::scale(c, -1.0) } else { c }
}

impl Exprs for LinearPattern {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![
            &mut self.spacing,
            &mut self.count,
            &mut self.spacing2,
            &mut self.count2,
        ]
    }
}

impl Feature for LinearPattern {
    fn keeps(&self) -> Vec<FeatureId> {
        kept(self.kind, self.separate, &self.bodies)
    }

    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = source_inputs(self.kind, &self.feature, &self.bodies, self.body)?;
        v.extend(self.direction_ref.inputs());
        v.extend(self.direction2_ref.inputs());
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        source_pass(self.kind, &self.feature, &self.bodies, self.body)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let dir = match self.direction_ref.resolve(ctx)? {
            Some((_, d)) => d,
            None => self.direction,
        };
        if k::dot(dir, dir) < 1e-18 {
            return Err(EvalError::InvalidData("方向が 0 です".into()));
        }
        let dir = k::normalize(dir);
        let s = length(ctx, &self.spacing, "距離")?;
        let n = count(ctx, &self.count)?;
        let n2 = count(ctx, &self.count2)?;
        let (dir2, s2) = if n2 > 1 {
            let d2 = match self.direction2_ref.resolve(ctx)? {
                Some((_, d)) => d,
                None => self.direction2.unwrap_or_else(|| across(dir)),
            };
            if k::dot(d2, d2) < 1e-18 {
                return Err(EvalError::InvalidData("方向 2 が 0 です".into()));
            }
            let d2 = k::normalize(d2);
            if k::dot(k::cross(dir, d2), k::cross(dir, d2)) < 1e-12 {
                return Err(EvalError::InvalidData(
                    "方向 2 が方向 1 と平行です。別の向きを選んでください".into(),
                ));
            }
            (d2, length(ctx, &self.spacing2, "距離 2")?)
        } else {
            ([0.0; 3], 0.0)
        };
        // 格子の位置(元の (0, 0) は除く)。方向 1 を先に進める
        let mut a = offsets(n, s, self.distance_type, self.symmetric);
        let mut b = offsets(n2, s2, self.distance_type2, self.symmetric2);
        for v in [&mut a, &mut b] {
            if v.is_empty() {
                v.push(0.0);
            }
        }
        let ms: Vec<k::Matrix4> = b
            .iter()
            .flat_map(|&y| a.iter().map(move |&x| (x, y)))
            .filter(|&(x, y)| x != 0.0 || y != 0.0)
            .map(|(x, y)| k::translation(k::add(k::scale(dir, x), k::scale(dir2, y))))
            .collect();
        match self.kind {
            PatternKind::Features => repeat(ctx, &self.feature, self.body, &ms),
            PatternKind::Bodies => copy_bodies(ctx, &self.bodies, &ms, self.separate),
        }
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

fn distance_type_field(key: &str, label: &str) -> Field {
    Field::choice(key, label, &[("spacing", "間隔"), ("extent", "全体の幅")])
        .help("間隔: 写し同士の間の距離で決める。全体の幅: 両端の写しの間の距離で決め、その中に等しく並べる")
}

fn symmetric_field(key: &str, label: &str) -> Field {
    Field::bool(key, label)
        .help("元を中心に、両側へ同じ数ずつ並べる(個数は片側の数、全体の幅は両端の間)")
}

/// 軸まわりに並べる。`angle` が 360° なら等分、それ未満なら両端を含めて等分。
/// 軸の通る点 `origin` を省くと、機能の後のボディの外形の中心(板の中心で回すなど)。
/// 軸は `axis_ref` で原点の軸やスケッチの線にもできる(例: `"axis_ref": {"kind": "origin_z"}`。
/// 省くと `origin` と `axis`)。
/// 例: `{"feature": 4, "origin": [0,0,0], "axis": [0,0,1], "count": "6"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CircularPattern {
    #[serde(default, skip_serializing_if = "PatternKind::is_features")]
    pub kind: PatternKind,
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub feature: Vec<FeatureId>,
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub bodies: Vec<FeatureId>,
    /// 写しを元と別のボディにする(種類がボディの時)。元はそのまま残り、写し 1 はこの機能の
    /// ボディ、写し 2 からは写しのボディの機能([`PATTERN_COPY`])のボディになる
    #[serde(default, skip_serializing_if = "is_false")]
    pub separate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<FeatureId>,
    #[serde(default, skip_serializing_if = "AxisRef::is_vector")]
    pub axis_ref: AxisRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<P3>,
    #[serde(default = "z_dir")]
    pub axis: P3,
    pub count: String,
    #[serde(default = "full_turn")]
    pub angle: String,
}

impl Default for CircularPattern {
    fn default() -> Self {
        CircularPattern {
            kind: PatternKind::Features,
            feature: Vec::new(),
            bodies: Vec::new(),
            separate: false,
            body: None,
            axis_ref: AxisRef::Vector,
            origin: None,
            axis: z_dir(),
            count: "6".into(),
            angle: full_turn(),
        }
    }
}

impl Exprs for CircularPattern {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.count, &mut self.angle]
    }
}

impl Feature for CircularPattern {
    fn keeps(&self) -> Vec<FeatureId> {
        kept(self.kind, self.separate, &self.bodies)
    }

    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = source_inputs(self.kind, &self.feature, &self.bodies, self.body)?;
        v.extend(self.axis_ref.inputs());
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        source_pass(self.kind, &self.feature, &self.bodies, self.body)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let (origin, axis) = match self.axis_ref.resolve(ctx)? {
            Some(a) => a,
            None => (
                origin_or_center(ctx, self.origin, self.center_of()?)?,
                self.axis,
            ),
        };
        if k::dot(axis, axis) < 1e-18 {
            return Err(EvalError::InvalidData("軸の向きが 0 です".into()));
        }
        let n = count(ctx, &self.count)?;
        let total = angle(ctx, &self.angle, "角度")?;
        let full = (total.abs() - std::f64::consts::TAU).abs() < 1e-9;
        let step = if full || n < 2 {
            total / n as f64
        } else {
            total / (n - 1) as f64
        };
        let ms: Vec<k::Matrix4> = (1..n)
            .map(|i| k::rotation(origin, axis, step * i as f64))
            .collect();
        match self.kind {
            PatternKind::Features => repeat(ctx, &self.feature, self.body, &ms),
            PatternKind::Bodies => copy_bodies(ctx, &self.bodies, &ms, self.separate),
        }
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

impl CircularPattern {
    /// 通る点を省いた時に中心を取るボディ(機能なら最後の機能の後、ボディなら選んだボディ)。
    fn center_of(&self) -> Result<FeatureId, EvalError> {
        center_of(self.kind, &self.feature, &self.bodies, self.body)
    }
}

/// 通る点を省いた時に中心を取るボディ。
fn center_of(
    kind: PatternKind,
    features: &[FeatureId],
    bodies: &[FeatureId],
    body: Option<FeatureId>,
) -> Result<FeatureId, EvalError> {
    match kind {
        PatternKind::Features if !features.is_empty() => Ok(base(features, body)),
        PatternKind::Features => Err(EvalError::InvalidData("並べる機能を選んでください".into())),
        PatternKind::Bodies => bodies
            .first()
            .copied()
            .ok_or_else(|| EvalError::InvalidData("写すボディを選んでください".into())),
    }
}

fn x_dir() -> P3 {
    [1.0, 0.0, 0.0]
}

fn z_dir() -> P3 {
    [0.0, 0.0, 1.0]
}

/// 平面について写す(ミラー)。元の機能の形を、点 `origin` を通り法線が `normal` の平面の
/// 反対側に作り、同じ演算(新しいボディなら結合)を効かせる。`origin` を省くと、
/// 機能の後のボディの外形の中心(板の中央で写すなど)。平面は `plane_ref` で原点の平面や
/// 平らな面にもできる(例: `"plane_ref": {"kind": "origin_yz"}`、
/// `{"kind": "face", "feature": 5, "face": "f5:side:e1"}`。省くと `origin` と `normal`)。
/// 種類を「ボディ」にすると、選んだボディを写して元と足し合わせる。
/// 例: `{"feature": 4, "origin": [30, 0, 0], "normal": [1, 0, 0]}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mirror {
    #[serde(default, skip_serializing_if = "PatternKind::is_features")]
    pub kind: PatternKind,
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub feature: Vec<FeatureId>,
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub bodies: Vec<FeatureId>,
    /// 写しを元と別のボディにする(種類がボディの時)。元はそのまま残り、写し 1 はこの機能の
    /// ボディ、写し 2 からは写しのボディの機能([`PATTERN_COPY`])のボディになる
    #[serde(default, skip_serializing_if = "is_false")]
    pub separate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<FeatureId>,
    #[serde(default, skip_serializing_if = "PlaneRef::is_vector")]
    pub plane_ref: PlaneRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<P3>,
    #[serde(default = "x_dir")]
    pub normal: P3,
}

impl Default for Mirror {
    fn default() -> Self {
        Mirror {
            kind: PatternKind::Features,
            feature: Vec::new(),
            bodies: Vec::new(),
            separate: false,
            body: None,
            plane_ref: PlaneRef::Vector,
            origin: None,
            normal: x_dir(),
        }
    }
}

/// 平面(点 `origin`、法線 `normal`)について写す行列。
fn reflection(origin: P3, normal: P3) -> k::Matrix4 {
    let n = k::normalize(normal);
    let d = 2.0 * k::dot(origin, n);
    let mut m = k::IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] -= 2.0 * n[i] * n[j];
        }
        m[i][3] = d * n[i];
    }
    m
}

impl Feature for Mirror {
    fn keeps(&self) -> Vec<FeatureId> {
        kept(self.kind, self.separate, &self.bodies)
    }

    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = source_inputs(self.kind, &self.feature, &self.bodies, self.body)?;
        v.extend(self.plane_ref.inputs());
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        source_pass(self.kind, &self.feature, &self.bodies, self.body)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let at = center_of(self.kind, &self.feature, &self.bodies, self.body)?;
        let (origin, normal) = match self.plane_ref.resolve(ctx, "対称の平面")? {
            Some(p) => p,
            None => (origin_or_center(ctx, self.origin, at)?, self.normal),
        };
        if k::dot(normal, normal) < 1e-18 {
            return Err(EvalError::InvalidData("平面の法線が 0 です".into()));
        }
        if self.kind == PatternKind::Bodies {
            return copy_bodies(
                ctx,
                &self.bodies,
                &[reflection(origin, normal)],
                self.separate,
            );
        }
        let mut body = body_of(ctx, at)?;
        let mut parts = Vec::new();
        let mut copies = Vec::new();
        let mut preview = Vec::new();
        for f in &self.feature {
            let recipe = recipe_of(ctx, *f)?;
            let op = pattern_operation(&recipe)?;
            parts.push(recipe.moved(&k::IDENTITY, op));
            preview.push((recipe.clone(), op));
            copies.push((*f, recipe.mirrored(origin, normal, op)));
        }
        let mut misses = Misses::default();
        for (f, r) in copies {
            let op = r.operation();
            body = misses.apply(ctx, &r, op, body, &format!("f{}@m", f.0))?;
            parts.push(r);
        }
        let mut out = outputs(body, parts);
        with_preview(
            &mut out,
            copies_preview(ctx, &preview, &[reflection(origin, normal)]),
        );
        misses.warn(&mut out);
        Ok(out)
    }
}

/// 軸や平面の通る点。省いた時は、機能 `feature` の後のボディの外形の箱の中心。
fn origin_or_center(
    ctx: &EvalCtx<'_>,
    origin: Option<P3>,
    feature: FeatureId,
) -> Result<P3, EvalError> {
    if let Some(o) = origin {
        return Ok(o);
    }
    let (min, max) = ctx
        .kernel()
        .bounds(body_of(ctx, feature)?)
        .map_err(kernel_err)?;
    Ok([0, 1, 2].map(|i| (min[i] + max[i]) / 2.0))
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(LINEAR_PATTERN, "矩形状パターン")
            .menu("パターン")
            .icon("linpat")
            .pinned()
            .order(-20)
            .hint("機能の形を一定の間隔で並べる")
            .aliases("ぱたーん pattern linear ちょくせん ならべる こうし 格子 矩形 rectangular")
            .field(kind_field())
            .field(feature_field("並べるフィーチャ"))
            .field(bodies_field())
            .field(separate_field())
            .field(body_field())
            .field(
                Field::choice(
                    "direction_ref.kind",
                    "向き",
                    &kmb_cad::axis::axis_ref_choices("数で指定"),
                )
                .pick_on("line", PATTERN_AXIS)
                .hide_choice("edge"),
            )
            .field(Field::vector("direction", "向き(X, Y, Z)").unitless().only(
                "direction_ref.kind",
                "vector",
                "vector",
            ))
            .field(distance_type_field("distance_type", "距離の決め方"))
            .field(Field::expr("spacing", "距離").length().help("距離の決め方が「間隔」なら写し同士の間、「全体の幅」なら両端の写しの間"))
            .field(Field::expr("count", "個数").unitless().help("元を含む数(3 なら元と写し 2 つ)。対称なら片側の数"))
            .field(symmetric_field("symmetric", "対称"))
            .field(
                Field::choice(
                    "direction2_ref.kind",
                    "向き 2",
                    &kmb_cad::axis::axis_ref_choices("数で指定"),
                )
                .pick_on("line", PATTERN_AXIS2)
                .hide_choice("edge")
                .help("方向 2(格子のもう 1 つの向き)。個数 2 を 2 以上にすると使う"),
            )
            .field(
                Field::vector("direction2", "向き 2(X, Y, Z)").unitless()
                    .help("空なら、向きに直角な XY 平面の中の向き")
                    .only("direction2_ref.kind", "vector", "vector"),
            )
            .field(distance_type_field("distance_type2", "距離の決め方 2"))
            .field(Field::expr("spacing2", "距離 2").length())
            .field(
                Field::expr("count2", "個数 2").unitless()
                    .help("方向 2 の、元を含む数(対称なら片側の数)。1 なら向きの 1 列だけ(2 以上で格子)"),
            )
            .field(symmetric_field("symmetric2", "対称 2"))
            .on_bodies("kind", "bodies")
            .upgrade(fill_grid)
            .template(json!({"direction": [1.0, 0.0, 0.0], "spacing": "10 mm", "count": "3", "spacing2": "10 mm", "count2": "1"})),
        |d| Ok(Box::new(parse::<LinearPattern>(d)?)),
    );
    reg.register_described(
        solid(MIRROR, "ミラー")
            .menu("パターン")
            .icon("mirror")
            .pinned()
            // パターンの前の並び: ミラー・矩形状・円形状(Kie 2026-10-21 の画板)
            .order(-30)
            .hint("機能の形を、平面の反対側に写して作る(新しいボディの機能なら結合、切り取りなら切り取り)")
            .aliases("みらー mirror たいしょう うつす はんてん")
            .field(kind_field())
            .field(feature_field("写すフィーチャ"))
            .field(bodies_field())
            .field(separate_field())
            .field(body_field())
            .field(
                Field::choice("plane_ref.kind", "対称の平面", &kmb_cad::plane::plane_ref_choices("通る点と法線で指定"))
                    .pick_on("face", MIRROR_PLANE)
                    .help("原点の平面か、ボディの平らな面(クリックで選ぶ)。「数で指定」なら通る点と法線"),
            )
            .field(Field::info("plane_ref.face", "対称の面").only("plane_ref.kind", "face", "vector"))
            .field(
                Field::vector("origin", "平面の通る点").length()
                    .help("空なら形の外形の中心")
                    .only("plane_ref.kind", "vector", "vector"),
            )
            .field(Field::vector("normal", "平面の法線").unitless().only("plane_ref.kind", "vector", "vector"))
            .on_bodies("kind", "bodies")
            .fix("", "写す平面の「通る点」と「法線」が埋まっているか確かめる(面を右クリックの「この面でミラー」なら両方入る)")
            .template(json!({"normal": [1.0, 0.0, 0.0]})),
        |d| Ok(Box::new(parse::<Mirror>(d)?)),
    );
    reg.register_described(
        solid(CIRCULAR_PATTERN, "円形状パターン")
            .menu("パターン")
            .icon("circpat")
            .pinned()
            .order(-10)
            .hint("機能の形を軸まわりに並べる")
            .aliases("ぱたーん pattern circular えんけい ならべる")
            .field(kind_field())
            .field(feature_field("並べるフィーチャ"))
            .field(bodies_field())
            .field(separate_field())
            .field(body_field())
            .field(
                Field::choice(
                    "axis_ref.kind",
                    "軸",
                    &kmb_cad::axis::axis_ref_choices("通る点と向きで指定"),
                )
                .pick_on("line", PATTERN_AXIS)
                .hide_choice("edge"),
            )
            .field(
                Field::vector("origin", "軸の通る点")
                    .length()
                    .help("空なら形の外形の中心")
                    .only("axis_ref.kind", "vector", "vector"),
            )
            .field(Field::vector("axis", "軸の向き").unitless().only(
                "axis_ref.kind",
                "vector",
                "vector",
            ))
            .field(
                Field::expr("count", "個数")
                    .unitless()
                    .help("元を含む数(6 なら元と写し 5 つ)"),
            )
            .field(Field::expr("angle", "全体の角度").angle())
            .on_bodies("kind", "bodies")
            .template(json!({"axis": [0.0, 0.0, 1.0], "count": "6", "angle": "360 deg"})),
        |d| Ok(Box::new(parse::<CircularPattern>(d)?)),
    );
    reg.register_described(
        solid(PATTERN_COPY, "写しのボディ")
            .icon("linpat")
            .hint("ボディのパターン・ミラーの写しの 1 つを、別のボディにした物。並べ方はパターン・ミラーの方で直す")
            .field(
                Field::info("of", "パターン")
                    .help("この写しを作ったボディのパターン・ミラー。個数や間隔はそちらで直す"),
            )
            .field(Field::info("index", "写しの番号")),
        |d| Ok(Box::new(parse::<PatternCopy>(d)?)),
    );
}

use kmb_core::{Action, Context, Extension, Gesture, Handle, Outcome, Place};

/// 選んだ平らな面を対称の平面にして、ミラーを足す(写す機能は、写せる機能のうち最後のもの。
/// 足した後に欄でクリックして選び直せる)。
fn mirror(cx: &Context<'_>) -> Result<Outcome, String> {
    let plane = cx.picked_plane().ok_or(NEED_FACE)?;
    let d = cx
        .registry
        .descriptor(MIRROR)
        .ok_or("ミラーが登録されていません")?;
    let (mut data, _, _) = d
        .instantiate_with(&|slot| cx.candidates(slot), &[], &|_| false)
        .map_err(|label| format!("先に「{label}」に使える機能が要ります"))?;
    data["origin"] = serde_json::json!(plane.point);
    data["normal"] = serde_json::json!(plane.normal);
    Ok(Outcome::insert(
        "ミラーを追加",
        MIRROR,
        format!("{}{}", d.label, cx.count_of(MIRROR) + 1),
        data,
    ))
}

/// 選んでいるパターン(機能の番号、データ、軸を入れる項目)。
fn selected_pattern(cx: &Context<'_>) -> Option<(FeatureId, serde_json::Value, &'static str)> {
    let id = cx.selected?;
    let entry = cx.data.feature(id)?;
    let key = match entry.type_id.as_str() {
        CIRCULAR_PATTERN => "axis_ref",
        LINEAR_PATTERN => "direction_ref",
        _ => return None,
    };
    Some((id, entry.data.clone(), key))
}

/// パターンの軸を、手順で選んだ値(`axis`)にする。`second` なら直線のパターンの方向 2。
fn pattern_axis(cx: &Context<'_>, second: bool) -> Result<Outcome, String> {
    let (id, mut data, key) = selected_pattern(cx).ok_or("円形か直線のパターンを選んでいません")?;
    let key = if second {
        if key != "direction_ref" {
            return Err("方向 2 は直線のパターンだけです".into());
        }
        "direction2_ref"
    } else {
        key
    };
    let v = cx
        .arg("axis")
        .ok_or("axis に軸(x・y・z か {\"sketch\": スケッチ, \"line\": 線の番号})を指定します")?;
    let axis = AxisRef::from_choice(v, cx)?;
    data[key] = serde_json::to_value(axis).map_err(|e| e.to_string())?;
    Ok(Outcome::Change {
        label: if second {
            "パターンの方向 2 を選ぶ"
        } else {
            "パターンの軸を選ぶ"
        }
        .into(),
        edits: vec![kmb_core::Edit::SetData { id, data }],
    })
}

/// 選んでいる、種類がボディのパターン・ミラー(機能の番号とデータ)。
fn selected_body_pattern(cx: &Context<'_>) -> Option<(FeatureId, serde_json::Value)> {
    body_pattern(cx, cx.selected?)
}

/// 機能 `id` が種類がボディのパターン・ミラーなら、その番号とデータ。
fn body_pattern(cx: &Context<'_>, id: FeatureId) -> Option<(FeatureId, serde_json::Value)> {
    let entry = cx.data.feature(id)?;
    if ![LINEAR_PATTERN, CIRCULAR_PATTERN, MIRROR].contains(&entry.type_id.as_str())
        || entry.data.get("kind").and_then(|v| v.as_str()) != Some("bodies")
    {
        return None;
    }
    Some((id, entry.data.clone()))
}

/// 写しの数(パターンが計算できていなければ `None`)と、写しのボディの機能が受け持つ写しの番号。
fn copy_state(cx: &Context<'_>, id: FeatureId) -> (Option<usize>, Vec<usize>) {
    let copies = match cx.report.feature(id) {
        Some(kmb_core::FeatureStatus::Ok { outputs, .. }) => match outputs.get(COPIES) {
            Some(Value::Scalar(q)) => Some(q.value as usize),
            _ => None,
        },
        _ => None,
    };
    let taken = cx
        .data
        .features
        .iter()
        .filter(|f| f.type_id == PATTERN_COPY)
        .filter_map(|f| parse::<PatternCopy>(&f.data).ok())
        .filter(|c| c.of == id)
        .map(|c| c.index)
        .collect();
    (copies, taken)
}

/// 選んでいるボディのパターン・ミラー(写しを別のボディにする印つき)の、まだボディになっていない
/// 写し(写し 2 から。個数を増やした分を含む)に、写しのボディの機能を 1 つずつ足す。
fn split_copies(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, data) =
        selected_body_pattern(cx).ok_or("種類がボディのパターンかミラーを選んでいません")?;
    if data.get("separate").and_then(|v| v.as_bool()) != Some(true) {
        return Err("先に欄「写しを別のボディにする」を入れてください".into());
    }
    let (copies, taken) = copy_state(cx, id);
    let copies = copies.ok_or("パターンが計算できていません。先に誤りを直してください")?;
    let missing: Vec<usize> = (2..=copies).filter(|i| !taken.contains(i)).collect();
    if missing.is_empty() {
        return Err("写しはもう全部別のボディです".into());
    }
    let label = cx
        .registry
        .descriptor(PATTERN_COPY)
        .map_or_else(|| "写しのボディ".to_string(), |d| d.label);
    let n = cx.count_of(PATTERN_COPY);
    let edits = missing
        .into_iter()
        .enumerate()
        .map(|(k, index)| kmb_core::Edit::Insert {
            type_id: PATTERN_COPY.into(),
            name: format!("{label}{}", n + k + 1),
            data: json!({"of": id.0, "index": index}),
        })
        .collect();
    Ok(Outcome::Change {
        label: "写しを別のボディに分ける".into(),
        edits,
    })
}

/// 写しを別のボディにするパターン・ミラーの注意: まだボディになっていない写しと、個数が減って
/// 止まった写しのボディの機能。
fn copy_notes(cx: &Context<'_>, id: FeatureId) -> Vec<String> {
    let Some((_, data)) = body_pattern(cx, id) else {
        return Vec::new();
    };
    if data.get("separate").and_then(|v| v.as_bool()) != Some(true) {
        return Vec::new();
    }
    let (Some(copies), taken) = copy_state(cx, id) else {
        return Vec::new();
    };
    let list = |v: Vec<usize>| {
        v.iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("・")
    };
    let mut out = Vec::new();
    let missing: Vec<usize> = (2..=copies).filter(|i| !taken.contains(i)).collect();
    if !missing.is_empty() {
        out.push(format!(
            "写し {} はまだボディになっていません(見えません)。右クリックの「写しを別のボディに分ける」でボディにします",
            list(missing)
        ));
    }
    let extra: Vec<usize> = taken.into_iter().filter(|i| *i > copies).collect();
    if !extra.is_empty() {
        out.push(format!(
            "個数が減ったので、写し {} のボディの機能は止まっています。要らなければ消してください",
            list(extra)
        ));
    }
    out
}

/// 選んでいるミラー。
fn selected_mirror(cx: &Context<'_>) -> Option<(FeatureId, Mirror)> {
    let id = cx.selected?;
    let entry = cx.data.feature(id)?;
    if entry.type_id != MIRROR {
        return None;
    }
    Some((id, parse::<Mirror>(&entry.data).ok()?))
}

/// ミラーの対称の平面を、手順で選んだ値(`plane` が xy・yz・zx か face)にする。face なら
/// クリックした面(CLI・MCP は引数 `face` と `feature`)。面はミラーより前の形から探す
/// (ミラーの後のボディの面を選んでも、元のボディの同じ面を使う)。
fn mirror_plane(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, mut m) = selected_mirror(cx).ok_or("ミラーを選んでいません")?;
    m.plane_ref = PlaneRef::from_choice(cx, id, "対称の平面")?;
    Ok(Outcome::Change {
        label: "ミラーの対称の平面を選ぶ".into(),
        edits: vec![kmb_core::Edit::SetData {
            id,
            data: serde_json::to_value(&m).map_err(|e| e.to_string())?,
        }],
    })
}

/// 面でミラーする操作と、パターンの軸を選ぶ操作。
pub struct MirrorCommands;

impl Extension for MirrorCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let face = cx.picked_plane().is_some();
        let pattern = selected_pattern(cx).is_some();
        let linear = selected_pattern(cx).is_some_and(|(_, _, k)| k == "direction_ref");
        let mirror = selected_mirror(cx).is_some();
        let body_pattern = selected_body_pattern(cx).is_some();
        let separate = selected_body_pattern(cx)
            .is_some_and(|(_, d)| d.get("separate").and_then(|v| v.as_bool()) == Some(true));
        let on_face: &[Place] = if face { &[Place::FaceMenu] } else { &[] };
        let need_face = (!face).then_some(NEED_FACE);
        vec![
            Action::new(MIRROR_ON_FACE, "この面でミラー", "cad")
                .menu("パターン")
                .icon("mirror")
                .aliases("みらー mirror たいしょう めん")
                .hint("最後の押し出しなどの形を、この面の反対側に写す")
                .at(on_face)
                .disabled_if(need_face.or_else(|| {
                    cx.candidates("recipe")
                        .is_empty()
                        .then_some("写せる機能(押し出しなど)がない")
                })),
            Action::new(PATTERN_AXIS, "軸を選ぶ", "cad")
                .arg(
                    kmb_core::Arg::value(
                        "axis",
                        "軸(x・y・z か {\"sketch\": スケッチ, \"line\": 線の番号}、手順の値)",
                    )
                    .required(),
                )
                .menu("パターン")
                .icon("circpat")
                .aliases("ぱたーん じく 軸 むき 向き pattern axis 線 原点")
                .hint("選んでいるパターンの軸(直線のパターンは向き)を、原点の X・Y・Z 軸・スケッチの線・ボディの辺(円の辺はその円の軸)をクリックして決める")
                .at(if pattern { &[Place::FeatureMenu] } else { &[] })
                .disabled_if((!pattern).then_some("円形か直線のパターンを選んでから使う")),
            Action::new(PATTERN_AXIS2, "方向 2 を選ぶ", "cad")
                .arg(
                    kmb_core::Arg::value(
                        "axis",
                        "軸(x・y・z か {\"sketch\": スケッチ, \"line\": 線の番号}、手順の値)",
                    )
                    .required(),
                )
                .menu("パターン")
                .icon("linpat")
                .aliases("ぱたーん ほうこう 方向 2 こうし 格子 pattern direction")
                .hint("選んでいる直線のパターンの方向 2(格子のもう 1 つの向き)を、原点の X・Y・Z 軸・スケッチの線・ボディの辺(円の辺はその円の軸)をクリックして決める")
                .at(if linear { &[Place::FeatureMenu] } else { &[] })
                .disabled_if((!linear).then_some("直線のパターンを選んでから使う")),
            Action::new(SPLIT_COPIES, "写しを別のボディに分ける", "cad")
                .menu("パターン")
                .icon("linpat")
                .aliases("うつし 写し わける 分ける べつ 別 ぼでぃ ボディ ぶひん 部品 pattern copies separate bodies")
                .hint("選んでいるボディのパターン・ミラー(「写しを別のボディにする」を入れた物)の写しを、1 つずつ別のボディにする。個数を増やした後にもう一度使うと、増えた写しも分ける")
                .at(if body_pattern { &[Place::FeatureMenu] } else { &[] })
                .disabled_if(if !body_pattern {
                    Some("種類がボディのパターンかミラーを選んでから使う")
                } else if !separate {
                    Some("先に欄「写しを別のボディにする」を入れる")
                } else {
                    None
                }),
            Action::new(MIRROR_PLANE, "対称の平面を選ぶ", "cad")
                .args(kmb_cad::plane::args())
                .menu("パターン")
                .icon("mirror")
                .aliases("みらー たいしょう へいめん 平面 めん mirror plane")
                .hint("選んでいるミラーの対称の平面を、原点の XY・YZ・ZX 平面の四角か、ボディの平らな面をクリックして決める")
                .keep_edit()
                .at(if mirror {
                    &[Place::FeatureMenu, Place::Panel]
                } else {
                    &[]
                })
                .disabled_if((!mirror).then_some("ミラーを選んでから使う")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        Some(match id {
            MIRROR_ON_FACE => mirror(cx),
            PATTERN_AXIS => pattern_axis(cx, false),
            PATTERN_AXIS2 => pattern_axis(cx, true),
            MIRROR_PLANE => mirror_plane(cx),
            SPLIT_COPIES => split_copies(cx),
            _ => return None,
        })
    }

    fn notes(&self, cx: &Context<'_>, id: FeatureId) -> Vec<String> {
        copy_notes(cx, id)
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if id == MIRROR_PLANE {
            selected_mirror(cx)?;
            // スケッチの平面を選ぶ時と同じく、原点の平面の四角とボディの平らな面をクリックで選べる
            return Some(vec![Handle {
                choices: ["xy", "yz", "zx"]
                    .iter()
                    .enumerate()
                    .map(|(i, v)| kmb_core::HandleChoice::origin_plane(i, json!(v)))
                    .collect(),
                faces: true,
                ..Handle::new(
                    "plane",
                    "対称の平面にする原点の平面(XY・YZ・ZX の四角)か、ボディの平らな面をクリック",
                    Gesture::Choice,
                )
            }]);
        }
        if id != PATTERN_AXIS && id != PATTERN_AXIS2 {
            return None;
        }
        let (pid, _, _) = selected_pattern(cx)?;
        Some(vec![Handle {
            choices: kmb_cad::axis::axis_choices(cx, pid, None, None),
            show_origin: true,
            ..Handle::new(
                "axis",
                "軸にする原点の軸・スケッチの線・ボディの辺(円の辺はその円の軸)をクリック",
                Gesture::Choice,
            )
        }])
    }
}

/// この crate の名乗り(登録する種類と操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-pattern", register).with_extension(MirrorCommands)
}
