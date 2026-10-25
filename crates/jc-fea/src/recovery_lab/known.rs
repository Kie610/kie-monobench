//! 理論解のある見本(片持ち梁・厚肉の円筒の内圧)で、作り方ごとの誤差を表にする。

use super::*;

pub(super) struct Case {
    pub(super) name: &'static str,
    pub(super) p: Prepared,
    pub(super) e: f64,
    pub(super) nu: f64,
    pub(super) u: Vec<f64>,
    pub(super) exact: Box<dyn Fn(P3) -> S6>,
    pub(super) inside: Box<dyn Fn(P3) -> bool>,
    pub(super) scale: f64,
}

pub(super) fn report(c: &Case, out: &mut impl Write) {
    let p = &c.p;
    let m = &p.mesh;
    let en = elem_nodal(p, c.e, c.nu, &c.u);
    let gs = gauss_samples(p, c.e, c.nu, &c.u);
    // 境界の節点
    let mut on_b = vec![false; m.points.len()];
    for f in &m.faces {
        for v in f.nodes {
            on_b[v as usize] = true;
        }
    }
    writeln!(
        out,
        "\n## {}(要素 {}・節点 {}・h {:.2}、誤差は {:.3} MPa で割った %)",
        c.name,
        m.tets.len(),
        m.points.len(),
        p.h(),
        c.scale
    )
    .unwrap();
    // 要素そのもの(平均しない)の誤差: 積分点と節点
    let mut eg = Vec::new();
    let mut enode = Vec::new();
    for e in 0..m.tets.len() {
        for (x, s) in &gs[e] {
            if (c.inside)(*x) {
                eg.push(norm(&diff(s, &(c.exact)(*x))) / c.scale * 100.0);
            }
        }
        for a in 0..10 {
            let x = m.points[m.tets[e][a] as usize];
            if (c.inside)(x) {
                enode.push(norm(&diff(&en[e][a], &(c.exact)(x))) / c.scale * 100.0);
            }
        }
    }
    let line = |out: &mut dyn Write, label: &str, v: &mut Vec<f64>| {
        let rms = (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt();
        writeln!(
            out,
            "| {label} | {} | {rms:.2} | {:.2} | {:.2} | {:.2} |",
            v.len(),
            pct(v, 0.5),
            pct(v, 0.95),
            pct(v, 1.0)
        )
        .unwrap();
    };
    writeln!(
        out,
        "| 何の誤差 | 点の数 | RMS | 中央 | 95% | 最大 |\n|---|---|---|---|---|---|"
    )
    .unwrap();
    line(out, "要素の積分点(平均しない)", &mut eg);
    line(out, "要素の節点(平均しない)", &mut enode);
    let methods: Vec<(&str, Vec<S6>)> = vec![
        ("数で平均", r_avg(p, &en)),
        ("体積で平均", r_vol(p, &en)),
        ("角だけ平均・中点は両端", r_corner(p, &en)),
        ("SPR 1 次", r_spr(p, &gs, false)),
        ("SPR 2 次", r_spr(p, &gs, true)),
        ("L2 の射影", r_l2(p, c.e, c.nu, &c.u)),
    ];
    for (name, s) in &methods {
        for (label, pick) in [
            ("角・中", (true, false)),
            ("中点・中", (false, false)),
            ("角・表面", (true, true)),
            ("中点・表面", (false, true)),
        ] {
            let mut v: Vec<f64> = (0..m.points.len())
                .filter(|n| {
                    (*n < m.corners) == pick.0 && on_b[*n] == pick.1 && (c.inside)(m.points[*n])
                })
                .map(|n| norm(&diff(&s[n], &(c.exact)(m.points[n]))) / c.scale * 100.0)
                .collect();
            line(out, &format!("{name} {label}"), &mut v);
        }
        // 符号付きのミーゼスの差の平均(角と中点で偏りがあるか)
        let bias = |corner: bool| {
            let v: Vec<f64> = (0..m.points.len())
                .filter(|n| (*n < m.corners) == corner && !on_b[*n] && (c.inside)(m.points[*n]))
                .map(|n| (von_mises(&s[n]) - von_mises(&(c.exact)(m.points[n]))) / c.scale * 100.0)
                .collect();
            v.iter().sum::<f64>() / v.len().max(1) as f64
        };
        writeln!(
            out,
            "|   {name} ミーゼスの偏り 角 {:+.2} / 中点 {:+.2} | | | | | |",
            bias(true),
            bias(false)
        )
        .unwrap();
    }
}

pub(super) fn cantilever(h: f64) -> Case {
    let (l, w, e, nu, pf): (f64, f64, f64, f64, f64) = (100.0, 10.0, 70_000.0, 0.0, 100.0);
    let p = Prepared::new(&box_mesh([l as f32, w as f32, w as f32]), h, &|| false).unwrap();
    let m = &p.mesh;
    let mut k = assemble(m, 3, |el| Some(tet10::stiffness(&m.tet_points(el), e, nu)));
    let mut f = load_vector(
        &p,
        &[Load {
            faces: names(&["x1"]),
            kind: LoadKind::Force(pf),
            dir: Dir::Fixed([0.0, 0.0, -1.0]),
        }],
    )
    .unwrap();
    let fixed = fixed_dofs(&p, &names(&["x0"])).unwrap();
    apply_fixed(&mut k, &mut f, &fixed);
    let modes = rigid_modes(&m.points, &fixed_mask(k.rows, &fixed));
    let mut u = vec![0.0; f.len()];
    solve(k, &f, &mut u, 3, modes, 1e-10).unwrap();
    let i = w.powi(4) / 12.0;
    let c = w / 2.0;
    Case {
        name: "片持ち梁(ν = 0、Saint-Venant の解、15 < x < 85)",
        p,
        e,
        nu,
        u,
        exact: Box::new(move |x| {
            let z = x[2] - c;
            [
                pf * (l - x[0]) * z / i,
                0.0,
                0.0,
                0.0,
                0.0,
                pf / (2.0 * i) * (z * z - c * c),
            ]
        }),
        inside: Box::new(|x| x[0] > 15.0 && x[0] < 85.0),
        scale: pf * l * c / i,
    }
}

pub(super) fn lame(h: f64) -> Case {
    let (a, b, l, pr, e, nu) = (5.0, 10.0, 3.0, 10.0, 200_000.0, 0.3);
    let p = Prepared::new(&annulus(a, b, l, 120, 2), h, &|| false).unwrap();
    let m = &p.mesh;
    let mut k = assemble(m, 3, |el| Some(tet10::stiffness(&m.tet_points(el), e, nu)));
    let mut f = load_vector(
        &p,
        &[Load {
            faces: names(&["inner"]),
            kind: LoadKind::Pressure(pr),
            dir: Dir::Normal,
        }],
    )
    .unwrap();
    let mut fixed: Vec<(usize, f64)> = Vec::new();
    let ends = p.faces_of(&names(&["bottom", "top"])).unwrap();
    for n in p.nodes_of(&ends) {
        fixed.push((3 * n as usize + 2, 0.0));
    }
    let nearest = |q: [f64; 3]| {
        (0..m.points.len())
            .min_by(|i, j| {
                crate::geom::dist(m.points[*i], q).total_cmp(&crate::geom::dist(m.points[*j], q))
            })
            .unwrap()
    };
    let na = nearest([b, 0.0, 0.5 * l]);
    let nb = nearest([-b, 0.0, 0.5 * l]);
    fixed.extend([(3 * na, 0.0), (3 * na + 1, 0.0), (3 * nb + 1, 0.0)]);
    apply_fixed(&mut k, &mut f, &fixed);
    let modes = rigid_modes(&m.points, &fixed_mask(k.rows, &fixed));
    let mut u = vec![0.0; f.len()];
    solve(k, &f, &mut u, 3, modes, 1e-11).unwrap();
    let aa = pr * a * a / (b * b - a * a);
    Case {
        name: "厚肉の円筒の内圧(Lamé、全体)",
        p,
        e,
        nu,
        u,
        exact: Box::new(move |x| {
            let r2 = x[0] * x[0] + x[1] * x[1];
            let sr = aa * (1.0 - b * b / r2);
            let st = aa * (1.0 + b * b / r2);
            let (cc, ss, cs) = (x[0] * x[0] / r2, x[1] * x[1] / r2, x[0] * x[1] / r2);
            [
                sr * cc + st * ss,
                sr * ss + st * cc,
                nu * (sr + st),
                (sr - st) * cs,
                0.0,
                0.0,
            ]
        }),
        inside: Box::new(|_| true),
        scale: pr * (a * a + b * b) / (b * b - a * a),
    }
}

#[test]
#[ignore = "調べ物の道具(確かめはせず、表と絵を書くだけ)。JC_LAB_DIR などで手で流す"]
fn recovery_errors() {
    let path = std::env::var("JC_LAB_OUT").unwrap_or("/tmp/recovery.md".into());
    let mut out = std::fs::File::create(&path).unwrap();
    for c in [cantilever(2.5), cantilever(1.25), lame(0.9), lame(0.45)] {
        report(&c, &mut out);
    }
}

/// 厚肉の円筒の内の面の周の応力を、角と辺の中点に分けて、塗り方ごとに比べる。
#[test]
#[ignore = "調べ物の道具(確かめはせず、表を書くだけ)。--nocapture で手で流す"]
fn lame_inner_hoop_parts() {
    let (a, b, pr) = (5.0, 10.0, 10.0);
    let want = pr * (a * a + b * b) / (b * b - a * a);
    let hs: Vec<f64> = std::env::var("JC_LAB_HS")
        .map(|s| s.split(',').map(|x| x.parse().unwrap()).collect())
        .unwrap_or(vec![0.9, 0.6, 0.45]);
    for h in hs {
        let c = lame(h);
        let p = &c.p;
        let m = &p.mesh;
        let en = elem_nodal(p, c.e, c.nu, &c.u);
        let cur = r_corner(p, &en);
        let avg = r_avg(p, &en);
        let spr = r_spr(p, &gauss_samples(p, c.e, c.nu, &c.u), true);
        let r_of = |q: P3| (q[0] * q[0] + q[1] * q[1]).sqrt();
        let inner = |n: usize| (r_of(m.points[n]) - a).abs() < 5e-3;
        let hoop = |q: P3, s: &S6| {
            let r2 = q[0] * q[0] + q[1] * q[1];
            (s[0] * q[1] * q[1] + s[1] * q[0] * q[0] - 2.0 * s[3] * q[0] * q[1]) / r2
        };
        // 角を z 軸のまわりに回して中点の向きへ合わせてから平均する
        let rot = |s: &S6, from: P3, to: P3| -> S6 {
            let t = to[1].atan2(to[0]) - from[1].atan2(from[0]);
            let (cs, sn) = (t.cos(), t.sin());
            let r = [[cs, -sn, 0.0], [sn, cs, 0.0], [0.0, 0.0, 1.0]];
            let sm = [[s[0], s[3], s[5]], [s[3], s[1], s[4]], [s[5], s[4], s[2]]];
            let mut o = [[0.0; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    for k in 0..3 {
                        for l in 0..3 {
                            o[i][j] += r[i][k] * sm[k][l] * r[j][l];
                        }
                    }
                }
            }
            [o[0][0], o[1][1], o[2][2], o[0][1], o[1][2], o[0][2]]
        };
        let mut corner = std::collections::BTreeSet::new();
        let mut mids: HashMap<usize, (usize, usize)> = HashMap::new();
        let mut own: HashMap<usize, (S6, f64)> = HashMap::new();
        let mut raw: HashMap<usize, Vec<f64>> = HashMap::new();
        for (e, t) in m.tets.iter().enumerate() {
            for i in 0..4 {
                let v = t[i] as usize;
                if inner(v) {
                    corner.insert(v);
                    raw.entry(v).or_default().push(hoop(m.points[v], &en[e][i]));
                }
            }
            for (k, ed) in EDGES.iter().enumerate() {
                let v = t[4 + k] as usize;
                if inner(v) {
                    mids.insert(v, (t[ed[0]] as usize, t[ed[1]] as usize));
                    let o = own.entry(v).or_insert(([0.0; 6], 0.0));
                    for c in 0..6 {
                        o.0[c] += en[e][4 + k][c];
                    }
                    o.1 += 1.0;
                }
            }
        }
        let pc = |v: f64| (v / want - 1.0) * 100.0;
        let mean = |it: &mut dyn Iterator<Item = f64>| {
            let v: Vec<f64> = it.collect();
            (v.iter().sum::<f64>() / v.len() as f64, v.len())
        };
        let (cv, nc) = mean(&mut corner.iter().map(|&v| hoop(m.points[v], &cur[v])));
        let (ca, _) = mean(&mut corner.iter().map(|&v| hoop(m.points[v], &avg[v])));
        let (cs, _) = mean(&mut corner.iter().map(|&v| hoop(m.points[v], &spr[v])));
        let (rmin, _) = mean(
            &mut corner
                .iter()
                .map(|v| raw[v].iter().copied().fold(f64::MAX, f64::min)),
        );
        let (rmax, _) = mean(
            &mut corner
                .iter()
                .map(|v| raw[v].iter().copied().fold(f64::MIN, f64::max)),
        );
        let (mv, nm) = mean(&mut mids.keys().map(|&v| hoop(m.points[v], &cur[v])));
        let (mo, _) = mean(&mut mids.keys().map(|&v| {
            let (s, n) = own[&v];
            hoop(m.points[v], &s.map(|x| x / n))
        }));
        let (mr, _) = mean(&mut mids.iter().map(|(&v, &(x, y))| {
            let q = m.points[v];
            let sa = rot(&cur[x], m.points[x], q);
            let sb = rot(&cur[y], m.points[y], q);
            hoop(q, &std::array::from_fn(|i| 0.5 * (sa[i] + sb[i])))
        }));
        let (ms, _) = mean(&mut mids.keys().map(|&v| hoop(m.points[v], &spr[v])));
        let (msr, _) = mean(&mut mids.iter().map(|(&v, &(x, y))| {
            let q = m.points[v];
            let sa = rot(&spr[x], m.points[x], q);
            let sb = rot(&spr[y], m.points[y], q);
            hoop(q, &std::array::from_fn(|i| 0.5 * (sa[i] + sb[i])))
        }));
        // ミーゼス応力の誤差(内の面の全部の節点、角の平均・回す・SPR と回す)
        let ex = |q: P3| von_mises(&(c.exact)(q));
        let vm_err = |f: &dyn Fn(usize) -> S6| {
            let v: Vec<f64> = corner
                .iter()
                .copied()
                .chain(mids.keys().copied())
                .map(|n| (von_mises(&f(n)) / ex(m.points[n]) - 1.0) * 100.0)
                .collect();
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
            let p95 = pct(&mut v.iter().map(|x| x.abs()).collect::<Vec<_>>(), 0.95);
            format!("平均 {mean:+.2}% ばらつき {sd:.2}% 95% 点 {p95:.2}%")
        };
        let rotated = |base: &Vec<S6>| {
            let mut o = base.clone();
            for (&v, &(x, y)) in &mids {
                let q = m.points[v];
                let sa = rot(&base[x], m.points[x], q);
                let sb = rot(&base[y], m.points[y], q);
                o[v] = std::array::from_fn(|i| 0.5 * (sa[i] + sb[i]));
            }
            o
        };
        let cur_r = rotated(&cur);
        let mut spr_c = cur.clone();
        for &v in &corner {
            spr_c[v] = spr[v];
        }
        let spr_r = rotated(&spr_c);
        println!("  ミーゼス 角の平均: {}", vm_err(&|n| cur[n]));
        println!("  ミーゼス 中点を回す: {}", vm_err(&|n| cur_r[n]));
        println!("  ミーゼス 角を SPR・中点を回す: {}", vm_err(&|n| spr_r[n]));
        println!("  中点 SPR の角を回して平均 {:+.2}%", pc(msr));
        // 表面で分かっている力(内圧)に合わせる: 法線の向きの成分を σ·n = −p n に置き換える
        let fix = |sv: &S6, q: P3| -> S6 {
            let r = r_of(q);
            let n = [-q[0] / r, -q[1] / r, 0.0];
            let t = n.map(|x| -pr * x);
            let sm = [
                [sv[0], sv[3], sv[5]],
                [sv[3], sv[1], sv[4]],
                [sv[5], sv[4], sv[2]],
            ];
            let pm: [[f64; 3]; 3] = std::array::from_fn(|i| {
                std::array::from_fn(|j| (i == j) as u8 as f64 - n[i] * n[j])
            });
            let nt: f64 = (0..3).map(|i| n[i] * t[i]).sum();
            let mut o = [[0.0; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    for k in 0..3 {
                        for l in 0..3 {
                            o[i][j] += pm[i][k] * sm[k][l] * pm[l][j];
                        }
                    }
                    o[i][j] += n[i] * t[j] + t[i] * n[j] - nt * n[i] * n[j];
                }
            }
            [o[0][0], o[1][1], o[2][2], o[0][1], o[1][2], o[0][2]]
        };
        let mut cur_f = cur.clone();
        for &v in &corner {
            cur_f[v] = fix(&cur[v], m.points[v]);
        }
        let cur_fr = rotated(&cur_f);
        println!(
            "  ミーゼス 角を力に合わせ・中点を回す: {}",
            vm_err(&|n| cur_fr[n])
        );
        let mut spr_f = cur.clone();
        for &v in &corner {
            spr_f[v] = fix(&spr[v], m.points[v]);
        }
        let spr_fr = rotated(&spr_f);
        println!(
            "  ミーゼス 角を SPR と力・中点を回す: {}",
            vm_err(&|n| spr_fr[n])
        );
        let mut own_m = cur.clone();
        for (&v, (sum, n)) in &own {
            own_m[v] = sum.map(|x| x / n);
        }
        println!(
            "  ミーゼス 中点は要素の中点の平均: {}",
            vm_err(&|n| own_m[n])
        );
        // 画面の塗る値: 内の面の三角形を中点で 4 つに分け、その中を成分の線形で補間する
        let paint = |vals: &Vec<S6>, local: bool, tag: &str| {
            let mut e = Vec::new();
            let mut dump: Vec<(P3, f64)> = Vec::new();
            for f in m
                .faces
                .iter()
                .filter(|f| f.nodes.iter().all(|&v| inner(v as usize)))
            {
                let nd = f.nodes.map(|v| v as usize);
                for sub in [[0, 3, 5], [3, 1, 4], [5, 4, 2], [3, 4, 5]] {
                    let k = sub.map(|i| nd[i]);
                    for w in [
                        [1.0 / 3.0; 3],
                        [0.5, 0.5, 0.0],
                        [0.0, 0.5, 0.5],
                        [0.5, 0.0, 0.5],
                    ] {
                        let mut q = [0.0; 3];
                        for i in 0..3 {
                            for a in 0..3 {
                                q[a] += w[i] * m.points[k[i]][a];
                            }
                        }
                        let mut sv = [0.0; 6];
                        for i in 0..3 {
                            let si = if local {
                                rot(&vals[k[i]], m.points[k[i]], q)
                            } else {
                                vals[k[i]]
                            };
                            for c in 0..6 {
                                sv[c] += w[i] * si[c];
                            }
                        }
                        e.push((von_mises(&sv) / ex(q) - 1.0) * 100.0);
                        dump.push((q, *e.last().unwrap()));
                    }
                }
            }
            if let Ok(dir) = std::env::var("JC_LAB_DIR") {
                let mut f = std::fs::File::create(format!("{dir}/lame_{h}_{tag}.csv")).unwrap();
                for (q, v) in &dump {
                    writeln!(f, "{},{},{},{}", q[0], q[1], q[2], v).unwrap();
                }
            }
            let mean = e.iter().sum::<f64>() / e.len() as f64;
            let sd = (e.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / e.len() as f64).sqrt();
            let p95 = pct(&mut e.iter().map(|x| x.abs()).collect::<Vec<_>>(), 0.95);
            format!("平均 {mean:+.2}% ばらつき {sd:.2}% 95% 点 {p95:.2}%")
        };
        println!("  塗る値 角の平均: {}", paint(&cur, false, "plain"));
        println!("  塗る値 中点を回す: {}", paint(&cur_r, false, "turned"));
        println!(
            "  塗る値 中点を回す・塗る所でも回す: {}",
            paint(&cur_r, true, "turned2")
        );
        let prod = crate::statics::stresses(p, c.nu, &c.u, |_| c.e).0;
        println!(
            "  塗る値 JC の塗り方(statics::stresses): {}",
            paint(&prod, false, "jc")
        );
        println!(
            "h {h}(要素 {}・内の角 {nc}・中点 {nm}): 角 角の平均 {:+.2}% 全部の平均 {:+.2}% SPR {:+.2}% 要素の値の幅 {:+.2}〜{:+.2}% | 中点 角の平均 {:+.2}% 要素の中点の平均 {:+.2}% 回して平均 {:+.2}% SPR {:+.2}%",
            m.tets.len(),
            pc(cv),
            pc(ca),
            pc(cs),
            pc(rmin),
            pc(rmax),
            pc(mv),
            pc(mo),
            pc(mr),
            pc(ms)
        );
    }
}
