//! 長さの表示の単位(候補 19)。計算と保存はいつも mm で、画面に出す・打つ時だけ換算する。
//!
//! 文書ごとに持つ単位(設計の属性の `units`)を、画面が今のタブの文書から [`set_display_unit`] で
//! 入れる。寸法の札・式の欄の値・パラメータの表・測る道具はここから読む。スレッドごとに持つ
//! (画面は 1 本のスレッドで描く。試験が並んで走っても互いに混ざらない)。画面の無い CLI・MCP は
//! 入れないので mm のまま。

use crate::{Dim, Quantity};
use serde::{Deserialize, Serialize};
use std::cell::Cell;

/// 長さの単位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LengthUnit {
    #[default]
    Mm,
    Cm,
    M,
    In,
    Ft,
}

impl LengthUnit {
    pub const ALL: [LengthUnit; 5] = [
        LengthUnit::Mm,
        LengthUnit::Cm,
        LengthUnit::M,
        LengthUnit::In,
        LengthUnit::Ft,
    ];

    /// 式に書く名前(`mm`・`cm`・`m`・`in`・`ft`)。
    pub fn name(self) -> &'static str {
        match self {
            LengthUnit::Mm => "mm",
            LengthUnit::Cm => "cm",
            LengthUnit::M => "m",
            LengthUnit::In => "in",
            LengthUnit::Ft => "ft",
        }
    }

    /// 選ぶ一覧に出す名前。
    pub fn label(self) -> &'static str {
        match self {
            LengthUnit::Mm => "ミリメートル(mm)",
            LengthUnit::Cm => "センチメートル(cm)",
            LengthUnit::M => "メートル(m)",
            LengthUnit::In => "インチ(in)",
            LengthUnit::Ft => "フィート(ft)",
        }
    }

    /// 1 単位が何 mm か。
    pub fn mm(self) -> f64 {
        match self {
            LengthUnit::Mm => 1.0,
            LengthUnit::Cm => 10.0,
            LengthUnit::M => 1000.0,
            LengthUnit::In => 25.4,
            LengthUnit::Ft => 304.8,
        }
    }

    /// 名前から(`inch`・`foot` も受ける。大小は区別しない)。
    pub fn parse(s: &str) -> Option<LengthUnit> {
        match s.trim().to_lowercase().as_str() {
            "mm" => Some(LengthUnit::Mm),
            "cm" => Some(LengthUnit::Cm),
            "m" => Some(LengthUnit::M),
            "in" | "inch" | "inches" => Some(LengthUnit::In),
            "ft" | "foot" | "feet" => Some(LengthUnit::Ft),
            _ => None,
        }
    }

    /// 画面に出す桁数(mm・cm・m は 2、in は 3、ft は 4。どれも 0.03 mm ほどまで読める)。
    pub fn digits(self) -> usize {
        match self {
            LengthUnit::In => 3,
            LengthUnit::Ft => 4,
            _ => 2,
        }
    }

    pub fn is_mm(&self) -> bool {
        *self == LengthUnit::Mm
    }

    /// mm の値をこの単位の値に。
    pub fn from_mm(self, mm: f64) -> f64 {
        mm / self.mm()
    }

    /// 長さ(mm)を `digits` 桁で、単位の名前を付けて書く(例 `0.500 in`。-0 は 0 にする)。
    pub fn length(self, mm: f64, digits: usize) -> String {
        format!("{} {}", fixed(self.from_mm(mm), digits), self.name())
    }

    /// 長さの n 乗(面積は 2、体積は 3)を `digits` 桁で書く(例 `12.000 mm²`)。
    pub fn power(self, value: f64, n: i32, digits: usize) -> String {
        let sup = match n {
            2 => "²",
            3 => "³",
            _ => "",
        };
        format!(
            "{} {}{sup}",
            fixed(value / self.mm().powi(n), digits),
            self.name()
        )
    }

    /// 量を、長さはこの単位で書く(式の欄の「= 値」・パラメータの表の値。末尾の 0 は落とす)。
    /// 長さを含まない量(角度・無次元)と、長さと角度を掛けた量は [`Quantity`] の表示のまま。
    pub fn show(self, q: Quantity) -> String {
        if self.is_mm() || q.dim.length == 0 || q.dim.angle != 0 {
            return q.to_string();
        }
        let n = i32::from(q.dim.length);
        let v = trim(q.value / self.mm().powi(n));
        if q.dim == Dim::LENGTH {
            format!("{v} {}", self.name())
        } else {
            format!("{v} {}^{n}", self.name())
        }
    }
}

thread_local! {
    static DISPLAY: Cell<LengthUnit> = const { Cell::new(LengthUnit::Mm) };
}

/// 画面に出す長さの単位(今の文書の単位。入れていなければ mm)。
pub fn display_unit() -> LengthUnit {
    DISPLAY.with(Cell::get)
}

/// 画面に出す長さの単位を入れる(画面がタブの文書の単位を入れる)。
pub fn set_display_unit(u: LengthUnit) {
    DISPLAY.with(|c| c.set(u));
}

/// 決まった桁数で書く(-0.00 は 0.00 にする)。
fn fixed(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    match s.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => s,
    }
}

/// 小数 6 桁で丸め、末尾の 0 を落とす([`Quantity`] の表示と同じ)。
fn trim(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_are_converted_only_for_showing() {
        let u = LengthUnit::In;
        assert_eq!(u.length(12.7, 3), "0.500 in");
        assert_eq!(u.length(-0.0001, 2), "0.00 in");
        assert_eq!(LengthUnit::Cm.length(15.0, 2), "1.50 cm");
        assert_eq!(u.power(645.16, 2, 3), "1.000 in²");
        assert_eq!(LengthUnit::Mm.power(8.0, 3, 1), "8.0 mm³");
        assert_eq!(u.show(Quantity::mm(25.4)), "1 in");
        assert_eq!(
            u.show(Quantity::new(
                645.16,
                Dim {
                    length: 2,
                    angle: 0
                }
            )),
            "1 in^2"
        );
        // 角度と無次元は変えない
        assert_eq!(u.show(Quantity::deg(30.0)), "30 deg");
        assert_eq!(u.show(Quantity::scalar(3.0)), "3");
        assert_eq!(LengthUnit::Mm.show(Quantity::mm(12.5)), "12.5 mm");
    }

    #[test]
    fn names_round_trip_and_the_display_unit_is_per_thread() {
        for u in LengthUnit::ALL {
            assert_eq!(LengthUnit::parse(u.name()), Some(u));
        }
        assert_eq!(LengthUnit::parse("Inch"), Some(LengthUnit::In));
        assert_eq!(LengthUnit::parse("Feet"), Some(LengthUnit::Ft));
        assert_eq!(LengthUnit::Ft.length(304.8, 4), "1.0000 ft");
        assert_eq!(LengthUnit::Ft.show(Quantity::mm(457.2)), "1.5 ft");
        assert_eq!(LengthUnit::parse("yd"), None);
        assert_eq!(display_unit(), LengthUnit::Mm);
        set_display_unit(LengthUnit::In);
        assert_eq!(display_unit(), LengthUnit::In);
        std::thread::spawn(|| assert_eq!(display_unit(), LengthUnit::Mm))
            .join()
            .unwrap();
        set_display_unit(LengthUnit::Mm);
    }
}
