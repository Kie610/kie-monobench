//! ラックの輪郭。ピッチ線を X 軸、歯先を Y の正の側とし、始点から歯を円ピッチごとに並べる。
//! 歯面はピッチ線の法線から圧力角だけ傾く直線。丸みは隣り合う直線に接する円弧にする。

use super::{Outline, Params};
use crate::tooth::{P2, arc, edge};
use kmb_core::{ProfileEdge, Segment};
use std::f64::consts::PI;

/// 全長に収まる完全な歯の数。端の余りには歯を置かない。
pub fn teeth_for_length(module: f64, length: f64) -> Result<u32, String> {
    if !module.is_finite() || module <= 0.0 {
        return Err("モジュールは有限の 0 より大きい数にしてください".into());
    }
    let pitch = PI * module;
    let count = length / pitch;
    if !length.is_finite() || count < 1.0 || count >= u32::MAX as f64 {
        return Err(format!(
            "ラックの長さは円ピッチ {pitch:.3} mm 以上の有限の値にしてください"
        ));
    }
    // 式で歯数 × 円ピッチと打った時の丸め誤差だけを吸収する。
    let nearest = count.round();
    let count = if (count - nearest).abs() < 8.0 * f64::EPSILON * count {
        nearest
    } else {
        count.floor()
    };
    Ok(count as u32)
}

#[derive(Clone)]
struct Corner {
    at: P2,
    radius: f64,
    name: String,
}

fn unit(a: P2, b: P2) -> (P2, f64) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0].hypot(d[1]);
    ([d[0] / length, d[1] / length], length)
}

/// 丸みの両端と、角で切り取る直線の長さ。
fn trim(prev: &Corner, at: &Corner, next: &Corner) -> (f64, P2, P2, f64) {
    let (a, _) = unit(prev.at, at.at);
    let (b, _) = unit(at.at, next.at);
    let turn = (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]);
    let cut = at.radius * (turn.abs() / 2.0).tan();
    (cut, a, b, turn)
}

pub fn outline(p: &Params) -> Result<Outline, String> {
    let m = p.module;
    if !m.is_finite() || m <= 0.0 {
        return Err("モジュールは有限の 0 より大きい数にしてください".into());
    }
    if p.teeth == 0 {
        return Err("ラックの歯数は 1 以上にしてください".into());
    }
    if !p.pressure_angle.is_finite()
        || p.pressure_angle <= 0.0
        || p.pressure_angle >= 45f64.to_radians()
    {
        return Err("圧力角は 0 deg より大きく 45 deg より小さくしてください".into());
    }
    if p.profile_shift != 0.0 {
        return Err("ラックの転位係数は 0 にしてください".into());
    }
    if !p.tip_round.is_finite()
        || !p.root_round.is_finite()
        || p.tip_round < 0.0
        || p.root_round < 0.0
    {
        return Err("歯先と歯底の丸みは有限の 0 以上の数にしてください".into());
    }
    let pitch = PI * m;
    let length = p.rack_length.unwrap_or(pitch * p.teeth as f64);
    if !length.is_finite() || length < pitch * p.teeth as f64 - pitch * 1e-12 {
        return Err("ラックの長さに指定した歯数が入りません".into());
    }
    let back = p.rack_back.unwrap_or(2.0 * m);
    if !back.is_finite() || back <= 0.0 {
        return Err("背の厚さは有限の 0 より大きい数にしてください".into());
    }
    if [
        pitch,
        pitch * p.teeth as f64,
        m * p.tip_round,
        m * p.root_round,
        1.25 * m + back,
    ]
    .iter()
    .any(|v| !v.is_finite())
    {
        return Err("ラックの寸法か丸みが大きすぎます。有限の寸法になる値にしてください".into());
    }
    let root_y = -1.25 * m;
    let tip_y = m;
    let root_half = pitch / 4.0 + 1.25 * m * p.pressure_angle.tan();
    let tip_half = pitch / 4.0 - m * p.pressure_angle.tan();
    if root_half >= pitch / 2.0 || tip_half <= 0.0 {
        return Err("圧力角が大きく歯溝が無くなります。圧力角を小さくしてください".into());
    }
    let corner = |at, radius, name: String| Corner { at, radius, name };
    let mut corners = vec![corner([0.0, root_y], 0.0, "start".into())];
    for i in 0..p.teeth {
        let center = (i as f64 + 0.5) * pitch;
        corners.extend([
            corner(
                [center - root_half, root_y],
                p.root_round * m,
                format!("t{i}:lfillet"),
            ),
            corner(
                [center - tip_half, tip_y],
                p.tip_round * m,
                format!("t{i}:lround"),
            ),
            corner(
                [center + tip_half, tip_y],
                p.tip_round * m,
                format!("t{i}:rround"),
            ),
            corner(
                [center + root_half, root_y],
                p.root_round * m,
                format!("t{i}:rfillet"),
            ),
        ]);
    }
    corners.extend([
        corner([length, root_y], 0.0, "end".into()),
        corner([length, root_y - back], 0.0, "back_end".into()),
        corner([0.0, root_y - back], 0.0, "back_start".into()),
    ]);
    let n = corners.len();
    let cuts: Vec<_> = (0..n)
        .map(|i| {
            trim(
                &corners[(i + n - 1) % n],
                &corners[i],
                &corners[(i + 1) % n],
            )
            .0
        })
        .collect();
    // 丸みどうしが重ならず、歯面と歯底に正の長さを残す。
    let mut scale = 1.0f64;
    for i in 0..n {
        let j = (i + 1) % n;
        let used = cuts[i] + cuts[j];
        if !used.is_finite() {
            return Err("ラックの丸みが大きすぎます".into());
        }
        if used > 0.0 {
            scale = scale.min(0.999 * unit(corners[i].at, corners[j].at).1 / used);
        }
    }
    let mut warnings = Vec::new();
    if scale < 1.0 {
        warnings.push(format!(
            "歯の幅に入るよう歯先と歯底の丸みを {:.3} 倍にしました",
            scale
        ));
        for c in &mut corners {
            c.radius *= scale;
        }
    }
    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();
    let mut rounds = Vec::new();
    for i in 0..n {
        let c = &corners[i];
        let (cut, a, b, turn) = trim(&corners[(i + n - 1) % n], c, &corners[(i + 1) % n]);
        let from = [c.at[0] - cut * a[0], c.at[1] - cut * a[1]];
        let to = [c.at[0] + cut * b[0], c.at[1] + cut * b[1]];
        incoming.push(from);
        outgoing.push(to);
        rounds.push(if c.radius > 0.0 {
            let sign = turn.signum();
            let center = [
                from[0] - sign * c.radius * a[1],
                from[1] + sign * c.radius * a[0],
            ];
            Some(arc(c.name.clone(), center, c.radius, from, to, turn > 0.0))
        } else {
            None
        });
    }
    let mut edges: Vec<ProfileEdge> = Vec::new();
    for i in 0..n {
        if let Some(round) = rounds[i].take() {
            edges.push(round);
        }
        let a = outgoing[i];
        let b = incoming[(i + 1) % n];
        let name = match i {
            0 => "lead".into(),
            i if i <= p.teeth as usize * 4 => {
                let tooth = (i - 1) / 4;
                let side = ["l0", "tip", "r0", "root"][(i - 1) % 4];
                format!("t{tooth}:{side}")
            }
            i if i == n - 3 => "end".into(),
            i if i == n - 2 => "back".into(),
            _ => "start".into(),
        };
        edges.push(edge(name, Segment::Line { a, b }, a));
    }
    // 上の歯面を左から右へたどった輪を反時計回りへそろえる。
    edges.reverse();
    for e in &mut edges {
        e.reversed = !e.reversed;
    }
    Ok(Outline {
        edges,
        reference: Vec::new(),
        reference_lines: vec![edge(
            "pitch",
            Segment::Line {
                a: [0.0, 0.0],
                b: [length, 0.0],
            },
            [0.0, 0.0],
        )],
        warnings,
    })
}

#[cfg(test)]
#[path = "rack_tests.rs"]
mod tests;
