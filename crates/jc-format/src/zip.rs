//! zip の書き手と、書いた zip の読み手。

/// ファイルを並べた zip を作る(圧縮は deflate、時刻は 1980-01-01 0:00、名前は UTF-8)。
pub fn write(files: &[(&str, &[u8])]) -> Vec<u8> {
    // 時刻は 1980-01-01 0:00(zip の時刻の始まり)
    let (time, date) = (0u16, 0x21u16);
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        let crc = crc32fast::hash(data);
        let offset = out.len() as u32;
        let common = |o: &mut Vec<u8>| {
            o.extend(20u16.to_le_bytes()); // 読むのに要る版
            o.extend(0x0800u16.to_le_bytes()); // 名前は UTF-8
            o.extend(8u16.to_le_bytes()); // deflate
            o.extend(time.to_le_bytes());
            o.extend(date.to_le_bytes());
            o.extend(crc.to_le_bytes());
            o.extend((packed.len() as u32).to_le_bytes());
            o.extend((data.len() as u32).to_le_bytes());
            o.extend((name.len() as u16).to_le_bytes());
            o.extend(0u16.to_le_bytes()); // 拡張の長さ
        };
        out.extend(0x0403_4b50u32.to_le_bytes());
        common(&mut out);
        out.extend(name.as_bytes());
        out.extend(&packed);
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend(20u16.to_le_bytes()); // 作った版
        common(&mut central);
        central.extend(0u16.to_le_bytes()); // コメントの長さ
        central.extend(0u16.to_le_bytes()); // ディスク番号
        central.extend(0u16.to_le_bytes()); // 内部の属性
        central.extend(0u32.to_le_bytes()); // 外部の属性
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let cd = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]); // ディスク番号
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(cd.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

/// [`write`] が書いた zip から、名前 `part` の中身を取り出す(中央の目録は見ず、頭から順に
/// 読むので、ほかのソフトが書いた zip は読めないことがある)。
pub fn read_written(b: &[u8], part: &str) -> Option<Vec<u8>> {
    let u16_at = |i: usize| Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize);
    let u32_at = |i: usize| Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize);
    let mut i = 0;
    while u32_at(i)? == 0x0403_4b50 {
        let (size, n, x) = (u32_at(i + 18)?, u16_at(i + 26)?, u16_at(i + 28)?);
        let name = std::str::from_utf8(b.get(i + 30..i + 30 + n)?).ok()?;
        let data = b.get(i + 30 + n + x..i + 30 + n + x + size)?;
        if name == part {
            return miniz_oxide::inflate::decompress_to_vec(data).ok();
        }
        i += 30 + n + x + size;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_files_read_back_by_name() {
        let z = write(&[("a.txt", b"hello"), ("dir/b.json", b"{\"x\":1}")]);
        assert_eq!(&z[..4], b"PK\x03\x04");
        assert_eq!(read_written(&z, "a.txt").as_deref(), Some(&b"hello"[..]));
        assert_eq!(
            read_written(&z, "dir/b.json").as_deref(),
            Some(&b"{\"x\":1}"[..])
        );
        assert_eq!(read_written(&z, "c"), None);
        // 終わりの目録に 2 つと書く
        let end = z.len() - 22;
        assert_eq!(&z[end..end + 4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([z[end + 10], z[end + 11]]), 2);
    }
}
