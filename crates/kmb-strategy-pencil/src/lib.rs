//! 3D のペンシル加工(`cam.pencil3d@1`)。工具が部品の 2 つの面に同時に当たる所(内側の角、
//! 谷、半径が工具より小さいフィレット)を 1 本ずつなぞり、走査線や等高線が残した角の
//! 削り残しを取る。
//!
//! 方式: 部品の高さの格子の全節点で工具を下ろした高さ(工具の先端が通れる面、`DropField`)を
//! 求める。この面は、工具が 2 点で当たる所で谷のように折れる(折れの角度は、2 つの当たる面の
//! 法線のなす角と同じ)。格子の 4 つの向き(X・Y・2 本の斜め)ごとに、節点の両側 `w` 節点先
//! までの上りの角度の和を「折れ」とし、それが `angle` を超える帯を谷とみなす。帯の中では、
//! 近い所(2 節点先)で測った折れが向きに沿っていちばん大きい節点を残す。残した節点を
//! 隣どうしでつないで線にし(並んだ節点は 1 本にまとめる)、少しならしてから、その位置で
//! 工具を下ろし直した高さをなぞる。
//!
//! `rest_tool`(前の工具)を指定すると、前の工具が届いた面の高さ(前の工具の形を全節点に
//! 置いた下側の包み)を求め、今の工具がそれより `REST_MIN` 以上深く削る節点だけを残す。
//! 前の工具は部品の全体を削ったとみなす(前の加工が範囲を限っていても考えない)。
//!
//! 線の順番は今いる所から近い順。次の線が近く、まっすぐ動いても部品を削らなければ、工具を
//! 上げずに移る。
//!
//! 本数(`passes`)を増やすと、谷の線の両側に `pitch` ずつ離した線を片側 `passes` 本ずつ足し、
//! 外側の線から谷の線へ順に(谷ごとにまとめて)なぞる。離した線も、その位置で工具を下ろし直した
//! 高さをなぞる。
//!
//! 3D に共通の欄([`Limits`]): 径方向に残す厚さは工具を太らせて下ろす。加工の範囲の外の点は
//! なぞらない(線をそこで切る)。

use kmb_cam_dropcutter::limits::{boundary_fields, leave_radial_field};
use kmb_cam_dropcutter::{Cutter, DropField, Heights, Limits, Region, par_map};
use kmb_cam_setup::{
    OpBase, Planner, Prepared, StrategyInput, StrategyOutput, approach, expr_refs, invalid, length,
    plan_error, prepare,
};
use kmb_cam_tool::{Tool, ToolKind, ToolLibrary};
use kmb_core::{Dim, EvalCtx, EvalError, Feature, Field, InputRef, Outputs, Registry};
use kmb_toolpath::{Move, P3};
use serde::{Deserialize, Serialize};

pub const PENCIL: &str = "cam.pencil3d@1";

/// 谷とみなす折れの角度の既定(度)。2 つの当たる面の法線が 20° 以上開いていれば谷。
pub const DEFAULT_ANGLE_DEG: f64 = 20.0;

/// 残り削りで「前の工具が残した」とみなす厚さの下限(mm)。
pub const REST_MIN: f64 = 0.01;

/// 工具を下げたまま次の線へ移る、つなぎの長さの上限(工具の直径の倍数)。
const STAY_DOWN: f64 = 2.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pencil {
    #[serde(flatten)]
    pub base: OpBase,
    /// 部品の面に残す厚さ(上向きに測る)
    #[serde(default = "zero")]
    pub leave: String,
    /// 部品の高さを調べる格子の幅
    #[serde(default = "resolution")]
    pub resolution: String,
    /// 谷とみなす折れの角度の下限(例: `20 deg`)。空なら 20 deg
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// 残り削り: 前の工具の番号。前の工具が残した所だけをなぞる
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_tool: Option<u32>,
    /// 部品のいちばん高い所から、どこまで下をなぞるか(省くと部品の底まで)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
    /// 谷の線の片側に足す線の本数(0 なら谷の線だけ)
    #[serde(default, skip_serializing_if = "is_zero")]
    pub passes: u32,
    /// 足す線の間隔。空なら工具の送り幅
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<String>,
    /// 径方向に残す厚さ・加工の範囲
    #[serde(flatten)]
    pub limits: Limits,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn zero() -> String {
    "0 mm".into()
}

fn resolution() -> String {
    "0.05 mm".into()
}

/// 谷の線を探す時の設定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// 折れの角度の下限(ラジアン)
    pub angle: f64,
    /// 面に残す厚さ
    pub leave: f64,
    /// これより低い所(工具の先端の高さ)はなぞらない
    pub z_min: f64,
    /// これより高い所(ストックの上面)はなぞらない
    pub z_max: f64,
    /// 径方向に残す厚さ(工具を太らせる量)
    pub grow: f64,
    /// 谷の線の片側に足す線の本数と間隔
    pub passes: u32,
    pub pitch: f64,
}

/// なぞる線(工具の先端が通る点の列)。`closed` なら最後の点から最初の点へ戻る輪。
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub points: Vec<P3>,
    pub closed: bool,
}

/// 格子の 4 つの向き(節点の番号の差)。
const DIRS: [(isize, isize); 4] = [(1, 0), (0, 1), (1, 1), (1, -1)];

/// 8 つの隣(節点の番号の差)。
const NEIGHBORS: [(isize, isize); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// 谷の節点を探して線にする。`rest` は前の工具。
pub fn pencil_chains(
    heights: Heights,
    tool: &Tool,
    rest: Option<&Tool>,
    o: &Options,
) -> Vec<Chain> {
    pencil_groups(heights, tool, rest, o, None)
        .into_iter()
        .flatten()
        .collect()
}

// 計測する呼び出しだけが印を受ける。通常の計算では何もしない関数に展開する。
#[derive(Debug, Clone, Copy)]
enum Phase {
    Heights,
    CurrentEnvelope,
    RestEnvelope,
    ValleyNodes,
    LeftBy,
    Trace,
    Join,
}

/// 谷ごとの、なぞる線の組(外側の線から谷の線への順)。`region` があれば、その外の点を除く
/// (線をそこで切る)。
pub fn pencil_groups(
    heights: Heights,
    tool: &Tool,
    rest: Option<&Tool>,
    o: &Options,
    region: Option<&Region>,
) -> Vec<Vec<Chain>> {
    pencil_groups_profile::<false>(heights, tool, rest, o, region, &mut |_| {})
}

fn pencil_groups_profile<const EXHAUSTIVE: bool>(
    heights: Heights,
    tool: &Tool,
    rest: Option<&Tool>,
    o: &Options,
    region: Option<&Region>,
    mark: &mut impl FnMut(Phase),
) -> Vec<Vec<Chain>> {
    let cutter = Cutter::new(tool.clone(), o.grow);
    let field = DropField::new(heights.clone(), cutter.clone());
    mark(Phase::CurrentEnvelope);
    let rest_field = rest.map(|t| DropField::new(heights, Cutter::new(t.clone(), o.grow)));
    mark(Phase::RestEnvelope);
    let mut found = valley_nodes(&field, o);
    mark(Phase::ValleyNodes);
    if let Some(rf) = &rest_field {
        let keep = par_map(&found, |&(n, d)| {
            #[cfg(test)]
            if EXHAUSTIVE {
                return rest_filter_tests::exhaustive_left_by(&field, rf, n, d, o.leave) > REST_MIN;
            }
            left_by_more_than(&field, rf, n, d, o.leave, REST_MIN)
        });
        found = found
            .into_iter()
            .zip(keep)
            .filter_map(|(f, k)| k.then_some(f))
            .collect();
    }
    mark(Phase::LeftBy);
    let h = &field.heights;
    let mut flagged = vec![false; h.nx * h.ny];
    for &(n, _) in &found {
        flagged[n] = true;
    }
    let min_length = (4.0 * h.cell).max(0.5 * tool.radius());
    let length = |xy: &[[f64; 2]]| -> f64 {
        xy.windows(2)
            .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
            .sum()
    };
    // 位置の列を、工具を下ろした高さの線にする(範囲の外で切る)
    let to_chains = |xy: &[[f64; 2]], closed: bool| -> Vec<Chain> {
        let runs: Vec<(Vec<[f64; 2]>, bool)> = match region {
            Some(r) if !xy.iter().all(|q| r.contains(*q)) => {
                let mut v = Vec::new();
                let mut cur = Vec::new();
                for q in xy {
                    if r.contains(*q) {
                        cur.push(*q);
                    } else if !cur.is_empty() {
                        v.push((std::mem::take(&mut cur), false));
                    }
                }
                if !cur.is_empty() {
                    v.push((cur, false));
                }
                v
            }
            _ => vec![(xy.to_vec(), closed)],
        };
        runs.into_iter()
            .filter(|(xy, _)| length(xy) >= min_length)
            .map(|(xy, closed)| {
                let pts: Vec<P3> = xy
                    .iter()
                    .map(|q| [q[0], q[1], (field.drop_at(*q) + o.leave).max(o.z_min)])
                    .collect();
                let mut points = kmb_cam_dropcutter::simplify(&pts);
                if closed && points.len() > 2 && points.first() == points.last() {
                    points.pop();
                }
                Chain { points, closed }
            })
            .collect()
    };
    let groups = trace(&flagged, h.nx, h.ny)
        .into_iter()
        .filter_map(|(nodes, closed)| {
            let xy: Vec<[f64; 2]> = nodes.iter().map(|&n| h.at(n % h.nx, n / h.nx)).collect();
            let xy = smooth(&xy, closed);
            if length(&xy) < min_length {
                return None;
            }
            // 外側の線から谷の線へ
            let mut group = Vec::new();
            for k in (1..=o.passes).rev() {
                for side in [-1.0, 1.0] {
                    let off = offset_line(&xy, closed, side * k as f64 * o.pitch);
                    group.extend(to_chains(&off, closed));
                }
            }
            group.extend(to_chains(&xy, closed));
            (!group.is_empty()).then_some(group)
        })
        .collect();
    mark(Phase::Trace);
    groups
}

/// 線を横へ `d` だけずらす(進む向きの左が正)。各点の向きは前後の点から決める。
fn offset_line(xy: &[[f64; 2]], closed: bool, d: f64) -> Vec<[f64; 2]> {
    let n = xy.len();
    if n < 2 {
        return xy.to_vec();
    }
    (0..n)
        .map(|k| {
            let (a, b) = if closed {
                (xy[(k + n - 1) % n], xy[(k + 1) % n])
            } else {
                (xy[k.saturating_sub(1)], xy[(k + 1).min(n - 1)])
            };
            let (tx, ty) = (b[0] - a[0], b[1] - a[1]);
            let l = tx.hypot(ty).max(1e-12);
            [xy[k][0] - ty / l * d, xy[k][1] + tx / l * d]
        })
        .collect()
}

/// 谷の節点(格子の番号)と、いちばん大きく折れた向き。
fn valley_nodes(field: &DropField, o: &Options) -> Vec<(usize, usize)> {
    let h = &field.heights;
    let (nx, ny, cell) = (h.nx, h.ny, h.cell);
    // 折れを測る幅。工具の半径の 1/4(格子の 2〜40 節点)
    let w = ((0.25 * field.tool.radius() / cell).round() as isize).clamp(2, 40);
    let node = |i: isize, j: isize| -> Option<f64> {
        (i >= 0 && j >= 0 && (i as usize) < nx && (j as usize) < ny)
            .then(|| field.node(i as usize, j as usize))
    };
    // 節点 (i, j) で向き d に、両側 k 節点先までの上りの角度の和
    let bend = |i: isize, j: isize, d: (isize, isize), k: isize| -> Option<f64> {
        let z0 = node(i, j)?;
        let zl = node(i - k * d.0, j - k * d.1)?;
        let zr = node(i + k * d.0, j + k * d.1)?;
        let len = k as f64 * cell * ((d.0 * d.0 + d.1 * d.1) as f64).sqrt();
        Some(((zl - z0) / len).atan() + ((zr - z0) / len).atan())
    };
    let rows: Vec<usize> = (0..ny).collect();
    par_map(&rows, |&j| {
        let mut out = Vec::new();
        for i in 0..nx {
            let (ii, jj) = (i as isize, j as isize);
            let z = field.node(i, j) + o.leave;
            if z < o.z_min || z >= o.z_max {
                continue;
            }
            let mut best: Option<(f64, usize)> = None;
            for (k, &d) in DIRS.iter().enumerate() {
                if bend(ii, jj, d, w).is_none_or(|b| b <= o.angle) {
                    continue;
                }
                // 谷の帯の中で、近くで測った折れが向きに沿っていちばん大きい所
                let near = |s: isize| bend(ii + s * d.0, jj + s * d.1, d, 2);
                let Some(b) = near(0) else { continue };
                if b > 0.5 * o.angle
                    && near(1).is_none_or(|v| b >= v)
                    && near(-1).is_none_or(|v| b > v)
                    && best.is_none_or(|(v, _)| b > v)
                {
                    best = Some((b, k));
                }
            }
            if let Some((_, k)) = best {
                out.push((j * nx + i, k));
            }
        }
        out
    })
    .into_iter()
    .flatten()
    .collect()
}

/// 前の工具が下限より厚く残した点があるか。谷を横切る9点のうち一つでも、前の工具が
/// 届いた面と今の刃の差が下限を超えればよい。各点の候補の最小値が下限以下になったら
/// その点を、超える点が確定したら残りの点を調べずに終える。差の計算順は変えない。
fn left_by_more_than(
    field: &DropField,
    rest: &DropField,
    n: usize,
    d: usize,
    leave: f64,
    minimum: f64,
) -> bool {
    let h = &field.heights;
    let (i, j) = (n % h.nx, n / h.nx);
    let c = h.at(i, j);
    let z = field.node(i, j) + leave;
    let (di, dj) = DIRS[d];
    let len = ((di * di + dj * dj) as f64).sqrt();
    let u = [di as f64 / len, dj as f64 / len];
    let r = field.tool.radius();
    let rr = rest.tool.radius();
    let span = (rr / h.cell).ceil() as isize;
    'probe: for k in -4..=4 {
        let t = r * k as f64 / 4.0;
        let x = [c[0] + u[0] * t, c[1] + u[1] * t];
        let Some(tool_z) = field.tool.height_at(t.abs()).map(|v| z + v) else {
            continue;
        };
        // 前の工具が x で届いた面の高さ
        let (xi, xj) = (h.index(x[0], 0) as isize, h.index(x[1], 1) as isize);
        let mut reached = f64::INFINITY;
        for cj in (xj - span).max(0)..=(xj + span).min(h.ny as isize - 1) {
            for ci in (xi - span).max(0)..=(xi + span).min(h.nx as isize - 1) {
                let p = h.at(ci as usize, cj as usize);
                if let Some(v) = rest.tool.height_at((p[0] - x[0]).hypot(p[1] - x[1])) {
                    reached = reached.min(rest.node(ci as usize, cj as usize) + v);
                    // 一つでも刃との差が下限以下なら、この点では削り残しを示せない。
                    if reached - tool_z <= minimum {
                        continue 'probe;
                    }
                }
            }
        }
        if reached - tool_z > minimum {
            return true;
        }
    }
    false
}

/// 印の付いた節点を、隣どうしでつないだ線にする。線の脇に並んだ節点は同じ線にまとめる。
/// 端(隣が 1 つ以下)の節点から先に始めるので、線は端から端へなぞる。
fn trace(flagged: &[bool], nx: usize, ny: usize) -> Vec<(Vec<usize>, bool)> {
    let at = |n: usize, d: (isize, isize)| -> Option<usize> {
        let (i, j) = ((n % nx) as isize + d.0, (n / nx) as isize + d.1);
        (i >= 0 && j >= 0 && (i as usize) < nx && (j as usize) < ny)
            .then(|| j as usize * nx + i as usize)
    };
    let degree = |n: usize| {
        NEIGHBORS
            .iter()
            .filter(|&&d| at(n, d).is_some_and(|m| flagged[m]))
            .count()
    };
    let mut seeds: Vec<usize> = (0..flagged.len()).filter(|&n| flagged[n]).collect();
    seeds.sort_by_key(|&n| (degree(n) > 1, n));
    let mut used = vec![false; flagged.len()];
    let unit = |a: usize, b: usize| {
        let (dx, dy) = (
            (b % nx) as f64 - (a % nx) as f64,
            (b / nx) as f64 - (a / nx) as f64,
        );
        let l = dx.hypot(dy);
        [dx / l, dy / l]
    };
    // `start` から、`heading` に近い向きへ印の節点をたどる
    let walk = |used: &mut [bool], start: usize, heading: Option<[f64; 2]>| {
        let mut path = vec![start];
        let (mut cur, mut heading) = (start, heading);
        loop {
            let mut best: Option<(f64, usize)> = None;
            for ring in 1..=2isize {
                for dj in -ring..=ring {
                    for di in -ring..=ring {
                        if di.abs().max(dj.abs()) != ring {
                            continue;
                        }
                        let Some(m) = at(cur, (di, dj)).filter(|&m| flagged[m] && !used[m]) else {
                            continue;
                        };
                        let v = unit(cur, m);
                        let ahead = heading.map_or(0.0, |h| v[0] * h[0] + v[1] * h[1]);
                        if ahead < -0.2 {
                            continue;
                        }
                        if best.is_none_or(|(s, _)| ahead > s) {
                            best = Some((ahead, m));
                        }
                    }
                }
                if best.is_some() {
                    break;
                }
            }
            let Some((_, next)) = best else { break };
            let step = unit(cur, next);
            // 脇に並んだ節点は、この線にまとめる(後ろの節点は逆向きにたどる時に使う)
            for &d in &NEIGHBORS {
                if let Some(m) = at(cur, d).filter(|&m| flagged[m] && !used[m] && m != next) {
                    let v = unit(cur, m);
                    if (v[0] * step[0] + v[1] * step[1]).abs() < 0.8 {
                        used[m] = true;
                    }
                }
            }
            used[next] = true;
            path.push(next);
            heading = Some(step);
            cur = next;
        }
        path
    };
    let mut out = Vec::new();
    for s in seeds {
        if used[s] {
            continue;
        }
        used[s] = true;
        let fwd = walk(&mut used, s, None);
        let back_heading = fwd.get(1).map(|&n| {
            let v = unit(s, n);
            [-v[0], -v[1]]
        });
        let back = walk(&mut used, s, back_heading);
        let mut nodes: Vec<usize> = back.into_iter().rev().collect();
        nodes.extend(&fwd[1..]);
        let (first, last) = (nodes[0], nodes[nodes.len() - 1]);
        let gap = ((first % nx) as isize - (last % nx) as isize)
            .abs()
            .max(((first / nx) as isize - (last / nx) as isize).abs());
        let closed = nodes.len() >= 8 && gap <= 2;
        out.push((nodes, closed));
    }
    out
}

/// 節点の階段をならす(前後 2 点ずつとの平均。開いた線の両端は動かさない)。
fn smooth(xy: &[[f64; 2]], closed: bool) -> Vec<[f64; 2]> {
    let n = xy.len();
    if n < 5 {
        return xy.to_vec();
    }
    (0..n)
        .map(|k| {
            if !closed && (k < 2 || k + 2 >= n) {
                return xy[k];
            }
            let mut s = [0.0, 0.0];
            for o in -2isize..=2 {
                let q = xy[(k as isize + o).rem_euclid(n as isize) as usize];
                s[0] += q[0];
                s[1] += q[1];
            }
            [s[0] / 5.0, s[1] / 5.0]
        })
        .collect()
}

/// `a` から `b` へまっすぐ動いても、部品を削らない(`leave` を残す)か。
fn clear(h: &Heights, tool: &Cutter, a: P3, b: P3, leave: f64) -> bool {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let n = (len / (h.cell * 0.5)).ceil().max(1.0) as usize;
    (0..=n).all(|k| {
        let t = k as f64 / n as f64;
        let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        h.exact_at(tool, q) + leave <= a[2] + (b[2] - a[2]) * t + 1e-6
    })
}

/// 角度の式を読む(ラジアン)。
fn angle_of(ctx: &EvalCtx<'_>, src: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if q.dim != Dim::ANGLE {
        return Err(invalid(format!(
            "谷の角度 `{src}` は角度の単位(deg)が必要です(値: {q})"
        )));
    }
    Ok(q.value)
}

/// 谷ごとの線の組、部品の高さ、軸方向の残し、太らせた工具。
type Traced = (Vec<Vec<Chain>>, Heights, f64, Cutter);

impl Pencil {
    /// なぞる線と、つなぎの検査に使う格子。
    fn resolve(&self, ctx: &EvalCtx<'_>, p: &Prepared) -> Result<PencilPlan, EvalError> {
        let leave = length(ctx, &self.leave, "残し量")?;
        let cell = length(ctx, &self.resolution, "格子の幅")?;
        if !(0.01..=0.2).contains(&cell) {
            return Err(invalid("格子の幅は 0.01〜0.2 mm にしてください"));
        }
        let angle = match &self.angle {
            Some(src) => angle_of(ctx, src)?,
            None => DEFAULT_ANGLE_DEG.to_radians(),
        };
        if !(1f64.to_radians()..=90f64.to_radians()).contains(&angle) {
            return Err(invalid("谷の角度は 1〜90° にしてください"));
        }
        let rest = match self.rest_tool {
            Some(n) => {
                let t = ToolLibrary::tool_from(ctx, self.base.tools, n)?;
                if t.kind == ToolKind::Drill {
                    return Err(invalid("前の工具にドリルは使えません"));
                }
                Some(t)
            }
            None => None,
        };
        let depth = match &self.depth {
            Some(d) => {
                let d = length(ctx, d, "なぞる深さ")?;
                if d <= 0.0 {
                    return Err(invalid("なぞる深さは 0 より大きくしてください"));
                }
                Some(d)
            }
            None => None,
        };
        let limits = self.limits.resolve(ctx, p, cell.max(0.05))?;
        let pitch = match &self.pitch {
            Some(s) => length(ctx, s, "線の間隔")?,
            None => p.tool.diameter * p.cutting.stepover,
        };
        if self.passes > 0 && pitch <= 0.0 {
            return Err(invalid("線の間隔は 0 より大きくしてください"));
        }
        if self.passes > 50 {
            return Err(invalid("足す線の本数は 50 本までにしてください"));
        }
        Ok(PencilPlan {
            leave,
            cell,
            angle,
            rest,
            depth,
            limits,
            pitch,
            passes: self.passes,
        })
    }
}

/// ホストが解いたペンシルの値(`plan` の入力の `params`)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PencilPlan {
    pub leave: f64,
    pub cell: f64,
    /// 谷の角度(ラジアン)
    pub angle: f64,
    /// 取残し: 前の工具
    pub rest: Option<Tool>,
    /// なぞる深さ(部品の上面から)
    pub depth: Option<f64>,
    pub limits: kmb_cam_dropcutter::Resolved,
    pub pitch: f64,
    pub passes: u32,
}

/// なぞる線と、つなぎの検査に使う格子。
fn chains_profile<const EXHAUSTIVE: bool>(
    p: &Planner,
    f: &PencilPlan,
    mark: &mut impl FnMut(Phase),
) -> Result<Traced, EvalError> {
    let PencilPlan {
        leave,
        cell,
        angle,
        ref rest,
        depth,
        ref limits,
        pitch,
        passes,
    } = *f;
    let (m0, m1) = (p.model.min, p.model.max);
    // 部品の無い所は、部品の底より下の高さにして、なぞる線から外す
    let below = p.info.stock_min[2].min(m0[2]) - 1.0;
    let mut z_min = below + 0.5;
    if let Some(d) = depth {
        z_min = z_min.max(m1[2] - d);
    }
    let reach = p.r() + limits.leave_radial + rest.as_ref().map_or(0.0, |t| t.radius());
    let pad = reach + 3.0 * cell + passes as f64 * pitch;
    let heights = Heights::new(
        [m0[0] - pad, m0[1] - pad],
        [m1[0] + pad, m1[1] + pad],
        cell,
        below,
        &p.model.triangles,
    );
    mark(Phase::Heights);
    let o = Options {
        angle,
        leave,
        z_min,
        z_max: p.info.stock_max[2],
        grow: limits.leave_radial,
        passes,
        pitch,
    };
    let chains = pencil_groups_profile::<EXHAUSTIVE>(
        heights.clone(),
        &p.tool,
        rest.as_ref(),
        &o,
        limits.region.as_ref(),
        mark,
    );
    if chains.is_empty() {
        return Err(invalid(if rest.is_some() {
            "なぞる谷がありません(前の工具が角まで届いています)"
        } else {
            "なぞる谷がありません(谷の角度を小さくすると見つかることがあります)"
        }));
    }
    Ok((
        chains,
        heights,
        leave,
        Cutter::new(p.tool.clone(), limits.leave_radial),
    ))
}

/// 戦略の純関数: 解いた入力(`params` は [`PencilPlan`])だけから経路を作る。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    run_profile::<false>(input, &mut |_| {})
}

fn run_profile<const EXHAUSTIVE: bool>(
    input: &StrategyInput,
    mark: &mut impl FnMut(Phase),
) -> Result<StrategyOutput, EvalError> {
    let p = Planner::new(input);
    let f: PencilPlan = input.params().map_err(invalid)?;
    let (mut groups, heights, leave, cutter) = chains_profile::<EXHAUSTIVE>(&p, &f, mark)?;
    let lowest = groups
        .iter()
        .flatten()
        .flat_map(|c| c.points.iter().map(|q| q[2]))
        .fold(f64::INFINITY, f64::min);
    p.check_depth(p.info.stock_max[2], lowest)?;
    let mut tp = p.toolpath();
    let (feed, plunge) = (p.cutting.feed, p.cutting.plunge);
    let reach = STAY_DOWN * p.tool.diameter;
    let mut here = [p.model.min[0], p.model.min[1]];
    let mut down: Option<P3> = None;
    // 線の始める所(開いた線は近い端、輪は近い点)と、そこまでの距離
    let start_of = |c: &Chain, here: [f64; 2]| {
        let d = |q: P3| (q[0] - here[0]).hypot(q[1] - here[1]);
        let s = if c.closed {
            (0..c.points.len())
                .min_by(|&a, &b| d(c.points[a]).total_cmp(&d(c.points[b])))
                .unwrap_or(0)
        } else if d(c.points[c.points.len() - 1]) < d(c.points[0]) {
            c.points.len() - 1
        } else {
            0
        };
        (s, d(c.points[s]))
    };
    while !groups.is_empty() {
        // 外側の線が近い谷から、その谷の線を外側から順に
        let best = (0..groups.len())
            .min_by(|&a, &b| {
                start_of(&groups[a][0], here)
                    .1
                    .total_cmp(&start_of(&groups[b][0], here).1)
            })
            .expect("線がある");
        for c in groups.swap_remove(best) {
            let (start, dist) = start_of(&c, here);
            let pts: Vec<P3> = if c.closed {
                let n = c.points.len();
                (0..=n).map(|m| c.points[(start + m) % n]).collect()
            } else if start == 0 {
                c.points
            } else {
                c.points.into_iter().rev().collect()
            };
            let s = pts[0];
            match down {
                Some(e) if dist <= reach && clear(&heights, &cutter, e, s, leave) => {
                    tp.moves.push(Move::Linear { to: s, feed });
                }
                _ => {
                    approach(&mut tp, [s[0], s[1]]);
                    tp.moves.push(Move::Linear {
                        to: s,
                        feed: plunge,
                    });
                }
            }
            tp.moves
                .extend(pts[1..].iter().map(|&to| Move::Linear { to, feed }));
            let e = *pts.last().expect("点がある");
            here = [e[0], e[1]];
            down = Some(e);
        }
    }
    kmb_cam_setup::retract(&mut tp);
    mark(Phase::Join);
    Ok(p.output(tp))
}

impl Feature for Pencil {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs([&self.leave, &self.resolution])?);
        for e in [&self.angle, &self.depth, &self.pitch]
            .into_iter()
            .flatten()
        {
            v.extend(expr_refs([e])?);
        }
        v.extend(expr_refs(self.limits.exprs())?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if p.tool.kind == ToolKind::Drill {
            return Err(invalid(
                "ペンシルの加工にはエンドミル(ボールか平)を使ってください",
            ));
        }
        let f = self.resolve(ctx, &p)?;
        let input = p.input(ctx.feature_id(), f, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut fields = vec![&mut c.leave, &mut c.resolution];
        for e in [&mut c.angle, &mut c.depth, &mut c.pitch]
            .into_iter()
            .flatten()
        {
            fields.push(e);
        }
        fields.extend(c.limits.exprs_mut());
        Ok(c.base
            .rename(fields, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    let mut fields = vec![
        Field::expr("leave", "残し量").length(),
        leave_radial_field(),
        Field::expr("resolution", "格子の幅").length(),
        Field::opt_expr("angle", "谷の角度").angle().help(
            "工具が当たる 2 つの面の向きがこれ以上違う所をなぞる。例: 20 deg。空なら 20 deg",
        ),
        Field::opt_integer("rest_tool", "取残し加工: 前の工具の番号").unitless()
            .help("前の工具が届かず残した所だけをなぞる。空なら谷をすべてなぞる"),
        Field::opt_expr("depth", "なぞる深さ").length().help("空なら部品の底まで"),
        Field::integer("passes", "片側に足す線の本数").unitless()
            .help("谷の線の両側に、間隔ずつ離した線をこの本数ずつ足し、外側から谷へ順になぞる。0 なら谷の線だけ"),
        Field::opt_expr("pitch", "足す線の間隔").length().help("例: 0.3 mm。空なら工具の送り幅"),
    ];
    fields.extend(boundary_fields());
    reg.register_described(
        kmb_cam_setup::operation(
            PENCIL,
            "ペンシル(3D)",
            "内側の角や谷を 1 本ずつなぞり、走査線や等高線の削り残しを取る",
            fields,
        )
        .icon("pencil"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Pencil>(d)?)),
    );
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> kmb_cam_setup::StrategyModule {
    use kmb_cam_setup::{Strategy, StrategyModule, fitted, to_data};
    use kmb_cam_tool::ToolKind;
    StrategyModule::new(
        "kmb-strategy-pencil",
        register,
        vec![
            Strategy::new(PENCIL, "cam.add.pencil", "ペンシル(3D)", |p| {
                to_data(Pencil {
                    base: fitted(p.with_tool(ToolKind::BallEnd)),
                    leave: "0 mm".into(),
                    resolution: "0.05 mm".into(),
                    angle: None,
                    rest_tool: None,
                    depth: None,
                    passes: 0,
                    pitch: None,
                    limits: Default::default(),
                })
            })
            .order(26)
            .menu("3D")
            .icon("pencil")
            .not_front()
            .aliases("ぺんしる pencil 3d すみ かど たに のこり rest corner")
            .hint("内側の角や谷を 1 本ずつなぞり、走査線や等高線の削り残しを取る(ボールエンドミルがあればそれを使う)"),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ball(d: f64) -> Tool {
        Tool {
            kind: ToolKind::BallEnd,
            diameter: d,
            ..ToolLibrary::starter().tools[0].clone()
        }
    }

    fn quad(a: P3, b: P3, c: P3, e: P3) -> [[P3; 3]; 2] {
        [[a, b, c], [a, c, e]]
    }

    /// 高さ 0 の床に、x 0〜10・y -10〜10・高さ 5 の箱(上面だけで十分。壁は格子が縦にする)。
    pub(super) fn box_on_floor() -> Vec<[P3; 3]> {
        let mut t = Vec::new();
        t.extend(quad(
            [-20.0, -20.0, 0.0],
            [30.0, -20.0, 0.0],
            [30.0, 20.0, 0.0],
            [-20.0, 20.0, 0.0],
        ));
        t.extend(quad(
            [0.0, -10.0, 5.0],
            [10.0, -10.0, 5.0],
            [10.0, 10.0, 5.0],
            [0.0, 10.0, 5.0],
        ));
        t
    }

    /// x の向きに伸びる、両側が `deg` 度で上る V の溝(底は y = 0、高さ 0)。
    pub(super) fn v_groove(deg: f64) -> Vec<[P3; 3]> {
        let s = deg.to_radians().tan();
        let mut t = Vec::new();
        t.extend(quad(
            [-10.0, -6.0, 6.0 * s],
            [10.0, -6.0, 6.0 * s],
            [10.0, 0.0, 0.0],
            [-10.0, 0.0, 0.0],
        ));
        t.extend(quad(
            [-10.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 6.0, 6.0 * s],
            [-10.0, 6.0, 6.0 * s],
        ));
        t
    }

    fn opts() -> Options {
        Options {
            angle: DEFAULT_ANGLE_DEG.to_radians(),
            leave: 0.0,
            z_min: -0.5,
            z_max: 100.0,
            grow: 0.0,
            passes: 0,
            pitch: 0.3,
        }
    }

    #[test]
    fn the_corner_around_a_box_is_one_closed_ring_at_the_tool_radius() {
        let tool = ball(3.175);
        let r = tool.radius();
        let h = Heights::new([-6.0, -16.0], [16.0, 16.0], 0.05, -1.0, &box_on_floor());
        let chains = pencil_chains(h, &tool, None, &opts());
        assert_eq!(
            chains.len(),
            1,
            "{:?}",
            chains.iter().map(|c| c.points.len()).collect::<Vec<_>>()
        );
        let c = &chains[0];
        assert!(c.closed);
        for q in &c.points {
            assert!(q[2].abs() < 1e-6, "床の高さ: {q:?}");
            // 箱までの横の距離(箱の外)が工具の半径ほど
            let dx = (0.0 - q[0]).max(q[0] - 10.0).max(0.0);
            let dy = (-10.0 - q[1]).max(q[1] - 10.0).max(0.0);
            let d = dx.hypot(dy);
            assert!((r - 1e-6..r + 0.2).contains(&d), "箱から {d}: {q:?}");
        }
    }

    #[test]
    fn a_v_groove_is_traced_along_its_bottom_only_when_it_is_sharp_enough() {
        let tool = ball(3.175);
        let grid = || Heights::new([-8.0, -5.0], [8.0, 5.0], 0.05, -1.0, &v_groove(30.0));
        // 両側 30° の溝は、当たる面の法線が 60° 開く
        let chains = pencil_chains(grid(), &tool, None, &opts());
        assert_eq!(chains.len(), 1);
        let c = &chains[0];
        assert!(!c.closed);
        let xs: Vec<f64> = c.points.iter().map(|q| q[0]).collect();
        let span = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - xs.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(span > 14.0, "溝に沿って長い: {span}");
        for q in &c.points {
            assert!(q[1].abs() < 0.11, "溝の底の上: {q:?}");
            // ボールの先は、両側の面に当たって溝の底より r(1/cos30° − 1) 上で止まる
            let want = tool.radius() * (1.0 / 30f64.to_radians().cos() - 1.0);
            assert!((q[2] - want).abs() < 0.06, "{} と {want}", q[2]);
        }
        // 谷の角度を 70° にすると、60° の溝はなぞらない
        let steep = Options {
            angle: 70f64.to_radians(),
            ..opts()
        };
        assert!(pencil_chains(grid(), &tool, None, &steep).is_empty());
    }

    #[test]
    fn smooth_bowls_and_slopes_have_no_valleys() {
        // 半径 20 の球の内側(工具より十分大きい)と、30° の斜面
        let mut tris = Vec::new();
        let n = 40;
        let z = |x: f64, y: f64| 20.0 - (400.0 - x * x - y * y).max(0.0).sqrt();
        for a in 0..n {
            for b in 0..n {
                let p = |i: usize, j: usize| {
                    let (x, y) = (
                        -8.0 + 16.0 * i as f64 / n as f64,
                        -8.0 + 16.0 * j as f64 / n as f64,
                    );
                    [x, y, z(x, y)]
                };
                tris.extend(quad(p(a, b), p(a + 1, b), p(a + 1, b + 1), p(a, b + 1)));
            }
        }
        let tool = ball(3.175);
        let h = Heights::new([-6.0, -6.0], [6.0, 6.0], 0.05, -1.0, &tris);
        assert!(pencil_chains(h, &tool, None, &opts()).is_empty());
        let s = 30f64.to_radians().tan();
        let slope = quad(
            [-8.0, -8.0, 0.0],
            [8.0, -8.0, 16.0 * s],
            [8.0, 8.0, 16.0 * s],
            [-8.0, 8.0, 0.0],
        );
        let h = Heights::new([-6.0, -6.0], [6.0, 6.0], 0.05, -1.0, &slope);
        assert!(pencil_chains(h, &tool, None, &opts()).is_empty());
    }

    #[test]
    fn rest_keeps_only_what_the_previous_tool_left() {
        let small = ball(2.0);
        let grid = || Heights::new([-8.0, -5.0], [8.0, 5.0], 0.05, -1.0, &v_groove(30.0));
        // 前が太いボール(6 mm)なら溝の底に残るので、なぞる
        let big = ball(6.0);
        let after_big = pencil_chains(grid(), &small, Some(&big), &opts());
        assert_eq!(after_big.len(), 1);
        // 前が同じ太さなら、残りはない
        assert!(pencil_chains(grid(), &small, Some(&ball(2.0)), &opts()).is_empty());
        // 前が平の 6 mm でも、V の溝の底には届かないので、なぞる
        let flat = ToolLibrary::starter().tools[0].clone();
        assert_eq!(pencil_chains(grid(), &small, Some(&flat), &opts()).len(), 1);
    }

    /// 箱までの横の距離(箱の外)。
    fn from_box(q: P3) -> f64 {
        let dx = (0.0 - q[0]).max(q[0] - 10.0).max(0.0);
        let dy = (-10.0 - q[1]).max(q[1] - 10.0).max(0.0);
        dx.hypot(dy)
    }

    fn median(mut v: Vec<f64>) -> f64 {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    }

    #[test]
    fn extra_passes_are_added_on_both_sides_outer_first() {
        let tool = ball(3.175);
        let r = tool.radius();
        let h = Heights::new([-6.0, -16.0], [16.0, 16.0], 0.05, -1.0, &box_on_floor());
        let o = Options {
            passes: 2,
            pitch: 0.3,
            ..opts()
        };
        let groups = pencil_groups(h, &tool, None, &o, None);
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.len(), 5, "片側 2 本ずつと谷の線");
        // 最初の 2 本は 0.6 mm 離した線。外側の 1 本は床の上で、箱から r + 0.6
        let outer: Vec<f64> = g[..2]
            .iter()
            .map(|c| median(c.points.iter().map(|&q| from_box(q)).collect()))
            .collect();
        assert!(
            outer.iter().any(|d| (d - (r + 0.6)).abs() < 0.1),
            "{outer:?}"
        );
        // 箱の側へずらした線は、箱の角に乗り上げて床より高い
        let inner_z = g[..2]
            .iter()
            .map(|c| median(c.points.iter().map(|q| q[2]).collect()))
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(inner_z > 0.01, "{inner_z}");
        // 最後が谷の線(箱から r)
        let last = median(g[4].points.iter().map(|&q| from_box(q)).collect());
        assert!((last - r).abs() < 0.1, "{last}");
        // 本数 0 なら谷の線だけ
        let one = pencil_groups(
            Heights::new([-6.0, -16.0], [16.0, 16.0], 0.05, -1.0, &box_on_floor()),
            &tool,
            None,
            &opts(),
            None,
        );
        assert_eq!(one.iter().map(Vec::len).sum::<usize>(), 1);
    }

    #[test]
    fn radial_leave_moves_the_line_away_from_the_wall() {
        let tool = ball(3.175);
        let r = tool.radius();
        let h = Heights::new([-6.0, -16.0], [16.0, 16.0], 0.05, -1.0, &box_on_floor());
        let o = Options {
            grow: 0.5,
            ..opts()
        };
        let chains = pencil_chains(h, &tool, None, &o);
        assert_eq!(chains.len(), 1);
        for q in &chains[0].points {
            let d = from_box(*q);
            assert!(d > r + 0.45 && d < r + 0.7, "箱から {d}: {q:?}");
        }
    }

    #[test]
    fn the_region_cuts_the_lines() {
        let tool = ball(3.175);
        let h = Heights::new([-6.0, -16.0], [16.0, 16.0], 0.05, -1.0, &box_on_floor());
        // y < 0 の側だけ
        let half = Region::new(&kmb_cam_dropcutter::areas_from_loops(vec![vec![
            [-30.0, -30.0],
            [30.0, -30.0],
            [30.0, 0.0],
            [-30.0, 0.0],
        ]]));
        let chains: Vec<Chain> = pencil_groups(h, &tool, None, &opts(), Some(&half))
            .into_iter()
            .flatten()
            .collect();
        assert!(!chains.is_empty());
        for c in &chains {
            assert!(!c.closed);
            assert!(c.points.iter().all(|q| q[1] <= 0.0), "{:?}", c.points);
        }
    }
}
#[cfg(test)]
mod performance {
    use super::*;

    fn fixture_input(case: &str) -> StrategyInput {
        if case == "reference" {
            return serde_json::from_str(include_str!("../tests/reference-rest-input.json"))
                .unwrap();
        }
        let triangles = match case {
            "box" => super::tests::box_on_floor(),
            "v" => super::tests::v_groove(45.0),
            _ => panic!("計測する形がありません: {case}"),
        };
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for point in triangles.iter().flatten() {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(point[axis]);
                hi[axis] = hi[axis].max(point[axis]);
            }
        }
        let mesh = kmb_cam_setup::testing::mesh_of(&[("part", triangles)]);
        let rest = ToolLibrary::starter().tools[3].clone();
        let tool = Tool {
            number: 9,
            diameter: 1.0,
            flute_length: 12.0,
            ..rest.clone()
        };
        let p = kmb_cam_setup::testing::prepared(&mesh, lo, [hi[0], hi[1], hi[2] + 1.0], tool);
        p.input(
            kmb_core::FeatureId(1),
            PencilPlan {
                leave: 0.0,
                cell: 0.1,
                angle: DEFAULT_ANGLE_DEG.to_radians(),
                rest: Some(rest),
                depth: None,
                limits: Default::default(),
                pitch: p.tool.diameter * p.cutting.stepover,
                passes: 0,
            },
            Default::default(),
        )
    }

    // 確かめること: 見本・箱・V溝の動きの列が、同じ環境の全候補の計算と完全に一致する。
    #[test]
    fn reference_box_and_v_paths_match_the_exhaustive_filter() {
        for (case, count) in [("reference", 274), ("box", 34), ("v", 5)] {
            let input = fixture_input(case);
            let expected = run_profile::<true>(&input, &mut |_| {}).unwrap();
            let output = plan(&input).unwrap();
            assert_eq!(output.toolpath.moves.len(), count, "{case}");
            assert_eq!(output, expected, "{case}");
            assert_eq!(
                serde_json::to_vec(&output).unwrap(),
                serde_json::to_vec(&expected).unwrap(),
                "{case}"
            );
        }
    }

    // 確かめること: 同じ解いた入力の計算時間と、経路のバイト列を単独で測る。
    #[test]
    #[ignore = "道具: 残り削りの段ごとの時間を、ほかの重い処理を止めて測る"]
    fn profile_rest_pencil() {
        let mut input: StrategyInput = match std::env::var("JC_PENCIL_INPUT") {
            Ok(path) => serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap(),
            Err(_) => fixture_input(
                &std::env::var("JC_PENCIL_CASE").unwrap_or_else(|_| "reference".into()),
            ),
        };
        if let Ok(path) = std::env::var("JC_PENCIL_INPUT_OUT") {
            std::fs::write(path, serde_json::to_vec(&input).unwrap()).unwrap();
        }
        if let Ok(cell) = std::env::var("JC_PENCIL_CELL") {
            input.params["cell"] = serde_json::json!(cell.parse::<f64>().unwrap());
        }
        let started = std::time::Instant::now();
        let mut last = std::time::Instant::now();
        let mut phases = Vec::new();
        let output = run_profile::<false>(&input, &mut |phase| {
            let now = std::time::Instant::now();
            phases.push((
                format!("{phase:?}"),
                now.duration_since(last).as_secs_f64() * 1000.0,
            ));
            last = now;
        })
        .unwrap();
        for (phase, ms) in &phases {
            println!("phase={phase}, ms={ms:.3}");
        }
        let elapsed = started.elapsed();
        let bytes = serde_json::to_vec(&output).unwrap();
        println!(
            "total_ms={:.3}, cell={}, moves={}, cut_length={:.12}",
            elapsed.as_secs_f64() * 1000.0,
            input.params["cell"],
            output.toolpath.moves.len(),
            output.toolpath.lengths().0
        );
        if let Ok(path) = std::env::var("JC_PENCIL_BASELINE") {
            assert_eq!(bytes, std::fs::read(path).unwrap(), "経路が変わりました");
        }
        if let Ok(path) = std::env::var("JC_PENCIL_OUTPUT") {
            std::fs::write(path, &bytes).unwrap();
        }
    }
}

#[cfg(test)]
mod rest_filter_tests {
    use super::*;
    pub(super) fn exhaustive_left_by(
        field: &DropField,
        rest: &DropField,
        n: usize,
        d: usize,
        leave: f64,
    ) -> f64 {
        let h = &field.heights;
        let (i, j) = (n % h.nx, n / h.nx);
        let c = h.at(i, j);
        let z = field.node(i, j) + leave;
        let (di, dj) = DIRS[d];
        let len = ((di * di + dj * dj) as f64).sqrt();
        let u = [di as f64 / len, dj as f64 / len];
        let r = field.tool.radius();
        let rr = rest.tool.radius();
        let span = (rr / h.cell).ceil() as isize;
        let mut worst = f64::NEG_INFINITY;
        for k in -4..=4 {
            let t = r * k as f64 / 4.0;
            let x = [c[0] + u[0] * t, c[1] + u[1] * t];
            let Some(tool_z) = field.tool.height_at(t.abs()).map(|v| z + v) else {
                continue;
            };
            // 前の工具が x で届いた面の高さ
            let (xi, xj) = (h.index(x[0], 0) as isize, h.index(x[1], 1) as isize);
            let mut reached = f64::INFINITY;
            for cj in (xj - span).max(0)..=(xj + span).min(h.ny as isize - 1) {
                for ci in (xi - span).max(0)..=(xi + span).min(h.nx as isize - 1) {
                    let p = h.at(ci as usize, cj as usize);
                    if let Some(v) = rest.tool.height_at((p[0] - x[0]).hypot(p[1] - x[1])) {
                        reached = reached.min(rest.node(ci as usize, cj as usize) + v);
                    }
                }
            }
            worst = worst.max(reached - tool_z);
        }
        worst
    }
    // 確かめること: 全候補の最大の差と、短絡する判定が、下限のちょうど前後でも一致する。
    #[test]
    fn short_circuit_matches_the_exhaustive_gap_at_its_float_boundary() {
        for (case, same_tool, grow, leave) in [
            ("v", false, 0.0, 0.0),
            ("v", true, 0.15, 0.1),
            ("box", false, 0.0, -0.1),
        ] {
            let triangles = if case == "v" {
                super::tests::v_groove(45.0)
            } else {
                super::tests::box_on_floor()
            };
            let h = Heights::new([-3.0, -3.0], [3.0, 3.0], 0.25, -1.0, &triangles);
            let template = ToolLibrary::starter().tools[3].clone();
            let tool = Tool {
                diameter: 1.0,
                ..template.clone()
            };
            let previous = if same_tool { tool.clone() } else { template };
            let field = DropField::new(h.clone(), Cutter::new(tool, grow));
            let rest = DropField::new(h, Cutter::new(previous, grow));
            let h = &field.heights;
            for (i, j) in [
                (0, 0),
                (h.nx - 1, h.ny - 1),
                (h.nx / 2, h.ny / 2),
                (h.nx / 2 + 1, h.ny / 2 - 1),
            ] {
                for d in 0..DIRS.len() {
                    let n = j * h.nx + i;
                    let gap = exhaustive_left_by(&field, &rest, n, d, leave);
                    for minimum in [gap.next_down(), gap, gap.next_up(), REST_MIN] {
                        assert_eq!(
                            left_by_more_than(&field, &rest, n, d, leave, minimum),
                            gap > minimum,
                            "{case}, same={same_tool}, grow={grow}, leave={leave}, n={n}, d={d}, gap={gap}, minimum={minimum}"
                        );
                    }
                }
            }
        }
    }
}
