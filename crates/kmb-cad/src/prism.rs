//! 平らな輪郭を向きに沿って伸ばした柱の三角形(押し出す道具を色付きの立体で見せるため)。
//!
//! 輪郭の線(世界座標、同じ平面の上)を平面の 2D に写し、入れ子の深さで外周と穴に分け、
//! 穴を外周へ橋でつないでから耳を切り落として蓋を三角形に分ける。蓋の分け方は伸ばす長さに
//! よらないので、[`PrismCap`] を一度作れば長さを変えて何度でも柱にできる。

type P3 = [f64; 3];
type P2 = [f64; 2];

/// 三角形の点の数がこれを超える輪郭は蓋を作らない(耳を切る計算が重いため。側面だけ出す)。
const MAX_CAP_POINTS: usize = 6000;

/// 蓋を三角形に分けた輪郭。
#[derive(Debug, Clone, Default)]
pub struct PrismCap {
    /// 輪郭の点(世界座標)。外周は左回り、穴は右回り(`dir` から見て)にそろえてある
    loops: Vec<Vec<P3>>,
    /// 蓋の三角形(`loops` を並べた番号。`dir` から見て左回り)
    tris: Vec<[usize; 3]>,
    dir: P3,
}

impl PrismCap {
    /// `loops` は同じ平面の上の閉じた線(最後の点が最初と同じでもよい)、`dir` は平面の法線。
    pub fn new(loops: &[Vec<P3>], dir: P3) -> PrismCap {
        let dir = unit(dir);
        let (u, v) = basis(dir);
        let mut rings: Vec<Vec<P3>> = loops
            .iter()
            .map(|l| {
                let mut l = l.clone();
                while l.len() > 1 && dist2(l[0], l[l.len() - 1]) < 1e-18 {
                    l.pop();
                }
                l
            })
            .filter(|l| l.len() >= 3)
            .collect();
        let to2 = |p: P3| [dot(p, u), dot(p, v)];
        let flat: Vec<Vec<P2>> = rings
            .iter()
            .map(|l| l.iter().map(|p| to2(*p)).collect())
            .collect();
        // 入れ子の深さ: 偶数は外周、奇数は穴
        let depth: Vec<usize> = (0..flat.len())
            .map(|i| {
                (0..flat.len())
                    .filter(|&j| j != i && inside(&flat[j], flat[i][0]))
                    .count()
            })
            .collect();
        // 外周は左回り、穴は右回り
        for (i, l) in rings.iter_mut().enumerate() {
            let ccw = area(&flat[i]) > 0.0;
            if ccw != depth[i].is_multiple_of(2) {
                l.reverse();
            }
        }
        let flat: Vec<Vec<P2>> = rings
            .iter()
            .map(|l| l.iter().map(|p| to2(*p)).collect())
            .collect();
        let mut start = Vec::with_capacity(rings.len());
        let mut n = 0;
        for l in &rings {
            start.push(n);
            n += l.len();
        }
        let mut tris = Vec::new();
        if n <= MAX_CAP_POINTS {
            for (i, _) in depth
                .iter()
                .enumerate()
                .filter(|(_, d)| d.is_multiple_of(2))
            {
                // この外周の真下の穴(深さが 1 つ深く、この外周の中にある)
                let holes: Vec<usize> = (0..flat.len())
                    .filter(|&j| depth[j] == depth[i] + 1 && inside(&flat[i], flat[j][0]))
                    .collect();
                let mut poly: Vec<usize> = (0..flat[i].len()).map(|k| start[i] + k).collect();
                let pts: Vec<P2> = flat.iter().flatten().copied().collect();
                let mut holes = holes;
                // 右へいちばん出ている穴から橋を架ける
                holes.sort_by(|a, b| max_x(&flat[*b]).total_cmp(&max_x(&flat[*a])));
                for h in holes {
                    let hole: Vec<usize> = (0..flat[h].len()).map(|k| start[h] + k).collect();
                    bridge(&pts, &mut poly, &hole);
                }
                ear_clip(&pts, poly, &mut tris);
            }
        }
        PrismCap {
            loops: rings,
            tris,
            dir,
        }
    }

    /// 平面から `from`〜`to`(`dir` の向きの距離、mm)の柱の三角形。外から見て左回り。
    pub fn solid(&self, from: f64, to: f64) -> Vec<[P3; 3]> {
        self.solid_by(&|_| from, &|_| to, 0.0)
    }

    /// 壁を傾けた柱の三角形(押し出しのテーパー)。輪郭の平面から離れるほど、外周は `lean`
    /// (テーパー角の tan。正は細く、負は太く)× 離れた距離だけ内へ寄り、穴は逆に広がる。
    pub fn solid_tapered(&self, from: f64, to: f64, lean: f64) -> Vec<[P3; 3]> {
        self.solid_by(&|_| from, &|_| to, lean)
    }

    /// 輪郭を平面から `s` 離した所に置いた線(壁を `lean` で傾ける。[`Self::solid_tapered`] と同じ寄せ方)。
    pub fn rings_at(&self, s: f64, lean: f64) -> Vec<Vec<P3>> {
        let miter = self.miters();
        let mut k = 0;
        self.loops
            .iter()
            .map(|l| {
                l.iter()
                    .map(|p| {
                        let q = self.place(*p, miter[k], s, lean);
                        k += 1;
                        q
                    })
                    .collect()
            })
            .collect()
    }

    /// 点 `p` を `dir` の向きに `s`、材料の内へ `|s| × lean` 寄せた所。
    fn place(&self, p: P3, miter: P3, s: f64, lean: f64) -> P3 {
        let inward = s.abs() * lean;
        [0, 1, 2].map(|i| p[i] + self.dir[i] * s + miter[i] * inward)
    }

    /// 点ごとの、材料の内へ 1 mm 寄せる向き(隣の 2 辺を 1 mm ずつ内へずらした線の交わる所)。
    /// 外周は左回り・穴は右回りにそろえてあるので、どの輪でも進む向きの左が材料の内。
    fn miters(&self) -> Vec<P3> {
        let mut out = Vec::new();
        for l in &self.loops {
            let n = l.len();
            for k in 0..n {
                let e1 = unit([0, 1, 2].map(|i| l[k][i] - l[(k + n - 1) % n][i]));
                let e2 = unit([0, 1, 2].map(|i| l[(k + 1) % n][i] - l[k][i]));
                let (a, b) = (cross(self.dir, e1), cross(self.dir, e2));
                let c = 1.0 + dot(a, b);
                // 折り返す尖りでは寄せる向きが決まらないので、手前の辺の内へ
                out.push(if c < 0.1 {
                    a
                } else {
                    [0, 1, 2].map(|i| (a[i] + b[i]) / c)
                });
            }
        }
        out
    }

    /// 平面から `from` の所から、斜めの平面(点 `point`・法線 `normal`)までの柱の三角形
    /// (押し出しの範囲「対象まで」で、輪郭と平行でない面に止める時)。平面が `dir` と平行なら空。
    pub fn solid_to_plane(&self, from: f64, point: P3, normal: P3) -> Vec<[P3; 3]> {
        let c = dot(self.dir, normal);
        if c.abs() < 1e-9 {
            return Vec::new();
        }
        let end = |p: P3| dot([0, 1, 2].map(|i| point[i] - p[i]), normal) / c;
        self.solid_by(&|_| from, &end, 0.0)
    }

    /// 平面から `dir` の向きに `len` mm の柱を、3D のビューに描く網にしたもの(手順の下絵
    /// [`kmb_core::Handle::shape`] に使う)。`key` は輪郭と向きで決まる。
    pub fn shape(&self, len: f64, label: &str) -> kmb_core::OverlayShape {
        use std::hash::{Hash, Hasher};
        let tris = self.solid(0.0, len);
        let mut k = std::collections::hash_map::DefaultHasher::new();
        self.loops
            .iter()
            .flatten()
            .chain([&self.dir])
            .for_each(|p| p.map(f64::to_bits).hash(&mut k));
        len.to_bits().hash(&mut k);
        let mut mesh = kmb_core::Mesh::default();
        for t in &tris {
            let n = unit(cross(
                [0, 1, 2].map(|i| t[1][i] - t[0][i]),
                [0, 1, 2].map(|i| t[2][i] - t[0][i]),
            ));
            let base = mesh.positions.len() as u32;
            for p in t {
                mesh.positions.push(p.map(|v| v as f32));
                mesh.normals.push(n.map(|v| v as f32));
            }
            mesh.triangles.push([base, base + 1, base + 2]);
            mesh.triangle_faces.push(0);
        }
        mesh.face_names.push(label.into());
        kmb_core::OverlayShape {
            key: k.finish(),
            label: label.into(),
            mesh,
            ..Default::default()
        }
    }

    /// 点ごとに `a`〜`b`(`dir` の向きの距離)の柱。どの点も `a` と `b` の大小は同じとする。
    /// `lean` が 0 でなければ壁を傾ける([`Self::solid_tapered`])。
    fn solid_by(&self, a: &dyn Fn(P3) -> f64, b: &dyn Fn(P3) -> f64, lean: f64) -> Vec<[P3; 3]> {
        let pts: Vec<P3> = self.loops.iter().flatten().copied().collect();
        let Some(&p0) = pts.first() else {
            return Vec::new();
        };
        let swap = b(p0) < a(p0);
        let range = |p: P3| if swap { (b(p), a(p)) } else { (a(p), b(p)) };
        if pts.iter().all(|p| {
            let (lo, hi) = range(*p);
            hi - lo < 1e-9
        }) {
            return Vec::new();
        }
        let miter = if lean == 0.0 {
            vec![[0.0; 3]; pts.len()]
        } else {
            self.miters()
        };
        // 点の番号(並べた `pts` の)で、下と上の位置
        let lo = |k: usize| self.place(pts[k], miter[k], range(pts[k]).0, lean);
        let hi = |k: usize| self.place(pts[k], miter[k], range(pts[k]).1, lean);
        let mut out = Vec::with_capacity(self.tris.len() * 2 + pts.len() * 2);
        for t in &self.tris {
            // 上の蓋は dir から見て左回りのまま、下の蓋は裏返す
            out.push(t.map(hi));
            out.push([t[0], t[2], t[1]].map(lo));
        }
        let mut start = 0;
        for l in &self.loops {
            let n = l.len();
            for k in 0..n {
                let (a, b) = (start + k, start + (k + 1) % n);
                out.push([lo(a), lo(b), hi(b)]);
                out.push([lo(a), hi(b), hi(a)]);
            }
            start += n;
        }
        out
    }
}

/// 穴 `hole`(右回り)を外周 `poly` へ、互いに見える点どうしの橋でつなぐ。
fn bridge(pts: &[P2], poly: &mut Vec<usize>, hole: &[usize]) {
    // 穴のいちばん右の点 M から右へ水平に見て、最初に当たる外周の辺
    let (mi, m) = hole
        .iter()
        .enumerate()
        .max_by(|a, b| pts[*a.1][0].total_cmp(&pts[*b.1][0]))
        .map(|(i, k)| (i, pts[*k]))
        .expect("穴は 3 点以上");
    let mut best: Option<(f64, usize)> = None;
    for k in 0..poly.len() {
        let (a, b) = (pts[poly[k]], pts[poly[(k + 1) % poly.len()]]);
        if (a[1] - m[1]) * (b[1] - m[1]) > 0.0 || (a[1] - b[1]).abs() < 1e-15 {
            continue;
        }
        let x = a[0] + (m[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
        if x < m[0] - 1e-12 {
            continue;
        }
        // 当たった辺の右寄りの端
        let p = if a[0] > b[0] { k } else { (k + 1) % poly.len() };
        if best.is_none_or(|(bx, _)| x < bx) {
            best = Some((x, p));
        }
    }
    let Some((x, mut pi)) = best else {
        return;
    };
    // 三角形 M・当たった点・端の中に外周の点があれば、M から見て角度の小さい物へ替える
    let p = pts[poly[pi]];
    let hit = [x, m[1]];
    let mut best_angle = f64::INFINITY;
    for (k, &q) in poly.iter().enumerate() {
        let c = pts[q];
        if q == poly[pi] || !in_tri(c, m, hit, p) {
            continue;
        }
        let ang = (c[1] - m[1]).abs().atan2(c[0] - m[0]);
        if ang < best_angle {
            best_angle = ang;
            pi = k;
        }
    }
    // 外周 … P, M, 穴を一周, M, P … とつなぐ
    let mut ring: Vec<usize> = Vec::with_capacity(hole.len() + 2);
    for j in 0..=hole.len() {
        ring.push(hole[(mi + j) % hole.len()]);
    }
    ring.push(poly[pi]);
    poly.splice(pi + 1..pi + 1, ring);
}

/// 左回りの多角形 `poly` を耳から切り落として三角形にする。
fn ear_clip(pts: &[P2], mut poly: Vec<usize>, out: &mut Vec<[usize; 3]>) {
    let mut guard = 0;
    while poly.len() > 3 && guard < poly.len() * 2 {
        let n = poly.len();
        let mut cut = None;
        for i in 0..n {
            let (ia, ib, ic) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            if cross2(a, b, c) <= 1e-14 {
                continue;
            }
            let blocked = poly.iter().any(|&k| {
                k != ia
                    && k != ib
                    && k != ic
                    && pts[k] != a
                    && pts[k] != b
                    && pts[k] != c
                    && in_tri(pts[k], a, b, c)
            });
            if !blocked {
                cut = Some(i);
                break;
            }
        }
        match cut {
            Some(i) => {
                out.push([poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]]);
                poly.remove(i);
                guard = 0;
            }
            // 耳が見つからない(自己交差など): 一直線の点を 1 つ捨てて続ける
            None => {
                poly.remove(0);
                guard += 1;
            }
        }
    }
    if poly.len() == 3 && cross2(pts[poly[0]], pts[poly[1]], pts[poly[2]]) > 0.0 {
        out.push([poly[0], poly[1], poly[2]]);
    }
}

fn cross2(a: P2, b: P2, c: P2) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn in_tri(p: P2, a: P2, b: P2, c: P2) -> bool {
    let (d1, d2, d3) = (cross2(a, b, p), cross2(b, c, p), cross2(c, a, p));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

fn area(l: &[P2]) -> f64 {
    (0..l.len())
        .map(|k| {
            let (a, b) = (l[k], l[(k + 1) % l.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

fn inside(l: &[P2], p: P2) -> bool {
    let mut c = false;
    for k in 0..l.len() {
        let (a, b) = (l[k], l[(k + 1) % l.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        {
            c = !c;
        }
    }
    c
}

fn max_x(l: &[P2]) -> f64 {
    l.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max)
}

fn basis(n: P3) -> (P3, P3) {
    let a = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = unit(cross(a, n));
    let v = cross(n, u);
    (u, v)
}

fn unit(a: P3) -> P3 {
    let l = dot(a, a).sqrt();
    if l < 1e-15 {
        [0.0, 0.0, 1.0]
    } else {
        a.map(|x| x / l)
    }
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dist2(a: P3, b: P3) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(c: [f64; 2], h: f64, z: f64) -> Vec<P3> {
        vec![
            [c[0] - h, c[1] - h, z],
            [c[0] + h, c[1] - h, z],
            [c[0] + h, c[1] + h, z],
            [c[0] - h, c[1] + h, z],
        ]
    }

    /// 柱の体積(発散定理)。三角形が外向きにそろっていれば正しい体積になる。
    fn volume(t: &[[P3; 3]]) -> f64 {
        t.iter().map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0).sum()
    }

    /// テーパー: 外周は上へ行くほど内へ寄り、穴は広がる(20 mm 角に 10 mm 角の穴、高さ 5 mm、
    /// tan = 0.1 で上は 19 mm 角と 11 mm 角)。体積は角すい台の差。下の蓋は寄せない。
    #[test]
    fn a_tapered_tube_leans_its_walls_into_the_material() {
        let cap = PrismCap::new(
            &[square([0.0, 0.0], 10.0, 0.0), square([1.0, 0.0], 5.0, 0.0)],
            [0.0, 0.0, 1.0],
        );
        let frustum = |a1: f64, a2: f64| 5.0 / 3.0 * (a1 + a2 + (a1 * a2).sqrt());
        let want = frustum(400.0, 361.0) - frustum(100.0, 121.0);
        let v = volume(&cap.solid_tapered(0.0, 5.0, 0.1));
        assert!((v - want).abs() < 1e-6, "{v} {want}");
        let top = cap.rings_at(5.0, 0.1);
        let bottom = cap.rings_at(0.0, 0.1);
        let max_x = |l: &Vec<P3>| l.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
        assert!((max_x(&top[0]) - 9.5).abs() < 1e-9);
        assert!((max_x(&top[1]) - 6.5).abs() < 1e-9);
        assert!((max_x(&bottom[0]) - 10.0).abs() < 1e-9);
        // 傾けなければ今までと同じ
        assert_eq!(cap.solid_tapered(0.0, 5.0, 0.0), cap.solid(0.0, 5.0));
    }

    #[test]
    fn a_square_with_a_square_hole_makes_a_closed_tube() {
        // 20 mm 角に 10 mm 角の穴、高さ 5 mm: 体積 (400 - 100) × 5 = 1500
        let cap = PrismCap::new(
            &[square([0.0, 0.0], 10.0, 2.0), square([1.0, 0.0], 5.0, 2.0)],
            [0.0, 0.0, 1.0],
        );
        let t = cap.solid(0.0, 5.0);
        assert!((volume(&t) - 1500.0).abs() < 1e-6, "{}", volume(&t));
        // 下へ伸ばしても外向き
        let t = cap.solid(0.0, -5.0);
        assert!((volume(&t) - 1500.0).abs() < 1e-6, "{}", volume(&t));
    }

    #[test]
    fn a_circle_drawn_clockwise_still_points_outward() {
        let ring: Vec<P3> = (0..64)
            .map(|k| {
                let a = -(k as f64) / 64.0 * std::f64::consts::TAU;
                [5.0 * a.cos(), 0.0, 5.0 * a.sin()]
            })
            .collect();
        let cap = PrismCap::new(&[ring], [0.0, -1.0, 0.0]);
        let t = cap.solid(0.0, 3.0);
        let disc = 64.0 / 2.0 * 25.0 * (std::f64::consts::TAU / 64.0).sin();
        assert!((volume(&t) - disc * 3.0).abs() < 1e-6, "{}", volume(&t));
    }

    #[test]
    fn two_separate_squares_each_get_a_cap() {
        let cap = PrismCap::new(
            &[square([0.0, 0.0], 1.0, 0.0), square([5.0, 0.0], 1.0, 0.0)],
            [0.0, 0.0, 1.0],
        );
        let t = cap.solid(0.0, 1.0);
        assert!((volume(&t) - 8.0).abs() < 1e-9, "{}", volume(&t));
    }
}
