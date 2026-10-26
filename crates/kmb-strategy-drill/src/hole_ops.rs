//! 穴の操作: 穴の機能の属性から穴あけをまとめて作る、3D のビューで選んだ穴を穴あけに足す、
//! 同じ径の穴を全部選ぶ。
//!
//! 「穴から加工を作る」は、穴の機能(種類・ネジ・径・ザグリ・皿)ごとに削る操作の並び
//! (もみつけ・下穴・ザグリ・皿の面取り)を決め、同じ並びになる穴の機能を 1 つにまとめて、
//! 穴の機能を指す穴あけを足す。穴あけは穴の機能の出力に依存するので、穴の径や位置を直すと
//! 経路も作り直される。種類を直して要る操作が変わった時は、もう一度押すと足りない操作を足し、
//! 合わなくなった工具を替える(足した操作は消さない)。
//! 工具は径の合う工具から単純な規則で 1 本入れるだけで、利用者が替える(docs/避ける形.md のとおり、
//! 工具の最適な組み合わせは選ばない)。合う工具が無い時は要る工具を知らせ、1 押しで足せる案を出す。

//!
//! 加工の操作(kmb-cam-commands)が、穴あけの戦略の名乗りから受け取って自分の操作と同じ所に並べる。

use crate::holes::{self, Need, PlannedOp};
use crate::{DRILL, Drill};
use kmb_cam_setup::{OpBase, SETUP, Setup};
use kmb_cam_tool::{TOOLS, TOOLS_V1, ToolKind, ToolLibrary};
use kmb_core::{Action, Context, Edit, Extension, FeatureId, Outcome, Payload, Place, Value};
use kmb_interchange::{HOLE_OUTPUT, HoleInfo, HolePart};
use std::sync::Arc;

pub const HOLE_OPS: &str = "cam.holes.ops";
pub const HOLE_TOOLS: &str = "cam.holes.add_tools";
pub const PICK_HOLE: &str = "cam.drill.pick";
pub const SAME_DIAMETER: &str = "cam.drill.same_diameter";
pub const AUTO_HOLES: &str = "cam.drill.auto";

/// 穴の操作。穴あけの戦略の名乗りに付ける([`kmb_cam_setup::Strategy::commands`])。
pub struct HoleOps;

impl Extension for HoleOps {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        actions(cx)
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        run(id, cx)
    }
}

/// 穴の操作を作る(戦略の名乗りに渡す)。
pub fn commands() -> Arc<dyn Extension> {
    Arc::new(HoleOps)
}

fn setup_of(cx: &Context<'_>) -> Option<FeatureId> {
    let sel = cx.selected.and_then(|s| {
        let f = cx.data.feature(s)?;
        if f.type_id == SETUP {
            return Some(s);
        }
        let s = FeatureId(f.data.get("setup")?.as_u64()?);
        (cx.data.feature(s)?.type_id == SETUP).then_some(s)
    });
    sel.or_else(|| cx.last_where(|t| t == SETUP))
}

fn tools_of(cx: &Context<'_>) -> Option<FeatureId> {
    cx.last_where(|t| t == TOOLS || t == TOOLS_V1)
}

fn library(cx: &Context<'_>, tools: FeatureId) -> Option<ToolLibrary> {
    serde_json::from_value(cx.data.feature(tools)?.data.clone()).ok()
}

/// 計算できた穴の機能と、その穴の情報(タイムラインの順)。
fn hole_features(cx: &Context<'_>) -> Vec<(FeatureId, HoleInfo)> {
    cx.data
        .features
        .iter()
        .filter(|f| !f.suppressed)
        .filter_map(|f| Some((f.id, HoleInfo::from_value(cx.output(f.id, HOLE_OUTPUT)?)?)))
        .collect()
}

/// 穴の機能をセットアップのボディが持っているか(面の名前で見る)。
fn in_body(cx: &Context<'_>, setup: FeatureId, hole: FeatureId) -> bool {
    let Some(s) = cx
        .data
        .feature(setup)
        .and_then(|f| serde_json::from_value::<Setup>(f.data.clone()).ok())
    else {
        return false;
    };
    let Some(Value::Shape(h)) = cx.output(s.body, "body") else {
        return false;
    };
    let prefix = kmb_interchange::hole::face_prefix(hole);
    cx.kernel
        .faces(*h)
        .is_ok_and(|fs| fs.iter().any(|f| f.name.starts_with(&prefix)))
}

/// 削る順の段: ザグリ、もみつけ、穴、皿。
fn stage(op: &PlannedOp) -> u8 {
    match (op.part, op.spot) {
        (HolePart::Counterbore, _) => 0,
        (HolePart::Wall, true) => 1,
        (HolePart::Wall, false) => 2,
        (HolePart::Countersink, _) => 3,
    }
}

/// 同じ操作の並びになる穴の機能のまとまり。
struct Group {
    holes: Vec<FeatureId>,
    label: String,
    ops: Vec<PlannedOp>,
}

fn groups(cx: &Context<'_>, lib: &ToolLibrary, only: &dyn Fn(FeatureId) -> bool) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for (id, h) in hole_features(cx) {
        if !only(id) {
            continue;
        }
        let plan = holes::plan(&h, lib);
        if let Some(g) = out
            .iter_mut()
            .find(|g| g.label == plan.label && g.ops == plan.ops)
        {
            g.holes.push(id);
        } else {
            out.push(Group {
                holes: vec![id],
                label: plan.label,
                ops: plan.ops,
            });
        }
    }
    out
}

/// セットアップの穴あけのうち、穴の機能 `hole` の部分 `part`(V ならもみつけ)を削るもの。
fn covering(
    cx: &Context<'_>,
    lib: &ToolLibrary,
    setup: FeatureId,
    hole: FeatureId,
    op: &PlannedOp,
) -> Option<(FeatureId, Drill)> {
    cx.data
        .features
        .iter()
        .filter(|f| f.type_id == DRILL)
        .filter_map(|f| Some((f.id, serde_json::from_value::<Drill>(f.data.clone()).ok()?)))
        .find(|(_, d)| {
            let v = lib
                .get(d.base.tool)
                .is_some_and(|t| t.kind == ToolKind::VBit);
            d.base.setup == setup
                && d.holes.contains(&hole)
                && d.part == op.part
                && (op.part == HolePart::Countersink || v == op.spot)
        })
}

/// 足す操作・直す操作と、足せなかった操作に要る工具。
struct Proposal {
    edits: Vec<Edit>,
    needs: Vec<Need>,
}

fn propose(cx: &Context<'_>, setup: FeatureId, tools: FeatureId, check_body: bool) -> Proposal {
    let mut p = Proposal {
        edits: vec![],
        needs: vec![],
    };
    let Some(lib) = library(cx, tools) else {
        return p;
    };
    let only = |id: FeatureId| !check_body || in_body(cx, setup, id);
    let gs = groups(cx, &lib, &only);
    let mut steps: Vec<(u8, usize, usize)> = Vec::new();
    for (gi, g) in gs.iter().enumerate() {
        for (oi, op) in g.ops.iter().enumerate() {
            steps.push((stage(op), gi, oi));
        }
    }
    steps.sort();
    let mut retooled: Vec<FeatureId> = Vec::new();
    for (_, gi, oi) in steps {
        let g = &gs[gi];
        let op = &g.ops[oi];
        let mut missing = Vec::new();
        for &h in &g.holes {
            match covering(cx, &lib, setup, h, op) {
                Some((id, mut d)) => {
                    // 穴を直して工具が合わなくなったら、既定の工具に替える
                    let fits = lib
                        .get(d.base.tool)
                        .is_some_and(|t| holes::how(t, op.diameter, op.part, op.spot).is_some());
                    if let (false, Some(t)) = (fits, op.tool)
                        && !retooled.contains(&id)
                    {
                        d.base.tool = t;
                        retooled.push(id);
                        p.edits.push(Edit::SetData {
                            id,
                            data: serde_json::to_value(&d).expect("JSON にできる"),
                        });
                    }
                }
                None => missing.push(h),
            }
        }
        if missing.is_empty() {
            continue;
        }
        let Some(tool) = op.tool else {
            if let Some(n) = &op.need
                && !p.needs.contains(n)
            {
                p.needs.push(n.clone());
            }
            continue;
        };
        let names: Vec<String> = missing
            .iter()
            .filter_map(|h| cx.data.feature(*h).map(|f| f.name.clone()))
            .collect();
        let d = Drill {
            holes: missing,
            part: op.part,
            ..Drill::new(OpBase::new(setup, tools, tool))
        };
        // 名前は穴の機能の名前で付ける(径や数は穴を直すと古くなるので入れない)
        p.edits.push(Edit::Insert {
            type_id: DRILL.into(),
            name: format!("{}: {}", names.join("・"), op.step),
            data: serde_json::to_value(&d).expect("JSON にできる"),
        });
    }
    p
}

fn need_text(needs: &[Need]) -> String {
    needs.iter().map(Need::text).collect::<Vec<_>>().join("、")
}

/// 選んでいる穴あけ。
fn selected_drill(cx: &Context<'_>) -> Option<(FeatureId, Drill)> {
    let id = cx.selected?;
    let f = cx.data.feature(id)?;
    (f.type_id == DRILL)
        .then(|| serde_json::from_value(f.data.clone()).ok())
        .flatten()
        .map(|d| (id, d))
}

/// 3D のビューで選んでいる面が、平らでない面(穴の壁かもしれない面)なら、その名前。
fn picked_hole(cx: &Context<'_>) -> Option<String> {
    let p = cx.picked?;
    cx.picked_plane()
        .is_none()
        .then(|| p.face.clone())
        .filter(|f| f.contains(":side:"))
}

fn actions(cx: &Context<'_>) -> Vec<Action> {
    let mut v = Vec::new();
    let setup = setup_of(cx);
    let tools = tools_of(cx);
    let at_setup = cx.selected.is_some_and(|s| setup == Some(s));
    let here: &[Place] = if at_setup {
        &[Place::Menu, Place::FeatureMenu, Place::Panel]
    } else {
        // 前には出さない(Kie 2026-10-21 の画板)
        &[Place::Menu]
    };
    let has_holes = !hole_features(cx).is_empty();
    v.push(
        Action::new(HOLE_OPS, "穴から加工を作る", "cam")
            .menu("穴")
            .icon("holeop")
            .aliases("あなからかこう ねじあな したあな ざぐり さら もみつけ hole feature recognition")
            .hint("穴の機能の種類・ネジ・径から、もみつけ・下穴・ザグリ・皿の面取りの穴あけをまとめて足す。穴を直すと経路も作り直される。種類を変えた時はもう一度押すと足りない操作を足す")
            .at(here)
            .disabled_if(if setup.is_none() || tools.is_none() {
                Some("先にセットアップを足す")
            } else if !has_holes {
                Some("設計で「穴」の機能を作ってから使う")
            } else {
                None
            }),
    );
    if let (Some(s), Some(t), true) = (setup, tools, has_holes) {
        let p = propose(cx, s, t, false);
        if !p.needs.is_empty() {
            let tools_v2 = cx.data.feature(t).is_some_and(|f| f.type_id == TOOLS);
            v.push(
                Action::new(HOLE_TOOLS, "穴に要る工具を足す", "cam")
                    .menu("穴")
                    .icon("tool")
                    .aliases("こうぐ どりる vびっと tool drill")
                    .hint(&format!(
                        "工具ライブラリに合う工具がない穴があります。{} を案として工具ライブラリに足す(切削条件は近い径の工具から写すので、確かめて直す)",
                        need_text(&p.needs)
                    ))
                    .at(if at_setup {
                        &[Place::FeatureMenu, Place::Panel]
                    } else {
                        &[]
                    })
                    .disabled_if((!tools_v2).then_some("古い形式の工具ライブラリには足せません")),
            );
        }
    }
    let picked = picked_hole(cx);
    let drill = selected_drill(cx);
    if let Some((_, d)) = &drill {
        v.push(
            Action::new(PICK_HOLE, "選んだ穴を足す", "cam")
                .menu("穴")
                .icon("drill")
                .aliases("あなをえらぶ select hole")
                .hint("3D のビューでクリックした穴を、この穴あけで削る穴に足す")
                .at(&[Place::Panel, Place::FaceMenu])
                .disabled_if(
                    picked
                        .is_none()
                        .then_some("3D のビューで穴の壁をクリックしてから使う"),
                ),
        );
        v.push(
            Action::new(SAME_DIAMETER, "同じ径の穴を全部選ぶ", "cam")
                .menu("穴")
                .icon("drill")
                .aliases("おなじけい same diameter")
                .hint("選んだ穴と同じ径の穴を、すべて削る")
                .at(&[Place::Panel, Place::FaceMenu])
                .disabled_if(
                    (picked.is_none() && d.faces.is_empty())
                        .then_some("3D のビューで穴の壁をクリックしてから使う"),
                ),
        );
        if !d.faces.is_empty() {
            v.push(
                Action::new(AUTO_HOLES, "穴を自動で拾う", "cam")
                    .menu("穴")
                    .icon("drill")
                    .hint("選んだ穴を外し、上から削れる丸い穴をすべて削る")
                    .at(&[Place::Panel, Place::FeatureMenu]),
            );
        }
    } else if picked.is_some() && setup.is_some() && tools.is_some() {
        v.push(
            Action::new(SAME_DIAMETER, "同じ径の穴をあける", "cam")
                .menu("穴")
                .icon("drill")
                .aliases("おなじけい same diameter")
                .hint("クリックした穴と同じ径の穴を、すべて削る穴あけを足す")
                .at(&[Place::FaceMenu]),
        );
    }
    v
}

fn run(id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
    if ![HOLE_OPS, HOLE_TOOLS, PICK_HOLE, SAME_DIAMETER, AUTO_HOLES].contains(&id) {
        return None;
    }
    Some((|| {
        let setup = setup_of(cx).ok_or("セットアップがありません")?;
        let tools = tools_of(cx).ok_or("工具ライブラリがありません")?;
        match id {
            HOLE_OPS => {
                let p = propose(cx, setup, tools, true);
                if p.edits.is_empty() {
                    return Err(if p.needs.is_empty() {
                        "穴の加工はもう揃っています(穴の機能を足すか直してから押す)".into()
                    } else {
                        format!(
                            "工具ライブラリに合う工具がありません。要る工具: {}(「穴に要る工具を足す」で足せる)",
                            need_text(&p.needs)
                        )
                    });
                }
                Ok(Outcome::Change {
                    label: "穴から加工を作る".into(),
                    edits: p.edits,
                })
            }
            HOLE_TOOLS => {
                let p = propose(cx, setup, tools, false);
                let mut lib = library(cx, tools).ok_or("工具ライブラリが読めません")?;
                let add = holes::tools_for(&lib, &p.needs);
                if add.is_empty() {
                    return Err("足す工具はありません".into());
                }
                lib.tools.extend(add);
                Ok(Outcome::Change {
                    label: "穴に要る工具を足す".into(),
                    edits: vec![Edit::SetData {
                        id: tools,
                        data: serde_json::to_value(&lib).map_err(|e| e.to_string())?,
                    }],
                })
            }
            AUTO_HOLES => {
                let (sel, mut d) = selected_drill(cx).ok_or("穴あけを選んでいません")?;
                d.faces.clear();
                d.same_diameter = false;
                Ok(change("穴を自動で拾う", sel, &d))
            }
            _ => {
                let picked = picked_hole(cx);
                match selected_drill(cx) {
                    Some((sel, mut d)) => {
                        if let Some(f) = picked
                            && !d.faces.contains(&f)
                        {
                            d.faces.push(f);
                        }
                        if d.faces.is_empty() {
                            return Err("3D のビューで穴の壁をクリックしてください".into());
                        }
                        let label = if id == PICK_HOLE {
                            "選んだ穴を足す"
                        } else {
                            d.same_diameter = true;
                            "同じ径の穴を全部選ぶ"
                        };
                        Ok(change(label, sel, &d))
                    }
                    None => {
                        let f = picked.ok_or("3D のビューで穴の壁をクリックしてください")?;
                        let tool = library(cx, tools)
                            .and_then(|l| {
                                l.tools
                                    .iter()
                                    .find(|t| t.kind == ToolKind::Drill)
                                    .or_else(|| {
                                        l.tools
                                            .iter()
                                            .filter(|t| t.kind == ToolKind::FlatEnd)
                                            .min_by(|a, b| a.diameter.total_cmp(&b.diameter))
                                    })
                                    .map(|t| t.number)
                            })
                            .unwrap_or(1);
                        let d = Drill {
                            faces: vec![f],
                            same_diameter: id == SAME_DIAMETER,
                            ..Drill::new(OpBase::new(setup, tools, tool))
                        };
                        Ok(Outcome::insert(
                            "同じ径の穴をあける",
                            DRILL,
                            "同じ径の穴".into(),
                            serde_json::to_value(&d).map_err(|e| e.to_string())?,
                        ))
                    }
                }
            }
        }
    })().map_err(Into::into))
}

fn change(label: &str, id: FeatureId, d: &Drill) -> Outcome {
    Outcome::Change {
        label: label.into(),
        edits: vec![Edit::SetData {
            id,
            data: serde_json::to_value(d).expect("JSON にできる"),
        }],
    }
}
