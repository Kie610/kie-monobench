//! 経路への円弧のあてはめ(アークフィット)。
//!
//! 3D の加工や負荷制御粗取りの経路は、細かい直線(G1)をたくさん並べたものになる。続いた
//! 直線の点の列を、元の点から許容差の中に収まる限り、長い直線と XY 平面の円弧(Z が角度に
//! 比例して変われば、らせん)にまとめる。G-code が小さくなり、機械が細かい区間の継ぎ目ごとに
//! 速さを落とすのを減らせる。
//!
//! 方式: 点の列の頭から、直線と円弧のそれぞれで「許容差に収まる最も先の点」を探し(倍々に
//! 延ばしてから二分で詰める)、先まで届く方を採って、そこから繰り返す。円弧は始点・中ほどの
//! 点・終点の 3 点を通る円とし、次をすべて満たせば合格とする。
//!
//! - 間の点が円から許容差の中にある
//! - 間の点の Z が、角度に比例して変わるらせんから許容差の中にある
//! - 角度が一方向に進む(戻らない、一周しない)
//! - 隣り合う点の間で、円弧と元の直線(弦)の離れ(矢高)が許容差の中にある
//!
//! 直線は、間の点が線分から許容差の中にあり、線分に沿って戻らなければ合格とする。
//! 早送り、元からある円弧、送りの速さが変わる所はまたがない。

use jc_toolpath::{ArcDir, Move, P3, arc_sweep};

/// 円弧の半径の下限(mm)。G-code の座標は 0.001 mm に丸めるので、小さすぎる円弧は
/// 始点と終点で半径が食い違って読まれる。
pub const MIN_RADIUS: f64 = 0.1;
/// 円弧の半径の上限(mm)。これより大きい円弧は中心が遠く、ほとんど直線なので直線にする。
pub const MAX_RADIUS: f64 = 500.0;

/// あてはめの設定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitOptions {
    /// 元の点からの離れの上限(mm)
    pub tolerance: f64,
    /// 円弧も使うか(偽なら直線をまとめるだけ)
    pub arcs: bool,
    pub min_radius: f64,
    pub max_radius: f64,
}

impl FitOptions {
    pub fn new(tolerance: f64) -> FitOptions {
        FitOptions {
            tolerance,
            arcs: true,
            min_radius: MIN_RADIUS,
            max_radius: MAX_RADIUS,
        }
    }
}

/// 動きの列の、続いた直線をまとめ直す。最初の動き(位置が決まる前)と早送り、円弧はそのまま。
pub fn fit(moves: &[Move], opt: &FitOptions) -> Vec<Move> {
    let mut out = Vec::with_capacity(moves.len());
    let mut cur: Option<P3> = None;
    let mut k = 0;
    while k < moves.len() {
        match (moves[k], cur) {
            (Move::Linear { feed, .. }, Some(start)) => {
                let mut pts = vec![start];
                while let Some(&Move::Linear { to, feed: f }) = moves.get(k)
                    && f == feed
                {
                    if jc_toolpath::dist(*pts.last().expect("始点がある"), to) > 1e-9 {
                        pts.push(to);
                    }
                    k += 1;
                }
                out.extend(fit_run(&pts, feed, opt));
                cur = pts.last().copied();
            }
            (m, _) => {
                out.push(m);
                cur = Some(m.to());
                k += 1;
            }
        }
    }
    out
}

/// 1 つの送りで続く点の列(`pts[0]` は今いる所)をまとめる。
fn fit_run(pts: &[P3], feed: f64, opt: &FitOptions) -> Vec<Move> {
    let last = pts.len() - 1;
    let mut out = Vec::new();
    let mut i = 0;
    while i < last {
        let jl = reach(i + 1, last, |j| line_ok(pts, i, j, opt.tolerance));
        let arc = (opt.arcs && i + 2 <= last && arc_of(pts, i, i + 2, opt).is_some())
            .then(|| reach(i + 2, last, |j| arc_of(pts, i, j, opt).is_some()))
            .filter(|&ja| ja > jl)
            .and_then(|ja| arc_of(pts, i, ja, opt).map(|a| (ja, a)));
        match arc {
            Some((ja, (center, dir))) => {
                out.push(Move::Arc {
                    to: pts[ja],
                    center,
                    dir,
                    feed,
                });
                i = ja;
            }
            None => {
                out.push(Move::Linear { to: pts[jl], feed });
                i = jl;
            }
        }
    }
    out
}

/// `start`(合格する)から `last` までで、`ok` が合格する最も先の番号。倍々に延ばし、
/// 不合格に当たったら、その手前との間を二分で詰める。
fn reach(start: usize, last: usize, ok: impl Fn(usize) -> bool) -> usize {
    let mut good = start;
    let mut bad = last + 1;
    let mut step = 1;
    while good < last {
        let t = (good + step).min(last);
        if ok(t) {
            good = t;
            step *= 2;
        } else {
            bad = t;
            break;
        }
    }
    while bad - good > 1 {
        let m = (good + bad) / 2;
        if ok(m) {
            good = m;
        } else {
            bad = m;
        }
    }
    good
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `pts[i]` から `pts[j]` への 1 本の直線で、間の点が許容差に収まるか。
fn line_ok(pts: &[P3], i: usize, j: usize, tol: f64) -> bool {
    let (a, b) = (pts[i], pts[j]);
    let ab = sub(b, a);
    let len2 = dot(ab, ab);
    let mut prev = 0.0;
    pts[i + 1..j].iter().all(|&p| {
        let ap = sub(p, a);
        let t = if len2 > 0.0 { dot(ap, ab) / len2 } else { 0.0 };
        // 線分に沿って戻る(行って戻る)所はまとめない
        let forward = t >= prev - tol / len2.sqrt().max(1e-12);
        prev = t;
        let tc = t.clamp(0.0, 1.0);
        let q = [a[0] + ab[0] * tc, a[1] + ab[1] * tc, a[2] + ab[2] * tc];
        forward && jc_toolpath::dist(p, q) <= tol
    })
}

/// XY で 3 点を通る円の中心。3 点がほぼ一直線なら None。
fn circumcenter(a: P3, b: P3, c: P3) -> Option<[f64; 2]> {
    let (bx, by) = (b[0] - a[0], b[1] - a[1]);
    let (cx, cy) = (c[0] - a[0], c[1] - a[1]);
    let d = 2.0 * (bx * cy - by * cx);
    let scale = (bx * bx + by * by).max(cx * cx + cy * cy);
    if d.abs() <= 1e-12 * scale.max(1e-12) {
        return None;
    }
    let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
    Some([
        a[0] + (cy * b2 - by * c2) / d,
        a[1] + (bx * c2 - cx * b2) / d,
    ])
}

/// `pts[i]` から `pts[j]` への 1 つの円弧(中心と向き)。許容差に収まらなければ None。
fn arc_of(pts: &[P3], i: usize, j: usize, opt: &FitOptions) -> Option<([f64; 2], ArcDir)> {
    use std::f64::consts::TAU;
    let tol = opt.tolerance;
    let (a, b, m) = (pts[i], pts[j], pts[(i + j) / 2]);
    let c = circumcenter(a, m, b)?;
    let r = (a[0] - c[0]).hypot(a[1] - c[1]);
    if !(opt.min_radius..=opt.max_radius).contains(&r) {
        return None;
    }
    let turn = (m[0] - a[0]) * (b[1] - m[1]) - (m[1] - a[1]) * (b[0] - m[0]);
    let dir = if turn > 0.0 { ArcDir::Ccw } else { ArcDir::Cw };
    let sweep = arc_sweep(a, b, c, dir);
    let s = sweep.abs();
    if s >= TAU - 1e-6 {
        return None;
    }
    let sign = sweep.signum();
    let a0 = (a[1] - c[1]).atan2(a[0] - c[0]);
    // 始点から向きに沿って進んだ角度(0 以上 2π 未満)
    let progressed = |p: P3| ((p[1] - c[1]).atan2(p[0] - c[0]) - a0) * sign;
    let mut prev = 0.0;
    for (k, &p) in pts.iter().enumerate().take(j + 1).skip(i + 1) {
        let t = if k == j {
            s
        } else {
            let t = progressed(p).rem_euclid(TAU);
            let radial = ((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs();
            if radial > tol || t > s + 1e-9 {
                return None;
            }
            t
        };
        if t <= prev {
            return None;
        }
        let z = a[2] + (b[2] - a[2]) * t / s;
        // 隣の点との間の、円弧と弦の離れ(矢高)
        let sagitta = r * (1.0 - ((t - prev) / 2.0).cos());
        if (p[2] - z).abs() > tol || sagitta > tol {
            return None;
        }
        prev = t;
    }
    Some((c, dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_toolpath::{Toolpath, check, dist};

    const TOL: f64 = 0.005;

    fn start(p: P3) -> Vec<Move> {
        vec![Move::Rapid { to: p }]
    }

    fn lines(pts: &[P3], feed: f64) -> Vec<Move> {
        pts.iter().map(|&to| Move::Linear { to, feed }).collect()
    }

    /// 動きの列を細かい点に分ける(始めの早送りの所から)。
    fn sampled(moves: &[Move]) -> Vec<P3> {
        let mut cur = moves[0].to();
        let mut v = vec![cur];
        for m in &moves[1..] {
            v.extend(m.points(cur, 0.01));
            cur = m.to();
        }
        v
    }

    fn seg_dist(p: P3, a: P3, b: P3) -> f64 {
        let ab = sub(b, a);
        let l2 = dot(ab, ab);
        let t = if l2 > 0.0 {
            (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        dist(p, [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t])
    }

    /// 元の点がすべて、まとめた経路から `tol` の中にあるか(と、その最大の離れ)。
    fn max_deviation(orig: &[Move], fitted: &[Move]) -> f64 {
        let f = sampled(fitted);
        let mut worst: f64 = 0.0;
        for p in sampled(orig).iter().step_by(7) {
            let d = f
                .windows(2)
                .map(|w| seg_dist(*p, w[0], w[1]))
                .fold(f64::INFINITY, f64::min);
            worst = worst.max(d);
        }
        worst
    }

    fn tp(moves: Vec<Move>) -> Toolpath {
        Toolpath {
            name: "t".into(),
            tool: jc_cam_tool::ToolLibrary::starter().tools[0].clone(),
            rpm: 10000.0,
            safe_z: 50.0,
            retract_z: 40.0,
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
            moves,
        }
    }

    #[test]
    fn points_on_a_circle_become_one_arc() {
        // 半径 5 の円の 3/4 周を 0.5° ごとの直線で
        let pts: Vec<P3> = (0..=540)
            .map(|k| {
                let a = (k as f64 * 0.5).to_radians();
                [5.0 * a.cos(), 5.0 * a.sin(), -1.0]
            })
            .collect();
        let mut orig = start(pts[0]);
        orig.extend(lines(&pts[1..], 600.0));
        let fitted = fit(&orig, &FitOptions::new(TOL));
        assert_eq!(fitted.len(), 2, "{fitted:?}");
        let Move::Arc { center, dir, .. } = fitted[1] else {
            panic!("円弧になる: {:?}", fitted[1]);
        };
        assert_eq!(dir, ArcDir::Ccw);
        assert!(center[0].hypot(center[1]) < 1e-6, "{center:?}");
        assert!(max_deviation(&orig, &fitted) <= TOL + 1e-6);
        assert!(
            check(&tp(fitted))
                .iter()
                .all(|p| p.message.contains("安全高さ"))
        );
    }

    #[test]
    fn a_helix_keeps_its_rise() {
        // 半径 3 で 1 周弱を下りながら(時計回り)
        let pts: Vec<P3> = (0..=300)
            .map(|k| {
                let a = -(k as f64).to_radians();
                [3.0 * a.cos(), 3.0 * a.sin(), -0.01 * k as f64]
            })
            .collect();
        let mut orig = start(pts[0]);
        orig.extend(lines(&pts[1..], 300.0));
        let fitted = fit(&orig, &FitOptions::new(TOL));
        assert!(fitted.len() <= 3, "{fitted:?}");
        assert!(fitted.iter().any(|m| matches!(
            m,
            Move::Arc {
                dir: ArcDir::Cw,
                ..
            }
        )));
        assert_eq!(fitted.last().unwrap().to(), pts[300]);
        assert!(max_deviation(&orig, &fitted) <= TOL + 1e-6);
    }

    #[test]
    fn corners_stay_sharp_and_small_wobbles_are_merged() {
        // 正方形の角は円弧にしない
        let sq = [
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let mut orig = start([0.0, 0.0, 0.0]);
        orig.extend(lines(&sq, 500.0));
        assert_eq!(fit(&orig, &FitOptions::new(TOL)), orig);
        // 許容差より小さい揺れのある直線は 1 本に
        let wobble: Vec<P3> = (1..=200)
            .map(|k| [k as f64 * 0.1, 0.002 * (k as f64).sin(), -0.5])
            .collect();
        let mut orig = start([0.0, 0.0, -0.5]);
        orig.extend(lines(&wobble, 500.0));
        let fitted = fit(&orig, &FitOptions::new(TOL));
        assert_eq!(fitted.len(), 2, "{fitted:?}");
        assert!(max_deviation(&orig, &fitted) <= TOL + 1e-6);
    }

    #[test]
    fn going_back_along_the_same_line_is_kept() {
        let pts = [[10.0, 0.0, 0.0], [5.0, 0.0, 0.0], [12.0, 0.0, 0.0]];
        let mut orig = start([0.0, 0.0, 0.0]);
        orig.extend(lines(&pts, 500.0));
        assert_eq!(fit(&orig, &FitOptions::new(TOL)).len(), 4);
    }

    #[test]
    fn rapids_arcs_and_feed_changes_are_not_crossed() {
        let mut orig = start([0.0, 0.0, 5.0]);
        orig.extend(lines(&[[0.0, 0.0, 0.0]], 100.0)); // 下ろす(送りが違う)
        orig.extend(lines(&[[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], 500.0));
        orig.push(Move::Arc {
            to: [4.0, 0.0, 0.0],
            center: [3.0, 0.0],
            dir: ArcDir::Cw,
            feed: 500.0,
        });
        orig.extend(lines(&[[5.0, 0.0, 0.0], [6.0, 0.0, 0.0]], 500.0));
        orig.push(Move::Rapid {
            to: [6.0, 0.0, 5.0],
        });
        let fitted = fit(&orig, &FitOptions::new(TOL));
        let kinds: Vec<&str> = fitted
            .iter()
            .map(|m| match m {
                Move::Rapid { .. } => "R",
                Move::Linear { .. } => "L",
                Move::Arc { .. } => "A",
            })
            .collect();
        assert_eq!(kinds, ["R", "L", "L", "A", "L", "R"]);
    }

    #[test]
    fn a_wavy_3d_path_stays_within_the_tolerance() {
        // 走査線のような、曲がった面をなぞる行(XZ 平面の曲線)と、斜めに曲がりながら
        // 下りる線(らせんに近いが一定ではない)
        let row: Vec<P3> = (0..=800)
            .map(|k| {
                let x = k as f64 * 0.05;
                [x, 3.0, 2.0 * (x * 0.3).sin() - 0.1 * x]
            })
            .collect();
        let swirl: Vec<P3> = (1..=600)
            .map(|k| {
                let a = k as f64 * 0.01;
                let r = 8.0 + a;
                [
                    40.0 + r * a.cos() - 8.0,
                    3.0 + r * a.sin(),
                    row[800][2] - 0.02 * k as f64,
                ]
            })
            .collect();
        let mut orig = start(row[0]);
        orig.extend(lines(&row[1..], 800.0));
        orig.extend(lines(&swirl, 800.0));
        for tol in [0.001, 0.005, 0.02] {
            let fitted = fit(&orig, &FitOptions::new(tol));
            let dev = max_deviation(&orig, &fitted);
            assert!(dev <= tol + 1e-6, "許容差 {tol}: 離れ {dev}");
            assert!(
                fitted.len() < orig.len() / 3,
                "{} → {}",
                orig.len(),
                fitted.len()
            );
            assert_eq!(fitted.last().unwrap().to(), orig.last().unwrap().to());
        }
        // 円弧を使わない時も許容差に収まる
        let no_arcs = FitOptions {
            arcs: false,
            ..FitOptions::new(TOL)
        };
        let fitted = fit(&orig, &no_arcs);
        assert!(fitted.iter().all(|m| !matches!(m, Move::Arc { .. })));
        assert!(max_deviation(&orig, &fitted) <= TOL + 1e-6);
    }
}
