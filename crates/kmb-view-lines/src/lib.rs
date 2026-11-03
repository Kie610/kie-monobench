//! 網と世界座標の線を平行投影し、曲面の輪郭と隠れを求める純関数。

use kmb_core::kernel::{Mesh, P3, cross, dot, normalize, sub};
use std::collections::{BTreeMap, HashMap, HashSet};

/// 視点(どれも長さ 1、互いに直交)。2D の x は `right`、y は `up`。`eye` は視点へ向かう向き。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub right: P3,
    pub up: P3,
    pub eye: P3,
}

impl View {
    /// 視点へ向かう向き `eye` と、画面の上に近い向き `up` から作る。
    pub fn new(eye: P3, up: P3) -> Option<View> {
        let eye = normalize(eye);
        let right = cross(up, eye);
        if dot(right, right) < 1e-12 || !eye.iter().all(|v| v.is_finite()) {
            return None;
        }
        let right = normalize(right);
        Some(View {
            right,
            up: cross(eye, right),
            eye,
        })
    }

    /// 名前の決まった視点(上・下・前・後ろ・右・左・等角)。
    pub fn named(name: &str) -> Option<View> {
        let z = [0.0, 0.0, 1.0];
        match name {
            "top" | "上" => View::new(z, [0.0, 1.0, 0.0]),
            "bottom" | "下" => View::new([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            "front" | "前" => View::new([0.0, -1.0, 0.0], z),
            "back" | "後ろ" => View::new([0.0, 1.0, 0.0], z),
            "right" | "右" => View::new([1.0, 0.0, 0.0], z),
            "left" | "左" => View::new([-1.0, 0.0, 0.0], z),
            "iso" | "等角" => View::new([1.0, -1.0, 1.0], z),
            _ => None,
        }
    }

    pub fn project(&self, p: P3) -> [f64; 3] {
        [dot(p, self.right), dot(p, self.up), dot(p, self.eye)]
    }
}

/// 投影した三角形(2D の 3 点と、各点の近さ)。
struct Tri {
    p: [[f64; 2]; 3],
    z: [f64; 3],
}

/// 投影した三角形を格子に分けて持つ(点の手前にある三角形を速く探すため)。
struct Grid {
    tris: Vec<Tri>,
    lo: [f64; 2],
    cell: f64,
    n: [usize; 2],
    cells: Vec<Vec<u32>>,
}

impl Grid {
    fn new(tris: Vec<Tri>) -> Grid {
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for t in &tris {
            for p in &t.p {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        if tris.is_empty() {
            lo = [0.0; 2];
            hi = [1.0; 2];
        }
        let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-6);
        // 三角形の数に合わせて、1 つの升に平均で数個入るくらいの細かさ(最大 256×256)
        let per = ((tris.len() as f64).sqrt().ceil() as usize).clamp(1, 256);
        let cell = size / per as f64;
        let n = [
            (((hi[0] - lo[0]) / cell) as usize + 1).min(per + 1),
            (((hi[1] - lo[1]) / cell) as usize + 1).min(per + 1),
        ];
        let mut cells = vec![Vec::new(); n[0] * n[1]];
        for (i, t) in tris.iter().enumerate() {
            let (a, b) = Self::span(lo, cell, n, t);
            for y in a[1]..=b[1] {
                for x in a[0]..=b[0] {
                    cells[y * n[0] + x].push(i as u32);
                }
            }
        }
        Grid {
            tris,
            lo,
            cell,
            n,
            cells,
        }
    }

    fn index(lo: [f64; 2], cell: f64, n: [usize; 2], p: [f64; 2]) -> [usize; 2] {
        [0, 1].map(|k| (((p[k] - lo[k]) / cell).max(0.0) as usize).min(n[k] - 1))
    }

    fn span(lo: [f64; 2], cell: f64, n: [usize; 2], t: &Tri) -> ([usize; 2], [usize; 2]) {
        let mn = [0, 1].map(|k| t.p.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min));
        let mx = [0, 1].map(|k| t.p.iter().map(|p| p[k]).fold(f64::NEG_INFINITY, f64::max));
        (Self::index(lo, cell, n, mn), Self::index(lo, cell, n, mx))
    }

    /// 投影した点 `q`(2D と近さ)の手前を、どれかの三角形がふさいでいるか。
    /// 点 `q`(投影した座標)が、手前の三角形に隠れるか。`tol` は奥行きの許容差、`near` は
    /// 三角形の面からこれより近い点を隠さない距離(線と面は別々に網にするので、曲面の縁の線は
    /// 面の網から弦の深さぶん内側へずれる。ずれた点を、その縁に接する三角形が隠さないように)。
    fn covered(&self, q: [f64; 3], tol: f64, near: f64) -> bool {
        let [x, y] = Self::index(self.lo, self.cell, self.n, [q[0], q[1]]);
        self.cells[y * self.n[0] + x].iter().any(|&i| {
            let t = &self.tris[i as usize];
            let [a, b, c] = t.p;
            let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if det.abs() < 1e-14 {
                return false;
            }
            let u = ((q[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (q[1] - a[1])) / det;
            let v = ((b[0] - a[0]) * (q[1] - a[1]) - (q[0] - a[0]) * (b[1] - a[1])) / det;
            let w = 1.0 - u - v;
            // 三角形の縁ちょうどの点は、ふさいでいないとみなす(線そのものが縁にある)
            let e = 1e-9;
            if u < e || v < e || w < e {
                return false;
            }
            let z = w * t.z[0] + u * t.z[1] + v * t.z[2];
            if z <= q[2] + tol {
                return false;
            }
            // 三角形の面までの距離(奥行きの差 × 面の法線の視線向きの成分)
            let n = cross(
                [b[0] - a[0], b[1] - a[1], t.z[1] - t.z[0]],
                [c[0] - a[0], c[1] - a[1], t.z[2] - t.z[0]],
            );
            let len = dot(n, n).sqrt();
            len > 0.0 && (z - q[2]) * n[2].abs() / len > near
        })
    }
}

/// 網の頂点を、位置で 1 つにまとめた番号(面ごとに分かれた同じ位置の頂点を同じにする)。
fn welded(m: &Mesh) -> Vec<u32> {
    let mut map: HashMap<[i64; 3], u32> = HashMap::new();
    m.positions
        .iter()
        .map(|p| {
            let key = p.map(|v| (f64::from(v) * 1e4).round() as i64);
            let next = map.len() as u32;
            *map.entry(key).or_insert(next)
        })
        .collect()
}

/// 曲面の輪郭の線: 同じ面の 2 つの三角形が接する辺のうち、片方が視点を向き、もう片方が向かないもの。
fn silhouette_segments(m: &Mesh, view: &View) -> Vec<(Option<u32>, Vec<P3>)> {
    let id = welded(m);
    let at = |i: u32| m.positions[i as usize].map(f64::from);
    let facing: Vec<bool> = m
        .triangles
        .iter()
        .map(|t| {
            let n = cross(sub(at(t[1]), at(t[0])), sub(at(t[2]), at(t[0])));
            dot(n, view.eye) > 0.0
        })
        .collect();
    // 辺(まとめた頂点の組)ごとの、(三角形の番号、元の頂点 2 つ)。線を書く順がこの並びになるので、
    // 毎回同じ順の BTreeMap にする(HashMap の巡回順に出力を依存させない)
    type Uses = Vec<(usize, u32, u32)>;
    let mut edges: BTreeMap<(u32, u32), Uses> = BTreeMap::new();
    for (k, t) in m.triangles.iter().enumerate() {
        for j in 0..3 {
            let (a, b) = (t[j], t[(j + 1) % 3]);
            let (wa, wb) = (id[a as usize], id[b as usize]);
            if wa == wb {
                continue;
            }
            edges
                .entry((wa.min(wb), wa.max(wb)))
                .or_default()
                .push((k, a, b));
        }
    }
    let face = |k: usize| m.triangle_faces.get(k).copied();
    let mut out = Vec::new();
    for list in edges.values() {
        if let [(k1, a, b), (k2, _, _)] = list[..]
            && face(k1) == face(k2)
            && facing[k1] != facing[k2]
        {
            out.push((face(k1), vec![at(a), at(b)]));
        }
    }
    out
}

/// 曲面の輪郭の線を、名前の有無にかかわらず返す。
pub fn silhouettes(mesh: &Mesh, view: &View) -> Vec<Vec<P3>> {
    silhouette_segments(mesh, view)
        .into_iter()
        .map(|(_, p)| p)
        .collect()
}

/// 同じ線(両端を位置でまとめて同じ組)を 1 本にする。
fn dedup(lines: Vec<Vec<P3>>) -> Vec<Vec<P3>> {
    let key = |p: &P3| p.map(|v| (v * 1e4).round() as i64);
    let mut seen = HashSet::new();
    lines
        .into_iter()
        .filter(|l| l.len() >= 2)
        .filter(|l| {
            let mut ks: Vec<[i64; 3]> = l.iter().map(key).collect();
            if ks.first() > ks.last() {
                ks.reverse();
            }
            seen.insert(ks)
        })
        .collect()
}

/// 投影した線の、見える所か隠れた所の連なり(2D の折れ線、見えるか)。
type Run = (Vec<[f64; 2]>, bool);

/// 投影した線を、見える所と隠れた所の連なりに分ける。
fn split_lines(lines: &[Vec<P3>], view: &View, grid: &Grid, size: f64, chord: f64) -> Vec<Run> {
    let step = (size / 400.0).max(0.05);
    let tol = 1e-6 * size.max(1.0) + 1e-4;
    let near = 2.5 * chord + tol;
    let mut out: Vec<Run> = Vec::new();
    for l in lines {
        let pts: Vec<[f64; 3]> = l.iter().map(|p| view.project(*p)).collect();
        let mut cur: Option<Run> = None;
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            // 視線に沿った線(点に見える)は書かない
            if len < 1e-9 {
                continue;
            }
            let n = ((len / step).ceil() as usize).max(1);
            let lerp = |t: f64| [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t);
            for i in 0..n {
                let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
                let seen = !grid.covered(lerp((t0 + t1) / 2.0), tol, near);
                let (p0, p1) = (lerp(t0), lerp(t1));
                match &mut cur {
                    Some((poly, v)) if *v == seen => poly.push([p1[0], p1[1]]),
                    _ => {
                        if let Some(done) = cur.take() {
                            out.push(done);
                        }
                        cur = Some((vec![[p0[0], p0[1]], [p1[0], p1[1]]], seen));
                    }
                }
            }
        }
        out.extend(cur);
    }
    // 同じ向きに続く点を間引く(まっすぐな線は両端だけにする)
    for (poly, _) in &mut out {
        let mut keep: Vec<[f64; 2]> = Vec::with_capacity(poly.len());
        for p in poly.iter() {
            if keep.len() >= 2 {
                let (a, b) = (keep[keep.len() - 2], keep[keep.len() - 1]);
                let c = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                let d = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                let forward = (b[0] - a[0]) * (p[0] - b[0]) + (b[1] - a[1]) * (p[1] - b[1]);
                if d > 0.0 && forward >= 0.0 && (c / d).abs() < 1e-7 * size.max(1.0) {
                    keep.pop();
                }
            }
            keep.push(*p);
        }
        *poly = keep;
    }
    out
}

/// 見える所と隠れた所に分けた、投影した線(2D の折れ線、見えるか)。`sketches` は一緒に書く
/// スケッチの線(世界座標の折れ線。ボディの線と同じく、手前にボディがあれば隠れ線)。
/// `chord` は網の許容差 mm。
pub fn view_runs(
    meshes: &[Mesh],
    sketches: &[Vec<P3>],
    view: &View,
    chord: f64,
) -> Vec<(Vec<[f64; 2]>, bool)> {
    let mut lines: Vec<Vec<P3>> = sketches.iter().filter(|l| !l.is_empty()).cloned().collect();
    for m in meshes {
        lines.extend(
            m.edges
                .iter()
                .map(|e| e.iter().map(|p| p.map(f64::from)).collect::<Vec<P3>>()),
        );
        lines.extend(silhouettes(m, view));
    }
    let lines = dedup(lines);
    project_lines(meshes, &lines, view, chord)
}

/// 線の囲み(最小の角と最大の角)。線が無ければ原点。
pub fn runs_bounds(runs: &[(Vec<[f64; 2]>, bool)]) -> ([f64; 2], [f64; 2]) {
    let (lo, hi) = runs.iter().flat_map(|r| r.0.iter()).fold(
        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
        |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        },
    );
    if lo[0].is_finite() {
        (lo, hi)
    } else {
        ([0.0; 2], [0.0; 2])
    }
}

/// 指定した世界座標の線だけを、全ボディの網による隠れで分ける。
/// 稜線の名前を持つ呼び手が、線と名前の対応を保つために使う。
pub fn project_lines(
    meshes: &[Mesh],
    lines: &[Vec<P3>],
    view: &View,
    chord: f64,
) -> Vec<(Vec<[f64; 2]>, bool)> {
    Projector::new(meshes, view, chord).lines(lines)
}

/// 一回の投影の間だけ持つ三角形の格子。世界座標の線の名前を呼び手が保てる。
pub struct Projector {
    grid: Grid,
    view: View,
    chord: f64,
    size: f64,
}
impl Projector {
    pub fn new(meshes: &[Mesh], view: &View, chord: f64) -> Self {
        let tris = meshes
            .iter()
            .flat_map(|m| {
                m.triangles.iter().map(|t| {
                    let q = t.map(|i| view.project(m.positions[i as usize].map(f64::from)));
                    Tri {
                        p: q.map(|v| [v[0], v[1]]),
                        z: q.map(|v| v[2]),
                    }
                })
            })
            .collect();
        let grid = Grid::new(tris);
        let size = (grid.n[0].max(grid.n[1]) as f64 * grid.cell).max(1.0);
        Self {
            grid,
            view: *view,
            chord,
            size,
        }
    }
    pub fn lines(&self, lines: &[Vec<P3>]) -> Vec<(Vec<[f64; 2]>, bool)> {
        split_lines(lines, &self.view, &self.grid, self.size, self.chord)
    }
}

/// 曲面の輪郭を、その面の名前とともに返す。
pub fn face_silhouettes(mesh: &Mesh, view: &View) -> Vec<(String, Vec<P3>)> {
    silhouette_lines(mesh, view)
        .into_iter()
        .filter_map(|(face, p)| Some((face?, p)))
        .collect()
}

/// 面の名前を持たない網の輪郭も省かず返す。
pub fn silhouette_lines(mesh: &Mesh, view: &View) -> Vec<(Option<String>, Vec<P3>)> {
    silhouette_segments(mesh, view)
        .into_iter()
        .map(|(face, p)| {
            (
                face.and_then(|i| mesh.face_names.get(i as usize)).cloned(),
                p,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_circle_seen_edge_on_keeps_its_diameter() {
        let points = (0..=32)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / 32.;
                [5. * a.cos(), 5. * a.sin(), 0.]
            })
            .collect();
        let runs = project_lines(&[], &[points], &View::named("front").unwrap(), 0.01);
        let (lo, hi) = runs_bounds(&runs);
        assert!((hi[0] - lo[0] - 10.).abs() < 1e-8, "{runs:?}");
    }
}
