//! 「対象まで」の相手: 平らな面か、ボディの頂点(押し出し・穴・回転で共通)。
//! 頂点は、その点を通る平面(押し出しと穴は輪郭と平行、回転は軸を含む)まで、と読む。

use jc_core::kernel as k;
use jc_core::{Context, FeatureId, ShapeHandle};

/// 手順・CLI で選んだ「対象まで」の相手。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopPick {
    /// 面の名前
    Face(String),
    /// 頂点の名前
    Vertex(String),
}

impl StopPick {
    /// 保存する形(`face`, `vertex`)。頂点なら面の名前は空。
    pub fn parts(self) -> (String, Option<String>) {
        match self {
            StopPick::Face(f) => (f, None),
            StopPick::Vertex(v) => (String::new(), Some(v)),
        }
    }
}

/// 選んだ相手: 引数 `face`(面の名前か `{"vertex": 名前, "feature": 番号}`)・`vertex`、なければ
/// 3D のビューで選んでいる面か頂点。番号は引数 `feature`(か値の中の `feature`)か、選んだ物の機能。
pub fn stop_pick(cx: &Context<'_>) -> Option<(StopPick, Option<FeatureId>)> {
    let num = |v: Option<&serde_json::Value>| v.and_then(|v| v.as_u64()).map(FeatureId);
    let given = num(cx.arg("feature"));
    match cx.arg("face") {
        Some(serde_json::Value::String(f)) => {
            // 手順で押した面は、選んでいる面(押した所)の機能から探す
            let from = given.or(cx.picked.filter(|p| p.face == *f).map(|p| p.feature));
            return Some((StopPick::Face(f.clone()), from));
        }
        Some(o @ serde_json::Value::Object(_)) => {
            let v = o.get("vertex")?.as_str()?;
            return Some((StopPick::Vertex(v.into()), num(o.get("feature")).or(given)));
        }
        _ => {}
    }
    if let Some(v) = cx.arg("vertex").and_then(|v| v.as_str()) {
        return Some((StopPick::Vertex(v.into()), given));
    }
    if let Some(p) = cx.picked {
        return Some((StopPick::Face(p.face.clone()), Some(p.feature)));
    }
    cx.vertices
        .first()
        .map(|v| (StopPick::Vertex(v.vertex.clone()), Some(v.feature)))
}

/// ボディ `body` に相手 `pick` があるか(面なら平らな面だけ)。
pub fn has(kernel: &dyn jc_core::Kernel, body: ShapeHandle, pick: &StopPick) -> bool {
    match pick {
        StopPick::Face(face) => kernel
            .faces(body)
            .is_ok_and(|fs| fs.iter().any(|x| x.name == *face && x.planar.is_some())),
        StopPick::Vertex(v) => jc_core::vertices(kernel, body).iter().any(|x| x.name == *v),
    }
}

/// ボディ `body` の頂点 `name` の位置。
pub fn vertex_at(
    kernel: &dyn jc_core::Kernel,
    body: ShapeHandle,
    name: &str,
) -> Result<k::P3, String> {
    jc_core::vertices(kernel, body)
        .into_iter()
        .find(|x| x.name == name)
        .map(|x| x.at)
        .ok_or_else(|| {
            format!("対象の頂点 {name} が見つかりません(形が変わって頂点が無くなった時は、選び直してください)")
        })
}

/// [`stop_pick`] が読む引数(押し出しなどの「対象まで」の相手)。
pub fn args() -> Vec<jc_core::Arg> {
    use jc_core::Arg;
    vec![
        Arg::value(
            "face",
            "相手の平らな面の名前か {\"vertex\": 頂点の名前, \"feature\": 機能の番号}。省くと 3D のビューで選んでいる面か頂点",
        ),
        Arg::text("vertex", "相手の頂点の名前"),
        Arg::feature(
            "feature",
            "相手の面・頂点を持つ機能の番号(省くと選んだ物の機能)",
        ),
    ]
}
