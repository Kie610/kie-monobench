//! 干渉の確認と部品表。どちらも見えているボディ(ジョイントで動かした位置)で調べる。

use crate::fastener::{FASTENER_BODY, fastener_of, name_of};
use kmb_core::kernel::{
    BooleanOp, Kernel, KernelError, Mesh, P2, P3, Plane, dot, polygon_area, sub,
};
use kmb_core::{
    Context, FeatureId, Outcome, ShapeHandle, Table, TableColumn, TableMark, TableOption,
    body_labels, body_root,
};
use serde::Serialize;

/// 重なっているボディの組 1 つ(「一致した面を含める」なら、面で接するだけの組も)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Clash {
    /// 2 つのボディ(最後の機能)
    pub bodies: [FeatureId; 2],
    pub names: [String; 2],
    /// 重なった所の体積(mm³)。面で接するだけの組は 0
    pub volume: f64,
    /// 面で接するだけの組か
    pub touch: bool,
    /// 接する面の面積(mm²。面で接するだけの組だけ)
    pub area: f64,
    /// 3D のビューで色を付ける所(重なった立体か、接する面)
    #[serde(skip)]
    pub mark: TableMark,
}

/// 外形の箱が重なるか(`gap` だけ広げて比べる)。
fn boxes_meet(a: &(P3, P3), b: &(P3, P3), gap: f64) -> bool {
    (0..3).all(|i| a.0[i] <= b.1[i] + gap && b.0[i] <= a.1[i] + gap)
}

/// 重なりとして数えない体積(mm³)。面で接するだけの組(蓋が箱に載っている)を外す。
const TOUCH: f64 = 1e-3;

/// 色を付ける立体の網の細かさ(mm。3D のビューのボディと同じ)。
const MESH_TOLERANCE: f64 = 0.05;

/// 同じ平面とみなす面の間の隔たり(mm)。
const PLANE_GAP: f64 = 1e-4;

/// 接するとみなす面積(mm²)。辺や点で触れるだけの組を外す。
const TOUCH_AREA: f64 = 1e-6;

/// 網を三角形と稜線にする。
fn mark_of(m: &Mesh) -> TableMark {
    let p = |i: u32| m.positions[i as usize];
    TableMark {
        tris: m
            .triangles
            .iter()
            .map(|&[a, b, c]| [p(a), p(b), p(c)])
            .collect(),
        edges: m
            .edges
            .iter()
            .flat_map(|l| l.windows(2).map(|w| [w[0], w[1]]))
            .collect(),
    }
}

/// 凸の多角形 `poly` を、左回りの三角形 `t` の内側で切る(Sutherland–Hodgman)。
fn clip(mut poly: Vec<P2>, t: &[P2; 3]) -> Vec<P2> {
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        let side = |p: P2| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let mut out = Vec::with_capacity(poly.len() + 1);
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
            }
        }
        poly = out;
        if poly.len() < 3 {
            return Vec::new();
        }
    }
    poly
}

/// 2 つのボディが面で接する所: 向かい合う平らな面が同じ平面にある組の、重なった所の三角形と面積。
/// 曲がった面(穴と同じ径の軸など)は見ない。稜線は付けない(三角形の切れ目が面の中に線で出るため)。
fn contact(
    kernel: &dyn Kernel,
    a: ShapeHandle,
    b: ShapeHandle,
) -> Result<(TableMark, f64), KernelError> {
    let (fa, fb) = (kernel.faces(a)?, kernel.faces(b)?);
    let mut pairs: Vec<(&str, &str, Plane)> = Vec::new();
    for x in &fa {
        let Some(px) = &x.planar else { continue };
        for y in &fb {
            let Some(py) = &y.planar else { continue };
            if dot(px.normal, py.normal) < -1.0 + 1e-6
                && dot(sub(py.point, px.point), px.normal).abs() < PLANE_GAP
            {
                pairs.push((
                    &x.name,
                    &y.name,
                    Plane::from_point_normal(px.point, px.normal),
                ));
            }
        }
    }
    let mut mark = TableMark::default();
    let mut area = 0.0;
    if pairs.is_empty() {
        return Ok((mark, area));
    }
    let (ma, mb) = (
        kernel.mesh(a, MESH_TOLERANCE)?,
        kernel.mesh(b, MESH_TOLERANCE)?,
    );
    // 面の名前の三角形を、平面の上の左回りの 2D にする
    let flat = |m: &Mesh, face: &str, plane: &Plane| -> Vec<[P2; 3]> {
        let Some(f) = m.face_names.iter().position(|n| n == face) else {
            return Vec::new();
        };
        m.triangles
            .iter()
            .zip(&m.triangle_faces)
            .filter(|(_, tf)| **tf as usize == f)
            .map(|(t, _)| {
                let mut q = t.map(|i| plane.to_local(m.positions[i as usize].map(f64::from)));
                if polygon_area(&q) < 0.0 {
                    q.swap(1, 2);
                }
                q
            })
            .collect()
    };
    for (x, y, plane) in &pairs {
        let (ta, tb) = (flat(&ma, x, plane), flat(&mb, y, plane));
        for t in &ta {
            for u in &tb {
                let poly = clip(t.to_vec(), u);
                let s = polygon_area(&poly);
                if poly.len() < 3 || s <= 0.0 {
                    continue;
                }
                area += s;
                let w = |p: P2| plane.to_world(p).map(|v| v as f32);
                for i in 1..poly.len() - 1 {
                    let (p, q, r) = (w(poly[0]), w(poly[i]), w(poly[i + 1]));
                    // 両側から見えるよう表と裏を置く
                    mark.tris.push([p, q, r]);
                    mark.tris.push([p, r, q]);
                }
            }
        }
    }
    Ok((mark, area))
}

/// 調べられなかった組。名前と、カーネルが言った理由(読み込んだ形だけの
/// ボディは足し引きできない、など)。表には「調べていない」として理由と一緒に出す。
#[derive(Debug, Clone, PartialEq)]
pub struct Unchecked {
    pub names: [String; 2],
    pub reason: String,
}

/// 見えているボディどうしの重なり(重なった組と、調べられなかった組)。
pub fn interferences(cx: &Context<'_>) -> (Vec<Clash>, Vec<Unchecked>) {
    interferences_with(cx, false)
}

/// [`interferences`] に、`coincident` なら平らな面で接するだけの組(体積 0)も加える。
pub fn interferences_with(cx: &Context<'_>, coincident: bool) -> (Vec<Clash>, Vec<Unchecked>) {
    let bodies = cx.visible_bodies();
    let ids: Vec<FeatureId> = bodies.iter().map(|b| b.0).collect();
    // 部品に入ったボディは「部品の名前(ボディの名前)」で出す
    let names: Vec<String> = body_labels(cx.data, cx.registry, &ids)
        .into_iter()
        .zip(&ids)
        .map(
            |(n, id)| match cx.data.component_of(body_root(cx.data, cx.registry, *id)) {
                Some(c) => format!("{}({n})", c.name),
                None => n,
            },
        )
        .collect();
    let boxes: Vec<Option<(P3, P3)>> = bodies.iter().map(|b| cx.kernel.bounds(b.1).ok()).collect();
    let kernel: &dyn Kernel = &**cx.kernel;
    let mut clashes = Vec::new();
    let mut failed = Vec::new();
    for i in 0..bodies.len() {
        for j in i + 1..bodies.len() {
            if let (Some(a), Some(b)) = (&boxes[i], &boxes[j])
                && !boxes_meet(a, b, 1e-6)
            {
                continue;
            }
            let pair = [names[i].clone(), names[j].clone()];
            let clash = |volume, touch, area, mark| Clash {
                bodies: [ids[i], ids[j]],
                names: pair.clone(),
                volume,
                touch,
                area,
                mark,
            };
            let v = kernel
                .boolean(BooleanOp::Intersect, bodies[i].1, bodies[j].1)
                .and_then(|h| Ok((h, kernel.volume(h)?)));
            match v {
                Ok((h, v)) if v > TOUCH => {
                    let mark = kernel
                        .mesh(h, MESH_TOLERANCE)
                        .map(|m| mark_of(&m))
                        .unwrap_or_default();
                    clashes.push(clash(v, false, 0.0, mark));
                }
                Ok(_) if coincident => match contact(kernel, bodies[i].1, bodies[j].1) {
                    Ok((mark, area)) if area > TOUCH_AREA => {
                        clashes.push(clash(0.0, true, area, mark));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!(
                            "干渉: {} と {} の接する面を調べられない: {e}",
                            pair[0],
                            pair[1]
                        );
                        failed.push(Unchecked {
                            names: pair,
                            reason: e.to_string(),
                        });
                    }
                },
                Ok(_) => {}
                // 交わりの無い組でカーネルが誤りを返すことがあるので、箱が重なる時だけ知らせる
                Err(e) => {
                    log::warn!("干渉: {} と {} を調べられない: {e}", pair[0], pair[1]);
                    failed.push(Unchecked {
                        names: pair,
                        reason: e.to_string(),
                    });
                }
            }
        }
    }
    log::info!(
        "干渉: ボディ {} 個、重なり {} 組、面で接する組 {}、調べられない組 {}(一致した面を含める: {coincident})",
        bodies.len(),
        clashes.iter().filter(|c| !c.touch).count(),
        clashes.iter().filter(|c| c.touch).count(),
        failed.len()
    );
    (clashes, failed)
}

/// 干渉の表(グループ・体積・部品 1・部品 2)。
pub fn interference_table(
    clashes: &[Clash],
    failed: &[Unchecked],
    n: usize,
    coincident: bool,
) -> Table {
    let col = TableColumn::new;
    let columns = vec![
        col("group", "グループ", true, true),
        col("volume", "体積 mm³", true, true),
        col("body1", "部品 1", false, true),
        col("body2", "部品 2", false, true),
        col("kind", "種類", false, false),
        col("area", "接する面積 mm²", true, false),
    ];
    let rows = clashes
        .iter()
        .enumerate()
        .map(|(i, c)| {
            vec![
                (i + 1).to_string(),
                format!("{:.3}", c.volume),
                c.names[0].clone(),
                c.names[1].clone(),
                if c.touch { "面の一致" } else { "重なり" }.into(),
                if c.touch {
                    format!("{:.2}", c.area)
                } else {
                    String::new()
                },
            ]
        })
        .collect();
    let mut note = format!("見えているボディ {n} 個を、ジョイントで動かした位置で調べました。");
    if !coincident {
        note += "面で接するだけの組は数えません(「一致した面を含める」で出します)。";
    } else {
        note += "面の一致は平らな面どうしだけを見ます。";
    }
    // 調べられなかった組は、干渉が無いと読まれないよう理由と一緒に「調べていない」と書く
    for f in failed {
        note += &format!(
            "\n{} と {} は調べていません({})。",
            f.names[0], f.names[1], f.reason
        );
    }
    Table {
        key: crate::INTERFERENCE.into(),
        columns,
        rows,
        note,
        empty: "干渉は見つかりませんでした。".into(),
        options: vec![TableOption {
            id: "coincident".into(),
            label: "一致した面を含める".into(),
            hint: "平らな面どうしで接するだけの組も、体積 0 の行として出す".into(),
            on: coincident,
        }],
        marks: clashes.iter().map(|c| c.mark.clone()).collect(),
        marks_label: "すべての干渉を表示".into(),
        ..Table::default()
    }
}

pub(crate) fn interference_outcome(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let n = cx.visible_bodies().len();
    if n < 2 {
        return Err("ボディが 2 つ以上いります".into());
    }
    let coincident = cx
        .arg("coincident")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let (clashes, failed) = interferences_with(cx, coincident);
    Ok(Outcome::Table {
        title: "干渉".into(),
        table: interference_table(&clashes, &failed, n, coincident),
    })
}

/// 部品表の 1 行(同じ寸法と材料の物はまとめる)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BomRow {
    /// 部品かボディの名前(まとめた物は全部)
    pub names: Vec<String>,
    /// 「部品」「ボディ」(部品に入っていない)「規格部品」
    pub kind: String,
    pub count: usize,
    /// 物理材料(付けていなければ空、ボディごとに違えば「混在」)
    pub material: String,
    /// 外形の箱の辺を長い順に(長さ・幅・厚さ、mm)
    pub size: [f64; 3],
    /// 1 個の体積(mm³)
    pub volume: f64,
    /// 1 個の質量(g。材料の無いボディは既定の材料の鋼で数える。材料の名前が表に無ければ `None`)
    pub mass: Option<f64>,
}

/// ボディ `root` が規格部品なら、その名前と材料。
fn fastener_item(cx: &Context<'_>, root: FeatureId) -> Option<(String, String)> {
    let e = cx
        .data
        .feature(root)
        .filter(|e| e.type_id == FASTENER_BODY)?;
    let (gid, f) = fastener_of(cx.data, e.id)?;
    Some((name_of(cx, gid, &f), f.material.clone()))
}

/// 部品表の行にする物 1 つ(名前、種類、ボディの形と最初の機能、規格部品の材料)。
type Item = (
    String,
    &'static str,
    Vec<(ShapeHandle, FeatureId)>,
    Option<String>,
);

/// 部品表: 中にボディを持つ部品ごとと、部品に入っていないボディごとに 1 つ。規格部品は部品に
/// 入っていても 1 個ずつ数える。外形・体積・質量を測り、材料と寸法(0.1 mm)と体積(1 mm³)が同じ物を
/// 1 行にまとめて数える(カットリスト)。出現の部品は、形によらず元の部品の行で数える。外形の箱は世界の軸に沿った箱(斜めに置いた板は大きく出る)。
/// 部品表の行に対応する、文書の部品かボディ。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BomSource {
    Component { id: u64 },
    Body { root: FeatureId },
}

pub fn bom_rows(cx: &Context<'_>) -> Vec<BomRow> {
    bom_rows_with_sources(cx).0
}

/// 表の集約と同じ判断で、番号札が参照する部品・ボディを行へ対応付ける。
pub fn bom_rows_with_sources(
    cx: &Context<'_>,
) -> (Vec<BomRow>, std::collections::BTreeMap<BomSource, usize>) {
    let data = cx.data;
    let bodies = cx.visible_bodies();
    let ids: Vec<FeatureId> = bodies.iter().map(|b| b.0).collect();
    let labels = body_labels(data, cx.registry, &ids);
    let roots: Vec<FeatureId> = ids
        .iter()
        .map(|id| body_root(data, cx.registry, *id))
        .collect();
    let fasteners: Vec<Option<(String, String)>> =
        roots.iter().map(|r| fastener_item(cx, *r)).collect();
    let mut items: Vec<Item> = Vec::new();
    let mut sources = Vec::new();
    let mut source_rows = std::collections::BTreeMap::new();
    // 行ごとの部品(出現なら元の部品)。部品の行は、材料・寸法でなくこれでまとめる
    let mut keys: Vec<Option<u64>> = Vec::new();
    for c in &data.components {
        let shapes: Vec<(ShapeHandle, FeatureId)> = bodies
            .iter()
            .zip(&roots)
            .zip(&fasteners)
            .filter(|((_, r), f)| c.bodies.contains(r) && f.is_none())
            .map(|(((_, h), r), _)| (*h, *r))
            .collect();
        if !shapes.is_empty() {
            // 出現の部品は元の部品の行で数える
            let src = crate::occurrence::source_of(data, c.id);
            let name = src
                .and_then(|s| data.component(s))
                .map_or_else(|| c.name.clone(), |s| s.name.clone());
            keys.push(Some(src.unwrap_or(c.id)));
            items.push((name, "部品", shapes, None));
            sources.push(BomSource::Component { id: c.id });
        }
    }
    for (i, (_, h)) in bodies.iter().enumerate() {
        let r = roots[i];
        let before = items.len();
        match &fasteners[i] {
            Some((name, material)) => items.push((
                name.clone(),
                "規格部品",
                vec![(*h, r)],
                Some(material.clone()),
            )),
            None if data.component_of(r).is_none() => {
                items.push((labels[i].clone(), "ボディ", vec![(*h, r)], None));
            }
            None => {}
        }
        if items.len() != before {
            sources.push(BomSource::Body { root: r });
        }
    }
    let materials_of = kmb_look::material::of(data);
    keys.resize(items.len(), None);
    let mut rows: Vec<BomRow> = Vec::new();
    // 部品(出現なら元の部品)→ 数えた行
    let mut row_of: Vec<(u64, usize)> = Vec::new();
    for (((name, kind, shapes, fallback), key), source) in items.into_iter().zip(keys).zip(sources)
    {
        if let Some(&(_, i)) = key.and_then(|k| row_of.iter().find(|(c, _)| *c == k)) {
            rows[i].count += 1;
            source_rows.insert(source, i);
            for (_, root) in &shapes {
                source_rows.insert(BomSource::Body { root: *root }, i);
            }
            continue;
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut volume = 0.0;
        let mut mass = Some(0.0);
        let mut materials: Vec<String> = Vec::new();
        for (h, root) in &shapes {
            let Ok(m) = cx.kernel.mass(*h) else { continue };
            for i in 0..3 {
                lo[i] = lo[i].min(m.min[i]);
                hi[i] = hi[i].max(m.max[i]);
            }
            volume += m.volume;
            let mat = materials_of
                .get(*root)
                .map(str::to_string)
                .or_else(|| fallback.clone())
                .unwrap_or_else(|| kmb_look::material::DEFAULT_MATERIAL.to_string());
            mass = match (mass, kmb_look::material::phys_material(&mat)) {
                (Some(g), Some(p)) => Some(g + m.volume * p.density / 1000.0),
                _ => None,
            };
            if !materials.contains(&mat) {
                materials.push(mat);
            }
        }
        if lo[0] > hi[0] {
            continue;
        }
        let mut size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        size.sort_by(|a, b| b.total_cmp(a));
        let material = match materials.as_slice() {
            [m] => m.clone(),
            _ => "混在".into(),
        };
        let same = |r: &BomRow| {
            r.material == material
                && r.kind == kind
                && (0..3).all(|i| ((r.size[i] - size[i]) * 10.0).round() == 0.0)
                && (r.volume - volume).round() == 0.0
        };
        let at = rows.iter().position(same).unwrap_or(rows.len());
        source_rows.insert(source, at);
        for (_, root) in &shapes {
            source_rows.insert(BomSource::Body { root: *root }, at);
        }
        if let Some(k) = key {
            row_of.push((k, at));
        }
        match rows.get_mut(at) {
            Some(r) => {
                r.count += 1;
                if !r.names.contains(&name) {
                    r.names.push(name);
                }
            }
            None => rows.push(BomRow {
                names: vec![name],
                kind: kind.into(),
                count: 1,
                material,
                size,
                volume,
                mass,
            }),
        }
    }
    (rows, source_rows)
}

fn fmt_mass(m: Option<f64>) -> String {
    m.map_or_else(String::new, |g| format!("{g:.1}"))
}

/// 部品表の表(列を選べる窓に出す。CLI・MCP は既定の列の文)。
pub fn bom_table(rows: &[BomRow]) -> Table {
    let col = TableColumn::new;
    let columns = vec![
        col("item", "番号", true, true),
        col("name", "名前", false, true),
        col("kind", "種類", false, false),
        col("count", "数", true, true),
        col("material", "材料", false, true),
        col("length", "長さ mm", true, true),
        col("width", "幅 mm", true, true),
        col("thickness", "厚さ mm", true, true),
        col("volume", "体積 cm³", true, true),
        col("mass", "質量 g", true, true),
        col("total_mass", "合計質量 g", true, false),
    ];
    let rows = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            vec![
                (i + 1).to_string(),
                r.names.join("、"),
                r.kind.clone(),
                r.count.to_string(),
                r.material.clone(),
                format!("{:.1}", r.size[0]),
                format!("{:.1}", r.size[1]),
                format!("{:.1}", r.size[2]),
                format!("{:.2}", r.volume / 1000.0),
                fmt_mass(r.mass),
                fmt_mass(r.mass.map(|g| g * r.count as f64)),
            ]
        })
        .collect();
    Table {
        key: crate::BOM.into(),
        export: Some(crate::BOM_CSV.into()),
        columns,
        rows,
        note: "同じ種類・材料・寸法・体積の物は 1 行にまとめて数えます。寸法は世界の軸に沿った外形の箱です。質量は材料を付けたボディだけ出ます。".into(),
        empty: "ボディがありません".into(),
        ..Table::default()
    }
}

/// CSV の 1 つの値(RFC 4180)。
fn cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// 部品表の CSV(RFC 4180。表計算ソフトが UTF-8 と分かるように頭に BOM を付ける)。
pub fn bom_csv(rows: &[BomRow]) -> String {
    let mut t =
        String::from("\u{feff}番号,名前,数,材料,長さ(mm),幅(mm),厚さ(mm),体積(cm3),質量(g)\r\n");
    for (i, r) in rows.iter().enumerate() {
        let line = [
            (i + 1).to_string(),
            cell(&r.names.join("、")),
            r.count.to_string(),
            cell(&r.material),
            format!("{:.1}", r.size[0]),
            format!("{:.1}", r.size[1]),
            format!("{:.1}", r.size[2]),
            format!("{:.3}", r.volume / 1000.0),
            fmt_mass(r.mass),
        ];
        t += &line.join(",");
        t += "\r\n";
    }
    t
}
