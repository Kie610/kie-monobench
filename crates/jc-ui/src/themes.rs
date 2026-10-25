//! テーマの選び方と、利用者のテーマのファイル(データフォルダの `themes/*.json`。U38)。
//!
//! 同梱のテーマは標準・ハイコントラスト・色覚の型向け 2 つ(U27)で、どれも明るい・暗いの 2 組を持ち、設定の
//! 「明るさ」で替わる。利用者のテーマは同梱のテーマを元にして、変えたい色だけを書く:
//!
//! ```json
//! {
//!   "name": "青い設計",
//!   "base": "standard",
//!   "colors": { "design": "#1f5fbf" },
//!   "light": { "canvas": "#ffffff" },
//!   "dark": { "canvas": "#101418" }
//! }
//! ```
//!
//! `colors` は明暗の両方に、`light`・`dark` はその明るさの時だけ効く。色は `#rrggbb`(透かす色は
//! `#rrggbbaa`)。知らない鍵と読めない色は無視して、設定の窓に知らせる。読めないファイルは飛ばす
//! (起動できなくしない)。色の名前の一覧は、設定の「今のテーマを書き出す」で出る。
//!
//! `base` に書けるのは同梱のテーマ(プログラムの中の値)だけで、別のテーマのファイルは元にできない
//! (ファイルからファイルへの継ぎ足しは作らない。docs/避ける形.md)。

use crate::layout::Appearance;
use crate::theme::{Palette, Theme};
use egui::Color32;
use std::path::{Path, PathBuf};

/// 同梱のテーマの名前。
pub const STANDARD: &str = "standard";
pub const HIGH_CONTRAST: &str = "high_contrast";
/// 色覚の型に合わせたテーマ(1 型・2 型向け、3 型向け。U27)。
pub const CVD_RG: &str = "cvd_rg";
pub const CVD_TRI: &str = "cvd_tri";

/// 同梱のテーマ(名前、画面に出す名前、テーマ)。
pub const BUNDLED: [(&str, &str, Theme); 4] = [
    (STANDARD, "標準", Theme::STANDARD),
    (HIGH_CONTRAST, "ハイコントラスト", Theme::HIGH_CONTRAST),
    (CVD_RG, "色覚 1・2 型向け(赤と緑)", Theme::CVD_RG),
    (CVD_TRI, "色覚 3 型向け(青と黄)", Theme::CVD_TRI),
];

/// 同梱のテーマ(見つからなければ `None`)。
pub fn bundled(id: &str) -> Option<Theme> {
    BUNDLED.iter().find(|b| b.0 == id).map(|b| b.2)
}
/// 利用者のテーマの名前の頭(続けてファイルの名前)。
pub const USER: &str = "user:";
/// データフォルダの中の、利用者のテーマのフォルダ。
pub const DIR: &str = "themes";

/// 利用者のテーマ(読んだ 1 つのファイル)。
#[derive(Clone, Debug, PartialEq)]
pub struct UserTheme {
    /// ファイルの名前(拡張子なし)
    pub id: String,
    /// 画面に出す名前(`name` が無ければファイルの名前)
    pub name: String,
    pub light: Palette,
    pub dark: Palette,
    /// 無視した鍵や色(設定の窓に出す)
    pub notes: Vec<String>,
}

/// 選べるテーマ(同梱と、データフォルダの `themes/` から読んだもの)。
#[derive(Clone, Debug, Default)]
pub struct Themes {
    /// 利用者のテーマのフォルダ(データフォルダが無ければ `None`)
    pub dir: Option<PathBuf>,
    pub user: Vec<UserTheme>,
    /// 読めなかったファイルと理由
    pub broken: Vec<(String, String)>,
}

impl Themes {
    /// データフォルダの `themes/*.json` を読む(フォルダが無ければ同梱のテーマだけ)。
    pub fn load(data_dir: Option<&Path>) -> Themes {
        let dir = data_dir.map(|d| d.join(DIR));
        let mut out = Themes {
            dir: dir.clone(),
            ..Themes::default()
        };
        let Some(dir) = dir else {
            return out;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return out;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("json"))
            })
            .collect();
        files.sort();
        for path in files {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let file = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            match std::fs::read_to_string(&path)
                .map_err(|e| format!("読めません: {e}"))
                .and_then(|text| parse(&text, &stem))
            {
                Ok(t) => {
                    if !t.notes.is_empty() {
                        log::warn!("テーマ {file}: {}", t.notes.join("、"));
                    }
                    out.user.push(t)
                }
                Err(e) => {
                    log::warn!("テーマ {file} を飛ばしました: {e}");
                    out.broken.push((file, e));
                }
            }
        }
        out
    }

    /// 選べるテーマ(名前と画面に出す名前)。同梱が先、利用者のテーマはファイルの名前の順。
    pub fn choices(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = BUNDLED
            .iter()
            .map(|b| (b.0.to_string(), b.1.to_string()))
            .collect();
        out.extend(
            self.user
                .iter()
                .map(|t| (format!("{USER}{}", t.id), t.name.clone())),
        );
        out
    }

    /// テーマの画面に出す名前(見つからなければ、標準に戻したことが分かる名前)。
    pub fn label(&self, id: &str) -> String {
        self.choices()
            .into_iter()
            .find(|(k, _)| k == id)
            .map(|(_, n)| n)
            .unwrap_or_else(|| format!("標準(「{}」が見つかりません)", id.trim_start_matches(USER)))
    }

    /// 選んだ利用者のテーマ。
    pub fn user_theme(&self, id: &str) -> Option<&UserTheme> {
        let stem = id.strip_prefix(USER)?;
        self.user.iter().find(|t| t.id == stem)
    }

    /// 見た目の設定から、今のテーマ(色と倍率)を作る。見つからないテーマは標準にする。
    pub fn theme(&self, a: &Appearance) -> Theme {
        let base = match bundled(&a.theme) {
            Some(t) => t,
            None => match self.user_theme(&a.theme) {
                Some(t) => Theme {
                    light: t.light,
                    dark: t.dark,
                    ..Theme::STANDARD
                },
                None => Theme::STANDARD,
            },
        };
        let mut t = Theme {
            scale: a.scale(),
            density: a.density.factor(),
            ..base
        };
        if !a.role_colors {
            for p in [&mut t.light, &mut t.dark] {
                p.icon_base = p.ink;
                p.role_make = p.ink;
                p.role_modify = p.ink;
                p.role_repeat = p.ink;
                p.role_define = p.ink;
            }
        }
        t
    }
}

/// 色を `#rrggbb`(透かす色は `#rrggbbaa`)で書く。
pub fn to_hex(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// `#rgb`・`#rrggbb`・`#rrggbbaa` を読む。
pub fn parse_color(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    if !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        3 => {
            let v: Vec<u8> = h
                .chars()
                .map(|c| c.to_digit(16).map(|d| d as u8 * 17))
                .collect::<Option<_>>()?;
            Some(Color32::from_rgb(v[0], v[1], v[2]))
        }
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)?,
        )),
        _ => None,
    }
}

/// 色の上書きを当てる。無視した鍵と色を `notes` に足す。
fn overlay(
    p: &mut Palette,
    colors: &serde_json::Map<String, serde_json::Value>,
    part: &str,
    notes: &mut Vec<String>,
) {
    let mut known = std::collections::HashSet::new();
    p.each_mut(&mut |k, c| {
        known.insert(k.to_string());
        if let Some(v) = colors.get(k) {
            match v.as_str().and_then(parse_color) {
                Some(new) => *c = new,
                None => notes.push(format!("{part}.{k} の色 {v} を読めません")),
            }
        }
    });
    for k in colors.keys() {
        if !known.contains(k) {
            notes.push(format!("{part}.{k} という色はありません"));
        }
    }
}

/// テーマのファイルの中身を読む。`stem` はファイルの名前(拡張子なし)。
pub fn parse(text: &str, stem: &str) -> Result<UserTheme, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("JSON として読めません: {e}"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| "中身が { … } ではありません".to_string())?;
    let mut notes = vec![];
    let base = match obj.get("base").and_then(|b| b.as_str()) {
        None => Theme::STANDARD,
        Some(id) => bundled(id).unwrap_or_else(|| {
            notes.push(format!("base の {id} は無いので standard を元にしました"));
            Theme::STANDARD
        }),
    };
    let (mut light, mut dark) = (base.light, base.dark);
    let empty = serde_json::Map::new();
    let part = |k: &str, notes: &mut Vec<String>| match obj.get(k) {
        None => &empty,
        Some(serde_json::Value::Object(m)) => m,
        Some(_) => {
            notes.push(format!("{k} が {{ … }} ではないので無視しました"));
            &empty
        }
    };
    let both = part("colors", &mut notes);
    overlay(&mut light, both, "colors", &mut notes);
    overlay(&mut dark, both, "colors", &mut notes);
    // 両方に書いた時は、色を少なく書くことが多い colors の後に、明暗ごとの物を当てる
    overlay(&mut light, part("light", &mut notes), "light", &mut notes);
    overlay(&mut dark, part("dark", &mut notes), "dark", &mut notes);
    for k in obj.keys() {
        if !["name", "base", "colors", "light", "dark"].contains(&k.as_str()) {
            notes.push(format!("{k} という鍵は使いません"));
        }
    }
    // colors は明暗の両方に当てるので、同じ知らせが 2 つ出る
    let mut seen = std::collections::HashSet::new();
    notes.retain(|n| seen.insert(n.clone()));
    let name = obj
        .get("name")
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(stem)
        .to_string();
    Ok(UserTheme {
        id: stem.to_string(),
        name,
        light,
        dark,
        notes,
    })
}

/// テーマを、全部の色を書いたファイルの中身にする(利用者が写して直す元)。
pub fn template(name: &str, base: &str, theme: &Theme) -> String {
    let colors = |p: &Palette| {
        serde_json::Value::Object(
            p.entries()
                .into_iter()
                .map(|(k, c)| (k, serde_json::Value::String(to_hex(c))))
                .collect(),
        )
    };
    let mut obj = serde_json::Map::new();
    obj.insert("name".into(), name.into());
    obj.insert("base".into(), base.into());
    obj.insert("light".into(), colors(&theme.light));
    obj.insert("dark".into(), colors(&theme.dark));
    serde_json::to_string_pretty(&serde_json::Value::Object(obj)).expect("JSON にできる")
}

/// 今のテーマを `dir` に書き出す。重ならないファイルの名前にして、書いた道を返す。
pub fn export(dir: &Path, themes: &Themes, a: &Appearance) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{} を作れません: {e}", dir.display()))?;
    let label = themes.label(&a.theme);
    let base = match bundled(&a.theme) {
        Some(_) => a.theme.as_str(),
        None => STANDARD,
    };
    let stem = match a.theme.strip_prefix(USER) {
        Some(s) => format!("{s}-写し"),
        None => format!("{label}-写し"),
    };
    let mut path = dir.join(format!("{stem}.json"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}{n}.json"));
        n += 1;
    }
    // 役の色を切っていても、写しにはテーマの役の色を書く(切り替えは設定の側に残す)
    let colored = Appearance {
        role_colors: true,
        ..a.clone()
    };
    let text = template(&format!("{label}の写し"), base, &themes.theme(&colored));
    // ほかの書き出しと同じ道で書き、記録に残す
    jc_session::write_output(&path, text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, HC_DARK, HC_LIGHT, LIGHT};

    #[test]
    fn colors_round_trip_as_hex() {
        for c in [
            Color32::from_rgb(1, 2, 3),
            Color32::from_rgb(255, 128, 0),
            Color32::from_rgba_unmultiplied(10, 20, 30, 40),
        ] {
            assert_eq!(parse_color(&to_hex(c)), Some(c), "{}", to_hex(c));
        }
        assert_eq!(parse_color("#f80"), Some(Color32::from_rgb(255, 136, 0)));
        for bad in ["", "f80", "#12345", "#ggg", "#ｆｆｆ"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_user_theme_overrides_only_what_it_names() {
        let t = parse(
            r##"{ "name": "青", "colors": { "design": "#1f5fbf", "axis_x": "#ff0000" },
                  "dark": { "canvas": "#101418", "nope": "#000000" }, "extra": 1 }"##,
            "blue",
        )
        .unwrap();
        assert_eq!(t.name, "青");
        assert_eq!(t.light.design, Color32::from_rgb(0x1f, 0x5f, 0xbf));
        assert_eq!(t.dark.design, Color32::from_rgb(0x1f, 0x5f, 0xbf));
        assert_eq!(t.light.inks.axis[0], Color32::from_rgb(255, 0, 0));
        assert_eq!(t.dark.canvas, Color32::from_rgb(0x10, 0x14, 0x18));
        // 書かなかった色は元のテーマのまま
        assert_eq!(t.light.canvas, LIGHT.canvas);
        assert_eq!(t.light.ink, LIGHT.ink);
        assert_eq!(t.dark.ink, DARK.ink);
        assert_eq!(t.notes.len(), 2, "{:?}", t.notes);
        // 元をハイコントラストにする
        let hc = parse(r#"{ "base": "high_contrast" }"#, "hc").unwrap();
        assert_eq!(
            (hc.light, hc.dark, hc.name.as_str()),
            (HC_LIGHT, HC_DARK, "hc")
        );
        assert!(parse("[1]", "x").is_err());
        assert!(parse("{", "x").is_err());
    }

    #[test]
    fn every_color_has_a_unique_name_and_the_template_reads_back() {
        let names: Vec<String> = LIGHT.entries().into_iter().map(|(k, _)| k).collect();
        let set: std::collections::HashSet<&String> = names.iter().collect();
        assert_eq!(set.len(), names.len(), "名前が重なっている");
        let text = template("写し", HIGH_CONTRAST, &Theme::HIGH_CONTRAST);
        let back = parse(&text, "copy").unwrap();
        assert!(back.notes.is_empty(), "{:?}", back.notes);
        assert_eq!((back.light, back.dark), (HC_LIGHT, HC_DARK));
    }

    #[test]
    fn themes_load_from_the_data_folder_and_skip_broken_files() {
        let dir = std::env::temp_dir().join(format!("jc-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(DIR)).unwrap();
        std::fs::write(
            dir.join(DIR).join("a.json"),
            r##"{ "colors": { "ok": "#00ff00" } }"##,
        )
        .unwrap();
        std::fs::write(dir.join(DIR).join("b.json"), "壊れた").unwrap();
        std::fs::write(dir.join(DIR).join("c.txt"), "{}").unwrap();
        let t = Themes::load(Some(&dir));
        assert_eq!(t.user.len(), 1);
        assert_eq!(t.broken.len(), 1);
        assert_eq!(t.choices().len(), BUNDLED.len() + 1);
        let a = Appearance {
            theme: "user:a".into(),
            ..Appearance::default()
        };
        assert_eq!(t.theme(&a).light.ok, Color32::from_rgb(0, 255, 0));
        // 見つからないテーマは標準
        let gone = Appearance {
            theme: "user:gone".into(),
            ..Appearance::default()
        };
        assert_eq!(t.theme(&gone), Theme::STANDARD);
        // 書き出すと、次に読んだ時に選べる
        let path = export(&dir.join(DIR), &t, &a).unwrap();
        let again = Themes::load(Some(&dir));
        assert_eq!(again.user.len(), 2, "{}", path.display());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn turning_role_colors_off_draws_icons_in_one_ink() {
        let t = Themes::load(None);
        let on = t.theme(&Appearance::default());
        assert_ne!(on.light.role_make, on.light.ink);
        let off = t.theme(&Appearance {
            role_colors: false,
            ..Appearance::default()
        });
        for p in [off.light, off.dark] {
            let inks = p.icon_inks();
            assert_eq!(inks, crate::icons::IconInks::mono(p.ink));
        }
        // 役の色のほかは変わらない
        assert_eq!(off.light.chrome, on.light.chrome);
    }
}
