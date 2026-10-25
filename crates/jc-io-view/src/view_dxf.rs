//! 今の視点から見たボディの線を、2D の DXF にする。
//!
//! 線は網の稜線(面の境目)と、曲面の輪郭(視線に対して表と裏が入れ替わる所)。視線に沿った
//! 平行投影で、1 mm を 1 単位(mm)のまま書く(レーザー加工の下地にそのまま使える大きさ)。
//! 見えるかどうかは、線を短く刻んだ各区間の真ん中の点で、手前に三角形があるかで決める。
//! 隠れ線は省くか、破線の画層 `HIDDEN` に分ける。
//!
//! 書き出しは DXF R12(AC1009)の最小の形(HEADER・TABLES・ENTITIES)。線は POLYLINE。

use jc_core::kernel::{Mesh, P3, cross, dot, normalize, sub};
use jc_format::dxf;
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

    fn project(&self, p: P3) -> [f64; 3] {
        [dot(p, self.right), dot(p, self.up), dot(p, self.eye)]
    }
}

/// 書き出しの結果(DXF の文字列と、見える線・隠れ線の本数)。
#[derive(Debug, Clone, PartialEq)]
pub struct ViewDxf {
    pub dxf: String,
    pub visible: usize,
    pub hidden: usize,
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
fn silhouettes(m: &Mesh, view: &View) -> Vec<Vec<P3>> {
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
    // 毎回同じ順の BTreeMap にする(HashMap は起動ごとに順が変わり、同じ形の DXF の中身が毎回違った)
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
            out.push(vec![at(a), at(b)]);
        }
    }
    out
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
                if d > 0.0 && (c / d).abs() < 1e-7 * size.max(1.0) {
                    keep.pop();
                }
            }
            keep.push(*p);
        }
        *poly = keep;
    }
    out
}

/// 数を短く書く(`-0` は `0`)。
fn num(v: f64) -> String {
    let v = if v.abs() < 5e-7 { 0.0 } else { v };
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    }
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
    let mut tris = Vec::new();
    let mut lines: Vec<Vec<P3>> = sketches.iter().filter(|l| !l.is_empty()).cloned().collect();
    for m in meshes {
        let at = |i: u32| view.project(m.positions[i as usize].map(f64::from));
        for t in &m.triangles {
            let q = t.map(at);
            tris.push(Tri {
                p: q.map(|v| [v[0], v[1]]),
                z: q.map(|v| v[2]),
            });
        }
        lines.extend(
            m.edges
                .iter()
                .map(|e| e.iter().map(|p| p.map(f64::from)).collect::<Vec<P3>>()),
        );
        lines.extend(silhouettes(m, view));
    }
    let lines = dedup(lines);
    let grid = Grid::new(tris);
    let size = ((grid.n[0].max(grid.n[1])) as f64 * grid.cell).max(1.0);
    split_lines(&lines, view, &grid, size, chord)
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

/// 網 `meshes` とスケッチの線 `sketches` を視点 `view` から見た線を DXF にする。`hidden` が真なら
/// 隠れ線も破線の画層に書く。`chord` は網にしたときの許容差 mm(曲面の縁の線と面の網のずれを、
/// 隠れの判定で見込む)。
pub fn view_to_dxf(
    meshes: &[Mesh],
    sketches: &[Vec<P3>],
    view: &View,
    hidden: bool,
    chord: f64,
) -> ViewDxf {
    let runs = view_runs(meshes, sketches, view, chord);
    let (lo, hi) = runs_bounds(&runs);

    let mut o = dxf::R12::begin(
        num,
        lo,
        hi,
        &[
            ("0", 7, dxf::CONTINUOUS),
            ("VISIBLE", 7, dxf::CONTINUOUS),
            ("HIDDEN", 8, dxf::DASHED),
        ],
    );
    let (mut visible, mut hid) = (0, 0);
    for (poly, seen) in &runs {
        if !seen && !hidden {
            continue;
        }
        let layer = if *seen { "VISIBLE" } else { "HIDDEN" };
        if *seen {
            visible += 1;
        } else {
            hid += 1;
        }
        match poly[..] {
            [a, b] => o.line(layer, a, b),
            _ => o.polyline(layer, poly),
        }
    }
    ViewDxf {
        dxf: o.finish(),
        visible,
        hidden: hid,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 軸に平行な箱の網(面ごとに頂点を分け、外向きの巻き。稜線は 12 本)。
    pub(crate) fn boxed(lo: P3, hi: P3) -> Mesh {
        let mut m = Mesh::default();
        let c = |x: usize, y: usize, z: usize| {
            [
                if x == 0 { lo[0] } else { hi[0] },
                if y == 0 { lo[1] } else { hi[1] },
                if z == 0 { lo[2] } else { hi[2] },
            ]
        };
        // 各面の 4 隅(外から見て反時計回り)
        let faces = [
            [c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0)], // 下
            [c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)], // 上
            [c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1)], // 前
            [c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0)], // 後ろ
            [c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0)], // 左
            [c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)], // 右
        ];
        for (k, f) in faces.iter().enumerate() {
            let base = m.positions.len() as u32;
            for p in f {
                m.positions.push(p.map(|v| v as f32));
                m.normals.push([0.0; 3]);
            }
            m.triangles.push([base, base + 1, base + 2]);
            m.triangles.push([base, base + 2, base + 3]);
            m.triangle_faces.extend([k as u32, k as u32]);
            m.face_names.push(format!("f{k}"));
        }
        let e = |a: P3, b: P3| vec![a.map(|v| v as f32), b.map(|v| v as f32)];
        for (a, b) in [
            ((0, 0, 0), (1, 0, 0)),
            ((0, 1, 0), (1, 1, 0)),
            ((0, 0, 1), (1, 0, 1)),
            ((0, 1, 1), (1, 1, 1)),
            ((0, 0, 0), (0, 1, 0)),
            ((1, 0, 0), (1, 1, 0)),
            ((0, 0, 1), (0, 1, 1)),
            ((1, 0, 1), (1, 1, 1)),
            ((0, 0, 0), (0, 0, 1)),
            ((1, 0, 0), (1, 0, 1)),
            ((0, 1, 0), (0, 1, 1)),
            ((1, 1, 0), (1, 1, 1)),
        ] {
            m.edges.push(e(c(a.0, a.1, a.2), c(b.0, b.1, b.2)));
        }
        m
    }

    /// 表示中のスケッチの線も書く。ボディの線と同じく、手前にボディがあれば隠れ線
    #[test]
    fn sketch_lines_are_written_with_the_bodies() {
        let m = boxed([0.0; 3], [40.0, 20.0, 10.0]);
        let top = View::named("top").unwrap();
        // 箱の横の床の線(見える)と、箱の下に隠れた床の線
        let beside = vec![[50.0, 0.0, 0.0], [60.0, 10.0, 0.0]];
        let under = vec![[10.0, 5.0, 0.0], [30.0, 8.0, 0.0]];
        let r = view_to_dxf(std::slice::from_ref(&m), &[beside, under], &top, true, 0.02);
        assert_eq!((r.visible, r.hidden), (8 + 1, 1), "{}", r.dxf);
        assert_eq!(r.hidden, 1, "{}", r.dxf);
        assert!(r.dxf.contains("60.0"), "{}", r.dxf);
        let svg = crate::view_svg::view_to_svg(
            &[m],
            &[vec![[50.0, 0.0, 0.0], [60.0, 10.0, 0.0]]],
            &top,
            false,
            0.02,
        );
        // 箱の輪郭 1 つと、横の線 1 本
        assert_eq!(svg.visible, 2, "{}", svg.svg);
    }

    #[test]
    fn a_box_from_the_top_shows_its_outline_and_hides_nothing_under_it() {
        let m = boxed([0.0; 3], [40.0, 20.0, 10.0]);
        let top = View::named("top").unwrap();
        let r = view_to_dxf(std::slice::from_ref(&m), &[], &top, true, 0.02);
        // 上から: 上面の縁 4 本が見え、下面の縁 4 本は上面の縁と重なる(縁ちょうどは隠さない)。
        // 縦の 4 本は点に見えるので書かない
        assert_eq!(r.hidden, 0, "{}", r.dxf);
        assert_eq!(r.visible, 8);
        assert!(r.dxf.contains("$INSUNITS"));
        assert!(r.dxf.contains("40.0"), "1 mm を 1 単位のまま書く");
        assert!(r.dxf.trim_end().ends_with("EOF"));
    }

    #[test]
    fn a_box_from_an_iso_view_has_three_hidden_edges() {
        let m = boxed([0.0; 3], [40.0, 20.0, 10.0]);
        let iso = View::named("iso").unwrap();
        let with = view_to_dxf(std::slice::from_ref(&m), &[], &iso, true, 0.02);
        // 等角の視点からは、奥の角に集まる 3 本の稜線が隠れる
        assert_eq!(with.hidden, 3, "{}", with.dxf);
        assert_eq!(with.visible, 9);
        assert!(with.dxf.contains("HIDDEN") && with.dxf.contains("DASHED"));
        let without = view_to_dxf(&[m], &[], &iso, false, 0.02);
        assert_eq!(without.hidden, 0);
        assert_eq!(without.visible, 9);
        assert!(!without.dxf.contains("\n  8\nHIDDEN\n"), "隠れ線を書かない");
    }

    #[test]
    fn a_front_box_hides_the_part_of_a_box_behind_it() {
        // 前の低い箱(高さ 5)が、後ろの高い箱(高さ 20)の下の方を隠す
        let front = boxed([0.0, 0.0, 0.0], [40.0, 10.0, 5.0]);
        let back = boxed([10.0, 20.0, 0.0], [30.0, 30.0, 20.0]);
        let view = View::named("front").unwrap();
        let r = view_to_dxf(&[front, back], &[], &view, true, 0.02);
        // 後ろの箱の縦の稜線は、下の 5 mm だけ隠れる
        // 見える線は前の箱の 8 本と、後ろの箱の横の 4 本と縦の上の 15 mm の 4 本
        assert_eq!(r.hidden, 4, "{}", r.dxf);
        assert_eq!(r.visible, 16, "{}", r.dxf);
    }

    /// 円柱(半径 5・高さ 10)を 24 角柱で近づけた網の側面(1 つの面)。
    fn prism_side() -> Mesh {
        let mut m = Mesh::default();
        let n = 24;
        for i in 0..n {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let b = std::f64::consts::TAU * (i + 1) as f64 / n as f64;
            let p = |t: f64, z: f64| [(5.0 * t.cos()) as f32, (5.0 * t.sin()) as f32, z as f32];
            let base = m.positions.len() as u32;
            m.positions
                .extend([p(a, 0.0), p(b, 0.0), p(b, 10.0), p(a, 10.0)]);
            m.normals.extend([[0.0; 3]; 4]);
            m.triangles.push([base, base + 1, base + 2]);
            m.triangles.push([base, base + 2, base + 3]);
            m.triangle_faces.extend([0, 0]);
        }
        m
    }

    #[test]
    fn silhouettes_come_from_faces_that_turn_away() {
        // 上から見ると輪郭は無く、前から見ると両脇に出る
        let m = prism_side();
        let front = View::named("front").unwrap();
        let s = silhouettes(&m, &front);
        assert_eq!(s.len(), 2, "左右の 2 本");
        for l in &s {
            assert!((l[0][2] - l[1][2]).abs() > 9.0, "縦の線");
        }
        assert!(silhouettes(&m, &View::named("top").unwrap()).is_empty());
    }

    /// 同じ形の DXF は毎回同じ中身(輪郭の線を HashMap の順で書くと、起動ごとに線の順が変わる。
    /// HashMap は作るたびに順の種が替わるので、1 つの試験の中で何度も作れば違いが出る)。
    #[test]
    fn the_same_view_writes_the_same_dxf_every_time() {
        let m = prism_side();
        let front = View::named("front").unwrap();
        let first = view_to_dxf(std::slice::from_ref(&m), &[], &front, true, 0.02).dxf;
        for _ in 0..20 {
            assert_eq!(
                view_to_dxf(std::slice::from_ref(&m), &[], &front, true, 0.02).dxf,
                first
            );
        }
    }

    #[test]
    fn views_are_orthonormal_and_reject_a_bad_up() {
        let v = View::new([1.0, 1.0, 1.0], [0.0, 0.0, 1.0]).unwrap();
        for (a, b) in [(v.right, v.up), (v.up, v.eye), (v.eye, v.right)] {
            assert!(dot(a, b).abs() < 1e-12);
        }
        assert!(View::new([0.0, 0.0, 1.0], [0.0, 0.0, 1.0]).is_none());
        assert!(View::named("斜め").is_none());
    }

    #[test]
    fn the_root_edge_of_a_round_boss_stays_visible_where_the_face_mesh_is_finer() {
        // 円柱(半径 5・高さ 5)の側面を 64 分割、付け根の縁の線を 32 分割にする(実際のカーネルも
        // 線と面を別々に網にし、線の弦の真ん中が側面の網より内側に入る)
        let (r, h, nf, ne) = (5.0f64, 5.0f64, 64, 32);
        let at = |k: usize, n: usize, z: f64| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            [r * a.cos(), r * a.sin(), z].map(|v| v as f32)
        };
        let mut m = Mesh::default();
        for k in 0..nf {
            let base = m.positions.len() as u32;
            for p in [
                at(k, nf, 0.0),
                at(k + 1, nf, 0.0),
                at(k + 1, nf, h),
                at(k, nf, h),
            ] {
                m.positions.push(p);
                m.normals.push([0.0; 3]);
            }
            m.triangles.push([base, base + 1, base + 2]);
            m.triangles.push([base, base + 2, base + 3]);
            m.triangle_faces.extend([0, 0]);
        }
        // 上のふた(扇形)
        let center = m.positions.len() as u32;
        m.positions.push([0.0, 0.0, h as f32]);
        m.normals.push([0.0; 3]);
        for k in 0..nf {
            let base = m.positions.len() as u32;
            m.positions.extend([at(k, nf, h), at(k + 1, nf, h)]);
            m.normals.extend([[0.0; 3]; 2]);
            m.triangles.push([center, base, base + 1]);
            m.triangle_faces.push(1);
        }
        m.face_names.extend(["側面".into(), "上".into()]);
        m.edges.push((0..=ne).map(|k| at(k, ne, 0.0)).collect());
        m.edges.push((0..=ne).map(|k| at(k, ne, h)).collect());
        let iso = View::named("iso").unwrap();
        let out = view_to_dxf(std::slice::from_ref(&m), &[], &iso, true, 0.02);
        // 見える線: 上の縁、輪郭 2 本、付け根の手前の半周(書き始めの角度で 2 つに分かれる)。
        // 隠れ線は付け根の奥の半周だけ
        assert_eq!((out.visible, out.hidden), (5, 1), "{}", out.dxf);
    }
}
