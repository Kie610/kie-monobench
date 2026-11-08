//! 歯車のボディ(`cad.gear_body@1`)。歯車の輪郭の機能 1 つを読み、輪郭を歯の幅だけ押し出して
//! ボディ(`body`)を作り、輪郭の諸元に歯の幅を足した諸元(`gear`)を出す。押し出した面の名前は
//! 輪郭の区間の名前を受け継ぐ(`f<この機能>:side:t3:r0` など。押し出しと同じ決まり)。
//!
//! 歯の輪・腕・ボスをそれぞれの厚さに押し出す。腕とボスは歯の幅の中央か片側へそろえる。

use kmb_cad::{
    Exprs, Operation, Recipe, expr_refs, finish, keep_target, length, operation_fields, output,
    rename, target_inputs,
};
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, InputRef, Outputs, Payload, Profile, Registry,
};
use kmb_interchange::{GEAR_OUTPUT, GearSpec};
use serde::{Deserialize, Serialize};

use crate::hub::{PARTS_OUTPUT, Parts};

pub const GEAR_BODY: &str = "cad.gear_body@1";

pub(crate) const HELICAL_KIND_UNSUPPORTED: &str = "ねじれ角・ねじれの向き・やまばは、外歯か内歯の歯車だけで使えます。 / Helix angle, helix hand, and herringbone are available only for external and internal gears.";
pub(crate) const HELICAL_THICKNESS_UNSUPPORTED: &str = "はすばとやまばでは、腕とボスの厚さを歯の幅にそろえてください。異なる厚さには対応していません。 / Helical and herringbone gears require web and boss thicknesses equal to the face width. Different thicknesses are not supported.";
pub(crate) const HELICAL_GROOVE_HOLES_UNSUPPORTED: &str = "軽くする穴やキー溝があるやまばは、中央の溝の幅を 0 mm にしてください。この組み合わせには対応していません。 / Herringbone gears with lightening holes or a keyway require a zero center groove width. This combination is not supported.";

/// 歯の幅をどちらへ取るか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// 輪郭の平面の法線の向きへ
    #[default]
    Normal,
    /// 法線の逆へ
    Reverse,
    /// 輪郭の平面を真ん中に両側へ半分ずつ
    Symmetric,
}

/// 腕とボスを歯の幅へそろえる位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Centered,
    Start,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn width_default() -> String {
    String::new()
}

/// 歯車のボディの機能のデータ。例: `{"gear": 3, "width": "8 mm"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GearBody {
    /// 歯車の輪郭の機能
    pub gear: FeatureId,
    /// 歯の幅。空欄なら鎖車は鎖幅から決め、ほかの歯車は 10 mm。
    #[serde(default = "width_default", skip_serializing_if = "String::is_empty")]
    pub width: String,
    /// 腕(ウェブ)の厚さ(空なら歯の幅)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<String>,
    /// ボスの長さ(空なら歯の幅)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boss: Option<String>,
    /// 円板全体を左右で逆向きにねじる
    #[serde(default, skip_serializing_if = "is_default")]
    pub herringbone: bool,
    /// 中央の溝の幅(空か 0 なら溝なし)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groove: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub alignment: Alignment,
    #[serde(default, skip_serializing_if = "is_default")]
    pub direction: Side,
    #[serde(default, skip_serializing_if = "is_default")]
    pub operation: Operation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<FeatureId>,
}

impl GearBody {
    pub fn new(gear: FeatureId) -> Self {
        GearBody {
            gear,
            width: width_default(),
            web: None,
            boss: None,
            herringbone: false,
            groove: None,
            alignment: Alignment::default(),
            direction: Side::default(),
            operation: Operation::default(),
            target: None,
        }
    }
}

impl GearBody {
    fn part_span(&self, from: f64, to: f64, thickness: f64) -> (f64, f64) {
        match self.alignment {
            Alignment::Centered => {
                let middle = (from + to) / 2.0;
                (middle - thickness / 2.0, middle + thickness / 2.0)
            }
            Alignment::Start if self.direction == Side::Reverse => (to - thickness, to),
            Alignment::Start => (from, from + thickness),
        }
    }
}

impl Exprs for GearBody {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut v = Vec::new();
        if !self.width.trim().is_empty() {
            v.push(&mut self.width);
        }
        v.extend(
            [&mut self.web, &mut self.boss, &mut self.groove]
                .into_iter()
                .flatten(),
        );
        v
    }
}

impl Feature for GearBody {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![
            output(self.gear, "profile"),
            output(self.gear, GEAR_OUTPUT),
            output(self.gear, PARTS_OUTPUT),
        ];
        if self.width.trim().is_empty() {
            v.push(output(self.gear, crate::DEFAULT_WIDTH_OUTPUT));
        }
        v.extend(target_inputs(self.operation, self.target)?);
        for i in expr_refs(self)? {
            if !v.contains(&i) {
                v.push(i);
            }
        }
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        keep_target(self.operation, self.target)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let not_gear =
            || EvalError::InvalidData(format!("{} は歯車の輪郭ではありません", self.gear));
        let mut profile =
            Profile::from_value(ctx.output(self.gear, "profile")?).ok_or_else(not_gear)?;
        let mut spec =
            GearSpec::from_value(ctx.output(self.gear, GEAR_OUTPUT)?).ok_or_else(not_gear)?;
        let specified_width = if self.width.trim().is_empty() {
            let kmb_core::Value::Scalar(q) = ctx.output(self.gear, crate::DEFAULT_WIDTH_OUTPUT)?
            else {
                return Err(EvalError::InvalidData("歯の幅の既定値がありません".into()));
            };
            if q.dim != kmb_core::Dim::LENGTH {
                return Err(EvalError::InvalidData(
                    "歯の幅の既定値が長さではありません".into(),
                ));
            }
            q.value
        } else {
            length(ctx, &self.width, "歯の幅")?
        };
        if !specified_width.is_finite() || specified_width <= 0.0 {
            return Err(EvalError::InvalidData(
                "歯の幅は 0 より大きくしてください".into(),
            ));
        }
        let width = if spec.kind.is_pulley() {
            let values: Vec<_> = profile
                .regions
                .iter()
                .flat_map(|r| &r.outer.edges)
                .flat_map(|e| [e.from_point()[1], e.to_point()[1]])
                .collect();
            let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let width = hi - lo;
            if !width.is_finite() || width <= 0.0 {
                return Err(EvalError::InvalidData(
                    "ベルト車かロープ車の軸断面に幅がありません".into(),
                ));
            }
            width
        } else {
            specified_width
        };
        let parts = Parts::from_value(ctx.output(self.gear, PARTS_OUTPUT)?).ok_or_else(not_gear)?;
        let mut warnings = Vec::new();
        let (web, boss) = if spec.kind.is_pulley() {
            if (specified_width - width).abs() > 1e-9 {
                warnings.push(format!(
                    "ベルト車とロープ車の歯の幅は使いません。輪郭の幅 {width:.3} mm で作りました"
                ));
            }
            for (src, what) in [(&self.web, "腕の厚さ"), (&self.boss, "ボスの長さ")] {
                if let Some(s) = src.as_ref().filter(|s| !s.trim().is_empty()) {
                    let v = length(ctx, s, what)?;
                    if (v - width).abs() > 1e-9 {
                        warnings.push(format!(
                            "{what}はベルト車とロープ車では使いません(輪郭の幅で作りました)"
                        ));
                    }
                }
            }
            (width, width)
        } else {
            let thickness = |src: &Option<String>, what: &str| {
                let value = src
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map_or(Ok(width), |s| length(ctx, s, what))?;
                if !value.is_finite() || value <= 0.0 {
                    return Err(EvalError::InvalidData(format!(
                        "{what}は 0 より大きくしてください"
                    )));
                }
                Ok(value)
            };
            let mut web = thickness(&self.web, "腕の厚さ")?;
            let mut boss = thickness(&self.boss, "ボスの長さ")?;
            if spec.helix_angle != 0.0
                && ((web - width).abs() > 1e-9 || (boss - width).abs() > 1e-9)
            {
                return Err(EvalError::Other(HELICAL_THICKNESS_UNSUPPORTED.into()));
            }
            for (present, value, what) in [
                (parts.web.is_some(), &mut web, "腕の厚さ"),
                (parts.boss.is_some(), &mut boss, "ボスの長さ"),
            ] {
                if !present && (*value - width).abs() > 1e-9 {
                    warnings.push(format!("{what}は、その領域がないため使いません"));
                    *value = width;
                }
            }
            (web, boss)
        };
        let (from, to) = match self.direction {
            Side::Normal => (0.0, width),
            Side::Reverse => (-width, 0.0),
            Side::Symmetric => (-width / 2.0, width / 2.0),
        };
        let gap = self
            .groove
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map_or(Ok(0.0), |s| length(ctx, s, "中央の溝の幅"))?;
        if !gap.is_finite() || gap < 0.0 || gap >= width {
            return Err(EvalError::InvalidData(
                "中央の溝の幅は 0 以上、歯の幅より小さくしてください".into(),
            ));
        }
        if (self.herringbone || gap != 0.0)
            && !matches!(
                spec.kind,
                kmb_interchange::GearKind::External | kmb_interchange::GearKind::Internal
            )
        {
            return Err(EvalError::Other(HELICAL_KIND_UNSUPPORTED.into()));
        }
        if (self.herringbone || gap != 0.0) && spec.helix_angle == 0.0 {
            return Err(EvalError::InvalidData(
                "やまばには 0 deg より大きいねじれ角を指定してください".into(),
            ));
        }
        let extrude = |profile, from, to| Recipe::Extrude {
            operation: self.operation,
            profile,
            from,
            to,
            through: false,
            stop: None,
            taper: 0.0,
        };
        let recipe = if spec.helix_angle != 0.0 {
            if self.herringbone && gap > 0.0 && !crate::helical::concentric_holes(&profile, &spec) {
                return Err(EvalError::Other(HELICAL_GROOVE_HOLES_UNSUPPORTED.into()));
            }
            crate::helical::recipe(
                &profile,
                &spec,
                from,
                to,
                self.herringbone,
                gap,
                self.operation,
            )
            .map_err(EvalError::InvalidData)?
        } else if spec.kind.is_pulley() {
            let direction = spec.axis.direction;
            profile.plane.origin = [0, 1, 2].map(|i| profile.plane.origin[i] + direction[i] * from);
            Recipe::Revolve {
                operation: self.operation,
                profile,
                origin: spec.axis.origin,
                dir: direction,
                angle: std::f64::consts::TAU,
            }
        } else if (web - width).abs() < 1e-9 && (boss - width).abs() < 1e-9 {
            // 同じ厚さなら分割する前の輪郭を一度押し出し、余分な面を作らない。
            extrude(profile, from, to)
        } else {
            if self.operation == Operation::Intersect {
                return Err(EvalError::InvalidData("厚さが異なる歯車の交差には対応していません。歯の幅・腕の厚さ・ボスの長さをそろえるか、新しいボディとして作ってください".into()));
            }
            let mut recipes = vec![extrude(parts.rim, from, to)];
            for (part, size) in [(parts.web, web), (parts.boss, boss)] {
                if let Some(part) = part {
                    let (a, b) = self.part_span(from, to, size);
                    recipes.push(extrude(part, a, b));
                }
            }
            Recipe::Many { parts: recipes }
        };
        let mut out = finish(ctx, recipe, self.target)?;
        // 諸元の軸の点は歯の幅の始まりの側に置く
        let n = spec.axis.direction;
        spec.axis.origin = [0, 1, 2].map(|i| spec.axis.origin[i] + n[i] * from);
        spec.face_width = Some(width);
        if self.herringbone {
            spec.hand = kmb_interchange::gear::Hand::Both;
        }
        out.insert(GEAR_OUTPUT.to_string(), spec.to_value());
        kmb_core::add_warnings(&mut out, warnings);
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

/// 歯車のボディの種類を登録する。ツールバーには出さない(操作「歯車」が輪郭と一緒に足す)。
pub fn register(reg: &mut Registry) {
    use kmb_core::Field;
    reg.register_described(
        kmb_cad::solid(GEAR_BODY, "歯車のボディ")
            .icon("gear")
            .hint("歯車の輪郭を歯の幅だけ押し出すかねじって立体にする。やまばは中央で左右の歯すじを合わせる")
            .aliases("はぐるま gear ぎあ ぼでぃ")
            .field(Field::output("gear", "歯車の輪郭", GEAR_OUTPUT))
            .field(
                Field::opt_expr("width", "歯の幅")
                    .length()
                    .help("空なら鎖車は鎖の内幅の 0.93 倍か 0.95 倍、ほかの歯車は 10 mm。ベルト車とロープ車は輪郭の幅を使い、値が違う時は注意を出す"),
            )
            .field(
                Field::opt_expr("web", "腕の厚さ")
                    .length()
                    .help("空なら歯の幅"),
            )
            .field(
                Field::opt_expr("boss", "ボスの長さ")
                    .length()
                    .help("空なら歯の幅"),
            )
            .field(Field::choice(
                "alignment",
                "腕とボスの位置",
                &[("centered", "中央にそろえる"), ("start", "片側にそろえる")],
            ))
            .field(Field::choice(
                "direction",
                "方向",
                &[
                    ("normal", "法線の向き"),
                    ("reverse", "逆向き"),
                    ("symmetric", "対称"),
                ],
            ))
            .field(Field::bool("herringbone", "やまば").help("はすばは腕とボスも歯の幅にそろえ、穴とキー溝を含む円板全体をねじる").only_reference_of_type("gear",crate::GEAR,"kind","external|internal","external"))
            .field(Field::opt_expr("groove", "中央の溝の幅").length().help("やまばの時に使う。0 mm なら溝なし。歯の幅より小さくする。軽くする穴やキー溝があれば 0 mm にする").only_reference_of_type("gear",crate::GEAR,"kind","external|internal","external"))
            .fields(operation_fields()),
        |d| {
            let g: GearBody = serde_json::from_value(d.clone())
                .map_err(|e| EvalError::InvalidData(e.to_string()))?;
            Ok(Box::new(g))
        },
    );
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: 歯の幅の全方向で、腕とボスの中央そろえ・片側そろえを保つ。
    #[test]
    fn all_part_spans_preserve_alignment_for_both_short_and_long_parts() {
        for (direction, alignment, from, to, short, long) in [
            (
                Side::Normal,
                Alignment::Centered,
                0.0,
                8.0,
                (2.5, 5.5),
                (-2.0, 10.0),
            ),
            (
                Side::Reverse,
                Alignment::Centered,
                -8.0,
                0.0,
                (-5.5, -2.5),
                (-10.0, 2.0),
            ),
            (
                Side::Symmetric,
                Alignment::Centered,
                -4.0,
                4.0,
                (-1.5, 1.5),
                (-6.0, 6.0),
            ),
            (
                Side::Normal,
                Alignment::Start,
                0.0,
                8.0,
                (0.0, 3.0),
                (0.0, 12.0),
            ),
            (
                Side::Reverse,
                Alignment::Start,
                -8.0,
                0.0,
                (-3.0, 0.0),
                (-12.0, 0.0),
            ),
            (
                Side::Symmetric,
                Alignment::Start,
                -4.0,
                4.0,
                (-4.0, -1.0),
                (-4.0, 8.0),
            ),
        ] {
            let body = GearBody {
                direction,
                alignment,
                ..GearBody::new(FeatureId(1))
            };
            assert_eq!(body.part_span(from, to, 3.0), short);
            assert_eq!(body.part_span(from, to, 12.0), long);
        }
    }
}
