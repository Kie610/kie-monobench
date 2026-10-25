//! ボディのブーリアン(`solid.boolean@1`)。対象 1 つに、道具をいくつでも順に当てる。
use kmb_cad::*;
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, RefKind, Registry, Value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const BOOLEAN: &str = "solid.boolean@1";

/// ボディの演算。例: `{"target": 2, "tool": 5, "operation": "cut"}`。道具が 2 つ以上なら
/// `"tool": [5, 7]`(1 つの時は前と同じく番号だけで書く)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BooleanFeature {
    pub target: FeatureId,
    #[serde(with = "kmb_core::one_or_many")]
    pub tool: Vec<FeatureId>,
    pub operation: Operation,
    /// 道具のボディを使い切らずに残す
    #[serde(default, skip_serializing_if = "is_false")]
    pub keep_tools: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Feature for BooleanFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        if self.operation == Operation::New {
            return Err(EvalError::InvalidData(
                "ブーリアンの演算は join、cut、intersect のどれかです".into(),
            ));
        }
        if self.tool.is_empty() {
            return Err(EvalError::InvalidData(
                "道具のボディを選んでください".into(),
            ));
        }
        if self.tool.contains(&self.target) {
            return Err(EvalError::InvalidData(
                "対象のボディを道具にもしています。道具から外してください".into(),
            ));
        }
        Ok(std::iter::once(self.target)
            .chain(self.tool.iter().copied())
            .map(|f| output(f, "body"))
            .collect())
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        vec![("body".into(), output(self.target, "body"))]
    }

    fn keeps(&self) -> Vec<FeatureId> {
        if self.keep_tools {
            self.tool.clone()
        } else {
            Vec::new()
        }
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut body = body_of(ctx, self.target)?;
        let b = self.operation.boolean().expect("inputs で確かめている");
        for t in &self.tool {
            let tool = body_of(ctx, *t)?;
            body = checked_boolean(ctx.kernel(), b, body, tool).map_err(kernel_err)?;
        }
        Ok([("body".to_string(), Value::Shape(body))].into())
    }
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(BOOLEAN, "ブーリアン")
            .menu("修正")
            .icon("boolean")
            .pinned()
            .order(-30)
            .hint("対象のボディに、道具のボディを結合、切り取り、交差する(道具はいくつでも)")
            .aliases("ぶーりあん boolean けつごう きりとり union subtract")
            .field(
                Field::pick("target", "ターゲットボディ", &[RefKind::Body], 1, Some(1))
                    .help("残すボディ。3D のビューでボディの面をクリックして選ぶ"),
            )
            .field(
                Field::pick("tool", "ツールボディ", &[RefKind::Body], 1, None)
                    .help("当てるボディ(いくつでも)。もう一度クリックすると外す"),
            )
            .field(Field::choice("operation", "演算", &OPERATIONS[1..]))
            .field(
                Field::bool("keep_tools", "ツールを維持")
                    .help("入れると、道具のボディを消さずに残す(同じ道具を別のボディにも使う時)"),
            )
            .template(json!({"operation": "cut"})),
        |d| Ok(Box::new(parse::<BooleanFeature>(d)?)),
    );
}

/// この crate の名乗り(登録する種類)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-boolean", register)
}
