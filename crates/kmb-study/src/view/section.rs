//! 格子の結果の断面: 平面の上の点を流体のセルから補間して塗り、物体の表面の網を平面で切った
//! 線で物体の中を塗らずに残す。

use super::*;

/// 格子の点 `p`(ローカル)の値を、流体のセルだけから補間する(成分は 3 つまで)。
/// まわりに流体のセルが無ければ `None`。
pub fn sample(g: &Grid, vals: &Values, comps: usize, p: [f64; 3]) -> Option<[f32; 3]> {
    let mut lo = [0usize; 3];
    let mut fr = [0.0f64; 3];
    for a in 0..3 {
        if g.n[a] == 1 {
            continue;
        }
        let x = (p[a] - g.origin[a]) / g.h - 0.5;
        if x < -0.5 || x > g.n[a] as f64 - 0.5 {
            return None;
        }
        let i = x.floor().clamp(0.0, (g.n[a] - 2) as f64);
        lo[a] = i as usize;
        fr[a] = (x - i).clamp(0.0, 1.0);
    }
    let mut sum = [0.0f64; 3];
    let mut wsum = 0.0;
    let corners = if g.flat() { 4 } else { 8 };
    for c in 0..corners {
        let d = [c & 1, (c >> 1) & 1, (c >> 2) & 1];
        let mut idx = [0usize; 3];
        let mut w = 1.0;
        for a in 0..3 {
            if g.n[a] == 1 {
                continue;
            }
            idx[a] = lo[a] + d[a];
            w *= if d[a] == 1 { fr[a] } else { 1.0 - fr[a] };
        }
        if w <= 0.0 {
            continue;
        }
        let x = g.index(idx[0], idx[1], idx[2]);
        if g.solid[x] {
            continue;
        }
        for (k, sum) in sum.iter_mut().enumerate().take(comps) {
            let v = vals.at(x * comps + k);
            if !v.is_finite() {
                continue;
            }
            *sum += w * v as f64;
        }
        wsum += w;
    }
    if wsum < 1e-9 {
        // まわりが物体ばかり: 近い流体のセル
        let c = [0, 1, 2].map(|a| {
            if g.n[a] == 1 {
                0
            } else {
                (lo[a] + (fr[a] >= 0.5) as usize).min(g.n[a] - 1)
            }
        });
        for r in 1..=2isize {
            for dk in if g.flat() { 0..=0 } else { -r..=r } {
                for dj in -r..=r {
                    for di in -r..=r {
                        let ii = c[0] as isize + di;
                        let jj = c[1] as isize + dj;
                        let kk = c[2] as isize + dk;
                        if ii < 0
                            || jj < 0
                            || kk < 0
                            || ii >= g.n[0] as isize
                            || jj >= g.n[1] as isize
                            || kk >= g.n[2] as isize
                        {
                            continue;
                        }
                        let x = g.index(ii as usize, jj as usize, kk as usize);
                        if !g.solid[x] {
                            let mut v = [0.0f32; 3];
                            for (k, v) in v.iter_mut().enumerate().take(comps) {
                                *v = vals.at(x * comps + k);
                            }
                            return Some(v);
                        }
                    }
                }
            }
        }
        return None;
    }
    let mut v = [0.0f32; 3];
    for k in 0..comps {
        v[k] = (sum[k] / wsum) as f32;
    }
    Some(v)
}

fn magnitude(v: [f32; 3], comps: usize) -> f32 {
    if comps == 1 {
        v[0]
    } else {
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    }
}

/// 物体の表面の網を平面 `local[a] = c` で切った線分(平面の中の (u, v) の座標)。
fn outline(
    g: &Grid,
    a: usize,
    c: f64,
    (u, v): (usize, usize),
    body: (&Vec<[f32; 3]>, &Vec<[u32; 3]>),
) -> Vec<[[f64; 2]; 2]> {
    let (points, tris) = body;
    let local: Vec<[f64; 3]> = points
        .iter()
        .map(|p| g.to_local(p.map(f64::from)))
        .collect();
    let mut out = Vec::new();
    for t in tris {
        let p = t.map(|i| local[i as usize]);
        let d = p.map(|q| q[a] - c);
        let mut cut = Vec::with_capacity(2);
        for k in 0..3 {
            let (i, j) = (k, (k + 1) % 3);
            if (d[i] > 0.0) != (d[j] > 0.0) {
                let f = d[i] / (d[i] - d[j]);
                cut.push([
                    p[i][u] + (p[j][u] - p[i][u]) * f,
                    p[i][v] + (p[j][v] - p[i][v]) * f,
                ]);
            }
        }
        if cut.len() == 2 {
            out.push([cut[0], cut[1]]);
        }
    }
    out
}

/// 線 `x2 = at` と交わる線分の、もう一方の座標(並べて)。`(x1, x2)` は線分の座標の番号。
fn crossings(segs: &[[[f64; 2]; 2]], at: f64, x1: usize, x2: usize) -> Vec<f64> {
    let mut out = Vec::new();
    for s in segs {
        let (p, q) = (s[0], s[1]);
        if (p[x2] > at) != (q[x2] > at) {
            out.push(p[x1] + (at - p[x2]) * (q[x1] - p[x1]) / (q[x2] - p[x2]));
        }
    }
    out.sort_by(f64::total_cmp);
    out
}

/// 断面の網: 流体の所だけを、本当の外形で切って塗る。
#[allow(clippy::too_many_arguments)]
pub(super) fn section_mesh(
    b: &mut Builder,
    g: &Grid,
    vals: &Values,
    comps: usize,
    axis: usize,
    at: f64,
    body: Option<Tris<'_>>,
    s: &Scale,
) {
    let a = g.local_axis(axis);
    if g.flat() && a != 2 {
        return;
    }
    let (u, v) = match a {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    // 平面の位置(ローカル)。辺や頂点にちょうど乗らないよう、わずかにずらす
    let c = if g.flat() {
        g.origin[2]
    } else {
        let c = at * g.signs[a] + g.h * 1.37e-6;
        let lo = g.origin[a] + 0.5 * g.h;
        let hi = g.origin[a] + (g.n[a] as f64 - 0.5) * g.h;
        if !(lo..=hi).contains(&c) {
            return;
        }
        c
    };
    // 平面の上の格子の点(セルの間隔の半分。多すぎる時は粗く)
    let count = |ax: usize| ((g.n[ax].saturating_sub(1)) * 2).clamp(1, 640) + 1;
    let (nu, nv) = (count(u), count(v));
    let u0 = g.origin[u] + 0.5 * g.h + g.h * 2.1e-6;
    let v0 = g.origin[v] + 0.5 * g.h + g.h * 3.3e-6;
    let du = (g.n[u] as f64 - 1.0) * g.h / (nu - 1).max(1) as f64;
    let dv = (g.n[v] as f64 - 1.0) * g.h / (nv - 1).max(1) as f64;
    let us: Vec<f64> = (0..nu).map(|i| u0 + i as f64 * du).collect();
    let vs: Vec<f64> = (0..nv).map(|j| v0 + j as f64 * dv).collect();
    let segs = body
        .map(|bd| outline(g, a, c, (u, v), bd))
        .unwrap_or_default();
    let rows: Vec<Vec<f64>> = vs.iter().map(|y| crossings(&segs, *y, 0, 1)).collect();
    let cols: Vec<Vec<f64>> = us.iter().map(|x| crossings(&segs, *x, 1, 0)).collect();
    // 点が物体の中か(行の交わりの数の偶奇)
    let mut inside = vec![false; nu * nv];
    for j in 0..nv {
        let mut k = 0;
        for i in 0..nu {
            while k < rows[j].len() && rows[j][k] < us[i] {
                k += 1;
            }
            inside[i + nu * j] = k % 2 == 1;
        }
    }
    let local = |x: f64, y: f64| {
        let mut p = [0.0; 3];
        p[a] = c;
        p[u] = x;
        p[v] = y;
        p
    };
    let world = |p: [f64; 3]| g.to_world(p);
    let value: Vec<f32> = (0..nu * nv)
        .map(|k| {
            if inside[k] {
                return f32::NAN;
            }
            let (i, j) = (k % nu, k / nu);
            sample(g, vals, comps, local(us[i], vs[j])).map_or(f32::NAN, |x| magnitude(x, comps))
        })
        .collect();
    let fluid = |i: usize, j: usize| !inside[i + nu * j] && value[i + nu * j].is_finite();
    // 線の上の交わり(a から b へ。流体の端 `from` に近い物)
    let cross = |list: &[f64], from: f64, to: f64| -> f64 {
        let (lo, hi) = (from.min(to), from.max(to));
        let mut best = None;
        for x in list {
            if *x > lo && *x < hi {
                let d = (x - from).abs();
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, *x));
                }
            }
        }
        best.map_or(0.5 * (from + to), |(_, x)| x)
    };
    for j in 0..nv - 1 {
        for i in 0..nu - 1 {
            let cs = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let f = cs.map(|(x, y)| fluid(x, y));
            if !f.iter().any(|x| *x) {
                continue;
            }
            let mut poly: Vec<([f64; 3], f64)> = Vec::with_capacity(6);
            for k in 0..4 {
                let (p, q) = (cs[k], cs[(k + 1) % 4]);
                if f[k] {
                    let x = value[p.0 + nu * p.1];
                    poly.push((world(local(us[p.0], vs[p.1])), s.t(x)));
                }
                if f[k] != f[(k + 1) % 4] {
                    let (wet, dry) = if f[k] { (p, q) } else { (q, p) };
                    let x = value[wet.0 + nu * wet.1];
                    let pt = if p.1 == q.1 {
                        // 横の辺(行 j の上)
                        let xu = cross(&rows[p.1], us[wet.0], us[dry.0]);
                        local(xu, vs[p.1])
                    } else {
                        let yv = cross(&cols[p.0], vs[wet.1], vs[dry.1]);
                        local(us[p.0], yv)
                    };
                    poly.push((world(pt), s.t(x)));
                }
            }
            if poly.len() < 3 {
                continue;
            }
            for k in 1..poly.len() - 1 {
                let (p0, p1, p2) = (poly[0], poly[k], poly[k + 1]);
                b.banded([p0.0, p1.0, p2.0], [p0.1, p1.1, p2.1], s);
            }
        }
    }
}
