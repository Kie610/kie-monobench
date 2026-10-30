//! 確かめること: 重ね描きで渡された全材料の箱と、最初の材料を基準にする原点の軸。

use super::*;

#[test]
fn every_stock_box_is_drawn_and_the_origin_axes_keep_the_first_stock_scale() {
    for (inks, dark) in [(crate::Inks::LIGHT, false), (crate::Inks::DARK, true)] {
        let colors = Colors::from_look(Palette {
            dark,
            canvas: if dark { Color32::BLACK } else { Color32::WHITE },
            ink: if dark { Color32::WHITE } else { Color32::BLACK },
            ink2: Color32::GRAY,
            ink3: Color32::GRAY,
            err: Color32::RED,
            warn: Color32::YELLOW,
            design: Color32::BLUE,
            cam: Color32::GREEN,
            shade3: Color32::GRAY,
            accent: Color32::BLUE,
            inks,
        });
        let draw = |count| {
            let overlay = kmb_core::Overlay {
                boxes: (0..count)
                    .map(|n| {
                        let x = f64::from(n) * 25.0;
                        (
                            [x, 0.0, 0.0],
                            [x + 10.0, 10.0, 3.0],
                            kmb_core::BoxKind::new(kmb_interchange::ink::boxes::WORK, "ストック"),
                        )
                    })
                    .collect(),
                points: vec![[0.0; 3]],
                ..Default::default()
            };
            let mut cam = Camera {
                fitted: true,
                zoom: 3.0,
                target: [25.0, 5.0, 0.0],
                ..Default::default()
            };
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                    ..Default::default()
                },
                |ui| {
                    view3d(
                        ui,
                        &mut cam,
                        &Scene::default(),
                        &[],
                        &overlay,
                        &[],
                        Controls::default(),
                        &colors,
                        Shading::Wireframe,
                        Guides {
                            grid: false,
                            axes: true,
                        },
                    );
                },
            );
            let stocks = output.shapes.iter().filter(|s| matches!(&s.shape,
                egui::Shape::LineSegment { stroke, .. } if stroke.color == colors.stock && stroke.width == 1.5)).count();
            let axes: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::LineSegment { points, stroke } if stroke.width == 2.5 => {
                        Some(points.to_owned())
                    }
                    _ => None,
                })
                .collect();
            // この試験は線の出力だけを調べ、字の画像は描かない。
            output.textures_delta.clear();
            (stocks, axes)
        };
        let (single, axes) = draw(1);
        assert_eq!(single, 12);
        assert_eq!(axes.len(), 3);
        let (two_stocks, same_axes) = draw(2);
        assert_eq!(two_stocks, 24, "材料の箱 2 個、暗い画面 {dark}");
        assert_eq!(same_axes, axes);
    }
}
