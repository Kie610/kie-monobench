//! バイナリ DXF の値をグループコードの組に直し、共通の読み手へ渡す。

use kmb_sketch::Imported;

const SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\0";

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let end = self.at.checked_add(N).ok_or("DXF の位置が大きすぎます")?;
        let bytes = self
            .data
            .get(self.at..end)
            .ok_or("バイナリ DXF の値が途中で切れています")?;
        self.at = end;
        Ok(bytes.try_into().expect("決まった長さ"))
    }

    fn string(&mut self) -> Result<String, String> {
        let length = self.data[self.at..]
            .iter()
            .position(|b| *b == 0)
            .ok_or("バイナリ DXF の文字列が途中で切れています")?;
        let text = std::str::from_utf8(&self.data[self.at..self.at + length])
            .map_err(|_| "DXF の文字列を UTF-8 として読めません")?
            .to_string();
        self.at += length + 1;
        Ok(text)
    }

    fn value(&mut self, code: i32) -> Result<String, String> {
        Ok(match code {
            310..=319 | 1004 => {
                let length = self.take::<1>()?[0] as usize;
                let mut text = String::with_capacity(2 * length);
                use std::fmt::Write;
                for _ in 0..length {
                    let _ = write!(text, "{:02X}", self.take::<1>()?[0]);
                }
                text
            }
            0..=9
            | 100
            | 102
            | 105
            | 300..=309
            | 320..=369
            | 390..=399
            | 410..=419
            | 430..=439
            | 470..=481
            | 999
            | 1000..=1009 => self.string()?,
            10..=59 | 110..=149 | 210..=239 | 460..=469 | 1010..=1059 => {
                f64::from_le_bytes(self.take()?).to_string()
            }
            60..=79 | 170..=179 | 270..=289 | 370..=389 | 400..=409 | 1060..=1070 => {
                i16::from_le_bytes(self.take()?).to_string()
            }
            90..=99 | 420..=429 | 440..=459 | 1071 => i32::from_le_bytes(self.take()?).to_string(),
            160..=169 => i64::from_le_bytes(self.take()?).to_string(),
            290..=299 => {
                let value = self.take::<1>()?[0];
                if value > 1 {
                    return Err("バイナリ DXF の真偽値は 0 か 1 にしてください".into());
                }
                value.to_string()
            }
            _ => {
                return Err(format!(
                    "バイナリ DXF のグループコード {code} の型を読めません"
                ));
            }
        })
    }
}

/// ASCII とバイナリの両方を読む。バイナリのグループコードは R13 以降の2バイトと R12 の1バイトに対応する。
pub fn from_dxf_bytes(bytes: &[u8]) -> Result<Imported, String> {
    from_dxf_bytes_with_layers(bytes, None).map(|read| read.imported)
}

/// ASCII とバイナリで同じ画層の選びを使う。
pub fn from_dxf_bytes_with_layers(
    bytes: &[u8],
    selected: Option<&[String]>,
) -> Result<crate::DxfRead, String> {
    if !bytes.starts_with(b"AutoCAD Binary DXF") {
        return crate::from_dxf_with_layers(
            std::str::from_utf8(bytes).map_err(|_| "DXF の文字を UTF-8 として読めません")?,
            selected,
        );
    }
    let data = bytes
        .strip_prefix(SENTINEL)
        .ok_or("バイナリ DXF の先頭が途中で切れているか正しくありません")?;
    let modern = data.starts_with(&[0, 0]);
    let mut reader = Reader { data, at: 0 };
    let mut pairs = Vec::new();
    let mut ended = false;
    while reader.at < data.len() {
        let code = if modern {
            i32::from(u16::from_le_bytes(reader.take()?))
        } else {
            let first = reader.take::<1>()?[0];
            if first == 255 {
                i32::from(u16::from_le_bytes(reader.take()?))
            } else {
                i32::from(first)
            }
        };
        let value = reader.value(code)?;
        let eof = code == 0 && value == "EOF";
        if code != 999 {
            pairs.push((code, value));
        }
        if eof {
            ended = true;
            break;
        }
    }
    if !ended {
        return Err("バイナリ DXF の終わりが途中で切れています".into());
    }
    let pairs: Vec<_> = pairs
        .iter()
        .map(|(code, value)| (*code, value.trim()))
        .collect();
    crate::dxf::from_pairs(&pairs, selected)
}
