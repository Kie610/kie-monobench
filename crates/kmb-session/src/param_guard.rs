//! 表から入る式を、文書へ適用する前に共通の評価で確かめる。
use crate::Headless;
use kmb_core::{
    Document, DocumentData, Edit, FeatureStatus, Op, ParamId, ParamStatus, Problem, ProblemKind,
    RecomputeReport, Registry, SectionType, Transaction,
};
use kmb_interchange::configuration::{self, ConfigColumn, Configurations};
use std::collections::BTreeSet;

fn input(message: impl Into<String>) -> Problem {
    Problem::new(ProblemKind::Input, message)
}

pub(crate) fn snapshot(data: &DocumentData) -> Result<Document, Problem> {
    Document::from_json(&serde_json::to_string(data).expect("文書はJSONにできる"))
        .map_err(|e| input(e.to_string()))
}

fn evaluated(data: &DocumentData, registry: &Registry) -> RecomputeReport {
    let mut data = data.clone();
    for f in &mut data.features {
        f.suppressed = true;
    }
    // 空の控えで実際のカーネルの形を解放しないよう、形を作らないカーネルで評価する。
    kmb_core::recompute_detached(
        &data,
        registry,
        &kmb_core::NullKernel,
        &mut kmb_core::Cache::default(),
    )
}

fn parameter_errors(
    before: &DocumentData,
    after: &DocumentData,
    registry: &Registry,
    targets: &BTreeSet<ParamId>,
    fields: bool,
    shapes: Option<&RecomputeReport>,
) -> Vec<(Option<ParamId>, String)> {
    let old = evaluated(before, registry);
    let checked = evaluated(after, registry);
    let mut errors = Vec::new();
    for p in &checked.params {
        let was_bad = old
            .params
            .iter()
            .any(|q| q.id == p.id && matches!(q.status, ParamStatus::Error { .. }));
        let reason = match &p.status {
            ParamStatus::Error { message } if targets.contains(&p.id) || !was_bad => {
                Some(message.clone())
            }
            ParamStatus::Ok { value } if targets.contains(&p.id) => after
                .params
                .iter()
                .find(|q| q.id == p.id)
                .and_then(|q| q.unit.check(value.dim).err()),
            _ => None,
        };
        if let Some(reason) = reason {
            errors.push((Some(p.id), format!("{}: {reason}", p.name)));
        }
    }
    for p in &checked.model_params {
        let previous = old
            .model_params
            .iter()
            .find(|q| q.feature == p.feature && q.key == p.key);
        let selected = fields && previous.is_none_or(|q| q.expr != p.expr);
        let was_bad = previous.is_some_and(|q| matches!(q.status, ParamStatus::Error { .. }));
        if let ParamStatus::Error { message } = &p.status
            && (selected || !was_bad)
        {
            errors.push((None, format!("{}: {message}", p.name)));
        }
        if selected
            && let (ParamStatus::Ok { value }, Some(shapes)) = (&p.status, shapes)
            && matches!(shapes.feature(p.feature), Some(FeatureStatus::Ok { .. }))
            && let Some(previous) = shapes
                .model_params
                .iter()
                .find(|q| q.feature == p.feature && q.key == p.key)
            && let ParamStatus::Ok { value: was } = &previous.status
            && value.dim != was.dim
        {
            errors.push((None, format!("{}: 寸法の式の単位が合いません", p.name)));
        }
    }
    if fields {
        let previous = crate::params::feature_values(before, registry, &old);
        for row in crate::params::feature_values(after, registry, &checked) {
            if previous
                .iter()
                .find(|q| q.feature == row.feature && q.key == row.key)
                .is_some_and(|q| q.expr == row.expr)
            {
                continue;
            }
            let reason = match &row.status {
                ParamStatus::Error { message } => Some(message.clone()),
                _ => None,
            };
            if let Some(reason) = reason {
                errors.push((None, format!("{}: {reason}", row.label)));
            }
        }
    }
    errors
}

/// 保存済みの構成表の、式を直す必要のあるセル。文書の中身は変えない。
#[derive(Clone, Debug)]
pub struct ConfigurationIssue {
    pub row: String,
    pub param: Option<ParamId>,
    pub message: String,
}

fn row_errors(
    data: &DocumentData,
    registry: &Registry,
    table: &Configurations,
    row: &str,
) -> Vec<ConfigurationIssue> {
    let checked = (|| -> Result<Vec<(Option<ParamId>, String)>, Problem> {
        let mut draft = snapshot(data)?;
        let edits = table.activate_edits(data, row).map_err(input)?;
        draft
            .apply_edits("構成の式を確かめる", edits)
            .map_err(|e| input(e.to_string()))?;
        let targets = table
            .columns
            .iter()
            .filter_map(|c| match c {
                ConfigColumn::Param { param } => Some(*param),
                _ => None,
            })
            .collect();
        Ok(parameter_errors(
            data,
            draft.data(),
            registry,
            &targets,
            false,
            None,
        ))
    })();
    checked
        .unwrap_or_else(|e| vec![(None, e.to_string())])
        .into_iter()
        .map(|(param, message)| ConfigurationIssue {
            row: row.into(),
            param,
            message,
        })
        .collect()
}

fn check_candidate(
    before: &DocumentData,
    after: &DocumentData,
    registry: &Registry,
    mut targets: BTreeSet<ParamId>,
    fields: bool,
    shapes: &RecomputeReport,
) -> Result<(), Problem> {
    for p in &after.params {
        if before
            .params
            .iter()
            .find(|q| q.id == p.id)
            .is_none_or(|q| q.expr != p.expr || q.unit != p.unit)
        {
            targets.insert(p.id);
        }
    }
    let mut errors = if targets.is_empty() && !fields {
        vec![]
    } else {
        parameter_errors(before, after, registry, &targets, fields, Some(shapes))
            .into_iter()
            .map(|(_, e)| e)
            .collect()
    };
    let old = configuration::of(before);
    let table = configuration::of(after);
    for row in &table.rows {
        if old.columns != table.columns
            || old.row(&row.name) != Some(row)
            || old.active != table.active && table.active.as_deref() == Some(&row.name)
        {
            errors.extend(
                row_errors(after, registry, &table, &row.name)
                    .into_iter()
                    .map(|e| format!("構成「{}」の{}", e.row, e.message)),
            );
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(input(errors.join("\n")))
    }
}

fn parameter_target(op: &Op, before: &DocumentData) -> Option<ParamId> {
    match op {
        Op::AddParam { param, .. } => Some(param.id),
        Op::SetParamExpr { id, .. } => Some(*id),
        Op::SetParamInfo { id, unit, .. }
            if before.params.iter().any(|p| p.id == *id && p.unit != *unit) =>
        {
            Some(*id)
        }
        _ => None,
    }
}

impl Headless {
    pub(crate) fn check_param_transaction(&self, tx: &Transaction) -> Result<(), Problem> {
        self.check_table_transaction(tx, false)
    }

    pub(crate) fn check_table_transaction(
        &self,
        tx: &Transaction,
        fields: bool,
    ) -> Result<(), Problem> {
        let relevant = fields
            || tx.ops.iter().any(|op| {
                parameter_target(op, self.doc().data()).is_some()
                    || matches!(op, Op::SetSection { key, .. } if key == Configurations::KEY)
            });
        if !relevant {
            return Ok(());
        }
        let mut draft = snapshot(self.doc().data())?;
        draft.apply(tx.clone()).map_err(|e| input(e.to_string()))?;
        let targets = tx
            .ops
            .iter()
            .filter_map(|op| parameter_target(op, self.doc().data()))
            .collect();
        check_candidate(
            self.doc().data(),
            draft.data(),
            &self.modules().registry,
            targets,
            fields,
            self.report(),
        )
    }

    pub(crate) fn check_param_edits(&self, edits: &[Edit]) -> Result<(), Problem> {
        if !edits.iter().any(|e| {
            matches!(e, Edit::SetParamExpr { .. })
                || matches!(e, Edit::SetSection { key, .. } if key == Configurations::KEY)
        }) {
            return Ok(());
        }
        let mut draft = snapshot(self.doc().data())?;
        draft
            .apply_edits("表の式を確かめる", edits.to_vec())
            .map_err(|e| input(e.to_string()))?;
        let targets = edits
            .iter()
            .filter_map(|e| match e {
                Edit::SetParamExpr { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        check_candidate(
            self.doc().data(),
            draft.data(),
            &self.modules().registry,
            targets,
            false,
            self.report(),
        )
    }

    pub(crate) fn check_parameter_data(
        &self,
        after: &DocumentData,
        fields: bool,
    ) -> Result<(), Problem> {
        check_candidate(
            self.doc().data(),
            after,
            &self.modules().registry,
            BTreeSet::new(),
            fields,
            self.report(),
        )
    }

    pub(crate) fn check_model_expression(&self, name: &str, expr: &str) -> Result<(), Problem> {
        let mut draft = snapshot(self.doc().data())?;
        draft
            .set_model_param(&self.modules().registry, name, expr)
            .map_err(|e| input(e.to_string()))?;
        check_candidate(
            self.doc().data(),
            draft.data(),
            &self.modules().registry,
            BTreeSet::new(),
            true,
            self.report(),
        )
    }

    /// 保存した構成表の誤りを読む。修正や保存・再計算は行わない。
    pub fn configuration_issues(&self) -> Vec<ConfigurationIssue> {
        let table = configuration::of(self.doc().data());
        table
            .rows
            .iter()
            .flat_map(|r| row_errors(self.doc().data(), &self.modules().registry, &table, &r.name))
            .collect()
    }
}
