//! ボディの分割(`solid.split@1`)と、その残りの側(`solid.split_part@1`)。
//!
//! 今の形は「1 つの機能 = 1 つのボディ」なので、割った 2 つは 2 つの機能の組にする。分割は割る
//! ボディを使い切り、平面の法線の側を自分のボディ(`body`。元のボディの名前を受け継ぐ)、反対の側を
//! 出力 `rest` に出す。残りの側の機能は `rest` を読んで新しいボディにするだけで、欄を持たない
//! (平面を直すのは分割の方)。画面・CLI・MCP から分割を足すと、残りの側も続けて足す
//! ([`kmb_core::Descriptor::companion`])。
use kmb_cad::plane::PlaneRef;
use kmb_cad::*;
use kmb_core::kernel::{self as k, P3, Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
use kmb_core::{
    BooleanOp, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, RefKind, Registry,
    Value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const SPLIT: &str = "solid.split@1";
pub const SPLIT_PART: &str = "solid.split_part@1";

fn z_dir() -> P3 {
    [0.0, 0.0, 1.0]
}

/// ボディを平面で 2 つに割る。平面は `plane_ref`(原点の平面か平らな面)か、`origin` と `normal`
/// (`origin` を省くとボディの外形の中心。板を厚みの半分で割り、表と裏を別の段取りで削る時など)。
/// 例: `{"body": 2, "normal": [0, 0, 1]}`、`{"body": 2, "plane_ref": {"kind": "origin_yz"}}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Split {
    pub body: FeatureId,
    #[serde(default, skip_serializing_if = "PlaneRef::is_vector")]
    pub plane_ref: PlaneRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<P3>,
    #[serde(default = "z_dir")]
    pub normal: P3,
}

impl Split {
    /// 割る平面(通る点と単位の法線)。
    fn plane(&self, ctx: &EvalCtx<'_>, src: k::ShapeHandle) -> Result<(P3, P3), EvalError> {
        let (p, n) = match self.plane_ref.resolve(ctx, "割る平面")? {
            Some(v) => v,
            None => {
                let p = match self.origin {
                    Some(p) => p,
                    None => {
                        let (lo, hi) = ctx.kernel().bounds(src).map_err(kernel_err)?;
                        k::scale(k::add(lo, hi), 0.5)
                    }
                };
                (p, self.normal)
            }
        };
        if k::dot(n, n) < 1e-18 {
            return Err(EvalError::InvalidData("割る平面の法線が 0 です".into()));
        }
        Ok((p, k::normalize(n)))
    }
}

impl Feature for Split {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.body, "body")];
        v.extend(self.plane_ref.inputs());
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        vec![("body".into(), output(self.body, "body"))]
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let src = body_of(ctx, self.body)?;
        let (p, n) = self.plane(ctx, src)?;
        let kernel = ctx.kernel();
        let (lo, hi) = kernel.bounds(src).map_err(kernel_err)?;
        // 外形の箱の 8 つの角が平面のどちらの側にあるか。片側だけなら割れない
        let mut side = (f64::INFINITY, f64::NEG_INFINITY);
        for c in 0..8 {
            let q = [
                if c & 1 == 0 { lo[0] } else { hi[0] },
                if c & 2 == 0 { lo[1] } else { hi[1] },
                if c & 4 == 0 { lo[2] } else { hi[2] },
            ];
            let d = k::dot(k::sub(q, p), n);
            side = (side.0.min(d), side.1.max(d));
        }
        if side.0 >= -1e-6 || side.1 <= 1e-6 {
            return Err(EvalError::InvalidData(
                "平面がボディを横切っていません。ボディを通る平面を選んでください".into(),
            ));
        }
        // 平面の法線の側を埋める大きな箱。積が法線の側、差が反対の側
        let reach = k::dot(k::sub(hi, lo), k::sub(hi, lo)).sqrt() * 2.0 + 10.0;
        let tag = tag(ctx);
        // 箱の始まりの正方形は、外形の中心を平面に下ろした所を真ん中にする
        let center = k::scale(k::add(lo, hi), 0.5);
        let foot = k::add(center, k::scale(n, k::dot(k::sub(p, center), n)));
        let half = half_space(kernel, foot, n, reach, &tag).map_err(kernel_err)?;
        let fail = |e: k::KernelError| {
            EvalError::Kernel(format!(
                "ボディを割れませんでした({e})。平面を少しずらすと割れることがあります"
            ))
        };
        let upper = kernel
            .boolean(BooleanOp::Intersect, src, half)
            .map_err(fail)?;
        let lower = kernel
            .boolean(BooleanOp::Subtract, src, half)
            .map_err(fail)?;
        for (h, which) in [(upper, "法線の側"), (lower, "反対の側")] {
            let v = kernel.mass(h).map_err(kernel_err)?.volume;
            if !(v.is_finite() && v > 1e-9) {
                return Err(EvalError::InvalidData(format!(
                    "平面がボディを横切っていません(割った{which}が空になります)。ボディを通る平面を選んでください"
                )));
            }
        }
        log::info!("ボディ {} を平面 {p:?} 法線 {n:?} で割った", self.body);
        Ok([
            ("body".to_string(), Value::Shape(upper)),
            ("rest".to_string(), Value::Shape(lower)),
        ]
        .into())
    }
}

/// 点 `origin` を通り、法線 `n` の側を `reach` まで埋める箱(一辺 2 `reach` の正方形を押し出す)。
/// 割った面は箱の始まりの面(`{tag}:start`)から名前を受け継ぐ。
pub(crate) fn half_space(
    kernel: &dyn k::Kernel,
    origin: P3,
    n: P3,
    reach: f64,
    tag: &str,
) -> Result<k::ShapeHandle, k::KernelError> {
    let t = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let x = k::normalize(k::cross(t, n));
    let y = k::cross(n, x);
    let c =
        [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|q| [q[0] * reach, q[1] * reach]);
    let outer = ProfileLoop {
        edges: (0..4)
            .map(|i| ProfileEdge {
                name: format!("q{}", i + 1),
                segment: Segment::Line {
                    a: c[i],
                    b: c[(i + 1) % 4],
                },
                reversed: false,
            })
            .collect(),
    };
    let profile = Profile {
        plane: Plane {
            origin,
            x_axis: x,
            y_axis: y,
        },
        regions: vec![Region {
            outer,
            holes: vec![],
        }],
    };
    kernel.extrude(&profile, 0.0, reach, tag)
}

/// 分割の残りの側。分割 `of` の出力 `rest` を、新しいボディとして出す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitPart {
    pub of: FeatureId,
}

impl Feature for SplitPart {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![output(self.of, "rest")])
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        match ctx.output(self.of, "rest")? {
            Value::Shape(h) => Ok([("body".to_string(), Value::Shape(*h))].into()),
            _ => Err(EvalError::InvalidData(format!(
                "{} はボディの分割ではありません",
                self.of
            ))),
        }
    }
}

/// 割る平面を選ぶ操作。
pub const SPLIT_PLANE: &str = "cad.split_plane";

pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(SPLIT, "ボディを分割")
            .menu("修正")
            .icon("splitbody")
            .pinned()
            .order(-20)
            .hint("ボディを平面で 2 つに割る(表と裏を別の段取りで削る時など)。法線の側が元のボディの名前を受け継ぎ、反対の側は新しいボディになる")
            .aliases("ぶんかつ わる 割る split body ぼでぃ 2つ りょうめん 両面")
            .field(
                Field::pick("body", "ボディ", &[RefKind::Body], 1, Some(1))
                    .help("割るボディ。3D のビューでボディの面をクリックして選ぶ"),
            )
            .field(
                Field::choice(
                    "plane_ref.kind",
                    "割る平面",
                    &kmb_cad::plane::plane_ref_choices("通る点と法線で指定"),
                )
                .pick_on("face", SPLIT_PLANE)
                .hide_choice("datum"),
            )
            .field(
                Field::vector("origin", "平面の通る点").length()
                    .help("空ならボディの外形の中心(板なら厚みの半分で割る)")
                    .only("plane_ref.kind", "vector", "vector"),
            )
            .field(Field::vector("normal", "法線").unitless().only("plane_ref.kind", "vector", "vector"))
            .template(json!({"normal": [0.0, 0.0, 1.0]}))
            .companion(SPLIT_PART, json!({"of": {"$new": 0}})),
        |d| Ok(Box::new(parse::<Split>(d)?)),
    );
    reg.register_described(
        solid(SPLIT_PART, "分割の残り")
            .icon("splitbody")
            .hint("ボディの分割で、平面の反対の側にできたボディ。平面は分割の方で直す")
            .field(
                Field::info("of", "分割")
                    .help("この残りを作ったボディの分割。平面を変えるには分割の方を直す"),
            ),
        |d| Ok(Box::new(parse::<SplitPart>(d)?)),
    );
}
