//! 自動拘束。
//! 描き終えた図や読み込んだ DXF に、足りない拘束と寸法の候補を作る。AI は使わず、決まった
//! 規則で作る。
//!
//! 1. 拘束の候補は「今の位置で既に成り立っている」物だけ。種類の表の順(一致 → 線上・円周上 →
//!    水平・垂直 → 接する → 同心 → 平行・直角)で、同じ種類の中は要素の番号の順。
//! 2. 寸法の候補(頼まれた時だけ)は、円の直径・円弧の半径・線分の長さ、最後に残った点の
//!    基準の点(原点か、利用者が選んだ点)からの横と縦の距離。値は今の形で測った値。
//! 3. 候補を上の順に 1 つずつ試し、足すと自由度が減る(余分にならない)物だけを残す
//!    ([`crate::solver`] の `independent`)。
//!
//! 避ける形(docs/避ける形.md の自動の拘束): 等しい(長さ・半径)は候補に
//! しない。独立の命令にし、引いている最中には走らせない。付けた物は普通の
//! 硬い拘束。順は表で 1 回決め、選んだ要素からの距離で並べ直さない。
//! 近い点の組から代表を選んで他の関係を捨てる作りにしない。寸法を全域木で
//! 選ばず、付けた寸法を自動で名前付きのパラメータにしない。

use crate::dimension::measure;
use crate::ends::{on_curve, tied_groups, to_curve};
use crate::geom::{len, sub};
use crate::model::{ConstraintId, ConstraintKind as K, Entity, EntityId, Sketch};
use crate::solver::{SketchError, Solution, independent};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// 候補の種類(画面の一覧のまとまり。この順に試す)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AutoGroup {
    Coincident,
    OnCurve,
    HorizontalVertical,
    Tangent,
    Concentric,
    ParallelPerpendicular,
    /// 直径・半径・長さ
    Size,
    /// 基準の点からの横と縦の距離
    Position,
}

impl AutoGroup {
    pub const ALL: [AutoGroup; 8] = [
        AutoGroup::Coincident,
        AutoGroup::OnCurve,
        AutoGroup::HorizontalVertical,
        AutoGroup::Tangent,
        AutoGroup::Concentric,
        AutoGroup::ParallelPerpendicular,
        AutoGroup::Size,
        AutoGroup::Position,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AutoGroup::Coincident => "一致",
            AutoGroup::OnCurve => "線上・円周上",
            AutoGroup::HorizontalVertical => "水平・垂直",
            AutoGroup::Tangent => "接する",
            AutoGroup::Concentric => "同心",
            AutoGroup::ParallelPerpendicular => "平行・直角",
            AutoGroup::Size => "直径・半径・長さ",
            AutoGroup::Position => "基準の点からの位置",
        }
    }

    /// 寸法のまとまりか。
    pub fn is_dimension(self) -> bool {
        matches!(self, AutoGroup::Size | AutoGroup::Position)
    }
}

/// 自動拘束の指定。
#[derive(Debug, Clone, PartialEq)]
pub struct AutoOptions {
    /// 同じ位置・線の上とみなす距離(mm)
    pub gap: f64,
    /// 水平・垂直・接する・平行・直角とみなす角度(rad)
    pub angle: f64,
    /// 寸法の候補も作る
    pub dimensions: bool,
    /// 位置の寸法の基準の点(無ければ位置の寸法は作らない)
    pub datum: Option<EntityId>,
    /// 対象の要素(空なら全部)。写した要素は相手にだけなる
    pub only: BTreeSet<EntityId>,
}

/// 描く時の推測と同じ角度の幅(3°)。
pub const AUTO_ANGLE_DEG: f64 = 3.0;
/// 同じ位置とみなす既定の距離(mm)。
pub const AUTO_GAP: f64 = 0.001;

impl Default for AutoOptions {
    fn default() -> Self {
        AutoOptions {
            gap: AUTO_GAP,
            angle: AUTO_ANGLE_DEG.to_radians(),
            dimensions: true,
            datum: None,
            only: BTreeSet::new(),
        }
    }
}

/// 残した候補 1 つ。寸法なら `kind` の値に測った値の式が入る。
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub group: AutoGroup,
    pub kind: K,
}

/// 自動拘束の結果。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AutoResult {
    pub suggestions: Vec<Suggestion>,
    /// 付ける前と、全部を付けた後の自由度
    pub dof_before: usize,
    pub dof_after: usize,
}

type P = [f64; 2];

/// 2 つの向きのなす角の sin の大きさ(平行なら 0)。
fn sin_between(u: P, v: P) -> f64 {
    let (lu, lv) = (len(u), len(v));
    if lu < 1e-12 || lv < 1e-12 {
        return 1.0;
    }
    ((u[0] * v[1] - u[1] * v[0]) / (lu * lv)).abs()
}

/// 足りない拘束と寸法の候補を作り、足すと自由度が減る物だけを返す。`sol` は今の解、
/// `values` は今の寸法の値(ソルバに渡す値)。
pub fn auto_constrain(
    sketch: &Sketch,
    sol: &Solution,
    values: &HashMap<ConstraintId, f64>,
    opts: &AutoOptions,
) -> Result<AutoResult, SketchError> {
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    let cands = candidates(&s, sol, opts);
    let pairs: Vec<(K, Option<f64>)> = cands
        .iter()
        .map(|(_, k)| (k.clone(), k.value().and_then(|_| measure(&s, sol, k))))
        .collect();
    let picked = independent(&s, values, &pairs)?;
    // 付けた後に解き直して確かめることはしない(ソルバは拘束ごとに SVD をし直すので、線が数十本の
    // 図で遅い)。行を外す時の許容(`INDEPENDENT_TOL`)はソルバの階数の許容より厳しいので、
    // 残した物がソルバに余分と見られることはない
    let suggestions: Vec<Suggestion> = picked
        .kept
        .iter()
        .map(|&i| {
            let (group, kind) = &cands[i];
            Suggestion {
                group: *group,
                kind: with_measured(kind, pairs[i].1),
            }
        })
        .collect();
    Ok(AutoResult {
        suggestions,
        dof_before: picked.dof_before,
        dof_after: picked.dof_after,
    })
}

/// 候補を付けたスケッチ(形は今の解の位置)。付ける順は渡した順。取り消しは呼ぶ側で 1 回にする。
pub fn apply(sketch: &Sketch, sol: &Solution, picks: &[Suggestion]) -> Sketch {
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    for p in picks {
        s.add_constraint(p.kind.clone());
    }
    s
}

/// 測った値を寸法の式にした寸法(長さは mm、小数 3 桁まで)。
fn with_measured(kind: &K, v: Option<f64>) -> K {
    let mut k = kind.clone();
    if let (Some(slot), Some(v)) = (k.value_mut(), v) {
        let t = format!("{v:.3}");
        *slot = format!("{} mm", t.trim_end_matches('0').trim_end_matches('.'));
    }
    k
}

/// 決まった順の候補(まとまり、拘束)。
fn candidates(s: &Sketch, sol: &Solution, o: &AutoOptions) -> Vec<(AutoGroup, K)> {
    let (projected, projected_circles) = s.projected_parts();
    let fixed = |id: EntityId| {
        projected.contains(&id) || projected_circles.contains(&id) || s.is_projected(id)
    };
    // 対象: 指定が無ければ全部。指定した線の点も対象
    let mut only: BTreeSet<EntityId> = o.only.clone();
    for id in o.only.iter() {
        if let Some(e) = s.entity(*id) {
            only.extend(e.points());
        }
    }
    let target = |id: EntityId| !fixed(id) && (o.only.is_empty() || only.contains(&id));
    let at = |id: EntityId| sol.points.get(&id).copied();
    let groups = tied_groups(s);
    let group = |p: EntityId| groups.get(&p).copied().unwrap_or(p);
    let sin_tol = o.angle.sin();
    let mut out: Vec<(AutoGroup, K)> = Vec::new();

    let points: Vec<EntityId> = s
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Point { .. }))
        .map(|e| e.id())
        .collect();
    let centers: BTreeSet<EntityId> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Circle { center, .. } | Entity::Arc { center, .. } => Some(*center),
            _ => None,
        })
        .collect();
    // 点を使う曲線(同じ曲線の点どうしは一致にしない)
    let mut owners: BTreeMap<EntityId, BTreeSet<EntityId>> = BTreeMap::new();
    for e in &s.entities {
        for p in e.points() {
            owners.entry(p).or_default().insert(e.id());
        }
    }
    let share_curve = |p: EntityId, q: EntityId| {
        owners
            .get(&p)
            .zip(owners.get(&q))
            .is_some_and(|(a, b)| a.intersection(b).next().is_some())
    };

    // 1. 一致(円・円弧の中心どうしは同心にする)
    for (i, &p) in points.iter().enumerate() {
        for &q in &points[i + 1..] {
            if (!target(p) && !target(q))
                || group(p) == group(q)
                || (centers.contains(&p) && centers.contains(&q))
                || share_curve(p, q)
            {
                continue;
            }
            if let (Some(a), Some(b)) = (at(p), at(q))
                && len(sub(a, b)) <= o.gap
            {
                out.push((AutoGroup::Coincident, K::Coincident { a: p, b: q }));
            }
        }
    }
    // 2. 線上・円周上(端や点が別の線分・円・円弧の途中に乗る)
    let curves: Vec<&Entity> = s
        .entities
        .iter()
        .filter(|e| {
            matches!(
                e,
                Entity::Line { .. } | Entity::Circle { .. } | Entity::Arc { .. }
            )
        })
        .collect();
    for &p in &points {
        let Some(pp) = at(p) else { continue };
        for c in &curves {
            let cid = c.id();
            if (!target(p) && !target(cid)) || c.points().contains(&p) {
                continue;
            }
            if to_curve(s, sol, cid, pp).is_some_and(|d| d <= o.gap) {
                out.push((AutoGroup::OnCurve, on_curve(s, p, cid)));
            }
        }
    }
    // 3. 水平・垂直
    let lines: Vec<(EntityId, EntityId, EntityId)> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { id, a, b, .. } => Some((*id, *a, *b)),
            _ => None,
        })
        .collect();
    let dir = |a: EntityId, b: EntityId| Some(sub(at(b)?, at(a)?));
    for &(id, a, b) in &lines {
        if !target(id) {
            continue;
        }
        let Some(d) = dir(a, b) else { continue };
        if len(d) < 1e-9 {
            continue;
        }
        if sin_between(d, [1.0, 0.0]) <= sin_tol {
            out.push((AutoGroup::HorizontalVertical, K::Horizontal { line: id }));
        } else if sin_between(d, [0.0, 1.0]) <= sin_tol {
            out.push((AutoGroup::HorizontalVertical, K::Vertical { line: id }));
        }
    }
    // 端を分け合う(同じ点・一致でつながる・`gap` より近い)
    let meet = |p: EntityId, q: EntityId| {
        p == q
            || group(p) == group(q)
            || at(p)
                .zip(at(q))
                .is_some_and(|(a, b)| len(sub(a, b)) <= o.gap)
    };
    // 4. 接する(同じ点を端に持つ線分と円弧、円弧と円弧)。一致でつながるだけの別の点では、
    //    ソルバは接するを距離の式で書き、接する所で微分が 0 になって余分に見えるので作らない
    let arcs: Vec<(EntityId, EntityId, [EntityId; 2])> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Arc {
                id,
                center,
                start,
                end,
                ..
            } => Some((*id, *center, [*start, *end])),
            _ => None,
        })
        .collect();
    for &(l, a, b) in &lines {
        for &(c, center, ends) in &arcs {
            if !target(l) && !target(c) {
                continue;
            }
            let Some(cp) = at(center) else { continue };
            let touch = [a, b]
                .into_iter()
                .flat_map(|p| ends.into_iter().map(move |q| (p, q)))
                .find(|(p, q)| p == q);
            let Some((p, _)) = touch else { continue };
            let (Some(d), Some(pp)) = (dir(a, b), at(p)) else {
                continue;
            };
            // 円弧の接線は半径に直角: 線分の向きと半径の向きの cos が 0 に近い
            let r = sub(pp, cp);
            let cos = ((d[0] * r[0] + d[1] * r[1]) / (len(d) * len(r)).max(1e-12)).abs();
            if cos <= sin_tol {
                out.push((AutoGroup::Tangent, K::Tangent { line: l, circle: c }));
            }
        }
    }
    for (i, &(c1, m1, e1)) in arcs.iter().enumerate() {
        for &(c2, m2, e2) in &arcs[i + 1..] {
            if (!target(c1) && !target(c2)) || group(m1) == group(m2) {
                continue;
            }
            let touch = e1
                .into_iter()
                .flat_map(|p| e2.into_iter().map(move |q| (p, q)))
                .find(|(p, q)| p == q);
            let (Some((p, _)), Some(a), Some(b)) = (touch, at(m1), at(m2)) else {
                continue;
            };
            let Some(pp) = at(p) else { continue };
            // 接する所で、2 つの中心と接点が一直線に並ぶ
            if sin_between(sub(pp, a), sub(pp, b)) <= sin_tol {
                out.push((AutoGroup::Tangent, K::TangentCircles { a: c1, b: c2 }));
            }
        }
    }
    // 5. 同心(円・円弧の中心どうしが `gap` 以内)
    let rounds: Vec<(EntityId, EntityId)> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Circle { id, center, .. } | Entity::Arc { id, center, .. } => {
                Some((*id, *center))
            }
            _ => None,
        })
        .collect();
    for (i, &(c1, m1)) in rounds.iter().enumerate() {
        for &(c2, m2) in &rounds[i + 1..] {
            if (!target(c1) && !target(c2)) || m1 == m2 || group(m1) == group(m2) {
                continue;
            }
            if at(m1)
                .zip(at(m2))
                .is_some_and(|(a, b)| len(sub(a, b)) <= o.gap)
            {
                out.push((AutoGroup::Concentric, K::Concentric { a: c1, b: c2 }));
            }
        }
    }
    // 6. 平行・直角(端を分け合う 2 本だけ。全部の組を見ると候補が多すぎる)
    for (i, &(l1, a1, b1)) in lines.iter().enumerate() {
        for &(l2, a2, b2) in &lines[i + 1..] {
            if !target(l1) && !target(l2) {
                continue;
            }
            let shared = [a1, b1]
                .into_iter()
                .any(|p| [a2, b2].into_iter().any(|q| meet(p, q)));
            let (Some(d1), Some(d2)) = (dir(a1, b1), dir(a2, b2)) else {
                continue;
            };
            if !shared {
                continue;
            }
            let sin = sin_between(d1, d2);
            if sin <= sin_tol {
                out.push((
                    AutoGroup::ParallelPerpendicular,
                    K::Parallel { a: l1, b: l2 },
                ));
            } else if sin >= (std::f64::consts::FRAC_PI_2 - o.angle).sin() {
                out.push((
                    AutoGroup::ParallelPerpendicular,
                    K::Perpendicular { a: l1, b: l2 },
                ));
            }
        }
    }
    if !o.dimensions {
        return out;
    }
    // 7. 直径・半径・長さ(1 つずつ。等しいにまとめない)
    let value = String::new();
    for e in &s.entities {
        let id = e.id();
        if !target(id) {
            continue;
        }
        match e {
            Entity::Circle { .. } => out.push((
                AutoGroup::Size,
                K::Diameter {
                    circle: id,
                    value: value.clone(),
                },
            )),
            Entity::Arc { .. } => out.push((
                AutoGroup::Size,
                K::Radius {
                    circle: id,
                    value: value.clone(),
                },
            )),
            Entity::Line { .. } => out.push((
                AutoGroup::Size,
                K::Length {
                    line: id,
                    value: value.clone(),
                },
            )),
            _ => {}
        }
    }
    // 8. 基準の点からの横と縦の距離(点の番号の順)
    if let Some(d) = o.datum.filter(|d| at(*d).is_some()) {
        for &p in &points {
            if p == d || !target(p) || group(p) == group(d) {
                continue;
            }
            out.push((
                AutoGroup::Position,
                K::HorizontalDistance {
                    a: d,
                    b: p,
                    value: value.clone(),
                },
            ));
            out.push((
                AutoGroup::Position,
                K::VerticalDistance {
                    a: d,
                    b: p,
                    value: value.clone(),
                },
            ));
        }
    }
    out
}

/// 写した原点(無ければ足し、解にも位置 (0, 0) を入れる)。位置の寸法の基準に使う。
/// 原点は、どの平面でもスケッチの座標の (0, 0)。
pub fn with_origin(sketch: &mut Sketch, sol: &mut Solution) -> EntityId {
    let id = sketch.add_projection(&crate::origin_ref(&jc_core::Plane::XY));
    sol.points.entry(id).or_insert([0.0, 0.0]);
    id
}

/// 今の解で測った、形を決める寸法の値(ソルバに渡す値)。式を評価できない所(CLI・MCP の
/// 操作)で使う。解は寸法を満たしているので、式から求めた値と同じ。
pub fn measured_values(sketch: &Sketch, sol: &Solution) -> HashMap<ConstraintId, f64> {
    sketch
        .constraints
        .iter()
        .filter(|c| c.kind.value().is_some() && !c.reference)
        .filter_map(|c| Some((c.id, measure(sketch, sol, &c.kind)?)))
        .collect()
}
