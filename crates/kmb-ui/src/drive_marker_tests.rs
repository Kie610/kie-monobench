//! 可視印の名前は通常の入力を図から奪わない。
use super::*;

#[test]
fn visible_marker_keeps_the_canvas_click_and_hover_and_omits_clipped_targets() {
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(320.0, 240.0))
        .build_ui_state(
            |ui, state: &mut (usize, bool, egui::Pos2)| {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(200.0, 160.0), egui::Sense::click_and_drag());
                let marker = Rect::from_center_size(rect.center(), egui::vec2(8.0, 8.0));
                ui.painter()
                    .circle_filled(marker.center(), 3.5, egui::Color32::WHITE);
                named_canvas_marker(
                    ui,
                    marker,
                    response.id.with("point"),
                    "view-sketch-input-point",
                    " 2",
                );
                named_canvas_marker(
                    ui,
                    Rect::from_center_size(egui::pos2(-10.0, -10.0), egui::vec2(8.0, 8.0)),
                    response.id.with("outside"),
                    "view-sketch-input-point",
                    " outside",
                );
                state.0 += usize::from(response.clicked());
                state.1 = response.hovered();
                state.2 = rect.center();
            },
            (0, false, egui::Pos2::ZERO),
        );
    h.ctx.add_plugin(UiDriver::default());
    h.ctx.enable_accesskit();
    h.step();
    let nodes = h
        .ctx
        .plugin::<UiDriver>()
        .lock()
        .nodes(h.ctx.pixels_per_point());
    let targets: Vec<_> = nodes
        .iter()
        .filter(|n| n.name.starts_with("スケッチの入力点"))
        .collect();
    assert_eq!(targets.len(), 1, "clip外の印は名前を出さない");
    let node = targets[0];
    assert_eq!(node.role, "Unknown");
    assert_eq!(node.name, "スケッチの入力点 2");
    let at = node.rect.center();
    assert_eq!(at, h.state().2, "clipで実際の印の中心をずらさない");
    h.ctx.plugin::<UiDriver>().lock().push([
        Event::PointerMoved(at),
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.step();
    h.ctx
        .plugin::<UiDriver>()
        .lock()
        .push([Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
    h.step();
    assert_eq!(h.state().0, 1, "名前の中心への実入力は通常Canvasが受ける");
    assert!(h.state().1, "hover-onlyの印は通常Canvasのhoverを奪わない");
}
