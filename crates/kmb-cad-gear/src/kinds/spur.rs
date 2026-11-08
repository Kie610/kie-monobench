//! 外歯の平歯車(インボリュートの歯形)。
//!
//! 基準ピッチ円 d = m z、基礎円 db = d cos α、歯先円 da = d + 2m(1 + x)、歯底円 df = d − 2m(1.25 − x)。
//! 歯面は基礎円のインボリュートを二円弧で近似する。基礎円より内側(df < db の時)は半径の向きの線で
//! 歯底までつなぎ、歯底の丸みで歯底円に接続する。切り下げ(歯車を作る工具が歯元を削り込む形)は
//! 作らず、起きる歯数なら注意を出す。

use super::{Outline, Params};
use crate::tooth::{
    HalfTooth, P2, arc, edge, fit_biarcs, inv, involute, polar, ring_of_teeth, roll_at,
    round_between, tolerance,
};
use std::f64::consts::PI;

pub fn outline(p: &Params) -> Result<Outline, String> {
    let m = p.module;
    let z = p.teeth as f64;
    let alpha = p.pressure_angle;
    let x = p.profile_shift;
    if m.is_nan() || m <= 0.0 {
        return Err("モジュールは 0 より大きくしてください".into());
    }
    if p.teeth < 3 {
        return Err("歯数は 3 以上にしてください".into());
    }
    if !(alpha > 0.0 && alpha < 45f64.to_radians()) {
        return Err("圧力角は 0 deg より大きく 45 deg より小さくしてください".into());
    }
    let r = m * z / 2.0;
    let rb = r * alpha.cos();
    let ra = r + m * (1.0 + x);
    let rf = r - m * (1.25 - x);
    if rf <= 0.0 {
        return Err(format!(
            "歯底円の半径が 0 以下になります(歯底円の径 {:.3} mm)。歯数を増やすか転位係数を大きくしてください",
            2.0 * rf
        ));
    }
    // ピッチ円の上の歯の厚さの半分の角と、基礎円の上での歯面の角
    let psi = (PI / 2.0 + 2.0 * x * alpha.tan()) / z;
    let delta = psi + inv(alpha);
    let (sd, cd) = (-delta).sin_cos();
    let turn = move |q: P2| [cd * q[0] - sd * q[1], sd * q[0] + cd * q[1]];
    // 右の歯面(角の負の側)の、半径 ρ での点と外へ進む向き
    let flank = move |rho: f64| -> (P2, P2) {
        if rho < rb {
            (polar(rho, -delta), polar(1.0, -delta))
        } else {
            let (q, d) = involute(rb, roll_at(rb, rho));
            (turn(q), turn(d))
        }
    };
    let tip_half = delta - inv((rb / ra).min(1.0).acos());
    if ra <= rb || tip_half <= 1e-9 {
        return Err("歯先がとがります。転位係数を小さくするか、圧力角を小さくしてください".into());
    }
    let mut warnings = Vec::new();
    let undercut = 2.0 * (1.0 - x) / alpha.sin().powi(2);
    if z < undercut - 1e-9 {
        warnings.push(format!(
            "歯数 {} では歯元の切り下げが起きます(この圧力角で転位なしなら {} 枚以上)。形は切り下げなしで作ります。転位係数を {:.3} 以上にすると起きません",
            p.teeth,
            undercut.ceil(),
            1.0 - z * alpha.sin().powi(2) / 2.0
        ));
    }
    let gap_mid = -PI / z;
    // 歯底の丸み: 歯溝の真ん中を越えない大きさまで縮める
    let mut root = None;
    let mut rho = p.root_round * m;
    let wanted = rho;
    while rho > 1e-6 {
        if let Some((rs, c)) = round_between(&flank, rf, ra, rf, rho, -1.0, true)
            && c[1].atan2(c[0]) > gap_mid
        {
            root = Some((rs, c, rho));
            break;
        }
        rho *= 0.8;
    }
    if wanted > 1e-6 && root.is_none_or(|(_, _, got)| got < wanted - 1e-9) {
        warnings.push(format!(
            "歯底の丸みは歯溝に入らないので、半径 {:.3} mm にしました",
            root.map_or(0.0, |(_, _, got)| got)
        ));
    }
    let start = root.map_or(rf, |(rs, _, _)| rs);
    // 歯先の丸み: 歯の中心の線を越えない大きさまで縮める
    let mut tip = None;
    let mut rho = p.tip_round * m;
    let wanted = rho;
    while rho > 1e-6 {
        if let Some((rt, c)) = round_between(&flank, start.max(rb), ra, ra, rho, 1.0, false)
            && c[1].atan2(c[0]) < 0.0
        {
            tip = Some((rt, c, rho));
            break;
        }
        rho *= 0.8;
    }
    if wanted > 1e-6 && tip.is_none_or(|(_, _, got)| got < wanted - 1e-9) {
        warnings.push(format!(
            "歯先の丸みは歯の幅に入らないので、半径 {:.3} mm にしました",
            tip.map_or(0.0, |(_, _, got)| got)
        ));
    }
    let end = tip.map_or(ra, |(rt, _, _)| rt);
    let tol = tolerance(m);
    let mut half = Vec::new();
    if let Some((rs, c, rho)) = root {
        let a = c[1].atan2(c[0]);
        half.push((
            "fillet".to_string(),
            arc("", c, rho, polar(rf, a), flank(rs).0, false),
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
    let (t0, t1) = (roll_at(rb, start.max(rb)), roll_at(rb, end));
    for (k, seg) in fit_biarcs(&along, t0, t1, tol).into_iter().enumerate() {
        let from = if k == 0 {
            flank(start.max(rb)).0
        } else {
            half.last()
                .map(|(_, e): &(String, kmb_core::ProfileEdge)| e.to_point())
                .unwrap_or_default()
        };
        half.push((k.to_string(), edge("", seg, from)));
    }
    if let Some((rt, c, rho)) = tip {
        let a = c[1].atan2(c[0]);
        half.push((
            "round".to_string(),
            arc("", c, rho, flank(rt).0, polar(ra, a), true),
        ));
    }
    let edges = ring_of_teeth(
        &HalfTooth {
            flank: half,
            tip_radius: ra,
            root_radius: rf,
        },
        p.teeth,
    );
    Ok(Outline {
        axial: false,
        edges,
        reference: vec![
            ("pitch", [0.0, 0.0], r),
            ("base", [0.0, 0.0], rb),
            ("tip", [0.0, 0.0], ra),
            ("root", [0.0, 0.0], rf),
        ],
        warnings,
        reference_lines: Vec::new(),
    })
}

#[cfg(test)]
#[path = "spur_tests.rs"]
mod tests;
