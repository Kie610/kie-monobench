//! ボディの移動・コピー(`solid.move@1`)。値を打って動かす(移動・回転・点から点)。引いて動かす取っ手は作らない: 初めの値は 0 なので、
//! 打つまで何も動かない(選んだだけで物が跳ねない)。「コピー」を入れると元のボディを残し、
//! 動かした写しを新しいボディにする。点から点の元の点と先の点は、座標か点の参照([`EndPoint`])で持つ。
//! 参照で持てば、元の形やスケッチの寸法を変えた時に動かしたボディが付いて来る。
use kmb_cad::*;
use kmb_core::kernel::{self as k, P3};
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, RefKind, Registry, Value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const MOVE: &str = "solid.move@1";
/// 回転の軸を、原点の軸かスケッチの線のクリックで選ぶ操作。
pub const MOVE_AXIS: &str = "cad.move_axis";
/// 点から点の元の点と先の点を、続けてクリックで選ぶ操作(移動タイプを「点から点」にすると始まる)。
pub const MOVE_POINTS: &str = "cad.move_points";
/// 点から点の元の点を、ボディの角・円の中心・辺の中点・スケッチの点・作業点のクリックで選ぶ操作。
pub const MOVE_FROM: &str = "cad.move_from";
/// 点から点の先の点を、同じくクリックで選ぶ操作。
pub const MOVE_TO: &str = "cad.move_to";

/// 点から点の端の点(`from_ref`・`to_ref`)。`At` は座標の欄(`from_point`・`to_point`)で決める
/// (前から保存していた文書の形。鍵が無ければこれ)。`Pick` は欄で「クリックで選ぶ」を選んだだけで、
/// まだ点が無い。ほかは点の参照で、参照が切れたら座標へ戻さず誤りにする。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EndPoint {
    #[default]
    At,
    Pick,
    #[serde(untagged)]
    Ref(PointRef),
}

impl EndPoint {
    pub fn is_at(&self) -> bool {
        *self == EndPoint::At
    }

    /// 端の点の位置。`coords` は座標の欄、`what` は「元の点」か「先の点」。
    fn resolve(&self, ctx: &EvalCtx<'_>, coords: Option<P3>, what: &str) -> Result<P3, EvalError> {
        match self {
            EndPoint::At => coords
                .ok_or_else(|| EvalError::InvalidData(format!("{what}の座標を入れてください"))),
            EndPoint::Pick => Err(EvalError::InvalidData(format!(
                "{what}をまだ選んでいません。右クリックの「{what}を選ぶ」で、ボディの角・円の中心・辺の中点・スケッチの点・作業点をクリックしてください"
            ))),
            EndPoint::Ref(r) => r.resolve(ctx),
        }
    }
}

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
/// (`origin` を省くとボディの外形の中心)。点から点の端は `from_ref`・`to_ref` に点の参照も書ける
/// (例: `"to_ref": {"kind": "vertex", "feature": 3, "vertex": "f3:end|f3:side:e0|f3:side:e1"}`)。
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
    #[serde(default, skip_serializing_if = "EndPoint::is_at")]
    pub from_ref: EndPoint,
    #[serde(default, skip_serializing_if = "EndPoint::is_at")]
    pub to_ref: EndPoint,
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
    /// 点から点の端のうち、点の参照で持つ物。
    fn point_refs(&self) -> impl Iterator<Item = &PointRef> {
        [&self.from_ref, &self.to_ref]
            .into_iter()
            .filter_map(|e| match e {
                EndPoint::Ref(r) => Some(r),
                _ => None,
            })
    }

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
                if self.from_ref.is_at()
                    && self.to_ref.is_at()
                    && (self.from_point.is_none() || self.to_point.is_none())
                {
                    return Err(EvalError::InvalidData(
                        "元の点と先の点を入れてください".into(),
                    ));
                }
                let a = self.from_ref.resolve(ctx, self.from_point, "元の点")?;
                let b = self.to_ref.resolve(ctx, self.to_point, "先の点")?;
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
        if self.mode == MoveMode::PointToPoint {
            for r in self.point_refs() {
                let i = r.input();
                if !v.contains(&i) {
                    v.push(i);
                }
            }
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
        let mut v = if self.copy {
            vec![self.body]
        } else {
            Vec::new()
        };
        // 点を借りるだけのほかのボディは、読んでも見えるボディに残す
        if self.mode == MoveMode::PointToPoint {
            for f in self.point_refs().filter_map(PointRef::body) {
                if f != self.body && !v.contains(&f) {
                    v.push(f);
                }
            }
        }
        v
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

/// 端の点の欄。参照の種類はクリックで選んだ時だけ入るので、欄では隠す。
fn end_choice(key: &str, label: &str) -> Field {
    let mut f = Field::choice(
        key,
        label,
        &[
            ("at", "座標で指定"),
            ("pick", "クリックで選ぶ"),
            ("vertex", "ボディの角"),
            ("center", "円の中心"),
            ("midpoint", "辺の中点"),
            ("sketch", "スケッチの点"),
            ("datum", "作業点"),
        ],
    )
    .only("mode", "point_to_point", "translate");
    for hidden in ["vertex", "center", "midpoint", "sketch", "datum"] {
        f = f.hide_choice(hidden);
    }
    f
}

pub fn register(reg: &mut Registry) {
    let rotate = |f: Field| f.only("mode", "rotate", "translate");
    reg.register_described(
        solid(MOVE, "移動/コピー")
            .menu("修正")
            .icon("movecopy")
            .pinned()
            .order(-10)
            .role(kmb_interchange::role::kind::COPY_BODY)
            .hint("ボディを値で動かす・回す(コピーを入れると元を残して写す)。値を打つまで動かない")
            .aliases(
                "いどう 移動 こぴー コピー move copy かいてん 回転 てんからてん 点から点 ずらす",
            )
            .field(
                Field::pick("body", "ボディ", &[RefKind::Body], 1, Some(1))
                    .help("動かすボディ。3D のビューでボディの面をクリックして選ぶ"),
            )
            .field(
                Field::choice(
                    "mode",
                    "移動タイプ",
                    &[
                        ("translate", "移動"),
                        ("rotate", "回転"),
                        ("point_to_point", "点から点"),
                    ],
                )
                .pick_on("point_to_point", MOVE_POINTS),
            )
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
                    &kmb_cad::axis::axis_ref_choices("通る点と向きで指定"),
                ))
                .pick_on("line", MOVE_AXIS)
                .hide_choice("edge")
                .hide_choice("datum")
                .hide_choice("segment"),
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
                end_choice("from_ref.kind", "元の点")
                    .pick_on("pick", MOVE_FROM)
                    .help("動かすボディの上の点。クリックで選ぶと、元の形を変えても同じ点を追う"),
            )
            .field(
                Field::vector("from_point", "元の点の座標")
                    .length()
                    .only("mode", "point_to_point", "translate")
                    .and_only("from_ref.kind", "at", "at")
                    .help("動かすボディの上の点の座標"),
            )
            .field(
                end_choice("to_ref.kind", "先の点")
                    .pick_on("pick", MOVE_TO)
                    .help(
                        "元の点が来る所。クリックで選ぶと、相手の形やスケッチを変えても付いて来る",
                    ),
            )
            .field(
                Field::vector("to_point", "先の点の座標")
                    .length()
                    .only("mode", "point_to_point", "translate")
                    .and_only("to_ref.kind", "at", "at")
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
