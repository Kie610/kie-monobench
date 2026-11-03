//! 歯車の輪郭(`cad.gear@1`)。平面の上に歯車の閉じた輪郭(`profile`)と線(`curves`)、参照の円
//! (`reference`。ピッチ円・基礎円・歯先円・歯底円。加工はなぞらない)、諸元(`gear`、
//! [`kmb_interchange::GearSpec`])を出す。輪郭はそのまま押し出し・2D 輪郭の加工・ポケット・レーザーに
//! 使える。ベルト車とロープ車の `profile` は加工用でなく軸断面で、ボディは軸のまわりに回して作る。
//! 厚さを付けた立体は歯車のボディ(`cad.gear_body@1`、[`body`])が輪郭を読んで作る。
//!
//! 置き方は文字と同じ: 平面(原点の平面・作業平面・平らな面)の座標 `x`・`y` か、スケッチの点
//! `anchor` からのずれに中心を置き、`phase` で 1 枚目の歯の向きを回す。
//!
//! 区間の名前(押し出した面の名前の元): `t<i>:r<k>`・`t<i>:l<k>`(i 枚目の歯の右・左の歯面の k 番目の
//! 円弧。歯を外へ向けて見た右と左)、`t<i>:rfillet`・`t<i>:rneck`・`t<i>:rround`(歯底の丸み・基礎円
//! より内の線・歯先の丸み)、`t<i>:tip`(歯先)、`t<i>:root`(i 枚目と次の歯の間の歯底)、`bore`(軸の
//! 穴)。歯数を減らして消えた歯の面を指していた後ろの機能は、参照切れで止まる(別の歯へ付け替えない)。

pub mod body;
pub mod commands;
pub mod kinds;
pub mod tooth;

pub use body::{GEAR_BODY, GearBody};
pub use commands::{GEAR_ADD, GEAR_OUTLINE_ADD, GearCommands};
pub use kinds::pulley::VSection;

use kmb_cad::plane::PlaneRef;
use kmb_cad::{Exprs, angle, count, expr_refs, length, output, rename};
use kmb_core::{
    Curves, Dim, EvalCtx, EvalError, Feature, FeatureId, InputRef, Outputs, Payload, Plane,
    PointId, Points, Profile, ProfileEdge, ProfileLoop, Region, Registry, Segment,
};
use kmb_interchange::{GEAR_OUTPUT, GearAxis, GearKind, GearSpec};
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
    /// 置く平面(原点の平面・平らな面・作業平面。`anchor` があれば、そのスケッチの平面を使う)
    #[serde(default = "plane_default")]
    pub plane: PlaneRef,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pulley_diameter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pulley_width: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crown_height: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groove_count: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groove_angle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groove_top_width: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groove_depth: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rope_diameter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flange_height: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub v_section: VSection,

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
            pressure_angle: pressure_default(),
            profile_shift: None,
            tip_round: None,
            root_round: None,
            bore: None,
            pulley_diameter: None,
            pulley_width: None,
            crown_height: None,
            groove_count: None,
            groove_angle: None,
            groove_top_width: None,
            groove_depth: None,
            rope_diameter: None,
            flange_height: None,
            v_section: VSection::default(),
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
                &mut self.profile_shift,
                &mut self.tip_round,
                &mut self.root_round,
                &mut self.bore,
                &mut self.phase,
                &mut self.pulley_diameter,
                &mut self.pulley_width,
                &mut self.crown_height,
                &mut self.groove_count,
                &mut self.groove_angle,
                &mut self.groove_top_width,
                &mut self.groove_depth,
                &mut self.rope_diameter,
                &mut self.flange_height,
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
    fn params(&self, ctx: &EvalCtx<'_>) -> Result<kinds::Params, EvalError> {
        let len = |src: &Option<String>, what: &str, default: f64| {
            given(src).map_or(Ok(default), |s| length(ctx, s, what))
        };
        let optional_len =
            |src: &Option<String>, what: &str| given(src).map(|s| length(ctx, s, what)).transpose();
        let diameter = if self.kind.is_pulley() {
            len(&self.pulley_diameter, "ピッチ円の径", 100.0)?
        } else {
            100.0
        };
        let width = if self.kind.is_pulley() {
            len(&self.pulley_width, "幅", 30.0)?
        } else {
            30.0
        };
        let bore = if self.kind.is_pulley() {
            len(&self.bore, "軸の穴の径", 0.0)?
        } else {
            0.0
        };
        let pulley = if matches!(self.kind, GearKind::FlatPulley | GearKind::VPulley) {
            Some(kinds::pulley::Pulley {
                diameter,
                width,
                bore,
                crown: len(&self.crown_height, "中高の高さ", 0.0)?,
                grooves: given(&self.groove_count)
                    .map_or(Ok(1), |s| count(ctx, s))
                    .and_then(|n| {
                        u32::try_from(n)
                            .map_err(|_| EvalError::InvalidData("溝の数が大きすぎます".into()))
                    })?,
                section: self.v_section,
                angle: given(&self.groove_angle)
                    .map(|s| angle(ctx, s, "溝の角"))
                    .transpose()?,
                top_width: optional_len(&self.groove_top_width, "溝の上の幅")?,
                depth: optional_len(&self.groove_depth, "溝の深さ")?,
            })
        } else {
            None
        };
        let sheave = if self.kind == GearKind::RopeSheave {
            Some(kinds::sheave::Sheave {
                diameter,
                width,
                bore,
                rope: len(&self.rope_diameter, "ロープの径", 10.0)?,
                flange: len(&self.flange_height, "縁の高さ", 3.0)?,
            })
        } else {
            None
        };
        Ok(kinds::Params {
            module: length(ctx, &self.module, "モジュール")?,
            teeth: count(ctx, &self.teeth)? as u32,
            pressure_angle: angle(ctx, &self.pressure_angle, "圧力角")?,
            profile_shift: ratio(ctx, &self.profile_shift, "転位係数", 0.0)?,
            tip_round: ratio(ctx, &self.tip_round, "歯先の丸み", 0.0)?,
            root_round: ratio(ctx, &self.root_round, "歯底の丸み", kinds::ROOT_ROUND)?,
            pulley,
            sheave,
        })
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
        let phase = given(&self.phase).map_or(Ok(0.0), |s| angle(ctx, s, "回す角"))?;
        let bore = given(&self.bore).map_or(Ok(0.0), |s| length(ctx, s, "軸の穴の径"))?;
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
        let profile_plane = if shape.axial {
            let (sin, cos) = phase.sin_cos();
            Plane {
                origin: plane.to_world(center),
                x_axis: [0, 1, 2].map(|i| plane.x_axis[i] * cos + plane.y_axis[i] * sin),
                y_axis: plane.normal(),
            }
        } else {
            plane
        };
        let outer = ProfileLoop {
            edges: if shape.axial {
                shape.edges.clone()
            } else {
                shape.edges.iter().map(place).collect()
            },
        };
        let mut holes = Vec::new();
        if !shape.axial && bore > 0.0 {
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
        let segments: Vec<Segment> = if shape.axial {
            let radius = shape
                .reference
                .iter()
                .find(|(n, _, _)| *n == "tip")
                .map_or(0.0, |(_, _, r)| *r);
            vec![Segment::Circle { center, radius }]
        } else {
            outer
                .edges
                .iter()
                .chain(holes.iter().flat_map(|h| &h.edges))
                .map(|e| e.segment.clone())
                .collect()
        };
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
            module: if shape.axial { 0.0 } else { params.module },
            normal_module: false,
            pressure_angle: if shape.axial {
                0.0
            } else {
                params.pressure_angle
            },
            teeth: if shape.axial { 0 } else { params.teeth },
            profile_shift: if shape.axial {
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
            helix_angle: 0.0,
            hand: kmb_interchange::gear::Hand::Right,
            cone_angle: 0.0,
            chain_pitch: None,
            starts: 1,
        };
        let profile = Profile {
            plane: profile_plane,
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
        let mut warnings = shape.warnings;
        if shape.axial {
            warnings.push(
                "ベルト車とロープ車の profile は軸断面です。加工の 2D 輪郭には使わないでください"
                    .into(),
            );
        }
        kmb_core::add_warnings(&mut out, warnings);
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
            .field(Field::expr("module", "モジュール").length().help("歯の大きさ。ピッチ円の径 = モジュール × 歯数").only("kind","external","external"))
            .field(Field::expr("teeth", "歯数").unitless().step(1.0, 3.0).only("kind","external","external"))
            .field(Field::expr("pressure_angle", "圧力角").angle().help("ふつうは 20 deg").only("kind","external","external"))
            .field(Field::opt_expr("profile_shift", "転位係数").unitless().help("空なら 0。正で歯が厚く、歯先円と歯底円が外へ広がる").only("kind","external","external"))
            .field(Field::opt_expr("tip_round", "歯先の丸み").unitless().help("丸みの半径をモジュールに対する割合で。空なら 0").only("kind","external","external"))
            .field(Field::opt_expr("root_round", "歯底の丸み").unitless().help("丸みの半径をモジュールに対する割合で。空なら 0.38").only("kind","external","external"))
            .field(Field::opt_expr("pulley_diameter","ピッチ円の径").length().help("空なら 100 mm").only("kind","flat_pulley|v_pulley|rope_sheave","external"))
            .field(Field::opt_expr("pulley_width","幅").length().help("空なら 30 mm。ベルト車とロープ車の軸方向の幅").only("kind","flat_pulley|v_pulley|rope_sheave","external"))
            .field(Field::opt_expr("crown_height","中高の高さ").length().help("空なら 0 mm").only("kind","flat_pulley","external"))
            .field(Field::opt_expr("groove_count","溝の数").unitless().step(1.0,1.0).help("空なら 1").only("kind","v_pulley","external"))
            .field(Field::choice("v_section","溝の型",&[("A","A"),("B","B")]).only("kind","v_pulley","external"))
            .field(Field::opt_expr("groove_angle","溝の角").angle().help("空なら型とピッチ円の径から選ぶ").only("kind","v_pulley","external"))
            .field(Field::opt_expr("groove_top_width","溝の上の幅").length().help("空なら型のピッチの位置の幅から求める").only("kind","v_pulley","external"))
            .field(Field::opt_expr("groove_depth","溝の深さ").length().help("溝の上端から底まで。空なら型の寸法から求める").only("kind","v_pulley","external"))
            .field(Field::opt_expr("rope_diameter","ロープの径").length().help("空なら 10 mm").only("kind","rope_sheave","external"))
            .field(Field::opt_expr("flange_height","縁の高さ").length().help("空なら 3 mm。ロープの中心からの高さ").only("kind","rope_sheave","external"))
            .field(Field::opt_expr("bore", "軸の穴の径").length().help("空か 0 なら穴なし"))
            .field(
                Field::opt_output("anchor", "中心の点のスケッチ", "points")
                    .help("スケッチの単独の点に中心を合わせる。点は拘束で動かせる"),
            )
            .field(Field::expr("x", "中心 x").length().help("中心の点があれば、そこからのずれ"))
            .field(Field::expr("y", "中心 y").length())
            .field(Field::opt_expr("phase", "回す角").angle().help("1 枚目の歯の向き。空なら 0"))
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
