//! 1 本の IR にかける信号処理: 立ち上がりの検出、最小位相、切り出しの窓、高い所のならし。

use crate::fft::{C, Fft};

/// 立ち上がりの検出の倍の補間。
pub const UP: usize = 8;
/// 立ち上がりの閾値(頂点からの dB)。
pub const ONSET_DB: f64 = -10.0;

/// 立ち上がりを見つける。IR を長さ n に 0 で伸ばし、2〜4 kHz の余弦の縁(3 kHz で −6 dB)で高い所を
/// 落としてから 8 倍に補間し、包絡線が頂点の −10 dB を初めて超える点を小数の sample で返す。
/// 余弦の縁は前に出る揺れを閾値より十分小さく保つ(切り立った縁の第 1 の側帯は −13 dB で閾値に近い)。
pub struct OnsetFinder {
    fft: Fft,
    up: Fft,
    weight: Vec<f64>,
}

impl OnsetFinder {
    /// 長さ `n`(2 の累乗)の枠で、サンプリング周波数 `fs` の IR を調べる用意。
    pub fn new(n: usize, fs: f64) -> OnsetFinder {
        let weight = (0..=n / 2)
            .map(|k| {
                let f = k as f64 * fs / n as f64;
                0.5 + 0.5 * (std::f64::consts::PI * ((f - 2000.0) / 2000.0).clamp(0.0, 1.0)).cos()
            })
            .collect();
        OnsetFinder {
            fft: Fft::new(n),
            up: Fft::new(n * UP),
            weight,
        }
    }

    pub fn frame(&self) -> usize {
        self.fft.len()
    }

    /// `ir`(長さは枠以下)の立ち上がり(sample)。
    pub fn find(&self, ir: &[f64]) -> f64 {
        let n = self.fft.len();
        let s = self.fft.spectrum(ir);
        let mut big = vec![C::new(0.0, 0.0); n * UP];
        big[0] = s[0] * self.weight[0];
        // n/2(ナイキスト)の項は 4 kHz より上で重みが 0 なので入れない。
        for k in 1..n / 2 {
            let v = s[k] * self.weight[k];
            big[k] = v;
            big[n * UP - k] = v.conj();
        }
        self.up.inverse(&mut big);
        let env: Vec<f64> = big.iter().map(|v| v.re.abs()).collect();
        let peak = env.iter().cloned().fold(0.0, f64::max);
        if peak <= 0.0 {
            return 0.0;
        }
        let thr = peak * 10f64.powf(ONSET_DB / 20.0);
        let i = env.iter().position(|&v| v >= thr).unwrap_or(0);
        i as f64 / UP as f64
    }
}

/// 大きさの特性から最小位相の IR を作る。IR を長さ 8F に伸ばした大きさ(1e−9 で下を止める)の
/// 対数から実ケプストラムを取り、因果の側へ折り返して指数を取る。
pub struct MinPhase {
    frame: usize,
    fft: Fft,
}

impl MinPhase {
    pub fn new(frame: usize) -> MinPhase {
        MinPhase {
            frame,
            fft: Fft::new(frame * UP),
        }
    }

    /// `ir`(長さは枠以下の時間の並び)と同じ大きさの最小位相の IR を、頭の `keep` tap に切って返す。
    /// 切った終わりの max(keep / 8, 4) 点は半ハン窓で 0 へならす。輪の枠の IR は、立ち上がりの前の分が
    /// 枠の終わりに回ったままだと細かい周波数で大きさが波打ち、最小位相の IR が長く伸びて切った所で
    /// 大きさが崩れるので、呼ぶ側が先に時間の並びへ戻す(`linear_from_circular`)。
    pub fn ir(&self, ir: &[f64], keep: usize) -> Vec<f64> {
        let n = self.fft.len();
        let s = self.fft.spectrum(ir);
        let mut c: Vec<C> = s
            .iter()
            .map(|v| C::new(v.norm().max(1e-9).ln(), 0.0))
            .collect();
        self.fft.inverse(&mut c);
        for (t, v) in c.iter_mut().enumerate() {
            v.im = 0.0;
            if t > 0 && t < n / 2 {
                v.re *= 2.0;
            } else if t > n / 2 {
                v.re = 0.0;
            }
        }
        self.fft.forward(&mut c);
        for v in c.iter_mut() {
            *v = v.exp();
        }
        let mut h = self.fft.real(c);
        let keep = keep.min(self.frame);
        h.truncate(keep);
        let k = (keep / 8).max(4).min(keep);
        for (v, w) in h[keep - k..].iter_mut().zip(half_hann(k).into_iter().rev()) {
            *v *= w;
        }
        h
    }
}

/// 輪の枠の IR(立ち上がりが 0 番、前の分が終わりに回っている)を、立ち上がりの前に枠の 5/16
/// (512 なら 160 tap)を空けた時間の並びに戻す。最小位相を取る前に使う。
pub fn linear_from_circular(ir: &[f64]) -> Vec<f64> {
    let mut out = ir.to_vec();
    out.rotate_right(ir.len() * 5 / 16);
    out
}

/// 半ハン窓の立ち上がり k 点(0 から 1 へ)。
pub fn half_hann(k: usize) -> Vec<f64> {
    (0..k)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::PI * (i as f64 + 0.5) / k as f64).cos())
        .collect()
}

/// 長さ `frame` の輪の枠で、0 番の立ち上がりの前 `before`・後ろ `after` tap を残す窓。端は半ハン窓で
/// ならす(前は `fade_before` 点で立ち上がり、後ろは `fade_after` 点で下がる)。
pub fn trim_window(
    frame: usize,
    before: usize,
    after: usize,
    fade_before: usize,
    fade_after: usize,
) -> Vec<f64> {
    let mut w = vec![0.0; frame];
    w[..after].fill(1.0);
    let ka = fade_after.min(after);
    for (i, v) in half_hann(ka).into_iter().rev().enumerate() {
        w[after - ka + i] = v;
    }
    if before > 0 {
        let kb = fade_before.min(before);
        w[frame - before..].fill(1.0);
        for (i, v) in half_hann(kb).into_iter().enumerate() {
            w[frame - before + i] = v;
        }
    }
    w
}

/// 周波数 `from_hz` からナイキストまで上げ余弦で 0 へ下げる重み(番号 0..=F/2)。
pub fn rolloff(frame: usize, fs: f64, from_hz: f64) -> Vec<f64> {
    (0..=frame / 2)
        .map(|k| {
            let f = k as f64 * fs / frame as f64;
            let x = ((f - from_hz) / (fs / 2.0 - from_hz)).clamp(0.0, 1.0);
            0.5 + 0.5 * (std::f64::consts::PI * x).cos()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: 立ち上がりは整数の遅れにそのまま付いて動き、頂点より少し前に出る。
    #[test]
    fn onset_follows_delay() {
        let finder = OnsetFinder::new(512, 48000.0);
        let mut a = vec![0.0; 256];
        a[20] = 1.0;
        a[21] = -0.4;
        let mut b = vec![0.0; 256];
        b[30] = 1.0;
        b[31] = -0.4;
        let (ta, tb) = (finder.find(&a), finder.find(&b));
        assert!((tb - ta - 10.0).abs() < 1e-12, "{ta} {tb}");
        // 3 kHz で低域を通した山は幅が広く、頂点の −10 dB は頂点より数 sample 前に来る。
        assert!(ta > 10.0 && ta < 20.0, "{ta}");
    }

    /// 確かめること: 最小位相の IR は元と同じ大きさで、元が最小位相なら元に戻る。
    #[test]
    fn min_phase_keeps_magnitude() {
        let mp = MinPhase::new(64);
        // 零点が単位円の外にある(最小位相でない)短い IR。
        let x = [0.3, 1.0, 0.2];
        let h = mp.ir(&x, 64);
        let fft = Fft::new(64);
        let (sx, sh) = (fft.spectrum(&x), fft.spectrum(&h));
        for k in 0..64 {
            assert!((sx[k].norm() - sh[k].norm()).abs() < 1e-6, "k={k}");
        }
        // 最小位相の IR(零点が内側)はそのまま戻る。
        let y = [1.0, 0.5, 0.06];
        let g = mp.ir(&y, 64);
        for (t, v) in g.iter().enumerate() {
            let want = y.get(t).copied().unwrap_or(0.0);
            assert!((v - want).abs() < 1e-6, "t={t} {v}");
        }
    }

    /// 確かめること: 窓は前後の決めた tap だけを残し、端の半ハン窓が 0 と 1 の間でつながる。
    #[test]
    fn window_shape() {
        let w = trim_window(32, 4, 8, 4, 4);
        assert_eq!(w[..4], [1.0; 4]);
        assert!(w[4] < 1.0 && w[7] > 0.0 && w[7] < w[4]);
        assert!(w[8..28].iter().all(|&v| v == 0.0));
        assert!(w[28] > 0.0 && w[28] < w[31] && w[31] < 1.0);
    }
}
