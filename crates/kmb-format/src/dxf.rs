//! DXF R12(AC1009)の書き手。R12 はハンドルも CLASSES も要らず、ほとんどのソフト(LightBurn など)が読める最小の形(HEADER・TABLES・ENTITIES)で書く。単位は mm。

use std::fmt::Write as _;

/// 書いている DXF。数の書き方 `num` は呼ぶ側が決める(スケッチは往復で値が変わらない形、
/// 投影した線は小数 6 桁など)。
pub struct R12 {
    out: String,
    num: fn(f64) -> String,
}

/// 線種 CONTINUOUS(実線)と DASHED(破線)。
pub const CONTINUOUS: &str = "CONTINUOUS";
pub const DASHED: &str = "DASHED";

impl R12 {
    /// HEADER(版・単位 mm・図の囲み `lo`〜`hi`)と、TABLES の線種(実線と破線)と画層
    /// `layers`(名前・色の番号・線種)を書き、ENTITIES を始める。
    pub fn begin(
        num: fn(f64) -> String,
        lo: [f64; 2],
        hi: [f64; 2],
        layers: &[(&str, i32, &str)],
    ) -> R12 {
        let mut o = R12 {
            out: String::new(),
            num,
        };
        // HEADER
        o.pair(0, "SECTION");
        o.pair(2, "HEADER");
        o.pair(9, "$ACADVER");
        o.pair(1, "AC1009");
        o.pair(9, "$INSUNITS");
        o.pair(70, 4);
        o.pair(9, "$MEASUREMENT");
        o.pair(70, 1);
        o.pair(9, "$EXTMIN");
        o.xy(10, lo);
        o.pair(9, "$EXTMAX");
        o.xy(10, hi);
        o.pair(0, "ENDSEC");
        // TABLES(線種と画層)
        o.pair(0, "SECTION");
        o.pair(2, "TABLES");
        o.pair(0, "TABLE");
        o.pair(2, "LTYPE");
        o.pair(70, 2);
        o.pair(0, "LTYPE");
        o.pair(2, CONTINUOUS);
        o.pair(70, 0);
        o.pair(3, "Solid line");
        o.pair(72, 65);
        o.pair(73, 0);
        o.pair(40, "0.0");
        o.pair(0, "LTYPE");
        o.pair(2, DASHED);
        o.pair(70, 0);
        o.pair(3, "__ __ __");
        o.pair(72, 65);
        o.pair(73, 2);
        o.pair(40, "3.0");
        o.pair(49, "2.0");
        o.pair(49, "-1.0");
        o.pair(0, "ENDTAB");
        o.pair(0, "TABLE");
        o.pair(2, "LAYER");
        o.pair(70, layers.len());
        for (name, color, ltype) in layers {
            o.pair(0, "LAYER");
            o.pair(2, name);
            o.pair(70, 0);
            o.pair(62, color);
            o.pair(6, ltype);
        }
        o.pair(0, "ENDTAB");
        o.pair(0, "ENDSEC");
        // ENTITIES
        o.pair(0, "SECTION");
        o.pair(2, "ENTITIES");
        o
    }

    /// 組 1 つ(組の番号を右に寄せた 3 文字と値)。
    pub fn pair(&mut self, code: i32, value: impl std::fmt::Display) {
        let _ = write!(self.out, "{code:>3}\n{value}\n");
    }

    /// 点(`code` が x、`code + 10` が y、`code + 20` が z = 0)。
    pub fn xy(&mut self, code: i32, p: [f64; 2]) {
        let num = self.num;
        self.pair(code, num(p[0]));
        self.pair(code + 10, num(p[1]));
        self.pair(code + 20, "0.0");
    }

    pub fn line(&mut self, layer: &str, a: [f64; 2], b: [f64; 2]) {
        self.pair(0, "LINE");
        self.pair(8, layer);
        self.xy(10, a);
        self.xy(11, b);
    }

    pub fn circle(&mut self, layer: &str, center: [f64; 2], radius: f64) {
        let num = self.num;
        self.pair(0, "CIRCLE");
        self.pair(8, layer);
        self.xy(10, center);
        self.pair(40, num(radius));
    }

    /// 円弧(反時計回りに `start` から `end` まで。角はラジアンで渡し、度の 0 以上 360 未満で書く)。
    pub fn arc(&mut self, layer: &str, center: [f64; 2], radius: f64, start: f64, end: f64) {
        let num = self.num;
        self.pair(0, "ARC");
        self.pair(8, layer);
        self.xy(10, center);
        self.pair(40, num(radius));
        self.pair(50, num(start.to_degrees().rem_euclid(360.0)));
        self.pair(51, num(end.to_degrees().rem_euclid(360.0)));
    }

    pub fn point(&mut self, layer: &str, p: [f64; 2]) {
        self.pair(0, "POINT");
        self.pair(8, layer);
        self.xy(10, p);
    }

    /// 開いた折れ線(POLYLINE と VERTEX と SEQEND)。
    pub fn polyline(&mut self, layer: &str, pts: &[[f64; 2]]) {
        self.pair(0, "POLYLINE");
        self.pair(8, layer);
        self.pair(66, 1);
        self.xy(10, [0.0, 0.0]);
        self.pair(70, 0);
        for p in pts {
            self.pair(0, "VERTEX");
            self.pair(8, layer);
            self.xy(10, *p);
        }
        self.pair(0, "SEQEND");
        self.pair(8, layer);
    }

    /// ENTITIES を閉じ、ファイルの終わりを書いて文字列を返す。
    pub fn finish(mut self) -> String {
        self.pair(0, "ENDSEC");
        self.pair(0, "EOF");
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(v: f64) -> String {
        format!("{v}")
    }

    #[test]
    fn the_sections_and_entities_come_in_order() {
        let mut d = R12::begin(n, [0.0, 0.0], [10.0, 5.0], &[("0", 7, CONTINUOUS)]);
        d.line("0", [0.0, 0.0], [10.0, 5.0]);
        d.arc("0", [1.0, 1.0], 2.0, -std::f64::consts::FRAC_PI_2, 0.0);
        let s = d.finish();
        let at = |w: &str| s.find(w).unwrap_or_else(|| panic!("{w} が無い"));
        assert!(at("HEADER") < at("TABLES") && at("TABLES") < at("ENTITIES"));
        assert!(at("AC1009") < at("$EXTMAX") && at("LINE") < at("ARC"));
        assert!(s.contains(" 50\n270\n 51\n0\n"), "{s}");
        assert!(s.ends_with("  0\nENDSEC\n  0\nEOF\n"));
    }
}
