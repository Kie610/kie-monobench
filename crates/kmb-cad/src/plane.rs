//! 平面の選び方の共通部: 原点の XY・YZ・ZX 平面と、ボディの平らな面と、作業平面(ミラー、ボディの
//! 分割、勾配が使う)。平面を選ぶ間は原点の平面の四角・作業平面の四角・ボディの平らな面をクリックで
//! 選べる。
use crate::{body_of, kernel_err, output};
use kmb_core::kernel::P3;
use kmb_core::{Context, EvalCtx, EvalError, FeatureId, HandleChoice, InputRef, Value};
use serde::{Deserialize, Serialize};
use serde_json::json;

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
    /// 作業平面の機能 `feature`(kmb-cad-datum)。作業平面を作れなかった時は、別の平面に替えず誤りにする
    Datum {
        feature: FeatureId,
    },
}

/// 平面の欄の選択肢(`Vector` の名前は機能ごと)。`datum` は平面を選ぶ手順で作業平面をクリックした時
/// だけ入るので、欄では隠す(`Field::hide_choice`)。
pub fn plane_ref_choices(vector: &'static str) -> [(&'static str, &'static str); 6] {
    [
        ("vector", vector),
        ("origin_xy", "原点の XY 平面"),
        ("origin_yz", "原点の YZ 平面"),
        ("origin_zx", "原点の ZX 平面"),
        ("face", "平らな面(クリックで選ぶ)"),
        ("datum", "作業平面"),
    ]
}

/// 平面を選ぶ手順の候補: 原点の XY・YZ・ZX 平面の四角(値 xy・yz・zx)と、機能 `owner` より前の
/// 作業平面の四角(値は機能の番号)。ボディの平らな面は手順の `faces` で選ぶ。
pub fn plane_choices(cx: &Context<'_>, owner: FeatureId) -> Vec<HandleChoice> {
    ["xy", "yz", "zx"]
        .iter()
        .enumerate()
        .map(|(i, v)| HandleChoice::origin_plane(i, json!(v)))
        .chain(kmb_interchange::datum_choices(cx, Some(owner)))
        .collect()
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
            PlaneRef::Datum { feature } => {
                vec![output(*feature, kmb_interchange::DATUM_OUTPUT)]
            }
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
            PlaneRef::Datum { feature } => {
                let p = kmb_interchange::datum_plane(ctx, *feature)?;
                (p.origin, p.normal())
            }
        }))
    }

    /// 手順で選んだ値(`plane` が xy・yz・zx か face か作業平面の機能の番号)から平面を作る。face なら
    /// クリックした面(CLI・MCP は引数 `face` と `feature`)。面は機能 `owner` より前の形から探す
    /// (`owner` の後のボディの面を選んでも、元のボディの同じ面を使う)。作業平面も `owner` より前の物
    /// だけ(後ろの物を指すと計算の順が回る)。
    pub fn from_choice(cx: &Context<'_>, owner: FeatureId, what: &str) -> Result<Self, String> {
        const NEED: &str =
            "plane に xy・yz・zx か face(クリックした平らな面)か作業平面の機能の番号を指定します";
        let arg = cx.arg("plane").ok_or(NEED)?;
        if let Some(n) = arg.as_u64() {
            let feature = FeatureId(n);
            if kmb_interchange::datum_choices(cx, Some(owner))
                .iter()
                .all(|c| c.value != json!(n))
            {
                return Err(format!(
                    "機能 {n} はこの機能より前の作業平面ではありません(作業平面を作れていない時は、先にその誤りを直してください)"
                ));
            }
            return Ok(PlaneRef::Datum { feature });
        }
        let v = arg.as_str().ok_or(NEED)?;
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
            other => {
                return Err(format!(
                    "平面 {other} はありません(xy・yz・zx か face か作業平面の機能の番号)"
                ));
            }
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
pub fn args() -> Vec<kmb_core::Arg> {
    use kmb_core::Arg;
    vec![
        Arg::value(
            "plane",
            "平面(xy・yz・zx・face か作業平面の機能の番号。face ならクリックした平らな面か引数 face)",
        ),
        Arg::text("face", "平面にする平らな面の名前(省くと選んでいる面)"),
        Arg::feature("feature", "面を持つ機能の番号"),
    ]
}
