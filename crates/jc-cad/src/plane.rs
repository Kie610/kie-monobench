//! 平面の選び方の共通部: 原点の XY・YZ・ZX 平面と、ボディの平らな面(ミラー、ボディの分割が使う)。
//! 平面を選ぶ間は原点の平面の四角とボディの平らな面をクリックで選べる。
use crate::{body_of, kernel_err, output};
use jc_core::kernel::P3;
use jc_core::{Context, EvalCtx, EvalError, FeatureId, InputRef, Value};
use serde::{Deserialize, Serialize};

/// 平面。`Vector` は機能の数の欄(通る点と法線)で決める。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaneRef {
    #[default]
    Vector,
    OriginXy,
    OriginYz,
    OriginZx,
    /// 機能 `feature` の出力 `body` の平らな面 `face`。欄で「平らな面」を選んだだけの時は、面がまだ無い
    /// (面のクリックで入る)
    Face {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feature: Option<FeatureId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<String>,
    },
}

/// 平面の欄の選択肢(`Vector` の名前は機能ごと)。
pub fn plane_ref_choices(vector: &'static str) -> [(&'static str, &'static str); 5] {
    [
        ("vector", vector),
        ("origin_xy", "原点の XY 平面"),
        ("origin_yz", "原点の YZ 平面"),
        ("origin_zx", "原点の ZX 平面"),
        ("face", "平らな面(クリックで選ぶ)"),
    ]
}

impl PlaneRef {
    pub fn is_vector(&self) -> bool {
        *self == PlaneRef::Vector
    }

    pub fn inputs(&self) -> Vec<InputRef> {
        match self {
            PlaneRef::Face {
                feature: Some(f), ..
            } => vec![output(*f, "body")],
            _ => Vec::new(),
        }
    }

    /// 通る点と法線。`Vector` は `None`(機能の欄で決める)。`what` は誤りの文に出す平面の名前
    /// (「対称の平面」など)。
    pub fn resolve(&self, ctx: &EvalCtx<'_>, what: &str) -> Result<Option<(P3, P3)>, EvalError> {
        Ok(Some(match self {
            PlaneRef::Vector => return Ok(None),
            PlaneRef::OriginXy => ([0.0; 3], [0.0, 0.0, 1.0]),
            PlaneRef::OriginYz => ([0.0; 3], [1.0, 0.0, 0.0]),
            PlaneRef::OriginZx => ([0.0; 3], [0.0, 1.0, 0.0]),
            PlaneRef::Face { feature, face } => {
                let (Some(f), Some(name)) = (feature, face) else {
                    return Err(EvalError::InvalidData(format!(
                        "{what}をまだ選んでいません。「{what}を選ぶ」で、原点の平面か平らな面をクリックしてください"
                    )));
                };
                let p = ctx
                    .kernel()
                    .faces(body_of(ctx, *f)?)
                    .map_err(kernel_err)?
                    .into_iter()
                    .find(|x| &x.name == name)
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!(
                            "{what}にした面 {name} が見つかりません。選び直してください"
                        ))
                    })?
                    .planar
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!("面 {name} は平らではありません"))
                    })?;
                (p.point, p.normal)
            }
        }))
    }

    /// 手順で選んだ値(`plane` が xy・yz・zx か face)から平面を作る。face ならクリックした面
    /// (CLI・MCP は引数 `face` と `feature`)。面は機能 `owner` より前の形から探す(`owner` の後の
    /// ボディの面を選んでも、元のボディの同じ面を使う)。
    pub fn from_choice(cx: &Context<'_>, owner: FeatureId, what: &str) -> Result<Self, String> {
        let v = cx
            .arg_str("plane")
            .ok_or("plane に xy・yz・zx か face(クリックした平らな面)を指定します")?;
        Ok(match v {
            "xy" => PlaneRef::OriginXy,
            "yz" => PlaneRef::OriginYz,
            "zx" => PlaneRef::OriginZx,
            "face" => {
                let face = cx
                    .arg_str("face")
                    .map(String::from)
                    .or_else(|| cx.picked.map(|p| p.face.clone()))
                    .ok_or(format!("{what}にする平らな面をクリックしてください"))?;
                let given = cx.arg("feature").and_then(|v| v.as_u64()).map(FeatureId);
                let at = |f: FeatureId| cx.data.index_of(f).unwrap_or(usize::MAX);
                let before: Vec<FeatureId> = cx.data.features
                    [..at(owner).min(cx.data.features.len())]
                    .iter()
                    .rev()
                    .map(|f| f.id)
                    .collect();
                let feature = [given, cx.picked.map(|p| p.feature)]
                    .into_iter()
                    .flatten()
                    .chain(before)
                    .find(|f| at(*f) < at(owner) && has_planar_face(cx, *f, &face))
                    .ok_or(format!(
                        "この機能より前からある平らな面を{what}に選んでください"
                    ))?;
                PlaneRef::Face {
                    feature: Some(feature),
                    face: Some(face),
                }
            }
            other => return Err(format!("平面 {other} はありません(xy・yz・zx か face)")),
        })
    }
}

/// 機能 `f` の出力のボディに、平らな面 `face` があるか。
pub fn has_planar_face(cx: &Context<'_>, f: FeatureId, face: &str) -> bool {
    let Some(Value::Shape(h)) = cx.output(f, "body") else {
        return false;
    };
    cx.kernel
        .faces(*h)
        .is_ok_and(|fs| fs.iter().any(|x| x.name == face && x.planar.is_some()))
}

/// [`PlaneRef::from_choice`] が読む引数(作る平面)。
pub fn args() -> Vec<jc_core::Arg> {
    use jc_core::Arg;
    vec![
        Arg::choice(
            "plane",
            "平面(face ならクリックした平らな面か引数 face)",
            &[("xy", "XY"), ("yz", "YZ"), ("zx", "ZX"), ("face", "面")],
        ),
        Arg::text("face", "平面にする平らな面の名前(省くと選んでいる面)"),
        Arg::feature("feature", "面を持つ機能の番号"),
    ]
}
