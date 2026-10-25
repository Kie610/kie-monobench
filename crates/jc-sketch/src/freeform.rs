//! 楕円・スプライン・円錐曲線の作図([`Sketch::add_ellipse`]・[`Sketch::add_spline`]・
//! [`Sketch::add_conic`])と、トリム・オフセット。どれも保存されている座標(先に
//! [`Sketch::adopt_solution`] で解いた位置を写しておく)を元に要素と拘束を足す。

use crate::curve::{BSpline, Shape, conic_part, ellipse_minor_point};
use crate::geom::{cross, len, scale, sub};
use crate::model::{Entity, EntityId, Sketch};
use crate::tools::Spot;

type P2 = [f64; 2];

/// 交点 1 つ(曲線のパラメータ、位置、交わった曲線)。
pub(crate) type Cut = (f64, P2, EntityId);

/// トリムで消す所。
pub(crate) enum Plan {
    /// まるごと消す(交点が足りない)
    Whole,
    /// 開いた曲線: 押した所の手前と先の交点(なければ端まで消す)
    Open { lo: Option<Cut>, hi: Option<Cut> },
    /// 閉じた曲線: 残すのは `keep_from` から進んで `keep_to` まで
    Closed { keep_from: Cut, keep_to: Cut },
}

/// 曲線 `shape` の `at` に近い区間を消す時の、残す所と消す所。
pub(crate) fn plan(shape: &Shape, cuts: &[(P2, EntityId)], at: P2) -> Plan {
    let (lo, hi) = shape.domain();
    let span = hi - lo;
    let tc = shape.closest(at).0;
    let mut ts: Vec<Cut> = cuts
        .iter()
        .map(|(p, by)| (shape.closest(*p).0, *p, *by))
        .collect();
    if shape.closed() {
        let rel = |t: f64| (t - tc).rem_euclid(span);
        ts.sort_by(|a, b| rel(a.0).total_cmp(&rel(b.0)));
        ts.dedup_by(|a, b| (rel(a.0) - rel(b.0)).abs() < 1e-9 * span || len(sub(a.1, b.1)) < 1e-9);
        if ts.len() < 2 {
            return Plan::Whole;
        }
        // 押した所から進んで最初の交点が残す所の始まり、戻って最初の交点が終わり
        return Plan::Closed {
            keep_from: ts[0],
            keep_to: ts[ts.len() - 1],
        };
    }
    let eps = 1e-6 * span;
    let inner: Vec<Cut> = ts
        .into_iter()
        .filter(|c| c.0 > lo + eps && c.0 < hi - eps)
        .collect();
    let lo_cut = inner
        .iter()
        .filter(|c| c.0 < tc)
        .max_by(|x, y| x.0.total_cmp(&y.0))
        .copied();
    let hi_cut = inner
        .iter()
        .filter(|c| c.0 > tc)
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .copied();
    if lo_cut.is_none() && hi_cut.is_none() {
        return Plan::Whole;
    }
    Plan::Open {
        lo: lo_cut,
        hi: hi_cut,
    }
}

/// 節点を 0〜1 にそろえる(形は変わらない)。
fn normalized(mut s: BSpline) -> BSpline {
    let (a, b) = s.domain();
    let w = (b - a).max(1e-300);
    for k in &mut s.knots {
        *k = (*k - a) / w;
    }
    s
}

/// B スプラインの `t0`〜`t1`(閉じた曲線なら継ぎ目をまたいでよい)を、clamped な 1 本にする。
pub(crate) fn spline_part(s: &BSpline, closed: bool, t0: f64, t1: f64) -> Option<BSpline> {
    if !closed || t0 < t1 {
        return s.extract(t0, t1).map(normalized);
    }
    let (lo, hi) = s.domain();
    let a = s.extract(t0, hi);
    let b = s.extract(lo, t1);
    Some(normalized(match (a, b) {
        (Some(a), Some(b)) => BSpline::join(&a, &b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    }))
}

impl Sketch {
    /// 中心 `center`、長軸の端 `major` と、短軸の大きさを決める点 `minor_point`(長軸からの距離が
    /// 短半径)から楕円を描く。長軸と短軸の補助線(中心から各軸の端への線分)も
    /// 足す(楕円の次の 2 つの番号が長軸・短軸の補助線)。軸の補助線に水平・垂直や長さの寸法を
    /// 付ければ、向きと大きさが決まる。描けない(長さ 0)なら `None`。
    pub fn add_ellipse(
        &mut self,
        center: Spot,
        major: Spot,
        minor_point: [f64; 2],
    ) -> Option<EntityId> {
        let m = ellipse_minor_point(center.1, major.1, minor_point)?;
        if len(sub(major.1, center.1)) < 1e-6 || len(sub(m, center.1)) < 1e-6 {
            return None;
        }
        let c = self.spot(center);
        let a = self.spot(major);
        let b = self.add_point(m[0], m[1]);
        let id = self.add_entity(&Entity::Ellipse {
            id: EntityId(0),
            center: c,
            major: a,
            minor: b,
            ends: None,
            construction: false,
        });
        for end in [a, b] {
            let l = self.add_line(c, end);
            self.set_construction(l, true);
        }
        Some(id)
    }

    /// スプラインを描く。`control` が偽なら点 `points` を通るフィット点のスプライン、真なら
    /// 点を制御点とする 3 次(点が 3 つなら 2 次、2 つなら 1 次)の B スプライン。`closed` なら
    /// 閉じた(周期の)スプライン(最後の点が最初の点と同じなら、それは除く)。点が足りない
    /// (開いたものは 2 つ、閉じたものは 3 つ)か形にならなければ `None`。
    pub fn add_spline(&mut self, points: &[Spot], control: bool, closed: bool) -> Option<EntityId> {
        self.add_spline_degree(points, control, closed, 3)
    }

    /// [`Sketch::add_spline`] の、制御点のスプラインの次数を選べるもの(3 か 5)。点が次数より少なければ、点の数 - 1 の次数になる。フィット点のスプラインは
    /// 次数を使わない(3 次)。
    pub fn add_spline_degree(
        &mut self,
        points: &[Spot],
        control: bool,
        closed: bool,
        degree: u8,
    ) -> Option<EntityId> {
        let degree = if control { degree.max(1) } else { 3 };
        let mut pts: Vec<Spot> = points.to_vec();
        if closed && pts.len() > 1 {
            let (f, l) = (pts[0], pts[pts.len() - 1]);
            if (f.0.is_some() && f.0 == l.0) || len(sub(f.1, l.1)) < 1e-9 {
                pts.pop();
            }
        }
        let need = if closed { 3 } else { 2 };
        if pts.len() < need {
            return None;
        }
        let pos: Vec<P2> = pts.iter().map(|p| p.1).collect();
        Shape::spline(&pos, control, closed, degree, None)?;
        let ids: Vec<EntityId> = pts.into_iter().map(|p| self.spot(p)).collect();
        Some(self.add_entity(&Entity::Spline {
            id: EntityId(0),
            points: ids,
            control,
            closed,
            degree,
            knots: None,
            construction: false,
        }))
    }

    /// 始点 `start`・終点 `end` と、両端の接線が交わる点 `apex`、形の値 `rho`(0 < rho < 1)から
    /// 円錐曲線を描く。描けない(rho が範囲の外、頂点が両端を結ぶ線の上)なら `None`。
    pub fn add_conic(&mut self, start: Spot, end: Spot, apex: Spot, rho: f64) -> Option<EntityId> {
        let (s, e, a) = (start.1, end.1, apex.1);
        let chord = sub(e, s);
        if !(rho > 0.0 && rho < 1.0) || len(chord) < 1e-6 {
            return None;
        }
        if (cross(chord, sub(a, s)) / len(chord)).abs() < 1e-6 {
            return None;
        }
        let (s, e, a) = (self.spot(start), self.spot(end), self.spot(apex));
        Some(self.add_entity(&Entity::Conic {
            id: EntityId(0),
            start: s,
            end: e,
            apex: a,
            rho,
            construction: false,
        }))
    }

    /// 制御点 `ctrl` の B スプラインを、端の点を `first`・`last`(既にある点)にして足す。
    pub(crate) fn add_control_spline(
        &mut self,
        s: &BSpline,
        first: EntityId,
        last: EntityId,
        construction: bool,
    ) -> EntityId {
        let n = s.ctrl.len();
        let mut ids = vec![first];
        for c in &s.ctrl[1..n - 1] {
            ids.push(self.add_point(c[0], c[1]));
        }
        ids.push(last);
        self.add_entity(&Entity::Spline {
            id: EntityId(0),
            points: ids,
            control: true,
            closed: false,
            degree: s.degree as u8,
            knots: Some(s.knots.clone()),
            construction,
        })
    }

    /// 交点の位置に点を置き、交わった曲線の上に拘束する。
    pub(crate) fn cut_point(&mut self, c: &Cut) -> EntityId {
        let m = self.add_point(c.1[0], c.1[1]);
        self.attach(m, c.2);
        m
    }

    /// 楕円・スプライン・円錐曲線のトリム([`Sketch::trim`] から呼ぶ)。楕円は楕円弧に、
    /// スプラインは制御点のスプライン(節点挿入で形を変えずに分ける)に、円錐曲線は
    /// 円錐曲線(有理 2 次ベジエの分割)になる。分けた端は点にして、交わった曲線の上に拘束する。
    pub(crate) fn trim_free(&mut self, target: EntityId, at: P2, cuts: &[(P2, EntityId)]) -> bool {
        let (Some(e), Some(shape)) = (self.entity(target).cloned(), self.shape(target)) else {
            return false;
        };
        let constr = e.is_construction();
        let plan = plan(&shape, cuts, at);
        if matches!(plan, Plan::Whole) {
            self.remove_entities(&[target]);
            return true;
        }
        let mut pieces = vec![];
        match (&e, &shape, plan) {
            (
                Entity::Ellipse {
                    center,
                    major,
                    minor,
                    ..
                },
                _,
                Plan::Closed { keep_from, keep_to },
            ) => {
                let (s, t) = (self.cut_point(&keep_from), self.cut_point(&keep_to));
                pieces.push(self.ellipse_arc(*center, *major, *minor, s, t, constr));
            }
            (
                Entity::Ellipse {
                    center,
                    major,
                    minor,
                    ends: Some((s0, e0)),
                    ..
                },
                _,
                Plan::Open { lo, hi },
            ) => {
                if let Some(c) = lo {
                    let m = self.cut_point(&c);
                    pieces.push(self.ellipse_arc(*center, *major, *minor, *s0, m, constr));
                }
                if let Some(c) = hi {
                    let m = self.cut_point(&c);
                    pieces.push(self.ellipse_arc(*center, *major, *minor, m, *e0, constr));
                }
            }
            (
                Entity::Spline { .. },
                Shape::Spline { s, closed },
                Plan::Closed { keep_from, keep_to },
            ) => {
                let Some(part) = spline_part(s, *closed, keep_from.0, keep_to.0) else {
                    return false;
                };
                let (a, b) = (self.cut_point(&keep_from), self.cut_point(&keep_to));
                pieces.push(self.add_control_spline(&part, a, b, constr));
            }
            (Entity::Spline { points, .. }, Shape::Spline { s, .. }, Plan::Open { lo, hi }) => {
                let (dlo, dhi) = s.domain();
                let (first, last) = (points[0], points[points.len() - 1]);
                if let Some(c) = lo
                    && let Some(part) = s.extract(dlo, c.0).map(normalized)
                {
                    let m = self.cut_point(&c);
                    pieces.push(self.add_control_spline(&part, first, m, constr));
                }
                if let Some(c) = hi
                    && let Some(part) = s.extract(c.0, dhi).map(normalized)
                {
                    let m = self.cut_point(&c);
                    pieces.push(self.add_control_spline(&part, m, last, constr));
                }
            }
            (Entity::Conic { start, end, .. }, Shape::Conic { p, w }, Plan::Open { lo, hi }) => {
                // 手前の片は始点から交点まで、先の片は交点から終点まで
                for (c, before) in [(lo, true), (hi, false)] {
                    let Some(c) = c else { continue };
                    let (t0, t1) = if before { (0.0, c.0) } else { (c.0, 1.0) };
                    let Some((_, apex, _, rho)) = conic_part(*p, *w, t0, t1) else {
                        continue;
                    };
                    let m = self.cut_point(&c);
                    let a = self.add_point(apex[0], apex[1]);
                    let (s, t) = if before { (*start, m) } else { (m, *end) };
                    pieces.push(self.add_entity(&Entity::Conic {
                        id: EntityId(0),
                        start: s,
                        end: t,
                        apex: a,
                        rho,
                        construction: constr,
                    }));
                }
            }
            _ => return false,
        }
        if pieces.is_empty() {
            return false;
        }
        self.retarget_pieces(target, &pieces);
        self.replace_curve(target, pieces[0]);
        true
    }

    pub(crate) fn ellipse_arc(
        &mut self,
        center: EntityId,
        major: EntityId,
        minor: EntityId,
        s: EntityId,
        e: EntityId,
        construction: bool,
    ) -> EntityId {
        self.add_entity(&Entity::Ellipse {
            id: EntityId(0),
            center,
            major,
            minor,
            ends: Some((s, e)),
            construction,
        })
    }

    /// トリムで消える区間の折れ線([`Sketch::trim_preview`] から呼ぶ。座標は保存されている位置)。
    pub(crate) fn trim_free_preview(
        &self,
        target: EntityId,
        at: P2,
        cuts: &[(P2, EntityId)],
    ) -> Option<Vec<P2>> {
        let shape = self.shape(target)?;
        let (lo, hi) = shape.domain();
        let (a, b) = match plan(&shape, cuts, at) {
            Plan::Whole => (lo, hi),
            Plan::Open { lo: l, hi: h } => (l.map_or(lo, |c| c.0), h.map_or(hi, |c| c.0)),
            Plan::Closed { keep_from, keep_to } => {
                // 消すのは keep_to から進んで keep_from まで(押した所を通る)
                let mut b = keep_from.0;
                if b <= keep_to.0 {
                    b += hi - lo;
                }
                (keep_to.0, b)
            }
        };
        let n = 96;
        Some(
            (0..=n)
                .map(|i| shape.at(a + (b - a) * i as f64 / n as f64))
                .collect(),
        )
    }

    /// 楕円・スプライン・円錐曲線 `id` を距離 `distance` だけずらした、フィット点のスプライン
    /// (ずらした点を通る)を足す。閉じた曲線は閉じたスプラインにし、`outward` なら外へ、
    /// 開いた曲線はたどる向きの左へずらす。作れなければ `None`。
    pub(crate) fn offset_free(
        &mut self,
        id: EntityId,
        distance: f64,
        outward: bool,
    ) -> Option<EntityId> {
        let shape = self.shape(id)?;
        let closed = shape.closed();
        // 区切り(スプラインの節点、楕円の軸の端)ごとに数点ずつ取る
        let poly = shape.polyline(1e-3 * shape.size().max(1.0));
        let (lo, hi) = shape.domain();
        let count = poly.len().clamp(12, 48);
        let n = if closed { count } else { count + 1 };
        let ts: Vec<f64> = (0..n)
            .map(|i| lo + (hi - lo) * i as f64 / count as f64)
            .collect();
        let area: f64 = {
            let p: Vec<P2> = poly.iter().map(|(_, p)| *p).collect();
            (0..p.len())
                .map(|i| cross(p[i], p[(i + 1) % p.len()]))
                .sum()
        };
        let left = if closed {
            (area < 0.0) == outward
        } else {
            outward
        };
        let side = if left { 1.0 } else { -1.0 };
        let pts: Vec<Spot> = ts
            .iter()
            .map(|t| {
                let p = shape.at(*t);
                let d = shape.tangent(*t);
                let nrm = scale([-d[1], d[0]], side * distance);
                (None, [p[0] + nrm[0], p[1] + nrm[1]])
            })
            .collect();
        let new = self.add_spline(&pts, false, closed)?;
        let constr = self.is_construction(id);
        self.set_construction(new, constr);
        Some(new)
    }
}

#[cfg(test)]
#[path = "freeform_tests.rs"]
mod tests;
