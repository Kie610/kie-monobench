//! ロープ車の軸断面。ロープの中心をピッチ円に置き、半円の溝を開ける。

use super::pulley::{line, validate_section};
use super::{Outline, Params};
use crate::tooth::arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sheave {
    pub diameter: f64,
    pub width: f64,
    pub bore: f64,
    pub rope: f64,
    pub flange: f64,
}

impl Default for Sheave {
    fn default() -> Self {
        Self {
            diameter: 100.0,
            width: 30.0,
            bore: 0.0,
            rope: 10.0,
            flange: 3.0,
        }
    }
}

pub fn outline(p: &Params) -> Result<Outline, String> {
    let p = p.sheave.unwrap_or_default();
    let r = p.diameter / 2.0;
    let rho = p.rope * 0.53;
    let outer = r + p.flange;
    let b = p.bore / 2.0;
    validate_section(p.diameter, p.width, p.bore, r - rho)?;
    if !rho.is_finite()
        || rho <= 0.0
        || !p.flange.is_finite()
        || p.flange < 0.0
        || !outer.is_finite()
        || p.width <= 2.0 * rho
    {
        return Err("ロープの径は 0 より大きく、縁の高さは 0 以上、幅はロープの径の 1.06 倍より大きくしてください".into());
    }
    let lo = p.width / 2.0 - rho;
    let hi = p.width / 2.0 + rho;
    let mut edges = vec![
        line("end_start", [b, 0.0], [outer, 0.0]),
        line("land_start", [outer, 0.0], [outer, lo]),
    ];
    if p.flange > 0.0 {
        edges.push(line("groove_start", [outer, lo], [r, lo]));
    }
    edges.push(arc(
        "groove",
        [r, p.width / 2.0],
        rho,
        [r, lo],
        [r, hi],
        false,
    ));
    if p.flange > 0.0 {
        edges.push(line("groove_finish", [r, hi], [outer, hi]));
    }
    edges.extend([
        line("land_finish", [outer, hi], [outer, p.width]),
        line("end_finish", [outer, p.width], [b, p.width]),
        line("bore", [b, p.width], [b, 0.0]),
    ]);
    Ok(Outline {
        edges,
        reference: vec![
            ("pitch", [0.0, 0.0], r),
            ("tip", [0.0, 0.0], outer),
            ("root", [0.0, 0.0], r - rho),
        ],
        warnings: Vec::new(),
        axial: true,
    })
}

#[cfg(test)]
#[path = "sheave_tests.rs"]
mod tests;
