//! 文書の外観(面ごとの外観)。
//!
//! 外観は名前で指す。名前はまず文書の「このデザイン内」の外観([`AppearanceDef`]。色・粗さを
//! 直した物や複製した物)を引き、無ければ JC のライブラリ([`crate::material::LIBRARY`])を引く。
//! ボディ・部品・面に付けた外観と「このデザイン内」の外観は、文書の区画 [`Appearances`]
//! (`render.appearances`。レンダリングのモジュール jc-io-render が名乗る)に入る。
//! 面の外観は、ボディ(最初に作った機能)と面の名前の組に付ける。面の名前は形を作り直しても
//! 同じなので、寸法を変えても付いたまま。面が無くなっても別の面に付け替えない(見つからない面として残す)。
//! 塗る順は、面の外観、ボディの外観、部品の外観、物理材料の色。

use crate::material::{self, Finish, Grain};
use jc_core::{DocumentData, FeatureId, SectionType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 1 つの文書に持てる「このデザイン内」の外観の数の上限。
const MAX_DEFS: usize = 500;

/// 「このデザイン内」の外観(ライブラリの外観の色・粗さを直した物か、複製した物)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceDef {
    pub name: String,
    /// 分類([`crate::material::CATEGORIES`] のどれか。知らない分類は「その他」に並べる)
    #[serde(default = "other")]
    pub category: String,
    /// 色(sRGB。木目なら早材の色)
    pub color: [u8; 3],
    /// 金属か(0 か 1。間は金属と塗装の混ざった見た目)
    #[serde(default)]
    pub metallic: f32,
    /// 面の粗さ(0 は鏡、1 はつやなし)
    pub roughness: f32,
    /// 元にしたライブラリの外観(木目はこの外観のを使う)。無ければ同じ名前のライブラリの外観
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

fn other() -> String {
    "その他".into()
}

/// 文書の区画 `render.appearances`: 文書の外観の一式。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Appearances {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defs: Vec<AppearanceDef>,
    /// 面の外観。鍵はボディ(最初に作った機能)、その中は面の名前 → 外観の名前
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub faces: BTreeMap<FeatureId, BTreeMap<String, String>>,
    /// ボディの外観。鍵はボディ(最初に作った機能)。無いボディは部品の外観、無ければ材料の色
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bodies: BTreeMap<FeatureId, String>,
    /// 部品の外観。鍵は部品の番号。外観を付けていない中のボディをこの色で塗る
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<u64, String>,
    /// このアプリの知らない項目(新しいアプリが足した物)。読んだまま書き戻す
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl SectionType for Appearances {
    const KEY: &'static str = "render.appearances";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "外観";

    fn check(&self) -> Result<(), String> {
        check_appearances(self)
    }

    fn is_empty(&self) -> bool {
        self.defs.is_empty()
            && self.faces.is_empty()
            && self.bodies.is_empty()
            && self.components.is_empty()
            && self.rest.is_empty()
    }
}

/// 文書の外観。区画を読めなければログに残して空(どのボディも材料の色)。
pub fn of(data: &DocumentData) -> Appearances {
    data.section().unwrap_or_else(|e| {
        log::warn!("{e}");
        Appearances::default()
    })
}

/// 文書のボディ `root` の外観の名前([`Appearances::appearance_of`])。
pub fn appearance_of(data: &DocumentData, root: FeatureId) -> Option<String> {
    of(data).appearance_of(data, root).map(str::to_string)
}

/// 名前から引いた外観の値。
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub name: String,
    pub category: String,
    pub color: [u8; 3],
    pub finish: Finish,
    /// 「このデザイン内」で直した外観か(偽ならライブラリのまま)
    pub custom: bool,
}

impl Appearances {
    /// ボディ `root` の外観: ボディに付けた外観、なければ入っている部品(近い方から)の外観。
    pub fn appearance_of(&self, data: &DocumentData, root: FeatureId) -> Option<&str> {
        if let Some(a) = self.bodies.get(&root) {
            return Some(a);
        }
        let mut cur = data.component_of(root);
        for _ in 0..=data.components.len() {
            let c = cur?;
            if let Some(a) = self.components.get(&c.id) {
                return Some(a);
            }
            cur = c.parent.and_then(|p| data.component(p));
        }
        None
    }

    /// ボディ `root` の外観を `name` にする。空か `None` なら外す(部品か材料の色に戻る)。
    pub fn set_body(&mut self, root: FeatureId, name: Option<&str>) {
        match name.map(str::trim) {
            Some(n) if !n.is_empty() => {
                self.bodies.insert(root, n.to_string());
            }
            _ => {
                self.bodies.remove(&root);
            }
        }
    }

    /// 部品 `id` の外観を `name` にする。空か `None` なら外す。
    pub fn set_component(&mut self, id: u64, name: Option<&str>) {
        match name.map(str::trim) {
            Some(n) if !n.is_empty() => {
                self.components.insert(id, n.to_string());
            }
            _ => {
                self.components.remove(&id);
            }
        }
    }

    /// 部品の外観のうち、文書 `data` に今ある部品の物(消した部品の外観は取り消しで戻るまで残る)。
    fn live_components<'a>(
        &'a self,
        data: &'a DocumentData,
    ) -> impl Iterator<Item = (u64, &'a String)> + 'a {
        self.components
            .iter()
            .filter(|(id, _)| data.component(**id).is_some())
            .map(|(id, a)| (*id, a))
    }

    /// 外観 `name` を使っている所: ボディ(最初の機能)、部品(文書に今ある物)、面(ボディと面の名前)。
    pub fn users(
        &self,
        data: &DocumentData,
        name: &str,
    ) -> (Vec<FeatureId>, Vec<u64>, Vec<(FeatureId, String)>) {
        let bodies = self
            .bodies
            .iter()
            .filter(|(_, a)| a.as_str() == name)
            .map(|(r, _)| *r)
            .collect();
        let comps = self
            .live_components(data)
            .filter(|(_, a)| a.as_str() == name)
            .map(|(c, _)| c)
            .collect();
        let faces = self
            .faces
            .iter()
            .flat_map(|(r, m)| {
                m.iter()
                    .filter(|(_, a)| a.as_str() == name)
                    .map(|(f, _)| (*r, f.clone()))
            })
            .collect();
        (bodies, comps, faces)
    }

    /// 「このデザイン内」の外観の名前: 直した・複製した外観、次にボディ・部品・面で使っている
    /// ライブラリの外観(出てきた順。重ねない)。
    pub fn design_names(&self, data: &DocumentData) -> Vec<String> {
        let mut out: Vec<String> = self.defs.iter().map(|d| d.name.clone()).collect();
        let used = self
            .bodies
            .values()
            .chain(self.live_components(data).map(|(_, a)| a))
            .chain(self.faces.values().flat_map(|m| m.values()));
        for n in used {
            if !out.contains(n) {
                out.push(n.clone());
            }
        }
        out
    }

    /// 名前 `name` の「このデザイン内」の外観。
    pub fn def(&self, name: &str) -> Option<&AppearanceDef> {
        let name = name.trim();
        self.defs.iter().find(|d| d.name == name)
    }

    /// 名前の外観(「このデザイン内」の外観、無ければライブラリ)。知らない名前は `None`。
    pub fn resolve(&self, name: &str) -> Option<Resolved> {
        if let Some(d) = self.def(name) {
            let grain: Option<Grain> = d
                .base
                .as_deref()
                .and_then(material::library_look)
                .or_else(|| material::library_look(&d.name))
                .and_then(|l| l.finish.grain);
            return Some(Resolved {
                name: d.name.clone(),
                category: d.category.clone(),
                color: d.color,
                finish: Finish {
                    metallic: d.metallic,
                    roughness: d.roughness,
                    grain,
                },
                custom: true,
            });
        }
        material::library_look(name).map(|l| Resolved {
            name: l.name.to_string(),
            category: l.category.to_string(),
            color: l.color,
            finish: l.finish,
            custom: false,
        })
    }

    /// ボディ `root` の面 `face` の外観の名前。
    pub fn face(&self, root: FeatureId, face: &str) -> Option<&str> {
        self.faces.get(&root)?.get(face).map(String::as_str)
    }

    /// `base` を元にした、まだ使われていない名前(「アルミ (2)」のように番号を付ける。決まりは [`jc_core::unique_name`])。
    pub fn free_name(&self, base: &str) -> String {
        let taken = |n: &str| self.def(n).is_some() || material::library_look(n).is_some();
        jc_core::unique_name(base, |n| n == base || taken(n))
    }
}

/// 外観の一式が使える値か(名前が空でなく重ならない、値が 0〜1、面の名前と外観の名前が空でない)。
/// 面やボディが今あるかは問わない(取り消しで戻るため。無い面は窓が「見つからない面」と出す)。
fn check_appearances(a: &Appearances) -> Result<(), String> {
    if a.defs.len() > MAX_DEFS {
        return Err(format!("外観は {MAX_DEFS} 個までです"));
    }
    for (i, d) in a.defs.iter().enumerate() {
        if d.name.trim().is_empty() || d.name.trim() != d.name {
            return Err("外観の名前が空か、前後に空白があります".into());
        }
        if a.defs[..i].iter().any(|o| o.name == d.name) {
            return Err(format!("外観の名前 {} が重なっています", d.name));
        }
        let ok = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        if !ok(d.metallic) || !ok(d.roughness) {
            return Err(format!("外観 {} の金属・粗さは 0〜1 です", d.name));
        }
    }
    if a.bodies
        .values()
        .chain(a.components.values())
        .any(|n| n.trim().is_empty() || n.trim() != n)
    {
        return Err("ボディか部品の外観の名前が空か、前後に空白があります".into());
    }
    for faces in a.faces.values() {
        if faces.is_empty() {
            return Err("面の外観の空の組があります".into());
        }
        if faces
            .iter()
            .any(|(f, n)| f.trim().is_empty() || n.trim().is_empty())
        {
            return Err("面の名前か外観の名前が空です".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str) -> AppearanceDef {
        AppearanceDef {
            name: name.into(),
            category: "金属".into(),
            color: [10, 20, 30],
            metallic: 1.0,
            roughness: 0.4,
            base: None,
        }
    }

    #[test]
    fn a_design_appearance_overrides_the_library_of_the_same_name() {
        let mut a = Appearances::default();
        assert_eq!(a.resolve("アルミ").map(|r| r.custom), Some(false));
        a.defs.push(def("アルミ"));
        let r = a.resolve("アルミ").unwrap();
        assert!(r.custom);
        assert_eq!(r.color, [10, 20, 30]);
        assert_eq!(r.finish.roughness, 0.4);
        assert!(a.resolve("知らない外観").is_none());
    }

    #[test]
    fn a_copy_of_a_wood_keeps_the_grain() {
        let mut a = Appearances::default();
        let mut d = def("板 (2)");
        d.base = Some("ウォルナット".into());
        a.defs.push(d);
        assert!(a.resolve("板 (2)").unwrap().finish.grain.is_some());
        assert_eq!(a.free_name("板"), "板 (3)");
    }

    #[test]
    fn bad_values_are_refused() {
        let mut a = Appearances::default();
        a.defs.push(def("x"));
        assert!(check_appearances(&a).is_ok());
        a.defs.push(def("x"));
        assert!(check_appearances(&a).is_err());
        a.defs.pop();
        a.defs[0].roughness = 1.5;
        assert!(check_appearances(&a).is_err());
        a.defs[0].roughness = 0.5;
        a.faces.insert(FeatureId(1), BTreeMap::new());
        assert!(check_appearances(&a).is_err());
        a.faces.insert(
            FeatureId(1),
            BTreeMap::from([("f1:end".into(), "赤".into())]),
        );
        assert!(check_appearances(&a).is_ok());
        assert_eq!(a.face(FeatureId(1), "f1:end"), Some("赤"));
    }
}
