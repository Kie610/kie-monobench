//! truck による形状カーネル。kmb-core の [`Kernel`] を実装する。
//!
//! 1 つの形状(ボディ)は、つながっていない複数のソリッドを持てる。truck の
//! ブーリアンは、つながっていないソリッドを 1 つにまとめると誤った結果を返すため、
//! ソリッドを分けて持ち、外形の箱が重なる組だけに演算をかける。
//!
//! 面の名前は、面の幾何(曲面の定義)を鍵にして入力から引き継ぐ。truck の
//! ブーリアンは入力の曲面をそのまま使うので、鍵が一致する。1 つの面が分かれた時は
//! 位置の順に `#0`、`#1` … を付ける。

mod import;
mod parts;

use kmb_core::kernel::{
    self as k, BooleanOp, EdgeInfo, FaceInfo, Kernel, KernelError, MassProps, Mesh, P3, PlanarFace,
    Profile, ProfileLoop, Region, Seam, Segment, ShapeHandle, dedupe_names,
};
use parts::{intersect_part, subtract_part, union_part};
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use truck_meshalgo::prelude::*;
use truck_modeling as tm;

/// ブーリアンの許容差(mm)の候補。最初は truck の例で使われている値(A)。
/// 失敗したら次の値で試す。
const BOOLEAN_TOLERANCES: [f64; 3] = [0.05, 0.08, 0.03];

fn t_and(a: &tm::Solid, b: &tm::Solid) -> Option<tm::Solid> {
    BOOLEAN_TOLERANCES
        .iter()
        .find_map(|&tol| truck_shapeops::and(a, b, tol))
}

fn t_or(a: &tm::Solid, b: &tm::Solid) -> Option<tm::Solid> {
    BOOLEAN_TOLERANCES
        .iter()
        .find_map(|&tol| truck_shapeops::or(a, b, tol))
}
/// 外形の箱と面の情報を求める時のテッセレーションの許容差(mm)。
const COARSE_TOLERANCE: f64 = 0.05;
/// 体積と表面積を求める時のテッセレーションの許容差(mm)。これより細かくすると、
/// truck が小さな面の網を誤って作り、体積が大きくずれることがあった(0.002 mm で実測)。
/// 誤差はほぼ許容差に比例するので、2 倍の許容差の値と組み合わせて外挿する。
const MASS_TOLERANCE: f64 = 0.01;

#[derive(Clone)]
struct Part {
    solid: tm::Solid,
    /// 面ごとの名前。`faces_of(&solid)` の順。
    names: Vec<String>,
    min: P3,
    max: P3,
    /// 粗い網で求めた体積。ブーリアンの結果の検査に使う
    volume: f64,
    /// 面ごとの粗い網(`COARSE_TOLERANCE`、`faces_of(&solid)` の順)。面の列挙で作り直さない
    meshes: Arc<Vec<Option<PolygonMesh>>>,
}

#[derive(Clone, Default)]
struct Body {
    parts: Vec<Part>,
}

impl Body {
    fn volume(&self) -> f64 {
        self.parts.iter().map(|p| p.volume).sum()
    }
}

/// truck のブーリアンは、失敗を返さずに誤った形を返すことがある(実測)。
/// 体積が取りうる範囲に入っているかを確かめる。
fn check_volume(op: BooleanOp, target: f64, tool: f64, result: f64) -> Result<(), KernelError> {
    let eps = 1e-3 * target.max(tool) + 1e-6;
    let (lo, hi) = match op {
        BooleanOp::Subtract => ((target - tool).max(0.0), target),
        BooleanOp::Union => (target.max(tool), target + tool),
        BooleanOp::Intersect => (0.0, target.min(tool)),
    };
    if result < lo - eps || result > hi + eps {
        return Err(KernelError::Failed(format!(
            "ブーリアンの結果が不正です(体積 {result:.3} が {lo:.3}〜{hi:.3} の外)"
        )));
    }
    Ok(())
}

#[derive(Default)]
struct Store {
    next: u64,
    bodies: HashMap<u64, Body>,
    /// STEP から読み込んだ形(網だけを持つ)
    imported: HashMap<u64, Arc<import::Imported>>,
    /// 作った網(形と許容差ごと)。形は作った後に変わらないので、同じ網を加工ごとに作り直さない
    meshes: HashMap<(u64, u64), Mesh>,
    /// 求めた体積と表面積(形ごと)。細かい網を 2 回作るので、同じ形で求め直さない
    mass: HashMap<u64, MassProps>,
}

/// truck による [`Kernel`] の実装。
#[derive(Default)]
pub struct TruckKernel {
    store: Mutex<Store>,
    /// 出現の表([`Kernel::instance`])。出現は写して動かした形を持つ(B-rep は共有しない)
    instances: Mutex<k::Instances>,
}

impl TruckKernel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 持っている形状の数。テストと動作確認用。
    pub fn shape_count(&self) -> usize {
        self.lock().bodies.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn get(&self, h: ShapeHandle) -> Result<Body, KernelError> {
        let s = self.lock();
        if let Some(b) = s.bodies.get(&h.0) {
            return Ok(b.clone());
        }
        if s.imported.contains_key(&h.0) {
            return Err(KernelError::Failed(
                "網のボディは足し引き・面の編集・フィレット・STEP の書き出しに使えません(三角形の網で、面と辺の形を持たないため)"
                    .into(),
            ));
        }
        Err(KernelError::UnknownShape(h))
    }

    /// 形を行列で写して動かした複製(面の名前の末尾に `suffix` を足す)。
    fn moved_copy(
        &self,
        shape: ShapeHandle,
        m: &k::Matrix4,
        suffix: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if let Some(why) = k::placement_problem(m) {
            return Err(KernelError::Failed(why.into()));
        }
        if let Some(im) = self.imported(shape) {
            // 読み込んだ形は網なので、点と向きと辺の折れ線を動かす(U12 の立てる。回すと
            // 動かすだけなので、体積と面積はそのまま)
            let moved = transform_imported(&im, m, suffix)?;
            let mut s = self.lock();
            s.next += 1;
            let id = s.next;
            s.imported.insert(id, Arc::new(moved));
            return Ok(ShapeHandle(id));
        }
        let body = self.get(shape)?;
        let mat = to_matrix(m);
        let moved = guard("移動", || {
            Ok(Body {
                parts: body
                    .parts
                    .iter()
                    .map(|p| {
                        Part::new(
                            tm::builder::transformed(&p.solid, mat),
                            p.names.iter().map(|n| format!("{n}{suffix}")).collect(),
                        )
                    })
                    .collect(),
            })
        })?;
        Ok(self.put(moved))
    }

    fn imported(&self, h: ShapeHandle) -> Option<Arc<import::Imported>> {
        self.lock().imported.get(&h.0).cloned()
    }

    fn put(&self, body: Body) -> ShapeHandle {
        let mut s = self.lock();
        s.next += 1;
        let id = s.next;
        s.bodies.insert(id, body);
        ShapeHandle(id)
    }
}

/// truck の panic を、カーネルの失敗に変える。
fn guard<T>(what: &str, f: impl FnOnce() -> Result<T, KernelError>) -> Result<T, KernelError> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|p| {
        let msg = p
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        Err(KernelError::Failed(format!(
            "{what}の内部で失敗しました: {msg}"
        )))
    })
}

fn pt(p: P3) -> tm::Point3 {
    tm::Point3::new(p[0], p[1], p[2])
}

fn vc(p: P3) -> tm::Vector3 {
    tm::Vector3::new(p[0], p[1], p[2])
}

fn faces_of(solid: &tm::Solid) -> Vec<tm::Face> {
    solid
        .boundaries()
        .iter()
        .flat_map(|sh| sh.face_iter().cloned().collect::<Vec<_>>())
        .collect()
}

fn surface_key(face: &tm::Face) -> String {
    serde_json::to_string(&face.surface()).unwrap_or_default()
}

/// テッセレーションした形(辺は折れ線、面は三角形の網)。
type Meshed = truck_topology::Solid<tm::Point3, PolylineCurve<tm::Point3>, Option<PolygonMesh>>;

/// 面ごとの三角形の網(面の向きを反映済み)。
fn face_meshes(solid: &tm::Solid, tol: f64) -> Vec<Option<PolygonMesh>> {
    meshes_of(&solid.triangulation(tol))
}

fn meshes_of(meshed: &Meshed) -> Vec<Option<PolygonMesh>> {
    meshed
        .boundaries()
        .iter()
        .flat_map(|sh| {
            sh.face_iter()
                .map(|f| {
                    f.surface().map(|mut m| {
                        if !f.orientation() {
                            m.invert();
                        }
                        m
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn triangles(mesh: &PolygonMesh) -> impl Iterator<Item = [P3; 3]> + '_ {
    let pos = mesh.positions();
    mesh.faces().triangle_iter().map(move |t| {
        let p = |i: usize| {
            let q = pos[t[i].pos];
            [q.x, q.y, q.z]
        };
        [p(0), p(1), p(2)]
    })
}

fn tri_area(t: &[P3; 3]) -> f64 {
    let c = k::cross(k::sub(t[1], t[0]), k::sub(t[2], t[0]));
    k::dot(c, c).sqrt() / 2.0
}

/// 網の囲む体積(面の向きで符号が付く)。三角形と基準点が作る四面体を足す。基準点は世界の原点で
/// なく網の外形の箱の中心にする: 閉じた網なら同じ値だが、網を作れなかった面のすき間があると、
/// 原点を使った和は形を原点から離すほどずれる(置いた場所で体積が変わる)。
fn signed_volume(meshes: &[Option<PolygonMesh>]) -> f64 {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for t in meshes.iter().flatten().flat_map(triangles) {
        for p in t {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
    }
    if lo[0] > hi[0] {
        return 0.0;
    }
    let c = [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0);
    meshes
        .iter()
        .flatten()
        .flat_map(triangles)
        .map(|t| {
            let [a, b, d] = t.map(|p| k::sub(p, c));
            k::dot(a, k::cross(b, d)) / 6.0
        })
        .sum()
}

/// 面の代表点: いちばん大きい三角形の重心。曲面の上にほぼ乗る。
fn representative_points(meshes: &[Option<PolygonMesh>]) -> Vec<P3> {
    meshes
        .iter()
        .map(|m| {
            m.as_ref()
                .and_then(|m| {
                    triangles(m)
                        .max_by(|a, b| tri_area(a).total_cmp(&tri_area(b)))
                        .map(|t| k::scale(k::add(k::add(t[0], t[1]), t[2]), 1.0 / 3.0))
                })
                .unwrap_or([0.0; 3])
        })
        .collect()
}

fn bounds(meshes: &[Option<PolygonMesh>]) -> (P3, P3) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for m in meshes.iter().flatten() {
        for p in m.positions() {
            for (i, v) in [p.x, p.y, p.z].into_iter().enumerate() {
                min[i] = min[i].min(v);
                max[i] = max[i].max(v);
            }
        }
    }
    (min, max)
}

fn polar(c: [f64; 2], r: f64, a: f64) -> [f64; 2] {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}

/// 輪郭を truck のワイヤにする。区間の名前を `names` に足す(円は 2 つに分けて `.a`、`.b`)。
fn build_wire(
    plane: &k::Plane,
    lp: &ProfileLoop,
    names: &mut Vec<String>,
) -> Result<tm::Wire, KernelError> {
    let w = |p: [f64; 2]| pt(plane.to_world(p));
    if let [only] = lp.edges.as_slice() {
        let Segment::Circle { center, radius } = only.segment else {
            return Err(KernelError::Failed(
                "1 区間だけの輪郭は円でなければなりません".into(),
            ));
        };
        let v0 = tm::builder::vertex(w(polar(center, radius, 0.0)));
        let v1 = tm::builder::vertex(w(polar(center, radius, std::f64::consts::PI)));
        let (t0, t1) = if only.reversed {
            (-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2)
        } else {
            (std::f64::consts::FRAC_PI_2, -std::f64::consts::FRAC_PI_2)
        };
        let e0 = tm::builder::circle_arc(&v0, &v1, w(polar(center, radius, t0)));
        let e1 = tm::builder::circle_arc(&v1, &v0, w(polar(center, radius, t1)));
        names.push(format!("{}.a", only.name));
        names.push(format!("{}.b", only.name));
        return Ok(vec![e0, e1].into());
    }
    let n = lp.edges.len();
    if n < 2 {
        return Err(KernelError::Failed("輪郭が空です".into()));
    }
    for i in 0..n {
        let a = lp.edges[i].to_point();
        let b = lp.edges[(i + 1) % n].from_point();
        if (a[0] - b[0]).hypot(a[1] - b[1]) > 1e-6 {
            return Err(KernelError::Failed(format!(
                "輪郭が {} と {} の間で閉じていません",
                lp.edges[i].name,
                lp.edges[(i + 1) % n].name
            )));
        }
    }
    let verts: Vec<tm::Vertex> = lp
        .edges
        .iter()
        .map(|e| tm::builder::vertex(w(e.from_point())))
        .collect();
    let mut edges = Vec::with_capacity(n);
    for (i, e) in lp.edges.iter().enumerate() {
        let (v0, v1) = (&verts[i], &verts[(i + 1) % n]);
        let edge = match e.segment {
            Segment::Line { .. } => tm::builder::line(v0, v1),
            Segment::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let mid = start + k::arc_sweep(start, end) / 2.0;
                tm::builder::circle_arc(v0, v1, w(polar(center, radius, mid)))
            }
            Segment::Circle { .. } => {
                return Err(KernelError::Failed(format!(
                    "円 {} は他の区間とつなげられません",
                    e.name
                )));
            }
        };
        edges.push(edge);
        names.push(e.name.clone());
    }
    Ok(edges.into())
}

fn region_face(plane: &k::Plane, region: &Region) -> Result<(tm::Face, Vec<String>), KernelError> {
    let mut names = Vec::new();
    let mut wires = vec![build_wire(plane, &region.outer, &mut names)?];
    for h in &region.holes {
        wires.push(build_wire(plane, h, &mut names)?);
    }
    let face = tm::builder::try_attach_plane(&wires)
        .map_err(|e| KernelError::Failed(format!("輪郭から面を作れません: {e}")))?;
    Ok((face, names))
}

fn to_matrix(m: &k::Matrix4) -> tm::Matrix4 {
    // cgmath は列優先
    tm::Matrix4::new(
        m[0][0], m[1][0], m[2][0], m[3][0], m[0][1], m[1][1], m[2][1], m[3][1], m[0][2], m[1][2],
        m[2][2], m[3][2], m[0][3], m[1][3], m[2][3], m[3][3],
    )
}

/// 読み込んだ形(網)を `m` で動かす(回転と平行移動だけを前提にする)。
fn transform_imported(
    im: &import::Imported,
    m: &k::Matrix4,
    suffix: &str,
) -> Result<import::Imported, KernelError> {
    if let Some(mesh) = &im.raw {
        let mut mesh = mesh.clone();
        for p in &mut mesh.positions {
            *p = k::transform_point(m, p.map(f64::from)).map(|x| x as f32);
        }
        // 法線は逆転置で動かす。非一様な尺度でも面に垂直な向きを保つ。
        let columns: [P3; 3] = std::array::from_fn(|j| std::array::from_fn(|i| m[i][j]));
        let cofactor = [
            k::cross(columns[1], columns[2]),
            k::cross(columns[2], columns[0]),
            k::cross(columns[0], columns[1]),
        ];
        let sign = k::dot(columns[0], cofactor[0]).signum();
        for n in &mut mesh.normals {
            *n = k::normalize(std::array::from_fn(|i| {
                sign * (0..3)
                    .map(|j| cofactor[j][i] * f64::from(n[j]))
                    .sum::<f64>()
            }))
            .map(|x| x as f32);
        }
        if sign < 0.0 {
            for t in &mut mesh.triangles {
                t.swap(1, 2);
            }
        }
        for line in &mut mesh.edges {
            for p in line {
                *p = k::transform_point(m, p.map(f64::from)).map(|x| x as f32);
            }
        }
        for name in &mut mesh.face_names {
            name.push_str(suffix);
        }
        return raw_imported(mesh);
    }
    let point = |p: [f64; 3]| k::transform_point(m, p);
    let dir = |v: [f64; 3]| {
        let mut out = [0.0; 3];
        for (i, o) in out.iter_mut().enumerate() {
            *o = m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2];
        }
        out
    };
    let meshes: Vec<Option<PolygonMesh>> = im
        .meshes
        .iter()
        .map(|mesh| {
            mesh.as_ref().map(|mesh| {
                let mut mesh = mesh.clone();
                for p in mesh.positions_mut() {
                    let q = point([p.x, p.y, p.z]);
                    *p = tm::Point3::new(q[0], q[1], q[2]);
                }
                for n in mesh.normals_mut() {
                    let q = dir([n.x, n.y, n.z]);
                    *n = tm::Vector3::new(q[0], q[1], q[2]);
                }
                mesh
            })
        })
        .collect();
    let edges: Vec<Vec<[f32; 3]>> = im
        .edges
        .iter()
        .map(|line| {
            line.iter()
                .map(|p| point(p.map(f64::from)).map(|x| x as f32))
                .collect()
        })
        .collect();
    // 体積と面積は、大きさを変える行列(尺度など)の時だけ取り直す。体積は行列式の分だけ変わる
    let (volume, area) = if k::is_rigid(m) {
        (im.volume, im.area)
    } else {
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        let area: f64 = meshes
            .iter()
            .flatten()
            .flat_map(triangles)
            .map(|t| tri_area(&t))
            .sum();
        (im.volume * det.abs(), area)
    };
    // 外形の箱は、箱の 8 つの角を動かして取り直す
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for c in 0..8 {
        let corner = [0, 1, 2].map(|i| {
            if c >> i & 1 == 0 {
                im.min[i]
            } else {
                im.max[i]
            }
        });
        let q = point(corner);
        for i in 0..3 {
            min[i] = min[i].min(q[i]);
            max[i] = max[i].max(q[i]);
        }
    }
    Ok(import::Imported {
        raw: None,
        meshes,
        names: im.names.iter().map(|n| format!("{n}{suffix}")).collect(),
        edges,
        min,
        max,
        volume,
        area,
        open_edges: im.open_edges,
        centroid: im.centroid.map(|p| k::transform_point(m, p)),
    })
}

/// 預かった網の量を 1 回だけ求める。問い合わせでは Arc の参照と控えだけを読む。
fn raw_imported(mesh: Mesh) -> Result<import::Imported, KernelError> {
    let g = kmb_format::mesh::Geometry {
        vertices: mesh.positions.clone(),
        triangles: mesh.triangles.clone(),
    };
    let (open_edges, volume, area, centroid) =
        kmb_format::mesh::properties(&g).map_err(KernelError::Failed)?;
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for i in 0..3 {
            min[i] = min[i].min(f64::from(p[i]));
            max[i] = max[i].max(f64::from(p[i]));
        }
    }
    Ok(import::Imported {
        meshes: vec![],
        names: mesh.face_names.clone(),
        edges: vec![],
        min,
        max,
        volume,
        area,
        open_edges,
        centroid,
        raw: Some(mesh),
    })
}

/// 面ごとの網を、表示と加工の網に足す。
fn push_face_meshes(out: &mut Mesh, meshes: &[Option<PolygonMesh>], names: &[String]) {
    for (m, name) in meshes.iter().zip(names) {
        let face = out.face_names.len() as u32;
        out.face_names.push(name.clone());
        let Some(m) = m else { continue };
        let pos = m.positions();
        let nor = m.normals();
        for t in m.faces().triangle_iter() {
            let ps: Vec<P3> = t
                .iter()
                .map(|v| {
                    let q = pos[v.pos];
                    [q.x, q.y, q.z]
                })
                .collect();
            let c = k::cross(k::sub(ps[1], ps[0]), k::sub(ps[2], ps[0]));
            // 細い三角形では面の法線が当てにならないので、向きの比較に使わない
            let reliable = k::dot(c, c).sqrt() > 1e-9;
            let flat = k::normalize(c);
            let base = out.positions.len() as u32;
            for (v, p) in t.iter().zip(&ps) {
                out.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
                let n = match v.nor {
                    Some(i) if i < nor.len() => {
                        let n = nor[i];
                        let n = [n.x, n.y, n.z];
                        // 面の向きを反転した網では、法線が逆のことがある
                        if reliable && k::dot(n, flat) < 0.0 {
                            k::scale(n, -1.0)
                        } else {
                            n
                        }
                    }
                    _ => flat,
                };
                out.normals.push([n[0] as f32, n[1] as f32, n[2] as f32]);
            }
            out.triangles.push([base, base + 1, base + 2]);
            out.triangle_faces.push(face);
        }
    }
}

/// 網がすべて同じ向きの三角形なら、その平面。
fn planar_of(m: &PolygonMesh) -> Option<PlanarFace> {
    let mut normal: Option<P3> = None;
    let mut point = None;
    for t in triangles(m) {
        let c = k::cross(k::sub(t[1], t[0]), k::sub(t[2], t[0]));
        if k::dot(c, c).sqrt() < 1e-9 {
            continue;
        }
        let n = k::normalize(c);
        match normal {
            None => {
                normal = Some(n);
                point = Some(t[0]);
            }
            Some(m) if k::dot(m, n) > 1.0 - 1e-9 => {}
            Some(_) => return None,
        }
    }
    Some(PlanarFace {
        point: point?,
        normal: normal?,
    })
}

impl Kernel for TruckKernel {
    fn name(&self) -> &str {
        "truck"
    }

    fn body_kind(&self, shape: ShapeHandle) -> Result<k::BodyKind, KernelError> {
        if self.imported(shape).is_some() {
            Ok(k::BodyKind::Mesh)
        } else {
            self.get(shape)?;
            Ok(k::BodyKind::Solid)
        }
    }

    fn import_mesh(&self, mesh: &Mesh, tag: &str) -> Result<ShapeHandle, KernelError> {
        if (!mesh.normals.is_empty() && mesh.normals.len() != mesh.positions.len())
            || (!mesh.triangle_faces.is_empty()
                && mesh.triangle_faces.len() != mesh.triangles.len())
            || mesh.normals.iter().flatten().any(|x| !x.is_finite())
            || mesh
                .edges
                .iter()
                .flatten()
                .flatten()
                .any(|x| !x.is_finite())
        {
            return Err(KernelError::Failed(
                "網の法線・面番号・表示の辺が不正です".into(),
            ));
        }
        let mut mesh = mesh.clone();
        mesh.face_names = vec![format!("{tag}:mesh")];
        mesh.triangle_faces = vec![0; mesh.triangles.len()];
        let im = raw_imported(mesh)?;
        let mut s = self.lock();
        s.next += 1;
        let id = s.next;
        s.imported.insert(id, Arc::new(im));
        Ok(ShapeHandle(id))
    }

    fn extrude_faces(
        &self,
        shape: ShapeHandle,
        _faces: &[String],
        _from: f64,
        _to: f64,
        _tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        self.get(shape)?;
        Err(KernelError::Unsupported("面の縁のままの押し出し".into()))
    }
    fn blend_edges(
        &self,
        shape: ShapeHandle,
        _blends: &[k::EdgeBlend],
        _tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        self.get(shape)?;
        Err(KernelError::Unsupported("辺の丸め".into()))
    }
    fn shell(
        &self,
        shape: ShapeHandle,
        _open: &[String],
        _thickness: f64,
        _tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        self.get(shape)?;
        Err(KernelError::Unsupported("シェル".into()))
    }
    fn offset_faces(
        &self,
        shape: ShapeHandle,
        _faces: &[String],
        _distance: f64,
        _tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        self.get(shape)?;
        Err(KernelError::Unsupported("面のオフセット".into()))
    }

    fn extrude(
        &self,
        profile: &Profile,
        from: f64,
        to: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if !(to - from).is_finite() || to <= from {
            return Err(KernelError::Failed(
                "押し出しの距離は 0 より大きくしてください".into(),
            ));
        }
        if profile.regions.is_empty() {
            return Err(KernelError::Failed("閉じた輪郭がありません".into()));
        }
        let n = profile.plane.normal();
        let body = guard("押し出し", || {
            let mut parts = Vec::new();
            for region in &profile.regions {
                let (face, edge_names) = region_face(&profile.plane, region)?;
                let face = tm::builder::translated(&face, vc(k::scale(n, from)));
                let solid: tm::Solid = tm::builder::tsweep(&face, vc(k::scale(n, to - from)));
                let count = faces_of(&solid).len();
                // truck の掃引は [元の面(反転), 区間ごとの側面…, 移した面] の順に並ぶ
                let mut names = vec![format!("{tag}:start")];
                names.extend(edge_names.iter().map(|e| format!("{tag}:side:{e}")));
                names.push(format!("{tag}:end"));
                if names.len() != count {
                    names = (0..count).map(|i| format!("{tag}:face{i}")).collect();
                }
                parts.push(Part::new(solid, names));
            }
            Ok(Body { parts })
        })?;
        Ok(self.put(body))
    }

    fn revolve(
        &self,
        profile: &Profile,
        axis_origin: P3,
        axis_dir: P3,
        angle: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if !angle.is_finite() || angle <= 1e-9 {
            return Err(KernelError::Failed(
                "回転の角度は 0 より大きくしてください".into(),
            ));
        }
        if profile.regions.is_empty() {
            return Err(KernelError::Failed("閉じた輪郭がありません".into()));
        }
        let plane = profile.plane;
        let a = k::normalize(axis_dir);
        if k::dot(a, plane.normal()).abs() > 1e-6
            || k::dot(k::sub(axis_origin, plane.origin), plane.normal()).abs() > 1e-6
        {
            return Err(KernelError::Failed(
                "回転軸はスケッチの平面の上に置いてください".into(),
            ));
        }
        // 平面の中で軸に直交する向き。輪郭がある側を正にする
        let mut u = k::cross(plane.normal(), a);
        let pts: Vec<P3> = profile
            .regions
            .iter()
            .flat_map(|r| r.outer.polyline())
            .map(|p| plane.to_world(p))
            .collect();
        let sum: f64 = pts.iter().map(|p| k::dot(k::sub(*p, axis_origin), u)).sum();
        if sum < 0.0 {
            u = k::scale(u, -1.0);
        }
        if pts
            .iter()
            .any(|p| k::dot(k::sub(*p, axis_origin), u) < -1e-9)
        {
            return Err(KernelError::Failed(
                "輪郭が回転軸をまたいでいます(軸の両側に輪郭があります。軸を「自動」か、輪郭の辺が乗る軸にしてください)".into(),
            ));
        }
        let full = angle >= std::f64::consts::TAU - 1e-9;
        let end_plane_normal = {
            let r = k::rotation(axis_origin, a, angle);
            k::sub(
                k::transform_point(&r, plane.normal()),
                k::transform_point(&r, [0.0; 3]),
            )
        };
        let body = guard("回転", || {
            let mut parts = Vec::new();
            for region in &profile.regions {
                let (face, _) = region_face(&plane, region)?;
                let solid: tm::Solid = tm::builder::rsweep(
                    &face,
                    pt(axis_origin),
                    vc(a),
                    tm::Rad(if full { std::f64::consts::TAU } else { angle }),
                );
                let meshes = face_meshes(&solid, COARSE_TOLERANCE);
                let reps = representative_points(&meshes);
                let edges: Vec<&k::ProfileEdge> = std::iter::once(&region.outer)
                    .chain(&region.holes)
                    .flat_map(|l| &l.edges)
                    .collect();
                let names: Vec<String> = reps
                    .iter()
                    .map(|&p| {
                        let d = k::sub(p, axis_origin);
                        let h = k::dot(d, a);
                        let radial = k::sub(d, k::scale(a, h));
                        let r = k::dot(radial, radial).sqrt();
                        let on_start = k::dot(d, plane.normal()).abs() < 1e-3 && k::dot(d, u) > 0.0;
                        let on_end = !full && k::dot(d, end_plane_normal).abs() < 1e-3;
                        let q = plane
                            .to_local(k::add(axis_origin, k::add(k::scale(a, h), k::scale(u, r))));
                        let (best, dist) = edges
                            .iter()
                            .map(|e| (e, e.segment.distance(q)))
                            .min_by(|x, y| x.1.total_cmp(&y.1))
                            .map(|(e, d)| (e.name.clone(), d))
                            .unwrap_or_default();
                        // 始まりと終わりの面は、代表点がどの区間の上にもない
                        if dist > 1e-2 && (on_start || on_end) {
                            if on_start {
                                format!("{tag}:start")
                            } else {
                                format!("{tag}:end")
                            }
                        } else {
                            format!("{tag}:side:{best}")
                        }
                    })
                    .collect();
                let names = dedupe_names(names, &reps);
                parts.push(Part::with_meshes(solid, names, meshes));
            }
            Ok(Body { parts })
        })?;
        Ok(self.put(body))
    }

    fn boolean(
        &self,
        op: BooleanOp,
        target: ShapeHandle,
        tool: ShapeHandle,
    ) -> Result<ShapeHandle, KernelError> {
        let target = self.get(target)?;
        let tool = self.get(tool)?;
        let body = guard("ブーリアン", || {
            let parts = match op {
                BooleanOp::Subtract => {
                    let mut result = Vec::new();
                    for t in &target.parts {
                        let mut pieces = vec![t.clone()];
                        for s in &tool.parts {
                            let mut next = Vec::new();
                            for p in &pieces {
                                next.extend(subtract_part(p, s)?);
                            }
                            pieces = next;
                        }
                        result.extend(pieces);
                    }
                    result
                }
                BooleanOp::Intersect => {
                    let mut result = Vec::new();
                    for t in &target.parts {
                        for s in &tool.parts {
                            result.extend(intersect_part(t, s, &[t, s])?);
                        }
                    }
                    result
                }
                BooleanOp::Union => {
                    let mut parts = target.parts.clone();
                    for s in &tool.parts {
                        let mut merged = s.clone();
                        let mut keep = Vec::new();
                        for p in parts.drain(..) {
                            if !p.overlaps(&merged) {
                                keep.push(p);
                                continue;
                            }
                            match union_part(&merged, &p)? {
                                Some(m) => merged = m,
                                None => keep.push(p),
                            }
                        }
                        keep.push(merged);
                        parts = keep;
                    }
                    parts
                }
            };
            let body = Body { parts };
            check_volume(op, target.volume(), tool.volume(), body.volume())?;
            Ok(body)
        })?;
        Ok(self.put(body))
    }

    fn transform(
        &self,
        shape: ShapeHandle,
        m: &k::Matrix4,
        suffix: &str,
    ) -> Result<ShapeHandle, KernelError> {
        if suffix.is_empty() && k::is_rigid(m) {
            return self.instance(shape, m);
        }
        self.moved_copy(shape, m, suffix)
    }

    fn instance(&self, shape: ShapeHandle, m: &k::Matrix4) -> Result<ShapeHandle, KernelError> {
        k::place_instance(&self.instances, shape, m, |b, m| self.moved_copy(b, m, ""))
    }

    fn instance_of(&self, shape: ShapeHandle) -> Option<(ShapeHandle, k::Matrix4)> {
        self.instances
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .of(shape)
    }

    fn face_names(&self, shape: ShapeHandle) -> Result<Vec<String>, KernelError> {
        let s = self.lock();
        if let Some(im) = s.imported.get(&shape.0) {
            return Ok(im.names.clone());
        }
        let body = s
            .bodies
            .get(&shape.0)
            .ok_or(KernelError::UnknownShape(shape))?;
        Ok(body.parts.iter().flat_map(|p| p.names.clone()).collect())
    }

    /// 面の名前を付け替えた形(形は同じ。面の網と体積は前のまま使う)。
    fn rename_faces(
        &self,
        shape: ShapeHandle,
        renames: &[(String, String)],
    ) -> Result<ShapeHandle, KernelError> {
        let mut body = self.get(shape)?;
        for (from, to) in renames {
            let n = body
                .parts
                .iter_mut()
                .flat_map(|p| p.names.iter_mut())
                .find(|n| *n == from)
                .ok_or_else(|| KernelError::Failed(format!("面 {from} がありません")))?;
            *n = to.clone();
        }
        Ok(self.put(body))
    }

    fn faces(&self, shape: ShapeHandle) -> Result<Vec<FaceInfo>, KernelError> {
        if let Some(im) = self.imported(shape) {
            if let Some(mesh) = &im.raw {
                return Ok(vec![FaceInfo {
                    name: mesh.face_names[0].clone(),
                    planar: None,
                    area: im.area,
                }]);
            }
            return Ok(im
                .meshes
                .iter()
                .zip(&im.names)
                .map(|(m, n)| FaceInfo {
                    name: n.clone(),
                    planar: m.as_ref().and_then(planar_of),
                    area: m
                        .as_ref()
                        .map(|m| triangles(m).map(|t| tri_area(&t)).sum())
                        .unwrap_or(0.0),
                })
                .collect());
        }
        let body = self.get(shape)?;
        guard("面の列挙", || {
            let mut out = Vec::new();
            for p in &body.parts {
                let faces = faces_of(&p.solid);
                for ((f, n), m) in faces.iter().zip(&p.names).zip(p.meshes.iter()) {
                    let area = m
                        .as_ref()
                        .map(|m| triangles(m).map(|t| tri_area(&t)).sum())
                        .unwrap_or(0.0);
                    let planar = match f.surface() {
                        tm::Surface::Plane(pl) => {
                            let nv = pl.normal();
                            let mut normal = [nv.x, nv.y, nv.z];
                            if !f.orientation() {
                                normal = k::scale(normal, -1.0);
                            }
                            let o = pl.origin();
                            Some(PlanarFace {
                                point: [o.x, o.y, o.z],
                                normal: k::normalize(normal),
                            })
                        }
                        _ => None,
                    };
                    out.push(FaceInfo {
                        name: n.clone(),
                        planar,
                        area,
                    });
                }
            }
            Ok(out)
        })
    }

    fn edges(&self, shape: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
        if self.imported(shape).is_some() {
            return Ok(vec![]);
        }
        let body = self.get(shape)?;
        guard("辺の列挙", || {
            let mut out = Vec::new();
            for p in &body.parts {
                // 辺ごとに、それを囲む面(面の番号、外向きの法線、面の中での辺の向き、両端)を集める
                type Side = (usize, P3, P3, P3, P3);
                let mut by_edge: HashMap<tm::EdgeID, Vec<Side>> = HashMap::new();
                for (i, f) in faces_of(&p.solid).iter().enumerate() {
                    let tm::Surface::Plane(pl) = f.surface() else {
                        continue;
                    };
                    let nv = pl.normal();
                    let mut n = k::normalize([nv.x, nv.y, nv.z]);
                    if !f.orientation() {
                        n = k::scale(n, -1.0);
                    }
                    for w in f.boundaries() {
                        for e in w.edge_iter() {
                            if !matches!(e.curve(), tm::Curve::Line(_)) {
                                continue;
                            }
                            let a = e.front().point();
                            let b = e.back().point();
                            let (a, b) = ([a.x, a.y, a.z], [b.x, b.y, b.z]);
                            by_edge
                                .entry(e.id())
                                .or_default()
                                .push((i, n, k::sub(b, a), a, b));
                        }
                    }
                }
                for v in by_edge.values() {
                    let [(ia, na, ta, a, b), (ib, nb, _, _, _)] = v.as_slice() else {
                        continue;
                    };
                    // 面 A の中へ向かう向き(面の境界は法線から見て左回りなので、左側が内側)
                    let inward = k::cross(*na, *ta);
                    let convex = k::dot(inward, *nb) < 0.0;
                    let mut faces = [p.names[*ia].clone(), p.names[*ib].clone()];
                    let (mut a, mut b) = (*a, *b);
                    if faces[0] > faces[1] {
                        faces.swap(0, 1);
                        std::mem::swap(&mut a, &mut b);
                    }
                    out.push(EdgeInfo {
                        curve: Vec::new(),
                        faces,
                        a,
                        b,
                        convex,
                        seam: Seam::from_normals(*na, *nb),
                        arc: None,
                    });
                }
            }
            out.sort_by(|x, y| x.faces.cmp(&y.faces));
            Ok(out)
        })
    }

    fn mass(&self, shape: ShapeHandle) -> Result<MassProps, KernelError> {
        if let Some(im) = self.imported(shape) {
            if im.open_edges > 0 {
                return Err(KernelError::Failed(format!(
                    "網が閉じていないので体積を求められません(開いた縁が {} 本あります)。表示・加工・書き出しには使えます",
                    im.open_edges
                )));
            }
            return Ok(MassProps {
                volume: im.volume,
                area: im.area,
                min: im.min,
                max: im.max,
                centroid: im.centroid,
            });
        }
        if let Some(m) = self.lock().mass.get(&shape.0) {
            return Ok(*m);
        }
        let body = self.get(shape)?;
        let mass = guard("体積の計算", || {
            let measure = |tol: f64| {
                let mut volume = 0.0;
                let mut area = 0.0;
                let mut min = [f64::INFINITY; 3];
                let mut max = [f64::NEG_INFINITY; 3];
                for p in &body.parts {
                    let meshes = face_meshes(&p.solid, tol);
                    volume += signed_volume(&meshes);
                    area += meshes
                        .iter()
                        .flatten()
                        .flat_map(triangles)
                        .map(|t| tri_area(&t))
                        .sum::<f64>();
                    let (a, b) = bounds(&meshes);
                    for i in 0..3 {
                        min[i] = min[i].min(a[i]);
                        max[i] = max[i].max(b[i]);
                    }
                }
                MassProps {
                    volume,
                    area,
                    min,
                    max,
                    centroid: None,
                }
            };
            let fine = measure(MASS_TOLERANCE);
            let coarse = measure(2.0 * MASS_TOLERANCE);
            // 2 つの値が近い時だけ外挿する(どちらかの網が壊れていたら細かい方を使う)
            let extrapolate = |f: f64, c: f64| {
                if (f - c).abs() <= 1e-3 * f.abs().max(1.0) {
                    2.0 * f - c
                } else {
                    f
                }
            };
            Ok(MassProps {
                volume: extrapolate(fine.volume, coarse.volume),
                area: extrapolate(fine.area, coarse.area),
                ..fine
            })
        })?;
        self.lock().mass.insert(shape.0, mass);
        Ok(mass)
    }

    fn bounds(&self, shape: ShapeHandle) -> Result<(P3, P3), KernelError> {
        if let Some(im) = self.imported(shape) {
            return Ok((im.min, im.max));
        }
        let body = self.get(shape)?;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for p in &body.parts {
            for i in 0..3 {
                min[i] = min[i].min(p.min[i]);
                max[i] = max[i].max(p.max[i]);
            }
        }
        Ok((min, max))
    }

    fn mesh(&self, shape: ShapeHandle, tolerance: f64) -> Result<Mesh, KernelError> {
        if let Some(im) = self.imported(shape) {
            if let Some(mesh) = &im.raw {
                return Ok(mesh.clone());
            }
            // 読み込んだ時の網(許容差 IMPORT_TOLERANCE)をそのまま返す
            let mut out = Mesh::default();
            push_face_meshes(&mut out, &im.meshes, &im.names);
            out.edges = im.edges.clone();
            return Ok(out);
        }
        let key = (shape.0, tolerance.to_bits());
        if let Some(m) = self.lock().meshes.get(&key) {
            return Ok(m.clone());
        }
        let body = self.get(shape)?;
        let mesh = guard("テッセレーション", || {
            let mut out = Mesh::default();
            for p in &body.parts {
                let meshed = p.solid.triangulation(tolerance);
                for shell in meshed.boundaries() {
                    for e in shell.edge_iter() {
                        let line: Vec<[f32; 3]> = e
                            .curve()
                            .iter()
                            .map(|q| [q.x as f32, q.y as f32, q.z as f32])
                            .collect();
                        if line.len() >= 2 {
                            out.edges.push(line);
                        }
                    }
                }
                // 辺と面を同じ網から取る(テッセレーションは 1 回)
                push_face_meshes(&mut out, &meshes_of(&meshed), &p.names);
            }
            Ok(out)
        })?;
        self.lock().meshes.insert(key, mesh.clone());
        Ok(mesh)
    }

    fn export_step(&self, shapes: &[ShapeHandle]) -> Result<String, KernelError> {
        let mut solids = Vec::new();
        for h in shapes {
            solids.extend(self.get(*h)?.parts.into_iter().map(|p| p.solid));
        }
        if solids.is_empty() {
            return Err(KernelError::Failed("書き出す形状がありません".into()));
        }
        guard("STEP 出力", || {
            let compressed: Vec<_> = solids.iter().map(|s| s.compress()).collect();
            let models: truck_stepio::out::StepModels<_, _, _> = compressed.iter().collect();
            Ok(truck_stepio::out::CompleteStepDisplay::new(
                models,
                truck_stepio::out::StepHeaderDescriptor {
                    organization_system: "kieMonoBench".to_owned(),
                    ..Default::default()
                },
            )
            .to_string())
        })
    }

    fn import_step(&self, step: &str) -> Result<ShapeHandle, KernelError> {
        // truck の網づくりは、壊れた STEP で終わらないことがある(自分の書き出した交線の
        // STEP で実測)。別のスレッドで行い、待つ時間に上限を設ける。止まらなかった
        // スレッドは捨てる(計算は裏で続くが、結果は使わない)
        let (tx, rx) = std::sync::mpsc::channel();
        let text = step.to_string();
        std::thread::spawn(move || {
            let _ = tx.send(guard("STEP の読み込み", || import::read_step(&text)));
        });
        let im = rx
            .recv_timeout(import::IMPORT_TIMEOUT)
            .map_err(|_| {
                KernelError::Failed(format!(
                    "STEP の網づくりが {} 秒で終わりませんでした(対応していない曲線や面が入っている可能性があります)",
                    import::IMPORT_TIMEOUT.as_secs()
                ))
            })??;
        let mut s = self.lock();
        s.next += 1;
        let id = s.next;
        s.imported.insert(id, Arc::new(im));
        Ok(ShapeHandle(id))
    }

    fn retain(&self, live: &HashSet<ShapeHandle>) {
        let live = &self
            .instances
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(live);
        let mut s = self.lock();
        s.bodies.retain(|id, _| live.contains(&ShapeHandle(*id)));
        s.imported.retain(|id, _| live.contains(&ShapeHandle(*id)));
        s.meshes
            .retain(|(id, _), _| live.contains(&ShapeHandle(*id)));
        s.mass.retain(|id, _| live.contains(&ShapeHandle(*id)));
    }
}

#[cfg(test)]
mod tests;
