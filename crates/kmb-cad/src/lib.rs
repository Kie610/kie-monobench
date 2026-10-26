//! ソリッドの機能の共通部品: 演算(新規・結合・切り取り・交差)、道具の作り方([`Recipe`])と
//! その適用、同じ平面で接する面の扱い、式の扱い、記述子の部品、平らな面の縁の輪郭
//! ([`face_profile`])、輪郭を伸ばした柱の三角形([`prism`]。押し出す道具の下絵)。
//! 機能はそれぞれ別の crate(kmb-cad-extrude など)にあり、ここにだけ依存する。

pub mod axis;
mod chain;
mod face_profile;
mod flush;
mod inspect;
pub mod plane;
pub mod prism;
pub mod regions;
pub mod stop_at;
pub mod thin;
mod timeline;
pub use axis::AxisRef;
pub use chain::{TANGENT_DEG, smooth_faces, tangent_chain, tangent_chains};
pub use face_profile::{FaceProfile, face_profile};
pub use flush::{EXTENSIONS, extend_flush};
pub use inspect::{InspectCommands, MEASURE};

use kmb_core::{Descriptor, Field, category};

/// 道具の作り方の型(出力の `recipe` スロット)。パターンとミラーが使う。
pub const RECIPE_TYPE: &str = "solid.recipe@1";
/// 面を選ばずに面の操作を使った時の理由。
pub const NEED_FACE: &str = "3D のビューで平らな面を選んでから使う";

pub const OPERATIONS: &[(&str, &str)] = &[
    ("new", "新規ボディ"),
    ("join", "結合"),
    ("cut", "切り取り"),
    ("intersect", "交差"),
];

pub fn solid(type_id: &str, label: &str) -> Descriptor {
    Descriptor::new(type_id, category::SOLID, label)
}

/// 新しいボディを作るか、どのボディに効かせるか。
pub fn operation_fields() -> [Field; 2] {
    [
        Field::choice("operation", "演算", OPERATIONS),
        Field::opt_output("target", "対象", "body").unless("operation", "new"),
    ]
}

use kmb_core::kernel::{self as k, P3, Profile, Segment};
use kmb_core::{
    BooleanOp, Dim, EvalCtx, EvalError, FeatureId, InputRef, Outputs, ShapeHandle, Value,
    expr_inputs,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub fn parse<T: DeserializeOwned>(d: &serde_json::Value) -> Result<T, EvalError> {
    serde_json::from_value(d.clone()).map_err(|e| EvalError::InvalidData(e.to_string()))
}

/// 新しいボディを作るか、対象のボディにどう効かせるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    #[default]
    New,
    Join,
    Cut,
    Intersect,
}

impl Operation {
    pub fn boolean(self) -> Option<BooleanOp> {
        match self {
            Operation::New => None,
            Operation::Join => Some(BooleanOp::Union),
            Operation::Cut => Some(BooleanOp::Subtract),
            Operation::Intersect => Some(BooleanOp::Intersect),
        }
    }
}

/// 押し出す向き。`Normal` はスケッチの平面の法線の向き。`TwoSides` は表と裏へ別の距離で
/// (裏側の距離は押し出しの `distance2`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Normal,
    Reverse,
    Symmetric,
    TwoSides,
}

/// 押し出す範囲。`All` は対象のボディを向きの先まで全部貫く(距離を打たない)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Extent {
    #[default]
    Distance,
    All,
    /// 選んだ平らな面まで。面はスケッチの平面と平行な物だけ
    ToFace,
}

impl Extent {
    pub fn is_distance(&self) -> bool {
        *self == Extent::Distance
    }
}

/// 式を持つ項目。パラメータ名の変更と依存の宣言に使う。
pub trait Exprs: Serialize + Clone {
    fn exprs_mut(&mut self) -> Vec<&mut String>;
    fn exprs(&self) -> Vec<String> {
        self.clone()
            .exprs_mut()
            .into_iter()
            .map(|s| s.clone())
            .collect()
    }
}

pub fn expr_refs<T: Exprs>(t: &T) -> Result<Vec<InputRef>, EvalError> {
    let mut out = Vec::new();
    for e in t.exprs() {
        for i in expr_inputs(&e)? {
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    Ok(out)
}

pub fn rename<T: Exprs>(
    t: &T,
    old: &str,
    new: &str,
) -> Result<Option<serde_json::Value>, EvalError> {
    let mut c = t.clone();
    let mut changed = false;
    for e in c.exprs_mut() {
        let r = kmb_core::rename_reference(e, old, new)?;
        if r != *e {
            *e = r;
            changed = true;
        }
    }
    Ok(changed.then(|| serde_json::to_value(&c).expect("機能のデータは JSON にできる")))
}

pub fn output(feature: FeatureId, slot: &str) -> InputRef {
    InputRef::Output {
        feature,
        slot: slot.into(),
    }
}

/// 長さの欄の式(mm)。単位の無い値は mm として読む(数だけの式は欄の単位。
/// `50 / 2` は 25 mm)。
pub fn length(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if !q.dim.is_none() && q.dim != Dim::LENGTH {
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は長さの単位が必要です(値: {q})"
        )));
    }
    sized(q.value, false, src, what)
}

/// 桁違いの長さ・角度は、カーネルへ渡す前にこの機能の誤りにする(ほかの機能は計算を続ける。10-24 k12)。
fn sized(v: f64, angle: bool, src: &str, what: &str) -> Result<f64, EvalError> {
    let dim = if angle { Dim::ANGLE } else { Dim::LENGTH };
    match kmb_core::Quantity::new(v, dim).size_problem() {
        Some(why) => Err(EvalError::InvalidData(format!("{what} `{src}` が{why}"))),
        None => Ok(v),
    }
}

/// 角度の欄の式(rad)。単位の無い値は deg として読む(`90 / 2` は 45 度。ラジアンで書くなら
/// `PI/4 rad`)。
pub fn angle(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    let v = if q.dim.is_none() {
        q.value.to_radians()
    } else if q.dim != Dim::ANGLE {
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は角度の単位が必要です(値: {q})"
        )));
    } else {
        q.value
    };
    sized(v, true, src, what)
}

pub fn count(ctx: &EvalCtx<'_>, src: &str) -> Result<usize, EvalError> {
    count_of(ctx.expr(src)?, src)
}

/// 個数の式の値を確かめる。誤りの文は直し方の例まで言う(10-26 k5)。
fn count_of(q: kmb_core::Quantity, src: &str) -> Result<usize, EvalError> {
    let n = q.value.round();
    let why = if q.dim == Dim::LENGTH {
        format!(
            "が長さになっています(値: {q})。長さで割って個数にしてください(例: `floor(({src}) / 10 mm)` は 10 mm ごとに 1 つ)"
        )
    } else if q.dim != Dim::NONE {
        format!("に単位が残っています(値: {q})。同じ単位の量で割って、単位の無い数にしてください")
    } else if (q.value - n).abs() > 1e-9 {
        format!(
            "が整数ではありません(値: {q})。丸めるなら `round({src})`、切り捨てるなら `floor({src})`"
        )
    } else if !(1.0..=1000.0).contains(&n) {
        format!("は 1 から 1000 までにしてください(値: {q})")
    } else {
        return Ok(n as usize);
    };
    Err(EvalError::InvalidData(format!("個数 `{src}` {why}")))
}

pub fn kernel_err(e: kmb_core::KernelError) -> EvalError {
    EvalError::Kernel(e.to_string())
}

pub fn body_of(ctx: &EvalCtx<'_>, id: FeatureId) -> Result<ShapeHandle, EvalError> {
    match ctx.output(id, "body")? {
        Value::Shape(h) => Ok(*h),
        _ => Err(EvalError::InvalidData(format!(
            "{id} はボディではありません"
        ))),
    }
}

pub fn profile_from(ctx: &EvalCtx<'_>, id: FeatureId) -> Result<Profile, EvalError> {
    kmb_core::Profile::from_value(ctx.output(id, "profile")?)
        .ok_or_else(|| EvalError::InvalidData(format!("{id} はスケッチではありません")))
}

pub fn tag(ctx: &EvalCtx<'_>) -> String {
    format!("f{}", ctx.feature_id().0)
}

pub fn target_inputs(op: Operation, target: Option<FeatureId>) -> Result<Vec<InputRef>, EvalError> {
    match (op, target) {
        (Operation::New, _) => Ok(vec![]),
        (_, Some(t)) => Ok(vec![output(t, "body")]),
        (_, None) => Err(EvalError::InvalidData(
            "結合・切り取り・交差には対象のボディ(target)が必要です".into(),
        )),
    }
}

/// 抑制した時に対象のボディをそのまま渡す(新しいボディを作る時は何も渡さない)。
pub fn keep_target(op: Operation, target: Option<FeatureId>) -> Vec<(String, InputRef)> {
    match (op, target) {
        (Operation::New, _) | (_, None) => vec![],
        (_, Some(t)) => vec![("body".into(), output(t, "body"))],
    }
}

impl kmb_core::Payload for Recipe {
    const TYPE: &'static str = RECIPE_TYPE;
}

fn one() -> f64 {
    1.0
}

fn is_one(v: &f64) -> bool {
    *v == 1.0
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// 道具の作り方。パターンはこれを動かして作り直す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recipe {
    Extrude {
        operation: Operation,
        profile: Profile,
        from: f64,
        to: f64,
        /// 貫通穴なら、`from` の端も材料の外へ出ているので延ばしてよい
        through: bool,
        /// 道具の終わりを、輪郭と平行でない平面で切る(押し出しの範囲「対象まで」で斜めの面に止める時)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop: Option<StopPlane>,
        /// 壁の傾き(ラジアン。正は輪郭の平面から離れるほど細くなる)。0 ならまっすぐ
        #[serde(default, skip_serializing_if = "is_zero")]
        taper: f64,
    },
    Revolve {
        operation: Operation,
        profile: Profile,
        origin: P3,
        dir: P3,
        angle: f64,
    },
    /// 輪郭を経路に沿って動かす(スイープ)
    Sweep {
        operation: Operation,
        profile: Profile,
        path: k::PlanarPath,
        /// 終わりまでに輪郭を経路のまわりに回す角度(ラジアン)。0 なら回さない
        #[serde(default, skip_serializing_if = "is_zero")]
        twist: f64,
        /// 終わりの輪郭の大きさの倍率。1 なら変えない
        #[serde(default = "one", skip_serializing_if = "is_one")]
        scale: f64,
    },
    /// 断面をつなぐ(ロフト)。`rails` はガイドレール
    Loft {
        operation: Operation,
        sections: Vec<Profile>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        rails: Vec<k::PlanarPath>,
    },
    /// 複数の道具を順に効かせる(ザグリのある穴を複数の点にあける時など)。演算はどれも同じ
    Many { parts: Vec<Recipe> },
}

/// 押し出しの道具を切る平面。道具のうち、点 `point` を通る平面の、法線 `normal` の側を捨てる。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StopPlane {
    pub point: P3,
    pub normal: P3,
}

impl StopPlane {
    /// 動かした平面。
    pub fn moved(&self, m: &k::Matrix4) -> StopPlane {
        let o = k::transform_point(m, [0.0; 3]);
        StopPlane {
            point: k::transform_point(m, self.point),
            normal: k::sub(k::transform_point(m, self.normal), o),
        }
    }
}

/// 道具 `tool`(輪郭 `profile` を `from`〜`to` に押し出した柱)の、平面 `stop` の向こう側を切り捨てる。
/// 平面の上に大きな正方形を置いて手前へ厚く押し出した板と、道具の交わりを取る。
fn cut_by_stop(
    kernel: &dyn kmb_core::Kernel,
    tool: ShapeHandle,
    stop: &StopPlane,
    profile: &Profile,
    from: f64,
    to: f64,
    tag: &str,
) -> Result<ShapeHandle, EvalError> {
    let n = profile.plane.normal();
    // 板の大きさ: 道具の点のどれよりも、平面の点から十分遠くまで
    let mut r: f64 = 1.0;
    for region in &profile.regions {
        for l in std::iter::once(&region.outer).chain(&region.holes) {
            for q in l.polyline() {
                let p = profile.plane.to_world(q);
                for t in [from, to] {
                    let w = [0, 1, 2].map(|i| p[i] + n[i] * t);
                    let d = k::sub(w, stop.point);
                    r = r.max(k::dot(d, d).sqrt());
                }
            }
        }
    }
    let r = r * 2.0 + 1.0;
    let plane = k::Plane::from_point_normal(stop.point, stop.normal);
    let corners = [[-r, -r], [r, -r], [r, r], [-r, r]];
    let edges = (0..4)
        .map(|i| k::ProfileEdge {
            name: format!("s{i}"),
            segment: Segment::Line {
                a: corners[i],
                b: corners[(i + 1) % 4],
            },
            reversed: false,
        })
        .collect();
    let slab = Profile {
        plane,
        regions: vec![k::Region {
            outer: k::ProfileLoop { edges },
            holes: vec![],
        }],
    };
    let slab = kernel
        .extrude(&slab, -2.0 * r, 0.0, tag)
        .map_err(kernel_err)?;
    kernel
        .boolean(BooleanOp::Intersect, tool, slab)
        .map_err(kernel_err)
}

pub fn transform_plane(m: &k::Matrix4, p: &k::Plane) -> k::Plane {
    let o = k::transform_point(m, [0.0; 3]);
    let v = |a: P3| k::sub(k::transform_point(m, a), o);
    k::Plane {
        origin: k::transform_point(m, p.origin),
        x_axis: v(p.x_axis),
        y_axis: v(p.y_axis),
    }
}

impl Recipe {
    pub fn operation(&self) -> Operation {
        match self {
            Recipe::Extrude { operation, .. }
            | Recipe::Revolve { operation, .. }
            | Recipe::Sweep { operation, .. }
            | Recipe::Loft { operation, .. } => *operation,
            Recipe::Many { parts } => parts.first().map_or(Operation::Cut, Recipe::operation),
        }
    }

    /// 平面(点 `origin`、法線 `normal`)について写した道具の作り方。演算は `op` に置き換える。
    /// 写すと座標の向きが裏返るので、輪郭の平面の y 軸を反転し、輪郭の y 座標も反転して
    /// 押し出しの向きを保つ。回転の角度は向きが逆になる。
    pub fn mirrored(&self, origin: P3, normal: P3, op: Operation) -> Recipe {
        if let Recipe::Many { parts } = self {
            return Recipe::Many {
                parts: parts
                    .iter()
                    .map(|p| p.mirrored(origin, normal, op))
                    .collect(),
            };
        }
        let n = k::normalize(normal);
        let d = 2.0 * k::dot(origin, n);
        let mut m = k::IDENTITY;
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] -= 2.0 * n[i] * n[j];
            }
            m[i][3] = d * n[i];
        }
        let mut r = self.moved(&m, op);
        match &mut r {
            Recipe::Extrude { profile, .. } => mirror_profile(profile),
            Recipe::Revolve { profile, angle, .. } => {
                *angle = -*angle;
                mirror_profile(profile);
            }
            Recipe::Sweep {
                profile,
                path,
                twist,
                ..
            } => {
                // 鏡に写すと、ねじれの向きが逆になる
                *twist = -*twist;
                mirror_profile(profile);
                path.plane.y_axis = k::scale(path.plane.y_axis, -1.0);
                for e in &mut path.edges {
                    mirror_edge(e);
                }
            }
            Recipe::Loft {
                sections, rails, ..
            } => {
                sections.iter_mut().for_each(mirror_profile);
                for p in rails {
                    p.plane.y_axis = k::scale(p.plane.y_axis, -1.0);
                    for e in &mut p.edges {
                        mirror_edge(e);
                    }
                }
            }
            Recipe::Many { .. } => unreachable!("先に部品ごとに写した"),
        }
        r
    }

    /// 動かした道具の作り方。演算は `op` に置き換える。
    pub fn moved(&self, m: &k::Matrix4, op: Operation) -> Recipe {
        let mut r = self.clone();
        match &mut r {
            Recipe::Extrude {
                operation,
                profile,
                stop,
                ..
            } => {
                *operation = op;
                profile.plane = transform_plane(m, &profile.plane);
                if let Some(s) = stop {
                    *s = s.moved(m);
                }
            }
            Recipe::Revolve {
                operation,
                profile,
                origin,
                dir,
                ..
            } => {
                *operation = op;
                profile.plane = transform_plane(m, &profile.plane);
                let o = k::transform_point(m, [0.0; 3]);
                *origin = k::transform_point(m, *origin);
                *dir = k::sub(k::transform_point(m, *dir), o);
            }
            Recipe::Sweep {
                operation,
                profile,
                path,
                ..
            } => {
                *operation = op;
                profile.plane = transform_plane(m, &profile.plane);
                path.plane = transform_plane(m, &path.plane);
            }
            Recipe::Loft {
                operation,
                sections,
                rails,
            } => {
                *operation = op;
                for p in sections {
                    p.plane = transform_plane(m, &p.plane);
                }
                for p in rails {
                    p.plane = transform_plane(m, &p.plane);
                }
            }
            Recipe::Many { parts } => {
                for p in parts {
                    *p = p.moved(m, op);
                }
            }
        }
        r
    }
}

/// 平面の y 軸を裏返した時の、輪郭の区間(y 座標を反転し、円弧は反時計回りを保つ)。
fn mirror_edge(e: &mut k::ProfileEdge) {
    e.segment = match e.segment {
        Segment::Line { a, b } => Segment::Line {
            a: [a[0], -a[1]],
            b: [b[0], -b[1]],
        },
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => {
            // 反時計回りの向きを保つため、始まりと終わりを入れ替えてたどる向きを逆にする
            e.reversed = !e.reversed;
            Segment::Arc {
                center: [center[0], -center[1]],
                radius,
                start: -end,
                end: -start,
            }
        }
        Segment::Circle { center, radius } => Segment::Circle {
            center: [center[0], -center[1]],
            radius,
        },
    };
}

/// 写すと座標の向きが裏返るので、輪郭の平面の y 軸を反転し、輪郭の y 座標も反転する
/// (外周は反時計回り、穴は時計回りのまま)。
fn mirror_profile(profile: &mut Profile) {
    profile.plane.y_axis = k::scale(profile.plane.y_axis, -1.0);
    for region in &mut profile.regions {
        for l in std::iter::once(&mut region.outer).chain(region.holes.iter_mut()) {
            let before = l.signed_area();
            for e in &mut l.edges {
                mirror_edge(e);
            }
            if l.signed_area() * before < 0.0 {
                l.reverse();
            }
        }
    }
}

/// 道具を作って対象に効かせる。押し出しの道具の面が対象の面と同じ平面に重なる時は
/// はみ出させ、ブーリアンが失敗したら、はみ出す量を変えて作り直す。
pub fn apply_recipe(
    ctx: &EvalCtx<'_>,
    recipe: &Recipe,
    target: Option<ShapeHandle>,
    tag: &str,
) -> Result<ShapeHandle, EvalError> {
    let kernel = ctx.kernel();
    let op = recipe.operation();
    let boolean = |tool: ShapeHandle| -> Result<ShapeHandle, EvalError> {
        match (op.boolean(), target) {
            (None, _) => Ok(tool),
            // 空や壊れた結果は失敗として扱う(次の延ばし方も試せる)
            (Some(b), Some(t)) => checked_boolean(kernel, b, t, tool).map_err(kernel_err),
            (Some(_), None) => Err(EvalError::InvalidData(
                "結合・切り取り・交差には対象のボディ(target)が必要です".into(),
            )),
        }
    };
    match recipe {
        Recipe::Many { parts } => {
            let mut body = target.filter(|_| op != Operation::New);
            for p in parts {
                body = Some(match body {
                    // 新しいボディを作る時は、2 つ目からを足し合わせる
                    Some(b) if op == Operation::New => kernel
                        .boolean(BooleanOp::Union, b, apply_recipe(ctx, p, None, tag)?)
                        .map_err(kernel_err)?,
                    _ => apply_recipe(ctx, p, body, tag)?,
                });
            }
            body.ok_or_else(|| EvalError::InvalidData("道具がありません".into()))
        }
        Recipe::Revolve {
            profile,
            origin,
            dir,
            angle,
            ..
        } => boolean(
            kernel
                .revolve(profile, *origin, *dir, *angle, tag)
                .map_err(kernel_err)?,
        ),
        Recipe::Sweep {
            profile,
            path,
            twist,
            scale,
            ..
        } => boolean(
            kernel
                .sweep(profile, path, *twist, *scale, tag)
                .map_err(kernel_err)?,
        ),
        Recipe::Loft {
            sections, rails, ..
        } => boolean(kernel.loft(sections, rails, tag).map_err(kernel_err)?),
        Recipe::Extrude {
            profile,
            from,
            to,
            through,
            stop,
            taper,
            ..
        } => {
            let extrude = |p: &Profile, f: f64, t: f64| -> Result<ShapeHandle, EvalError> {
                let tool = if *taper == 0.0 {
                    kernel.extrude(p, f, t, tag)
                } else {
                    kernel.extrude_taper(p, f, t, *taper, tag)
                }
                .map_err(kernel_err)?;
                match stop {
                    Some(s) => cut_by_stop(kernel, tool, s, p, f, t, tag),
                    None => Ok(tool),
                }
            };
            // 傾いた壁は、断面を動かすと形が変わるので、重なる面をはみ出させずにそのまま効かせる
            if *taper != 0.0 {
                return boolean(extrude(profile, *from, *to)?);
            }
            let Some(t) = target.filter(|_| op != Operation::New) else {
                return boolean(extrude(profile, *from, *to)?);
            };
            let faces = kernel.faces(t).map_err(kernel_err)?;
            let mut last = None;
            let mut tried: Vec<(Profile, f64, f64)> = Vec::new();
            for ext in EXTENSIONS {
                let from = if *through { from - ext * 10.0 } else { *from };
                let adjusted = extend_flush(profile, from, *to, op, &faces, ext);
                if tried.contains(&adjusted) {
                    break;
                }
                tried.push(adjusted.clone());
                let (p, f, t) = adjusted;
                let tool = extrude(&p, f, t)?;
                match boolean(tool) {
                    Ok(body) => return Ok(body),
                    Err(e @ EvalError::Kernel(_)) => last = Some(e),
                    Err(e) => return Err(e),
                }
            }
            Err(last.unwrap_or_else(|| EvalError::Kernel("ブーリアンに失敗しました".into())))
        }
    }
}

/// 形が空か(外形の箱が裏返っている)。
/// ブーリアンをして、結果が壊れていないか確かめる。ブーリアンを使う機能はどれもこれを通す。
///
/// カーネルは黙って空の形や壊れた形を返すことがある(ほぼ重なる円筒を切る、継ぎ目の辺の順で
/// 体積 0 になるなど)。そのまま通すと形が黙って消えるか、後の機能が壊れた形の上に積まれるので、
/// 誤りにして止める(呼ぶ側は大きさを変えて作り直してよい)。
/// - 対象があるのに結果が空: 交差なら当たっていない、ほかは計算の失敗
/// - 体積が向きに合わない(削るのに増える・足すのに減る・足した形が道具より小さい・交わりが道具より大きい)
pub fn checked_boolean(
    kernel: &dyn kmb_core::Kernel,
    op: BooleanOp,
    cur: ShapeHandle,
    tool: ShapeHandle,
) -> Result<ShapeHandle, kmb_core::KernelError> {
    let out = kernel.boolean(op, cur, tool)?;
    if is_empty(kernel, out) && !is_empty(kernel, cur) {
        return Err(kmb_core::KernelError::Failed(match op {
            BooleanOp::Intersect => {
                "交差すると何も残りません(道具のボディが対象に当たっていません)".into()
            }
            _ => "形の計算に失敗しました(結果が空になりました。重なりかけた面がないか見てください)"
                .into(),
        }));
    }
    let (Ok(v0), Ok(v1)) = (kernel.volume(cur), kernel.volume(out)) else {
        return Ok(out);
    };
    // 道具の体積とも比べる(結合で対象が消え、道具だけが残った形も見つける)
    let vt = kernel.volume(tool).ok();
    let eps = 1e-6 * v0.abs().max(vt.unwrap_or(0.0).abs()).max(1.0);
    let sane = v1 > eps
        && match op {
            BooleanOp::Subtract => v1 <= v0 + eps,
            BooleanOp::Intersect => v1 <= v0.min(vt.unwrap_or(v0)) + eps,
            BooleanOp::Union => v1 >= v0.max(vt.unwrap_or(v0)) - eps,
        };
    if sane {
        Ok(out)
    } else {
        Err(kmb_core::KernelError::Failed(format!(
            "結果の形が壊れています(体積 {v0:.3} → {v1:.3})"
        )))
    }
}

fn is_empty(kernel: &dyn kmb_core::Kernel, shape: ShapeHandle) -> bool {
    kernel
        .bounds(shape)
        .is_ok_and(|(min, max)| (0..3).any(|i| min[i] > max[i]))
}

/// 作り方を対象に効かせ、`body` と `recipe` を出す。
pub fn finish(
    ctx: &EvalCtx<'_>,
    recipe: Recipe,
    target: Option<FeatureId>,
) -> Result<Outputs, EvalError> {
    let target = match target {
        Some(t) if recipe.operation() != Operation::New => Some(body_of(ctx, t)?),
        _ => None,
    };
    let body = apply_recipe(ctx, &recipe, target, &tag(ctx))?;
    Ok([
        ("body".to_string(), Value::Shape(body)),
        ("recipe".to_string(), kmb_core::Payload::to_value(&recipe)),
    ]
    .into())
}

/// 角度の既定(1 周)。
pub fn full_turn() -> String {
    "360 deg".into()
}

/// この crate の名乗り(操作と、ボディに付けた物理材料の区画 `cad.materials`)。組み立て(kmb-bundle)が
/// 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad", |reg| {
        reg.declare_section::<kmb_look::material::BodyMaterials>();
    })
    .with_extension(InspectCommands)
    .with_extension(timeline::TimelineCommands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::Quantity;

    fn msg(q: Quantity, src: &str) -> String {
        count_of(q, src).unwrap_err().to_string()
    }

    #[test]
    fn a_count_that_is_a_length_says_to_divide_by_a_length() {
        // 個数の式が長さになった時は、長さで割る例まで言う(10-26 k5)
        let m = msg(Quantity::mm(420.0), "W");
        assert!(m.contains("長さ"), "{m}");
        assert!(m.contains("floor((W) / 10 mm)"), "{m}");
    }

    #[test]
    fn a_count_that_is_not_whole_says_how_to_round() {
        let m = msg(Quantity::scalar(2.5), "n / 2");
        assert!(m.contains("round(n / 2)"), "{m}");
        assert_eq!(count_of(Quantity::scalar(4.0), "4").unwrap(), 4);
        assert!(msg(Quantity::scalar(0.0), "0").contains("1 から 1000"));
    }
}
