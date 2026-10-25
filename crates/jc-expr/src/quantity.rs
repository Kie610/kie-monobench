use serde::{Deserialize, Serialize};
use std::fmt;

/// 物理的な次元。長さと角度の指数で表す(面積なら length = 2)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Dim {
    pub length: i8,
    pub angle: i8,
}

impl Dim {
    pub const NONE: Dim = Dim {
        length: 0,
        angle: 0,
    };
    pub const LENGTH: Dim = Dim {
        length: 1,
        angle: 0,
    };
    pub const ANGLE: Dim = Dim {
        length: 0,
        angle: 1,
    };

    pub fn is_none(self) -> bool {
        self == Dim::NONE
    }
}

impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        match self.length {
            0 => {}
            1 => parts.push("mm".to_string()),
            n => parts.push(format!("mm^{n}")),
        }
        match self.angle {
            0 => {}
            1 => parts.push("rad".to_string()),
            n => parts.push(format!("rad^{n}")),
        }
        if parts.is_empty() {
            write!(f, "無次元")
        } else {
            write!(f, "{}", parts.join("·"))
        }
    }
}

/// 単位つきの値。`value` は内部単位(mm、rad)での値。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quantity {
    pub value: f64,
    pub dim: Dim,
}

impl Quantity {
    pub fn new(value: f64, dim: Dim) -> Self {
        Quantity { value, dim }
    }
    pub fn scalar(value: f64) -> Self {
        Quantity::new(value, Dim::NONE)
    }
    pub fn mm(value: f64) -> Self {
        Quantity::new(value, Dim::LENGTH)
    }
    pub fn deg(value: f64) -> Self {
        Quantity::new(value.to_radians(), Dim::ANGLE)
    }

    /// 形に使えないほど大きいか数でない時の理由(角度は [`MAX_ANGLE_DEG`]、ほかは長さとして
    /// [`MAX_LENGTH_MM`] まで)。
    pub fn size_problem(&self) -> Option<String> {
        if !self.value.is_finite() {
            return Some("数になっていません".into());
        }
        let (v, max, unit) = if self.dim == Dim::ANGLE {
            (self.value.to_degrees(), MAX_ANGLE_DEG, "deg")
        } else {
            (self.value, MAX_LENGTH_MM, "mm")
        };
        (v.abs() > max).then(|| format!("大きすぎます({} {unit} まで)", trim(max)))
    }

    /// 足し引きの相手 `other` が長さか角度で自分が無次元なら、自分を既定の単位(mm・deg)の
    /// 値として読む(`30 deg + 15` は 45 度)。
    /// ほかはそのまま。
    pub(crate) fn like(self, other: Quantity) -> Quantity {
        if !self.dim.is_none() {
            self
        } else if other.dim == Dim::LENGTH {
            Quantity::mm(self.value)
        } else if other.dim == Dim::ANGLE {
            Quantity::deg(self.value)
        } else {
            self
        }
    }
}

/// 形に使う長さの上限(mm。1 km)。壊れたファイルや AI の打ち込みで来る桁違いの値を、カーネルへ渡す前に
/// その機能の誤りとして止める(採用分 10-24 k12)。
pub const MAX_LENGTH_MM: f64 = 1.0e6;
/// 形に使う角度の上限(deg。ねじれの多い巻きでも足りる)。
pub const MAX_ANGLE_DEG: f64 = 1.0e6;

impl fmt::Display for Quantity {
    /// 表示用。角度は deg で表示する。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.dim == Dim::ANGLE {
            write!(f, "{} deg", trim(self.value.to_degrees()))
        } else if self.dim.is_none() {
            write!(f, "{}", trim(self.value))
        } else {
            write!(f, "{} {}", trim(self.value), self.dim)
        }
    }
}

fn trim(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}
