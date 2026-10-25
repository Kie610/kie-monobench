//! 面のオフセット(`solid.offset_faces@1`): 選んだ面を、外向きの法線の向きへ決めた距離だけ動かす。
//! 曲がった面(穴の壁、円柱の側面、フィレットの面)にも使える。
//! プレス/プル(Q)で曲がった面を選んだ時の行き先でもある。カーネルの
//! [`kmb_core::Kernel::offset_faces`] を使う(今は SMLib だけ)。
use kmb_cad::*;
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Ref, RefKind, Registry, Value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const OFFSET_FACES: &str = "solid.offset_faces@1";

/// 例: `{"faces": [{"kind": "face", "feature": 3, "face": "f1:side:e9"}], "distance": "-1 mm"}`。
/// 正なら外へ肉を足し、負なら内へ削る(穴の壁を負にすると穴が広がる)。面はどれも 1 つのボディの物。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetFaces {
    #[serde(default, with = "kmb_core::one_or_many")]
    pub faces: Vec<Ref>,
    pub distance: String,
}

impl OffsetFaces {
    /// 面のあるボディ(どれも同じボディ)。
    fn body(&self) -> Result<FeatureId, EvalError> {
        let first = self
            .faces
            .first()
            .ok_or_else(|| EvalError::InvalidData("動かす面を選んでください".into()))?;
        if self.faces.iter().any(|r| r.feature() != first.feature()) {
            return Err(EvalError::InvalidData(
                "オフセットする面は 1 つのボディから選んでください".into(),
            ));
        }
        Ok(first.feature())
    }

    fn names(&self) -> Result<Vec<String>, EvalError> {
        self.faces
            .iter()
            .map(|r| match r {
                Ref::Face { face, .. } => Ok(face.clone()),
                _ => Err(EvalError::InvalidData(
                    "面のオフセットには面を選んでください(辺・頂点・ボディは使えません)".into(),
                )),
            })
            .collect()
    }
}

impl Exprs for OffsetFaces {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.distance]
    }
}

impl Feature for OffsetFaces {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.body()?, "body")];
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        self.body()
            .map(|b| vec![("body".into(), output(b, "body"))])
            .unwrap_or_default()
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let d = length(ctx, &self.distance, "距離")?;
        let names = self.names()?;
        let body = body_of(ctx, self.body()?)?;
        let tag = tag(ctx);
        let h = ctx
            .kernel()
            .offset_faces(body, &names, d, &tag)
            .map_err(|e| {
                EvalError::InvalidData(format!(
                    "面をオフセットできませんでした({e})。距離を小さくするか、面を選び直してください"
                ))
            })?;
        log::info!("面 {names:?} を {d} mm オフセットした");
        Ok([("body".to_string(), Value::Shape(h))].into())
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(OFFSET_FACES, "面をオフセット")
            .menu("修正")
            .icon("offset_face")
            .role(kmb_interchange::role::kind::OFFSET_FACES)
            .hint("選んだ面を法線の向きへ動かす。正で肉を足し、負で削る(曲がった面にも使える。穴の壁を負にすると穴が広がる)")
            .aliases("めん オフセット offset face おふせっと うごかす 穴を広げる")
            .field(
                Field::pick("faces", "面", &[RefKind::Face], 1, None)
                    .help("動かす面をクリックで足す(もう一度で外す)。どれも 1 つのボディの面"),
            )
            .field(Field::expr("distance", "距離").length().help("正で外へ肉を足し、負で内へ削る"))
            .template(json!({"distance": "2 mm"})),
        |d| Ok(Box::new(parse::<OffsetFaces>(d)?)),
    );
}
