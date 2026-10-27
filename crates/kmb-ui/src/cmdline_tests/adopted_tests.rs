//! 名前で探したスライダを人と同じ入力で引き、引いているコマの文書と強調表示を読む。

use super::{harness, run};
use crate::Workspace;
use kmb_samples::sample::{DocBuilder, SketchBuilder};

fn joint_document() -> kmb_core::Document {
    let mut b = DocBuilder::new();
    let mut make = |name: &str, z: f64, height: f64| {
        let mut s = SketchBuilder::xy();
        let p = s.fixed_point(0.0, 0.0);
        s.rectangle(p, "120 mm", "80 mm", [0.0, 0.0, 120.0, 80.0]);
        let sketch = b.sketch(&format!("{name}のスケッチ"), s);
        let body = b.feature(
            "solid.extrude@1",
            name,
            serde_json::json!({
                "profile": sketch, "distance": format!("{height} mm")
            }),
        );
        let placed = if z == 0.0 {
            body
        } else {
            b.feature(
                "solid.move@1",
                &format!("{name}を上げる"),
                serde_json::json!({
                    "body": body, "z": format!("{z} mm")
                }),
            )
        };
        (body, placed)
    };
    let (base, _) = make("本体", 0.0, 60.0);
    let (lid, placed) = make("蓋", 60.0, 6.0);
    b.feature(
        "asm.joint@1",
        "蝶番",
        serde_json::json!({
            "kind": "revolute", "moving": {"kind": "body", "root": lid},
            "base": {"kind": "body", "root": base}, "bodies": [placed],
            "origin": [0.0, 80.0, 66.0], "axis": [1.0, 0.0, 0.0],
            "angle_min": "-110 deg", "angle_max": "0 deg"
        }),
    );
    b.finish("欄のドラッグ")
}

fn drag_frames(mut observe: impl FnMut(&egui_kittest::Harness<'_, Workspace>)) {
    let mut h = harness(Workspace::new(
        joint_document(),
        None,
        kmb_bundle::modules(),
    ));
    let script = include_str!("../../../../apps/kmb-app/tests/ui/欄を引いても別の式を保つ.jcs");
    let (setup, drag) = script.split_once("# 引く間の確認").unwrap();
    let result = run(&mut h, setup);
    assert!(result.ok, "{}", result.report());
    let nodes = h
        .ctx
        .plugin::<crate::drive::UiDriver>()
        .lock()
        .nodes(h.ctx.pixels_per_point());
    let sliders = crate::drive::Query::parse("slider:");
    assert_eq!(
        sliders.find(&nodes, h.ctx.content_rect()).len(),
        1,
        "蝶番の角度スライダは一つ"
    );
    let batch = h.state_mut().run_script(drag).unwrap();
    let start = std::time::Instant::now();
    let mut dragging = 0;
    let mut changed = false;
    loop {
        h.step();
        if h.ctx.input(|i| i.pointer.primary_down()) {
            let joint = h
                .state()
                .document()
                .data()
                .features
                .iter()
                .find(|f| f.name == "蝶番")
                .unwrap();
            dragging += 1;
            changed |= joint.data["angle"].as_str().is_some_and(|a| a != "0 deg");
            observe(&h);
        }
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            break;
        }
        assert!(start.elapsed().as_secs() < 60, "台本が終わる");
        if h.state().is_busy() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    assert!(dragging >= 4, "引いている複数のコマを観測する: {dragging}");
    assert!(changed, "スライダの入力で角度が実際に変わる");
    eprintln!("DRAG_FRAMES={dragging} ANGLE_CHANGED={changed}");
}

#[test]
fn dragging_a_value_keeps_the_expressions_in_other_fields() {
    drag_frames(|h| {
        let joint = h
            .state()
            .document()
            .data()
            .features
            .iter()
            .find(|f| f.name == "蝶番")
            .unwrap();
        assert_eq!(joint.data["angle_min"], "-55 deg * 2");
        assert_eq!(joint.data["angle_max"], "10 deg - 10 deg");
    });
}

#[test]
fn dragging_a_field_does_not_highlight_the_shape_under_the_pointer() {
    let mut over_shape = 0;
    drag_frames(|h| {
        let ws = h.state();
        let Some(pos) = h.ctx.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if crate::view3d::pick_at(&ws.cur.scene, &ws.cur.camera, ws.win.view_rect, pos).is_some() {
            over_shape += 1;
            assert!(
                ws.cur.hover_kind.is_none(),
                "欄を引いている間は形を強調しない"
            );
            assert!(
                ws.cur.hover_feature.is_none(),
                "欄を引いている間は機能を強調しない"
            );
        }
    });
    assert!(over_shape >= 1, "欄を引くカーソルが実際に形の上を通る");
    eprintln!("DRAG_FRAMES_OVER_SHAPE={over_shape}");
}
