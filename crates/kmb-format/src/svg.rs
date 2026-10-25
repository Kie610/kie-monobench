//! SVG を書く時の小さな部品。

/// 数を小数 4 桁までで短く書く(`-0` と空は `0`)。
pub fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

/// 本文と属性に入れる文字の置き換え(`&`・`<`・`>`・`"`)。
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// XML の宣言と、紙の大きさを mm で決めた `svg` の開きタグ(`viewBox` は 0 0 幅 高さ、1 単位 = 1 mm)
/// の 2 行。px で書くと読む側の dpi の決め方(96 か 72 か)で大きさが変わるため mm で書く。
pub fn begin_mm(w: f64, h: f64) -> String {
    let (w, h) = (num(w), num(h));
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short_and_text_is_escaped() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(2.50004), "2.5");
        assert_eq!(num(-3.125), "-3.125");
        assert_eq!(esc("a<b & \"c\""), "a&lt;b &amp; &quot;c&quot;");
        assert!(begin_mm(10.0, 5.0).contains("width=\"10mm\" height=\"5mm\" viewBox=\"0 0 10 5\""));
    }
}
