//! パラメータの表の式をまとめて入れる共通の口。
use crate::Headless;
use kmb_core::{
    Action, Arg, Context, Document, DocumentData, Edit, Extension, FieldUnit, Outcome, ParamKey,
    ParamStatus, ParamUnit, Problem, ProblemKind, RecomputeReport, Registry,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const APPLY: &str = "params.apply_batch";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub row: ParamKey,
    pub expr: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub row: ParamKey,
    pub label: String,
    pub reason: String,
}

impl Headless {
    /// 表の式を、全件が通る時だけ一度に入れる。操作の引数の宣言も通す。
    pub fn set_param_batch(&mut self, changes: &[Change]) -> Result<(), Problem> {
        let action = self
            .action_for_role(&kmb_interchange::role::action::APPLY_PARAM_BATCH)
            .map_err(|why| Problem::new(ProblemKind::Unavailable, why))?;
        self.run(
            &action,
            None,
            None,
            &serde_json::json!({"changes": changes}),
        )?;
        Ok(())
    }

    /// 候補の式だけを検査する。文書・履歴・形状・再計算の回数は変えない。
    pub fn check_param_batch(&self, changes: &[Change]) -> Vec<Failure> {
        prepare(
            self.doc().data(),
            &self.modules().registry,
            self.report(),
            changes,
        )
        .err()
        .unwrap_or_default()
    }
}

pub(crate) struct Commands;

impl Extension for Commands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        vec![
            Action::new(APPLY, "式をまとめて適用", "cad")
                .args([Arg::list(
                    "changes",
                    "変更する行と式の並び。各項目は row(パラメータの表の行のIDと鍵)と expr(式)",
                )
                .required()])
                .role(kmb_interchange::role::action::APPLY_PARAM_BATCH)
                .hint("パラメータの表の複数の式を確かめ、すべて通れば1回の変更として適用する")
                .icon("param")
                .at(&[]),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        if id != APPLY {
            return None;
        }
        Some((|| {
            let given = cx.arg("changes").ok_or("changes を指定してください")?;
            let changes: Vec<Change> = serde_json::from_value(given.clone())
                .map_err(|e| format!("行と式の並びを読めません: {e}"))?;
            // 行の型が知らない欄を受け流さないよう、宣言した形との一致も確かめる。
            if serde_json::to_value(&changes).expect("行と式はJSONにできる") != *given {
                return Err(Problem::new(
                    ProblemKind::Input,
                    "行と式の指定に知らない欄があります",
                ));
            }
            let edits = prepare(cx.data, cx.registry, cx.report, &changes).map_err(|errors| {
                Problem::new(
                    ProblemKind::Input,
                    errors
                        .iter()
                        .map(|e| format!("{}: {}", e.label, e.reason))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            })?;
            if edits.is_empty() {
                Ok(Outcome::Message("式の変更はありません".into()))
            } else {
                Ok(Outcome::Change {
                    label: "式をまとめて適用".into(),
                    edits,
                })
            }
        })())
    }
}

fn prepare(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    changes: &[Change],
) -> Result<Vec<Edit>, Vec<Failure>> {
    if changes.is_empty() {
        return Ok(vec![]);
    }
    let failure = |row: &ParamKey, reason: String| Failure {
        row: row.clone(),
        label: crate::params::key_label(data, registry, report, row)
            .unwrap_or_else(|| crate::params::key_text(report, data, row)),
        reason,
    };
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    let mut draft = Document::from_json(&serde_json::to_string(data).expect("文書はJSONにできる"))
        .map_err(|e| vec![failure(&changes[0].row, e.to_string())])?;
    let mut rows = Vec::new();
    for change in changes {
        if !seen.insert(change.row.clone()) {
            errors.push(failure(&change.row, "同じ行が重なっています".into()));
            continue;
        }
        let Some(mut row) = crate::param_io::rows(
            draft.data(),
            registry,
            report,
            std::slice::from_ref(&change.row),
        )
        .pop() else {
            errors.push(failure(&change.row, "この行は文書にありません".into()));
            continue;
        };
        row.expr = completed(data, registry, report, &change.row, &change.expr);
        if let Err(why) = crate::param_io::checked_row(draft.data(), &row) {
            errors.push(failure(&change.row, why));
        }
        rows.push(row);
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    // 同じ機能の式も一度に重ね、一時の文書に行ごとの履歴をためない。
    let (tx, result) = crate::param_io::import(&mut draft, registry, report, &rows);
    if !result.skipped.is_empty() {
        return Err(changes
            .iter()
            .map(|change| failure(&change.row, result.skipped.join("\n")))
            .collect());
    }
    if let Some(tx) = tx
        && let Err(why) = draft.apply(tx)
    {
        return Err(changes
            .iter()
            .map(|change| failure(&change.row, why.to_string()))
            .collect());
    }

    // 形を計算せず、文字・単位・循環を含む共通のパラメータの評価だけを使う。
    // 実際のカーネルへ空のキャッシュを渡すと保持中の形を捨てるため、形を作らないカーネルを使う。
    let mut only_params = draft.data().clone();
    for f in &mut only_params.features {
        f.suppressed = true;
    }
    let checked = kmb_core::recompute_detached(
        &only_params,
        registry,
        &kmb_core::NullKernel,
        &mut kmb_core::Cache::default(),
    );
    for p in &checked.params {
        let key = ParamKey::User { id: p.id };
        let old_error = report
            .params
            .iter()
            .any(|old| old.id == p.id && matches!(old.status, ParamStatus::Error { .. }));
        if let ParamStatus::Error { message } = &p.status
            && (seen.contains(&key) || !old_error)
        {
            errors.push(failure(&key, message.clone()));
        }
    }
    for p in &checked.model_params {
        let key = ParamKey::Model {
            feature: p.feature,
            key: if p.key.is_empty() {
                p.name.clone()
            } else {
                p.key.clone()
            },
        };
        let old = report
            .model_params
            .iter()
            .find(|old| old.feature == p.feature && old.name == p.name);
        let reason = match (&p.status, old.map(|p| &p.status)) {
            (ParamStatus::Error { message }, old)
                if seen.contains(&key) || !matches!(old, Some(ParamStatus::Error { .. })) =>
            {
                Some(message.clone())
            }
            (ParamStatus::Ok { value }, Some(ParamStatus::Ok { value: previous }))
                if seen.contains(&key)
                    && value.dim != previous.dim
                    && matches!(
                        report.feature(p.feature),
                        Some(kmb_core::FeatureStatus::Ok { .. })
                    ) =>
            {
                Some("寸法の式の単位が合いません".into())
            }
            _ => None,
        };
        if let Some(why) = reason {
            errors.push(failure(&key, why));
        }
    }
    for value in crate::params::feature_values(draft.data(), registry, &checked) {
        let key = ParamKey::Field {
            feature: value.feature,
            key: value.key.clone(),
        };
        if !seen.contains(&key) {
            continue;
        }
        let why = match value.status {
            ParamStatus::Error { message } => Some(message),
            ParamStatus::Ok { value: q } => {
                let unit = draft
                    .data()
                    .feature(value.feature)
                    .and_then(|f| registry.descriptor(&f.type_id))
                    .and_then(|d| d.field_at(&value.key))
                    .and_then(|f| f.unit);
                let unit = match unit {
                    Some(FieldUnit::Length) => Some(ParamUnit::Length),
                    Some(FieldUnit::Angle) => Some(ParamUnit::Angle),
                    Some(FieldUnit::Plain(_) | FieldUnit::Ratio | FieldUnit::Unitless) => {
                        Some(ParamUnit::Unitless)
                    }
                    _ => None,
                };
                unit.and_then(|u| u.check(q.dim).err())
            }
            ParamStatus::Text { .. } => Some("この欄は数の式で指定します".into()),
        };
        if let Some(why) = why {
            errors.push(failure(&key, why));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut edits = Vec::new();
    for p in &draft.data().params {
        if data
            .params
            .iter()
            .any(|old| old.id == p.id && old.expr != p.expr)
        {
            edits.push(Edit::SetParamExpr {
                id: p.id,
                expr: p.expr.clone(),
            });
        }
    }
    for f in &draft.data().features {
        if data.feature(f.id).is_some_and(|old| old.data != f.data) {
            edits.push(Edit::SetData {
                id: f.id,
                data: f.data.clone(),
            });
        }
    }
    Ok(edits)
}

fn completed(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    key: &ParamKey,
    expr: &str,
) -> String {
    let unit = match key {
        ParamKey::User { id } => {
            data.params
                .iter()
                .find(|p| p.id == *id)
                .and_then(|p| match p.unit {
                    ParamUnit::Length => Some(FieldUnit::Length),
                    ParamUnit::Angle => Some(FieldUnit::Angle),
                    _ => None,
                })
        }
        ParamKey::Field { feature, key } => data
            .feature(*feature)
            .and_then(|f| registry.descriptor(&f.type_id))
            .and_then(|d| d.field_at(key))
            .and_then(|f| f.unit),
        ParamKey::Model { .. } => None,
    };
    if let Some(unit) = unit {
        return crate::params::typed_expr(Some(&unit), expr, data.properties.units);
    }
    let status = match key {
        ParamKey::Model { feature, key } => report
            .model_params
            .iter()
            .find(|p| {
                p.feature == *feature && (p.key == *key || p.key.is_empty() && p.name == *key)
            })
            .map(|p| &p.status),
        ParamKey::Field { feature, key } => {
            return crate::params::feature_values(data, registry, report)
                .into_iter()
                .find(|v| v.feature == *feature && v.key == *key)
                .map_or_else(
                    || expr.trim().to_string(),
                    |v| crate::params::with_unit_of_value(expr, &v.status, data.properties.units),
                );
        }
        _ => None,
    };
    status.map_or_else(
        || expr.trim().to_string(),
        |s| crate::params::with_unit_of_value(expr, s, data.properties.units),
    )
}

#[cfg(test)]
#[path = "param_batch_tests.rs"]
mod tests;
