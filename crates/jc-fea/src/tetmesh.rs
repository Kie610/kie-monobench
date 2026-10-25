//! 形に沿った四面体の網。ボディの表示用の網(閉じた三角形の網、面の名前つき)から、10 節点の
//! 四面体の網を作る。辺 → 面 → 中の順に形に合わせる:
//! 1. 表面を要素の大きさに合わせて張り直す。面の境の辺はそのまま保つ([`crate::surfmesh`])。
//! 2. 中を 3 次元の Delaunay で埋め、欠けた表面を辺を割って取り戻し、外接球の中心に点を足して
//!    細かくする([`crate::volmesh`])。
//! 3. 平たい四面体を、辺の取り除き・面の取り除き・点の移動で直す([`crate::tetopt`])。
//! 4. 辺の中点の節点を足して 10 節点にし、表面の辺の中点は本当の面(細かい表示用の網)へ、面の境の
//!    辺の中点は境の線へ乗せる。ヤコビアンの下限が正でない要素は、中点を半分ずつまっすぐへ戻す。
//!
//! 前の作り方(体心立方の格子を形で切り詰める isosurface stuffing)は、縁に沿って要素の形と
//! 向きが不規則にそろわず、固定した縁で応力が上下した(8 倍に細かくしても消えない)ので替えた。
//!
//! 結果には網の確かさの数字(網の体積と形の体積、表面のずれ、要素の形の良さ)を付ける。

use crate::geom::{P3, Tree, add, cross, dist, dot, lerp, norm, scale, sub, tet_volume, unit};
use crate::surfmesh::{SizeRule, reference, remesh};
use crate::tet10;
use jc_core::Mesh;
use std::collections::{HashMap, HashSet};

/// 表面の 6 節点の三角形(外向き)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BFace {
    /// 角 3 つと辺の中点 3 つ(角 a→b、b→c、c→a)
    pub nodes: [u32; 6],
    /// 属する要素
    pub elem: u32,
    /// 形の面の番号(`face_names` の番号。決まらなければ `u32::MAX`)
    pub face: u32,
}

/// 網の確かさの数字。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshStats {
    /// 格子の間隔(mm)
    pub h: f64,
    /// 網の体積(曲がった要素のまま積分、mm³)
    pub volume: f64,
    /// 形の体積(表示用の網で囲んだ体積、mm³)
    pub shape_volume: f64,
    /// 網の表面の点から形の表面までの距離の最大と平均(mm)
    pub dev_max: f64,
    pub dev_mean: f64,
    /// 形の表面の点から網の表面までの距離の最大(mm)と、その面の名前(網から抜けた所を見つける)
    pub miss_max: f64,
    pub miss_face: String,
    /// 角の要素(まっすぐな四面体)の二面角の最小と最大(度)
    pub min_dihedral: f64,
    pub max_dihedral: f64,
    /// 二面角が 5° 未満か 175° を超える要素の数
    pub bad_angles: usize,
    /// 半径比(3·内接球 / 外接球。正四面体で 1)の最小と、0.2 以上の要素の割合
    pub min_radius_ratio: f64,
    pub good_share: f64,
    /// 曲げると裏返るので、中点をまっすぐへ戻した要素の数
    pub straightened: usize,
    /// 要素の大きさの場のいちばん小さい所(曲率・細い面・薄い所で小さくした大きさ、mm)
    pub min_size: f64,
    /// 張り直した表面の三角形のいちばん小さい角(度)
    pub surface_min_angle: f64,
    /// 鋭い縁に沿うそろった帯を置いた縁の点の数(`surfmesh::band`)
    pub band_points: usize,
    /// 網を作る前に大きさの場から見込んだ要素の数(`estimate_tets`)
    pub expected: f64,
}

/// 10 節点の四面体の網。
#[derive(Debug, Clone, Default)]
pub struct TetMesh {
    /// 節点の位置。前の `corners` 個が角の点、残りが辺の中点
    pub points: Vec<P3>,
    pub corners: usize,
    pub tets: Vec<[u32; 10]>,
    /// 表面の三角形
    pub faces: Vec<BFace>,
    pub face_names: Vec<String>,
    pub stats: MeshStats,
}

/// 網の要素の数の上限(これを超える細かさは誤りにする。メモリと時間を守るため)。
pub const MAX_TETS: usize = 150_000;
/// 網の外接の箱。
pub fn bounds(mesh: &Mesh) -> Option<(P3, P3)> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a] as f64);
            hi[a] = hi[a].max(p[a] as f64);
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

/// 閉じた網で囲んだ体積(向きのそろった三角形の符号つきの和)。
pub fn enclosed_volume(mesh: &Mesh) -> f64 {
    let at = |i: u32| mesh.positions[i as usize].map(f64::from);
    mesh.triangles
        .iter()
        .map(|t| tet_volume([0.0; 3], at(t[0]), at(t[1]), at(t[2])))
        .sum()
}

/// 要素の大きさから、要素の数の見込み(1 辺 h の立方体ごとに 12 個。形に沿う網は箱で約 1.15 倍)。
pub fn expected_tets(volume: f64, h: f64) -> f64 {
    12.0 * volume.abs() / h.powi(3)
}

/// 表示用の網の三角形の木(面積の無い三角形を除く)。
fn shape_tree(mesh: &Mesh) -> Tree {
    let at = |i: u32| mesh.positions[i as usize].map(f64::from);
    let mut tris = Vec::with_capacity(mesh.triangles.len());
    let mut tri_face = Vec::with_capacity(mesh.triangles.len());
    for (t, f) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        let p = t.map(at);
        if norm(cross(sub(p[1], p[0]), sub(p[2], p[0]))) > 0.0 {
            tris.push(p);
            tri_face.push(*f);
        }
    }
    Tree::new(tris, tri_face)
}

/// 要素の大きさの場 `r.sizing` から見込む要素の数。形の外接の箱に格子の点を置き、形の中の点
/// (半直線と表面の交わりが奇数)での大きさ l から 13.8 / l³ の平均を求め、形の体積を掛ける
/// ([`expected_tets`] の 12 に、形に沿う網が箱で約 1.15 倍になる分を掛けた係数。箱・見本の
/// 部品・板・穴・輪・球で本当の数の 0.87〜1.04 倍)。中の点が少なければ格子を細かくする。
fn estimate_from(r: &crate::surfmesh::Reference, shape: &Tree, volume: f64) -> f64 {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for t in &shape.tris {
        for p in t {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
    }
    let d = sub(hi, lo);
    if !(d[0] > 0.0 && d[1] > 0.0 && d[2] > 0.0) {
        return expected_tets(volume, r.sizing.min());
    }
    // 半直線は軸から少し傾けて、表面の辺や点をちょうど通らないように
    let ray = scale(unit([1.0, 0.001_234_5, 0.002_345_6]), 2.0 * norm(d) + 1.0);
    let mut cells = 20_000.0;
    let mut hits = Vec::new();
    for _ in 0..4 {
        let s = (d[0] * d[1] * d[2] / cells).cbrt();
        let n = d.map(|v| (v / s).ceil().max(1.0) as usize);
        let (mut sum, mut inside) = (0.0, 0usize);
        for i in 0..n[0] {
            for j in 0..n[1] {
                for k in 0..n[2] {
                    let c = [
                        lo[0] + d[0] * (i as f64 + 0.5) / n[0] as f64,
                        lo[1] + d[1] * (j as f64 + 0.5) / n[1] as f64,
                        lo[2] + d[2] * (k as f64 + 0.5) / n[2] as f64,
                    ];
                    shape.hits(c, add(c, ray), &mut hits);
                    if hits.len() % 2 == 1 {
                        sum += 13.8 / r.sizing.at(c).powi(3);
                        inside += 1;
                    }
                }
            }
        }
        if inside >= 200 {
            return volume * sum / inside as f64;
        }
        cells *= 8.0;
    }
    expected_tets(volume, r.sizing.min())
}

/// 要素の大きさ `h` のボディの網 `mesh` の要素の数を、網を作る前に大きさの場から見込む
/// (薄い所・細い所・曲率で小さくした大きさも入れる。厚さ 1 mm の板では h だけの
/// [`expected_tets`] の数十倍になる)。
pub fn estimate_tets(mesh: &Mesh, h: f64) -> Result<f64, String> {
    let (r, _) = reference(mesh, h, SizeRule::default())?;
    Ok(estimate_from(
        &r,
        &shape_tree(mesh),
        enclosed_volume(mesh).abs(),
    ))
}

/// 見込んだ要素の数が多すぎる時の誤り。
fn too_many(h: f64, expected: f64, min_size: f64) -> String {
    if min_size < 0.5 * h {
        format!(
            "要素の大きさ {h:.3} mm では要素が約 {expected:.0} 個になり多すぎます(上限 {MAX_TETS})。薄い所・細い所・小さな丸みでは要素をそこの厚さ・幅まで小さくするので(いちばん小さい所 {min_size:.3} mm)、要素の大きさを大きくしても減りにくいです。要素の大きさを大きくするか、形を簡単にしてください"
        )
    } else {
        format!(
            "要素の大きさ {h:.3} mm では要素が約 {expected:.0} 個になり多すぎます(上限 {MAX_TETS})。大きくしてください"
        )
    }
}

/// 要素の数がおよそ `target` になる要素の大きさ。
pub fn size_for(volume: f64, target: usize) -> f64 {
    (12.0 * volume.abs().max(1e-9) / target.max(1) as f64).cbrt()
}

/// 四面体の形の良さ(平均比 12·(3V)^(2/3) / Σ辺²。正四面体で 1、潰れると 0、裏返ると負)。
pub fn quality(a: P3, b: P3, c: P3, d: P3) -> f64 {
    let v = tet_volume(a, b, c, d);
    let l2: f64 = [(a, b), (a, c), (a, d), (b, c), (b, d), (c, d)]
        .iter()
        .map(|(p, q)| {
            let e = sub(*p, *q);
            dot(e, e)
        })
        .sum();
    if l2 <= 0.0 {
        return 0.0;
    }
    let q = 12.0 * (3.0 * v.abs()).powf(2.0 / 3.0) / l2;
    if v > 0.0 { q } else { -q }
}

/// 二面角(6 本の辺、度)。
pub fn dihedral_angles(p: [P3; 4]) -> [f64; 6] {
    let mut out = [0.0; 6];
    for (k, (a, b, c, d)) in [
        (0, 1, 2, 3),
        (0, 2, 1, 3),
        (0, 3, 1, 2),
        (1, 2, 0, 3),
        (1, 3, 0, 2),
        (2, 3, 0, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let e = unit(sub(p[b], p[a]));
        let perp = |x: P3| {
            let v = sub(x, p[a]);
            unit(sub(v, scale(e, dot(v, e))))
        };
        let (u, w) = (perp(p[c]), perp(p[d]));
        out[k] = dot(u, w).clamp(-1.0, 1.0).acos().to_degrees();
    }
    out
}

/// 半径比 3·r_内 / R_外(正四面体で 1)。
pub fn radius_ratio(p: [P3; 4]) -> f64 {
    let v = tet_volume(p[0], p[1], p[2], p[3]).abs();
    if v <= 0.0 {
        return 0.0;
    }
    let area = |a: P3, b: P3, c: P3| norm(cross(sub(b, a), sub(c, a))) / 2.0;
    let s = area(p[1], p[2], p[3])
        + area(p[0], p[2], p[3])
        + area(p[0], p[1], p[3])
        + area(p[0], p[1], p[2]);
    let r_in = 3.0 * v / s;
    let (a, b, c) = (sub(p[1], p[0]), sub(p[2], p[0]), sub(p[3], p[0]));
    let num = add(
        add(scale(cross(b, c), dot(a, a)), scale(cross(c, a), dot(b, b))),
        scale(cross(a, b), dot(c, c)),
    );
    let r_out = norm(num) / (12.0 * v);
    (3.0 * r_in / r_out).min(1.0)
}

/// 表面の網(角の点の三角形、外向き)と要素の面。
fn boundary_of(tets: &[[u32; 4]]) -> Vec<([u32; 3], u32, usize)> {
    let mut count: HashMap<[u32; 3], (u32, u32, usize)> = HashMap::new();
    for (e, t) in tets.iter().enumerate() {
        for (f, (c, _)) in tet10::FACES.iter().enumerate() {
            let tri = c.map(|i| t[i]);
            let mut k = tri;
            k.sort_unstable();
            count
                .entry(k)
                .and_modify(|x| x.0 += 1)
                .or_insert((1, e as u32, f));
        }
    }
    let mut out: Vec<([u32; 3], u32, usize)> = count
        .into_values()
        .filter(|x| x.0 == 1)
        .map(|(_, e, f)| (tet10::FACES[f].0.map(|i| tets[e as usize][i]), e, f))
        .collect();
    out.sort_unstable_by_key(|x| (x.1, x.2));
    out
}

/// ボディの表示用の網 `mesh` から、要素の大きさ `h` の 10 節点の四面体の網を作る。
pub fn build(mesh: &Mesh, h: f64, cancelled: &dyn Fn() -> bool) -> Result<TetMesh, String> {
    const CANCELLED: &str = "中止しました";
    bounds(mesh).ok_or("ボディの網が空です")?;
    if h.is_nan() || h <= 0.0 {
        return Err("要素の大きさは 0 より大きくしてください".into());
    }
    let shape_volume = enclosed_volume(mesh).abs();
    if expected_tets(shape_volume, h) > MAX_TETS as f64 * 1.3 {
        return Err(format!(
            "要素の大きさ {h:.3} mm では要素が約 {:.0} 個になり多すぎます(上限 {MAX_TETS})。大きくしてください",
            expected_tets(shape_volume, h)
        ));
    }
    // 1. 表面
    let (r, s0) = reference(mesh, h, SizeRule::default())?;
    if cancelled() {
        return Err(CANCELLED.into());
    }
    // 大きさの場(薄い所・細い所・曲率で小さくした大きさ)から要素の数を見込み、多すぎれば
    // 網を作る前に断る(h だけの見込みは、厚さ 1 mm の板で本当の数の 1/76 だった)
    let shape = shape_tree(mesh);
    let expected = estimate_from(&r, &shape, shape_volume);
    if expected > MAX_TETS as f64 * 1.15 {
        return Err(too_many(h, expected, r.sizing.min()));
    }
    let surface = remesh(&r, s0, cancelled)?;
    let surface_stats = surface.stats(&r);
    let band_points = surface.band.len();
    // 2. 中(点の数はおよそ要素の数の 1/5)
    let mut lin = crate::volmesh::fill(&r, surface, MAX_TETS / 3, cancelled)?;
    if lin.tets.len() > MAX_TETS {
        return Err(format!(
            "要素が {} 個になり多すぎます(上限 {MAX_TETS})。要素の大きさを {h:.3} mm より大きくしてください",
            lin.tets.len()
        ));
    }
    // 3. 形を良くする
    let imp = crate::tetopt::improve(
        &r,
        &mut lin,
        crate::tetopt::GOAL_DEG.to_radians().sin(),
        cancelled,
    )?;
    if cancelled() {
        return Err(CANCELLED.into());
    }
    // 4. 10 節点にする
    let corners = lin.points.len();
    let mut points = lin.points.clone();
    let mut edge_id: HashMap<(u32, u32), u32> = HashMap::new();
    let mut tets10: Vec<[u32; 10]> = Vec::with_capacity(lin.tets.len());
    for t in &lin.tets {
        let mut e10 = [0u32; 10];
        e10[..4].copy_from_slice(t);
        for (k, [a, b]) in tet10::EDGES.iter().enumerate() {
            let (u, w) = (t[*a], t[*b]);
            let key = (u.min(w), u.max(w));
            e10[4 + k] = *edge_id.entry(key).or_insert_with(|| {
                points.push(lerp(points[u as usize], points[w as usize], 0.5));
                (points.len() - 1) as u32
            });
        }
        tets10.push(e10);
    }
    let straight: Vec<P3> = points.clone();
    let label_of: HashMap<[u32; 3], u32> = lin
        .faces
        .iter()
        .map(|(f, l)| {
            let mut k = *f;
            k.sort_unstable();
            (k, *l)
        })
        .collect();
    let bnd = boundary_of(&lin.tets);
    let mut edge_labels: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut faces: Vec<BFace> = Vec::with_capacity(bnd.len());
    for (tri, e, f) in &bnd {
        let mut k = *tri;
        k.sort_unstable();
        let l = label_of.get(&k).copied().unwrap_or(u32::MAX);
        let t = &tets10[*e as usize];
        let (c, m) = tet10::FACES[*f];
        let nodes = [t[c[0]], t[c[1]], t[c[2]], t[m[0]], t[m[1]], t[m[2]]];
        for v in &nodes[3..] {
            edge_labels.entry(*v).or_default().push(l);
        }
        faces.push(BFace {
            nodes,
            elem: *e,
            face: l,
        });
    }
    if faces.len() != lin.faces.len() || faces.iter().any(|f| f.face == u32::MAX) {
        return Err(format!(
            "四面体の網の表面({} 個)が張り直した表面({} 個)と合いません",
            faces.len(),
            lin.faces.len()
        ));
    }
    // 表面の辺の中点を本当の面(面の境なら境の線)へ
    let ends: HashMap<u32, (u32, u32)> = edge_id.iter().map(|(k, v)| (*v, *k)).collect();
    let mut curved: Vec<u32> = edge_labels.keys().copied().collect();
    curved.sort_unstable();
    for &m in &curved {
        let (a, b) = ends[&m];
        let mid = points[m as usize];
        let len = dist(points[a as usize], points[b as usize]);
        let mut ls = edge_labels[&m].clone();
        ls.sort_unstable();
        ls.dedup();
        let target = match ls.as_slice() {
            [f] => Some(r.on_face(*f, mid)),
            [f, g] => r.group(*f, *g).map(|grp| r.on_chain(grp, mid)),
            _ => None,
        };
        if let Some(q) = target
            && dist(q, mid) < 0.5 * len
        {
            points[m as usize] = q;
        }
    }
    // 裏返る要素は中点をまっすぐへ戻す
    let pulled = pull_back(&tets10, &mut points, &straight);
    let mut tm = TetMesh {
        points,
        corners,
        tets: tets10,
        faces,
        face_names: mesh.face_names.clone(),
        stats: MeshStats {
            h,
            shape_volume,
            straightened: pulled.len(),
            min_size: r.sizing.min(),
            surface_min_angle: surface_stats.min_angle,
            band_points,
            expected,
            ..MeshStats::default()
        },
    };
    tm.measure(&shape);
    log::info!(
        "四面体の網: 要素 {}・節点 {}(要素の大きさ {h:.3} mm、いちばん小さい所 {:.3} mm)、体積 {:.1}/{:.1} mm³、表面のずれ 最大 {:.3} mm、形から 最大 {:.3} mm({})、二面角 {:.1}°〜{:.1}°、表面の三角形の最小角 {:.1}°、鋭い縁に沿う帯 {} 点(中の点 {})、表面を取り戻すのに割った辺 {}、中に足した点 {}、形を直した 辺 {}・面 {}・点 {}、曲げを戻した要素 {}",
        tm.tets.len(),
        tm.points.len(),
        r.sizing.min(),
        tm.stats.volume,
        tm.stats.shape_volume,
        tm.stats.dev_max,
        tm.stats.miss_max,
        tm.stats.miss_face,
        tm.stats.min_dihedral,
        tm.stats.max_dihedral,
        surface_stats.min_angle,
        tm.stats.band_points,
        lin.stats.band_inner,
        lin.stats.recovery_splits + lin.stats.encroached_splits,
        lin.stats.inserted,
        imp.edge_removals,
        imp.face_removals,
        imp.moves,
        tm.stats.straightened
    );
    Ok(tm)
}

/// 曲げた中点(まっすぐの位置から動いた中点)を持つ要素のうち、ヤコビアンの下限が線形の 1/10
/// 以下の物の中点を、半分ずつまっすぐへ戻す(6 回目からは全部まっすぐ)。戻した要素の番号。
///
/// 調べるのは曲げた中点を 1 つでも持つ要素全部(表面の辺だけを持ち表面の面を持たない要素も)。
/// 戻した中点は隣の要素とも共にするので、毎回全部を調べ直す(表面の面を持つ要素だけや、前の回に
/// 悪かった要素だけを調べると、裏返った要素が漏れる)。
fn pull_back(tets10: &[[u32; 10]], points: &mut [P3], straight: &[P3]) -> HashSet<u32> {
    let mut pulled: HashSet<u32> = HashSet::new();
    let bent = |points: &[P3], m: u32| dist(points[m as usize], straight[m as usize]) > 0.0;
    let check: Vec<u32> = (0..tets10.len() as u32)
        .filter(|e| tets10[*e as usize][4..].iter().any(|m| bent(points, *m)))
        .collect();
    for round in 0..8 {
        let mut bad = Vec::new();
        for &e in &check {
            let t = &tets10[e as usize];
            if !t[4..].iter().any(|m| bent(points, *m)) {
                continue;
            }
            let x = t.map(|i| points[i as usize]);
            let (low, _) = tet10::jacobian_bounds(&x);
            let lin_det = 6.0 * tet_volume(x[0], x[1], x[2], x[3]);
            if low <= 0.1 * lin_det {
                bad.push(e);
            }
        }
        if bad.is_empty() {
            break;
        }
        for &e in &bad {
            pulled.insert(e);
            for &m in &tets10[e as usize][4..] {
                let s = straight[m as usize];
                points[m as usize] = if round >= 6 {
                    s
                } else {
                    lerp(s, points[m as usize], 0.5)
                };
            }
        }
    }
    pulled
}

/// 6 節点の三角形の自然座標 (r, s) の位置(角 a,b,c、中点 ab, bc, ca)。
pub fn face_point(x: &[P3; 6], r: f64, s: f64) -> P3 {
    let t = 1.0 - r - s;
    let n = [
        t * (2.0 * t - 1.0),
        r * (2.0 * r - 1.0),
        s * (2.0 * s - 1.0),
        4.0 * t * r,
        4.0 * r * s,
        4.0 * s * t,
    ];
    let mut p = [0.0; 3];
    for k in 0..6 {
        p = add(p, scale(x[k], n[k]));
    }
    p
}

/// 6 節点の三角形の積分点(Dunavant の 4 次、6 点。重みは面積 1/2 込み)。
pub fn face_gauss() -> [(f64, f64, f64); 6] {
    let (a1, b1, w1) = (
        0.445_948_490_915_965,
        0.108_103_018_168_070,
        0.223_381_589_678_011,
    );
    let (a2, b2, w2) = (
        0.091_576_213_509_771,
        0.816_847_572_980_459,
        0.109_951_743_655_322,
    );
    [
        (a1, a1, w1 / 2.0),
        (b1, a1, w1 / 2.0),
        (a1, b1, w1 / 2.0),
        (a2, a2, w2 / 2.0),
        (b2, a2, w2 / 2.0),
        (a2, b2, w2 / 2.0),
    ]
}

/// 6 節点の三角形の形関数と、(r, s) での x_r × x_s(面積の要素と法線)。
pub fn face_shape(x: &[P3; 6], r: f64, s: f64) -> ([f64; 6], P3) {
    let t = 1.0 - r - s;
    let n = [
        t * (2.0 * t - 1.0),
        r * (2.0 * r - 1.0),
        s * (2.0 * s - 1.0),
        4.0 * t * r,
        4.0 * r * s,
        4.0 * s * t,
    ];
    let dr = [
        -(4.0 * t - 1.0),
        4.0 * r - 1.0,
        0.0,
        4.0 * (t - r),
        4.0 * s,
        -4.0 * s,
    ];
    let ds = [
        -(4.0 * t - 1.0),
        0.0,
        4.0 * s - 1.0,
        -4.0 * r,
        4.0 * r,
        4.0 * (t - s),
    ];
    let mut xr = [0.0; 3];
    let mut xs = [0.0; 3];
    for k in 0..6 {
        xr = add(xr, scale(x[k], dr[k]));
        xs = add(xs, scale(x[k], ds[k]));
    }
    (n, cross(xr, xs))
}

impl TetMesh {
    pub fn tet_points(&self, e: usize) -> [P3; 10] {
        self.tets[e].map(|i| self.points[i as usize])
    }

    pub fn face_points(&self, f: usize) -> [P3; 6] {
        self.faces[f].nodes.map(|i| self.points[i as usize])
    }

    /// 網の確かさの数字を測る。
    fn measure(&mut self, shape: &Tree) {
        let st = &mut self.stats;
        st.volume = (0..self.tets.len())
            .map(|e| tet10::volume(&self.tets[e].map(|i| self.points[i as usize])))
            .sum();
        // 網 → 形
        let (mut dmax, mut dsum, mut dn) = (0.0f64, 0.0, 0usize);
        for f in &self.faces {
            let x = f.nodes.map(|i| self.points[i as usize]);
            let samples = [
                x[0],
                x[1],
                x[2],
                x[3],
                x[4],
                x[5],
                face_point(&x, 1.0 / 3.0, 1.0 / 3.0),
            ];
            for p in samples {
                if let Some((d, _, _)) = shape.nearest(p, |_| true) {
                    dmax = dmax.max(d);
                    dsum += d;
                    dn += 1;
                }
            }
        }
        st.dev_max = dmax;
        st.dev_mean = dsum / dn.max(1) as f64;
        // 形 → 網(網の表面を細かい三角形の木にして)
        let mut tris = Vec::with_capacity(self.faces.len() * 4);
        for f in &self.faces {
            let x = f.nodes.map(|i| self.points[i as usize]);
            for t in [[0, 3, 5], [3, 1, 4], [5, 4, 2], [3, 4, 5]] {
                tris.push(t.map(|k| x[k]));
            }
        }
        let n = tris.len();
        let tree = Tree::new(tris, vec![0; n]);
        let (mut mmax, mut mface) = (0.0f64, u32::MAX);
        for (t, f) in shape.tris.iter().zip(&shape.face) {
            let c = scale(add(add(t[0], t[1]), t[2]), 1.0 / 3.0);
            for p in [t[0], t[1], t[2], c] {
                if let Some((d, _, _)) = tree.nearest(p, |_| true)
                    && d > mmax
                {
                    mmax = d;
                    mface = *f;
                }
            }
        }
        st.miss_max = mmax;
        st.miss_face = self
            .face_names
            .get(mface as usize)
            .cloned()
            .unwrap_or_default();
        // 要素の形
        let (mut lo, mut hi, mut bad) = (180.0f64, 0.0f64, 0usize);
        let (mut rmin, mut good) = (1.0f64, 0usize);
        for t in &self.tets {
            let p = [0, 1, 2, 3].map(|k| self.points[t[k] as usize]);
            let a = dihedral_angles(p);
            let (l, u) = a
                .iter()
                .fold((180.0f64, 0.0f64), |(l, u), x| (l.min(*x), u.max(*x)));
            lo = lo.min(l);
            hi = hi.max(u);
            if l < 5.0 || u > 175.0 {
                bad += 1;
            }
            let r = radius_ratio(p);
            rmin = rmin.min(r);
            if r >= 0.2 {
                good += 1;
            }
        }
        st.min_dihedral = lo;
        st.max_dihedral = hi;
        st.bad_angles = bad;
        st.min_radius_ratio = rmin;
        st.good_share = good as f64 / self.tets.len().max(1) as f64;
    }

    /// 節点ごとの要素の形の良さ(まわりの要素の半径比の最小)。
    pub fn node_quality(&self) -> Vec<f64> {
        let mut q = vec![1.0f64; self.points.len()];
        for t in &self.tets {
            let p = [0, 1, 2, 3].map(|k| self.points[t[k] as usize]);
            let r = radius_ratio(p);
            for v in t {
                q[*v as usize] = q[*v as usize].min(r);
            }
        }
        q
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 円柱(半径 r、高さ l、Z の向き、底は z = 0)の表示用の網。面は side・bottom・top。
    pub(crate) fn cylinder(r: f64, l: f64, seg: usize) -> Mesh {
        annulus(0.0, r, l, seg, 4)
    }

    /// 中空の円柱(内の半径 a、外の半径 b、高さ l)。a = 0 なら中の詰まった円柱。
    /// 面は inner・outer・bottom・top(a = 0 なら side・bottom・top)。高さの向きは `rows` 段に分ける。
    pub(crate) fn annulus(a: f64, b: f64, l: f64, seg: usize, rows: usize) -> Mesh {
        let mut m = Mesh::default();
        let solid = a <= 0.0;
        m.face_names = if solid {
            vec!["side".into(), "bottom".into(), "top".into()]
        } else {
            vec![
                "inner".into(),
                "outer".into(),
                "bottom".into(),
                "top".into(),
            ]
        };
        let (fi_in, fi_out, fi_bot, fi_top) = if solid { (9, 0, 1, 2) } else { (0, 1, 2, 3) };
        let ang = |i: usize| std::f64::consts::TAU * i as f64 / seg as f64;
        let push = |m: &mut Mesh, p: [f64; 3]| -> u32 {
            m.positions.push(p.map(|v| v as f32));
            (m.positions.len() - 1) as u32
        };
        // 側面(外は外向き、内は内向き)
        for (rad, f, out) in [(b, fi_out, true), (a, fi_in, false)] {
            if rad <= 0.0 {
                continue;
            }
            let mut ring = Vec::new();
            for z in 0..=rows {
                let zz = l * z as f64 / rows as f64;
                ring.push(
                    (0..seg)
                        .map(|i| push(&mut m, [rad * ang(i).cos(), rad * ang(i).sin(), zz]))
                        .collect::<Vec<u32>>(),
                );
            }
            for z in 0..rows {
                for i in 0..seg {
                    let j = (i + 1) % seg;
                    let (p, q, r, s) = (ring[z][i], ring[z][j], ring[z + 1][j], ring[z + 1][i]);
                    if out {
                        m.triangles.push([p, q, r]);
                        m.triangles.push([p, r, s]);
                    } else {
                        m.triangles.push([p, r, q]);
                        m.triangles.push([p, s, r]);
                    }
                    m.triangle_faces.extend([f, f]);
                }
            }
        }
        // 底と上(輪の帯、または扇)
        for (zz, f, up) in [(0.0, fi_bot, false), (l, fi_top, true)] {
            let outer: Vec<u32> = (0..seg)
                .map(|i| push(&mut m, [b * ang(i).cos(), b * ang(i).sin(), zz]))
                .collect();
            if solid {
                let c = push(&mut m, [0.0, 0.0, zz]);
                for i in 0..seg {
                    let j = (i + 1) % seg;
                    if up {
                        m.triangles.push([c, outer[i], outer[j]]);
                    } else {
                        m.triangles.push([c, outer[j], outer[i]]);
                    }
                    m.triangle_faces.push(f);
                }
            } else {
                let inner: Vec<u32> = (0..seg)
                    .map(|i| push(&mut m, [a * ang(i).cos(), a * ang(i).sin(), zz]))
                    .collect();
                for i in 0..seg {
                    let j = (i + 1) % seg;
                    let (p, q, r, s) = (inner[i], outer[i], outer[j], inner[j]);
                    if up {
                        m.triangles.push([p, q, r]);
                        m.triangles.push([p, r, s]);
                    } else {
                        m.triangles.push([p, r, q]);
                        m.triangles.push([p, s, r]);
                    }
                    m.triangle_faces.extend([f, f]);
                }
            }
        }
        m
    }

    fn check_mesh(tm: &TetMesh) {
        for (e, t) in tm.tets.iter().enumerate() {
            let x = tm.tet_points(e);
            assert!(
                tet10::jacobian_bounds(&x).0 > 0.0,
                "要素 {e} が裏返っている"
            );
            let _ = t;
        }
        assert!(tm.faces.iter().all(|f| f.face != u32::MAX));
    }

    #[test]
    fn a_box_is_meshed_to_its_exact_volume_with_sharp_edges() {
        let m = crate::validation_tests::box_mesh([30.0, 10.0, 8.0]);
        let tm = build(&m, 2.5, &|| false).unwrap();
        check_mesh(&tm);
        let s = &tm.stats;
        eprintln!("{s:?} 要素 {}", tm.tets.len());
        assert!(
            (s.volume - 2400.0).abs() / 2400.0 < 1e-3,
            "体積 {}",
            s.volume
        );
        assert!(s.miss_max < 0.05, "形から網へのずれ {}", s.miss_max);
        assert!(s.dev_max < 1e-6, "網から形へのずれ {}", s.dev_max);
        // 6 つの面の名前が全部どこかの表面の三角形に付く
        for f in 0..6 {
            assert!(tm.faces.iter().any(|b| b.face == f));
        }
        assert!(s.min_dihedral > 3.0, "二面角 {}", s.min_dihedral);
    }

    #[test]
    fn a_cylinder_keeps_its_volume_with_curved_elements() {
        let (r, l) = (10.0, 20.0);
        let m = cylinder(r, l, 96);
        let tm = build(&m, 2.5, &|| false).unwrap();
        check_mesh(&tm);
        let s = &tm.stats;
        eprintln!("{s:?} 要素 {}", tm.tets.len());
        // 表示用の網(96 角形)の体積に合う
        assert!(
            (s.volume - s.shape_volume).abs() / s.shape_volume < 2e-3,
            "体積 {} と {}",
            s.volume,
            s.shape_volume
        );
        let exact = std::f64::consts::PI * r * r * l;
        assert!((s.volume - exact).abs() / exact < 3e-3);
        assert!(s.miss_max < 0.1, "形から網へのずれ {}", s.miss_max);
    }

    #[test]
    fn a_hollow_cylinder_keeps_both_walls() {
        let (a, b, l) = (5.0, 10.0, 6.0);
        let m = annulus(a, b, l, 96, 2);
        let tm = build(&m, 1.25, &|| false).unwrap();
        check_mesh(&tm);
        let s = &tm.stats;
        eprintln!("{s:?} 要素 {}", tm.tets.len());
        assert!(
            (s.volume - s.shape_volume).abs() / s.shape_volume < 2e-3,
            "体積 {} と {}",
            s.volume,
            s.shape_volume
        );
        assert!(s.miss_max < 0.1, "形から網へのずれ {}", s.miss_max);
        for f in 0..4 {
            assert!(tm.faces.iter().any(|x| x.face == f), "面 {f}");
        }
    }

    /// 直径 2 mm の穴(要素の大きさ 1 mm)の表面の辺の中点が穴の面に乗る。形を良くする時に
    /// 穴の面の点を面に沿って動かして長い辺を作ったり、穴の面に沿う平たい四面体を残したりすると、
    /// 中点を面へ乗せた要素が裏返り、中点をまっすぐへ戻すことになる(要素 5 個で、中点が穴の面から
    /// 0.15 mm 外れた)。
    #[test]
    fn midpoints_stay_on_a_small_hole() {
        let m = annulus(1.0, 5.0, 4.0, 96, 2);
        let tm = build(&m, 1.0, &|| false).unwrap();
        check_mesh(&tm);
        let s = &tm.stats;
        eprintln!("{s:?} 要素 {}", tm.tets.len());
        assert!(
            s.straightened <= 1,
            "まっすぐへ戻した要素 {}",
            s.straightened
        );
        assert!(s.dev_max < 0.05, "網から形へのずれ {}", s.dev_max);
        assert!(s.min_dihedral > 12.0, "二面角 {}", s.min_dihedral);
    }

    /// 表面の辺だけを持ち表面の面を持たない要素も、曲げた中点で裏返るなら中点をまっすぐへ戻す
    /// (表面の面を持つ要素だけを調べると漏れる)。
    #[test]
    fn an_element_with_only_a_bent_edge_is_pulled_back() {
        let c = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        assert!(tet_volume(c[0], c[1], c[2], c[3]) > 0.0);
        let mut points: Vec<P3> = c.to_vec();
        for [a, b] in tet10::EDGES {
            points.push(lerp(c[a], c[b], 0.5));
        }
        let straight = points.clone();
        let tets = vec![[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]];
        // 辺 0–1 の中点だけを中へ大きく曲げる(表面の面は無い)
        points[4] = add(points[4], [0.0, 0.3, 0.3]);
        let x = tets[0].map(|i| points[i as usize]);
        assert!(tet10::jacobian_bounds(&x).0 <= 0.0, "曲げると裏返る");
        let pulled = pull_back(&tets, &mut points, &straight);
        assert!(pulled.contains(&0));
        let x = tets[0].map(|i| points[i as usize]);
        assert!(tet10::jacobian_bounds(&x).0 > 0.0, "戻した後も裏返っている");
    }

    /// 同じ入力からは同じ網(表面を取り戻す時に割る辺の向きが、表面の三角形を集める HashMap の
    /// 順で変わり、見本の部品の網が毎回変わっていた)。
    #[test]
    fn the_same_input_gives_the_same_mesh() {
        let m = crate::surfmesh::tests::sample_part();
        let a = build(&m, 8.0, &|| false).unwrap();
        let b = build(&m, 8.0, &|| false).unwrap();
        assert_eq!(a.tets, b.tets, "同じ入力で違う網");
        assert_eq!(a.points, b.points);
    }
}
