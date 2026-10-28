//! 加工用の幾何。形状カーネルには触らず、三角形の網([`Mesh`])だけから作る。
//!
//! - 水平な平らな面([`FlatFace`]): 高さと、外周・内周の輪郭([`Area`])
//! - 縦の円筒面([`Bore`]): 穴とボス。中心、半径、高さの範囲
//! - 輪郭のオフセット([`offset_area`])。cavalier_contours を使い、円弧はそのまま円弧で返す
//!
//! 網の円は折れ線になっているので、輪郭は [`fit_arcs`] で円弧に戻してから使う。
//! 座標は作業座標系(WCS)に移したものを返す。
//! 閉じた折れ線の輪をたどる部品は [`ring`]。

pub mod ring;

use cavalier_contours::polyline::{PlineOffsetOptions, PlineSource, PlineSourceMut, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};
use kmb_core::Mesh;
use std::collections::HashMap;
use std::f64::consts::PI;

pub type P2 = [f64; 2];

/// 輪郭の頂点。`bulge` は次の頂点までの円弧のふくらみ(tan(回転角 / 4)、0 なら直線)。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    pub bulge: f64,
}

/// 閉じた輪郭。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Loop {
    pub verts: Vec<Vertex>,
}

/// 1 つの区間: 始点、終点、円弧なら中心と向き(反時計回りなら真)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Line { a: P2, b: P2 },
    Arc { a: P2, b: P2, center: P2, ccw: bool },
}

impl Loop {
    pub fn from_points(pts: &[P2]) -> Loop {
        Loop {
            verts: pts
                .iter()
                .map(|p| Vertex {
                    x: p[0],
                    y: p[1],
                    bulge: 0.0,
                })
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.verts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.verts.is_empty()
    }

    pub fn segments(&self) -> Vec<Seg> {
        let n = self.verts.len();
        (0..n)
            .map(|i| {
                let v = self.verts[i];
                let w = self.verts[(i + 1) % n];
                seg_of([v.x, v.y], [w.x, w.y], v.bulge)
            })
            .collect()
    }

    /// 符号つきの面積(反時計回りが正)。円弧のふくらみも含める。
    pub fn signed_area(&self) -> f64 {
        to_pline(self).area()
    }

    pub fn reversed(&self) -> Loop {
        let mut pl = to_pline(self);
        pl.invert_direction_mut();
        from_pline(&pl)
    }

    /// 円弧を `tol` 以下の誤差の折れ線にした点の列。
    pub fn flatten(&self, tol: f64) -> Vec<P2> {
        let mut out = Vec::new();
        for s in self.segments() {
            match s {
                Seg::Line { a, .. } => out.push(a),
                Seg::Arc { a, b, center, ccw } => {
                    let r = (a[0] - center[0]).hypot(a[1] - center[1]);
                    let sweep = sweep_of(a, b, center, ccw);
                    let step = if r > tol {
                        2.0 * (1.0 - tol / r).acos()
                    } else {
                        PI / 4.0
                    };
                    let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).max(1);
                    let a0 = (a[1] - center[1]).atan2(a[0] - center[0]);
                    for k in 0..n {
                        let t = a0 + sweep * k as f64 / n as f64;
                        out.push([center[0] + r * t.cos(), center[1] + r * t.sin()]);
                    }
                }
            }
        }
        out
    }

    /// 点が輪郭の内側にあるか(向きによらない)。
    pub fn contains(&self, p: P2) -> bool {
        point_in_polygon(&self.flatten(1e-3), p)
    }

    /// 外接する箱 (min, max)。
    pub fn bounds(&self) -> (P2, P2) {
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for p in self.flatten(1e-3) {
            for i in 0..2 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        (min, max)
    }
}

fn seg_of(a: P2, b: P2, bulge: f64) -> Seg {
    if bulge.abs() < 1e-12 {
        return Seg::Line { a, b };
    }
    // ふくらみから中心を求める
    let sweep = 4.0 * bulge.atan();
    let chord = [b[0] - a[0], b[1] - a[1]];
    let c = chord[0].hypot(chord[1]);
    let r = c / (2.0 * (sweep / 2.0).sin().abs());
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let h = (r * r - c * c / 4.0).max(0.0).sqrt();
    // 反時計回りで回転角が半周未満なら、中心は弦の左側
    let left = [-chord[1] / c, chord[0] / c];
    let s = if (bulge > 0.0) == (sweep.abs() < PI) {
        1.0
    } else {
        -1.0
    };
    Seg::Arc {
        a,
        b,
        center: [mid[0] + left[0] * h * s, mid[1] + left[1] * h * s],
        ccw: bulge > 0.0,
    }
}

/// 円弧の回転角(反時計回りが正)。
pub fn sweep_of(a: P2, b: P2, center: P2, ccw: bool) -> f64 {
    let a0 = (a[1] - center[1]).atan2(a[0] - center[0]);
    let a1 = (b[1] - center[1]).atan2(b[0] - center[0]);
    let mut s = a1 - a0;
    if ccw {
        while s <= 1e-12 {
            s += 2.0 * PI;
        }
    } else {
        while s >= -1e-12 {
            s -= 2.0 * PI;
        }
    }
    s
}

/// 折れ線の輪(最後の点から最初の点へ閉じる)の符号つきの面積(反時計回りが正)。
pub fn polygon_area(pts: &[P2]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

/// 点 `q` から線分 `a`-`b` までの距離。
pub fn seg_dist(q: P2, a: P2, b: P2) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (q[0] - a[0] - dx * t).hypot(q[1] - a[1] - dy * t)
}

/// 点 `p` が閉じた折れ線 `poly` の内側か(偶奇の規則。式はコアの [`kmb_core::point_in_polygon`])。
pub fn point_in_polygon(poly: &[P2], p: P2) -> bool {
    kmb_core::point_in_polygon(p, poly)
}

fn to_pline(l: &Loop) -> Polyline<f64> {
    let mut pl = Polyline::new_closed();
    for v in &l.verts {
        pl.add(v.x, v.y, v.bulge);
    }
    pl
}

fn from_pline(pl: &Polyline<f64>) -> Loop {
    Loop {
        verts: (0..pl.vertex_count())
            .map(|i| {
                let v = pl.at(i);
                Vertex {
                    x: v.x,
                    y: v.y,
                    bulge: v.bulge,
                }
            })
            .collect(),
    }
}

/// 外周(反時計回り)と内周(時計回り)で囲まれた領域。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Area {
    pub outer: Loop,
    pub holes: Vec<Loop>,
}

impl Area {
    /// 向きを揃える(外周は反時計回り、内周は時計回り)。
    pub fn normalized(mut self) -> Area {
        if self.outer.signed_area() < 0.0 {
            self.outer = self.outer.reversed();
        }
        for h in &mut self.holes {
            if h.signed_area() > 0.0 {
                *h = h.reversed();
            }
        }
        self
    }

    pub fn area(&self) -> f64 {
        self.outer.signed_area().abs()
            - self
                .holes
                .iter()
                .map(|h| h.signed_area().abs())
                .sum::<f64>()
    }

    pub fn contains(&self, p: P2) -> bool {
        self.outer.contains(p) && !self.holes.iter().any(|h| h.contains(p))
    }
}

/// 領域を `d` だけ内側へ寄せる(負なら外側へ広げる)。分かれた領域ごとに返す。
pub fn offset_area(area: &Area, d: f64) -> Vec<Area> {
    let a = area.clone().normalized();
    let mut plines = vec![to_pline(&a.outer)];
    plines.extend(a.holes.iter().map(to_pline));
    let shape = Shape::from_plines(plines);
    let off = shape.parallel_offset(d, &ShapeOffsetOptions::default());
    let mut areas: Vec<Area> = off
        .ccw_plines
        .iter()
        .map(|p| Area {
            outer: from_pline(&p.polyline),
            holes: vec![],
        })
        .collect();
    for p in &off.cw_plines {
        let h = from_pline(&p.polyline);
        let q = [h.verts[0].x, h.verts[0].y];
        // 内周を含む外周のうち、いちばん小さいものに入れる
        if let Some(a) = areas
            .iter_mut()
            .filter(|a| a.outer.contains(q))
            .min_by(|x, y| {
                x.outer
                    .signed_area()
                    .abs()
                    .total_cmp(&y.outer.signed_area().abs())
            })
        {
            a.holes.push(h);
        }
    }
    areas.into_iter().map(Area::normalized).collect()
}

/// 1 本の閉じた輪郭を `d` だけ外側(負なら内側)へずらす。分かれたら全部返す。
pub fn offset_loop_outward(l: &Loop, d: f64) -> Vec<Loop> {
    let area = Area {
        outer: l.clone(),
        holes: vec![],
    };
    offset_area(&area, -d)
        .into_iter()
        .map(|a| a.outer)
        .collect()
}

/// 開いた折れ線 `pts` を、進む向きの左へ `d`(負なら右)だけずらした折れ線(点の列)。
/// 出っ張った角は円弧で回り(誤差 `tol` の折れ線にする)、へこんだ角の食い込む所は切る。
/// 分かれたら全部返す。
pub fn offset_open(pts: &[P2], d: f64, tol: f64) -> Vec<Vec<P2>> {
    let mut pl = Polyline::new();
    for p in pts {
        pl.add(p[0], p[1], 0.0);
    }
    if pl.vertex_count() < 2 {
        return vec![];
    }
    let opt = PlineOffsetOptions {
        handle_self_intersects: true,
        ..Default::default()
    };
    pl.parallel_offset_opt(d, &opt)
        .iter()
        .map(|o| {
            let n = o.vertex_count();
            let mut out: Vec<P2> = Vec::new();
            for i in 0..n {
                let v = o.at(i);
                out.push([v.x, v.y]);
                if i + 1 < n && v.bulge.abs() > 1e-12 {
                    let w = o.at(i + 1);
                    let l = Loop {
                        verts: vec![
                            Vertex {
                                x: v.x,
                                y: v.y,
                                bulge: v.bulge,
                            },
                            Vertex {
                                x: w.x,
                                y: w.y,
                                bulge: 0.0,
                            },
                        ],
                    };
                    // 2 点の輪郭の初めの区間が円弧(始点は足し済み)
                    let arc: Vec<P2> = l.flatten(tol);
                    let k = arc
                        .iter()
                        .position(|q| (q[0] - w.x).hypot(q[1] - w.y) < 1e-12)
                        .unwrap_or(arc.len());
                    out.extend(arc[1..k].iter().copied());
                }
            }
            out
        })
        .filter(|o| o.len() >= 2)
        .collect()
}

/// 折れ線の点の列を、誤差 `tol` 以内で直線と円弧にまとめる(閉じた輪郭)。
pub fn fit_arcs(pts: &[P2], tol: f64) -> Loop {
    let pts = dedup_closed(pts, 1e-6);
    let n = pts.len();
    if n < 3 {
        return Loop::from_points(&pts);
    }
    // 角(向きが急に変わる点)から始めると、円弧が始点をまたがない
    let start = (0..n)
        .max_by(|&i, &j| turn(&pts, i).total_cmp(&turn(&pts, j)))
        .unwrap_or(0);
    let p: Vec<P2> = (0..=n).map(|k| pts[(start + k) % n]).collect();
    let mut verts = Vec::new();
    let mut i = 0;
    while i < n {
        // できるだけ長い円弧を探す(3 区間以上、回転角は半周まで)
        let mut best: Option<(usize, f64)> = None;
        let mut j = i + 3;
        while j <= n {
            match arc_through(&p[i..=j], tol) {
                Some(b) => {
                    best = Some((j, b));
                    j += 1;
                }
                None => break,
            }
        }
        if let Some((j, bulge)) = best {
            verts.push(Vertex {
                x: p[i][0],
                y: p[i][1],
                bulge,
            });
            i = j;
            continue;
        }
        // 直線: 同じ直線に乗る点はまとめる
        let mut j = i + 1;
        while j < n && on_line(&p[i..=j + 1], tol) {
            j += 1;
        }
        verts.push(Vertex {
            x: p[i][0],
            y: p[i][1],
            bulge: 0.0,
        });
        i = j;
    }
    merge_wrapped_arc(&mut verts, tol);
    Loop { verts }
}

/// 始点が円弧の途中に来た時、最後と最初の円弧を 1 つにまとめる。
fn merge_wrapped_arc(verts: &mut Vec<Vertex>, tol: f64) {
    let n = verts.len();
    if n < 3 {
        return;
    }
    let (first, last) = (verts[0], verts[n - 1]);
    if first.bulge == 0.0 || last.bulge == 0.0 || (first.bulge > 0.0) != (last.bulge > 0.0) {
        return;
    }
    let s1 = seg_of([last.x, last.y], [first.x, first.y], last.bulge);
    let s2 = seg_of([first.x, first.y], [verts[1].x, verts[1].y], first.bulge);
    let (Seg::Arc { center: c1, .. }, Seg::Arc { center: c2, .. }) = (s1, s2) else {
        return;
    };
    let r1 = (first.x - c1[0]).hypot(first.y - c1[1]);
    let r2 = (first.x - c2[0]).hypot(first.y - c2[1]);
    if (c1[0] - c2[0]).hypot(c1[1] - c2[1]) > tol || (r1 - r2).abs() > tol {
        return;
    }
    let sweep = 4.0 * last.bulge.atan() + 4.0 * first.bulge.atan();
    if sweep.abs() > PI + 1e-9 {
        return;
    }
    verts[n - 1].bulge = (sweep / 4.0).tan();
    verts.remove(0);
}

fn dedup_closed(pts: &[P2], eps: f64) -> Vec<P2> {
    let mut out: Vec<P2> = Vec::with_capacity(pts.len());
    for p in pts {
        if out
            .last()
            .is_none_or(|q: &P2| (p[0] - q[0]).hypot(p[1] - q[1]) > eps)
        {
            out.push(*p);
        }
    }
    while out.len() > 1 {
        let (a, b) = (out[0], out[out.len() - 1]);
        if (a[0] - b[0]).hypot(a[1] - b[1]) <= eps {
            out.pop();
        } else {
            break;
        }
    }
    out
}

fn turn(p: &[P2], i: usize) -> f64 {
    let n = p.len();
    let a = p[(i + n - 1) % n];
    let b = p[i];
    let c = p[(i + 1) % n];
    let u = [b[0] - a[0], b[1] - a[1]];
    let v = [c[0] - b[0], c[1] - b[1]];
    let cross = u[0] * v[1] - u[1] * v[0];
    let dot = u[0] * v[0] + u[1] * v[1];
    cross.atan2(dot).abs()
}

fn on_line(p: &[P2], tol: f64) -> bool {
    let (a, b) = (p[0], p[p.len() - 1]);
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]);
    if l < 1e-12 {
        return false;
    }
    p[1..p.len() - 1].iter().all(|q| {
        let t = ((q[0] - a[0]) * d[0] + (q[1] - a[1]) * d[1]) / (l * l);
        let dist = ((q[0] - a[0]) * d[1] - (q[1] - a[1]) * d[0]).abs() / l;
        dist <= tol && (0.0..=1.0).contains(&t)
    })
}

/// 点の列が 1 つの円弧に乗るなら、そのふくらみを返す。
fn arc_through(p: &[P2], tol: f64) -> Option<f64> {
    let (a, m, b) = (p[0], p[p.len() / 2], p[p.len() - 1]);
    let center = circumcenter(a, m, b)?;
    let r = (a[0] - center[0]).hypot(a[1] - center[1]);
    if !r.is_finite() || r > 1e4 {
        return None;
    }
    // 向きは中間点の側で決まる
    let cross = (m[0] - a[0]) * (b[1] - a[1]) - (m[1] - a[1]) * (b[0] - a[0]);
    let ccw = cross > 0.0;
    let sweep = sweep_of(a, b, center, ccw);
    if sweep.abs() > PI + 1e-9 {
        return None;
    }
    // すべての点が円に乗り、角度が単調に進むこと
    let a0 = (a[1] - center[1]).atan2(a[0] - center[0]);
    let mut last = 0.0;
    for q in p {
        let d = (q[0] - center[0]).hypot(q[1] - center[1]);
        if (d - r).abs() > tol {
            return None;
        }
        let mut t = (q[1] - center[1]).atan2(q[0] - center[0]) - a0;
        if ccw {
            while t < -1e-9 {
                t += 2.0 * PI;
            }
        } else {
            while t > 1e-9 {
                t -= 2.0 * PI;
            }
        }
        if (ccw && t < last - 1e-9) || (!ccw && t > last + 1e-9) {
            return None;
        }
        last = t;
    }
    // 区間の弦の中点も円に近いこと(粗い折れ線を円弧にしない)
    for w in p.windows(2) {
        let mid = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
        let d = (mid[0] - center[0]).hypot(mid[1] - center[1]);
        if (r - d).abs() > tol * 2.0 + 0.02 {
            return None;
        }
    }
    Some((sweep / 4.0).tan())
}

fn circumcenter(a: P2, b: P2, c: P2) -> Option<P2> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let a2 = a[0] * a[0] + a[1] * a[1];
    let b2 = b[0] * b[0] + b[1] * b[1];
    let c2 = c[0] * c[0] + c[1] * c[1];
    Some([
        (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d,
        (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d,
    ])
}

/// 水平な平らな面。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FlatFace {
    pub name: String,
    pub z: f64,
    /// 上を向いているか(下向きなら偽)
    pub up: bool,
    pub area: Area,
}

/// 縦の円筒面(穴かボス)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Bore {
    pub center: P2,
    pub radius: f64,
    pub z_min: f64,
    pub z_max: f64,
    /// 穴なら真(面が軸の側を向く)、ボスなら偽
    pub hole: bool,
    pub faces: Vec<String>,
}

impl Bore {
    pub fn diameter(&self) -> f64 {
        self.radius * 2.0
    }
}

/// 加工に使う形の情報。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct MachiningModel {
    pub flats: Vec<FlatFace>,
    pub bores: Vec<Bore>,
    pub min: [f64; 3],
    pub max: [f64; 3],
    /// 作業座標系の三角形(高さを調べるのに使う)
    pub triangles: Vec<[[f64; 3]; 3]>,
    /// 三角形ごとの面の番号(`face_names` の添字)。3D の加工の範囲を面で決めるのに使う
    pub triangle_faces: Vec<u32>,
    pub face_names: Vec<String>,
}

/// 円弧に戻す時の誤差(mm)。網の許容差より少し大きくする。
pub const FIT_TOLERANCE: f64 = 0.02;

type Key = (i64, i64, i64);

impl MachiningModel {
    /// 網から作る。`origin` を引いて作業座標系に移す。
    pub fn from_mesh(mesh: &Mesh, origin: [f64; 3]) -> MachiningModel {
        let pos: Vec<[f64; 3]> = mesh
            .positions
            .iter()
            .map(|p| {
                [
                    p[0] as f64 - origin[0],
                    p[1] as f64 - origin[1],
                    p[2] as f64 - origin[2],
                ]
            })
            .collect();
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for p in &pos {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        let mut by_face: HashMap<u32, Vec<[u32; 3]>> = HashMap::new();
        for (t, f) in mesh.triangle_faces.iter().enumerate() {
            by_face.entry(*f).or_default().push(mesh.triangles[t]);
        }
        let mut faces: Vec<_> = by_face.into_iter().collect();
        faces.sort_by_key(|(f, _)| *f);
        let mut flats = Vec::new();
        let mut bores: Vec<Bore> = Vec::new();
        for (f, tris) in faces {
            let name = mesh.face_names.get(f as usize).cloned().unwrap_or_default();
            // 細い三角形は位置から求めた法線がぶれるので、網の頂点の法線を使う
            let normals: Vec<Option<[f64; 3]>> = tris
                .iter()
                .map(|&t| vertex_normal(mesh, t).or_else(|| tri_normal(&pos, t)))
                .collect();
            let real: Vec<[f64; 3]> = normals.iter().flatten().copied().collect();
            if real.is_empty() {
                continue;
            }
            let all_up = real.iter().all(|n| n[2] > 0.999);
            let all_down = real.iter().all(|n| n[2] < -0.999);
            if all_up || all_down {
                let n = (tris.len() * 3) as f64;
                let z = tris
                    .iter()
                    .flat_map(|t| t.iter().map(|&i| pos[i as usize][2]))
                    .sum::<f64>()
                    / n;
                if let Some(area) = area_from_loops(boundary_loops(&pos, &tris)) {
                    flats.push(FlatFace {
                        name,
                        z,
                        up: all_up,
                        area,
                    });
                }
                continue;
            }
            if real.iter().all(|n| n[2].abs() < 0.02)
                && let Some(b) = fit_bore(&pos, &tris, &normals, name)
            {
                // 半分ずつの面(円が 2 つに分かれる)をまとめる
                if let Some(o) = bores.iter_mut().find(|o| {
                    (o.center[0] - b.center[0]).hypot(o.center[1] - b.center[1]) < 1e-3
                        && (o.radius - b.radius).abs() < 1e-3
                        && o.hole == b.hole
                        && o.z_min <= b.z_max + 1e-6
                        && b.z_min <= o.z_max + 1e-6
                }) {
                    o.z_min = o.z_min.min(b.z_min);
                    o.z_max = o.z_max.max(b.z_max);
                    o.faces.extend(b.faces);
                } else {
                    bores.push(b);
                }
            }
        }
        let triangles = mesh
            .triangles
            .iter()
            .map(|t| t.map(|i| pos[i as usize]))
            .collect();
        MachiningModel {
            flats,
            bores,
            min,
            max,
            triangles,
            triangle_faces: mesh.triangle_faces.clone(),
            face_names: mesh.face_names.clone(),
        }
    }

    /// 名前が `names` のどれかの面の三角形。
    pub fn triangles_of(&self, names: &[String]) -> Vec<[[f64; 3]; 3]> {
        self.triangles
            .iter()
            .zip(&self.triangle_faces)
            .filter(|(_, f)| {
                self.face_names
                    .get(**f as usize)
                    .is_some_and(|n| names.contains(n))
            })
            .map(|(t, _)| *t)
            .collect()
    }

    /// 点 `p` の真上から見た、部品のいちばん高い所。部品がなければ `None`。
    pub fn top_at(&self, p: P2) -> Option<f64> {
        let mut best: Option<f64> = None;
        for t in &self.triangles {
            if let Some(z) = z_on_triangle(t, p) {
                best = Some(best.map_or(z, |b: f64| b.max(z)));
            }
        }
        best
    }

    pub fn flat(&self, name: &str) -> Option<&FlatFace> {
        self.flats.iter().find(|f| f.name == name)
    }

    /// 写しが共有する名前を、位置ごとの面としてすべて返す。
    pub fn flats_named(&self, name: &str) -> Vec<&FlatFace> {
        self.flats.iter().filter(|f| f.name == name).collect()
    }

    /// 下を向いた面のうち、外周の囲む面積が最も大きいものの外周(部品の外形として使う)。
    pub fn outline(&self) -> Option<Loop> {
        // 外周の囲む面積がいちばん大きい面(同じなら低い方)。いちばん低い面を選ぶと、
        // 裏返して置いた部品ではボスの底のような小さな面になり、板を切ってしまう
        self.flats
            .iter()
            .filter(|f| !f.up)
            .max_by(|a, b| {
                let (sa, sb) = (
                    a.area.outer.signed_area().abs(),
                    b.area.outer.signed_area().abs(),
                );
                if (sa - sb).abs() > 1e-6 {
                    sa.total_cmp(&sb)
                } else {
                    b.z.total_cmp(&a.z)
                }
            })
            .map(|f| f.area.outer.clone())
    }
}

/// 三角形を真上から見て `p` を含むなら、その点の高さ。
pub fn z_on_triangle(t: &[[f64; 3]; 3], p: P2) -> Option<f64> {
    let [a, b, c] = *t;
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1e-12 {
        return None;
    }
    let l1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
    let l2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
    let l3 = 1.0 - l1 - l2;
    let e = -1e-9;
    (l1 >= e && l2 >= e && l3 >= e).then(|| l1 * a[2] + l2 * b[2] + l3 * c[2])
}

fn vertex_normal(mesh: &Mesh, t: [u32; 3]) -> Option<[f64; 3]> {
    let mut n = [0.0; 3];
    for i in t {
        let v = mesh.normals.get(i as usize)?;
        for k in 0..3 {
            n[k] += v[k] as f64;
        }
    }
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    (l > 1e-9).then(|| [n[0] / l, n[1] / l, n[2] / l])
}

fn tri_normal(pos: &[[f64; 3]], t: [u32; 3]) -> Option<[f64; 3]> {
    let a = pos[t[0] as usize];
    let b = pos[t[1] as usize];
    let c = pos[t[2] as usize];
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    // つぶれた三角形は向きを問わない
    (l > 1e-12).then(|| [n[0] / l, n[1] / l, n[2] / l])
}

/// 位置で頂点をまとめる鍵(網は面ごとに頂点を持つので、同じ位置の頂点を同一視する)。
fn key(p: [f64; 3]) -> Key {
    let q = |v: f64| (v * 1e4).round() as i64;
    (q(p[0]), q(p[1]), q(p[2]))
}

/// 三角形の集まりの境界を、閉じた点の列にする。
fn boundary_loops(pos: &[[f64; 3]], tris: &[[u32; 3]]) -> Vec<Vec<P2>> {
    let mut count: HashMap<(Key, Key), i32> = HashMap::new();
    let mut point: HashMap<Key, [f64; 3]> = HashMap::new();
    for t in tris {
        for k in 0..3 {
            let a = pos[t[k] as usize];
            let b = pos[t[(k + 1) % 3] as usize];
            let (ka, kb) = (key(a), key(b));
            if ka == kb {
                continue;
            }
            point.insert(ka, a);
            point.insert(kb, b);
            // 向きつきの辺。逆向きの辺があれば打ち消す
            if let Some(c) = count.get_mut(&(kb, ka)) {
                *c -= 1;
                if *c == 0 {
                    count.remove(&(kb, ka));
                }
            } else {
                *count.entry((ka, kb)).or_default() += 1;
            }
        }
    }
    let mut next: HashMap<Key, Vec<Key>> = HashMap::new();
    for ((a, b), c) in &count {
        for _ in 0..*c {
            next.entry(*a).or_default().push(*b);
        }
    }
    let mut loops = Vec::new();
    let mut starts: Vec<Key> = next.keys().copied().collect();
    starts.sort();
    for s in starts {
        while let Some(first) = next.get_mut(&s).and_then(|v| v.pop()) {
            let mut lp = vec![s];
            let mut cur = first;
            while cur != s {
                lp.push(cur);
                cur = match next.get_mut(&cur).and_then(|v| v.pop()) {
                    Some(n) => n,
                    None => break,
                };
            }
            if cur == s && lp.len() >= 3 {
                loops.push(
                    lp.iter()
                        .map(|k| {
                            let p = point[k];
                            [p[0], p[1]]
                        })
                        .collect(),
                );
            }
        }
    }
    loops
}

fn area_from_loops(loops: Vec<Vec<P2>>) -> Option<Area> {
    let mut fitted: Vec<Loop> = loops.iter().map(|l| fit_arcs(l, FIT_TOLERANCE)).collect();
    if fitted.is_empty() {
        return None;
    }
    let outer_i = (0..fitted.len()).max_by(|&i, &j| {
        fitted[i]
            .signed_area()
            .abs()
            .total_cmp(&fitted[j].signed_area().abs())
    })?;
    let outer = fitted.remove(outer_i);
    Some(
        Area {
            outer,
            holes: fitted,
        }
        .normalized(),
    )
}

fn fit_bore(
    pos: &[[f64; 3]],
    tris: &[[u32; 3]],
    normals: &[Option<[f64; 3]>],
    name: String,
) -> Option<Bore> {
    let verts: Vec<usize> = tris
        .iter()
        .flat_map(|t| t.iter().map(|&i| i as usize))
        .collect();
    // 最小二乗の円(Kasa 法)
    let n = verts.len() as f64;
    let mx = verts.iter().map(|&i| pos[i][0]).sum::<f64>() / n;
    let my = verts.iter().map(|&i| pos[i][1]).sum::<f64>() / n;
    let (mut suu, mut svv, mut suv, mut suuu, mut svvv, mut suvv, mut svuu) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for &i in &verts {
        let u = pos[i][0] - mx;
        let v = pos[i][1] - my;
        suu += u * u;
        svv += v * v;
        suv += u * v;
        suuu += u * u * u;
        svvv += v * v * v;
        suvv += u * v * v;
        svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    if det.abs() < 1e-12 {
        return None;
    }
    let r1 = 0.5 * (suuu + suvv);
    let r2 = 0.5 * (svvv + svuu);
    let uc = (r1 * svv - r2 * suv) / det;
    let vc = (r2 * suu - r1 * suv) / det;
    let center = [mx + uc, my + vc];
    let radius = (uc * uc + vc * vc + (suu + svv) / n).sqrt();
    let max_err = verts
        .iter()
        .map(|&i| ((pos[i][0] - center[0]).hypot(pos[i][1] - center[1]) - radius).abs())
        .fold(0.0, f64::max);
    if max_err > 0.02 || radius < 0.05 {
        return None;
    }
    // 面の向き: 三角形の重心から中心への向きと法線を比べる
    let mut toward = 0.0;
    for (t, n) in tris.iter().zip(normals) {
        let Some(n) = n else { continue };
        let c = [
            t.iter().map(|&i| pos[i as usize][0]).sum::<f64>() / 3.0,
            t.iter().map(|&i| pos[i as usize][1]).sum::<f64>() / 3.0,
        ];
        toward += n[0] * (center[0] - c[0]) + n[1] * (center[1] - c[1]);
    }
    let z_min = verts
        .iter()
        .map(|&i| pos[i][2])
        .fold(f64::INFINITY, f64::min);
    let z_max = verts
        .iter()
        .map(|&i| pos[i][2])
        .fold(f64::NEG_INFINITY, f64::max);
    Some(Bore {
        center,
        radius,
        z_min,
        z_max,
        hole: toward > 0.0,
        faces: vec![name],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_line_is_offset_to_its_left_and_round_outside_corners() {
        // L の形(右へ進んでから上へ)。左へずらすと、内側の角は切り、外へは出ない
        let l = offset_open(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], 1.0, 1e-3);
        assert_eq!(l.len(), 1);
        assert!((l[0][0][1] - 1.0).abs() < 1e-9 && l[0][0][0].abs() < 1e-9);
        let end = l[0][l[0].len() - 1];
        assert!((end[0] - 9.0).abs() < 1e-9 && (end[1] - 10.0).abs() < 1e-9);
        // 右へずらすと、外の角を半径 1 の円弧で回る
        let r = offset_open(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], -1.0, 1e-3);
        assert_eq!(r.len(), 1);
        assert!(r[0].len() > 4);
        assert!(
            r[0].iter()
                .all(|q| ((q[0] - 10.0).max(0.0)).hypot((-q[1]).max(0.0)) < 1.0 + 1e-6)
        );
        assert!(r[0].iter().any(|q| q[0] > 10.5 && q[1] < -0.5));
    }

    fn circle_pts(c: P2, r: f64, n: usize, ccw: bool) -> Vec<P2> {
        (0..n)
            .map(|i| {
                let t = 2.0 * PI * i as f64 / n as f64 * if ccw { 1.0 } else { -1.0 };
                [c[0] + r * t.cos(), c[1] + r * t.sin()]
            })
            .collect()
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
        Loop::from_points(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
    }

    #[test]
    fn circles_become_arcs_and_rectangles_stay_lines() {
        let l = fit_arcs(&circle_pts([5.0, 5.0], 4.0, 90, true), 0.01);
        assert!(l.len() <= 3, "{} 個の区間", l.len());
        assert!(l.verts.iter().all(|v| v.bulge > 0.0));
        assert!((l.signed_area() - PI * 16.0).abs() < 1e-6);
        // 辺の途中に点がある長方形
        let pts = [
            [0.0, 0.0],
            [5.0, 0.0],
            [10.0, 0.0],
            [10.0, 4.0],
            [5.0, 4.0],
            [0.0, 4.0],
        ];
        let l = fit_arcs(&pts, 0.01);
        assert_eq!(l.len(), 4);
        assert!((l.signed_area() - 40.0).abs() < 1e-9);
        // 角の丸い長方形(10 × 4、角の半径 1)
        let mut pts = vec![];
        for (cx, cy, a0) in [
            (9.0, 1.0, -90.0f64),
            (9.0, 3.0, 0.0),
            (1.0, 3.0, 90.0),
            (1.0, 1.0, 180.0),
        ] {
            for k in 0..=10 {
                let t = (a0 + 9.0 * k as f64).to_radians();
                pts.push([cx + t.cos(), cy + t.sin()]);
            }
        }
        let l = fit_arcs(&pts, 0.005);
        assert_eq!(l.verts.iter().filter(|v| v.bulge != 0.0).count(), 4);
        assert_eq!(l.len(), 8);
        let expect = 10.0 * 4.0 - (4.0 - PI);
        assert!(
            (l.signed_area() - expect).abs() < 1e-6,
            "{}",
            l.signed_area()
        );
    }

    #[test]
    fn offsetting_an_area_with_an_island() {
        let area = Area {
            outer: rect(0.0, 0.0, 20.0, 10.0),
            holes: vec![fit_arcs(&circle_pts([10.0, 5.0], 2.0, 60, false), 0.01)],
        };
        let off = offset_area(&area, 1.0);
        assert_eq!(off.len(), 1);
        assert_eq!(off[0].holes.len(), 1);
        // 外周は 18 × 8 で角は直角のまま、島は半径 3 の円
        let expect = 18.0 * 8.0 - PI * 9.0;
        assert!((off[0].area() - expect).abs() < 1e-3, "{}", off[0].area());
        // 島の周りが閉じるほど寄せると、2 つに分かれる
        let off = offset_area(&area, 3.2);
        assert_eq!(off.len(), 2);
        // 外側へ広げると角が丸くなる
        let out = offset_loop_outward(&rect(0.0, 0.0, 10.0, 10.0), 1.0);
        assert_eq!(out.len(), 1);
        assert!((out[0].signed_area().abs() - (144.0 - (4.0 - PI))).abs() < 1e-6);
    }

    #[test]
    fn area_contains() {
        let area = Area {
            outer: rect(0.0, 0.0, 20.0, 10.0),
            holes: vec![rect(5.0, 2.0, 8.0, 8.0)],
        }
        .normalized();
        assert!(area.contains([2.0, 2.0]));
        assert!(!area.contains([6.0, 5.0]));
        assert!(!area.contains([25.0, 5.0]));
    }

    /// V 彫りと同じく、0.1 mm ずつ寄せて消えるまで続ける(長方形 25×24 と円 φ8 は、細かい直線のままだと
    /// cavalier_contours が panic する形)。
    fn shrink_until_gone(area: &Area, step: f64) -> usize {
        let mut d = step;
        let mut n = 0;
        loop {
            let rings = offset_area(area, d);
            if rings.is_empty() {
                return n;
            }
            n += 1;
            d += step;
            assert!(n < 100_000);
        }
    }

    #[test]
    fn shrinking_to_nothing_does_not_panic() {
        // 網羅の調べの部品(60×40 の板の上面、長方形 25×24 と円 φ8)。座標は機械の原点から
        let rect_area = Area {
            outer: rect(7.0, 8.0, 32.0, 32.0),
            holes: vec![],
        };
        let circle = Area {
            outer: fit_arcs(&circle_pts([46.0, 20.0], 4.0, 64, true), 0.001),
            holes: vec![],
        };
        for step in [0.1, 0.05, 0.2, 0.3] {
            let n = shrink_until_gone(&rect_area, step);
            assert!(
                n as f64 * step > 11.0,
                "長方形は半分の幅近くまで寄る({step}: {n})"
            );
            let n = shrink_until_gone(&circle, step);
            assert!(n as f64 * step > 3.5, "円は半径近くまで寄る({step}: {n})");
        }
    }
}
