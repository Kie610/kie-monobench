//! 基準のファイル(`tests/golden/`)との比べ方。
//!
//! 行ごとに、数でない部分と整数どうしは同じ文字、ほかの数は 0.0015 までの違いを許して比べる。
//! G-code の座標は小数 3 桁(末尾の 0 は省く)なので、丸めが 1 つずれるのを許す
//! (`X12.345` と `X12.346`、`X68` と `X67.999` は同じ。`G0` と `G1`、`T1` と `T2` は違う)。
//! 行の数や数でない部分が違えば、最初に違う行を示して止まる。
//! `JC_BLESS=1` のときは比べずに基準を書き直す。

use std::path::PathBuf;

/// 小数の数の違いを許す幅。
const TOL: f64 = 0.0015;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// `actual` を基準の `name` と比べる。
pub fn check(name: &str, actual: &str) {
    let path = dir().join(name);
    if std::env::var_os("JC_BLESS").is_some() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(&path, actual).unwrap();
        println!("基準を書き直した: {}", path.display());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "基準 {} を読めません({e})。JC_BLESS=1 で作る",
            path.display()
        )
    });
    // Windows で改行が CRLF に変わっていても同じとみなす
    let expected = expected.replace("\r\n", "\n");
    if let Err(msg) = compare(&expected, actual) {
        let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.actual"));
        std::fs::write(&out, actual).ok();
        panic!(
            "基準 {name} と違う: {msg}\n今回の結果: {}\n意図した変更なら JC_BLESS=1 で基準を書き直し、差分を確かめてコミットする",
            out.display()
        );
    }
}

fn compare(expected: &str, actual: &str) -> Result<(), String> {
    let (e, a): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    for (i, (le, la)) in e.iter().zip(&a).enumerate() {
        if !same_line(le, la) {
            return Err(format!("{} 行目\n  基準: {le}\n  今回: {la}", i + 1));
        }
    }
    if e.len() != a.len() {
        return Err(format!("行の数 基準 {} 行、今回 {} 行", e.len(), a.len()));
    }
    Ok(())
}

/// 文字と数の並びに分ける。数は (書いたまま, 値, 小数点があるか)。
#[derive(Debug, PartialEq)]
enum Piece<'a> {
    Text(&'a str),
    Num(&'a str, f64, bool),
}

fn pieces(s: &str) -> Vec<Piece<'_>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut text) = (0, 0);
    while i < b.len() {
        let starts_num = b[i].is_ascii_digit()
            || (matches!(b[i], b'-' | b'.')
                && b.get(i + 1)
                    .is_some_and(|c| c.is_ascii_digit() || *c == b'.'));
        if !starts_num {
            i += 1;
            continue;
        }
        let start = i;
        if b[i] == b'-' {
            i += 1;
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let mut frac = false;
        if i < b.len() && b[i] == b'.' {
            i += 1;
            frac = true;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        // `-` や `.` だけなど、数でなければ文字のまま
        if let Ok(v) = s[start..i].parse::<f64>() {
            if text < start {
                out.push(Piece::Text(&s[text..start]));
            }
            out.push(Piece::Num(&s[start..i], v, frac));
            text = i;
        }
    }
    if text < b.len() {
        out.push(Piece::Text(&s[text..]));
    }
    out
}

fn same_line(e: &str, a: &str) -> bool {
    if e == a {
        return true;
    }
    let (pe, pa) = (pieces(e), pieces(a));
    pe.len() == pa.len()
        && pe.iter().zip(&pa).all(|(x, y)| match (x, y) {
            (Piece::Text(x), Piece::Text(y)) => x == y,
            // 整数(G・M・T の番号など)はそのまま
            (Piece::Num(x, _, false), Piece::Num(y, _, false)) => x == y,
            // 小数を含めば、小数 3 桁の丸めが 1 つずれるのを許す
            (Piece::Num(_, x, _), Piece::Num(_, y, _)) => (x - y).abs() <= TOL,
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rounding_flip_in_the_last_digit_is_the_same() {
        assert!(same_line(
            "G1 X12.345 Y-3.2 F600",
            "G1 X12.346 Y-3.201 F600"
        ));
        assert!(same_line("X68 F900", "X67.999 F900"));
        assert!(same_line("X-0.001", "X0"));
        assert!(same_line("X12.3", "X12.301"));
    }

    #[test]
    fn a_bigger_change_or_other_words_differ() {
        assert!(!same_line("G1 X12.345", "G1 X12.347"));
        assert!(!same_line("G1 X12.345", "G0 X12.345"));
        assert!(!same_line("M6 T1", "M6 T2"));
        assert!(!same_line("F600", "F601"));
        assert!(!same_line("X12.3", "X12.302"));
        assert!(!same_line("X68", "X68.01"));
        assert!(!same_line("(時間の目安 1.2 分)", "(時間の目安 1.3 分)"));
        assert!(!same_line("G1 X12.345", "G1 Y12.345"));
        assert!(!same_line("G1 X12.345", "G1 X12.345 Y1"));
        assert!(compare("a\nb\n", "a\n").is_err());
        assert!(compare("a\n", "a\nb\n").is_err());
        assert!(compare("X1.000\n", "X1.001\n").is_ok());
    }
}
