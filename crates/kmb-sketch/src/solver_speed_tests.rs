//! 拘束を解く速さを測る道具。普段の検査では流さない(`#[ignore]`)。
//! `cargo test -p kmb-sketch --release speed_ -- --ignored --nocapture --test-threads 1`
//!
//! 線の多いスケッチ・大きなパターン・形が付いて来られない向きに引く 1 コマ・拘束を足す前の確かめと
//! 自動拘束を測り、解く手順の段ごと(式を組む・解く・自由度と余分な拘束を調べる)の時間と、行列の大きさを出す。

use super::*;
use crate::pattern::{
    AngularSpacing, CircularPattern, DistanceType, PatternDef, PatternDirection, RectangularPattern,
};
use crate::tools::Spot;
use std::time::Instant;

fn pos(s: &Sketch, p: EntityId) -> [f64; 2] {
    match s.entity(p) {
        Some(Entity::Point { x, y, .. }) => [*x, *y],
        _ => unreachable!("点"),
    }
}

/// `f` を `n` 回流し、いちばん速かった回の時間(ms)と最後の結果を返す。
fn best<T>(n: usize, mut f: impl FnMut() -> T) -> (f64, T) {
    let mut out = None;
    let mut ms = f64::INFINITY;
    for _ in 0..n {
        let t = Instant::now();
        let v = f();
        ms = ms.min(t.elapsed().as_secs_f64() * 1000.0);
        out = Some(v);
    }
    (ms, out.expect("1 回は流した"))
}

/// 横に並べた `k` 個の長方形(線 4k 本)。隣の長方形と左下と右下の点を共有する。
/// `dims` なら左端を固定し、幅の寸法を付ける。`heights` なら高さの寸法も付ける(完全拘束)。
/// 付けなければ、右上の点は縦にだけ動ける。返すのは寸法の値と、長方形ごとの右上の点。
pub(super) fn chain(
    k: usize,
    dims: bool,
    heights: bool,
) -> (Sketch, HashMap<ConstraintId, f64>, Vec<EntityId>) {
    let mut s = Sketch::default();
    let mut values = HashMap::new();
    let mut corner: Spot = (None, [0.0, 0.0]);
    let mut tops = vec![];
    for i in 0..k {
        let x = corner.1[0];
        // 少し崩した位置から解かせる(寸法と合わない)
        let (w, h) = (10.0 + (i % 3) as f64, 6.0 + (i % 2) as f64);
        let l = s
            .add_rectangle(corner, (None, [x + w + 0.3, 6.5]))
            .expect("長方形");
        let ends = |s: &Sketch, line: EntityId| match s.entity(line) {
            Some(Entity::Line { a, b, .. }) => (*a, *b),
            _ => unreachable!("足したばかりの線分"),
        };
        if dims {
            if i == 0 {
                s.add_constraint(ConstraintKind::Fix {
                    point: ends(&s, l[0]).0,
                });
            }
            let cw = s.add_constraint(ConstraintKind::Length {
                line: l[0],
                value: format!("{w} mm"),
            });
            values.insert(cw, w);
            if heights {
                let ch = s.add_constraint(ConstraintKind::Length {
                    line: l[1],
                    value: format!("{h} mm"),
                });
                values.insert(ch, h);
            }
        }
        let (_, right) = ends(&s, l[0]);
        tops.push(ends(&s, l[1]).1);
        let at = pos(&s, right);
        corner = (Some(right), at);
    }
    (s, values, tops)
}

pub(super) fn lines_of(s: &Sketch) -> Vec<EntityId> {
    s.entities
        .iter()
        .filter(|e| matches!(e, Entity::Line { .. }))
        .map(|e| e.id())
        .collect()
}

/// 幅と高さを決めた長方形 1 つ(左下を固定)を `count` 個、原点から 100 mm の所を中心に一周に並べる。
pub(super) fn circular(count: usize) -> (Sketch, HashMap<ConstraintId, f64>) {
    let (mut s, values, _) = chain(1, true, true);
    let lines = lines_of(&s);
    s.add_circular_pattern(CircularPattern {
        entities: lines,
        center: PatternCenter::At([-100.0, 0.0]),
        count,
        spacing: AngularSpacing::Total,
        angle: std::f64::consts::TAU,
        suppressed: vec![],
    })
    .expect("パターン");
    (s, values)
}

/// 幅(`heights` なら高さも)を決めた長方形 1 つ(左下を固定)を `n` × `n` に並べたパターン。
pub(super) fn pattern(n: usize, heights: bool) -> (Sketch, HashMap<ConstraintId, f64>) {
    let (mut s, values, _) = chain(1, true, heights);
    let lines = lines_of(&s);
    let dir = |angle: f64| PatternDirection {
        axis: PatternAxis::Angle(angle),
        count: n,
        distance: 20.0,
        distance_type: DistanceType::Spacing,
        symmetric: false,
    };
    s.add_rectangular_pattern(RectangularPattern {
        entities: lines,
        directions: [dir(0.0), dir(std::f64::consts::FRAC_PI_2)],
        suppressed: vec![],
    })
    .expect("パターン");
    let _ = PatternDef::Rectangular;
    (s, values)
}

/// 解く段ごとの時間を出す。
fn report(name: &str, s: &Sketch, values: &HashMap<ConstraintId, f64>) {
    let lines = s
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Line { .. }))
        .count();
    let reps = 3;
    let (t_build, sys) = best(reps, || build(s, values).expect("式を組む"));
    let (t_values, (x, it)) = best(reps, || solve_values(&sys));
    let (t_finish, sol) = best(reps, || finish(s, &sys, x.clone(), it));
    let j = sparse_jacobian(&sys.equations, &x);
    let (t_jac, _) = best(reps, || sparse_jacobian(&sys.equations, &x));
    let (t_factor, _) = best(reps, || Factor::new(&j, x.len(), None));
    let (t_fixed, _) = best(reps, || Factor::new(&j, x.len(), None).determined());
    let (t_all, _) = best(reps, || solve(s, values).expect("解く"));
    eprintln!(
        "{name}: 線 {lines}・変数 {}・式 {}・反復 {it} | 全体 {t_all:.1} ms = 式を組む {t_build:.1} + 解く {t_values:.1} + 自由度と余分 {t_finish:.1} \
         | 内訳: ヤコビ行列 {t_jac:.1}・疎な分解 {t_factor:.1}・分解と決まった変数 {t_fixed:.1} | {:?} 自由度 {}",
        sys.x0.len(),
        sys.equations.len(),
        status_name(&sol.status),
        sol.dof,
    );
}

fn status_name(s: &SolveStatus) -> String {
    match s {
        SolveStatus::WellConstrained => "完全拘束".into(),
        SolveStatus::UnderConstrained { dof } => format!("拘束不足 {dof}"),
        SolveStatus::OverConstrained { redundant } => format!("余分 {}", redundant.len()),
        SolveStatus::Inconsistent { conflicting } => format!("矛盾 {}", conflicting.len()),
    }
}

/// 点 `p` を (dx, dy) 引いた 1 コマ(押した直後の 1 コマ目)を測る。範囲を広げて試す回数と、
/// 全部を動かせるものとして 1 回解く時間も出す。
fn report_drag(
    name: &str,
    s: &Sketch,
    values: &HashMap<ConstraintId, f64>,
    p: EntityId,
    d: [f64; 2],
    reps: usize,
) {
    let mut start = s.clone();
    start.adopt_solution(&solve(s, values).expect("解く"));
    let at = pos(&start, p);
    let mut moved = start.clone();
    moved.set_point(p, at[0] + d[0], at[1] + d[1]);
    let held = BTreeSet::from([p]);
    let (t_frame, sol) = best(reps, || {
        solve_dragging(&start, &moved, values, &held, None).expect("引く")
    });
    let got = sol.points[&p];
    let follows = (got[0] - at[0] - d[0]).abs() < 1e-3 && (got[1] - at[1] - d[1]).abs() < 1e-3;
    // 範囲の数と、全部を動かせる時の 1 回
    let sys = build(&start, values).expect("式を組む");
    let target = build(&moved, values).expect("式を組む").x0;
    let dist = drag_tiers(&start, &sys, &held);
    let mut limits: Vec<Hops> = dist.iter().flatten().copied().collect();
    limits.sort_unstable();
    limits.dedup();
    let tiers = limits.len().min(MAX_TIERS);
    let mut weight = vec![1.0; sys.x0.len()];
    for &v in sys.entity_vars.get(&p).into_iter().flatten() {
        weight[v] = HELD_WEIGHT;
    }
    for (w, d) in weight.iter_mut().zip(&dist) {
        if d.is_none() {
            *w = 0.0;
        }
    }
    let (t_full, (x, k)) = best(reps, || solve_moving(&sys, &sys.x0, &target, &weight));
    let (t_finish, _) = best(reps, || finish(&start, &sys, x.clone(), k));
    let n = sys.x0.len();
    eprintln!(
        "{name}: 変数 {n}・式 {} | 1 コマ {t_frame:.1} ms・付いて来た {follows} | 試す範囲 {tiers}(上限 {MAX_TIERS})+ 全部 1 回 \
         | 全部を動かす 1 回 {t_full:.1} ms(反復 {k})・自由度と余分 {t_finish:.1} ms",
        sys.equations.len(),
    );
}

#[test]
#[ignore = "速さを測る道具(--release で手で流す)"]
fn speed_many_lines() {
    for k in [4, 13, 25, 50] {
        let (s, v, _) = chain(k, true, true);
        report(&format!("寸法つきの長方形 {k} 個"), &s, &v);
    }
    for k in [13, 25] {
        let (s, v, _) = chain(k, false, false);
        report(&format!("寸法の無い長方形 {k} 個"), &s, &v);
    }
}

#[test]
#[ignore = "速さを測る道具(--release で手で流す)"]
fn speed_large_pattern() {
    for n in [3, 5, 8, 10] {
        let (s, v) = pattern(n, true);
        report(&format!("長方形のパターン {n}×{n}"), &s, &v);
    }
    // 元の幅を間隔(20 mm)より大きく変える: 写しが重なっても解けるか(「矛盾」と出ないか)
    let (s, mut v) = pattern(10, true);
    for (_, x) in v.iter_mut().filter(|(_, x)| **x == 10.0) {
        *x = 25.0;
    }
    report("長方形のパターン 10×10 の幅を 25 mm に", &s, &v);
    for count in [12, 36, 72] {
        let (s, v) = circular(count);
        report(&format!("長方形の円形状パターン {count} 個"), &s, &v);
    }
}

#[test]
#[ignore = "速さを測る道具(--release で手で流す)"]
fn speed_drag_frame() {
    // 画面は、決まりきった点(完全拘束)を引かせない(kmb-ui-sketch の drag.rs の STUCK)。
    // 測るのは、縦にだけ動ける右上の点を斜めに引く(横には付いて来られない)1 コマと、
    // 付いて来られる向き(真上)に引く 1 コマ
    for k in [4, 13, 25, 50] {
        let (s, v, tops) = chain(k, true, false);
        let reps = if k > 25 { 1 } else { 3 };
        let name = format!("高さの自由な長方形 {k} 個の右上を斜めに引く");
        report_drag(&name, &s, &v, tops[k / 2], [3.0, 2.0], reps);
        let name = format!("高さの自由な長方形 {k} 個の右上を真上に引く");
        report_drag(&name, &s, &v, tops[k / 2], [0.0, 2.0], reps);
    }
    for n in [5, 8, 10] {
        let (s, v) = pattern(n, false);
        let corner = s
            .entities
            .iter()
            .find_map(|e| match e {
                Entity::Point { id, x, y }
                    if (*x - 10.3).abs() < 1e-9 && (*y - 6.5).abs() < 1e-9 =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .expect("元の右上");
        let reps = if n > 5 { 1 } else { 3 };
        let name = format!("高さの自由な長方形のパターン {n}×{n} の元の右上を斜めに引く");
        report_drag(&name, &s, &v, corner, [3.0, 2.0], reps);
        let name = format!("高さの自由な長方形のパターン {n}×{n} の元の右上を真上に引く");
        report_drag(&name, &s, &v, corner, [0.0, 2.0], reps);
    }
}

/// 拘束を足す前の確かめ(既に決まっているか。決めている拘束も探す)と、自動拘束の 1 回を測る。
fn report_checks(name: &str, s: &Sketch, values: &HashMap<ConstraintId, f64>, reps: usize) {
    let sol = solve(s, values).expect("解く");
    let line = lines_of(s)[0];
    let (t_by, by) = best(reps, || {
        redundant_with(s, values, &ConstraintKind::Horizontal { line })
    });
    let opts = crate::auto_constrain::AutoOptions::default();
    let (t_auto, auto) = best(reps, || {
        crate::auto_constrain::auto_constrain(s, &sol, values, &opts).expect("自動拘束")
    });
    eprintln!(
        "{name}: 変数 {} | 足す前の確かめ {t_by:.1} ms(決めている拘束 {:?} 個)| 自動拘束 {t_auto:.1} ms(候補のうち残した物 {} 個)",
        build(s, values).expect("式を組む").x0.len(),
        by.map(|b| b.len()),
        auto.suggestions.len(),
    );
}

#[test]
#[ignore = "速さを測る道具(--release で手で流す)"]
fn speed_checks() {
    for k in [13, 25, 50] {
        let reps = if k > 25 { 1 } else { 3 };
        let (s, v, _) = chain(k, true, true);
        report_checks(&format!("寸法つきの長方形 {k} 個"), &s, &v, reps);
        let (s, v, _) = chain(k, false, false);
        report_checks(&format!("寸法の無い長方形 {k} 個"), &s, &v, reps);
    }
    for n in [5, 8] {
        let (s, v) = pattern(n, true);
        report_checks(&format!("長方形のパターン {n}×{n}"), &s, &v, 1);
    }
}
