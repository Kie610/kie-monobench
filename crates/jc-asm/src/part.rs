//! 部品(ジョイントで動かす・土台にする物)と、ボディの受け継ぎ。
//!
//! 部品は文書の部品の木の 1 つ(中の部品ごと)か、部品に入っていないボディ 1 つ。ボディは
//! 最初に作った機能(`root`)で指し、ジョイントが読む時はその位置までの最後の機能(受け継いだ
//! 機能)に直す。

use jc_core::{
    DocumentData, FeatureId, FeatureStatus, InputRef, RecomputeReport, Registry, Value, body_root,
};
use serde::{Deserialize, Serialize};

/// ジョイントで動かす・土台にする物。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Part {
    /// 部品の木の部品(中の部品も一緒)
    Component { id: u64 },
    /// 部品に入っていないボディ(最初に作った機能)
    Body { root: FeatureId },
}

impl Part {
    /// ボディ `root` を含む部品: 入っている部品(すぐ外側の物)か、そのボディだけ。
    pub fn of_body(data: &DocumentData, root: FeatureId) -> Part {
        match data.component_of(root) {
            Some(c) => Part::Component { id: c.id },
            None => Part::Body { root },
        }
    }

    /// 中のボディすべて(最初に作った機能)。
    pub fn roots(&self, data: &DocumentData) -> Vec<FeatureId> {
        match self {
            Part::Component { id } => data.component_bodies(*id),
            Part::Body { root } => vec![*root],
        }
    }

    /// 画面に出す名前(部品の名前か、ボディの名前)。
    pub fn label(
        &self,
        data: &DocumentData,
        registry: &Registry,
        report: &RecomputeReport,
    ) -> String {
        match self {
            Part::Component { id } => data
                .component(*id)
                .map_or_else(|| format!("部品 {id}"), |c| c.name.clone()),
            Part::Body { root } => body_label(data, registry, report, *root),
        }
    }
}

/// ボディ `root` の名前(付けた名前か「ボディN」)。
pub fn body_label(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    root: FeatureId,
) -> String {
    let bodies: Vec<FeatureId> = jc_core::visible_bodies(data, registry, report)
        .into_iter()
        .map(|b| b.0)
        .collect();
    let roots: Vec<FeatureId> = bodies
        .iter()
        .map(|b| body_root(data, registry, *b))
        .collect();
    match roots.iter().position(|r| *r == root) {
        Some(i) => jc_core::body_labels(data, registry, &bodies)
            .into_iter()
            .nth(i)
            .unwrap_or_default(),
        None => data
            .body_names
            .get(&root)
            .cloned()
            .unwrap_or_else(|| format!("ボディ({root})")),
    }
}

/// ボディ `root` を、機能 `until` の手前(`None` なら最後)まで受け継いだ機能。後の機能が `body` を
/// 読んで自分も `body` を出すたびにその機能へ移る([`jc_core::body_heir`] の、途中で止める物)。
pub fn heir_before(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    root: FeatureId,
    until: Option<FeatureId>,
) -> FeatureId {
    let mut cur = root;
    for r in report
        .features
        .iter()
        .skip_while(|r| r.id != root)
        .skip(1)
        .take_while(|r| until.is_none_or(|u| r.id != u))
    {
        let Some(entry) = data.feature(r.id) else {
            continue;
        };
        let Some(Ok(f)) = registry.instantiate(&entry.type_id, &entry.data) else {
            continue;
        };
        let reads = |i: &InputRef, body_only: bool| matches!(i, InputRef::Output { feature, slot } if *feature == cur && (!body_only || slot == "body"));
        let takes = match &r.status {
            _ if f.keeps().contains(&cur) => false,
            FeatureStatus::Suppressed => f.pass_through().iter().any(|(_, i)| reads(i, false)),
            FeatureStatus::Ok { outputs, .. }
                if matches!(outputs.get("body"), Some(Value::Shape(_))) =>
            {
                f.inputs()
                    .unwrap_or_default()
                    .iter()
                    .any(|i| reads(i, true))
            }
            _ => false,
        };
        if takes {
            cur = r.id;
        }
    }
    cur
}
