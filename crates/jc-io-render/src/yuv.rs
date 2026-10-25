//! RGBA(sRGB)を動画の YUV 4:2:0(限られた幅。Y は 16〜235、色差は 16〜240)にする。
//!
//! 色差は 2×2 の平均。幅と高さは偶数に切り詰める([`even_size`])。アルファは見ない(動画は透けない)。

use crate::Rgba;

/// RGB から YUV への式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix {
    /// SD の式(BT.601)。
    Bt601,
    /// HD の式(BT.709)。
    Bt709,
}

impl Matrix {
    /// 動画に式が書いていない時に再生する側が推す物(高さ 720 からは BT.709)。
    /// Windows の H.264 は式を流れに書くとは限らないので、こちらに合わせる。
    pub fn guessed(height: u32) -> Self {
        if height >= 720 {
            Self::Bt709
        } else {
            Self::Bt601
        }
    }

    /// (Y、U、V)の R・G・B の重み(256 倍)。
    fn weights(self) -> [[i32; 3]; 3] {
        match self {
            Self::Bt601 => [[66, 129, 25], [-38, -74, 112], [112, -94, -18]],
            // 色差の重みの和を 0 にして、灰色が 128 から外れないようにする
            Self::Bt709 => [[47, 157, 16], [-26, -86, 112], [112, -102, -10]],
        }
    }
}

/// 動画にする大きさ(偶数に切り詰めた物)。16×16 から 3840×2160 まで。
pub fn even_size(width: u32, height: u32) -> Result<(u32, u32), String> {
    let size = (width & !1, height & !1);
    if size.0 < 16 || size.1 < 16 {
        return Err(format!(
            "MP4 にできる大きさは 16×16 からです({width}×{height})"
        ));
    }
    if size.0.max(size.1) > 3840 || size.0.min(size.1) > 2160 {
        return Err(format!(
            "MP4 にできる大きさは 3840×2160 までです({width}×{height})"
        ));
    }
    Ok(size)
}

/// `img` の左上の `w`×`h`(偶数)を I420(Y の面、U の面、V の面の順)にして `out` に入れる。
pub fn rgba_to_i420(img: &Rgba, (w, h): (u32, u32), m: Matrix, out: &mut Vec<u8>) {
    let (w, h, stride) = (w as usize, h as usize, img.width as usize);
    let [ky, ku, kv] = m.weights();
    let dot = |k: [i32; 3], (r, g, b): (i32, i32, i32)| (k[0] * r + k[1] * g + k[2] * b + 128) >> 8;
    out.clear();
    out.resize(w * h * 3 / 2, 0);
    let px = |x: usize, y: usize| {
        let i = (y * stride + x) * 4;
        let p = &img.pixels[i..i + 3];
        (p[0] as i32, p[1] as i32, p[2] as i32)
    };
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = (dot(ky, px(x, y)) + 16) as u8;
        }
    }
    let (cw, ch) = (w / 2, h / 2);
    for cy in 0..ch {
        for cx in 0..cw {
            let (mut r, mut g, mut b) = (0, 0, 0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = px(cx * 2 + dx, cy * 2 + dy);
                r += p.0;
                g += p.1;
                b += p.2;
            }
            let avg = ((r + 2) / 4, (g + 2) / 4, (b + 2) / 4);
            out[w * h + cy * cw + cx] = (dot(ku, avg) + 128) as u8;
            out[w * h + cw * ch + cy * cw + cx] = (dot(kv, avg) + 128) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(c: [u8; 3], m: Matrix) -> (u8, u8, u8) {
        let mut img = Rgba::new(2, 2);
        for p in img.pixels.chunks_mut(4) {
            p.copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
        let mut out = Vec::new();
        rgba_to_i420(&img, (2, 2), m, &mut out);
        (out[0], out[4], out[5])
    }

    #[test]
    fn colours_become_limited_range() {
        for m in [Matrix::Bt601, Matrix::Bt709] {
            assert_eq!(one([0, 0, 0], m), (16, 128, 128));
            assert_eq!(one([255, 255, 255], m), (235, 128, 128));
        }
        // 赤は V が大きく、BT.709 の方が Y が小さい
        let (y601, _, v601) = one([255, 0, 0], Matrix::Bt601);
        let (y709, _, v709) = one([255, 0, 0], Matrix::Bt709);
        assert_eq!((y601, v601), (82, 240));
        assert_eq!((y709, v709), (63, 240));
    }

    #[test]
    fn sizes_are_made_even_and_checked() {
        assert_eq!(even_size(961, 541), Ok((960, 540)));
        assert!(even_size(15, 100).is_err());
        assert!(even_size(4000, 100).is_err());
        assert_eq!(Matrix::guessed(540), Matrix::Bt601);
        assert_eq!(Matrix::guessed(1080), Matrix::Bt709);
    }
}
