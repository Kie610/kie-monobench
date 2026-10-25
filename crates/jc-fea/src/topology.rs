//! 軽い形の参考(トポロジー最適化)。要素ごとの密度 x(0〜1)を変数にし、縦弾性係数を
//! E_min + x^p (E − E_min) とする素の SIMP(密度法、Bendsøe 1989)で、体積を決めた割合に
//! 抑えながら、たわみにくさ(コンプライアンスの最小)を求める。密度は半径の内の要素(重心の
//! 距離)で、距離と体積の重みを付けて均す(密度フィルタ、Bruns・Tortorelli 2001 / Bourdin 2001)。
//! 要素の大きさがそろわない四面体の網なので、体積は要素の体積で重みを付けて数える。
//! 更新は最適性規準法。
//!
//! 結果は「参考の形」として見せるだけで、形(B-rep)には戻さない。加工の制約(削る向き、
//! 工具の太さ、層ごとの深さ)は入れない(docs/避ける形.md)。

use crate::amg::{Amg, rigid_modes};
use crate::boundary::Prepared;
use crate::geom::{P3, add, dist, scale};
use crate::solve::{apply_fixed, assemble, blocks, pcg};
use crate::statics::{Load, element_u, fixed_dofs, fixed_mask, load_vector};
use crate::tet10;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct TopologyInput {
    pub e: f64,
    pub nu: f64,
    pub fixed: Vec<String>,
    pub loads: Vec<Load>,
    /// 残す体積の割合(0〜1)
    pub volume: f64,
    /// 均す半径(mm)
    pub radius: f64,
    /// 繰り返しの上限
    pub iterations: usize,
}

#[derive(Debug, Clone)]
pub struct TopologyOutput {
    /// 要素ごとの密度(均した後、0〜1)
    pub density: Vec<f64>,
    /// 繰り返しごとのコンプライアンス(N·mm)
    pub compliance: Vec<f64>,
    /// 固定と荷重の面に接する、密度を 1 のまま残した要素
    pub passive: Vec<bool>,
}

const PENAL: f64 = 3.0;
const MOVE: f64 = 0.2;
/// 要素の剛性を覚えておく要素の数の上限(1 要素 7 KB)。
const KEEP_STIFFNESS: usize = 30_000;

/// 要素の重心。
fn centroids(p: &Prepared) -> Vec<P3> {
    p.mesh
        .tets
        .iter()
        .map(|t| {
            let mut c = [0.0; 3];
            for v in &t[..4] {
                c = add(c, p.mesh.points[*v as usize]);
            }
            scale(c, 0.25)
        })
        .collect()
}

/// 均す重み(要素ごとに、重心が半径の内の要素と重み (r − 距離)·体積)。
fn filter(c: &[P3], vol: &[f64], radius: f64) -> Vec<Vec<(u32, f64)>> {
    let key = |p: P3| p.map(|v| (v / radius).floor() as i64);
    let mut cells: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    for (e, p) in c.iter().enumerate() {
        cells.entry(key(*p)).or_default().push(e as u32);
    }
    c.iter()
        .map(|p| {
            let k = key(*p);
            let mut v = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let Some(list) = cells.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) else {
                            continue;
                        };
                        for &j in list {
                            let w = radius - dist(*p, c[j as usize]);
                            if w > 0.0 {
                                v.push((j, w * vol[j as usize]));
                            }
                        }
                    }
                }
            }
            v.sort_unstable_by_key(|x| x.0);
            v
        })
        .collect()
}

pub fn solve_topology(
    p: &Prepared,
    input: &TopologyInput,
    mut progress: impl FnMut(usize, f64),
) -> Result<TopologyOutput, String> {
    if !(0.05..=0.95).contains(&input.volume) {
        return Err("残す体積は 5% から 95% にしてください".into());
    }
    if input.loads.is_empty() {
        return Err("荷重を付けてください(面と力か圧力)".into());
    }
    let m = &p.mesh;
    let ne = m.tets.len();
    let e_min = input.e * 1e-9;
    let vol: Vec<f64> = (0..ne).map(|e| tet10::volume(&m.tet_points(e))).collect();
    let total: f64 = vol.iter().sum();
    let f = load_vector(p, &input.loads)?;
    let fixed = fixed_dofs(p, &input.fixed)?;
    // 固定と荷重の面に接する要素は残す
    let mut passive = vec![false; ne];
    let mut faces: Vec<String> = input.fixed.clone();
    for l in &input.loads {
        faces.extend(l.faces.iter().cloned());
    }
    for f in p.faces_of(&faces)? {
        passive[m.faces[f].elem as usize] = true;
    }
    let modes = rigid_modes(&m.points, &fixed_mask(m.points.len() * 3, &fixed));
    let w = filter(&centroids(p), &vol, input.radius);
    let wsum: Vec<f64> = w.iter().map(|v| v.iter().map(|x| x.1).sum()).collect();
    let smooth = |x: &[f64]| -> Vec<f64> {
        w.iter()
            .zip(&wsum)
            .map(|(v, s)| v.iter().map(|(j, wj)| wj * x[*j as usize]).sum::<f64>() / s)
            .collect()
    };
    let mut x: Vec<f64> = passive
        .iter()
        .map(|p| if *p { 1.0 } else { input.volume })
        .collect();
    let mut u = vec![0.0; f.len()];
    let mut history = Vec::new();
    let mut xs = smooth(&x);
    // 縦弾性係数 1 の要素の剛性(大きすぎる網では毎回作り直す)
    let unit = |e: usize| tet10::stiffness(&m.tet_points(e), 1.0, input.nu);
    let cache: Option<Vec<Vec<f64>>> = (ne <= KEEP_STIFFNESS).then(|| (0..ne).map(unit).collect());
    let ke_of = |e: usize| -> std::borrow::Cow<'_, [f64]> {
        match &cache {
            Some(c) => std::borrow::Cow::Borrowed(&c[e]),
            None => std::borrow::Cow::Owned(unit(e)),
        }
    };
    for it in 0..input.iterations.max(1) {
        let ef = |e: usize| e_min + xs[e].powf(PENAL) * (input.e - e_min);
        let mut a = assemble(m, 3, |e| Some(ke_of(e).iter().map(|k| k * ef(e)).collect()));
        let mut b = f.clone();
        apply_fixed(&mut a, &mut b, &fixed);
        // 密度が変わると前の前処理は効かなくなる(試すと回数が何倍にもなった)ので、毎回作る
        let mg = Amg::new(a, blocks(m.points.len(), 3), modes.clone())?;
        pcg(mg.matrix(), &b, &mut u, &|r, z| mg.apply(r, z), 1e-6, 500)?;
        // コンプライアンスと、均した密度での感度
        let mut c = 0.0;
        let mut dc = vec![0.0; ne];
        for e in 0..ne {
            let ue = element_u(p, &u, e);
            let ke = ke_of(e);
            let mut ce = 0.0;
            for r in 0..30 {
                let row = &ke[r * 30..r * 30 + 30];
                ce += ue[r] * row.iter().zip(&ue).map(|(k, v)| k * v).sum::<f64>();
            }
            c += ef(e) * ce;
            dc[e] = -PENAL * xs[e].powf(PENAL - 1.0) * (input.e - e_min) * ce;
        }
        history.push(c);
        progress(it, c);
        // 均す前の密度での感度(フィルタの連鎖律)
        let mut dcx = vec![0.0; ne];
        let mut dvx = vec![0.0; ne];
        for (e, v) in w.iter().enumerate() {
            for (j, wj) in v {
                dcx[*j as usize] += wj * dc[e] / wsum[e];
                dvx[*j as usize] += wj * vol[e] / wsum[e];
            }
        }
        // 最適性規準法: 体積が決めた割合になる乗数を二分法で探す
        let target = input.volume * total;
        let (mut l1, mut l2) = (0.0f64, 1e9f64);
        let mut next = x.clone();
        while (l2 - l1) / (l1 + l2).max(1e-30) > 1e-4 {
            let lm = 0.5 * (l1 + l2);
            for e in 0..ne {
                if passive[e] {
                    next[e] = 1.0;
                    continue;
                }
                let be = (-dcx[e] / (dvx[e] * lm).max(1e-30)).max(0.0).sqrt();
                next[e] = (x[e] * be).clamp(x[e] - MOVE, x[e] + MOVE).clamp(0.0, 1.0);
            }
            let now: f64 = smooth(&next).iter().zip(&vol).map(|(x, v)| x * v).sum();
            if now > target {
                l1 = lm;
            } else {
                l2 = lm;
            }
        }
        let change = next
            .iter()
            .zip(&x)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        x = next;
        xs = smooth(&x);
        log::info!(
            "軽い形: {} 回目 コンプライアンス {c:.4e} 変化 {change:.3}",
            it + 1
        );
        if change < 0.01 && it > 5 {
            break;
        }
    }
    Ok(TopologyOutput {
        density: xs,
        compliance: history,
        passive,
    })
}
