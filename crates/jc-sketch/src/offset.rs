//! オフセット: つながった線分と円弧を、距離だけずらして写す。線分は平行に、円弧は同じ中心で
//! 半径を変え、隣どうしの交わる所で角を作る。円は同心の円を足す。

use crate::geom::{Curve, EPS, add, angle_of, cross, dot, intersect, len, scale, sub};
use crate::model::{ConstraintKind as K, Entity, EntityId, OffsetPart, Sketch};
use std::f64::consts::{PI, TAU};

/// 連なりの 1 つ(線分か円弧)と、たどる向き。
#[derive(Clone, Copy, Debug)]
struct Step {
    id: EntityId,
    /// たどる始まりと終わりの点
    from: EntityId,
    to: EntityId,
    /// 円弧を時計回りにたどる(円弧は反時計回りに持つので、終点から始点へ)
    reversed: bool,
}

/// ずらした形(延ばして交点を求める)。
#[derive(Clone, Copy, Debug)]
enum Shifted {
    Line([f64; 2], [f64; 2]),
    Arc { center: [f64; 2], radius: f64 },
}

impl Shifted {
    /// 延ばした形(線分は十分に長い線分、円弧は円)。
    fn extended(&self) -> Curve {
        match *self {
            Shifted::Line(p, q) => {
                let d = sub(q, p);
                Curve::Seg(sub(p, scale(d, 1e6)), add(q, scale(d, 1e6)))
            }
            Shifted::Arc { center, radius } => Curve::Arc(center, radius, 0.0, TAU),
        }
    }
}

/// オフセットを断った理由(画面の知らせに出す)。
pub(crate) const RADIUS_GONE: &str = "内へずらすと円・円弧の半径が 0 以下になります";
pub(crate) const LINE_FLIPS: &str = "内へずらしすぎて線分が裏返ります(始まりと終わりが入れ替わる)";
pub(crate) const ARC_FLIPS: &str =
    "内へずらしすぎて円弧が裏返ります(両端が入れ替わり、遠回りの円弧になる)";

impl Sketch {
    /// 選んだ線分・円弧・円を、距離 `value`(式。形を作る時は `distance` mm)だけずらした写しを作る。
    /// 端でつながった線分と円弧は 1 つの連なりとして写し、隣どうしは交わる所でつなぐ。
    /// `outward` が真なら、閉じた輪郭の外へ(開いた連なりなら、たどる向きの左へ)ずらす。
    /// 角を丸めるなら [`Sketch::offset_corners`]。
    pub fn offset(
        &mut self,
        ids: &[EntityId],
        distance: f64,
        value: &str,
        outward: bool,
    ) -> Vec<EntityId> {
        self.offset_corners(ids, distance, value, outward, false)
    }

    /// [`Sketch::offset`] の角の作り方を選べるもの。`round` が真なら、ずらすと開く角(外の角)を、
    /// 元の角の点を中心に距離を半径とする円弧でつなぐ(角の点から等距離)。偽なら辺を延ばして
    /// 交わる所で角を作る。
    ///
    /// 拘束: 写した線全部に 1 つのオフセットの拘束([`ConstraintKind::Offset`])を付け、距離は
    /// 代表の寸法 1 つ(式 `value`)で決める。線分は元と平行で同じ距離、円・円弧は元の中心を
    /// 使って半径が距離だけ違い、元で接していた所は写しも接する。どの関係も同じ距離を使うので、
    /// 寸法を変えると全部が付いて来る(寸法を消しても同じ距離の関係は残る)。
    /// 作った曲線を返す(内へずらして半径が 0 以下になる連なりは作らない)。
    pub fn offset_corners(
        &mut self,
        ids: &[EntityId],
        distance: f64,
        value: &str,
        outward: bool,
        round: bool,
    ) -> Vec<EntityId> {
        self.offset_corners_why(ids, distance, value, outward, round)
            .0
    }

    /// [`Sketch::offset_corners`] と同じに作り、作らなかった物があればその理由も返す
    /// (画面が「オフセットできませんでした」の後に出す。黙って作らないことをしない)。
    pub fn offset_corners_why(
        &mut self,
        ids: &[EntityId],
        distance: f64,
        value: &str,
        outward: bool,
        round: bool,
    ) -> (Vec<EntityId>, Option<&'static str>) {
        let mut why = None;
        let out = self.offset_inner(ids, distance, value, outward, round, &mut why);
        (out, why)
    }

    fn offset_inner(
        &mut self,
        ids: &[EntityId],
        distance: f64,
        value: &str,
        outward: bool,
        round: bool,
        why: &mut Option<&'static str>,
    ) -> Vec<EntityId> {
        let mut out = vec![];
        let mut parts = vec![];
        for id in ids {
            if let Some(Entity::Circle { center, radius, .. }) = self.entity(*id).cloned() {
                let r = if outward {
                    radius + distance
                } else {
                    radius - distance
                };
                if r > 1e-6 {
                    let new = self.add_circle(center, r);
                    parts.push(OffsetPart::Radius { from: *id, to: new });
                    out.push(new);
                } else {
                    *why = Some(RADIUS_GONE);
                }
            }
            // 楕円・スプライン・円錐曲線は、ずらした点を通るフィット点のスプラインにする
            if self.entity(*id).is_some_and(crate::curve::is_free)
                && let Some(new) = self.offset_free(*id, distance, outward)
            {
                out.push(new);
            }
        }
        for chain in chains(self, ids) {
            match self.offset_chain(&chain, distance, outward, round, &mut parts) {
                Ok(made) => out.extend(made),
                Err(e) => *why = Some(e),
            }
        }
        if parts.is_empty() {
            return out;
        }
        // 代表の寸法(線分があれば線分の間、なければ円周の間)
        let dim = parts
            .iter()
            .find_map(|p| match p {
                OffsetPart::Distance { from, to } => match self.entity(*to) {
                    Some(Entity::Line { a, .. }) => Some(K::PointLineDistance {
                        point: *a,
                        line: *from,
                        value: value.to_string(),
                    }),
                    _ => None,
                },
                _ => None,
            })
            .or_else(|| {
                parts.iter().find_map(|p| match p {
                    OffsetPart::Radius { from, to } => Some(K::CircleDistance {
                        circle: *from,
                        other: *to,
                        far: false,
                        value: value.to_string(),
                    }),
                    _ => None,
                })
            });
        // 関係の並び: 円弧の半径と接する所を先に、線分の平行と距離を後に(長穴の線のように、接する
        // 円弧で決まる所は後から足した方を外す)
        let rank = |p: &OffsetPart| match p {
            OffsetPart::Radius { .. } | OffsetPart::Corner { .. } => 0,
            OffsetPart::Tangent { .. } => 1,
            OffsetPart::Parallel { .. } => 2,
            OffsetPart::Distance { .. } => 3,
        };
        parts.sort_by_key(rank);
        let id = self.add_constraint(K::Offset { parts });
        self.drop_redundant_parts(id);
        if let Some(k) = dim {
            let d = self.add_constraint(k);
            if self.redundant_now(distance).contains(&d) {
                self.remove_constraint(d);
            }
        }
        out
    }

    /// 寸法を今の形で測った値(式は評価できないので)。`added` の寸法は `distance`。
    fn measured_values(
        &self,
        distance: f64,
    ) -> std::collections::HashMap<crate::ConstraintId, f64> {
        let sol = crate::Solution {
            status: crate::SolveStatus::WellConstrained,
            dof: 0,
            points: self
                .entities
                .iter()
                .filter_map(|e| match e {
                    Entity::Point { id, x, y } => Some((*id, [*x, *y])),
                    _ => None,
                })
                .collect(),
            radii: self
                .entities
                .iter()
                .filter_map(|e| match e {
                    Entity::Circle { id, radius, .. } => Some((*id, *radius)),
                    _ => None,
                })
                .collect(),
            fixed: Default::default(),
            lone: vec![],
            iterations: 0,
            max_residual: 0.0,
            measured: Default::default(),
        };
        self.constraints
            .iter()
            .filter(|c| c.kind.value().is_some())
            .map(|c| {
                let v = crate::measure(self, &sol, &c.kind).unwrap_or(distance);
                (c.id, v)
            })
            .collect()
    }

    /// 今の形で解いた時に、余分になる拘束。
    fn redundant_now(&self, distance: f64) -> Vec<crate::ConstraintId> {
        match crate::solve(self, &self.measured_values(distance)) {
            Ok(crate::Solution {
                status: crate::SolveStatus::OverConstrained { redundant },
                ..
            }) => redundant,
            _ => vec![],
        }
    }

    /// オフセットの拘束 `id` の関係のうち、ほかの関係や拘束で既に決まるもの(余分)を外す。
    fn drop_redundant_parts(&mut self, id: crate::ConstraintId) {
        let values = self.measured_values(0.0);
        let Ok(gone) = crate::solver::redundant_parts(self, &values, id) else {
            return;
        };
        for c in &mut self.constraints {
            if c.id == id
                && let K::Offset { parts } = &mut c.kind
            {
                let mut i = 0;
                parts.retain(|_| {
                    i += 1;
                    !gone.contains(&(i - 1))
                });
            }
        }
    }

    fn offset_chain(
        &mut self,
        chain: &[Step],
        distance: f64,
        outward: bool,
        round: bool,
        parts: &mut Vec<OffsetPart>,
    ) -> Result<Vec<EntityId>, &'static str> {
        let n = chain.len();
        let closed =
            n > 1 && chain[0].from == chain[n - 1].to || n == 1 && chain[0].from == chain[0].to;
        // 閉じた輪郭の向き(反時計回りなら正)で、外向きの側を決める。円弧は折れ線にして測る
        let mut ring: Vec<[f64; 2]> = vec![];
        for st in chain {
            ring.extend(self.trace(st));
        }
        let area: f64 = (0..ring.len())
            .map(|i| cross(ring[i], ring[(i + 1) % ring.len()]))
            .sum();
        let left = if closed {
            // 反時計回りの輪郭の外は右側
            (area < 0.0) == outward
        } else {
            outward
        };
        let side = if left { 1.0 } else { -1.0 };
        // 各曲線をずらした形と、ずらした両端、たどる向きの両端の接線
        let mut shapes: Vec<(Shifted, [f64; 2], [f64; 2])> = vec![];
        let mut tangents: Vec<([f64; 2], [f64; 2])> = vec![];
        for st in chain {
            let (Some(p), Some(q)) = (self.pos(st.from), self.pos(st.to)) else {
                return Ok(vec![]);
            };
            match self.entity(st.id) {
                Some(Entity::Line { .. }) => {
                    let d = sub(q, p);
                    let nrm = scale([-d[1], d[0]], side * distance / len(d).max(EPS));
                    let (a, b) = (add(p, nrm), add(q, nrm));
                    shapes.push((Shifted::Line(a, b), a, b));
                    tangents.push((d, d));
                }
                Some(Entity::Arc { center, .. }) => {
                    let c = self.pos(*center).unwrap_or([0.0, 0.0]);
                    let r = len(sub(p, c));
                    // 反時計回りにたどると左が中心の側(半径が減る)
                    let k = if st.reversed { 1.0 } else { -1.0 };
                    let r2 = r + k * side * distance;
                    if r2 <= 1e-6 || r < EPS {
                        return Err(RADIUS_GONE);
                    }
                    let at = |x: [f64; 2]| add(c, scale(sub(x, c), r2 / r));
                    shapes.push((
                        Shifted::Arc {
                            center: c,
                            radius: r2,
                        },
                        at(p),
                        at(q),
                    ));
                    // 反時計回りの接線は半径を左へ 90° 回した向き
                    let tan = |x: [f64; 2]| {
                        let v = sub(x, c);
                        let t = [-v[1], v[0]];
                        if st.reversed { scale(t, -1.0) } else { t }
                    };
                    tangents.push((tan(p), tan(q)));
                }
                _ => return Ok(vec![]),
            }
        }
        // 継ぎ目 i(曲線 i - 1 の終わりと曲線 i の始まり)の作り方
        enum Joint {
            /// 1 つの点(なめらかなら真)
            One([f64; 2], bool),
            /// 角を丸める円弧(前の曲線の終わり、次の曲線の始まり)
            Round([f64; 2], [f64; 2]),
        }
        let joint = |i: usize, j: usize| -> Joint {
            let (e, b) = (shapes[i].2, shapes[j].1);
            if len(sub(e, b)) < 1e-7 * (1.0 + distance) {
                return Joint::One(e, true);
            }
            // ずらすと開く角(外の角)を丸める: 曲がる向きとずらす向きが逆
            if round && cross(tangents[i].1, tangents[j].0) * side < 0.0 {
                return Joint::Round(e, b);
            }
            let mid = scale(add(e, b), 0.5);
            let best = intersect(&shapes[i].0.extended(), &shapes[j].0.extended())
                .into_iter()
                .min_by(|x, y| len(sub(*x, mid)).total_cmp(&len(sub(*y, mid))));
            Joint::One(best.unwrap_or(mid), false)
        };
        // 曲線ごとの始まりと終わりの点、なめらかな継ぎ目、丸めた角(継ぎ目の番号、円弧)
        let mut starts: Vec<Option<EntityId>> = vec![None; n];
        let mut ends: Vec<Option<EntityId>> = vec![None; n];
        let mut smooth: Vec<usize> = vec![];
        let mut rounds: Vec<(usize, [f64; 2], [f64; 2])> = vec![];
        let joints = if closed { n } else { n + 1 };
        let js: Vec<Joint> = (0..joints)
            .map(|k| {
                let (prev, next) = (if k == 0 { n - 1 } else { k - 1 }, k % n);
                if !closed && k == 0 {
                    Joint::One(shapes[0].1, false)
                } else if !closed && k == n {
                    Joint::One(shapes[n - 1].2, false)
                } else {
                    joint(prev, next)
                }
            })
            .collect();
        // 内へずらしすぎて線分・円弧が裏返る(始まりと終わりが入れ替わる)なら作らない
        for (i, st) in chain.iter().enumerate() {
            let start = match js[i] {
                Joint::One(p, _) | Joint::Round(_, p) => p,
            };
            let end = match js[(i + 1) % joints] {
                Joint::One(p, _) | Joint::Round(p, _) => p,
            };
            match self.entity(st.id) {
                Some(Entity::Line { .. }) => {
                    if dot(sub(end, start), tangents[i].0) <= 0.0 {
                        return Err(LINE_FLIPS);
                    }
                }
                Some(Entity::Arc { center, .. }) => {
                    let c = self.pos(*center).unwrap_or([0.0, 0.0]);
                    let sweep = |p: [f64; 2], q: [f64; 2]| {
                        let (a0, a1) = (angle_of(c, p), angle_of(c, q));
                        if st.reversed {
                            -(a0 - a1).rem_euclid(TAU)
                        } else {
                            (a1 - a0).rem_euclid(TAU)
                        }
                    };
                    let (Some(p), Some(q)) = (self.pos(st.from), self.pos(st.to)) else {
                        continue;
                    };
                    // 弧の中ほどが弦の反対側へ移ったら(開きが半周をまたいだら)裏返り
                    let (old, new) = (sweep(p, q).abs(), sweep(start, end).abs());
                    let side = |s: f64| s < PI;
                    if (old - PI).abs() > 1e-3 && (new - PI).abs() > 1e-3 && side(old) != side(new)
                    {
                        return Err(ARC_FLIPS);
                    }
                }
                _ => {}
            }
        }
        for (k, j) in js.into_iter().enumerate() {
            let (prev, next) = (if k == 0 { n - 1 } else { k - 1 }, k % n);
            match j {
                Joint::One(p, s) => {
                    let id = self.add_point(p[0], p[1]);
                    if closed || k > 0 {
                        ends[prev] = Some(id);
                    }
                    if closed || k < n {
                        starts[next] = Some(id);
                    }
                    if s {
                        smooth.push(k);
                    }
                }
                Joint::Round(e, b) => {
                    let (ie, ib) = (self.add_point(e[0], e[1]), self.add_point(b[0], b[1]));
                    ends[prev] = Some(ie);
                    starts[next] = Some(ib);
                    rounds.push((k, e, b));
                }
            }
        }
        let mut made = vec![];
        for (i, st) in chain.iter().enumerate() {
            let (Some(a), Some(b)) = (starts[i], ends[i]) else {
                return Ok(made);
            };
            let new = match self.entity(st.id).cloned() {
                Some(Entity::Line { .. }) => {
                    let l = self.add_line(a, b);
                    parts.push(OffsetPart::Parallel { from: st.id, to: l });
                    parts.push(OffsetPart::Distance { from: st.id, to: l });
                    l
                }
                Some(Entity::Arc { center, .. }) => {
                    let arc = if st.reversed {
                        self.add_arc(center, b, a)
                    } else {
                        self.add_arc(center, a, b)
                    };
                    parts.push(OffsetPart::Radius {
                        from: st.id,
                        to: arc,
                    });
                    arc
                }
                _ => continue,
            };
            made.push(new);
        }
        let tangent = |s: &Sketch, x: EntityId, y: EntityId| -> Option<OffsetPart> {
            let lineish = |id| matches!(s.entity(id), Some(Entity::Line { .. }));
            (!(lineish(x) && lineish(y))).then_some(OffsetPart::Tangent { a: x, b: y })
        };
        // 元で接していた所は写しも接する(角の丸めや長穴の円弧が、隣の線と一緒に動く)
        for k in smooth {
            let (prev, next) = (if k == 0 { n - 1 } else { k - 1 }, k % n);
            if let Some(t) = tangent(self, made[prev], made[next]) {
                parts.push(t);
            }
        }
        // 丸めた角: 元の角の点を中心に、前後の曲線に接する円弧
        for (k, e, b) in rounds {
            let (prev, next) = (if k == 0 { n - 1 } else { k - 1 }, k % n);
            let center = chain[prev].to;
            let Some(v) = self.pos(center) else { continue };
            let (ie, ib) = (ends[prev].expect("作った"), starts[next].expect("作った"));
            let arc = if cross(sub(e, v), sub(b, v)) > 0.0 {
                self.add_arc(center, ie, ib)
            } else {
                self.add_arc(center, ib, ie)
            };
            parts.push(OffsetPart::Corner { arc });
            parts.push(OffsetPart::Tangent {
                a: made[prev],
                b: arc,
            });
            parts.push(OffsetPart::Tangent {
                a: arc,
                b: made[next],
            });
            made.push(arc);
        }
        Ok(made)
    }

    /// 連なりの 1 つを、たどる向きの折れ線にする(始まりを含み、終わりは含まない)。
    fn trace(&self, st: &Step) -> Vec<[f64; 2]> {
        let (Some(p), Some(q)) = (self.pos(st.from), self.pos(st.to)) else {
            return vec![];
        };
        match self.entity(st.id) {
            Some(Entity::Arc { center, .. }) => {
                let c = self.pos(*center).unwrap_or([0.0, 0.0]);
                let r = len(sub(p, c));
                let (a0, a1) = (angle_of(c, p), angle_of(c, q));
                let sweep = if st.reversed {
                    -(a0 - a1).rem_euclid(TAU)
                } else {
                    (a1 - a0).rem_euclid(TAU)
                };
                (0..24)
                    .map(|i| {
                        let a = a0 + sweep * i as f64 / 24.0;
                        [c[0] + r * a.cos(), c[1] + r * a.sin()]
                    })
                    .collect()
            }
            _ => vec![p],
        }
    }
}

/// 選んだ線分と円弧を、端でつながる連なりに分けて並べる(向きをそろえる)。
fn chains(s: &Sketch, ids: &[EntityId]) -> Vec<Vec<Step>> {
    let mut rest: Vec<Step> = ids
        .iter()
        .filter_map(|id| match s.entity(*id)? {
            Entity::Line { a, b, .. } => Some(Step {
                id: *id,
                from: *a,
                to: *b,
                reversed: false,
            }),
            Entity::Arc { start, end, .. } => Some(Step {
                id: *id,
                from: *start,
                to: *end,
                reversed: false,
            }),
            _ => None,
        })
        .collect();
    let flip = |st: Step| Step {
        from: st.to,
        to: st.from,
        reversed: !st.reversed,
        ..st
    };
    let mut out = vec![];
    while !rest.is_empty() {
        // 開いた連なりなら、1 つにしか使われていない端から始める
        let degree =
            |p: EntityId, r: &[Step]| r.iter().filter(|x| x.from == p || x.to == p).count();
        let start = rest
            .iter()
            .position(|x| degree(x.from, &rest) == 1 || degree(x.to, &rest) == 1)
            .unwrap_or(0);
        let mut first = rest.remove(start);
        if degree(first.from, &rest) > 0 && degree(first.to, &rest) == 0 {
            first = flip(first);
        }
        let mut chain = vec![first];
        loop {
            let tail = chain.last().map(|x| x.to).expect("1 つはある");
            if tail == chain[0].from {
                break;
            }
            let Some(i) = rest.iter().position(|x| x.from == tail || x.to == tail) else {
                break;
            };
            let x = rest.remove(i);
            chain.push(if x.from == tail { x } else { flip(x) });
        }
        out.push(chain);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SolveStatus, dimension_values, solve};

    fn solved(s: &Sketch) -> crate::Solution {
        let v = dimension_values(s, &|_| None).unwrap();
        solve(s, &v).unwrap()
    }

    fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
        len(sub(a, b))
    }

    /// 中心を固定し、半径 5 の長穴(中心の間 20)。
    fn slot() -> (Sketch, [EntityId; 4], EntityId, EntityId) {
        let mut s = Sketch::default();
        let (c1, c2) = (s.add_point(0.0, 0.0), s.add_point(20.0, 0.0));
        s.add_constraint(K::Fix { point: c1 });
        s.add_constraint(K::Fix { point: c2 });
        let parts = s
            .add_slot((Some(c1), [0.0, 0.0]), (Some(c2), [20.0, 0.0]), 5.0)
            .unwrap();
        s.add_constraint(K::Radius {
            circle: parts[2],
            value: "5 mm".into(),
        });
        (s, parts, c1, c2)
    }

    #[test]
    fn a_slot_is_offset_with_its_arcs_and_stays_fully_determined() {
        let (mut s, parts, c1, c2) = slot();
        assert_eq!(solved(&s).status, SolveStatus::WellConstrained);
        let new = s.offset(&parts, 2.0, "2 mm", true);
        assert_eq!(new.len(), 4, "線分 2 本と円弧 2 本");
        let sol = solved(&s);
        assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
        for id in &new {
            match s.entity(*id) {
                Some(Entity::Arc {
                    center, start, end, ..
                }) => {
                    assert!(*center == c1 || *center == c2, "元の中心を使う");
                    for p in [start, end] {
                        let r = dist(sol.points[p], sol.points[center]);
                        assert!((r - 7.0).abs() < 1e-6, "外へ 2 mm: {r}");
                    }
                }
                Some(Entity::Line { a, .. }) => {
                    assert!((sol.points[a][1].abs() - 7.0).abs() < 1e-6);
                }
                _ => panic!("曲線だけ"),
            }
        }
        // 内へ
        let (mut s, parts, c1, _) = slot();
        let new = s.offset(&parts, 2.0, "2 mm", false);
        let sol = solved(&s);
        let arc = new
            .iter()
            .find_map(|id| match s.entity(*id) {
                Some(Entity::Arc { center, start, .. }) if *center == c1 => Some(*start),
                _ => None,
            })
            .unwrap();
        assert!((dist(sol.points[&arc], sol.points[&c1]) - 3.0).abs() < 1e-6);
        // 半径より深く内へは作らない
        let (mut s, parts, ..) = slot();
        assert!(s.offset(&parts, 6.0, "6 mm", false).is_empty());
    }

    #[test]
    fn a_rounded_corner_is_offset_and_the_distance_follows_its_value() {
        let mut s = Sketch::default();
        let l = s
            .add_rectangle((None, [0.0, 0.0]), (None, [20.0, 10.0]))
            .unwrap();
        let corner = match s.entity(l[0]) {
            Some(Entity::Line { b, .. }) => *b,
            _ => unreachable!(),
        };
        let arc = s.fillet_corner(corner, 3.0, "3 mm").unwrap();
        let mut ids: Vec<EntityId> = l.to_vec();
        ids.push(arc);
        let new = s.offset(&ids, 1.0, "d", false);
        assert_eq!(new.len(), 5);
        let v = |d: f64| {
            dimension_values(&s, &|n| (n == "d").then_some(jc_core::Quantity::mm(d))).unwrap()
        };
        for d in [1.0, 2.0] {
            let sol = solve(&s, &v(d)).unwrap();
            let Some(Entity::Arc { center, start, .. }) =
                new.iter().find_map(|id| match s.entity(*id) {
                    e @ Some(Entity::Arc { .. }) => e.cloned(),
                    _ => None,
                })
            else {
                panic!("円弧がある")
            };
            let r = dist(sol.points[&start], sol.points[&center]);
            assert!((r - (3.0 - d)).abs() < 1e-6, "d = {d}: r = {r}");
            assert!(
                !matches!(sol.status, SolveStatus::OverConstrained { .. }),
                "{:?}",
                sol.status
            );
        }
    }

    #[test]
    fn an_offset_circle_follows_the_diameter_of_the_original() {
        let mut s = Sketch::default();
        let c = s.add_point(0.0, 0.0);
        s.add_constraint(K::Fix { point: c });
        let circle = s.add_circle(c, 5.0);
        s.add_constraint(K::Diameter {
            circle,
            value: "10 mm".into(),
        });
        let new = s.offset(&[circle], 2.0, "2 mm", true);
        assert_eq!(new.len(), 1);
        let sol = solved(&s);
        assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
        assert!((sol.radii[&new[0]] - 7.0).abs() < 1e-6);
    }
}
