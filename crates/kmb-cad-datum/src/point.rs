//! 作業点(`cad.datum_point@1`)。ボディの角・円の辺の中心・座標の 3 つの作り方を、点 1 つの種類で
//! 持ち、出力 [`DATUM_OUTPUT`] に位置([`DatumPoint`])を出す。作業平面の 3 点と作業軸の 2 点に使える。
use crate::{Point, point_inputs};
use kmb_cad::*;
use kmb_core::{EvalCtx, EvalError, Feature, Field, InputRef, Outputs, Payload, Registry};
use kmb_interchange::{DATUM_OUTPUT, DatumPoint};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const DATUM_POINT: &str = "cad.datum_point@1";

/// 作業点。例: `{"point": {"kind": "vertex", "feature": 2, "vertex": "…"}}`、
/// `{"point": {"kind": "at", "at": [10, 0, 5]}}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatumPointFeature {
    pub point: Point,
}

impl Default for DatumPointFeature {
    fn default() -> Self {
        DatumPointFeature {
            point: Point::At { at: [0.0; 3] },
        }
    }
}

impl Feature for DatumPointFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = Vec::new();
        point_inputs(std::slice::from_ref(&self.point), &mut v);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        if let Point::Datum { feature } = &self.point
            && *feature == ctx.feature_id()
        {
            return Err(EvalError::InvalidData("作業点が自分を指しています".into()));
        }
        let at = self.point.resolve(ctx)?;
        log::debug!("作業点 {}: {:?}", ctx.feature_id().0, at);
        Ok([(DATUM_OUTPUT.to_string(), DatumPoint { at }.to_value())].into())
    }
}

pub(crate) fn register(reg: &mut Registry) {
    reg.register_described(
        kmb_core::Descriptor::new(DATUM_POINT, kmb_core::category::SOLID, "作業点")
            .menu("構築")
            .icon("datumPoint")
            .hint("作業平面や作業軸の基準にする点を作る(ボディの角、円の辺の中心、座標)")
            .aliases("さぎょうてん こうちく てん datum point construction")
            .field(
                Field::choice(
                    "point.kind",
                    "作り方",
                    &[
                        ("vertex", "ボディの角"),
                        ("center", "円の中心"),
                        ("at", "座標"),
                        ("datum", "作業点"),
                    ],
                )
                .hide_choice("datum"),
            )
            .field(
                Field::vector("point.at", "座標")
                    .length()
                    .only("point.kind", "at", "at"),
            )
            .template(json!({"point": {"kind": "at", "at": [0, 0, 0]}})),
        |d| Ok(Box::new(parse::<DatumPointFeature>(d)?)),
    );
}
