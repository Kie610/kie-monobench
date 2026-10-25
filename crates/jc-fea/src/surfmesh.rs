//! 表面の張り直し(形に沿ったメッシュの 1 段目)。ボディの表示用の網(閉じた三角形の網、面の
//! 名前つき)を、要素の大きさに合わせた形のよい三角形の網に張り直す。面の境(面の名前が変わる
//! 辺)はそのまま保つので、張り直した三角形はどれも 1 つの面に属し、固定や荷重の面はそのまま
//! 使える。
//!
//! 作り方(Botsch・Kobbelt 2004「A Remeshing Approach to Multiresolution Modeling」の等方的な
//! 張り直しの考え。コードはどこからも写していない):
//! 1. 表示用の網の同じ位置の点を 1 つにまとめ、面の境の辺の鎖と角(3 つ以上の面が会う点、鎖が
//!    折れる点、角の無い輪に置く 3 点)を取り出す。
//! 2. 要素の大きさの場を決める。曲面の曲率(隣の三角形の法線の角)、細い面(別の鎖までの
//!    距離)、薄い所(隣でない面までの距離)で小さくし、そこから離れるほど一定の割合で大きくする。
//! 3. 長い辺を割る → 短い辺を縮める → 辺を入れ替えて Delaunay に近づける → 面に沿ってならす、
//!    を繰り返す。新しい点と動かした点は元の面(鎖の点は元の鎖)の上へ戻す。角は動かさない。
//!    鎖の点は面の点と別に、角から角までを大きさの場で測って等しい間に並べる(短い辺は縮めず、
//!    数を合わせてから動かす)。辺に沿って点がそろうと、そこに接する四面体もそろう。

use crate::geom::{P3, Tree, add, closest_on_segment, cross, dist, dot, lerp, norm, scale, sub};
use jc_core::Mesh;
use std::collections::{HashMap, HashSet};

mod band;
#[cfg(test)]
pub(crate) mod tests;

/// 点の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 動かさない点
    Corner,
    /// 面の境の鎖の上の点(鎖の組の番号。組は隣り合う 2 つの面で決まる)
    Chain(u32),
    /// 面の中の点(面の番号)
    Face(u32),
}

/// 張り直した表面(外向きの三角形)。
#[derive(Debug, Clone, Default)]
pub struct Surface {
    pub points: Vec<P3>,
    pub tris: Vec<[u32; 3]>,
    /// 三角形ごとの面の番号(`Mesh::face_names` の番号)
    pub label: Vec<u32>,
    pub kind: Vec<Kind>,
    /// 鋭い縁に沿う帯の点の組(縁の点・片側の面の列の点・もう片側の面の列の点)。中の網は、
    /// 組ごとに 2 つの列の点から平行四辺形の 4 つ目の点を中へ足す(`band`)
    pub band: Vec<[u32; 3]>,
    /// 三角形ごとの、帯の三角形か(中の網を作る時も割らない。取り戻せない時だけ割る)
    pub strip: Vec<bool>,
}

/// 要素の大きさの場。曲率や細い所で決めた見本の点から、距離に比例して大きくする。
#[derive(Debug, Clone, Default)]
pub struct Sizing {
    pub h: f64,
    /// 離れるほど大きくする割合(1 mm 離れるごとに増やす大きさ)
    pub grade: f64,
    samples: Vec<(P3, f64)>,
    cell: f64,
    reach: i64,
    grid: HashMap<[i64; 3], Vec<u32>>,
}

impl Sizing {
    fn new(h: f64, grade: f64, samples: Vec<(P3, f64)>) -> Self {
        let lmin = samples.iter().map(|s| s.1).fold(h, f64::min);
        let cell = h;
        let reach = ((h - lmin) / grade / cell).ceil() as i64;
        let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
        for (i, (p, _)) in samples.iter().enumerate() {
            grid.entry(p.map(|v| (v / cell).floor() as i64))
                .or_default()
                .push(i as u32);
        }
        Sizing {
            h,
            grade,
            samples,
            cell,
            reach,
            grid,
        }
    }

    /// 一様な大きさ。
    pub fn uniform(h: f64) -> Self {
        Self::new(h, 0.4, Vec::new())
    }

    /// 点 x での要素の大きさ。
    pub fn at(&self, x: P3) -> f64 {
        let mut l = self.h;
        if self.samples.is_empty() {
            return l;
        }
        let k = x.map(|v| (v / self.cell).floor() as i64);
        let r = self.reach;
        for dx in -r..=r {
            for dy in -r..=r {
                for dz in -r..=r {
                    if let Some(v) = self.grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                        for &i in v {
                            let (p, s) = self.samples[i as usize];
                            l = l.min(s + self.grade * dist(p, x));
                        }
                    }
                }
            }
        }
        l
    }

    /// 見本の中のいちばん小さい大きさ。
    pub fn min(&self) -> f64 {
        self.samples.iter().map(|s| s.1).fold(self.h, f64::min)
    }
}

/// 元の形(点を戻す先)。
pub struct Reference {
    /// 面ごとの三角形の木
    faces: Vec<Tree>,
    /// 鎖の組ごとの線分
    groups: Vec<Vec<[P3; 2]>>,
    group_of: HashMap<(u32, u32), u32>,
    pub sizing: Sizing,
    /// 形の外接の箱の対角の長さ
    pub diag: f64,
}

impl Reference {
    /// 面 `f` の上で p に最も近い点。
    pub fn on_face(&self, f: u32, p: P3) -> P3 {
        self.faces
            .get(f as usize)
            .and_then(|t| t.nearest(p, |_| true))
            .map_or(p, |x| x.1)
    }

    /// 鎖の組 `g` の上で p に最も近い点。
    pub fn on_chain(&self, g: u32, p: P3) -> P3 {
        let mut best = (f64::INFINITY, p);
        for s in &self.groups[g as usize] {
            let (q, _) = closest_on_segment(p, s[0], s[1]);
            let d = dist(p, q);
            if d < best.0 {
                best = (d, q);
            }
        }
        best.1
    }

    /// p から距離 d の内に、鎖の組 `skip` 以外の鎖があるか。
    pub fn near_other_chain(&self, p: P3, skip: Option<u32>, d: f64) -> bool {
        self.groups.iter().enumerate().any(|(g, segs)| {
            Some(g as u32) != skip
                && segs
                    .iter()
                    .any(|s| dist(p, closest_on_segment(p, s[0], s[1]).0) < d)
        })
    }

    /// 2 つの面の境の鎖の組(無ければ None)。
    pub fn group(&self, a: u32, b: u32) -> Option<u32> {
        self.group_of.get(&(a.min(b), a.max(b))).copied()
    }

    /// 種類に合わせて点を形へ戻す。
    pub fn project(&self, k: Kind, p: P3) -> P3 {
        match k {
            Kind::Corner => p,
            Kind::Chain(g) => self.on_chain(g, p),
            Kind::Face(f) => self.on_face(f, p),
        }
    }
}

/// 大きさの場を決める係数。
#[derive(Debug, Clone, Copy)]
pub struct SizeRule {
    /// 曲率の半径に対する要素の大きさ(円をおよそ 2π / これ に分ける)
    pub curvature: f64,
    /// 細い面の幅・薄い所の厚さに対する要素の大きさ
    pub thickness: f64,
    /// 離れるほど大きくする割合
    pub grade: f64,
    /// いちばん小さい大きさ(h に対する割合)
    pub min_share: f64,
}

impl Default for SizeRule {
    fn default() -> Self {
        SizeRule {
            curvature: 0.7,
            thickness: 1.0,
            grade: 0.4,
            min_share: 1.0 / 8.0,
        }
    }
}

fn ek(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

fn tri_normal(a: P3, b: P3, c: P3) -> P3 {
    cross(sub(b, a), sub(c, a))
}

/// 三角形のいちばん小さい角(度)。
pub fn min_angle(a: P3, b: P3, c: P3) -> f64 {
    let ang = |p: P3, q: P3, r: P3| {
        let (u, v) = (sub(q, p), sub(r, p));
        let d = norm(u) * norm(v);
        if d <= 0.0 {
            0.0
        } else {
            (dot(u, v) / d).clamp(-1.0, 1.0).acos().to_degrees()
        }
    };
    ang(a, b, c).min(ang(b, c, a)).min(ang(c, a, b))
}

/// 三角形の上に、間が `step` 以下になるように置いた見本の点(角を含む)。ふつうの三角形は
/// 角 0 からの 2 辺を k = ⌈いちばん長い辺 / step⌉ に分けた格子((k+1)(k+2)/2 点)。細長い
/// 三角形では格子の点の数がいちばん長い辺の 2 乗に比例して無駄に増える(円柱の側面の長さ
/// 100 mm・幅 0.25 mm の三角形 1 つで 2 万点、円柱 1 本の reference が 9.4 秒)ので、格子の点が
/// 面積に比例する並べ方の 3 倍を超える時は、いちばん長い辺に沿う列を向かいの角へ向かって高さを
/// step 以下に分けて並べる。
fn tri_samples(p: [P3; 3], step: f64) -> Vec<P3> {
    let len = [dist(p[0], p[1]), dist(p[1], p[2]), dist(p[2], p[0])];
    let i = (0..3).fold(0, |b, k| if len[k] > len[b] { k } else { b });
    let longest = len[i];
    let height = if longest > 0.0 {
        norm(tri_normal(p[0], p[1], p[2])) / longest
    } else {
        0.0
    };
    let k = (longest / step).ceil().max(1.0) as usize;
    let rows = (height / step).ceil().max(1.0) as usize;
    let mut out = Vec::new();
    if (k + 1) * (k + 2) / 2 <= 3 * (rows + 1) * (k + 1) {
        for i in 0..=k {
            for j in 0..=k - i {
                let (u, v) = (i as f64 / k as f64, j as f64 / k as f64);
                out.push(add(
                    add(scale(p[0], 1.0 - u - v), scale(p[1], u)),
                    scale(p[2], v),
                ));
            }
        }
        return out;
    }
    let (a, b, c) = (p[i], p[(i + 1) % 3], p[(i + 2) % 3]);
    for r in 0..=rows {
        let t = r as f64 / rows as f64;
        let (s, e) = (lerp(a, c, t), lerp(b, c, t));
        let n = if r == rows {
            0
        } else {
            ((1.0 - t) * longest / step).ceil().max(1.0) as usize
        };
        for j in 0..=n {
            out.push(if n == 0 {
                s
            } else {
                lerp(s, e, j as f64 / n as f64)
            });
        }
    }
    out
}

/// 表示用の網から、まとめた点の網・元の形・大きさの場を作る。
pub fn reference(mesh: &Mesh, h: f64, rule: SizeRule) -> Result<(Reference, Surface), String> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a] as f64);
            hi[a] = hi[a].max(p[a] as f64);
        }
    }
    if lo[0] > hi[0] {
        return Err("ボディの網が空です".into());
    }
    let diag = dist(lo, hi).max(1e-9);
    let tol = diag * 1e-7;
    // 1. 同じ位置の点を 1 つにまとめる
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    let mut pts: Vec<P3> = Vec::new();
    let merged: Vec<u32> = mesh
        .positions
        .iter()
        .map(|p| {
            let p = p.map(f64::from);
            *ids.entry(p.map(|v| (v / tol).round() as i64))
                .or_insert_with(|| {
                    pts.push(p);
                    (pts.len() - 1) as u32
                })
        })
        .collect();
    let mut tris: Vec<[u32; 3]> = Vec::with_capacity(mesh.triangles.len());
    let mut label: Vec<u32> = Vec::with_capacity(mesh.triangles.len());
    for (t, f) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        let m = t.map(|i| merged[i as usize]);
        if m[0] == m[1] || m[1] == m[2] || m[0] == m[2] {
            continue;
        }
        let p = m.map(|i| pts[i as usize]);
        if norm(tri_normal(p[0], p[1], p[2])) <= tol * tol {
            continue;
        }
        tris.push(m);
        label.push(*f);
    }
    if tris.is_empty() {
        return Err("ボディの網が空です".into());
    }
    // 全体が裏返った網(三角形が内向き)は、囲んだ体積が負になる。裏返しのままでは中の四面体を
    // 外と取り違える(閉じていないという誤った理由で失敗した)ので、向きをそろえ直す。空洞の
    // 内向きの殻は外の殻より小さいので、全体の体積は正のまま
    let signed: f64 = tris
        .iter()
        .map(|t| {
            let p = t.map(|i| pts[i as usize]);
            crate::geom::tet_volume(lo, p[0], p[1], p[2])
        })
        .sum();
    if signed < 0.0 {
        log::warn!("ボディの網の三角形が内向きなので、向きを裏返して使います");
        for t in &mut tris {
            t.swap(1, 2);
        }
    }
    let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for (ti, t) in tris.iter().enumerate() {
        for k in 0..3 {
            edges
                .entry(ek(t[k], t[(k + 1) % 3]))
                .or_default()
                .push(ti as u32);
        }
    }
    let open = edges.values().filter(|v| v.len() != 2).count();
    if open > 0 {
        return Err(format!(
            "ボディの網が閉じていません(1 つの面にしか付かない辺か、3 つ以上の面に付く辺が {open} 本)"
        ));
    }
    let n = pts.len();
    let nf = mesh
        .face_names
        .len()
        .max(label.iter().map(|l| *l as usize + 1).max().unwrap_or(0));
    // 面の境の辺
    let mut bnb: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut vlabels: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut adj_faces: Vec<HashSet<u32>> = vec![HashSet::new(); nf];
    for (ti, t) in tris.iter().enumerate() {
        for v in t {
            vlabels[*v as usize].push(label[ti]);
        }
    }
    for v in &mut vlabels {
        v.sort_unstable();
        v.dedup();
    }
    let mut bedges: Vec<(u32, u32)> = Vec::new();
    let mut sorted_edges: Vec<(&(u32, u32), &Vec<u32>)> = edges.iter().collect();
    sorted_edges.sort_unstable_by_key(|x| *x.0);
    for &(&(a, b), ts) in &sorted_edges {
        let (la, lb) = (label[ts[0] as usize], label[ts[1] as usize]);
        if la != lb {
            bnb[a as usize].push(b);
            bnb[b as usize].push(a);
            bedges.push((a, b));
            adj_faces[la as usize].insert(lb);
            adj_faces[lb as usize].insert(la);
        }
    }
    // 角
    let cos_turn = 30f64.to_radians().cos();
    let mut corner = vec![false; n];
    for v in 0..n {
        let b = &bnb[v];
        if vlabels[v].len() >= 3 || (!b.is_empty() && b.len() != 2) {
            corner[v] = true;
        } else if b.len() == 2 {
            let d1 = crate::geom::unit(sub(pts[v], pts[b[0] as usize]));
            let d2 = crate::geom::unit(sub(pts[b[1] as usize], pts[v]));
            if dot(d1, d2) < cos_turn {
                corner[v] = true;
            }
        }
    }
    // 鎖(角から角へ)。角の無い輪は 3 点を角にする
    let mut chains: Vec<Vec<u32>> = Vec::new();
    let mut used: HashSet<(u32, u32)> = HashSet::new();
    let walk =
        |start: u32, next: u32, used: &mut HashSet<(u32, u32)>, corner: &[bool]| -> Vec<u32> {
            let mut path = vec![start];
            let (mut prev, mut cur) = (start, next);
            loop {
                used.insert(ek(prev, cur));
                path.push(cur);
                if corner[cur as usize] || cur == start {
                    break;
                }
                let Some(&nx) = bnb[cur as usize]
                    .iter()
                    .find(|w| **w != prev && !used.contains(&ek(**w, cur)))
                else {
                    break;
                };
                prev = cur;
                cur = nx;
            }
            path
        };
    for v in 0..n as u32 {
        if !corner[v as usize] || bnb[v as usize].is_empty() {
            continue;
        }
        for &w in &bnb[v as usize].clone() {
            if !used.contains(&ek(v, w)) {
                chains.push(walk(v, w, &mut used, &corner));
            }
        }
    }
    for v in 0..n as u32 {
        for &w in &bnb[v as usize].clone() {
            if used.contains(&ek(v, w)) {
                continue;
            }
            let path = walk(v, w, &mut used, &corner);
            let k = path.len() - 1;
            let marks = if k >= 3 {
                vec![0, k / 3, 2 * k / 3]
            } else {
                (0..k).collect()
            };
            // 輪は細い面の距離を測るために 3 つに分けるだけ(角にはしない。輪が 2 点に
            // 縮むことは、縮める時の輪の条件が防ぐ)
            let mut cuts = marks.clone();
            cuts.push(k);
            for w in cuts.windows(2) {
                chains.push(path[w[0]..=w[1]].to_vec());
            }
        }
    }
    // 鎖の組(隣り合う 2 つの面)
    let mut group_of: HashMap<(u32, u32), u32> = HashMap::new();
    let mut groups: Vec<Vec<[P3; 2]>> = Vec::new();
    let mut edge_group: HashMap<(u32, u32), u32> = HashMap::new();
    for &(a, b) in &bedges {
        let ts = &edges[&(a, b)];
        let (la, lb) = (label[ts[0] as usize], label[ts[1] as usize]);
        let key = (la.min(lb), la.max(lb));
        let g = *group_of.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            (groups.len() - 1) as u32
        });
        groups[g as usize].push([pts[a as usize], pts[b as usize]]);
        edge_group.insert((a, b), g);
    }
    let kind: Vec<Kind> = (0..n)
        .map(|v| {
            if corner[v] {
                Kind::Corner
            } else if vlabels[v].len() == 2 {
                // 2 つの面が点だけで触れる所(2 つの球が極で触れるなど)は、点に付く面の境の辺が
                // 無いので鎖に乗らない: 動かさない点にする
                match group_of.get(&(vlabels[v][0], vlabels[v][1])) {
                    Some(g) if !bnb[v].is_empty() => Kind::Chain(*g),
                    _ => Kind::Corner,
                }
            } else {
                Kind::Face(vlabels[v][0])
            }
        })
        .collect();
    // 面ごとの木
    let mut per_face: Vec<(Vec<[P3; 3]>, Vec<u32>)> = vec![(Vec::new(), Vec::new()); nf];
    for (t, l) in tris.iter().zip(&label) {
        per_face[*l as usize].0.push(t.map(|i| pts[i as usize]));
        per_face[*l as usize].1.push(*l);
    }
    let faces: Vec<Tree> = per_face.into_iter().map(|(t, f)| Tree::new(t, f)).collect();
    let all = Tree::new(
        tris.iter().map(|t| t.map(|i| pts[i as usize])).collect(),
        label.clone(),
    );
    // 2. 大きさの場。三角形と鎖の上に h/2 おきに見本の点を置き、そこでの大きさを決める
    let lmin = h * rule.min_share;
    let step = 0.5 * h;
    let mut samples: Vec<(P3, f64)> = Vec::new();
    let mut put = |p: P3, l: f64| {
        if l.is_finite() && l < h * 0.999 {
            samples.push((p, l.max(lmin)));
        }
    };
    // 三角形ごとの曲率による大きさ(同じ面の隣の三角形の法線の角 / 辺からの高さの平均)
    let mut tri_size = vec![f64::INFINITY; tris.len()];
    for &(&(a, b), ts) in &sorted_edges {
        let (t0, t1) = (ts[0] as usize, ts[1] as usize);
        if label[t0] != label[t1] {
            continue;
        }
        let other = |t: usize| {
            *tris[t]
                .iter()
                .find(|v| **v != a && **v != b)
                .expect("三角形の 3 点目")
        };
        let (pa, pb) = (pts[a as usize], pts[b as usize]);
        let tn = |t: usize| {
            tri_normal(
                pts[tris[t][0] as usize],
                pts[tris[t][1] as usize],
                pts[tris[t][2] as usize],
            )
        };
        let (n0, n1) = (tn(t0), tn(t1));
        let c = (dot(n0, n1) / (norm(n0) * norm(n1))).clamp(-1.0, 1.0);
        let theta = c.acos();
        if theta < 1e-4 {
            continue;
        }
        let e = dist(pa, pb).max(1e-12);
        let ht = |t: usize| norm(tri_normal(pa, pb, pts[other(t) as usize])) / e;
        let l = rule.curvature * 0.5 * (ht(t0) + ht(t1)) / theta;
        tri_size[t0] = tri_size[t0].min(l);
        tri_size[t1] = tri_size[t1].min(l);
    }
    // 薄い所: 隣でない面までの距離
    let skip_of = |ls: &[u32]| -> HashSet<u32> {
        let mut skip = HashSet::new();
        for l in ls {
            skip.insert(*l);
            skip.extend(adj_faces[*l as usize].iter().copied());
        }
        skip
    };
    let thick = |p: P3, skip: &HashSet<u32>| -> f64 {
        all.nearest_within(p, h / rule.thickness, |ti| !skip.contains(&all.face[ti]))
            .map_or(f64::INFINITY, |x| rule.thickness * x.0)
    };
    for (ti, t) in tris.iter().enumerate() {
        let skip = skip_of(&[label[ti]]);
        for q in tri_samples(t.map(|i| pts[i as usize]), step) {
            put(q, tri_size[ti].min(thick(q, &skip)));
        }
    }
    // 鎖: 曲がりと、角を共にしない別の鎖までの距離(細い面)
    let chain_ends: Vec<[u32; 2]> = chains.iter().map(|c| [c[0], c[c.len() - 1]]).collect();
    let cell = h;
    let mut seg_grid: HashMap<[i64; 3], Vec<(u32, u32)>> = HashMap::new();
    for (ci, c) in chains.iter().enumerate() {
        for (k, w) in c.windows(2).enumerate() {
            let (p, q) = (pts[w[0] as usize], pts[w[1] as usize]);
            let mut a = [0i64; 3];
            let mut z = [0i64; 3];
            for ax in 0..3 {
                a[ax] = (p[ax].min(q[ax]) / cell).floor() as i64;
                z[ax] = (p[ax].max(q[ax]) / cell).floor() as i64;
            }
            for i in a[0]..=z[0] {
                for j in a[1]..=z[1] {
                    for kk in a[2]..=z[2] {
                        seg_grid
                            .entry([i, j, kk])
                            .or_default()
                            .push((ci as u32, k as u32));
                    }
                }
            }
        }
    }
    let shares = |c1: usize, c2: usize| {
        let (e1, e2) = (chain_ends[c1], chain_ends[c2]);
        e1.iter().any(|x| e2.contains(x))
    };
    let turn = |v: usize| -> f64 {
        if corner[v] || bnb[v].len() != 2 {
            return f64::INFINITY;
        }
        let (a, b) = (pts[bnb[v][0] as usize], pts[bnb[v][1] as usize]);
        let (d1, d2) = (sub(pts[v], a), sub(b, pts[v]));
        let phi = (dot(d1, d2) / (norm(d1) * norm(d2)))
            .clamp(-1.0, 1.0)
            .acos();
        if phi > 1e-4 {
            rule.curvature * 0.5 * (norm(d1) + norm(d2)) / phi
        } else {
            f64::INFINITY
        }
    };
    for (ci, c) in chains.iter().enumerate() {
        for w in c.windows(2) {
            let (pa, pb) = (pts[w[0] as usize], pts[w[1] as usize]);
            let bend = turn(w[0] as usize).min(turn(w[1] as usize));
            let k = (dist(pa, pb) / step).ceil().max(1.0) as usize;
            for i in 0..=k {
                let p = lerp(pa, pb, i as f64 / k as f64);
                let key = p.map(|x| (x / cell).floor() as i64);
                let mut best = f64::INFINITY;
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            let Some(list) = seg_grid.get(&[key[0] + dx, key[1] + dy, key[2] + dz])
                            else {
                                continue;
                            };
                            for &(c2, s2) in list {
                                let c2 = c2 as usize;
                                if c2 == ci || shares(ci, c2) {
                                    continue;
                                }
                                let ch = &chains[c2];
                                let (q, _) = closest_on_segment(
                                    p,
                                    pts[ch[s2 as usize] as usize],
                                    pts[ch[s2 as usize + 1] as usize],
                                );
                                best = best.min(dist(p, q));
                            }
                        }
                    }
                }
                put(p, bend.min(rule.thickness * best));
            }
        }
    }
    let sizing = Sizing::new(h, rule.grade, samples);
    Ok((
        Reference {
            faces,
            groups,
            group_of,
            sizing,
            diag,
        },
        Surface {
            points: pts,
            tris,
            label,
            kind,
            band: Vec::new(),
            strip: Vec::new(),
        },
    ))
}

/// 張り直しの途中の網(四面体の網を作る時も、表面の辺を割るのに使う)。
pub(crate) struct Work<'a> {
    r: &'a Reference,
    pub(crate) p: Vec<P3>,
    pub(crate) kind: Vec<Kind>,
    size: Vec<f64>,
    pub(crate) t: Vec<[u32; 3]>,
    pub(crate) lab: Vec<u32>,
    pub(crate) alive: Vec<bool>,
    vt: Vec<Vec<u32>>,
    /// 帯の三角形(張り直しの仕上げで動かさない・割らない・入れ替えない)
    pub(crate) frozen: Vec<bool>,
    pub(crate) band: Vec<[u32; 3]>,
    /// 鎖ごとの大きさの倍率(帯を置く曲がった縁を細かくする。`band`)
    chain_scale: HashMap<u32, f64>,
}

impl<'a> Work<'a> {
    pub(crate) fn new(r: &'a Reference, s: Surface) -> Self {
        let mut vt = vec![Vec::new(); s.points.len()];
        for (ti, t) in s.tris.iter().enumerate() {
            for v in t {
                vt[*v as usize].push(ti as u32);
            }
        }
        let size = s.points.iter().map(|p| r.sizing.at(*p)).collect();
        Work {
            r,
            alive: vec![true; s.tris.len()],
            frozen: if s.strip.len() == s.tris.len() {
                s.strip
            } else {
                vec![false; s.tris.len()]
            },
            p: s.points,
            kind: s.kind,
            size,
            t: s.tris,
            lab: s.label,
            vt,
            band: s.band,
            chain_scale: HashMap::new(),
        }
    }

    pub(crate) fn edge_tris(&self, a: u32, b: u32) -> Vec<u32> {
        self.vt[a as usize]
            .iter()
            .copied()
            .filter(|t| self.t[*t as usize].contains(&b))
            .collect()
    }

    fn neighbors(&self, v: u32) -> Vec<u32> {
        let mut n: Vec<u32> = self.vt[v as usize]
            .iter()
            .flat_map(|t| self.t[*t as usize])
            .filter(|w| *w != v)
            .collect();
        n.sort_unstable();
        n.dedup();
        n
    }

    fn edges(&self) -> Vec<(u32, u32)> {
        let mut e: Vec<(u32, u32)> = Vec::new();
        for (ti, t) in self.t.iter().enumerate() {
            if !self.alive[ti] {
                continue;
            }
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if a < b {
                    e.push((a, b));
                }
            }
        }
        e
    }

    fn target(&self, a: u32, b: u32) -> f64 {
        self.size[a as usize].min(self.size[b as usize])
    }

    fn len(&self, a: u32, b: u32) -> f64 {
        dist(self.p[a as usize], self.p[b as usize])
    }

    fn add_tri(&mut self, t: [u32; 3], l: u32) {
        let ti = self.t.len() as u32;
        self.t.push(t);
        self.lab.push(l);
        self.alive.push(true);
        self.frozen.push(false);
        for v in t {
            self.vt[v as usize].push(ti);
        }
    }

    /// 帯の三角形 ti を割れるようにする(中の網で取り戻せない時)。
    pub(crate) fn unfreeze(&mut self, ti: u32) {
        self.frozen[ti as usize] = false;
    }

    /// 点 q が、`around` の点に付く帯の三角形の外接円の中に入るか。入る点があると、その帯の
    /// 三角形は中の網の Delaunay に現れない。
    fn in_band_circle(&self, q: P3, around: &[u32]) -> bool {
        around.iter().any(|&v| {
            self.vt[v as usize].iter().any(|&ti| {
                self.frozen[ti as usize]
                    && crate::volmesh::encroaches(
                        self.t[ti as usize].map(|w| self.p[w as usize]),
                        q,
                    )
            })
        })
    }

    /// 点が帯の三角形に付いているか。
    fn frozen_v(&self, v: u32) -> bool {
        self.vt[v as usize].iter().any(|t| self.frozen[*t as usize])
    }

    fn kill_tri(&mut self, ti: u32) {
        self.alive[ti as usize] = false;
        for v in self.t[ti as usize] {
            self.vt[v as usize].retain(|x| *x != ti);
        }
    }

    /// 点 p(種類 k)の要素の大きさ(帯を置く曲がった縁は細かく)。
    fn size_at(&self, p: P3, k: Kind) -> f64 {
        let scale = match k {
            Kind::Chain(g) => self.chain_scale.get(&g).copied().unwrap_or(1.0),
            _ => 1.0,
        };
        self.r.sizing.at(p) * scale
    }

    fn add_point(&mut self, p: P3, k: Kind) -> u32 {
        self.size.push(self.size_at(p, k));
        self.p.push(p);
        self.kind.push(k);
        self.vt.push(Vec::new());
        (self.p.len() - 1) as u32
    }

    /// 辺 ab の中点の種類(面の境なら鎖、そうでなければ面)。
    fn edge_kind(&self, ts: &[u32]) -> Kind {
        let (l0, l1) = (self.lab[ts[0] as usize], self.lab[ts[1] as usize]);
        if l0 == l1 {
            Kind::Face(l0)
        } else {
            match self.r.group(l0, l1) {
                Some(g) => Kind::Chain(g),
                None => Kind::Face(l0),
            }
        }
    }

    /// 辺 ab を割る(新しい点は形の上へ)。割った点の番号。
    pub(crate) fn split(&mut self, a: u32, b: u32) -> Option<u32> {
        let ts = self.edge_tris(a, b);
        if ts.len() != 2 || ts.iter().any(|t| self.frozen[*t as usize]) {
            return None;
        }
        let k = self.edge_kind(&ts);
        let m0 = lerp(self.p[a as usize], self.p[b as usize], 0.5);
        let q = self.r.project(k, m0);
        if self.in_band_circle(q, &[a, b]) {
            return None;
        }
        let m = self.add_point(q, k);
        for ti in ts {
            let t = self.t[ti as usize];
            let l = self.lab[ti as usize];
            // 辺 (x, y) が a b の向きの所を探す
            let i = (0..3)
                .find(|i| {
                    let (x, y) = (t[*i], t[(*i + 1) % 3]);
                    (x == a && y == b) || (x == b && y == a)
                })
                .expect("辺");
            let (x, y, z) = (t[i], t[(i + 1) % 3], t[(i + 2) % 3]);
            self.kill_tri(ti);
            self.add_tri([x, m, z], l);
            self.add_tri([m, y, z], l);
        }
        Some(m)
    }

    fn rank(k: Kind) -> u8 {
        match k {
            Kind::Face(_) => 0,
            Kind::Chain(_) => 1,
            Kind::Corner => 2,
        }
    }

    /// u を v へ縮められるか確かめて縮める。
    fn collapse(&mut self, u: u32, v: u32) -> bool {
        let (ku, kv) = (self.kind[u as usize], self.kind[v as usize]);
        let ts = self.edge_tris(u, v);
        // 帯の点は消さない(帯の点へ縮めるのはよい)
        if ts.len() != 2 || self.frozen_v(u) {
            return false;
        }
        match ku {
            Kind::Corner => return false,
            Kind::Chain(g) => {
                let ok_v = matches!(kv, Kind::Corner) || kv == Kind::Chain(g);
                if !ok_v || self.edge_kind(&ts) != Kind::Chain(g) {
                    return false;
                }
            }
            Kind::Face(_) => {}
        }
        // 輪の条件: 共通の隣は辺の両側の 2 点だけ
        let opp: Vec<u32> = ts
            .iter()
            .map(|t| {
                *self.t[*t as usize]
                    .iter()
                    .find(|w| **w != u && **w != v)
                    .expect("3 点目")
            })
            .collect();
        let nu = self.neighbors(u);
        let nv = self.neighbors(v);
        let common = nu.iter().filter(|w| nv.contains(w)).count();
        if common != 2 {
            return false;
        }
        let pv = self.p[v as usize];
        let hi = 4.0 / 3.0 * self.size[v as usize].min(self.size[u as usize]);
        for w in &nu {
            if *w != v && !opp.contains(w) && dist(pv, self.p[*w as usize]) > hi {
                return false;
            }
        }
        // 向きが大きく変わる三角形や潰れる三角形を作らない
        let tu: Vec<u32> = self.vt[u as usize].clone();
        for &ti in &tu {
            let t = self.t[ti as usize];
            if t.contains(&v) {
                continue;
            }
            let old = tri_normal(
                self.p[t[0] as usize],
                self.p[t[1] as usize],
                self.p[t[2] as usize],
            );
            let q = t.map(|x| if x == u { pv } else { self.p[x as usize] });
            let new = tri_normal(q[0], q[1], q[2]);
            let (lo, ln) = (norm(old), norm(new));
            if ln <= 1e-12 * hi * hi || dot(old, new) < 0.8 * lo * ln {
                return false;
            }
            if min_angle(q[0], q[1], q[2]) < 3.0 {
                return false;
            }
        }
        for &ti in &ts {
            self.kill_tri(ti);
        }
        for &ti in &self.vt[u as usize].clone() {
            let mut t = self.t[ti as usize];
            let l = self.lab[ti as usize];
            for x in &mut t {
                if *x == u {
                    *x = v;
                }
            }
            self.kill_tri(ti);
            self.add_tri(t, l);
        }
        true
    }

    /// 辺 ab を入れ替える(同じ面の中で、向かいの角の和が 180° を超える時)。
    fn flip(&mut self, a: u32, b: u32) -> bool {
        let ts = self.edge_tris(a, b);
        if ts.len() != 2
            || self.lab[ts[0] as usize] != self.lab[ts[1] as usize]
            || ts.iter().any(|t| self.frozen[*t as usize])
        {
            return false;
        }
        let l = self.lab[ts[0] as usize];
        // t1 = (a, b, c)、t2 = (b, a, d) の向きにそろえる
        let orient = |t: [u32; 3], x: u32, y: u32| -> Option<u32> {
            (0..3)
                .find(|i| t[*i] == x && t[(*i + 1) % 3] == y)
                .map(|i| t[(i + 2) % 3])
        };
        let (t0, t1) = (self.t[ts[0] as usize], self.t[ts[1] as usize]);
        let (c, d) = match (orient(t0, a, b), orient(t1, b, a)) {
            (Some(c), Some(d)) => (c, d),
            _ => match (orient(t1, a, b), orient(t0, b, a)) {
                (Some(c), Some(d)) => (c, d),
                _ => return false,
            },
        };
        if c == d || self.vt[a as usize].len() <= 3 || self.vt[b as usize].len() <= 3 {
            return false;
        }
        if self.neighbors(c).contains(&d) {
            return false;
        }
        let [pa, pb, pc, pd] = [a, b, c, d].map(|x| self.p[x as usize]);
        let angle = |p: P3, q: P3, r: P3| {
            let (u, v) = (sub(q, p), sub(r, p));
            (dot(u, v) / (norm(u) * norm(v))).clamp(-1.0, 1.0).acos()
        };
        if angle(pc, pa, pb) + angle(pd, pb, pa) <= std::f64::consts::PI + 1e-9 {
            return false;
        }
        let (o0, o1) = (tri_normal(pa, pb, pc), tri_normal(pb, pa, pd));
        let (n0, n1) = (tri_normal(pa, pd, pc), tri_normal(pd, pb, pc));
        let ok = |n: P3, o: P3| dot(n, o) > 0.8 * norm(n) * norm(o) && norm(n) > 0.0;
        if !(ok(n0, o0) && ok(n0, o1) && ok(n1, o0) && ok(n1, o1)) {
            return false;
        }
        if min_angle(pa, pd, pc).min(min_angle(pd, pb, pc))
            <= min_angle(pa, pb, pc).min(min_angle(pb, pa, pd))
        {
            // 角が良くならないなら(曲面で Delaunay の判定がずれる時)入れ替えない
            return false;
        }
        for ti in ts {
            self.kill_tri(ti);
        }
        self.add_tri([a, d, c], l);
        self.add_tri([d, b, c], l);
        true
    }

    /// 点 v を動かしても向きが大きく変わらず、まわりのいちばん悪い角が悪くならないか。
    fn move_ok(&self, v: u32, q: P3) -> bool {
        if self.frozen_v(v) {
            return false;
        }
        let around: Vec<u32> = self.vt[v as usize]
            .iter()
            .flat_map(|t| self.t[*t as usize])
            .collect();
        if self.in_band_circle(q, &around) {
            return false;
        }
        let (mut before, mut after) = (180.0f64, 180.0f64);
        for &ti in &self.vt[v as usize] {
            let t = self.t[ti as usize];
            let old = tri_normal(
                self.p[t[0] as usize],
                self.p[t[1] as usize],
                self.p[t[2] as usize],
            );
            let x = t.map(|w| if w == v { q } else { self.p[w as usize] });
            let new = tri_normal(x[0], x[1], x[2]);
            if dot(old, new) < 0.8 * norm(old) * norm(new) || norm(new) <= 0.0 {
                return false;
            }
            before = before.min(min_angle_t(&self.p, t));
            after = after.min(min_angle(x[0], x[1], x[2]));
        }
        after >= before.min(25.0) - 1e-9
    }

    /// 面に沿ってならす。
    fn smooth(&mut self) {
        for v in 0..self.p.len() as u32 {
            if self.vt[v as usize].is_empty() {
                continue;
            }
            let k = self.kind[v as usize];
            let pv = self.p[v as usize];
            let q = match k {
                Kind::Corner => continue,
                Kind::Face(_) => {
                    let ns = self.neighbors(v);
                    let mut c = [0.0; 3];
                    let mut wsum = 0.0;
                    for w in &ns {
                        // 大きさの場に合わせ、小さい所の点ほど重く
                        let s = 1.0 / self.size[*w as usize].max(1e-12);
                        c = add(c, scale(self.p[*w as usize], s));
                        wsum += s;
                    }
                    let c = scale(c, 1.0 / wsum);
                    let mut nrm = [0.0; 3];
                    for &ti in &self.vt[v as usize] {
                        let t = self.t[ti as usize];
                        nrm = add(
                            nrm,
                            tri_normal(
                                self.p[t[0] as usize],
                                self.p[t[1] as usize],
                                self.p[t[2] as usize],
                            ),
                        );
                    }
                    let nrm = crate::geom::unit(nrm);
                    let d = sub(c, pv);
                    let d = sub(d, scale(nrm, dot(nrm, d)));
                    add(pv, scale(d, 0.5))
                }
                // 鎖の点は equalize_chains が並べる
                Kind::Chain(_) => continue,
            };
            let q = self.r.project(k, q);
            if self.move_ok(v, q) {
                self.p[v as usize] = q;
                self.size[v as usize] = self.size_at(q, k);
            }
        }
    }

    /// 鎖の辺(面の境)か。
    fn is_chain_edge(&self, a: u32, b: u32) -> Option<u32> {
        let ts = self.edge_tris(a, b);
        if ts.len() != 2 {
            return None;
        }
        match self.edge_kind(&ts) {
            Kind::Chain(g) => Some(g),
            _ => None,
        }
    }

    /// 鎖を角から角(角の無い輪は 1 周)までの点の列に分ける。輪は始めと終わりが同じ点。
    fn chain_paths(&self) -> Vec<Vec<u32>> {
        let mut used: HashSet<(u32, u32)> = HashSet::new();
        let mut paths = Vec::new();
        let next = |v: u32, prev: u32, g: u32| -> Option<u32> {
            self.neighbors(v)
                .into_iter()
                .find(|w| *w != prev && self.is_chain_edge(v, *w) == Some(g))
        };
        let walk = |start: u32, first: u32, g: u32, used: &mut HashSet<(u32, u32)>| {
            let mut path = vec![start, first];
            used.insert(ek(start, first));
            let (mut prev, mut v) = (start, first);
            while self.kind[v as usize] != Kind::Corner && v != start {
                match next(v, prev, g) {
                    Some(w) if !used.contains(&ek(v, w)) => {
                        used.insert(ek(v, w));
                        path.push(w);
                        prev = v;
                        v = w;
                    }
                    _ => break,
                }
            }
            path
        };
        let verts: Vec<u32> = (0..self.p.len() as u32)
            .filter(|v| !self.vt[*v as usize].is_empty())
            .collect();
        // 角から
        for &c in &verts {
            if self.kind[c as usize] != Kind::Corner {
                continue;
            }
            for w in self.neighbors(c) {
                if let Some(g) = self.is_chain_edge(c, w)
                    && !used.contains(&ek(c, w))
                {
                    paths.push(walk(c, w, g, &mut used));
                }
            }
        }
        // 角の無い輪
        for &v in &verts {
            if let Kind::Chain(g) = self.kind[v as usize]
                && let Some(w) = self
                    .neighbors(v)
                    .into_iter()
                    .find(|w| self.is_chain_edge(v, *w) == Some(g) && !used.contains(&ek(v, *w)))
            {
                paths.push(walk(v, w, g, &mut used));
            }
        }
        paths
    }

    /// 鎖の点を、大きさの場で測って等しい間に並べる。間が不ぞろいだと、鎖に接する四面体の
    /// 形と数が点ごとに変わり、固定した縁などで値が点ごとに上下する(箱の固定した縁で確かめた)。
    /// 数は「長さ / 大きさ」の和を丸めた物にし、多ければ短い所を縮め、少なければ長い所を割る。
    fn equalize_chains(&mut self) -> usize {
        let mut changed = 0;
        for path in self.chain_paths() {
            let n_seg = path.len() - 1;
            let metric: f64 = path
                .windows(2)
                .map(|w| self.len(w[0], w[1]) / self.target(w[0], w[1]))
                .sum();
            // 割る閾値(4/3)を超える辺が残らない数
            let want = (metric.round() as usize)
                .max((metric * 0.75).ceil() as usize)
                .max(1);
            if want > n_seg {
                // 長い辺は split_long が割る。残りの不足は 1 回に 1 つ、いちばん長い辺を割る
                if let Some(w) = path
                    .windows(2)
                    .max_by(|x, y| self.len(x[0], x[1]).total_cmp(&self.len(y[0], y[1])))
                    && self.split(w[0], w[1]).is_some()
                {
                    changed += 1;
                }
                continue;
            }
            if want < n_seg && path.len() > 2 {
                // 両側の辺の和が短い中の点から、短い側の隣へ縮める(多すぎる数まで。1 回には
                // 隣り合わない点だけ。縮めた点の隣は辺の長さが変わるので次の回に回す)
                let mut order: Vec<usize> = (1..path.len() - 1).collect();
                order.sort_by(|i, j| {
                    let s =
                        |k: usize| self.len(path[k - 1], path[k]) + self.len(path[k], path[k + 1]);
                    s(*i).total_cmp(&s(*j))
                });
                let mut touched = vec![false; path.len()];
                let mut removed = 0;
                for i in order {
                    if removed == n_seg - want || touched[i] {
                        continue;
                    }
                    let (u, a, b) = (path[i], path[i - 1], path[i + 1]);
                    let v = if self.len(u, a) < self.len(u, b) {
                        a
                    } else {
                        b
                    };
                    if self.collapse(u, v) {
                        changed += 1;
                        removed += 1;
                        touched[i - 1] = true;
                        touched[i + 1] = true;
                    }
                }
                continue;
            }
            // 数が合っていれば、等しい間へ動かす
            let cum: Vec<f64> = std::iter::once(0.0)
                .chain(path.windows(2).scan(0.0, |acc, w| {
                    *acc += self.len(w[0], w[1]) / self.target(w[0], w[1]);
                    Some(*acc)
                }))
                .collect();
            let pts: Vec<P3> = path.iter().map(|v| self.p[*v as usize]).collect();
            let at = |s: f64| -> P3 {
                let k = cum.partition_point(|c| *c < s).clamp(1, cum.len() - 1);
                let t = ((s - cum[k - 1]) / (cum[k] - cum[k - 1]).max(1e-300)).clamp(0.0, 1.0);
                lerp(pts[k - 1], pts[k], t)
            };
            for (i, &v) in path.iter().enumerate().take(path.len() - 1).skip(1) {
                let k = self.kind[v as usize];
                let goal = self.r.project(k, at(metric * i as f64 / n_seg as f64));
                let pv = self.p[v as usize];
                if dist(goal, pv) < 1e-3 * self.size[v as usize] {
                    continue;
                }
                for t in [1.0, 0.5, 0.25] {
                    let q = self.r.project(k, lerp(pv, goal, t));
                    if self.move_ok(v, q) {
                        self.p[v as usize] = q;
                        self.size[v as usize] = self.size_at(q, k);
                        changed += 1;
                        break;
                    }
                }
            }
        }
        changed
    }

    fn split_long(&mut self) -> usize {
        let mut n = 0;
        for _ in 0..20 {
            let mut es: Vec<(f64, u32, u32)> = self
                .edges()
                .into_iter()
                .filter_map(|(a, b)| {
                    let l = self.len(a, b);
                    (l > 4.0 / 3.0 * self.target(a, b)).then_some((l, a, b))
                })
                .collect();
            if es.is_empty() {
                break;
            }
            es.sort_by(|x, y| y.0.total_cmp(&x.0).then((x.1, x.2).cmp(&(y.1, y.2))));
            for (_, a, b) in es {
                if self.len(a, b) > 4.0 / 3.0 * self.target(a, b) && self.split(a, b).is_some() {
                    n += 1;
                }
            }
        }
        n
    }

    fn collapse_short(&mut self) -> usize {
        let mut es: Vec<(f64, u32, u32)> = self
            .edges()
            .into_iter()
            .filter_map(|(a, b)| {
                let l = self.len(a, b);
                (l < 0.8 * self.target(a, b) && self.is_chain_edge(a, b).is_none())
                    .then_some((l, a, b))
            })
            .collect();
        es.sort_by(|x, y| x.0.total_cmp(&y.0).then((x.1, x.2).cmp(&(y.1, y.2))));
        let mut n = 0;
        for (_, a, b) in es {
            if self.edge_tris(a, b).len() != 2 || self.len(a, b) >= 0.8 * self.target(a, b) {
                continue;
            }
            let (ra, rb) = (
                Self::rank(self.kind[a as usize]),
                Self::rank(self.kind[b as usize]),
            );
            let done = if ra <= rb {
                self.collapse(a, b) || (ra == rb && self.collapse(b, a))
            } else {
                self.collapse(b, a)
            };
            if done {
                n += 1;
            }
        }
        n
    }

    pub(crate) fn flip_all(&mut self) -> usize {
        let mut n = 0;
        for _ in 0..10 {
            let mut k = 0;
            for (a, b) in self.edges() {
                if self.flip(a, b) {
                    k += 1;
                }
            }
            n += k;
            if k == 0 {
                break;
            }
        }
        n
    }

    pub(crate) fn finish(self) -> Surface {
        let mut map = vec![u32::MAX; self.p.len()];
        let mut points = Vec::new();
        let mut kind = Vec::new();
        let mut tris = Vec::new();
        let mut label = Vec::new();
        let mut strip = Vec::new();
        for (ti, t) in self.t.iter().enumerate() {
            if !self.alive[ti] {
                continue;
            }
            strip.push(self.frozen[ti]);
            let t = t.map(|v| {
                if map[v as usize] == u32::MAX {
                    map[v as usize] = points.len() as u32;
                    points.push(self.p[v as usize]);
                    kind.push(self.kind[v as usize]);
                }
                map[v as usize]
            });
            tris.push(t);
            label.push(self.lab[ti]);
        }
        let band = self
            .band
            .iter()
            .map(|b| b.map(|v| map[v as usize]))
            .filter(|b| !b.contains(&u32::MAX))
            .collect();
        Surface {
            points,
            tris,
            label,
            kind,
            band,
            strip,
        }
    }
}

fn min_angle_t(p: &[P3], t: [u32; 3]) -> f64 {
    min_angle(p[t[0] as usize], p[t[1] as usize], p[t[2] as usize])
}

/// 表示用の網を、大きさの場に合わせて張り直す。
pub fn remesh(r: &Reference, s: Surface, cancelled: &dyn Fn() -> bool) -> Result<Surface, String> {
    let mut w = Work::new(r, s);
    for it in 0..10 {
        let ne = w.equalize_chains();
        let ns = w.split_long();
        let nc = w.collapse_short();
        let nf = w.flip_all();
        w.smooth();
        log::debug!(
            "表面の張り直し {it}: 鎖 {ne}・割った辺 {ns}・縮めた辺 {nc}・入れ替えた辺 {nf}"
        );
        if cancelled() {
            return Err("中止しました".into());
        }
    }
    // 鎖の点の数と間をそろえ切る
    for _ in 0..40 {
        if w.equalize_chains() == 0 {
            break;
        }
        w.flip_all();
        w.smooth();
    }
    for _ in 0..3 {
        w.flip_all();
        w.smooth();
    }
    w.flip_all();
    // 帯を置く曲がった縁を細かくし直す
    if w.finer_curved_chains() {
        for _ in 0..3 {
            w.equalize_chains();
            w.split_long();
            w.collapse_short();
            w.flip_all();
            w.smooth();
        }
        for _ in 0..40 {
            if w.equalize_chains() == 0 {
                break;
            }
            w.flip_all();
            w.smooth();
        }
        w.flip_all();
    }
    // 鋭い縁に沿う帯を置き、まわりを整え直す(帯は動かさない)
    if w.add_bands() {
        for _ in 0..4 {
            w.split_long();
            w.collapse_short();
            w.flip_all();
            w.smooth();
        }
        w.flip_all();
    }
    Ok(w.finish())
}

/// 表面の網の確かさ。
#[derive(Debug, Clone, Default)]
pub struct SurfaceStats {
    pub tris: usize,
    pub points: usize,
    /// 1 つの面にしか付かない辺の数(閉じていれば 0)
    pub open_edges: usize,
    /// 三角形のいちばん小さい角: 最小・下から 1%・中央(度)
    pub min_angle: f64,
    pub angle_p1: f64,
    pub angle_median: f64,
    /// 30° 未満の三角形の割合
    pub below_30: f64,
    /// 点の、元の面からの距離の最大(mm。点は面の上に置くので 0 に近い)
    pub point_dev_max: f64,
    /// 三角形の重心の、元の面からの距離の最大(mm。曲面の弦のたるみ。2 次の要素の中点で埋める)
    pub sag_max: f64,
    /// 辺の長さ / 大きさの場: 中央と最大
    pub len_median: f64,
    pub len_max: f64,
}

impl Surface {
    /// 囲んだ体積。
    pub fn volume(&self) -> f64 {
        self.tris
            .iter()
            .map(|t| {
                let p = t.map(|i| self.points[i as usize]);
                crate::geom::tet_volume([0.0; 3], p[0], p[1], p[2])
            })
            .sum()
    }

    pub fn stats(&self, r: &Reference) -> SurfaceStats {
        let mut ec: HashMap<(u32, u32), usize> = HashMap::new();
        for t in &self.tris {
            for k in 0..3 {
                *ec.entry(ek(t[k], t[(k + 1) % 3])).or_default() += 1;
            }
        }
        let mut ang: Vec<f64> = self
            .tris
            .iter()
            .map(|t| min_angle_t(&self.points, *t))
            .collect();
        ang.sort_by(f64::total_cmp);
        let q = |v: &[f64], f: f64| {
            v.get(((v.len().max(1) - 1) as f64 * f) as usize)
                .copied()
                .unwrap_or(0.0)
        };
        let (mut dev, mut sag) = (0.0f64, 0.0f64);
        for (t, l) in self.tris.iter().zip(&self.label) {
            let p = t.map(|i| self.points[i as usize]);
            let c = scale(add(add(p[0], p[1]), p[2]), 1.0 / 3.0);
            sag = sag.max(dist(c, r.on_face(*l, c)));
            for x in p {
                dev = dev.max(dist(x, r.on_face(*l, x)));
            }
        }
        let mut lens: Vec<f64> = ec
            .keys()
            .map(|(a, b)| {
                let (p, q) = (self.points[*a as usize], self.points[*b as usize]);
                dist(p, q) / r.sizing.at(lerp(p, q, 0.5))
            })
            .collect();
        lens.sort_by(f64::total_cmp);
        SurfaceStats {
            tris: self.tris.len(),
            points: self.points.len(),
            open_edges: ec.values().filter(|c| **c != 2).count(),
            min_angle: ang.first().copied().unwrap_or(0.0),
            angle_p1: q(&ang, 0.01),
            angle_median: q(&ang, 0.5),
            below_30: ang.iter().filter(|a| **a < 30.0).count() as f64 / ang.len().max(1) as f64,
            point_dev_max: dev,
            sag_max: sag,
            len_median: q(&lens, 0.5),
            len_max: lens.last().copied().unwrap_or(0.0),
        }
    }
}
