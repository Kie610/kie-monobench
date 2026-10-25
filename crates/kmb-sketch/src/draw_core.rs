//! 描く道具のうち、ほかの線に接する・続く物: 線分・円弧の端から続く接線円弧、中点から引く線分、2 つ・3 つに接する円
//! (線分・円・円弧を混ぜてよい)、1 辺で決める正多角形、3 点円弧の長穴。どれも保存されている
//! 座標(先に [`Sketch::adopt_solution`] で解いた位置を写しておく)を元に要素と拘束を足し、
//! 作れなければ何も変えずに `None` を返す。画面に描く見本(`*_preview`)は解いた位置で求める。

use crate::draw::{circle_tangent_to_lines, circumcenter};
use crate::geom::{EPS, add, angle_of, cross, dot, len, scale, sub};
use crate::model::{ConstraintKind as K, Entity, EntityId, Sketch};
use crate::solver::Solution;
use crate::tools::Spot;
use std::f64::consts::{PI, TAU};

type P2 = [f64; 2];

/// 接する相手の形(線分は延ばした直線、円弧は円として扱う)。
#[derive(Debug, Clone, Copy)]
enum Touch {
    /// 直線の上の点(線分の始点)と単位の向き、線分の終点
    Line(P2, P2, P2),
    /// 中心と半径
    Round(P2, f64),
}

impl Touch {
    /// 点 `p` から形までの距離(符号なし)。
    fn gap(self, p: P2) -> f64 {
        match self {
            Touch::Line(a, u, _) => cross(u, sub(p, a)).abs(),
            Touch::Round(c, r) => (len(sub(p, c)) - r).abs(),
        }
    }

    /// 形の大きさの目安(探す半径の範囲を決める)。
    fn size(self, p: P2) -> f64 {
        match self {
            Touch::Line(a, ..) => len(sub(p, a)),
            Touch::Round(c, r) => len(sub(p, c)) + r,
        }
    }
}

/// 接し方。直線は中心の側(左が +1)、円は (k1, k2) で「中心どうしの距離 = k1 R + k2 r」
/// ((1, 1) は外から、(1, -1) は相手の内側に、(-1, 1) は相手を囲んで接する)。
#[derive(Debug, Clone, Copy)]
enum Mode {
    Side(f64),
    Round(f64, f64),
}

/// 半径 `r` で接する円の中心が乗る線(直線か円)。
#[derive(Debug, Clone, Copy)]
enum Locus {
    Line(P2, P2),
    Circle(P2, f64),
}

fn locus(t: Touch, m: Mode, r: f64) -> Option<Locus> {
    match (t, m) {
        (Touch::Line(a, u, _), Mode::Side(s)) => {
            Some(Locus::Line(add(a, scale([-u[1], u[0]], s * r)), u))
        }
        (Touch::Round(c, big), Mode::Round(k1, k2)) => {
            let rad = k1 * big + k2 * r;
            (rad > EPS).then_some(Locus::Circle(c, rad))
        }
        _ => None,
    }
}

/// 2 つの線(延ばした直線と円)の交点。
fn meet(a: Locus, b: Locus) -> Vec<P2> {
    match (a, b) {
        (Locus::Line(p, d), Locus::Line(q, e)) => {
            let den = cross(d, e);
            if den.abs() < 1e-12 {
                vec![]
            } else {
                vec![add(p, scale(d, cross(sub(q, p), e) / den))]
            }
        }
        (Locus::Line(p, d), Locus::Circle(c, r)) | (Locus::Circle(c, r), Locus::Line(p, d)) => {
            let f = sub(p, c);
            let b = dot(f, d);
            let disc = b * b - (dot(f, f) - r * r);
            if disc < 0.0 {
                vec![]
            } else {
                let sq = disc.sqrt();
                vec![add(p, scale(d, -b - sq)), add(p, scale(d, -b + sq))]
            }
        }
        (Locus::Circle(c1, r1), Locus::Circle(c2, r2)) => {
            let d = sub(c2, c1);
            let dl = len(d);
            if dl < EPS || dl > r1 + r2 || dl < (r1 - r2).abs() {
                return vec![];
            }
            let x = (dl * dl + r1 * r1 - r2 * r2) / (2.0 * dl);
            let h = (r1 * r1 - x * x).max(0.0).sqrt();
            let u = scale(d, 1.0 / dl);
            let m = add(c1, scale(u, x));
            let n = [-u[1], u[0]];
            vec![add(m, scale(n, h)), sub(m, scale(n, h))]
        }
    }
}

/// `near` から見た接し方(`near` のある側・内外に円を置く)。
fn mode_toward(t: Touch, near: P2) -> Mode {
    match t {
        Touch::Line(a, u, _) => Mode::Side(if cross(u, sub(near, a)) < 0.0 {
            -1.0
        } else {
            1.0
        }),
        Touch::Round(c, r) if len(sub(near, c)) < r => Mode::Round(1.0, -1.0),
        Touch::Round(..) => Mode::Round(1.0, 1.0),
    }
}

/// 2 つの形に接し、`near` を通る円(中心, 半径)。円は `near` のある側(直線の側、円の内外)に
/// 置き、そのうちいちばん小さいもの(2 直線なら角に寄る方)。平行な 2 直線なら半径は間の
/// 半分で、中心は `near` を中心線へ下ろした所(`near` は円周の上とは限らない)。
fn tangent_two(a: Touch, b: Touch, near: P2) -> Option<(P2, f64)> {
    if let (Touch::Line(p, u, _), Touch::Line(q, w, _)) = (a, b)
        && cross(u, w).abs() < 1e-9
    {
        // 平行な 2 直線: 間にある時だけ
        let gap = cross(u, sub(q, p));
        let at = cross(u, sub(near, p));
        if gap.abs() < EPS || at * gap <= 0.0 || at.abs() >= gap.abs() {
            return None;
        }
        let m = add(p, scale([-u[1], u[0]], gap / 2.0));
        return Some((add(m, scale(u, dot(sub(near, m), u))), gap.abs() / 2.0));
    }
    // near を半径 0 の円とみて、3 つに接する円(near は円周の上)として解く。初めの値は、
    // いろいろな半径で 2 つに接する中心(2 つの線の交点)
    let (ma, mb) = (mode_toward(a, near), mode_toward(b, near));
    let t = [a, b, Touch::Round(near, 0.0)];
    let m = [ma, mb, Mode::Round(1.0, 1.0)];
    let s = a.size(near).max(b.size(near)).max(1.0);
    let mut best: Option<(P2, f64)> = None;
    for k in -12..=6 {
        let r0 = s * 2f64.powi(k);
        let (Some(la), Some(lb)) = (locus(a, ma, r0), locus(b, mb, r0)) else {
            continue;
        };
        for c0 in meet(la, lb) {
            if let Some((c, r)) = newton3(&t, &m, c0, r0, s)
                && best.is_none_or(|(_, br)| r < br - 1e-9 * s)
            {
                best = Some((c, r));
            }
        }
    }
    best
}

/// 3 つの形に接する円(中心, 半径)のうち、中心が `near` に近いもの。接し方の組み合わせごとに、
/// `near` を中心にした初めの値からニュートン法で解く。
fn tangent_three(t: [Touch; 3], near: P2) -> Option<(P2, f64)> {
    let modes = |x: Touch| -> Vec<Mode> {
        match x {
            Touch::Line(..) => vec![Mode::Side(1.0), Mode::Side(-1.0)],
            Touch::Round(..) => vec![
                Mode::Round(1.0, 1.0),
                Mode::Round(1.0, -1.0),
                Mode::Round(-1.0, 1.0),
            ],
        }
    };
    let s = t.iter().map(|x| x.size(near)).fold(1.0, f64::max);
    let gaps: Vec<f64> = t.iter().map(|x| x.gap(near)).collect();
    let starts = [
        gaps.iter().copied().fold(f64::INFINITY, f64::min),
        gaps.iter().sum::<f64>() / 3.0,
        gaps.iter().copied().fold(0.0, f64::max),
    ];
    let mut best: Option<(P2, f64)> = None;
    for m0 in modes(t[0]) {
        for m1 in modes(t[1]) {
            for m2 in modes(t[2]) {
                let m = [m0, m1, m2];
                for r0 in starts {
                    let Some((c, r)) = newton3(&t, &m, near, r0.max(1e-3 * s), s) else {
                        continue;
                    };
                    let better = best.is_none_or(|(bc, br)| {
                        let (d, bd) = (len(sub(c, near)), len(sub(bc, near)));
                        d < bd - 1e-9 * s || ((d - bd).abs() <= 1e-9 * s && r < br)
                    });
                    if better {
                        best = Some((c, r));
                    }
                }
            }
        }
    }
    best
}

/// 接する式 3 つ(中心 x, y と半径 r)をニュートン法で解く。
fn newton3(t: &[Touch; 3], m: &[Mode; 3], c0: P2, r0: f64, s: f64) -> Option<(P2, f64)> {
    let mut z = [c0[0], c0[1], r0];
    for _ in 0..60 {
        let mut jac = [[0.0; 3]; 3];
        let mut f = [0.0; 3];
        for i in 0..3 {
            let c = [z[0], z[1]];
            (f[i], jac[i]) = match (t[i], m[i]) {
                (Touch::Line(a, u, _), Mode::Side(sd)) => (
                    sd * cross(u, sub(c, a)) - z[2],
                    [-sd * u[1], sd * u[0], -1.0],
                ),
                (Touch::Round(cc, big), Mode::Round(k1, k2)) => {
                    let v = sub(c, cc);
                    let d = len(v).max(EPS);
                    (d - (k1 * big + k2 * z[2]), [v[0] / d, v[1] / d, -k2])
                }
                _ => return None,
            };
        }
        if f.iter().all(|v| v.abs() < 1e-11 * s) {
            break;
        }
        let dz = solve3(jac, [-f[0], -f[1], -f[2]])?;
        // 大きく跳ぶ歩は縮める(別の解へ飛ばないように)
        let step = dz.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        let k = if step > s { s / step } else { 1.0 };
        for (zi, di) in z.iter_mut().zip(dz) {
            *zi += k * di;
        }
        if !z.iter().all(|v| v.is_finite()) {
            return None;
        }
    }
    let c = [z[0], z[1]];
    let ok = (0..3).all(|i| match (t[i], m[i]) {
        (Touch::Line(a, u, _), Mode::Side(sd)) => {
            (sd * cross(u, sub(c, a)) - z[2]).abs() < 1e-8 * s
        }
        (Touch::Round(cc, big), Mode::Round(k1, k2)) => {
            (len(sub(c, cc)) - (k1 * big + k2 * z[2])).abs() < 1e-8 * s
        }
        _ => false,
    });
    (ok && z[2] > 1e-6).then_some((c, z[2]))
}

/// 3 × 3 の連立一次方程式(クラメルの公式)。
fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det3 = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let det = det3(m);
    if det.abs() < 1e-14 {
        return None;
    }
    let col = |j: usize| {
        let mut k = m;
        for (row, bi) in k.iter_mut().zip(b) {
            row[j] = bi;
        }
        det3(k) / det
    };
    Some([col(0), col(1), col(2)])
}

/// 始点 `s` から向き `u`(単位)へ出て `e` で終わる、接線円弧の中心と、左へ曲がるか。
/// 終点が向きの直線の上なら `None`。
fn tangent_arc_center(s: P2, u: P2, e: P2) -> Option<(P2, bool)> {
    let n = [-u[1], u[0]];
    let se = sub(s, e);
    let k = dot(n, se);
    if k.abs() < 1e-9 || dot(se, se) < 1e-12 {
        return None;
    }
    // 中心は始点の法線の上: |s + t n - e| = |t|
    let t = -dot(se, se) / (2.0 * k);
    Some((add(s, scale(n, t)), t > 0.0))
}

/// 1 辺 `a`-`b` の正 `n` 角形の頂点(`a` から順に。`b` が 2 つ目)。`side` のある側に作る。
/// 作れない(辺の長さが 0、`side` が辺の直線の上、`n` が 3 未満)なら空。
pub fn edge_polygon_preview(a: P2, b: P2, n: usize, side: P2) -> Vec<P2> {
    edge_polygon(a, b, n, side).map_or(vec![], |(_, _, v)| v)
}

/// (中心, 外接円の半径, 頂点)。
fn edge_polygon(a: P2, b: P2, n: usize, side: P2) -> Option<(P2, f64, Vec<P2>)> {
    let d = sub(b, a);
    let l = len(d);
    if n < 3 || l < 1e-6 {
        return None;
    }
    let turn = cross(d, sub(side, a));
    if turn.abs() < 1e-9 * l {
        return None;
    }
    let sgn = turn.signum();
    let half = PI / n as f64;
    let apothem = l / (2.0 * half.tan());
    let big = l / (2.0 * half.sin());
    let normal = [-d[1] / l * sgn, d[0] / l * sgn];
    let c = add(scale(add(a, b), 0.5), scale(normal, apothem));
    let a0 = angle_of(c, a);
    let v = (0..n)
        .map(|k| {
            let t = a0 + sgn * TAU * k as f64 / n as f64;
            [c[0] + big * t.cos(), c[1] + big * t.sin()]
        })
        .collect::<Vec<_>>();
    Some((c, big, v))
}

impl Sketch {
    /// 要素の接する形。`sol` があれば解いた位置、なければ保存されている座標で。
    fn touch(&self, sol: Option<&Solution>, id: EntityId) -> Option<Touch> {
        let pos = |p: EntityId| match sol {
            Some(s) => s.points.get(&p).copied(),
            None => self.pos(p),
        };
        Some(match self.entity(id)? {
            Entity::Line { a, b, .. } => {
                let (p, q) = (pos(*a)?, pos(*b)?);
                let l = len(sub(q, p));
                if l < EPS {
                    return None;
                }
                Touch::Line(p, scale(sub(q, p), 1.0 / l), q)
            }
            Entity::Circle { center, radius, .. } => {
                let r = match sol {
                    Some(s) => *s.radii.get(&id)?,
                    None => *radius,
                };
                Touch::Round(pos(*center)?, r)
            }
            Entity::Arc { center, start, .. } => {
                let c = pos(*center)?;
                Touch::Round(c, len(sub(pos(*start)?, c)))
            }
            _ => return None,
        })
    }

    /// 円に接する拘束(線分なら [`K::Tangent`]、円・円弧なら [`K::TangentCircles`])を付ける。
    fn tangent_to(&mut self, other: EntityId, circle: EntityId) {
        let kind = match self.entity(other) {
            Some(Entity::Line { .. }) => K::Tangent {
                line: other,
                circle,
            },
            _ => K::TangentCircles {
                a: other,
                b: circle,
            },
        };
        self.add_constraint(kind);
    }

    /// 曲線 `curve`(線分か円弧)の端 `at` から出る向き(単位)と、その点。
    fn leaving(&self, sol: Option<&Solution>, curve: EntityId, at: EntityId) -> Option<(P2, P2)> {
        let pos = |p: EntityId| match sol {
            Some(s) => s.points.get(&p).copied(),
            None => self.pos(p),
        };
        let unit = |v: P2| {
            let l = len(v);
            (l > EPS).then(|| scale(v, 1.0 / l))
        };
        match self.entity(curve)? {
            Entity::Line { a, b, .. } => {
                let other = if at == *b {
                    *a
                } else if at == *a {
                    *b
                } else {
                    return None;
                };
                let s = pos(at)?;
                Some((s, unit(sub(s, pos(other)?))?))
            }
            Entity::Arc {
                center, start, end, ..
            } => {
                let (c, s) = (pos(*center)?, pos(at)?);
                let r = sub(s, c);
                // 円弧は始点から終点へ反時計回り。終点からは反時計回りの接線の向き、
                // 始点からは逆にたどる向きへ出る
                let u = if at == *end {
                    [-r[1], r[0]]
                } else if at == *start {
                    [r[1], -r[0]]
                } else {
                    return None;
                };
                Some((s, unit(u)?))
            }
            _ => None,
        }
    }

    /// 線分か円弧 `curve` の端 `at` から、そこでの向きに接して続く円弧を `end` まで描く
    /// (接線円弧)。線分なら [`Sketch::add_tangent_arc`] と同じ。円弧なら継ぎ目で接する拘束
    /// ([`K::TangentCircles`]。端を共有するので、両方の中心と継ぎ目が一直線の式)を付ける。
    /// 描けない(終点が向きの直線の上、`at` が端でない)なら `None`。
    pub fn add_tangent_arc_from(
        &mut self,
        curve: EntityId,
        at: EntityId,
        end: Spot,
    ) -> Option<EntityId> {
        match self.entity(curve)? {
            Entity::Line { .. } => return self.add_tangent_arc(curve, at, end),
            Entity::Arc { .. } => {}
            _ => return None,
        }
        if end.0 == Some(at) {
            return None;
        }
        let (s, u) = self.leaving(None, curve, at)?;
        let (c, left) = tangent_arc_center(s, u, end.1)?;
        let center = self.add_point(c[0], c[1]);
        let e = self.spot(end);
        // 左へ曲がるなら始点から反時計回り、右なら終点から反時計回り
        let arc = if left {
            self.add_arc(center, at, e)
        } else {
            self.add_arc(center, e, at)
        };
        self.add_constraint(K::TangentCircles { a: curve, b: arc });
        Some(arc)
    }

    /// [`Sketch::add_tangent_arc_from`] で描く円弧の見本(解いた位置 `sol` での折れ線。
    /// `at` から `end` へ、およそ 3° ごと)。描けなければ `None`。
    pub fn tangent_arc_preview(
        &self,
        sol: &Solution,
        curve: EntityId,
        at: EntityId,
        end: [f64; 2],
    ) -> Option<Vec<[f64; 2]>> {
        let (s, u) = self.leaving(Some(sol), curve, at)?;
        let (c, left) = tangent_arc_center(s, u, end)?;
        let r = len(sub(s, c));
        let a0 = angle_of(c, s);
        let sweep = if left {
            (angle_of(c, end) - a0).rem_euclid(TAU)
        } else {
            -(a0 - angle_of(c, end)).rem_euclid(TAU)
        };
        let n = ((sweep.abs() / 3f64.to_radians()).ceil() as usize).max(8);
        Some(
            (0..=n)
                .map(|k| {
                    let t = a0 + sweep * k as f64 / n as f64;
                    [c[0] + r * t.cos(), c[1] + r * t.sin()]
                })
                .collect(),
        )
    }

    /// 中点 `mid` から引く線分(中点の線分)。`end` が一方の端で、もう一方は `mid` について
    /// 点対称の位置。中点の点(`mid` が既存の点ならそれ)に [`K::Midpoint`] を付ける。
    /// `mid` と `end` が重なるなら `None`。
    pub fn add_midpoint_line(&mut self, mid: Spot, end: Spot) -> Option<EntityId> {
        if len(sub(end.1, mid.1)) < 1e-6 || (mid.0.is_some() && mid.0 == end.0) {
            return None;
        }
        let o = sub(scale(mid.1, 2.0), end.1);
        let a = self.add_point(o[0], o[1]);
        let b = self.spot(end);
        let line = self.add_line(a, b);
        let m = self.spot(mid);
        self.add_constraint(K::Midpoint { point: m, line });
        Some(line)
    }

    /// 2 つの線分・円・円弧 `a`、`b` に接する円を描く。円は
    /// カーソル `near` のある側(直線の側、円の内外)に置き、`near` を通る大きさのうち
    /// いちばん小さいもの(角に寄る方)。平行な 2 本の線分なら間に入る円。両方に接する拘束を
    /// 付ける(半径は自由)。描けなければ `None`。
    pub fn add_circle_tangent_two(
        &mut self,
        a: EntityId,
        b: EntityId,
        near: [f64; 2],
    ) -> Option<EntityId> {
        if a == b {
            return None;
        }
        let (c, r) = tangent_two(self.touch(None, a)?, self.touch(None, b)?, near)?;
        let cp = self.add_point(c[0], c[1]);
        let circle = self.add_circle(cp, r);
        self.tangent_to(a, circle);
        self.tangent_to(b, circle);
        Some(circle)
    }

    /// [`Sketch::add_circle_tangent_two`] で描く円の見本(解いた位置 `sol` での中心と半径)。
    pub fn circle_tangent_two_preview(
        &self,
        sol: &Solution,
        a: EntityId,
        b: EntityId,
        near: [f64; 2],
    ) -> Option<([f64; 2], f64)> {
        if a == b {
            return None;
        }
        tangent_two(self.touch(Some(sol), a)?, self.touch(Some(sol), b)?, near)
    }

    /// 3 つの線分・円・円弧に接する円を描く。候補のうち中心が `near` に近いもの(すべて線分なら
    /// [`Sketch::add_circle_tangent_to_lines`] と同じ円)。3 つそれぞれに接する拘束を付ける。
    /// 描けなければ `None`。
    pub fn add_circle_tangent_three(
        &mut self,
        ids: [EntityId; 3],
        near: [f64; 2],
    ) -> Option<EntityId> {
        let (c, r) = self.tangent_three_at(None, ids, near)?;
        let cp = self.add_point(c[0], c[1]);
        let circle = self.add_circle(cp, r);
        for id in ids {
            self.tangent_to(id, circle);
        }
        Some(circle)
    }

    /// [`Sketch::add_circle_tangent_three`] で描く円の見本(解いた位置 `sol` での中心と半径)。
    pub fn circle_tangent_three_preview(
        &self,
        sol: &Solution,
        ids: [EntityId; 3],
        near: [f64; 2],
    ) -> Option<([f64; 2], f64)> {
        self.tangent_three_at(Some(sol), ids, near)
    }

    fn tangent_three_at(
        &self,
        sol: Option<&Solution>,
        ids: [EntityId; 3],
        near: P2,
    ) -> Option<(P2, f64)> {
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
            return None;
        }
        let t = [
            self.touch(sol, ids[0])?,
            self.touch(sol, ids[1])?,
            self.touch(sol, ids[2])?,
        ];
        if let [
            Touch::Line(a, _, p),
            Touch::Line(b, _, q),
            Touch::Line(c, _, r),
        ] = t
        {
            return circle_tangent_to_lines([(a, p), (b, q), (c, r)], near);
        }
        tangent_three(t, near)
    }

    /// 1 辺 `a`-`b` から正 `n` 角形を描く。`side` のある側に作る。
    /// 拘束は [`Sketch::add_polygon`] と同じ(頂点を補助線の外接円の上に置き、辺の長さを等しく
    /// する)。足した辺を、`a`-`b` から順に返す。作れなければ `None`。
    pub fn add_edge_polygon(
        &mut self,
        a: Spot,
        b: Spot,
        n: usize,
        side: [f64; 2],
    ) -> Option<Vec<EntityId>> {
        if a.0.is_some() && a.0 == b.0 {
            return None;
        }
        let (c, big, v) = edge_polygon(a.1, b.1, n, side)?;
        let cp = self.add_point(c[0], c[1]);
        let circle = self.add_circle(cp, big);
        self.set_construction(circle, true);
        let mut pts = vec![self.spot(a), self.spot(b)];
        for p in &v[2..] {
            pts.push(self.add_point(p[0], p[1]));
        }
        let sides: Vec<EntityId> = (0..n)
            .map(|k| self.add_line(pts[k], pts[(k + 1) % n]))
            .collect();
        self.add_constraint(K::Polygon {
            circle,
            sides: sides.clone(),
        });
        Some(sides)
    }

    /// 3 点円弧の長穴: 中心線の円弧を始まり `a`、途中 `mid`、終わり `b` の 3 点で決め、幅の半分
    /// `w` の長穴を描く。足した要素と拘束は [`Sketch::add_arc_slot`] と同じで、
    /// (外の円弧、内の円弧、始まりの端、終わりの端)の順で返す。端の円弧の中心は、
    /// 中心線の円弧の始まり・終わり(反時計回りに見て)。`a`、`b` が既存の点ならその点を使う。
    /// 3 点が一直線、幅が中心線の半径より大きいなどで作れなければ `None`。
    pub fn add_three_point_arc_slot(
        &mut self,
        a: Spot,
        mid: [f64; 2],
        b: Spot,
        w: f64,
    ) -> Option<[EntityId; 4]> {
        if len(sub(b.1, a.1)) < 1e-6 || (a.0.is_some() && a.0 == b.0) {
            return None;
        }
        let c = circumcenter(a.1, b.1, mid)?;
        let a0 = angle_of(c, a.1);
        let ccw = (angle_of(c, mid) - a0).rem_euclid(TAU) < (angle_of(c, b.1) - a0).rem_euclid(TAU);
        let (start, end) = if ccw { (a, b) } else { (b, a) };
        self.arc_slot_spots((None, c), start, end, w)
    }
}
