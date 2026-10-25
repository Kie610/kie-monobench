//! 定常の熱伝導(温度の分布)。単位は mm・W・℃。熱伝導率は W/(m·K)、熱伝達係数は
//! W/(m²·K) で受け取り、mm に直して使う。

use crate::boundary::{Prepared, face_quadrature};
use crate::solve::{apply_fixed, assemble, solve};
use crate::tet10;

/// 熱の条件。
#[derive(Debug, Clone, PartialEq)]
pub struct ThermalInput {
    /// 熱伝導率 W/(m·K)
    pub k: f64,
    /// 温度を決める面と温度(℃)
    pub temps: Vec<(Vec<String>, f64)>,
    /// 熱が入る面と熱の量(W、面全体の合計)
    pub heats: Vec<(Vec<String>, f64)>,
    /// 空気へ逃げる面と熱伝達係数 W/(m²·K)
    pub convs: Vec<(Vec<String>, f64)>,
    /// 上で選んでいない残りの外の面の熱伝達係数(`None` なら逃げない)
    pub rest_h: Option<f64>,
    /// まわりの空気の温度(℃)
    pub ambient: f64,
}

#[derive(Debug, Clone)]
pub struct ThermalOutput {
    /// 節点ごとの温度(℃)
    pub t: Vec<f64>,
    /// 空気へ逃げた熱の合計(W)
    pub to_air: f64,
    pub iterations: usize,
}

pub fn solve_thermal(p: &Prepared, input: &ThermalInput) -> Result<ThermalOutput, String> {
    let m = &p.mesh;
    let n = m.points.len();
    let k_mm = input.k / 1000.0;
    let mut a = assemble(m, 1, |e| {
        let mut ke = tet10::conduction(&m.tet_points(e));
        ke.iter_mut().for_each(|v| *v *= k_mm);
        Some(ke)
    });
    let mut b = vec![0.0; n];
    let mut used = vec![false; m.faces.len()];
    for (faces, w) in &input.heats {
        let fs = p.faces_of(faces)?;
        let q = w / p.area(&fs).max(1e-30);
        p.add_flux(&fs, q, &mut b);
        for f in fs {
            used[f] = true;
        }
    }
    let mut fixed = Vec::new();
    for (faces, t) in &input.temps {
        let fs = p.faces_of(faces)?;
        fixed.extend(p.nodes_of(&fs).into_iter().map(|nd| (nd as usize, *t)));
        for f in fs {
            used[f] = true;
        }
    }
    let mut conv: Vec<(usize, f64)> = Vec::new(); // (表面の三角形、熱伝達係数 W/(mm²·K))
    for (faces, hc) in &input.convs {
        for f in p.faces_of(faces)? {
            used[f] = true;
            conv.push((f, hc / 1e6));
        }
    }
    if let Some(hc) = input.rest_h {
        conv.extend(
            (0..m.faces.len())
                .filter(|f| !used[*f])
                .map(|f| (f, hc / 1e6)),
        );
    }
    if fixed.is_empty() && conv.iter().all(|c| c.1 <= 0.0) {
        return Err("温度を決める面か、空気へ逃げる面(熱伝達係数)を付けてください".into());
    }
    // 空気へ逃げる熱: hc ∫ N_i N_j dA を行列へ、hc T∞ ∫ N_i dA を右辺へ
    for &(f, hc) in &conv {
        let nodes = m.faces[f].nodes;
        for (nn, _, w) in face_quadrature(&m.face_points(f)) {
            for i in 0..6 {
                b[nodes[i] as usize] += hc * input.ambient * nn[i] * w;
                for j in 0..6 {
                    a.add(nodes[i] as usize, nodes[j], hc * nn[i] * nn[j] * w);
                }
            }
        }
    }
    apply_fixed(&mut a, &mut b, &fixed);
    let mut free = vec![1.0; n];
    for (d, _) in &fixed {
        free[*d] = 0.0;
    }
    let mut t = vec![input.ambient; n];
    for (d, v) in &fixed {
        t[*d] = *v;
    }
    let (rep, _) = solve(a, &b, &mut t, 1, vec![free], 1e-9)?;
    let to_air = conv
        .iter()
        .map(|&(f, hc)| {
            let nodes = m.faces[f].nodes;
            face_quadrature(&m.face_points(f))
                .iter()
                .map(|(nn, _, w)| {
                    let tf: f64 = (0..6).map(|i| nn[i] * t[nodes[i] as usize]).sum();
                    hc * (tf - input.ambient) * w
                })
                .sum::<f64>()
        })
        .sum();
    Ok(ThermalOutput {
        t,
        to_air,
        iterations: rep.iterations,
    })
}
