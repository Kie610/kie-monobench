//! zip の書き手と、中央の目録から読む読み手。

/// 読み書きの両方で守る、展開後の合計の上限。
pub const UNPACKED_LIMIT: usize = 512 * 1024 * 1024;

// UTF-8の印がないzipで使うCP437の上半分。下半分はASCIIと同じ。
const CP437: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ',
    'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ',
    'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕',
    '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦',
    '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐',
    '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±',
    '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{00a0}',
];

/// 中央の目録を読み、無圧縮とdeflateのファイルを取り出す。展開先へは書かない。
/// ZIP64、暗号化、分割した書庫は受け付けない。展開した合計は512 MiBまで。
pub fn read(b: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let bad = || "zipの目録かファイルの内容が壊れています".to_string();
    let bytes = |at: usize, n: usize| b.get(at..at.checked_add(n)?);
    let u16_at = |at| -> Result<usize, String> {
        Ok(u16::from_le_bytes(bytes(at, 2).ok_or_else(bad)?.try_into().unwrap()) as usize)
    };
    let u32_at = |at| -> Result<usize, String> {
        Ok(u32::from_le_bytes(bytes(at, 4).ok_or_else(bad)?.try_into().unwrap()) as usize)
    };
    let end = (b.len().saturating_sub(65557)..b.len().saturating_sub(21))
        .rev()
        .find(|&i| {
            bytes(i, 4) == Some(b"PK\x05\x06")
                && u16_at(i + 20).ok().is_some_and(|n| i + 22 + n == b.len())
        })
        .ok_or_else(bad)?;
    if u16_at(end + 4)? != 0 || u16_at(end + 6)? != 0 || u16_at(end + 8)? != u16_at(end + 10)? {
        return Err("分割したzipは読めません".into());
    }
    let count = u16_at(end + 10)?;
    let size = u32_at(end + 12)?;
    let start = u32_at(end + 16)?;
    if count == 65535 || size == u32::MAX as usize || start == u32::MAX as usize {
        return Err("ZIP64は読めません".into());
    }
    if start.checked_add(size) != Some(end) {
        return Err(bad());
    }
    let mut at = start;
    let mut total = 0usize;
    let mut files = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for _ in 0..count {
        if bytes(at, 4) != Some(b"PK\x01\x02") {
            return Err(bad());
        }
        let flags = u16_at(at + 8)?;
        let method = u16_at(at + 10)?;
        let crc = u32_at(at + 16)? as u32;
        let packed = u32_at(at + 20)?;
        let unpacked = u32_at(at + 24)?;
        let n = u16_at(at + 28)?;
        let extra = u16_at(at + 30)?;
        let comment = u16_at(at + 32)?;
        if flags & 1 != 0 || u16_at(at + 34)? != 0 || ![0, 8].contains(&method) {
            return Err("暗号化・分割・未対応の圧縮方法のzipは読めません".into());
        }
        total = total
            .checked_add(unpacked)
            .filter(|n| *n <= UNPACKED_LIMIT)
            .ok_or("zipの展開後の合計が512 MiBを超えます")?;
        let name_bytes = bytes(at + 46, n).ok_or_else(bad)?;
        let name: String = if flags & 0x0800 != 0 {
            std::str::from_utf8(name_bytes)
                .map_err(|_| "zipのファイル名をUTF-8として読めません")?
                .to_owned()
        } else {
            name_bytes
                .iter()
                .map(|&b| {
                    if b < 128 {
                        char::from(b)
                    } else {
                        CP437[usize::from(b - 128)]
                    }
                })
                .collect()
        };
        if !names.insert(name.clone()) {
            return Err(format!("zipに同じ名前が重なっています: {name}"));
        }
        let local = u32_at(at + 42)?;
        if bytes(local, 4) != Some(b"PK\x03\x04")
            || u16_at(local + 6)? != flags
            || u16_at(local + 8)? != method
        {
            return Err(bad());
        }
        let ln = u16_at(local + 26)?;
        let lx = u16_at(local + 28)?;
        if bytes(local + 30, ln) != Some(name_bytes) {
            return Err(bad());
        }
        let data_at = local.checked_add(30 + ln + lx).ok_or_else(bad)?;
        if data_at.checked_add(packed).is_none_or(|n| n > start) {
            return Err(bad());
        }
        let data = bytes(data_at, packed).ok_or_else(bad)?;
        let data = if method == 0 {
            if packed != unpacked {
                return Err(bad());
            }
            data.to_vec()
        } else {
            miniz_oxide::inflate::decompress_to_vec_with_limit(data, unpacked).map_err(|_| bad())?
        };
        if data.len() != unpacked || crc32fast::hash(&data) != crc {
            return Err(bad());
        }
        files.push((name, data));
        at = at
            .checked_add(46 + n + extra + comment)
            .filter(|n| *n <= end)
            .ok_or_else(bad)?;
    }
    if at != end {
        return Err(bad());
    }
    Ok(files)
}

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
    fn central_directory_reads_data_descriptors_and_checks_crc() {
        let mut z = write(&[("部品.txt", b"abc")]);
        // データ記述子を使う書庫では、ローカルの長さとCRCは空でよい。
        z[6..8].copy_from_slice(&0x0808u16.to_le_bytes());
        let packed = u32::from_le_bytes(z[18..22].try_into().unwrap());
        z[14..26].fill(0);
        let cd = z.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
        let mut descriptor = Vec::new();
        for n in [0x0807_4b50u32, crc32fast::hash(b"abc"), packed, 3] {
            descriptor.extend(n.to_le_bytes());
        }
        z.splice(cd..cd, descriptor);
        let cd = cd + 16;
        let end = z.len() - 22;
        z[end + 16..end + 20].copy_from_slice(&(cd as u32).to_le_bytes());
        z[cd + 8..cd + 10].copy_from_slice(&0x0808u16.to_le_bytes());
        assert_eq!(
            read(&z).unwrap(),
            vec![("部品.txt".into(), b"abc".to_vec())]
        );
        z[cd + 16] ^= 1;
        assert!(read(&z).is_err());
    }

    #[test]
    fn stored_entries_and_malformed_archives() {
        let mut z = write(&[("a", b"abc")]);
        let old_cd = z.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
        let mut tail = z.split_off(old_cd);
        z.truncate(31);
        z.extend(b"abc");
        z[8..10].copy_from_slice(&0u16.to_le_bytes());
        z[18..22].copy_from_slice(&3u32.to_le_bytes());
        tail[10..12].copy_from_slice(&0u16.to_le_bytes());
        tail[20..24].copy_from_slice(&3u32.to_le_bytes());
        let end = tail.len() - 22;
        tail[end + 16..end + 20].copy_from_slice(&(z.len() as u32).to_le_bytes());
        z.extend(tail);
        assert_eq!(read(&z).unwrap()[0].1, b"abc");
        for cut in [0, 3, z.len() - 1] {
            assert!(read(&z[..cut]).is_err());
        }
        assert!(read(&write(&[("a", b"a"), ("a", b"b")])).is_err());
        assert_eq!(
            read(&write(&[("empty", b"")])).unwrap(),
            vec![("empty".into(), Vec::new())]
        );
    }

    #[test]
    fn a_zip_without_the_utf8_flag_uses_the_standard_character_set() {
        let mut z = write(&[("a.txt", b"text")]);
        let cd = z.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
        z[6..8].copy_from_slice(&0u16.to_le_bytes());
        z[cd + 8..cd + 10].copy_from_slice(&0u16.to_le_bytes());
        z[30] = 0x82;
        z[cd + 46] = 0x82;
        assert_eq!(read(&z).unwrap()[0].0, "é.txt");
    }

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
