//! 渦格子法(VLM)。翼の反りの面を四角の板に分け、板ごとに馬蹄渦(1/4 の所の束縛渦と、後ろへ
//! 真っすぐ伸びる 2 本の後流の渦)を置き、3/4 の所で流れが面を通り抜けない条件から渦の強さを解く。
//! 力は束縛渦にかかる Kutta–Joukowski の力、誘導抗力は後ろの遠く(トレフツ面)の吹き下ろしから出す。
//!
//! 座標は流れの座標(x が流れの向き、y が翼幅、z が上)、長さは m、速さは m/s。

use nalgebra::{DMatrix, DVector};

pub type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn lerp(a: V3, b: V3, t: f64) -> V3 {
    add(a, mul(sub(b, a), t))
}

/// 揚力面: 翼幅の向きに並べた断面(`stations`)ごとに、前縁から後縁までの点の列。
/// `points[k][m]` は断面 k の m 番目の点(m = 0 が前縁)。断面は y の小さい順。
#[derive(Debug, Clone)]
pub struct Surface {
    pub name: String,
    pub points: Vec<Vec<V3>>,
}

impl Surface {
    fn strips(&self) -> usize {
        self.points.len().saturating_sub(1)
    }
    fn chordwise(&self) -> usize {
        self.points.first().map_or(0, |p| p.len().saturating_sub(1))
    }

    /// 上から見た面積と、平均空力翼弦。
    pub fn area_and_mac(&self) -> (f64, f64) {
        let mut s = 0.0;
        let mut c2 = 0.0;
        for k in 0..self.strips() {
            let (a, b) = (&self.points[k], &self.points[k + 1]);
            let ca = a[a.len() - 1][0] - a[0][0];
            let cb = b[b.len() - 1][0] - b[0][0];
            let dy = b[0][1] - a[0][1];
            s += 0.5 * (ca + cb) * dy;
            c2 += (ca * ca + ca * cb + cb * cb) / 3.0 * dy;
        }
        (s, if s > 0.0 { c2 / s } else { 0.0 })
    }

    pub fn span(&self) -> f64 {
        match (self.points.first(), self.points.last()) {
            (Some(a), Some(b)) => b[0][1] - a[0][1],
            _ => 0.0,
        }
    }
}

/// 1 枚の板。
#[derive(Debug, Clone)]
pub struct Panel {
    pub surface: usize,
    pub strip: usize,
    pub corners: [V3; 4],
    /// 束縛渦の両端(1/4 の所)
    pub a: V3,
    pub b: V3,
    /// 条件を付ける点(3/4 の所)と法線(上向き)
    pub ctrl: V3,
    pub normal: V3,
    pub area: f64,
}

fn panels(surfaces: &[Surface]) -> Vec<Panel> {
    let mut out = Vec::new();
    for (si, s) in surfaces.iter().enumerate() {
        for k in 0..s.strips() {
            for m in 0..s.chordwise() {
                let p00 = s.points[k][m];
                let p01 = s.points[k][m + 1];
                let p10 = s.points[k + 1][m];
                let p11 = s.points[k + 1][m + 1];
                let a = lerp(p00, p01, 0.25);
                let b = lerp(p10, p11, 0.25);
                let ctrl = mul(add(lerp(p00, p01, 0.75), lerp(p10, p11, 0.75)), 0.5);
                let mut n = cross(sub(p11, p00), sub(p10, p01));
                let l = norm(n);
                let area = 0.5 * l;
                if l > 0.0 {
                    n = mul(n, 1.0 / l);
                }
                if n[2] < 0.0 {
                    n = mul(n, -1.0);
                }
                out.push(Panel {
                    surface: si,
                    strip: k,
                    corners: [p00, p01, p11, p10],
                    a,
                    b,
                    ctrl,
                    normal: n,
                    area,
                });
            }
        }
    }
    out
}

/// 有限の渦の線 a → b(強さ 1)が点 p に起こす速さ。`core` より近い所は 0。
fn segment(p: V3, a: V3, b: V3, core: f64) -> V3 {
    let r1 = sub(p, a);
    let r2 = sub(p, b);
    let r0 = sub(b, a);
    let c = cross(r1, r2);
    let c2 = dot(c, c);
    let (l1, l2) = (norm(r1), norm(r2));
    if c2 < core * core * dot(r0, r0) || l1 < 1e-12 || l2 < 1e-12 {
        return [0.0; 3];
    }
    let k = dot(r0, sub(mul(r1, 1.0 / l1), mul(r2, 1.0 / l2))) / (4.0 * std::f64::consts::PI * c2);
    mul(c, k)
}

/// 点 a から向き d(単位)へ無限に伸びる渦の線(強さ 1)が点 p に起こす速さ。
fn semi_infinite(p: V3, a: V3, d: V3, core: f64) -> V3 {
    let r = sub(p, a);
    let c = cross(d, r);
    let c2 = dot(c, c);
    let l = norm(r);
    if c2 < core * core || l < 1e-12 {
        return [0.0; 3];
    }
    mul(c, (1.0 + dot(d, r) / l) / (4.0 * std::f64::consts::PI * c2))
}

/// 馬蹄渦(強さ 1)が点 p に起こす速さ。後流は +x へ伸びる。
fn horseshoe(p: V3, pn: &Panel, core: f64) -> V3 {
    let d = [1.0, 0.0, 0.0];
    let v = sub(
        segment(p, pn.a, pn.b, core),
        semi_infinite(p, pn.a, d, core),
    );
    add(v, semi_infinite(p, pn.b, d, core))
}

/// 解いた結果。
#[derive(Debug, Clone)]
pub struct Solution {
    pub panels: Vec<Panel>,
    /// 板ごとの渦の強さ(m²/s)と力(N)
    pub gamma: Vec<f64>,
    pub force: Vec<V3>,
    /// 全体の力(流れの座標、N)と、揚力・誘導抗力(N)
    pub total: V3,
    pub lift: f64,
    pub induced_drag: f64,
    /// 原点まわりのモーメント(N·m)
    pub moment: V3,
}

/// 迎え角 `alpha`(rad)・速さ `speed`・空気の密度 `rho` で解く。
pub fn solve(surfaces: &[Surface], alpha: f64, speed: f64, rho: f64) -> Result<Solution, String> {
    let panels = panels(surfaces);
    let n = panels.len();
    if n == 0 {
        return Err("翼の板がありません".into());
    }
    let size = surfaces
        .iter()
        .map(|s| s.span())
        .fold(0.0, f64::max)
        .max(1e-6);
    let core = 1e-4 * size;
    let vinf = [speed * alpha.cos(), 0.0, speed * alpha.sin()];
    let mut a = DMatrix::<f64>::zeros(n, n);
    let mut rhs = DVector::<f64>::zeros(n);
    for (i, pi) in panels.iter().enumerate() {
        for (j, pj) in panels.iter().enumerate() {
            a[(i, j)] = dot(horseshoe(pi.ctrl, pj, core), pi.normal);
        }
        rhs[i] = -dot(vinf, pi.normal);
    }
    let g = a
        .lu()
        .solve(&rhs)
        .ok_or("渦の強さの式が解けません(板が重なっていないか確かめてください)")?;
    let gamma: Vec<f64> = g.iter().copied().collect();
    let mut force = Vec::with_capacity(n);
    let mut total = [0.0; 3];
    let mut moment = [0.0; 3];
    for (p, gm) in panels.iter().zip(&gamma) {
        let f = mul(cross(vinf, sub(p.b, p.a)), rho * gm);
        let mid = mul(add(p.a, p.b), 0.5);
        total = add(total, f);
        moment = add(moment, cross(mid, f));
        force.push(f);
    }
    let lift = dot(total, [-alpha.sin(), 0.0, alpha.cos()]);
    let induced_drag = trefftz(surfaces, &panels, &gamma, rho);
    Ok(Solution {
        panels,
        gamma,
        force,
        total,
        lift,
        induced_drag,
        moment,
    })
}

/// トレフツ面の誘導抗力。帯ごとの渦の和の差が、断面の後縁から後ろへ伸びる渦になる。
/// 主翼と尾翼のように同じ高さに並ぶ平らな翼は、後ろから見ると渦の面が重なる。重なった渦の
/// すぐそばで速さを読むと値が飛ぶので、高さの近い平らな翼は 1 枚の面にまとめ(翼幅の位置ごとに
/// 渦の和を足し合わせ)て解く。
fn trefftz(surfaces: &[Surface], panels: &[Panel], gamma: &[f64], rho: f64) -> f64 {
    // (y, z, 強さ) の渦と、(帯の中点 y, z, 帯の幅 dy, dz, 帯の渦の和)
    let mut vort: Vec<(f64, f64, f64)> = Vec::new();
    let mut strips: Vec<(f64, f64, f64, f64, f64)> = Vec::new();
    // 平らな翼の帯: (y の小さい端, 大きい端, z, 上向きの渦の和)
    let mut flat: Vec<(f64, f64, f64, f64)> = Vec::new();
    let mut size: f64 = 0.0;
    for (si, s) in surfaces.iter().enumerate() {
        let ns = s.strips();
        let mut sum = vec![0.0; ns];
        for (p, g) in panels.iter().zip(gamma) {
            if p.surface == si {
                sum[p.strip] += g;
            }
        }
        let te = |k: usize| *s.points[k].last().expect("後縁");
        size = size.max(s.span().abs());
        let is_flat = (0..ns).all(|k| {
            let (a, b) = (te(k), te(k + 1));
            (b[2] - a[2]).abs() < 0.2 * (b[1] - a[1]).abs()
        });
        if is_flat {
            for (k, g) in sum.iter().enumerate() {
                let (a, b) = (te(k), te(k + 1));
                // 帯の向きが -y なら、上向きの法線に合わせて符号を返す
                let g = if b[1] >= a[1] { *g } else { -*g };
                flat.push((a[1].min(b[1]), a[1].max(b[1]), 0.5 * (a[2] + b[2]), g));
            }
            continue;
        }
        for k in 0..=ns {
            let left = if k > 0 { sum[k - 1] } else { 0.0 };
            let right = if k < ns { sum[k] } else { 0.0 };
            let p = te(k);
            vort.push((p[1], p[2], left - right));
        }
        for (k, g) in sum.iter().enumerate() {
            let (a, b) = (te(k), te(k + 1));
            strips.push((
                0.5 * (a[1] + b[1]),
                0.5 * (a[2] + b[2]),
                b[1] - a[1],
                b[2] - a[2],
                *g,
            ));
        }
    }
    // 平らな帯を高さの近い組にまとめ、組ごとに翼幅の位置の区切りで渦の和を足し合わせる
    flat.sort_by(|a, b| a.2.total_cmp(&b.2));
    let tol = 0.02 * size;
    let mut i = 0;
    while i < flat.len() {
        let mut j = i + 1;
        while j < flat.len() && flat[j].2 - flat[i].2 <= tol {
            j += 1;
        }
        let group = &flat[i..j];
        let z = group.iter().map(|f| f.2).sum::<f64>() / group.len() as f64;
        let mut cuts: Vec<f64> = group.iter().flat_map(|f| [f.0, f.1]).collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9 * size.max(1e-9));
        let mut prev = 0.0;
        for w in cuts.windows(2) {
            let ym = 0.5 * (w[0] + w[1]);
            let g: f64 = group
                .iter()
                .filter(|f| f.0 <= ym && ym <= f.1)
                .map(|f| f.3)
                .sum();
            vort.push((w[0], z, prev - g));
            strips.push((ym, z, w[1] - w[0], 0.0, g));
            prev = g;
        }
        if let Some(last) = cuts.last() {
            vort.push((*last, z, prev));
        }
        i = j;
    }
    let mut d = 0.0;
    for &(y, z, dy, dz, g) in &strips {
        let l = (dy * dy + dz * dz).sqrt();
        if l <= 0.0 {
            continue;
        }
        // 帯の上向きの法線(帯の向きを +90 度回す)
        let n = (-dz / l, dy / l);
        let mut w = 0.0;
        for &(vy, vz, s) in &vort {
            let (ry, rz) = (y - vy, z - vz);
            let r2 = ry * ry + rz * rz;
            if r2 < 1e-16 {
                continue;
            }
            let (uy, uz) = (
                -rz * s / (2.0 * std::f64::consts::PI * r2),
                ry * s / (2.0 * std::f64::consts::PI * r2),
            );
            w += uy * n.0 + uz * n.1;
        }
        d -= 0.5 * rho * g * w * l;
    }
    d
}

/// 中立点(x の位置): 迎え角を少し変えた時に、まわりのモーメントが変わらない点。
pub fn neutral_point(a: &Solution, b: &Solution) -> Option<f64> {
    let dfz = b.total[2] - a.total[2];
    if dfz.abs() < 1e-12 {
        return None;
    }
    Some(-(b.moment[1] - a.moment[1]) / dfz)
}

/// 点 x(z = 0 の線の上)まわりの縦揺れのモーメント(上げ頭が正)。
pub fn pitching_moment(s: &Solution, x: f64) -> f64 {
    s.moment[1] + x * s.total[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 平らな翼: 翼幅 b、翼弦の関数 chord(y)(前縁は x = -c/4 にそろえる)。
    fn wing(b: f64, chord: impl Fn(f64) -> f64, ns: usize, nc: usize) -> Surface {
        let mut points = Vec::new();
        for k in 0..=ns {
            let t = k as f64 / ns as f64;
            // 端を細かく(余弦の間隔)
            let y = -0.5 * b * (std::f64::consts::PI * t).cos();
            let c = chord(y).max(1e-3 * b);
            let le = -0.25 * c;
            points.push(
                (0..=nc)
                    .map(|m| [le + c * m as f64 / nc as f64, y, 0.0])
                    .collect(),
            );
        }
        Surface {
            name: "翼".into(),
            points,
        }
    }

    #[test]
    fn elliptic_wing_matches_the_lifting_surface_slope_and_e_is_one() {
        let (b, c0) = (1.6, 0.2546);
        let s = wing(
            b,
            |y| c0 * (1.0 - (2.0 * y / b).powi(2)).max(0.0).sqrt(),
            40,
            6,
        );
        let (area, _) = s.area_and_mac();
        let ar = b * b / area;
        let alpha = 4f64.to_radians();
        let r = solve(&[s], alpha, 10.0, 1.225).unwrap();
        let q = 0.5 * 1.225 * 100.0;
        let cl = r.lift / (q * area);
        // Helmbold の式(揚力面の理論の近似)
        let want = 2.0 * std::f64::consts::PI * ar / (2.0 + (ar * ar + 4.0).sqrt()) * alpha;
        assert!(
            (cl - want).abs() / want < 0.03,
            "CL {cl} と {want}(AR {ar})"
        );
        let cdi = r.induced_drag / (q * area);
        let e = cl * cl / (std::f64::consts::PI * ar * cdi);
        assert!((e - 1.0).abs() < 0.04, "e {e}");
    }

    #[test]
    fn neutral_point_of_a_rectangular_wing_is_near_the_quarter_chord() {
        let c = 0.2;
        let s = wing(1.2, |_| c, 48, 6);
        let a = solve(std::slice::from_ref(&s), 2f64.to_radians(), 10.0, 1.225).unwrap();
        let b = solve(&[s], 4f64.to_radians(), 10.0, 1.225).unwrap();
        let x = neutral_point(&a, &b).unwrap();
        // 前縁は -c/4。中立点は前縁から 0.22〜0.27 c ほど
        let from_le = (x + 0.25 * c) / c;
        assert!((0.2..0.28).contains(&from_le), "前縁から {from_le} c");
        // 長方形の翼(AR 6)は楕円翼より e が少し小さい(帯が粗いと 1 を少し超える。48 本で 1.006、96 本で 0.995)
        let area = 1.2 * c;
        let q = 0.5 * 1.225 * 100.0;
        let cl = b.lift / (q * area);
        let cdi = b.induced_drag / (q * area);
        let e = cl * cl / (std::f64::consts::PI * 6.0 * cdi);
        assert!((0.9..1.01).contains(&e), "e {e}");
    }

    #[test]
    fn a_tail_in_the_same_plane_keeps_e_below_one() {
        // 主翼 1.0 × 0.2 m と、0.7 m 後ろの尾翼 0.4 × 0.08 m(同じ高さ)。後ろから見ると渦の面が
        // 重なる。平らな翼の幅が主翼の翼幅を越えないので、全体の e は 1 を越えない(Munk)
        let main = wing(1.0, |_| 0.2, 40, 6);
        let mut tail = wing(0.4, |_| 0.08, 40, 6);
        for row in &mut tail.points {
            for p in row {
                p[0] += 0.7;
            }
        }
        let r = solve(&[main, tail], 4f64.to_radians(), 10.0, 1.225).unwrap();
        let (area, ar) = (0.2, 5.0);
        let q = 0.5 * 1.225 * 100.0;
        let cl = r.lift / (q * area);
        let cdi = r.induced_drag / (q * area);
        let e = cl * cl / (std::f64::consts::PI * ar * cdi);
        assert!((0.8..1.01).contains(&e), "e {e}");
    }
}
