//! ボディの尺度(`solid.scale@1`。均一・不均一)。基準の点を中心に
//! 倍率で大きさを変える。面の名前は変えない(後の機能が尺度の前と同じ名前で指せる)。
//! 不均一は円を楕円にするので、穴や円筒の面は円でなくなる(後の穴の加工は円として扱えない)。
use jc_cad::*;
use jc_core::kernel::{self as k, P3};
use jc_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, RefKind, Registry, Value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const SCALE: &str = "solid.scale@1";

/// 倍率の付け方。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleMode {
    /// 3 方向を同じ倍率 `factor` で
    #[default]
    Uniform,
    /// X・Y・Z を別々の倍率で
    NonUniform,
}

fn one() -> String {
    "1".into()
}

/// 例: `{"body": 2, "factor": "2"}`、`{"body": 2, "mode": "non_uniform", "x": "1", "y": "1",
/// "z": "0.5", "point": [0, 0, 0]}`。`point` を省くと原点を中心にする(原点に置いた部品は
/// 原点に置いたまま大きさだけが変わる)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleBody {
    pub body: FeatureId,
    #[serde(default)]
    pub mode: ScaleMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<P3>,
    #[serde(default = "one")]
    pub factor: String,
    #[serde(default = "one")]
    pub x: String,
    #[serde(default = "one")]
    pub y: String,
    #[serde(default = "one")]
    pub z: String,
}

impl Exprs for ScaleBody {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.factor, &mut self.x, &mut self.y, &mut self.z]
    }
}

/// 倍率の欄の式。単位の無い正の数(0.001〜1000)だけを受ける。
fn factor(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if !q.dim.is_none() {
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は単位の無い数にしてください(値: {q})"
        )));
    }
    if !(1e-3..=1e3).contains(&q.value) {
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は 0.001 から 1000 までの数にしてください(値: {q})"
        )));
    }
    Ok(q.value)
}

impl ScaleBody {
    /// 3 方向の倍率。
    fn factors(&self, ctx: &EvalCtx<'_>) -> Result<P3, EvalError> {
        Ok(match self.mode {
            ScaleMode::Uniform => [factor(ctx, &self.factor, "尺度係数")?; 3],
            ScaleMode::NonUniform => [
                factor(ctx, &self.x, "X 尺度")?,
                factor(ctx, &self.y, "Y 尺度")?,
                factor(ctx, &self.z, "Z 尺度")?,
            ],
        })
    }
}

/// 点 `c` を中心に、軸ごとの倍率 `s` で大きさを変える行列。
pub fn scaling(c: P3, s: P3) -> k::Matrix4 {
    let mut m = [[0.0; 4]; 4];
    for i in 0..3 {
        m[i][i] = s[i];
        m[i][3] = c[i] * (1.0 - s[i]);
    }
    m[3][3] = 1.0;
    m
}

impl Feature for ScaleBody {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.body, "body")];
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        vec![("body".into(), output(self.body, "body"))]
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let src = body_of(ctx, self.body)?;
        let s = self.factors(ctx)?;
        let m = scaling(self.point.unwrap_or([0.0; 3]), s);
        let h = ctx
            .kernel()
            .transform(src, &m, "")
            .map_err(|e| EvalError::Kernel(format!("ボディの大きさを変えられませんでした({e})")))?;
        log::info!("ボディ {} の大きさを変えた(倍率 {s:?})", self.body);
        Ok([("body".to_string(), Value::Shape(h))].into())
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

pub fn register(reg: &mut Registry) {
    let non_uniform = |f: Field| f.only("mode", "non_uniform", "uniform");
    reg.register_described(
        solid(SCALE, "尺度")
            .menu("修正")
            .icon("scale")
            .hint("ボディの大きさを、基準の点を中心に倍率で変える(不均一なら X・Y・Z を別々に)")
            .aliases("しゃくど 尺度 すけーる スケール scale 拡大 縮小 かくだい しゅくしょう 倍率")
            .field(
                Field::pick("body", "エンティティ", &[RefKind::Body], 1, Some(1))
                    .help("大きさを変えるボディ。3D のビューでボディの面をクリックして選ぶ"),
            )
            .field(
                Field::vector("point", "点")
                    .length()
                    .help("大きさを変える中心の座標。空なら原点"),
            )
            .field(Field::choice(
                "mode",
                "尺度タイプ",
                &[("uniform", "均一"), ("non_uniform", "不均一")],
            ))
            .field(
                Field::expr("factor", "尺度係数")
                    .unitless()
                    .only("mode", "uniform", "uniform")
                    .help("2 で 2 倍、0.5 で半分"),
            )
            .field(non_uniform(Field::expr("x", "X 尺度").unitless()))
            .field(non_uniform(Field::expr("y", "Y 尺度").unitless()))
            .field(non_uniform(Field::expr("z", "Z 尺度").unitless()))
            .template(json!({"factor": "1", "x": "1", "y": "1", "z": "1"})),
        |d| Ok(Box::new(parse::<ScaleBody>(d)?)),
    );
}
