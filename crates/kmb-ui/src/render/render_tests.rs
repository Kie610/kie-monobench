//! 絵と動画(23a・23b・23c・24a)の試験: レンダリングの作業空間を開き、人の操作の順で
//! 材質を付け、キャンバス内レンダリングを回し、静止画・回す動画・glTF を書き出す。

use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::{reference_document, reference_part};

fn reg() -> kmb_core::Modules {
    kmb_bundle::modules()
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("kmb-ui-render-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// PNG の幅と高さ(IHDR)。
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert_eq!(&bytes[1..4], b"PNG");
    let be = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
    (be(16), be(20))
}

/// 小さな絵で速く試す。
fn small_scene(ws: &mut Workspace) {
    assert!(ws.change_settings(
        "render.scene",
        &serde_json::json!({
            "width": 160, "height": 100, "passes": 4,
            "video_width": 96, "video_height": 64, "seconds": 1, "fps": 8,
        }),
    ));
}

#[test]
fn the_render_workspace_has_its_own_tab_and_canvas_render_refines_the_view() {
    let mut ws = Workspace::new(reference_document(), None, reg());
    // データフォルダは操作のモジュールの中で共有なので、先に走った試験(with_data_dir)の物を消す
    ws.win.extensions.set_data_dir(None);
    // 人の手順: 作業空間を「レンダリング」へ替える → タブとまとまりが出る
    ws.switch_space(Space::Render);
    let tabs = ws.ribbon_tabs();
    assert_eq!(tabs.len(), 1, "{tabs:?}");
    assert_eq!(tabs[0].name, "レンダリング");
    let groups: Vec<&str> = tabs[0].groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(
        groups,
        [
            "設定",
            "キャンバス内レンダリング",
            "レンダリング",
            "書き出し"
        ]
    );
    let keys: Vec<&str> = tabs[0]
        .groups
        .iter()
        .flat_map(|g| g.items.iter().map(|i| i.key.as_str()))
        .collect();
    for k in [
        "u:scene",
        "u:canvas_render",
        "a:render.still",
        "a:render.turntable",
        "a:render.export_gltf",
    ] {
        assert!(keys.contains(&k), "{k} が無い: {keys:?}");
    }
    // 加工の操作と測る道具は出さない
    assert!(
        !keys
            .iter()
            .any(|k| k.starts_with("a:cam.") || *k == "u:measure")
    );
    // MP4 は何も入れずに書けるので、OpenH264 を入れるボタンは無い
    assert!(!keys.iter().any(|k| k.contains("openh264")), "{keys:?}");
    // 設計に戻すと、加工のタブではなく設計のタブ
    ws.switch_space(Space::Design);
    assert_eq!(ws.ribbon_tabs()[0].name, "ソリッド");
    ws.switch_space(Space::Render);

    // キャンバス内レンダリング: 押すと始まり、止まっている間に回した数が増える
    let mut h = Harness::builder()
        .with_size(egui::vec2(640.0, 480.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.state_mut().run_item("u:canvas_render");
    assert!(h.state().is_canvas_rendering());
    let t = std::time::Instant::now();
    while h.state().canvas_render_passes().unwrap_or(0) < 2 {
        assert!(
            t.elapsed().as_secs() < 120,
            "キャンバス内レンダリングが進まない"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
        h.step();
    }
    // もう一度押すと止める
    h.state_mut().run_item("u:canvas_render");
    assert!(!h.state().is_canvas_rendering());
    // 設計の作業空間では、押していても止まる
    h.state_mut().run_item("u:canvas_render");
    h.state_mut().switch_space(Space::Design);
    h.step();
    assert!(!h.state().is_canvas_rendering());
}

#[test]
fn stills_turntables_and_gltf_are_written_where_chosen() {
    let d = temp_dir("export");
    let mut ws = Workspace::new(reference_document(), None, reg());
    small_scene(&mut ws);
    // 人の手順: ボディに木の外観を付ける → 静止画 → 回す動画(GIF・連番 PNG・MP4) → glTF
    let (_, root) = ws.body_roots()[0];
    assert!(ws.set_body_appearance(root, "ウォルナット"));

    let still = d.join("絵.png");
    ws.run_action_with("render.still", Some(still.clone()));
    let bytes = std::fs::read(&still).unwrap_or_else(|_| panic!("{:?}", ws.message()));
    assert_eq!(png_size(&bytes), (160, 100));
    assert!(
        ws.message()
            .is_some_and(|m| m.contains("静止画を書きました"))
    );

    let gif = d.join("回す.gif");
    ws.run_action_with("render.turntable", Some(gif.clone()));
    let bytes = std::fs::read(&gif).unwrap_or_else(|_| panic!("{:?}", ws.message()));
    assert_eq!(&bytes[..6], b"GIF89a");

    let seq = d.join("コマ.png");
    ws.run_action_with("render.turntable", Some(seq.clone()));
    for i in 1..=8 {
        let f = d.join(format!("コマ-{i:04}.png"));
        let bytes = std::fs::read(&f).unwrap_or_else(|_| panic!("{f:?} {:?}", ws.message()));
        assert_eq!(png_size(&bytes), (96, 64));
    }
    assert!(!d.join("コマ-0009.png").exists(), "1 秒 × 8 枚");

    // MP4 は何も入れずに書ける(Windows は OS の H.264、ほかは AV1)
    let mp4 = d.join("回す.mp4");
    ws.run_action_with("render.turntable", Some(mp4.clone()));
    let bytes = std::fs::read(&mp4).unwrap_or_else(|_| panic!("{:?}", ws.message()));
    assert_eq!(&bytes[4..8], b"ftyp");
    assert!(
        ws.message()
            .is_some_and(|m| m.contains("回す動画を書きました")
                && (m.contains("H.264") || m.contains("AV1"))),
        "{:?}",
        ws.message()
    );

    let glb = d.join("形.glb");
    ws.run_action_with("render.export_gltf", Some(glb.clone()));
    let bytes = std::fs::read(&glb).unwrap_or_else(|_| panic!("{:?}", ws.message()));
    assert_eq!(&bytes[..4], b"glTF");
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    assert_eq!(json["asset"]["version"], "2.0");
    assert!(json["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"].is_array());
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn materials_are_shown_with_their_finish_and_can_be_turned_off() {
    let mut ws = Workspace::new(reference_document(), None, reg());
    let (_, root) = ws.body_roots()[0];
    // 外観の無いボディは、設計では材質で描かない(鋼の灰色を平らな光で描く)
    ws.refresh_body_colors();
    let colors = kmb_ui_view3d::view3d::Colors::from_look(&ws.win.look);
    ws.sync_effects();
    assert!(ws.cur.scene.body_look(0, &colors).is_none());
    // 木の外観を付けると木目のある材質
    assert!(ws.set_body_appearance(root, "メープル"));
    ws.sync_effects();
    let look = ws.cur.scene.body_look(0, &colors).expect("材質");
    assert!(look.finish.grain.is_some());
    // 表示の「効果」の「材質の質感」を切ると、材質で描かない
    ws.win.layout.effects.materials = false;
    ws.sync_effects();
    assert!(ws.cur.scene.body_look(0, &colors).is_none());
    // レンダリングの作業空間では、切っていても全部のボディを材質で描く
    ws.switch_space(Space::Render);
    ws.sync_effects();
    assert!(ws.cur.scene.body_look(0, &colors).is_some());
}

#[test]
fn the_cutting_is_written_as_a_video_from_the_machining_view() {
    let d = temp_dir("sim");
    let mut ws = Workspace::new(kmb_samples::reference_cam_document(), None, reg());
    small_scene(&mut ws);
    // 人の手順: 加工の作業空間で、ツールバーの「確かめる」の「削る様子の動画…」を押す → GIF で保存
    ws.switch_space(Space::Machining);
    let item = ws
        .ribbon_tabs()
        .iter()
        .flat_map(|t| &t.groups)
        .filter(|g| g.name == "確かめる")
        .flat_map(|g| &g.items)
        .find(|i| i.key == format!("a:{}", kmb_cam_commands::EXPORT_SIM_VIDEO))
        .cloned()
        .expect("確かめるに削る様子の動画");
    assert!(item.disabled.is_none(), "{:?}", item.disabled);
    let gif = d.join("削る.gif");
    ws.run_action_with(kmb_cam_commands::EXPORT_SIM_VIDEO, Some(gif.clone()));
    ws.wait_task();
    let bytes = std::fs::read(&gif).unwrap_or_else(|_| panic!("{:?}", ws.message()));
    assert_eq!(&bytes[..6], b"GIF89a");
    assert!(
        ws.message()
            .is_some_and(|m| m.contains("削る様子の動画を書きました")),
        "{:?}",
        ws.message()
    );
    // 連番 PNG なら 1 枚目(削る前)と最後(削った後)が違う
    let seq = d.join("コマ.png");
    ws.run_action_with(kmb_cam_commands::EXPORT_SIM_VIDEO, Some(seq.clone()));
    ws.wait_task();
    let first = std::fs::read(d.join("コマ-0001.png")).unwrap();
    let last = std::fs::read(d.join("コマ-0008.png")).unwrap();
    assert_ne!(first, last, "削って形が変わる");
    std::fs::remove_dir_all(&d).ok();
}

/// 静止画のレンダリング(#12): 描いている間は下の帯に進みが出て、「中止」で何も書かずにやめ、
/// 「ここで仕上げる」でそこまでの回数の絵を書く。
#[test]
fn a_still_render_shows_its_progress_and_can_be_cancelled_or_finished_early() {
    let d = temp_dir("progress");
    let mut ws = Workspace::open(reference_document(), None, true, reg());
    ws.wait();
    // 回す数を多くして、途中で止められるだけの時間をかける
    assert!(ws.change_settings(
        "render.scene",
        &serde_json::json!({"width": 400, "height": 250, "passes": 4096}),
    ));
    let wait_progress = |ws: &mut Workspace| {
        let t = std::time::Instant::now();
        loop {
            if let Some(p) = ws.task_progress()
                && p.fraction > 0.0
            {
                return p;
            }
            assert!(t.elapsed().as_secs() < 120, "進みが出ない");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };

    // 人の手順: 「レンダリング…」で保存先を選ぶ → 進みが出る → 「中止」
    let cancelled = d.join("中止.png");
    ws.run_action_with("render.still", Some(cancelled.clone()));
    assert!(ws.is_running());
    let p = wait_progress(&mut ws);
    assert!(p.note.starts_with("光線追跡 "), "{}", p.note);
    assert!(p.note.ends_with("/4096 回"), "{}", p.note);
    assert_eq!(p.finish.as_deref(), Some("ここで仕上げる"));
    ws.cancel_task();
    assert!(ws.task_stopping());
    ws.wait_task();
    assert!(!ws.is_running());
    assert!(ws.task_progress().is_none());
    assert!(!cancelled.exists(), "中止したのに書いた");
    assert!(
        ws.message().is_some_and(|m| m.contains("中止しました")),
        "{:?}",
        ws.message()
    );

    // もう一度 → 「ここで仕上げる」: そこまでの回数で書き、何回で仕上げたかを知らせる
    let finished = d.join("仕上げ.png");
    ws.run_action_with("render.still", Some(finished.clone()));
    wait_progress(&mut ws);
    ws.finish_task();
    ws.wait_task();
    let bytes = std::fs::read(&finished).unwrap_or_else(|_| panic!("{:?}", ws.message()));
    assert_eq!(png_size(&bytes), (400, 250));
    let m = ws.message().unwrap_or_default().to_string();
    assert!(
        m.contains("静止画を書きました") && m.contains("4096 回のうち"),
        "{m}"
    );
}

/// キャンバス内レンダリング(#12): 止める目安の回数で止まり、目安を上げると続きから回す。
/// 一時停止の間は回した数が増えない。
#[test]
fn canvas_render_stops_at_the_chosen_mark_and_can_pause() {
    let mut ws = Workspace::new(reference_document(), None, reg());
    ws.win.extensions.set_data_dir(None);
    ws.switch_space(Space::Render);
    let mut h = Harness::builder()
        .with_size(egui::vec2(480.0, 360.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    assert_eq!(h.state().canvas_render_target(), 256, "既定は仕上げ");
    // 人の手順: 目安を「下書き」(16 回)にしてから始める
    h.state_mut().set_canvas_render_target(16);
    h.state_mut().run_item("u:canvas_render");
    let wait = |h: &mut Harness<'_, Workspace>, n: u32| {
        let t = std::time::Instant::now();
        while h.state().canvas_render_passes().unwrap_or(0) < n {
            assert!(t.elapsed().as_secs() < 180, "{n} 回まで進まない");
            std::thread::sleep(std::time::Duration::from_millis(30));
            h.step();
        }
    };
    wait(&mut h, 16);
    // 目安に届いたら増えない
    std::thread::sleep(std::time::Duration::from_millis(400));
    h.step();
    assert_eq!(h.state().canvas_render_passes(), Some(16));
    // 目安を上げる → 続きから回す
    h.state_mut().set_canvas_render_target(64);
    wait(&mut h, 17);
    // 一時停止 → 増えない → 続ける → また増える
    h.state_mut().toggle_canvas_render_pause();
    std::thread::sleep(std::time::Duration::from_millis(400));
    h.step();
    let held = h.state().canvas_render_passes().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(400));
    h.step();
    assert_eq!(
        h.state().canvas_render_passes(),
        Some(held),
        "一時停止中に増えた"
    );
    h.state_mut().toggle_canvas_render_pause();
    wait(&mut h, held + 1);
}

/// レンダリング ギャラリー(#17): 保存した文書で描いた静止画は、描いた時の版と一緒に下の帯に並び、
/// 保存の後に変えてから描くと「+変更」の印。押すと大きく見る窓、消すと帯から消える。
#[test]
fn stills_go_to_the_gallery_with_their_version() {
    let d = temp_dir("gallery");
    let mut ws = Workspace::new(reference_document(), None, reg());
    small_scene(&mut ws);
    ws.cur.work.set_path(Some(d.join("台.json")));
    ws.save();
    ws.run_action_with("render.still", Some(d.join("1.png")));
    ws.wait_task();
    let g = ws.gallery();
    assert_eq!(g.len(), 1, "{:?}", ws.message());
    assert_eq!((g[0].version_number, g[0].changed), (Some(1), false));
    assert!(
        ws.message()
            .unwrap_or_default()
            .contains("ギャラリーの絵 1")
    );
    // 変えてから描く
    ws.add_param("w", "10 mm");
    ws.run_action_with("render.still", Some(d.join("2.png")));
    ws.wait_task();
    let g = ws.gallery();
    assert_eq!(g[0].number, 2);
    assert!(g[0].changed);
    assert_eq!(gallery_ui::version_label(&g[0]), "版 1 +変更");

    ws.switch_space(Space::Render);
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("絵 1").click();
    h.run();
    assert_eq!(h.state().viewing_render(), Some(1));
    h.get_by_label("レンダリングの絵 1");
    h.get_by_label("削除").click();
    h.run();
    assert_eq!(h.state().viewing_render(), None);
    assert_eq!(h.state().gallery().len(), 1);
    assert!(h.query_by_label("絵 1").is_none());
    let _ = std::fs::remove_dir_all(&d);
}

/// モジュールの設定は組み立てた物ごとに持ち、別に組み立てた作業場は初めの値から始まる。
/// 全部を初めの値に戻すこともできる(画面の台本の試験が台本ごとに使う)。
#[test]
fn module_settings_can_be_put_back_to_their_first_values() {
    let mut ws = Workspace::new(reference_document(), None, reg());
    assert!(ws.change_settings("render.scene", &serde_json::json!({"aspect": "1:1"})));
    let scene = |ws: &Workspace| {
        ws.win
            .extensions
            .settings()
            .into_iter()
            .find(|g| g.id == "render.scene")
            .unwrap()
    };
    assert_eq!(scene(&ws).value["aspect"], "1:1");
    // 設定は組み立てた物ごとに持つので、別の作業場には残らない
    let again = Workspace::new(reference_document(), None, reg());
    let g = scene(&again);
    assert_eq!(g.value, g.default, "別の組み立ては初めの値から");
    assert!(
        ws.reset_module_settings()
            .contains(&"render.scene".to_string())
    );
    let g = scene(&ws);
    assert_eq!(g.value, g.default);
    assert!(ws.reset_module_settings().is_empty());
}

/// 幅 `w`・高さ `h` の一色 `rgb` の BMP(24 ビット。デカールの絵)。
fn bmp(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let row = (w * 3).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut b = Vec::with_capacity(size as usize);
    b.extend_from_slice(b"BM");
    for v in [size, 0, 54, 40] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&(w as i32).to_le_bytes());
    b.extend_from_slice(&(h as i32).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    for v in [0u32, row * h, 2835, 2835, 0, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for _ in 0..h {
        for _ in 0..w {
            b.extend_from_slice(&[rgb[2], rgb[1], rgb[0]]);
        }
        b.resize(b.len() + (row - w * 3) as usize, 0);
    }
    b
}

/// 静止画を書いて読み直す(RGBA)。
fn still(ws: &mut Workspace, path: &std::path::Path) -> Vec<u8> {
    ws.run_action_with("render.still", Some(path.to_path_buf()));
    let bytes = std::fs::read(path).unwrap_or_else(|_| panic!("{:?}", ws.message()));
    kmb_image::decode_rgba(&bytes, 4096)
        .expect("PNG を読める")
        .2
}

/// 絵の明るさの平均(0〜255)と、赤い画素(デカールの色)の数。
fn brightness_and_red(rgba: &[u8]) -> (f64, usize) {
    let px: Vec<&[u8]> = rgba.chunks(4).collect();
    let mean = px
        .iter()
        .map(|p| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0)
        .sum::<f64>()
        / px.len() as f64;
    let red = px
        .iter()
        .filter(|p| p[0] as i32 > 2 * p[1] as i32 + 30 && p[0] as i32 > 2 * p[2] as i32 + 30)
        .count();
    (mean, red)
}

/// 光源とデカールの通し: レンダリングの作業空間の「設定」のまとまりの光源・デカールを、人の順で
/// 置いて静止画に出す。
/// - 光源を足すと、再計算した出力の光源(位置と明るさ)ができ、静止画が明るくなる
/// - ボスの上の面に赤い絵を貼ると、出力のデカールができ、静止画に赤が出る
/// - ボスを抑制して面が無くなると、デカールは別の面に付け替えずに誤りになる(光源は計算できる)。
///   抑制を戻すと同じ面に戻って、また絵に出る
#[test]
fn a_light_and_a_decal_are_placed_and_show_in_the_still() {
    let d = temp_dir("light-decal");
    let part = reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.win.extensions.set_data_dir(None);
    // ボスの上の面が絵の中で小さいので、ほかの試験より大きく描く
    assert!(ws.change_settings(
        "render.scene",
        &serde_json::json!({"width": 480, "height": 300, "passes": 2}),
    ));
    ws.switch_space(Space::Render);
    let settings: Vec<String> = ws.ribbon_tabs()[0]
        .groups
        .iter()
        .filter(|g| g.name == "設定")
        .flat_map(|g| g.items.iter().map(|i| i.key.clone()))
        .collect();
    for k in ["a:render.light_insert", "a:render.decal_insert"] {
        assert!(settings.iter().any(|s| s == k), "{k} が無い: {settings:?}");
    }
    let (base, base_red) = brightness_and_red(&still(&mut ws, &d.join("前.png")));

    // 1: 「光源」を押す → 点光源が入り、出力の光源はボディの上にあって明るさを持つ
    let n = ws.document().data().features.len();
    ws.run_item("a:render.light_insert");
    ws.commit_creation();
    ws.wait();
    let data = ws.document().data();
    assert_eq!(data.features.len(), n + 1, "{:?}", ws.message());
    let light = data.features.last().unwrap().id;
    let lamp = match ws.report().feature(light) {
        Some(kmb_core::FeatureStatus::Ok { outputs, .. }) => {
            <kmb_look::decor::Lamp as kmb_core::Payload>::from_value(
                &outputs[kmb_look::decor::LAMP],
            )
            .expect("光源の出力")
        }
        s => panic!("光源が計算できない: {s:?}"),
    };
    let (_, h) = ws.visible_bodies()[0];
    let (lo, hi) = ws.cur.work.kernel().bounds(h).unwrap();
    assert!(!lamp.spot && lamp.lumens > 0.0, "{lamp:?}");
    assert!(lamp.position[2] > hi[2], "ボディの上: {lamp:?}");
    let (lit, _) = brightness_and_red(&still(&mut ws, &d.join("光源.png")));
    assert!(lit > base + 1.0, "光源で明るくなる: {base:.1} → {lit:.1}");

    // 2: ボスの上の面(いちばん高い上向きの面)に赤い絵を貼る
    let (tip, h) = ws.visible_bodies()[0];
    let top = ws
        .cur
        .work
        .kernel()
        .faces(h)
        .unwrap()
        .into_iter()
        .filter(|f| f.planar.is_some_and(|p| p.normal[2] > 0.99))
        .max_by(|a, b| {
            let z = |f: &kmb_core::FaceInfo| f.planar.unwrap().point[2];
            z(a).total_cmp(&z(b))
        })
        .expect("上向きの面");
    assert!(top.planar.unwrap().point[2] > lo[2]);
    let png = d.join("赤.bmp");
    std::fs::write(&png, bmp(16, 16, [230, 20, 20])).unwrap();
    ws.run_action_args(
        "render.decal_insert",
        serde_json::json!({
            "path": png.to_string_lossy(),
            "face": kmb_core::Ref::Face { feature: tip, face: top.name.clone() },
        }),
    );
    ws.commit_creation();
    ws.wait();
    let decal = ws.document().data().features.last().unwrap().id;
    assert_ne!(decal, light, "{:?}", ws.message());
    match ws.report().feature(decal) {
        Some(kmb_core::FeatureStatus::Ok { outputs, .. }) => {
            assert!(outputs.contains_key(kmb_look::decor::DECAL));
        }
        s => panic!("デカールが計算できない: {s:?}"),
    }
    let (_, red) = brightness_and_red(&still(&mut ws, &d.join("デカール.png")));
    assert!(red > base_red + 15, "赤が出る: {base_red} → {red}");

    // 3: ボスを抑制すると貼った面が無くなり、デカールは誤り(ほかは計算できる)
    assert!(ws.set_suppressed(part.boss, true));
    ws.wait();
    match ws.report().feature(decal) {
        Some(kmb_core::FeatureStatus::Error { message }) => {
            assert!(message.contains("ありません"), "{message}");
        }
        s => panic!("面が無いのに誤りにならない: {s:?}"),
    }
    assert!(matches!(
        ws.report().feature(light),
        Some(kmb_core::FeatureStatus::Ok { .. })
    ));
    // 誤りがデカールだけなら、その分を除いて静止画を書き、除いたことを知らせる
    let (_, plain) = brightness_and_red(&still(&mut ws, &d.join("抑制.png")));
    assert!(plain <= base_red + 15, "誤りのデカールは描かない: {plain}");
    assert!(
        ws.message().is_some_and(|m| m.contains("デカール 1 件")),
        "{:?}",
        ws.message()
    );
    // 抑制を戻すと、デカールは同じ面に戻って絵に出る
    assert!(ws.set_suppressed(part.boss, false));
    ws.wait();
    assert!(matches!(
        ws.report().feature(decal),
        Some(kmb_core::FeatureStatus::Ok { .. })
    ));
    let (_, again) = brightness_and_red(&still(&mut ws, &d.join("戻した.png")));
    assert!(again > base_red + 15, "赤が戻る: {again}");
    std::fs::remove_dir_all(&d).ok();
}
