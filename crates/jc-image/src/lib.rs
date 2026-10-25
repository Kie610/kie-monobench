//! 写真や図の絵(PNG・JPEG・BMP)を読む・縮める。下絵(スケッチの下に置く絵)が使う。
//! HEIC・WebP などは読めない(読める形の名前を [`KINDS`] に出す)。`image` の crate を使うので、
//! WASM のプラグインも使う jc-format には入れず、別の crate にした(プラグインの依存を増やさない)。

use image::{ImageFormat, ImageReader};
use std::io::Cursor;

/// 読める絵の形の拡張子。
pub const KINDS: &[&str] = &["png", "jpg", "jpeg", "bmp"];

/// 絵の形(PNG・JPEG・BMP)と幅・高さ。読めない形なら理由。
pub fn probe(bytes: &[u8]) -> Result<(&'static str, u32, u32), String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let kind = match reader.format() {
        Some(ImageFormat::Png) => "png",
        Some(ImageFormat::Jpeg) => "jpeg",
        Some(ImageFormat::Bmp) => "bmp",
        _ => {
            return Err(
                "読める絵は PNG・JPEG・BMP です(HEIC などは PNG か JPEG にしてください)".into(),
            );
        }
    };
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 {
        return Err("幅か高さが 0 の絵です".into());
    }
    Ok((kind, w, h))
}

/// 絵を RGBA(1 画素 4 バイト、上の行から)にする。長い辺が `max_side` を超えれば縮める。
/// (幅, 高さ, RGBA) を返す。
pub fn decode_rgba(bytes: &[u8], max_side: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let img = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| format!("絵を読めません({e})"))?;
    let img = if img.width().max(img.height()) > max_side {
        img.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// 長い辺が `max_side` 以下の絵はそのまま、超える絵は縮めて書き直す(透明の無い絵は JPEG、
/// ある絵は PNG)。文書に入れる前に大きさを抑える。(中身, 幅, 高さ) を返す。
pub fn fit(bytes: &[u8], max_side: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let (_, w, h) = probe(bytes)?;
    if w.max(h) <= max_side {
        return Ok((bytes.to_vec(), w, h));
    }
    let img = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| format!("絵を読めません({e})"))?
        .resize(max_side, max_side, image::imageops::FilterType::Triangle);
    let mut out = Cursor::new(Vec::new());
    if img.color().has_alpha() {
        img.write_to(&mut out, ImageFormat::Png)
    } else {
        image::DynamicImage::ImageRgb8(img.to_rgb8()).write_to(&mut out, ImageFormat::Jpeg)
    }
    .map_err(|e| format!("縮めた絵を書けません({e})"))?;
    Ok((out.into_inner(), img.width(), img.height()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_png_size_and_shrinks_large_pictures() {
        // 横 6・縦 2 の RGBA(左半分は赤、右半分は半透明の青)
        let mut px = Vec::new();
        for _ in 0..2 {
            for x in 0..6 {
                px.extend(if x < 3 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 128]
                });
            }
        }
        let png = jc_format::png::write_rgba(6, 2, &px);
        assert_eq!(probe(&png).unwrap(), ("png", 6, 2));
        let (w, h, rgba) = decode_rgba(&png, 100).unwrap();
        assert_eq!((w, h), (6, 2));
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
        // 長い辺 3 に縮めると 3×1。透明があるので PNG のまま
        let (small, w, h) = fit(&png, 3).unwrap();
        assert_eq!((w, h), (3, 1));
        assert_eq!(probe(&small).unwrap().0, "png");
        // 小さい絵はそのまま
        assert_eq!(fit(&png, 10).unwrap().0, png);
        assert!(probe(b"not a picture").is_err());
    }
}
