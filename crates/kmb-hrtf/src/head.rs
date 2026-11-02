//! 頭の網の整え: 確かめ・小さな穴の埋め・耳からの距離で要素の大きさを変える作り直し。
//!
//! 境界要素法の重さは三角形の数の 2 乗で増える。HRTF は耳の近くの形(耳介の溝)で高い周波数が
//! 大きく変わり、耳から遠い所(特に反対側)はあまり効かない。そこで、要素の大きさを耳の入口からの
//! 距離で変える(耳の近く 1 mm から、反対の耳で 15 mm まで直線に大きくするのが既定)。片耳ずつ
//! 解くので、耳ごとに「その耳の近くだけ細かい」網を作る(既定)。
//!
//! 作り直しは強度の解析と同じ表面の張り直し(`kmb_fea::surfmesh`。辺を割る・縮める・入れ替える・
//! ならすの繰り返しで、点は元の表面へ戻す)に、耳からの距離の大きさの場を渡すだけにする。
//!
//! 確かめ(作り直しの前と後):
//! - 頂点の番号・数でない値・潰れた三角形・3 枚以上が付く辺・向きのそろわない辺は誤り。
//! - 全体が裏返った網(囲んだ体積が負)は向きを裏返して使う(形は変わらない)。
//! - 穴(1 枚の三角形にしか付かない辺の輪)は、差し渡しが辺の長さの中央の 10 倍未満なら輪の重心から
//!   扇に埋める。大きい穴は場所と大きさを書いて誤りにする(黙って埋めない)。
//! - 自分と交わる網(隣り合わない三角形どうしが交わる)は、場所を書いて誤りにする。

use std::collections::HashMap;

use kmb_core::Mesh;
use kmb_fea::geom::{P3, add, cross, dist, dot, norm, scale, segment_triangle, sub};
use kmb_fea::surfmesh::{SizeRule, Sizing, reference, remesh};
use kmb_wave::BemMesh;
use serde::{Deserialize, Serialize};

use crate::HrtfError;

/// 小さい穴とみなす差し渡し(辺の長さの中央に対する倍数)。
const SMALL_HOLE: f64 = 10.0;

/// 耳からの距離で要素の大きさを変える作り直しの設定(長さは m)。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grading {
    /// 耳の入口での辺の長さ
    #[serde(default = "default_near")]
    pub near: f64,
    /// いちばん大きい辺の長さ
    #[serde(default = "default_far")]
    pub far: f64,
    /// 耳から 1 m 離れるごとに増やす辺の長さ(単位の無い割合)。無ければ、耳から反対の耳までで
    /// `near` から `far` になる割合
    #[serde(default)]
    pub grade: Option<f64>,
    /// 耳ごとに網を作る(解く耳の近くだけ細かい)。false なら両耳の近くを細かくした 1 つの網
    #[serde(default = "default_per_ear")]
    pub per_ear: bool,
}

fn default_near() -> f64 {
    0.001
}
fn default_far() -> f64 {
    0.015
}
fn default_per_ear() -> bool {
    true
}

impl Default for Grading {
    fn default() -> Self {
        Self {
            near: default_near(),
            far: default_far(),
            grade: None,
            per_ear: default_per_ear(),
        }
    }
}

impl Grading {
    /// 割合(無ければ両耳の間の距離から決める)。
    pub fn grade_for(&self, ears: &[P3; 2]) -> Result<f64, HrtfError> {
        if !(self.near.is_finite() && self.near > 0.0 && self.far.is_finite())
            || self.far < self.near
        {
            return Err(HrtfError::input(format!(
                "辺の長さ(耳の近く {} m・いちばん大きい {} m)が正の数でないか、大小が逆",
                self.near, self.far
            )));
        }
        let g = match self.grade {
            Some(g) => g,
            None => {
                let span = dist(ears[0], ears[1]);
                if span.is_nan() || span <= 0.0 {
                    return Err(HrtfError::input("左と右の耳の入口が同じ点"));
                }
                (self.far - self.near) / span
            }
        };
        if !(g.is_finite() && g > 0.0) {
            return Err(HrtfError::input(format!(
                "大きさを増やす割合 {g} が正の数でない"
            )));
        }
        Ok(g)
    }
}

/// 網の穴(1 枚の三角形にしか付かない辺の輪)。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hole {
    /// 輪の点の重心(網と同じ単位)
    pub center: P3,
    /// 輪の差し渡し(点どうしのいちばん長い距離)
    pub span: f64,
    /// 輪の点の数
    pub points: usize,
}

/// 整えた網と、したこと。
#[derive(Clone, Debug, PartialEq)]
pub struct Tidy {
    pub mesh: BemMesh,
    /// 埋めた小さな穴
    pub filled: Vec<Hole>,
    /// 全体の向きを裏返したか
    pub flipped: bool,
}

/// 網を確かめ、小さな穴を埋め、裏返った網を外向きにする。大きな穴・自分との交わり・壊れた網は誤り。
pub fn tidy(mesh: &BemMesh) -> Result<Tidy, HrtfError> {
    basic_check(mesh)?;
    let mut out = BemMesh {
        vertices: mesh.vertices.clone(),
        triangles: mesh.triangles.clone(),
        groups: Vec::new(),
    };
    let holes = find_holes(mesh)?;
    let typical = median_edge(mesh);
    if let Some(big) = holes.iter().find(|h| h.span >= SMALL_HOLE * typical) {
        return Err(HrtfError::input(format!(
            "網に大きな穴がある(中心 {}、差し渡し {:.1} mm、辺の長さの中央 {:.2} mm の {SMALL_HOLE} 倍以上)。\
             穴を塞いだ網にしてください",
            mm(big.center),
            big.span * 1000.0,
            typical * 1000.0
        )));
    }
    for (h, ring) in holes.iter().zip(hole_rings(mesh)?) {
        let c = out.vertices.len() as u32;
        out.vertices.push(h.center);
        for k in 0..ring.len() {
            let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
            // 境の辺 a→b は既にある三角形の向き。埋める三角形は b→a の向きで付ける
            out.triangles.push([b, a, c]);
        }
    }
    let mut flipped = false;
    if signed_volume(&out) < 0.0 {
        log::warn!("頭の網の三角形が内向きなので、向きを裏返して使います");
        for t in &mut out.triangles {
            t.swap(1, 2);
        }
        flipped = true;
    }
    if !holes.is_empty() {
        log::info!("頭の網の小さな穴を {} 個埋めました", holes.len());
    }
    if let Some(p) = self_intersection(&out) {
        return Err(HrtfError::input(format!(
            "網が自分と交わっている({} の近く)",
            mm(p)
        )));
    }
    Ok(Tidy {
        mesh: out,
        filled: holes,
        flipped,
    })
}

/// 網を整えてから、`fine` の点(耳の入口)の近くを細かく、離れるほど粗く作り直す。
/// 作り直した網も確かめる(閉じて向きがそろい、自分と交わらない)。
pub fn regrade(
    mesh: &BemMesh,
    fine: &[P3],
    near: f64,
    far: f64,
    grade: f64,
    cancelled: &dyn Fn() -> bool,
) -> Result<BemMesh, HrtfError> {
    let tidied = tidy(mesh)?.mesh;
    // 表面の張り直しは mm で考えるので、mm に直して渡す
    let display = Mesh {
        positions: tidied
            .vertices
            .iter()
            .map(|p| p.map(|x| (x * 1000.0) as f32))
            .collect(),
        triangles: tidied.triangles.clone(),
        triangle_faces: vec![0; tidied.triangles.len()],
        face_names: vec!["頭".into()],
        ..Mesh::default()
    };
    let (far_mm, near_mm) = (far * 1000.0, near * 1000.0);
    let (mut r, s0) = reference(&display, far_mm, SizeRule::default()).map_err(HrtfError::input)?;
    r.sizing = Sizing::graded(
        far_mm,
        grade,
        fine.iter()
            .map(|p| (p.map(|x| x * 1000.0), near_mm))
            .collect(),
    );
    let s = remesh(&r, s0, cancelled).map_err(HrtfError::compute)?;
    if cancelled() {
        return Err(HrtfError::compute("中止しました"));
    }
    let out = BemMesh {
        vertices: s.points.iter().map(|p| p.map(|x| x / 1000.0)).collect(),
        triangles: s.tris,
        groups: Vec::new(),
    };
    basic_check(&out).map_err(|e| e.at("作り直した網"))?;
    if !find_holes(&out)?.is_empty() {
        return Err(HrtfError::compute("作り直した網が閉じていない"));
    }
    if let Some(p) = self_intersection(&out) {
        return Err(HrtfError::compute(format!(
            "作り直した網が自分と交わった({} の近く)。耳の近くの辺の長さを小さくしてください",
            mm(p)
        )));
    }
    Ok(out)
}

/// 穴(1 枚の三角形にしか付かない辺の輪)を探す。
pub fn find_holes(mesh: &BemMesh) -> Result<Vec<Hole>, HrtfError> {
    Ok(hole_rings(mesh)?
        .iter()
        .map(|ring| {
            let pts: Vec<P3> = ring.iter().map(|&i| mesh.vertices[i as usize]).collect();
            let center = scale(
                pts.iter().fold([0.0; 3], |a, p| add(a, *p)),
                1.0 / pts.len() as f64,
            );
            let mut span: f64 = 0.0;
            for (i, a) in pts.iter().enumerate() {
                for b in &pts[i + 1..] {
                    span = span.max(dist(*a, *b));
                }
            }
            Hole {
                center,
                span,
                points: pts.len(),
            }
        })
        .collect())
}

/// 番号・数でない値・潰れた三角形・3 枚以上が付く辺・向きのそろわない辺を確かめる。
fn basic_check(mesh: &BemMesh) -> Result<(), HrtfError> {
    if mesh.triangles.is_empty() {
        return Err(HrtfError::input("網に三角形が無い"));
    }
    let nv = mesh.vertices.len();
    if mesh.vertices.iter().flatten().any(|x| !x.is_finite()) {
        return Err(HrtfError::input("網の頂点に数でない値がある"));
    }
    let mut directed: HashMap<(u32, u32), u32> = HashMap::new();
    for (t, tri) in mesh.triangles.iter().enumerate() {
        if tri.iter().any(|&i| i as usize >= nv) {
            return Err(HrtfError::input(format!(
                "三角形 {t} が無い頂点を指している"
            )));
        }
        let [a, b, c] = tri.map(|i| mesh.vertices[i as usize]);
        let size = dist(a, b).max(dist(b, c)).max(dist(c, a));
        if norm(cross(sub(b, a), sub(c, a))) <= 1e-12 * size * size {
            return Err(HrtfError::input(format!(
                "三角形 {t} が潰れている({} の近く)",
                mm(a)
            )));
        }
        for k in 0..3 {
            *directed.entry((tri[k], tri[(k + 1) % 3])).or_default() += 1;
        }
    }
    for (&(a, b), &n) in &directed {
        if n > 1 {
            let p = mesh.vertices[a as usize];
            return Err(HrtfError::input(format!(
                "三角形の向きがそろっていないか、3 枚以上の三角形が付く辺がある(頂点 {a}–{b}、{} の近く)",
                mm(p)
            )));
        }
    }
    Ok(())
}

/// 穴の輪(境の辺を、三角形の向きに沿ってたどった頂点の並び)。
fn hole_rings(mesh: &BemMesh) -> Result<Vec<Vec<u32>>, HrtfError> {
    let mut directed: HashMap<(u32, u32), ()> = HashMap::new();
    for tri in &mesh.triangles {
        for k in 0..3 {
            directed.insert((tri[k], tri[(k + 1) % 3]), ());
        }
    }
    let mut next: HashMap<u32, u32> = HashMap::new();
    let mut starts: Vec<u32> = Vec::new();
    for &(a, b) in directed.keys() {
        if !directed.contains_key(&(b, a)) {
            if next.insert(a, b).is_some() {
                return Err(HrtfError::input(format!(
                    "網の 1 つの頂点で 2 つの穴が接している({} の近く)",
                    mm(mesh.vertices[a as usize])
                )));
            }
            starts.push(a);
        }
    }
    starts.sort_unstable();
    let mut used: HashMap<u32, ()> = HashMap::new();
    let mut rings = Vec::new();
    for s in starts {
        if used.contains_key(&s) {
            continue;
        }
        let mut ring = vec![s];
        used.insert(s, ());
        let mut cur = next[&s];
        while cur != s {
            if used.insert(cur, ()).is_some() {
                return Err(HrtfError::input("網の穴の縁がたどれない"));
            }
            ring.push(cur);
            cur = *next
                .get(&cur)
                .ok_or_else(|| HrtfError::input("網の穴の縁がたどれない"))?;
        }
        rings.push(ring);
    }
    Ok(rings)
}

/// 隣り合わない(頂点を共にしない)三角形どうしが交わる所を 1 つ返す。
fn self_intersection(mesh: &BemMesh) -> Option<P3> {
    let tri = |t: usize| mesh.triangles[t].map(|i| mesh.vertices[i as usize]);
    let cell = 2.0 * median_edge(mesh);
    let key = |p: P3| p.map(|x| (x / cell).floor() as i64);
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let mut boxes = Vec::with_capacity(mesh.triangles.len());
    for t in 0..mesh.triangles.len() {
        let v = tri(t);
        let lo = [0, 1, 2].map(|a| v[0][a].min(v[1][a]).min(v[2][a]));
        let hi = [0, 1, 2].map(|a| v[0][a].max(v[1][a]).max(v[2][a]));
        let (kl, kh) = (key(lo), key(hi));
        for x in kl[0]..=kh[0] {
            for y in kl[1]..=kh[1] {
                for z in kl[2]..=kh[2] {
                    grid.entry([x, y, z]).or_default().push(t as u32);
                }
            }
        }
        boxes.push((lo, hi));
    }
    let overlap = |a: usize, b: usize| {
        let ((la, ha), (lb, hb)) = (boxes[a], boxes[b]);
        (0..3).all(|k| la[k] <= hb[k] && lb[k] <= ha[k])
    };
    let crosses = |p: [P3; 3], q: [P3; 3]| {
        (0..3).find_map(|k| {
            let (a, b) = (p[k], p[(k + 1) % 3]);
            segment_triangle(a, b, q[0], q[1], q[2]).map(|s| add(a, scale(sub(b, a), s)))
        })
    };
    let mut seen: Vec<u32> = Vec::new();
    for (t, &(lo, hi)) in boxes.iter().enumerate() {
        let (kl, kh) = (key(lo), key(hi));
        seen.clear();
        for x in kl[0]..=kh[0] {
            for y in kl[1]..=kh[1] {
                for z in kl[2]..=kh[2] {
                    if let Some(list) = grid.get(&[x, y, z]) {
                        seen.extend(list.iter().copied().filter(|&u| u as usize > t));
                    }
                }
            }
        }
        seen.sort_unstable();
        seen.dedup();
        let tt = mesh.triangles[t];
        for &u in &seen {
            let u = u as usize;
            if mesh.triangles[u].iter().any(|i| tt.contains(i)) || !overlap(t, u) {
                continue;
            }
            let (p, q) = (tri(t), tri(u));
            if let Some(x) = crosses(p, q).or_else(|| crosses(q, p)) {
                return Some(x);
            }
        }
    }
    None
}

fn signed_volume(mesh: &BemMesh) -> f64 {
    mesh.triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| mesh.vertices[i as usize]);
            dot(a, cross(b, c)) / 6.0
        })
        .sum()
}

fn median_edge(mesh: &BemMesh) -> f64 {
    let mut lens: Vec<f64> = mesh
        .triangles
        .iter()
        .flat_map(|t| {
            let [a, b, c] = t.map(|i| mesh.vertices[i as usize]);
            [dist(a, b), dist(b, c), dist(c, a)]
        })
        .collect();
    lens.sort_by(f64::total_cmp);
    lens.get(lens.len() / 2).copied().unwrap_or(0.0)
}

/// 点を mm で書く(誤りの文の場所)。
fn mm(p: P3) -> String {
    format!(
        "({:.1}, {:.1}, {:.1}) mm",
        p[0] * 1000.0,
        p[1] * 1000.0,
        p[2] * 1000.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dir;
    use crate::compute::{Directions, HeadJob, head_meshes, transfer};
    use kmb_wave::exact::sphere_plane_wave;
    use kmb_wave::{Complex64, Medium, Solver, icosphere};

    const A: f64 = 0.0875;

    fn edges(m: &BemMesh) -> Vec<(P3, f64)> {
        m.triangles
            .iter()
            .flat_map(|t| {
                let [a, b, c] = t.map(|i| m.vertices[i as usize]);
                [(a, b), (b, c), (c, a)]
            })
            .map(|(a, b)| (scale(add(a, b), 0.5), dist(a, b)))
            .collect()
    }

    fn median(mut v: Vec<f64>) -> f64 {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    }

    /// 確かめること: 三角形 1 枚分の小さな穴は埋めて閉じ、裏返った網は外向きに直し、広く欠けた網と
    /// 自分と交わる網は場所を書いた誤りになる。
    #[test]
    fn tidy_fills_small_holes_and_rejects_big_ones() {
        let sphere = icosphere(A, 3);
        let mut one = sphere.clone();
        one.triangles.remove(7);
        let t = tidy(&one).unwrap();
        assert_eq!(t.filled.len(), 1);
        assert!(find_holes(&t.mesh).unwrap().is_empty());
        assert_eq!(t.mesh.triangles.len(), sphere.triangles.len() + 2);
        assert!(!t.flipped);

        let mut inverted = sphere.clone();
        for tri in &mut inverted.triangles {
            tri.swap(1, 2);
        }
        let t = tidy(&inverted).unwrap();
        assert!(t.flipped);
        assert!(signed_volume(&t.mesh) > 0.0);

        // 上半分を取り除くと、差し渡しが直径ほどの穴になる
        let mut half = sphere.clone();
        half.triangles
            .retain(|t| t.iter().any(|&i| sphere.vertices[i as usize][2] < -0.2 * A));
        let e = tidy(&half).unwrap_err();
        assert!(e.message.contains("大きな穴"), "{}", e.message);

        // 少しずらして重ねた 2 つの球
        let mut two = sphere.clone();
        let n = two.vertices.len() as u32;
        two.vertices
            .extend(sphere.vertices.iter().map(|p| add(*p, [0.5 * A, 0.0, 0.0])));
        two.triangles
            .extend(sphere.triangles.iter().map(|t| t.map(|i| i + n)));
        let e = tidy(&two).unwrap_err();
        assert!(e.message.contains("自分と交わ"), "{}", e.message);
    }

    /// 確かめること: 球を耳の近く 2 mm・最大 15 mm で作り直すと、耳の近くの辺は 2 mm ほど、反対側は
    /// 15 mm ほどになり、点は球の上に残り、網は閉じている。三角形は一様に 2 mm で張るより大幅に少ない。
    #[test]
    fn regrade_is_fine_near_the_ear_and_coarse_far_away() {
        let sphere = icosphere(A, 4);
        let ear = [0.0, A, 0.0];
        let (near, far) = (0.002, 0.015);
        let grade = (far - near) / (2.0 * A);
        let m = regrade(&sphere, &[ear], near, far, grade, &|| false).unwrap();
        let es = edges(&m);
        let close = median(
            es.iter()
                .filter(|(p, _)| dist(*p, ear) < 0.01)
                .map(|e| e.1)
                .collect(),
        );
        let away = median(
            es.iter()
                .filter(|(p, _)| p[1] < -0.8 * A)
                .map(|e| e.1)
                .collect(),
        );
        assert!((0.0015..0.003).contains(&close), "耳の近く {close}");
        assert!((0.010..0.018).contains(&away), "反対側 {away}");
        let off = m
            .vertices
            .iter()
            .map(|p| (norm(*p) - A).abs())
            .fold(0.0, f64::max);
        assert!(off < 0.002, "球から {off}");
        assert!(find_holes(&m).unwrap().is_empty());
        let uniform = 4.0 * std::f64::consts::PI * A * A / (0.433 * near * near);
        assert!(
            (m.triangles.len() as f64) < 0.2 * uniform,
            "三角形 {}(一様なら約 {uniform:.0})",
            m.triangles.len()
        );
    }

    /// 確かめること: 細かい球(三角形 5120)を耳ごとに耳の近く 4 mm・最大 25 mm で作り直すと、三角形は
    /// 一様な 1280 の網より少なく、剛い球の HRTF の級数解と ka ≈ 0.4〜3.2 の全部の方向で大きさ 0.5 dB・
    /// 位相 0.1 rad(2 kHz で 8 μs)以内に合う(遠い所を粗くしても耳で受ける音は大きく変わらない)。
    /// 2026-11-01 の実測は三角形 1162、0.33 dB・0.085 rad(一様な 1280 では 0.06 dB・0.013 rad)。
    #[test]
    fn regraded_sphere_head_matches_the_series() {
        let ears = [[0.0, A, 0.0], [0.0, -A, 0.0]];
        let dirs: Vec<Dir> = [
            (0.0, 0.0),
            (45.0, 0.0),
            (90.0, 0.0),
            (135.0, 0.0),
            (180.0, 0.0),
            (270.0, 0.0),
            (0.0, 60.0),
            (90.0, -60.0),
        ]
        .map(|(az, el)| Dir { az, el, dist: 1.0 })
        .to_vec();
        let job = HeadJob {
            mesh: icosphere(A, 4),
            center: [0.0; 3],
            ears,
            ear_radius: 0.0,
            grading: Some(Grading {
                near: 0.004,
                far: 0.025,
                ..Grading::default()
            }),
            directions: Directions::List { dirs: dirs.clone() },
            distance: None,
            fs: 8000.0,
            taps: 32,
            max_hz: 2000.0,
            lead: 4.0,
            medium: Medium::default(),
            solver: Solver::default(),
        };
        let meshes = head_meshes(&job, &|| false).unwrap();
        let t = transfer(&job, &()).unwrap();
        assert_eq!(t.frequencies.len(), 8);
        assert!(
            t.mesh_triangles.iter().all(|&n| n < 1280),
            "{:?}",
            t.mesh_triangles
        );
        let c = job.medium.speed_of_sound;
        let (mut worst_db, mut worst_rad): (f64, f64) = (0.0, 0.0);
        for (ear, m) in meshes.iter().enumerate() {
            // 振動させた三角形の重心を耳の点として級数解と比べる
            let [p, q, r] = m.triangles[t.ear_triangles[ear][0]].map(|i| m.vertices[i as usize]);
            let e = scale(add(add(p, q), r), 1.0 / 3.0);
            let eu = scale(e, 1.0 / norm(e));
            for (fi, f) in t.frequencies.iter().enumerate() {
                let k = 2.0 * std::f64::consts::PI * f / c;
                for (di, d) in dirs.iter().enumerate() {
                    let u = crate::grid::unit(d.az, d.el);
                    let want =
                        sphere_plane_wave(k, A, norm(e), -dot(eu, u), Complex64::new(0.0, 0.0));
                    let got = t.h[ear][fi][di];
                    worst_db = worst_db.max((20.0 * (got.norm() / want.norm()).log10()).abs());
                    worst_rad = worst_rad.max((got / want).arg().abs());
                }
            }
        }
        assert!(worst_db < 0.5, "大きさの差 {worst_db:.3} dB");
        assert!(worst_rad < 0.1, "位相の差 {worst_rad:.3} rad");
    }

    /// 確かめること: 耳の所の出っ張り(高さ 6 mm・幅 10 mm)は、耳の近くを 1 mm にした作り直しの後も
    /// 高さが 0.3 mm 以内で残る(耳の近くの形を細かい網で保つ)。
    #[test]
    fn regrade_keeps_the_shape_near_the_ear() {
        let mut mesh = icosphere(A, 5);
        let d = [0.0, 1.0, 0.0];
        for p in &mut mesh.vertices {
            let u = scale(*p, 1.0 / norm(*p));
            *p = scale(u, A + 0.006 * (-(dist(u, d) * A / 0.01).powi(2)).exp());
        }
        let top = |m: &BemMesh| m.vertices.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        let ear = [0.0, top(&mesh), 0.0];
        let m = regrade(&mesh, &[ear], 0.001, 0.015, 0.1, &|| false).unwrap();
        assert!(
            (top(&m) - ear[1]).abs() < 0.0003,
            "{} と {}",
            top(&m),
            ear[1]
        );
        assert!(
            m.triangles.len() < mesh.triangles.len() / 2,
            "{}",
            m.triangles.len()
        );
    }
}
