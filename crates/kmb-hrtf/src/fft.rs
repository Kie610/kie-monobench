//! 2 の累乗の長さの複素 FFT。HRIR の処理では長さが 512〜8192 程度に限られるので、外の crate に頼らず
//! 基数 2 の繰り返しで書く。回転因子と並べ替えの表は長さごとに 1 回作り、処理の段の中で使い回す
//! (プロセス全体の控えは持たない)。

use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub};

/// 複素数(f64)。FFT と位相の計算に要る分だけを持つ。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct C {
    pub re: f64,
    pub im: f64,
}

impl C {
    pub const fn new(re: f64, im: f64) -> C {
        C { re, im }
    }

    /// 大きさ r・偏角 theta(rad)の値。
    pub fn from_polar(r: f64, theta: f64) -> C {
        C::new(r * theta.cos(), r * theta.sin())
    }

    pub fn conj(self) -> C {
        C::new(self.re, -self.im)
    }

    pub fn norm_sqr(self) -> f64 {
        self.re * self.re + self.im * self.im
    }

    pub fn norm(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// 偏角(rad、−π〜π)。
    pub fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }

    pub fn exp(self) -> C {
        C::from_polar(self.re.exp(), self.im)
    }
}

impl Add for C {
    type Output = C;
    fn add(self, o: C) -> C {
        C::new(self.re + o.re, self.im + o.im)
    }
}

impl Sub for C {
    type Output = C;
    fn sub(self, o: C) -> C {
        C::new(self.re - o.re, self.im - o.im)
    }
}

impl Neg for C {
    type Output = C;
    fn neg(self) -> C {
        C::new(-self.re, -self.im)
    }
}

impl Mul for C {
    type Output = C;
    fn mul(self, o: C) -> C {
        C::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}

impl Mul<f64> for C {
    type Output = C;
    fn mul(self, s: f64) -> C {
        C::new(self.re * s, self.im * s)
    }
}

impl Div<f64> for C {
    type Output = C;
    fn div(self, s: f64) -> C {
        C::new(self.re / s, self.im / s)
    }
}

impl AddAssign for C {
    fn add_assign(&mut self, o: C) {
        *self = *self + o;
    }
}

impl MulAssign for C {
    fn mul_assign(&mut self, o: C) {
        *self = *self * o;
    }
}

impl MulAssign<f64> for C {
    fn mul_assign(&mut self, s: f64) {
        *self = *self * s;
    }
}

/// 決まった長さの FFT。
pub struct Fft {
    n: usize,
    twiddle: Vec<C>,
    rev: Vec<usize>,
}

impl Fft {
    /// 長さ `n`(2 以上の 2 の累乗)の FFT を用意する。
    pub fn new(n: usize) -> Fft {
        assert!(
            n >= 2 && n.is_power_of_two(),
            "FFT の長さは 2 以上の 2 の累乗"
        );
        let bits = n.trailing_zeros();
        let rev = (0..n)
            .map(|i| i.reverse_bits() >> (usize::BITS - bits))
            .collect();
        let twiddle = (0..n / 2)
            .map(|k| {
                let a = -2.0 * std::f64::consts::PI * k as f64 / n as f64;
                C::new(a.cos(), a.sin())
            })
            .collect();
        Fft { n, twiddle, rev }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 順変換 X[k] = Σ x[t] e^{-j 2π k t / n}(その場で置き換える)。
    pub fn forward(&self, x: &mut [C]) {
        self.run(x, false);
    }

    /// 逆変換 x[t] = (1/n) Σ X[k] e^{+j 2π k t / n}(その場で置き換える)。
    pub fn inverse(&self, x: &mut [C]) {
        self.run(x, true);
        let s = 1.0 / self.n as f64;
        for v in x.iter_mut() {
            *v *= s;
        }
    }

    /// 実数の列を長さ n に 0 で伸ばして順変換した、全部の周波数の値。`x` が n より長い時は呼ばない。
    pub fn spectrum(&self, x: &[f64]) -> Vec<C> {
        debug_assert!(x.len() <= self.n);
        let mut s = vec![C::new(0.0, 0.0); self.n];
        for (d, &v) in s.iter_mut().zip(x) {
            d.re = v;
        }
        self.forward(&mut s);
        s
    }

    /// 逆変換の実部(長さ n)。
    pub fn real(&self, mut s: Vec<C>) -> Vec<f64> {
        self.inverse(&mut s);
        s.into_iter().map(|v| v.re).collect()
    }

    fn run(&self, x: &mut [C], inverse: bool) {
        let n = self.n;
        assert_eq!(x.len(), n, "FFT の長さと列の長さが違う");
        for i in 0..n {
            let j = self.rev[i];
            if i < j {
                x.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let w = self.twiddle[k * step];
                    let w = if inverse { w.conj() } else { w };
                    let a = x[start + k];
                    let b = x[start + k + half] * w;
                    x[start + k] = a + b;
                    x[start + k + half] = a - b;
                }
            }
            len <<= 1;
        }
    }
}

/// 周波数の番号 k(0..n)の符号付きの値。n/2 までは正、その先は k − n(遅れの位相の傾きに使う)。
pub fn signed_bin(k: usize, n: usize) -> f64 {
    if k <= n / 2 {
        k as f64
    } else {
        k as f64 - n as f64
    }
}

/// 長さ n の枠の中で、列を `tau` sample 進める(負なら遅らせる)。小数も周波数の位相の傾きで扱い、
/// 枠の外へ出た分は反対の端へ回る。
pub fn advance(fft: &Fft, x: &[f64], tau: f64) -> Vec<f64> {
    let n = fft.len();
    let mut s = fft.spectrum(x);
    for (k, v) in s.iter_mut().enumerate() {
        let a = 2.0 * std::f64::consts::PI * signed_bin(k, n) * tau / n as f64;
        *v *= C::new(a.cos(), a.sin());
    }
    fft.real(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: 順変換が素の DFT と一致し、逆変換で元に戻る。
    #[test]
    fn matches_plain_dft_and_round_trips() {
        let n = 16;
        let x: Vec<f64> = (0..n).map(|t| ((t * 7 % 5) as f64 - 2.0) * 0.3).collect();
        let fft = Fft::new(n);
        let s = fft.spectrum(&x);
        for (k, got) in s.iter().enumerate() {
            let mut want = C::new(0.0, 0.0);
            for (t, &v) in x.iter().enumerate() {
                let a = -2.0 * std::f64::consts::PI * (k * t) as f64 / n as f64;
                want += C::new(a.cos(), a.sin()) * v;
            }
            assert!((*got - want).norm() < 1e-12, "k={k}");
        }
        let back = fft.real(s);
        for (a, b) in x.iter().zip(&back) {
            assert!((a - b).abs() < 1e-12);
        }
    }

    /// 確かめること: 整数の進めは列を回すのと同じになる。
    #[test]
    fn integer_advance_rotates() {
        let fft = Fft::new(8);
        let x = [0.0, 0.0, 1.0, 0.5, 0.0, 0.0, 0.0, 0.0];
        let y = advance(&fft, &x, 2.0);
        let want = [1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        for (a, b) in y.iter().zip(want) {
            assert!((a - b).abs() < 1e-12);
        }
    }
}
