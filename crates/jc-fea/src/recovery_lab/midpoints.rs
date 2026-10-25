//! 辺の中点の値の作り方: 色の帯の境の波打ち、縁と曲面の上の中点。

use super::known::lame;
use super::*;

/// 色の帯の境の波打ち: 厚肉の円筒(Lamé)の平らな上の面と高さの半分の断面で、辺の中点の値の
/// 作り方ごとに、塗る値の誤差を段の幅(全体の幅 / 40)で測る。正解は半径だけで決まるので、
/// 同じ半径の輪の上での誤差の振れ(最大 − 最小)が、帯の境の波の大きさ(段の何倍か)。
/// JC_LAB_DIR に表(midpoints.md)と絵(PPM)を書く。JC_LAB_H で網の大きさ(既定 0.9,0.6)。
#[test]
#[ignore = "調べ物の道具(確かめはせず、表と絵を書くだけ)。JC_LAB_DIR などで手で流す"]
fn midpoint_waviness() {
    let dir = std::env::var("JC_LAB_DIR").unwrap();
    let hs: Vec<f64> = std::env::var("JC_LAB_H")
        .unwrap_or("0.9,0.6".into())
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    let mut out = std::fs::File::create(format!("{dir}/midpoints.md")).unwrap();
    for h in hs {
        let c = lame(h);
        let p = &c.p;
        let m = &p.mesh;
        let en = elem_nodal(p, c.e, c.nu, &c.u);
        let gs = gauss_samples(p, c.e, c.nu, &c.u);
        let exact_nodes: Vec<S6> = m.points.iter().map(|x| (c.exact)(*x)).collect();
        let mut exact_corners = exact_nodes.clone();
        for t in &m.tets {
            for (k, [a, b]) in EDGES.iter().enumerate() {
                let (sa, sb) = (exact_nodes[t[*a] as usize], exact_nodes[t[*b] as usize]);
                exact_corners[t[4 + k] as usize] = std::array::from_fn(|i| 0.5 * (sa[i] + sb[i]));
            }
        }
        let methods: Vec<(&str, &str, Vec<S6>)> = vec![
            ("A", "中点は両端の角の平均", r_corner(p, &en)),
            ("B", "要素ごとの中点の値を平均", r_avg(p, &en)),
            ("C", "SPR 2 次(角も中点も)", r_spr(p, &gs, true)),
            (
                "D",
                "角は A と同じ・中点に SPR の曲がり",
                r_spr_curv(p, &en, &gs),
            ),
            ("E", "角は A と同じ・中点に勾配の差", r_hermite(p, &en)),
            ("F", "L2 の射影(2 次)", r_l2(p, c.e, c.nu, &c.u)),
            ("X", "正解の角・中点は両端の平均", exact_corners),
            ("Y", "正解を全部の節点に", exact_nodes),
        ];
        let (a_in, b_out, l) = (5.0, 10.0, 3.0);
        let vm_ex = |x: P3| von_mises(&(c.exact)(x));
        let (vlo, vhi) = (vm_ex([b_out, 0.0, 0.0]), vm_ex([a_in, 0.0, 0.0]));
        let step = (vhi - vlo) / 40.0;
        writeln!(
            out,
            "\n## 厚肉の円筒 h {h}(要素 {}・節点 {})。ミーゼス {vlo:.3}〜{vhi:.3} MPa、段の幅 {step:.4} MPa。誤差は段の幅に対する比",
            m.tets.len(),
            m.points.len()
        )
        .unwrap();
        writeln!(out, "\n### 辺の中点の値の誤差(ミーゼス、符号付き、段の幅に対する比)\n| 候補 | 半径の向きの辺 平均 | 周の向きの辺 平均 | 全部 RMS | 全部 最大 | 角 RMS |\n|---|---|---|---|---|---|").unwrap();
        let loc = Locator::new(m);
        let mut planes = Vec::new();
        for (key, name, s) in &methods {
            let (mut rad, mut circ, mut all, mut cor) =
                (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for t in &m.tets {
                for (k, [a, b]) in EDGES.iter().enumerate() {
                    let vm = t[4 + k] as usize;
                    let (xa, xb, xm) = (
                        m.points[t[*a] as usize],
                        m.points[t[*b] as usize],
                        m.points[vm],
                    );
                    let e = [0, 1, 2].map(|i| xb[i] - xa[i]);
                    let r = (xm[0] * xm[0] + xm[1] * xm[1]).sqrt();
                    let rh = [xm[0] / r, xm[1] / r, 0.0];
                    let el = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
                    let cr = (e[0] * rh[0] + e[1] * rh[1]).abs() / el;
                    let ct = (-e[0] * rh[1] + e[1] * rh[0]).abs() / el;
                    let err = (von_mises(&s[vm]) - vm_ex(xm)) / step;
                    all.push(err);
                    if cr > 0.9 {
                        rad.push(err);
                    } else if ct > 0.9 {
                        circ.push(err);
                    }
                }
            }
            for v in 0..m.corners {
                cor.push((von_mises(&s[v]) - vm_ex(m.points[v])) / step);
            }
            let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
            let rms =
                |v: &[f64]| (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt();
            let mx = |v: &[f64]| v.iter().map(|x| x.abs()).fold(0.0, f64::max);
            writeln!(
                out,
                "| {key} {name} | {:+.3} | {:+.3} | {:.3} | {:.3} | {:.3} |",
                mean(&rad),
                mean(&circ),
                rms(&all),
                mx(&all),
                rms(&cor)
            )
            .unwrap();
            // 面での塗る値
            let mut rows = Vec::new();
            for (pname, z, quad) in [
                ("上の面", l - 1e-6, false),
                ("断面", 0.5 * l, false),
                ("上の面・2 次で塗る", l - 1e-6, true),
            ] {
                let (mut errs, mut ptp, mut ptp_edge) = (Vec::new(), Vec::new(), Vec::new());
                let nr = 240;
                for i in 0..nr {
                    let r = a_in + 0.1 + (b_out - a_in - 0.2) * i as f64 / (nr - 1) as f64;
                    let mut ring = Vec::new();
                    for j in 0..720 {
                        let th = j as f64 * std::f64::consts::TAU / 720.0;
                        let q = [r * th.cos(), r * th.sin(), z];
                        let v = if quad {
                            sample_s6_quad(m, &loc, s, q)
                        } else {
                            sample_s6(m, &loc, s, q)
                        };
                        if let Some(v) = v {
                            ring.push((von_mises(&v) - vm_ex(q)) / step);
                        }
                    }
                    let (lo, hi) = ring
                        .iter()
                        .fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
                    if r - a_in < h || b_out - r < h {
                        ptp_edge.push(hi - lo);
                    } else {
                        ptp.push(hi - lo);
                    }
                    errs.extend(ring);
                }
                let rr = rms(&errs);
                let mut e2: Vec<f64> = errs.iter().map(|x| x.abs()).collect();
                let p95 = pct(&mut e2, 0.95);
                let mut pp = ptp.clone();
                rows.push(format!(
                    "| {key} {name} | {pname} | {rr:.3} | {p95:.3} | {:.3} | {:.3} | {:.3} | {:.3} |",
                    mx(&errs),
                    pct(&mut pp, 0.5),
                    pct(&mut ptp, 1.0),
                    pct(&mut ptp_edge, 1.0)
                ));
            }
            planes.extend(rows);
            // 絵: 上の面の 1/4(x, y 0..10.4)、1 mm = 60 画素
            let px_mm = 60.0;
            let w = (10.4 * px_mm) as usize;
            let (mut img, mut eimg) = (vec![[255u8; 3]; w * w], vec![[255u8; 3]; w * w]);
            for iy in 0..w {
                for ix in 0..w {
                    let q = [
                        (ix as f64 + 0.5) / px_mm,
                        (w - 1 - iy) as f64 / px_mm + 0.5 / px_mm,
                        l - 1e-6,
                    ];
                    let r = (q[0] * q[0] + q[1] * q[1]).sqrt();
                    if r < a_in || r > b_out {
                        continue;
                    }
                    let quad = std::env::var("JC_LAB_QUAD").is_ok();
                    let v = if quad {
                        sample_s6_quad(m, &loc, s, q)
                    } else {
                        sample_s6(m, &loc, s, q)
                    };
                    if let Some(v) = v {
                        let vv = von_mises(&v);
                        img[iy * w + ix] = band_color((vv - vlo) / (vhi - vlo));
                        eimg[iy * w + ix] = err_color((vv - vm_ex(q)) / step);
                    }
                }
            }
            write_png_ppm(&format!("{dir}/lame_h{h}_{key}_bands.ppm"), w, w, &img);
            write_png_ppm(&format!("{dir}/lame_h{h}_{key}_error.ppm"), w, w, &eimg);
        }
        writeln!(out, "\n### 塗る値の誤差(段の幅に対する比)と、同じ半径の輪の上の振れ(帯の境の波)\n| 候補 | 面 | RMS | 95% | 最大 | 輪の振れ 中央 | 輪の振れ 最大 | 縁から h 以内の輪の振れ 最大 |\n|---|---|---|---|---|---|---|---|").unwrap();
        for r in planes {
            writeln!(out, "{r}").unwrap();
        }
    }
}

/// 候補のいちばん起きやすい失敗(特異点・鋭い縁・境の片側だけのパッチでの暴れ)を見る:
/// 固定した箱(縁のむら・細かい網との差・上の面の最大)と見本の部品(細かい網との差)。
#[test]
#[ignore = "調べ物の道具(確かめはせず、表を書くだけ)。JC_LAB_DIR などで手で流す"]
fn midpoint_candidates_at_edges() {
    let dir = std::env::var("JC_LAB_DIR").unwrap();
    let mut out = std::fs::File::create(format!("{dir}/edges.md")).unwrap();
    let (e_mod, nu) = (69_000.0, 0.33);
    // 箱
    let body = box_mesh([60.0, 40.0, 10.0]);
    let solve_box = |p: Prepared| -> Solved {
        let m = &p.mesh;
        let mut k = assemble(m, 3, |el| {
            Some(tet10::stiffness(&m.tet_points(el), e_mod, nu))
        });
        let mut f = load_vector(
            &p,
            &[Load {
                faces: names(&["z1"]),
                kind: LoadKind::Force(100.0),
                dir: Dir::Fixed([0.0, 0.0, -1.0]),
            }],
        )
        .unwrap();
        let fixed = fixed_dofs(&p, &names(&["x1"])).unwrap();
        apply_fixed(&mut k, &mut f, &fixed);
        let modes = rigid_modes(&m.points, &fixed_mask(k.rows, &fixed));
        let mut u = vec![0.0; f.len()];
        solve(k, &f, &mut u, 3, modes, 1e-10).unwrap();
        Solved { p, u }
    };
    let reference = solve_box(Prepared::from_tets(
        &body,
        structured_box([60.0, 40.0, 10.0], 1.0, 0),
    ));
    let rloc = Locator::new(&reference.p.mesh);
    let s = solve_box(Prepared::new(&body, 2.0, &|| false).unwrap());
    let m = &s.p.mesh;
    let loc = Locator::new(m);
    let grid: Vec<P3> = (0..=116)
        .flat_map(|i| {
            (0..=152).map(move |j| [30.0 + i as f64 * 0.25, 1.0 + j as f64 * 0.25, 10.0 - 1e-6])
        })
        .collect();
    let rv: Vec<Option<f64>> = grid
        .iter()
        .map(|q| sample_exact(&reference, &rloc, *q, e_mod, nu).map(|s| von_mises(&s)))
        .collect();
    let vmax = rv.iter().flatten().copied().fold(0.0, f64::max);
    writeln!(out, "## 固定した箱 60×40×10(形に沿う網 h 2、要素 {})。参照は規則正しい網 h 1、上の面の最大 {vmax:.3} MPa。誤差は最大に対する %\n| 候補 | 縁から 0.5 mm のむら RMS / 最大 | 1 mm のむら RMS / 最大 | 縁から 1〜5 mm の誤差 RMS / 最大 | 5〜30 mm の誤差 RMS / 最大 | 上の面の節点の最大 MPa |\n|---|---|---|---|---|---|", m.tets.len()).unwrap();
    let on_top: Vec<usize> = (0..m.points.len())
        .filter(|n| m.points[*n][2] > 10.0 - 1e-6)
        .collect();
    for (name, rec) in candidates(&s.p, e_mod, nu, &s.u) {
        let mut cells = Vec::new();
        for d in [0.5, 1.0] {
            let v: Vec<f64> = (0..=300)
                .filter_map(|i| {
                    sample_s6(
                        m,
                        &loc,
                        &rec,
                        [60.0 - d - 1e-6, 5.0 + i as f64 * 0.1, 10.0 - 1e-6],
                    )
                })
                .map(|s| von_mises(&s))
                .collect();
            let (r, x) = jag_numbers(&v);
            cells.push(format!("{r:.2} / {x:.2}"));
        }
        for (lo, hi) in [(54.0, 59.0), (30.0, 55.0)] {
            let e: Vec<f64> = grid
                .iter()
                .zip(&rv)
                .filter(|(q, _)| q[0] >= lo && q[0] <= hi)
                .filter_map(|(q, r)| {
                    let v = von_mises(&sample_s6(m, &loc, &rec, *q)?);
                    Some(((v - (*r)?) / vmax * 100.0).abs())
                })
                .collect();
            let rms = (e.iter().map(|x| x * x).sum::<f64>() / e.len() as f64).sqrt();
            cells.push(format!(
                "{rms:.2} / {:.2}",
                e.iter().copied().fold(0.0, f64::max)
            ));
        }
        let top = on_top
            .iter()
            .map(|n| von_mises(&rec[*n]))
            .fold(0.0, f64::max);
        writeln!(out, "| {name} | {} | {top:.3} |", cells.join(" | ")).unwrap();
    }
    // 見本の部品
    let part = read_mesh(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/testdata/sample_part_surface.txt"
    ));
    let coarse = solve_plate(&part, 2.0);
    let fine = solve_plate(&part, 1.3);
    let floc = Locator::new(&fine.p.mesh);
    let m = &coarse.p.mesh;
    let refv: Vec<Option<f64>> = m
        .points
        .iter()
        .map(|q| sample_exact(&fine, &floc, *q, e_mod, nu).map(|s| von_mises(&s)))
        .collect();
    let vmax = refv.iter().flatten().copied().fold(0.0, f64::max);
    let mut on_b = vec![false; m.points.len()];
    for f in &m.faces {
        for v in f.nodes {
            on_b[v as usize] = true;
        }
    }
    let fixed = coarse.p.faces_of(&names(&["f8:side:e6"])).unwrap();
    let sing = crate::singular::find(&coarse.p, &fixed, 2);
    let ref_away = (0..m.points.len())
        .filter(|n| !sing.near[*n])
        .filter_map(|n| refv[n])
        .fold(0.0, f64::max);
    writeln!(out, "\n## 見本の部品(網 h 2、要素 {})。参照は h 1.3(要素 {})、最大 {vmax:.3} MPa、特異点の印から 2 層より離れた節点の最大 {ref_away:.3}。表面の節点の誤差、最大に対する %(特異点の近くを除く)\n| 候補 | 角 RMS / 99% / 最大 | 中点 RMS / 99% / 最大 | 表面の節点の最大 MPa | 特異点を除いた最大 MPa |\n|---|---|---|---|---|", m.tets.len(), fine.p.mesh.tets.len()).unwrap();
    for (name, rec) in candidates(&coarse.p, e_mod, nu, &coarse.u) {
        let mut cells = Vec::new();
        for corner in [true, false] {
            let mut e: Vec<f64> = (0..m.points.len())
                .filter(|n| on_b[*n] && (*n < m.corners) == corner && !sing.near[*n])
                .filter_map(|n| Some(((von_mises(&rec[n]) - refv[n]?) / vmax * 100.0).abs()))
                .collect();
            let rms = (e.iter().map(|x| x * x).sum::<f64>() / e.len() as f64).sqrt();
            cells.push(format!(
                "{rms:.2} / {:.2} / {:.2}",
                pct(&mut e, 0.99),
                pct(&mut e, 1.0)
            ));
        }
        let top = (0..m.points.len())
            .filter(|n| on_b[*n])
            .map(|n| von_mises(&rec[n]))
            .fold(0.0, f64::max);
        let away = (0..m.points.len())
            .filter(|n| !sing.near[*n])
            .map(|n| von_mises(&rec[n]))
            .fold(0.0, f64::max);
        writeln!(
            out,
            "| {name} | {} | {top:.3} | {away:.3} |",
            cells.join(" | ")
        )
        .unwrap();
    }
}

/// 曲面の上の辺の中点: 回してから平均(A 回す)・勾配の差だけ(E)・回してから勾配の差を足す
/// (E 回す)を、厚肉の円筒の内と外の面の周の応力で比べる(正解に対する %)。
#[test]
#[ignore = "調べ物の道具(確かめはせず、表を書くだけ)。--nocapture で手で流す"]
fn midpoints_on_curved_faces() {
    for h in [0.9, 0.6] {
        let c = lame(h);
        let p = &c.p;
        let m = &p.mesh;
        let en = elem_nodal(p, c.e, c.nu, &c.u);
        let a_cur = r_corner(p, &en);
        let e_her = r_hermite(p, &en);
        let turns = crate::statics::surface_mid_turns(m);
        let mut a_rot = a_cur.clone();
        let mut e_rot = e_her.clone();
        for (mid, a, b, ra, rb) in &turns {
            let (mid, a, b) = (*mid as usize, *a as usize, *b as usize);
            let sa = crate::statics::turn(&a_cur[a], ra);
            let sb = crate::statics::turn(&a_cur[b], rb);
            let avg: S6 = std::array::from_fn(|i| 0.5 * (sa[i] + sb[i]));
            a_rot[mid] = avg;
            let plain: S6 = std::array::from_fn(|i| 0.5 * (a_cur[a][i] + a_cur[b][i]));
            e_rot[mid] = std::array::from_fn(|i| avg[i] + e_her[mid][i] - plain[i]);
        }
        let r_of = |q: P3| (q[0] * q[0] + q[1] * q[1]).sqrt();
        let hoop = |q: P3, s: &S6| {
            let r2 = q[0] * q[0] + q[1] * q[1];
            (s[0] * q[1] * q[1] + s[1] * q[0] * q[0] - 2.0 * s[3] * q[0] * q[1]) / r2
        };
        let ex_hoop = |q: P3| hoop(q, &(c.exact)(q));
        eprintln!("h {h}: 要素 {}", m.tets.len());
        for (side, rad) in [("内", 5.0), ("外", 10.0)] {
            for (name, s) in [
                ("A 回す", &a_rot),
                ("A 回さない", &a_cur),
                ("E", &e_her),
                ("E 回す", &e_rot),
            ] {
                let mut mids = Vec::new();
                let mut cors = Vec::new();
                for n in 0..m.points.len() {
                    let q = m.points[n];
                    if (r_of(q) - rad).abs() > 5e-3 || q[2] < 1e-6 || q[2] > 3.0 - 1e-6 {
                        continue;
                    }
                    let e = (hoop(q, &s[n]) / ex_hoop(q) - 1.0) * 100.0;
                    if n < m.corners {
                        cors.push(e)
                    } else {
                        mids.push(e)
                    }
                }
                let st = |v: &[f64]| {
                    let mean = v.iter().sum::<f64>() / v.len() as f64;
                    let rms = (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt();
                    let mx = v.iter().map(|x| x.abs()).fold(0.0, f64::max);
                    format!("平均 {mean:+.3}% RMS {rms:.3}% 最大 {mx:.3}%")
                };
                eprintln!("  {side}の面 {name}: 角 {} / 中点 {}", st(&cors), st(&mids));
            }
        }
    }
}

/// 色の帯の境の波打ち(辺の中点の値の確かめ): 厚肉の円筒の平らな上の面では、正解のミーゼス応力は
/// 半径だけで決まり、帯の境は円になる。画面と同じ応力の出し方と塗り方(8 つの 1 次の四面体で成分を
/// 補間)で、同じ半径の輪の上の値の振れを 40 段の段の幅で測る。中点を両端の角の平均にしていた時は
/// 振れの中央が 0.54 段、中点の誤差の平均が半径の向きの辺で +0.17 段・周の向きの辺で −0.21 段。
#[test]
fn band_borders_on_a_flat_face_stay_round() {
    let c = lame(0.9);
    let p = &c.p;
    let m = &p.mesh;
    let (ns, _, _) = crate::statics::stresses(p, c.nu, &c.u, |_| c.e);
    let (a, b, l) = (5.0, 10.0, 3.0);
    let vm_ex = |x: P3| von_mises(&(c.exact)(x));
    let step = (vm_ex([a, 0.0, 0.0]) - vm_ex([b, 0.0, 0.0])) / 40.0;
    // 辺の中点の誤差の偏り(辺の向きごと)
    let (mut rad, mut circ) = (Vec::new(), Vec::new());
    for t in &m.tets {
        for (k, [ea, eb]) in EDGES.iter().enumerate() {
            let (xa, xb) = (m.points[t[*ea] as usize], m.points[t[*eb] as usize]);
            let mid = t[4 + k] as usize;
            let xm = m.points[mid];
            let e = [0, 1, 2].map(|i| xb[i] - xa[i]);
            let r = (xm[0] * xm[0] + xm[1] * xm[1]).sqrt();
            let el = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
            let along_r = (e[0] * xm[0] + e[1] * xm[1]).abs() / (r * el);
            let along_t = (-e[0] * xm[1] + e[1] * xm[0]).abs() / (r * el);
            let err = (von_mises(&ns[mid]) - vm_ex(xm)) / step;
            if along_r > 0.9 {
                rad.push(err);
            } else if along_t > 0.9 {
                circ.push(err);
            }
        }
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (mr, mc) = (mean(&rad), mean(&circ));
    // 上の面の輪の上の振れ(縁から h より内)
    let loc = Locator::new(m);
    let mut ptp: Vec<f64> = (0..60)
        .map(|i| {
            let r = a + 0.9 + (b - a - 1.8) * i as f64 / 59.0;
            let v: Vec<f64> = (0..360)
                .filter_map(|j| {
                    let th = j as f64 * std::f64::consts::TAU / 360.0;
                    let q = [r * th.cos(), r * th.sin(), l - 1e-6];
                    Some((von_mises(&sample_s6(m, &loc, &ns, q)?) - vm_ex(q)) / step)
                })
                .collect();
            assert!(v.len() > 350, "半径 {r} の輪の点 {}", v.len());
            v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min)
        })
        .collect();
    let med = pct(&mut ptp, 0.5);
    eprintln!(
        "中点の偏り 半径の向き {mr:+.3} 段・周の向き {mc:+.3} 段、輪の振れの中央 {med:.3} 段"
    );
    assert!(
        mr.abs() < 0.08 && mc.abs() < 0.08,
        "中点の偏り {mr:+.3} / {mc:+.3} 段"
    );
    assert!(med < 0.35, "帯の境の波 {med:.3} 段");
}
