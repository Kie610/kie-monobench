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
            alignment: Alignment::default(),
            direction: Side::default(),
            operation: Operation::default(),
            target: None,
        }
    }
}

impl Exprs for GearBody {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut v = Vec::new();
        if !self.width.trim().is_empty() {
            v.push(&mut self.width);
        }
        v.extend([&mut self.web, &mut self.boss].into_iter().flatten());
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
        let extrude = |profile, from, to| Recipe::Extrude {
            operation: self.operation,
            profile,
            from,
            to,
            through: false,
            stop: None,
            taper: 0.0,
        };
        let recipe = if spec.kind.is_pulley() {
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
            let span = |thickness: f64| match self.alignment {
                Alignment::Centered => {
                    let middle = (from + to) / 2.0;
                    (middle - thickness / 2.0, middle + thickness / 2.0)
                }
                Alignment::Start if self.direction == Side::Reverse => (to - thickness, to),
                Alignment::Start => (from, from + thickness),
            };
            let mut recipes = vec![extrude(parts.rim, from, to)];
            for (part, size) in [(parts.web, web), (parts.boss, boss)] {
                if let Some(part) = part {
                    let (a, b) = span(size);
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
            .icon("gears")
            .hint("歯車の輪郭を歯の幅だけ押し出して歯車の立体にする")
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
            .fields(operation_fields()),
        |d| {
            let g: GearBody = serde_json::from_value(d.clone())
                .map_err(|e| EvalError::InvalidData(e.to_string()))?;
            Ok(Box::new(g))
        },
    );
}
