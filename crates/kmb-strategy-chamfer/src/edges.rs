//! 面取り加工の辺の選び直し: 3D のビューで選んだ辺だけを削るように変える・辺の選びをやめて
//! 縁の種類(外周と穴など)に戻す。機能のデータ(`selected`)だけを書き換える。
//!
//! 加工の操作(kmb-cam-commands)が、面取りの戦略の名乗りから受け取って自分の操作と同じ所に並べる。

use crate::CHAMFER;
use kmb_core::{Action, Context, Edit, Extension, FeatureId, Outcome, Place};
use serde_json::{Value, json};
use std::sync::Arc;

pub const USE_EDGES: &str = "cam.chamfer.edges";
pub const ALL_EDGES: &str = "cam.chamfer.all_edges";

/// 辺の選び直しの操作。面取りの戦略の名乗りに付ける([`kmb_cam_setup::Strategy::commands`])。
pub struct ChamferEdges;

/// 辺の選び直しの操作を作る(戦略の名乗りに渡す)。
pub fn commands() -> Arc<dyn Extension> {
    Arc::new(ChamferEdges)
}

/// 選んでいる機能が面取り加工なら、その ID とデータ。
fn selected(cx: &Context<'_>) -> Option<(FeatureId, Value)> {
    let id = cx.selected?;
    let f = cx.data.feature(id)?;
    (f.type_id == CHAMFER).then(|| (id, f.data.clone()))
}

impl Extension for ChamferEdges {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let mut v = Vec::new();
        let Some((_, d)) = selected(cx) else {
            return v;
        };
        v.push(
            Action::new(USE_EDGES, "選んだ辺だけを面取りする", "cam")
                .menu("2D")
                .icon("chamferop")
                .aliases("へん えっじ めんとり edge chamfer")
                .hint("3D のビューの「辺」で選んだ縁だけを削る(Shift・Ctrl+クリックで足す・外す)")
                .at(&[Place::Panel, Place::FeatureMenu])
                .disabled_if(
                    cx.edges
                        .is_empty()
                        .then_some("3D のビューで、選ぶ種類を「辺」にして縁を選んでから使う"),
                ),
        );
        if d.get("selected")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
        {
            v.push(
                Action::new(ALL_EDGES, "辺の選びをやめる", "cam")
                    .menu("2D")
                    .icon("chamferop")
                    .hint("選んだ辺を消し、「削る縁」(外周と穴など)で削る")
                    .at(&[Place::Panel, Place::FeatureMenu]),
            );
        }
        v
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        if id != USE_EDGES && id != ALL_EDGES {
            return None;
        }
        Some((|| {
            let (fid, mut d) = selected(cx).ok_or("面取り加工を選んでいません")?;
            let o = d.as_object_mut().ok_or("データが読めません")?;
            let label = if id == USE_EDGES {
                if cx.edges.is_empty() {
                    return Err("辺を選んでいません".into());
                }
                let pairs: Vec<&[String; 2]> = cx.edges.iter().map(|e| &e.faces).collect();
                o.insert("selected".into(), json!(pairs));
                "面取りする辺を選ぶ"
            } else {
                o.remove("selected");
                "辺の選びをやめる"
            };
            Ok(Outcome::Change {
                label: label.into(),
                edits: vec![Edit::SetData { id: fid, data: d }],
            })
        })())
    }
}
