//! 3D のランプ(`cam.ramp3d@1`)。等高線(3D)と同じ段の輪を作り、段の高さで一周するかわりに、
//! 1 周ごとに次の段の高さまで壁に沿って斜めに下りる。工具が途切れずに当たり続け、段ごとの
//! 下り込みの跡が出ない。急な壁や縦の壁の仕上げ・中仕上げに使う。
//!
//! 下りる周は、隣り合う段の輪を弧長の割合で混ぜて作る(昔からある、等高線を渦巻きにつなぐやり方)。
//! 輪 k の上の割合 t の点と、輪 k+1 の上の割合 t の点を (1 − t) : t で混ぜ、高さも段の間を t で
//! 下げる。混ぜた点ごとに工具を下ろした高さを調べ、それより下には下げない(食い込まない)。
//! 周が短く傾きが `ramp_angle` を超える時は、同じ割合で何周かに分けて下りる。高さは混ぜた線の
//! 長さの割合で下げる。ボールが上の縁に乗る高さでは面の高さに沿うので、そこだけ傾きが上限を
//! 超えることがある。
//!
//! 上の段の輪に、横へ工具の直径の数倍以内で向きの同じ次の段の輪があれば、つないで下り続ける
//! (輪が分かれる・合わさる所は、残った輪から新しく始める)。いちばん下の輪は、その高さで
//! 一周して終える。加工の範囲・傾斜の範囲で切れた開いた線は、等高線と同じく高さを保って
//! たどる。

use crate::{Levels, Opts, Order, Ring, clear, levels};
use crate::{resolution, zero};
use jc_cam_dropcutter::Limits;
use jc_cam_dropcutter::limits::{boundary_fields, leave_radial_field, slope_fields};
use jc_cam_geom::Loop;
use jc_cam_geom::ring::{at, cumulative, dist, nearest, rotate};
use jc_cam_setup::{
    OpBase, Planner, Prepared, StrategyInput, StrategyOutput, approach, expr_refs, invalid, length,
    plan_error, prepare, ramp_angle, ramp_moves,
};
use jc_cam_tool::ToolKind;
use jc_core::{EvalCtx, EvalError, Feature, Field, InputRef, Outputs, Registry};
use jc_toolpath::{Move, Toolpath};
use serde::{Deserialize, Serialize};

pub const RAMP3D: &str = "cam.ramp3d@1";

/// 周の傾きの上限の既定(度)。
pub const DEFAULT_RAMP3D_DEG: f64 = 5.0;

/// 次の段の輪とつなぐ、横のずれの上限(工具の直径の倍数)。
const LINK: f64 = 4.0;

/// 混ぜた点の食い込みを調べる時の許し(mm)。この分だけ細い工具を下ろす。縦の壁の際では、輪が
/// 弦の膨らみほど(1 µm 未満)内へ入るだけで、元の太さの工具は壁の上に乗った高さになり、経路が
/// 針のように跳ねる。
const SNUG: f64 = 0.01;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ramp3d {
    #[serde(flatten)]
    pub base: OpBase,
    /// 部品の面に残す厚さ(軸方向。上向きに測る)
    #[serde(default = "zero")]
    pub leave: String,
    /// 1 周で下りる高さの上限(省くと工具の切り込み)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// 部品の高さを調べる格子の幅
    #[serde(default = "resolution")]
    pub resolution: String,
    /// 部品のいちばん高い所から、どこまで下を回るか(省くと部品の底かストックの底まで)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
    /// 周の傾きの上限(例: `5 deg`)。空なら 5 deg
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_angle: Option<String>,
    /// 径方向に残す厚さ・加工の範囲・傾斜の範囲
    #[serde(flatten)]
    pub limits: Limits,
}

impl Ramp3d {
    pub fn new(base: OpBase) -> Ramp3d {
        Ramp3d {
            base,
            leave: zero(),
            step: Some("0.5 mm".into()),
            resolution: resolution(),
            depth: None,
            ramp_angle: None,
            limits: Limits::default(),
        }
    }
}

type P2 = [f64; 2];

/// 点 `q` から折れ線の輪 `ring` までの距離。
fn to_ring(q: P2, ring: &[P2]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dy * dy;
            let t = if l2 < 1e-18 {
                0.0
            } else {
                (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
            };
            dist(q, [a[0] + dx * t, a[1] + dy * t])
        })
        .fold(f64::INFINITY, f64::min)
}

/// 輪 `b` の点が輪 `a` からどれだけ離れているか(`b` の点を間引いて測った最大)。
fn apart(a: &[P2], b: &[P2]) -> f64 {
    let k = (b.len() / 48).max(1);
    b.iter()
        .step_by(k)
        .map(|&q| to_ring(q, a))
        .fold(0.0, f64::max)
}

/// 輪 `a`(高さ `za`)から輪 `b`(高さ `zb`)へ、混ぜながら下りる点の列(`a` と `b` の始まりは
/// そろえてある)。最後の点は `b` の始まり。周の傾きが `angle` を超えないよう `turns` 周に分ける。
/// `floor(q)` は点 `q` で下げてよい高さの下限(工具を下ろした高さ + 残し量)。
pub fn blend(
    a: &[P2],
    za: f64,
    b: &[P2],
    zb: f64,
    angle: f64,
    ds: f64,
    floor: &dyn Fn(P2) -> f64,
) -> Vec<[f64; 3]> {
    let (ca, cb) = (cumulative(a), cumulative(b));
    let per = ca[a.len()].max(cb[b.len()]).max(1e-6);
    let turns = (((za - zb) / (per * angle.tan())).ceil() as usize).max(1);
    // 混ぜた線は短い方の輪より短くなりうるので、長い方の輪の長さで周の数を決めた上で、
    // 長さの割合で下げる
    let n = ((per * turns as f64 / ds).ceil() as usize).max(8);
    let xy: Vec<P2> = (1..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let f = if i == n {
                0.0
            } else {
                (t * turns as f64).fract()
            };
            let (p, q) = (at(a, &ca, f), at(b, &cb, f));
            [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]
        })
        .collect();
    // 高さは、混ぜた線の上の長さの割合で下げる(角で xy が詰まっても傾きが一様)
    let mut run = Vec::with_capacity(n);
    let mut s = 0.0;
    let mut last = a[0];
    for &q in &xy {
        s += dist(last, q);
        run.push(s);
        last = q;
    }
    let total = s.max(1e-12);
    xy.iter()
        .zip(&run)
        .map(|(&q, &s)| {
            let z = za + (zb - za) * (s / total);
            settle(q, z, ds, floor)
        })
        .collect()
}

/// 混ぜた点 `q` が高さ `z` で部品に当たる時は、下ろした高さの下り坂の向き(壁から離れる向き)へ
/// 当たらない所まで横へずらす。上げて避けると、壁の際では下ろした高さが急に変わるので経路が
/// 針のように跳ねる(輪の形が段ごとに違う角や縁で、割合で混ぜた点は壁の側へ寄る)。
/// 工具の直径ほどずらしても当たる時だけ、上げて避ける。
fn settle(q: P2, z: f64, ds: f64, floor: &dyn Fn(P2) -> f64) -> [f64; 3] {
    let f0 = floor(q);
    if f0 <= z + 1e-6 {
        return [q[0], q[1], z];
    }
    let h = 0.5 * ds;
    let gx = floor([q[0] + h, q[1]]) - floor([q[0] - h, q[1]]);
    let gy = floor([q[0], q[1] + h]) - floor([q[0], q[1] - h]);
    let g = gx.hypot(gy);
    if g > 1e-9 {
        let dir = [-gx / g, -gy / g];
        let at = |d: f64| [q[0] + dir[0] * d, q[1] + dir[1] * d];
        let step = 0.25 * ds;
        let reach = (8.0 * ds).max(1.0);
        let mut lo = 0.0;
        let mut d = step;
        while d <= reach {
            if floor(at(d)) <= z + 1e-6 {
                // 当たる lo と当たらない d の間を詰める(当たらない側を採る)
                let mut hi = d;
                for _ in 0..8 {
                    let m = 0.5 * (lo + hi);
                    if floor(at(m)) <= z + 1e-6 {
                        hi = m;
                    } else {
                        lo = m;
                    }
                }
                let p = at(hi);
                return [p[0], p[1], z];
            }
            lo = d;
            d += step;
        }
    }
    [q[0], q[1], f0]
}

/// 段の輪を、上から下へつないだ列にする(閉じた輪だけ。開いた線は 1 本ずつ)。
pub fn chains(lv: &Levels, reach: f64) -> Vec<Vec<Ring>> {
    let mut zs: Vec<f64> = lv.rings.iter().map(|r| r.z).collect();
    zs.dedup();
    let mut used = vec![false; lv.rings.len()];
    let mut out: Vec<Vec<Ring>> = Vec::new();
    for (i, r) in lv.rings.iter().enumerate() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut chain = vec![r.clone()];
        if r.closed {
            loop {
                let last = chain.last().expect("輪がある");
                let Some(k) = zs.iter().position(|&z| z == last.z) else {
                    break;
                };
                let Some(&next_z) = zs.get(k + 1) else {
                    break;
                };
                let sign = jc_cam_geom::polygon_area(&last.pts).signum();
                let best = lv
                    .rings
                    .iter()
                    .enumerate()
                    .filter(|(j, c)| {
                        !used[*j]
                            && c.closed
                            && c.z == next_z
                            && jc_cam_geom::polygon_area(&c.pts).signum() == sign
                    })
                    .map(|(j, c)| (j, apart(&last.pts, &c.pts).max(apart(&c.pts, &last.pts))))
                    .filter(|(_, d)| *d <= reach)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                let Some((j, _)) = best else {
                    break;
                };
                used[j] = true;
                chain.push(lv.rings[j].clone());
            }
        }
        out.push(chain);
    }
    out
}

/// ランプの経路の動き(`tp` に足す)。
pub fn moves(p: &Planner, lv: &Levels, o: &Opts, ramp: f64, tp: &mut Toolpath) {
    let (feed, plunge) = (p.cutting.feed, p.cutting.plunge);
    let reach = LINK * p.tool.diameter;
    let ds = 2.0 * o.cell;
    let mut todo = chains(lv, reach);
    let mut thin = lv.field.tool.clone();
    thin.tool.diameter = (thin.tool.diameter - 2.0 * SNUG).max(SNUG);
    if let Some(c) = thin.tool.corner_radius.as_mut() {
        *c = c.min(thin.tool.diameter / 2.0);
    }
    let floor = |q: P2| lv.field.heights.exact_at(&thin, q) + lv.leave;
    let mut here = [p.model.min[0], p.model.min[1]];
    let mut down: Option<f64> = None;
    while !todo.is_empty() {
        // 上の段から、今いる所に近い列を先に
        let top = todo
            .iter()
            .map(|c| c[0].z)
            .fold(f64::NEG_INFINITY, f64::max);
        let k = (0..todo.len())
            .filter(|&k| todo[k][0].z == top)
            .min_by(|&a, &b| {
                let d = |k: usize| {
                    let r = &todo[k][0];
                    r.pts
                        .iter()
                        .map(|&q| dist(q, here))
                        .fold(f64::INFINITY, f64::min)
                };
                d(a).total_cmp(&d(b))
            })
            .expect("列がある");
        let chain = todo.remove(k);
        let first = &chain[0];
        let start = if first.closed {
            nearest(&first.pts, here)
        } else {
            0
        };
        let pts = if first.closed {
            rotate(&first.pts, start)
        } else {
            first.pts.clone()
        };
        let s = pts[0];
        let z0 = first.z;
        // 近くて削らずに動ければ下げたまま横へ。でなければ上げて移り、1 つ上の段の高さまで下げる
        let from = match down {
            Some(zc)
                if zc >= z0 - 1e-9
                    && dist(here, s) <= reach
                    && clear(&lv.field, here, s, zc, lv.leave) =>
            {
                if dist(here, s) > 1e-9 {
                    tp.moves.push(Move::Linear {
                        to: [s[0], s[1], zc],
                        feed,
                    });
                }
                zc
            }
            _ => {
                approach(tp, s);
                let e = (z0 + lv.spacing).min(tp.retract_z);
                tp.moves.push(Move::Linear {
                    to: [s[0], s[1], e],
                    feed: plunge,
                });
                e
            }
        };
        if !first.closed {
            if from > z0 + 1e-9 {
                tp.moves.push(Move::Linear {
                    to: [s[0], s[1], z0],
                    feed: plunge,
                });
            }
            tp.moves.extend(pts[1..].iter().map(|q| Move::Linear {
                to: [q[0], q[1], z0],
                feed,
            }));
            here = *pts.last().expect("点がある");
            down = Some(z0);
            continue;
        }
        // 始めの輪へは輪に沿って下りる(傾きは周の上限も超えない)
        let m = tp.moves.len();
        tp.moves.extend(ramp_moves(
            &Loop::from_points(&pts),
            from,
            z0,
            o.ramp.min(ramp),
            feed,
            plunge,
        ));
        tp.mark(m, jc_toolpath::Role::Entry);
        // 次の段の輪へ、混ぜながら下りる
        let mut cur = pts;
        let mut z = z0;
        for next in &chain[1..] {
            let b = rotate(&next.pts, nearest(&next.pts, cur[0]));
            for q in blend(&cur, z, &b, next.z, ramp, ds, &floor) {
                tp.moves.push(Move::Linear { to: q, feed });
            }
            cur = b;
            z = next.z;
        }
        // いちばん下の輪はその高さで一周する
        for q in cur[1..].iter().chain(std::iter::once(&cur[0])) {
            tp.moves.push(Move::Linear {
                to: [q[0], q[1], z],
                feed,
            });
        }
        here = cur[0];
        down = Some(z);
    }
    jc_cam_setup::retract(tp);
}

impl Ramp3d {
    fn opts(&self, ctx: &EvalCtx<'_>, p: &mut Prepared) -> Result<(Opts, f64), EvalError> {
        let leave = length(ctx, &self.leave, "残し量")?;
        let cell = length(ctx, &self.resolution, "格子の幅")?;
        if !(0.02..=0.5).contains(&cell) {
            return Err(invalid("格子の幅は 0.02〜0.5 mm にしてください"));
        }
        let step = match &self.step {
            Some(s) => length(ctx, s, "1 周の下り")?,
            None => p.cutting.stepdown,
        };
        if step <= 0.0 {
            return Err(invalid("1 周の下りは 0 より大きくしてください"));
        }
        let (m0, m1) = (p.model.min, p.model.max);
        let top = m1[2].min(p.info.stock_max[2]);
        let mut bottom = p.info.stock_min[2].max(m0[2]);
        if let Some(d) = &self.depth {
            let d = length(ctx, d, "回る深さ")?;
            if d <= 0.0 {
                return Err(invalid("回る深さは 0 より大きくしてください"));
            }
            bottom = bottom.max(m1[2] - d);
        } else {
            bottom = p.reachable_bottom(p.info.stock_max[2], bottom);
        }
        let (top, bottom) = (p.top(top), p.bottom(bottom));
        if top - bottom < step * 0.5 {
            return Err(invalid("回る高さの幅が 1 周の下りより狭いです"));
        }
        p.check_depth(p.info.stock_max[2], bottom)?;
        let ramp = ramp_angle(ctx, &self.ramp_angle)?.unwrap_or(DEFAULT_RAMP3D_DEG.to_radians());
        let limits = self.limits.resolve(ctx, p, cell.max(0.05))?;
        Ok((
            Opts {
                leave,
                cell,
                step,
                bottom,
                top,
                ramp: crate::DEFAULT_RAMP_DEG.to_radians(),
                order: Order::TopDown,
                limits,
                entry: None,
            },
            ramp,
        ))
    }
}

/// 戦略の純関数: 解いた入力(`params` は等高線の [`Opts`] と下りる角度)だけから経路を作る。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    let p = Planner::new(input);
    let (o, ramp): (Opts, f64) = input.params()?;
    let lv = levels(&p, &o).map_err(plan_error)?;
    let mut tp = p.toolpath();
    moves(&p, &lv, &o, ramp, &mut tp);
    Ok(p.output(tp))
}

impl Feature for Ramp3d {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs(
            [&self.leave, &self.resolution]
                .into_iter()
                .chain(
                    [&self.step, &self.depth, &self.ramp_angle]
                        .into_iter()
                        .flatten(),
                )
                .chain(self.limits.exprs()),
        )?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if p.tool.kind == ToolKind::Drill {
            return Err(invalid(
                "ランプの加工にはエンドミル(平かボール)を使ってください",
            ));
        }
        let (o, ramp) = self.opts(ctx, &mut p)?;
        let input = p.input(ctx.feature_id(), (o, ramp), Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut fields = vec![&mut c.leave, &mut c.resolution];
        for e in [&mut c.step, &mut c.depth, &mut c.ramp_angle]
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
        Field::opt_expr("step", "1 周の下り")
            .length()
            .help("1 周で下りる高さの上限。空なら工具の切り込み"),
        Field::expr("resolution", "格子の幅").length(),
        Field::opt_expr("depth", "回る深さ")
            .length()
            .help("空なら部品かストックの底まで"),
        Field::opt_expr("ramp_angle", "ランプ角度")
            .angle()
            .help("周の傾きの上限。例: 5 deg。短い周はこれを超えないよう何周かに分けて下りる。空なら 5 deg"),
    ];
    fields.extend(slope_fields());
    fields.extend(boundary_fields());
    fields.extend(jc_cam_setup::height_fields());
    reg.register_described(
        jc_cam_setup::operation(
            RAMP3D,
            "ランプ(3D)",
            "縦の壁や急な斜面を、1 周ごとに斜めに下りながら途切れずに仕上げる",
            fields,
        )
        .icon("ramp3d"),
        |d| Ok(Box::new(jc_cam_setup::parse::<Ramp3d>(d)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_cam_dropcutter::{DropField, Resolved};
    use jc_cam_setup::testing::{mesh_of, prepared};
    use jc_cam_tool::ToolLibrary;
    use jc_toolpath::P3;

    fn quad(a: P3, b: P3, c: P3, d: P3) -> Vec<[P3; 3]> {
        vec![[a, b, c], [a, c, d]]
    }

    fn top(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Vec<[P3; 3]> {
        quad([x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z])
    }

    /// 縦の壁の箱の側面(上面は z1)。
    fn walls(x0: f64, y0: f64, x1: f64, y1: f64, z1: f64) -> Vec<[P3; 3]> {
        let c = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        (0..4)
            .flat_map(|i| {
                let (a, b) = (c[i], c[(i + 1) % 4]);
                quad(
                    [a[0], a[1], 0.0],
                    [b[0], b[1], 0.0],
                    [b[0], b[1], z1],
                    [a[0], a[1], z1],
                )
            })
            .collect()
    }

    /// 床(高さ 0)の上に、箱(x 0〜10、y -5〜5、高さ 6)。必要なら 2 つ目の箱(x 20〜26)。
    fn part(two: bool) -> Planner {
        let mut faces = vec![
            ("floor", top(-10.0, -10.0, 40.0, 10.0, 0.0)),
            ("a", top(0.0, -5.0, 10.0, 5.0, 6.0)),
            ("a-side", walls(0.0, -5.0, 10.0, 5.0, 6.0)),
        ];
        if two {
            faces.push(("b", top(20.0, -3.0, 26.0, 3.0, 4.0)));
            faces.push(("b-side", walls(20.0, -3.0, 26.0, 3.0, 4.0)));
        }
        let ball = ToolLibrary::starter().tools[3].clone();
        prepared(
            &mesh_of(&faces),
            [-10.0, -10.0, 0.0],
            [40.0, 10.0, 7.0],
            ball,
        )
        .planner()
    }

    fn opts(p: &Planner) -> Opts {
        Opts {
            leave: 0.0,
            cell: 0.1,
            step: 1.0,
            bottom: 0.0,
            top: p.model.max[2],
            ramp: crate::DEFAULT_RAMP_DEG.to_radians(),
            order: Order::TopDown,
            limits: Resolved::default(),
            entry: None,
        }
    }

    fn run(p: &Planner, angle_deg: f64) -> (Levels, Toolpath) {
        let o = opts(p);
        let lv = levels(p, &o).unwrap();
        let mut tp = Toolpath {
            name: "ramp".into(),
            tool: p.tool.clone(),
            rpm: 10000.0,
            safe_z: p.info.safe_z,
            retract_z: p.info.retract_z,
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
            moves: Vec::new(),
        };
        moves(p, &lv, &o, angle_deg.to_radians(), &mut tp);
        (lv, tp)
    }

    /// 送りの点(早送りを除く)。
    fn cuts(tp: &Toolpath) -> Vec<P3> {
        tp.moves
            .iter()
            .filter(|m| matches!(m, Move::Linear { .. } | Move::Arc { .. }))
            .map(|m| m.to())
            .filter(|q| q[2] < tp.retract_z - 1e-9)
            .collect()
    }

    #[test]
    fn the_wall_is_cut_in_one_descending_spiral_without_gouging() {
        let p = part(false);
        let (lv, tp) = run(&p, 30.0);
        // 箱のまわりの輪は 1 本の列につながる(上げて入るのは 1 回だけ)
        let chains = chains(&lv, LINK * p.tool.diameter);
        assert_eq!(chains.len(), 1, "{}", chains.len());
        let rapids = tp
            .moves
            .iter()
            .filter(|m| matches!(m, Move::Rapid { .. }))
            .count();
        assert!(rapids <= 3, "早送り {rapids}");
        let pts = cuts(&tp);
        // 壁を回りながら下りる: 最後の一周より前に、同じ高さのまま回り続ける所が無い
        let lowest = pts.iter().map(|q| q[2]).fold(f64::INFINITY, f64::min);
        let last_ring: usize = pts
            .iter()
            .rev()
            .take_while(|q| (q[2] - lowest).abs() < 1e-9)
            .count();
        let flat_before = pts[..pts.len() - last_ring]
            .windows(2)
            .filter(|w| (w[0][2] - w[1][2]).abs() < 1e-12 && w[0][2] < 5.5)
            .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
            .sum::<f64>();
        assert!(flat_before < 1.0, "平らに回った長さ {flat_before}");
        // 食い込まない: 箱の壁からの横の距離は工具の半径以上
        // 上の縁の近くでは、ボールの丸みで横へ近づいてよい。半径より下だけ見る
        for q in pts.iter().filter(|q| q[2] < 6.0 - p.r()) {
            let dx = (0.0 - q[0]).max(q[0] - 10.0).max(0.0);
            let dy = (-5.0 - q[1]).max(q[1] - 5.0).max(0.0);
            assert!(dx.hypot(dy) >= p.r() - 0.02, "{q:?}");
        }
        // ボールの中心から箱と床までの距離は、半径から許しの SNUG を引いた以上(面の向きに食い込まない)
        for q in &pts {
            let c = [q[0], q[1], q[2] + p.r()];
            let k = [
                c[0].clamp(0.0, 10.0),
                c[1].clamp(-5.0, 5.0),
                c[2].clamp(0.0, 6.0),
            ];
            let to_box =
                ((c[0] - k[0]).powi(2) + (c[1] - k[1]).powi(2) + (c[2] - k[2]).powi(2)).sqrt();
            assert!(to_box.min(c[2]) >= p.r() - SNUG - 1e-6, "{q:?} {to_box}");
        }
        // 針のように跳ねない: 隣の点との高さの差は、横の動きの数倍まで(縦の壁でも 1 周 1 mm)
        for w in pts.windows(2) {
            let run = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            assert!(
                (w[1][2] - w[0][2]).abs() <= 2.0 * run + 0.05,
                "{:?} {:?}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn two_islands_are_two_spirals() {
        let p = part(true);
        let (lv, _) = run(&p, 30.0);
        let chains = chains(&lv, LINK * p.tool.diameter);
        // 箱 A は高さ 6、箱 B は 4。床の上の外の輪は箱ごとに分かれる(段ごとに 2 本)
        let closed: Vec<_> = chains.iter().filter(|c| c[0].closed).collect();
        assert_eq!(closed.len(), 2, "{}", closed.len());
        for c in &closed {
            assert!(c.len() >= 3, "段 {}", c.len());
        }
    }

    #[test]
    fn short_rings_go_round_more_than_once_to_keep_the_angle() {
        let p = part(false);
        let (_, tp) = run(&p, 0.5);
        let pts = cuts(&tp);
        // 下りる所の傾きは 0.5 deg を大きく超えない(輪の周は約 50 mm、1 周 1 mm 下りると 1.1 deg)。
        // 上の縁の近く(ボールが縁に乗る高さ)は、食い込まないよう面の高さに沿うので除く
        let edge = 6.0 - p.r() - 1.0;
        let worst = pts
            .windows(2)
            .filter(|w| w[1][2] < w[0][2] - 1e-9 && w[0][2] < edge)
            .map(|w| {
                let run = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
                ((w[0][2] - w[1][2]) / run.max(1e-9)).atan().to_degrees()
            })
            .filter(|a| *a < 45.0)
            .fold(0.0, f64::max);
        assert!(worst < 0.7, "{worst}");
    }

    #[test]
    fn blending_lands_on_the_next_ring_start() {
        let a = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let b = vec![[-1.0, -1.0], [11.0, -1.0], [11.0, 11.0], [-1.0, 11.0]];
        let heights = jc_cam_dropcutter::Heights::new([-5.0, -5.0], [15.0, 15.0], 0.5, -10.0, &[]);
        let field = DropField::new(heights, ToolLibrary::starter().tools[3].clone());
        let floor = |q: P2| field.drop_at(q);
        let q = blend(&a, 5.0, &b, 4.0, 30f64.to_radians(), 0.2, &floor);
        let last = q.last().unwrap();
        assert!((last[0] + 1.0).abs() < 1e-9 && (last[1] + 1.0).abs() < 1e-9);
        assert!((last[2] - 4.0).abs() < 1e-9);
        // 高さは下りるだけ
        assert!(q.windows(2).all(|w| w[1][2] <= w[0][2] + 1e-12));
        // 半分の所は、二つの輪の中ほど
        let mid = q[q.len() / 2];
        assert!((mid[2] - 4.5).abs() < 0.05, "{mid:?}");
    }
}
