//! 光線追跡のノイズを減らす。À-Trous のウェーブレットの平滑化(Dammertz ほか 2010)を、1 回目に
//! 当たった所の色・法線・奥行きを手がかりにして、縁をまたがないようにかける。色は材質の色で割って
//! (光の量だけにして)ならし、後で掛け戻す(木目などの模様をぼかさない)。

use crate::math::*;
use crate::trace::Aov;

/// 5×5 の B3 スプラインの重み(1 次元)。
const KERNEL: [f32; 5] = [1.0 / 16.0, 1.0 / 4.0, 3.0 / 8.0, 1.0 / 4.0, 1.0 / 16.0];

/// `color`(線形)をならした色。`passes` は回した数(多いほどノイズが少ないので弱くかける)。
pub(crate) fn atrous(color: &[V3], aov: &[Aov], w: usize, h: usize, passes: u32) -> Vec<V3> {
    if w == 0 || h == 0 {
        return color.to_vec();
    }
    // 光の量(材質の色で割る)
    let mut light: Vec<V3> = color
        .iter()
        .zip(aov)
        .map(|(c, a)| [0, 1, 2].map(|k| c[k] / a.albedo[k].max(0.02)))
        .collect();
    // 回した数が多いほど色の差の許しを狭める
    let sigma_c0 = 4.0 / (passes as f32).sqrt().max(1.0);
    let iterations = 5;
    for it in 0..iterations {
        let step = 1usize << it;
        let sigma_c = sigma_c0 / (1u32 << it) as f32 + 0.02;
        let src = light.clone();
        let src = &src;
        crate::par_rows(&mut light, w, |y, row| {
            for (x, out) in row.iter_mut().enumerate() {
                let i = y * w + x;
                let c0 = src[i];
                let a0 = aov[i];
                let mut sum = [0.0f32; 3];
                let mut wsum = 0.0;
                for (ky, kwy) in KERNEL.iter().enumerate() {
                    let yy = y as isize + (ky as isize - 2) * step as isize;
                    if yy < 0 || yy >= h as isize {
                        continue;
                    }
                    for (kx, kwx) in KERNEL.iter().enumerate() {
                        let xx = x as isize + (kx as isize - 2) * step as isize;
                        if xx < 0 || xx >= w as isize {
                            continue;
                        }
                        let j = yy as usize * w + xx as usize;
                        let c = src[j];
                        let a = aov[j];
                        let dc = sub(c, c0);
                        let wc = (-dot(dc, dc).sqrt() / sigma_c).exp();
                        let wn = dot(a.normal, a0.normal).max(0.0).powi(32);
                        let dz = (a.depth - a0.depth).abs()
                            / (a0.depth.abs().max(1.0) * 0.01 * step as f32);
                        let wz = (-dz).exp();
                        let da = sub(a.albedo, a0.albedo);
                        let wa = (-dot(da, da) * 40.0).exp();
                        let k = kwx * kwy * wc * wn * wz * wa;
                        sum = madd(sum, c, k);
                        wsum += k;
                    }
                }
                *out = if wsum > 1e-9 {
                    scale(sum, 1.0 / wsum)
                } else {
                    c0
                };
            }
        });
    }
    light
        .iter()
        .zip(aov)
        .map(|(l, a)| [0, 1, 2].map(|k| l[k] * a.albedo[k].max(0.02)))
        .collect()
}
