//! 確かめること: ボディの欄でかざした時は、手前の1面だけでなく同じボディの別の面も光る。
use crate::*;
use egui_kittest::Harness;
use kmb_core::FeatureId;

fn fixture(type_id: Option<&str>) -> (Workspace, FeatureId) {
    let mut b = kmb_samples::sample::DocBuilder::new();
    let mut bodies = Vec::new();
    for (name, x) in [("左の箱", 0.0), ("右の箱", 30.0)] {
        let mut s = kmb_samples::sample::SketchBuilder::new(kmb_sketch::SketchPlane::Xy);
        let p = s.fixed_point(x, 0.0);
        s.rectangle(p, "10 mm", "10 mm", [x, 0.0, 10.0, 10.0]);
        let sketch = b.feature(kmb_sketch::SKETCH, &format!("{name}の輪郭"), s.build());
        let body = b.feature(
            kmb_cad_extrude::EXTRUDE,
            name,
            kmb_cad_extrude::Extrude {
                profile: Some(sketch),
                distance: "10 mm".into(),
                operation: kmb_cad::Operation::New,
                ..Default::default()
            },
        );
        bodies.push(body);
    }
    let mut ws = Workspace::new(b.finish("2つの箱"), None, kmb_bundle::modules());
    ws.clear_selection();
    let body = if let Some(type_id) = type_id {
        let id = ws.add_described(type_id).unwrap();
        let (_, field) = ws.active_pick_field().expect("形を選ぶ欄");
        let refs = field.refs(&ws.document().data().feature(id).unwrap().data);
        bodies
            .into_iter()
            .find(|body| refs.iter().all(|r| r.feature() != *body))
            .unwrap()
    } else {
        bodies[1]
    };
    (ws, body)
}

type Images = std::sync::Arc<
    std::sync::Mutex<std::collections::HashMap<egui::TextureId, std::sync::Arc<egui::ColorImage>>>,
>;
struct Capture(Images);
impl egui_kittest::TestRenderer for Capture {
    fn handle_delta(&mut self, delta: &mut egui::TexturesDelta) {
        let mut images = self.0.lock().unwrap();
        for (id, updates) in &delta.set {
            if *id == egui::TextureId::default() {
                continue;
            }
            for update in updates {
                if update.pos.is_none() {
                    let egui::ImageData::Color(image) = &update.image;
                    images.insert(*id, image.clone());
                }
            }
        }
        for id in &delta.free {
            images.remove(id);
        }
        delta.clear();
    }
}
fn image(images: &Images) -> std::sync::Arc<egui::ColorImage> {
    images
        .lock()
        .unwrap()
        .values()
        .max_by_key(|image| image.size[0] * image.size[1])
        .expect("3Dの絵")
        .clone()
}

fn pixel(image: &egui::ColorImage, rect: egui::Rect, at: egui::Pos2) -> egui::Color32 {
    let x = ((at.x - rect.left()) / rect.width() * image.size[0] as f32) as usize;
    let y = ((at.y - rect.top()) / rect.height() * image.size[1] as f32) as usize;
    image.pixels[y.min(image.size[1] - 1) * image.size[0] + x.min(image.size[0] - 1)]
}

fn hover_images(
    type_id: Option<&str>,
) -> (
    std::sync::Arc<egui::ColorImage>,
    std::sync::Arc<egui::ColorImage>,
    egui::Rect,
    Vec<egui::Pos2>,
) {
    let (ws, body) = fixture(type_id);
    let images = Images::default();
    let mut h = Harness::builder()
        .renderer(Capture(images.clone()))
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.event(egui::Event::PointerGone);
    h.state_mut().cur.camera.zoom *= 1.001;
    h.step();
    let before = image(&images);
    let saved = h.state().document().to_json();
    let ws = h.state();
    let rect = ws.win.view_rect;
    let mesh = &ws
        .cur
        .scene
        .bodies
        .iter()
        .find(|b| b.feature == body)
        .unwrap()
        .mesh;
    let mut faces = Vec::new();
    for (k, name) in mesh.face_names.iter().enumerate() {
        let points: Vec<_> = mesh
            .triangles
            .iter()
            .zip(&mesh.triangle_faces)
            .filter(|(_, face)| **face as usize == k)
            .flat_map(|(t, _)| t.iter().map(|i| mesh.positions[*i as usize]))
            .collect();
        if points.is_empty() {
            continue;
        }
        let center = points.iter().fold([0.0; 3], |mut a, p| {
            for i in 0..3 {
                a[i] += p[i] / points.len() as f32;
            }
            a
        });
        let at = ws.cur.camera.project(center, rect).0;
        if view3d::pick_at(&ws.cur.scene, &ws.cur.camera, rect, at)
            .is_some_and(|p| p.feature == body && p.face == *name)
        {
            faces.push(at);
        }
    }
    assert!(faces.len() >= 2, "斜めから見える箱の2面");
    h.event(egui::Event::PointerMoved(faces[0]));
    h.step();
    let after = image(&images);
    h.event(egui::Event::PointerGone);
    h.step();
    let cleared = image(&images);
    assert_eq!(
        pixel(&before, rect, faces[1]),
        pixel(&cleared, rect, faces[1]),
        "かざすのをやめると戻る"
    );
    assert_eq!(h.state().document().to_json(), saved);
    (before, after, rect, faces)
}

#[test]
fn hovering_a_body_field_lights_another_visible_face_of_the_same_body() {
    let (before, after, rect, faces) = hover_images(Some(kmb_cad_boolean::BOOLEAN));
    assert_ne!(
        pixel(&before, rect, faces[0]),
        pixel(&after, rect, faces[0]),
        "かざした面が光る"
    );
    assert_ne!(
        pixel(&before, rect, faces[1]),
        pixel(&after, rect, faces[1]),
        "同じボディの別の面も光る"
    );
}

#[test]
fn a_field_that_accepts_faces_and_bodies_keeps_hovering_just_the_face() {
    let (before, after, rect, faces) = hover_images(Some(kmb_cad_edge::SHELL));
    assert_ne!(
        pixel(&before, rect, faces[0]),
        pixel(&after, rect, faces[0])
    );
    assert_eq!(
        pixel(&before, rect, faces[1]),
        pixel(&after, rect, faces[1])
    );
}

#[test]
fn ordinary_hovering_keeps_highlighting_just_the_face() {
    let (before, after, rect, faces) = hover_images(None);
    assert_ne!(
        pixel(&before, rect, faces[0]),
        pixel(&after, rect, faces[0])
    );
    assert_eq!(
        pixel(&before, rect, faces[1]),
        pixel(&after, rect, faces[1])
    );
}
