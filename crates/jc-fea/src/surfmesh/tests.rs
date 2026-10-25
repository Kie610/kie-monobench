use super::*;

/// 見本の部品(板 60×40×10、穴・ポケット・ボス付き)の、カーネルが作った表示用の網。
pub(crate) fn sample_part() -> Mesh {
    let s = include_str!("../testdata/sample_part_surface.txt");
    let mut it = s.lines();
    let head: Vec<usize> = it
        .next()
        .unwrap()
        .split_whitespace()
        .map(|x| x.parse().unwrap())
        .collect();
    let mut m = Mesh::default();
    for _ in 0..head[0] {
        let v: Vec<f32> = it
            .next()
            .unwrap()
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        m.positions.push([v[0], v[1], v[2]]);
    }
    for _ in 0..head[1] {
        let v: Vec<u32> = it
            .next()
            .unwrap()
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        m.triangles.push([v[0], v[1], v[2]]);
        m.triangle_faces.push(v[3]);
    }
    for _ in 0..head[2] {
        m.face_names.push(it.next().unwrap().to_string());
    }
    m
}

fn run(m: &Mesh, h: f64) -> (Reference, Surface, SurfaceStats) {
    let (r, s0) = reference(m, h, SizeRule::default()).unwrap();
    let v0 = s0.volume();
    let s = remesh(&r, s0, &|| false).unwrap();
    let st = s.stats(&r);
    eprintln!(
        "h {h}: {st:?} 体積 {:.2} → {:.2} 最小の大きさ {:.3}",
        v0,
        s.volume(),
        r.sizing.min()
    );
    (r, s, st)
}

fn labels_kept(m: &Mesh, s: &Surface) {
    let mut want: Vec<u32> = m.triangle_faces.clone();
    want.sort_unstable();
    want.dedup();
    let mut got = s.label.clone();
    got.sort_unstable();
    got.dedup();
    assert_eq!(want, got, "面の名前が全部残る");
}

#[test]
fn the_sample_part_is_remeshed_closed_with_good_angles() {
    let m = sample_part();
    for h in [2.0, 1.0] {
        let (_, s, st) = run(&m, h);
        assert_eq!(st.open_edges, 0);
        labels_kept(&m, &s);
        // いちばん小さい角は、ボスの側面の帯の列のすぐ外の三角形(列の点の大きさは列の点の
        // 間の 0.9 mm なので、0.4 mm の辺を縮めると 4/3 倍を超える 2 mm の辺ができ、縮め
        // られない)。大きさの場のわずかな違いで 12〜15° の間を動く(見本の点の置き方を
        // 細長い三角形で変えた時に 15.0° → 12.1°)ので、下限は 12°
        assert!(st.min_angle > 12.0, "最小角 {}", st.min_angle);
        assert!(st.below_30 < 0.05, "30° 未満 {}", st.below_30);
        assert!(
            st.point_dev_max < 1e-6 * 100.0,
            "点のずれ {}",
            st.point_dev_max
        );
        // 半径 1.7 mm の穴で、弦のたるみは L²/(8r) ≈ 0.1 mm まで
        assert!(st.sag_max < 0.15, "たるみ {}", st.sag_max);
    }
}

#[test]
fn a_cylinder_is_remeshed_on_its_curved_face() {
    let m = crate::tetmesh::tests::cylinder(10.0, 20.0, 96);
    let (r, s, st) = run(&m, 2.5);
    assert_eq!(st.open_edges, 0);
    labels_kept(&m, &s);
    assert!(st.min_angle > 20.0, "最小角 {}", st.min_angle);
    // 側面の点は円柱の上(表示用の網の 96 角形の上)
    let _ = r;
    for (t, l) in s.tris.iter().zip(&s.label) {
        if *l == 0 {
            for v in t {
                let p = s.points[*v as usize];
                let rr = (p[0] * p[0] + p[1] * p[1]).sqrt();
                assert!(
                    rr <= 10.0 + 1e-6 && rr > 10.0 * (std::f64::consts::PI / 96.0).cos() - 1e-6
                );
            }
        }
    }
}

/// 角を丸めた箱(w×d×H、縦の角の丸みの半径 rc、丸み 1 つを n に分ける)。面は下・上・
/// 平らな横 4 つ・丸み 4 つ(平らな面と丸みの境は接して滑らか)。
pub(crate) fn rounded_box(w: f64, d: f64, hgt: f64, rc: f64, n: usize) -> Mesh {
    let mut m = Mesh::default();
    // 輪郭(反時計回り)。区間ごとの面の番号: 平らな横 2..6、丸み 6..10
    let mut ring: Vec<([f64; 2], u32)> = Vec::new();
    let cs = [[w - rc, rc], [w - rc, d - rc], [rc, d - rc], [rc, rc]];
    let start = [-90.0f64, 0.0, 90.0, 180.0];
    for k in 0..4 {
        for i in 0..=n {
            let a = (start[k] + 90.0 * i as f64 / n as f64).to_radians();
            // 丸みの区間の後(i == n の点から次の丸みの始まりまで)は平らな横
            let face = if i < n { 6 + k as u32 } else { 2 + k as u32 };
            ring.push(([cs[k][0] + rc * a.cos(), cs[k][1] + rc * a.sin()], face));
        }
    }
    m.face_names = [
        "bottom", "top", "s0", "s1", "s2", "s3", "r0", "r1", "r2", "r3",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    let push = |m: &mut Mesh, p: [f64; 3]| -> u32 {
        m.positions.push(p.map(|v| v as f32));
        (m.positions.len() - 1) as u32
    };
    let k = ring.len();
    for i in 0..k {
        let j = (i + 1) % k;
        let (a, b) = (ring[i].0, ring[j].0);
        let f = ring[i].1;
        let p = [
            push(&mut m, [a[0], a[1], 0.0]),
            push(&mut m, [b[0], b[1], 0.0]),
            push(&mut m, [b[0], b[1], hgt]),
            push(&mut m, [a[0], a[1], hgt]),
        ];
        m.triangles.push([p[0], p[1], p[2]]);
        m.triangles.push([p[0], p[2], p[3]]);
        m.triangle_faces.extend([f, f]);
    }
    for (z, f, up) in [(0.0, 0u32, false), (hgt, 1u32, true)] {
        let c = push(&mut m, [0.5 * w, 0.5 * d, z]);
        for i in 0..k {
            let j = (i + 1) % k;
            let a = push(&mut m, [ring[i].0[0], ring[i].0[1], z]);
            let b = push(&mut m, [ring[j].0[0], ring[j].0[1], z]);
            m.triangles.push(if up { [c, a, b] } else { [c, b, a] });
            m.triangle_faces.push(f);
        }
    }
    m
}

#[test]
fn a_rounded_box_keeps_the_smooth_face_borders() {
    let m = rounded_box(30.0, 20.0, 8.0, 3.0, 24);
    let (r, s, st) = run(&m, 2.0);
    assert_eq!(st.open_edges, 0);
    labels_kept(&m, &s);
    assert!(st.min_angle > 20.0, "最小角 {}", st.min_angle);
    // 平らな横と丸みの境(接して滑らかな所)の点は境の線の上にある
    for (k, p) in s.kind.iter().zip(&s.points) {
        if let Kind::Chain(g) = k {
            assert!(dist(*p, r.on_chain(*g, *p)) < 1e-9);
        }
    }
    // まっすぐな三角形なので、丸み(半径 3、弦 2)の弦の分だけ小さい(2 次の要素の中点で戻す)
    let exact = 30.0 * 20.0 * 8.0 - (4.0 - std::f64::consts::PI) * 9.0 * 8.0;
    assert!(
        (s.volume() - exact).abs() / exact < 4e-3,
        "体積 {}",
        s.volume()
    );
}

#[test]
fn a_small_hole_gets_smaller_elements() {
    // 穴の半径 1.7 mm、h 2 mm: 円を 2π·r / (0.7·r) ≈ 9 以上に分ける
    let m = crate::tetmesh::tests::annulus(1.7, 12.0, 6.0, 128, 2);
    let (_, s, st) = run(&m, 2.0);
    assert_eq!(st.open_edges, 0);
    let on_hole: HashSet<u32> = s
        .tris
        .iter()
        .zip(&s.label)
        .filter(|(_, l)| **l == 0)
        .flat_map(|(t, _)| *t)
        .filter(|v| s.points[*v as usize][2].abs() < 1e-9)
        .collect();
    assert!(on_hole.len() >= 9, "穴の縁の点 {}", on_hole.len());
}

/// 大きさの場の見本の点は、細長い三角形でも面積と長さに比例する数だけ置き、どこからも step
/// の内にある。いちばん長い辺の 2 乗に比例させると、表示用の網の細長い三角形(円柱の
/// 側面の長さ 100 mm・幅 0.25 mm)1 つで 2 万点になり、円柱 1 本の reference に 9.4 秒かかる。
#[test]
fn size_samples_follow_the_triangle_area() {
    let step = 0.5;
    for p in [
        [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 0.25, 0.0]],
        [[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [1.0, 2.5, 0.0]],
        [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 0.0, 1e-9]],
    ] {
        let q = tri_samples(p, step);
        let area = 0.5 * norm(tri_normal(p[0], p[1], p[2]));
        let longest = dist(p[0], p[1]).max(dist(p[1], p[2])).max(dist(p[2], p[0]));
        let k = (longest / step).ceil();
        let old = (k + 1.0) * (k + 2.0) / 2.0;
        eprintln!("点 {}(前の数え方 {old})", q.len());
        assert!(
            (q.len() as f64) < 4.0 * (area / (step * step) + longest / step) + 10.0,
            "点 {}",
            q.len()
        );
        for c in p {
            assert!(q.iter().any(|x| dist(*x, c) < 1e-12), "角 {c:?}");
        }
        // 三角形の中の点はどれも、いちばん近い見本の点まで step 以内
        for i in 0..=20 {
            for j in 0..=20 - i {
                let (u, v) = (i as f64 / 20.0, j as f64 / 20.0);
                let x = add(
                    add(scale(p[0], 1.0 - u - v), scale(p[1], u)),
                    scale(p[2], v),
                );
                let d = q.iter().map(|y| dist(x, *y)).fold(f64::INFINITY, f64::min);
                assert!(d <= step, "{x:?} から {d}");
            }
        }
    }
}
