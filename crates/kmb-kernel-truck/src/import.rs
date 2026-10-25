//! STEP の読み込み。truck の STEP の曲面は truck-modeling の曲面と型が違い、そのままでは
//! ブーリアンに使えないので、読み込んだ形は三角形の網として持つ。加工(網から作る)、
//! 体積の計測、表示には使えるが、足し引き・移動・フィレットには使えない。

use crate::{bounds, signed_volume, tri_area, triangles};
use kmb_core::kernel::{KernelError, P3};
use truck_meshalgo::prelude::*;
use truck_stepio::r#in::{Table, alias, ruststep};
use truck_topology::compress::{CompressedEdge, CompressedEdgeIndex};

/// 読み込んだ形(面ごとの網、向きは外向きにそろえ済み)。
#[derive(Clone)]
pub(crate) struct Imported {
    pub meshes: Vec<Option<PolygonMesh>>,
    pub names: Vec<String>,
    /// 面の境目の辺を折れ線にしたもの(表示と、測る道具の点・辺に使う)
    pub edges: Vec<Vec<[f32; 3]>>,
    pub min: P3,
    pub max: P3,
    pub volume: f64,
    pub area: f64,
}

/// 読み込みを待つ時間の上限。
pub(crate) const IMPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// 網を作る時の許容差(mm)。
pub(crate) const IMPORT_TOLERANCE: f64 = 0.01;

/// STEP の文字列から、殻ごとに網を作る。
pub(crate) fn read_step(text: &str) -> Result<Imported, KernelError> {
    let exchange = ruststep::parser::parse(text)
        .map_err(|e| KernelError::Failed(format!("STEP を読めません: {e}")))?;
    let data = exchange
        .data
        .first()
        .ok_or_else(|| KernelError::Failed("STEP にデータの節がありません".into()))?;
    let table = Table::from_data_section(data);
    let voids = void_shells(data);
    let mut shells: Vec<_> = table.shell.iter().collect();
    shells.sort_by_key(|(i, _)| **i);
    if shells.is_empty() {
        return Err(KernelError::Failed(
            "STEP に形(殻)がありません。面だけのデータや、アセンブリの参照だけのファイルは読めません".into(),
        ));
    }
    let mut meshes = Vec::new();
    let mut names = Vec::new();
    let mut edges = Vec::new();
    for (n, (id, shell)) in shells.into_iter().enumerate() {
        let cshell = table
            .to_compressed_shell(shell)
            .map_err(|e| KernelError::Failed(format!("STEP の殻 {n} を変換できません: {e}")))?;
        let meshed = cshell.robust_triangulation(IMPORT_TOLERANCE);
        // 円柱などの閉じた曲面の継ぎ目(1 つの面の境界に 2 回出る辺)は、曲面の作り方の都合で
        // できる線で、形の上に特別な所は無い。描かない(選ぶ・吸い付く対象にもしない)
        let mut seams = std::collections::HashSet::new();
        for f in &meshed.faces {
            let mut seen = std::collections::HashSet::new();
            for e in f.boundaries.iter().flatten() {
                if !seen.insert(e.index) {
                    seams.insert(e.index);
                }
            }
        }
        // 辺は網を作る時に折れ線になっている(隣り合う面の網と同じ点を通る)
        for (i, e) in meshed.edges.iter().enumerate() {
            if seams.contains(&i) {
                continue;
            }
            let line: Vec<[f32; 3]> = e
                .curve
                .iter()
                .map(|q| [q.x as f32, q.y as f32, q.z as f32])
                .collect();
            if line.len() >= 2 {
                edges.push(line);
            }
        }
        // 先が 1 点にすぼまる円錐の面は truck の網が崩れる(頂点の側の縁が無く、v の範囲も閉じないので
        // 面の範囲を囲めない)ので、縁から頂点へ扇に張る
        let fans: Vec<Option<PolygonMesh>> = cshell
            .faces
            .iter()
            .zip(&meshed.faces)
            .map(|(f, m)| cone_fan(&f.surface, &m.boundaries, &meshed.edges))
            .collect();
        let mut shell_meshes: Vec<Option<PolygonMesh>> = meshed
            .faces
            .into_iter()
            .zip(fans)
            .map(|(f, fan)| {
                fan.or(f.surface).map(|mut m| {
                    if !f.orientation {
                        m.invert();
                    }
                    m
                })
            })
            .collect();
        // 外側の殻は外向き、空洞の殻は空洞の側(内向き)にそろえる。向きは殻の印でなく
        // 囲む体積の符号で決める(印は書いた CAD により逆のことがある)
        if (signed_volume(&shell_meshes) < 0.0) != voids.contains(id) {
            for m in shell_meshes.iter_mut().flatten() {
                m.invert();
            }
        }
        for i in 0..shell_meshes.len() {
            names.push(format!("step{n}:f{i}"));
        }
        meshes.extend(shell_meshes);
    }
    if meshes.iter().all(|m| m.is_none()) {
        return Err(KernelError::Failed(
            "STEP の面の網を作れませんでした".into(),
        ));
    }
    let (min, max) = bounds(&meshes);
    let volume = signed_volume(&meshes);
    let area = meshes
        .iter()
        .flatten()
        .flat_map(triangles)
        .map(|t| tri_area(&t))
        .sum();
    Ok(Imported {
        meshes,
        names,
        edges,
        min,
        max,
        volume,
        area,
    })
}

/// 空洞の殻(BREP_WITH_VOIDS の 3 つ目の値に並ぶ殻)の番号。truck-stepio の表は殻
/// (CLOSED_SHELL)しか読まず、どれが空洞かを持たないので、データの節から直接拾う。空洞は
/// ORIENTED_CLOSED_SHELL(名前, *, 殻, 向きの印)で指すのが決まりだが、殻を直接指す物も受ける。
/// 向きの印は使わない。
fn void_shells(data: &ruststep::ast::DataSection) -> std::collections::HashSet<u64> {
    use ruststep::ast::{EntityInstance, Name, Parameter};
    let records: std::collections::HashMap<u64, (&str, &[Parameter])> = data
        .entities
        .iter()
        .filter_map(|e| match e {
            EntityInstance::Simple { id, record } => match &record.parameter {
                Parameter::List(p) => Some((*id, (record.name.as_str(), p.as_slice()))),
                _ => None,
            },
            EntityInstance::Complex { .. } => None,
        })
        .collect();
    let target = |p: &Parameter| match p {
        Parameter::Ref(Name::Entity(i)) => Some(*i),
        _ => None,
    };
    let mut out = std::collections::HashSet::new();
    for (name, params) in records.values() {
        if !name.eq_ignore_ascii_case("BREP_WITH_VOIDS") {
            continue;
        }
        let Some(Parameter::List(list)) = params.get(2) else {
            continue;
        };
        for v in list.iter().filter_map(target) {
            match records.get(&v) {
                Some((n, p)) if n.eq_ignore_ascii_case("ORIENTED_CLOSED_SHELL") => {
                    out.extend(p.get(2).and_then(target));
                }
                _ => {
                    out.insert(v);
                }
            }
        }
    }
    out
}

/// 先が 1 点(頂点)にすぼまる円錐の面を、縁から頂点へ扇に張った網。円錐は頂点を通る直線の集まりなので、
/// 縁の 1 区間と頂点の三角形は面の上にあり、ずれは縁の折れ線と同じだけになる。継ぎ目(面の縁に 2 回
/// 出る辺)を除いた縁が 1 つの輪の時だけ作り(円錐台など縁が 2 つの面は truck の網のまま)、
/// 三角形の向きは面の法線(STEP の向き。面の印は後で当てる)にそろえる。
fn cone_fan(
    surface: &alias::Surface,
    boundaries: &[Vec<CompressedEdgeIndex>],
    edges: &[CompressedEdge<PolylineCurve<Point3>>],
) -> Option<PolygonMesh> {
    let alias::Surface::ElementarySurface(e) = surface else {
        return None;
    };
    let alias::ElementarySurface::ConicalSurface(cone) = e.as_ref() else {
        return None;
    };
    let mut count = std::collections::HashMap::new();
    for e in boundaries.iter().flatten() {
        *count.entry(e.index).or_insert(0) += 1;
    }
    let mut rings: Vec<Vec<Point3>> = boundaries
        .iter()
        .map(|wire| {
            let mut ring: Vec<Point3> = Vec::new();
            for e in wire.iter().filter(|e| count[&e.index] == 1) {
                let mut pts: Vec<Point3> = edges.get(e.index)?.curve.iter().copied().collect();
                if !e.orientation {
                    pts.reverse();
                }
                if ring
                    .last()
                    .is_some_and(|q| pts.first().is_some_and(|p| p.near(q)))
                {
                    pts.remove(0);
                }
                ring.extend(pts);
            }
            Some(ring)
        })
        .collect::<Option<Vec<_>>>()?;
    rings.retain(|r| r.len() >= 3);
    let [ring] = rings.as_mut_slice() else {
        return None;
    };
    if ring.first()?.near(ring.last()?) {
        ring.pop();
    }
    if ring.len() < 3 {
        return None;
    }
    // 頂点: 1/4 回り違えた 2 本の母線(回る向きの値を止めて、もう一方の値を動かした直線)の交わり。
    // 回る向きは周期のある方
    let angle_is_u = cone.u_period().is_some();
    let line = |a: f64| {
        let at = |t: f64| {
            if angle_is_u {
                cone.subs(a, t)
            } else {
                cone.subs(t, a)
            }
        };
        (at(0.0), at(1.0))
    };
    let (a0, a1) = line(0.0);
    let (b0, b1) = line(std::f64::consts::FRAC_PI_2);
    let apex = closest_point(a0, a1 - a0, b0, b1 - b0)?;
    if ring.iter().any(|p| p.distance(apex) < 1e-6) {
        return None;
    }
    // 面の法線の向き(縁の 1 点で面に問い合わせる)
    let (u, v) = cone.search_parameter(ring[0], None, 100)?;
    let want = cone.normal(u, v);
    let n = ring.len();
    let tri_normal = |i: usize| {
        let (p, q) = (ring[i], ring[(i + 1) % n]);
        (q - p).cross(apex - p)
    };
    let total = (0..n).fold(Vector3::zero(), |s, i| s + tri_normal(i));
    let flip = total.dot(want) < 0.0;
    let mut positions = ring.clone();
    positions.push(apex);
    let mut normals: Vec<Vector3> = (0..n)
        .map(|i| {
            let t = ring[(i + 1) % n] - ring[(i + n - 1) % n];
            let m = t.cross(apex - ring[i]);
            if flip { -m } else { m }.normalize()
        })
        .collect();
    let mut tris = Vec::with_capacity(n);
    for i in 0..n {
        let j = (i + 1) % n;
        let m = tri_normal(i);
        if m.magnitude2() < 1e-24 {
            continue;
        }
        normals.push(if flip { -m } else { m }.normalize());
        let at = |pos: usize, nor: usize| StandardVertex {
            pos,
            uv: None,
            nor: Some(nor),
        };
        let top = at(n, normals.len() - 1);
        tris.push(if flip {
            [at(j, j), at(i, i), top]
        } else {
            [at(i, i), at(j, j), top]
        });
    }
    Some(PolygonMesh::new(
        StandardAttributes {
            positions,
            uv_coords: Vec::new(),
            normals,
        },
        Faces::from_iter(tris.iter()),
    ))
}

/// 直線 p + s d と q + t e のいちばん近い所の中点(平行なら無し)。
fn closest_point(p: Point3, d: Vector3, q: Point3, e: Vector3) -> Option<Point3> {
    let w = p - q;
    let (a, b, c) = (d.dot(d), d.dot(e), e.dot(e));
    let den = a * c - b * b;
    if den.abs() < 1e-12 * a * c {
        return None;
    }
    let (dd, ee) = (d.dot(w), e.dot(w));
    let s = (b * ee - c * dd) / den;
    let t = (a * ee - b * dd) / den;
    Some(Point3::from_vec(
        ((p + d * s).to_vec() + (q + e * t).to_vec()) / 2.0,
    ))
}
