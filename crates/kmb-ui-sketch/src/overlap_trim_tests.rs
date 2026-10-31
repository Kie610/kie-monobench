//! 確かめること: 投影に重なる描いた線のトリムと延長。
use crate::SketchEditor;
use crate::tests::*;
use egui_kittest::Harness;
use kmb_core::{Document, FeatureStatus};
use kmb_sketch::{Entity, EntityId, RefShape, Sketch};
use kmb_ui::Workspace;

fn at(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

struct Fixture {
    h: Harness<'static, Workspace>,
    projected: EntityId,
    own: Option<EntityId>,
    a: [f64; 2],
    b: [f64; 2],
}

fn fixture(own: bool, extend: bool) -> Fixture {
    // 実際のビューが作ったボディの参照から、試験のスケッチを組む。
    let probe = crate::project_tests::later_sketch(|s, body| {
        s.plane = kmb_sketch::SketchPlane::Face {
            feature: kmb_core::FeatureId(body),
            face: format!("f{body}:end"),
            origin: kmb_sketch::FaceOrigin::World,
        };
    });
    let refs = &probe
        .state()
        .editor::<SketchEditor>()
        .unwrap()
        .refs
        .as_ref()
        .unwrap()
        .1;
    let item = refs
        .iter()
        .find(|r| matches!(r.shape,RefShape::Line(a,b) if (a[0]-b[0]).hypot(a[1]-b[1])>10.0))
        .unwrap()
        .clone();
    let RefShape::Line(a, b) = item.shape else {
        unreachable!()
    };
    let mut sketch = Sketch {
        plane: current_sketch(probe.state()).plane,
        ..Default::default()
    };
    let projected = sketch.add_projection_as(&item, false);
    let own = own.then(|| {
        let p = sketch.add_point(a[0], a[1]);
        let q = sketch.add_point(b[0], b[1]);
        sketch.add_line(p, q)
    });
    if own.is_some() {
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let n = [-(b[1] - a[1]) / len * 4.0, (b[0] - a[0]) / len * 4.0];
        for t in if extend { vec![1.25] } else { vec![0.25, 0.75] } {
            let p = at(a, b, t);
            let u = sketch.add_point(p[0] - n[0], p[1] - n[1]);
            let v = sketch.add_point(p[0] + n[0], p[1] + n[1]);
            sketch.add_line(u, v);
        }
    }
    let id = probe.state().selected().unwrap();
    let mut data: serde_json::Value =
        serde_json::from_str(&probe.state().document().to_json()).unwrap();
    let target = data["features"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["id"].as_u64() == Some(id.0))
        .unwrap();
    target["data"] = serde_json::to_value(sketch).unwrap();
    drop(probe);
    let mut ws = ws_of(Document::from_json(&data.to_string()).unwrap(), None, reg());
    ws.open_edit(id);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.run();
    assert_eq!(h.state().report().error_count(), 0);
    Fixture {
        h,
        projected,
        own,
        a,
        b,
    }
}

fn script(h: &mut Harness<'static, Workspace>, text: &str) {
    let batch = h.state_mut().run_script(text).unwrap();
    let start = std::time::Instant::now();
    loop {
        h.step();
        if let Some(r) = h.state_mut().script_result(batch) {
            assert!(r.ok, "{}", r.report());
            break;
        }
        assert!(start.elapsed().as_secs() < 15, "台本の時間切れ");
    }
    h.run();
}

fn solution(h: &Harness<'static, Workspace>) -> kmb_sketch::Solution {
    let Some(FeatureStatus::Ok { outputs, .. }) =
        h.state().report().feature(h.state().selected().unwrap())
    else {
        panic!("スケッチを計算する");
    };
    outputs
        .get("sketch")
        .and_then(kmb_sketch::solution_of)
        .unwrap()
}

#[test]
fn trim_prefers_the_drawn_line_over_the_projected_edge() {
    let Fixture {
        mut h,
        projected,
        a,
        b,
        ..
    } = fixture(true, false);
    let before = serde_json::to_value(current_sketch(h.state())).unwrap();
    script(&mut h, "click \"トリム\"");
    click_at(&mut h, at(a, b, 0.5));
    h.run();
    let saved = current_sketch(h.state());
    let sol = solution(&h);
    assert!(saved.is_projected(projected));
    let hits = saved.hits(&sol, at(a, b, 0.5)[0], at(a, b, 0.5)[1], 0.01);
    let lines: Vec<_> = hits
        .iter()
        .filter(|h| matches!(saved.entity(h.entity), Some(Entity::Line { .. })))
        .collect();
    assert_eq!(lines.len(), 1, "描いた線の真ん中だけを切る");
    assert_eq!(lines[0].entity, projected, "投影した辺は残す");
    for t in [0.1, 0.9] {
        let p = at(a, b, t);
        assert!(
            saved
                .hits(&sol, p[0], p[1], 0.01)
                .iter()
                .any(|h| !saved.is_projected(h.entity)
                    && matches!(saved.entity(h.entity), Some(Entity::Line { .. })))
        );
    }
    script(&mut h, "key Esc");
    h.state_mut().undo();
    h.run();
    assert_eq!(
        serde_json::to_value(current_sketch(h.state())).unwrap(),
        before
    );
}

#[test]
fn extend_prefers_the_drawn_line_over_the_projected_edge() {
    let Fixture {
        mut h,
        projected,
        own,
        a,
        b,
    } = fixture(true, true);
    script(&mut h, "click \"修正 ▾\"\nclick \"延長\"");
    click_at(&mut h, at(a, b, 0.98));
    h.run();
    let saved = current_sketch(h.state());
    let sol = solution(&h);
    assert!(saved.is_projected(projected));
    let Some(Entity::Line { a: p, b: q, .. }) = saved.entity(own.unwrap()) else {
        panic!("描いた線を残す");
    };
    let end = at(a, b, 1.25);
    assert!(
        [sol.points[p], sol.points[q]]
            .iter()
            .any(|p| (p[0] - end[0]).hypot(p[1] - end[1]) < 1e-6),
        "描いた線だけを次の境まで延ばす"
    );
    let Some(Entity::Line { a: p, b: q, .. }) = saved.entity(projected) else {
        panic!("投影した辺を残す");
    };
    assert!(
        [sol.points[p], sol.points[q]]
            .iter()
            .any(|p| (p[0] - b[0]).hypot(p[1] - b[1]) < 1e-6)
    );
}
