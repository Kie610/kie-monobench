//! 寸法を置く流れの処理: 選んだ要素に合う寸法を決め([`dimension_for`])、今の形で測り
//! ([`measure`])、置く前に「参照寸法になる」かを予告する([`preview_dimension`])。
//!
//! 寸法の道具は「D → 要素をクリック → すぐ数字」で使う。画面は、クリックした要素で
//! [`dimension_for`] を呼んで寸法の種類を決め、[`preview_dimension`] の `text`(測った値)を
//! 選んだ状態の入力欄に出す。Enter で [`Sketch::add_dimension`] を 1 回呼ぶ(取り消しは 1 回)。
//! ほかの拘束で既に決まっている寸法は、予告のとおり括弧つきの参照寸法として置き、
//! 形は変えない。参照寸法を形を決める寸法へ戻す時も、余分になるなら断る
//! ([`Sketch::set_driving`])。

use crate::geom::sub;
use crate::model::{Constraint, ConstraintId, ConstraintKind as K, Entity, EntityId, Sketch};
use crate::solver::{SketchError, Solution, determined_by, solve};
use std::collections::HashMap;
use std::f64::consts::PI;

/// 置く前の寸法の見込み。
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionPreview {
    /// 今の形で測った値(長さは mm、角度は rad)
    pub measured: f64,
    /// 入力欄に出す式(例: `12.5 mm`、`30 deg`)
    pub text: String,
    /// ほかの拘束で既に決まっているので、参照寸法(括弧つき)になる
    pub reference: bool,
    /// 参照寸法になる時、それを決めている拘束
    pub determined_by: Vec<ConstraintId>,
}

impl DimensionPreview {
    /// 画面に出す一言。形を決める寸法なら `None`。
    pub fn note(&self, sketch: &Sketch) -> Option<String> {
        self.reference.then(|| {
            format!(
                "{}で既に決まっているので、参照寸法({})として置きます",
                by_text(sketch, &self.determined_by),
                self.text
            )
        })
    }
}

fn pt(sketch: &Sketch, sol: &Solution, id: EntityId) -> Option<[f64; 2]> {
    match sketch.entity(id)? {
        Entity::Point { .. } => sol.points.get(&id).copied(),
        _ => None,
    }
}

fn line(sketch: &Sketch, sol: &Solution, id: EntityId) -> Option<([f64; 2], [f64; 2])> {
    match sketch.entity(id)? {
        Entity::Line { a, b, .. } => Some((*sol.points.get(a)?, *sol.points.get(b)?)),
        _ => None,
    }
}

/// 円・円弧の解いた半径(ほかの要素は `None`)。
pub fn radius(sketch: &Sketch, sol: &Solution, id: EntityId) -> Option<f64> {
    match sketch.entity(id)? {
        Entity::Circle { .. } => sol.radii.get(&id).copied(),
        Entity::Arc { center, start, .. } => {
            let (c, s) = (sol.points.get(center)?, sol.points.get(start)?);
            Some((s[0] - c[0]).hypot(s[1] - c[1]))
        }
        _ => None,
    }
}

/// 円・円弧の中心。
pub(crate) fn center_of(sketch: &Sketch, sol: &Solution, id: EntityId) -> Option<[f64; 2]> {
    match sketch.entity(id)? {
        Entity::Circle { center, .. } | Entity::Arc { center, .. } => {
            sol.points.get(center).copied()
        }
        _ => None,
    }
}

/// 円 `circle` の円周から `other`(点・線分を延ばした直線・円)までの距離。`far` なら遠い側。
/// ソルバの式と同じ場合分け(近い側は、外なら d - r、中なら r - d。円どうしで重なる時は重なりの幅)。
fn circle_gap(
    sketch: &Sketch,
    sol: &Solution,
    circle: EntityId,
    other: EntityId,
    far: bool,
) -> Option<f64> {
    let (c, r) = (
        center_of(sketch, sol, circle)?,
        radius(sketch, sol, circle)?,
    );
    let near = |d: f64| (d - r).abs();
    Some(match sketch.entity(other)? {
        Entity::Point { .. } => {
            let d = {
                let v = sub(pt(sketch, sol, other)?, c);
                v[0].hypot(v[1])
            };
            if far { d + r } else { near(d) }
        }
        Entity::Line { .. } => {
            let (a, b) = line(sketch, sol, other)?;
            let (u, w) = (sub(b, a), sub(c, a));
            let d = (u[0] * w[1] - u[1] * w[0]).abs() / u[0].hypot(u[1]).max(1e-12);
            if far { d + r } else { near(d) }
        }
        _ => {
            let (c2, q) = (center_of(sketch, sol, other)?, radius(sketch, sol, other)?);
            let v = sub(c2, c);
            let d = v[0].hypot(v[1]);
            if far {
                d + r + q
            } else if d >= r + q {
                d - r - q
            } else if d <= (r - q).abs() {
                (r - q).abs() - d
            } else {
                r + q - d
            }
        }
    })
}

/// 寸法を、解いた形で測る(長さは mm、角度は rad)。寸法でない拘束や、参照先がないものは `None`。
/// 値はソルバの式と同じ向きで測る(水平・垂直の距離と点と直線の距離は絶対値、角度は
/// 線分 a から b へ反時計回りに測って 0 以上 π 未満)。
pub fn measure(sketch: &Sketch, sol: &Solution, kind: &K) -> Option<f64> {
    Some(match kind {
        K::Distance { a, b, .. } => {
            let d = sub(pt(sketch, sol, *b)?, pt(sketch, sol, *a)?);
            d[0].hypot(d[1])
        }
        K::PointLineDistance { point, line: l, .. } => {
            let p = pt(sketch, sol, *point)?;
            let (a, b) = line(sketch, sol, *l)?;
            let (u, w) = (sub(b, a), sub(p, a));
            (u[0] * w[1] - u[1] * w[0]).abs() / u[0].hypot(u[1]).max(1e-12)
        }
        K::HorizontalDistance { a, b, .. } => {
            (pt(sketch, sol, *b)?[0] - pt(sketch, sol, *a)?[0]).abs()
        }
        K::VerticalDistance { a, b, .. } => {
            (pt(sketch, sol, *b)?[1] - pt(sketch, sol, *a)?[1]).abs()
        }
        K::Length { line: l, .. } => {
            let (a, b) = line(sketch, sol, *l)?;
            let d = sub(b, a);
            d[0].hypot(d[1])
        }
        K::Radius { circle, .. } => radius(sketch, sol, *circle)?,
        K::Diameter { circle, .. } => 2.0 * radius(sketch, sol, *circle)?,
        K::Angle { a, b, .. } => {
            let ((p, q), (r, s)) = (line(sketch, sol, *a)?, line(sketch, sol, *b)?);
            let (u, w) = (sub(q, p), sub(s, r));
            let cross = u[0] * w[1] - u[1] * w[0];
            let dot = u[0] * w[0] + u[1] * w[1];
            cross.atan2(dot).rem_euclid(PI)
        }
        K::LinearDiameter { point, line: l, .. } => {
            let p = pt(sketch, sol, *point)?;
            let (a, b) = line(sketch, sol, *l)?;
            let (u, w) = (sub(b, a), sub(p, a));
            2.0 * (u[0] * w[1] - u[1] * w[0]).abs() / u[0].hypot(u[1]).max(1e-12)
        }
        K::CircleDistance {
            circle, other, far, ..
        } => circle_gap(sketch, sol, *circle, *other, *far)?,
        _ => return None,
    })
}

/// 測った値を寸法の式にする(長さは `mm`、角度は `deg`。小数 3 桁まで)。
fn value_text(kind: &K, v: f64) -> String {
    let (v, unit) = if kind.is_angle() {
        (v.to_degrees(), "deg")
    } else {
        (v, "mm")
    };
    let s = format!("{v:.3}");
    format!("{} {unit}", s.trim_end_matches('0').trim_end_matches('.'))
}

/// 寸法の式に `value` を書き込んだ寸法。
fn with_value(kind: &K, value: String) -> K {
    let mut k = kind.clone();
    if let Some(v) = k.value_mut() {
        *v = value;
    }
    k
}

/// 寸法の道具で要素をクリックした時に置く寸法の種類(値はまだ空)。選び方に合う寸法が
/// なければ `None`。線分 1 本は長さ、円は直径、円弧は半径、楕円は長半径(中心と長軸の端の
/// 距離)、点 2 つは距離、点と線分は
/// 点と直線の距離、平行な線分 2 本はその間の距離、平行でない線分 2 本は角度。
pub fn dimension_for(sketch: &Sketch, sol: &Solution, selection: &[EntityId]) -> Option<K> {
    let value = String::new();
    let ent = |id: EntityId| sketch.entity(id);
    Some(match selection {
        [l] if matches!(ent(*l)?, Entity::Line { .. }) => K::Length { line: *l, value },
        [c] if matches!(ent(*c)?, Entity::Circle { .. }) => K::Diameter { circle: *c, value },
        [c] if matches!(ent(*c)?, Entity::Arc { .. }) => K::Radius { circle: *c, value },
        // 楕円は長半径(中心から長軸の端までの距離)。短半径は中心と短軸の端の点 2 つで選ぶ
        [c] if matches!(ent(*c)?, Entity::Ellipse { .. }) => match ent(*c)? {
            Entity::Ellipse { center, major, .. } => K::Distance {
                a: *center,
                b: *major,
                value,
            },
            _ => return None,
        },
        // 中心線と点・線分: 回転体の直径(中心から測る距離は右クリックで選び直す)
        [a, b] if sketch.is_centerline(*a) || sketch.is_centerline(*b) => {
            let (axis, other) = if sketch.is_centerline(*a) {
                (*a, *b)
            } else {
                (*b, *a)
            };
            let point = match ent(other)? {
                Entity::Point { .. } => other,
                Entity::Line { a: p, .. } if !sketch.is_centerline(other) => *p,
                _ => return None,
            };
            K::LinearDiameter {
                point,
                line: axis,
                value,
            }
        }
        // 円・円弧と点・線分・円: 中心から測る(円周から測るのは右クリックで選ぶ)
        [a, b] if is_round(ent(*a)?) || is_round(ent(*b)?) => {
            let (c, other) = if is_round(ent(*a)?) {
                (*a, *b)
            } else {
                (*b, *a)
            };
            let center = round_center(ent(c)?)?;
            match ent(other)? {
                Entity::Point { .. } => K::Distance {
                    a: center,
                    b: other,
                    value,
                },
                Entity::Line { .. } => K::PointLineDistance {
                    point: center,
                    line: other,
                    value,
                },
                e if is_round(e) => K::Distance {
                    a: center,
                    b: round_center(e)?,
                    value,
                },
                _ => return None,
            }
        }
        [a, b] => match (ent(*a)?, ent(*b)?) {
            (Entity::Point { .. }, Entity::Point { .. }) => K::Distance {
                a: *a,
                b: *b,
                value,
            },
            (Entity::Point { .. }, Entity::Line { .. }) => K::PointLineDistance {
                point: *a,
                line: *b,
                value,
            },
            (Entity::Line { .. }, Entity::Point { .. }) => K::PointLineDistance {
                point: *b,
                line: *a,
                value,
            },
            (Entity::Line { .. }, Entity::Line { a: p, .. }) => {
                let ((p0, p1), (q0, q1)) = (line(sketch, sol, *a)?, line(sketch, sol, *b)?);
                let (u, w) = (sub(p1, p0), sub(q1, q0));
                let sin = (u[0] * w[1] - u[1] * w[0]) / (u[0].hypot(u[1]) * w[0].hypot(w[1]));
                if sin.abs() < 1e-9 {
                    K::PointLineDistance {
                        point: *p,
                        line: *a,
                        value,
                    }
                } else {
                    K::Angle {
                        a: *a,
                        b: *b,
                        value,
                    }
                }
            }
            _ => return None,
        },
        _ => return None,
    })
}

fn is_round(e: &Entity) -> bool {
    matches!(e, Entity::Circle { .. } | Entity::Arc { .. })
}

fn round_center(e: &Entity) -> Option<EntityId> {
    match e {
        Entity::Circle { center, .. } | Entity::Arc { center, .. } => Some(*center),
        _ => None,
    }
}

/// 寸法 `kind` を置く前の見込み。今の形で測り、ほかの拘束で既に決まっているかを調べる。
/// `values` は今ある寸法の値([`crate::dimension_values`])。
pub fn preview_dimension(
    sketch: &Sketch,
    values: &HashMap<ConstraintId, f64>,
    kind: &K,
) -> Result<DimensionPreview, SketchError> {
    let sol = solve(sketch, values)?;
    let measured = measure(sketch, &sol, kind).ok_or(SketchError::BadReference {
        owner: "寸法".into(),
        target: first_ref(kind),
        expected: "寸法を測れる要素",
    })?;
    let mut solved = sketch.clone();
    solved.adopt_solution(&sol);
    let by = determined_by(&solved, values, kind, Some(measured))?;
    Ok(DimensionPreview {
        measured,
        text: value_text(kind, measured),
        reference: by.is_some(),
        determined_by: by.unwrap_or_default(),
    })
}

fn first_ref(kind: &K) -> EntityId {
    match kind {
        K::Distance { a, .. }
        | K::HorizontalDistance { a, .. }
        | K::VerticalDistance { a, .. }
        | K::Angle { a, .. } => *a,
        K::PointLineDistance { point, .. } => *point,
        K::Length { line, .. } => *line,
        K::Radius { circle, .. }
        | K::Diameter { circle, .. }
        | K::CircleDistance { circle, .. } => *circle,
        K::LinearDiameter { point, .. } => *point,
        _ => EntityId(0),
    }
}

/// 拘束を短い言葉で(例: `C3(長さ 10 mm)`、`C2(水平)`)。
pub fn describe(c: &Constraint) -> String {
    let what = kind_word(&c.kind);
    match c.kind.value() {
        Some(v) => format!("{}({what} {v})", c.id),
        None => format!("{}({what})", c.id),
    }
}

/// 拘束の種類の言葉(例: 「距離」「水平」)。
pub(crate) fn kind_word(kind: &crate::ConstraintKind) -> &'static str {
    match kind {
        K::Coincident { .. } => "一致",
        K::Fix { .. } => "固定",
        K::Horizontal { .. } => "水平",
        K::Vertical { .. } => "垂直",
        K::Parallel { .. } => "平行",
        K::Perpendicular { .. } => "直交",
        K::Equal { .. } => "等しい",
        K::PointOnLine { .. } => "線上の点",
        K::PointOnCircle { .. } => "円上の点",
        K::Tangent { .. } | K::TangentCircles { .. } => "接線",
        K::Concentric { .. } => "同心円",
        K::Collinear { .. } => "同一直線上",
        K::Midpoint { .. } => "中点",
        K::Symmetric { .. } => "対称",
        K::Distance { .. } => "距離",
        K::PointLineDistance { .. } => "点と直線の距離",
        K::HorizontalDistance { .. } => "水平の距離",
        K::VerticalDistance { .. } => "垂直の距離",
        K::Length { .. } => "長さ",
        K::Radius { .. } => "半径",
        K::Diameter { .. } => "直径",
        K::Angle { .. } => "角度",
        K::PointOnCurve { .. } => "曲線上の点",
        K::TangentCurve { .. } => "接線",
        K::Curvature { .. } => "曲率",
        K::CircleDistance { far: false, .. } => "円周からの最短距離",
        K::CircleDistance { far: true, .. } => "円周からの最長距離",
        K::LinearDiameter { .. } => "中心線からの直径",
        K::Offset { .. } => "オフセット",
        K::Polygon { .. } => "ポリゴン",
        K::Pattern { .. } => "パターン",
    }
}

/// 決めている拘束の一覧を言葉に。寸法を先に、多ければ 4 つまでで「など」。
fn by_text(sketch: &Sketch, ids: &[ConstraintId]) -> String {
    let mut cs: Vec<&Constraint> = ids
        .iter()
        .filter_map(|id| sketch.constraints.iter().find(|c| c.id == *id))
        .collect();
    if cs.is_empty() {
        return "ほかの拘束".into();
    }
    cs.sort_by_key(|c| c.kind.value().is_none());
    let mut s: Vec<String> = cs.iter().take(4).map(|c| describe(c)).collect();
    if cs.len() > 4 {
        s.push("など".into());
    }
    s.join("、")
}

impl Sketch {
    /// 寸法を置く。値が空なら測った値にする。ほかの拘束で既に決まっている寸法は、
    /// 測った値の参照寸法として置き、形は変えない。置いた拘束の ID と見込みを返す。
    pub fn add_dimension(
        &mut self,
        kind: K,
        values: &HashMap<ConstraintId, f64>,
    ) -> Result<(ConstraintId, DimensionPreview), String> {
        if kind.value().is_none() {
            return Err("寸法ではありません".into());
        }
        let preview = preview_dimension(self, values, &kind).map_err(|e| e.to_string())?;
        let typed = kind.value().unwrap_or_default().trim().to_string();
        let value = if typed.is_empty() || preview.reference {
            preview.text.clone()
        } else {
            typed
        };
        let id = self.add_constraint(with_value(&kind, value));
        if let Some(c) = self.constraints.iter_mut().find(|c| c.id == id) {
            c.reference = preview.reference;
        }
        Ok((id, preview))
    }

    /// 寸法を形を決める寸法(`driving` が真)か参照寸法にする。形を決める寸法へ戻すと
    /// 余分になる(ほかの拘束で既に決まっている)なら断る。戻す時は今の形で測った値にする
    /// (形が跳ねない)。
    pub fn set_driving(
        &mut self,
        id: ConstraintId,
        driving: bool,
        values: &HashMap<ConstraintId, f64>,
    ) -> Result<(), String> {
        let c = self
            .constraints
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("拘束 {id} がありません"))?;
        if c.kind.value().is_none() {
            return Err(format!("{id} は寸法ではありません"));
        }
        if !driving || !c.reference {
            let c = self
                .constraints
                .iter_mut()
                .find(|c| c.id == id)
                .expect("ある");
            c.reference = !driving;
            return Ok(());
        }
        let kind = c.kind.clone();
        let mut rest = self.clone();
        rest.constraints.retain(|c| c.id != id);
        let preview = preview_dimension(&rest, values, &kind).map_err(|e| e.to_string())?;
        if preview.reference {
            return Err(format!(
                "{id} は{}で既に決まっています",
                by_text(self, &preview.determined_by)
            ));
        }
        let c = self
            .constraints
            .iter_mut()
            .find(|c| c.id == id)
            .expect("ある");
        c.kind = with_value(&kind, preview.text);
        c.reference = false;
        Ok(())
    }
}
