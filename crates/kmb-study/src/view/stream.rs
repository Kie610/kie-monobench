//! 格子の速度の流線: 上流の点から速度を追い、細い管で描いて速さで塗る。

use super::*;

/// 流線(ローカルの点の列と、点ごとの速さ)。
pub struct Streamline {
    pub points: Vec<[f64; 3]>,
    pub speed: Vec<f32>,
}

/// 上流の点から速度を追った流線。3D は物体の正面を覆う 6×4 の点、2D は断面の上の 24 点から。
pub fn streamlines(
    g: &Grid,
    vel: &Values,
    body: Option<&[[f32; 3]]>,
    plane: Option<(usize, f64)>,
) -> Vec<Streamline> {
    // 物体の外接の箱(ローカル)
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in body.unwrap_or(&[]) {
        let q = g.to_local(p.map(f64::from));
        for a in 0..3 {
            lo[a] = lo[a].min(q[a]);
            hi[a] = hi[a].max(q[a]);
        }
    }
    let box_lo = g.center(0, 0, 0);
    let box_hi = g.center(g.n[0] - 1, g.n[1] - 1, g.n[2] - 1);
    if lo[0] > hi[0] {
        lo = box_lo;
        hi = box_hi;
    }
    let size = (hi[1] - lo[1]).max(hi[2] - lo[2]).max(g.h);
    let x0 = (lo[0] - 0.6 * size).max(box_lo[0] + g.h);
    let mut seeds = Vec::new();
    if g.flat() {
        let (y0, y1) = (lo[1] - 0.4 * size, hi[1] + 0.4 * size);
        for k in 0..24 {
            let y = y0 + (y1 - y0) * (k as f64 + 0.5) / 24.0;
            seeds.push([x0, y.clamp(box_lo[1], box_hi[1]), g.origin[2]]);
        }
    } else {
        let grow = |a: usize, f: f64| {
            let m = 0.5 * (lo[a] + hi[a]);
            let r = 0.5 * (hi[a] - lo[a]).max(g.h) * f;
            (m - r, m + r)
        };
        let (y0, y1) = grow(1, 1.3);
        let (mut z0, mut z1) = grow(2, 1.3);
        // 断面が水平なら、その高さのまわりに寄せる
        if let Some((axis, at)) = plane {
            let a = g.local_axis(axis);
            if a == 2 {
                let c = at * g.signs[2];
                let r = 0.25 * (z1 - z0);
                z0 = c - r;
                z1 = c + r;
            }
        }
        for kz in 0..4 {
            for ky in 0..6 {
                let y = y0 + (y1 - y0) * (ky as f64 + 0.5) / 6.0;
                let z = z0 + (z1 - z0) * (kz as f64 + 0.5) / 4.0;
                seeds.push([
                    x0,
                    y.clamp(box_lo[1], box_hi[1]),
                    z.clamp(box_lo[2], box_hi[2]),
                ]);
            }
        }
    }
    let mut vmax = 0.0f32;
    for p in 0..g.len() {
        if !g.solid[p] {
            vmax = vmax.max(vel.magnitude(p, 3));
        }
    }
    let ds = 0.5 * g.h;
    let limit = 4 * (g.n[0] + g.n[1] + g.n[2]);
    let inside =
        |p: [f64; 3]| (0..3).all(|a| g.n[a] == 1 || (p[a] >= box_lo[a] && p[a] <= box_hi[a]));
    let dir = |p: [f64; 3]| -> Option<([f64; 3], f32)> {
        // 物体のセルに入ったら止める(補間の近い流体のセルで、物体を突き抜けないように)
        if g.solid[cell_of(g, p)?] {
            return None;
        }
        let v = sample(g, vel, 3, p)?;
        let mut v = v.map(f64::from);
        if g.flat() {
            v[2] = 0.0;
        }
        let s = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if s.is_nan() || s <= 1e-4 * vmax as f64 {
            return None;
        }
        Some((v.map(|x| x / s), s as f32))
    };
    let mut out = Vec::new();
    for s0 in seeds {
        let mut line = Streamline {
            points: vec![s0],
            speed: Vec::new(),
        };
        let mut p = s0;
        for _ in 0..limit {
            let Some((d1, sp)) = dir(p) else { break };
            if line.speed.is_empty() {
                line.speed.push(sp);
            }
            let mid = [0, 1, 2].map(|a| p[a] + 0.5 * ds * d1[a]);
            let Some((d2, _)) = dir(mid) else { break };
            let q = [0, 1, 2].map(|a| p[a] + ds * d2[a]);
            if !inside(q) {
                break;
            }
            p = q;
            line.points.push(p);
            line.speed.push(sp);
        }
        if line.points.len() >= 3 {
            out.push(line);
        }
    }
    out
}

/// 点 `p`(ローカル)を含むセル。
fn cell_of(g: &Grid, p: [f64; 3]) -> Option<usize> {
    let mut c = [0usize; 3];
    for a in 0..3 {
        if g.n[a] == 1 {
            continue;
        }
        let x = ((p[a] - g.origin[a]) / g.h).floor();
        if x < 0.0 || x >= g.n[a] as f64 {
            return None;
        }
        c[a] = x as usize;
    }
    Some(g.index(c[0], c[1], c[2]))
}

/// 流線を細い三角の管にして足す(線分ごとに速さの色)。
pub(super) fn tube(b: &mut Builder, g: &Grid, line: &Streamline, s: &Scale) {
    let r = 0.18 * g.h;
    let pts: Vec<[f64; 3]> = line.points.iter().map(|p| g.to_world(*p)).collect();
    for k in 0..pts.len() - 1 {
        let sp = 0.5 * (line.speed[k] + line.speed[(k + 1).min(line.speed.len() - 1)]);
        let face = s.face((s.t(sp).floor().max(0.0) as usize).min(s.levels - 1));
        segment(b, pts[k], pts[k + 1], r, face);
    }
}

/// 線分 p–q を、半径 `r` の三角柱にして足す(どの向きから見ても線に見える)。
pub(super) fn segment(b: &mut Builder, p: [f64; 3], q: [f64; 3], r: f64, face: u32) {
    let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if l.is_nan() || l <= 1e-12 {
        return;
    }
    let d = d.map(|x| x / l);
    let refv = if d[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let n1 = {
        let c = cross(d, refv);
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        c.map(|x| x / l)
    };
    let n2 = cross(d, n1);
    let ring = |c: [f64; 3]| -> [[f64; 3]; 3] {
        [0.0f64, 2.094_395, 4.188_79].map(|t| {
            let (cs, sn) = (t.cos() * r, t.sin() * r);
            [0, 1, 2].map(|a| c[a] + cs * n1[a] + sn * n2[a])
        })
    };
    let (ra, rb) = (ring(p), ring(q));
    for m in 0..3 {
        let n = (m + 1) % 3;
        b.tri([ra[m], ra[n], rb[n]], face);
        b.tri([ra[m], rb[n], rb[m]], face);
    }
}
