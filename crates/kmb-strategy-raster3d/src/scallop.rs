//! 3D のスキャロップ(`cam.scallop3d@1`)。加工の範囲の縁から、部品の面に沿って一定の間隔で
//! 内へずらしていく輪で仕上げる。急な所でも面の上の間隔が送り幅に保たれる(走査線や
//! モーフィング スパイラルは上から見た間隔なので、急な所で開く)。
//!
//! 輪の作り方:
//!
//! - 範囲の格子の節点ごとに工具を下ろし、工具の先端の高さ z を求める
//! - 範囲の外から、面に沿った距離(xy と z の 3D の距離)を 16 近傍で広げる(Dijkstra)
//! - その距離の等高線を、送り幅以下の同じ間隔で取る。距離の山(範囲の真ん中の稜線や、面の
//!   頂上)の近くで輪から送り幅の半分より離れる所が残れば、そこを囲む輪を足す
//! - 輪を外から内へ並べ、点ごとに工具を下ろしてなぞる
//!
//! 欄と範囲の決め方はモーフィング スパイラルと同じ。工具の負荷を見て線を変えることはしない。

use crate::cut_passes;
use crate::morph::{
    Morph, area, densify, dist, fields, follow, heights, inside_ring, nearest, rotate, signed_area,
};
use kmb_cam_dropcutter::region::level_loops;
use kmb_cam_dropcutter::{Cutter, Grid};
use kmb_cam_setup::{OpBase, Planner, StrategyInput, StrategyOutput, invalid, plan_error, prepare};
use kmb_cam_tool::ToolKind;
use kmb_core::{EvalCtx, EvalError, Feature, InputRef, Outputs, Registry};
use kmb_toolpath::P3;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub const SCALLOP3D: &str = "cam.scallop3d@1";

type P2 = [f64; 2];

/// 欄はモーフィング スパイラルと同じ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scallop {
    #[serde(flatten)]
    pub spec: Morph,
}

impl Scallop {
    pub fn new(base: OpBase) -> Scallop {
        Scallop {
            spec: Morph::new(base),
        }
    }
}

/// 面に沿った距離で数える傾きの上限(tan 72°)。これより急な所(縦の壁など)では、上から見た
/// 輪の間隔が格子 2 つ(送り幅の 1/3)より狭くならない。下ろした工具は縦の壁を xy の 1 点で
/// 上下するだけなので、壁の上に輪を何本並べても削れない
const STEEP: f64 = 3.0;

/// 3D の距離を広げる近傍(2 つ先まで、向きの重ならない 16 方向)。
const NEAR: [(isize, isize); 16] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
    (2, 1),
    (2, -1),
    (-2, 1),
    (-2, -1),
    (1, 2),
    (1, -2),
    (-1, 2),
    (-1, -2),
];

/// 印 `from` の節点(距離 0)から、範囲 `inside` の中へ、xy と高さ `z` の 3D の距離を広げる。
/// 縦の壁で距離が跳ねて輪が 1 か所に重ならないよう、高さの差は横の [`STEEP`] 倍までに抑える。
fn spread(grid: &Grid, inside: &[bool], z: &[f64], from: &[bool]) -> Vec<f64> {
    let (nx, ny) = (grid.nx as isize, grid.ny as isize);
    let mut d = vec![f64::INFINITY; inside.len()];
    let mut heap = BinaryHeap::new();
    for (k, &f) in from.iter().enumerate() {
        if f {
            d[k] = 0.0;
            heap.push((Reverse(Key(0.0)), k));
        }
    }
    while let Some((Reverse(Key(dk)), k)) = heap.pop() {
        if dk > d[k] {
            continue;
        }
        let (i, j) = ((k as isize) % nx, (k as isize) / nx);
        for (di, dj) in NEAR {
            let (a, b) = (i + di, j + dj);
            if a < 0 || b < 0 || a >= nx || b >= ny {
                continue;
            }
            let m = (b * nx + a) as usize;
            if !inside[m] {
                continue;
            }
            let run = ((di * di + dj * dj) as f64).sqrt() * grid.cell;
            let dz = (z[m] - z[k]).clamp(-STEEP * run, STEEP * run);
            let v = dk + run.hypot(dz);
            if v < d[m] {
                d[m] = v;
                heap.push((Reverse(Key(v)), m));
            }
        }
    }
    d
}

/// 距離を山の順に並べるための、比べられる数。
#[derive(PartialEq)]
struct Key(f64);

impl Eq for Key {}

impl PartialOrd for Key {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Key {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0)
    }
}

/// 輪の上の点にいちばん近い節点の印。
fn on_rings(grid: &Grid, rings: &[Vec<P2>]) -> Vec<bool> {
    let mut on = vec![false; grid.nx * grid.ny];
    for q in rings.iter().flat_map(|r| densify(r, 0.5 * grid.cell)) {
        let i = ((q[0] - grid.origin[0]) / grid.cell).round();
        let j = ((q[1] - grid.origin[1]) / grid.cell).round();
        if i >= 0.0 && j >= 0.0 && (i as usize) < grid.nx && (j as usize) < grid.ny {
            on[j as usize * grid.nx + i as usize] = true;
        }
    }
    on
}

/// 範囲 `inside`(`grid` の節点の印)を、節点の高さ `z` の面に沿って間隔 `step` 以下でずらした
/// 閉じた輪で埋める。外から内への順で、上から見て反時計回り(`clockwise` なら時計回り)。
pub fn scallop_xy(
    grid: &Grid,
    inside: &[bool],
    z: &[f64],
    step: f64,
    clockwise: bool,
) -> Vec<Vec<P2>> {
    let cell = grid.cell;
    let outside: Vec<bool> = inside.iter().map(|&m| !m).collect();
    let d = spread(grid, inside, z, &outside);
    let at = |k: usize| if inside[k] { d[k] } else { 0.0 };
    let field: Vec<f64> = (0..d.len()).map(at).collect();
    let dmax = field.iter().copied().fold(0.0, f64::max);
    // いちばん外の輪は境目のすぐ内側(工具の中心が範囲の縁を回る)
    let c0 = 0.5 * cell;
    if dmax <= c0 + cell {
        return Vec::new();
    }
    // 同じ間隔 s(送り幅以下)で、いちばん高い所が最後の輪から s/2 以内になる数
    let n = ((dmax - c0) / step + 0.5).ceil().max(1.0) as usize;
    let s = (dmax - c0) / (n as f64 - 0.5);
    let mut rings: Vec<(f64, Vec<P2>)> = (0..n)
        .map(|k| c0 + k as f64 * s)
        .flat_map(|l| {
            level_loops(grid, &field, l, 0.0)
                .into_iter()
                .map(move |r| (l, r))
        })
        .collect();
    // 距離の山の近くで輪から s/2 より離れた所を、その山を囲む輪で埋める
    let (nx, ny) = (grid.nx, grid.ny);
    for _ in 0..4 {
        let all: Vec<Vec<P2>> = rings.iter().map(|r| r.1.clone()).collect();
        let near = spread(grid, inside, z, &on_rings(grid, &all));
        let far: Vec<bool> = (0..d.len())
            .map(|k| inside[k] && near[k] > 0.5 * s + cell)
            .collect();
        if !far.iter().any(|&f| f) {
            break;
        }
        let mut seen = vec![false; far.len()];
        for start in 0..far.len() {
            if !far[start] || seen[start] {
                continue;
            }
            // 離れた所のつながり(4 近傍)
            let mut part = vec![start];
            seen[start] = true;
            let mut k = 0;
            while k < part.len() {
                let (i, j) = (part[k] % nx, part[k] / nx);
                k += 1;
                let mut look = |a: usize, b: usize| {
                    let m = b * nx + a;
                    if far[m] && !seen[m] {
                        seen[m] = true;
                        part.push(m);
                    }
                };
                if i > 0 {
                    look(i - 1, j);
                }
                if i + 1 < nx {
                    look(i + 1, j);
                }
                if j > 0 {
                    look(i, j - 1);
                }
                if j + 1 < ny {
                    look(i, j + 1);
                }
            }
            let top = part
                .iter()
                .copied()
                .max_by(|&a, &b| field[a].total_cmp(&field[b]))
                .expect("つながりは空でない");
            let l = field[top] - 0.5 * s;
            if l <= c0 {
                continue;
            }
            // その山の、l より高い所のつながりと、そのすぐ外の節点だけの場(ほかの所の輪を
            // 作り直さない。四角く切ると、切り口が輪になる)
            let mut local = vec![0.0; field.len()];
            let mut up = vec![top];
            local[top] = field[top];
            let mut k = 0;
            while k < up.len() {
                let (i, j) = (up[k] % nx, up[k] / nx);
                k += 1;
                for (a, b) in [
                    (i.wrapping_sub(1), j),
                    (i + 1, j),
                    (i, j.wrapping_sub(1)),
                    (i, j + 1),
                ] {
                    if a >= nx || b >= ny {
                        continue;
                    }
                    let m = b * nx + a;
                    if local[m] == 0.0 && field[m] > 0.0 {
                        local[m] = field[m];
                        if field[m] >= l {
                            up.push(m);
                        }
                    }
                }
            }
            rings.extend(
                level_loops(grid, &local, l, 0.0)
                    .into_iter()
                    .map(|r| (l, r)),
            );
        }
    }
    // 回る向きをそろえ、外の輪から順に、前の輪の終わりにいちばん近い輪・点からたどる
    for (_, r) in &mut rings {
        if (signed_area(r) > 0.0) == clockwise {
            r.reverse();
        }
    }
    // 低い段の輪の中にある輪は、その輪の後に削る(外から内へ)。削れる輪のうち、今の所に
    // いちばん近い輪から、その輪のいちばん近い点で始める(段ごとに範囲の端から端へ跳ばない)
    let n = rings.len();
    let parents: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            (0..n)
                .filter(|&j| rings[j].0 < rings[i].0 && inside_ring(&rings[j].1, rings[i].1[0]))
                .collect()
        })
        .collect();
    let mut done = vec![false; n];
    let mut out: Vec<Vec<P2>> = Vec::with_capacity(n);
    let mut here: Option<P2> = None;
    for _ in 0..n {
        let ready = (0..n).filter(|&i| !done[i] && parents[i].iter().all(|&j| done[j]));
        let pick = match here {
            // 始めはいちばん低い段の輪
            None => ready.min_by(|&a, &b| rings[a].0.total_cmp(&rings[b].0)),
            Some(h) => ready.min_by(|&a, &b| {
                let da = dist(rings[a].1[nearest(&rings[a].1, h)], h);
                let db = dist(rings[b].1[nearest(&rings[b].1, h)], h);
                da.total_cmp(&db)
            }),
        };
        let Some(k) = pick else {
            break;
        };
        done[k] = true;
        let r = &rings[k].1;
        let mut r = match here {
            Some(h) => rotate(r, nearest(r, h)),
            None => r.clone(),
        };
        r.push(r[0]);
        here = r.last().copied();
        out.push(r);
    }
    out
}

/// 工具の先端が通る点の続き(作業座標系)。
pub fn scallop_passes(p: &Planner, o: &crate::MorphOpts) -> Result<Vec<Vec<P3>>, EvalError> {
    let cutter = Cutter::new(p.tool.clone(), o.limits.leave_radial);
    let (grid, inside, lo, hi) = area(p, o)?;
    let h = heights(p, o, &cutter, lo, hi);
    let rows: Vec<usize> = (0..grid.ny).collect();
    let z: Vec<f64> = kmb_cam_dropcutter::par_map(&rows, |&j| {
        (0..grid.nx)
            .map(|i| h.exact_at(&cutter, grid.at(i, j)).max(o.floor))
            .collect::<Vec<_>>()
    })
    .concat();
    let mut lines = scallop_xy(&grid, &inside, &z, o.step, o.clockwise);
    // 工具の先端の高さが段になって落ちる所(部品の縦の壁から工具の半径だけ外など)の升目を通る
    // 輪の点は、段のどちら側かが点ごとに入れ替わり、壁を上下に何度も往復する。その点を段の上の
    // 側の際へ寄せる(輪がまっすぐなまま、段の上を通る)
    let cliff = 2.0 * STEEP * grid.cell;
    let lift = |q: P2| -> P2 {
        let (fi, fj) = (
            (q[0] - grid.origin[0]) / grid.cell,
            (q[1] - grid.origin[1]) / grid.cell,
        );
        if fi < 0.0 || fj < 0.0 {
            return q;
        }
        let (i, j) = (fi as usize, fj as usize);
        if i + 1 >= grid.nx || j + 1 >= grid.ny {
            return q;
        }
        let c = |a: usize, b: usize| z[b * grid.nx + a];
        let (z00, z10, z01, z11) = (c(i, j), c(i + 1, j), c(i, j + 1), c(i + 1, j + 1));
        let (lo, hi) = (
            z00.min(z10).min(z01).min(z11),
            z00.max(z10).max(z01).max(z11),
        );
        if hi - lo < cliff {
            return q;
        }
        let mid = 0.5 * (lo + hi);
        let at = |p: P2| h.exact_at(&cutter, p).max(o.floor);
        if at(q) >= mid {
            return q;
        }
        // 升目の中の高い向き
        let (gx, gy) = (z10 + z11 - z00 - z01, z01 + z11 - z00 - z10);
        let len = gx.hypot(gy);
        if len < 1e-12 {
            return q;
        }
        let (ux, uy) = (gx / len * 1.5 * grid.cell, gy / len * 1.5 * grid.cell);
        let far = [q[0] + ux, q[1] + uy];
        if at(far) < mid {
            return q;
        }
        let (mut a, mut b) = (0.0, 1.0);
        for _ in 0..14 {
            let t = 0.5 * (a + b);
            if at([q[0] + ux * t, q[1] + uy * t]) >= mid {
                b = t;
            } else {
                a = t;
            }
        }
        [q[0] + ux * b, q[1] + uy * b]
    };
    for l in &mut lines {
        let moved: Vec<P2> = densify(l, 0.5 * o.cell).into_iter().map(lift).collect();
        *l = moved;
    }
    if lines.is_empty() {
        return Err(invalid(
            "削る所がありません(加工の範囲が送り幅より狭いです)",
        ));
    }
    if o.outward {
        lines.reverse();
    }
    // 次の輪の始まりが近ければ(送り幅の 2 倍まで)、面をなぞって移り、1 本の線にする(輪ごとに
    // 上げ下げしない)
    let mut joined: Vec<Vec<P2>> = Vec::new();
    for l in lines {
        match joined.last_mut() {
            Some(last) if dist(*last.last().expect("輪は空でない"), l[0]) <= 2.0 * o.step => {
                last.extend(l);
            }
            _ => joined.push(l),
        }
    }
    follow(p, o, &cutter, &h, &joined)
}

/// 戦略の純関数: 解いた入力(`params` は [`crate::MorphOpts`])だけから経路を作る。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    let p = Planner::new(input);
    let o: crate::MorphOpts = input.params()?;
    let passes = scallop_passes(&p, &o).map_err(plan_error)?;
    let mut tp = p.toolpath();
    cut_passes(&p, &mut tp, passes);
    Ok(p.output(tp))
}

impl Feature for Scallop {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        self.spec.inputs()
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.spec.base)?;
        if p.tool.kind == ToolKind::Drill {
            return Err(invalid(
                "スキャロップの加工にはエンドミル(平かボール)を使ってください",
            ));
        }
        let o = self.spec.opts(ctx, &mut p)?;
        let input = p.input(ctx.feature_id(), o, Default::default());
        let out = plan(&input);
        p.finish_plan(self.spec.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        // 欄は同じなので、書き出す JSON も同じ
        self.spec.rename_param(old, new)
    }
}

pub fn register(reg: &mut Registry) {
    let fields = fields("面に沿った輪の間隔の上限。空なら工具の送り幅");
    reg.register_described(
        kmb_cam_setup::operation(
            SCALLOP3D,
            "スキャロップ(3D)",
            "加工の範囲の縁から、面に沿って一定の間隔で内へずらした輪で、急な所も同じ削り目で仕上げる",
            fields,
        )
        .icon("scallop3d"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Scallop>(d)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 60 × 40 の範囲(縁 4 mm を除く)の、半径 15 の半球(中心 30, 20)と平らな所。
    fn dome(cell: f64) -> (Grid, Vec<bool>, Vec<f64>, impl Fn(f64, f64) -> f64) {
        let grid = Grid {
            origin: [0.0, 0.0],
            cell,
            nx: (60.0 / cell) as usize + 1,
            ny: (40.0 / cell) as usize + 1,
        };
        let h = |x: f64, y: f64| {
            (225.0 - (x - 30.0).powi(2) - (y - 20.0).powi(2))
                .max(0.0)
                .sqrt()
        };
        let mut inside = Vec::new();
        let mut z = Vec::new();
        for j in 0..grid.ny {
            for i in 0..grid.nx {
                let q = grid.at(i, j);
                inside.push(q[0] > 4.0 && q[0] < 56.0 && q[1] > 4.0 && q[1] < 36.0);
                z.push(h(q[0], q[1]));
            }
        }
        (grid, inside, z, h)
    }

    #[test]
    fn rings_keep_the_step_along_the_surface_even_where_it_is_steep() {
        let (grid, inside, z, h) = dome(0.25);
        let step = 1.5;
        let rings = scallop_xy(&grid, &inside, &z, step, false);
        let lift = |q: &P2| [q[0], q[1], h(q[0], q[1])];
        let d3 = |a: [f64; 3], b: [f64; 3]| {
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        };
        // 内の輪の点から、1 つ外の輪までの面の上の距離(3D の弦)。半球の急な所(傾き 45° より
        // 急、中心から 10.6〜15 mm)の点だけを見る
        let mut gaps = Vec::new();
        for w in rings.windows(2) {
            let outer: Vec<[f64; 3]> = densify(&w[0], 0.1).iter().map(lift).collect();
            for q in densify(&w[1], 0.5) {
                let r = (q[0] - 30.0).hypot(q[1] - 20.0);
                if !(10.6..14.8).contains(&r) {
                    continue;
                }
                let p = lift(&q);
                gaps.push(
                    outer
                        .iter()
                        .map(|&o| d3(o, p))
                        .fold(f64::INFINITY, f64::min),
                );
            }
        }
        gaps.sort_by(f64::total_cmp);
        let at = |f: f64| gaps[((gaps.len() - 1) as f64 * f) as usize];
        println!(
            "急な所の面の上の間隔 10% {:.2}・中央 {:.2}・90% {:.2}({} 点)",
            at(0.1),
            at(0.5),
            at(0.9),
            gaps.len()
        );
        assert!(gaps.len() > 100);
        assert!(at(0.9) <= step * 1.1 && at(0.1) >= step * 0.7, "{gaps:?}");
    }

    #[test]
    fn every_point_is_within_half_a_step_of_a_ring() {
        let (grid, inside, z, h) = dome(0.25);
        let step = 1.5;
        let rings = scallop_xy(&grid, &inside, &z, step, false);
        let pts: Vec<[f64; 3]> = rings
            .iter()
            .flat_map(|r| densify(r, 0.1))
            .map(|q| [q[0], q[1], h(q[0], q[1])])
            .collect();
        // 4 つおきの節点で、輪の点までの 3D の距離のいちばん近い物
        let mut worst = 0.0f64;
        for j in (0..grid.ny).step_by(4) {
            for i in (0..grid.nx).step_by(4) {
                let k = j * grid.nx + i;
                if !inside[k] {
                    continue;
                }
                let q = grid.at(i, j);
                let near = pts
                    .iter()
                    .map(|p| {
                        let (dx, dy, dz) = (p[0] - q[0], p[1] - q[1], p[2] - z[k]);
                        dx * dx + dy * dy + dz * dz
                    })
                    .fold(f64::INFINITY, f64::min)
                    .sqrt();
                worst = worst.max(near);
            }
        }
        println!("輪からいちばん離れた所 {worst:.2} mm");
        assert!(worst <= 0.5 * step + 0.3, "{worst}");
    }

    #[test]
    fn rings_go_from_the_edge_inward_and_turn_the_chosen_way() {
        let (grid, inside, z, _) = dome(0.25);
        let ccw = scallop_xy(&grid, &inside, &z, 1.5, false);
        let cw = scallop_xy(&grid, &inside, &z, 1.5, true);
        assert!(signed_area(&ccw[0]) > 0.0 && signed_area(&cw[0]) < 0.0);
        // 最初の輪は範囲の縁(4 mm の所)を回る
        let xmin = ccw[0].iter().map(|q| q[0]).fold(f64::INFINITY, f64::min);
        assert!((xmin - 4.0).abs() < 0.3, "{xmin}");
        // 輪は閉じている
        assert!(ccw.iter().all(|r| r.first() == r.last()));
    }
}
