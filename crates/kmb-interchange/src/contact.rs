//! 接触セット(ブラウザの「接触セット」)。
//!
//! 接触セットは、ぶつかったら止まるボディ(最初に作った機能)の組。接触セットを有効にしている間、
//! 3D のビューでジョイントの部品を引くと、同じセットのほかのボディに食い込む手前で止まる
//! (止める計算は組立のモジュールが [`kmb_core::Extension::drag_limit`] で答える)。設計の形と値の
//! 計算は変えない。
//!
//! 文書には組立のモジュール(kmb-asm)が名乗る区画 [`Contacts`](`asm.contacts`)として入る
//! (版 3 までの文書では上の階の `contacts`。読む時に区画へ移す)。

use kmb_core::{FeatureId, SectionType};
use serde::{Deserialize, Serialize};

/// 1 つの文書に持てる接触セットの数の上限。
const MAX_SETS: usize = 100;

/// 接触セット 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContactSet {
    pub name: String,
    /// ぶつかったら止まるボディ(最初に作った機能。2 つ以上)
    pub bodies: Vec<FeatureId>,
    /// 偽ならこのセットは使わない(ブラウザの目)
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// 文書の接触セット全部と、全体の入切。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contacts {
    /// 接触セットを有効にしているか(切っている間はどのセットも使わない)
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sets: Vec<ContactSet>,
    /// このアプリの知らない項目(新しいアプリが足した物)。読んだまま書き戻す
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

/// 文書の接触セット(画面の一覧に使う)。区画を読めなければログに残して空。
pub fn of(data: &kmb_core::DocumentData) -> Contacts {
    data.section().unwrap_or_else(|e| {
        log::warn!("{e}");
        Contacts::default()
    })
}

impl SectionType for Contacts {
    const KEY: &'static str = "asm.contacts";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "接触セット";

    fn check(&self) -> Result<(), String> {
        check_contacts(self)
    }

    fn is_empty(&self) -> bool {
        Contacts::is_empty(self)
    }
}

impl Contacts {
    /// 何も決めていない(保存しない)。
    pub fn is_empty(&self) -> bool {
        !self.enabled && self.sets.is_empty() && self.rest.is_empty()
    }

    /// 今使うセットのうち、ボディ `moving` のどれかを持つ物の、`moving` 以外のボディ(止める相手)。
    /// 接触セットを切っていれば空。
    pub fn obstacles(&self, moving: &[FeatureId]) -> Vec<FeatureId> {
        if !self.enabled {
            return Vec::new();
        }
        let mut out: Vec<FeatureId> = Vec::new();
        for s in self.sets.iter().filter(|s| s.enabled) {
            if !s.bodies.iter().any(|b| moving.contains(b)) {
                continue;
            }
            for b in &s.bodies {
                if !moving.contains(b) && !out.contains(b) {
                    out.push(*b);
                }
            }
        }
        out
    }

    /// まだ使われていない「接触セットN」の名前。
    pub fn free_name(&self) -> String {
        (1..)
            .map(|i| format!("接触セット{i}"))
            .find(|n| self.sets.iter().all(|s| &s.name != n))
            .expect("どこかで空く")
    }
}

/// 接触セットが使える値か(名前が空でなく重ならない、ボディが 2 つ以上で重ならない)。
/// ボディが今あるかは問わない(取り消しで戻るため)。
pub fn check_contacts(c: &Contacts) -> Result<(), String> {
    if c.sets.len() > MAX_SETS {
        return Err(format!("接触セットは {MAX_SETS} 個までです"));
    }
    for (i, s) in c.sets.iter().enumerate() {
        if s.name.trim().is_empty() {
            return Err("接触セットの名前が空です".into());
        }
        if c.sets[..i].iter().any(|o| o.name == s.name) {
            return Err(format!("接触セットの名前「{}」が重なっています", s.name));
        }
        if s.bodies.len() < 2 {
            return Err(format!(
                "接触セット「{}」には、ボディを 2 つ以上入れてください",
                s.name
            ));
        }
        for (j, b) in s.bodies.iter().enumerate() {
            if s.bodies[..j].contains(b) {
                return Err(format!(
                    "接触セット「{}」にボディ {b} が 2 回あります",
                    s.name
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(name: &str, bodies: &[u64]) -> ContactSet {
        ContactSet {
            name: name.into(),
            bodies: bodies.iter().map(|b| FeatureId(*b)).collect(),
            enabled: true,
        }
    }

    #[test]
    fn obstacles_come_from_the_enabled_sets_that_hold_the_moving_bodies() {
        let mut c = Contacts {
            enabled: true,
            sets: vec![set("a", &[1, 2]), set("b", &[2, 3, 4]), set("c", &[5, 6])],
            ..Default::default()
        };
        assert_eq!(
            c.obstacles(&[FeatureId(2)]),
            vec![FeatureId(1), FeatureId(3), FeatureId(4)]
        );
        assert!(c.obstacles(&[FeatureId(9)]).is_empty());
        c.sets[1].enabled = false;
        assert_eq!(c.obstacles(&[FeatureId(2)]), vec![FeatureId(1)]);
        c.enabled = false;
        assert!(
            c.obstacles(&[FeatureId(2)]).is_empty(),
            "全体を切ると使わない"
        );
        assert_eq!(c.free_name(), "接触セット1");
    }

    #[test]
    fn bad_sets_are_refused() {
        let ok = Contacts {
            enabled: true,
            sets: vec![set("a", &[1, 2])],
            ..Default::default()
        };
        assert!(check_contacts(&ok).is_ok());
        let mut c = ok.clone();
        c.sets.push(set("a", &[3, 4]));
        assert!(check_contacts(&c).is_err(), "名前の重なり");
        let mut c = ok.clone();
        c.sets[0].bodies.pop();
        assert!(check_contacts(&c).is_err(), "ボディが 1 つ");
        let mut c = ok.clone();
        c.sets[0].bodies.push(FeatureId(1));
        assert!(check_contacts(&c).is_err(), "同じボディ");
        let mut c = ok;
        c.sets[0].name = " ".into();
        assert!(check_contacts(&c).is_err(), "空の名前");
        // 既定(何も決めていない)は保存しない
        assert!(Contacts::default().is_empty());
    }
}
