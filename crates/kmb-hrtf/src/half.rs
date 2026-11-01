//! float16(IEEE 754 の半精度)との変換。分けた表は float16 の little-endian で書く。f64 から直に
//! 最も近い値へ丸め(同じ距離なら偶数の側)、f32 を経た二重の丸めを避ける。

/// f64 を float16 のビット列に丸める。収まらない大きさは無限大、非数は非数になる。
pub fn f16_bits(x: f64) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 48) & 0x8000) as u16;
    let exp = ((b >> 52) & 0x7ff) as i64;
    let man = b & ((1u64 << 52) - 1);
    if exp == 0x7ff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }
    let e = exp - 1023 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        // 非正規化数: 値 = r × 2^-24。e < -10 は 2^-25 未満で 0 に丸まる。
        if e < -10 {
            return sign;
        }
        let m = man | (1u64 << 52);
        let shift = (43 - e) as u32;
        let half = 1u64 << (shift - 1);
        let mut r = m >> shift;
        let rem = m & ((1u64 << shift) - 1);
        if rem > half || (rem == half && r & 1 == 1) {
            r += 1;
        }
        return sign | r as u16;
    }
    let mut r = ((e as u64) << 10) | (man >> 42);
    let rem = man & ((1u64 << 42) - 1);
    let half = 1u64 << 41;
    if rem > half || (rem == half && r & 1 == 1) {
        // 仮数の繰り上がりは指数へ、最大の指数からは無限大へ自然に進む。
        r += 1;
    }
    sign | r as u16
}

/// float16 のビット列を f64 に戻す。
pub fn f16_value(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = f64::from(h & 0x3ff);
    match e {
        0 => sign * m * 2f64.powi(-24),
        0x1f if m == 0.0 => sign * f64::INFINITY,
        0x1f => f64::NAN,
        _ => sign * (1.0 + m / 1024.0) * 2f64.powi(e - 15),
    }
}

/// f64 を float16 に丸めて戻した値。
pub fn round_f16(x: f64) -> f64 {
    f16_value(f16_bits(x))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: 代表の値のビット列、偶数への丸め、非正規化数、あふれ。
    #[test]
    fn known_bits() {
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(-2.0), 0xc000);
        assert_eq!(f16_bits(0.5), 0x3800);
        assert_eq!(f16_bits(65504.0), 0x7bff);
        assert_eq!(f16_bits(65520.0), 0x7c00);
        assert_eq!(f16_bits(1e9), 0x7c00);
        assert_eq!(f16_bits(2f64.powi(-24)), 0x0001);
        assert_eq!(f16_bits(2f64.powi(-25)), 0x0000);
        assert_eq!(f16_bits(1.5 * 2f64.powi(-25)), 0x0001);
        assert_eq!(f16_bits(2f64.powi(-14)), 0x0400);
        // 1 と次の値(1 + 2^-10)のちょうど中間は偶数の 1 へ、少し上は次へ。
        assert_eq!(f16_bits(1.0 + 2f64.powi(-11)), 0x3c00);
        assert_eq!(f16_bits(1.0 + 3.0 * 2f64.powi(-11)), 0x3c02);
        assert_eq!(f16_bits(1.0 + 2f64.powi(-11) + 1e-12), 0x3c01);
        assert!(f16_value(f16_bits(f64::NAN)).is_nan());
    }

    /// 確かめること: すべての有限な float16 は f64 を経て同じビット列に戻る。
    #[test]
    fn every_finite_value_round_trips() {
        for h in 0..=u16::MAX {
            if (h >> 10) & 0x1f == 0x1f {
                continue;
            }
            assert_eq!(f16_bits(f16_value(h)), h, "{h:#06x}");
        }
    }
}
