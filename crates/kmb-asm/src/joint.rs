//! ジョイント(`asm.joint@1`)。
//!
//! 2 つの部品を、固定・回転・スライダ・円柱・ピンスロット・平面・ボールの関係でつなぐ
//! (7 種)。値(角度・距離)が 0 の時が作った位置で、値を変えると動く部品を軸(原点の Z)のまわりに
//! 回す・軸や原点の X・Y の向きに動かす。動く部品のボディは名前(最後の機能)を変えずに動かす
//! (コアの [`kmb_core::MOVED`])。
//!
//! 作り方は 2 つ([`How`]): 置いた位置のまま結ぶか、部品 1 の原点を
//! 部品 2 の原点に合わせて部品 1 を動かす([`Snap`])。合わせる時は、
//! 原点どうしを重ね、Z を向かい合わせ(面と面が付く向き)にし、X をそろえる。そこから角度・ずらし・
//! 反転で直せる。
//!
//! 一緒に動く物は作る時に決めてデータに持つ(`bodies`): 動く部品のボディと、それを土台にした
//! 別のジョイントの部品(その先も)。後から、動く部品を土台にするジョイントを足すと、そのジョイントの
//! 部品も前のジョイントの `bodies` に入る。土台が前のジョイントで動く時は、そのジョイントの動きを
//! 軸にも掛ける(`follows`)。部品の木を後から変えても、作ったジョイントの `bodies` は変わらない。

use crate::part::Part;
use kmb_cad::{Exprs, angle, body_of, expr_refs, kernel_err, length, output, parse, rename};
use kmb_core::kernel::{self as k, Matrix4, P3};
use kmb_core::{
    Descriptor, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Registry, Value,
    add_warnings, category, moved_slot,
};
use kmb_interchange::animation::{self as anim, DRIVE_SLOT, Drive};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const JOINT: &str = "asm.joint@1";
/// ジョイントの動き(4×4 の行列)の出力の型。
pub const TRANSFORM_TYPE: &str = "asm.transform@1";

/// ジョイントの種類。軸は原点の Z、X・Y は原点の X・Y。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    /// 動かない(一緒に動く関係だけを決める)
    Rigid,
    /// 軸のまわりに回る
    #[default]
    Revolute,
    /// 軸の向きに動く
    Slider,
    /// 軸のまわりに回り、軸の向きにも動く
    Cylindrical,
    /// 軸のまわりに回り、X の向きに動く
    PinSlot,
    /// X・Y の向きに動き、軸のまわりに回る(面に載せて滑らせる)
    Planar,
    /// 原点のまわりに、どの向きにも回る(角度・傾き・ひねり)
    Ball,
}

impl JointKind {
    /// 軸のまわりに回るか(欄 `angle`)。
    pub fn turns(self) -> bool {
        !matches!(self, JointKind::Rigid | JointKind::Slider)
    }

    /// 軸の向きに動くか(欄 `distance`)。
    pub fn slides(self) -> bool {
        matches!(self, JointKind::Slider | JointKind::Cylindrical)
    }

    /// X の向きに動くか(欄 `slide_x`)。
    pub fn slides_x(self) -> bool {
        matches!(self, JointKind::PinSlot | JointKind::Planar)
    }

    /// Y の向きに動くか(欄 `slide_y`)。
    pub fn slides_y(self) -> bool {
        self == JointKind::Planar
    }

    /// 傾きとひねりがあるか(欄 `tilt`・`twist`)。
    pub fn tilts(self) -> bool {
        self == JointKind::Ball
    }
}

pub const KINDS: &[(&str, &str)] = &[
    ("rigid", "固定"),
    ("revolute", "回転"),
    ("slider", "スライダ"),
    ("cylindrical", "円柱"),
    ("pin_slot", "ピンスロット"),
    ("planar", "平面"),
    ("ball", "ボール"),
];

/// 値の決め方。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveBy {
    /// 欄の値で動かす
    #[default]
    Value,
    /// ほかのジョイントに連動する([`Joint::link_joint`])
    Link,
}

impl DriveBy {
    fn is_value(&self) -> bool {
        *self == DriveBy::Value
    }
}

pub const DRIVE_BYS: &[(&str, &str)] =
    &[("value", "値で動かす"), ("link", "ほかのジョイントに連動")];

/// 作り方。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    /// 置いた位置のまま結ぶ
    #[default]
    AsBuilt,
    /// 部品 1 の原点を部品 2 の原点に合わせて、部品 1 を動かす([`Snap`])
    Snap,
}

impl How {
    fn is_as_built(&self) -> bool {
        *self == How::AsBuilt
    }
}

/// ジョイントの原点(点と、Z・X の向き。Y は Z × X)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub origin: P3,
    pub z: P3,
    pub x: P3,
}

impl Frame {
    /// 長さ 1 で直交した Z・X・Y。
    pub fn axes(&self) -> (P3, P3, P3) {
        basis(self.z, Some(self.x))
    }

    /// この原点の座標から世界座標への行列(列が X・Y・Z・原点)。
    pub fn matrix(&self) -> Matrix4 {
        let (z, x, y) = self.axes();
        let mut m = k::IDENTITY;
        for i in 0..3 {
            m[i][0] = x[i];
            m[i][1] = y[i];
            m[i][2] = z[i];
            m[i][3] = self.origin[i];
        }
        m
    }
}

/// 合わせるジョイントの、部品 1 の原点(作った時の、部品 1 を動かす前の世界座標)と、原点の
/// 取り方の名前(「円の中心」「2 面の間」など。タイムラインの説明に出す)。部品 2 の原点は
/// ジョイントの `origin`・`axis`・`x_dir`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snap {
    pub from: Frame,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from_label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to_label: String,
}

/// `z` に垂直な向き(世界の X・Y・Z のうち `z` といちばん離れた物を `z` に垂直にした物)。
pub fn perpendicular(z: P3) -> P3 {
    let z = k::normalize(z);
    let i = (0..3)
        .min_by(|a, b| z[*a].abs().total_cmp(&z[*b].abs()))
        .unwrap_or(0);
    let mut e = [0.0; 3];
    e[i] = 1.0;
    k::normalize(k::sub(e, k::scale(z, k::dot(e, z))))
}

/// 長さ 1 で直交した Z・X・Y(X は `x` を Z に垂直にした物。無いか Z と同じ向きなら [`perpendicular`])。
pub fn basis(z: P3, x: Option<P3>) -> (P3, P3, P3) {
    let z = k::normalize(z);
    let x = x
        .map(|x| k::sub(x, k::scale(z, k::dot(x, z))))
        .filter(|x| k::dot(*x, *x) > 1e-12)
        .map_or_else(|| perpendicular(z), k::normalize);
    (z, x, k::cross(z, x))
}

fn zero_deg() -> String {
    "0 deg".into()
}

fn zero_mm() -> String {
    "0 mm".into()
}

/// 例: `{"kind": "revolute", "moving": {"kind": "component", "id": 12}, "base": {"kind": "body",
/// "root": 3}, "bodies": [9, 10], "origin": [0, 0, 10], "axis": [1, 0, 0], "angle": "30 deg",
/// "angle_max": "110 deg", "angle_rest": "0 deg"}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Joint {
    #[serde(default)]
    pub kind: JointKind,
    /// 動く部品(名前を出すのと、後から足すジョイントが一緒に動く物を決めるのに使う)
    pub moving: Part,
    /// 土台の部品(`None` なら地面。動かない)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Part>,
    /// 一緒に動かすボディ(このジョイントの手前で受け継いだ機能)
    pub bodies: Vec<FeatureId>,
    /// 作った時の軸の通る点(世界座標。土台を動かす前)
    pub origin: P3,
    /// 作った時の軸の向き(原点の Z)
    pub axis: P3,
    /// 作った時の原点の X(ピンスロット・平面で動く向き、ボールの傾きの軸)。無ければ軸に垂直な向き
    /// ([`perpendicular`])
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_dir: Option<P3>,
    /// 作り方
    #[serde(default, skip_serializing_if = "How::is_as_built")]
    pub how: How,
    /// 合わせるジョイントの部品 1 の原点([`How::Snap`] の時だけ)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snap: Option<Snap>,
    /// 合わせる時に、部品 1 を原点の Z のまわりに回す角度
    #[serde(default = "zero_deg")]
    pub align_angle: String,
    /// 合わせる時のずらし(部品 2 の原点の X・Y・Z の向き)
    #[serde(default = "zero_mm")]
    pub offset_x: String,
    #[serde(default = "zero_mm")]
    pub offset_y: String,
    #[serde(default = "zero_mm")]
    pub offset_z: String,
    /// 合わせる時に、部品 1 の Z を部品 2 の Z と同じ向きにする(既定は向かい合わせ)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub align_flip: bool,
    /// 土台を動かす前のジョイント(タイムラインの順)。その動きを軸にも掛ける
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follows: Vec<FeatureId>,
    /// 軸の向きを逆にする
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    #[serde(default = "zero_deg")]
    pub angle: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub angle_min: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub angle_max: String,
    /// 戻る位置。部品を引いて離すと、この角度に戻る。空なら戻さない
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub angle_rest: String,
    #[serde(default = "zero_mm")]
    pub distance: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub distance_min: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub distance_max: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub distance_rest: String,
    /// X の向きの距離
    #[serde(default = "zero_mm")]
    pub slide_x: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub slide_x_min: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub slide_x_max: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub slide_x_rest: String,
    /// Y の向きの距離
    #[serde(default = "zero_mm")]
    pub slide_y: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub slide_y_min: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub slide_y_max: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub slide_y_rest: String,
    /// 傾き(X のまわりの角度)
    #[serde(default = "zero_deg")]
    pub tilt: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tilt_min: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tilt_max: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tilt_rest: String,
    /// ひねり(傾けた軸のまわりの角度)
    #[serde(default = "zero_deg")]
    pub twist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub twist_min: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub twist_max: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub twist_rest: String,
    /// 値の決め方。連動する時は、回る種類は角度、スライダは距離を、連動元の値から決める
    /// (ほかの動きは欄の値のまま)
    #[serde(default, skip_serializing_if = "DriveBy::is_value")]
    pub drive_by: DriveBy,
    /// 連動元のジョイント(タイムラインで前にある、回転かスライダのジョイント)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_joint: Option<FeatureId>,
    /// 連動元がこれだけ動く時に(回転なら角度、スライダなら距離。例: `360 deg`)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub link_lead: String,
    /// このジョイントがこれだけ動く(例: `10 mm`)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub link_follow: String,
    /// 連動する向きを逆にする
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub link_flip: bool,
}

impl Exprs for Joint {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut v = vec![
            &mut self.angle,
            &mut self.distance,
            &mut self.slide_x,
            &mut self.slide_y,
            &mut self.tilt,
            &mut self.twist,
            &mut self.align_angle,
            &mut self.offset_x,
            &mut self.offset_y,
            &mut self.offset_z,
        ];
        for e in [
            &mut self.angle_min,
            &mut self.angle_max,
            &mut self.distance_min,
            &mut self.distance_max,
            &mut self.angle_rest,
            &mut self.distance_rest,
            &mut self.slide_x_min,
            &mut self.slide_x_max,
            &mut self.slide_x_rest,
            &mut self.slide_y_min,
            &mut self.slide_y_max,
            &mut self.slide_y_rest,
            &mut self.tilt_min,
            &mut self.tilt_max,
            &mut self.tilt_rest,
            &mut self.twist_min,
            &mut self.twist_max,
            &mut self.twist_rest,
            &mut self.link_lead,
            &mut self.link_follow,
        ] {
            if !e.trim().is_empty() {
                v.push(e);
            }
        }
        v
    }
}

/// 行列の積 `a × b`(`b` を先に掛ける)。アニメーションの動きと同じ式を使う。
pub(crate) use kmb_interchange::animation::mul;

/// 動かす機能(ジョイント・拘束・位置)の出力: `bodies` を `m` で動かした形と、動き(`transform`)。
/// 動かない時(`m` が単位行列)は形を出さない。面の名前はそのまま(後の機能と加工が、動かす前と同じ
/// 名前で指せる)。
pub(crate) fn moved_outputs(
    ctx: &mut EvalCtx<'_>,
    bodies: &[FeatureId],
    m: &Matrix4,
    out: &mut Outputs,
) -> Result<(), EvalError> {
    if *m != k::IDENTITY {
        for b in bodies {
            let src = body_of(ctx, *b)?;
            let h = ctx.kernel().transform(src, m, "").map_err(kernel_err)?;
            out.insert(moved_slot(*b), Value::Shape(h));
        }
    }
    out.insert(
        "transform".into(),
        Value::Custom {
            type_key: TRANSFORM_TYPE.into(),
            data: json!(m),
        },
    );
    Ok(())
}

/// 回して動かすだけの行列の逆。
pub fn rigid_inverse(m: &Matrix4) -> Matrix4 {
    let mut inv = k::IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            inv[i][j] = m[j][i];
        }
    }
    for row in inv.iter_mut().take(3) {
        row[3] = -(0..3).map(|j| row[j] * m[j][3]).sum::<f64>();
    }
    inv
}

/// 向きに行列の回す部分だけを掛ける。
pub fn apply_dir(m: &Matrix4, d: P3) -> P3 {
    let mut out = [0.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        *o = m[i][0] * d[0] + m[i][1] * d[1] + m[i][2] * d[2];
    }
    out
}

/// ジョイントの出力 `transform` の行列。
pub fn transform_of(v: &Value) -> Option<Matrix4> {
    match v {
        Value::Custom { type_key, data } if type_key == TRANSFORM_TYPE => {
            serde_json::from_value(data.clone()).ok()
        }
        _ => None,
    }
}

type Read = fn(&EvalCtx<'_>, &str, &str) -> Result<f64, EvalError>;

/// 動きの 1 つ(自由度)の欄: 値・下限・上限・戻る位置と、警告の頭に付ける名前(角度・距離は空)。
struct Dof<'a> {
    value: &'a str,
    min: &'a str,
    max: &'a str,
    rest: &'a str,
    read: Read,
    what: &'a str,
    tag: &'a str,
}

impl Joint {
    fn dof<'a>(&'a self, key: &str) -> Dof<'a> {
        let (value, min, max, rest, read, what, tag): (_, _, _, _, Read, _, _) = match key {
            "angle" => (
                &self.angle,
                &self.angle_min,
                &self.angle_max,
                &self.angle_rest,
                angle,
                "角度",
                "",
            ),
            "distance" => (
                &self.distance,
                &self.distance_min,
                &self.distance_max,
                &self.distance_rest,
                length,
                "距離",
                "",
            ),
            "slide_x" => (
                &self.slide_x,
                &self.slide_x_min,
                &self.slide_x_max,
                &self.slide_x_rest,
                length,
                "X 距離",
                "X 距離の",
            ),
            "slide_y" => (
                &self.slide_y,
                &self.slide_y_min,
                &self.slide_y_max,
                &self.slide_y_rest,
                length,
                "Y 距離",
                "Y 距離の",
            ),
            "tilt" => (
                &self.tilt,
                &self.tilt_min,
                &self.tilt_max,
                &self.tilt_rest,
                angle,
                "傾き",
                "傾きの",
            ),
            _ => (
                &self.twist,
                &self.twist_min,
                &self.twist_max,
                &self.twist_rest,
                angle,
                "ひねり",
                "ひねりの",
            ),
        };
        Dof {
            value,
            min,
            max,
            rest,
            read,
            what,
            tag,
        }
    }

    /// 動きの 1 つの値(範囲に収めた物。外れていれば警告)と上下限(内部の単位。空なら `None`)。
    /// 下限が上限より大きい時と、戻る位置が範囲の外の時も警告を足す。
    fn value_of(
        &self,
        ctx: &EvalCtx<'_>,
        key: &str,
        warnings: &mut Vec<String>,
    ) -> Result<(f64, Option<f64>, Option<f64>), EvalError> {
        let d = self.dof(key);
        let one = |e: &str, what: &str| -> Result<Option<f64>, EvalError> {
            if e.trim().is_empty() {
                Ok(None)
            } else {
                (d.read)(ctx, e, what).map(Some)
            }
        };
        let (l, h) = (one(d.min, "下限")?, one(d.max, "上限")?);
        let tag = d.tag;
        let mut v = match self.lead(ctx)? {
            Some(l) if key == self.main_key() => self.leader_value(ctx, l)? * l.ratio,
            _ => (d.read)(ctx, d.value, d.what)?,
        };
        if let Some(l) = l
            && v < l
        {
            warnings.push(format!("{tag}値が下限より小さいので、下限まで動かしました"));
            v = l;
        }
        if let Some(h) = h
            && v > h
        {
            warnings.push(format!("{tag}値が上限より大きいので、上限まで動かしました"));
            v = h;
        }
        if let (Some(l), Some(h)) = (l, h)
            && l > h
        {
            warnings.push(format!(
                "{tag}下限が上限より大きいので、上限の値で止まります"
            ));
        }
        if let Some(r) = one(d.rest, "戻る位置")?
            && (l.is_some_and(|l| r < l - 1e-9) || h.is_some_and(|h| r > h + 1e-9))
        {
            warnings.push(format!(
                "{tag}戻る位置が下限と上限の範囲の外です(離すと範囲の端に止まります)"
            ));
        }
        Ok((v, l, h))
    }

    /// 連動する時に連動元の値から決める動き(回る種類は角度、スライダは距離)。
    pub fn main_key(&self) -> &'static str {
        if self.kind.turns() {
            "angle"
        } else {
            "distance"
        }
    }

    /// 連動するか(連動元を選んである時)。
    pub fn linked(&self) -> Option<FeatureId> {
        self.link_joint.filter(|_| self.drive_by == DriveBy::Link)
    }

    /// 連動元と比(この値 = 連動元の値 × 比、どちらも内部の単位)。連動しなければ `None`。
    fn lead(&self, ctx: &EvalCtx<'_>) -> Result<Option<anim::Lead>, EvalError> {
        let Some(joint) = self.linked() else {
            return Ok(None);
        };
        if !matches!(self.kind, JointKind::Revolute | JointKind::Slider) {
            return Err(EvalError::InvalidData(
                "連動できるのは回転かスライダのジョイントだけです(値の決め方を「値で動かす」に戻してください)".into(),
            ));
        }
        let leader = self.leader(ctx, joint)?;
        let or = |e: &str, d: &str| {
            if e.trim().is_empty() {
                d.to_string()
            } else {
                e.to_string()
            }
        };
        let read = |slide: bool, e: &str, what: &str| {
            if slide {
                length(ctx, e, what)
            } else {
                angle(ctx, e, what)
            }
        };
        let lead_default = if leader.slide { "10 mm" } else { "360 deg" };
        let follow_default = if self.kind.turns() {
            "360 deg"
        } else {
            "10 mm"
        };
        let a = read(
            leader.slide,
            &or(&self.link_lead, lead_default),
            "連動元の動き",
        )?;
        let b = read(
            !self.kind.turns(),
            &or(&self.link_follow, follow_default),
            "連動する動き",
        )?;
        if a.abs() < 1e-12 {
            return Err(EvalError::InvalidData("連動元の動きが 0 です".into()));
        }
        let ratio = if self.link_flip { -b / a } else { b / a };
        Ok(Some(anim::Lead { joint, ratio }))
    }

    /// 連動元のジョイントの今の様子。
    fn leader(&self, ctx: &EvalCtx<'_>, joint: FeatureId) -> Result<Drive, EvalError> {
        match ctx.output(joint, DRIVE_SLOT)? {
            Value::Custom { type_key, data } if type_key == anim::DRIVE_TYPE => {
                serde_json::from_value(data.clone())
                    .map_err(|e| EvalError::InvalidData(e.to_string()))
            }
            _ => Err(EvalError::InvalidData(format!(
                "連動元 {joint} は回転かスライダのジョイントではありません"
            ))),
        }
    }

    fn leader_value(&self, ctx: &EvalCtx<'_>, l: anim::Lead) -> Result<f64, EvalError> {
        Ok(self.leader(ctx, l.joint)?.value)
    }

    /// 土台の今の置き場所(前のジョイントの動きを順に掛けた物)。
    fn base_pose(&self, ctx: &EvalCtx<'_>) -> Result<Matrix4, EvalError> {
        let mut pose = k::IDENTITY;
        for j in &self.follows {
            let m = transform_of(ctx.output(*j, "transform")?)
                .ok_or_else(|| EvalError::InvalidData(format!("{j} はジョイントではありません")))?;
            pose = mul(&m, &pose);
        }
        Ok(pose)
    }

    /// 作った時の部品 2 の原点(土台を動かす前)。
    pub fn frame(&self) -> Frame {
        let (z, x, _) = basis(self.axis, self.x_dir);
        Frame {
            origin: self.origin,
            z,
            x,
        }
    }

    /// 合わせるジョイントで部品 1 を動かす行列(土台を動かす前の世界座標)。合わせない時は動かさない。
    pub fn placement(&self, ctx: &EvalCtx<'_>) -> Result<Matrix4, EvalError> {
        let Some(s) = self.snap.as_ref().filter(|_| self.how == How::Snap) else {
            return Ok(k::IDENTITY);
        };
        let a = angle(ctx, &self.align_angle, "合わせる角度")?;
        let off = [
            length(ctx, &self.offset_x, "X のずらし")?,
            length(ctx, &self.offset_y, "Y のずらし")?,
            length(ctx, &self.offset_z, "Z のずらし")?,
        ];
        // 部品 1 の原点の座標で: 向かい合わせなら X のまわりに裏返し、Z のまわりに回し、ずらす
        let mut local = mul(
            &k::translation(off),
            &k::rotation([0.0; 3], [0.0, 0.0, 1.0], a),
        );
        if !self.align_flip {
            local = mul(
                &local,
                &k::rotation([0.0; 3], [1.0, 0.0, 0.0], std::f64::consts::PI),
            );
        }
        Ok(mul(
            &self.frame().matrix(),
            &mul(&local, &rigid_inverse(&s.from.matrix())),
        ))
    }

    /// このジョイントの動き(世界座標の行列)と警告。
    pub fn motion(&self, ctx: &EvalCtx<'_>) -> Result<(Matrix4, Vec<String>), EvalError> {
        self.motion_and_drive(ctx).map(|(m, w, _)| (m, w))
    }

    /// 動きと警告と、今の様子(アニメーションが値を動かすのに使う。固定は `None`)。回る種類は
    /// 角度、スライダは距離を動かす。
    pub fn motion_and_drive(
        &self,
        ctx: &EvalCtx<'_>,
    ) -> Result<(Matrix4, Vec<String>, Option<Drive>), EvalError> {
        let mut warnings = Vec::new();
        let snapped = self.how == How::Snap && self.snap.is_some();
        if self.kind == JointKind::Rigid && !snapped {
            return Ok((k::IDENTITY, warnings, None));
        }
        let pose = self.base_pose(ctx)?;
        // 合わせる動きは土台を動かす前の座標で決めてあり、部品 1 は土台と一緒に動いてここへ来る
        let place = mul(&pose, &mul(&self.placement(ctx)?, &rigid_inverse(&pose)));
        if self.kind == JointKind::Rigid {
            return Ok((place, warnings, None));
        }
        let origin = k::transform_point(&pose, self.origin);
        let axis = apply_dir(&pose, self.axis);
        if k::dot(axis, axis) < 1e-18 {
            return Err(EvalError::InvalidData("軸の向きが 0 です".into()));
        }
        let (mut z, x, _) = basis(axis, self.x_dir.map(|x| apply_dir(&pose, x)));
        if self.flip {
            z = k::scale(z, -1.0);
        }
        let y = k::cross(z, x);
        let kind = self.kind;
        let turn = kind
            .turns()
            .then(|| self.value_of(ctx, "angle", &mut warnings))
            .transpose()?;
        let slide = kind
            .slides()
            .then(|| self.value_of(ctx, "distance", &mut warnings))
            .transpose()?;
        let mut t = slide.map_or([0.0; 3], |(d, ..)| k::scale(z, d));
        if kind.slides_x() {
            t = k::add(
                t,
                k::scale(x, self.value_of(ctx, "slide_x", &mut warnings)?.0),
            );
        }
        if kind.slides_y() {
            t = k::add(
                t,
                k::scale(y, self.value_of(ctx, "slide_y", &mut warnings)?.0),
            );
        }
        let mut rot = turn.map_or(k::IDENTITY, |(a, ..)| k::rotation(origin, z, a));
        if kind.tilts() {
            let tilt = self.value_of(ctx, "tilt", &mut warnings)?.0;
            let twist = self.value_of(ctx, "twist", &mut warnings)?.0;
            rot = mul(
                &rot,
                &mul(
                    &k::rotation(origin, x, tilt),
                    &k::rotation(origin, z, twist),
                ),
            );
        }
        let m = mul(&mul(&k::translation(t), &rot), &place);
        // 今の位置の軸: 回る軸は動いた分だけずれた原点を通る(Z のまわりに回すのは、動かした後に
        // 掛けても同じ)。スライダは軸の向きに動くだけ
        let (slide, (value, min, max)) = match (turn, slide) {
            (Some(v), _) => (false, v),
            (None, Some(v)) => (true, v),
            (None, None) => unreachable!("固定は先に返した"),
        };
        let drive = Drive {
            slide,
            origin: k::add(origin, t),
            axis: z,
            value,
            min,
            max,
            bodies: self.bodies.clone(),
            dofs: self.dofs(origin, (z, x, y)),
            lead: self.lead(ctx)?,
        };
        Ok((m, warnings, Some(drive)))
    }
}

impl Joint {
    /// 動きが 2 つ以上の種類の、動き全部(点に掛ける順: ひねり・傾き・角度の回転、それから移動。
    /// [`Joint::motion_and_drive`] の行列と同じ順)。`origin` と `(z, x, y)` は値が全部 0 の時の原点と向き。
    fn dofs(&self, origin: P3, (z, x, y): (P3, P3, P3)) -> Vec<kmb_core::Dof> {
        let kind = self.kind;
        if matches!(
            kind,
            JointKind::Rigid | JointKind::Revolute | JointKind::Slider
        ) {
            return Vec::new();
        }
        let dof = |field: &str, slide: bool, axis: P3| kmb_core::Dof {
            field: field.into(),
            slide,
            origin,
            axis,
        };
        let mut v = Vec::new();
        if kind.tilts() {
            v.push(dof("twist", false, z));
            v.push(dof("tilt", false, x));
        }
        if kind.turns() {
            v.push(dof("angle", false, z));
        }
        if kind.slides() {
            v.push(dof("distance", true, z));
        }
        if kind.slides_x() {
            v.push(dof("slide_x", true, x));
        }
        if kind.slides_y() {
            v.push(dof("slide_y", true, y));
        }
        v
    }
}

impl Feature for Joint {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v: Vec<InputRef> = self.bodies.iter().map(|b| output(*b, "body")).collect();
        if self.kind != JointKind::Rigid || self.how == How::Snap {
            v.extend(self.follows.iter().map(|j| output(*j, "transform")));
        }
        if let Some(j) = self.linked().filter(|_| self.kind != JointKind::Rigid) {
            v.push(output(j, DRIVE_SLOT));
        }
        for i in expr_refs(self)? {
            if !v.contains(&i) {
                v.push(i);
            }
        }
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        if self.bodies.is_empty() {
            return Err(EvalError::InvalidData("動かすボディがありません".into()));
        }
        let (m, warnings, drive) = self.motion_and_drive(ctx)?;
        let mut out = Outputs::new();
        if let Some(d) = drive {
            out.insert(DRIVE_SLOT.into(), d.to_value());
        }
        moved_outputs(ctx, &self.bodies, &m, &mut out)?;
        add_warnings(&mut out, warnings);
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

/// 動きの 1 つの欄(値のスライダ・下限・上限・戻る位置)。`kinds` はその欄を使う種類(`|` 区切り)。
fn dof_fields(
    d: Descriptor,
    key: &str,
    label: &str,
    kinds: &str,
    (lo, hi, unit): (f64, f64, &str),
    help: &str,
) -> Descriptor {
    let only = |f: Field| f.only("kind", kinds, "revolute");
    // 値・下限・上限・戻る位置は同じ量(スライダの単位が deg なら角度、mm なら長さ)
    let typed = |f: Field| if unit == "deg" { f.angle() } else { f.length() };
    let (min, max, rest) = (
        format!("{key}_min"),
        format!("{key}_max"),
        format!("{key}_rest"),
    );
    // 連動する回転・スライダは、角度・距離を連動元から決める(欄は隠す)
    let own = |f: Field| {
        if matches!(key, "angle" | "distance") {
            f.and_only("drive_by", "value", "value")
        } else {
            f
        }
    };
    d.field(
        own(only(typed(Field::expr(key, label))))
            .slider(&min, &max, lo, hi, unit)
            .rest(&rest)
            .help(&format!(
                "{help}。スライダを引くと動いて見える。範囲の外は入れられない"
            )),
    )
    .field(only(typed(Field::opt_expr(&min, &format!("{label}の下限")))).help("空なら決めない"))
    .field(only(typed(Field::opt_expr(&max, &format!("{label}の上限")))).help("空なら決めない"))
    .field(
        only(typed(Field::opt_expr(&rest, &format!("{label}の戻る位置"))))
            .help("空なら戻さない。部品やスライダを引いて離すとこの値に戻る"),
    )
}

pub fn descriptor() -> Descriptor {
    let snap = |f: Field| f.only("how", "snap", "as_built");
    let d = Descriptor::new(JOINT, category::SOLID, "ジョイント")
        .menu("組立")
        .icon("joint")
        .hint("2 つの部品をつなぐ(固定・回転・スライダ・円柱・ピンスロット・平面・ボール)。値を変えると動く")
        .aliases("じょいんと ジョイント joint ちょうつがい 蝶番 ひんじ hinge すらいだ スライダ かいてん 回転 こてい 固定 えんちゅう 円柱 ぴんすろっと ピンスロット へいめん 平面 ぼーる ボール 球 くみたて 組立")
        .field(Field::choice("kind", "種類", KINDS).help(
            "固定は一緒に動くだけ。回転は軸のまわりに回り、スライダは軸の向きに動く。円柱は回って軸の向きにも動き、ピンスロットは回って X の向きに動く。平面は X・Y の向きに動いて軸のまわりに回り、ボールはどの向きにも回る",
        ))
        .field(
            snap(Field::expr("align_angle", "合わせる角度").angle())
                .help("部品 1 を、合わせた原点の Z のまわりに回す"),
        )
        .field(snap(Field::expr("offset_x", "X のずらし").length()).help("部品 2 の原点の X の向きにずらす"))
        .field(snap(Field::expr("offset_y", "Y のずらし").length()).help("部品 2 の原点の Y の向きにずらす"))
        .field(
            snap(Field::expr("offset_z", "Z のずらし").length())
                .help("部品 2 の原点の Z の向きにずらす(面と面の間を空ける)"),
        )
        .field(
            snap(Field::bool("align_flip", "合わせる向きを反転"))
                .help("部品 1 を裏返して合わせる(既定は面と面が向かい合う向き)"),
        );
    let d = dof_fields(
        d,
        "angle",
        "角度",
        "revolute|cylindrical|pin_slot|planar|ball",
        (-180.0, 180.0, "deg"),
        "作った位置からの、軸(原点の Z)のまわりの角度",
    );
    let d = dof_fields(
        d,
        "distance",
        "距離",
        "slider|cylindrical",
        (-200.0, 200.0, "mm"),
        "作った位置からの、軸の向きの距離",
    );
    let d = dof_fields(
        d,
        "slide_x",
        "X 距離",
        "pin_slot|planar",
        (-200.0, 200.0, "mm"),
        "作った位置からの、原点の X の向きの距離",
    );
    let d = dof_fields(
        d,
        "slide_y",
        "Y 距離",
        "planar",
        (-200.0, 200.0, "mm"),
        "作った位置からの、原点の Y の向きの距離",
    );
    let d = dof_fields(
        d,
        "tilt",
        "傾き",
        "ball",
        (-180.0, 180.0, "deg"),
        "原点の X のまわりに傾ける角度",
    );
    let d = dof_fields(
        d,
        "twist",
        "ひねり",
        "ball",
        (-180.0, 180.0, "deg"),
        "傾けた軸のまわりにひねる角度",
    );
    let link = |f: Field| {
        f.only("drive_by", "link", "value")
            .and_only("kind", "revolute|slider", "revolute")
    };
    d.field(
        Field::bool("flip", "動く向きを反転")
            .unless("kind", "rigid")
            .help("回る向き・動く向きを逆にする"),
    )
    .field(
        Field::choice("drive_by", "値の決め方", DRIVE_BYS)
            .unless("kind", "rigid")
            .help("ほかのジョイントに連動すると、回る種類は角度、スライダは距離が、連動元の値から決まる"),
    )
    .field(link(Field::opt_output("link_joint", "連動元のジョイント", DRIVE_SLOT)).help("タイムラインで前にある、回転かスライダのジョイント"))
    .field(link(Field::opt_expr("link_lead", "連動元の動き").formula()).help("連動元がこれだけ動く時に(回転なら角度、スライダなら距離。空なら 360 deg か 10 mm)"))
    .field(link(Field::opt_expr("link_follow", "この動き").formula()).help("このジョイントがこれだけ動く(空なら 360 deg か 10 mm)"))
    .field(link(Field::bool("link_flip", "連動する向きを反転")).help("連動元と逆の向きに動かす"))
    .no_raw_editor()
}

pub fn register(reg: &mut Registry) {
    reg.register_described(descriptor(), |d| Ok(Box::new(parse::<Joint>(d)?)));
}

/// 軸を決める候補 1 つ(手順の候補の値)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Origin {
    /// 動かす部品をクリックした面のボディ(最後の機能)
    pub feature: FeatureId,
    pub point: P3,
    pub axis: P3,
}

/// 新しいジョイントのデータの既定(回転。円の中心でも、まっすぐな辺(蝶番)でも)。
pub fn new_joint(moving: Part, base: Option<Part>, bodies: Vec<FeatureId>, o: &Origin) -> Joint {
    Joint {
        kind: JointKind::Revolute,
        moving,
        base,
        bodies,
        origin: o.point,
        axis: k::normalize(o.axis),
        x_dir: None,
        how: How::AsBuilt,
        snap: None,
        align_angle: zero_deg(),
        offset_x: zero_mm(),
        offset_y: zero_mm(),
        offset_z: zero_mm(),
        align_flip: false,
        follows: Vec::new(),
        flip: false,
        angle: zero_deg(),
        angle_min: String::new(),
        angle_max: String::new(),
        angle_rest: String::new(),
        distance: zero_mm(),
        distance_min: String::new(),
        distance_max: String::new(),
        distance_rest: String::new(),
        slide_x: zero_mm(),
        slide_x_min: String::new(),
        slide_x_max: String::new(),
        slide_x_rest: String::new(),
        slide_y: zero_mm(),
        slide_y_min: String::new(),
        slide_y_max: String::new(),
        slide_y_rest: String::new(),
        tilt: zero_deg(),
        tilt_min: String::new(),
        tilt_max: String::new(),
        tilt_rest: String::new(),
        twist: zero_deg(),
        twist_min: String::new(),
        twist_max: String::new(),
        twist_rest: String::new(),
        drive_by: DriveBy::Value,
        link_joint: None,
        link_lead: String::new(),
        link_follow: String::new(),
        link_flip: false,
    }
}

/// 合わせるジョイントの既定(固定)。部品 1 の原点 `from` を
/// 部品 2 の原点 `to`(土台を動かす前の世界座標)に合わせる。
pub fn new_snap_joint(
    moving: Part,
    base: Option<Part>,
    bodies: Vec<FeatureId>,
    (from, from_label): (Frame, String),
    (to, to_label): (Frame, String),
) -> Joint {
    let o = Origin {
        feature: FeatureId(0),
        point: to.origin,
        axis: to.z,
    };
    Joint {
        kind: JointKind::Rigid,
        x_dir: Some(to.axes().1),
        how: How::Snap,
        snap: Some(Snap {
            from,
            from_label,
            to_label,
        }),
        ..new_joint(moving, base, bodies, &o)
    }
}

/// 部品 1 の原点 `from` が部品 2 の原点 `to`(どちらも今の位置)に既に重なり、Z が同じ線の上に
/// あれば(載せてある・接している面どうし)、今の向きのまま動かさない合わせる角度(度)と反転
/// (そろえるために回して動かさない。10-20 a5)。重なっていなければ `None`。
pub fn touching_alignment(from: &Frame, to: &Frame) -> Option<(f64, bool)> {
    let (fz, fx, _) = from.axes();
    let (tz, tx, ty) = to.axes();
    let gap = k::sub(from.origin, to.origin);
    let c = k::dot(fz, tz);
    if k::dot(gap, gap).sqrt() > 1e-6 || c.abs() < 1.0 - 1e-9 {
        return None;
    }
    // 部品 2 の原点の座標で、部品 1 の X は (cos a, sin a, 0)(向かい合わせでも、X のまわりの
    // 裏返しは X を変えない)
    let a = k::dot(fx, ty).atan2(k::dot(fx, tx)).to_degrees();
    Some(((a * 1e6).round() / 1e6 + 0.0, c > 0.0))
}

/// 面 `at` を 3D のビューで引いた時に動かすジョイント(ジョイントの自由度の中で部品を
/// 引いて動かす操作)。そのボディを動かす、固定でないジョイントのうち、動かすボディがいちばん
/// 少ない物(部品のすぐ外のジョイント。腕の先の手を引けば手首が回る)。回転は角度、スライダは
/// 距離を動かす(軸は今の位置)。動きが 2 つ以上の種類(円柱・ピンスロット・平面・ボール)は、動き
/// 全部でカーソルに付いてくる([`kmb_core::DragMotion::Follow`]。平面は回さない)。
pub fn drag(cx: &kmb_core::Context<'_>, at: &kmb_core::FacePick) -> Option<kmb_core::Drag> {
    let root = kmb_core::body_root(cx.data, cx.registry, at.feature);
    let (id, name, j, d) = cx
        .data
        .features
        .iter()
        .filter(|e| e.type_id == JOINT && !e.suppressed)
        .filter_map(|e| {
            Some((
                e.id,
                e.name.clone(),
                serde_json::from_value::<Joint>(e.data.clone()).ok()?,
                kmb_interchange::animation::drive_of(cx.report, e.id)?,
            ))
        })
        .filter(|(id, _, j, d)| {
            // 連動するジョイントは連動元で動く(引くのは連動元の部品)。剛性グループがジョイントの
            // 動きに乗せた部品を引いても、そのジョイントが動く
            j.kind != JointKind::Rigid
                && j.linked().is_none()
                && anim::moved_bodies(cx.data, cx.report, *id, d)
                    .iter()
                    .any(|b| kmb_core::body_root(cx.data, cx.registry, *b) == root)
        })
        .min_by_key(|(_, _, j, _)| j.bodies.len())?;
    if !d.dofs.is_empty() {
        // 動きが 2 つ以上: 動き全部でカーソルに付いてくる。平面は回さない(X・Y だけでどこへでも
        // 届くので、回転も動かすと引くうちに少しずつ回ってしまう)
        let still = if j.kind == JointKind::Planar {
            vec!["angle".to_string()]
        } else {
            Vec::new()
        };
        log::debug!("{name}({:?})を引いて動かす(面 {} から)", j.kind, at.face);
        let field = d.dofs[0].field.clone();
        return Some(kmb_core::Drag {
            feature: id,
            field,
            motion: kmb_core::DragMotion::Follow,
            label: format!("{name} を動かす"),
            dofs: d.dofs,
            still,
        });
    }
    let (field, motion, verb) = if d.slide {
        (
            "distance",
            kmb_core::DragMotion::Slide { axis: d.axis },
            "動かす",
        )
    } else {
        (
            "angle",
            kmb_core::DragMotion::Rotate {
                origin: d.origin,
                axis: d.axis,
            },
            "回す",
        )
    };
    log::debug!("{name}({:?})を引いて{verb}(面 {} から)", j.kind, at.face);
    Some(kmb_core::Drag {
        feature: id,
        field: field.into(),
        motion,
        label: format!("{name} を{verb}"),
        dofs: Vec::new(),
        still: Vec::new(),
    })
}
