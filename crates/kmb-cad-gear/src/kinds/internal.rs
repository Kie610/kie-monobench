//! 内歯車。内側に向く歯の先は r − m(1 − x)、歯底は r + m(1.25 + x)。
//! 歯溝をインボリュートの二円弧で作り、その輪を穴として使う。正の転位は歯厚を減らす。

use super::{Outline, Params};
use crate::tooth::{
    HalfTooth, P2, arc, edge, fit_biarcs, inv, involute, polar, ring_of_teeth, roll_at, rotate,
    round_between, tolerance,
};
use std::f64::consts::PI;

pub fn outline(p: &Params) -> Result<Outline, String> {
    let m = p.module;
    let z = p.teeth as f64;
    let alpha = p.pressure_angle;
    if !m.is_finite() || m <= 0.0 {
        return Err("モジュールは 0 より大きい有限の数にしてください".into());
    }
    if p.teeth < 3 {
        return Err("歯数は 3 以上にしてください".into());
    }
    if !(alpha > 0.0 && alpha < 45f64.to_radians()) {
        return Err("圧力角は 0 deg より大きく 45 deg より小さくしてください".into());
    }
    if !p.profile_shift.is_finite()
        || !p.tip_round.is_finite()
        || p.tip_round < 0.0
        || !p.root_round.is_finite()
        || p.root_round < 0.0
        || !(m * p.tip_round).is_finite()
        || !(m * p.root_round).is_finite()
    {
        return Err("転位係数は有限の数、丸みは 0 以上の有限の数にしてください".into());
    }
    let r = m * z / 2.0;
    let rb = r * alpha.cos();
    let ra = r - m * (1.0 - p.profile_shift);
    let rf = r + m * (1.25 + p.profile_shift);
    if !rf.is_finite() || ra <= 0.0 || rf <= rb {
        return Err("内歯車の歯先円の径を 0 より大きく、歯底円を基礎円より大きくしてください。歯数か転位係数を見直してください".into());
    }
    // 歯溝の半角。ピッチ円上の溝幅は πm/2 + 2xm tanα。
    let delta = (PI / 2.0 + 2.0 * p.profile_shift * alpha.tan()) / z + inv(alpha);
    let inner_half = delta - inv((rb / ra).min(1.0).acos());
    let outer_half = delta - inv((rb / rf).acos());
    if outer_half <= 1e-9 || inner_half >= PI / z - 1e-9 || inner_half <= 0.0 {
        return Err(
            "内歯車の歯先か歯底で歯形が重なります。歯数・圧力角・転位係数を見直してください".into(),
        );
    }
    let (sd, cd) = (-delta).sin_cos();
    let turn = move |q: P2| [cd * q[0] - sd * q[1], sd * q[0] + cd * q[1]];
    let flank = move |rho: f64| -> (P2, P2) {
        if rho < rb {
            (polar(rho, -delta), polar(1.0, -delta))
        } else {
            let (q, d) = involute(rb, roll_at(rb, rho));
            (turn(q), turn(d))
        }
    };
    let mut warnings = Vec::new();
    // 穴の凹側(小径)が歯の先、凸側(大径)が歯底なので、丸める側も半径方向に合わせる。
    let mut inner = None;
    let mut rho = p.tip_round * m;
    let wanted = rho;
    while rho > 1e-6 {
        if let Some((rs, c)) = round_between(&flank, ra, rf, ra, rho, -1.0, true)
            && c[1].atan2(c[0]) > -PI / z
        {
            inner = Some((rs, c, rho));
            break;
        }
        rho *= 0.8;
    }
    if wanted > 1e-6 && inner.is_none_or(|(_, _, got)| got < wanted - 1e-9) {
        warnings.push(format!(
            "歯先の丸みは歯の幅に入らないので、半径 {:.3} mm にしました",
            inner.map_or(0.0, |(_, _, got)| got)
        ));
    }
    let start = inner.map_or(ra, |(rs, _, _)| rs);
    let mut outer = None;
    let mut rho = p.root_round * m;
    let wanted = rho;
    while rho > 1e-6 {
        if let Some((rt, c)) = round_between(&flank, start.max(rb), rf, rf, rho, 1.0, false)
            && c[1].atan2(c[0]) < 0.0
        {
            outer = Some((rt, c, rho));
            break;
        }
        rho *= 0.8;
    }
    if wanted > 1e-6 && outer.is_none_or(|(_, _, got)| got < wanted - 1e-9) {
        warnings.push(format!(
            "歯底の丸みは歯溝に入らないので、半径 {:.3} mm にしました",
            outer.map_or(0.0, |(_, _, got)| got)
        ));
    }
    let end = outer.map_or(rf, |(rt, _, _)| rt);
    let mut half = Vec::new();
    if let Some((rs, c, rho)) = inner {
        let a = c[1].atan2(c[0]);
        half.push((
            "round".to_string(),
            arc("", c, rho, polar(ra, a), flank(rs).0, false),
        ));
    }
    if start < rb - 1e-9 {
        let (a, b) = (flank(start).0, flank(rb).0);
        half.push((
            "neck".to_string(),
            edge("", kmb_core::Segment::Line { a, b }, a),
        ));
    }
    let along = move |t: f64| {
        let (q, d) = involute(rb, t);
        (turn(q), turn(d))
    };
    for (k, seg) in fit_biarcs(
        &along,
        roll_at(rb, start.max(rb)),
        roll_at(rb, end),
        tolerance(m),
    )
    .into_iter()
    .enumerate()
    {
        let from = if k == 0 {
            flank(start.max(rb)).0
        } else {
            half.last()
                .map(|(_, e): &(String, kmb_core::ProfileEdge)| e.to_point())
                .unwrap_or_default()
        };
        half.push((k.to_string(), edge("", seg, from)));
    }
    if let Some((rt, c, rho)) = outer {
        let a = c[1].atan2(c[0]);
        half.push((
            "fillet".to_string(),
            arc("", c, rho, flank(rt).0, polar(rf, a), true),
        ));
    }
    let mut edges = ring_of_teeth(
        &HalfTooth {
            flank: half,
            tip_radius: rf,
            root_radius: ra,
        },
        p.teeth,
    );
    // 歯溝の中心を半ピッチ回すと、最初の内向きの歯が X 軸に来る。区間の名は穴でなく歯を指す。
    for e in &mut edges {
        let (tooth, part) = e.name.split_once(':').expect("歯の区間の名前");
        let i: u32 = tooth[1..].parse().expect("歯の番号");
        let (i, part) = match part {
            "tip" => (i, "root".to_string()),
            "root" => ((i + 1) % p.teeth, "tip".to_string()),
            other if other.starts_with('r') => (i, format!("l{}", &other[1..])),
            other => ((i + 1) % p.teeth, format!("r{}", &other[1..])),
        };
        *e = rotate(e, PI / z);
        e.name = format!("t{i}:{part}");
    }
    Ok(Outline {
        edges,
        reference: vec![
            ("pitch", [0.0, 0.0], r),
            ("base", [0.0, 0.0], rb),
            ("tip", [0.0, 0.0], ra),
            ("root", [0.0, 0.0], rf),
        ],
        warnings,
    })
}

#[cfg(test)]
#[path = "internal_tests.rs"]
mod tests;
