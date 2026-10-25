//! 輪郭加工の「タブを置く」操作。3D のビューで、タブの付く経路(最後の周の工具の中心)の上を
//! クリックしてタブを足す・外し、Enter で決める。選ばずに Enter なら、数の欄で等間隔に置く。

use crate::SetupInfo;
use crate::commands::round_um;
use kmb_cam_setup::Strategy;
use kmb_cam_setup::{TAB_PATHS, TabPath, TabPaths};
use kmb_core::{Action, Context, Edit, Gesture, Handle, HandleChoice, Outcome, Place};
use serde_json::{Value, json};

pub const PLACE_TABS: &str = "cam.contour.tabs";

/// 経路の上の候補の間隔(mm)。
const STEP: f64 = 2.0;

/// 選んでいる加工が、タブを置ける戦略(輪郭)なら、その機能。
fn selected_contour(cx: &Context<'_>, strategies: &[Strategy]) -> Option<kmb_core::FeatureId> {
    let id = cx.selected?;
    let t = &cx.data.feature(id)?.type_id;
    strategies
        .iter()
        .any(|s| s.tabs && s.type_id == t)
        .then_some(id)
}

fn tab_paths(cx: &Context<'_>, op: kmb_core::FeatureId) -> Vec<TabPath> {
    cx.output(op, TAB_PATHS)
        .and_then(<TabPaths as kmb_core::Payload>::from_value)
        .map(|t| t.paths)
        .unwrap_or_default()
}

pub fn actions(cx: &Context<'_>, info: Option<&SetupInfo>, strategies: &[Strategy]) -> Vec<Action> {
    let Some(op) = selected_contour(cx, strategies) else {
        return vec![];
    };
    let no_path = tab_paths(cx, op).is_empty();
    vec![
        Action::new(PLACE_TABS, "タブを置く", "cam")
            .arg(kmb_core::Arg::list("tabs", "タブの位置(手順で選んだ経路の上の点の並び)。空ならタブの数の欄で等間隔"))
            .menu("2D")
            .icon("contour")
            .aliases("たぶ ぶりっじ tab bridge")
            .hint(
                "経路の上をクリックでタブを足す・外し、Enter で決める。\
                 選ばずに Enter なら、タブの数の欄で等間隔に置く",
            )
            .at(&[Place::Panel, Place::FeatureMenu])
            .disabled_if(if info.is_none() {
                Some("セットアップの計算ができていません")
            } else if no_path {
                Some("タブの付く閉じた経路がありません(輪郭の計算ができていないか、開いた線だけです)")
            } else {
                None
            }),
    ]
}

/// 経路を `STEP` ごとに区切った候補の位置と、その所の向き(長さ 1)。
fn samples(pts: &[[f64; 2]]) -> Vec<([f64; 2], [f64; 2])> {
    let mut out = Vec::new();
    let mut carry = 0.0;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        if len < 1e-9 {
            continue;
        }
        let d = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        let mut s = carry;
        while s < len {
            out.push(([a[0] + d[0] * s, a[1] + d[1] * s], d));
            s += STEP;
        }
        carry = s - len;
    }
    out
}

pub fn handles(
    id: &str,
    cx: &Context<'_>,
    info: Option<&SetupInfo>,
    strategies: &[Strategy],
) -> Option<Vec<Handle>> {
    if id != PLACE_TABS {
        return None;
    }
    let i = info?;
    let op = selected_contour(cx, strategies)?;
    let placed: Vec<[f64; 2]> = cx
        .data
        .feature(op)
        .and_then(|f| f.data.get("tab_points"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    // ストックの上面の高さに描く(切り抜く部品の上の縁と重なる)
    let z = i.stock_max[2];
    let mut choices: Vec<HandleChoice> = Vec::new();
    for t in tab_paths(cx, op) {
        for (q, d) in samples(&t.pts) {
            // 経路に沿う向きに STEP、横に 3 mm の四角(並べると帯になり、どこをクリックしても拾える)
            let (h, w) = (STEP / 2.0, 1.5);
            let n = [-d[1], d[0]];
            let corner = |a: f64, b: f64| {
                i.to_world([q[0] + d[0] * a + n[0] * b, q[1] + d[1] * a + n[1] * b, z])
            };
            choices.push(HandleChoice {
                value: json!([round_um(q[0]), round_um(q[1])]),
                label: format!("タブ({:.1}, {:.1})", q[0], q[1]),
                outline: vec![vec![
                    corner(-h, -w),
                    corner(h, -w),
                    corner(h, w),
                    corner(-h, w),
                    corner(-h, -w),
                ]],
                group: 0,
                ..Default::default()
            });
        }
    }
    // 置いてあるタブは、いちばん近い候補を選んだまま始める(その候補の値は置いた位置のまま)
    let mut picked = Vec::new();
    for p in &placed {
        let near = choices
            .iter()
            .enumerate()
            .filter_map(|(k, c)| {
                let v: [f64; 2] = serde_json::from_value(c.value.clone()).ok()?;
                Some((k, (v[0] - p[0]).hypot(v[1] - p[1])))
            })
            .filter(|(_, d)| *d <= STEP)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, _)) = near {
            choices[k].value = json!(p);
            picked.push(choices[k].value.clone());
        }
    }
    Some(vec![Handle {
        choices,
        picked,
        optional: true,
        ..Handle::new(
            "tabs",
            "タブを置く所(経路の上)をクリックで足す・外し、Enter で決める。選ばずに Enter なら数の欄で等間隔",
            Gesture::Regions,
        )
    }])
}

pub fn run(id: &str, cx: &Context<'_>, strategies: &[Strategy]) -> Option<Result<Outcome, String>> {
    if id != PLACE_TABS {
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
        match cx.arg("tabs") {
            Some(Value::Array(a)) if !a.is_empty() => {
                obj.insert("tab_points".into(), Value::Array(a.clone()));
            }
            _ => {
                obj.remove("tab_points");
            }
        }
        Ok(Outcome::Change {
            label: "タブを置く".into(),
            edits: vec![Edit::SetData { id: sel, data }],
        })
    })())
}
