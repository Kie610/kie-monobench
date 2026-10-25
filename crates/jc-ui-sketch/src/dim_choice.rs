//! 寸法の道具で、置く前に右クリックで選び直す寸法の種類。
//! 半径か直径か、横・縦・沿う向き、円は中心から測るか円周の近い・遠い側から測るか、中心線からは
//! 直径か距離か、駆動か被駆動(参照)か。

use jc_sketch::{ConstraintKind as K, Entity, EntityId, Sketch};

/// 2 点・線分の寸法の向き。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Orient {
    Horizontal,
    Vertical,
    Aligned,
}

/// 円との距離の測り方。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Gap {
    Center,
    Near,
    Far,
}

/// 選び直した寸法の種類(選びが変わると戻す)。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DimChoice {
    /// 真なら半径、偽なら直径
    pub radius: Option<bool>,
    pub orient: Option<Orient>,
    pub gap: Option<Gap>,
    /// 中心線から: 真なら直径、偽なら距離
    pub axis_diameter: Option<bool>,
    /// 被駆動(参照寸法)として置く
    pub driven: bool,
}

/// 選び直しを当てた寸法。当てられない選び直しは無視する。
pub(crate) fn apply(kind: K, c: &DimChoice, sketch: &Sketch, sel: &[EntityId]) -> K {
    let value = String::new();
    let mut k = kind;
    if let Some(r) = c.radius {
        k = match k {
            K::Diameter { circle, .. } | K::Radius { circle, .. } if r => K::Radius {
                circle,
                value: value.clone(),
            },
            K::Diameter { circle, .. } | K::Radius { circle, .. } => K::Diameter {
                circle,
                value: value.clone(),
            },
            other => other,
        };
    }
    if let Some(o) = c.orient
        && let Some((a, b)) = two_points(&k, sketch)
    {
        k = match o {
            Orient::Horizontal => K::HorizontalDistance {
                a,
                b,
                value: value.clone(),
            },
            Orient::Vertical => K::VerticalDistance {
                a,
                b,
                value: value.clone(),
            },
            Orient::Aligned => match line_of(sel, sketch) {
                Some(line) => K::Length {
                    line,
                    value: value.clone(),
                },
                None => K::Distance {
                    a,
                    b,
                    value: value.clone(),
                },
            },
        };
    }
    if let Some(g) = c.gap
        && let Some((circle, other)) = round_pair(sel, sketch)
    {
        k = match g {
            Gap::Center => k,
            Gap::Near | Gap::Far => K::CircleDistance {
                circle,
                other,
                far: g == Gap::Far,
                value: value.clone(),
            },
        };
    }
    if c.axis_diameter == Some(false)
        && let K::LinearDiameter { point, line, .. } = k
    {
        k = K::PointLineDistance { point, line, value };
    }
    k
}

/// 右クリックのメニューに出す選び直し(名前と、選んだ時の選び直し)。`kind` は今ついてくる寸法。
pub(crate) fn options(
    kind: &K,
    c: &DimChoice,
    sketch: &Sketch,
    sel: &[EntityId],
) -> Vec<(&'static str, DimChoice)> {
    let mut v = Vec::new();
    let with = |f: &dyn Fn(&mut DimChoice)| {
        let mut n = *c;
        f(&mut n);
        n
    };
    match kind {
        K::Diameter { .. } => v.push(("半径にする", with(&|n| n.radius = Some(true)))),
        K::Radius { .. } => v.push(("直径にする", with(&|n| n.radius = Some(false)))),
        _ => {}
    }
    if two_points(kind, sketch).is_some() && round_pair(sel, sketch).is_none() {
        v.push(("水平の寸法", with(&|n| n.orient = Some(Orient::Horizontal))));
        v.push(("垂直の寸法", with(&|n| n.orient = Some(Orient::Vertical))));
        v.push((
            "平行の寸法(沿う)",
            with(&|n| n.orient = Some(Orient::Aligned)),
        ));
    }
    if round_pair(sel, sketch).is_some() {
        v.push(("中心から測る", with(&|n| n.gap = Some(Gap::Center))));
        v.push(("円周の近い側から測る", with(&|n| n.gap = Some(Gap::Near))));
        v.push(("円周の遠い側から測る", with(&|n| n.gap = Some(Gap::Far))));
    }
    match kind {
        K::LinearDiameter { .. } => v.push((
            "中心線からの距離にする",
            with(&|n| n.axis_diameter = Some(false)),
        )),
        K::PointLineDistance { line, .. } if sketch.is_centerline(*line) => v.push((
            "中心線からの直径にする",
            with(&|n| n.axis_diameter = Some(true)),
        )),
        _ => {}
    }
    if c.driven {
        v.push(("駆動寸法にする", with(&|n| n.driven = false)));
    } else {
        v.push(("参照寸法にする", with(&|n| n.driven = true)));
    }
    v
}

/// 寸法が測る 2 点(線分の長さは両端)。
fn two_points(k: &K, sketch: &Sketch) -> Option<(EntityId, EntityId)> {
    match k {
        K::Distance { a, b, .. }
        | K::HorizontalDistance { a, b, .. }
        | K::VerticalDistance { a, b, .. } => Some((*a, *b)),
        K::Length { line, .. } => match sketch.entity(*line)? {
            Entity::Line { a, b, .. } => Some((*a, *b)),
            _ => None,
        },
        _ => None,
    }
}

fn line_of(sel: &[EntityId], sketch: &Sketch) -> Option<EntityId> {
    match sel {
        [l] if matches!(sketch.entity(*l), Some(Entity::Line { .. })) => Some(*l),
        _ => None,
    }
}

/// 選んだ 2 つのうち、円・円弧と相手(点・線分・円・円弧)。
fn round_pair(sel: &[EntityId], sketch: &Sketch) -> Option<(EntityId, EntityId)> {
    let round = |id: &EntityId| {
        matches!(
            sketch.entity(*id),
            Some(Entity::Circle { .. } | Entity::Arc { .. })
        )
    };
    let other = |id: &EntityId| {
        matches!(
            sketch.entity(*id),
            Some(
                Entity::Point { .. }
                    | Entity::Line { .. }
                    | Entity::Circle { .. }
                    | Entity::Arc { .. }
            )
        )
    };
    match sel {
        [a, b] if round(a) && other(b) => Some((*a, *b)),
        [a, b] if round(b) && other(a) => Some((*b, *a)),
        _ => None,
    }
}
