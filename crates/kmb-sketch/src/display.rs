//! 寸法の値の見せ方(小数の桁数と単位)。PC 全体の設定で、文書には入らない。
//! 形の計算は桁を丸めない(見せる時だけ丸める)。設定の窓の「スケッチの寸法の表示」で変える。

use crate::model::ConstraintKind;
use serde_json::json;
use std::sync::RwLock;

/// 設定の名前(データフォルダの設定ファイルの鍵)。
pub const DISPLAY_SETTINGS: &str = "sketch.display";

/// 寸法の値の見せ方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimFormat {
    /// 長さの小数の桁数
    pub length_digits: u8,
    /// 角度の小数の桁数
    pub angle_digits: u8,
    /// 単位(mm、°)を付けるか
    pub units: bool,
}

/// 選べる小数の桁数の上限。
pub const MAX_DIGITS: u8 = 6;

impl Default for DimFormat {
    fn default() -> Self {
        DimFormat {
            length_digits: 2,
            angle_digits: 1,
            units: true,
        }
    }
}

/// スケッチの PC 全体の設定の入れ物(寸法の見せ方・面の縁を投影するか)。組み立てた物(画面の窓・
/// CLI・MCP の 1 回)ごとに 1 つ持ち、[`kmb_core::Registry::provide`] で置く(プロセス全体の static に
/// 置かない)。
pub struct SketchSettings {
    format: RwLock<DimFormat>,
    face_edges: std::sync::atomic::AtomicBool,
}

impl Default for SketchSettings {
    fn default() -> Self {
        SketchSettings {
            format: RwLock::new(DimFormat::default()),
            face_edges: std::sync::atomic::AtomicBool::new(true),
        }
    }
}

impl SketchSettings {
    /// 今の寸法の見せ方。
    pub fn dim_format(&self) -> DimFormat {
        *self.format.read().unwrap_or_else(|e| e.into_inner())
    }

    fn set_dim_format(&self, f: DimFormat) {
        *self.format.write().unwrap_or_else(|e| e.into_inner()) = f;
    }

    /// 面にスケッチを作る時、面の縁を写すか(既定は写す)。
    pub fn project_face_edges(&self) -> bool {
        self.face_edges.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// 組み立てた物の今の寸法の見せ方(このモジュールが組み立てに無ければ既定)。
pub fn dim_format(registry: &kmb_core::Registry) -> DimFormat {
    registry
        .service::<SketchSettings>()
        .map_or_else(DimFormat::default, |s| s.dim_format())
}

/// 決まった桁数で書く(-0.00 は 0.00 にする)。
fn fixed(v: f64, digits: u8) -> String {
    let s = format!("{v:.*}", digits as usize);
    match s.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => s,
    }
}

impl DimFormat {
    /// 長さ(mm)を文書の単位で(候補 19)。例 `15.00 mm`、インチの文書は `0.59 in`。
    pub fn length(&self, mm: f64) -> String {
        let u = kmb_core::display_unit();
        let s = fixed(u.from_mm(mm), self.length_digits);
        if self.units {
            format!("{s} {}", u.name())
        } else {
            s
        }
    }

    /// 角度(rad を度で)。例 `30.0°`。
    pub fn angle(&self, rad: f64) -> String {
        let s = fixed(rad.to_degrees(), self.angle_digits);
        if self.units { format!("{s}°") } else { s }
    }

    /// 寸法の種類に合わせて(長さは mm、角度は rad の値)。
    pub fn value(&self, kind: &ConstraintKind, v: f64) -> String {
        if kind.is_angle() {
            self.angle(v)
        } else {
            self.length(v)
        }
    }

    fn to_json(self) -> serde_json::Value {
        json!({
            "length_digits": self.length_digits.to_string(),
            "angle_digits": self.angle_digits.to_string(),
            "units": self.units,
        })
    }

    /// 設定の値から読む。数は数でも文字でもよい。正しくなければ理由を返す。
    fn from_json(v: &serde_json::Value) -> Result<DimFormat, String> {
        let base = DimFormat::default();
        let digits = |key: &str, default: u8| -> Result<u8, String> {
            let Some(x) = v.get(key) else {
                return Ok(default);
            };
            let n = match x {
                serde_json::Value::Number(n) => n.as_u64(),
                serde_json::Value::String(s) => s.trim().parse().ok(),
                _ => None,
            };
            match n {
                Some(n) if n <= MAX_DIGITS as u64 => Ok(n as u8),
                _ => Err(format!("{key} は 0〜{MAX_DIGITS} の整数にしてください")),
            }
        };
        Ok(DimFormat {
            length_digits: digits("length_digits", base.length_digits)?,
            angle_digits: digits("angle_digits", base.angle_digits)?,
            units: match v.get("units") {
                None => base.units,
                Some(serde_json::Value::Bool(b)) => *b,
                Some(_) => return Err("units は真か偽にしてください".into()),
            },
        })
    }
}

/// 設定の窓に出すまとまり。
pub(crate) fn settings(store: &SketchSettings) -> kmb_core::Settings {
    let choices: Vec<(String, String)> = (0..=MAX_DIGITS)
        .map(|d| (d.to_string(), format!("{d} 桁({})", fixed(12.345_678, d))))
        .collect();
    let refs: Vec<(&str, &str)> = choices
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    kmb_core::Settings {
        id: DISPLAY_SETTINGS.into(),
        label: "スケッチの寸法の表示".into(),
        group: "cad".into(),
        notes: vec![
            "図に出す寸法の数字の小数の桁数と単位。形の計算は丸めない(見せる時だけ丸める)".into(),
        ],
        fields: vec![
            kmb_core::Field::choice("length_digits", "長さの小数の桁数", &refs),
            kmb_core::Field::choice("angle_digits", "角度の小数の桁数", &refs),
            kmb_core::Field::bool("units", "単位を付ける(mm・in など、°)"),
        ],
        value: store.dim_format().to_json(),
        default: DimFormat::default().to_json(),
        role: None,
    }
}

/// 設定を変える。
pub(crate) fn set_settings(store: &SketchSettings, v: &serde_json::Value) -> Result<(), String> {
    store.set_dim_format(DimFormat::from_json(v)?);
    Ok(())
}

/// スケッチを作る時の設定の名前(データフォルダの設定ファイルの鍵)。
pub const CREATE_SETTINGS: &str = "sketch.create";

/// スケッチを作る時の設定の窓のまとまり。
pub(crate) fn create_settings(store: &SketchSettings) -> kmb_core::Settings {
    kmb_core::Settings {
        id: CREATE_SETTINGS.into(),
        label: "スケッチを作る時".into(),
        group: "cad".into(),
        notes: vec![
            "面にスケッチを作ると、面の縁を投影する(押し出せる輪郭になり、吸い付く先・寸法の相手にもなる。元の形が変わると付いて行く)".into(),
        ],
        fields: vec![kmb_core::Field::bool(
            "project_face_edges",
            "面にスケッチを作る時、面の縁を投影する",
        )],
        value: json!({ "project_face_edges": store.project_face_edges() }),
        default: json!({ "project_face_edges": true }),
        role: None,
    }
}

/// スケッチを作る時の設定を変える。
pub(crate) fn set_create_settings(
    store: &SketchSettings,
    v: &serde_json::Value,
) -> Result<(), String> {
    let on = match v.get("project_face_edges") {
        None => true,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return Err("project_face_edges は真か偽にしてください".into()),
    };
    store
        .face_edges
        .store(on, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_shown_with_fixed_digits_and_units() {
        let f = DimFormat::default();
        assert_eq!(f.length(15.0), "15.00 mm");
        assert_eq!(f.length(-0.001), "0.00 mm");
        assert_eq!(f.angle(std::f64::consts::FRAC_PI_6), "30.0°");
        let g = DimFormat {
            length_digits: 0,
            angle_digits: 3,
            units: false,
        };
        assert_eq!(g.length(12.6), "13");
        assert_eq!(g.angle(1.0), "57.296");
        // インチの文書では長さだけインチで出す(形の値は mm のまま)
        kmb_core::set_display_unit(kmb_core::LengthUnit::In);
        assert_eq!(f.length(12.7), "0.50 in");
        assert_eq!(f.angle(std::f64::consts::FRAC_PI_6), "30.0°");
        kmb_core::set_display_unit(kmb_core::LengthUnit::Mm);
    }

    #[test]
    fn settings_are_read_from_strings_or_numbers_and_checked() {
        let f = DimFormat::from_json(&json!({"length_digits": "4", "angle_digits": 0})).unwrap();
        assert_eq!((f.length_digits, f.angle_digits, f.units), (4, 0, true));
        assert!(DimFormat::from_json(&json!({"length_digits": "9"})).is_err());
        assert!(DimFormat::from_json(&json!({"units": "yes"})).is_err());
        assert_eq!(
            DimFormat::from_json(&DimFormat::default().to_json()).unwrap(),
            DimFormat::default()
        );
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    use kmb_core::Extension;

    /// 設定の窓と同じ道(モジュールの設定)で変えると、図の見せ方が変わる。
    #[test]
    fn the_settings_change_how_dimensions_are_shown() {
        let ext = crate::SketchCommands::default();
        let dim_format = || ext.settings_store().dim_format();
        let group = ext
            .settings()
            .into_iter()
            .find(|g| g.id == DISPLAY_SETTINGS)
            .unwrap();
        assert_eq!(group.fields.len(), 3);
        let r = ext.set_settings(
            DISPLAY_SETTINGS,
            &json!({"length_digits": "4", "angle_digits": "0", "units": false}),
        );
        assert_eq!(r, Some(Ok(())));
        assert_eq!(dim_format().length(15.0), "15.0000");
        assert_eq!(dim_format().angle(std::f64::consts::FRAC_PI_4), "45");
        // 正しくない値は何も変えない
        assert!(matches!(
            ext.set_settings(DISPLAY_SETTINGS, &json!({"length_digits": "12"})),
            Some(Err(_))
        ));
        assert_eq!(dim_format().length_digits, 4);
        ext.set_settings(DISPLAY_SETTINGS, &group.default);
        assert_eq!(dim_format(), DimFormat::default());
    }

    /// 設定は組み立てた物ごとに持つ。同じプロセスの 2 つ目の窓や CLI・MCP の 1 回の設定が、
    /// ほかの窓の寸法の見せ方を変えない。
    #[test]
    fn each_assembly_keeps_its_own_settings() {
        let (a, b) = (crate::module(), crate::module());
        let ext = a.extensions[0].clone();
        let (ra, rb) = (kmb_core::registry_of(&[a]), kmb_core::registry_of(&[b]));
        let r = ext.set_settings(DISPLAY_SETTINGS, &json!({"length_digits": "0"}));
        assert_eq!(r, Some(Ok(())));
        assert_eq!(dim_format(&ra).length_digits, 0);
        assert_eq!(dim_format(&rb), DimFormat::default());
        // 組み立てに無ければ既定
        assert_eq!(dim_format(&kmb_core::Registry::new()), DimFormat::default());
    }
}
