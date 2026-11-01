//! 方向の格子。
//!
//! 輪の格子(`Rings`): 仰角を −90° から 90° まで `step` ごとの輪に分け、仰角 el の輪に
//! round(360 cos(el) / step) 個(両極は 1 個)の方向を方位 j × 360 / N に置く。方位は前から左回り
//! (90° が左)。分けた表はこの格子の順(下の輪から、輪の中は方位の順)に並ぶ。引き方は最近傍で、
//! 輪 r = floor((el + 90) / step + 0.5)(端で止める)、方位 j = floor(frac(az / 360) × N_r + 0.5) mod N_r。

use crate::HrtfError;

/// 輪の格子。
#[derive(Clone, Debug, PartialEq)]
pub struct Rings {
    /// 仰角の刻み(度)。
    pub step: f64,
    /// 輪ごとの仰角(度、下から)。
    pub el: Vec<f64>,
    /// 輪ごとの方向の数。
    pub n: Vec<usize>,
    /// 輪ごとの最初の方向の番号。
    pub first: Vec<usize>,
}

impl Rings {
    /// 刻み `step` 度の輪の格子。180 / step が整数に近くない刻みも受けるが、輪の数は丸めて決まる。
    pub fn new(step: f64) -> Result<Rings, HrtfError> {
        if !(step.is_finite() && step > 0.0 && step <= 90.0) {
            return Err(HrtfError::input(format!(
                "格子の刻み {step}° は 0 より大きく 90 以下にする"
            )));
        }
        let rings = (180.0 / step).round() as usize;
        let el: Vec<f64> = (0..=rings)
            .map(|i| {
                if i == rings {
                    90.0
                } else {
                    -90.0 + i as f64 * 180.0 / rings as f64
                }
            })
            .collect();
        let mut n: Vec<usize> = el
            .iter()
            .map(|e| ((360.0 * e.to_radians().cos() / step).round_ties_even() as usize).max(1))
            .collect();
        n[0] = 1;
        n[rings] = 1;
        let mut first = Vec::with_capacity(n.len());
        let mut sum = 0;
        for &k in &n {
            first.push(sum);
            sum += k;
        }
        Ok(Rings { step, el, n, first })
    }

    /// 方向の数。
    pub fn len(&self) -> usize {
        self.first.last().copied().unwrap_or(0) + self.n.last().copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 格子の方向(方位、仰角。度)を表の順に。
    pub fn dirs(&self) -> Vec<(f64, f64)> {
        let mut out = Vec::with_capacity(self.len());
        for (&e, &k) in self.el.iter().zip(&self.n) {
            for j in 0..k {
                out.push((j as f64 * 360.0 / k as f64, e));
            }
        }
        out
    }

    /// 方向(方位、仰角。度)に最も近い格子の方向の番号(上の引き方の決まり)。
    pub fn lookup(&self, az: f64, el: f64) -> usize {
        let last = self.el.len() - 1;
        let r = ((el + 90.0) / self.step + 0.5)
            .floor()
            .clamp(0.0, last as f64) as usize;
        let k = self.n[r];
        let j = ((az / 360.0).rem_euclid(1.0) * k as f64 + 0.5).floor() as usize % k;
        self.first[r] + j
    }
}

/// 方向(方位、仰角。度)の単位ベクトル。x が前、y が左、z が上。
pub fn unit(az: f64, el: f64) -> [f64; 3] {
    let (a, e) = (az.to_radians(), el.to_radians());
    [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()]
}

/// 方向の集まりから最も近い方向を引く索引。単位ベクトルを立方体の格子のます目に分けて置き、
/// 問いのます目から外へ広げて探す。距離が同じなら番号の小さい方を返す。
pub struct Nearest {
    pts: Vec<[f64; 3]>,
    cells: Vec<Vec<usize>>,
    g: usize,
}

impl Nearest {
    pub fn new(dirs: &[(f64, f64)]) -> Nearest {
        let g = ((dirs.len() as f64 / 4.0).cbrt().ceil() as usize).clamp(1, 64);
        let pts: Vec<[f64; 3]> = dirs.iter().map(|&(a, e)| unit(a, e)).collect();
        let mut cells = vec![Vec::new(); g * g * g];
        for (i, p) in pts.iter().enumerate() {
            let c = Self::cell(p, g);
            cells[(c[0] * g + c[1]) * g + c[2]].push(i);
        }
        Nearest { pts, cells, g }
    }

    fn cell(p: &[f64; 3], g: usize) -> [usize; 3] {
        let f = |v: f64| (((v + 1.0) * 0.5 * g as f64).floor().max(0.0) as usize).min(g - 1);
        [f(p[0]), f(p[1]), f(p[2])]
    }

    /// 最も近い方向の番号。方向が 1 つも無い時は `None`。
    pub fn find(&self, az: f64, el: f64) -> Option<usize> {
        if self.pts.is_empty() {
            return None;
        }
        let q = unit(az, el);
        let g = self.g as isize;
        let c = Self::cell(&q, self.g).map(|v| v as isize);
        let h = 2.0 / self.g as f64;
        let mut best: Option<(f64, usize)> = None;
        for r in 0..=g {
            for x in (c[0] - r).max(0)..=(c[0] + r).min(g - 1) {
                for y in (c[1] - r).max(0)..=(c[1] + r).min(g - 1) {
                    for z in (c[2] - r).max(0)..=(c[2] + r).min(g - 1) {
                        let ring = (x - c[0]).abs().max((y - c[1]).abs()).max((z - c[2]).abs());
                        if ring != r {
                            continue;
                        }
                        for &i in &self.cells[((x * g + y) * g + z) as usize] {
                            let p = &self.pts[i];
                            let d = (p[0] - q[0]).powi(2)
                                + (p[1] - q[1]).powi(2)
                                + (p[2] - q[2]).powi(2);
                            if best.is_none_or(|(bd, bi)| d < bd || (d == bd && i < bi)) {
                                best = Some((d, i));
                            }
                        }
                    }
                }
            }
            // 次の輪のます目の点は、問いの点から少なくとも r × h 離れている。
            if let Some((d, _)) = best
                && d.sqrt() < r as f64 * h
            {
                break;
            }
        }
        best.map(|(_, i)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: 2° と 0.5° の輪の格子の方向の数と輪の数、引き方の決まり(極・方位の回り込み)。
    #[test]
    fn ring_counts_and_lookup() {
        let s = Rings::new(2.0).unwrap();
        assert_eq!(s.len(), 10318);
        assert_eq!(s.el.len(), 91);
        let o = Rings::new(0.5).unwrap();
        assert_eq!(o.len(), 165016);
        assert_eq!(o.el.len(), 361);
        assert_eq!(s.lookup(123.0, -90.0), 0);
        assert_eq!(s.lookup(5.0, 90.0), s.len() - 1);
        let eq = 45;
        assert_eq!(s.el[eq], 0.0);
        assert_eq!(s.n[eq], 180);
        assert_eq!(s.lookup(0.0, 0.0), s.first[eq]);
        assert_eq!(s.lookup(359.5, 0.0), s.first[eq]);
        assert_eq!(s.lookup(-1.2, 0.0), s.first[eq] + 179);
        assert_eq!(s.lookup(90.0, 0.4), s.first[eq] + 45);
        // ちょうど中間は上の隣へ。
        assert_eq!(s.lookup(1.0, 0.0), s.first[eq] + 1);
        assert_eq!(s.lookup(0.0, 1.0), s.first[eq + 1]);
    }

    /// 確かめること: 索引の最近傍が総当たりと同じになる。
    #[test]
    fn nearest_matches_brute_force() {
        let dirs = Rings::new(10.0).unwrap().dirs();
        let idx = Nearest::new(&dirs);
        for q in 0..400 {
            let az = (q as f64 * 37.3) % 360.0;
            let el = ((q as f64 * 11.7) % 180.0) - 90.0;
            let u = unit(az, el);
            let brute = (0..dirs.len())
                .min_by(|&a, &b| {
                    let d = |i: usize| {
                        let p = unit(dirs[i].0, dirs[i].1);
                        (p[0] - u[0]).powi(2) + (p[1] - u[1]).powi(2) + (p[2] - u[2]).powi(2)
                    };
                    d(a).total_cmp(&d(b)).then(a.cmp(&b))
                })
                .unwrap();
            assert_eq!(idx.find(az, el), Some(brute), "az={az} el={el}");
        }
    }
}
