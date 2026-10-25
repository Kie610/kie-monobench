//! 拘束ソルバ。拘束を残差の式に変え(ここ)、最小ノルムのガウス・ニュートン法で解く
//! (式と解き方は kmb-sketch-solver の `system`)。
//!
//! 最小ノルムの更新を使うので、拘束が足りない要素は、保存されている位置から
//! なるべく動かない。自由度はヤコビ行列の階数から求める。参照寸法は解かずに測るだけ。

use crate::curve::Shape;
use crate::model::{
    Constraint, ConstraintId, ConstraintKind, Entity, EntityId, OffsetPart, Sketch,
};
use crate::pattern::{PatternAxis, PatternCenter, Placement};
use kmb_sketch_solver::system::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum SketchError {
    DuplicateId(String),
    /// 参照先がない、または種類が違う
    BadReference {
        owner: String,
        target: EntityId,
        expected: &'static str,
    },
    MissingValue(ConstraintId),
}

impl fmt::Display for SketchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SketchError::DuplicateId(id) => write!(f, "ID {id} が重複しています"),
            SketchError::BadReference {
                owner,
                target,
                expected,
            } => {
                write!(f, "{owner} の参照先 {target} が{expected}ではありません")
            }
            SketchError::MissingValue(c) => write!(f, "{c} の寸法の値がありません"),
        }
    }
}

impl std::error::Error for SketchError {}

/// 拘束の状態(完全拘束・不足・過剰)。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SolveStatus {
    /// すべての要素の位置が決まっている
    #[default]
    WellConstrained,
    /// まだ動かせる要素がある
    UnderConstrained { dof: usize },
    /// 満たせてはいるが、ほかの拘束から決まる余分な拘束がある
    OverConstrained { redundant: Vec<ConstraintId> },
    /// 同時に満たせない拘束がある
    Inconsistent { conflicting: Vec<ConstraintId> },
}

/// 解いた結果。座標は mm。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Solution {
    pub status: SolveStatus,
    /// 残っている自由度の数
    pub dof: usize,
    pub points: BTreeMap<EntityId, [f64; 2]>,
    pub radii: BTreeMap<EntityId, f64>,
    /// 位置が完全に決まっている要素
    pub fixed: BTreeSet<EntityId>,
    /// 線や円に使われていない単独の点([`Sketch::lone_points`])
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lone: Vec<EntityId>,
    pub iterations: usize,
    pub max_residual: f64,
    /// 参照寸法を解いた形で測った値(長さは mm、角度は rad)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub measured: BTreeMap<ConstraintId, f64>,
}

/// 何も描いていないスケッチの状態の言葉。画面・CLI・AI で同じにする(動く物が無いだけで
/// 「完全拘束」と出すと、描き終えたと思わせる)。
pub const NO_SHAPE: &str = "まだ形がありません";

impl Solution {
    /// 点も円も無い(何も描いていない)か。状態の言葉は [`NO_SHAPE`] にする。
    pub fn is_blank(&self) -> bool {
        self.points.is_empty() && self.radii.is_empty()
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Point,
    Line,
    Circle,
    Arc,
    Ellipse,
    Spline,
    Conic,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Point => "点",
            Kind::Line => "線分",
            Kind::Circle => "円",
            Kind::Arc => "円弧",
            Kind::Ellipse => "楕円",
            Kind::Spline => "スプライン",
            Kind::Conic => "円錐曲線",
        }
    }
}

struct Builder<'a> {
    kinds: HashMap<EntityId, (Kind, &'a Entity)>,
    points: BTreeMap<EntityId, Pt>,
    radii: HashMap<EntityId, Radius>,
    /// 拘束どうしで同じものを指す曲線の形(形を作るのを 1 回で済ませる)
    shared: HashMap<EntityId, Arc<CurveDef>>,
}

impl Builder<'_> {
    fn expect(&self, owner: &str, id: EntityId, kind: Kind) -> Result<(), SketchError> {
        match self.kinds.get(&id) {
            Some((k, _)) if std::mem::discriminant(k) == std::mem::discriminant(&kind) => Ok(()),
            _ => Err(SketchError::BadReference {
                owner: owner.into(),
                target: id,
                expected: kind.name(),
            }),
        }
    }

    fn point(&self, owner: &str, id: EntityId) -> Result<Pt, SketchError> {
        self.expect(owner, id, Kind::Point)?;
        Ok(self.points[&id])
    }

    fn line(&self, owner: &str, id: EntityId) -> Result<(Pt, Pt), SketchError> {
        self.expect(owner, id, Kind::Line)?;
        match self.kinds[&id].1 {
            Entity::Line { a, b, .. } => Ok((self.points[a], self.points[b])),
            _ => unreachable!("種類は確かめた"),
        }
    }

    /// 曲線の端の点(始点, 終点)
    fn ends(&self, id: EntityId) -> Option<(EntityId, EntityId)> {
        self.kinds.get(&id).and_then(|k| k.1.ends())
    }

    /// 2 つの要素が共有する端の点(線分と円弧、円弧どうし)
    fn shared_end(&self, a: EntityId, b: EntityId) -> Option<Pt> {
        self.joint(a, b).map(|(p, ..)| p)
    }

    /// 2 つの曲線が共有する端の点と、それぞれのどちらの端か。
    fn joint(&self, a: EntityId, b: EntityId) -> Option<(Pt, At, At)> {
        let ((a0, a1), (b0, b1)) = (self.ends(a)?, self.ends(b)?);
        for (pa, at_a) in [(a0, At::Start), (a1, At::End)] {
            for (pb, at_b) in [(b0, At::Start), (b1, At::End)] {
                if pa == pb {
                    return Some((*self.points.get(&pa)?, at_a, at_b));
                }
            }
        }
        None
    }

    /// 要素 `id` の形 `def` を、ほかの拘束と共有する。
    fn share(&mut self, id: EntityId, def: CurveDef) -> Arc<CurveDef> {
        self.shared
            .entry(id)
            .or_insert_with(|| Arc::new(def))
            .clone()
    }

    /// 曲線(線分・円・円弧・楕円・スプライン・円錐曲線)の形
    fn curve(&self, owner: &str, id: EntityId) -> Result<CurveDef, SketchError> {
        let p = |q: &EntityId| self.points[q];
        Ok(match self.kinds.get(&id).map(|k| k.1) {
            Some(Entity::Line { a, b, .. }) => CurveDef::Line(p(a), p(b)),
            Some(Entity::Circle { center, .. }) => CurveDef::Circle(p(center), self.radii[&id]),
            Some(Entity::Arc {
                center, start, end, ..
            }) => CurveDef::Arc(p(center), p(start), p(end)),
            Some(Entity::Ellipse {
                center,
                major,
                minor,
                ends,
                ..
            }) => CurveDef::Ellipse(
                p(center),
                p(major),
                p(minor),
                ends.map(|(s, e)| (p(&s), p(&e))),
            ),
            Some(Entity::Spline {
                points,
                control,
                closed,
                degree,
                knots,
                ..
            }) => CurveDef::Spline {
                pts: points.iter().map(p).collect(),
                control: *control,
                closed: *closed,
                degree: *degree,
                knots: knots.clone(),
            },
            Some(Entity::Conic {
                start,
                end,
                apex,
                rho,
                ..
            }) => CurveDef::Conic(p(start), p(end), p(apex), *rho),
            _ => {
                return Err(SketchError::BadReference {
                    owner: owner.into(),
                    target: id,
                    expected: "曲線",
                });
            }
        })
    }

    /// 円または円弧の (中心, 半径)
    fn round(&self, owner: &str, id: EntityId) -> Result<(Pt, Radius), SketchError> {
        match self.kinds.get(&id).map(|k| k.1) {
            Some(Entity::Circle { center, .. }) | Some(Entity::Arc { center, .. }) => {
                Ok((self.points[center], self.radii[&id]))
            }
            _ => Err(SketchError::BadReference {
                owner: owner.into(),
                target: id,
                expected: "円または円弧",
            }),
        }
    }
}

fn build(sketch: &Sketch, values: &HashMap<ConstraintId, f64>) -> Result<System, SketchError> {
    let mut kinds = HashMap::new();
    for e in &sketch.entities {
        let k = match e {
            Entity::Point { .. } => Kind::Point,
            Entity::Line { .. } => Kind::Line,
            Entity::Circle { .. } => Kind::Circle,
            Entity::Arc { .. } => Kind::Arc,
            Entity::Ellipse { .. } => Kind::Ellipse,
            Entity::Spline { .. } => Kind::Spline,
            Entity::Conic { .. } => Kind::Conic,
        };
        if kinds.insert(e.id(), (k, e)).is_some() {
            return Err(SketchError::DuplicateId(e.id().to_string()));
        }
    }
    let mut x0 = Vec::new();
    let mut points = BTreeMap::new();
    let mut circles = BTreeMap::new();
    let mut entity_vars: BTreeMap<EntityId, Vec<usize>> = BTreeMap::new();
    for e in &sketch.entities {
        if let Entity::Point { id, x, y } = e {
            points.insert(*id, Pt(x0.len()));
            entity_vars.insert(*id, vec![x0.len(), x0.len() + 1]);
            x0.extend([*x, *y]);
        }
    }
    let mut b = Builder {
        kinds,
        points,
        radii: HashMap::new(),
        shared: HashMap::new(),
    };
    let mut equations = Vec::new();
    let mut traits = Vec::new();
    for e in &sketch.entities {
        let owner = e.id().to_string();
        match e {
            Entity::Point { .. } => {}
            Entity::Line { id, a, b: end, .. } => {
                let (pa, pb) = (b.point(&owner, *a)?, b.point(&owner, *end)?);
                entity_vars.insert(*id, vec![pa.0, pa.0 + 1, pb.0, pb.0 + 1]);
                traits.push(Trait::Direction(pa, pb));
            }
            Entity::Circle {
                id, center, radius, ..
            } => {
                let c = b.point(&owner, *center)?;
                circles.insert(*id, x0.len());
                b.radii.insert(*id, Radius::Var(x0.len()));
                entity_vars.insert(*id, vec![c.0, c.0 + 1, x0.len()]);
                traits.push(Trait::Positive(x0.len()));
                x0.push(*radius);
            }
            Entity::Arc {
                id,
                center,
                start,
                end,
                ..
            } => {
                let (c, s, t) = (
                    b.point(&owner, *center)?,
                    b.point(&owner, *start)?,
                    b.point(&owner, *end)?,
                );
                b.radii.insert(*id, Radius::Arc(c, s));
                entity_vars.insert(*id, vec![c.0, c.0 + 1, s.0, s.0 + 1, t.0, t.0 + 1]);
                equations.push((None, Residual::ArcEnd(c, s, t)));
                traits.push(Trait::Sweep(c, s, t));
            }
            Entity::Ellipse {
                id,
                center,
                major,
                minor,
                ends,
                ..
            } => {
                let (c, ma, mi) = (
                    b.point(&owner, *center)?,
                    b.point(&owner, *major)?,
                    b.point(&owner, *minor)?,
                );
                // 中の式: 長軸と短軸は直角。短軸の端は長軸の同じ側に保つ(短半径が 0 を越えて裏返らない)
                equations.push((None, Residual::Dot(c, ma, c, mi)));
                traits.push(Trait::CrossSign(c, ma, c, mi));
                let mut vars = vec![c.0, c.0 + 1, ma.0, ma.0 + 1, mi.0, mi.0 + 1];
                if let Some((s, e)) = ends {
                    let (s, e) = (b.point(&owner, *s)?, b.point(&owner, *e)?);
                    equations.push((None, Residual::OnEllipse(c, ma, mi, s)));
                    equations.push((None, Residual::OnEllipse(c, ma, mi, e)));
                    traits.push(Trait::EllipseSweep(c, ma, mi, s, e));
                    vars.extend([s.0, s.0 + 1, e.0, e.0 + 1]);
                }
                entity_vars.insert(*id, vars);
            }
            Entity::Spline { id, points, .. } => {
                let mut vars = vec![];
                for p in points {
                    let q = b.point(&owner, *p)?;
                    vars.extend([q.0, q.0 + 1]);
                }
                entity_vars.insert(*id, vars);
            }
            Entity::Conic {
                id,
                start,
                end,
                apex,
                ..
            } => {
                let (s, t, a) = (
                    b.point(&owner, *start)?,
                    b.point(&owner, *end)?,
                    b.point(&owner, *apex)?,
                );
                // 頂点が両端を結ぶ線の向こうへ回ると、曲線が裏返る
                traits.push(Trait::Side(a, s, t));
                entity_vars.insert(*id, vec![s.0, s.0 + 1, t.0, t.0 + 1, a.0, a.0 + 1]);
            }
        }
    }

    // 写した要素は元の形の位置に置いたまま動かさない(利用者の拘束ではないので、余分の判定に出さない)
    let (projected, projected_circles) = sketch.projected_parts();
    for id in projected {
        if let Some(p) = b.points.get(&id) {
            equations.push((None, Residual::Value(p.0, x0[p.0])));
            equations.push((None, Residual::Value(p.0 + 1, x0[p.0 + 1])));
        }
    }
    for id in projected_circles {
        if let Some(&i) = circles.get(&id) {
            equations.push((None, Residual::Value(i, x0[i])));
        }
    }

    // 固定した点(円の固定は、中心を別に固定していれば半径だけを止める)
    let fixed_points: BTreeSet<EntityId> = sketch
        .constraints
        .iter()
        .filter(|c| !c.reference)
        .filter_map(|c| match c.kind {
            ConstraintKind::Fix { point } => Some(point),
            _ => None,
        })
        .collect();
    let mut part_rows = Vec::new();
    let mut ids = BTreeSet::new();
    for c in &sketch.constraints {
        if !ids.insert(c.id) {
            return Err(SketchError::DuplicateId(c.id.to_string()));
        }
        if c.reference {
            continue;
        }
        let owner = c.id.to_string();
        let o = owner.as_str();
        let value = || {
            values
                .get(&c.id)
                .copied()
                .ok_or(SketchError::MissingValue(c.id))
        };
        let tag = Some(c.id);
        let mut push = |r: Residual| equations.push((tag, r));
        let mut keep = |t: Trait| traits.push(t);
        use ConstraintKind as K;
        match &c.kind {
            K::Coincident { a, b: q } => {
                let (p, q) = (b.point(o, *a)?, b.point(o, *q)?);
                push(Residual::Diff(p.0, q.0));
                push(Residual::Diff(p.0 + 1, q.0 + 1));
            }
            // 円を固定: 中心と半径を今の値に。中心の点を
            // 別に固定していれば、中心はそちらが止める(円と中心を別々に固定・解除できる)
            K::Fix { point } if matches!(b.kinds.get(point), Some((Kind::Circle, _))) => {
                let (c, r) = b.round(o, *point)?;
                let center = b.kinds[point].1.points()[0];
                if !fixed_points.contains(&center) {
                    push(Residual::Value(c.0, x0[c.0]));
                    push(Residual::Value(c.0 + 1, x0[c.0 + 1]));
                }
                push(Residual::Rad(r, 1.0, r.get(&x0)));
            }
            K::Fix { point } => {
                let p = b.point(o, *point)?;
                push(Residual::Value(p.0, x0[p.0]));
                push(Residual::Value(p.0 + 1, x0[p.0 + 1]));
            }
            K::Horizontal { line } => {
                let (p, q) = b.line(o, *line)?;
                push(Residual::Diff(p.0 + 1, q.0 + 1));
            }
            K::Vertical { line } => {
                let (p, q) = b.line(o, *line)?;
                push(Residual::Diff(p.0, q.0));
            }
            K::HorizontalPoints { a, b: q } => {
                let (p, q) = (b.point(o, *a)?, b.point(o, *q)?);
                push(Residual::Diff(p.0 + 1, q.0 + 1));
            }
            K::VerticalPoints { a, b: q } => {
                let (p, q) = (b.point(o, *a)?, b.point(o, *q)?);
                push(Residual::Diff(p.0, q.0));
            }
            K::Parallel { a, b: l2 } => {
                let ((p, q), (r, s)) = (b.line(o, *a)?, b.line(o, *l2)?);
                push(Residual::Cross(p, q, r, s));
                keep(Trait::DotSign(p, q, r, s));
            }
            K::Perpendicular { a, b: l2 } => {
                let ((p, q), (r, s)) = (b.line(o, *a)?, b.line(o, *l2)?);
                push(Residual::Dot(p, q, r, s));
                keep(Trait::CrossSign(p, q, r, s));
            }
            K::Equal { a, b: e2 } => {
                if let (Ok((p, q)), Ok((r, s))) = (b.line(o, *a), b.line(o, *e2)) {
                    push(Residual::EqualLen(p, q, r, s));
                } else {
                    let ((_, r1), (_, r2)) = (b.round(o, *a)?, b.round(o, *e2)?);
                    push(Residual::EqualRadius(r1, r2));
                }
            }
            K::PointOnLine { point, line } => {
                let (p, (a, q)) = (b.point(o, *point)?, b.line(o, *line)?);
                push(Residual::OnLine(p, a, q));
            }
            K::PointOnCircle { point, circle } => {
                let (p, (c, r)) = (b.point(o, *point)?, b.round(o, *circle)?);
                push(Residual::OnCircle(p, c, r));
            }
            K::Tangent { line, circle } => {
                let ((p, q), (c, r)) = (b.line(o, *line)?, b.round(o, *circle)?);
                // 半径と直角でも、線が円の中心の反対側へ回ると交差した形になる
                keep(Trait::Side(c, p, q));
                if let Some(at) = b.shared_end(*line, *circle) {
                    // 端を共有していれば、そこで半径と線が直角(距離で書くと接する所で
                    // 微分が 0 になり、長穴や角の丸めが「余分な拘束」に見えてしまう)
                    push(Residual::Dot(c, at, p, q));
                } else {
                    // 円が直線のどちら側にあるかは、保存されている位置で決める
                    let s = sign_of(signed_distance(c, p, q, &x0));
                    push(Residual::Tangent(p, q, c, r, s));
                }
            }
            K::TangentCircles { a, b: e2 } => {
                let ((c1, r1), (c2, r2)) = (b.round(o, *a)?, b.round(o, *e2)?);
                if let Some(at) = b.shared_end(*a, *e2) {
                    // 端を共有していれば、両方の中心とその点が一直線(中心が同じ側か反対側かは保つ)
                    push(Residual::Cross(c1, at, c2, at));
                    keep(Trait::DotSign(at, c1, at, c2));
                    continue;
                }
                // 外から接するか内から接するかは、保存されている位置で近い方に決める
                let d = dist(c1.get(&x0), c2.get(&x0));
                let (v1, v2) = (r1.get(&x0), r2.get(&x0));
                let (k1, k2) = if (d - (v1 + v2)).abs() <= (d - (v1 - v2).abs()).abs() {
                    (1.0, 1.0)
                } else {
                    let s = sign_of(v1 - v2);
                    (s, -s)
                };
                push(Residual::RoundTangent(c1, r1, c2, r2, k1, k2));
            }
            K::Concentric { a, b: e2 } => {
                let ((c1, _), (c2, _)) = (b.round(o, *a)?, b.round(o, *e2)?);
                push(Residual::Diff(c1.0, c2.0));
                push(Residual::Diff(c1.0 + 1, c2.0 + 1));
            }
            K::Collinear { a, b: l2 } => {
                let ((p, q), (r, s)) = (b.line(o, *a)?, b.line(o, *l2)?);
                push(Residual::OnLine(r, p, q));
                push(Residual::OnLine(s, p, q));
            }
            K::Symmetric { a, b: q, line } => {
                let (p1, p2, (l1, l2)) = (b.point(o, *a)?, b.point(o, *q)?, b.line(o, *line)?);
                push(Residual::MidOnLine(p1, p2, l1, l2));
                push(Residual::Dot(p1, p2, l1, l2));
                keep(Trait::Side(p1, l1, l2));
            }
            K::SymmetricPairs {
                a1,
                b1,
                a2,
                b2,
                line,
            } => {
                let (l1, l2) = b.line(o, *line)?;
                for (p, q) in [(a1, b1), (a2, b2)] {
                    let (p1, p2) = (b.point(o, *p)?, b.point(o, *q)?);
                    push(Residual::MidOnLine(p1, p2, l1, l2));
                    push(Residual::Dot(p1, p2, l1, l2));
                    keep(Trait::Side(p1, l1, l2));
                }
            }
            K::Midpoint { point, line } => {
                let (m, (p, q)) = (b.point(o, *point)?, b.line(o, *line)?);
                push(Residual::Mid(m, p, q, true));
                push(Residual::Mid(m, p, q, false));
            }
            K::Distance { a, b: q, .. } => {
                let (p, q) = (b.point(o, *a)?, b.point(o, *q)?);
                push(Residual::Dist(p, q, value()?));
            }
            K::PointLineDistance { point, line, .. } => {
                let (p, (l1, l2)) = (b.point(o, *point)?, b.line(o, *line)?);
                // 点が直線のどちら側にあるかは、保存されている位置で決める
                let s = sign_of(signed_distance(p, l1, l2, &x0));
                push(Residual::SideDistance(p, l1, l2, s, value()?));
            }
            K::HorizontalDistance { a, b: q, .. } => {
                let (p, q) = (b.point(o, *a)?, b.point(o, *q)?);
                let s = sign_of(x0[q.0] - x0[p.0]);
                push(Residual::SignedDiff(q.0, p.0, s, value()?));
            }
            K::VerticalDistance { a, b: q, .. } => {
                let (p, q) = (b.point(o, *a)?, b.point(o, *q)?);
                let s = sign_of(x0[q.0 + 1] - x0[p.0 + 1]);
                push(Residual::SignedDiff(q.0 + 1, p.0 + 1, s, value()?));
            }
            K::Length { line, .. } => {
                let (p, q) = b.line(o, *line)?;
                push(Residual::Len(p, q, value()?));
            }
            K::LinearDiameter { point, line, .. } => {
                let (p, (l1, l2)) = (b.point(o, *point)?, b.line(o, *line)?);
                let s = sign_of(signed_distance(p, l1, l2, &x0));
                push(Residual::SideDistance(p, l1, l2, s, value()? / 2.0));
            }
            K::CircleDistance {
                circle, other, far, ..
            } => {
                let (c, r1) = b.round(o, *circle)?;
                let v = value()?;
                let r0 = r1.get(&x0);
                // 近い側の式は、保存されている位置での外・内で選ぶ(値を変えても跳ばない)
                let r = match b.kinds.get(other).map(|k| k.1) {
                    Some(Entity::Point { .. }) => {
                        let q = b.point(o, *other)?;
                        let d0 = dist(c.get(&x0), q.get(&x0));
                        let (sd, k) = gap_signs(*far, d0, r0);
                        Residual::Gap(c, q, None, sd, r1, k, None, v)
                    }
                    Some(Entity::Line { .. }) => {
                        let (q, q2) = b.line(o, *other)?;
                        let s0 = signed_distance(c, q, q2, &x0);
                        let (sd, k) = gap_signs(*far, s0.abs(), r0);
                        Residual::Gap(c, q, Some(q2), sd * sign_of(s0), r1, k, None, v)
                    }
                    _ => {
                        let (c2, r2) = b.round(o, *other)?;
                        let d0 = dist(c.get(&x0), c2.get(&x0));
                        let q0 = r2.get(&x0);
                        let (sd, k1, k2) = if *far {
                            (1.0, 1.0, 1.0)
                        } else if d0 >= r0 + q0 {
                            (1.0, -1.0, -1.0)
                        } else if d0 <= (r0 - q0).abs() {
                            let t = sign_of(r0 - q0);
                            (-1.0, t, -t)
                        } else {
                            (-1.0, 1.0, 1.0)
                        };
                        Residual::Gap(c, c2, None, sd, r1, k1, Some((r2, k2)), v)
                    }
                };
                push(r);
            }
            K::Radius { circle, .. } => {
                let (_, r) = b.round(o, *circle)?;
                push(Residual::Rad(r, 1.0, value()?));
            }
            K::Diameter { circle, .. } => {
                let (_, r) = b.round(o, *circle)?;
                push(Residual::Rad(r, 2.0, value()?));
            }
            K::Angle { a, b: l2, .. } => {
                let ((p, q), (r, s)) = (b.line(o, *a)?, b.line(o, *l2)?);
                // 角度の寸法は向きのない 2 本の線の角(0〜π で測る)。保存されている位置の向きつきの
                // 角度 θ0 を、値の変化だけ回した向きを目標にする(30° を 150° にすると、線は 120° 回る。
                // -30° へ裏返らない)。測った値が 0 と π の境目にあれば、値に近い側で測る
                let v = value()?;
                let t0 = turn(p, q, r, s, &x0);
                let mut m0 = t0.rem_euclid(std::f64::consts::PI);
                for k in [-1.0, 1.0] {
                    let m = m0 + k * std::f64::consts::PI;
                    if (v - m).abs() < 1e-3 && (v - m0).abs() > 1e-3 {
                        m0 = m;
                    }
                }
                push(Residual::Angle(p, q, r, s, t0 + (v - m0)));
            }
            K::PointOnCurve { point, curve } => {
                let p = b.point(o, *point)?;
                match b.curve(o, *curve)? {
                    CurveDef::Line(a, q) => push(Residual::OnLine(p, a, q)),
                    CurveDef::Circle(c, r) => push(Residual::OnCircle(p, c, r)),
                    CurveDef::Arc(c, s, _) => push(Residual::OnCircle(p, c, Radius::Arc(c, s))),
                    CurveDef::Ellipse(c, ma, mi, _) => push(Residual::OnEllipse(c, ma, mi, p)),
                    def => {
                        // 曲線のパラメータを補助の変数にして、その点と一致させる(微分できる形)
                        let (px, py) = p.get(&x0);
                        let t0 = def.shape(&x0).map_or(0.0, |s| s.closest([px, py]).0);
                        let tv = x0.len();
                        x0.push(t0);
                        let def = b.share(*curve, def);
                        push(Residual::OnCurve(def.clone(), tv, p, true));
                        push(Residual::OnCurve(def, tv, p, false));
                    }
                }
            }
            K::TangentCurve { a, b: e2 } => {
                let (da, db) = (b.curve(o, *a)?, b.curve(o, *e2)?);
                let (da, db) = (b.share(*a, da), b.share(*e2, db));
                if let Some((_, at_a, at_b)) = b.joint(*a, *e2) {
                    // 端を共有していれば、そこで向きがそろう(折り返さない)
                    push(Residual::Turn(da.clone(), at_a, db.clone(), at_b));
                    keep(Trait::TangentDot(da, at_a, db, at_b));
                } else {
                    // 触れる所を両方の曲線のパラメータ(補助の変数)で持ち、そこで点と向きがそろう
                    let (ta, tb) = closest_pair(da.shape(&x0), db.shape(&x0));
                    let (va, vb) = (x0.len(), x0.len() + 1);
                    x0.extend([ta, tb]);
                    push(Residual::Meet(da.clone(), va, db.clone(), vb, true));
                    push(Residual::Meet(da.clone(), va, db.clone(), vb, false));
                    push(Residual::Turn(da, At::Var(va), db, At::Var(vb)));
                }
            }
            K::Curvature { a, b: e2 } => {
                let (da, db) = (b.curve(o, *a)?, b.curve(o, *e2)?);
                let (da, db) = (b.share(*a, da), b.share(*e2, db));
                let Some((_, at_a, at_b)) = b.joint(*a, *e2) else {
                    return Err(SketchError::BadReference {
                        owner: owner.clone(),
                        target: *e2,
                        expected: "端を共有する曲線",
                    });
                };
                // つなぎ目を a から b へ通る向きで測る(逆にたどる曲線は曲率の符号が変わる)
                let sa = if at_a == At::End { 1.0 } else { -1.0 };
                let sb = if at_b == At::Start { 1.0 } else { -1.0 };
                // 曲率は 1/mm なので、形の大きさを掛けてほかの式と同じくらいの大きさにする
                let size = da.shape(&x0).map_or(1.0, |s| s.size())
                    + db.shape(&x0).map_or(1.0, |s| s.size());
                let k = (size / 2.0).clamp(1.0, 1e4);
                push(Residual::Turn(da.clone(), at_a, db.clone(), at_b));
                push(Residual::Bend(
                    da.clone(),
                    at_a,
                    sa,
                    db.clone(),
                    at_b,
                    sb,
                    k,
                ));
                keep(Trait::TangentDot(da, at_a, db, at_b));
            }
            K::Offset { parts } => {
                // 距離はまとまりで 1 つの補助の変数(初めの値は今の形で測る)
                let d = x0.len();
                let mut d0 = None;
                let mut rows: Vec<(usize, Residual)> = vec![];
                for (i, part) in parts.iter().enumerate() {
                    match part {
                        OffsetPart::Parallel { from, to } => {
                            let ((p, q), (r, s)) = (b.line(o, *from)?, b.line(o, *to)?);
                            rows.push((i, Residual::Cross(p, q, r, s)));
                            traits.push(Trait::DotSign(p, q, r, s));
                        }
                        OffsetPart::Distance { from, to } => {
                            let ((l1, l2), (p, _)) = (b.line(o, *from)?, b.line(o, *to)?);
                            let sd = signed_distance(p, l1, l2, &x0);
                            d0.get_or_insert(sd.abs());
                            rows.push((i, Residual::SideDistanceVar(p, l1, l2, sign_of(sd), d)));
                        }
                        OffsetPart::Radius { from, to } => {
                            let ((_, r1), (_, r2)) = (b.round(o, *from)?, b.round(o, *to)?);
                            let gap = r2.get(&x0) - r1.get(&x0);
                            d0.get_or_insert(gap.abs());
                            rows.push((i, Residual::RadiusGap(r2, Some(r1), sign_of(gap), d)));
                        }
                        OffsetPart::Corner { arc } => {
                            let (_, r) = b.round(o, *arc)?;
                            d0.get_or_insert(r.get(&x0));
                            rows.push((i, Residual::RadiusGap(r, None, 1.0, d)));
                        }
                        OffsetPart::Tangent { a, b: e2 } => {
                            let Some(at) = b.shared_end(*a, *e2) else {
                                continue;
                            };
                            match (b.line(o, *a), b.line(o, *e2)) {
                                (Ok((p, q)), Err(_)) | (Err(_), Ok((p, q))) => {
                                    let other = if b.line(o, *a).is_ok() { *e2 } else { *a };
                                    let (c, _) = b.round(o, other)?;
                                    rows.push((i, Residual::Dot(c, at, p, q)));
                                    traits.push(Trait::Side(c, p, q));
                                }
                                (Err(_), Err(_)) => {
                                    let ((c1, _), (c2, _)) = (b.round(o, *a)?, b.round(o, *e2)?);
                                    rows.push((i, Residual::Cross(c1, at, c2, at)));
                                    traits.push(Trait::DotSign(at, c1, at, c2));
                                }
                                (Ok(_), Ok(_)) => continue,
                            }
                        }
                    }
                }
                x0.push(d0.unwrap_or(0.0));
                traits.push(Trait::Positive(d));
                for (i, r) in rows {
                    part_rows.push((equations.len(), c.id, i));
                    equations.push((tag, r));
                }
            }
            K::Polygon { circle, sides } => {
                let (c, r) = b.round(o, *circle)?;
                let lines = sides
                    .iter()
                    .map(|l| b.line(o, *l))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut corners: Vec<usize> = vec![];
                for (p, q) in &lines {
                    for v in [p.0, q.0] {
                        if !corners.contains(&v) {
                            corners.push(v);
                        }
                    }
                }
                for v in corners {
                    push(Residual::OnCircle(Pt(v), c, r));
                }
                if let Some(&(p, q)) = lines.first() {
                    for &(r2, s2) in &lines[1..] {
                        push(Residual::EqualLen(p, q, r2, s2));
                    }
                }
            }
            K::Pattern { pattern } => {
                let Some(links) = sketch.pattern_links(*pattern) else {
                    continue;
                };
                for (place, pairs) in links {
                    for (from, to) in pairs {
                        match (
                            b.kinds.get(&from).map(|k| k.0),
                            b.kinds.get(&to).map(|k| k.0),
                        ) {
                            (Some(Kind::Point), Some(Kind::Point)) => {
                                let (q, p) = (b.points[&from], b.points[&to]);
                                for is_x in [true, false] {
                                    push(match place {
                                        Placement::Turn(center, a) => {
                                            let (c, at) = match center {
                                                PatternCenter::Point(id) => {
                                                    (Some(b.point(o, id)?), [0.0, 0.0])
                                                }
                                                PatternCenter::At(at) => (None, at),
                                            };
                                            Residual::Turned(p, q, c, at, a.cos(), a.sin(), is_x)
                                        }
                                        Placement::Shift(dirs) => {
                                            let mut sh = [Shift {
                                                line: None,
                                                unit: [1.0, 0.0],
                                                amount: 0.0,
                                            };
                                                2];
                                            for (k, (axis, amount)) in dirs.into_iter().enumerate()
                                            {
                                                sh[k] = match axis {
                                                    PatternAxis::Angle(t) => Shift {
                                                        line: None,
                                                        unit: [t.cos(), t.sin()],
                                                        amount,
                                                    },
                                                    PatternAxis::Line(l) => Shift {
                                                        line: Some(b.line(o, l)?),
                                                        unit: [1.0, 0.0],
                                                        amount,
                                                    },
                                                };
                                            }
                                            Residual::Shifted(p, q, sh, is_x)
                                        }
                                    });
                                }
                            }
                            // 円の大きさ(円弧・楕円などは点で決まる)
                            (Some(Kind::Circle), Some(Kind::Circle)) => {
                                push(Residual::EqualRadius(b.radii[&to], b.radii[&from]));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok(System {
        x0,
        equations,
        points: b.points,
        circles,
        entity_vars,
        traits,
        rounds: {
            let mut r: Vec<(EntityId, Radius)> = b.radii.iter().map(|(k, v)| (*k, *v)).collect();
            r.sort_by_key(|(k, _)| *k);
            r.into_iter().map(|(_, v)| v).collect()
        },
        part_rows,
    })
}

/// まとまりの拘束 `id`(オフセット)の関係のうち、ほかの式で既に決まる(余分な)関係の番号。
/// 関係を順に足し、ヤコビ行列の階数が増えないものを余分とする。
pub(crate) fn redundant_parts(
    sketch: &Sketch,
    values: &HashMap<ConstraintId, f64>,
    id: ConstraintId,
) -> Result<Vec<usize>, SketchError> {
    let sys = build(sketch, values)?;
    let eqs = &sys.equations;
    let j = jacobian(eqs, &sys.x0);
    let mine: Vec<(usize, usize)> = sys
        .part_rows
        .iter()
        .filter(|r| r.1 == id)
        .map(|r| (r.0, r.2))
        .collect();
    let own: BTreeSet<usize> = mine.iter().map(|r| r.0).collect();
    let mut rows: Vec<usize> = (0..eqs.len()).filter(|i| !own.contains(i)).collect();
    let mut current = rank(&j.select_rows(rows.iter()));
    let mut out = vec![];
    for (row, part) in mine {
        rows.push(row);
        let r = rank(&j.select_rows(rows.iter()));
        if r > current {
            current = r;
        } else {
            rows.pop();
            out.push(part);
        }
    }
    Ok(out)
}

/// 2 つの曲線のいちばん近い所のパラメータ(接する拘束の補助の変数の初めの値)。
fn closest_pair(a: Option<Shape>, b: Option<Shape>) -> (f64, f64) {
    let (Some(a), Some(b)) = (a, b) else {
        return (0.0, 0.0);
    };
    let size = a.size().max(b.size()).max(1e-3);
    let mut best = (a.domain().0, b.domain().0, f64::INFINITY);
    for (t, p) in a.polyline(0.01 * size) {
        let (u, d) = b.closest(p);
        if d < best.2 {
            best = (t, u, d);
        }
    }
    let (mut ta, mut tb) = (best.0, best.1);
    for _ in 0..8 {
        ta = a.closest(b.at(tb)).0;
        tb = b.closest(a.at(ta)).0;
    }
    (ta, tb)
}

/// スケッチを解く。`values` は寸法の値(長さは mm、角度は rad)。
///
/// 保存されている位置から一度に解いて形の性質([`Trait`])が変わったら(寸法を大きく変えて
/// 長穴の線が交差した、円弧が裏返ったなど)、保存されている位置での残差を少しずつ 0 へ
/// 近づけて解き直す(ニュートン・ホモトピー。方式案 3.1)。それでも保てなければ、一度に
/// 解いた形を返す。
pub fn solve(
    sketch: &Sketch,
    values: &HashMap<ConstraintId, f64>,
) -> Result<Solution, SketchError> {
    let sys = build(sketch, values)?;
    let (x, iterations) = solve_values(&sys);
    Ok(finish(sketch, &sys, x, iterations))
}

/// ドラッグの途中で解く。`start` は押した時の形(解いた位置を写したもの)、`to` はそこから
/// 引いた要素の位置をカーソルへ書き換えた形(要素も拘束も同じもの)、`last` は前のフレームで
/// 解けた形(なければ `start`)。`held` の要素(点、円は中心と半径、線分・円弧は端と中心)は
/// なるべく動かさず、ほかの要素を動かして拘束を満たす。引いた所がカーソルについてくる。
/// 拘束で動けない向きには、引いた要素も戻る(長方形の辺は、拘束が許す向きにだけ動く)。
///
/// `last` から `to` へ少しずつ動かし、1 歩ごとに前の歩の解から、その形にいちばん近い解を
/// 求める(引いていない円・円弧の半径は、押した時の値になるべく保つ)。形の性質が変わる歩は
/// 刻みを小さくし、それでも変わるなら、そこで止めた形を返す(長穴の線が交差する、
/// 円弧が裏返るなどの崩れた形にはしない)。
///
/// 動かす範囲は、引いた要素の近くから順に広げる([`drag_tiers`])。引いた要素がカーソルに
/// 付いて来られるいちばん狭い範囲で解き、その外の要素は動かさない(長穴の片方の中心を
/// 引いても、もう片方の中心は動かない)。どの範囲でも付いて来られなければ、全部を動かせる
/// ものとして解く(拘束で動けない向きには、引いた要素も戻る)。
pub fn solve_dragging(
    start: &Sketch,
    to: &Sketch,
    values: &HashMap<ConstraintId, f64>,
    held: &BTreeSet<EntityId>,
    last: Option<&Solution>,
) -> Result<Solution, SketchError> {
    let sys = build(start, values)?;
    let target = build(to, values)?.x0;
    if target.len() != sys.x0.len() {
        return Err(SketchError::BadReference {
            owner: "ドラッグ".into(),
            target: EntityId(0),
            expected: "同じ要素の形",
        });
    }
    // 変数ごとの重み。重みで変数を測り直して最小ノルムで解くと、重みの小さい変数ほど動かない
    let mut weight = vec![1.0; sys.x0.len()];
    let mut held_vars = vec![];
    for id in held {
        for &v in sys.entity_vars.get(id).into_iter().flatten() {
            weight[v] = HELD_WEIGHT;
            held_vars.push(v);
        }
    }
    let mut from = sys.x0.clone();
    if let Some(sol) = last {
        for (id, p) in &sys.points {
            if let Some(q) = sol.points.get(id) {
                from[p.0] = q[0];
                from[p.0 + 1] = q[1];
            }
        }
        for (id, &i) in &sys.circles {
            if let Some(r) = sol.radii.get(id) {
                from[i] = *r;
            }
        }
    }
    // 引いた点の動く量(mm)。付いて来たかの判定の許容に使う
    let reach = held_vars
        .iter()
        .map(|&v| (target[v] - from[v]).abs())
        .fold(0.0, f64::max);
    let follows = |x: &[f64]| {
        held_vars
            .iter()
            .all(|&v| (x[v] - target[v]).abs() <= 1e-6 + 1e-4 * reach)
    };
    let mut iterations = 0;
    let dist = drag_tiers(start, &sys, held);
    // 引いた要素と式でつながらない変数は、どう解いても動かないので外して解く
    for (w, d) in weight.iter_mut().zip(&dist) {
        if d.is_none() {
            *w = 0.0;
        }
    }
    if reach > 0.0 {
        let mut limits: Vec<Hops> = dist.iter().flatten().copied().collect();
        limits.sort_unstable();
        limits.dedup();
        for limit in limits.into_iter().take(MAX_TIERS) {
            // 範囲の外の変数は重み 0(動かさない)
            let w: Vec<f64> = weight
                .iter()
                .zip(&dist)
                .map(|(w, d)| {
                    if d.is_some_and(|d| d <= limit) {
                        *w
                    } else {
                        0.0
                    }
                })
                .collect();
            let (x, k) = solve_moving(&sys, &from, &target, &w);
            iterations += k;
            if follows(&x) {
                return Ok(finish(start, &sys, x, iterations));
            }
        }
    }
    let (x, k) = solve_moving(&sys, &from, &target, &weight);
    Ok(finish(start, &sys, x, iterations + k))
}

/// 引いた要素からの隔たり。(拘束・補助線を渡った回数, 要素を渡った回数)の辞書順で比べる。
type Hops = (u32, u32);
/// ドラッグで、動かす範囲を広げて試す回数の上限(超えたら全部を動かせるものとして解く)
const MAX_TIERS: usize = 12;

/// 変数ごとの、引いた要素(`held`)からの隔たり(つながっていなければ `None`)。
/// 線分・円弧・円などの要素は、使う点の間を 1 つ渡る。一致の拘束で重ねた点は同じ所に数える。
/// ほかの拘束と補助線は、要素をいくつ渡るよりも遠く数える(拘束でつながっているだけの物は、
/// 形でつながった物を動かしても足りない時にだけ動かす)。
fn drag_tiers(sketch: &Sketch, sys: &System, held: &BTreeSet<EntityId>) -> Vec<Option<Hops>> {
    let n = sys.x0.len();
    // (変数の組, 渡る重さ)
    let mut edges: Vec<(Vec<usize>, Hops)> = vec![];
    for e in &sketch.entities {
        let Some(vars) = sys.entity_vars.get(&e.id()) else {
            continue;
        };
        let cost = match e {
            Entity::Point { .. } => (0, 0),
            _ if e.is_construction() => (1, 0),
            _ => (0, 1),
        };
        edges.push((vars.clone(), cost));
    }
    // 拘束は、式が実際に使う変数でつなぐ
    let coincident: BTreeSet<ConstraintId> = sketch
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Coincident { .. }))
        .map(|c| c.id)
        .collect();
    for (id, r) in &sys.equations {
        let mut vars = vec![];
        r.vars(&mut vars);
        let cost = match id {
            None => (0, 1),
            Some(id) if coincident.contains(id) => (0, 0),
            Some(_) => (1, 0),
        };
        edges.push((vars, cost));
    }
    let mut touching: Vec<Vec<usize>> = vec![vec![]; n];
    for (k, (vars, _)) in edges.iter().enumerate() {
        for &v in vars {
            if v < n {
                touching[v].push(k);
            }
        }
    }
    let mut dist: Vec<Option<Hops>> = vec![None; n];
    let mut queue = std::collections::BinaryHeap::new();
    for id in held {
        for &v in sys.entity_vars.get(id).into_iter().flatten() {
            dist[v] = Some((0, 0));
            queue.push(std::cmp::Reverse(((0, 0), v)));
        }
    }
    while let Some(std::cmp::Reverse((d, v))) = queue.pop() {
        if dist[v].is_some_and(|best| best < d) {
            continue;
        }
        for &k in &touching[v] {
            let (vars, cost) = &edges[k];
            let next = (d.0 + cost.0, d.1 + cost.1);
            for &u in vars {
                if u < n && dist[u].is_none_or(|best| next < best) {
                    dist[u] = Some(next);
                    queue.push(std::cmp::Reverse((next, u)));
                }
            }
        }
    }
    dist
}

/// 解いた位置から、拘束の状態・自由度・決まった要素を求めて解にまとめる。
fn finish(sketch: &Sketch, sys: &System, x: Vec<f64>, iterations: usize) -> Solution {
    let eqs = &sys.equations;
    let f = residuals(eqs, &x);
    let max_residual = max_abs(&f);
    let converged = max_residual <= RESIDUAL_OK;

    let j = jacobian(eqs, &x);
    let space = RowSpace::of(&j);
    let r = space.rank();
    let dof = x.len() - r;

    // 拘束を順に足し、行の数だけ空間が広がらないものを余分な拘束とする(重なりを許す種類は、
    // 1 行も広がらない時だけ。[`ConstraintKind::tolerates_overlap`])
    let overlap_ok: BTreeSet<ConstraintId> = sketch
        .constraints
        .iter()
        .filter(|c| c.kind.tolerates_overlap())
        .map(|c| c.id)
        .collect();
    let mut redundant = Vec::new();
    let mut span = RowSpace::empty_like(&j);
    let mut i = 0;
    while i < eqs.len() {
        let tag = eqs[i].0;
        let mut k = i;
        while k < eqs.len() && eqs[k].0 == tag {
            k += 1;
        }
        let mut trial = span.clone();
        let added = (i..k)
            .filter(|&row| trial.add(j.row(row).transpose()))
            .count();
        if let Some(id) = tag
            && (added == 0 || (added < k - i && !overlap_ok.contains(&id)))
        {
            redundant.push(id);
        }
        if added > 0 {
            span = trial;
        }
        i = k;
    }

    let status = if !converged {
        let mut conflicting: Vec<ConstraintId> = eqs
            .iter()
            .filter(|(_, r)| r.eval(&x).abs() > RESIDUAL_OK)
            .filter_map(|(id, _)| *id)
            .chain(redundant.iter().copied())
            .collect();
        conflicting.sort();
        conflicting.dedup();
        SolveStatus::Inconsistent { conflicting }
    } else if !redundant.is_empty() {
        SolveStatus::OverConstrained { redundant }
    } else if dof > 0 {
        SolveStatus::UnderConstrained { dof }
    } else {
        SolveStatus::WellConstrained
    };

    // 変数 v が決まっている ⇔ 単位ベクトル e_v が J の行空間にある
    let determined: Vec<bool> = (0..x.len()).map(|v| space.has_unit(v, x.len())).collect();
    let fixed = sys
        .entity_vars
        .iter()
        .filter(|(_, vars)| vars.iter().all(|&v| determined[v]))
        .map(|(id, _)| *id)
        .collect();

    let mut sol = Solution {
        status,
        dof,
        points: sys
            .points
            .iter()
            .map(|(id, p)| (*id, [x[p.0], x[p.0 + 1]]))
            .collect(),
        radii: sys.circles.iter().map(|(id, &i)| (*id, x[i])).collect(),
        fixed,
        lone: sketch.lone_points(),
        iterations,
        max_residual,
        measured: BTreeMap::new(),
    };
    sol.measured = sketch
        .constraints
        .iter()
        .filter(|c| c.reference)
        .filter_map(|c| Some((c.id, crate::dimension::measure(sketch, &sol, &c.kind)?)))
        .collect();
    sol
}

/// 拘束 `candidate`(寸法なら値 `value`)を足すと、ほかの拘束で既に決まっていて余分になるか。
/// 余分なら、それを決めている拘束(1 つ外すと決まらなくなるもの。拘束の順)を返す。
/// 決まっていなければ `None`。`sketch` の座標は解いた位置を写したもの
/// ([`Sketch::adopt_solution`])を渡す。その位置でのヤコビ行列の階数で調べる。
pub(crate) fn determined_by(
    sketch: &Sketch,
    values: &HashMap<ConstraintId, f64>,
    candidate: &ConstraintKind,
    value: Option<f64>,
) -> Result<Option<Vec<ConstraintId>>, SketchError> {
    let mut s = sketch.clone();
    let id = ConstraintId(s.constraints.iter().map(|c| c.id.0).max().unwrap_or(0) + 1);
    s.constraints.push(Constraint::new(id, candidate.clone()));
    let mut values = values.clone();
    if let Some(v) = value {
        values.insert(id, v);
    }
    let sys = build(&s, &values)?;
    let eqs = &sys.equations;
    let j = jacobian(eqs, &sys.x0);
    // 印が `keep` に合う行の張る空間に、候補の行を足すと広がるか(候補ごと・拘束ごとに SVD を
    // し直さない。RowSpace)
    let mine: Vec<usize> = (0..eqs.len()).filter(|&i| eqs[i].0 == Some(id)).collect();
    let empty = RowSpace::empty_like(&j);
    let grows = |keep: &dyn Fn(Option<ConstraintId>) -> bool| -> bool {
        let mut span = empty.clone();
        for i in (0..eqs.len()).filter(|&i| eqs[i].0 != Some(id) && keep(eqs[i].0)) {
            span.add(j.row(i).transpose());
        }
        mine.iter().any(|&i| span.add(j.row(i).transpose()))
    };
    if grows(&|_| true) {
        return Ok(None);
    }
    let mut tags: Vec<ConstraintId> = eqs.iter().filter_map(|(t, _)| *t).collect();
    tags.dedup();
    tags.retain(|t| *t != id);
    let by = tags
        .into_iter()
        .filter(|&t| grows(&|x| x != Some(t)))
        .collect();
    Ok(Some(by))
}

/// 拘束 `candidate`(寸法でないもの)を今の形に足すと余分になるか。余分なら、それを決めている
/// 拘束を返す(足す前に「既に決まっています」と知らせるのに使う)。解けなければ `None`。
pub fn redundant_with(
    sketch: &Sketch,
    values: &HashMap<ConstraintId, f64>,
    candidate: &ConstraintKind,
) -> Option<Vec<ConstraintId>> {
    let sol = solve(sketch, values).ok()?;
    let mut solved = sketch.clone();
    solved.adopt_solution(&sol);
    determined_by(&solved, values, candidate, None)
        .ok()
        .flatten()
}

/// [`independent`] の結果。
pub(crate) struct Independent {
    /// 残した候補の番号(渡した順)
    pub kept: Vec<usize>,
    /// 候補を足す前と、残した物を足した後の自由度
    pub dof_before: usize,
    pub dof_after: usize,
}

/// 候補の拘束(寸法なら今の形で測った値と組)を渡した順に 1 つずつ試し、足すと式の数だけ
/// 階数が上がる(自由度が減り、余分にならない)物だけを残す(自動拘束)。順は呼ぶ側の
/// 種類の表で 1 回決め、ここでは並べ直さない(docs/避ける形.md の自動の拘束 (b))。
/// 形は動かさず、今の位置でのヤコビ行列の行を、それまでに残した行の張る空間から外して
/// 残りが有るかで見る(候補ごとに SVD をし直さない)。
pub(crate) fn independent(
    sketch: &Sketch,
    values: &HashMap<ConstraintId, f64>,
    candidates: &[(ConstraintKind, Option<f64>)],
) -> Result<Independent, SketchError> {
    let mut s = sketch.clone();
    let mut values = values.clone();
    let base = s.constraints.iter().map(|c| c.id.0).max().unwrap_or(0) + 1;
    let ids: Vec<ConstraintId> = (0..candidates.len())
        .map(|i| ConstraintId(base + i as u32))
        .collect();
    for ((kind, value), id) in candidates.iter().zip(&ids) {
        s.constraints.push(Constraint::new(*id, kind.clone()));
        if let Some(v) = value {
            values.insert(*id, *v);
        }
    }
    let sys = build(&s, &values)?;
    let j = jacobian(&sys.equations, &sys.x0);
    let n = j.ncols();
    let row = |i: usize| -> Vec<f64> { (0..n).map(|c| j[(i, c)]).collect() };
    let mut basis: Vec<Vec<f64>> = Vec::new();
    // `v` を基底の張る空間から外した残り(2 回外して丸めの誤差を抑える)。残りが無ければ `None`
    let reduce = |basis: &[Vec<f64>], mut v: Vec<f64>| -> Option<Vec<f64>> {
        let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-12 {
            return None;
        }
        v.iter_mut().for_each(|x| *x /= norm);
        for _ in 0..2 {
            for b in basis {
                let d: f64 = v.iter().zip(b).map(|(x, y)| x * y).sum();
                v.iter_mut().zip(b).for_each(|(x, y)| *x -= d * y);
            }
        }
        let rest = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        (rest > INDEPENDENT_TOL).then(|| v.into_iter().map(|x| x / rest).collect())
    };
    let first = ConstraintId(base);
    let is_candidate = |t: Option<ConstraintId>| t.is_some_and(|t| t.0 >= first.0);
    for (i, (tag, _)) in sys.equations.iter().enumerate() {
        if !is_candidate(*tag)
            && let Some(v) = reduce(&basis, row(i))
        {
            basis.push(v);
        }
    }
    let dof_before = n - basis.len();
    let mut kept = Vec::new();
    for (k, id) in ids.iter().enumerate() {
        let rows: Vec<usize> = (0..sys.equations.len())
            .filter(|&i| sys.equations[i].0 == Some(*id))
            .collect();
        if rows.is_empty() {
            continue;
        }
        let mut trial = basis.clone();
        let all = rows.iter().all(|&i| match reduce(&trial, row(i)) {
            Some(v) => {
                trial.push(v);
                true
            }
            None => false,
        });
        if all {
            basis = trial;
            kept.push(k);
        }
    }
    Ok(Independent {
        kept,
        dof_before,
        dof_after: n - basis.len(),
    })
}

/// [`independent`] で、行が残りを持つとみなす大きさ(長さ 1 にした行の、外した後の残り)。
/// 差分で求めたヤコビ行列の誤差より十分大きく、ほぼ平行な別の拘束より十分小さい
const INDEPENDENT_TOL: f64 = 1e-6;
