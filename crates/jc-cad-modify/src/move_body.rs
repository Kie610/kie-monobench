//! ボディの移動・コピー(`solid.move@1`)。値を打って動かす(移動・回転・点から点)。引いて動かす取っ手は作らない: 初めの値は 0 なので、
//! 打つまで何も動かない(選んだだけで物が跳ねない)。「コピー」を入れると元のボディを残し、
//! 動かした写しを新しいボディにする。
use jc_cad::*;
use jc_core::kernel::{self as k, P3};
use jc_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, RefKind, Registry, Value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const MOVE: &str = "solid.move@1";
/// 回転の軸を、原点の軸かスケッチの線のクリックで選ぶ操作。
pub const MOVE_AXIS: &str = "cad.move_axis";

/// 動かし方。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveMode {
    /// X・Y・Z の距離だけ平行に動かす
    #[default]
    Translate,
    /// 軸まわりに回す
    Rotate,
    /// 点 `from_point` が点 `to_point` に来るように平行に動かす
    PointToPoint,
}

fn zero() -> String {
    "0 mm".into()
}

fn zero_deg() -> String {
    "0 deg".into()
}

fn z_dir() -> P3 {
    [0.0, 0.0, 1.0]
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 例: `{"body": 2, "x": "10 mm"}`、`{"body": 2, "mode": "rotate", "axis_ref": {"kind": "origin_z"},
/// "angle": "90 deg", "copy": true}`、`{"body": 2, "mode": "point_to_point", "from_point": [0, 0, 0],
/// "to_point": [5, 5, 0]}`。回転の軸は `axis_ref`(原点の軸かスケッチの線)か、`origin` と `axis`
/// (`origin` を省くとボディの外形の中心)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveBody {
    pub body: FeatureId,
    #[serde(default)]
    pub mode: MoveMode,
    #[serde(default = "zero")]
    pub x: String,
    #[serde(default = "zero")]
    pub y: String,
    #[serde(default = "zero")]
    pub z: String,
    #[serde(default, skip_serializing_if = "AxisRef::is_vector")]
    pub axis_ref: AxisRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<P3>,
    #[serde(default = "z_dir")]
    pub axis: P3,
    #[serde(default = "zero_deg")]
    pub angle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_point: Option<P3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_point: Option<P3>,
    /// 元のボディを残し、動かした写しを新しいボディにする
    #[serde(default, skip_serializing_if = "is_false")]
    pub copy: bool,
}

impl Exprs for MoveBody {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.x, &mut self.y, &mut self.z, &mut self.angle]
    }
}

impl MoveBody {
    /// 動かす行列。
    fn matrix(&self, ctx: &EvalCtx<'_>, src: k::ShapeHandle) -> Result<k::Matrix4, EvalError> {
        Ok(match self.mode {
            MoveMode::Translate => k::translation([
                length(ctx, &self.x, "X 距離")?,
                length(ctx, &self.y, "Y 距離")?,
                length(ctx, &self.z, "Z 距離")?,
            ]),
            MoveMode::Rotate => {
                let (origin, axis) = match self.axis_ref.resolve(ctx)? {
                    Some(a) => a,
                    None => {
                        let origin = match self.origin {
                            Some(p) => p,
                            None => {
                                let (lo, hi) = ctx.kernel().bounds(src).map_err(kernel_err)?;
                                k::scale(k::add(lo, hi), 0.5)
                            }
                        };
                        (origin, self.axis)
                    }
                };
                if k::dot(axis, axis) < 1e-18 {
                    return Err(EvalError::InvalidData("回転の軸の向きが 0 です".into()));
                }
                k::rotation(origin, axis, angle(ctx, &self.angle, "角度")?)
            }
            MoveMode::PointToPoint => {
                let (Some(a), Some(b)) = (self.from_point, self.to_point) else {
                    return Err(EvalError::InvalidData(
                        "元の点と先の点を入れてください".into(),
                    ));
                };
                k::translation(k::sub(b, a))
            }
        })
    }
}

impl Feature for MoveBody {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.body, "body")];
        if self.mode == MoveMode::Rotate {
            v.extend(self.axis_ref.inputs());
        }
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        // コピーを抑制した時は写しが無くなるだけ(元のボディは元の機能のまま見えている)
        if self.copy {
            Vec::new()
        } else {
            vec![("body".into(), output(self.body, "body"))]
        }
    }

    fn keeps(&self) -> Vec<FeatureId> {
        if self.copy {
            vec![self.body]
        } else {
            Vec::new()
        }
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let src = body_of(ctx, self.body)?;
        let m = self.matrix(ctx, src)?;
        // 面の名前はそのまま(後の機能が動かす前と同じ名前で指せる)。写しは別のボディなので重ならない
        let h = ctx
            .kernel()
            .transform(src, &m, "")
            .map_err(|e| EvalError::Kernel(format!("ボディを動かせませんでした({e})")))?;
        log::info!(
            "ボディ {} を{}({:?})",
            self.body,
            if self.copy {
                "写した"
            } else {
                "動かした"
            },
            self.mode
        );
        Ok([("body".to_string(), Value::Shape(h))].into())
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

pub fn register(reg: &mut Registry) {
    let rotate = |f: Field| f.only("mode", "rotate", "translate");
    reg.register_described(
        solid(MOVE, "移動/コピー")
            .menu("修正")
            .icon("movecopy")
            .pinned()
            .order(-10)
            .role(jc_interchange::role::kind::COPY_BODY)
            .hint("ボディを値で動かす・回す(コピーを入れると元を残して写す)。値を打つまで動かない")
            .aliases(
                "いどう 移動 こぴー コピー move copy かいてん 回転 てんからてん 点から点 ずらす",
            )
            .field(
                Field::pick("body", "ボディ", &[RefKind::Body], 1, Some(1))
                    .help("動かすボディ。3D のビューでボディの面をクリックして選ぶ"),
            )
            .field(Field::choice(
                "mode",
                "移動タイプ",
                &[
                    ("translate", "移動"),
                    ("rotate", "回転"),
                    ("point_to_point", "点から点"),
                ],
            ))
            .field(
                Field::expr("x", "X 距離")
                    .length()
                    .only("mode", "translate", "translate"),
            )
            .field(
                Field::expr("y", "Y 距離")
                    .length()
                    .only("mode", "translate", "translate"),
            )
            .field(
                Field::expr("z", "Z 距離")
                    .length()
                    .only("mode", "translate", "translate"),
            )
            .field(
                rotate(Field::choice(
                    "axis_ref.kind",
                    "軸",
                    &jc_cad::axis::axis_ref_choices("通る点と向きで指定"),
                ))
                .pick_on("line", MOVE_AXIS)
                .hide_choice("edge"),
            )
            .field(
                rotate(Field::vector("origin", "軸の通る点").length())
                    .and_only("axis_ref.kind", "vector", "vector")
                    .help("空ならボディの外形の中心"),
            )
            .field(
                rotate(Field::vector("axis", "軸の向き").unitless()).and_only(
                    "axis_ref.kind",
                    "vector",
                    "vector",
                ),
            )
            .field(rotate(Field::expr("angle", "角度").angle()))
            .field(
                Field::vector("from_point", "元の点")
                    .length()
                    .only("mode", "point_to_point", "translate")
                    .help("動かすボディの上の点の座標"),
            )
            .field(
                Field::vector("to_point", "先の点")
                    .length()
                    .only("mode", "point_to_point", "translate")
                    .help("元の点が来る所の座標"),
            )
            .field(
                Field::bool("copy", "コピーを作成")
                    .help("入れると元のボディを残し、動かした写しを新しいボディにする"),
            )
            .template(json!({"x": "0 mm", "y": "0 mm", "z": "0 mm", "angle": "0 deg"})),
        |d| Ok(Box::new(parse::<MoveBody>(d)?)),
    );
}
