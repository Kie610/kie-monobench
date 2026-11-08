//! 歯車の輪郭(`cad.gear@1`)。平面の上に歯車の閉じた輪郭(`profile`)と線(`curves`)、参照の円
//! (`reference`。ピッチ円・基礎円・歯先円・歯底円、ラックはピッチ線。加工はなぞらない)、諸元(`gear`、
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
//! 内歯車は外径の円(`rim`)と、歯の輪郭の穴で作る。内向きの歯の先が `tip`、大径の歯底が `root`。

pub mod body;
pub mod commands;
pub mod helical;
pub mod hub;
pub mod kinds;
pub mod tooth;

pub use body::{GEAR_BODY, GearBody};
pub use commands::{GEAR_ADD, GEAR_OUTLINE_ADD, GearCommands};
pub use kinds::sprocket::ChainSize;

use kmb_cad::plane::PlaneRef;
use kmb_cad::{Exprs, angle, count, expr_refs, length, output, rename};
use kmb_core::{
    Curves, Dim, EvalCtx, EvalError, Feature, FeatureId, InputRef, Outputs, Payload, Plane,
    PointId, Points, Profile, ProfileEdge, ProfileLoop, Region, Registry, Segment,
};
use kmb_interchange::{GEAR_OUTPUT, GearAxis, GearKind, GearSpec};
use serde::{Deserialize, Serialize};

pub const GEAR: &str = "cad.gear@1";

/// 参照の円や線を入れる出力の名前。
pub const REFERENCE_OUTPUT: &str = "reference";
/// 歯車のボディが空欄の歯幅を解決する長さの出力。
pub const DEFAULT_WIDTH_OUTPUT: &str = "default_width";

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// ラックの長さの決め方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RackSize {
    /// 歯数 × 円ピッチ
    #[default]
    Teeth,
    /// 指定した全長に入るだけの歯
    Length,
}

/// 歯車の輪郭の機能のデータ。例: `{"plane": {"kind": "xy"}, "module": "2 mm", "teeth": "20"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gear {
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: GearKind,
    /// 置く平面(原点の平面・平らな面・作業平面。`anchor` があれば、そのスケッチの平面を使う)
    #[serde(default = "plane_default")]
    pub plane: PlaneRef,
    #[serde(default = "module_default")]
    pub module: String,
    #[serde(default = "teeth_default")]
    pub teeth: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub rack_size: RackSize,
    /// ラックの全長(長さで指定する時に必須)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rack_length: Option<String>,
    /// ラックの歯底から背までの厚さ(空なら 2 × モジュール)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rack_back: Option<String>,
    #[serde(default = "pressure_default")]
    pub pressure_angle: String,
    /// 歯すじのねじれ角(空なら 0 = 平歯車)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helix_angle: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub hand: kmb_interchange::gear::Hand,
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
    /// 鎖の呼び。鎖車の時だけ使う。
    #[serde(default, skip_serializing_if = "is_default")]
    pub chain: ChainSize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_pitch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roller_diameter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_inner_width: Option<String>,
    /// 軽くする穴の数(空か 0 なら穴なし)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lightening_count: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lightening_diameter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lightening_circle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boss_diameter: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub keyway: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_width: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_depth: Option<String>,
    /// 内歯車の輪の外径(内歯車では必須)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_diameter: Option<String>,
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
    pub anchor_point: Option<PointId>,
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

fn plane_default() -> PlaneRef {
    PlaneRef::OriginXy
}

fn zero() -> String {
    "0 mm".into()
}

impl Default for Gear {
    fn default() -> Self {
        Gear {
            kind: GearKind::default(),
            plane: plane_default(),
            module: module_default(),
            teeth: teeth_default(),
            rack_size: RackSize::default(),
            rack_length: None,
            rack_back: None,
            pressure_angle: pressure_default(),
            helix_angle: None,
            hand: Default::default(),
            profile_shift: None,
            tip_round: None,
            root_round: None,
            bore: None,
            chain: ChainSize::default(),
            chain_pitch: None,
            roller_diameter: None,
            chain_inner_width: None,
            lightening_count: None,
            lightening_diameter: None,
            lightening_circle: None,
            boss_diameter: None,
            keyway: false,
            key_width: None,
            key_depth: None,
            outer_diameter: None,
            x: zero(),
            y: zero(),
            phase: None,
            anchor: None,
            anchor_point: None,
        }
    }
}

impl Exprs for Gear {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut v = vec![
            &mut self.module,
            &mut self.teeth,
            &mut self.pressure_angle,
            &mut self.x,
            &mut self.y,
        ];
        v.extend(
            [
                &mut self.rack_length,
                &mut self.rack_back,
                &mut self.helix_angle,
                &mut self.profile_shift,
                &mut self.tip_round,
                &mut self.root_round,
                &mut self.bore,
                &mut self.lightening_count,
                &mut self.lightening_diameter,
                &mut self.lightening_circle,
                &mut self.boss_diameter,
                &mut self.key_width,
                &mut self.key_depth,
                &mut self.outer_diameter,
                &mut self.phase,
                &mut self.chain_pitch,
                &mut self.roller_diameter,
                &mut self.chain_inner_width,
            ]
            .into_iter()
            .flatten(),
        );
        v
    }
}

/// 空でない式の欄の中身。
fn given(src: &Option<String>) -> Option<&str> {
    src.as_deref().filter(|s| !s.trim().is_empty())
}

/// 単位の無い数の欄(係数)。空なら `default`。
fn ratio(
    ctx: &EvalCtx<'_>,
    src: &Option<String>,
    what: &str,
    default: f64,
) -> Result<f64, EvalError> {
    let Some(src) = given(src) else {
        return Ok(default);
    };
    let q = ctx.expr(src)?;
    if q.dim != Dim::NONE {
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は単位の無い数にしてください(値: {q})"
        )));
    }
    Ok(q.value)
}

impl Gear {
    fn chain_dimensions(
        &self,
        ctx: &EvalCtx<'_>,
    ) -> Result<Option<kinds::sprocket::Chain>, EvalError> {
        if self.kind != GearKind::Sprocket {
            return Ok(None);
        }
        if let Some(chain) = self.chain.dimensions() {
            return Ok(Some(chain));
        }
        let read = |src: &Option<String>, what: &str| {
            let src = given(src)
                .ok_or_else(|| EvalError::InvalidData(format!("{what}を入力してください")))?;
            length(ctx, src, what)
        };
        let chain = kinds::sprocket::Chain {
            pitch: read(&self.chain_pitch, "鎖のピッチ")?,
            roller: read(&self.roller_diameter, "ローラーの径")?,
            inner_width: read(&self.chain_inner_width, "鎖の内幅")?,
        }
        .validate()
        .map_err(EvalError::InvalidData)?;
        Ok(Some(chain))
    }

    fn normal_params(&self, ctx: &EvalCtx<'_>) -> Result<kinds::Params, EvalError> {
        let module = length(ctx, &self.module, "モジュール")?;
        let rack_length = if self.kind == GearKind::Rack && self.rack_size == RackSize::Length {
            let source = given(&self.rack_length)
                .ok_or_else(|| EvalError::InvalidData("ラックの長さを入力してください".into()))?;
            Some(length(ctx, source, "ラックの長さ")?)
        } else {
            None
        };
        let teeth = match rack_length {
            Some(value) => {
                kinds::rack::teeth_for_length(module, value).map_err(EvalError::InvalidData)?
            }
            None => u32::try_from(count(ctx, &self.teeth)?)
                .map_err(|_| EvalError::InvalidData("歯数が大きすぎます".into()))?,
        };
        Ok(kinds::Params {
            module,
            teeth,
            pressure_angle: angle(ctx, &self.pressure_angle, "圧力角")?,
            profile_shift: ratio(ctx, &self.profile_shift, "転位係数", 0.0)?,
            tip_round: ratio(ctx, &self.tip_round, "歯先の丸み", 0.0)?,
            root_round: ratio(ctx, &self.root_round, "歯底の丸み", kinds::ROOT_ROUND)?,
            chain: self.chain_dimensions(ctx)?,
            rack_length,
            rack_back: if self.kind == GearKind::Rack {
                given(&self.rack_back)
                    .map(|s| length(ctx, s, "背の厚さ"))
                    .transpose()?
            } else {
                None
            },
        })
    }

    fn helix_angle(&self, ctx: &EvalCtx<'_>) -> Result<f64, EvalError> {
        given(&self.helix_angle).map_or(Ok(0.0), |s| angle(ctx, s, "ねじれ角"))
    }

    fn params(&self, ctx: &EvalCtx<'_>) -> Result<kinds::Params, EvalError> {
        let normal = self.normal_params(ctx)?;
        let beta = self.helix_angle(ctx)?;
        if !matches!(self.kind, GearKind::External | GearKind::Internal) {
            if beta != 0.0 || self.hand != kmb_interchange::gear::Hand::Right {
                return Err(EvalError::InvalidData(
                    "ねじれ角と向きは外歯か内歯の歯車に指定してください".into(),
                ));
            }
            return Ok(normal);
        }
        helical::transverse(&normal, beta).map_err(EvalError::InvalidData)
    }

    /// 置く平面と中心(平面の座標)。
    fn origin(&self, ctx: &EvalCtx<'_>) -> Result<(Plane, [f64; 2]), EvalError> {
        let offset = [
            length(ctx, &self.x, "中心 x")?,
            length(ctx, &self.y, "中心 y")?,
        ];
        let Some(sketch) = self.anchor else {
            return Ok((self.plane(ctx)?, offset));
        };
        let sol = Points::from_value(ctx.output(sketch, "points")?)
            .ok_or_else(|| EvalError::InvalidData(format!("{sketch} はスケッチではありません")))?;
        let plane = sol.plane;
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

    /// 置く平面。原点の平面と作業平面は X・Y の向きもそのまま使い、平らな面は面の点と法線から
    /// 決める(X の向きは法線から決まる向き。寸法を変えて面が動いても座標が面の形に引きずられない)。
    fn plane(&self, ctx: &EvalCtx<'_>) -> Result<Plane, EvalError> {
        Ok(match &self.plane {
            PlaneRef::OriginXy => Plane::XY,
            PlaneRef::OriginYz => Plane::YZ,
            PlaneRef::OriginZx => Plane::ZX,
            PlaneRef::Datum { feature } => kmb_interchange::datum_plane(ctx, *feature)?,
            PlaneRef::Vector => {
                return Err(EvalError::InvalidData(
                    "置く平面を選んでください(原点の平面・平らな面・作業平面)".into(),
                ));
            }
            face => {
                let (point, normal) = face
                    .resolve(ctx, "置く平面")?
                    .expect("平らな面は点と法線になる");
                Plane::from_point_normal(point, normal)
            }
        })
    }
}

impl Feature for Gear {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut out = match self.anchor {
            Some(s) => vec![output(s, "points")],
            None => self.plane.inputs(),
        };
        for i in expr_refs(self)? {
            if !out.contains(&i) {
                out.push(i);
            }
        }
        Ok(out)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let params = self.params(ctx)?;
        let normal = self.normal_params(ctx)?;
        let beta = self.helix_angle(ctx)?;
        if self.hand == kmb_interchange::gear::Hand::Both {
            return Err(EvalError::InvalidData(
                "ねじれの向きは右か左を選んでください。やまばはボディの欄で選びます".into(),
            ));
        }
        if beta != 0.0 && !matches!(self.kind, GearKind::External | GearKind::Internal) {
            return Err(EvalError::InvalidData(
                "ねじれ角は外歯か内歯の歯車に指定してください".into(),
            ));
        }
        let phase = given(&self.phase).map_or(Ok(0.0), |s| angle(ctx, s, "回す角"))?;
        let bore = given(&self.bore).map_or(Ok(0.0), |s| length(ctx, s, "軸の穴の径"))?;
        if self.kind == GearKind::Rack && bore != 0.0 {
            return Err(EvalError::InvalidData(
                "ラックに軸の穴は付けられません".into(),
            ));
        }
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
        let internal = self.kind == GearKind::Internal;
        if internal && bore > 0.0 {
            return Err(EvalError::InvalidData(
                "内歯車には軸の穴の径を指定できません。輪の外径を指定してください".into(),
            ));
        }
        let bore_limit = if let Some(chain) = params.chain {
            let d =
                kinds::sprocket::dimensions(chain, params.teeth).map_err(EvalError::InvalidData)?;
            d.pitch_radius - d.seat_radius
        } else {
            root
        };
        if !internal && bore / 2.0 >= bore_limit {
            return Err(EvalError::InvalidData(format!(
                "軸の穴の径 {bore:.3} mm が歯底円の径 {:.3} mm 以上です",
                2.0 * bore_limit
            )));
        }
        let place = |e: &ProfileEdge| tooth::translate(&tooth::rotate(e, phase), center);
        let hole_count = ratio(ctx, &self.lightening_count, "軽くする穴の数", 0.0)?;
        if !hole_count.is_finite()
            || !(0.0..=1000.0).contains(&hole_count)
            || (hole_count - hole_count.round()).abs() > 1e-9
        {
            return Err(EvalError::InvalidData(
                "軽くする穴の数は 0 から 1000 までの整数にしてください".into(),
            ));
        }
        let optional_length =
            |src: &Option<String>, what| given(src).map(|s| length(ctx, s, what)).transpose();
        let hub_params = hub::Params {
            count: hole_count.round() as u32,
            hole_diameter: optional_length(&self.lightening_diameter, "軽くする穴の径")?
                .unwrap_or(0.0),
            circle_diameter: optional_length(&self.lightening_circle, "穴を並べる円の径")?
                .unwrap_or(0.0),
            boss_diameter: optional_length(&self.boss_diameter, "ボスの外径")?.unwrap_or(0.0),
            keyway: self.keyway,
            key_width: optional_length(&self.key_width, "キー溝の幅")?,
            key_depth: optional_length(&self.key_depth, "キー溝の深さ")?,
        };
        let layout = if self.kind == GearKind::External {
            hub::layout(ProfileLoop { edges: shape.edges }, root, params.module, bore / 2.0, &hub_params)
                .map_err(EvalError::InvalidData)?
        } else {
            if hub_params != hub::Params::default() {
                return Err(EvalError::InvalidData("軽くする穴・ボス・キー溝は平歯車で指定してください".into()));
            }
            let outline = ProfileLoop { edges: shape.edges };
            let (outer, mut holes) = if internal {
                let src = given(&self.outer_diameter).ok_or_else(|| EvalError::InvalidData("内歯車では輪の外径を指定してください".into()))?;
                let diameter = length(ctx, src, "輪の外径")?;
                let minimum = 2.0 * root + 2.0 * params.module;
                if !diameter.is_finite() || diameter < minimum {
                    return Err(EvalError::InvalidData(format!("輪の外径は歯底円の径 + モジュールの 2 倍以上にしてください(最小の外径 {minimum:.3} mm)")));
                }
                let hole = ProfileLoop { edges: outline.edges.into_iter().rev().map(|mut e| { e.reversed = !e.reversed; e }).collect() };
                (ProfileLoop { edges: vec![ProfileEdge { name: "rim".into(), segment: Segment::Circle { center: [0.0; 2], radius: diameter / 2.0 }, reversed: false }] }, vec![hole])
            } else { (outline, Vec::new()) };
            if !internal && bore > 0.0 {
                holes.push(ProfileLoop {
                    edges: vec![ProfileEdge {
                        name: "bore".into(),
                        segment: Segment::Circle { center: [0.0; 2], radius: bore / 2.0 },
                        reversed: true,
                    }],
                });
            }
            // 内歯の穴とラック・鎖車の有限な輪郭を、分割せずボディへ渡す。
            let profile = Profile { plane: Plane::XY, regions: vec![Region { outer, holes }] };
            hub::Layout { parts: hub::Parts { rim: profile.clone(), web: None, boss: None }, profile }
        }.placed(plane, phase, center);
        let segments: Vec<Segment> = layout
            .profile
            .regions
            .iter()
            .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
            .flat_map(|ring| &ring.edges)
            .map(|e| e.segment.clone())
            .collect();
        let mut reference: Vec<Segment> = shape
            .reference
            .iter()
            .map(|(_, c, r)| Segment::Circle {
                center: [c[0] + center[0], c[1] + center[1]],
                radius: *r,
            })
            .collect();
        reference.extend(shape.reference_lines.iter().map(|e| place(e).segment));
        let pitch_radius = shape
            .reference
            .iter()
            .find(|(n, _, _)| *n == "pitch")
            .map_or(0.0, |(_, _, r)| *r);
        let spec = GearSpec {
            kind: self.kind,
            form: kmb_interchange::gear::ToothForm::Involute,
            module: if params.chain.is_some() {
                0.0
            } else {
                normal.module
            },
            normal_module: beta != 0.0,
            pressure_angle: if params.chain.is_some() {
                0.0
            } else {
                normal.pressure_angle
            },
            teeth: params.teeth,
            profile_shift: if params.chain.is_some() {
                0.0
            } else {
                params.profile_shift
            },
            pitch_radius,
            axis: GearAxis {
                origin: plane.to_world(center),
                direction: plane.normal(),
                reference: plane.x_axis,
            },
            phase,
            face_width: None,
            helix_angle: beta,
            hand: self.hand,
            cone_angle: 0.0,
            chain_pitch: params.chain.map(|c| c.pitch),
            starts: 1,
        };
        let default_width = params.chain.map_or(10.0, |c| c.default_width());
        let mut out: Outputs = [
            (
                DEFAULT_WIDTH_OUTPUT.to_string(),
                kmb_core::Value::Scalar(kmb_core::Quantity::mm(default_width)),
            ),
            ("curves".to_string(), Curves { plane, segments }.to_value()),
            (
                REFERENCE_OUTPUT.to_string(),
                Curves {
                    plane,
                    segments: reference,
                }
                .to_value(),
            ),
            ("profile".to_string(), layout.profile.to_value()),
            (hub::PARTS_OUTPUT.to_string(), layout.parts.to_value()),
            (GEAR_OUTPUT.to_string(), spec.to_value()),
        ]
        .into();
        kmb_core::add_warnings(&mut out, shape.warnings);
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
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
            .field(
                GearKind::ALL
                    .iter()
                    .filter(|k| !kinds::available(**k))
                    .fold(Field::choice("kind", "種類", &kinds), |f, k| {
                        f.hide_choice(k.key())
                    }),
            )
            .field(Field::expr("module", "モジュール").length().help("歯の大きさ。ねじれ角があれば歯直角のモジュール。ピッチ円の径 = モジュール × 歯数 / cos(ねじれ角)").only("kind", "external|internal|rack", "external"))
            .field(Field::choice("rack_size", "ラックの長さの指定", &[("teeth", "歯数"), ("length", "長さ")]).only("kind", "rack", "external"))
            .field(Field::expr("teeth", "歯数").unitless().step(1.0, 1.0).help("ラックでは長さの指定が「歯数」の時だけ使う").only("rack_size", "teeth", "teeth"))
            .field(Field::opt_expr("rack_length", "ラックの長さ").length().help("歯を含む全長。円ピッチごとに入るだけの歯を並べる").only("kind", "rack", "external").and_only("rack_size", "length", "teeth"))
            .field(Field::opt_expr("rack_back", "背の厚さ").length().help("歯底から背の面まで。空ならモジュールの 2 倍").only("kind", "rack", "external"))
            .field(Field::expr("pressure_angle", "圧力角").angle().help("ふつうは 20 deg。ねじれ角を指定する時は歯直角の圧力角").only("kind", "external|internal|rack", "external"))
            .field(Field::opt_expr("profile_shift", "転位係数").unitless().help("空なら 0。正で外歯は厚く、内歯は薄くなり、歯先円と歯底円が外へ広がる").only("kind", "external|internal", "external"))
            .field(Field::opt_expr("tip_round", "歯先の丸み").unitless().help("丸みの半径をモジュールに対する割合で。空なら 0").only("kind", "external|internal|rack", "external"))
            .field(Field::opt_expr("root_round", "歯底の丸み").unitless().help("丸みの半径をモジュールに対する割合で。空なら 0.38").only("kind", "external|internal|rack", "external"))
            .field(Field::opt_expr("bore", "軸の穴の径").length().only("kind", "external|sprocket", "external").help("空か 0 なら穴なし"))
            .field(Field::opt_expr("helix_angle", "ねじれ角").angle().help("空か 0 deg なら平歯車。0 deg 以上、90 deg 未満").only("kind", "external|internal", "external"))
            .field(Field::choice("hand", "ねじれの向き", &[("right", "右"), ("left", "左")]).only("kind", "external|internal", "external"))
            .field(Field::expr("outer_diameter", "輪の外径").length().only("kind", "internal", "external").help("内歯車で必須。歯底円の径 + モジュールの 2 倍以上"))
            .field(Field::opt_expr("lightening_count", "軽くする穴の数").unitless().step(1.0, 0.0).help("空か 0 なら穴なし").only("kind", "external", "external"))
            .field(Field::opt_expr("lightening_diameter", "軽くする穴の径").length().only("kind", "external", "external"))
            .field(Field::opt_expr("lightening_circle", "穴を並べる円の径").length().only("kind", "external", "external"))
            .field(Field::opt_expr("boss_diameter", "ボスの外径").length().help("空か 0 ならボスなし").only("kind", "external", "external"))
            .field(Field::bool("keyway", "キー溝").only("kind", "external", "external"))
            .field(Field::opt_expr("key_width", "キー溝の幅").length().help("空なら軸の穴の径で表から選ぶ").only("kind", "external", "external"))
            .field(Field::opt_expr("key_depth", "キー溝の深さ").length().help("空なら軸の穴の径で表から選ぶ").only("kind", "external", "external"))
            .field(Field::choice("chain", "鎖の呼び", &ChainSize::CHOICES).only("kind", "sprocket", "external"))
            .field(Field::opt_expr("chain_pitch", "鎖のピッチ").length().only("kind", "sprocket", "external").and_only("chain", "manual", "40"))
            .field(Field::opt_expr("roller_diameter", "ローラーの径").length().only("kind", "sprocket", "external").and_only("chain", "manual", "40"))
            .field(Field::opt_expr("chain_inner_width", "鎖の内幅").length().only("kind", "sprocket", "external").and_only("chain", "manual", "40"))
            .field(
                Field::opt_output("anchor", "中心の点のスケッチ", "points")
                    .help("スケッチの単独の点に中心を合わせる。点は拘束で動かせる"),
            )
            .field(Field::expr("x", "中心 x").length().help("中心の点があれば、そこからのずれ。ラックはピッチ線の始点"))
            .field(Field::expr("y", "中心 y").length().help("ラックはピッチ線の始点"))
            .field(Field::opt_expr("phase", "回す角").angle().help("1 枚目の歯の向き。ラックは進む向き。空なら 0"))
            .template(serde_json::json!({"module": "1 mm", "teeth": "20"}))
            // 輪郭を足す時は、それを押し出すボディも同じ取り消し単位で足す(直すのは輪郭の欄)。
            // ボディの無い輪郭は操作「歯車の輪郭」で足す
            .companion(body::GEAR_BODY, serde_json::json!({"gear": {"$new": 0}})),
        |d| {
            let g: Gear = serde_json::from_value(d.clone())
                .map_err(|e| EvalError::InvalidData(e.to_string()))?;
            Ok(Box::new(g))
        },
    );
}

/// この crate の名乗り(歯車の輪郭とボディの種類、操作「歯車」「歯車の輪郭」)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-gear", |reg| {
        register(reg);
        body::register(reg);
    })
    .with_extension(GearCommands)
}

#[cfg(test)]
mod commands_tests;
#[cfg(test)]
mod tests;
