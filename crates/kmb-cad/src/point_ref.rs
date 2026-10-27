//! 点の参照。位置は再計算した出力から読み、番号や名前が消えた時は止める。
use crate::{body_of, output};
use kmb_core::{EvalCtx, EvalError, FeatureId, InputRef, Payload, PointId, Points, kernel::P3};
use serde::{Deserialize, Serialize};

/// 作業点、スケッチの点、ボディの頂点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointRef {
    Datum {
        #[serde(rename = "datum")]
        feature: FeatureId,
    },
    Sketch {
        #[serde(rename = "sketch")]
        feature: FeatureId,
        point: PointId,
    },
    Vertex {
        feature: FeatureId,
        vertex: String,
    },
}

impl PointRef {
    pub fn input(&self) -> InputRef {
        match self {
            Self::Datum { feature } => output(*feature, kmb_interchange::DATUM_OUTPUT),
            Self::Sketch { feature, .. } => output(*feature, "points"),
            Self::Vertex { feature, .. } => output(*feature, "body"),
        }
    }

    pub fn resolve(&self, ctx: &EvalCtx<'_>) -> Result<P3, EvalError> {
        match self {
            Self::Datum { feature } => kmb_interchange::datum_point(ctx, *feature),
            Self::Sketch { feature, point } => {
                let points =
                    Points::from_value(ctx.output(*feature, "points")?).ok_or_else(|| {
                        EvalError::InvalidData(
                            "端の点のスケッチがありません。点を選び直してください".into(),
                        )
                    })?;
                points
                    .points
                    .get(point)
                    .map(|p| points.plane.to_world(*p))
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!(
                            "スケッチの点 {} がありません。端の点を選び直してください",
                            point.0
                        ))
                    })
            }
            Self::Vertex { feature, vertex } => {
                let body = body_of(ctx, *feature)?;
                kmb_core::vertices(ctx.kernel(), body)
                    .into_iter()
                    .find(|v| v.name == *vertex)
                    .map(|v| v.at)
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!(
                            "端の頂点 {vertex} がありません。点を選び直してください"
                        ))
                    })
            }
        }
    }
}
