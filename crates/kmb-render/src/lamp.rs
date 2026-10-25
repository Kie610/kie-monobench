//! 点光源とスポットライト。文書の光源の機能の出力
//! ([`kmb_look::decor::Lamp`])を、描く時の値にする。
//!
//! 明るさは lm で打ち、光度 I = lm / 4π(cd)、距離 d(m)の所の照度 I / d²(lx)にする。環境の光は
//! 1000 lx で既定の明るさになるように決めてあるので、ランプの照度も同じ物差しで環境の光に足す
//! (800 lm の電球を 30 cm 離すと約 700 lx で、既定の環境の光と同じくらい)。露出(EV)は効き、
//! 環境の明るさ(lx)は効かない(環境の明るさを下げて電球だけで照らせる)。

use crate::math::{V3, dot, normalize, scale, smoothstep, srgb_to_linear, sub};

/// 描く時の光源 1 つ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lamp {
    /// 位置(mm)
    pub pos: V3,
    /// スポットの向き(長さ 1)
    pub dir: V3,
    /// スポットの円すいの外と内の cos(点光源はどちらも -1 より小さく、どこにも切らない)
    pub cos_outer: f32,
    pub cos_inner: f32,
    /// 距離 1 mm・正面の白い拡散の面の輝度の π 倍(線形の色。[`Lamp::white_at`])
    pub power: V3,
    /// 大きさ(半径 mm。影の縁のぼけ)
    pub radius: f32,
}

/// 照度 1 lx が、白い拡散の面に返す輝度(環境の 1000 lx の明るさから)。
fn lx_unit() -> f32 {
    let env = crate::Env::new(crate::EnvKind::StudioLight, [0.0, -1.0, 0.0]);
    let c = env.diffuse([0.0, 0.0, 1.0], false);
    (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) / crate::settings::DEFAULT_LX
}

impl Lamp {
    /// 文書の光源の機能の出力から。
    pub fn from_doc(l: &kmb_look::decor::Lamp) -> Lamp {
        let cd = (l.lumens.max(0.0) / (4.0 * std::f64::consts::PI)) as f32;
        // 白い面の輝度 = 照度(lx)× 単位。照度 = cd / d²(m)、1 m² = 1e6 mm²。光線追跡の材質の式は
        // アルベド / π を掛けるので、π を掛けておく([`Lamp::white_at`] は π で割る)
        let k = cd * lx_unit() * 1.0e6 * std::f32::consts::PI;
        let color = l.color.map(srgb_to_linear);
        let (outer, inner) = if l.spot {
            let half = (l.cone_deg.clamp(1.0, 179.0) / 2.0) as f32;
            let blur = (l.blur_deg.max(0.0) as f32).min(half);
            (
                half.to_radians().cos(),
                (half - blur).max(0.0).to_radians().cos(),
            )
        } else {
            (-2.0, -2.0)
        };
        Lamp {
            pos: l.position.map(|v| v as f32),
            dir: normalize(l.direction.map(|v| v as f32)),
            cos_outer: outer,
            cos_inner: inner.max(outer + 1e-4),
            power: scale(color, k),
            radius: l.radius.max(0.0) as f32,
        }
    }

    /// 光源から向き `out`(光源から外へ、長さ 1)への明るさの割合(スポットの円すいの外は 0)。
    pub fn spot(&self, out: V3) -> f32 {
        if self.cos_outer < -1.0 {
            return 1.0;
        }
        smoothstep(self.cos_outer, self.cos_inner, dot(out, self.dir))
    }

    /// 点 `p`・法線 `n` の白い拡散の面に返す輝度(影は見ない。露出の前)。
    pub fn white_at(&self, p: V3, n: V3) -> V3 {
        let to = sub(self.pos, p);
        let d2 = dot(to, to).max(self.radius * self.radius).max(1e-6);
        let l = scale(to, 1.0 / d2.sqrt());
        let c = dot(n, l);
        if c <= 0.0 {
            return [0.0; 3];
        }
        let k = c * self.spot(scale(l, -1.0)) / (std::f32::consts::PI * d2);
        scale(self.power, k)
    }
}

/// 機能の出力(光源の出力 [`kmb_look::decor::LAMP`] の値)から、描く光源。
pub fn lamps_of<'a>(outputs: impl IntoIterator<Item = &'a kmb_core::Value>) -> Vec<Lamp> {
    use kmb_core::Payload;
    outputs
        .into_iter()
        .filter_map(kmb_look::decor::Lamp::from_value)
        .map(|l| Lamp::from_doc(&l))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bulb(lm: f64) -> kmb_look::decor::Lamp {
        kmb_look::decor::Lamp {
            spot: false,
            position: [0.0, 0.0, 300.0],
            direction: [0.0, 0.0, -1.0],
            color: [255; 3],
            lumens: lm,
            cone_deg: 40.0,
            blur_deg: 10.0,
            radius: 0.0,
        }
    }

    #[test]
    fn a_bulb_lights_like_the_lux_it_gives() {
        // 800 lm を 30 cm 上に: 照度 800/4π/0.09 ≒ 707 lx。環境の 1000 lx の 0.707 倍の明るさ
        let l = Lamp::from_doc(&bulb(800.0));
        let w = l.white_at([0.0; 3], [0.0, 0.0, 1.0]);
        let env = crate::Env::new(crate::EnvKind::StudioLight, [0.0, -1.0, 0.0]);
        let e = env.diffuse([0.0, 0.0, 1.0], false);
        let lum = |c: V3| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let ratio = lum(w) / lum(e);
        assert!((ratio - 0.7074).abs() < 0.01, "{ratio}");
        // 距離が 2 倍なら 1/4、裏の面は 0
        let far = l.white_at([0.0, 0.0, -300.0], [0.0, 0.0, 1.0]);
        assert!((lum(far) / lum(w) - 0.25).abs() < 1e-3);
        assert_eq!(l.white_at([0.0; 3], [0.0, 0.0, -1.0]), [0.0; 3]);
    }

    #[test]
    fn a_spot_cuts_outside_its_cone() {
        let mut d = bulb(800.0);
        d.spot = true;
        let l = Lamp::from_doc(&d);
        // 真下は全部、40 度の円すいの外(真下から 30 度)は 0
        assert_eq!(l.spot([0.0, 0.0, -1.0]), 1.0);
        let out = normalize([0.5, 0.0, -(0.75f32).sqrt()]);
        assert_eq!(l.spot(out), 0.0);
        // 縁のぼかしの中(真下から 15 度)は間
        let a = 15f32.to_radians();
        let mid = l.spot([a.sin(), 0.0, -a.cos()]);
        assert!(mid > 0.0 && mid < 1.0, "{mid}");
    }
}
