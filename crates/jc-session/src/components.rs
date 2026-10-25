//! 部品の木の変更: 部品を中の部品ごと複製する、部品を別の部品の中や前後へ移す、ボディを部品へ
//! 移す。画面のブラウザ(ドラッグと右クリック)も CLI・MCP([`crate::Headless`])も、ここで組んだ
//! [`Transaction`] を通るので、どこから頼んでも同じ結果になる。

use crate::edit::Changes;
use jc_core::{
    Component, Document, DocumentData, FeatureEntry, FeatureId, Kernel, RecomputeReport, Registry,
    Transaction,
};
use std::collections::HashMap;

/// 複製した部品を横へずらす時の隙間(mm)。
pub const DUPLICATE_GAP: f64 = 10.0;

/// 部品を移す先。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// 部品の中(`None` は文書の直下)の最後へ
    Into(Option<u64>),
    /// 部品のすぐ前(その部品と同じ親)へ
    Before(u64),
    /// 部品のすぐ後ろ(その部品と同じ親)へ
    After(u64),
}

fn name_of(data: &DocumentData, id: u64) -> String {
    data.component(id)
        .map(|c| c.name.clone())
        .unwrap_or_default()
}

fn exists(data: &DocumentData, id: u64) -> Result<(), String> {
    data.component(id)
        .map(|_| ())
        .ok_or_else(|| format!("部品 {id} がありません"))
}

/// 部品 `id` を `at` へ移せるか。自分や自分の中へは移せない。今と同じ親の中へ移すのは変わらない。
pub fn can_move_component(data: &DocumentData, id: u64, at: Place) -> Result<(), String> {
    exists(data, id)?;
    let inner = data.component_tree(id);
    match at {
        Place::Into(to) => {
            if let Some(t) = to {
                exists(data, t)?;
                if inner.contains(&t) {
                    return Err(format!(
                        "部品「{}」を自分か自分の中の部品へは入れられません",
                        name_of(data, id)
                    ));
                }
            }
            if data.component(id).is_some_and(|c| c.parent == to) {
                return Err(format!("部品「{}」はもうそこにあります", name_of(data, id)));
            }
        }
        Place::Before(c) | Place::After(c) => {
            exists(data, c)?;
            if inner.contains(&c) {
                return Err(format!(
                    "部品「{}」を自分か自分の中の部品の前後へは移せません",
                    name_of(data, id)
                ));
            }
        }
    }
    Ok(())
}

/// 部品 `id` を中の部品ごと `at` へ移す(部品の木の並べ替え。1 回の取り消し単位)。並びが今と
/// 同じなら `None`。
pub fn move_component(
    data: &DocumentData,
    id: u64,
    at: Place,
) -> Result<Option<Transaction>, String> {
    can_move_component(data, id, at)?;
    let mut list = data.components.clone();
    let Some(i) = list.iter().position(|c| c.id == id) else {
        return Err(format!("部品 {id} がありません"));
    };
    let mut moving = list.remove(i);
    let label = match at {
        Place::Into(to) => {
            moving.parent = to;
            list.push(moving);
            match to {
                Some(t) => format!(
                    "部品「{}」を「{}」の中へ移す",
                    name_of(data, id),
                    name_of(data, t)
                ),
                None => format!("部品「{}」を文書の直下へ移す", name_of(data, id)),
            }
        }
        Place::Before(c) | Place::After(c) => {
            let Some(j) = list.iter().position(|x| x.id == c) else {
                return Err(format!("部品 {c} がありません"));
            };
            moving.parent = list[j].parent;
            let j = if matches!(at, Place::After(_)) {
                j + 1
            } else {
                j
            };
            list.insert(j, moving);
            format!("部品「{}」の並びを変える", name_of(data, id))
        }
    };
    if list == data.components {
        return Ok(None);
    }
    log::info!("部品の木を変える: {label}");
    Ok(Some(crate::edit::set_components(&label, list)))
}

/// ボディ(どれかの機能の番号。最初に作った機能へたどる)を部品 `to`(`None` は部品から外す)へ
/// 移す。どれも今と同じ所なら `None`。
pub fn move_bodies(
    data: &DocumentData,
    registry: &Registry,
    bodies: &[FeatureId],
    to: Option<u64>,
) -> Result<Option<Transaction>, String> {
    if bodies.is_empty() {
        return Err("移すボディがありません".into());
    }
    let mut roots = Vec::new();
    for b in bodies {
        if data.feature(*b).is_none() {
            return Err(format!("機能 {b} がありません"));
        }
        let r = jc_core::body_root(data, registry, *b);
        if !roots.contains(&r) {
            roots.push(r);
        }
    }
    if let Some(t) = to {
        exists(data, t)?;
    }
    let list = data.components_moving(&roots, to);
    if list == data.components {
        return Ok(None);
    }
    let label = match to {
        Some(t) => format!("ボディを部品「{}」へ移す", name_of(data, t)),
        None => "ボディを部品から外す".to_string(),
    };
    log::info!("部品の木を変える: {label}(ボディ {} 個)", roots.len());
    Ok(Some(crate::edit::set_components(&label, list)))
}

/// 部品 `id` を中の部品ごと複製する変更(1 回の取り消し単位)と、作った部品の番号。中のボディは
/// 元の形を追う写し(役割 `jc_interchange::role::kind::COPY_BODY` の機能)にし、新しい部品は元の
/// 部品のすぐ後ろ(同じ親)に置く。ボディがあれば、外形の X の幅と隙間だけ横へずらした位置(役割
/// `jc_interchange::role::kind::COMPONENT_POSITION` の機能)を足す。材料・外観・ボディの名前も写す。
pub fn duplicate_component(
    doc: &mut Document,
    registry: &Registry,
    kernel: &dyn Kernel,
    report: &RecomputeReport,
    id: u64,
) -> Result<(Transaction, u64), String> {
    // 写す機能は名前で呼ばず、記述子の役割で探す(層をまたいで種類の名前を書かない)
    let by_role = |role| {
        registry
            .descriptors()
            .into_iter()
            .find(|d| d.role == Some(role))
            .map(|d| d.type_id)
    };
    let copy_type = by_role(jc_interchange::role::kind::COPY_BODY)
        .ok_or("ボディを写す機能が組み込まれていないので、複製できません")?;
    let data = doc.data().clone();
    let orig = data
        .component(id)
        .cloned()
        .ok_or_else(|| format!("部品 {id} がありません"))?;
    let tree = data.component_tree(id);
    let visible = jc_core::visible_bodies(&data, registry, report);
    let lasts: Vec<FeatureId> = visible.iter().map(|(f, _)| *f).collect();
    let labels = jc_core::body_labels(&data, registry, &lasts);
    // ボディ(最初の機能)→(最後の機能、形、名前)
    let bodies: HashMap<FeatureId, (FeatureId, jc_core::ShapeHandle, String)> = visible
        .iter()
        .zip(labels)
        .map(|((last, h), label)| {
            (
                jc_core::body_root(&data, registry, *last),
                (*last, *h, label),
            )
        })
        .collect();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut ops = Changes::new();
    let mut materials = jc_look::material::of(&data);
    let mut looks = jc_look::appearance::of(&data);
    let (materials_before, looks_before) = (materials.clone(), looks.clone());
    let mut copies: HashMap<FeatureId, FeatureId> = HashMap::new();
    let mut new_ids = Vec::new();
    for c in &tree {
        let Some(comp) = data.component(*c) else {
            continue;
        };
        for root in &comp.bodies {
            let Some((last, h, label)) = bodies.get(root) else {
                continue;
            };
            if let Ok((a, b)) = kernel.bounds(*h) {
                for k in 0..3 {
                    lo[k] = lo[k].min(a[k]);
                    hi[k] = hi[k].max(b[k]);
                }
            }
            let nid = doc.alloc_feature_id();
            ops.insert_feature(
                None,
                FeatureEntry {
                    id: nid,
                    type_id: copy_type.clone(),
                    name: format!("写し({label})"),
                    data: serde_json::json!({"body": last.0, "copy": true}),
                    suppressed: false,
                },
            );
            if let Some(name) = data.body_names.get(root) {
                ops.rename_body(nid, Some(name.clone()));
            }
            if let Some(m) = materials.get(*root).map(str::to_string) {
                materials.set(nid, Some(&m));
            }
            if let Some(a) = looks.bodies.get(root).cloned() {
                looks.set_body(nid, Some(&a));
            }
            copies.insert(*root, nid);
            new_ids.push(nid);
        }
    }
    let comp_ids: HashMap<u64, u64> = tree.iter().map(|c| (*c, doc.alloc_group_id())).collect();
    let top = comp_ids[&id];
    let name = jc_core::unique_name(&orig.name, |t| data.components.iter().any(|c| c.name == t));
    let made: Vec<Component> = tree
        .iter()
        .filter_map(|c| data.component(*c))
        .map(|c| Component {
            id: comp_ids[&c.id],
            name: if c.id == id {
                name.clone()
            } else {
                c.name.clone()
            },
            parent: if c.id == id {
                c.parent
            } else {
                c.parent.and_then(|p| comp_ids.get(&p).copied())
            },
            bodies: c
                .bodies
                .iter()
                .filter_map(|b| copies.get(b).copied())
                .collect(),
        })
        .collect();
    let dx = hi[0] - lo[0];
    // 位置の機能も名前で呼ばず、記述子の役割で探す(組立のモジュールが無ければずらさない)
    if let Some(position_type) = by_role(jc_interchange::role::kind::COMPONENT_POSITION)
        && !new_ids.is_empty()
        && dx.is_finite()
    {
        let dx = ((dx + DUPLICATE_GAP) * 10.0).round() / 10.0;
        let n = data
            .features
            .iter()
            .filter(|f| f.type_id == position_type)
            .count()
            + 1;
        ops.insert_feature(
            None,
            FeatureEntry {
                id: doc.alloc_feature_id(),
                type_id: position_type,
                name: format!("位置{n}"),
                data: serde_json::json!({
                    "moving": {"kind": "component", "id": top},
                    "bodies": new_ids.iter().map(|f| f.0).collect::<Vec<_>>(),
                    "x": format!("{dx} mm"),
                    "y": "0 mm",
                    "z": "0 mm",
                }),
                suppressed: false,
            },
        );
    }
    let mut components = data.components.clone();
    let at = components
        .iter()
        .position(|c| c.id == id)
        .map_or(components.len(), |i| i + 1);
    components.splice(at..at, made);
    ops.set_components(components);
    // 写した部品の外観も写す
    for c in &tree {
        if let Some(a) = looks.components.get(c).cloned() {
            looks.set_component(comp_ids[c], Some(&a));
        }
    }
    if materials != materials_before {
        ops.set_section(&data, &materials)?;
    }
    if looks != looks_before {
        ops.set_section(&data, &looks)?;
    }
    log::info!(
        "部品「{}」を「{name}」に複製する(ボディ {} 個、部品 {} 個)",
        orig.name,
        new_ids.len(),
        tree.len()
    );
    let label = format!("部品「{}」を複製", orig.name);
    Ok((ops.into_transaction(label), top))
}
