//! 視点の回し方・投影・寄る・決まった向きの試験。

use super::*;

#[test]
fn a_view_squeezed_to_nothing_still_draws_a_bounded_grid() {
    // 字とアイコンを 250 % にすると 3D のビューの幅が 0 になり、合わせた拡大が 0 になって
    // 格子の線を 40 億本引こうとして止まっていた
    let mut c = Camera::default();
    let none = Rect::from_min_size(Pos2::ZERO, Vec2::new(0.0, 300.0));
    c.fit_box([-10.0; 3], [10.0; 3], none);
    assert!(c.zoom.is_finite() && c.zoom > 0.0, "{}", c.zoom);
    let colors = Colors::from_look(Palette {
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
    });
    let g = Guides {
        grid: true,
        axes: true,
    };
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    for zoom in [c.zoom, 0.0, f32::MIN_POSITIVE] {
        let mut cam = c;
        cam.zoom = zoom;
        let mut lines = 0usize;
        guide_segments(&cam, rect, &colors, g, &mut |_, _, _| {
            lines += 1;
            assert!(lines <= 10_000, "拡大 {zoom} で格子の線が多すぎる");
        });
    }
}

#[test]
fn the_grid_lies_on_the_origin_plane_the_view_faces() {
    // M10: 上・斜めからは XY 平面、前からは ZX 平面、右からは YZ 平面。真横でも斜めの向きなら描かない
    let colors = Colors::from_look(Palette {
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
    });
    let g = Guides {
        grid: true,
        axes: false,
    };
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    for (yaw, pitch, flat) in [
        (-60.0, 30.0, Some(2)),
        (-90.0, 90.0, Some(2)),
        (-90.0, 0.0, Some(1)),
        (90.0, 0.0, Some(1)),
        (0.0, 0.0, Some(0)),
        (180.0, 0.0, Some(0)),
        (-45.0, 0.0, None),
    ] {
        let mut cam = Camera {
            target: [3.0, 5.0, 7.0],
            ..Default::default()
        };
        cam.set_view(yaw, pitch);
        let mut lines = 0;
        guide_segments(&cam, rect, &colors, g, &mut |a, b, _| {
            lines += 1;
            let k = flat.expect("描かない向き");
            assert!(a[k] == 0.0 && b[k] == 0.0, "{yaw} {pitch}: {a:?} {b:?}");
        });
        assert_eq!(lines > 0, flat.is_some(), "{yaw} {pitch}");
    }
}

#[test]
fn flat_light_tells_the_visible_faces_apart() {
    // 視点の真正面からの光 1 つでは、等角の向きで見える上・手前・右の面がほぼ同じ明るさだった
    let eye = normalize([0.45, -0.75, 0.5]);
    let flat = FlatLight::new(eye, [0.0; 3], 30.0);
    let k = [[0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]]
        .map(|n| flat.shade([0.0; 3], n, 1.0).0);
    for i in 0..3 {
        for j in i + 1..3 {
            assert!((k[i] - k[j]).abs() > 0.05, "{k:?}");
        }
    }
    // 上の面が一番明るい
    assert!(k[0] > k[1] && k[0] > k[2], "{k:?}");
    assert!(k.iter().all(|v| (0.4..=1.1).contains(v)), "{k:?}");
}

#[test]
fn flat_top_is_brighter_near_the_viewer() {
    // 平らな上の面の中に、手前が明るく奥が暗い濃淡が付く
    let eye = normalize([0.45, -0.75, 0.5]);
    let flat = FlatLight::new(eye, [0.0; 3], 30.0);
    let near = flat.shade([10.0, -15.0, 0.0], [0.0, 0.0, 1.0], 1.0).0;
    let mid = flat.shade([0.0; 3], [0.0, 0.0, 1.0], 1.0).0;
    let far = flat.shade([-10.0, 15.0, 0.0], [0.0, 0.0, 1.0], 1.0).0;
    assert!(near > mid && mid > far, "{near} {mid} {far}");
    // 遮られた所は暗い
    assert!(flat.shade([0.0; 3], [0.0, 0.0, 1.0], 0.5).0 < mid);
}

#[test]
fn body_lines_stand_out_from_the_body() {
    // M8: 暗い画面で 2 つ目のボディを明るくしすぎると、明るい稜線と同じ明るさになって辺が消えた
    // ボディの稜線はボディの色から作るので、どの色のボディでも面と差が付く
    for (inks, dark) in [(crate::Inks::LIGHT, false), (crate::Inks::DARK, true)] {
        let colors = Colors::from_look(Palette {
            dark,
            canvas: if dark {
                Color32::from_gray(30)
            } else {
                Color32::WHITE
            },
            ink: if dark {
                Color32::from_rgb(0xe2, 0xe8, 0xe6)
            } else {
                Color32::from_rgb(0x1f, 0x27, 0x25)
            },
            ink2: if dark {
                Color32::from_rgb(0xa3, 0xb0, 0xad)
            } else {
                Color32::from_rgb(0x66, 0x72, 0x6f)
            },
            ink3: Color32::GRAY,
            err: Color32::RED,
            warn: Color32::YELLOW,
            design: Color32::BLUE,
            cam: Color32::GREEN,
            shade3: Color32::GRAY,
            accent: Color32::BLUE,
            inks,
        });
        for i in 0..8 {
            let body = body_tint(colors.body, colors.edge, i);
            let (b, e) = (luma(body), luma(line_color(body, dark)));
            assert!((b - e).abs() > 25.0, "{dark} {i}: ボディ {b} 稜線 {e}");
            assert_eq!(e > b, dark, "{dark} {i}: ボディ {b} 稜線 {e}");
        }
    }
}

/// 暗い画面の明るい外観(白・アルミ・黄)と、明るい画面の黒い外観でも、稜線がボディと見分けられる。
/// (暗い画面で必ず白い側へ寄せると、白いボディで線の明るさがボディとほぼ同じになる)
#[test]
fn body_lines_stand_out_from_bright_and_dark_appearances() {
    let bright = [
        Color32::from_rgb(240, 240, 236),
        Color32::from_rgb(196, 200, 206),
        Color32::from_rgb(230, 200, 80),
    ];
    for body in bright {
        let (b, e) = (luma(body), luma(line_color(body, true)));
        assert!(b - e > 60.0, "暗い画面 {body:?}: ボディ {b} 稜線 {e}");
    }
    let black = Color32::from_rgb(52, 52, 56);
    let (b, e) = (luma(black), luma(line_color(black, false)));
    assert!(e - b > 60.0, "明るい画面の黒: ボディ {b} 稜線 {e}");
}

#[test]
fn the_section_hatch_draws_thin_diagonal_stripes() {
    // M11: 縞は 45 度の向きで、6 point ごとに 1.4 point の太さ(倍率が変わっても point で同じ)
    for ppp in [1.0, 2.0] {
        let n = (60.0 * ppp) as usize;
        let on: Vec<f32> = (0..n).map(|x| hatch(x as f32 + 0.5, 0.5, ppp)).collect();
        let covered: f32 = on.iter().sum::<f32>() / ppp;
        // 横に 60 point 進むと、縞を 60 / (6 √2) ≒ 7 本横切る(1 本は横に 1.4 √2 point)
        assert!(
            (covered - 7.07 * 1.4 * 2f32.sqrt()).abs() < 2.0,
            "{ppp}: {covered}"
        );
        // 斜めに同じ量だけ動かすと同じ値(縞は x + y の向きに並ぶ)
        assert!((hatch(10.5, 0.5, ppp) - hatch(5.5, 5.5, ppp)).abs() < 1e-5);
    }
}

#[test]
fn a_turn_goes_the_short_way_and_ends_exactly_on_its_goal() {
    // U24: 軸の玉で向きを替える時は 0.3 秒かけて回り、終わりは行き先の値ちょうど
    let mut c = Camera::default();
    c.set_view(170.0, 0.0);
    c.turn_to(-170.0, 30.0);
    assert!(c.turning() && c.is_view(-170.0, 30.0), "行き先で比べる");
    c.step_turn(10.0);
    c.step_turn(10.0 + TURN_SECS / 2.0);
    // 340 度戻らず、180 度をまたいで 20 度だけ回る(半分の時に 180 度のあたり)
    let half = c.yaw.to_degrees().rem_euclid(360.0);
    assert!((half - 180.0).abs() < 1.0, "{half}");
    assert!((c.pitch.to_degrees() - 15.0).abs() < 1.0);
    assert!(!c.step_turn(10.0 + TURN_SECS));
    assert!(!c.turning());
    assert_eq!(
        (c.yaw, c.pitch),
        ((-170f32).to_radians(), 30f32.to_radians())
    );
}

#[test]
fn dragging_or_setting_the_view_stops_a_turn() {
    let mut c = Camera::default();
    c.turn_to(0.0, 0.0);
    c.step_turn(0.0);
    c.step_turn(0.1);
    c.orbit(Vec2::new(5.0, 0.0));
    assert!(!c.turning(), "引くと今の向きから回す");
    c.turn_to(90.0, 0.0);
    c.set_view(-90.0, 89.9);
    assert!(!c.turning() && c.is_view(-90.0, 89.9));
    // 同じ向きへは回さない
    c.turn_to(-90.0, 89.9);
    assert!(!c.turning());
}

#[test]
fn looking_along_a_normal_faces_it_and_keeps_x_right_from_above() {
    let mut c = Camera::default();
    for (n, yaw, pitch) in [
        ([0.0, -1.0, 0.0], -90.0, 0.0),
        ([1.0, 0.0, 0.0], 0.0, 0.0),
        ([0.0, 0.0, 1.0], -90.0, 89.9),
        ([0.0, 0.0, -1.0], -90.0, -89.9),
        ([1.0, 1.0, 0.0], 45.0, 0.0),
    ] {
        c.look_along(n);
        assert!(c.is_view(yaw, pitch), "{n:?}");
        c.step_turn(0.0);
        c.step_turn(1.0);
        let (_, _, eye) = c.basis_pub();
        let n = normalize(n);
        assert!(dot(eye, n) > 0.999, "{n:?} を真正面から: {eye:?}");
    }
}

fn close3(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 2e-3)
}

#[test]
fn free_orbit_turns_over_the_top_and_about_the_screen_axes() {
    // 前から見て(視点は -Y)縦に引き続けると、真上を越えて裏から逆さまに見る
    let mut c = Camera::default();
    c.set_view(-90.0, 0.0);
    let step = std::f32::consts::PI / 0.01 / 100.0;
    for _ in 0..100 {
        c.orbit_free(Vec2::new(0.0, step));
    }
    let (r, u, e) = c.basis_pub();
    assert!(close3(e, [0.0, 1.0, 0.0]), "裏から見る: {e:?}");
    assert!(close3(u, [0.0, 0.0, -1.0]), "逆さま: {u:?}");
    assert!(close3(r, [1.0, 0.0, 0.0]), "右は X のまま: {r:?}");
    // 拘束オービットは真上で止まる(比べるため)
    let mut k = Camera::default();
    k.set_view(-90.0, 0.0);
    for _ in 0..100 {
        k.orbit(Vec2::new(0.0, step));
    }
    assert!(k.basis_pub().2[2] > 0.99, "拘束は真上で止まる");
    // 斜めから横に引くと、画面の上下の軸のまわりに回る(上の向きは変わらない)
    let mut c = Camera::default();
    let (_, u0, e0) = c.basis_pub();
    c.orbit_free(Vec2::new(30.0, 0.0));
    let (_, u1, e1) = c.basis_pub();
    assert!(close3(u0, u1), "{u0:?} {u1:?}");
    assert!((dot(e0, e1) - 0.3f32.cos()).abs() < 1e-3);
    // 拘束オービットは Z のまわりなので、斜めからでは上の向きが変わる
    let mut k = Camera::default();
    k.orbit(Vec2::new(30.0, 0.0));
    assert!(!close3(k.basis_pub().1, u0) || u0[2] > 0.999);
}

#[test]
fn after_a_free_orbit_the_view_buttons_and_the_constrained_orbit_set_z_up_again() {
    let mut c = Camera::default();
    c.orbit_free(Vec2::new(40.0, 25.0));
    c.orbit_free(Vec2::new(-10.0, 60.0));
    assert!(c.roll.abs() > 1e-3, "傾いている");
    assert!(!c.is_view(c.yaw.to_degrees(), c.pitch.to_degrees()));
    c.set_view(-90.0, 0.0);
    assert_eq!(c.roll, 0.0);
    assert!(c.is_view(-90.0, 0.0));
    let (_, u, _) = c.basis_pub();
    assert!(close3(u, [0.0, 0.0, 1.0]));
    // 拘束オービットも上を Z に戻してから回す
    c.orbit_free(Vec2::new(40.0, 25.0));
    c.orbit(Vec2::new(5.0, 0.0));
    assert_eq!(c.roll, 0.0);
    let (r, _, _) = c.basis_pub();
    assert!(r[2].abs() < 1e-5, "右の向きは水平: {r:?}");
    // スケッチの視点の傾き(フリーオービットでない)は、拘束オービットで消さない
    let mut s = Camera {
        roll: 0.5,
        ..Default::default()
    };
    s.orbit(Vec2::new(5.0, 0.0));
    assert_eq!(s.roll, 0.5);
}

#[test]
fn looking_at_the_same_face_again_looks_from_behind_and_then_back() {
    let mut c = Camera::default();
    let n = [0.0, -1.0, 0.0];
    let settle = |c: &mut Camera| {
        c.step_turn(0.0);
        c.step_turn(1.0);
    };
    assert!(!c.look_along_or_back(n));
    settle(&mut c);
    assert!(c.is_view(-90.0, 0.0));
    // もう一度押すと裏から、さらにもう一度で表に戻る
    assert!(c.look_along_or_back(n));
    settle(&mut c);
    assert!(c.is_view(90.0, 0.0));
    assert!(!c.look_along_or_back(n));
    settle(&mut c);
    assert!(c.is_view(-90.0, 0.0));
}

#[test]
fn dragging_back_to_where_the_drag_began_gives_the_starting_view_again() {
    // 10-20 u9: 真上で止まった分を失わず、押した所へ戻すと元の向き(拘束・フリーとも)
    let start = Camera::default();
    let mut c = start;
    c.start_drag();
    for _ in 0..40 {
        c.orbit(Vec2::new(3.0, 10.0));
    }
    assert!(c.pitch >= 1.55 - 1e-6, "真上で止まる: {}", c.pitch);
    for _ in 0..40 {
        c.orbit(Vec2::new(-3.0, -10.0));
    }
    c.end_drag();
    assert!((c.yaw - start.yaw).abs() < 1e-4, "{} {}", c.yaw, start.yaw);
    assert!(
        (c.pitch - start.pitch).abs() < 1e-4,
        "{} {}",
        c.pitch,
        start.pitch
    );
    // ドラッグの外(毎回足す)では、真上で止まった分だけずれる(比べるため)
    let mut k = start;
    for _ in 0..40 {
        k.orbit(Vec2::new(0.0, 10.0));
    }
    for _ in 0..40 {
        k.orbit(Vec2::new(0.0, -10.0));
    }
    assert!((k.pitch - start.pitch).abs() > 0.1, "{}", k.pitch);
    // フリーオービットも、斜めに行って戻すと元の向き
    let mut f = start;
    f.start_drag();
    for d in [(40.0, 25.0), (-10.0, 60.0), (-30.0, -85.0)] {
        f.orbit_free(Vec2::new(d.0, d.1));
    }
    let (r0, u0, e0) = start.basis_pub();
    let (r, u, e) = f.basis_pub();
    assert!(
        close3(r, r0) && close3(u, u0) && close3(e, e0),
        "{r:?} {u:?} {e:?}"
    );
}

#[test]
fn the_grid_stays_under_the_middle_of_the_view_when_zoomed_in_above_the_part() {
    // 10-20 u10: 回す中心が平面の上 50 mm、45 度から寄ると、真下を真ん中にした格子は画面の外へ出ていた
    let colors = Colors::from_look(Palette {
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
    });
    let g = Guides {
        grid: true,
        axes: false,
    };
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    let mut cam = Camera {
        target: [0.0, 0.0, 50.0],
        zoom: 40.0,
        ..Default::default()
    };
    cam.set_view(-90.0, 45.0);
    // 画面の真ん中の視線が平面に当たる所
    let (o, d) = cam.ray(rect.center(), rect);
    let t = -o[2] / d[2];
    let hit = [o[0] + d[0] * t, o[1] + d[1] * t];
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    guide_segments(&cam, rect, &colors, g, &mut |a, b, _| {
        for p in [a, b] {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
    });
    assert!(
        (0..2).all(|i| lo[i] < hit[i] && hit[i] < hi[i]),
        "格子 {lo:?}〜{hi:?} が真ん中 {hit:?} を覆わない"
    );
}

#[test]
fn the_3d_mouse_turns_like_the_orbit_and_zooms_to_the_pivot() {
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    // 拘束オービット: マウスで引くのと同じ向きに回り、視線のまわりは回さず Z が上のまま
    let mut c = Camera::default();
    let mut m = Camera::default();
    m.orbit(Vec2::new(20.0, 10.0));
    c.nav(
        &Nav {
            spin: 0.2,
            tilt: 0.1,
            roll: 0.5,
            ..Nav::default()
        },
        rect,
        false,
    );
    assert!((c.yaw - m.yaw).abs() < 1e-5 && (c.pitch - m.pitch).abs() < 1e-5);
    assert_eq!(c.roll, 0.0);
    assert!(c.basis_pub().1[2] > 0.0, "上は Z の側");
    // フリーオービット: 視線のまわりにも回る(傾く)。視線の向きは変わらない
    let mut f = Camera::default();
    let e0 = f.basis_pub().2;
    f.nav(
        &Nav {
            roll: 0.3,
            ..Nav::default()
        },
        rect,
        true,
    );
    assert!(close3(f.basis_pub().2, e0), "視線のまわりだけ");
    assert!(
        (f.roll - 0.3).abs() < 1e-3 || (f.roll + 0.3).abs() < 1e-3,
        "{}",
        f.roll
    );
    // ズームは回す中心が動かずに寄る。画面移動は回す中心も動く
    let mut z = Camera::default();
    let (t0, k0) = (z.target, z.zoom);
    z.nav(
        &Nav {
            zoom: 2f32.ln(),
            ..Nav::default()
        },
        rect,
        false,
    );
    assert!(close3(z.target, t0) && (z.zoom - 2.0 * k0).abs() < 1e-3);
    z.nav(
        &Nav {
            pan: Vec2::new(10.0, 0.0),
            ..Nav::default()
        },
        rect,
        false,
    );
    assert!(!close3(z.target, t0));
    // ボタン: 全体表示は次に描く時に合わせ直す。視点のボタンは表の向きへ滑らかに回す
    z.fitted = true;
    z.nav(
        &Nav {
            press: Some(NavPress::Fit),
            ..Nav::default()
        },
        rect,
        false,
    );
    assert!(!z.fitted);
    z.nav(
        &Nav {
            press: Some(NavPress::View(3)),
            ..Nav::default()
        },
        rect,
        false,
    );
    assert!(z.is_view(-60.0, 30.0));
    assert!(Nav::default().is_none() && !Nav::default().turns());
}
