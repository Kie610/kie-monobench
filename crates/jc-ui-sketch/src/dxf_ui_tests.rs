//! スケッチの DXF・SVG の挿入と DXF として保存(網羅版 K3)を、人の操作(クリック)で確かめる。

use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use jc_core::Document;
use jc_sketch::Entity;
use jc_ui::{Dialogs, Unsaved, Workspace};
use std::path::PathBuf;

/// 決まったファイルを返すダイアログ。
struct Files {
    open: Option<PathBuf>,
    save: Option<PathBuf>,
}

impl Dialogs for Files {
    fn open_document(&self) -> Option<PathBuf> {
        None
    }
    fn save_document(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn open_file(&self, _: &str, _: &str, _: &[&str]) -> Option<PathBuf> {
        self.open.clone()
    }
    fn save_file(&self, _: &str, _: &str, _: &[&str], _: &str) -> Option<PathBuf> {
        self.save.clone()
    }
    fn unsaved(&self, _: &str) -> Unsaved {
        Unsaved::Cancel
    }
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jc-ui-sketch-dxf-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// 20 × 10 mm の四角(線分 4 本)と、半径 3 mm の円の DXF。
fn square_dxf() -> String {
    let mut s = String::from("  0\nSECTION\n  2\nENTITIES\n");
    let pts = [[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]];
    for k in 0..4 {
        let (a, b) = (pts[k], pts[(k + 1) % 4]);
        s += &format!(
            "  0\nLINE\n  8\n0\n 10\n{}\n 20\n{}\n 11\n{}\n 21\n{}\n",
            a[0], a[1], b[0], b[1]
        );
    }
    s += "  0\nCIRCLE\n  8\n0\n 10\n10\n 20\n5\n 40\n3\n";
    s += "  0\nENDSEC\n  0\nEOF\n";
    s
}

/// 空のスケッチを開いた作業場(ダイアログは `files` を返す)。
fn sketch_with(files: Files) -> Harness<'static, Workspace> {
    let ws = ws_of(Document::new(), None, reg()).with_dialogs(Box::new(files));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.state_mut().run_action(jc_sketch::ADD_SKETCH);
    h.run();
    h
}

fn lines(h: &mut Harness<'static, Workspace>) -> usize {
    let s = current_sketch(h.state());
    s.entities
        .iter()
        .filter(|e| matches!(e, Entity::Line { .. }))
        .count()
}

fn circles(h: &mut Harness<'static, Workspace>) -> usize {
    let s = current_sketch(h.state());
    s.entities
        .iter()
        .filter(|e| matches!(e, Entity::Circle { .. }))
        .count()
}

#[test]
fn the_insert_ribbon_adds_a_dxf_to_the_open_sketch_and_one_undo_removes_it() {
    let file = temp("四角.dxf");
    std::fs::write(&file, square_dxf()).unwrap();
    let mut h = sketch_with(Files {
        open: Some(file),
        save: None,
    });
    assert_eq!(lines(&mut h), 0);
    h.get_by_label("DXF を挿入").click();
    h.run();
    h.run();
    assert_eq!(lines(&mut h), 4, "四角の 4 辺");
    assert_eq!(circles(&mut h), 1);
    // スケッチを直しているまま(終えない)
    let id = editor(h.state_mut()).editor.feature.unwrap();
    assert!(h.state().is_editing(id));
    // Ctrl+Z の 1 回で挿入の前へ
    h.state_mut().undo();
    h.run();
    assert_eq!(lines(&mut h), 0);
}

#[test]
fn the_insert_menu_adds_an_svg() {
    let file = temp("円.svg");
    std::fs::write(
        &file,
        r#"<svg width="40mm" height="40mm" viewBox="0 0 40 40"><circle cx="20" cy="20" r="8"/><path d="M 2 2 L 12 2 L 12 8"/></svg>"#,
    )
    .unwrap();
    let mut h = sketch_with(Files {
        open: Some(file),
        save: None,
    });
    h.get_by_label("挿入 ▾").click();
    h.run();
    h.get_by_label("SVG を挿入").click();
    h.run();
    h.run();
    assert_eq!(circles(&mut h), 1);
    assert_eq!(lines(&mut h), 2);
}

#[test]
fn choosing_no_file_changes_nothing_and_says_so() {
    let mut h = sketch_with(Files {
        open: None,
        save: None,
    });
    h.get_by_label("DXF を挿入").click();
    h.run();
    h.run();
    assert_eq!(lines(&mut h), 0);
    let m = h.state().message().unwrap_or_default().to_string();
    assert!(m.contains("選ばれなかった"), "{m}");
}

#[test]
fn right_click_saves_the_sketch_as_dxf() {
    let out = temp("保存.dxf");
    let _ = std::fs::remove_file(&out);
    let mut h = sketch_with(Files {
        open: None,
        save: Some(out.clone()),
    });
    // 四角を描く
    editor(h.state_mut()).set_tool(crate::Tool::Rectangle);
    h.run();
    click_at(&mut h, [1.19, 16.36]);
    click_at(&mut h, [11.62, 8.54]);
    press_key(&mut h, egui::Key::Escape);
    // 何もない所を右クリック →「DXF として保存…」
    right_click_at(&mut h, [22.04, 0.72]);
    h.get_by_label("DXF として保存…").click();
    h.run();
    h.run();
    let text = std::fs::read_to_string(&out).expect("書き出した");
    assert_eq!(text.matches("\nLINE\n").count(), 4, "{text}");
    // 書いたものを読み直せる
    let back = jc_io_dxf::from_dxf(&text).unwrap();
    assert_eq!(back.shapes.len(), 4);
}

#[test]
fn inserting_without_a_sketch_makes_a_new_one_on_xy() {
    let file = temp("新規.dxf");
    std::fs::write(&file, square_dxf()).unwrap();
    let mut ws = Workspace::new(Document::new(), None, reg());
    ws.run_action_with(jc_io_dxf::INSERT_DXF, Some(file));
    let sketches: Vec<_> = ws
        .document()
        .data()
        .features
        .iter()
        .filter(|f| f.type_id == jc_sketch::SKETCH)
        .collect();
    assert_eq!(sketches.len(), 1);
    let s: jc_sketch::Sketch = serde_json::from_value(sketches[0].data.clone()).unwrap();
    assert_eq!(s.plane, jc_sketch::SketchPlane::Xy);
    // 四角の線 4 本と円 1 つ(点は数えない)
    let lines = s
        .entities
        .iter()
        .filter(|e| matches!(e, jc_sketch::Entity::Line { .. }))
        .count();
    let circles = s
        .entities
        .iter()
        .filter(|e| matches!(e, jc_sketch::Entity::Circle { .. }))
        .count();
    assert_eq!((lines, circles), (4, 1), "{:?}", s.entities);
}
