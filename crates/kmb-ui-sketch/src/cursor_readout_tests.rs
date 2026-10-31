//! 確かめること: 描いている長さの札がマウスの矢印と重ならない。
use crate::tests::*;
use egui::{Pos2, Rect, Vec2};
use egui_kittest::Harness;
use kmb_ui::Workspace;

fn numbers(h: &Harness<'static, Workspace>) -> Vec<Rect> {
    let bounds = h
        .state()
        .editor::<crate::SketchEditor>()
        .unwrap()
        .view
        .seen
        .unwrap()
        .1;
    let shapes = &h.output().shapes;
    shapes
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let egui::Shape::Text(t) = &s.shape else {
                return None;
            };
            t.galley.text().trim_end_matches('°').parse::<f64>().ok()?;
            let text = Rect::from_min_size(t.pos, t.galley.size());
            if !bounds.contains(t.pos) {
                return None;
            }
            shapes[..i].iter().rev().find_map(|s| {
                let egui::Shape::Rect(r) = &s.shape else {
                    return None;
                };
                r.rect.contains_rect(text).then_some(r.rect)
            })
        })
        .collect()
}

#[test]
fn a_short_line_readout_does_not_cover_the_arrow() {
    let mut h = empty_sketch();
    press_key(&mut h, egui::Key::L);
    click_at(&mut h, [5.0, 5.0]);
    let p = screen(&h, [5.0, 5.0]) - Vec2::splat(12.0);
    h.hover_at(p);
    h.run();
    let arrow = Rect::from_min_size(p, Vec2::splat(24.0));
    let rects = numbers(&h);
    assert!(rects.len() >= 2, "長さと角度の札: {rects:?}");
    let tip = h
        .output()
        .shapes
        .iter()
        .find_map(|s| {
            let egui::Shape::Text(t) = &s.shape else {
                return None;
            };
            (t.galley.text() == "次の点を指定")
                .then(|| Rect::from_min_size(t.pos, t.galley.size()).expand2(Vec2::new(5.0, 2.0)))
        })
        .expect("次の点を指定する案内");
    assert!(
        rects.iter().all(|r| !r.intersects(tip)),
        "案内とも重ならない: {tip:?} {rects:?}"
    );
    assert!(
        rects.iter().all(|r| !r.intersects(arrow)),
        "矢印 {arrow:?} と札 {rects:?}"
    );
}

#[test]
fn untyped_readouts_stay_inside_the_view_at_each_corner() {
    for (width, scale) in [(1280.0, 1.0), (1920.0, 1.0), (1280.0, 2.5), (1920.0, 2.5)] {
        for corner in 0..4 {
            let mut h = empty_sketch();
            h.state_mut().layout_mut().appearance.text_scale = scale;
            h.set_size(Vec2::new(width, 800.0));
            h.run();
            press_key(&mut h, egui::Key::L);
            let bounds = editor(h.state_mut()).view.seen.unwrap().1;
            let p = Pos2::new(
                if corner % 2 == 0 {
                    bounds.left() + 2.0
                } else {
                    bounds.right() - 2.0
                },
                if corner < 2 {
                    bounds.top() + 2.0
                } else {
                    bounds.bottom() - 2.0
                },
            );
            let start = p + Vec2::new(
                if corner % 2 == 0 { 12.0 } else { -12.0 },
                if corner < 2 { 12.0 } else { -12.0 },
            );
            let world = editor(h.state_mut())
                .view
                .last
                .as_ref()
                .unwrap()
                .to_world(start);
            click_at(&mut h, world);
            h.hover_at(p);
            h.run();
            let arrow = Rect::from_min_size(p, Vec2::splat(24.0));
            let rects = numbers(&h);
            assert_eq!(
                rects.len(),
                2,
                "長さと角度 {width}/{scale}/{corner}: {rects:?}"
            );
            for r in &rects {
                assert!(
                    bounds.contains_rect(*r),
                    "札がビューに収まる: {bounds:?} {r:?}"
                );
                assert!(!r.intersects(arrow), "矢印と重ならない: {arrow:?} {r:?}");
            }
            assert!(
                !rects[0].intersects(rects[1]),
                "札どうしも重ならない: {rects:?}"
            );
        }
    }
}
