//! 測った長さ・選んだ物の説明と、解けた様子の言葉。

use super::*;

pub(crate) fn status_text(s: &Solution) -> String {
    if s.is_blank() {
        return kmb_sketch::NO_SHAPE.into();
    }
    match &s.status {
        SolveStatus::WellConstrained => "完全拘束".into(),
        SolveStatus::UnderConstrained { dof } => format!("拘束不足(自由度 {dof})"),
        SolveStatus::OverConstrained { redundant } => {
            format!("過剰な拘束: {}", join(redundant))
        }
        SolveStatus::Inconsistent { conflicting } => {
            format!("満たせない拘束: {}", join(conflicting))
        }
    }
}

pub(crate) fn join<T: std::fmt::Display>(v: &[T]) -> String {
    v.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// 右下に出す、選んだ物の数と長さの合計。何も選んでいなければ `None`。
pub(crate) fn selection_info(sketch: &Sketch, sol: &Solution, editor: &Editor) -> Option<String> {
    if editor.selection.is_empty() {
        return None;
    }
    let sel: Vec<EntityId> = editor.selection.iter().copied().collect();
    let (n, len) = selection_length(sketch, sol, &sel);
    let points = sel
        .iter()
        .filter(|id| matches!(sketch.entity(**id), Some(Entity::Point { .. })))
        .count();
    let mut parts = Vec::new();
    if n > 0 {
        parts.push(format!("{n} スケッチ曲線"));
    }
    if points > 0 {
        parts.push(format!("{points} 点"));
    }
    if n > 0 {
        parts.push(format!("長さ: {}", kmb_core::display_unit().length(len, 2)));
    }
    (!parts.is_empty()).then(|| parts.join(" | "))
}

/// 測る道具の結果(選んだ要素の長さ・距離・角度)。何も選んでいなければ `None`。
pub(crate) fn measure_text(
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
    editor: &Editor,
) -> Option<String> {
    let sel: Vec<EntityId> = editor.selection.iter().copied().collect();
    if sel.is_empty() {
        return None;
    }
    let fmt = editor.fmt;
    let mut parts: Vec<String> = Vec::new();
    // 円・円弧は半径と直径の両方を出す
    if let [id] = sel.as_slice()
        && let Some(Entity::Circle { radius, .. }) = sketch.entity(*id)
    {
        let r = sol.radii.get(id).copied().unwrap_or(*radius);
        let k = ConstraintKind::Diameter {
            circle: *id,
            value: String::new(),
        };
        parts.push(format!(
            "直径 {}・半径 {}・円周 {}",
            fmt.value(&k, 2.0 * r),
            fmt.value(&k, r),
            fmt.value(&k, std::f64::consts::TAU * r)
        ));
    } else if let Some(k) = kmb_sketch::dimension_for(sketch, sol, &sel)
        && let Some(v) = values
        && let Ok(p) = kmb_sketch::preview_dimension(sketch, v, &k)
    {
        parts.push(format!("{}: {}", measure_name(&k), p.text));
    }
    let (n, len) = crate::view::selection_length(sketch, sol, &sel);
    if n > 1 {
        parts.push(format!("線 {n} 本の長さの合計 {}", fmt.length(len)));
    }
    Some(if parts.is_empty() {
        "この組は測れません(線分、円、点 2 つ、点と線分、線分 2 本を選ぶ)".into()
    } else {
        format!("測った値  {}", parts.join("   "))
    })
}

pub(crate) fn measure_name(k: &ConstraintKind) -> &'static str {
    match k {
        ConstraintKind::Length { .. } => "長さ",
        ConstraintKind::Distance { .. } | ConstraintKind::PointLineDistance { .. } => "距離",
        ConstraintKind::HorizontalDistance { .. } => "横の距離",
        ConstraintKind::VerticalDistance { .. } => "縦の距離",
        ConstraintKind::Angle { .. } => "角度",
        ConstraintKind::Radius { .. } => "半径",
        ConstraintKind::Diameter { .. } | ConstraintKind::LinearDiameter { .. } => "直径",
        ConstraintKind::CircleDistance { .. } => "円周からの距離",
        _ => "値",
    }
}

/// 選んだ線(線分・円弧・円)の数と、長さの合計(mm)。
pub(crate) fn selection_length(sketch: &Sketch, sol: &Solution, sel: &[EntityId]) -> (usize, f64) {
    let pos = |id: &EntityId| sol.points.get(id).copied();
    let mut n = 0;
    let mut total = 0.0;
    for id in sel {
        let l = match sketch.entity(*id) {
            Some(Entity::Line { a, b, .. }) => pos(a)
                .zip(pos(b))
                .map(|(p, q)| (q[0] - p[0]).hypot(q[1] - p[1])),
            Some(Entity::Circle { radius, .. }) => {
                Some(std::f64::consts::TAU * sol.radii.get(id).copied().unwrap_or(*radius))
            }
            Some(Entity::Arc {
                center, start, end, ..
            }) => pos(center)
                .zip(pos(start))
                .zip(pos(end))
                .map(|((c, s), e)| {
                    let r = (s[0] - c[0]).hypot(s[1] - c[1]);
                    let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
                    let a1 = (e[1] - c[1]).atan2(e[0] - c[0]);
                    r * (a1 - a0).rem_euclid(std::f64::consts::TAU)
                }),
            _ => None,
        };
        if let Some(l) = l {
            n += 1;
            total += l;
        }
    }
    (n, total)
}

/// 図の印で選んだ拘束を外す(選んでいなければ `None`)。
pub(crate) fn remove_picked_constraint(
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
) -> Option<Sketch> {
    let id = editor.picked_constraint.take()?;
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    s.remove_constraint(id);
    Some(s)
}
