//! 数の欄と寸法で使う「名前=式」の解釈と、取り消せる変更への組み込み。

use kmb_core::{
    Document, DocumentData, Field, FieldKind, FieldUnit, LengthUnit, Op, Param, Quantity,
    RecomputeReport, Registry, Transaction,
};
use std::collections::HashSet;

/// 名前付きの式を受け付けられない理由。画面は種類から現在の言語の文言を選ぶ。
#[derive(Debug, Clone, PartialEq)]
pub enum NamedParamError {
    Syntax,
    InvalidName(String),
    Duplicate { name: String, dimension: bool },
    Expression { expr: String, reason: String },
    UnitMismatch { length: bool },
}

impl std::fmt::Display for NamedParamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax => write!(f, "名前と式を「名前=式」の形で1つずつ指定してください"),
            Self::InvalidName(n) => write!(f, "「{n}」はパラメータの名前に使えません"),
            Self::Duplicate { name, dimension } => write!(
                f,
                "{}「{name}」は既にあります(値に {name} とだけ打てば使えます)",
                if *dimension {
                    "寸法の名前"
                } else {
                    "パラメータ"
                }
            ),
            Self::Expression { expr, reason } => write!(f, "{expr} を計算できません: {reason}"),
            Self::UnitMismatch { length: true } => write!(f, "長さの欄に角度を書いています"),
            Self::UnitMismatch { length: false } => write!(f, "角度の欄に長さを書いています"),
        }
    }
}
impl std::error::Error for NamedParamError {}

/// 欄と同じ単位で計算できた、新しいパラメータ。
#[derive(Debug, Clone)]
pub struct NamedParameter {
    pub name: String,
    pub expr: String,
    pub value: Quantity,
}

/// `=` が無ければ通常の式。あれば名前・重複・式を確認して、数だけの右辺に欄の単位を補う。
pub fn parse_named_parameter(
    data: &DocumentData,
    report: &RecomputeReport,
    typed: &str,
    unit: Option<&FieldUnit>,
    length: LengthUnit,
) -> Result<Option<NamedParameter>, NamedParamError> {
    let Some((name, expr)) = typed.split_once('=') else {
        return Ok(None);
    };
    let (name, expr) = (name.trim(), expr.trim());
    if name.is_empty() || expr.is_empty() || expr.contains('=') {
        return Err(NamedParamError::Syntax);
    }
    if !kmb_core::is_valid_name(name) {
        return Err(NamedParamError::InvalidName(name.into()));
    }
    if data.param_by_name(name).is_some() {
        return Err(NamedParamError::Duplicate {
            name: name.into(),
            dimension: false,
        });
    }
    if report.model_params.iter().any(|p| p.name == name) {
        return Err(NamedParamError::Duplicate {
            name: name.into(),
            dimension: true,
        });
    }
    let expr = super::typed_expr(unit, expr, length);
    let values = report.param_values();
    let value = kmb_core::eval_str(&expr, &|n| values.get(n).copied()).map_err(|e| {
        NamedParamError::Expression {
            expr: expr.clone(),
            reason: e.to_string(),
        }
    })?;
    if super::unit_mismatch(unit, value.dim).is_some() {
        return Err(NamedParamError::UnitMismatch {
            length: matches!(unit, Some(FieldUnit::Length)),
        });
    }
    Ok(Some(NamedParameter {
        name: name.into(),
        expr,
        value,
    }))
}

struct Assignment {
    op: usize,
    field: Field,
    param: NamedParameter,
}

fn assignments(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    tx: &Transaction,
) -> Result<Vec<Assignment>, NamedParamError> {
    let mut out = Vec::new();
    let mut names: HashSet<String> = tx
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::AddParam { param, .. } => Some(param.name.clone()),
            _ => None,
        })
        .collect();
    for (op, change) in tx.ops.iter().enumerate() {
        let Op::SetFeatureData { id, data: new } = change else {
            continue;
        };
        let Some(old) = data.feature(*id) else {
            continue;
        };
        let Some(desc) = registry.descriptor(&old.type_id) else {
            continue;
        };
        let mut fields = Vec::new();
        for f in &desc.fields {
            match &f.kind {
                FieldKind::Expr { .. } => fields.push(f.clone()),
                FieldKind::Rows { fields: sub } => {
                    for row in 0..f.row_count(new) {
                        fields.extend(
                            sub.iter()
                                .filter(|s| matches!(s.kind, FieldKind::Expr { .. }))
                                .map(|s| f.row_field(row, s)),
                        );
                    }
                }
                _ => {}
            }
        }
        for field in fields {
            if !field.applies(new) || field.get(new) == field.get(&old.data) {
                continue;
            }
            let Some(typed) = field.get(new).and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(param) = parse_named_parameter(
                data,
                report,
                typed,
                field.unit.as_ref(),
                data.properties.units,
            )?
            else {
                continue;
            };
            if !names.insert(param.name.clone()) {
                return Err(NamedParamError::Duplicate {
                    name: param.name,
                    dimension: false,
                });
            }
            out.push(Assignment { op, field, param });
        }
    }
    Ok(out)
}

/// 画面で変更を確定する前の確認。変更の組み込みと同じ判定を使い、文書は変えない。
pub fn validate_named_parameter_change(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    tx: &Transaction,
) -> Result<(), NamedParamError> {
    assignments(data, registry, report, tx).map(|_| ())
}

/// すべて確認してから番号を割り当て、パラメータと欄を同じ取り消し単位にする。
pub(crate) fn complete_named_parameter_change(
    doc: &mut Document,
    registry: &Registry,
    report: &RecomputeReport,
    mut tx: Transaction,
) -> Result<Transaction, NamedParamError> {
    let found = assignments(doc.data(), registry, report, &tx)?;
    let mut params = Vec::new();
    for assignment in found {
        let Op::SetFeatureData { data, .. } = &mut tx.ops[assignment.op] else {
            unreachable!()
        };
        assignment.field.set(
            data,
            Some(serde_json::Value::String(assignment.param.name.clone())),
        );
        let id = doc.alloc_param_id();
        params.push(Op::AddParam {
            index: None,
            param: Param::new(id, assignment.param.name, assignment.param.expr),
        });
    }
    params.append(&mut tx.ops);
    tx.ops = params;
    Ok(tx)
}
