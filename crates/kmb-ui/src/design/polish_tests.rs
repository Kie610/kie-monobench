//! 確かめること: 設計の下絵・編集パネル・書き出しの並びを、実際の画面と操作で調べる。

use crate::tests::reg;
use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::sample::{DocBuilder, SketchBuilder};

fn harness(ws: Workspace, size: [f32; 2]) -> Harness<'static, Workspace> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    crate::install_fonts(&h.ctx, None);
    for _ in 0..3 {
        h.run();
    }
    h
}

fn script(h: &mut Harness<'_, Workspace>, text: &str) {
    let batch = h.state_mut().run_script(text).expect("台本を読める");
    let start = std::time::Instant::now();
    loop {
        h.step();
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            return;
        }
        assert!(
            start.elapsed().as_secs() < 30,
            "台本の時間切れ: {:?}",
            h.state().command_log()
        );
    }
}

#[test]
fn the_edit_footer_has_no_overlapping_rectangles_at_250_percent() {
    for (changes, rollback) in [(1, false), (99, false), (99, true)] {
        let mut b = DocBuilder::new();
        let profile = rectangle(&mut b, "輪郭", [0.0, 0.0], 10.0);
        let id = b.feature(
            kmb_cad_extrude::EXTRUDE,
            "板",
            kmb_cad_extrude::Extrude {
                profile: Some(profile),
                distance: "2 mm".into(),
                ..Default::default()
            },
        );
        let mut ws = Workspace::new(b.finish("編集パネルの帯"), None, reg());
        ws.win.layout.appearance.text_scale = 2.5;
        ws.win.layout.appearance.icon_scale = 2.5;
        ws.open_edit(id);
        for n in 0..changes {
            assert!(ws.rename_feature(id, &format!("輪郭{n}")));
        }
        if rollback {
            let tx = kmb_session::edit::step_rollback(ws.document().data(), -1).unwrap();
            assert!(ws.apply(tx));
        }
        let edits = changes + usize::from(rollback);
        let mut h = harness(ws, [1280.0, 800.0]);
        h.run();
        let status = if rollback {
            "ロールバックの後"
        } else {
            "計算できた"
        };
        let labels = [
            "OKEnter".to_string(),
            "キャンセルEsc".into(),
            status.into(),
            format!("{edits} 回直した"),
        ];
        let rects: Vec<_> = labels
            .iter()
            .map(|label| {
                h.get_all_by_label(label)
                    .next()
                    .unwrap_or_else(|| panic!("編集の帯に {label} がある"))
                    .rect()
            })
            .collect();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        for (i, r) in rects.iter().enumerate() {
            assert!(
                r.is_positive() && screen.contains_rect(*r),
                "{}: {r:?}",
                labels[i]
            );
            for j in 0..i {
                assert!(
                    !r.intersect(rects[j]).is_positive(),
                    "{} {r:?} と {} {:?} が重なる",
                    labels[i],
                    labels[j],
                    rects[j]
                );
            }
        }
    }
}

#[test]
fn the_export_group_has_a_large_button_with_or_without_selection() {
    let part = kmb_samples::reference_part();
    let selected = part.hole_pattern;
    let mut h = harness(Workspace::new(part.doc, None, reg()), [1280.0, 800.0]);
    for width in [1280.0, 1920.0] {
        h.set_size(egui::vec2(width, 800.0));
        for pick in [false, true] {
            if pick {
                h.state_mut().select(selected);
            } else {
                h.state_mut().clear_selection();
            }
            h.run();
            let header = h.get_by_label("書き出し ▾").rect();
            let tile = h
                .query_by_label("STEP を書き出す…")
                .or_else(|| h.query_by_label("書き出し(畳んだまとまり)"))
                .expect("書き出しの名前だけの空列にしない")
                .rect();
            assert!(
                tile.is_positive() && tile.height() > header.height(),
                "{width} 選択{pick}: {tile:?}"
            );
            assert!(
                tile.min.x >= 0.0 && tile.max.x <= width && tile.min.y >= 0.0,
                "{width}: {tile:?}"
            );
            assert!(
                tile.bottom() <= header.top() + 1.0,
                "名前の上に大きなボタンがある: {tile:?}, {header:?}"
            );
            assert!(
                tile.center().x >= header.left() - 8.0 && tile.center().x <= header.right() + 8.0
            );
        }
    }
}

fn rectangle(b: &mut DocBuilder, name: &str, corner: [f64; 2], size: f64) -> FeatureId {
    let mut sketch = SketchBuilder::xy();
    let p = sketch.fixed_point(corner[0], corner[1]);
    sketch.rectangle(
        p,
        &format!("{size} mm"),
        &format!("{size} mm"),
        [corner[0], corner[1], size, size],
    );
    b.feature(kmb_sketch::SKETCH, name, sketch.build())
}

#[test]
fn a_tapered_extrude_keeps_sloping_preview_edges_while_height_is_typed() {
    let mut b = DocBuilder::new();
    let outer = rectangle(&mut b, "板の輪郭", [0.0, 0.0], 40.0);
    let plate = b.feature(
        kmb_cad_extrude::EXTRUDE,
        "板",
        kmb_cad_extrude::Extrude {
            profile: Some(outer),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let inner = rectangle(&mut b, "切る輪郭", [10.0, 10.0], 20.0);
    let cut = b.feature(
        kmb_cad_extrude::EXTRUDE,
        "切り取り",
        kmb_cad_extrude::Extrude {
            profile: Some(inner),
            distance: "5 mm".into(),
            target: Some(plate),
            operation: kmb_cad::Operation::Cut,
            ..Default::default()
        },
    );
    let mut ws = Workspace::new(b.finish("テーパーの予告"), None, reg());
    assert_eq!(ws.report().error_count(), 0);
    ws.open_edit(cut);
    let mut h = harness(ws, [1400.0, 1100.0]);
    script(
        &mut h,
        r#"type "テーパー角度" "10 deg"
type "距離" "7 mm" none"#,
    );
    for _ in 0..4 {
        h.step();
    }
    assert!(h.state().is_editing(cut), "入力中は編集を続ける");
    assert!(
        h.ctx.memory(|m| m.focused().is_some()),
        "確定前の入力欄にキーボードがある"
    );
    assert_eq!(h.state().report().error_count(), 0);
    let colors = view3d::Colors::from_look(&h.state().win.look);
    let ghosts = h.state_mut().tool_ghosts(&colors);
    assert_eq!(ghosts.len(), 1);
    let pts: Vec<_> = ghosts[0].edges.iter().flatten().collect();
    let span = |z: f32| {
        let xs: Vec<_> = pts
            .iter()
            .filter(|p| (p[2] - z).abs() < 1e-4)
            .map(|p| p[0])
            .collect();
        assert!(!xs.is_empty(), "高さ{z}の縁がある");
        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max)
            - xs.iter().copied().fold(f32::INFINITY, f32::min)
    };
    let expected = 2.0 * 7.0 * 10.0_f32.to_radians().tan();
    assert!(
        (span(0.0) - span(7.0) - expected).abs() < 1e-3,
        "距離とテーパー角で壁が傾く"
    );
}
