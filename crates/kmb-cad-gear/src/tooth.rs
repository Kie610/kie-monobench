//! 歯の曲線の計算(文書にもカーネルにも触らない純関数)。種類ごとの形(`kinds/`)が使う共通部:
//! 区間の作り方・回し方と写し方、曲線の二円弧の近似、インボリュート、2 つの線に接する丸み。
//!
//! 輪郭の区間は線・円弧・円だけなので、歯面の曲線は二円弧(つなぎ目で向きのそろう 2 つの円弧)を
//! 並べて近似する。円弧なので加工のポストは円弧の指令のまま出せ、面の数も少ない。

use kmb_core::{ProfileEdge, Segment};

pub type P2 = [f64; 2];

/// 近似で許す隔たり: 0.002 mm かモジュールの 0.1% の小さい方。
pub fn tolerance(module: f64) -> f64 {
    (module * 1e-3).min(0.002)
}

pub fn polar(r: f64, a: f64) -> P2 {
    [r * a.cos(), r * a.sin()]
}

fn dist(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// 区間 `seg` を `from` から たどる名前つきの区間(円弧は反時計回りの形しか持たないので、終点から
/// 始まる時は逆にたどる印を付ける)。
pub fn edge(name: impl Into<String>, seg: Segment, from: P2) -> ProfileEdge {
    let reversed = match seg {
        Segment::Circle { .. } => false,
        _ => dist(seg.end_point(), from) < dist(seg.start_point(), from),
    };
    ProfileEdge {
        name: name.into(),
        segment: seg,
        reversed,
    }
}

/// 中心 `c`・半径 `r` の円弧で `from` から `to` へ(`ccw` なら反時計回り)。
pub fn arc(name: impl Into<String>, c: P2, r: f64, from: P2, to: P2, ccw: bool) -> ProfileEdge {
    let a = (from[1] - c[1]).atan2(from[0] - c[0]);
    let b = (to[1] - c[1]).atan2(to[0] - c[0]);
    let (start, end, reversed) = if ccw {
        (a, a + (b - a).rem_euclid(std::f64::consts::TAU), false)
    } else {
        (b, b + (a - b).rem_euclid(std::f64::consts::TAU), true)
    };
    ProfileEdge {
        name: name.into(),
        segment: Segment::Arc {
            center: c,
            radius: r,
            start,
            end,
        },
        reversed,
    }
}

/// 原点のまわりに `a` 回した区間。
pub fn rotate(e: &ProfileEdge, a: f64) -> ProfileEdge {
    let (s, c) = a.sin_cos();
    let r = |p: P2| [c * p[0] - s * p[1], s * p[0] + c * p[1]];
    let segment = match e.segment {
        Segment::Line { a: p, b: q } => Segment::Line { a: r(p), b: r(q) },
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => Segment::Arc {
            center: r(center),
            radius,
            start: start + a,
            end: end + a,
        },
        Segment::Circle { center, radius } => Segment::Circle {
            center: r(center),
            radius,
        },
    };
    ProfileEdge {
        segment,
        ..e.clone()
    }
}

/// X 軸で写した区間。並びを逆にしてたどると写した側の輪になる(たどる向きの印はそのまま)。
pub fn mirror(e: &ProfileEdge, name: impl Into<String>) -> ProfileEdge {
    let m = |p: P2| [p[0], -p[1]];
    let segment = match e.segment {
        Segment::Line { a, b } => Segment::Line { a: m(b), b: m(a) },
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => Segment::Arc {
            center: m(center),
            radius,
            start: -end,
            end: -start,
        },
        Segment::Circle { center, radius } => Segment::Circle {
            center: m(center),
            radius,
        },
    };
    ProfileEdge {
        name: name.into(),
        segment,
        reversed: e.reversed,
    }
}

/// `d` だけ平行に動かした区間。
pub fn translate(e: &ProfileEdge, d: P2) -> ProfileEdge {
    let t = |p: P2| [p[0] + d[0], p[1] + d[1]];
    let segment = match e.segment {
        Segment::Line { a, b } => Segment::Line { a: t(a), b: t(b) },
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => Segment::Arc {
            center: t(center),
            radius,
            start,
            end,
        },
        Segment::Circle { center, radius } => Segment::Circle {
            center: t(center),
            radius,
        },
    };
    ProfileEdge {
        segment,
        ..e.clone()
    }
}

/// 左右の同じ歯の右半分。X 軸の向きに立てた歯の、右(角の負の側)の歯面を歯底から歯先へたどる
/// 区間を、名前の後ろの部分(`fillet`・`neck`・`0`・`1`…・`round`)と並べる。最初の区間は歯底円の上から
/// 始まり、最後の区間は歯先円の上で終わる。
pub struct HalfTooth {
    pub flank: Vec<(String, ProfileEdge)>,
    pub tip_radius: f64,
    pub root_radius: f64,
}

/// 右半分 `half` を写して歯 1 つにし、`teeth` 枚を 1 回りに並べた閉じた輪(反時計回り)。1 枚目の歯は
/// X 軸の向き。区間の名前は `t<歯の番号>:` に、右の歯面は `r`、左は `l` を付け(`t3:r0`・`t3:lfillet`)、
/// 歯先の円弧は `t<i>:tip`、i 枚目と次の歯の間の歯底の円弧は `t<i>:root`。
pub fn ring_of_teeth(half: &HalfTooth, teeth: u32) -> Vec<ProfileEdge> {
    let step = std::f64::consts::TAU / teeth as f64;
    let Some((_, first)) = half.flank.first() else {
        return Vec::new();
    };
    let Some((_, last)) = half.flank.last() else {
        return Vec::new();
    };
    let root_at = {
        let p = first.from_point();
        -p[1].atan2(p[0])
    };
    let tip_from = last.to_point();
    let mut out = Vec::new();
    for i in 0..teeth {
        let mut tooth = Vec::new();
        for (k, e) in &half.flank {
            tooth.push(ProfileEdge {
                name: format!("t{i}:r{k}"),
                ..e.clone()
            });
        }
        if tip_from[1] < -1e-12 {
            let to = [tip_from[0], -tip_from[1]];
            tooth.push(arc(
                format!("t{i}:tip"),
                [0.0, 0.0],
                half.tip_radius,
                tip_from,
                to,
                true,
            ));
        }
        for (k, e) in half.flank.iter().rev() {
            tooth.push(mirror(e, format!("t{i}:l{k}")));
        }
        if step - 2.0 * root_at > 1e-12 {
            tooth.push(arc(
                format!("t{i}:root"),
                [0.0, 0.0],
                half.root_radius,
                polar(half.root_radius, root_at),
                polar(half.root_radius, step - root_at),
                true,
            ));
        }
        out.extend(tooth.iter().map(|e| rotate(e, step * i as f64)));
    }
    out
}

/// 点 `p` から向き `t` で出て点 `q` に着く円弧(ほぼまっすぐなら線分)。
fn arc_leaving(p: P2, t: P2, q: P2) -> Segment {
    let chord = [q[0] - p[0], q[1] - p[1]];
    let left = [-t[1], t[0]];
    let side = left[0] * chord[0] + left[1] * chord[1];
    let len2 = chord[0] * chord[0] + chord[1] * chord[1];
    // 弦からの膨らみが弦の長さの 1e-9 未満なら線分にする(半径が大きすぎる円弧を作らない)
    if side.abs() <= 1e-9 * len2.sqrt() * len2.sqrt().max(1.0) || len2 < 1e-24 {
        return Segment::Line { a: p, b: q };
    }
    let s = len2 / (2.0 * side);
    let c = [p[0] + left[0] * s, p[1] + left[1] * s];
    arc("", c, s.abs(), p, q, s > 0.0).segment
}

/// 両端の点と進む向き(単位ベクトル)から、つなぎ目で向きのそろう 2 つの円弧(両側の接線の長さを
/// 等しく取る作り方)。作れなければ `None`。
fn biarc(p0: P2, t0: P2, p1: P2, t1: P2) -> Option<Vec<Segment>> {
    let v = [p1[0] - p0[0], p1[1] - p0[1]];
    let vv = v[0] * v[0] + v[1] * v[1];
    if vv < 1e-24 {
        return None;
    }
    let vt = v[0] * (t0[0] + t1[0]) + v[1] * (t0[1] + t1[1]);
    let k = 2.0 * (1.0 - (t0[0] * t1[0] + t0[1] * t1[1]));
    // 接線の長さ d は、2 つの接線の先の点の間が 2d になる式 k d² + 2 vt d − vv = 0 の正の根
    let d = if k.abs() < 1e-12 {
        if vt.abs() < 1e-12 {
            return None;
        }
        vv / (2.0 * vt)
    } else {
        (-vt + (vt * vt + k * vv).sqrt()) / k
    };
    if !(d.is_finite() && d > 0.0) {
        return None;
    }
    let join = [
        (p0[0] + d * t0[0] + p1[0] - d * t1[0]) / 2.0,
        (p0[1] + d * t0[1] + p1[1] - d * t1[1]) / 2.0,
    ];
    let first = arc_leaving(p0, t0, join);
    let back = arc_leaving(p1, [-t1[0], -t1[1]], join);
    let second = match back {
        Segment::Line { a, b } => Segment::Line { a: b, b: a },
        other => other,
    };
    Some(vec![first, second])
}

/// 曲線 `f`(引数 → 点と進む向きの単位ベクトル)の `t0`〜`t1` を、隔たり `tol` 以下の二円弧で近似した
/// 区間(たどる順)。合わない所は半分に分けて近似し直す。
pub fn fit_biarcs(f: &dyn Fn(f64) -> (P2, P2), t0: f64, t1: f64, tol: f64) -> Vec<Segment> {
    let mut out = Vec::new();
    fit_into(f, t0, t1, tol, 0, &mut out);
    out
}

fn fit_into(
    f: &dyn Fn(f64) -> (P2, P2),
    t0: f64,
    t1: f64,
    tol: f64,
    depth: u32,
    out: &mut Vec<Segment>,
) {
    let (p0, d0) = f(t0);
    let (p1, d1) = f(t1);
    if let Some(segs) = biarc(p0, d0, p1, d1) {
        let worst = (1..8)
            .map(|i| {
                let q = f(t0 + (t1 - t0) * i as f64 / 8.0).0;
                segs.iter()
                    .map(|s| s.distance(q))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max);
        if worst <= tol || depth >= 14 {
            out.extend(segs);
            return;
        }
    } else if depth >= 14 {
        out.push(Segment::Line { a: p0, b: p1 });
        return;
    }
    let mid = (t0 + t1) / 2.0;
    fit_into(f, t0, mid, tol, depth + 1, out);
    fit_into(f, mid, t1, tol, depth + 1, out);
}

/// インボリュート関数 inv φ = tan φ − φ。
pub fn inv(phi: f64) -> f64 {
    phi.tan() - phi
}

/// 基礎円の半径 `rb` のインボリュートの、巻き戻しの角 `t` での点と進む向き(t = 0 で基礎円の上の
/// (rb, 0) から X の向きへ出て、反時計回りに開く)。
pub fn involute(rb: f64, t: f64) -> (P2, P2) {
    let (s, c) = t.sin_cos();
    ([rb * (c + t * s), rb * (s - t * c)], [c, s])
}

/// 半径 `r` での巻き戻しの角(`r` は基礎円の半径 `rb` 以上)。
pub fn roll_at(rb: f64, r: f64) -> f64 {
    ((r / rb).powi(2) - 1.0).max(0.0).sqrt()
}

/// 線 `f`(半径 → 点と外へ進む向き)と、原点を中心とする半径 `circle` の円の両方に接する半径 `rho`
/// の丸みの、線の上の接点の半径と丸みの中心。`side` は丸みの中心を置く側で、進む向きを右に 90° 回す
/// 側なら −1、左なら +1。`outside` なら円の外(歯底)、そうでなければ円の内(歯先)に接する。
/// 線の半径 `lo`〜`hi` の中で接する所が無ければ `None`。
pub fn round_between(
    f: &dyn Fn(f64) -> (P2, P2),
    lo: f64,
    hi: f64,
    circle: f64,
    rho: f64,
    side: f64,
    outside: bool,
) -> Option<(f64, P2)> {
    let target = if outside { circle + rho } else { circle - rho };
    let center = |r: f64| {
        let (p, d) = f(r);
        let n = [-d[1] * side, d[0] * side];
        [p[0] + rho * n[0], p[1] + rho * n[1]]
    };
    let g = |r: f64| {
        let c = center(r);
        c[0].hypot(c[1]) - target
    };
    let (mut a, mut b) = (lo, hi);
    let (ga, gb) = (g(a), g(b));
    if ga.signum() == gb.signum() {
        return None;
    }
    for _ in 0..80 {
        let m = (a + b) / 2.0;
        if g(m).signum() == ga.signum() {
            a = m;
        } else {
            b = m;
        }
    }
    let r = (a + b) / 2.0;
    Some((r, center(r)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biarcs_of_an_involute_stay_within_the_tolerance() {
        let rb = 18.79;
        let tol = tolerance(2.0);
        let f = |t: f64| involute(rb, t);
        let segs = fit_biarcs(&f, 0.0, 0.6, tol);
        assert!(segs.len() >= 2);
        // 端がつながる
        for w in segs.windows(2) {
            assert!(
                dist(w[0].end_point(), w[1].start_point()) < 1e-9
                    || dist(w[0].start_point(), w[1].start_point()) < 1e-9
                    || dist(w[0].end_point(), w[1].end_point()) < 1e-9
                    || dist(w[0].start_point(), w[1].end_point()) < 1e-9
            );
        }
        for i in 0..=400 {
            let q = involute(rb, 0.6 * i as f64 / 400.0).0;
            let d = segs
                .iter()
                .map(|s| s.distance(q))
                .fold(f64::INFINITY, f64::min);
            assert!(d <= tol + 1e-9, "{i} {d}");
        }
    }

    #[test]
    fn the_involute_leaves_the_base_circle_square_to_it() {
        let (p, d) = involute(10.0, 0.0);
        assert!(dist(p, [10.0, 0.0]) < 1e-12 && dist(d, [1.0, 0.0]) < 1e-12);
        // 半径 r での極角は inv φ(cos φ = rb / r)
        let r = 12.0;
        let (q, _) = involute(10.0, roll_at(10.0, r));
        assert!((q[0].hypot(q[1]) - r).abs() < 1e-9);
        let phi = (10.0f64 / r).acos();
        assert!((q[1].atan2(q[0]) - inv(phi)).abs() < 1e-12);
    }

    #[test]
    fn mirrored_and_rotated_arcs_keep_their_ends() {
        let e = arc("a", [1.0, 1.0], 1.0, [2.0, 1.0], [1.0, 2.0], true);
        let m = mirror(&e, "b");
        assert!(
            dist(m.to_point(), [2.0, -1.0]) < 1e-12,
            "{:?}",
            m.to_point()
        );
        assert!(dist(m.from_point(), [1.0, -2.0]) < 1e-12);
        let r = rotate(&e, std::f64::consts::FRAC_PI_2);
        assert!(dist(r.from_point(), [-1.0, 2.0]) < 1e-12);
        let cw = arc("c", [0.0, 0.0], 1.0, [0.0, 1.0], [1.0, 0.0], false);
        assert!(dist(cw.from_point(), [0.0, 1.0]) < 1e-12);
        assert!(dist(cw.to_point(), [1.0, 0.0]) < 1e-12);
    }
}
