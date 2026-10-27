//! ボディの長く使える ID と、ボディの形の移り変わりの表(系譜の表)。
//!
//! 機能のデータが持つ機能の番号(入力の `body`・`part:n`、選ぶ欄の参照の機能)は「そのボディ」を
//! 指す印と読み、再計算が「読む機能の手前での、そのボディの最新の形」に解く。前の機能の後ろに
//! 機能を差し込むと、後ろの機能はそれを読む。表は再計算の派生物で、文書と登録だけから作る
//! (カーネルは使わない)。保存の形の版 5 までの機能には、データの番号の形をそのまま読む印
//! ([`crate::FeatureEntry::pin_bodies`])が付く。

use crate::document::DocumentData;
use crate::feature::InputRef;
use crate::{FeatureId, Registry};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};
use std::fmt;

/// ボディの長く使える ID。ボディを最初に作った機能(根)と、その機能の中の番号
/// (0 は出力 `body`、n ≥ 1 は出力 `part:n`)。後から機能を足しても変わらない。
///
/// JSON は番号が 0 なら数(`3`)、ほかは文字(`"3.2"`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BodyId {
    pub root: FeatureId,
    pub part: u32,
}

impl BodyId {
    /// 機能 `root` の出力 `body` のボディ。
    pub fn new(root: FeatureId) -> Self {
        BodyId { root, part: 0 }
    }

    /// ボディを入れる出力の名前(`body`・`part:n`)なら、その番号。
    pub fn part_of_slot(slot: &str) -> Option<u32> {
        if slot == "body" {
            return Some(0);
        }
        slot.strip_prefix("part:")?
            .parse::<u32>()
            .ok()
            .filter(|n| *n >= 1)
    }
}

impl fmt::Display for BodyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.part {
            0 => write!(f, "{}", self.root.0),
            n => write!(f, "{}.{n}", self.root.0),
        }
    }
}

impl std::str::FromStr for BodyId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let bad = || format!("ボディの ID ではありません: {s}");
        let (root, part) = match s.split_once('.') {
            Some((r, p)) => (r, p.parse::<u32>().map_err(|_| bad())?),
            None => (s, 0),
        };
        let root = root.parse::<u64>().map_err(|_| bad())?;
        Ok(BodyId {
            root: FeatureId(root),
            part,
        })
    }
}

impl Serialize for BodyId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.part {
            0 => s.serialize_u64(self.root.0),
            _ => s.serialize_str(&self.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for BodyId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Num(u64),
            Text(String),
        }
        match Raw::deserialize(d)? {
            Raw::Num(n) => Ok(BodyId::new(FeatureId(n))),
            Raw::Text(t) => t.parse().map_err(serde::de::Error::custom),
        }
    }
}

/// 機能と出力の名前の組(ボディの形の 1 つ)。
pub type BodyState = (FeatureId, String);

/// 1 つのボディの形の移り変わり。
#[derive(Debug, Clone, PartialEq)]
pub struct BodyLine {
    pub id: BodyId,
    /// そのボディの形を出した機能と出力の名前(タイムラインの順。抑制してそのまま渡した機能も入る)
    pub states: Vec<BodyState>,
    /// ほかのボディに足された(結合・切り取りの道具に使われた)機能と、その先のボディ
    pub merged: Option<(FeatureId, BodyId)>,
}

/// ボディの系譜の表。再計算の最初に文書と登録だけから作り、[`crate::RecomputeReport::bodies`] に残す。
#[derive(Debug, Clone, Default)]
pub struct BodyTable {
    lines: BTreeMap<BodyId, BodyLine>,
    of: HashMap<BodyState, BodyId>,
    position: HashMap<FeatureId, usize>,
    /// 機能ごとの、読むボディの付け替え(データの形 → 読む形。違う物だけ)
    redirect: HashMap<FeatureId, BTreeMap<BodyState, BodyState>>,
    /// 固定の印のため、最新と違う形を読んでいる機能(データの形, 最新の形)
    stale: BTreeMap<FeatureId, Vec<(BodyState, BodyState)>>,
    /// 付け替えの無い機能に渡す空の表
    none: BTreeMap<BodyState, BodyState>,
}

/// 入力がボディの出力(`body`・`part:n`)なら、その形。
fn body_input(input: &InputRef) -> Option<BodyState> {
    match input {
        InputRef::Output { feature, slot } if BodyId::part_of_slot(slot).is_some() => {
            Some((*feature, slot.clone()))
        }
        _ => None,
    }
}

impl BodyTable {
    /// 文書 `data` の表を作る。タイムラインの順に、機能が読むボディを「その手前の最新の形」に解き
    /// (固定の印の付いた機能はデータの形のまま)、ボディを受け継ぐ機能をそのボディの形として足す。
    ///
    /// ボディを受け継ぐのは、抑制した時に後ろへそのまま渡す `body`([`crate::Feature::pass_through`])
    /// の元(元を残す `keeps` の物を除く)。抑制していなければ、その機能が読むほかのボディ(`keeps` を
    /// 除く)は、その機能のボディに足されたことにする(結合・切り取りの道具)。
    pub fn build(data: &DocumentData, registry: &Registry) -> Self {
        let mut t = BodyTable::default();
        for (i, e) in data.features.iter().enumerate() {
            t.position.insert(e.id, i);
            let Some(Ok(f)) = registry.instantiate(&e.type_id, &e.data) else {
                continue;
            };
            let raw: Vec<InputRef> = if e.suppressed {
                f.pass_through()
                    .into_iter()
                    .filter(|(out, _)| out == "body")
                    .map(|(_, from)| from)
                    .collect()
            } else {
                f.inputs().unwrap_or_default()
            };
            let reads: Vec<BodyState> = raw.iter().filter_map(body_input).collect();
            let mut map = BTreeMap::new();
            for s in &reads {
                let Some(tip) = t.tip_before_state(s, i) else {
                    continue;
                };
                if &tip == s {
                    continue;
                }
                if e.pin_bodies {
                    t.stale.entry(e.id).or_default().push((s.clone(), tip));
                } else {
                    map.insert(s.clone(), tip);
                }
            }
            let resolved = |s: &BodyState| map.get(s).cloned().unwrap_or_else(|| s.clone());
            // 受け継ぐボディは、抑制した時に後ろへ渡す `body` の元(その機能が変えるボディの宣言)
            let inherit = f
                .pass_through()
                .into_iter()
                .find(|(out, _)| out == "body")
                .and_then(|(_, from)| body_input(&from));
            let keeps = f.keeps();
            let kept = |s: &BodyState| keeps.contains(&s.0);
            if let Some(from) = inherit.filter(|s| !kept(s)) {
                let body = t.line_of(&resolved(&from));
                t.push(body, (e.id, "body".into()));
                if !e.suppressed {
                    for tool in reads.iter().filter(|s| **s != from && !kept(s)) {
                        let tool = t.line_of(&resolved(tool));
                        if tool != body
                            && let Some(line) = t.lines.get_mut(&tool)
                            && line.merged.is_none()
                        {
                            line.merged = Some((e.id, body));
                        }
                    }
                }
            }
            if !map.is_empty() {
                t.redirect.insert(e.id, map);
            }
        }
        t
    }

    /// 形 `s` のボディ。表に無ければ、`s` を最初の形とするボディを作る。
    fn line_of(&mut self, s: &BodyState) -> BodyId {
        if let Some(b) = self.of.get(s) {
            return *b;
        }
        let id = BodyId {
            root: s.0,
            part: BodyId::part_of_slot(&s.1).unwrap_or(0),
        };
        self.lines.entry(id).or_insert_with(|| BodyLine {
            id,
            states: vec![s.clone()],
            merged: None,
        });
        self.of.insert(s.clone(), id);
        id
    }

    fn push(&mut self, body: BodyId, s: BodyState) {
        if let Some(line) = self.lines.get_mut(&body) {
            line.states.push(s.clone());
        }
        self.of.insert(s, body);
    }

    fn pos(&self, id: FeatureId) -> usize {
        self.position.get(&id).copied().unwrap_or(usize::MAX)
    }

    /// 形 `s` のボディの、タイムラインの位置 `index` より前での最新の形。表に無い形は `None`。
    fn tip_before_state(&self, s: &BodyState, index: usize) -> Option<BodyState> {
        let body = *self.of.get(s)?;
        self.tip_at(body, index)
    }

    fn tip_at(&self, mut body: BodyId, index: usize) -> Option<BodyState> {
        // 足された先へたどる(輪にはならないが、数で区切る)
        for _ in 0..=self.lines.len() {
            let line = self.lines.get(&body)?;
            match line.merged {
                Some((by, into)) if self.pos(by) < index => body = into,
                _ => {
                    return line
                        .states
                        .iter()
                        .rev()
                        .find(|(f, _)| self.pos(*f) < index)
                        .cloned();
                }
            }
        }
        None
    }

    /// 機能 `feature` の出力 `body` が属するボディ。
    pub fn body_of(&self, feature: FeatureId) -> Option<BodyId> {
        self.of.get(&(feature, "body".to_string())).copied()
    }

    /// ボディ `body` の移り変わり。
    pub fn line(&self, body: BodyId) -> Option<&BodyLine> {
        self.lines.get(&body)
    }

    /// 機能 `reader` の手前での、ボディ `body` の最新の形(足された先へもたどる)。
    pub fn tip_before(&self, body: BodyId, reader: FeatureId) -> Option<BodyState> {
        self.tip_at(body, self.pos(reader))
    }

    /// タイムラインの最後での、ボディ `body` の最新の形。
    pub fn tip(&self, body: BodyId) -> Option<BodyState> {
        self.tip_at(body, usize::MAX)
    }

    /// 機能 `reader` が読むボディの付け替え(データの形 → 読む形。違う物だけ)。
    pub fn redirects(&self, reader: FeatureId) -> &BTreeMap<BodyState, BodyState> {
        self.redirect.get(&reader).unwrap_or(&self.none)
    }

    /// 機能 `reader` が入力 `input` として読む物(ボディなら解いた形、ほかはそのまま)。
    pub fn resolve(&self, reader: FeatureId, input: &InputRef) -> InputRef {
        match body_input(input).and_then(|s| self.redirects(reader).get(&s)) {
            Some((feature, slot)) => InputRef::Output {
                feature: *feature,
                slot: slot.clone(),
            },
            None => input.clone(),
        }
    }

    /// 固定の印が付き、最新と違う形を読んでいる機能と、その(データの形, 最新の形)。
    pub fn stale(&self) -> &BTreeMap<FeatureId, Vec<(BodyState, BodyState)>> {
        &self.stale
    }
}
