//! 格子の線の濃さの試験: 拡大の段の替わり目でつながり、浅い角度で遠くの線が詰まらない。

use super::*;
use std::collections::HashMap;

fn colors() -> Colors {
    Colors::from_look(Palette {
        dark: false,
        canvas: Color32::WHITE,
        ink: Color32::BLACK,
        ink2: Color32::GRAY,
        ink3: Color32::GRAY,
        err: Color32::RED,
        warn: Color32::YELLOW,
        design: Color32::BLUE,
        cam: Color32::GREEN,
        shade3: Color32::GRAY,
        accent: Color32::BLUE,
        inks: crate::Inks::LIGHT,
    })
}

const GRID: Guides = Guides {
    grid: true,
    axes: false,
};

/// 線ごと(向きと位置)のいちばん濃い所の透明度。
fn line_alphas(cam: &Camera, rect: Rect) -> HashMap<(usize, i64), u8> {
    let mut out = HashMap::new();
    guide_segments(cam, rect, &colors(), GRID, &mut |a, b, c| {
        // X に沿う線は Y の位置、Y に沿う線は X の位置で見分ける
        let (along, at) = if a[1] == b[1] { (0, a[1]) } else { (1, a[0]) };
        let e = out
            .entry((along, (at * 1000.0).round() as i64))
            .or_insert(0u8);
        *e = (*e).max(c.a());
    });
    out
}

#[test]
fn the_grid_does_not_jump_where_the_spacing_changes() {
    // 拡大で細かい線の間隔が替わる所で、格子全体がぱっと替わってちらついた(11-01 v1)。替わり目の
    // すぐ前とすぐ後で、どの線の濃さもほとんど変わらない
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    let mut cam = Camera::default();
    cam.set_view(-90.0, 90.0);
    let mut checked = 0;
    let mut zoom = 0.05f32;
    while zoom < 300.0 {
        let next = zoom * 1.002;
        if grid_level(zoom, GRID_PX.0) != grid_level(next, GRID_PX.0) {
            cam.zoom = zoom;
            let before = line_alphas(&cam, rect);
            cam.zoom = next;
            let after = line_alphas(&cam, rect);
            // 画面の真ん中の辺り(薄れる輪の内側)の線を比べる
            let near = |k: &(usize, i64)| (k.1 as f32 / 1000.0).abs() < 200.0 / zoom;
            for k in before.keys().chain(after.keys()).filter(|k| near(k)) {
                let (p, q) = (
                    before.get(k).copied().unwrap_or(0),
                    after.get(k).copied().unwrap_or(0),
                );
                assert!(
                    p.abs_diff(q) <= 12,
                    "拡大 {zoom}: 線 {k:?} の濃さが {p} から {q} へ飛ぶ"
                );
            }
            checked += 1;
        }
        zoom = next;
    }
    assert!(checked >= 6, "替わり目が {checked} か所しかない");
}

#[test]
fn the_grid_keeps_its_strength_between_spacing_changes() {
    // 替わり目の間でも、いちばん粗い線は濃く見える(全体が薄れたままにならない)
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    let mut cam = Camera::default();
    cam.set_view(-90.0, 90.0);
    for zoom in [0.7f32, 1.3, 3.0, 9.0, 21.0] {
        cam.zoom = zoom;
        let most = line_alphas(&cam, rect).values().copied().max().unwrap_or(0);
        assert_eq!(most, 255, "拡大 {zoom}");
    }
}

#[test]
fn far_grid_lines_do_not_crowd_at_a_shallow_angle() {
    // 透視で低く見ると、遠くの線が画面の上で詰まって縞に乱れた。画面の真ん中の縦の線と交わる濃い
    // 線どうしは、数画素より近づかない
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    for (perspective, pitch) in [(true, 6.0), (true, 12.0), (false, 4.0)] {
        let mut cam = Camera {
            zoom: 4.0,
            perspective,
            ..Default::default()
        };
        cam.set_view(-90.0, pitch);
        let x0 = rect.center().x;
        let mut ys = Vec::new();
        guide_segments(&cam, rect, &colors(), GRID, &mut |a, b, c| {
            if a[1] != b[1] || c.a() < 60 {
                return;
            }
            let Some((p, q)) = cam.segment(a, b, rect) else {
                return;
            };
            if (p.x - x0) * (q.x - x0) > 0.0 || (p.x - q.x).abs() < 1e-3 {
                return;
            }
            let t = (x0 - p.x) / (q.x - p.x);
            ys.push(p.y + (q.y - p.y) * t);
        });
        ys.sort_by(f32::total_cmp);
        assert!(
            ys.len() >= 4,
            "{perspective} {pitch}: 濃い線が {} 本",
            ys.len()
        );
        let tight = ys.windows(2).map(|w| w[1] - w[0]).fold(f32::MAX, f32::min);
        assert!(
            tight >= 6.0,
            "{perspective} {pitch}: 濃い線の間が {tight} 画素"
        );
    }
}

#[test]
fn the_ground_grid_stays_when_seen_from_low() {
    // 低く見ると地面の格子が薄れて見えなくなった。5 度でも濃い線が残る(前は 8 度ほどから薄れ、5 度でほぼ消えていた)
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    let mut cam = Camera {
        zoom: 4.0,
        perspective: true,
        ..Default::default()
    };
    cam.set_view(-60.0, 5.0);
    let mut most = 0u8;
    guide_segments(&cam, rect, &colors(), GRID, &mut |a, b, c| {
        assert!(a[2] == 0.0 && b[2] == 0.0);
        most = most.max(c.a());
    });
    assert!(most >= 200, "{most}");
}
