//! 確かめ用の解析解(球)。時間の向きは e^{jωt}、外へ出る波は球ハンケル関数 h_n^{(2)} = j_n − j·y_n。
//!
//! 平面波は +z の向きに進む e^{−jkz} = Σ (2n+1)(−j)^n j_n(kr) P_n(cos θ)(θ は +z から測る)。

use num_complex::Complex64 as C;

/// 半径 `a` の球で、+z へ進む振幅 1 の平面波が散る時の音圧(来た音との和)。
/// 点は球の中心から `r`(≥ a)、+z からの角度の余弦 `cos_theta`。面は比音響アドミタンス `beta`
/// (0 なら剛い球。面の条件は ∂p/∂r = jkβp)。
pub fn sphere_plane_wave(k: f64, a: f64, r: f64, cos_theta: f64, beta: C) -> C {
    // 来た音は閉じた式で、散った音は ka で決まる項の数までの級数で足す
    let terms = terms(k * a);
    let (ja, ya) = bessel(k * a, terms + 1);
    let (jr, yr) = bessel(k * r, terms);
    let p = legendre(cos_theta, terms);
    let ph = -k * r * cos_theta;
    let mut sum = C::new(ph.cos(), ph.sin());
    for (n, pn) in p.iter().enumerate() {
        let cn = C::new(0.0, -1.0).powu(n as u32) * (2 * n + 1) as f64;
        let an = coefficient(n, k * a, &ja, &ya, beta);
        sum += cn * an * C::new(jr[n], -yr[n]) * pn;
    }
    sum
}

/// [`sphere_plane_wave`] の散った音だけの遠方の振幅 F(音圧は F·e^{−jkr}/r)。
pub fn sphere_plane_wave_far(k: f64, a: f64, cos_theta: f64, beta: C) -> C {
    let terms = terms(k * a);
    let (ja, ya) = bessel(k * a, terms + 1);
    let p = legendre(cos_theta, terms);
    let mut sum = C::new(0.0, 0.0);
    for (n, pn) in p.iter().enumerate() {
        let cn = C::new(0.0, -1.0).powu(n as u32) * (2 * n + 1) as f64;
        let an = coefficient(n, k * a, &ja, &ya, beta);
        // h_n^{(2)}(x) → j^{n+1} e^{−jx}/x
        sum += cn * an * C::new(0.0, 1.0).powu(n as u32 + 1) * pn;
    }
    sum / k
}

/// 半径 `a` の球の全面が外へ速さ `v`(m/s)で振動する時の、中心から `r` の音圧。
pub fn pulsating_sphere(k: f64, a: f64, r: f64, density: f64, speed_of_sound: f64, v: C) -> C {
    let omega = k * speed_of_sound;
    // p(r) = jωρv a² e^{−jk(r−a)} / ((1 + jka) r)
    let ph = -k * (r - a);
    C::new(0.0, omega * density) * v * a * a * C::new(ph.cos(), ph.sin()) / (C::new(1.0, k * a) * r)
}

/// 散った波の係数 A_n(面の条件 j_n' + A h_n' = jβ(j_n + A h_n))。
fn coefficient(n: usize, x: f64, j: &[f64], y: &[f64], beta: C) -> C {
    let (jd, yd) = (derivative(j, n, x), derivative(y, n, x));
    let jb = C::new(0.0, 1.0) * beta;
    let h = C::new(j[n], -y[n]);
    let hd = C::new(jd, -yd);
    -(jd - jb * j[n]) / (hd - jb * h)
}

fn terms(x: f64) -> usize {
    (x + 4.0 * x.cbrt() + 12.0).ceil() as usize
}

/// f_n' = f_{n−1} − (n+1)/x f_n(n ≥ 1)、f_0' = −f_1。
fn derivative(f: &[f64], n: usize, x: f64) -> f64 {
    if n == 0 {
        -f[1]
    } else {
        f[n - 1] - (n as f64 + 1.0) / x * f[n]
    }
}

/// 球ベッセル関数 j_n と y_n(n = 0..=nmax)。j は下向きの漸化式で作り j_0 か j_1 でそろえる
/// (上向きは n > x で崩れる)。y は上向きで安定。
pub(crate) fn bessel(x: f64, nmax: usize) -> (Vec<f64>, Vec<f64>) {
    let mut y = vec![0.0; nmax + 1];
    y[0] = -x.cos() / x;
    if nmax >= 1 {
        y[1] = -x.cos() / (x * x) - x.sin() / x;
    }
    for n in 1..nmax {
        y[n + 1] = (2 * n + 1) as f64 / x * y[n] - y[n - 1];
    }
    // 下向きの漸化式は j_n が減っていく所(n > x)から始めると収まる。x^{1/3} の幅を余分に取る
    let start = nmax.max(x as usize) + 20 + (10.0 * x.cbrt()) as usize;
    let mut f = vec![0.0; start + 2];
    f[start] = 1e-30;
    for n in (1..=start).rev() {
        f[n - 1] = (2 * n + 1) as f64 / x * f[n] - f[n + 1];
        // 大きくなりすぎたらそろえ直す(比だけが要る)
        if f[n - 1].abs() > 1e250 {
            for v in f.iter_mut().skip(n - 1) {
                *v *= 1e-250;
            }
        }
    }
    let j0 = x.sin() / x;
    let j1 = x.sin() / (x * x) - x.cos() / x;
    let s = if j0.abs() >= j1.abs() {
        j0 / f[0]
    } else {
        j1 / f[1]
    };
    let j = f[..=nmax].iter().map(|v| v * s).collect();
    (j, y)
}

/// ルジャンドル多項式 P_n(μ)(n = 0..=nmax)。
pub(crate) fn legendre(mu: f64, nmax: usize) -> Vec<f64> {
    let mut p = vec![0.0; nmax + 1];
    p[0] = 1.0;
    if nmax >= 1 {
        p[1] = mu;
    }
    for n in 1..nmax {
        p[n + 1] = ((2 * n + 1) as f64 * mu * p[n] - n as f64 * p[n - 1]) / (n + 1) as f64;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: 球ベッセル関数が閉じた式(n = 2)と、ロンスキアン j_n y_n' − j_n' y_n = 1/x² に合う。
    #[test]
    fn spherical_bessel_matches_closed_forms() {
        for x in [0.3, 1.0, 3.0, 12.0] {
            let (j, y) = bessel(x, 30);
            let j2 = (3.0 / (x * x) - 1.0) * x.sin() / x - 3.0 * x.cos() / (x * x);
            let y2 = -(3.0 / (x * x) - 1.0) * x.cos() / x - 3.0 * x.sin() / (x * x);
            assert!((j[2] - j2).abs() < 1e-12, "{x}");
            assert!((y[2] - y2).abs() < 1e-9 * y2.abs().max(1.0), "{x}");
            for n in 0..25 {
                let w = j[n] * derivative(&y, n, x) - derivative(&j, n, x) * y[n];
                assert!((w * x * x - 1.0).abs() < 1e-8, "x {x} n {n}: {w}");
            }
        }
    }

    /// 確かめること: 剛い球の面の音圧が、ka が小さい時の近似(1 + 1.5 ka cos θ の大きさ)に近い。
    #[test]
    fn small_rigid_sphere_follows_the_low_frequency_limit() {
        let (k, a) = (0.05, 1.0);
        for mu in [-1.0, 0.0, 1.0] {
            let p = sphere_plane_wave(k, a, a, mu, C::new(0.0, 0.0));
            // e^{−jkz} の面での値に、剛い球の双極の寄与(係数 3/2)が足される
            let want = C::new(1.0, -1.5 * k * a * mu);
            assert!((p - want).norm() < 5e-3, "{mu}: {p} {want}");
        }
    }

    /// 確かめること: 遠方の振幅は、遠い点の散った音 × r·e^{jkr} に近づく。
    #[test]
    fn far_field_is_the_limit_of_the_near_field() {
        let (k, a, r) = (20.0, 0.1, 4000.0);
        let beta = C::new(0.3, 0.1);
        for mu in [-0.7, 0.2, 0.9] {
            let total = sphere_plane_wave(k, a, r, mu, beta);
            let ph = -k * r * mu;
            let scat = total - C::new(ph.cos(), ph.sin());
            let f = scat * r * C::new((k * r).cos(), (k * r).sin());
            let want = sphere_plane_wave_far(k, a, mu, beta);
            assert!((f - want).norm() < 2e-2 * want.norm(), "{mu}: {f} {want}");
        }
    }
}
