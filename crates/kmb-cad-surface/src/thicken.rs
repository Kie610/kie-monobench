//! 面のボディを、同じボディの閉じた立体へ続ける。
use kmb_cad::*;
use kmb_core::kernel::{BodyKind, ThickenSide};
use kmb_core::{
    Action, Arg, Context, EvalCtx, EvalError, Extension, Feature, FeatureId, Field, Gesture,
    Handle, HandleChoice, InputRef, Outcome, Outputs, Place, Registry, Value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const THICKEN: &str = "solid.thicken@1";
pub const ADD_THICKEN: &str = "cad.thicken";
const HINT: &str = "面のボディに厚みを付けて、閉じたソリッドにする";
const SIDES: &[(&str, &str)] = &[("front", "表側"), ("back", "裏側"), ("both", "両側")];
fn front() -> ThickenSide {
    ThickenSide::Front
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thicken {
    pub body: FeatureId,
    pub thickness: String,
    #[serde(default = "front")]
    pub side: ThickenSide,
}
impl Exprs for Thicken {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.thickness]
    }
}
impl Feature for Thicken {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut inputs = vec![output(self.body, "body")];
        inputs.extend(expr_refs(self)?);
        Ok(inputs)
    }
    fn pass_through(&self) -> Vec<(String, InputRef)> {
        vec![("body".into(), output(self.body, "body"))]
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let thickness = length(ctx, &self.thickness, "厚み")?;
        if !thickness.is_finite() || thickness <= 0. {
            return Err(EvalError::InvalidData(
                "厚みは 0 より大きい有限の値にしてください".into(),
            ));
        }
        let source = body_of(ctx, self.body)?;
        let shape = ctx
            .kernel()
            .thicken(source, thickness, self.side, &tag(ctx))
            .map_err(kernel_err)?;
        Ok([("body".into(), Value::Shape(shape))].into())
    }
    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}
pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(THICKEN, "厚みを付ける")
            .menu("修正")
            .icon("body")
            .hint(HINT)
            .field(
                Field::pick("body", "ボディ", &[kmb_core::RefKind::Body], 1, Some(1))
                    .help("厚みの無い面のボディを選ぶ"),
            )
            .field(Field::expr("thickness", "厚み").length())
            .field(Field::choice("side", "向き", SIDES))
            .template(json!({"thickness":"1 mm", "side":"front"})),
        |d| Ok(Box::new(parse::<Thicken>(d)?)),
    );
}
fn selected_body(cx: &Context<'_>) -> Result<Option<FeatureId>, kmb_core::Problem> {
    if let Some(value) = cx.arg("body") {
        return serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| {
                kmb_core::Problem::new(
                    kmb_core::ProblemKind::Input,
                    "body はボディの機能番号で指定します",
                )
            });
    }
    Ok(cx.picked.map(|p| p.feature).or(cx
        .selected
        .filter(|id| matches!(cx.output(*id, "body"), Some(Value::Shape(_))))))
}
pub(super) struct Commands;
impl Extension for Commands {
    fn actions(&self, _: &Context<'_>) -> Vec<Action> {
        vec![
            Action::new(ADD_THICKEN, "厚みを付ける", "cad")
                .arg(Arg::feature(
                    "body",
                    "面のボディの機能番号。省くと選んだボディ",
                ))
                .arg(Arg::length("thickness", "厚み(mm)").default("1 mm"))
                .arg(Arg::choice("side", "厚みを付ける向き", SIDES).default("front"))
                .menu("修正")
                .icon("body")
                .hint(HINT)
                .at(&[Place::Menu]),
        ]
    }
    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        if id != ADD_THICKEN {
            return None;
        }
        Some((|| {
            let body = selected_body(cx)?.ok_or("面のボディを選んでください")?;
            let handle = match cx.output(body, "body") {
                Some(Value::Shape(h)) => *h,
                _ => return Err("面のボディがありません。選び直してください".into()),
            };
            if cx.kernel.body_kind(handle).map_err(|e| e.to_string())? != BodyKind::Sheet {
                return Err(
                    "ソリッドには厚みを付けられません。中を抜くならシェルを使ってください".into(),
                );
            }
            let thickness = cx.arg("thickness").cloned().unwrap_or(json!("1 mm"));
            let thickness = if let Some(v) = thickness.as_str() {
                v.to_owned()
            } else {
                format!(
                    "{} mm",
                    thickness
                        .as_f64()
                        .ok_or("厚みは長さの式で指定してください")?
                )
            };
            let side = cx.arg_str("side").unwrap_or("front");
            let data: Thicken =
                serde_json::from_value(json!({"body":body,"thickness":thickness,"side":side}))
                    .map_err(|e| e.to_string())?;
            Ok(Outcome::insert(
                "厚みを付ける",
                THICKEN,
                format!("厚みを付ける{}", cx.count_of(THICKEN) + 1),
                json!(data),
            ))
        })())
    }
    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if id != ADD_THICKEN || selected_body(cx).ok().flatten().is_some() {
            return None;
        }
        let choices = cx
            .visible_bodies()
            .into_iter()
            .filter(|(_, h)| cx.kernel.body_kind(*h).ok() == Some(BodyKind::Sheet))
            .map(|(feature, h)| HandleChoice {
                value: json!(feature),
                label: cx
                    .data
                    .feature(feature)
                    .map_or_else(|| feature.to_string(), |f| f.name.clone()),
                outline: cx
                    .kernel
                    .curve_edges(h)
                    .unwrap_or_default()
                    .iter()
                    .map(|e| e.polyline(64))
                    .collect(),
                ..Default::default()
            })
            .collect();
        Some(vec![Handle {
            choices,
            ..Handle::new(
                "body",
                "面のボディをクリックし、厚みは欄で決める",
                Gesture::Choice,
            )
        }])
    }
}
