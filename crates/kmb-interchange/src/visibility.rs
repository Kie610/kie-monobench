//! 文書の目(ブラウザの行の目・まとまりの目・原点の目の開け閉め)。
//!
//! 目の状態は文書に保存し、開け閉めは文書の変更なので取り消しで戻る。文書には CAD の共通の
//! モジュール(kmb-cad)が名乗る区画 [`Visibility`](`cad.visibility`)として入る。目を替えるのは
//! 操作「表示/非表示」(役割 [`crate::role::action::SET_VISIBILITY`])で、画面・CLI・MCP が同じ操作を
//! 通る。区画は形の計算の鍵に入らないので、目を替えても形は計算し直さない。
//!
//! - 区画の無い文書(目を保存する前の文書)は、後の機能が使っているスケッチと作業平面などを隠し、
//!   ほかを出した状態とみなす([`of`])。読むだけでは文書を変えない。
//! - 機能を足すか直して、スケッチと作業平面などを初めて使った時に、その変更と同じ変更で隠す
//!   ([`hide_first_used`]。変更を取り消せば目も戻る)。ほかの時に目を自動で替えない。
//! - 目を全部開けても区画は外さない(区画の無い文書と分けるため)。
//! - 選択表示と、機能を直す間の一時の見せは画面の状態で、ここには入れない。

use kmb_core::{Context, FeatureId, InputRef, SectionType, category};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// ブラウザのまとまり(目で中を全部隠せる)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Folder {
    /// スケッチ
    Sketches,
    /// ボディ
    Bodies,
    /// 作業平面・作業軸・作業点
    Datums,
}

impl Folder {
    pub const ALL: [Folder; 3] = [Folder::Sketches, Folder::Bodies, Folder::Datums];

    /// 引数と保存に使う名前。
    pub fn key(self) -> &'static str {
        match self {
            Folder::Sketches => "sketches",
            Folder::Bodies => "bodies",
            Folder::Datums => "datums",
        }
    }

    /// 画面と変更の名前に出す名前。
    pub fn label(self) -> &'static str {
        match self {
            Folder::Sketches => "スケッチ",
            Folder::Bodies => "ボディ",
            Folder::Datums => "作業平面など",
        }
    }

    pub fn from_key(key: &str) -> Option<Folder> {
        Folder::ALL.into_iter().find(|f| f.key() == key)
    }
}

/// 文書の目の状態。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Visibility {
    /// 行の目を閉じた機能。ボディは最初に作った機能で持つ(後から機能を足しても隠したまま)
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden: BTreeSet<FeatureId>,
    /// 目を閉じた部品(中のボディを全部隠す)
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden_components: BTreeSet<u64>,
    /// 目を閉じたまとまり。中の行の目はそのまま残す
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden_folders: BTreeSet<Folder>,
    /// 原点の平面をいつも出すか(既定は隠す)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub origin_shown: bool,
    /// このアプリの知らない項目(新しいアプリが足した物)。読んだまま書き戻す
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl SectionType for Visibility {
    const KEY: &'static str = "cad.visibility";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "表示/非表示";

    fn is_empty(&self) -> bool {
        // 目を全部開けた状態も残す(区画の無い文書は、使われているスケッチを隠して始めるため)
        false
    }
}

/// 文書の目の状態。区画が無ければ、後の機能が使っているスケッチと作業平面などを隠した状態。
/// 区画を読めなければログに残して、何も隠さない状態。
pub fn of(cx: &Context<'_>) -> Visibility {
    if !cx.data.sections.contains_key(Visibility::KEY) {
        return Visibility {
            hidden: used_references(cx),
            ..Visibility::default()
        };
    }
    cx.data.section().unwrap_or_else(|e| {
        log::warn!("{e}");
        Visibility::default()
    })
}

/// 機能 `id` が入るブラウザのまとまり(スケッチ・作業平面など・ボディ)。どれでもなければ `None`。
/// 作業平面などは、計算の結果に作業平面・作業軸・作業点を出した機能。
pub fn folder_of(cx: &Context<'_>, id: FeatureId) -> Option<Folder> {
    let f = cx.data.feature(id)?;
    match cx.registry.category(&f.type_id)? {
        category::SKETCH => Some(Folder::Sketches),
        category::SOLID if cx.output(id, crate::DATUM_OUTPUT).is_some() => Some(Folder::Datums),
        category::SOLID => Some(Folder::Bodies),
        _ => None,
    }
}

/// 行の目で持つ機能: ボディの機能は最初に作った機能、ほかはそのまま。
pub fn eye_of(cx: &Context<'_>, id: FeatureId) -> FeatureId {
    match folder_of(cx, id) {
        Some(Folder::Bodies) => kmb_core::body_root(cx.data, cx.registry, id),
        _ => id,
    }
}

impl Visibility {
    /// 部品 `id` か、その親のどれかの目を閉じているか。
    fn component_hidden(&self, cx: &Context<'_>, mut id: u64) -> bool {
        for _ in 0..=cx.data.components.len() {
            if self.hidden_components.contains(&id) {
                return true;
            }
            match cx.data.components.iter().find(|c| c.id == id) {
                Some(c) => match c.parent {
                    Some(p) => id = p,
                    None => return false,
                },
                None => return false,
            }
        }
        false
    }

    /// 機能 `id` を 3D で隠すか(行の目・部品の目・まとまりの目を合わせる)。ボディは、そのボディの
    /// どの機能で聞いてもよい。
    pub fn hides(&self, cx: &Context<'_>, id: FeatureId) -> bool {
        let folder = folder_of(cx, id);
        if folder.is_some_and(|f| self.hidden_folders.contains(&f)) {
            return true;
        }
        if folder != Some(Folder::Bodies) {
            return self.hidden.contains(&id);
        }
        let root = kmb_core::body_root(cx.data, cx.registry, id);
        self.hidden.contains(&root)
            || cx
                .data
                .components
                .iter()
                .filter(|c| c.bodies.contains(&root))
                .any(|c| self.component_hidden(cx, c.id))
    }

    /// 3D で隠す機能の全部(タイムラインの順。ボディは、そのボディの機能を全部入れる)。
    pub fn hidden_features(&self, cx: &Context<'_>) -> Vec<FeatureId> {
        cx.data
            .features
            .iter()
            .map(|f| f.id)
            .filter(|id| self.hides(cx, *id))
            .collect()
    }
}

/// 止めていない後の機能が入力に使っているスケッチと作業平面など。
fn used_references(cx: &Context<'_>) -> BTreeSet<FeatureId> {
    cx.data
        .features
        .iter()
        .filter(|f| !f.suppressed)
        .flat_map(|f| references(cx, f.id))
        .collect()
}

/// 機能 `id` が入力に使っているスケッチと作業平面など(自分は除く)。
pub fn references(cx: &Context<'_>, id: FeatureId) -> Vec<FeatureId> {
    let Some(f) = cx.data.feature(id) else {
        return Vec::new();
    };
    let Some(Ok(feature)) = cx.registry.instantiate(&f.type_id, &f.data) else {
        return Vec::new();
    };
    feature
        .inputs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i {
            InputRef::Output { feature, .. } => Some(feature),
            _ => None,
        })
        .filter(|r| {
            *r != id && matches!(folder_of(cx, *r), Some(Folder::Sketches | Folder::Datums))
        })
        .collect()
}

/// 足すか直した機能 `touched` が初めて使うスケッチと作業平面などを隠した目(使った時に 1 回隠す)。
/// `before` は変更の前に `touched` が使っていた物([`references`])。隠す物が無ければ `None`。
/// ほかの機能がもう使っている物は、利用者が目を開けたかもしれないので隠さない。区画の無い文書
/// (新しい文書と目を保存する前の文書)は、この時に [`of`] の見え方(使われている物を隠す)で区画を
/// 書き始める(区画が無いままだと、使われなくなった物が出し直されてしまう)。作業平面などは前の計算の
/// 結果で見分けるので、同じ変更で足した作業平面は入らない。
pub fn hide_first_used(
    cx: &Context<'_>,
    touched: &[FeatureId],
    before: &BTreeSet<FeatureId>,
) -> Option<Visibility> {
    let fresh: BTreeSet<FeatureId> = touched
        .iter()
        .flat_map(|id| references(cx, *id))
        .filter(|r| !before.contains(r))
        .collect();
    if fresh.is_empty() {
        return None;
    }
    if !cx.data.sections.contains_key(Visibility::KEY) {
        return Some(of(cx));
    }
    let used_by_others: BTreeSet<FeatureId> = cx
        .data
        .features
        .iter()
        .filter(|f| !touched.contains(&f.id))
        .flat_map(|f| references(cx, f.id))
        .collect();
    let mut v = of(cx);
    let hidden = v.hidden.clone();
    v.hidden
        .extend(fresh.into_iter().filter(|r| !used_by_others.contains(r)));
    (v.hidden != hidden).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_reads_back_what_it_wrote_and_keeps_unknown_items() {
        let v: Visibility = serde_json::from_value(serde_json::json!({
            "hidden": [3, 5],
            "hidden_folders": ["sketches"],
            "origin_shown": true,
            "later": 1
        }))
        .unwrap();
        assert_eq!(v.hidden, BTreeSet::from([FeatureId(3), FeatureId(5)]));
        assert_eq!(v.hidden_folders, BTreeSet::from([Folder::Sketches]));
        assert!(v.origin_shown);
        let back = serde_json::to_value(&v).unwrap();
        assert_eq!(back["later"], 1);
        assert!(
            back.get("hidden_components").is_none(),
            "空の項目は書かない"
        );
        for f in Folder::ALL {
            assert_eq!(Folder::from_key(f.key()), Some(f));
            assert_eq!(serde_json::to_value(f).unwrap(), f.key());
        }
    }
}
