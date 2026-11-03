//! 歯車の輪郭(`cad.gear@1`)。平面の上に歯車の閉じた輪郭(`profile`)と線(`curves`)、参照の円
//! (`reference`。ピッチ円・基礎円・歯先円・歯底円。加工はなぞらない)、諸元(`gear`、
//! [`kmb_interchange::GearSpec`])を出す。輪郭はそのまま押し出し・2D 輪郭の加工・ポケット・レーザーに
//! 使える。厚さを付けた立体は歯車のボディ(`cad.gear_body@1`、[`body`])が輪郭を読んで作る。
//!
//! 置き方は文字と同じ: 平面(原点の平面・作業平面・平らな面)の座標 `x`・`y` か、スケッチの点
//! `anchor` からのずれに中心を置き、`phase` で 1 枚目の歯の向きを回す。
//!
//! 区間の名前(押し出した面の名前の元): `t<i>:r<k>`・`t<i>:l<k>`(i 枚目の歯の右・左の歯面の k 番目の
//! 円弧。歯を外へ向けて見た右と左)、`t<i>:rfillet`・`t<i>:rneck`・`t<i>:rround`(歯底の丸み・基礎円
//! より内の線・歯先の丸み)、`t<i>:tip`(歯先)、`t<i>:root`(i 枚目と次の歯の間の歯底)、`bore`(軸の
//! 穴)。歯数を減らして消えた歯の面を指していた後ろの機能は、参照切れで止まる(別の歯へ付け替えない)。

pub mod body;
pub mod kinds;
pub mod tooth;

pub use body::{GEAR_BODY, GearBody};

use kmb_core::{
    Curves, Dim, EvalCtx, EvalError, Feature, FeatureId, InputRef, Outputs, Payload, Profile,
    ProfileEdge, ProfileLoop, Region, Registry, Segment, expr_inputs,
};
use kmb_interchange::{GEAR_OUTPUT, GearAxis, GearKind, GearSpec};
use kmb_sketch::{EntityId, SketchPlane, profile_value, resolve_plane, solution_of};
use serde::{Deserialize, Serialize};

pub const GEAR: &str = "cad.gear@1";

/// 参照の円を入れる出力の名前。
pub const REFERENCE_OUTPUT: &str = "reference";

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// 歯車の輪郭の機能のデータ。例: `{"plane": {"kind": "xy"}, "module": "2 mm", "teeth": "20"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gear {
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: GearKind,
    /// 置く平面(`anchor` があれば、そのスケッチの平面を使う)
    #[serde(default)]
    pub plane: SketchPlane,
    #[serde(default = "module_default")]
    pub module: String,
    #[serde(default = "teeth_default")]
    pub teeth: String,
    #[serde(default = "pressure_default")]
    pub pressure_angle: String,
    /// 転位係数(空なら 0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_shift: Option<String>,
    /// 歯先の丸みの半径の係数(モジュールに対する割合。空なら 0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_round: Option<String>,
    /// 歯底の丸みの半径の係数(空なら基準ラックの 0.38)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_round: Option<String>,
    /// 軸の穴の径(空か 0 なら穴なし)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bore: Option<String>,
    /// 中心。平面の座標(`anchor` があれば、その点からのずれ)
    #[serde(default = "zero")]
    pub x: String,
    #[serde(default = "zero")]
    pub y: String,
    /// 1 枚目の歯の向き(平面の X から反時計回り。空なら 0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// 中心の点のあるスケッチ
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<FeatureId>,
    /// 中心の点(省くと、そのスケッチの単独の点のうち最初のもの)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_point: Option<EntityId>,
}

fn module_default() -> String {
    "1 mm".into()
}

fn teeth_default() -> String {
    "20".into()
}

fn pressure_default() -> String {
    "20 deg".into()
}

fn zero() -> String {
    "0 mm".into()
}

impl Default for Gear {
    fn default() -> Self {
        Gear {
            kind: GearKind::default(),
            plane: SketchPlane::default(),
            module: module_default(),
            teeth: teeth_default(),
            pressure_angle: pressure_default(),
            profile_shift: None,
            tip_round: None,
            root_round: None,
            bore: None,
            x: zero(),
            y: zero(),
            phase: None,
            anchor: None,
            anchor_point: None,
        }
    }
}

fn exprs(g: &mut Gear) -> Vec<&mut String> {
    let mut v = vec![
        &mut g.module,
        &mut g.teeth,
        &mut g.pressure_angle,
        &mut g.x,
        &mut g.y,
    ];
    v.extend(
        [
            &mut g.profile_shift,
            &mut g.tip_round,
            &mut g.root_round,
            &mut g.bore,
            &mut g.phase,
        ]
        .into_iter()
        .flatten(),
    );
    v
}

fn value(ctx: &EvalCtx<'_>, src: &str, what: &str, dim: Dim) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if q.dim != dim {
        let kind = if dim == Dim::LENGTH {
            "長さ"
        } else if dim == Dim::ANGLE {
            "角度"
        } else {
            "単位の無い数"
        };
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は{kind}ではありません(値: {q})"
        )));
    }
    Ok(q.value)
}

fn opt_value(
    ctx: &EvalCtx<'_>,
    src: &Option<String>,
    what: &str,
    dim: Dim,
    default: f64,
) -> Result<f64, EvalError> {
    match src {
        Some(s) if !s.trim().is_empty() => value(ctx, s, what, dim),
        _ => Ok(default),
    }
}

impl Gear {
    fn params(&self, ctx: &EvalCtx<'_>) -> Result<kinds::Params, EvalError> {
        let teeth = value(ctx, &self.teeth, "歯数", Dim::NONE)?;
        if teeth.fract().abs() > 1e-9 || teeth < 1.0 || teeth > 10_000.0 {
            return Err(EvalError::InvalidData(format!(
                "歯数 `{}` は 1 以上の整数にしてください(値: {teeth})",
                self.teeth
            )));
        }
        Ok(kinds::Params {
            module: value(ctx, &self.module, "モジュール", Dim::LENGTH)?,
            teeth: teeth.round() as u32,
            pressure_angle: value(ctx, &self.pressure_angle, "圧力角", Dim::ANGLE)?,
            profile_shift: opt_value(ctx, &self.profile_shift, "転位係数", Dim::NONE, 0.0)?,
            tip_round: opt_value(ctx, &self.tip_round, "歯先の丸み", Dim::NONE, 0.0)?,
            root_round: opt_value(
                ctx,
                &self.root_round,
                "歯底の丸み",
                Dim::NONE,
                kinds::ROOT_ROUND,
            )?,
        })
    }

    /// 置く平面と中心(平面の座標)。
    fn origin(&self, ctx: &EvalCtx<'_>) -> Result<(kmb_core::Plane, [f64; 2]), EvalError> {
        let offset = [
            value(ctx, &self.x, "中心 x", Dim::LENGTH)?,
            value(ctx, &self.y, "中心 y", Dim::LENGTH)?,
        ];
        let Some(sketch) = self.anchor else {
            return Ok((resolve_plane(&self.plane, ctx)?, offset));
        };
        let not_sketch = || EvalError::InvalidData(format!("{sketch} はスケッチではありません"));
        let sol = solution_of(ctx.output(sketch, "sketch")?).ok_or_else(not_sketch)?;
        let plane = profile_value(ctx.output(sketch, "profile")?)
            .ok_or_else(not_sketch)?
            .plane;
        let point = match self.anchor_point {
            Some(p) => p,
            None => *sol.lone.first().ok_or_else(|| {
                EvalError::InvalidData(
                    "中心の点がありません(スケッチに、線や円に使っていない点を描いてください)"
                        .into(),
                )
            })?,
        };
        let p = sol.points.get(&point).ok_or_else(|| {
            EvalError::InvalidData(format!("中心の点 {point} がスケッチにありません"))
        })?;
        Ok((plane, [p[0] + offset[0], p[1] + offset[1]]))
    }
}

impl Feature for Gear {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut out = Vec::new();
        if let Some(s) = self.anchor {
            for slot in ["sketch", "profile"] {
                out.push(InputRef::Output {
                    feature: s,
                    slot: slot.into(),
                });
            }
        } else {
            out.extend(self.plane.inputs());
        }
        for e in exprs(&mut self.clone()) {
            for i in expr_inputs(e)? {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
        }
        Ok(out)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let params = self.params(ctx)?;
        let phase = opt_value(ctx, &self.phase, "回す角", Dim::ANGLE, 0.0)?;
        let bore = opt_value(ctx, &self.bore, "軸の穴の径", Dim::LENGTH, 0.0)?;
        let (plane, center) = self.origin(ctx)?;
        let shape = kinds::outline(self.kind, &params).map_err(EvalError::InvalidData)?;
        let root = shape
            .reference
            .iter()
            .find(|(n, _, _)| *n == "root")
            .map_or(f64::INFINITY, |(_, _, r)| *r);
        if bore < 0.0 {
            return Err(EvalError::InvalidData(
                "軸の穴の径は 0 以上にしてください".into(),
            ));
        }
        if bore / 2.0 >= root {
            return Err(EvalError::InvalidData(format!(
                "軸の穴の径 {bore:.3} mm が歯底円の径 {:.3} mm 以上です",
                2.0 * root
            )));
        }
        let place = |e: &ProfileEdge| tooth::translate(&tooth::rotate(e, phase), center);
        let outer = ProfileLoop {
            edges: shape.edges.iter().map(place).collect(),
        };
        let mut holes = Vec::new();
        if bore > 0.0 {
            holes.push(ProfileLoop {
                edges: vec![ProfileEdge {
                    name: "bore".into(),
                    segment: Segment::Circle {
                        center,
                        radius: bore / 2.0,
                    },
                    reversed: true,
                }],
            });
        }
        let segments: Vec<Segment> = outer
            .edges
            .iter()
            .chain(holes.iter().flat_map(|h| &h.edges))
            .map(|e| e.segment.clone())
            .collect();
        let reference: Vec<Segment> = shape
            .reference
            .iter()
            .map(|(_, c, r)| Segment::Circle {
                center: [c[0] + center[0], c[1] + center[1]],
                radius: *r,
            })
            .collect();
        let pitch_radius = shape
            .reference
            .iter()
            .find(|(n, _, _)| *n == "pitch")
            .map_or(0.0, |(_, _, r)| *r);
        let spec = GearSpec {
            kind: self.kind,
            form: kmb_interchange::gear::ToothForm::Involute,
            module: params.module,
            normal_module: false,
            pressure_angle: params.pressure_angle,
            teeth: params.teeth,
            profile_shift: params.profile_shift,
            pitch_radius,
            axis: GearAxis {
                origin: plane.to_world(center),
                direction: plane.normal(),
                reference: plane.x_axis,
            },
            phase,
            face_width: None,
            helix_angle: 0.0,
            hand: kmb_interchange::gear::Hand::Right,
            cone_angle: 0.0,
            chain_pitch: None,
            starts: 1,
        };
        let profile = Profile {
            plane,
            regions: vec![Region { outer, holes }],
        };
        let mut out: Outputs = [
            ("curves".to_string(), Curves { plane, segments }.to_value()),
            (
                REFERENCE_OUTPUT.to_string(),
                Curves {
                    plane,
                    segments: reference,
                }
                .to_value(),
            ),
            ("profile".to_string(), profile.to_value()),
            (GEAR_OUTPUT.to_string(), spec.to_value()),
        ]
        .into();
        kmb_core::add_warnings(&mut out, shape.warnings);
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut changed = false;
        for e in exprs(&mut c) {
            let r = kmb_core::rename_reference(e, old, new)
                .map_err(|x| EvalError::InvalidData(x.to_string()))?;
            if r != *e {
                *e = r;
                changed = true;
            }
        }
        Ok(changed.then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    use kmb_core::Field;
    let kinds: Vec<(&str, &str)> = GearKind::ALL.iter().map(|k| (k.key(), k.label())).collect();
    reg.register_described(
        kmb_core::Descriptor::new(GEAR, kmb_core::category::SKETCH, "歯車の輪郭")
            .icon("gear")
            .hint("平面の上に歯車の輪郭を置く。モジュールと歯数から歯の形を作り、押し出し・2D 輪郭の加工・レーザーにそのまま使える")
            .aliases("はぐるま gear ぎあ へいはぐるま spur いんぼりゅーと involute")
            .field(Field::choice("kind", "種類", &kinds))
            .field(Field::expr("module", "モジュール").length().help("歯の大きさ。ピッチ円の径 = モジュール × 歯数"))
            .field(Field::expr("teeth", "歯数").unitless().step(1.0, 3.0))
            .field(Field::expr("pressure_angle", "圧力角").angle().help("ふつうは 20 deg"))
            .field(Field::opt_expr("profile_shift", "転位係数").unitless().help("空なら 0。正で歯が厚く、歯先円と歯底円が外へ広がる"))
            .field(Field::opt_expr("tip_round", "歯先の丸み").unitless().help("丸みの半径をモジュールに対する割合で。空なら 0"))
            .field(Field::opt_expr("root_round", "歯底の丸み").unitless().help("丸みの半径をモジュールに対する割合で。空なら 0.38"))
            .field(Field::opt_expr("bore", "軸の穴の径").length().help("空か 0 なら穴なし"))
            .field(
                Field::opt_output("anchor", "中心の点のスケッチ", "sketch")
                    .help("スケッチの単独の点に中心を合わせる。点は拘束で動かせる"),
            )
            .field(Field::expr("x", "中心 x").length().help("中心の点があれば、そこからのずれ"))
            .field(Field::expr("y", "中心 y").length())
            .field(Field::opt_expr("phase", "回す角").angle().help("1 枚目の歯の向き。空なら 0"))
            .template(serde_json::json!({"module": "1 mm", "teeth": "20"})),
        |d| {
            let g: Gear = serde_json::from_value(d.clone())
                .map_err(|e| EvalError::InvalidData(e.to_string()))?;
            Ok(Box::new(g))
        },
    );
}

/// この crate の名乗り(歯車の輪郭とボディの種類)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-gear", |reg| {
        register(reg);
        body::register(reg);
    })
}

#[cfg(test)]
mod tests;
