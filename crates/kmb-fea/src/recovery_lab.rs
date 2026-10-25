//! 調べ物: 節点の応力の作り方(要素の節点の値から節点の値を作る方法)ごとの局所の誤差を、
//! 理論解と細かい網の解に照らして表と絵にする(docs/verify.md の 10 節)。
//!
//! - ignore の試験は表と絵を書くだけで確かめはしない。`JC_LAB_DIR`・`JC_LAB_OUT` を付けて手で流す。
//! - 見本の部品の網を読む試験は、研究が作った表示用の網を `JC_LAB_DUMP` へ書き出す一時の変更を
//!   当てて作ったファイルを読む。
//! - ignore の無い試験(`a_clamped_edge_is_smooth`・`a_clamped_round_edge_is_smooth`・
//!   `band_borders_on_a_flat_face_stay_round`)は、ここで測った縁のギザギザと帯の境の波打ちが
//!   戻らないことを確かめる。
#![allow(clippy::needless_range_loop, clippy::type_complexity)]

use crate::amg::rigid_modes;
use crate::boundary::Prepared;
use crate::geom::P3;
use crate::solve::{apply_fixed, assemble, pcg, solve};
use crate::statics::{Dir, Load, LoadKind, element_u, fixed_dofs, fixed_mask, load_vector};
use crate::tet10::{self, EDGES, NODE_XI, gauss_fine, gauss4, von_mises};
use crate::tetmesh::tests::annulus;
use crate::validation_tests::box_mesh;
use nalgebra::{DMatrix, DVector};
use std::collections::HashMap;
use std::io::Write;

mod clamped;
mod known;
mod methods;
mod midpoints;
mod sampling;

use methods::*;
use sampling::*;

type S6 = [f64; 6];

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// 要素ごとの、節点での応力(10 個)。
fn elem_nodal(p: &Prepared, e_mod: f64, nu: f64, u: &[f64]) -> Vec<[S6; 10]> {
    let m = &p.mesh;
    (0..m.tets.len())
        .map(|e| {
            let x = m.tet_points(e);
            let ue = element_u(p, u, e);
            NODE_XI.map(|xi| tet10::stress_at(&x, e_mod, nu, &ue, xi))
        })
        .collect()
}

fn norm(s: &S6) -> f64 {
    (s[0] * s[0] + s[1] * s[1] + s[2] * s[2] + 2.0 * (s[3] * s[3] + s[4] * s[4] + s[5] * s[5]))
        .sqrt()
}

fn diff(a: &S6, b: &S6) -> S6 {
    [0, 1, 2, 3, 4, 5].map(|k| a[k] - b[k])
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}
