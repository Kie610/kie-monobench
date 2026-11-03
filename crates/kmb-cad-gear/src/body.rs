//! 歯車のボディ(`cad.gear_body@1`)。歯車の輪郭の機能 1 つを読み、輪郭を歯の幅だけ押し出して
//! ボディ(`body`)を作り、輪郭の諸元に歯の幅を足した諸元(`gear`)を出す。押し出した面の名前は
//! 輪郭の区間の名前を受け継ぐ(`f<この機能>:side:t3:r0` など。押し出しと同じ決まり)。
//!
//! 腕(ウェブ)の厚さとボスの長さの欄も持つ。輪郭が歯の輪・腕・ボスの領域に分かれていない間(円板)は
//! 使わず、空でない時は注意を出す。

use kmb_cad::{
    Exprs, Operation, Recipe, expr_refs, finish, keep_target, length, operation_fields, output,
    rename, target_inputs,
};
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, InputRef, Outputs, Payload, Profile, Registry,
};
use kmb_interchange::{GEAR_OUTPUT, GearSpec};
use serde::{Deserialize, Serialize};

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

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn width_default() -> String {
    "10 mm".into()
}

/// 歯車のボディの機能のデータ。例: `{"gear": 3, "width": "8 mm"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GearBody {
    /// 歯車の輪郭の機能
    pub gear: FeatureId,
    /// 歯の幅(歯の輪の厚さ)
    #[serde(default = "width_default")]
    pub width: String,
    /// 腕(ウェブ)の厚さ(空なら歯の幅)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<String>,
    /// ボスの長さ(空なら歯の幅)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boss: Option<String>,
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
            direction: Side::default(),
            operation: Operation::default(),
            target: None,
        }
    }
}

impl Exprs for GearBody {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut v = vec![&mut self.width];
        v.extend([&mut self.web, &mut self.boss].into_iter().flatten());
        v
    }
}

impl Feature for GearBody {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.gear, "profile"), output(self.gear, GEAR_OUTPUT)];
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
        let profile =
            Profile::from_value(ctx.output(self.gear, "profile")?).ok_or_else(not_gear)?;
        let mut spec =
            GearSpec::from_value(ctx.output(self.gear, GEAR_OUTPUT)?).ok_or_else(not_gear)?;
        let width = length(ctx, &self.width, "歯の幅")?;
        if width <= 0.0 {
            return Err(EvalError::InvalidData(
                "歯の幅は 0 より大きくしてください".into(),
            ));
        }
        let mut warnings = Vec::new();
        for (src, what) in [(&self.web, "腕の厚さ"), (&self.boss, "ボスの長さ")] {
            if let Some(s) = src.as_ref().filter(|s| !s.trim().is_empty()) {
                let v = length(ctx, s, what)?;
                if (v - width).abs() > 1e-9 {
                    warnings.push(format!(
                        "{what}は、輪郭が円板の間は使いません(歯の幅で作りました)"
                    ));
                }
            }
        }
        let (from, to) = match self.direction {
            Side::Normal => (0.0, width),
            Side::Reverse => (-width, 0.0),
            Side::Symmetric => (-width / 2.0, width / 2.0),
        };
        let recipe = Recipe::Extrude {
            operation: self.operation,
            profile,
            from,
            to,
            through: false,
            stop: None,
            taper: 0.0,
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
            .icon("gear")
            .hint("歯車の輪郭を歯の幅だけ押し出して歯車の立体にする")
            .aliases("はぐるま gear ぎあ ぼでぃ")
            .field(Field::output("gear", "歯車の輪郭", GEAR_OUTPUT))
            .field(Field::expr("width", "歯の幅").length())
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
