//! STEP の読み込み。truck の STEP の曲面は truck-modeling の曲面と型が違い、そのままでは
//! ブーリアンに使えないので、読み込んだ形は三角形の網として持つ。加工(網から作る)、
//! 体積の計測、表示と移動に使える。面の形を持たず、足し引きやフィレットには使えない。

use crate::{bounds, signed_volume, tri_area, triangles};
use kmb_core::kernel::{KernelError, P3};
use truck_meshalgo::prelude::*;
use truck_stepio::r#in::{Table, alias, ruststep};
use truck_topology::compress::{CompressedEdge, CompressedEdgeIndex};

/// 読み込んだ形(面ごとの網、向きは外向きにそろえ済み)。
#[derive(Clone)]
pub(crate) struct Imported {
    /// 三角形の網を直接預かった時は、頂点・法線・表示の辺を分け直さずに保つ。
    pub raw: Option<kmb_core::Mesh>,
    pub meshes: Vec<Option<PolygonMesh>>,
    pub names: Vec<String>,
    /// 面の境目の辺を折れ線にしたもの(表示と、測る道具の点・辺に使う)
    pub edges: Vec<Vec<[f32; 3]>>,
    pub min: P3,
    pub max: P3,
    pub volume: f64,
    pub area: f64,
    pub open_edges: usize,
    pub centroid: Option<P3>,
}

/// 読み込みを待つ時間の上限。
pub(crate) const IMPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// 網を作る時の許容差(mm)。
pub(crate) const IMPORT_TOLERANCE: f64 = 0.01;

/// STEP の文字列から、殻ごとに網を作る。長さはファイルの単位(inch・m など)から mm に直す
/// (読めない単位はファイルの数を mm とみなす。注意は読み込みの機能が出す)。閉じた殻と一緒に
/// 置かれた OPEN_SHELL(面だけのボディ。公差の補助の面など)は網に入れない(番号は数える)。
pub(crate) fn read_step(text: &str) -> Result<Imported, KernelError> {
    let unit = kmb_format::step::length_unit(text).mm;
    let data = &data_section(text)?;
    let table = Table::from_data_section(data);
    let voids = void_shells(data);
    let open = open_shells(data);
    let mut shells: Vec<_> = table.shell.iter().collect();
    shells.sort_by_key(|(i, _)| **i);
    if shells.is_empty() {
        return Err(KernelError::Failed(
            "STEP に形(殻)がありません。面だけのデータや、アセンブリの参照だけのファイルは読めません".into(),
        ));
    }
    let solid = shells.iter().any(|(id, _)| !open.contains(id));
    let mut meshes = Vec::new();
    let mut names = Vec::new();
    let mut edges = Vec::new();
    for (n, (id, shell)) in shells.into_iter().enumerate() {
        if solid && open.contains(id) {
            continue;
        }
        let cshell = table
            .to_compressed_shell(shell)
            .map_err(|e| KernelError::Failed(format!("STEP の殻 {n} を変換できません: {e}")))?;
        // 網はファイルの単位で作るので、許容差もファイルの単位にする。ただし 1e-4 より細かくしない
        // (m の STEP で 0.01 mm を求めると、40 mm の板 1 枚でも網づくりに数十秒かかる。m の物は
        // 0.1 mm の網になる)
        let meshed = cshell.robust_triangulation((IMPORT_TOLERANCE / unit).max(1e-4));
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
                .map(|q| (q * unit).map(|v| v as f32).into())
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
        if unit != 1.0 {
            for m in shell_meshes.iter_mut().flatten() {
                for p in m.positions_mut() {
                    *p *= unit;
                }
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
    let mut geometry = kmb_format::mesh::Geometry::default();
    for t in meshes.iter().flatten().flat_map(triangles) {
        let base = geometry.vertices.len() as u32;
        geometry.vertices.extend(t.map(|p| p.map(|x| x as f32)));
        geometry.triangles.push([base, base + 1, base + 2]);
    }
    let (open_edges, _, _, centroid) =
        kmb_format::mesh::properties(&geometry).map_err(KernelError::Failed)?;
    Ok(Imported {
        raw: None,
        meshes,
        names,
        edges,
        min,
        max,
        volume,
        area,
        open_edges,
        centroid,
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

/// truck の表が読む実体(形の実体)と、空洞を拾うのに使う実体。これらが読めない時は読み込みを止める。
const SHAPE_ENTITIES: &[&str] = &[
    "CARTESIAN_POINT",
    "DIRECTION",
    "VECTOR",
    "PLACEMENT",
    "AXIS1_PLACEMENT",
    "AXIS2_PLACEMENT_2D",
    "AXIS2_PLACEMENT_3D",
    "LINE",
    "POLYLINE",
    "B_SPLINE_CURVE_WITH_KNOTS",
    "BEZIER_CURVE",
    "QUASI_UNIFORM_CURVE",
    "UNIFORM_CURVE",
    "RATIONAL_B_SPLINE_CURVE",
    "CIRCLE",
    "ELLIPSE",
    "HYPERBOLA",
    "PARABOLA",
    "PCURVE",
    "SURFACE_CURVE",
    "SEAM_CURVE",
    "PLANE",
    "SPHERICAL_SURFACE",
    "CYLINDRICAL_SURFACE",
    "TOROIDAL_SURFACE",
    "CONICAL_SURFACE",
    "B_SPLINE_SURFACE_WITH_KNOTS",
    "UNIFORM_SURFACE",
    "QUASI_UNIFORM_SURFACE",
    "BEZIER_SURFACE",
    "RATIONAL_B_SPLINE_SURFACE",
    "SURFACE_OF_LINEAR_EXTRUSION",
    "SURFACE_OF_REVOLUTION",
    "VERTEX_POINT",
    "EDGE_CURVE",
    "ORIENTED_EDGE",
    "EDGE_LOOP",
    "FACE_BOUND",
    "FACE_OUTER_BOUND",
    "FACE_SURFACE",
    "ADVANCED_FACE",
    "ORIENTED_FACE",
    "OPEN_SHELL",
    "CLOSED_SHELL",
    "ORIENTED_SHELL",
    "ORIENTED_CLOSED_SHELL",
    "MANIFOLD_SOLID_BREP",
    "BREP_WITH_VOIDS",
    "DEFINITIONAL_REPRESENTATION",
];

/// データの節を読む。truck の使う字句の読み手は、空の並び `()` など決まりどおりの書き方を読めない
/// 所があり(公差・注記の実体に多い)、1 つでも読めないと全体を読めず、誤りの場所もデータの節の頭を
/// 指す。全体で読めない時は実体を 1 つずつ読み、形に関わらない実体は飛ばし、形の実体(複合の実体を
/// 含む)が読めない時はその行を返す。
fn data_section(text: &str) -> Result<ruststep::ast::DataSection, KernelError> {
    if let Ok(mut ex) = ruststep::parser::parse(text)
        && !ex.data.is_empty()
    {
        return Ok(ex.data.swap_remove(0));
    }
    let mut entities = Vec::new();
    for (at, inst) in kmb_format::step::instances_at(text) {
        let src = format!("{inst};");
        match ruststep::parser::exchange::entity_instance(&src) {
            Ok((rest, e)) if rest.trim().is_empty() => entities.push(e),
            _ => {
                let name = kmb_format::step::keyword(inst);
                if name.is_empty() || SHAPE_ENTITIES.contains(&name) {
                    let line = kmb_format::step::line_at(text, at);
                    let id = inst.split('=').next().unwrap_or("").trim();
                    let what = if name.is_empty() {
                        "複合の実体"
                    } else {
                        name
                    };
                    return Err(KernelError::Failed(format!(
                        "STEP の {line} 行目の {id}({what})を読めません"
                    )));
                }
                // 形に関わらない実体(公差・注記・表示)は網に使わないので飛ばす
            }
        }
    }
    if entities.is_empty() {
        return Err(KernelError::Failed("STEP にデータの節がありません".into()));
    }
    Ok(ruststep::ast::DataSection {
        meta: Vec::new(),
        entities,
    })
}

/// OPEN_SHELL の番号。
fn open_shells(data: &ruststep::ast::DataSection) -> std::collections::HashSet<u64> {
    use ruststep::ast::EntityInstance;
    data.entities
        .iter()
        .filter_map(|e| match e {
            EntityInstance::Simple { id, record }
                if record.name.eq_ignore_ascii_case("OPEN_SHELL") =>
            {
                Some(*id)
            }
            _ => None,
        })
        .collect()
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
