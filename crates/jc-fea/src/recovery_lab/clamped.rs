//! 一端を固定した箱と円筒: 細かい網の解を参照にして、固定した縁のまわりの誤差とギザギザを測る。

use super::*;

#[test]
#[ignore = "調べ物の道具(確かめはせず、表と絵を書くだけ)。JC_LAB_DIR などで手で流す"]
fn plate_against_a_fine_mesh() {
    let dir = std::env::var("JC_LAB_DIR").unwrap();
    let mesh = read_mesh(&format!("{dir}/plate.txt"));
    let (e_mod, nu) = (69_000.0, 0.33);
    let href: f64 = std::env::var("JC_LAB_HREF")
        .map(|s| s.parse().unwrap())
        .unwrap_or(1.0);
    let coarse = solve_plate(&mesh, 2.0);
    let fine = solve_plate(&mesh, href);
    let floc = Locator::new(&fine.p.mesh);
    let cloc = Locator::new(&coarse.p.mesh);
    let p = &coarse.p;
    let m = &p.mesh;
    let en = elem_nodal(p, e_mod, nu, &coarse.u);
    let gs = gauss_samples(p, e_mod, nu, &coarse.u);
    let methods: Vec<(&str, Vec<S6>)> = vec![
        ("数で平均", r_avg(p, &en)),
        ("SPR 2 次", r_spr(p, &gs, true)),
    ];
    // 参照の解(細かい網の要素の中の値)
    let refv: Vec<Option<S6>> = m
        .points
        .iter()
        .map(|q| sample_exact(&fine, &floc, *q, e_mod, nu))
        .collect();
    let maxvm = refv
        .iter()
        .flatten()
        .map(|s| von_mises(s))
        .fold(0.0, f64::max);
    let mut out = std::fs::File::create(format!("{dir}/plate.md")).unwrap();
    let mut on_b = vec![false; m.points.len()];
    for f in &m.faces {
        for v in f.nodes {
            on_b[v as usize] = true;
        }
    }
    writeln!(out, "## 見本の部品 h 2(要素 {})を h {href}(要素 {})と比べる。誤差はミーゼスの差を参照の最大 {maxvm:.3} MPa で割った %", m.tets.len(), fine.p.mesh.tets.len()).unwrap();
    writeln!(
        out,
        "| 作り方・節点 | 数 | RMS | 中央 | 95% | 最大 |\n|---|---|---|---|---|---|"
    )
    .unwrap();
    let near_fix = |q: P3| q[0] > 60.0 - 4.0; // 固定した +X の面から 4 mm 以内
    for (name, s) in &methods {
        for (label, corner, bnd) in [
            ("角・中", true, false),
            ("中点・中", false, false),
            ("角・表面", true, true),
            ("中点・表面", false, true),
        ] {
            for (zone, far) in [
                ("固定から 4 mm より遠い", true),
                ("固定から 4 mm 以内", false),
            ] {
                let mut v: Vec<f64> = (0..m.points.len())
                    .filter(|n| {
                        (*n < m.corners) == corner
                            && on_b[*n] == bnd
                            && near_fix(m.points[*n]) != far
                    })
                    .filter_map(|n| {
                        refv[n].map(|r| (von_mises(&s[n]) - von_mises(&r)).abs() / maxvm * 100.0)
                    })
                    .collect();
                let rms = (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt();
                writeln!(
                    out,
                    "| {name} {label} {zone} | {} | {rms:.2} | {:.2} | {:.2} | {:.2} |",
                    v.len(),
                    pct(&mut v, 0.5),
                    pct(&mut v, 0.95),
                    pct(&mut v, 1.0)
                )
                .unwrap();
            }
        }
    }
    let vm_of = |s: &Vec<S6>| s.iter().map(|x| von_mises(x)).collect::<Vec<f64>>();
    let fine_avg = r_avg(&fine.p, &elem_nodal(&fine.p, e_mod, nu, &fine.u));
    // いちばん外れた節点
    let mut worst: Vec<(f64, usize)> = (0..m.points.len())
        .filter_map(|n| refv[n].map(|r| ((von_mises(&methods[0].1[n]) - von_mises(&r)).abs(), n)))
        .collect();
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    writeln!(
        out,
        "\n外れの大きい節点(数で平均): 差 MPa・位置・平均・参照"
    )
    .unwrap();
    for (d, n) in worst.iter().take(12) {
        let q = m.points[*n];
        writeln!(
            out,
            "- {d:.3} @ ({:.2}, {:.2}, {:.2}) 角 {} 平均 {:.3} 参照 {:.3}",
            q[0],
            q[1],
            q[2],
            *n < m.corners,
            von_mises(&methods[0].1[*n]),
            von_mises(&refv[*n].unwrap())
        )
        .unwrap();
    }
    // 固定した縁(x = 60、上の面)に沿った値
    let zt = 10.0;
    for (label, dx) in [
        ("縁の上", 0.0),
        ("縁から 1 mm 内", 1.0),
        ("縁から 2 mm 内", 2.0),
    ] {
        let mut row: Vec<(f64, f64, f64, Option<f64>)> = Vec::new();
        for y in 0..=80 {
            let q = [60.0 - dx - 1e-6, y as f64 * 0.5, zt - 1e-6];
            let a = sample_linear(m, &cloc, &vm_of(&methods[0].1), q);
            let b = sample_linear(m, &cloc, &vm_of(&methods[1].1), q);
            let r = sample_linear(&fine.p.mesh, &floc, &vm_of(&fine_avg), q);
            if let (Some(a), Some(b)) = (a, b) {
                row.push((q[1], a, b, r));
            }
        }
        writeln!(out, "\n{label}(y, 数で平均, SPR, 参照):").unwrap();
        for (y, a, b, r) in row {
            writeln!(
                out,
                "{y:.1} {a:.3} {b:.3} {}",
                r.map_or("-".into(), |r| format!("{r:.3}"))
            )
            .unwrap();
        }
    }
    // 断面の絵: z の真ん中、x 0..60、y 0..40
    let zmid = std::env::var("JC_LAB_Z")
        .map(|s| s.parse().unwrap())
        .unwrap_or(5.0);
    let (w, hgt, px_mm) = (600usize, 400usize, 10.0);
    let mut fields: Vec<(String, Vec<Option<f64>>)> = Vec::new();
    let px_of = |i: usize| {
        [
            (i % w) as f64 / px_mm,
            (hgt - 1 - i / w) as f64 / px_mm,
            zmid,
        ]
    };
    for (name, s) in &methods {
        let v = vm_of(s);
        fields.push((
            format!("{name}・ミーゼスを補間"),
            (0..w * hgt)
                .map(|i| sample_linear(m, &cloc, &v, px_of(i)))
                .collect(),
        ));
    }
    for (name, s) in &methods {
        let cols: Vec<Vec<f64>> = (0..6).map(|k| s.iter().map(|x| x[k]).collect()).collect();
        fields.push((
            format!("{name}・成分を補間"),
            (0..w * hgt)
                .map(|i| sample_tensor_vm(m, &cloc, &cols, px_of(i)))
                .collect(),
        ));
    }
    fields.push((
        "参照(細かい網の要素の値)".into(),
        (0..w * hgt)
            .map(|i| sample_exact(&fine, &floc, px_of(i), e_mod, nu).map(|x| von_mises(&x)))
            .collect(),
    ));
    let nref = fields.len() - 1;
    // 断面の上の誤差の数(参照の最大で割った %)
    for (name, f) in fields.iter().take(nref) {
        let mut v: Vec<f64> = f
            .iter()
            .zip(&fields[nref].1)
            .filter_map(|(a, b)| Some((a.as_ref()? - b.as_ref()?).abs() / maxvm * 100.0))
            .collect();
        let rms = (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt();
        writeln!(
            out,
            "断面の誤差 {name}: RMS {rms:.3}% 95% {:.3}% 最大 {:.3}%",
            pct(&mut v, 0.95),
            pct(&mut v, 1.0)
        )
        .unwrap();
    }
    let (lo, hi) = fields
        .iter()
        .flat_map(|f| f.1.iter().flatten())
        .fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    writeln!(out, "\n断面 z = {zmid}: 値の範囲 {lo:.3}〜{hi:.3} MPa").unwrap();
    for (k, (name, f)) in fields.iter().enumerate() {
        for banded in [true, false] {
            let px: Vec<[u8; 3]> = f
                .iter()
                .map(|v| match v {
                    Some(v) => {
                        let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
                        let t = if banded {
                            ((t * 40.0).floor().min(39.0)) / 39.0
                        } else {
                            t
                        };
                        jc_study::view::ramp(t as f32)
                    }
                    None => [40, 44, 48],
                })
                .collect();
            write_png_ppm(
                &format!("{dir}/sec{k}{}.ppm", if banded { "b" } else { "s" }),
                w,
                hgt,
                &px,
            );
        }
        writeln!(out, "sec{k}: {name}").unwrap();
    }
    // 誤差の絵(数で平均 − 参照)
    for (k, (name, f)) in fields.iter().enumerate().take(nref) {
        let px: Vec<[u8; 3]> = f
            .iter()
            .zip(&fields[nref].1)
            .map(|(a, b)| match (a, b) {
                (Some(a), Some(b)) => {
                    let t = (0.5 + (a - b) / (0.04 * maxvm)).clamp(0.0, 1.0);
                    jc_study::view::ramp(t as f32)
                }
                _ => [40, 44, 48],
            })
            .collect();
        write_png_ppm(&format!("{dir}/err{k}.ppm"), w, hgt, &px);
        writeln!(out, "err{k}: {name} − 参照(±2% of max で色)").unwrap();
    }
}

#[test]
#[ignore = "調べ物の道具(確かめはせず、表と絵を書くだけ)。JC_LAB_DIR などで手で流す"]
fn plate_top_face_near_the_clamp() {
    let dir = std::env::var("JC_LAB_DIR").unwrap();
    let mesh = read_mesh(&format!("{dir}/plate.txt"));
    let (e_mod, nu) = (69_000.0, 0.33);
    let href: f64 = std::env::var("JC_LAB_HREF")
        .map(|s| s.parse().unwrap())
        .unwrap_or(1.0);
    let coarse = solve_plate(&mesh, 2.0);
    let fine = solve_plate(&mesh, href);
    let floc = Locator::new(&fine.p.mesh);
    let cloc = Locator::new(&coarse.p.mesh);
    let p = &coarse.p;
    let m = &p.mesh;
    let avg = r_avg(p, &elem_nodal(p, e_mod, nu, &coarse.u));
    let vm: Vec<f64> = avg.iter().map(|s| von_mises(s)).collect();
    let cols: Vec<Vec<f64>> = (0..6).map(|k| avg.iter().map(|x| x[k]).collect()).collect();
    // 上の面(z = 10)のすぐ下、x 36..60、y 0..40、1 mm に 20 画素。手前の横の面(y = 0)は y の代わりに z
    let (x0, px_mm) = (36.0, 20.0);
    let (w, hgt) = (((60.0 - x0) * px_mm) as usize, (40.0 * px_mm) as usize);
    let fields: Vec<(&str, Box<dyn Fn(P3) -> Option<f64>>)> = vec![
        (
            "ミーゼスを線形に補間",
            Box::new(|q| sample_linear(m, &cloc, &vm, q)),
        ),
        (
            "成分を線形に補間",
            Box::new(|q| sample_tensor_vm(m, &cloc, &cols, q)),
        ),
        (
            "成分を 2 次で補間",
            Box::new(|q| {
                let (e, xi) = locate(m, &cloc, q)?;
                let n = tet10::shape(xi);
                let mut c = [0.0; 6];
                for a in 0..10 {
                    for k in 0..6 {
                        c[k] += n[a] * avg[m.tets[e][a] as usize][k];
                    }
                }
                Some(von_mises(&c))
            }),
        ),
        (
            "要素の値(平均しない)",
            Box::new(|q| {
                let (e, xi) = locate(m, &cloc, q)?;
                Some(von_mises(&tet10::stress_at(
                    &m.tet_points(e),
                    e_mod,
                    nu,
                    &element_u(p, &coarse.u, e),
                    xi,
                )))
            }),
        ),
        (
            "参照(細かい網の要素の値)",
            Box::new(|q| sample_exact(&fine, &floc, q, e_mod, nu).map(|s| von_mises(&s))),
        ),
    ];
    let mut out = std::fs::File::create(format!("{dir}/top.md")).unwrap();
    for (view, to_q) in [
        (
            "top",
            Box::new(move |i: usize| {
                [
                    x0 + (i % w) as f64 / px_mm,
                    (hgt - 1 - i / w) as f64 / px_mm,
                    10.0 - 1e-6,
                ]
            }) as Box<dyn Fn(usize) -> P3>,
        ),
        (
            "front",
            Box::new(move |i: usize| {
                [
                    x0 + (i % w) as f64 / px_mm,
                    1e-6,
                    (hgt / 4 - 1 - i / w) as f64 / px_mm,
                ]
            }) as Box<dyn Fn(usize) -> P3>,
        ),
    ] {
        let rows = if view == "top" { hgt } else { hgt / 4 };
        let vals: Vec<Vec<Option<f64>>> = fields
            .iter()
            .map(|(_, f)| (0..w * rows).map(|i| f(to_q(i))).collect())
            .collect();
        let (lo, hi) = (0.0, 4.69);
        for (k, (name, _)) in fields.iter().enumerate() {
            let px: Vec<[u8; 3]> = vals[k]
                .iter()
                .map(|v| match v {
                    Some(v) => {
                        let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
                        jc_study::view::ramp((((t * 40.0).floor().min(39.0)) / 39.0) as f32)
                    }
                    None => [40, 44, 48],
                })
                .collect();
            write_png_ppm(&format!("{dir}/{view}{k}.ppm"), w, rows, &px);
            writeln!(out, "{view}{k}: {name}").unwrap();
        }
    }
}

/// 固定した縁のギザギザ: 板と同じ大きさの箱(60×40×10、+X の面を固定、上の面を下へ 100 N)で、
/// 網の大きさを変えて固定した縁に沿う値を測る。
#[test]
#[ignore = "調べ物の道具(確かめはせず、表と絵を書くだけ)。JC_LAB_DIR などで手で流す"]
fn clamped_edge_profile() {
    let dir = std::env::var("JC_LAB_DIR").unwrap();
    let mut out = std::fs::File::create(format!(
        "{dir}/edge_{}.md",
        std::env::var("JC_LAB_REC").unwrap_or_default()
    ))
    .unwrap();
    let (e_mod, nu) = (69_000.0, 0.33);
    let hs: Vec<f64> = std::env::var("JC_LAB_HS")
        .map(|s| s.split(',').map(|x| x.parse().unwrap()).collect())
        .unwrap_or(vec![2.0, 2.17, 1.0]);
    for h in hs {
        let p = Prepared::new(&box_mesh([60.0, 40.0, 10.0]), h, &|| false).unwrap();
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
        let en = elem_nodal(&p, e_mod, nu, &u);
        let avg = match std::env::var("JC_LAB_REC").as_deref() {
            Ok("corner") => r_corner(&p, &en),
            Ok("spr") => r_spr(&p, &gauss_samples(&p, e_mod, nu, &u), true),
            _ => r_avg(&p, &en),
        };
        let cols: Vec<Vec<f64>> = (0..6).map(|c| avg.iter().map(|x| x[c]).collect()).collect();
        let loc = Locator::new(m);
        writeln!(out, "\n## h {h}(要素 {})", m.tets.len()).unwrap();
        for d in [0.0, 0.5, 1.0, 2.0, 4.0] {
            let v: Vec<f64> = (0..=300)
                .filter_map(|i| {
                    sample_tensor_vm(
                        m,
                        &loc,
                        &cols,
                        [60.0 - d - 1e-6, 5.0 + i as f64 * 0.1, 10.0 - 1e-6],
                    )
                })
                .collect();
            writeln!(out, "{}", jag(d, &v)).unwrap();
        }
        // 上の面の絵(x 48..60、y 0..40、1 mm に 20 画素)
        let (x0, px_mm) = (48.0, 20.0);
        let (w, hgt) = (((60.0 - x0) * px_mm) as usize, (40.0 * px_mm) as usize);
        let px: Vec<[u8; 3]> = (0..w * hgt)
            .map(|i| {
                let q = [
                    x0 + (i % w) as f64 / px_mm,
                    (hgt - 1 - i / w) as f64 / px_mm,
                    10.0 - 1e-6,
                ];
                match sample_tensor_vm(m, &loc, &cols, q) {
                    Some(v) => {
                        let t = (v / 6.0).clamp(0.0, 1.0);
                        jc_study::view::ramp((((t * 40.0).floor().min(39.0)) / 39.0) as f32)
                    }
                    None => [40, 44, 48],
                }
            })
            .collect();
        write_png_ppm(
            &format!(
                "{dir}/edge_h{h}_{}.ppm",
                std::env::var("JC_LAB_REC").unwrap_or_default()
            ),
            w,
            hgt,
            &px,
        );
    }
}

/// 構造的な網(揺らし方を変える)で、固定した縁に沿う値の振れを測る。
#[test]
#[ignore = "調べ物の道具(確かめはせず、表と絵を書くだけ)。JC_LAB_DIR などで手で流す"]
fn clamped_edge_on_structured_meshes() {
    let dir = std::env::var("JC_LAB_DIR").unwrap();
    let mut out = std::fs::File::create(format!(
        "{dir}/edge_structured_{}.md",
        std::env::var("JC_LAB_REC").unwrap_or_default()
    ))
    .unwrap();
    let (e_mod, nu) = (69_000.0, 0.33);
    let body = box_mesh([60.0, 40.0, 10.0]);
    let cases: Vec<(f64, u32)> = std::env::var("JC_LAB_CASES")
        .map(|s| {
            s.split(',')
                .map(|c| {
                    let (a, b) = c.split_once(':').unwrap();
                    (a.parse().unwrap(), b.parse().unwrap())
                })
                .collect()
        })
        .unwrap_or(vec![
            (2.0, 0),
            (2.0, 1),
            (2.0, 2),
            (2.0, 3),
            (1.0, 0),
            (1.0, 2),
        ]);
    for (h, jit) in cases {
        let p = Prepared::from_tets(&body, structured_box([60.0, 40.0, 10.0], h, jit));
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
        let en = elem_nodal(&p, e_mod, nu, &u);
        let avg = match std::env::var("JC_LAB_REC").as_deref() {
            Ok("corner") => r_corner(&p, &en),
            Ok("spr") => r_spr(&p, &gauss_samples(&p, e_mod, nu, &u), true),
            _ => r_avg(&p, &en),
        };
        let cols: Vec<Vec<f64>> = (0..6).map(|c| avg.iter().map(|x| x[c]).collect()).collect();
        let loc = Locator::new(m);
        writeln!(out, "\n## 構造的 h {h} 揺らし {jit}(要素 {})", m.tets.len()).unwrap();
        for d in [0.0, 0.5, 1.0, 2.0, 4.0] {
            let v: Vec<f64> = (0..=300)
                .filter_map(|i| {
                    sample_tensor_vm(
                        m,
                        &loc,
                        &cols,
                        [60.0 - d - 1e-6, 5.0 + i as f64 * 0.1, 10.0 - 1e-6],
                    )
                })
                .collect();
            writeln!(out, "{}", jag(d, &v)).unwrap();
        }
        let (x0, px_mm) = (48.0, 20.0);
        let (w, hgt) = (((60.0 - x0) * px_mm) as usize, (40.0 * px_mm) as usize);
        let px: Vec<[u8; 3]> = (0..w * hgt)
            .map(|i| {
                let q = [
                    x0 + (i % w) as f64 / px_mm,
                    (hgt - 1 - i / w) as f64 / px_mm,
                    10.0 - 1e-6,
                ];
                match sample_tensor_vm(m, &loc, &cols, q) {
                    Some(v) => {
                        let t = (v / 6.0).clamp(0.0, 1.0);
                        jc_study::view::ramp((((t * 40.0).floor().min(39.0)) / 39.0) as f32)
                    }
                    None => [40, 44, 48],
                }
            })
            .collect();
        write_png_ppm(
            &format!(
                "{dir}/sedge_h{h}_j{jit}_{}.ppm",
                std::env::var("JC_LAB_REC").unwrap_or_default()
            ),
            w,
            hgt,
            &px,
        );
    }
}

/// 固定した箱の縁のむら(鋭い縁に沿う帯の確かめ): 縁から 0.5・1 mm の上の面の値の、4 mm の移動
/// 平均からの外れが 1% 未満。帯の無い網では 1 mm で 2.6% だった。
#[test]
fn a_clamped_edge_is_smooth() {
    let p = Prepared::new(&box_mesh([60.0, 40.0, 10.0]), 2.0, &|| false).unwrap();
    let m = &p.mesh;
    assert!(m.stats.band_points > 100, "帯の点 {}", m.stats.band_points);
    // 画面と同じ解き方と応力の出し方
    let out = crate::statics::solve_static(
        &p,
        &crate::statics::StaticInput {
            e: 69_000.0,
            nu: 0.33,
            fixed: names(&["x1"]),
            loads: vec![Load {
                faces: names(&["z1"]),
                kind: LoadKind::Force(100.0),
                dir: Dir::Fixed([0.0, 0.0, -1.0]),
            }],
        },
    )
    .unwrap();
    let cols: Vec<Vec<f64>> = (0..6)
        .map(|c| out.node_stress.iter().map(|x| x[c]).collect())
        .collect();
    let loc = Locator::new(m);
    for d in [0.5, 1.0] {
        let v: Vec<f64> = (0..=300)
            .filter_map(|i| {
                sample_tensor_vm(
                    m,
                    &loc,
                    &cols,
                    [60.0 - d - 1e-6, 5.0 + i as f64 * 0.1, 10.0 - 1e-6],
                )
            })
            .collect();
        assert_eq!(v.len(), 301);
        let (rms, max) = jag_numbers(&v);
        eprintln!("縁から {d} mm: RMS {rms:.2}% 最大 {max:.2}%");
        assert!(rms < 1.0, "縁から {d} mm のむら RMS {rms:.2}%");
    }
}

/// 固定した縁の上の節点ごとの値と、まわりの四面体のつながり方(ギザギザの原因の切り分け)。
#[test]
#[ignore = "調べ物の道具(確かめはせず、節点ごとの表を書くだけ)。--nocapture で手で流す"]
fn clamped_edge_nodes() {
    let (e_mod, nu) = (69_000.0, 0.33);
    let p = Prepared::new(&box_mesh([60.0, 40.0, 10.0]), 2.0, &|| false).unwrap();
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
    let en = elem_nodal(&p, e_mod, nu, &u);
    let avg = r_avg(&p, &en);
    let on_x = |q: P3| (q[0] - 60.0).abs() < 1e-6;
    let on_z = |q: P3| (q[2] - 10.0).abs() < 1e-6;
    let mut nodes: Vec<usize> = (0..m.corners)
        .filter(|i| on_x(m.points[*i]) && on_z(m.points[*i]))
        .collect();
    nodes.sort_by(|a, b| m.points[*a][1].total_cmp(&m.points[*b][1]));
    eprintln!(
        "y | ミーゼス(平均) | 四面体 | 固定面に面を持つ | 上の面に面を持つ | 両方 | 要素の値の幅"
    );
    for &n in &nodes {
        let y = m.points[n][1];
        if !(8.0..=32.0).contains(&y) {
            continue;
        }
        let (mut cnt, mut fx, mut fz, mut both) = (0, 0, 0, 0);
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for (e, t) in m.tets.iter().enumerate() {
            if let Some(a) = t[..4].iter().position(|v| *v as usize == n) {
                cnt += 1;
                let c: Vec<P3> = t[..4].iter().map(|v| m.points[*v as usize]).collect();
                let nx = c.iter().filter(|q| on_x(**q)).count();
                let nz = c.iter().filter(|q| on_z(**q)).count();
                fx += (nx >= 3) as usize;
                fz += (nz >= 3) as usize;
                both += (nx >= 3 && nz >= 3) as usize;
                let v = von_mises(&en[e][a]);
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        eprintln!(
            "{y:.2} | {:.3} | {cnt} | {fx} | {fz} | {both} | {lo:.2}〜{hi:.2}",
            von_mises(&avg[n])
        );
    }
}

/// 上の面の誤差の分布: 細かい規則正しい網(h 1)の解を参照にして、網ごとの誤差(最大応力に対する %)
/// を、縁から 1 mm 以上・5 mm 以上の所に分けて測る。
#[test]
#[ignore = "調べ物の道具(確かめはせず、表と絵を書くだけ)。JC_LAB_DIR などで手で流す"]
fn top_face_error_map() {
    let dir = std::env::var("JC_LAB_DIR").unwrap();
    let (e_mod, nu) = (69_000.0, 0.33);
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
    let href: f64 = std::env::var("JC_LAB_HREF")
        .map(|s| s.parse().unwrap())
        .unwrap_or(1.0);
    let reference = solve_box(Prepared::from_tets(
        &body,
        structured_box([60.0, 40.0, 10.0], href, 0),
    ));
    let rloc = Locator::new(&reference.p.mesh);
    let mut cases: Vec<(String, Prepared)> = vec![(
        "形に沿う網 h 2".into(),
        Prepared::new(&body, 2.0, &|| false).unwrap(),
    )];
    if std::env::var("JC_LAB_STRUCT").is_ok() {
        cases.push((
            "規則正しい網 h 2".into(),
            Prepared::from_tets(&body, structured_box([60.0, 40.0, 10.0], 2.0, 0)),
        ));
        cases.push((
            "点を崩した網 h 2".into(),
            Prepared::from_tets(&body, structured_box([60.0, 40.0, 10.0], 2.0, 2)),
        ));
    }
    let mut out = std::fs::File::create(format!("{dir}/error_map.md")).unwrap();
    let step = 0.25;
    let grid: Vec<P3> = (0..=((59.0 - 30.0) / step) as usize)
        .flat_map(|i| {
            (0..=((39.0 - 1.0) / step) as usize)
                .map(move |j| [30.0 + i as f64 * step, 1.0 + j as f64 * step, 10.0 - 1e-6])
        })
        .collect();
    let rv: Vec<Option<f64>> = grid
        .iter()
        .map(|q| sample_exact(&reference, &rloc, *q, e_mod, nu).map(|s| von_mises(&s)))
        .collect();
    let vmax = rv.iter().flatten().copied().fold(0.0, f64::max);
    writeln!(
        out,
        "参照: 規則正しい網 h {href}(要素 {})、上の面の最大 {vmax:.3} MPa",
        reference.p.mesh.tets.len()
    )
    .unwrap();
    for (name, p) in cases {
        let s = solve_box(p);
        let p = &s.p;
        let m = &p.mesh;
        let en = elem_nodal(p, e_mod, nu, &s.u);
        let avg = match std::env::var("JC_LAB_REC").as_deref() {
            Ok("avg") => r_avg(p, &en),
            _ => r_corner(p, &en),
        };
        let cols: Vec<Vec<f64>> = (0..6).map(|c| avg.iter().map(|x| x[c]).collect()).collect();
        let loc = Locator::new(m);
        for (band, lo, hi) in [
            ("縁から 1〜5 mm", 54.0, 59.0),
            ("縁から 5〜30 mm", 30.0, 55.0),
        ] {
            let mut e: Vec<f64> = grid
                .iter()
                .zip(&rv)
                .filter(|(q, _)| q[0] >= lo && q[0] <= hi)
                .filter_map(|(q, r)| {
                    let v = sample_tensor_vm(m, &loc, &cols, *q)?;
                    Some(((v - (*r)?) / vmax * 100.0).abs())
                })
                .collect();
            let rms = (e.iter().map(|x| x * x).sum::<f64>() / e.len() as f64).sqrt();
            let p95 = pct(&mut e, 0.95);
            let mx = e.iter().copied().fold(0.0, f64::max);
            writeln!(
                out,
                "{name}(要素 {}) {band}: 誤差 RMS {rms:.2}% 95% 点 {p95:.2}% 最大 {mx:.2}%",
                m.tets.len()
            )
            .unwrap();
        }
    }
}

/// 固定した円柱の根元の輪の縁(曲がった縁)のむら: 円柱(半径 10、高さ 40)の底を固定し、上の面を
/// 横へ 100 N 押す。側面の上で、底から d の所の値を周に沿って測り、滑らかな式
/// (1・cos θ・sin θ・cos 2θ・sin 2θ)に合わせた残りの RMS を、平均に対する % で書く。
#[test]
#[ignore = "調べ物の道具(確かめはせず、表を書くだけ)。--nocapture で手で流す"]
fn clamped_ring_profile() {
    let (e_mod, nu) = (69_000.0, 0.33);
    let h: f64 = std::env::var("JC_LAB_H")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(2.0);
    let seg: usize = std::env::var("JC_LAB_SEG")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(96);
    let p = Prepared::new(&annulus(0.0, 10.0, 40.0, seg, 8), h, &|| false).unwrap();
    let m = &p.mesh;
    let mut k = assemble(m, 3, |el| {
        Some(tet10::stiffness(&m.tet_points(el), e_mod, nu))
    });
    let mut f = load_vector(
        &p,
        &[Load {
            faces: names(&["top"]),
            kind: LoadKind::Force(100.0),
            dir: Dir::Fixed([1.0, 0.0, 0.0]),
        }],
    )
    .unwrap();
    let fixed = fixed_dofs(&p, &names(&["bottom"])).unwrap();
    apply_fixed(&mut k, &mut f, &fixed);
    let modes = rigid_modes(&m.points, &fixed_mask(k.rows, &fixed));
    let mut u = vec![0.0; f.len()];
    solve(k, &f, &mut u, 3, modes, 1e-10).unwrap();
    let en = elem_nodal(&p, e_mod, nu, &u);
    let avg = r_corner(&p, &en);
    let cols: Vec<Vec<f64>> = (0..6).map(|c| avg.iter().map(|x| x[c]).collect()).collect();
    let loc = Locator::new(m);
    eprintln!("h {h}(要素 {})", m.tets.len());
    if std::env::var("JC_LAB_NODES").is_ok() {
        let mut nodes: Vec<(f64, usize)> = (0..m.corners)
            .filter(|i| {
                let q = m.points[*i];
                q[2].abs() < 1e-6 && (q[0] * q[0] + q[1] * q[1]).sqrt() > 9.9
            })
            .map(|i| (m.points[i][1].atan2(m.points[i][0]).to_degrees(), i))
            .filter(|(a, _)| a.abs() < 80.0)
            .collect();
        nodes.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (a, n) in nodes {
            let cnt = m
                .tets
                .iter()
                .filter(|t| t[..4].contains(&(n as u32)))
                .count();
            eprintln!("{a:.1}° 四面体 {cnt} 値 {:.3}", von_mises(&avg[n]));
        }
    }
    for d in [0.0, 0.5, 1.0, 2.0, 4.0] {
        let (n, mean, rms, max) = ring_jag(m, &loc, &cols, d);
        eprintln!("底から {d} mm: 点 {n} 平均 {mean:.3} むら RMS {rms:.2}% 最大 {max:.2}%");
    }
}

/// 固定した円柱の根元の輪の縁(曲がった縁の帯の確かめ): 底から 0.5・1 mm の側面の値の、滑らかな
/// 式からの外れが 1.5% 未満。帯の無い網では 1 mm で 3.0% だった。
#[test]
fn a_clamped_round_edge_is_smooth() {
    let p = Prepared::new(&annulus(0.0, 10.0, 40.0, 96, 8), 2.0, &|| false).unwrap();
    let m = &p.mesh;
    let out = crate::statics::solve_static(
        &p,
        &crate::statics::StaticInput {
            e: 69_000.0,
            nu: 0.33,
            fixed: names(&["bottom"]),
            loads: vec![Load {
                faces: names(&["top"]),
                kind: LoadKind::Force(100.0),
                dir: Dir::Fixed([1.0, 0.0, 0.0]),
            }],
        },
    )
    .unwrap();
    let cols: Vec<Vec<f64>> = (0..6)
        .map(|c| out.node_stress.iter().map(|x| x[c]).collect())
        .collect();
    let loc = Locator::new(m);
    for d in [0.5, 1.0] {
        let (n, _, rms, max) = ring_jag(m, &loc, &cols, d);
        assert_eq!(n, 301);
        eprintln!("底から {d} mm: RMS {rms:.2}% 最大 {max:.2}%");
        assert!(rms < 1.5, "底から {d} mm のむら RMS {rms:.2}%");
    }
}
