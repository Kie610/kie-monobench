//! 同じ領域を複数の工具で削る「工具の順番」。ポケットと、その隅を削る輪郭もつなぐ。
//!
//! 順番の 2 本目からは、同じ面・同じセットアップの残り削り(`rest_tool` に前の工具)として並ぶ。
//! 順番は利用者が決める(どの工具を使うかを自動で選ぶ働きは作らない。docs/避ける形.md)。
//! 残り削りは太い工具から細い工具の順でしか意味がないので、並べ替えの操作は持たない
//! (足す工具を選ぶ順が、そのまま削る順になる)。外す時はタイムラインで消す。
//! 足せるのは平・V・テーパーの工具で、先(いちばん下)の径が順番の最後の工具より細いもの。
//! 負荷制御粗取りの後に V やテーパーを足す時は、ポケットの残り削りとして足す
//! (負荷制御粗取りは平の工具の経路のため)。
//! 画面は、ここが出す操作を編集パネルに並べるだけ。
//! 順番を持てる種類は、戦略の名乗り([`Strategy::tool_order`])で決まる。ここは戦略の名前を知らない。

use crate::ToolLibrary;
use kmb_cam_setup::{OpBase, Strategy};
use kmb_core::{Action, Context, FeatureId, Outcome, Place};
use serde::Deserialize;

/// 工具を順番に足す操作の ID の前置き(後ろに工具の番号)。
pub const ADD_TOOL: &str = "cam.tool_order.add:";

/// 順番を持てる操作の、順番に関わる値(セットアップ、面、工具ライブラリ、工具、前の工具)。
struct Step {
    id: FeatureId,
    setup: FeatureId,
    face: String,
    tools: FeatureId,
    tool: u32,
    rest: Option<u32>,
    /// 前の工具を直径で打った残り削り(番号でたどる順番には入らない)
    sized: bool,
}

/// 削る領域を指す面かスケッチ。部分の番号があれば、全体とは別の領域として扱う。
#[derive(Deserialize)]
#[serde(untagged)]
enum Boundary {
    Face {
        face: String,
        ring: Option<usize>,
    },
    Sketch {
        sketch: FeatureId,
        chain: Option<usize>,
    },
}

/// 面の外周と、その面を底にする加工は同じ領域として工具の順番をつなぐ。
fn area_key(a: &Area, default_side: Option<&str>) -> String {
    let mut faces = a.faces.clone();
    let mut profiles = Vec::new();
    let mut parts = Vec::new();
    if a.chains.is_empty() {
        faces.extend(a.face.iter().filter(|s| !s.is_empty()).cloned());
        profiles.extend(a.profile);
    } else {
        faces.clear();
        for boundary in &a.chains {
            match boundary {
                Boundary::Face {
                    face,
                    ring: Some(0),
                } => faces.push(face.clone()),
                Boundary::Sketch {
                    sketch,
                    chain: None,
                } => profiles.push(*sketch),
                Boundary::Face { face, ring } => parts.push(format!("face:{face:?}:{ring:?}")),
                Boundary::Sketch { sketch, chain } => {
                    parts.push(format!("sketch:{sketch:?}:{chain:?}"))
                }
            }
        }
    }
    faces.sort();
    faces.dedup();
    profiles.sort();
    profiles.dedup();
    parts.sort();
    parts.dedup();
    // 外側と線の上を削る輪郭は、面の内側を削る加工とつながない。
    let side = a
        .side
        .as_deref()
        .or(default_side)
        .filter(|s| *s != "inside");
    format!("{faces:?}|{profiles:?}|{parts:?}|{side:?}")
}

/// 順番を持てる操作のデータのうち、順番に関わる欄。
#[derive(Deserialize)]
struct Area {
    #[serde(flatten)]
    base: OpBase,
    #[serde(default)]
    face: Option<String>,
    #[serde(default)]
    faces: Vec<String>,
    #[serde(default)]
    profile: Option<FeatureId>,
    #[serde(default)]
    chains: Vec<Boundary>,
    #[serde(default)]
    side: Option<String>,
    #[serde(default)]
    rest_tool: Option<u32>,
    #[serde(default)]
    rest_diameter: Option<String>,
}

fn step(cx: &Context<'_>, all: &[Strategy], id: FeatureId) -> Option<Step> {
    let f = cx.data.feature(id)?;
    let strategy = all
        .iter()
        .find(|s| s.tool_order && s.type_id == f.type_id)?;
    let a: Area = serde_json::from_value(f.data.clone()).ok()?;
    Some(Step {
        id,
        setup: a.base.setup,
        face: area_key(&a, strategy.tool_order_default_side),
        tools: a.base.tools,
        tool: a.base.tool,
        rest: a.rest_tool,
        sized: a.rest_tool.is_none() && a.rest_diameter.is_some(),
    })
}

/// 操作 `id` が入っている工具の順番(タイムラインの順。最初は残り削りでない操作)。
pub fn chain(cx: &Context<'_>, all: &[Strategy], id: FeatureId) -> Vec<FeatureId> {
    let Some(me) = step(cx, all, id) else {
        return vec![];
    };
    let same: Vec<Step> = cx
        .data
        .features
        .iter()
        .filter_map(|f| step(cx, all, f.id))
        .filter(|s| s.setup == me.setup && s.tools == me.tools && s.face == me.face)
        .collect();
    // 最初の工具(残り削りでないもの)から、前の工具が一致するものをたどる
    for root in same.iter().filter(|s| s.rest.is_none() && !s.sized) {
        let mut out = vec![root.id];
        let mut last = root.tool;
        let mut from = cx.data.index_of(root.id).unwrap_or(0);
        while let Some(next) = same
            .iter()
            .find(|s| s.rest == Some(last) && cx.data.index_of(s.id).is_some_and(|i| i > from))
        {
            out.push(next.id);
            last = next.tool;
            from = cx.data.index_of(next.id).unwrap_or(from);
        }
        if out.contains(&id) {
            return out;
        }
    }
    vec![id]
}

fn library(cx: &Context<'_>, tools: FeatureId) -> Option<ToolLibrary> {
    serde_json::from_value(cx.data.feature(tools)?.data.clone()).ok()
}

/// 「T2 平 6 mm → T5 平 2 mm」(編集パネルの要約)。
pub fn describe(cx: &Context<'_>, all: &[Strategy], id: FeatureId) -> Option<String> {
    let c = chain(cx, all, id);
    let first = step(cx, all, *c.first()?)?;
    let lib = library(cx, first.tools);
    let names: Vec<String> = c
        .iter()
        .filter_map(|i| step(cx, all, *i))
        .map(|s| {
            let t = lib
                .as_ref()
                .and_then(|l| l.tools.iter().find(|t| t.number == s.tool));
            let mark = if s.id == id { "▶" } else { "" };
            match t {
                Some(t) => format!("{mark}T{} {}", t.number, t.name),
                None => format!("{mark}T{}", s.tool),
            }
        })
        .collect();
    Some(format!("工具の順番: {}", names.join(" → ")))
}

/// 選んだ操作に出す、工具の順番の操作。
pub fn actions(cx: &Context<'_>, all: &[Strategy]) -> Vec<Action> {
    let Some(sel) = cx.selected else {
        return vec![];
    };
    let Some(me) = step(cx, all, sel) else {
        return vec![];
    };
    let c = chain(cx, all, sel);
    let used: Vec<u32> = c
        .iter()
        .filter_map(|i| step(cx, all, *i))
        .map(|s| s.tool)
        .collect();
    let mut v = Vec::new();
    if let Some(lib) = library(cx, me.tools) {
        let last = used.last().copied().unwrap_or(me.tool);
        let previous = lib.tools.iter().find(|t| t.number == last);
        let type_id = &cx.data.feature(sel).unwrap().type_id;
        for t in lib.tools.iter().filter(|t| !used.contains(&t.number)) {
            // 元の種類が使えない工具は、同じ領域を引き継げる種類でも候補を調べる。
            let kinds = all
                .iter()
                .find(|s| {
                    (s.type_id == type_id || (s.tool_order && s.adopt.is_some()))
                        && s.rest.contains(&t.kind)
                })
                .map(|s| s.rest);
            let why = if kinds.is_none() {
                Some("この種類の工具は、この加工の取残しに使えません")
            } else if previous.is_some_and(|previous| {
                !kmb_cam_setup::can_follow_tool(t, previous, kinds.unwrap())
            }) {
                Some("順番の最後の工具より先の細い工具だけ足せる(前の工具が削り残した所を削るため)")
            } else {
                None
            };
            v.push(
                Action::new(
                    &format!("{ADD_TOOL}{}", t.number),
                    &format!("T{} {} を順番に足す", t.number, t.name),
                    "cam",
                )
                .menu("工具の順番")
                .icon("tool")
                .hint("前の工具が削り残した所だけを、この工具で削る操作を後ろに足す")
                .at(&[Place::Panel, Place::FeatureMenu])
                .disabled_if(why),
            );
        }
    }
    v
}

pub fn run(
    id: &str,
    cx: &Context<'_>,
    all: &[Strategy],
) -> Option<Result<Outcome, kmb_core::Problem>> {
    let sel = cx.selected;
    if let Some(n) = id.strip_prefix(ADD_TOOL) {
        return Some((|| {
            let n: u32 = n.parse().map_err(|_| "工具の番号が読めません")?;
            let sel = sel.ok_or("ポケットを選んでいません")?;
            let c = chain(cx, all, sel);
            let last = *c.last().ok_or("操作がありません")?;
            let s = step(cx, all, last).ok_or("工具の順番を持てない操作です")?;
            let f = cx.data.feature(last).ok_or("操作がありません")?;
            let kind = library(cx, s.tools).and_then(|l| l.get(n).map(|t| t.kind));
            let fits = |st: &Strategy| kind.is_none_or(|k| st.rest.contains(&k));
            let (type_id, mut d) = match all.iter().find(|st| st.type_id == f.type_id) {
                Some(me) if !fits(me) => {
                    // 元の種類が使えない工具(負荷制御粗取りの後の V・テーパー)は、その工具を使える
                    // 同じ領域の種類(ポケット)の残り削りにする
                    let other = all
                        .iter()
                        .find(|st| st.tool_order && fits(st) && st.adopt.is_some())
                        .ok_or("この工具で残り削りを足せる種類がありません")?;
                    let adopt = other.adopt.expect("adopt がある物を選んだ");
                    (other.type_id.to_string(), adopt(&f.data)?)
                }
                _ => (f.type_id.clone(), f.data.clone()),
            };
            make_rest(&mut d, s.tool, n);
            let root = cx
                .data
                .feature(c[0])
                .map_or(f.name.clone(), |r| r.name.clone());
            Ok(Outcome::insert(
                "工具の順番に足す",
                &type_id,
                format!("{root} の T{n}"),
                d,
            ))
        })());
    }
    None
}

/// 加工のデータ `d` を、前の工具 `prev` の削り残しを工具 `tool` で削るものにする。
/// 「工具の順番に足す」と「取残し加工」の両方が使う。
pub(crate) fn make_rest(d: &mut serde_json::Value, prev: u32, tool: u32) {
    d["tool"] = serde_json::json!(tool);
    d["rest_tool"] = serde_json::json!(prev);
    if let Some(o) = d.as_object_mut() {
        // 前の工具は番号で決まる(直径で打った前の工具は使わない)
        o.remove("rest_diameter");
        o.remove("rest_corner");
        // 仕上げと入る位置は、残り削りでは使わない
        o.remove("finish");
        o.remove("entry");
        // 切削条件の上書きは前の工具のもの。新しい工具は自分の条件で削る
        for k in ["rpm", "feed", "plunge", "stepdown", "stepover", "load"] {
            o.remove(k);
        }
    }
}
