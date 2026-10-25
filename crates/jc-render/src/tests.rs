use crate::env::{Env, EnvKind};
use crate::raster::{RasterOptions, render};
use crate::trace::{TraceOptions, Tracer};
use crate::*;
use jc_core::Mesh;

/// 箱の網(面ごとに頂点を分け、法線は面の向き)。
pub(crate) fn box_mesh(min: [f32; 3], max: [f32; 3]) -> Mesh {
    let mut m = Mesh::default();
    let c = |i: usize| {
        [
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
        ]
    };
    // 面: 4 隅の番号(外から見て反時計回り)と法線
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

fn lum(p: [u8; 4]) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

fn plate(look: Look) -> RenderScene {
    RenderScene::new(vec![Object::new(
        box_mesh([0.0, 0.0, 0.0], [100.0, 60.0, 12.0]),
        look,
    )])
}

/// 試験の絵を `JC_RENDER_OUT` のフォルダに書く(目で見て確かめる時だけ)。
fn dump(name: &str, img: &Image) {
    let Ok(dir) = std::env::var("JC_RENDER_OUT") else {
        return;
    };
    let mut ppm = format!("P6 {} {} 255\n", img.width, img.height).into_bytes();
    for px in img.rgba.chunks(4) {
        ppm.extend_from_slice(&px[..3]);
    }
    let _ = std::fs::write(format!("{dir}/{name}.ppm"), ppm);
}

#[test]
fn the_fast_drawing_shows_the_body_its_shadow_and_wood_rings() {
    let scene = plate(Look::named(Some("木材"), None));
    let cam = Camera::fit(
        [0.0, 0.0, 0.0],
        [100.0, 60.0, 12.0],
        -60.0,
        30.0,
        camera::FOV_DEG,
        4.0 / 3.0,
    );
    let env = Env::new(EnvKind::StudioLight, cam.toward_eye());
    let opt = RasterOptions {
        width: 160,
        height: 120,
        ..Default::default()
    };
    let img = render(&scene, &cam, &env, &opt);
    dump("wood", &img);
    assert_eq!((img.width, img.height), (160, 120));
    assert!(img.rgba.chunks(4).all(|p| p[3] == 255), "背景は不透明");
    // 真ん中は板(木の色: 赤みが青みより強い)、隅は背景(灰色)
    let mid = img.pixel(80, 60);
    assert!(
        mid[0] as i32 - mid[2] as i32 > 25,
        "板が木の色でない: {mid:?}"
    );
    let corner = img.pixel(2, 2);
    assert!(
        (corner[0] as i32 - corner[2] as i32).abs() < 12,
        "背景が灰色でない: {corner:?}"
    );
    // 板の上面に木目(明るさのばらつき)がある
    let row: Vec<f32> = (60..100).map(|x| lum(img.pixel(x, 55))).collect();
    let spread =
        row.iter().cloned().fold(f32::MIN, f32::max) - row.iter().cloned().fold(f32::MAX, f32::min);
    assert!(spread > 8.0, "木目が見えない(明るさの幅 {spread})");

    // 同じ入力なら同じ絵
    assert_eq!(render(&scene, &cam, &env, &opt), img);

    // 影なしと比べ、主の光と反対の側(右奥)の床が暗い所がある
    let flat = render(
        &scene,
        &cam,
        &env,
        &RasterOptions {
            shadows: false,
            ..opt.clone()
        },
    );
    let darker = img
        .rgba
        .chunks(4)
        .zip(flat.rgba.chunks(4))
        .filter(|(a, b)| lum([a[0], a[1], a[2], 255]) + 12.0 < lum([b[0], b[1], b[2], 255]))
        .count();
    assert!(darker > 100, "影の画素が {darker}");
}

#[test]
fn a_transparent_background_leaves_only_the_body() {
    let scene = plate(Look::named(Some("アルミ"), None));
    let cam = Camera::fit([0.0; 3], [100.0, 60.0, 12.0], -60.0, 30.0, 0.0, 1.0);
    let env = Env::new(EnvKind::StudioDark, cam.toward_eye());
    let img = render(
        &scene,
        &cam,
        &env,
        &RasterOptions {
            width: 64,
            height: 64,
            background: Background::Transparent,
            ..Default::default()
        },
    );
    assert_eq!(img.pixel(0, 0)[3], 0, "隅は透明");
    assert_eq!(img.pixel(32, 32)[3], 255, "真ん中は板");
    // 縁は半分透ける画素がある(ぎざぎざを抑えた)
    assert!(img.rgba.chunks(4).any(|p| p[3] > 0 && p[3] < 255));
}

#[test]
fn metal_and_paint_look_different() {
    let cam = Camera::fit(
        [0.0; 3],
        [100.0, 60.0, 12.0],
        -60.0,
        30.0,
        camera::FOV_DEG,
        1.0,
    );
    let env = Env::new(EnvKind::StudioLight, cam.toward_eye());
    let opt = RasterOptions {
        width: 48,
        height: 48,
        ..Default::default()
    };
    let al = render(&plate(Look::named(Some("アルミ"), None)), &cam, &env, &opt);
    let white = render(&plate(Look::named(Some("白"), None)), &cam, &env, &opt);
    assert_ne!(al, white);
}

#[test]
fn tracing_converges_and_the_denoiser_smooths_the_noise() {
    let scene = plate(Look::named(Some("樹脂"), None));
    let cam = Camera::fit(
        [0.0; 3],
        [100.0, 60.0, 12.0],
        -60.0,
        30.0,
        camera::FOV_DEG,
        4.0 / 3.0,
    );
    let env = Env::new(EnvKind::StudioLight, cam.toward_eye());
    let opt = TraceOptions {
        width: 64,
        height: 48,
        ..Default::default()
    };
    let mut t = Tracer::new(&scene, cam, env.clone(), opt.clone());
    for _ in 0..4 {
        t.pass();
    }
    assert_eq!(t.passes, 4);
    let raw = t.image(false);
    let smooth = t.image(true);
    dump("trace-raw", &raw);
    dump("trace-denoised", &smooth);
    // 板の上面の真ん中あたりの画素のばらつき(隣どうしの差)が、ならすと小さくなる
    let rough = |img: &Image| -> f32 {
        let mut s = 0.0;
        for y in 20..28 {
            for x in 26..38 {
                s += (lum(img.pixel(x, y)) - lum(img.pixel(x + 1, y))).abs();
            }
        }
        s
    };
    assert!(
        rough(&smooth) < rough(&raw) * 0.8,
        "{} → {}",
        rough(&raw),
        rough(&smooth)
    );
    // 同じ回数なら同じ絵(乱数は画素と回の番号から)
    let mut t2 = Tracer::new(&scene, cam, env.clone(), opt.clone());
    for _ in 0..4 {
        t2.pass();
    }
    assert_eq!(t2.image(false), raw);

    // 速い描き方と、だいたい同じ明るさになる(板の真ん中)
    let fast = render(
        &scene,
        &cam,
        &env,
        &RasterOptions {
            width: 64,
            height: 48,
            ..Default::default()
        },
    );
    let mut t3 = Tracer::new(&scene, cam, env, opt);
    for _ in 0..24 {
        t3.pass();
    }
    let traced = t3.image(true);
    dump("trace-24", &traced);
    let a = lum(fast.pixel(32, 24));
    let b = lum(traced.pixel(32, 24));
    assert!(
        (a - b).abs() < 45.0,
        "速い描き方 {a} と光線追跡 {b} の明るさが離れすぎ"
    );
}

#[test]
fn the_box_tree_finds_the_same_hits_as_testing_every_triangle() {
    let scene = RenderScene::new(vec![
        Object::new(box_mesh([0.0; 3], [10.0, 10.0, 10.0]), Look::default()),
        Object::new(
            box_mesh([20.0, 0.0, 0.0], [25.0, 30.0, 4.0]),
            Look::default(),
        ),
        Object::new(
            box_mesh([-8.0, 12.0, 2.0], [-2.0, 18.0, 9.0]),
            Look::default(),
        ),
    ]);
    let tris = crate::raster::triangles(&scene);
    let bvh = crate::bvh::Bvh::build(&tris);
    let mut rng = crate::math::Rng::new(7);
    for _ in 0..500 {
        let o = [
            rng.f() * 80.0 - 40.0,
            rng.f() * 80.0 - 40.0,
            rng.f() * 40.0 - 10.0,
        ];
        let d = crate::math::normalize([rng.f() - 0.5, rng.f() - 0.5, rng.f() - 0.5]);
        let brute = tris
            .iter()
            .enumerate()
            .filter_map(|(i, t)| crate::raster::hit_tri(o, d, t).map(|h| (h.0, i)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let fast = bvh.hit(&tris, o, d, f32::INFINITY);
        match (brute, fast) {
            (None, None) => {}
            (Some((tb, _)), Some((tf, _, _, _))) => assert!((tb - tf).abs() < 1e-3),
            (b, f) => panic!("{b:?} と {f:?}"),
        }
        assert_eq!(bvh.occluded(&tris, o, d, f32::INFINITY), brute.is_some());
    }
}

#[test]
fn environments_have_names_and_the_key_light_comes_from_the_front_left() {
    for k in EnvKind::ALL {
        assert_eq!(EnvKind::from_name(k.key()), Some(k));
        assert_eq!(EnvKind::from_name(k.label()), Some(k));
    }
    // 前から見る(視点は -Y)と、主の光は左(-X)・手前(-Y)・上から
    let env = Env::new(EnvKind::StudioLight, [0.0, -1.0, 0.0]);
    let k = env.key().dir;
    assert!(k[0] < 0.0 && k[1] < 0.0 && k[2] > 0.3, "{k:?}");
    // 光源の方を向いた面は、背を向けた面より明るい
    let lit = env.diffuse(k, false);
    let away = env.diffuse(k.map(|v| -v), false);
    assert!(lit[1] > away[1] * 1.5);
}

/// 目で見る見本(`JC_RENDER_OUT=<フォルダ> cargo test -p jc-render --release preview -- --ignored`)。
#[test]
#[ignore = "道具: JC_RENDER_OUT に目で見る見本の絵を書く"]
fn preview() {
    let mut objs = vec![
        Object::new(
            box_mesh([0.0, 0.0, 0.0], [120.0, 70.0, 15.0]),
            Look::named(Some("木材"), None),
        ),
        Object::new(
            box_mesh([20.0, 15.0, 15.0], [50.0, 45.0, 45.0]),
            Look::named(Some("アルミ"), None),
        ),
        Object::new(
            box_mesh([70.0, 20.0, 15.0], [100.0, 50.0, 30.0]),
            Look::named(Some("赤"), None),
        ),
    ];
    objs.push(Object::new(
        box_mesh([130.0, 0.0, 0.0], [190.0, 40.0, 20.0]),
        Look::named(Some("ウォルナット"), None),
    ));
    let scene = RenderScene::new(objs);
    let (min, max) = scene.bounds();
    for kind in EnvKind::ALL {
        let cam = Camera::fit(min, max, -60.0, 28.0, camera::FOV_DEG, 4.0 / 3.0);
        let env = Env::new(kind, cam.toward_eye());
        let t0 = std::time::Instant::now();
        let img = render(
            &scene,
            &cam,
            &env,
            &RasterOptions {
                width: 640,
                height: 480,
                ..Default::default()
            },
        );
        eprintln!("{} 速い描き方 {:?}", kind.key(), t0.elapsed());
        dump(&format!("preview-{}-raster", kind.key()), &img);
        let t0 = std::time::Instant::now();
        let img = crate::trace::render(
            &scene,
            &cam,
            &env,
            &TraceOptions {
                width: 640,
                height: 480,
                ..Default::default()
            },
            32,
            true,
        );
        eprintln!("{} 光線追跡 32 回 {:?}", kind.key(), t0.elapsed());
        dump(&format!("preview-{}-trace", kind.key()), &img);
    }
}

#[test]
fn envmap_white_sky_is_one() {
    // 一様に白い景色なら、どの粗さの映り込みも拡散の光も 1
    let m = crate::EnvMap::bake(&|_| [1.0; 3]);
    for d in [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.3, -0.5, -0.8]] {
        let d = crate::math::normalize(d);
        for r in [0.0, 0.2, 0.5, 1.0] {
            let s = m.specular(d, r);
            assert!((s[0] - 1.0).abs() < 1e-3, "粗さ {r}: {s:?}");
        }
        let e = m.diffuse(d);
        assert!((e[0] - 1.0).abs() < 0.02, "拡散 {e:?}");
    }
}

#[test]
fn envmap_blur_spreads_light() {
    // 画面用のスタジオ: 主の光の向きは鋭い段ほど明るく、ぼかすほど暗く広がる
    let m = crate::EnvMap::viewport();
    let (a, e) = crate::envmap::VIEWPORT_KEY;
    let (a, e) = (a.to_radians(), e.to_radians());
    let key = [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()];
    let peak: Vec<f32> = (0..crate::envmap::LEVELS)
        .map(|k| m.levels[k].sample(key)[0])
        .collect();
    assert!(peak.windows(2).all(|w| w[0] >= w[1]), "{peak:?}");
    // 主の光から 30° 離れた向きは、少しぼかすと明るくなる
    let off = crate::math::normalize([key[0], key[1], key[2] + 0.6]);
    assert!(m.specular(off, 0.6)[0] > m.specular(off, 0.0)[0]);
}

#[test]
fn f16_bits_round_trip() {
    use crate::envmap::f16_bits;
    assert_eq!(f16_bits(0.0), 0);
    assert_eq!(f16_bits(1.0), 0x3c00);
    assert_eq!(f16_bits(-2.0), 0xc000);
    assert_eq!(f16_bits(0.5), 0x3800);
    assert_eq!(f16_bits(65504.0), 0x7bff);
    assert_eq!(f16_bits(1e6), 0x7c00);
    // 非正規化数
    assert_eq!(f16_bits(2.0f32.powi(-24)), 1);
}

#[test]
fn a_face_look_splits_the_body_and_shows_on_that_face_only() {
    let mesh = box_mesh([0.0, 0.0, 0.0], [100.0, 60.0, 12.0]);
    let mut faces = vec![None; 6];
    // 上の面(面1)だけ赤
    faces[1] = Some(Look::named(Some("赤"), None));
    let parts = Object::with_faces(mesh.clone(), Look::named(Some("アルミ"), None), &faces);
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].mesh.triangles.len(), 10);
    assert_eq!(parts[1].mesh.triangles.len(), 2);
    assert_eq!(parts[1].mesh.positions.len(), 4);
    assert!(parts[1].mesh.triangle_faces.iter().all(|f| *f == 1));
    // 面の外観が無ければ分けない
    assert_eq!(Object::with_faces(mesh, Look::default(), &[]).len(), 1);
    // 上から見ると赤、ボディ全体を赤にした絵と同じ所が赤い
    let cam = Camera::fit(
        [0.0; 3],
        [100.0, 60.0, 12.0],
        -90.0,
        89.0,
        camera::FOV_DEG,
        1.0,
    );
    let env = Env::new(EnvKind::StudioLight, cam.toward_eye());
    let opt = RasterOptions {
        width: 32,
        height: 32,
        ..Default::default()
    };
    let img = render(&RenderScene::new(parts), &cam, &env, &opt);
    let mid = img.rgba[(16 * 32 + 16) * 4..][..4].to_vec();
    assert!(mid[0] > mid[1] + 40 && mid[0] > mid[2] + 40, "{mid:?}");
}

/// 上から見た板の上面の真ん中の明るさ(光線追跡、32 回)。
fn top_middle(scene: &RenderScene, env: Env) -> [u8; 4] {
    let cam = Camera::fit(
        [0.0, 0.0, 0.0],
        [100.0, 60.0, 12.0],
        0.0,
        89.0,
        camera::FOV_DEG,
        1.0,
    );
    let mut t = Tracer::new(
        scene,
        cam,
        env,
        TraceOptions {
            width: 48,
            height: 48,
            ..Default::default()
        },
    );
    for _ in 0..32 {
        t.pass();
    }
    let img = t.image(true);
    dump("lamp", &img);
    img.pixel(24, 24)
}

#[test]
fn a_spot_lamp_lights_the_plate_and_its_exposure_follows_the_camera() {
    let doc = |lm: f64| jc_look::decor::Lamp {
        spot: true,
        position: [50.0, 30.0, 300.0],
        direction: [0.0, 0.0, -1.0],
        color: [255, 255, 255],
        lumens: lm,
        cone_deg: 30.0,
        blur_deg: 5.0,
        radius: 2.0,
    };
    // 環境の明るさを 1/20 に下げた暗い部屋(環境の明るさはランプに効かない)
    let mut dark = Env::new(EnvKind::StudioDark, [0.0, -1.0, 0.0]);
    dark.exposure = 0.05;
    let plain = plate(Look::plain([200, 200, 200]));
    let base = lum(top_middle(&plain, dark.clone()));
    let lit = plain
        .clone()
        .with_lamps(vec![lamp::Lamp::from_doc(&doc(500.0))]);
    let with = lum(top_middle(&lit, dark.clone()));
    assert!(
        with > base + 40.0,
        "スポットで明るくならない: {base} → {with}"
    );
    // 露出を 2 EV 上げると暗くなる
    let mut low = dark.clone();
    low.lamp_exposure = 0.25;
    let dim = lum(top_middle(&lit, low));
    assert!(dim < with - 20.0, "露出が効かない: {with} → {dim}");
    // 円すいの外(照らす向きを横へ)なら明るくならない
    let mut aside = doc(500.0);
    aside.direction = [1.0, 0.0, 0.0];
    let off = plain.with_lamps(vec![lamp::Lamp::from_doc(&aside)]);
    let side = lum(top_middle(&off, dark));
    assert!(
        (side - base).abs() < 8.0,
        "円すいの外が明るい: {base} → {side}"
    );
}

#[test]
fn a_decal_shows_on_its_face_in_both_drawings() {
    // 上面(面 1)に、真っ赤な 2 × 2 の絵を板いっぱいに貼る
    let img = std::sync::Arc::new(decal::DecalImage {
        key: 9,
        width: 2,
        height: 2,
        rgba: [255, 0, 0, 255].repeat(4),
    });
    let place = jc_look::decor::Decal {
        body: jc_core::FeatureId(1),
        face: "面1".into(),
        image: String::new(),
        key: 9,
        px: [2, 2],
        offset: [0.0, 0.0],
        width: 80.0,
        angle: 0.0,
        opacity: 1.0,
        flip_h: false,
        flip_v: false,
    };
    let mesh = box_mesh([0.0, 0.0, 0.0], [100.0, 60.0, 12.0]);
    let body = Look::plain([200, 200, 200]);
    let decals =
        std::collections::BTreeMap::from([("面1".to_string(), decal::DecalLook::new(img, place))]);
    let faces = decal::with_decals(Vec::new(), &mesh.face_names, &body, Some(&decals));
    assert!(faces[1].as_ref().is_some_and(|l| l.decal.is_some()));
    assert!(faces[0].is_none());
    let scene = RenderScene::new(Object::with_faces(mesh, body, &faces));
    let env = Env::new(EnvKind::StudioLight, [0.0, -1.0, 0.0]);
    let p = top_middle(&scene, env.clone());
    assert!(
        p[0] as i32 > p[2] as i32 + 60,
        "光線追跡に絵が出ない: {p:?}"
    );
    let cam = Camera::fit(
        [0.0, 0.0, 0.0],
        [100.0, 60.0, 12.0],
        0.0,
        89.0,
        camera::FOV_DEG,
        1.0,
    );
    let img = render(
        &scene,
        &cam,
        &env,
        &RasterOptions {
            width: 48,
            height: 48,
            ..Default::default()
        },
    );
    let q = img.pixel(24, 24);
    assert!(
        q[0] as i32 > q[2] as i32 + 60,
        "速い描き方に絵が出ない: {q:?}"
    );
}
