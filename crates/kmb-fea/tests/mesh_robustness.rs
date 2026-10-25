//! 形に沿う四面体の網の、変わった形・網での確かめ(敵対的レビュー D2 で見つかった不具合)。

use kmb_core::Mesh;
use kmb_fea::geom::tet_volume;
use kmb_fea::tet10;
use kmb_fea::tetmesh::{MAX_TETS, TetMesh, build, enclosed_volume, estimate_tets, expected_tets};
use std::f64::consts::{PI, TAU};

fn push(m: &mut Mesh, p: [f64; 3]) -> u32 {
    m.positions.push(p.map(|v| v as f32));
    (m.positions.len() - 1) as u32
}

/// 箱(原点 o、大きさ l)。面の番号は base から 6 つ。
fn add_box(m: &mut Mesh, o: [f64; 3], l: [f64; 3], base: u32) {
    let q: [[[f64; 3]; 4]; 6] = [
        [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
        [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]],
        [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
        [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
        [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]],
        [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
    ];
    for (fi, f) in q.iter().enumerate() {
        let v: Vec<u32> = f
            .iter()
            .map(|p| push(m, [0, 1, 2].map(|a| o[a] + p[a] * l[a])))
            .collect();
        m.triangles.push([v[0], v[1], v[2]]);
        m.triangles.push([v[0], v[2], v[3]]);
        m.triangle_faces.extend([base + fi as u32; 2]);
        m.face_names.push(format!("f{}", base + fi as u32));
    }
}

fn boxm(l: [f64; 3]) -> Mesh {
    let mut m = Mesh::default();
    add_box(&mut m, [0.0; 3], l, 0);
    m
}

/// UV の球(1 つの面 `face`)。中心 c、半径 r。
fn add_sphere(m: &mut Mesh, c: [f64; 3], r: f64, nu: usize, nv: usize, face: u32) {
    let s = push(m, [c[0], c[1], c[2] - r]);
    let n = push(m, [c[0], c[1], c[2] + r]);
    let rings: Vec<Vec<u32>> = (1..nv)
        .map(|j| {
            let th = PI * j as f64 / nv as f64 - PI / 2.0;
            (0..nu)
                .map(|i| {
                    let ph = TAU * i as f64 / nu as f64;
                    let p = [th.cos() * ph.cos(), th.cos() * ph.sin(), th.sin()];
                    push(m, [0, 1, 2].map(|a| c[a] + r * p[a]))
                })
                .collect()
        })
        .collect();
    let last = rings.len() - 1;
    for i in 0..nu {
        let k = (i + 1) % nu;
        m.triangles.push([s, rings[0][k], rings[0][i]]);
        m.triangles.push([n, rings[last][i], rings[last][k]]);
        m.triangle_faces.extend([face; 2]);
        for j in 0..last {
            let (a, b, cc, d) = (rings[j][i], rings[j][k], rings[j + 1][k], rings[j + 1][i]);
            m.triangles.push([a, b, cc]);
            m.triangles.push([a, cc, d]);
            m.triangle_faces.extend([face; 2]);
        }
    }
}

/// 角の四面体と 2 次の要素が裏返っておらず、体積が形に合う。
fn check(tm: &TetMesh, volume: f64) {
    for e in 0..tm.tets.len() {
        let x = tm.tet_points(e);
        assert!(
            tet_volume(x[0], x[1], x[2], x[3]) > 0.0,
            "要素 {e} の角が裏返り"
        );
        assert!(tet10::jacobian_bounds(&x).0 > 0.0, "要素 {e} が裏返り");
    }
    assert!(
        (tm.stats.volume - volume).abs() < 2e-3 * volume,
        "体積 {} と {volume}",
        tm.stats.volume
    );
}

/// 2 つの球が極の点だけで触れる網(点をまとめると、2 つの面の名前を持つのに面の境の辺が無い点
/// ができる)。`surfmesh::reference` が鎖の組を引けなくてもパニックしない。
#[test]
fn two_spheres_touching_at_a_pole() {
    let mut m = Mesh::default();
    add_sphere(&mut m, [0.0, 0.0, 0.0], 5.0, 24, 12, 0);
    add_sphere(&mut m, [0.0, 0.0, 10.0], 5.0, 24, 12, 1);
    m.face_names = vec!["a".into(), "b".into()];
    let tm = build(&m, 2.5, &|| false).unwrap();
    check(&tm, enclosed_volume(&m));
    assert!(tm.faces.iter().any(|f| f.face == 0) && tm.faces.iter().any(|f| f.face == 1));
}

/// 全部の三角形が内向きの網も、向きをそろえ直して網にする(「表面が閉じていない所が
/// あります」という誤った理由で失敗しない)。
#[test]
fn an_inside_out_mesh_is_meshed() {
    let mut m = boxm([10.0, 8.0, 6.0]);
    for t in &mut m.triangles {
        t.swap(1, 2);
    }
    assert!(enclosed_volume(&m) < 0.0);
    let tm = build(&m, 2.0, &|| false).unwrap();
    check(&tm, 480.0);
}

/// 厚さ 1 mm の板(200×200)は、大きさの場が厚さで決まるので h だけの見込み(3840 個)よりずっと
/// 多くなる。網を作る前に大きさの場から見込んで断る(中を埋めてから「点が 50000 を超えました」で
/// 断ると 19 秒かかる)。
#[test]
fn a_thin_plate_is_refused_before_meshing() {
    let m = boxm([200.0, 200.0, 1.0]);
    let est = estimate_tets(&m, 5.0).unwrap();
    assert!(expected_tets(40_000.0, 5.0) < 4_000.0);
    assert!(est > MAX_TETS as f64, "見込み {est}");
    let e = build(&m, 5.0, &|| false).unwrap_err();
    assert!(e.contains("多すぎ") && e.contains("薄い所"), "{e}");
}

/// 大きさの場から見込む要素の数は、薄い板でも本当の数に近い(h だけの見込みは 1/70 だった)。
#[test]
fn the_estimate_follows_a_thin_plate() {
    let m = boxm([30.0, 20.0, 1.0]);
    let est = estimate_tets(&m, 5.0).unwrap();
    let tm = build(&m, 5.0, &|| false).unwrap();
    check(&tm, 600.0);
    let n = tm.tets.len() as f64;
    eprintln!(
        "見込み {est:.0}、h だけ {:.0}、本当 {n}",
        expected_tets(600.0, 5.0)
    );
    assert!(est > 0.7 * n && est < 1.3 * n, "見込み {est} と本当 {n}");
}
