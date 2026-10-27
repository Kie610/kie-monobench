//! パターンの式。寸法と同じ名前の解決を通り、形は評価した数値から作る。

use crate::names::DimExpr;
use crate::{ConstraintId, PatternAxis, PatternDef, Sketch};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// 矩形の 1 方向の式。向きが線なら angle は持たない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectionExpressions {
    pub count: String,
    pub distance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
}

/// 保存する式。数値の定義は最後に編集した時の形として持つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatternExpressions {
    Circular {
        count: String,
        angle: String,
    },
    Rectangular {
        directions: [DirectionExpressions; 2],
    },
}

impl PatternExpressions {
    /// 数値の定義を単位つきの式にする。
    pub fn from_def(def: &PatternDef) -> Self {
        match def {
            PatternDef::Circular(c) => Self::Circular {
                count: c.count.to_string(),
                angle: format!("{} deg", c.angle.to_degrees()),
            },
            PatternDef::Rectangular(r) => Self::Rectangular {
                directions: r.directions.map(|d| DirectionExpressions {
                    count: d.count.to_string(),
                    distance: format!("{} mm", d.distance),
                    angle: match d.axis {
                        PatternAxis::Angle(a) => Some(format!("{} deg", a.to_degrees())),
                        PatternAxis::Line(_) => None,
                    },
                }),
            },
        }
    }

    pub(crate) fn strings_mut(&mut self) -> Vec<&mut String> {
        match self {
            Self::Circular { count, angle } => vec![count, angle],
            Self::Rectangular { directions } => directions
                .iter_mut()
                .flat_map(|d| {
                    let mut v = vec![&mut d.count, &mut d.distance];
                    v.extend(d.angle.as_mut());
                    v
                })
                .collect(),
        }
    }

    pub(crate) fn fields(&self) -> Vec<DimExpr<'_>> {
        let field = |expr, angle, count_max, signed| DimExpr {
            id: ConstraintId(0),
            name: None,
            expr,
            angle,
            count_max,
            signed,
        };
        match self {
            Self::Circular { count, angle } => vec![
                field(count.as_str(), false, Some(360), false),
                field(angle.as_str(), true, None, true),
            ],
            Self::Rectangular { directions } => directions
                .iter()
                .flat_map(|d| {
                    let mut v = vec![
                        field(d.count.as_str(), false, Some(200), false),
                        field(d.distance.as_str(), false, None, true),
                    ];
                    if let Some(a) = &d.angle {
                        v.push(field(a.as_str(), true, None, true));
                    }
                    v
                })
                .collect(),
        }
    }
}

impl Sketch {
    /// 式だけの値には、保存する拘束と重ならない一時的な番号を振る。
    pub(crate) fn pattern_exprs(&self) -> Result<Vec<DimExpr<'_>>, String> {
        let mut next = self.constraints.iter().map(|c| c.id.0).max().unwrap_or(0);
        let mut out = Vec::new();
        for p in &self.patterns {
            if let Some(e) = &p.expressions {
                for mut f in e.fields() {
                    next = next
                        .checked_add(1)
                        .ok_or("パターンの式の番号が上限を超えました")?;
                    f.id = ConstraintId(next);
                    out.push(f);
                }
            }
        }
        Ok(out)
    }

    /// 評価した式で写しの数と配置を決める。残る写しの要素番号は保つ。
    /// 消える写しへの参照があれば、黙って落とさず理由を返す。
    pub fn evaluated_patterns(
        &self,
        values: &HashMap<ConstraintId, f64>,
    ) -> Result<Sketch, String> {
        let fields = self.pattern_exprs()?;
        let mut values = fields.iter().map(|f| {
            values
                .get(&f.id)
                .copied()
                .ok_or_else(|| format!("パターンの式 `{}` の値がありません", f.expr))
        });
        let mut s = self.clone();
        for at in 0..s.patterns.len() {
            let p = &s.patterns[at];
            let Some(exprs) = &p.expressions else {
                continue;
            };
            let mut def = p.def.clone();
            let mut take = || values.next().ok_or("パターンの式が不足しています")?;
            match (exprs, &mut def) {
                (PatternExpressions::Circular { .. }, PatternDef::Circular(c)) => {
                    c.count = take()? as usize;
                    c.angle = take()?;
                }
                (PatternExpressions::Rectangular { directions }, PatternDef::Rectangular(r)) => {
                    for (e, d) in directions.iter().zip(&mut r.directions) {
                        d.count = take()? as usize;
                        d.distance = take()?;
                        match (&e.angle, &mut d.axis) {
                            (Some(_), PatternAxis::Angle(a)) => *a = take()?,
                            (None, PatternAxis::Line(_)) => {}
                            _ => return Err("パターンの向きの式と向きの種類が合いません".into()),
                        }
                    }
                }
                _ => return Err("パターンの式と種類が合いません".into()),
            }
            let layout = s
                .layout(&def)
                .ok_or("この値ではパターンを作れません(角度・距離や参照を確かめてください)")?;
            let live: BTreeSet<_> = layout.iter().map(|(i, _)| *i).collect();
            let gone: BTreeSet<_> = s.patterns[at]
                .copies
                .iter()
                .filter(|c| !live.contains(&c.index))
                .flat_map(|c| c.entities.iter().copied())
                .collect();
            if s.constraints
                .iter()
                .any(|c| crate::edit::refs(&c.kind).iter().any(|e| gone.contains(e)))
                || s.entities.iter().any(|e| {
                    !gone.contains(&e.id()) && e.points().iter().any(|id| gone.contains(id))
                })
                || s.patterns
                    .iter()
                    .enumerate()
                    .any(|(i, p)| i != at && p.def.entities().iter().any(|e| gone.contains(e)))
            {
                return Err("減らす写しに寸法・図形・パターンが付いています。参照を外してから数を減らしてください".into());
            }
            s.entities.retain(|e| !gone.contains(&e.id()));
            s.centerlines.retain(|e| !gone.contains(e));
            s.patterns[at].copies.retain(|c| live.contains(&c.index));
            let existing = s.patterns[at].copies.iter().map(|c| c.index).collect();
            let added = s
                .generate_missing(&def, &existing)
                .ok_or("パターンの元の図形が見つかりません")?;
            s.patterns[at].def = def;
            s.patterns[at].copies.extend(added);
            s.patterns[at].copies.sort_by_key(|c| c.index);
        }
        Ok(s)
    }
}
