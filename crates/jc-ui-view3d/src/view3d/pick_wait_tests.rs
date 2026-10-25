//! 大きな網を置いた直後に拾う時の試験(拾う箱の木を待たない)。

use super::*;

/// 細かく分けた板(1 つの面)。`n × n` の升に 2 つずつの三角形。
fn plate(n: usize) -> Mesh {
    let mut m = Mesh::default();
    let w = (n + 1) as u32;
    for j in 0..=n {
        for i in 0..=n {
            m.positions.push([i as f32, j as f32, 0.0]);
            m.normals.push([0.0, 0.0, 1.0]);
        }
    }
    for j in 0..n as u32 {
        for i in 0..n as u32 {
            let a = j * w + i;
            m.triangles.push([a, a + 1, a + w + 1]);
            m.triangles.push([a, a + w + 1, a + w]);
            m.triangle_faces.extend([0, 0]);
        }
    }
    m.face_names = vec!["上".into()];
    m
}

/// 大きな網の箱の木を別のスレッドで作っている間も、拾うと同じ面が見つかる(木を待たずに全部調べる)。
/// 出来上がった後は木を使う。
#[test]
fn picking_a_big_mesh_while_its_tree_is_being_built_finds_the_face() {
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    let mut sc = Scene::default();
    sc.set(&[], vec![(FeatureId(1), plate(120))]);
    let b = &sc.bodies[0];
    assert!(b.mesh.triangles.len() >= BIG_MESH);
    let mut cam = Camera::default();
    cam.fit(&sc, rect);
    // 木を作り始めた印が立っていて、作っている途中でも拾える
    assert!(b.bvh.started.load(std::sync::atomic::Ordering::Acquire));
    let hit = picks_at(&sc, &cam, rect, rect.center());
    assert_eq!(hit.first().map(|h| h.0.face.as_str()), Some("上"));
    let t = std::time::Instant::now();
    while b.bvh.tree.get().is_none() {
        assert!(t.elapsed().as_secs() < 30, "木が出来上がらない");
        std::thread::yield_now();
    }
    let faces = |h: &[(Pick, [f32; 3])]| h.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    assert_eq!(
        faces(&picks_at(&sc, &cam, rect, rect.center())),
        faces(&hit)
    );
}

/// 大きな網を置いた直後に拾うのにかかる時間(u23。木が出来上がるのを待たない):
/// `cargo test -p jc-ui-view3d --release first_pick_speed -- --ignored --nocapture`。
#[test]
#[ignore = "速さを測る道具(--release で手で流す)"]
fn first_pick_speed() {
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 1000.0));
    for n in [250usize, 500] {
        let mut sc = Scene::default();
        sc.set(&[], vec![(FeatureId(1), plate(n))]);
        let mut cam = Camera::default();
        cam.fit(&sc, rect);
        let t = std::time::Instant::now();
        let _ = picks_at(&sc, &cam, rect, rect.center());
        let first = t.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "三角形 {}: 置いた直後に拾う 1 回 {first:.2} ms",
            sc.bodies[0].mesh.triangles.len()
        );
    }
}
