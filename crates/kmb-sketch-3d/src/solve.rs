//! 3D スケッチの拘束を解く。点の x・y・z と円の半径を変数にし、拘束を残差の式にして、
//! ガウス・ニュートン法(擬似逆行列の一番小さい動き)で解く。決まっていない所は始めの位置から
//! できるだけ動かさない。解いた後、ヤコビ行列の階数から自由度と余分の拘束を求める。数値の部分は
//! [`kmb_sketch_solver::rows`](2D と同じく、作り直しは解き手の crate で比べる)。

use crate::faces::{Face3d, find_faces};
use crate::geom::{angle, cross, dot, len, newell, sub, unit};
use crate::model::{Entity3d, FaceRef, Kind3d, P3, PlaneRef, Sketch3d};
use crate::planes::{faces_of, loop_points, plane_at, plane_expr};
use kmb_sketch::{ConstraintId, EntityId, SketchError, SolveStatus};
use kmb_sketch_solver::rows::{
    Row, free_variables, jacobian, least_change, max_residual, redundant, residuals,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// 解いた結果。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Solution3d {
    pub points: BTreeMap<EntityId, P3>,
    pub radii: BTreeMap<EntityId, f64>,
    pub status: SolveStatus3,
    /// 位置がまだ決まっていない(動かせる)点と円
    pub free: BTreeSet<EntityId>,
    /// 見つけた面
    pub faces: Vec<Face3d>,
    /// 寸法の今の値(mm・ラジアン。被駆動の寸法の表示と、寸法を足す時の始めの値)
    pub measured: BTreeMap<ConstraintId, f64>,
    /// 作図面に描いた円の今の向き(作図面に付いて回る)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub normals: BTreeMap<EntityId, P3>,
    /// ボディから求めた線(番号ごとの折れ線。スケッチの座標)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub derived: BTreeMap<u32, Vec<Vec<P3>>>,
}

/// 解の状態(2D のスケッチと同じ分け方)。
pub type SolveStatus3 = SolveStatus;

pub const SOLUTION3D_TYPE: &str = "sketch3d.solution@1";

impl Solution3d {
    /// 点も円も無い(何も描いていない)か。状態の言葉は [`kmb_sketch::NO_SHAPE`] にする。
    pub fn is_blank(&self) -> bool {
        self.points.is_empty() && self.radii.is_empty()
    }

    pub fn to_value(&self) -> kmb_core::Value {
        kmb_core::Value::Custom {
            type_key: SOLUTION3D_TYPE.into(),
            data: serde_json::to_value(self).expect("JSON にできる"),
        }
    }

    pub fn from_value(v: &kmb_core::Value) -> Option<Self> {
        match v {
            kmb_core::Value::Custom { type_key, data } if type_key == SOLUTION3D_TYPE => {
                serde_json::from_value(data.clone()).ok()
            }
            _ => None,
        }
    }

    /// 面を線分の名前で探す。
    pub fn face(&self, f: &FaceRef) -> Option<&Face3d> {
        self.faces.iter().find(|x| x.lines == *f)
    }
}

/// 変数の並び。
struct Vars {
    point: HashMap<EntityId, usize>,
    radius: HashMap<EntityId, usize>,
    x0: Vec<f64>,
}

/// 式と、その式を出した拘束(`None` は面を平らに保つ内側の式)。
struct System {
    vars: Vars,
    rows: Vec<(Option<ConstraintId>, Row)>,
}

fn bad(owner: ConstraintId, target: EntityId, expected: &'static str) -> SketchError {
    SketchError::BadReference {
        owner: owner.to_string(),
        target,
        expected,
    }
}

fn build(sketch: &Sketch3d, values: &HashMap<ConstraintId, f64>) -> Result<System, SketchError> {
    let mut point = HashMap::new();
    let mut radius = HashMap::new();
    let mut x0 = Vec::new();
    for e in &sketch.entities {
        match e {
            Entity3d::Point { id, p } => {
                point.insert(*id, x0.len());
                x0.extend_from_slice(p);
            }
            Entity3d::Circle { id, radius: r, .. } => {
                radius.insert(*id, x0.len());
                x0.push(*r);
            }
            Entity3d::Line { .. } => {}
        }
    }
    let vars = Vars { point, radius, x0 };
    // 要素を確かめる
    let kinds: HashMap<EntityId, &Entity3d> = sketch.entities.iter().map(|e| (e.id(), e)).collect();
    for e in &sketch.entities {
        for p in e.points() {
            if !vars.point.contains_key(&p) {
                return Err(SketchError::BadReference {
                    owner: e.id().to_string(),
                    target: p,
                    expected: "点",
                });
            }
        }
    }
    let mut rows: Vec<(Option<ConstraintId>, Row)> = Vec::new();
    let pi = |id: EntityId| vars.point[&id];
    // 面の平面を変数から求める関数(輪の点の位置からニューウェルの法線と重心)
    let mut planes_used: BTreeSet<FaceRef> = BTreeSet::new();
    // 固定した点(円の固定は、中心を別に固定していれば半径だけを止める)
    let fixed_points: BTreeSet<EntityId> = sketch
        .constraints
        .iter()
        .filter(|c| !c.reference)
        .filter_map(|c| match c.kind {
            Kind3d::Fix { point } => Some(point),
            _ => None,
        })
        .collect();
    for c in sketch.constraints.iter().filter(|c| !c.reference) {
        let o = c.id;
        let point = |id: EntityId| -> Result<usize, SketchError> {
            match kinds.get(&id) {
                Some(Entity3d::Point { .. }) => Ok(pi(id)),
                _ => Err(bad(o, id, "点")),
            }
        };
        let line = |id: EntityId| -> Result<(usize, usize), SketchError> {
            match kinds.get(&id) {
                Some(Entity3d::Line { a, b, .. }) => Ok((pi(*a), pi(*b))),
                _ => Err(bad(o, id, "線分")),
            }
        };
        let circle = |id: EntityId| -> Result<(usize, usize), SketchError> {
            match kinds.get(&id) {
                Some(Entity3d::Circle { center, .. }) => Ok((pi(*center), vars.radius[&id])),
                _ => Err(bad(o, id, "円")),
            }
        };
        let value = || values.get(&o).copied().ok_or(SketchError::MissingValue(o));
        let plane = |p: &PlaneRef| -> Result<PlaneFn, SketchError> {
            let e = plane_expr(sketch, p, &|id| vars.point.get(&id).copied()).ok_or_else(|| {
                SketchError::BadReference {
                    owner: o.to_string(),
                    target: match p {
                        PlaneRef::Face { lines } => lines.0.first().copied().unwrap_or(EntityId(0)),
                        PlaneRef::Work { id } => EntityId(*id),
                        PlaneRef::Axis { .. } => EntityId(0),
                    },
                    expected: "平面(閉じた面の輪か作図面)",
                }
            })?;
            Ok(Box::new(move |x: &[f64]| e.eval(x)))
        };
        let mut used = |p: &PlaneRef| {
            planes_used.extend(faces_of(sketch, p));
        };
        let tag = Some(o);
        let mut push = |r: Row| rows.push((tag, r));
        let get = |x: &[f64], i: usize| -> P3 { [x[i], x[i + 1], x[i + 2]] };
        match &c.kind {
            Kind3d::Coincident { a, b } => {
                let (a, b) = (point(*a)?, point(*b)?);
                for k in 0..3 {
                    push(Box::new(move |x| x[a + k] - x[b + k]));
                }
            }
            Kind3d::Fix { point: id } => match kinds.get(id) {
                Some(Entity3d::Circle { center, .. }) => {
                    let (c, r) = circle(*id)?;
                    let x0 = &vars.x0;
                    if !fixed_points.contains(center) {
                        for k in 0..3 {
                            let v = x0[c + k];
                            push(Box::new(move |x| x[c + k] - v));
                        }
                    }
                    let v = x0[r];
                    push(Box::new(move |x| x[r] - v));
                }
                _ => {
                    let p = point(*id)?;
                    for k in 0..3 {
                        let v = vars.x0[p + k];
                        push(Box::new(move |x| x[p + k] - v));
                    }
                }
            },
            Kind3d::Along { line: l, axis } => {
                let (a, b) = line(*l)?;
                for k in (0..3).filter(|k| *k != axis.index()) {
                    push(Box::new(move |x| x[b + k] - x[a + k]));
                }
            }
            Kind3d::ParallelToPlane { line: l, plane: p } => {
                used(p);
                let (a, b) = line(*l)?;
                let pf = plane(p)?;
                push(Box::new(move |x| {
                    let d = sub(get(x, b), get(x, a));
                    dot(d, pf(x).1) / len(d).max(1e-12)
                }));
            }
            Kind3d::Parallel { a, b } => {
                let ((p, q), (r, s)) = (line(*a)?, line(*b)?);
                for k in 0..3 {
                    push(Box::new(move |x| {
                        let (u, v) = (sub(get(x, q), get(x, p)), sub(get(x, s), get(x, r)));
                        cross(u, v)[k] / (len(u) * len(v)).max(1e-12)
                    }));
                }
            }
            Kind3d::Perpendicular { a, b } => {
                let ((p, q), (r, s)) = (line(*a)?, line(*b)?);
                push(Box::new(move |x| {
                    let (u, v) = (sub(get(x, q), get(x, p)), sub(get(x, s), get(x, r)));
                    dot(u, v) / (len(u) * len(v)).max(1e-12)
                }));
            }
            Kind3d::Equal { a, b } => match (kinds.get(a), kinds.get(b)) {
                (Some(Entity3d::Circle { .. }), Some(Entity3d::Circle { .. })) => {
                    let ((_, ra), (_, rb)) = (circle(*a)?, circle(*b)?);
                    push(Box::new(move |x| x[ra] - x[rb]));
                }
                _ => {
                    let ((p, q), (r, s)) = (line(*a)?, line(*b)?);
                    push(Box::new(move |x| {
                        len(sub(get(x, q), get(x, p))) - len(sub(get(x, s), get(x, r)))
                    }));
                }
            },
            Kind3d::PointOnLine { point: pt, line: l } => {
                let (m, (a, b)) = (point(*pt)?, line(*l)?);
                for k in 0..3 {
                    push(Box::new(move |x| {
                        let d = sub(get(x, b), get(x, a));
                        cross(sub(get(x, m), get(x, a)), d)[k] / len(d).max(1e-12)
                    }));
                }
            }
            Kind3d::Midpoint { point: pt, line: l } => {
                let (m, (a, b)) = (point(*pt)?, line(*l)?);
                for k in 0..3 {
                    push(Box::new(move |x| x[m + k] - (x[a + k] + x[b + k]) / 2.0));
                }
            }
            Kind3d::PointOnPlane {
                point: pt,
                plane: p,
            } => {
                used(p);
                let m = point(*pt)?;
                let pf = plane(p)?;
                push(Box::new(move |x| {
                    let (o, n) = pf(x);
                    dot(sub(get(x, m), o), n)
                }));
            }
            Kind3d::Length { line: l, .. } => {
                let (a, b) = line(*l)?;
                let v = value()?;
                push(Box::new(move |x| len(sub(get(x, b), get(x, a))) - v));
            }
            Kind3d::Distance { a, b, .. } => {
                let (a, b) = (point(*a)?, point(*b)?);
                let v = value()?;
                push(Box::new(move |x| len(sub(get(x, b), get(x, a))) - v));
            }
            Kind3d::AxisDistance {
                a, b, axis, flip, ..
            } => {
                let (a, b) = (point(*a)?, point(*b)?);
                let v = if *flip { -value()? } else { value()? };
                let k = axis.index();
                push(Box::new(move |x| x[b + k] - x[a + k] - v));
            }
            Kind3d::Angle { a, b, .. } => {
                let ((p, q), (r, s)) = (line(*a)?, line(*b)?);
                let v = value()?;
                push(Box::new(move |x| {
                    angle(sub(get(x, q), get(x, p)), sub(get(x, s), get(x, r))) - v
                }));
            }
            Kind3d::PlaneAngle {
                line: l, plane: p, ..
            } => {
                used(p);
                let (a, b) = line(*l)?;
                let pf = plane(p)?;
                let v = value()?;
                push(Box::new(move |x| {
                    let d = sub(get(x, b), get(x, a));
                    let n = pf(x).1;
                    dot(d, n).abs().atan2(len(cross(d, n))) - v
                }));
            }
            Kind3d::FaceAngle { a, b, .. } => {
                used(a);
                used(b);
                let (fa, fb) = (plane(a)?, plane(b)?);
                let v = value()?;
                // 平面どうしの角度(0〜90°。法線の向きに左右されない)
                push(Box::new(move |x| {
                    let (na, nb) = (fa(x).1, fb(x).1);
                    len(cross(na, nb)).atan2(dot(na, nb).abs()) - v
                }));
            }
            Kind3d::PlaneDistance {
                point: pt,
                plane: p,
                ..
            } => {
                used(p);
                let m = point(*pt)?;
                let pf = plane(p)?;
                let v = value()?;
                // 今いる側を保つ(符号つきの距離で解く)
                let side = {
                    let (o, n) = pf(&vars.x0);
                    if dot(sub(get(&vars.x0, m), o), n) < 0.0 {
                        -1.0
                    } else {
                        1.0
                    }
                };
                push(Box::new(move |x| {
                    let (o, n) = pf(x);
                    dot(sub(get(x, m), o), n) - side * v
                }));
            }
            Kind3d::Radius { circle: id, .. } => {
                let (_, r) = circle(*id)?;
                let v = value()?;
                push(Box::new(move |x| x[r] - v));
            }
            Kind3d::Diameter { circle: id, .. } => {
                let (_, r) = circle(*id)?;
                let v = value()?;
                push(Box::new(move |x| 2.0 * x[r] - v));
            }
        }
    }
    // 取り込んだボディの点と円は動かさない(位置は計算の時に元の形から取り直してある)
    let (inc_points, inc_circles) = sketch.included_parts();
    for p in inc_points {
        if let Some(&i) = vars.point.get(&p) {
            for k in 0..3 {
                let v = vars.x0[i + k];
                rows.push((None, Box::new(move |x: &[f64]| x[i + k] - v)));
            }
        }
    }
    for c in inc_circles {
        if let Some(&i) = vars.radius.get(&c) {
            let v = vars.x0[i];
            rows.push((None, Box::new(move |x: &[f64]| x[i] - v)));
        }
    }
    // 拘束に使った面は平らに保つ(4 点目からは、最初の 3 点の平面に乗せる)
    for f in planes_used {
        let Some(pts) = loop_points(sketch, &f) else {
            continue;
        };
        let idx: Vec<usize> = pts.iter().map(|p| pi(*p)).collect();
        for k in 3..idx.len() {
            let all = idx.clone();
            let m = idx[k];
            rows.push((
                None,
                Box::new(move |x: &[f64]| {
                    let xyz: Vec<P3> = all.iter().map(|i| [x[*i], x[i + 1], x[i + 2]]).collect();
                    let n = unit(newell(&xyz));
                    dot(sub([x[m], x[m + 1], x[m + 2]], xyz[0]), n)
                }),
            ));
        }
    }
    Ok(System { vars, rows })
}

/// 平面(点と単位の法線)を変数から求める関数。
type PlaneFn = Box<dyn Fn(&[f64]) -> (P3, P3)>;

/// 3D スケッチを解く。`values` は寸法の値(mm・ラジアン)。
pub fn solve3d(
    sketch: &Sketch3d,
    values: &HashMap<ConstraintId, f64>,
) -> Result<Solution3d, SketchError> {
    let sys = build(sketch, values)?;
    let (x, _) = least_change(&sys.rows, sys.vars.x0.clone(), None);
    Ok(finish(sketch, &sys, x))
}

/// 点 `point` を `target` へ引きながら解く(ドラッグ)。拘束を満たす範囲で近づける。
///
/// 動かす範囲は、引いた点の近くから順に広げる(2D の [`kmb_sketch::solve_dragging`] と同じ)。
/// 点がカーソルに付いて来られるいちばん狭い範囲で解き、その外の点と円は動かさない。
/// どの範囲でも付いて来られなければ、全部を動かせるものとして解く。
pub fn solve3d_dragging(
    sketch: &Sketch3d,
    values: &HashMap<ConstraintId, f64>,
    point: EntityId,
    target: P3,
) -> Result<Solution3d, SketchError> {
    let mut sys = build(sketch, values)?;
    let Some(&i) = sys.vars.point.get(&point) else {
        return solve3d(sketch, values);
    };
    // 引く式は弱く足して近づけ、外して拘束をきちんと満たす
    let n = sys.rows.len();
    for k in 0..3 {
        let v = target[k];
        sys.rows
            .push((None, Box::new(move |x: &[f64]| 1e-3 * (x[i + k] - v))));
    }
    if sys.vars.x0[i..i + 3] != target {
        // 点をカーソルの所に置いて止め、近くの物から順に動かせる範囲を広げて拘束を満たす
        let dist = drag_tiers3d(sketch, &sys.vars, point);
        let mut limits: Vec<(u32, u32)> = dist.iter().flatten().copied().collect();
        limits.sort_unstable();
        limits.dedup();
        let mut at = sys.vars.x0.clone();
        at[i..i + 3].copy_from_slice(&target);
        for limit in limits.into_iter().take(12) {
            let mut free: Vec<bool> = dist.iter().map(|d| d.is_some_and(|d| d <= limit)).collect();
            free[i..i + 3].fill(false);
            let (x, _) = least_change(&sys.rows[..n], at.clone(), Some(&free));
            if max_residual(&sys.rows[..n], &x) <= 1e-9 {
                sys.rows.truncate(n);
                return Ok(finish(sketch, &sys, x));
            }
        }
    }
    // どの範囲でも付いて来られない(拘束で決まった向きがある): 弱く引いて近づける
    let (x, _) = least_change(&sys.rows, sys.vars.x0.clone(), None);
    sys.rows.truncate(n);
    let (x, _) = least_change(&sys.rows, x, None);
    Ok(finish(sketch, &sys, x))
}

/// 変数ごとの、引いた点からの隔たり(つながっていなければ `None`)。(拘束・補助線を渡った
/// 回数, 線分・円を渡った回数)の辞書順で比べる。一致の拘束で重ねた点は同じ所に数える。
fn drag_tiers3d(sketch: &Sketch3d, vars: &Vars, point: EntityId) -> Vec<Option<(u32, u32)>> {
    let n = vars.x0.len();
    let of_point = |id: &EntityId| vars.point.get(id).map(|&i| vec![i, i + 1, i + 2]);
    let of = |id: &EntityId| -> Vec<usize> {
        match sketch.entities.iter().find(|e| e.id() == *id) {
            Some(Entity3d::Point { .. }) => of_point(id).unwrap_or_default(),
            Some(Entity3d::Line { a, b, .. }) => {
                [a, b].into_iter().filter_map(of_point).flatten().collect()
            }
            Some(Entity3d::Circle { center, .. }) => {
                let mut v = of_point(center).unwrap_or_default();
                v.extend(vars.radius.get(id));
                v
            }
            None => vec![],
        }
    };
    let mut edges: Vec<(Vec<usize>, (u32, u32))> = vec![];
    for e in &sketch.entities {
        let cost = match e {
            Entity3d::Point { .. } => continue,
            Entity3d::Line {
                construction: true, ..
            }
            | Entity3d::Circle {
                construction: true, ..
            } => (1, 0),
            _ => (0, 1),
        };
        edges.push((of(&e.id()), cost));
    }
    for c in sketch.constraints.iter().filter(|c| !c.reference) {
        let cost = match c.kind {
            Kind3d::Coincident { .. } => (0, 0),
            _ => (1, 0),
        };
        edges.push((c.kind.refs().iter().flat_map(&of).collect(), cost));
    }
    let mut touching: Vec<Vec<usize>> = vec![vec![]; n];
    for (k, (vs, _)) in edges.iter().enumerate() {
        for &v in vs {
            touching[v].push(k);
        }
    }
    let mut dist = vec![None; n];
    let mut queue = std::collections::BinaryHeap::new();
    for v in of_point(&point).unwrap_or_default() {
        dist[v] = Some((0, 0));
        queue.push(std::cmp::Reverse(((0, 0), v)));
    }
    while let Some(std::cmp::Reverse((d, v))) = queue.pop() {
        if dist[v].is_some_and(|best| best < d) {
            continue;
        }
        for &k in &touching[v] {
            let (vs, cost) = &edges[k];
            let next = (d.0 + cost.0, d.1 + cost.1);
            for &u in vs {
                if dist[u].is_none_or(|best| next < best) {
                    dist[u] = Some(next);
                    queue.push(std::cmp::Reverse((next, u)));
                }
            }
        }
    }
    dist
}

fn finish(sketch: &Sketch3d, sys: &System, mut x: Vec<f64>) -> Solution3d {
    // 取り込んだ点と円は元の値のまま返す(計算の誤差で 1e-24 ずれても位置は元の形)
    let (inc_points, inc_circles) = sketch.included_parts();
    for p in inc_points {
        if let Some(&i) = sys.vars.point.get(&p) {
            x[i..i + 3].copy_from_slice(&sys.vars.x0[i..i + 3]);
        }
    }
    for c in inc_circles {
        if let Some(&i) = sys.vars.radius.get(&c) {
            x[i] = sys.vars.x0[i];
        }
    }
    let r = residuals(&sys.rows, &x);
    let n = x.len();
    let mut status = SolveStatus::WellConstrained;
    let mut free = BTreeSet::new();
    if !sys.rows.is_empty() || n > 0 {
        let j = jacobian(&sys.rows, &x, None);
        let conflicting: Vec<ConstraintId> = sys
            .rows
            .iter()
            .zip(r.iter())
            .filter(|(_, v)| v.abs() > 1e-6)
            .filter_map(|((t, _), _)| *t)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let (rank, redundant) = if sys.rows.is_empty() {
            (0, vec![])
        } else {
            redundant(&j, &sys.rows.iter().map(|(t, _)| *t).collect::<Vec<_>>())
        };
        status = if !conflicting.is_empty() {
            SolveStatus::Inconsistent { conflicting }
        } else if !redundant.is_empty() {
            SolveStatus::OverConstrained { redundant }
        } else if rank < n {
            SolveStatus::UnderConstrained { dof: n - rank }
        } else {
            SolveStatus::WellConstrained
        };
        // 動かせる変数: 行の空間に入らない向き(零空間)の成分がある変数
        if rank < n {
            let free_vars = free_variables(&j);
            for (id, i) in &sys.vars.point {
                if (0..3).any(|k| free_vars[i + k]) {
                    free.insert(*id);
                }
            }
            for (id, i) in &sys.vars.radius {
                if free_vars[*i] {
                    free.insert(*id);
                }
            }
        }
    }
    let mut points = BTreeMap::new();
    for (id, i) in &sys.vars.point {
        points.insert(*id, [x[*i], x[i + 1], x[i + 2]]);
    }
    let mut radii = BTreeMap::new();
    for (id, i) in &sys.vars.radius {
        radii.insert(*id, x[*i]);
    }
    let faces = find_faces(sketch, &points);
    let mut sol = Solution3d {
        points,
        radii,
        status,
        free,
        faces,
        measured: BTreeMap::new(),
        normals: BTreeMap::new(),
        derived: BTreeMap::new(),
    };
    for e in &sketch.entities {
        if let Entity3d::Circle {
            id,
            normal,
            plane: Some(w),
            ..
        } = e
            && let Some((_, n)) = plane_at(sketch, &sol, &PlaneRef::Work { id: *w })
        {
            let n = if dot(n, *normal) < 0.0 {
                [-n[0], -n[1], -n[2]]
            } else {
                n
            };
            sol.normals.insert(*id, n);
        }
    }
    for c in &sketch.constraints {
        if let Some(v) = measure(sketch, &sol, &c.kind) {
            sol.measured.insert(c.id, v);
        }
    }
    sol
}

/// 拘束が測る今の値(寸法の拘束だけ。mm・ラジアン)。
pub fn measure(sketch: &Sketch3d, sol: &Solution3d, kind: &Kind3d) -> Option<f64> {
    let p = |id: &EntityId| sol.points.get(id).copied();
    let line = |id: &EntityId| {
        let (a, b) = sketch.line_ends(*id)?;
        Some(sub(p(&b)?, p(&a)?))
    };
    let plane = |r: &PlaneRef| plane_at(sketch, sol, r);
    let center_of = |id: &EntityId| match sketch.entity(*id)? {
        Entity3d::Circle { id, .. } => sol.radii.get(id).copied(),
        _ => None,
    };
    Some(match kind {
        Kind3d::Length { line: l, .. } => len(line(l)?),
        Kind3d::Distance { a, b, .. } => len(sub(p(b)?, p(a)?)),
        Kind3d::AxisDistance {
            a, b, axis, flip, ..
        } => {
            let d = p(b)?[axis.index()] - p(a)?[axis.index()];
            if *flip { -d } else { d }
        }
        Kind3d::Angle { a, b, .. } => angle(line(a)?, line(b)?),
        Kind3d::PlaneAngle {
            line: l, plane: r, ..
        } => {
            let (d, n) = (line(l)?, plane(r)?.1);
            dot(d, n).abs().atan2(len(cross(d, n)))
        }
        Kind3d::FaceAngle { a, b, .. } => {
            let (na, nb) = (plane(a)?.1, plane(b)?.1);
            len(cross(na, nb)).atan2(dot(na, nb).abs())
        }
        Kind3d::PlaneDistance {
            point, plane: r, ..
        } => {
            let (o, n) = plane(r)?;
            dot(sub(p(point)?, o), n).abs()
        }
        Kind3d::Radius { circle, .. } => center_of(circle)?,
        Kind3d::Diameter { circle, .. } => 2.0 * center_of(circle)?,
        _ => return None,
    })
}

/// 寸法の式を計算する(長さは mm、角度はラジアン)。`lookup` はパラメータの名前から値を引く
/// (同じスケッチの寸法の名前はスケッチの中で解く)。単位の無い値は mm(角度は deg)。
pub fn dimension_values3d(
    sketch: &Sketch3d,
    lookup: &dyn Fn(&str) -> Option<kmb_core::Quantity>,
) -> Result<HashMap<ConstraintId, f64>, String> {
    let dims = sketch.dim_exprs();
    let qs = kmb_sketch::dims::eval_dims(&dims, lookup)?;
    let mut values = HashMap::new();
    for d in &dims {
        let signed = sketch
            .constraint(d.id)
            .is_some_and(|c| matches!(c.kind, Kind3d::AxisDistance { .. }));
        let v = kmb_sketch::dims::checked(d.id, d.name, d.expr, qs[&d.id], d.angle, signed)?;
        values.insert(d.id, v);
    }
    Ok(values)
}
