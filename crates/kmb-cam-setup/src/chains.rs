//! スケッチなどの線を、機械に置いた向き(作業座標系)の折れ線の列にする。輪郭加工に使う。

use crate::{Prepared, SetupInfo, invalid};
use kmb_cam_geom::{Loop, P2, fit_arcs};
use kmb_core::{Curves, EdgeInfo, EvalCtx, EvalError, FeatureId, Ref};

type P3 = [f64; 3];

/// 円弧を折れ線にする時の誤差(mm)。
const CHORD: f64 = 0.005;

/// 線のつながり(1 本にまとめた折れ線)。閉じていれば輪にもできる。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Chain {
    /// 点の列(閉じていれば最後の点は最初と同じ)
    pub pts: Vec<P2>,
    pub closed: bool,
    /// 初めの線を、描いた向きと逆にたどる
    pub reversed: bool,
}

impl Chain {
    /// 閉じた線を輪(直線と円弧)にする。開いていれば `None`。
    pub fn to_loop(&self) -> Option<Loop> {
        if !self.closed || self.pts.len() < 4 {
            return None;
        }
        let l = fit_arcs(&self.pts[..self.pts.len() - 1], CHORD);
        (l.verts.len() >= 2).then_some(l)
    }
}

/// 機能 `id` の線(出力 `curves`)を読み、作業座標系の高さと、つながりの列を返す。
/// 線の平面は機械に置いた向きで水平でなければならない。補助線は含まない。
pub fn sketch_chains(
    ctx: &EvalCtx<'_>,
    p: &Prepared,
    id: FeatureId,
) -> Result<(f64, Vec<Chain>), EvalError> {
    let curves = Curves::from_value(ctx.output(id, "curves")?)
        .ok_or_else(|| invalid(format!("{id} は線を出しません")))?;
    chains_of(&curves, &p.info)
}

/// 線を出す機能 `id` の線(つなぐ前の 1 本ずつ)を、機械に置いた向きの作業座標系の折れ線にする。
/// 線の平面の高さも返す。彫刻・スロット・レーザーが使う。
pub fn read_lines(
    ctx: &EvalCtx<'_>,
    p: &Prepared,
    id: FeatureId,
) -> Result<(Vec<Vec<P2>>, f64), EvalError> {
    let curves = Curves::from_value(ctx.output(id, "curves")?)
        .ok_or_else(|| invalid(format!("{id} は線を出しません")))?;
    if curves.segments.is_empty() {
        return Err(invalid("なぞる線がありません(補助線は使いません)"));
    }
    let (z, lines) = lines_of(&curves, &p.info)?;
    Ok((lines, z))
}

/// 線 `curves` を、セットアップ `info` の座標(作業座標系)の高さと、つながりの列にする。
pub fn chains_of(curves: &Curves, info: &SetupInfo) -> Result<(f64, Vec<Chain>), EvalError> {
    if curves.segments.is_empty() {
        return Err(invalid("使える線がありません(補助線は使いません)"));
    }
    let (z, lines) = lines_of(curves, info)?;
    Ok((z, chains(lines)))
}

/// 線 `curves` の 1 本ずつを、機械に置いた向き(セットアップの回し方 [`SetupInfo::place`])の
/// 作業座標系の折れ線にする。線の平面の高さも返す。平面が水平でなければ誤り。
/// 輪郭加工も彫刻もこれを通すので、裏返しと向きの回し方が同じに当たる。
pub fn lines_of(curves: &Curves, info: &SetupInfo) -> Result<(f64, Vec<Vec<P2>>), EvalError> {
    let place = |q: [f64; 3]| {
        let w = info.place(q);
        [
            w[0] - info.origin[0],
            w[1] - info.origin[1],
            w[2] - info.origin[2],
        ]
    };
    let mut z: Option<f64> = None;
    let mut lines: Vec<Vec<P2>> = Vec::new();
    for s in &curves.segments {
        let mut pts = s.polyline(s.steps_per_turn(CHORD));
        if matches!(s, kmb_core::Segment::Circle { .. }) {
            pts.push(pts[0]);
        } else {
            pts.push(s.end_point());
        }
        let mut line = Vec::with_capacity(pts.len());
        for q in pts {
            let w = place(curves.plane.to_world(q));
            match z {
                None => z = Some(w[2]),
                Some(z0) if (w[2] - z0).abs() > 1e-4 => {
                    return Err(invalid(
                        "線の平面が、機械に置いた向きで水平ではありません(上か下を向いた面に描いてください)",
                    ));
                }
                _ => {}
            }
            line.push([w[0], w[1]]);
        }
        if line.len() >= 2 {
            lines.push(line);
        }
    }
    let z = z.ok_or_else(|| invalid("使える線がありません"))?;
    Ok((z, lines))
}

/// 線を、つながるものを 1 本にまとめた折れ線の列にする(近い端から順に)。
pub fn chains(mut lines: Vec<Vec<P2>>) -> Vec<Chain> {
    let near = |a: P2, b: P2| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-4;
    let mut out = Vec::new();
    let mut at: P2 = [0.0, 0.0];
    while !lines.is_empty() {
        let (i, rev) = lines
            .iter()
            .enumerate()
            .flat_map(|(i, l)| [(i, false, l[0]), (i, true, l[l.len() - 1])])
            .min_by(|a, b| {
                let d = |p: P2| (p[0] - at[0]).hypot(p[1] - at[1]);
                d(a.2).total_cmp(&d(b.2))
            })
            .map(|(i, r, _)| (i, r))
            .expect("空でない");
        let mut chain = lines.swap_remove(i);
        if rev {
            chain.reverse();
        }
        loop {
            let end = chain[chain.len() - 1];
            let Some((j, rev)) = lines.iter().enumerate().find_map(|(j, l)| {
                if near(l[0], end) {
                    Some((j, false))
                } else if near(l[l.len() - 1], end) {
                    Some((j, true))
                } else {
                    None
                }
            }) else {
                break;
            };
            let mut next = lines.swap_remove(j);
            if rev {
                next.reverse();
            }
            chain.extend(next.into_iter().skip(1));
        }
        let closed = chain.len() > 2 && near(chain[0], chain[chain.len() - 1]);
        at = chain[chain.len() - 1];
        out.push(Chain {
            pts: chain,
            closed,
            reversed: rev,
        });
    }
    out
}

/// 辺 `e` を折れ線にする(円の辺は誤差 [`CHORD`] の細かさで)。
pub fn edge_points(e: &EdgeInfo) -> Vec<P3> {
    let segs = match e.arc {
        Some(a) if a.radius > CHORD => {
            let step = 2.0 * (1.0 - CHORD / a.radius).clamp(-1.0, 1.0).acos();
            (a.sweep.abs() / step.max(1e-3)).ceil().max(2.0) as usize
        }
        _ => 1,
    };
    e.polyline(segs)
}

/// 選んだ辺の折れ線(セットアップの座標)。カーネルの辺を読むのはホストの役目なので、戦略の crate
/// でなくここに置く(戦略はカーネルに触らない。トレースが使う)。
pub fn edge_lines(
    ctx: &EvalCtx<'_>,
    p: &Prepared,
    picks: &[Ref],
) -> Result<Vec<Vec<P3>>, EvalError> {
    if picks.is_empty() {
        return Ok(vec![]);
    }
    let kernel = ctx.kernel();
    let mut all: Vec<EdgeInfo> = Vec::new();
    for h in std::iter::once(&p.info.body).chain(&p.info.more_bodies) {
        all.extend(kernel.edges(*h).unwrap_or_default());
        all.extend(kernel.arc_edges(*h).unwrap_or_default());
        all.extend(kernel.curve_edges(*h).unwrap_or_default());
    }
    let mut out = Vec::new();
    for r in picks {
        let Ref::Edge { faces, .. } = r else {
            return Err(invalid("なぞる辺には、辺を選んでください"));
        };
        let same = |e: &&EdgeInfo| {
            (e.faces[0] == faces[0] && e.faces[1] == faces[1])
                || (e.faces[0] == faces[1] && e.faces[1] == faces[0])
        };
        let found: Vec<&EdgeInfo> = all.iter().filter(same).collect();
        if found.is_empty() {
            return Err(invalid(format!(
                "選んだ辺({} と {} の境目)が、セットアップのボディに見つかりません(形が変わったか、別のボディの辺です)",
                faces[0], faces[1]
            )));
        }
        for e in found {
            let pts: Vec<P3> = edge_points(e)
                .into_iter()
                .map(|q| p.info.to_setup(q))
                .collect();
            if pts.len() >= 2 {
                out.push(pts);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_lines_join_and_a_closed_one_becomes_a_loop() {
        let c = chains(vec![
            vec![[10.0, 0.0], [10.0, 10.0]],
            vec![[0.0, 0.0], [10.0, 0.0]],
            vec![[50.0, 50.0], [60.0, 50.0]],
            vec![[10.0, 10.0], [0.0, 10.0]],
            vec![[0.0, 10.0], [0.0, 0.0]],
        ]);
        assert_eq!(c.len(), 2);
        assert!(c[0].closed);
        let l = c[0].to_loop().expect("閉じている");
        assert!((l.signed_area().abs() - 100.0).abs() < 1e-6);
        assert!(!c[1].closed);
        assert!(c[1].to_loop().is_none());
    }

    #[test]
    fn lines_follow_the_turn_of_the_setup() {
        // 上から見て 90 度回したセットアップでは、X 方向の線が Y 方向になる(彫刻と同じ置き方)
        let mut info = crate::testing::prepared(
            &crate::testing::mesh_of(&[]),
            [-50.0, -50.0, 0.0],
            [50.0, 50.0, 10.0],
            kmb_cam_tool::ToolLibrary::starter().tools[0].clone(),
        )
        .info;
        info.rot = Some(crate::placement::turn_matrix(90.0));
        let curves = Curves {
            plane: kmb_core::Plane::XY,
            segments: vec![kmb_core::Segment::Line {
                a: [10.0, 0.0],
                b: [20.0, 0.0],
            }],
        };
        let (z, c) = chains_of(&curves, &info).unwrap();
        assert!(z.abs() < 1e-9);
        let near = |p: P2, q: P2| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-9;
        assert!(near(c[0].pts[0], [0.0, 10.0]), "{:?}", c[0].pts);
        assert!(
            near(*c[0].pts.last().unwrap(), [0.0, 20.0]),
            "{:?}",
            c[0].pts
        );
    }
}
