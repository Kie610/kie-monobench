//! 画面の試験: ボディの物理材料と外観(M26)、測る道具の質量と重心(M47)、
//! フィレットの行の半径タイプの欄(M16)。

use crate::Workspace;
use crate::measure::Target;
use jc_cad_extrude::{EXTRUDE, Extrude};
use jc_core::FacePick;
use jc_samples::sample::{DocBuilder, SketchBuilder};
use jc_sketch::SketchPlane;

/// 30×20×10 の板。
fn plate() -> Workspace {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "30 mm", "20 mm", [0.0, 0.0, 30.0, 20.0]);
    let sk = b.feature(jc_sketch::SKETCH, "板のスケッチ", s.build());
    b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut ws = Workspace::new(b.finish("板"), None, jc_bundle::modules());
    ws.wait();
    ws
}

fn rgb(c: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

#[test]
fn a_material_colours_the_body_and_gives_the_measured_mass() {
    // 1 つの板で順に確かめる:
    // - 材料でボディに色が付き、外観が先。取り消しで戻る(元 a_material_colours_the_body_and_an_appearance_overrides_it)
    // - 面を測るとボディの重心と、材料を付けると質量が出る(元 measuring_a_face_shows_the_mass_and_centroid_of_its_body)
    let mut ws = plate();
    let (last, root) = ws.body_roots()[0];
    assert_eq!(
        ws.cur.scene.bodies[0].tint, None,
        "材料の無いボディは既定の色"
    );
    assert!(ws.set_body_material(root, "アルミ"));
    ws.wait();
    let al = jc_look::material::phys_material("アルミ").unwrap();
    assert_eq!(ws.cur.scene.bodies[0].feature, last);
    assert_eq!(ws.cur.scene.bodies[0].tint, Some(rgb(al.color)));
    // 外観は材料の色より先
    assert!(ws.set_body_appearance(root, "赤"));
    ws.wait();
    let red = jc_look::material::appearance("赤").unwrap();
    assert_eq!(ws.cur.scene.bodies[0].tint, Some(rgb(red.color)));
    // 取り消すと材料の色に戻り、もう一度で既定の色
    ws.undo();
    ws.wait();
    assert_eq!(ws.cur.scene.bodies[0].tint, Some(rgb(al.color)));
    ws.undo();
    ws.wait();
    assert_eq!(ws.cur.scene.bodies[0].tint, None);

    // 取り消して材料を付けていない板で、面を測る
    let (last, root) = ws.body_roots()[0];
    ws.toggle_measure();
    let face = ws.cur.scene.bodies[0].mesh.face_names[0].clone();
    let pick = || {
        Target::Face(FacePick {
            feature: last,
            face: face.clone(),
        })
    };
    ws.cur.measure.as_mut().unwrap().picks = vec![pick()];
    ws.measure_refresh();
    let rows = ws.cur.measure.as_ref().unwrap().report.rows.clone();
    let row = |name: &str| rows.iter().find(|r| r.0 == name).map(|r| r.1.clone());
    assert_eq!(
        row("ボディの重心").as_deref(),
        Some("(15.000, 10.000, 5.000) mm")
    );
    // 材料を付けていない板は既定の鋼(6000 mm³ × 7.85 g/cm³ = 47.1 g)
    assert!(row("材料").unwrap().starts_with("鋼"), "{rows:?}");
    assert_eq!(row("質量").as_deref(), Some("47.100 g"), "{rows:?}");
    assert!(ws.cur.measure.as_ref().unwrap().report.point.is_some());
    // 材料を付けると質量が出る(6000 mm³ × 2.7 g/cm³ = 16.2 g)
    ws.set_body_material(root, "アルミ");
    ws.wait();
    let (last, _) = ws.body_roots()[0];
    ws.cur.measure.as_mut().unwrap().picks = vec![Target::Face(FacePick {
        feature: last,
        face: face.clone(),
    })];
    ws.measure_refresh();
    let rows = ws.cur.measure.as_ref().unwrap().report.rows.clone();
    let row = |name: &str| rows.iter().find(|r| r.0 == name).map(|r| r.1.clone());
    assert_eq!(row("質量").as_deref(), Some("16.200 g"), "{rows:?}");
}

#[test]
fn the_fillet_row_shows_end_and_mid_radii_only_when_variable() {
    let ws = plate();
    let desc = ws.win.registry.descriptor(jc_cad_edge::FILLET).unwrap();
    let rows = desc.fields.iter().find(|f| f.key == "rows").unwrap();
    let jc_core::FieldKind::Rows { fields } = &rows.kind else {
        panic!("行の並び")
    };
    let end = fields.iter().find(|f| f.key == "end_radius").unwrap();
    let data = serde_json::json!({"rows": [
        {"radius": "1 mm"},
        {"radius": "1 mm", "kind": "variable"},
    ]});
    assert!(
        !rows.row_field(0, end).applies(&data),
        "一定の行には出さない"
    );
    assert!(rows.row_field(1, end).applies(&data), "徐変の行には出す");
    assert!(rows.applies_in_some_row(end, &data));
    let constant = serde_json::json!({"rows": [{"radius": "1 mm"}]});
    assert!(!rows.applies_in_some_row(end, &constant));
}
