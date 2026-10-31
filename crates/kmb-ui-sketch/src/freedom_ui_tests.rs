//! 確かめること: MCP と同じ画面への要求で一覧を開き、要素を強調する。文書は変えない。
use crate::tests::*;
use egui_kittest::{Harness, kittest::Queryable};
use kmb_core::{Document, Edit};
use kmb_sketch::{ConstraintKind, EntityId, Sketch};
use kmb_ui::Workspace;
use serde_json::json;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(
    language: &str,
    live: bool,
) -> (Harness<'static, Workspace>, EntityId, EntityId, Scratch) {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(10.0, 0.0);
    let line = s.add_line(a, b);
    s.add_constraint(ConstraintKind::Fix { point: a });
    s.add_constraint(ConstraintKind::Horizontal { line });
    let point = s.add_point(20.0, 5.0);
    let mut doc = Document::new();
    let id = doc
        .apply_edits(
            "自由な要素",
            vec![Edit::Insert {
                type_id: kmb_sketch::SKETCH.into(),
                name: "部分的な拘束".into(),
                data: serde_json::to_value(s).unwrap(),
            }],
        )
        .unwrap()[0];
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        Scratch(std::env::temp_dir().join(format!("sketch-freedom-{}-{n}", std::process::id())));
    let mut ws = ws_of(doc, None, reg()).with_data_dir(dir.0.clone());
    ws.layout_mut().ai_live = live;
    ws.layout_mut().appearance.language = language.into();
    ws.open_edit(id);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    assert_eq!(
        h.state().live_port().is_some(),
        live,
        "画面の要求の待ち受け"
    );
    (h, line, point, dir)
}

fn app(h: &mut Harness<'static, Workspace>, dir: &Path, tool: &str, args: serde_json::Value) {
    let dir = dir.to_owned();
    let tool = tool.to_owned();
    let request = std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args));
    let start = std::time::Instant::now();
    while !request.is_finished() {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(start.elapsed().as_secs() < 15, "画面の要求が時間切れ");
    }
    let reply = request.join().unwrap().unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    h.run_steps(8);
}

fn line_is_lit(h: &Harness<'static, Workspace>) -> bool {
    let a = screen(h, [0.0, 0.0]);
    let b = screen(h, [10.0, 0.0]);
    h.output().shapes.iter().any(|s| match &s.shape {
        egui::Shape::Path(p) => {
            p.stroke.width >= 8.0
                && p.points.len() == 2
                && ((p.points[0].distance(a) < 1.0 && p.points[1].distance(b) < 1.0)
                    || (p.points[1].distance(a) < 1.0 && p.points[0].distance(b) < 1.0))
        }
        _ => false,
    })
}

#[test]
fn mcp_ui_tools_open_pick_and_hover_free_entities_without_changing_the_document() {
    let (mut h, line, point, dir) = fixture("ja", true);
    let before = h.state().document().to_json();
    assert!(!line_is_lit(&h));
    app(&mut h, &dir.0, "ui_click", json!({"target":"自由度"}));
    let row = format!("{line}   1");
    app(&mut h, &dir.0, "ui_click", json!({"target":row}));
    app(
        &mut h,
        &dir.0,
        "ui_expect",
        json!({"target":row,"state":"exists"}),
    );
    assert_eq!(editor(h.state_mut()).editor.freedom.picked, Some(line));
    assert!(line_is_lit(&h));
    h.get_by_label(&format!("{point}   2")).hover();
    h.run_steps(8);
    assert_eq!(editor(h.state_mut()).editor.freedom.hovered, Some(point));
    let at = screen(&h, [20.0, 5.0]);
    assert!(h.output().shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Circle(c) if c.center.distance(at)<1.0 && (c.radius-8.0).abs()<0.1)),"自由な点も強調して描く");
    app(&mut h, &dir.0, "ui_click", json!({"target":"自由度"}));
    assert!(!editor(h.state_mut()).editor.freedom.open);
    assert!(!line_is_lit(&h));
    assert_eq!(h.state().document().to_json(), before);
    h.state_mut().live_disconnect();
    drop(h);
}

#[test]
fn the_read_only_freedom_field_uses_the_selected_language() {
    let (mut h, line, _, dir) = fixture("en", true);
    app(
        &mut h,
        &dir.0,
        "ui_click",
        json!({"target":"Degrees of freedom"}),
    );
    app(
        &mut h,
        &dir.0,
        "ui_expect",
        json!({"target":format!("{line}   1"),"state":"exists"}),
    );
    assert!(h.query_by_label("自由度").is_none());
    h.state_mut().live_disconnect();
    drop(h);
}

#[test]
fn the_picked_free_line_is_also_highlighted_after_turning_to_3d() {
    let (mut h, line, _, _dir) = fixture("ja", false);
    let before = h.state().document().to_json();
    let script = format!("click \"自由度\"\nclick \"{line}   1\"");
    let batch = h.state_mut().run_script(&script).unwrap();
    let start = std::time::Instant::now();
    loop {
        h.step();
        if let Some(reply) = h.state_mut().script_result(batch) {
            assert!(reply.ok, "{}", reply.report());
            break;
        }
        assert!(start.elapsed().as_secs() < 15, "自由度の台本が時間切れ");
    }
    h.run_steps(8);
    assert!(line_is_lit(&h));
    let at = editor(h.state_mut())
        .view
        .last
        .as_ref()
        .unwrap()
        .center_screen;
    crate::space_view_tests::right_drag(&mut h, at, at + egui::vec2(90.0, 50.0));
    assert!(editor(h.state_mut()).view.space.is_some());
    assert!(line_is_lit(&h), "3D でも同じ要素を光らせる");
    assert_eq!(h.state().document().to_json(), before);
}

#[test]
fn freedom_rows_remain_readable_in_the_visible_panel_at_large_text_scale() {
    for width in [1280.0, 1920.0] {
        let (mut h, _, point, _dir) = fixture("ja", false);
        h.state_mut().layout_mut().appearance.text_scale = 2.5;
        h.state_mut().layout_mut().appearance.icon_scale = 2.5;
        h.set_size(egui::vec2(width, 800.0));
        h.run_steps(16);
        let batch = h.state_mut().run_script("click \"自由度\"").unwrap();
        let start = std::time::Instant::now();
        loop {
            h.step();
            if let Some(r) = h.state_mut().script_result(batch) {
                assert!(r.ok, "{}", r.report());
                break;
            }
            assert!(start.elapsed().as_secs() < 15);
        }
        h.run_steps(32);
        let readable = |h: &Harness<'static, Workspace>, label: &str| {
            h.output().shapes.iter().any(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == label => s
                    .clip_rect
                    .contains_rect(egui::Rect::from_min_size(t.pos, t.galley.size())),
                _ => false,
            })
        };
        assert!(readable(&h, "E2   1"), "最初の行の文字が切れない: {width}");
        let last = format!("{point}   2");
        h.get_by_label(&last).scroll_to_me();
        h.run_steps(32);
        assert!(
            readable(&h, &last),
            "最後の行へ送って文字が切れない: {width}"
        );
    }
}
