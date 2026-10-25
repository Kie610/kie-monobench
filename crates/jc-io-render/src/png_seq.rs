//! 絵を 1 枚ずつ PNG にする(連番 PNG)。動画の編集の道具で読み込める。

use crate::Rgba;

/// 1 枚の絵を PNG(RGBA、8 ビット)にする。
///
/// # Panics
///
/// 画素の数が幅と高さに合わない時([`Rgba::is_valid`] が偽)。
pub fn png_bytes(img: &Rgba) -> Vec<u8> {
    assert!(img.is_valid(), "画素の数が幅と高さに合いません");
    jc_format::png::write_rgba(img.width, img.height, &img.pixels)
}

/// 絵を `stem-0001.png`、`stem-0002.png`… の名前と中身の組にする(番号は 1 から、4 桁以上)。
pub fn png_sequence(frames: &[Rgba], stem: &str) -> Vec<(String, Vec<u8>)> {
    frames
        .iter()
        .enumerate()
        .map(|(i, f)| (format!("{stem}-{:04}.png", i + 1), png_bytes(f)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_reads_back_with_the_same_pixels() {
        let frames = crate::test_frames(17, 9, 3);
        let seq = png_sequence(&frames, "回転");
        let names: Vec<&str> = seq.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["回転-0001.png", "回転-0002.png", "回転-0003.png"]);
        let dec = png::Decoder::new(std::io::Cursor::new(&seq[1].1));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (17, 9));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(&buf[..info.buffer_size()], &frames[1].pixels[..]);
    }
}
