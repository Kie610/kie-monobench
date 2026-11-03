//! 圧縮・フォント・時計を使わない、用紙の mm 座標の PDF の書き手。

use std::fmt::Write as _;

const PT: f64 = 72.0 / 25.4;

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Move([f64; 2]),
    Line([f64; 2]),
    Cubic([[f64; 2]; 3]),
    Close,
    /// 長さは mm。空の破線の並びは実線。
    Stroke {
        width: f64,
        dash: Vec<f64>,
    },
    FillEvenOdd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub size_mm: [f64; 2],
    pub ops: Vec<Op>,
}

fn number(v: f64) -> String {
    let s = format!("{:.6}", v * PT);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn content(page: &Page) -> Result<String, String> {
    let mut out = String::from("0 G 0 g 1 J 1 j\n");
    let mut active = false;
    let point = |p: [f64; 2]| -> Result<String, String> {
        if !p.iter().all(|x| x.is_finite()) {
            return Err("PDF の座標は有限の数にしてください".into());
        }
        Ok(format!("{} {}", number(p[0]), number(p[1])))
    };
    for op in &page.ops {
        match op {
            Op::Move(p) => {
                writeln!(out, "{} m", point(*p)?).unwrap();
                active = true;
            }
            Op::Line(p) => {
                if !active {
                    return Err("PDF の線の始点がありません".into());
                }
                writeln!(out, "{} l", point(*p)?).unwrap();
            }
            Op::Cubic(p) => {
                if !active {
                    return Err("PDF の曲線の始点がありません".into());
                }
                writeln!(out, "{} {} {} c", point(p[0])?, point(p[1])?, point(p[2])?).unwrap();
            }
            Op::Close => {
                if !active {
                    return Err("PDF の閉じる輪がありません".into());
                }
                out.push_str("h\n");
            }
            Op::Stroke { width, dash } => {
                if !width.is_finite()
                    || *width <= 0.0
                    || dash.iter().any(|x| !x.is_finite() || *x < 0.0)
                    || !dash.is_empty() && dash.iter().all(|x| *x == 0.0)
                {
                    return Err("PDF の線の太さと破線の長さを確認してください".into());
                }
                if !active {
                    return Err("PDF の描く線がありません".into());
                }
                let dash = dash
                    .iter()
                    .map(|x| number(*x))
                    .collect::<Vec<_>>()
                    .join(" ");
                writeln!(out, "{} w [{}] 0 d S", number(*width), dash).unwrap();
                active = false;
            }
            Op::FillEvenOdd => {
                if !active {
                    return Err("PDF の塗る輪がありません".into());
                }
                out.push_str("f*\n");
                active = false;
            }
        }
    }
    if active {
        return Err("PDF の線を描くか塗ってから終えてください".into());
    }
    Ok(out)
}

/// YYYY-MM-DD を受け、時計を読まずに PDF の作成日時にする。
fn date_value(date: &str) -> Result<String, String> {
    let bytes = date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return Err("日付は YYYY-MM-DD で指定してください".into());
    }
    let year: u32 = date[..4].parse().map_err(|_| "年を読めません")?;
    let month: usize = date[5..7].parse().map_err(|_| "月を読めません")?;
    let day: u32 = date[8..].parse().map_err(|_| "日を読めません")?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if year == 0 || !(1..=12).contains(&month) || day == 0 || day > days[month - 1] {
        return Err("日付に存在する年月日を指定してください".into());
    }
    Ok(format!("D:{}000000Z", date.replace('-', "")))
}

/// 書き出しの日付を、PDF と表題欄で共通に確かめる。
pub fn validate_date(date: &str) -> Result<(), String> {
    date_value(date).map(|_| ())
}

/// 用紙ごとに一ページ。作成日は指定された時だけ入れ、同じ入力は同じバイトになる。
pub fn write(pages: &[Page], date: Option<&str>) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("PDF に書く用紙がありません".into());
    }
    let date = date.map(date_value).transpose()?;
    let kids = (0..pages.len())
        .map(|i| format!("{} 0 R", 3 + 2 * i))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Count {} /Kids [{}] >>", pages.len(), kids),
    ];
    for (i, page) in pages.iter().enumerate() {
        if !page.size_mm.iter().all(|x| x.is_finite() && *x > 0.0) {
            return Err("PDF の用紙の大きさは正の有限の数にしてください".into());
        }
        let body = content(page)?;
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << >> /Contents {} 0 R >>",number(page.size_mm[0]),number(page.size_mm[1]),4+2*i));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}endstream",
            body.len(),
            body
        ));
    }
    let mut info = String::new();
    if let Some(date) = date {
        objects.push(format!("<< /CreationDate ({date}) >>"));
        info = format!(" /Info {} 0 R", objects.len());
    }
    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut positions = vec![0];
    for (i, object) in objects.iter().enumerate() {
        positions.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, object).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", positions.len()).as_bytes());
    for position in positions.iter().skip(1) {
        out.extend_from_slice(format!("{position:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R{} >>\nstartxref\n{}\n%%EOF\n",
            positions.len(),
            info,
            xref
        )
        .as_bytes(),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_have_exact_byte_offsets_sizes_and_optional_explicit_date() {
        let pages = [
            Page {
                size_mm: [210.0, 297.0],
                ops: vec![
                    Op::Move([0.0, 0.0]),
                    Op::Line([25.4, 0.0]),
                    Op::Stroke {
                        width: 0.5,
                        dash: vec![6.0, 1.5],
                    },
                ],
            },
            Page {
                size_mm: [420.0, 297.0],
                ops: vec![
                    Op::Move([0.0, 0.0]),
                    Op::Line([1.0, 0.0]),
                    Op::Line([0.0, 1.0]),
                    Op::Close,
                    Op::FillEvenOdd,
                ],
            },
        ];
        let bytes = write(&pages, None).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let xref = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(&bytes[xref..xref + 4], b"xref");
        let entries = std::str::from_utf8(&bytes[xref..])
            .unwrap()
            .lines()
            .skip(3)
            .take(6)
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 6);
        for (i, line) in entries.iter().enumerate() {
            let offset = line[..10].parse::<usize>().unwrap();
            assert!(bytes[offset..].starts_with(format!("{} 0 obj\n", i + 1).as_bytes()));
        }
        assert!(text.contains("/Count 2"));
        assert!(text.contains("/MediaBox [0 0 595.275591 841.889764]"));
        assert!(text.contains("/MediaBox [0 0 1190.551181 841.889764]"));
        assert!(text.contains("72 0 l"));
        assert!(text.contains("f*"));
        assert!(!text.contains("CreationDate"));
        assert!(!text.contains("/Font"));
        assert_eq!(bytes, write(&pages, None).unwrap());
        let dated = write(&pages, Some("2024-02-29")).unwrap();
        assert!(String::from_utf8_lossy(&dated).contains("/CreationDate (D:20240229000000Z)"));
        assert!(write(&pages, Some("2025-02-29")).is_err());
        assert!(write(&pages, Some("2026-10-30\n")).is_err());
    }
}
