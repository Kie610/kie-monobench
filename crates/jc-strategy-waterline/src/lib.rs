//! 3D の等高線加工(`cam.waterline@1`)。高さを `step` ずつ下げながら、工具がその高さまで
//! 下りられる所の境目を一周ずつ回る。走査線の苦手な急な壁(縦の面、急な斜面、縦の
//! フィレット)の仕上げに使う。
//!
//! 境目は `jc-cam-dropcutter` の、全節点で工具を下ろした高さの等高線から作る。島の周りは
//! 時計回り、穴の中は反時計回り(主軸が時計回りの時のダウンカット)。
//!
//! 次の輪が近く、今の高さのまま横へ動いても部品を削らなければ、工具を上げずに移る。
//! 下の段へは、次の輪に沿って `ramp_angle` の傾きで下りてから(輪が短ければらせんに
//! なる)、その高さで一周する。移れない時は安全高さへ上げ、1 つ上の段の高さまで真下に
//! 下げてから、同じように輪に沿って下りる。
//!
//! 3D に共通の欄([`Limits`]): 径方向に残す厚さは工具を太らせて下ろす。加工の範囲の外と
//! 傾斜の範囲の外の点は削らず、輪をその所で切った開いた線をたどる(開いた線は向きを変えず、
//! 始まりで下りる)。順(`order`)を下から上にすると、低い段から順に回り、上の段へは
//! 上がってから移る。
//!
//! 同じ段の輪を、1 周ごとに斜めに下りながらつなぐランプ(`cam.ramp3d@1`)は [`ramp`]。

use jc_cam_dropcutter::limits::{boundary_fields, leave_radial_field, slope_fields};
use jc_cam_dropcutter::{Cutter, DropField, Heights, Limits, Resolved};
use jc_cam_geom::Loop;
use jc_cam_setup::{
    OpBase, Planner, Prepared, StrategyInput, StrategyOutput, approach, expr_refs, invalid, length,
    loop_moves, plan_error, prepare, ramp_angle, ramp_moves,
};
use jc_cam_tool::ToolKind;
use jc_core::Field;
use jc_core::{EvalCtx, EvalError, Feature, InputRef, Outputs, Registry};
use jc_toolpath::{Move, Toolpath};
use serde::{Deserialize, Serialize};

pub mod ramp;
pub use ramp::{RAMP3D, Ramp3d};

pub const WATERLINE: &str = "cam.waterline@1";

/// 段の間を輪に沿って下りる角度の既定(度)。
pub const DEFAULT_RAMP_DEG: f64 = 3.0;

/// 工具を下げたまま次の輪へ移る、つなぎの長さの上限(工具の直径の倍数)。
/// 削り残しのストックの中を長く送らないよう、短い所だけにする。
const STAY_DOWN: f64 = 4.0;

/// 範囲の外の点が続く所を、輪の上をなぞってつなぐ長さの上限(工具の直径の倍数)。
const BRIDGE: f64 = 1.0;

/// 段を回る順。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// 上から下へ
    #[default]
    TopDown,
    /// 下から上へ
    BottomUp,
}

impl Order {
    pub fn is_top_down(&self) -> bool {
        *self == Order::TopDown
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waterline {
    #[serde(flatten)]
    pub base: OpBase,
    /// 部品の面に残す厚さ(軸方向。上向きに測る)
    #[serde(default = "zero")]
    pub leave: String,
    /// 段の間隔(省くと工具の切り込み)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// 部品の高さを調べる格子の幅
    #[serde(default = "resolution")]
    pub resolution: String,
    /// 部品のいちばん高い所から、どこまで下を回るか(省くと部品の底かストックの底まで)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
    /// 段の間を輪に沿って下りる角度(例: `3 deg`)。空なら 3 deg
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_angle: Option<String>,
    /// 径方向に残す厚さ・加工の範囲・傾斜の範囲
    #[serde(flatten)]
    pub limits: Limits,
    /// 段を回る順
    #[serde(default, skip_serializing_if = "Order::is_top_down")]
    pub order: Order,
    /// 入る位置(作業座標系の XY)。段ごとに、ここに最も近い輪の、最も近い所から回る。空なら自動
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<[f64; 2]>,
}

pub(crate) fn zero() -> String {
    "0 mm".into()
}

pub(crate) fn resolution() -> String {
    "0.1 mm".into()
}

/// 1 本の線(先端の高さ、工具の中心が通る点の列、閉じた輪か)。
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub z: f64,
    pub pts: Vec<[f64; 2]>,
    pub closed: bool,
}

/// 段ごとの輪と、輪の間を動く時の検査に使うもの。
pub struct Levels {
    pub rings: Vec<Ring>,
    /// 段の間隔
    pub spacing: f64,
    pub field: DropField,
    pub leave: f64,
}

/// 経路の計算の設定(式を数にしたもの)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opts {
    /// 軸方向に残す厚さ
    pub leave: f64,
    pub cell: f64,
    pub step: f64,
    /// 回る底(先端の高さ)
    pub bottom: f64,
    pub top: f64,
    pub ramp: f64,
    pub order: Order,
    pub limits: Resolved,
    /// 入る位置(作業座標系の XY)。段ごとに、ここに近い所から回る
    pub entry: Option<[f64; 2]>,
}

/// `a` から `b` まで、工具の先端を高さ `z` に保って動いても部品を削らない(`leave` を残す)か。
fn clear(field: &DropField, a: [f64; 2], b: [f64; 2], z: f64, leave: f64) -> bool {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let n = (len / (field.heights.cell * 0.5)).ceil().max(1.0) as usize;
    (0..=n).all(|k| {
        let t = k as f64 / n as f64;
        let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        field.drop_at(q) <= z - leave + 1e-6
    })
}

/// 閉じた輪を、範囲と傾斜の中の開いた線に分ける。全部が中なら閉じたまま返す。
fn limit_ring(
    field: &DropField,
    lim: &Resolved,
    z: f64,
    ring: Vec<[f64; 2]>,
    bridge: f64,
    min_length: f64,
) -> Vec<Ring> {
    // 点の間が長いと見落とすので、格子の幅の 2 倍ごとに細かくする
    let step = 2.0 * field.heights.cell;
    let n = ring.len();
    let mut fine: Vec<[f64; 2]> = Vec::new();
    for k in 0..n {
        let (a, b) = (ring[k], ring[(k + 1) % n]);
        let m = (((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil() as usize).max(1);
        for j in 0..m {
            let t = j as f64 / m as f64;
            fine.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    let n = fine.len();
    let mut keep: Vec<bool> = fine.iter().map(|&q| lim.keep(field, q)).collect();
    if keep.iter().all(|k| *k) {
        return vec![Ring {
            z,
            pts: ring,
            closed: true,
        }];
    }
    let Some(first) = keep.iter().position(|k| *k) else {
        return Vec::new();
    };
    let dist = |a: usize, b: usize| (fine[b][0] - fine[a][0]).hypot(fine[b][1] - fine[a][1]);
    // 外の点が短く続くだけの所は、輪の上をなぞってつなぐ
    let mut k = 0;
    while k < n {
        let i = (first + k) % n;
        if keep[i] {
            k += 1;
            continue;
        }
        let s = k;
        while k < n && !keep[(first + k) % n] {
            k += 1;
        }
        let len: f64 = (s - 1..k)
            .map(|m| dist((first + m) % n, (first + m + 1) % n))
            .sum();
        if len <= bridge {
            for m in s..k {
                keep[(first + m) % n] = true;
            }
        }
    }
    if keep.iter().all(|k| *k) {
        return vec![Ring {
            z,
            pts: ring,
            closed: true,
        }];
    }
    // 外の点から回り始め、中の点の続きを 1 本にする
    let start = keep.iter().position(|k| !*k).expect("外の点がある");
    let mut out = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    for j in 0..=n {
        let i = (start + j) % n;
        if keep[i] && j < n {
            cur.push(fine[i]);
        } else if !cur.is_empty() {
            let pts = jc_cam_dropcutter::simplify(
                &cur.iter().map(|q| [q[0], q[1], 0.0]).collect::<Vec<_>>(),
            )
            .into_iter()
            .map(|q| [q[0], q[1]])
            .collect::<Vec<_>>();
            let len: f64 = pts
                .windows(2)
                .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
                .sum();
            if len >= min_length && pts.len() >= 2 {
                out.push(Ring {
                    z,
                    pts,
                    closed: false,
                });
            }
            cur.clear();
        }
    }
    out
}

/// 段ごとの輪(式を数にした後の計算)。
pub fn levels(p: &Planner, o: &Opts) -> Result<Levels, EvalError> {
    let (cell, leave, step) = (o.cell, o.leave, o.step);
    let (top, bottom) = (o.top, o.bottom);
    let (m0, m1) = (p.model.min, p.model.max);
    let r = p.r() + o.limits.leave_radial;
    let pad = r + 3.0 * cell;
    let heights = Heights::new(
        [m0[0] - r - pad, m0[1] - r - pad],
        [m1[0] + r + pad, m1[1] + r + pad],
        cell,
        p.info.stock_min[2].min(bottom) - 1.0,
        &p.model.triangles,
    );
    let field = DropField::new(heights, Cutter::new(p.tool.clone(), o.limits.leave_radial));
    let n = ((top - bottom) / step - 1e-9).ceil().max(1.0) as usize;
    // 段ごとの等高線は互いに関係しないので、スレッドに分ける
    let mut zs: Vec<f64> = (1..=n)
        .map(|k| top - (top - bottom) * k as f64 / n as f64)
        .collect();
    if !o.order.is_top_down() {
        zs.reverse();
    }
    // 工具の先端の高さが z、面には leave だけ残す
    let found = jc_cam_dropcutter::par_map(&zs, |&z| field.contours(z - leave, 4.0 * cell));
    let bridge = BRIDGE * p.tool.diameter;
    let mut out = Vec::new();
    for (&z, rings) in zs.iter().zip(found) {
        for ring in rings {
            if o.limits.limits_anything() {
                out.extend(limit_ring(&field, &o.limits, z, ring, bridge, 4.0 * cell));
            } else {
                out.push(Ring {
                    z,
                    pts: ring,
                    closed: true,
                });
            }
        }
    }
    if out.is_empty() {
        return Err(invalid(if o.limits.limits_anything() {
            "回る所がありません(加工の範囲か傾斜の範囲に入る壁がありません)"
        } else {
            "回る所がありません(部品の壁が見つかりません)"
        }));
    }
    Ok(Levels {
        rings: out,
        spacing: (top - bottom) / n as f64,
        field,
        leave,
    })
}

/// 等高線の経路の動き(`tp` に足す)。
pub fn moves(p: &Planner, lv: &Levels, o: &Opts, tp: &mut Toolpath) {
    let rings = &lv.rings;
    let reach = STAY_DOWN * p.tool.diameter;
    let (feed, plunge) = (p.cutting.feed, p.cutting.plunge);
    let up = !o.order.is_top_down();
    // 段ごとに、今いる所から近い輪を順に回る
    let mut here = [p.model.min[0], p.model.min[1]];
    // 工具を下げて輪を回り終えた時の先端の高さ(上げてあれば None)
    let mut down: Option<f64> = None;
    let mut k = 0;
    while k < rings.len() {
        let z = rings[k].z;
        let mut level: Vec<&Ring> = rings[k..].iter().take_while(|r| r.z == z).collect();
        k += level.len();
        // 入る位置があれば、段ごとにそこに近い所から回る(継ぎ目を揃える)
        let mut aim = o.entry.unwrap_or(here);
        while !level.is_empty() {
            let near = |q: [f64; 2]| (q[0] - aim[0]).hypot(q[1] - aim[1]);
            let d = |q: [f64; 2]| (q[0] - here[0]).hypot(q[1] - here[1]);
            // 輪は近い点から、開いた線は始まりから(向きを変えない)
            let (best, start) = level
                .iter()
                .enumerate()
                .map(|(n, ring)| {
                    let s = if ring.closed {
                        (0..ring.pts.len())
                            .min_by(|&a, &b| near(ring.pts[a]).total_cmp(&near(ring.pts[b])))
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    (n, s, near(ring.pts[s]))
                })
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(n, s, _)| (n, s))
                .expect("輪がある");
            let ring = level.swap_remove(best);
            let s = ring.pts[start];
            // 近くて削らずに動ければ、今の高さのまま横へ(下から上の順なら、先に上がってから)。
            // でなければ上げて移り、1 つ上の段の高さまで真下に下げる
            let from = match down {
                Some(zc)
                    if up && zc < z && d(s) <= reach && clear(&lv.field, here, s, z, lv.leave) =>
                {
                    tp.moves.push(Move::Linear {
                        to: [here[0], here[1], z],
                        feed,
                    });
                    if d(s) > 1e-9 {
                        tp.moves.push(Move::Linear {
                            to: [s[0], s[1], z],
                            feed,
                        });
                    }
                    z
                }
                Some(zc)
                    if (!up || zc >= z)
                        && d(s) <= reach
                        && clear(&lv.field, here, s, zc, lv.leave) =>
                {
                    if d(s) > 1e-9 {
                        tp.moves.push(Move::Linear {
                            to: [s[0], s[1], zc],
                            feed,
                        });
                    }
                    zc
                }
                _ => {
                    approach(tp, s);
                    let e = (z + lv.spacing).min(tp.retract_z);
                    tp.moves.push(Move::Linear {
                        to: [s[0], s[1], e],
                        feed: plunge,
                    });
                    e
                }
            };
            if ring.closed {
                let l = Loop::from_points(
                    &(0..ring.pts.len())
                        .map(|m| ring.pts[(start + m) % ring.pts.len()])
                        .collect::<Vec<_>>(),
                );
                // 輪に沿って z まで下り、z で一周する
                let m = tp.moves.len();
                tp.moves
                    .extend(ramp_moves(&l, from, z, o.ramp, feed, plunge));
                tp.mark(m, jc_toolpath::Role::Entry);
                tp.moves.extend(loop_moves(&l, z, feed));
                here = s;
            } else {
                // 開いた線は始まりで真下に下り、終わりまでたどる
                if from > z + 1e-9 {
                    tp.moves.push(Move::Linear {
                        to: [s[0], s[1], z],
                        feed: plunge,
                    });
                }
                tp.moves.extend(ring.pts[1..].iter().map(|q| Move::Linear {
                    to: [q[0], q[1], z],
                    feed,
                }));
                here = *ring.pts.last().expect("点がある");
            }
            aim = here;
            down = Some(z);
        }
    }
    jc_cam_setup::retract(tp);
}

impl Waterline {
    /// 式を数にした設定。
    fn opts(&self, ctx: &EvalCtx<'_>, p: &mut Prepared) -> Result<Opts, EvalError> {
        let leave = length(ctx, &self.leave, "残し量")?;
        let cell = length(ctx, &self.resolution, "格子の幅")?;
        if !(0.02..=0.5).contains(&cell) {
            return Err(invalid("格子の幅は 0.02〜0.5 mm にしてください"));
        }
        let step = match &self.step {
            Some(s) => length(ctx, s, "段の間隔")?,
            None => p.cutting.stepdown,
        };
        if step <= 0.0 {
            return Err(invalid("段の間隔は 0 より大きくしてください"));
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
            return Err(invalid("回る高さの幅が段の間隔より狭いです"));
        }
        p.check_depth(p.info.stock_max[2], bottom)?;
        let ramp = ramp_angle(ctx, &self.ramp_angle)?.unwrap_or(DEFAULT_RAMP_DEG.to_radians());
        let limits = self.limits.resolve(ctx, p, cell.max(0.05))?;
        Ok(Opts {
            leave,
            cell,
            step,
            bottom,
            top,
            ramp,
            order: self.order,
            limits,
            entry: self.entry,
        })
    }
}

/// 戦略の純関数: 解いた入力(`params` は [`Opts`])だけから経路を作る。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    let p = Planner::new(input);
    let o: Opts = input.params()?;
    let lv = levels(&p, &o).map_err(plan_error)?;
    let mut tp = p.toolpath();
    moves(&p, &lv, &o, &mut tp);
    Ok(p.output(tp))
}

impl Feature for Waterline {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs([&self.leave, &self.resolution])?);
        for e in [&self.step, &self.depth, &self.ramp_angle]
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
                "等高線の加工にはエンドミル(平かボール)を使ってください",
            ));
        }
        let o = self.opts(ctx, &mut p)?;
        let input = p.input(ctx.feature_id(), o, Default::default());
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
        Field::opt_expr("step", "段の間隔")
            .length()
            .help("空なら工具の切り込み"),
        Field::expr("resolution", "格子の幅").length(),
        Field::opt_expr("depth", "回る深さ")
            .length()
            .help("空なら部品かストックの底まで"),
        Field::opt_expr("ramp_angle", "段の間を下りる角度")
            .angle()
            .help("例: 3 deg。空なら 3 deg"),
        Field::choice(
            "order",
            "段の順",
            &[("top_down", "上から下へ"), ("bottom_up", "下から上へ")],
        )
        .help("下から上へは、低い段から回り、上の段へは上がってから移る"),
    ];
    fields.extend(slope_fields());
    fields.extend(boundary_fields());
    fields.extend(jc_cam_setup::height_fields());
    reg.register_described(
        jc_cam_setup::operation(
            WATERLINE,
            "等高線(3D)",
            "縦の壁や急な斜面を高さごとに一周して仕上げる",
            fields,
        )
        .icon("waterline"),
        |d| Ok(Box::new(jc_cam_setup::parse::<Waterline>(d)?)),
    );
    ramp::register(reg);
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> jc_cam_setup::StrategyModule {
    use jc_cam_setup::{Strategy, StrategyModule, fitted, to_data};
    use jc_cam_tool::ToolKind;
    StrategyModule::new(
        "jc-strategy-waterline",
        register,
        vec![
            Strategy::new(WATERLINE, "cam.add.waterline", "等高線(3D)", |p| {
                to_data(Waterline {
                    base: fitted(p.with_tool(ToolKind::BallEnd)),
                    leave: "0 mm".into(),
                    step: Some("0.5 mm".into()),
                    resolution: "0.1 mm".into(),
                    depth: None,
                    ramp_angle: None,
                    entry: None,
                    order: Default::default(),
                    limits: Default::default(),
                })
            })
            .order(23)
            .menu("3D")
            .icon("waterline")
            .aliases("とうこうせん waterline 3d")
            .hint("縦の壁や急な斜面を、0.5 mm ごとの高さで一周ずつ回る(ボールエンドミルがあればそれを使う)")
            .entry("段ごとに、クリックした所に最も近い所から回り始める(継ぎ目が揃う)"),
            Strategy::new(RAMP3D, "cam.add.ramp3d", "ランプ(3D)", |p| {
                to_data(Ramp3d::new(fitted(p.with_tool(ToolKind::BallEnd))))
            })
            .order(25)
            .menu("3D")
            .icon("ramp3d")
            .not_front()
            .aliases("らんぷ ramp 3d けいしゃ かべ らせん spiral contour")
            .hint("縦の壁や急な斜面を、1 周 0.5 mm ずつ斜めに下りながら途切れずに仕上げる(ボールエンドミルがあればそれを使う)")
            .no_reuse(),
        ],
    )
}

#[cfg(test)]
mod tests;
