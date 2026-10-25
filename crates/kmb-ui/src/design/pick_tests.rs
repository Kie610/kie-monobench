//! 面と辺を拾う試験: 箱の木の拾い方が前の拾い方(三角形を全部写す)と同じ答えになること、
//! 拾う速さ(`--ignored`)。

use crate::Workspace;
use crate::view3d::{self, Camera, Scene};
use egui::{Pos2, Rect};
use kmb_core::{FeatureId, Mesh};
use std::time::Instant;

fn reference_workspace() -> Workspace {
    let part = kmb_samples::reference_part();
    Workspace::new(part.doc, None, kmb_bundle::modules())
}

/// 見る向き(平行・透視、等角・上・前・寄った所)。
fn views(scene: &Scene, rect: Rect) -> Vec<Camera> {
    let mut out = Vec::new();
    for perspective in [false, true] {
        for (yaw, pitch, zoom) in [
            (-60.0, 30.0, 1.0),
            (-90.0, 90.0, 1.0),
            (-90.0, 0.0, 1.0),
            (-35.0, 50.0, 4.0),
        ] {
            let mut cam = Camera::default();
            cam.perspective = perspective;
            cam.fit(scene, rect);
            cam.set_view(yaw, pitch);
            cam.zoom *= zoom;
            out.push(cam);
        }
    }
    out
}

fn grid(rect: Rect, nx: usize, ny: usize) -> Vec<Pos2> {
    let mut out = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            out.push(Pos2::new(
                rect.min.x + rect.width() * (i as f32 + 0.5) / nx as f32,
                rect.min.y + rect.height() * (j as f32 + 0.5) / ny as f32,
            ));
        }
    }
    out
}

/// 視線と箱の木で拾った面は、三角形を全部写して拾った面と(縁の上を除き)同じで、当たった点も同じ。
#[test]
fn ray_picking_finds_the_faces_that_projecting_every_triangle_finds() {
    let ws = reference_workspace();
    let rect = Rect::from_min_size(Pos2::new(30.0, 20.0), egui::vec2(800.0, 600.0));
    let size = {
        let (lo, hi) = ws.cur.scene.bounds();
        (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max)
    };
    let (mut same, mut total) = (0, 0);
    for cam in views(&ws.cur.scene, rect) {
        for p in grid(rect, 48, 36) {
            let new = view3d::picks_at(&ws.cur.scene, &cam, rect, p);
            let old = view3d::picks_at_brute(&ws.cur.scene, &cam, rect, p);
            total += 1;
            let first = |v: &[(view3d::Pick, [f32; 3])]| v.first().cloned();
            match (first(&new), first(&old)) {
                (None, None) => same += 1,
                (Some((a, pa)), Some((b, pb))) if a == b => {
                    let d = (0..3).map(|k| (pa[k] - pb[k]).abs()).fold(0.0, f32::max);
                    assert!(d < 1e-3 * size, "同じ面で当たった点が違う: {pa:?} {pb:?}");
                    same += 1;
                }
                _ => {}
            }
        }
    }
    // 違うのは面の縁のちょうど上(どちらの面とも言える所)だけ
    assert!(
        same as f64 >= 0.995 * total as f64,
        "同じ答え {same}/{total}"
    );
}

/// 細かい網の球(緯度の帯ごとに面を分ける)。`n` は経度の分け数(三角形は約 n² 個)。
fn sphere(n: usize, r: f32) -> Mesh {
    let mut m = Mesh::default();
    let rows = n / 2;
    for j in 0..=rows {
        let v = std::f32::consts::PI * j as f32 / rows as f32;
        for i in 0..=n {
            let u = std::f32::consts::TAU * i as f32 / n as f32;
            let p = [v.sin() * u.cos(), v.sin() * u.sin(), v.cos()];
            m.positions.push(p.map(|c| c * r));
            m.normals.push(p);
        }
    }
    let w = (n + 1) as u32;
    let bands = 40.min(rows);
    for j in 0..rows as u32 {
        for i in 0..n as u32 {
            let a = j * w + i;
            let face = j * bands as u32 / rows as u32;
            m.triangles.push([a, a + w, a + w + 1]);
            m.triangles.push([a, a + w + 1, a + 1]);
            m.triangle_faces.extend([face, face]);
        }
    }
    m.face_names = (0..bands).map(|k| format!("帯{k}")).collect();
    m
}

/// 拾う速さを、前の拾い方と比べる: `cargo test -p kmb-ui --release pick_speed -- --ignored --nocapture`。
#[test]
#[ignore = "速さを測る道具(--release で手で流す)"]
fn pick_speed() {
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let ws = reference_workspace();
    let tris: usize = ws
        .cur
        .scene
        .bodies
        .iter()
        .map(|b| b.mesh.triangles.len())
        .sum();
    let mut cases: Vec<(String, &Scene)> =
        vec![(format!("見本の部品(三角形 {tris})"), &ws.cur.scene)];
    let mut big = Vec::new();
    for n in [256usize, 1024] {
        let mesh = sphere(n, 40.0);
        let mut sc = Scene::default();
        let label = format!("細かい球(三角形 {})", mesh.triangles.len());
        sc.set(&[], vec![(FeatureId(1), mesh)]);
        big.push((label, sc));
    }
    for (l, sc) in &big {
        cases.push((l.clone(), sc));
    }
    for (label, scene) in cases {
        let mut cam = Camera::default();
        cam.fit(scene, rect);
        let pts = grid(rect, 20, 10);
        // 箱の木を作る(初めの 1 回)
        let t = Instant::now();
        let _ = view3d::picks_at(scene, &cam, rect, rect.center());
        let build = t.elapsed().as_secs_f64() * 1000.0;
        let time = |f: &dyn Fn(Pos2)| {
            let t = Instant::now();
            for p in &pts {
                f(*p);
            }
            t.elapsed().as_secs_f64() * 1000.0 / pts.len() as f64
        };
        let new = time(&|p| {
            let _ = view3d::picks_at(scene, &cam, rect, p);
        });
        let old = time(&|p| {
            let _ = view3d::picks_at_brute(scene, &cam, rect, p);
        });
        eprintln!(
            "{label}: 1 回 前 {old:.3} ms → 後 {new:.4} ms({:.0} 倍)。箱の木を作るのに {build:.1} ms",
            old / new
        );
    }
}

/// 真上から見ると、貫通穴の上と下の縁は画面の上で重なる。隠れた下の面の辺は拾わず手前の縁を拾う
/// (前の CPU の拾い方は、先に見つけた方を拾うので下の縁を拾うことがあった)。線だけの表示では拾える。
#[test]
fn edges_hidden_below_are_not_picked_from_above() {
    let part = kmb_samples::reference_part();
    let plate_bottom = format!("f{}:start", part.plate.0);
    let mut ws = Workspace::new(part.doc, None, kmb_bundle::modules());
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
    let edges = ws.scene_edges().to_vec();
    let mut cam = Camera::default();
    cam.fit(&ws.cur.scene, rect);
    cam.set_view(-90.0, 90.0);
    let bottom = |e: &kmb_core::EdgePick| e.faces.contains(&plate_bottom);
    let (mut found, mut through) = (0, 0);
    for (_, list) in &edges {
        for e in list {
            for q in view3d::edge_polyline(e) {
                let p = cam.project(q.map(|v| v as f32), rect).0;
                let got = view3d::edge_at(&ws.cur.scene, &cam, rect, p, &edges, false);
                assert!(
                    !got.as_ref().is_some_and(bottom),
                    "上から見て下の面の辺を拾った: {got:?}"
                );
                found += usize::from(got.is_some());
                let seen = view3d::edge_at(&ws.cur.scene, &cam, rect, p, &edges, true);
                through += usize::from(seen.as_ref().is_some_and(bottom));
            }
        }
    }
    assert!(found > 0, "上の面の辺は拾える");
    assert!(through > 0, "線だけの表示では下の面の辺も拾える");
}

/// 速さを測る重い部品(200 × 200 mm の板に 20 × 20 個の貫通穴)を書く:
/// `JC_HEAVY_OUT=heavy.json cargo test -p kmb-ui --release write_heavy_part -- --ignored`。
#[test]
#[ignore = "道具: JC_HEAVY_OUT に速さを測る重い部品を書く"]
fn write_heavy_part() {
    let Ok(out) = std::env::var("JC_HEAVY_OUT") else {
        return;
    };
    let doc = heavy_doc();
    kmb_session::save(std::path::Path::new(&out), &doc).expect("書ける");
    let ws = Workspace::new(doc, None, kmb_bundle::modules());
    let tris: usize = ws
        .cur
        .scene
        .bodies
        .iter()
        .map(|b| b.mesh.triangles.len())
        .sum();
    eprintln!("{out}: 三角形 {tris}");
}

/// 速さを測る重い部品(200 × 200 mm の板に 20 × 20 個の貫通穴)。
fn heavy_doc() -> kmb_core::Document {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "200 mm", "200 mm", [0.0, 0.0, 200.0, 200.0]);
    let plate_sketch = b.feature(kmb_sketch::SKETCH, "板の輪郭", s.build());
    let plate = b.feature(
        kmb_cad_extrude::EXTRUDE,
        "板",
        kmb_cad_extrude::Extrude {
            profile: Some(plate_sketch),
            regions: Vec::new(),
            distance: "10 mm".into(),
            direction: kmb_cad::Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: kmb_cad::Operation::New,
            target: None,
            ..Default::default()
        },
    );
    // 穴の位置は拘束しない点(400 個の点を式で決めると、スケッチを解くのに長くかかる)
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let mut points = Vec::new();
    for j in 0..20 {
        for i in 0..20 {
            let p = s.point(5.0 + 10.0 * i as f64, 5.0 + 10.0 * j as f64);
            points.push(kmb_core::PointId(p.0));
        }
    }
    let sketch = b.feature(kmb_sketch::SKETCH, "穴の位置", s.build());
    b.feature(
        kmb_cad_hole::HOLE,
        "穴",
        kmb_cad_hole::Hole {
            points,
            diameter: "6 mm".into(),
            depth: None,
            ..kmb_cad_hole::Hole::new(plate, sketch)
        },
    );
    b.finish("重い部品")
}

/// 辺を拾う速さ(重い部品の、画面の格子の点ごと): `cargo test -p kmb-ui --release edge_pick_speed -- --ignored --nocapture`。
#[test]
#[ignore = "速さを測る道具(--release で手で流す)"]
fn edge_pick_speed() {
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let mut ws = Workspace::new(heavy_doc(), None, kmb_bundle::modules());
    let edges = ws.scene_edges().to_vec();
    let n: usize = edges.iter().map(|(_, l)| l.len()).sum();
    for cam in views(&ws.cur.scene, rect) {
        let _ = view3d::picks_at(&ws.cur.scene, &cam, rect, rect.center());
        let pts = grid(rect, 20, 10);
        for see_through in [false, true] {
            let t = Instant::now();
            for p in &pts {
                let _ = view3d::edge_at(&ws.cur.scene, &cam, rect, *p, &edges, see_through);
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0 / pts.len() as f64;
            eprintln!(
                "辺 {n}、透視 {}・向き {:.0}°/{:.0}°・線だけ {see_through}: 1 回 {ms:.3} ms",
                cam.perspective,
                cam.yaw.to_degrees(),
                cam.pitch.to_degrees()
            );
        }
    }
}
