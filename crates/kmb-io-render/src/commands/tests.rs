use super::*;
use kmb_core::Mesh;

fn box_mesh(min: [f32; 3], max: [f32; 3]) -> Mesh {
    let mut m = Mesh::default();
    let c = |i: usize| {
        [
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
        ]
    };
    let faces: [([usize; 4], [f32; 3]); 6] = [
        ([0, 2, 3, 1], [0.0, 0.0, -1.0]),
        ([4, 5, 7, 6], [0.0, 0.0, 1.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([2, 6, 7, 3], [0.0, 1.0, 0.0]),
        ([0, 4, 6, 2], [-1.0, 0.0, 0.0]),
        ([1, 3, 7, 5], [1.0, 0.0, 0.0]),
    ];
    for (fi, (q, n)) in faces.iter().enumerate() {
        let base = m.positions.len() as u32;
        for &k in q {
            m.positions.push(c(k));
            m.normals.push(*n);
        }
        m.triangles.push([base, base + 1, base + 2]);
        m.triangles.push([base, base + 2, base + 3]);
        m.triangle_faces.extend([fi as u32, fi as u32]);
        m.face_names.push(format!("面{fi}"));
    }
    m
}

fn scene() -> RenderScene {
    RenderScene::new(vec![Object::new(
        box_mesh([0.0; 3], [60.0, 40.0, 18.0]),
        Look::named(Some("ウォルナット"), None),
    )])
}

fn small() -> Scene {
    Scene {
        width: 64,
        height: 40,
        passes: 2,
        video_width: 48,
        video_height: 32,
        seconds: 1.0,
        fps: 6,
        ..Scene::default()
    }
}

#[test]
fn scene_settings_check_their_ranges_and_keep_unwritten_items() {
    let base = Scene::default();
    let s = Scene::from_json(
        &json!({"env": "outdoor", "width": 800, "background": "transparent"}),
        &base,
    )
    .unwrap();
    assert_eq!(s.env, EnvKind::Outdoor);
    assert_eq!(s.width, 800);
    assert!(s.transparent());
    assert_eq!(s.height, base.height, "書いていない項目は元のまま");
    assert_eq!(Scene::from_json(&base.to_json(), &small()).unwrap(), base);
    for bad in [
        json!({"width": 8}),
        json!({"width": 100.5}),
        json!({"passes": 0}),
        json!({"seconds": 120}),
        json!({"fps": 60}),
        json!({"env": "moon"}),
        json!({"background": "red"}),
    ] {
        assert!(Scene::from_json(&bad, &base).is_err(), "{bad}");
    }
}

#[test]
fn the_still_and_the_turntable_use_the_scene_sizes_and_go_round_once() {
    let s = small();
    let img = render_still(&scene(), &Value::Null, &s);
    assert_eq!((img.width, img.height), (64, 40));
    let frames = turntable_frames(&scene(), &Value::Null, &s, false);
    assert_eq!(
        frames.len(),
        6,
        "1 秒 × 6 枚(最後の 1 枚は最初と同じなので描かない)"
    );
    assert!(
        frames
            .iter()
            .all(|f| (f.width, f.height) == (48, 32) && f.is_valid())
    );
    assert_ne!(frames[0], frames[2], "回っている");
    // 透明の背景は、物の無い所が透明
    let clear = turntable_frames(&scene(), &Value::Null, &s, true);
    assert_eq!(clear[0].pixel(0, 0)[3], 0);
    assert_eq!(frames[0].pixel(0, 0)[3], 255);
}

#[test]
fn the_view_of_the_screen_is_read_from_the_arguments() {
    // 上から見る向き
    let (yaw, pitch) =
        view_angles(&json!({"view": {"eye": [0.0, 0.0, 1.0], "up": [0.0, 1.0, 0.0]}}));
    assert!((pitch - 89.0).abs() < 1e-3, "{yaw} {pitch}");
    // 渡さなければ等角
    assert_eq!(view_angles(&Value::Null), (-60.0, 30.0));
    let args = json!({"camera": {
        "eye": [100.0, 0.0, 0.0], "right": [0.0, 1.0, 0.0], "up": [0.0, 0.0, 1.0],
        "toward": [1.0, 0.0, 0.0], "fov": 30.0, "ortho_height": 0.0, "shift": [0.1, -0.2],
    }});
    let cam = camera_arg(&args).unwrap();
    assert!(cam.perspective());
    assert_eq!(cam.shift, [0.1, -0.2]);
    assert_eq!(cam.eye, [100.0, 0.0, 0.0]);
    assert_eq!(view_angles(&args), (0.0, 0.0));
    // 画角も平行投影の高さも無ければ使わない
    let mut bad = args.clone();
    bad["camera"]["fov"] = json!(0.0);
    assert!(camera_arg(&bad).is_none());
}

/// 表示/非表示の段で隠したボディは描かず、外観の段の材質で描き、ビューの段の視点はコマごとに
/// 替わる。ビューの段の視点は画面のカメラと同じ式(注視点から見える高さに合う距離)。
#[test]
fn storyboard_frames_hide_bodies_change_looks_and_follow_the_view_steps() {
    use kmb_interchange::animation::ViewPose;
    let id = kmb_core::FeatureId(1);
    let objects = vec![(
        id,
        box_mesh([0.0; 3], [20.0, 20.0, 20.0]),
        Look::named(Some("アルミ"), None),
        Vec::new(),
    )];
    let view = |yaw| ViewPose {
        target: [10.0, 10.0, 10.0],
        yaw,
        pitch: 30.0,
        height: 80.0,
        perspective: true,
    };
    let at = |yaw| Frame {
        view: Some(view(yaw)),
        ..Default::default()
    };
    let frames = posed_frames(
        &objects,
        &[],
        &[
            at(-60.0),
            at(30.0),
            Frame {
                hidden: [id].into(),
                ..at(30.0)
            },
            Frame {
                looks: [(id, Look::plain([200, 70, 64]))].into(),
                ..at(30.0)
            },
        ],
        &Value::Null,
        &small(),
        false,
    );
    assert_ne!(frames[0].pixels, frames[1].pixels, "視点が替わる");
    assert_ne!(frames[1].pixels, frames[2].pixels, "隠すと消える");
    assert_ne!(frames[1].pixels, frames[3].pixels, "外観が替わる");
    let c = view_camera(&view(0.0));
    let d = (0..3)
        .map(|i| (c.eye[i] - 10.0).powi(2))
        .sum::<f32>()
        .sqrt();
    let want = 40.0 / (kmb_render::camera::FOV_DEG.to_radians() / 2.0).tan();
    assert!((d - want).abs() < 1e-3, "{d} {want}");
    assert!(
        c.eye[0] > 10.0 && c.eye[2] > 10.0,
        "ヨー 0・ピッチ 30 は +X の斜め上から"
    );
    let flat = view_camera(&ViewPose {
        perspective: false,
        ..view(0.0)
    });
    assert_eq!(flat.ortho_height, 80.0);
}

/// 段の帯のコマ: 初めと終わりを含み、1 秒の枚数どおり。動かす物は全部のコマで画面に入る。
#[test]
fn the_storyboard_frames_cover_the_start_and_the_end_and_keep_moved_parts_in_view() {
    let t = animation_times(3.0, 8);
    assert_eq!(t.len(), 25);
    assert_eq!((t[0], t[24]), (0.0, 3.0));
    assert_eq!(animation_times(0.0, 30).len(), 2);
    let id = kmb_core::FeatureId(1);
    let objects = vec![(
        id,
        box_mesh([0.0; 3], [20.0, 20.0, 20.0]),
        Look::named(Some("アルミ"), None),
        Vec::new(),
    )];
    // 2 枚目は 200 mm 上へ動かす。視点を渡さなければ、両方を包む箱に合わせる
    let up =
        std::collections::BTreeMap::from([(id, kmb_core::kernel::translation([0.0, 0.0, 200.0]))]);
    let frames = posed_frames(
        &objects,
        &[],
        &[
            Frame::default(),
            Frame {
                poses: up,
                ..Default::default()
            },
        ],
        &Value::Null,
        &small(),
        false,
    );
    assert_eq!(frames.len(), 2);
    assert_ne!(frames[0].pixels, frames[1].pixels, "動いた絵");
    // 1 枚目は下の半分に、2 枚目は上の半分に箱がある(視点は 2 枚で同じ)
    let half = |f: &Rgba, top: bool| -> Vec<u8> {
        let n = (f.width * f.height / 2 * 4) as usize;
        if top {
            f.pixels[..n].to_vec()
        } else {
            f.pixels[n..].to_vec()
        }
    };
    assert_ne!(half(&frames[0], true), half(&frames[1], true));
    assert_ne!(half(&frames[0], false), half(&frames[1], false));
}

/// シーンの設定の環境とカメラが静止画に効く。
#[test]
fn the_scene_look_changes_the_still() {
    let base = small();
    let at = |s: &Scene| render_still(&scene(), &Value::Null, s);
    let mean = |img: &kmb_render::Image| {
        img.rgba
            .chunks(4)
            .map(|p| p[0] as f64 + p[1] as f64 + p[2] as f64)
            .sum::<f64>()
            / img.rgba.len() as f64
    };
    let plain = at(&base);
    // 単色の背景: 物と床の無い隅は打った色そのまま(明るさを変えても)
    let mut color = Scene::from_json(
        &json!({"background": "color", "background_color": "#204060", "brightness": 4000}),
        &base,
    )
    .unwrap();
    let with_floor = at(&color);
    assert_eq!(with_floor.pixel(0, 0), [0x20, 0x40, 0x60, 255]);
    // 床を出さないと、床だった所(影を含む)も背景の色になる
    let bg = |img: &kmb_render::Image| {
        img.rgba
            .chunks(4)
            .filter(|p| *p == [0x20, 0x40, 0x60, 255])
            .count()
    };
    color.look.ground.show = false;
    assert!(bg(&at(&color)) > bg(&with_floor) + 100, "床を出さない");
    // 明るさを上げると明るく、露出を上げると暗く
    let bright = Scene::from_json(&json!({"brightness": 3000}), &base).unwrap();
    let dark = Scene::from_json(&json!({"exposure": 11.5}), &base).unwrap();
    assert!(mean(&at(&bright)) > mean(&plain) + 5.0);
    assert!(mean(&at(&dark)) < mean(&plain) - 5.0);
    // 縦横比を決めると高さは幅から
    let square = Scene::from_json(&json!({"aspect": "1:1"}), &base).unwrap();
    let img = at(&square);
    assert_eq!((img.width, img.height), (64, 64));
    // 焦点距離を長くすると(同じ箱に合わせた視点で)遠近が弱まり、絵が変わる
    let tele = Scene::from_json(&json!({"focal_length": 200}), &base).unwrap();
    assert_ne!(at(&tele), plain);
}

#[test]
#[ignore = "道具: JC_SCREENS_OUT にシーンの設定ごとの静止画を書く(目で見る確かめ)"]
fn write_scene_look_stills() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let base = Scene {
        width: 480,
        height: 300,
        passes: 48,
        aspect: "custom".into(),
        ..Scene::default()
    };
    let scene = RenderScene::new(vec![
        Object::new(
            box_mesh([0.0; 3], [60.0, 40.0, 18.0]),
            Look::named(Some("ウォルナット"), None),
        ),
        Object::new(
            box_mesh([70.0, -30.0, 0.0], [90.0, -10.0, 40.0]),
            Look::named(None, Some("アルミニウム")),
        ),
        Object::new(
            box_mesh([-40.0, 50.0, 0.0], [-20.0, 70.0, 25.0]),
            Look::named(Some("ABS(赤)"), None),
        ),
    ]);
    let args = json!({"view": {"eye": [-1.0, -1.6, 0.9], "up": [0.0, 0.0, 1.0]}});
    for (name, v) in [
        ("1-既定", json!({})),
        ("2-明るさ3000lx", json!({"brightness": 3000})),
        ("3-環境を90度回す", json!({"env_rotation": 90})),
        (
            "4-単色の背景",
            json!({"background": "color", "background_color": "#c8c8c8"}),
        ),
        (
            "5-床の映り込み粗さ0.1",
            json!({"ground_reflection": true, "ground_roughness": 0.1}),
        ),
        ("6-床なし", json!({"ground": false})),
        ("7-焦点距離120mm", json!({"focal_length": 120})),
        ("8-露出11EV", json!({"exposure": 11})),
        (
            "9-被写界深度ぼかし3",
            json!({"depth_of_field": true, "blur": 3, "focal_length": 60}),
        ),
    ] {
        let s = Scene::from_json(&v, &base).unwrap();
        let img = render_still(&scene, &args, &s);
        let png = crate::png_seq::png_bytes(&Rgba {
            width: img.width,
            height: img.height,
            pixels: img.rgba,
        });
        std::fs::write(format!("{dir}/{name}.png"), png).unwrap();
    }
}
