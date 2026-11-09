//! 組立の質量特性: 部品の配置ごと・剛性グループごと・組立全体の質量・重心・慣性。
//! ボディの値はカーネルの質量の口(密度 1 の体積と重心まわりの慣性)に材料の密度を掛け、
//! 組は平行軸の定理([`kmb_core::inertia_about`])で組の重心のまわりへ移して足す。
//! 見えているボディ(ジョイントで動かした位置)で求める。

use kmb_core::kernel::{
    InertiaTensor, MassProps, P3, PrincipalInertia, inertia_about, principal_inertia,
};
use kmb_core::{Context, FeatureId, Table, TableColumn, body_labels, body_root};
use serde::Serialize;

/// 質量を持つ物 1 つの値。質量は g、重心は mm、慣性は重心のまわりの kg·mm²。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Mass {
    pub mass: f64,
    pub centroid: P3,
    /// 求められないカーネル(truck)や、慣性を求められないボディを含む組は `None`
    pub inertia: Option<InertiaTensor>,
}

/// 密度 g/cm³ を g/mm³ に直す係数。
const G_PER_MM3: f64 = 1e-3;
/// g·mm² を kg·mm² に直す係数。
const KG_PER_G: f64 = 1e-3;

impl Mass {
    /// カーネルの質量の値(密度 1)と密度(g/cm³)から。重心の無い値(面のボディ)は `None`。
    pub fn of_body(props: &MassProps, density: f64) -> Option<Self> {
        let centroid = props.centroid?;
        let rho = density * G_PER_MM3;
        Some(Self {
            mass: props.volume * rho,
            centroid,
            inertia: props
                .inertia
                .map(|t| t.map(|r| r.map(|v| v * rho * KG_PER_G))),
        })
    }

    /// 主軸と主慣性モーメント(kg·mm²)。
    pub fn principal(&self) -> Option<PrincipalInertia> {
        self.inertia.as_ref().map(principal_inertia)
    }
}

/// 物の組を 1 つに合わせる: 質量は和、重心は質量で重みを付けた平均、慣性は各物の慣性を
/// 組の重心のまわりへ移して(平行軸の定理)足す。空か質量が 0 なら `None`。
pub fn combine(parts: &[Mass]) -> Option<Mass> {
    let mass: f64 = parts.iter().map(|p| p.mass).sum();
    if parts.is_empty() || mass <= 0.0 {
        return None;
    }
    let c: P3 =
        std::array::from_fn(|i| parts.iter().map(|p| p.centroid[i] * p.mass).sum::<f64>() / mass);
    let inertia = parts.iter().try_fold([[0.0; 3]; 3], |mut acc, p| {
        // 慣性は kg·mm² なので、平行軸の足し分の質量も kg で渡す
        let t = inertia_about(p.inertia?, p.mass * KG_PER_G, p.centroid, c);
        for i in 0..3 {
            for j in 0..3 {
                acc[i][j] += t[i][j];
            }
        }
        Some(acc)
    });
    Some(Mass {
        mass,
        centroid: c,
        inertia,
    })
}

/// 質量特性の表の 1 行。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MassRow {
    pub name: String,
    /// 「組立全体」「部品」「剛性グループ」「ボディ」(部品に入っていない)
    pub kind: String,
    pub value: Mass,
    /// 質量を数えなかったボディの数(面のボディ・表に無い材料・測れない形)
    pub skipped: usize,
}

/// 見えているボディの質量(材料の無いボディは既定の材料の鋼。部品表と同じ)。
fn body_masses(cx: &Context<'_>) -> Vec<(FeatureId, String, Option<Mass>)> {
    let data = cx.data;
    let bodies = cx.visible_bodies();
    let ids: Vec<FeatureId> = bodies.iter().map(|b| b.0).collect();
    let labels = body_labels(data, cx.registry, &ids);
    let materials = kmb_look::material::of(data);
    bodies
        .iter()
        .zip(labels)
        .map(|((id, h), label)| {
            let root = body_root(data, cx.registry, *id);
            let mat = materials
                .get(root)
                .map(str::to_string)
                .or_else(|| crate::inspect::fastener_item(cx, root).map(|(_, m)| m))
                .unwrap_or_else(|| kmb_look::material::DEFAULT_MATERIAL.to_string());
            let m = kmb_look::material::phys_material(&mat).and_then(|p| {
                let props = cx.kernel.mass(*h).ok()?;
                Mass::of_body(&props, p.density)
            });
            (root, label, m)
        })
        .collect()
}

/// 質量特性の行: 組立全体、部品ごと(下の部品のボディも含む。出現の部品も置いた所ごとに 1 行)、
/// 剛性グループごと、部品に入っていないボディごと。
pub fn mass_rows(cx: &Context<'_>) -> Vec<MassRow> {
    let bodies = body_masses(cx);
    let row = |name: String, kind: &str, roots: &dyn Fn(FeatureId) -> bool| {
        let picked: Vec<&Option<Mass>> = bodies
            .iter()
            .filter(|(r, _, _)| roots(*r))
            .map(|(_, _, m)| m)
            .collect();
        let parts: Vec<Mass> = picked.iter().filter_map(|m| **m).collect();
        let value = combine(&parts)?;
        Some(MassRow {
            name,
            kind: kind.into(),
            value,
            skipped: picked.len() - parts.len(),
        })
    };
    let data = cx.data;
    let mut rows: Vec<MassRow> = Vec::new();
    rows.extend(row("組立全体".into(), "組立全体", &|_| true));
    for c in &data.components {
        let inside = data.component_bodies(c.id);
        rows.extend(row(c.name.clone(), "部品", &|r| inside.contains(&r)));
    }
    let groups = crate::rigid::groups(cx);
    for ((id, _), roots) in crate::rigid::group_data(cx).iter().zip(&groups) {
        let name = data
            .feature(*id)
            .map_or_else(|| "剛性グループ".into(), |f| f.name.clone());
        rows.extend(row(name, "剛性グループ", &|r| roots.contains(&r)));
    }
    for (root, label, _) in &bodies {
        if data.component_of(*root).is_none() {
            rows.extend(row(label.clone(), "ボディ", &|r| r == *root));
        }
    }
    rows
}

fn fmt(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    // -0.000 を 0.000 にそろえる
    if s.trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        s.trim_start_matches('-').to_string()
    } else {
        s
    }
}

/// 質量特性の表(列を選べる窓に出す。CLI・MCP は既定の列の文)。
pub fn mass_table(rows: &[MassRow]) -> Table {
    let col = TableColumn::new;
    let columns = vec![
        col("name", "名前", false, true),
        col("kind", "種類", false, true),
        col("mass", "質量 g", true, true),
        col("cx", "重心 X mm", true, true),
        col("cy", "重心 Y mm", true, true),
        col("cz", "重心 Z mm", true, true),
        col("i1", "主慣性 1 kg·mm²", true, true),
        col("i2", "主慣性 2 kg·mm²", true, true),
        col("i3", "主慣性 3 kg·mm²", true, true),
        col("ixx", "Ixx kg·mm²", true, false),
        col("iyy", "Iyy kg·mm²", true, false),
        col("izz", "Izz kg·mm²", true, false),
        col("ixy", "Ixy kg·mm²", true, false),
        col("iyz", "Iyz kg·mm²", true, false),
        col("izx", "Izx kg·mm²", true, false),
        col("skipped", "数えないボディ", true, false),
    ];
    let rows = rows
        .iter()
        .map(|r| {
            let v = &r.value;
            let p = v.principal();
            let moment = |i: usize| {
                p.as_ref()
                    .map_or_else(String::new, |p| fmt(p.moments[i], 3))
            };
            let t = |i: usize, j: usize| v.inertia.map_or_else(String::new, |t| fmt(t[i][j], 3));
            vec![
                r.name.clone(),
                r.kind.clone(),
                fmt(v.mass, 1),
                fmt(v.centroid[0], 3),
                fmt(v.centroid[1], 3),
                fmt(v.centroid[2], 3),
                moment(0),
                moment(1),
                moment(2),
                t(0, 0),
                t(1, 1),
                t(2, 2),
                t(0, 1),
                t(1, 2),
                t(2, 0),
                r.skipped.to_string(),
            ]
        })
        .collect();
    Table {
        key: crate::MASS_PROPS.into(),
        columns,
        rows,
        note: "慣性は各行の重心のまわり(主慣性は小さい順)。材料の無いボディは鋼で数えます。面のボディ・表に無い材料のボディは数えません。慣性を求められない形状カーネルでは慣性の列が空になります。".into(),
        empty: "ボディがありません".into(),
        ..Table::default()
    }
}
