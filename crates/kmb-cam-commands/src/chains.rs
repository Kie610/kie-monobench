//! 輪郭加工の「削る輪郭を選ぶ」操作。3D のビューで、上を向いた平らな面の縁(外周と穴の縁)と、
//! スケッチの線のつながりをクリックで足す・外し、Enter で決める。選ばずに Enter なら部品の外形に戻す。

use crate::SetupInfo;
use kmb_cam_geom::MachiningModel;
use kmb_cam_setup::Strategy;
use kmb_core::{Action, Context, Curves, Edit, Gesture, Handle, HandleChoice, Outcome, Place};
use serde_json::json;

pub const PICK_CHAINS: &str = "cam.contour.chains";

/// 選んでいる加工が、削る輪郭を選べる戦略(輪郭)なら、その機能。
fn selected_contour(cx: &Context<'_>, strategies: &[Strategy]) -> Option<kmb_core::FeatureId> {
    let id = cx.selected?;
    let t = &cx.data.feature(id)?.type_id;
    strategies
        .iter()
        .any(|s| s.pick_chains && s.type_id == t)
        .then_some(id)
}

pub fn actions(cx: &Context<'_>, info: Option<&SetupInfo>, strategies: &[Strategy]) -> Vec<Action> {
    if selected_contour(cx, strategies).is_none() {
        return vec![];
    }
    vec![
        Action::new(PICK_CHAINS, "削る輪郭を選ぶ", "cam")
            .arg(kmb_core::Arg::list("chains", "削る輪郭(手順で選んだ線の並び)。空なら部品の外形"))
            .menu("2D")
            .icon("contour")
            .aliases("りんかく えらぶ chain contour edge sketch あな")
            .hint(
                "面の外周と穴の縁、スケッチの線をクリックで足す・外し、Enter で決める。いくつでも選べる。\
                 選ばずに Enter なら部品の外形",
            )
            .at(&[Place::Panel, Place::FeatureMenu])
            .disabled_if(
                info.is_none()
                    .then_some("セットアップの計算ができていません"),
            ),
    ]
}

/// 選べる輪郭(値・名前・線)。
fn candidates(cx: &Context<'_>, info: &SetupInfo, op: kmb_core::FeatureId) -> Vec<HandleChoice> {
    let mut out = Vec::new();
    let world = |q: [f64; 3]| info.to_world(q);
    // 上を向いた平らな面の縁
    if let Ok(mesh) = cx.kernel.mesh(info.body, info.tolerance) {
        let model = MachiningModel::from_mesh(&info.placed_mesh(&mesh), info.origin);
        for f in model.flats.iter().filter(|f| f.up) {
            let rings = std::iter::once(&f.area.outer).chain(&f.area.holes);
            for (k, l) in rings.enumerate() {
                let mut pts: Vec<[f64; 3]> = l
                    .flatten(0.05)
                    .into_iter()
                    .map(|q| world([q[0], q[1], f.z]))
                    .collect();
                if let Some(p) = pts.first().copied() {
                    pts.push(p);
                }
                out.push(HandleChoice {
                    value: json!({ "face": f.name, "ring": k }),
                    label: if k == 0 {
                        format!("面の外周(高さ {:.1} mm)", f.z)
                    } else {
                        format!("面の穴の縁 {k}(高さ {:.1} mm)", f.z)
                    },
                    outline: vec![pts],
                    group: 0,
                    ..Default::default()
                });
            }
        }
    }
    // スケッチの線(輪郭より前の機能だけ参照できる)
    let at = cx.data.index_of(op).unwrap_or(usize::MAX);
    for f in cx.data.features.iter().take(at) {
        let Some(c) = cx.output(f.id, "curves").and_then(Curves::from_value) else {
            continue;
        };
        let Ok((z, chains)) = kmb_cam_setup::chains_of(&c, info) else {
            continue;
        };
        for (k, ch) in chains.iter().enumerate() {
            out.push(HandleChoice {
                value: json!({ "sketch": f.id.0, "chain": k }),
                label: format!(
                    "{} の線 {}({})",
                    f.name,
                    k + 1,
                    if ch.closed {
                        "閉じた線"
                    } else {
                        "開いた線"
                    }
                ),
                outline: vec![ch.pts.iter().map(|q| world([q[0], q[1], z])).collect()],
                group: 0,
                ..Default::default()
            });
        }
    }
    out
}

pub fn handles(
    id: &str,
    cx: &Context<'_>,
    info: Option<&SetupInfo>,
    strategies: &[Strategy],
) -> Option<Vec<Handle>> {
    if id != PICK_CHAINS {
        return None;
    }
    let i = info?;
    let op = selected_contour(cx, strategies)?;
    let choices = candidates(cx, i, op);
    // 前に選んだ輪郭は選んだまま始める(候補にある物だけ)
    let picked = cx
        .data
        .feature(op)
        .and_then(|f| f.data.get("chains"))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter(|v| choices.iter().any(|c| c.value == **v))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    Some(vec![Handle {
        choices,
        picked,
        optional: true,
        ..Handle::new(
            "chains",
            "削る輪郭(面の縁・スケッチの線)をクリックで足す・外し、Enter で決める。選ばずに Enter なら部品の外形",
            Gesture::Regions,
        )
    }])
}

pub fn run(id: &str, cx: &Context<'_>, strategies: &[Strategy]) -> Option<Result<Outcome, String>> {
    if id != PICK_CHAINS {
        return None;
    }
    Some((|| {
        let sel = selected_contour(cx, strategies).ok_or("輪郭の加工を選んでいません")?;
        let mut data = cx
            .data
            .feature(sel)
            .ok_or("輪郭の加工がありません")?
            .data
            .clone();
        let obj = data.as_object_mut().ok_or("加工のデータが読めません")?;
        match cx.arg("chains") {
            Some(serde_json::Value::Array(a)) if !a.is_empty() => {
                obj.insert("chains".into(), serde_json::Value::Array(a.clone()));
            }
            _ => {
                obj.remove("chains");
            }
        }
        Ok(Outcome::Change {
            label: "削る輪郭を選ぶ".into(),
            edits: vec![Edit::SetData { id: sel, data }],
        })
    })())
}
