//! 設定の「画面」(U38)の試験: テーマ・大きさ・角・詰め具合・動きを、人の操作の順で選んで確かめる。

use super::*;
use crate::layout::{Appearance, Brightness, Corners, Density, Motion};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use jc_samples::reference_document;

fn reg() -> jc_core::Modules {
    jc_bundle::modules()
}

fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws)
}

fn light(mut ws: Workspace) -> Workspace {
    ws.layout_mut().brightness = Brightness::Light;
    ws
}

/// 上の帯の歯車で設定の窓を開く。
fn open_settings(h: &mut Harness<'static, Workspace>) {
    h.query_all_by_label("設定")
        .next()
        .expect("設定のボタン")
        .click();
    h.run();
    assert!(
        h.query_by_label("テーマ").is_some(),
        "設定の窓の先頭に「画面」の節"
    );
}

#[test]
fn corners_density_motion_and_text_size_change_the_standard_look() {
    // 1 つの画面で順に確かめる:
    // - 既定の見た目は標準の色・字・間・角のまま(元 the_default_appearance_keeps_the_standard_look)
    // - 角・詰め具合・動きで部品の形が変わる(元 corners_density_and_motion_change_the_widgets)
    // - 動きを止めると、軸の玉で向きがすぐ変わり終わる(元 motion_off_turns_the_view_at_once)
    // - 字を大きくすると字とリボンのボタンが広がる(元 bigger_text_grows_fonts_and_the_ribbon_buttons)
    let mut h = harness(light(Workspace::new(reference_document(), None, reg())));
    // ツールバーは帯に入りきらないとアイコンだけに畳むので、字を 1.4 倍にしても名前つきのボタンが
    // 並ぶ広さにする(名前つきのボタンが字に合わせて広がるのを見るため)
    h.set_size(egui::vec2(2800.0, 800.0));
    h.run();
    let l = theme::look(&h.ctx);
    assert_eq!(l.pal, theme::LIGHT);
    assert_eq!((l.scale, l.density), (theme::Scale::ONE, 1.0));
    let s = h.ctx.global_style();
    assert_eq!(s.text_styles[&egui::TextStyle::Body].size, 12.5);
    assert_eq!(s.spacing.item_spacing, egui::vec2(6.0, 5.0));
    assert_eq!(
        s.visuals.widgets.inactive.corner_radius,
        egui::CornerRadius::same(4)
    );
    // 試験の枠は動きを止めている(egui_kittest)。その元の長さを上書きしない
    assert_eq!(s.animation_time, 0.0);
    assert_eq!(theme::fs(12.5), 12.5);
    let default_button = h.get_by_label("押し出し").rect();

    // 角・詰め具合・動き: 四角・詰める・止める
    h.state_mut().win.layout.appearance = Appearance {
        corners: Corners::Square,
        density: Density::Compact,
        motion: Motion::Off,
        ..Appearance::default()
    };
    h.run();
    let s = h.ctx.global_style();
    assert_eq!(
        s.visuals.widgets.inactive.corner_radius,
        egui::CornerRadius::ZERO
    );
    assert_eq!(s.visuals.window_corner_radius, egui::CornerRadius::ZERO);
    assert!((s.spacing.item_spacing.x - 6.0 * 0.6).abs() < 1e-4);
    assert_eq!(s.animation_time, 0.0);
    assert_eq!(theme::corner(4.0), 0.0);
    // 動きを止めたまま軸の玉を押すと、次のこまを待たずに向きが変わり終わる
    let before = (h.state().cur.camera.yaw, h.state().cur.camera.pitch);
    h.get_by_label("右から(+X)").click();
    h.step();
    let cam = &h.state().cur.camera;
    assert!(!cam.turning(), "動きを止めると、すぐ行き先に着く");
    assert_ne!((cam.yaw, cam.pitch), before);
    // 丸い・広い
    h.state_mut().win.layout.appearance.corners = Corners::Round;
    h.state_mut().win.layout.appearance.density = Density::Roomy;
    h.run();
    let s = h.ctx.global_style();
    assert_eq!(
        s.visuals.widgets.inactive.corner_radius,
        egui::CornerRadius::same(8)
    );
    assert!((s.spacing.item_spacing.y - 5.0 * 1.5).abs() < 1e-4);

    // 字を大きく: 既定の見た目に戻してから、字とアイコンを大きくする
    h.state_mut().win.layout.appearance = Appearance::default();
    h.run();
    let before = h.get_by_label("押し出し").rect();
    assert_eq!(before, default_button, "既定に戻すと元の大きさ");
    h.state_mut().win.layout.appearance.text_scale = 1.4;
    h.state_mut().win.layout.appearance.icon_scale = 1.2;
    h.run();
    let s = h.ctx.global_style();
    assert!((s.text_styles[&egui::TextStyle::Body].size - 17.5).abs() < 1e-4);
    assert!((s.spacing.interact_size.y - 22.0 * 1.4).abs() < 1e-4);
    assert!((theme::fs(10.0) - 14.0).abs() < 1e-4);
    let after = h.get_by_label("押し出し").rect();
    assert!(
        after.height() > before.height() + 10.0 && after.width() > before.width() + 20.0,
        "ツールバーのボタンが字に合わせて広がる: {before:?} → {after:?}"
    );
    // 範囲の外の値は範囲に収める(手で書いた layout.json)
    h.state_mut().win.layout.appearance.text_scale = 9.0;
    h.run();
    assert_eq!(theme::scale().text, 2.5);
}

#[test]
fn motion_scales_the_apps_own_animation_time_and_comes_back() {
    // アプリと同じ egui の既定(開閉 0.2 秒、巻物の滑らかな移りあり)から
    let ctx = egui::Context::default();
    let natural = ctx.global_style().scroll_animation;
    let mut t = theme::Theme::STANDARD;
    for (m, want) in [
        (Motion::Full, 0.2),
        (Motion::Reduced, 0.1),
        (Motion::Off, 0.0),
        (Motion::Full, 0.2),
    ] {
        t.scale.motion = m.factor();
        theme::apply(&ctx, &t, crate::layout::Space::Design, false);
        let s = ctx.global_style();
        assert!(
            (s.animation_time - want).abs() < 1e-6,
            "{m:?}: {}",
            s.animation_time
        );
        assert_eq!(s.scroll_animation == natural, m != Motion::Off, "{m:?}");
    }
}

#[test]
fn the_bars_and_the_workspace_list_fit_their_text_at_every_size() {
    // 1 つの画面で字の倍率を替えて確かめる:
    // - N5: 字とアイコンを 250 % にしても、上と下の帯・編集パネルの下の欄の字が重ならない
    //   (元 the_largest_text_and_icons_keep_the_bars_and_the_panel_foot_apart)
    // - 作業空間の一覧は箱と同じ幅で、行の字は箱の名前と同じ大きさ(字の倍率 250 % と 100 %)。
    //   押すと作業空間が変わる(元 the_workspace_list_is_as_wide_as_its_box_at_every_size)
    let mut h = Harness::builder()
        .with_size(egui::vec2(1920.0, 1080.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            light(Workspace::new(reference_document(), None, reg())),
        );
    h.state_mut().win.layout.appearance.text_scale = 2.5;
    h.state_mut().win.layout.appearance.icon_scale = 2.5;
    h.run();
    h.run();
    let labels = |h: &Harness<'static, Workspace>, band: egui::Rect| -> Vec<(String, egui::Rect)> {
        h.query_all(egui_kittest::kittest::By::new().role(egui::accesskit::Role::Label))
            .filter(|n| band.intersects(n.rect()))
            .map(|n| {
                use egui_kittest::kittest::NodeT;
                (n.accesskit_node().label().unwrap_or_default(), n.rect())
            })
            .collect()
    };
    // 下の帯: 左の操作の案内は、修飾キーの印より左で終わる(入らない案内は出さない)
    let shift = h.get_by_label("Shift キー").rect();
    let hints: Vec<_> = labels(&h, shift.expand2(egui::vec2(2000.0, 0.0)))
        .into_iter()
        // 下の帯の行の物だけ(編集パネルの中で巻き取られて隠れた欄の名前を拾わない)
        .filter(|(_, r)| r.center().x < shift.left() && r.top() >= shift.top() - 8.0)
        .collect();
    assert!(!hints.is_empty());
    for (t, r) in &hints {
        assert!(
            r.right() <= shift.left(),
            "「{t}」が修飾キーに重なる: {r:?} {shift:?}"
        );
    }
    // 編集パネルの下の欄: 計算の札と「編集」のボタンが重ならない
    let chip = h.get_by_label("計算できた").rect();
    let edit = h.get_by_label_contains("編集Enter").rect();
    assert!(!chip.intersects(edit), "{chip:?} {edit:?}");
    // 上の帯: コマンド検索の欄に名前とキーが入る
    let search = h.get_by_label("コマンド検索").rect();
    let w = |t: &str, f| {
        h.ctx
            .fonts_mut(|x| x.layout_no_wrap(t.into(), f, egui::Color32::WHITE).size().x)
    };
    assert!(
        search.width()
            >= w("コマンドを探す", theme::prop(12.5)) + w("Ctrl+K", theme::mono(10.5)) + 16.0,
        "{search:?}"
    );
    // 作業空間の箱に「作業空間」と名前が入る
    let space = h.get_by_label_contains("作業空間:").rect();
    assert!(
        space.width() >= w("作業空間", theme::prop(10.5)) + 22.0,
        "{space:?}"
    );

    // 作業空間の一覧(250 % のまま、次に 100 % で)
    for k in [2.5, 1.0] {
        h.state_mut().switch_space(Space::Simulation);
        h.state_mut().win.layout.appearance.text_scale = k;
        h.state_mut().win.layout.appearance.icon_scale = k;
        h.run();
        let space = h.get_by_label_contains("作業空間:").rect();
        h.get_by_label_contains("作業空間:").click();
        h.run();
        let design = h.get_by_label("設計").rect();
        let cam = h.get_by_label("加工").rect();
        // 枠と余白の分(左右で 10 まで)を除けば箱と同じ幅か、長い名前が入るようにそれより広い
        assert!(
            design.width() >= space.width() - 10.0,
            "{k}: {design:?} {space:?}"
        );
        assert_eq!(design.width(), cam.width());
        assert!(cam.top() >= design.bottom(), "{design:?} {cam:?}");
        let name = h.ctx.fonts_mut(|f| {
            f.layout_no_wrap("設計".into(), theme::prop(16.0), egui::Color32::WHITE)
                .size()
                .y
        });
        assert!(design.height() >= name + 16.0, "{k}: {design:?}");
        // どの名前も行に収まる(名前の短い作業空間を選んだまま開いても、長い名前がはみ出さない)
        for s in Space::ALL {
            // 「解析」はリボンのタブにもあるので、一覧の行(設計の行と同じ左端)を選ぶ
            let row = h
                .get_all_by_label(s.label())
                .map(|n| n.rect())
                .find(|r| r.left() == design.left())
                .unwrap();
            let name = h.ctx.fonts_mut(|f| {
                f.layout_no_wrap(s.label().into(), theme::prop(16.0), egui::Color32::WHITE)
                    .size()
                    .x
            });
            assert!(row.width() >= name + 14.0, "{k}: {} {row:?}", s.label());
        }
        // 押すと作業空間が変わる
        h.get_by_label("加工").click();
        h.run();
        assert_eq!(h.state().win.layout.space, Space::Machining);
        // 次の倍率のため設計へ戻す
        h.state_mut().switch_space(Space::Design);
        h.run();
    }
}

#[test]
fn themes_and_sizes_are_chosen_in_the_settings_window() {
    // 1 つの設定の窓で順に確かめる:
    // - ハイコントラストを選ぶと画面と 3D の色が変わり、暗いで暗い組になる
    //   (元 picking_high_contrast_in_the_settings_window_recolors_the_screen)
    // - 色覚の型のテーマを選べ、明るさに従い、書き出し・元にもできる
    //   (元 color_vision_themes_are_chosen_in_the_settings_and_follow_the_brightness)
    // - 大きさの欄に打った数は % と読む(元 a_size_typed_into_the_percent_box_is_read_as_percent)
    let mut h = harness(light(Workspace::new(reference_document(), None, reg())));
    h.run();
    open_settings(&mut h);
    // テーマの欄(今は「標準」)を押し、一覧から選ぶ
    h.get_by_value("標準").click();
    h.run();
    h.get_by_label("ハイコントラスト").click();
    h.run();
    assert_eq!(h.state().win.layout.appearance.theme, themes::HIGH_CONTRAST);
    let l = theme::look(&h.ctx);
    assert_eq!(l.pal, theme::HC_LIGHT);
    assert_eq!(
        h.ctx.global_style().visuals.panel_fill,
        theme::HC_LIGHT.chrome
    );
    // 3D のビューとスケッチも同じ色を受け取る
    assert_eq!(jc_ui_view3d::inks(&h.ctx), theme::HC_LIGHT.inks);
    // 暗いにすると、ハイコントラストの暗い組になる
    h.get_by_label("暗い").click();
    h.run();
    h.run();
    assert_eq!(theme::look(&h.ctx).pal, theme::HC_DARK);

    // 色覚の型: 標準・明るいに戻してから、欄の一覧で選ぶ
    h.state_mut().win.layout.appearance.theme = themes::STANDARD.into();
    h.state_mut().layout_mut().brightness = Brightness::Light;
    h.run();
    h.run();
    assert_eq!(theme::look(&h.ctx).pal, theme::LIGHT);
    h.get_by_value("標準").click();
    h.run();
    h.get_by_label("色覚 1・2 型向け(赤と緑)").click();
    h.run();
    assert_eq!(h.state().win.layout.appearance.theme, themes::CVD_RG);
    assert_eq!(theme::look(&h.ctx).pal, theme::CVD_RG_LIGHT);
    // 3D のビューとスケッチにも同じ色が届く
    assert_eq!(jc_ui_view3d::inks(&h.ctx), theme::CVD_RG_LIGHT.inks);
    h.get_by_label("暗い").click();
    h.run();
    h.run();
    assert_eq!(theme::look(&h.ctx).pal, theme::CVD_RG_DARK);
    // 3 型向け。書き出すと、元のテーマの名前が base に入る
    h.state_mut().win.layout.appearance.theme = themes::CVD_TRI.into();
    h.run();
    assert_eq!(theme::look(&h.ctx).pal, theme::CVD_TRI_DARK);
    let text = themes::template("写し", themes::CVD_TRI, &theme::Theme::CVD_TRI);
    let back = themes::parse(&text, "copy").unwrap();
    assert!(back.notes.is_empty(), "{:?}", back.notes);
    assert_eq!(
        (back.light, back.dark),
        (theme::CVD_TRI_LIGHT, theme::CVD_TRI_DARK)
    );
    // 自分のテーマの元にもできる
    let t = themes::parse(r#"{ "base": "cvd_rg" }"#, "rg").unwrap();
    assert_eq!(t.light, theme::CVD_RG_LIGHT);

    // 大きさの欄: 欄は「100 %」と書くので、打った「50」は 50 %(0.5 倍)。
    // (50 倍と読むと上限の 250 % に張り付く)
    // 「UI の大きさ」「字の大きさ」「アイコンの大きさ」の欄のうち、上から 2 つ目を押す
    let click_text_field = |h: &mut Harness<'static, Workspace>| {
        let mut v: Vec<_> = h
            .query_all(egui_kittest::kittest::By::new().value("100 %"))
            .collect();
        v.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
        v[1].click();
        h.run();
    };
    for t in ["50", "50 %"] {
        h.state_mut().win.layout.appearance.text_scale = 1.0;
        h.run();
        click_text_field(&mut h);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text(t.into()));
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(h.state().win.layout.appearance.text_scale, 0.5, "{t}");
    }
}

#[test]
fn a_user_theme_in_the_data_folder_can_be_chosen_and_reset() {
    let dir = std::env::temp_dir().join(format!("jc-appearance-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(themes::DIR)).unwrap();
    std::fs::write(
        dir.join(themes::DIR).join("blue.json"),
        r##"{ "name": "青い設計", "colors": { "design": "#1f5fbf", "nope": "#000000" } }"##,
    )
    .unwrap();
    let ws = light(Workspace::new(reference_document(), None, reg()).with_data_dir(dir.clone()));
    let mut h = harness(ws);
    h.run();
    open_settings(&mut h);
    h.get_by_value("標準").click();
    h.run();
    h.get_by_label("青い設計").click();
    h.run();
    assert_eq!(h.state().win.layout.appearance.theme, "user:blue");
    let blue = egui::Color32::from_rgb(0x1f, 0x5f, 0xbf);
    assert_eq!(theme::look(&h.ctx).pal.design, blue);
    // 無視した色は設定の窓に出る
    assert!(
        h.query_by_label_contains("nope という色はありません")
            .is_some()
    );
    // 書き出すと、色の名前の一覧の入ったファイルができ、選べるテーマが増える
    h.get_by_label("今のテーマを書き出す").click();
    h.run();
    assert_eq!(h.state().win.themes.user.len(), 2);
    // 見た目を既定に戻す(UI の大きさも 1 倍に)
    h.ctx.set_zoom_factor(1.5);
    h.run();
    h.get_by_label("見た目を既定に戻す").scroll_to_me();
    h.run();
    h.get_by_label("見た目を既定に戻す").click();
    h.run();
    h.run();
    assert_eq!(h.state().win.layout.appearance, Appearance::default());
    assert_eq!(h.ctx.zoom_factor(), 1.0);
    assert_eq!(theme::look(&h.ctx).pal, theme::LIGHT);
    // 選んだ見た目は layout.json に書かれ、次の起動で読める
    h.state_mut().win.layout.appearance.text_scale = 1.2;
    h.run();
    let saved = Layout::load(&dir);
    assert_eq!(saved.appearance.text_scale, 1.2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn old_layout_files_read_with_the_default_appearance() {
    let old: Layout = serde_json::from_str(r#"{ "brightness": "dark" }"#).unwrap();
    assert_eq!(old.appearance, Appearance::default());
    let part: Layout =
        serde_json::from_str(r#"{ "appearance": { "text_scale": 1.25, "corners": "square" } }"#)
            .unwrap();
    assert_eq!(part.appearance.text_scale, 1.25);
    assert_eq!(part.appearance.corners, Corners::Square);
    assert_eq!(part.appearance.theme, themes::STANDARD);
    // 配置を既定に戻しても見た目は残す
    let mut l = part.clone();
    l.reset_arrangement();
    assert_eq!(l.appearance, part.appearance);
}

#[test]
fn safe_mode_does_not_read_user_themes() {
    let dir = std::env::temp_dir().join(format!("jc-appearance-safe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(themes::DIR)).unwrap();
    std::fs::write(dir.join(themes::DIR).join("x.json"), "{}").unwrap();
    let ws = Workspace::new(reference_document(), None, reg())
        .with_safe_mode()
        .with_data_dir(dir.clone());
    assert!(ws.win.themes.user.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 色覚の型の見え方を真似る(Machado, Oliveira, Fernandes 2009 の重さ 1.0 の行列。線形の RGB に掛ける)。
const PROTAN: [[f32; 3]; 3] = [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
];
const DEUTAN: [[f32; 3]; 3] = [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.011820, 0.042940, 0.968881],
];
const TRITAN: [[f32; 3]; 3] = [
    [1.255528, -0.076749, -0.178779],
    [-0.078411, 0.930809, 0.147602],
    [0.004733, 0.691367, 0.303900],
];
const NORMAL: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// 色を、`m` で真似た見え方の CIE L*a*b* にする。
fn seen(c: egui::Color32, m: &[[f32; 3]; 3]) -> [f32; 3] {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let rgb = [lin(c.r()), lin(c.g()), lin(c.b())];
    let s: Vec<f32> = (0..3)
        .map(|i| {
            (0..3)
                .map(|j| m[i][j] * rgb[j])
                .sum::<f32>()
                .clamp(0.0, 1.0)
        })
        .collect();
    let x = 0.4124 * s[0] + 0.3576 * s[1] + 0.1805 * s[2];
    let y = 0.2126 * s[0] + 0.7152 * s[1] + 0.0722 * s[2];
    let z = 0.0193 * s[0] + 0.1192 * s[1] + 0.9505 * s[2];
    let f = |t: f32| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.95047), f(y), f(z / 1.08883));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// 見分けたい色の組(名前つき)。
fn pairs(p: &theme::Palette) -> Vec<(String, egui::Color32, egui::Color32)> {
    let k = &p.inks;
    let mut groups: Vec<(&str, Vec<egui::Color32>)> = vec![
        ("状態", vec![p.ok, p.warn, p.err]),
        ("軸", k.axis.to_vec()),
        ("比較", k.deviation.to_vec()),
        (
            "道具の立体",
            vec![k.ghost_join, k.ghost_cut, k.ghost_intersect],
        ),
        ("食い込みと削り残し", vec![k.gouge, k.leftover]),
        (
            "スケッチの要素",
            vec![k.sketch_free, k.sketch_ref, k.sketch_pick],
        ),
        (
            "スケッチの道具",
            vec![k.sketch_draw, k.sketch_modify, k.sketch_constraint],
        ),
        ("グループ", k.group.to_vec()),
        (
            "役の色",
            vec![p.role_make, p.role_modify, p.role_repeat, p.role_define],
        ),
    ];
    let mut out = vec![];
    for (name, cs) in groups.drain(..) {
        for i in 0..cs.len() {
            for j in i + 1..cs.len() {
                out.push((format!("{name} {i}-{j}"), cs[i], cs[j]));
            }
        }
    }
    // 操作の色は順に使うので、隣どうし
    for i in 0..8 {
        out.push((format!("操作 {i}"), k.op[i], k.op[(i + 1) % 8]));
    }
    out
}

/// いちばん見分けにくい組と、その色の差(CIE76)。
fn worst(p: &theme::Palette, ms: &[&[[f32; 3]; 3]]) -> (f32, String) {
    let mut w = (f32::MAX, String::new());
    for (name, a, b) in pairs(p) {
        for m in ms {
            let (x, y) = (seen(a, m), seen(b, m));
            let d = ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt();
            if d < w.0 {
                w = (d, name.clone());
            }
        }
    }
    w
}

/// 色覚の型のテーマ(U27): 真似た見え方でも、見分けたい色の組がどれも 20 以上離れている。
#[test]
fn color_vision_themes_keep_the_colors_apart_for_their_type() {
    const ENOUGH: f32 = 20.0;
    for (name, p) in [
        ("1・2 型 明", theme::CVD_RG_LIGHT),
        ("1・2 型 暗", theme::CVD_RG_DARK),
    ] {
        let (d, pair) = worst(&p, &[&PROTAN, &DEUTAN, &NORMAL]);
        assert!(d >= ENOUGH, "{name}: {pair} が {d:.1}");
    }
    for (name, p) in [
        ("3 型 明", theme::CVD_TRI_LIGHT),
        ("3 型 暗", theme::CVD_TRI_DARK),
    ] {
        let (d, pair) = worst(&p, &[&TRITAN, &NORMAL]);
        assert!(d >= ENOUGH, "{name}: {pair} が {d:.1}");
    }
    // 標準のテーマは、真似た見え方では見分けにくい組がある(このテーマを足した理由)
    assert!(worst(&theme::LIGHT, &[&PROTAN, &DEUTAN]).0 < ENOUGH);
    assert!(worst(&theme::LIGHT, &[&TRITAN]).0 < ENOUGH);
}

/// 相対輝度の比(WCAG)。
fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
    let y = |c: egui::Color32| {
        let lin = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    };
    let (a, b) = (y(a), y(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// 道具のアイコンの土台と役の色は、ツールバーの地の上で 3:1 以上(絵なので字の 4.5:1 は要らない。
/// ハイコントラストは 4.5:1 以上)。役の色は状態の色(済み・注意・失敗)と同じにしない。
#[test]
fn icon_colors_stand_out_on_the_toolbar() {
    for (name, p, need) in [
        ("標準 明", theme::LIGHT, 3.0),
        ("標準 暗", theme::DARK, 3.0),
        ("ハイコントラスト 明", theme::HC_LIGHT, 4.5),
        ("ハイコントラスト 暗", theme::HC_DARK, 4.5),
        ("1・2 型 明", theme::CVD_RG_LIGHT, 3.0),
        ("1・2 型 暗", theme::CVD_RG_DARK, 3.0),
        ("3 型 明", theme::CVD_TRI_LIGHT, 3.0),
        ("3 型 暗", theme::CVD_TRI_DARK, 3.0),
    ] {
        let roles = [p.role_make, p.role_modify, p.role_repeat, p.role_define];
        for (k, c) in std::iter::once(("土台", p.icon_base)).chain(
            ["作る", "変える", "増やす", "決める"]
                .into_iter()
                .zip(roles),
        ) {
            let r = contrast(c, p.chrome);
            assert!(r >= need, "{name} の{k}が地の上で {r:.2}:1");
        }
        for c in roles {
            assert!(
                ![p.ok, p.warn, p.err].contains(&c),
                "{name} の役の色が状態の色と同じ"
            );
        }
    }
}
