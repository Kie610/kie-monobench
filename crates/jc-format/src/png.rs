//! 8 ビットの PNG の書き手と、書いた RGB・RGBA の PNG の読み手。

/// 8 ビットの RGB(1 画素 3 バイト、上の行から)を PNG にする。
pub fn write_rgb(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    write(width, height, rgb, 3)
}

/// 8 ビットの RGBA(1 画素 4 バイト、上の行から)を PNG にする。
pub fn write_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    write(width, height, rgba, 4)
}

fn write(width: u32, height: u32, px: &[u8], channels: usize) -> Vec<u8> {
    let row = width as usize * channels;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in px.chunks(row.max(1)).take(height as usize) {
        raw.push(0); // 行ごとの前処理なし
        raw.extend_from_slice(line);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    // 8 ビット、RGB(2)か RGBA(6)、deflate、標準の前処理、飛び越しなし
    ihdr.extend([8, if channels == 4 { 6 } else { 2 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(
        &mut out,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend(kind);
    out.extend(data);
    out.extend(crc.finalize().to_be_bytes());
}

/// [`write_rgb`] が書いた PNG(8 ビットの RGB、行の前処理なし、飛び越しなし)を読み戻す。
/// ほかの形の PNG と、幅か高さが 0 か `max_side` を超える物は `None`。(幅, 高さ, RGB) を返す。
pub fn read_rgb(bytes: &[u8], max_side: u32) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h, channels, px) = read(bytes, max_side)?;
    (channels == 3).then_some((w, h, px))
}

/// [`write_rgb`] か [`write_rgba`] が書いた PNG を RGBA で読み戻す(RGB なら不透明)。
/// (幅, 高さ, RGBA) を返す。
pub fn read_rgba(bytes: &[u8], max_side: u32) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h, channels, px) = read(bytes, max_side)?;
    if channels == 4 {
        return Some((w, h, px));
    }
    let mut out = Vec::with_capacity(px.len() / 3 * 4);
    for c in px.chunks(3) {
        out.extend_from_slice(c);
        out.push(255);
    }
    Some((w, h, out))
}

/// このモジュールが書いた形の PNG を読む。(幅, 高さ, 1 画素のバイト数, 画素)。
fn read(bytes: &[u8], max_side: u32) -> Option<(u32, u32, usize, Vec<u8>)> {
    let mut rest = bytes.strip_prefix(b"\x89PNG\r\n\x1a\n")?;
    let (mut width, mut height, mut channels) = (0u32, 0u32, 0usize);
    let mut idat = Vec::new();
    while rest.len() >= 12 {
        let len = u32::from_be_bytes(rest[0..4].try_into().ok()?) as usize;
        let kind = &rest[4..8];
        let data = rest.get(8..8 + len)?;
        match kind {
            b"IHDR" => {
                width = u32::from_be_bytes(data.get(0..4)?.try_into().ok()?);
                height = u32::from_be_bytes(data.get(4..8)?.try_into().ok()?);
                channels = match data.get(8..13)? {
                    [8, 2, 0, 0, 0] => 3,
                    [8, 6, 0, 0, 0] => 4,
                    _ => return None,
                };
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        rest = rest.get(12 + len..)?;
    }
    if width == 0 || height == 0 || width > max_side || height > max_side {
        return None;
    }
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&idat).ok()?;
    let row = width as usize * channels;
    if raw.len() != (row + 1) * height as usize {
        return None;
    }
    let mut px = Vec::with_capacity(row * height as usize);
    for line in raw.chunks(row + 1) {
        if line[0] != 0 {
            return None;
        }
        px.extend_from_slice(&line[1..]);
    }
    Some((width, height, channels, px))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_reads_back_and_rgba_has_the_alpha_color_type() {
        let rgb: Vec<u8> = (0..2 * 3 * 3).map(|i| i as u8 * 7).collect();
        let png = write_rgb(3, 2, &rgb);
        assert_eq!(read_rgb(&png, 100), Some((3, 2, rgb)));
        assert_eq!(read_rgb(&png, 2), None);
        let rgba = write_rgba(1, 1, &[1, 2, 3, 4]);
        // IHDR の色の種類(8 + 4 + 4 + 8 = 24 バイト目の次)
        assert_eq!(rgba[25], 6);
        assert_eq!(read_rgb(&rgba, 100), None);
        assert_eq!(read_rgba(&rgba, 100), Some((1, 1, vec![1, 2, 3, 4])));
        assert_eq!(read_rgba(&png, 100).map(|(_, _, p)| p[3]), Some(255));
    }
}
