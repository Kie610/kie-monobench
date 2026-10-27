//! 形を調べる操作(体積・表面積・外形を測る)。

use kmb_core::{Action, Context, Extension, Outcome, Place};

/// 選んでいるボディ(なければ最後に見えているボディ)の体積などを測る操作。
pub const MEASURE: &str = "cad.measure";

/// 調べる操作。
pub struct InspectCommands;

impl Extension for InspectCommands {
    /// 辺のダブルクリック: 接してつながる辺をたどる([`crate::tangent_chains`])。
    fn edge_chain(&self, edges: &[kmb_core::EdgeInfo], seeds: &[usize]) -> Option<Vec<usize>> {
        Some(crate::tangent_chains(edges, seeds, crate::TANGENT_DEG))
    }

    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let bodies = cx.visible_bodies();
        let on_body = cx.selected.is_some_and(|s| bodies.iter().any(|b| b.0 == s));
        vec![
            Action::new(MEASURE, "体積を計測", "cad")
                .menu("検査")
                .icon("volume")
                .aliases("たいせき はかる measure volume")
                .hint("選んでいるボディ(なければ最後のボディ)の体積・表面積・外形")
                .at(&[Place::Toolbar])
                .at(if on_body {
                    &[Place::Panel, Place::FeatureMenu]
                } else {
                    &[]
                })
                .disabled_if(bodies.is_empty().then_some("ボディがありません")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        if id != MEASURE {
            return None;
        }
        let bodies = cx.visible_bodies();
        let body = cx
            .selected
            .and_then(|s| bodies.iter().find(|b| b.0 == s))
            .or(bodies.last());
        Some(match body {
            None => Err("ボディがありません".into()),
            Some((f, h))
                if cx.kernel.body_kind(*h).ok() == Some(kmb_core::kernel::BodyKind::Sheet) =>
            {
                (|| {
                    let (min, max) = cx.kernel.bounds(*h).map_err(|e| e.to_string())?;
                    let area: f64 = cx
                        .kernel
                        .faces(*h)
                        .map_err(|e| e.to_string())?
                        .iter()
                        .map(|f| f.area)
                        .sum();
                    let name = cx
                        .data
                        .feature(*f)
                        .map_or(String::new(), |e| e.name.clone());
                    Ok(Outcome::Message(format!(
                        "{name}: 面のボディ(体積なし)、面積 {area:.3} mm²、外形 {:.3} × {:.3} × {:.3} mm",
                        max[0] - min[0],
                        max[1] - min[1],
                        max[2] - min[2]
                    )))
                })()
            }
            Some((f, h)) => cx
                .kernel
                .mass(*h)
                .map_err(|e| e.to_string())
                .map(|m| {
                    let name = cx
                        .data
                        .feature(*f)
                        .map_or(String::new(), |e| e.name.clone());
                    Outcome::Message(format!(
                        "{name}: 体積 {:.3} mm³、表面積 {:.3} mm²、外形 {:.3} × {:.3} × {:.3} mm",
                        m.volume,
                        m.area,
                        m.max[0] - m.min[0],
                        m.max[1] - m.min[1],
                        m.max[2] - m.min[2]
                    ))
                })
                .map_err(Into::into),
        })
    }
}
