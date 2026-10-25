//! 残り削り(取残し加工)の共通の部品。前の太い工具が届かなかった隅や細い溝だけを、今の細い工具で削る。
//!
//! 削る領域(工具の中心が入ってよい側)と前の工具から削り残しを見積もり、今の工具の輪のうち
//! 工具の円が削り残しにかかる部分だけを開いた線としてたどる。2D ポケットと 2D 輪郭が使う。
//! 前の工具が V・テーパーなら、高さごとに前の工具の中心が届いた所を先の半径だけ広げた所を
//! 削れたとみなす(削り残しを多めに見るので、空を削ることはあっても削り忘れはない)。

use crate::{Prepared, approach, invalid, opt_length, retract};
use jc_cam_geom::{Area, Loop, P2, offset_area};
use jc_cam_tool::{Tool, ToolKind, ToolLibrary};
use jc_core::{EvalCtx, EvalError, FeatureId, Field};
use jc_toolpath::{Move, Toolpath};
use serde::{Deserialize, Serialize};

/// 取残し加工の前の工具を数字で打つ欄。工具ライブラリに
/// 無い工具の後を削る時に使う。前の工具の番号(`rest_tool`)があればそちらを使う。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RestSize {
    /// 前の工具の直径
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_diameter: Option<String>,
    /// 前の工具のコーナー半径(空なら 0、平の工具)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_corner: Option<String>,
}

impl RestSize {
    pub fn exprs_mut(&mut self) -> impl Iterator<Item = &mut String> {
        self.rest_diameter
            .as_mut()
            .into_iter()
            .chain(self.rest_corner.as_mut())
    }

    /// 欄(前の工具の番号の欄の後に並べる)。
    pub fn fields() -> [Field; 2] {
        [
            Field::opt_expr("rest_diameter", "取残し加工: 前の工具の直径")
                .length()
                .help(
                    "例: 6.35 mm。工具ライブラリに無い工具の後を削る時に打つ。\
                     前の工具の番号があれば、そちらを使う",
                ),
            Field::opt_expr("rest_corner", "取残し加工: 前の工具のコーナー半径")
                .length()
                .help("例: 0.5 mm。空なら 0(平の工具)"),
        ]
    }

    /// 打った直径とコーナー半径の仮の工具(平。コーナー半径があればブルノーズ)。直径が空なら `None`。
    pub fn tool(&self, ctx: &EvalCtx<'_>) -> Result<Option<Tool>, EvalError> {
        let Some(d) = opt_length(ctx, &self.rest_diameter, "前の工具の直径")? else {
            if self.rest_corner.is_some() {
                return Err(invalid(
                    "前の工具のコーナー半径だけでは使えません。前の工具の直径も打ってください",
                ));
            }
            return Ok(None);
        };
        let r = opt_length(ctx, &self.rest_corner, "前の工具のコーナー半径")?.unwrap_or(0.0);
        if d <= 0.0 {
            return Err(invalid("前の工具の直径は 0 より大きくしてください"));
        }
        if r > d / 2.0 + 1e-9 {
            return Err(invalid(format!(
                "前の工具のコーナー半径({r} mm)は直径の半分以下にしてください"
            )));
        }
        // 形だけが要る(削り残しの見積もり)。条件と刃長は使わない
        let mut t = ToolLibrary::laser_tool();
        t.name = format!("前の工具(直径 {d} mm)");
        t.kind = if r > 0.0 {
            ToolKind::BullNose
        } else {
            ToolKind::FlatEnd
        };
        t.diameter = d;
        t.flute_length = 1e6;
        t.corner_radius = (r > 0.0).then_some(r);
        Ok(Some(t))
    }
}

/// 残り削りの前の工具: 番号 `n` があれば工具ライブラリの工具([`rest_tool`])、無ければ `size` の
/// 仮の工具(今の工具と合うかも同じに確かめる)。どちらも無ければ `None`。
/// 加工のデータ `d` が残り削りか(前の工具を番号か直径で打っている)。
pub fn is_rest_data(d: &serde_json::Value) -> bool {
    ["rest_tool", "rest_diameter"]
        .iter()
        .any(|k| d.get(*k).is_some_and(|v| !v.is_null()))
}

pub fn rest_tool_or_size(
    ctx: &EvalCtx<'_>,
    p: &Prepared,
    tools: FeatureId,
    n: Option<u32>,
    size: &RestSize,
) -> Result<Option<Tool>, EvalError> {
    match n {
        Some(n) => rest_tool(ctx, p, tools, n).map(Some),
        None => match size.tool(ctx)? {
            Some(t) => check_rest(p, t).map(Some),
            None => Ok(None),
        },
    }
}

/// 残り削りの前の工具(工具ライブラリ `tools` の番号 `n`)を読み、今の工具と合うか確かめる。
pub fn rest_tool(
    ctx: &EvalCtx<'_>,
    p: &Prepared,
    tools: FeatureId,
    n: u32,
) -> Result<Tool, EvalError> {
    check_rest(p, ToolLibrary::tool_from(ctx, tools, n)?)
}

/// 前の工具 `big` が、今の工具の残り削りに使えるか(今の工具の種類と、先の太さ)。
fn check_rest(p: &Prepared, big: Tool) -> Result<Tool, EvalError> {
    if !matches!(
        p.tool.kind,
        ToolKind::FlatEnd
            | ToolKind::VBit
            | ToolKind::TaperEnd
            | ToolKind::BullNose
            | ToolKind::TaperBall
    ) {
        return Err(invalid(format!(
            "残り削りに使えるのは平・ブルノーズ・V・テーパーの工具です(工具 {})",
            p.tool.name
        )));
    }
    if big.tip_radius() <= p.tool.tip_radius() + 1e-6 {
        return Err(invalid(format!(
            "残り削りの前の工具 {} の先は、今の工具 {} の先より太くしてください",
            big.name, p.tool.name
        )));
    }
    Ok(big)
}

/// 前の太い工具で削った後の削り残しと、今の工具がそこにかかるかの調べ方。
/// 削り残しは高さで変わることがある(前の工具が V・テーパーだと壁が斜めに残る)ので、
/// 高さの見本ごとに「前の工具が削った所」と「その高さでの今の工具の半径」を持つ。
struct Rest {
    /// 削る所(壁から残す厚さだけ内側、端は少し詰める)
    target: Vec<Area>,
    /// (その高さで前の工具が削った所、その高さでの今の工具の半径)
    samples: Vec<(Vec<Area>, f64)>,
}

impl Rest {
    fn new(area: &Area, leave: f64) -> Rest {
        Rest {
            target: offset_area(area, leave + 0.01),
            samples: Vec::new(),
        }
    }

    /// 前の工具の中心が壁から `center` 離れた所まで届き、そこから半径 `cut` で削った高さで、
    /// 今の工具の半径が `r`。平の工具なら `center` と `cut` は両方とも工具の半径。
    fn sample(mut self, area: &Area, leave: f64, center: f64, cut: f64, r: f64) -> Rest {
        let cut = offset_area(area, center + leave)
            .iter()
            .flat_map(|a| offset_area(a, -cut))
            .collect();
        self.samples.push((cut, r));
        self
    }

    fn left(&self, cut: &[Area], x: P2) -> bool {
        self.target.iter().any(|a| a.contains(x)) && !cut.iter().any(|a| a.contains(x))
    }

    /// 中心 `c` の工具が、どれかの高さで削り残しにかかるか。
    fn touched(&self, c: P2) -> bool {
        self.samples.iter().any(|(cut, r)| {
            self.left(cut, c)
                || (*r > 1e-9
                    && (0..24).any(|i| {
                        let t = i as f64 / 24.0 * std::f64::consts::TAU;
                        [0.5, 0.95].iter().any(|k| {
                            self.left(cut, [c[0] + k * r * t.cos(), c[1] + k * r * t.sin()])
                        })
                    }))
        })
    }
}

/// 輪のうち、工具が削り残しにかかる部分を開いた線の列にする(前後に 1 点ずつ伸ばす)。
fn rest_runs(l: &Loop, rest: &Rest) -> Vec<Vec<P2>> {
    let pts = l.flatten(2e-3);
    let n = pts.len();
    if n < 3 {
        return vec![];
    }
    // 点の間が長いと見落とすので、0.2 mm ごとに細かくする
    let mut fine: Vec<P2> = Vec::new();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let k = (((b[0] - a[0]).hypot(b[1] - a[1]) / 0.2).ceil() as usize).max(1);
        for j in 0..k {
            let t = j as f64 / k as f64;
            fine.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    let n = fine.len();
    let hit: Vec<bool> = fine.iter().map(|&c| rest.touched(c)).collect();
    if hit.iter().all(|h| *h) {
        let mut all = fine.clone();
        all.push(fine[0]);
        return vec![all];
    }
    // 当たらない点から回り始め、当たる点の続きを 1 本にする
    let first = hit.iter().position(|h| !h).expect("当たらない点がある");
    let mut runs = Vec::new();
    let mut cur: Vec<P2> = Vec::new();
    for j in 0..=n {
        let i = (first + j) % n;
        if hit[i] {
            if cur.is_empty() {
                cur.push(fine[(i + n - 1) % n]);
            }
            cur.push(fine[i]);
        } else if !cur.is_empty() {
            cur.push(fine[i]);
            runs.push(std::mem::take(&mut cur));
        }
    }
    runs
}

/// 残り削りの経路。細い工具の輪を内側から外側へ、削り残しにかかる所だけたどる。
/// 同じ線が続く段はまとめ、開いた線は行って戻る(持ち上げずに次の深さへ)。
pub fn rest_levels(
    tp: &mut Toolpath,
    p: &impl jc_toolpath_moves::Cutter,
    levels: &[(f64, Vec<Vec<P2>>)],
    ramp: Option<f64>,
) {
    let mut i = 0;
    while i < levels.len() {
        let mut j = i + 1;
        while j < levels.len() && levels[j].1 == levels[i].1 {
            j += 1;
        }
        let zs: Vec<f64> = levels[i..j].iter().map(|l| l.0).collect();
        // 前の段の高さまでは、この工具で削った後なので速めに下げてよい
        let above = if i == 0 { None } else { Some(levels[i - 1].0) };
        rest_level(tp, p, &levels[i].1, &zs, ramp, above);
        i = j;
    }
}

/// 閉じた線(始めと終わりが同じ点)か。
fn closed(run: &[P2]) -> bool {
    run.len() > 3
        && (run[0][0] - run[run.len() - 1][0]).hypot(run[0][1] - run[run.len() - 1][1]) < 1e-6
}

/// 閉じた線の上を、高さ `from` から `to` まで角度 `angle` で下りながらたどる(始めの点に戻る)。
fn ramp_on_run(run: &[P2], from: f64, to: f64, angle: f64, feed: f64) -> Vec<Move> {
    let per: f64 = run
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum();
    let drop = from - to;
    if per < 1e-3 || drop <= 1e-9 {
        return vec![];
    }
    let laps = ((drop / angle.tan()) / per - 1e-9).ceil().max(1.0) as usize;
    let total = laps as f64 * per;
    let mut out = Vec::new();
    let mut done = 0.0;
    for _ in 0..laps {
        for w in run.windows(2) {
            done += (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            out.push(Move::Linear {
                to: [w[1][0], w[1][1], from - drop * (done / total).min(1.0)],
                feed,
            });
        }
    }
    out
}

fn rest_level(
    tp: &mut Toolpath,
    p: &impl jc_toolpath_moves::Cutter,
    runs: &[Vec<P2>],
    levels: &[f64],
    ramp: Option<f64>,
    above: Option<f64>,
) {
    for run in runs {
        approach(tp, run[0]);
        let mut forward = true;
        let mut prev = above;
        for &z in levels {
            let pts: Vec<P2> = if forward {
                run.clone()
            } else {
                run.iter().rev().copied().collect()
            };
            match (ramp, closed(&pts)) {
                (Some(a), true) => {
                    let from = prev.unwrap_or(tp.retract_z).min(tp.retract_z);
                    if from < tp.retract_z {
                        tp.moves.push(Move::Linear {
                            to: [pts[0][0], pts[0][1], from],
                            feed: p.feed(),
                        });
                    }
                    tp.moves.extend(ramp_on_run(&pts, from, z, a, p.feed()));
                }
                _ => tp.moves.push(Move::Linear {
                    to: [pts[0][0], pts[0][1], z],
                    feed: p.plunge(),
                }),
            }
            tp.moves.extend(pts.iter().skip(1).map(|q| Move::Linear {
                to: [q[0], q[1], z],
                feed: p.feed(),
            }));
            prev = Some(z);
            forward = !forward;
        }
        retract(tp);
    }
}

/// 壁から `max` より遠い輪は作らない [`rings`](残り削りは壁の近くだけを使う)。
fn rings_near(area: &Area, first: f64, step: f64, r: f64, max: f64) -> Vec<Area> {
    let mut out = Vec::new();
    let mut cur = offset_area(area, first);
    let mut dist = first;
    while !cur.is_empty() && dist <= max + 1e-9 {
        let mut next = Vec::new();
        for a in &cur {
            let mut n = offset_area(a, step);
            if n.is_empty() && step > 0.95 * r && r > 1e-6 {
                n = offset_area(a, 0.95 * r);
            }
            next.extend(n);
        }
        out.extend(cur);
        cur = next;
        dist += step;
    }
    out
}

/// 残り削りの段ごとの線。`big` は前の工具、`cur` は今の工具。
/// `outer` が偽なら、輪の外周は使わず内周(穴の縁)だけをたどる(輪郭の外側を削る時は、
/// 領域の外周は輪郭から遠い仮の箱なので)。
#[allow(clippy::too_many_arguments)]
pub fn rest_plan(
    area: &Area,
    leave: f64,
    big: &Tool,
    cur: &Tool,
    stepover: f64,
    top: f64,
    levels: &[f64],
    outer: bool,
) -> Vec<(f64, Vec<Vec<P2>>)> {
    let mut out: Vec<(f64, Vec<Vec<P2>>)> = Vec::new();
    let mut last: Option<([i64; 2], Vec<Vec<P2>>)> = None;
    for &z in levels {
        let d = (top - z).max(0.0);
        // 今の工具の、壁の上端での半径。前の工具の中心が届いた壁からの距離
        let rc = cur.radius_at(d).unwrap_or(cur.radius());
        let rb = big.radius_at(d).unwrap_or(big.radius());
        let key = [(rc * 1e4).round() as i64, (rb * 1e4).round() as i64];
        if let Some((k, runs)) = &last
            && *k == key
        {
            out.push((z, runs.clone()));
            continue;
        }
        // 前の工具が平なら削り残しはどの高さでも同じで、今の工具は上面での半径 rc がいちばん太い。
        // 前の工具が V・テーパーなら、上面から先端までの 5 つの高さで見る
        let rest = if big.is_tapered() {
            (0..=4).fold(Rest::new(area, leave), |rest, k| {
                let h = d * k as f64 / 4.0;
                rest.sample(
                    area,
                    leave,
                    big.radius_at(h).unwrap_or(big.radius()),
                    big.tip_radius(),
                    cur.radius_at(d - h).unwrap_or(cur.radius()),
                )
            })
        } else {
            Rest::new(area, leave).sample(area, leave, rb, big.tip_radius(), rc)
        };
        let step = (2.0 * rc * stepover).max(0.05);
        let rs = rings_near(area, rc + leave, step, rc, rc + leave + rb + step);
        let runs: Vec<Vec<P2>> = rs
            .iter()
            .rev()
            .flat_map(|a| a.holes.iter().chain(outer.then_some(&a.outer)))
            .flat_map(|l| rest_runs(l, &rest))
            .collect();
        out.push((z, runs.clone()));
        last = Some((key, runs));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L 字の輪(反時計回り)。へこんだ角は (20, 20)。
    fn l_shape() -> Loop {
        Loop::from_points(&[
            [0.0, 0.0],
            [40.0, 0.0],
            [40.0, 20.0],
            [20.0, 20.0],
            [20.0, 40.0],
            [0.0, 40.0],
        ])
    }

    /// 6 mm の後に 2 mm で削る線の、全部の点。
    fn run_points(area: &Area, outer: bool) -> Vec<P2> {
        let lib = ToolLibrary::starter();
        let (big, cur) = (&lib.tools[0], &lib.tools[2]);
        rest_plan(area, 0.0, big, cur, 0.4, 0.0, &[-1.0], outer)
            .into_iter()
            .flat_map(|l| l.1.into_iter().flatten())
            .collect()
    }

    fn near(p: P2, c: P2) -> f64 {
        (p[0] - c[0]).hypot(p[1] - c[1])
    }

    #[test]
    fn outside_an_l_only_the_notch_is_left() {
        let area = Area {
            outer: Loop::from_points(&[[-30.0, -30.0], [70.0, -30.0], [70.0, 70.0], [-30.0, 70.0]]),
            holes: vec![l_shape()],
        }
        .normalized();
        let pts = run_points(&area, false);
        assert!(!pts.is_empty());
        // 6 mm の削り残しは角から 3 mm ほど。2 mm の工具の中心はそこから 1 mm ほど外まで
        assert!(pts.iter().all(|&p| near(p, [20.0, 20.0]) < 6.0), "{pts:?}");
        // 外周(仮の箱)を使うと、箱の隅も削り残しに見える
        let with_box = run_points(&area, true);
        assert!(with_box.iter().any(|&p| near(p, [-30.0, -30.0]) < 6.0));
    }

    #[test]
    fn inside_an_l_the_five_corners_are_left_but_not_the_notch() {
        let area = Area {
            outer: l_shape(),
            holes: vec![],
        };
        let pts = run_points(&area, true);
        let corners = [
            [0.0, 0.0],
            [40.0, 0.0],
            [40.0, 20.0],
            [20.0, 40.0],
            [0.0, 40.0],
        ];
        for c in corners {
            assert!(pts.iter().any(|&p| near(p, c) < 3.0), "隅 {c:?} を削らない");
        }
        assert!(
            pts.iter()
                .all(|&p| corners.iter().any(|&c| near(p, c) < 6.0)),
            "隅のほかも削る"
        );
    }
}
