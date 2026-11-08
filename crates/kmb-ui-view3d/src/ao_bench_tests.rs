//! 道具: 重い形を回す間のフレームの時間を、入り隅の陰なし・あり・動かす間の軽い陰で測る。
//! `cargo test -p kmb-ui-view3d --lib ao_bench -- --ignored --nocapture`。大きさは `KMB_BENCH_SIZE`
//! (既定 1920x1200)、筒の数は `KMB_BENCH_TUBES`(既定 64)、回す回数は `KMB_BENCH_FRAMES`(既定 30)。
//! `KMB_BENCH_OUT` にフォルダを渡すと、場合ごとの絵(PPM)も書く(陰の見え方を並べるため)。
use super::section_gpu_tests::tube;
use super::*;
use crate::view3d::Palette;
use kmb_core::{FeatureId, ShapeHandle};
use std::time::{Duration, Instant};

fn env_or(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "道具: GPU の実描画で、回す間のフレームの時間を測る"]
fn ao_bench() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let rs = pollster_block(egui_wgpu::RenderState::create(
        &Default::default(),
        &instance,
        None,
        Default::default(),
    ))
    .expect("GPU の描画装置が要ります");
    let info = rs.adapter.get_info();
    let gpu = Gpu::new(&rs).expect("GPU の描く管を作る");
    let (w, h) = std::env::var("KMB_BENCH_SIZE")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once('x')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .unwrap_or((1920usize, 1200usize));
    let tubes = env_or("KMB_BENCH_TUBES", 64);
    let frames = env_or("KMB_BENCH_FRAMES", 30);
    // 筒を格子に並べる(内側の穴と隣の筒の間に入り隅ができる)
    let side = (tubes as f32).sqrt().ceil() as usize;
    let bodies: Vec<_> = (0..tubes)
        .map(|i| {
            let mut m = tube(8.0, 20.0, 1024, 0.0);
            let (x, y) = ((i % side) as f32 * 42.0, (i / side) as f32 * 42.0);
            for p in &mut m.positions {
                p[0] += x;
                p[1] += y;
            }
            (FeatureId(i as u64 + 1), m)
        })
        .collect();
    let keys: Vec<_> = bodies
        .iter()
        .map(|(id, _)| (*id, ShapeHandle(id.0)))
        .collect();
    let triangles: usize = bodies.iter().map(|(_, m)| m.triangles.len()).sum();
    let mut scene = Scene::default();
    scene.set(&keys, bodies);
    let colors = Colors::from_look(Palette {
        dark: true,
        canvas: egui::Color32::from_gray(28),
        ink: egui::Color32::from_gray(150),
        ink2: egui::Color32::from_gray(120),
        ink3: egui::Color32::from_gray(90),
        err: egui::Color32::RED,
        warn: egui::Color32::YELLOW,
        design: egui::Color32::BLUE,
        cam: egui::Color32::GREEN,
        shade3: egui::Color32::from_gray(80),
        accent: egui::Color32::BLUE,
        inks: crate::Inks::DARK,
    });
    let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w as f32, h as f32));
    let mut cam = Camera::default();
    cam.set_view(-60.0, 30.0);
    cam.fit(&scene, rect);
    println!(
        "GPU: {} ({:?}) 大きさ {w}x{h} 三角形 {triangles} 回数 {frames}",
        info.name, info.backend
    );
    for (name, ao, moving) in [
        ("陰なし", false, false),
        ("陰あり(止まっている時と同じ 32 標本)", true, false),
        ("陰あり・動かす間(8 標本)", true, true),
    ] {
        let settings = crate::render::RenderSettings {
            ao,
            ..Default::default()
        };
        crate::render::set_current(&settings);
        let id = egui::Id::new(("ao-bench", name));
        let mut spent = Vec::new();
        for f in 0..frames + 3 {
            cam.orbit(egui::vec2(6.0, 0.0));
            let t0 = Instant::now();
            gpu.render(
                id,
                f as u64,
                &scene,
                &cam,
                [w, h],
                1.0,
                &[],
                None,
                &colors,
                Shading::ShadedEdges,
                Guides {
                    grid: true,
                    axes: true,
                },
                &[],
                &Behind::default(),
                moving,
            );
            gpu.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(30)),
                })
                .unwrap();
            if f >= 3 {
                spent.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
        }
        if let Ok(dir) = std::env::var("KMB_BENCH_OUT") {
            std::fs::create_dir_all(&dir).unwrap();
            let path = std::path::Path::new(&dir).join(format!("{name}.ppm"));
            save(&gpu, &scene, &cam, &colors, [960, 600], ao, moving, &path);
        }
        spent.sort_by(f64::total_cmp);
        let mid = spent[spent.len() / 2];
        let mean = spent.iter().sum::<f64>() / spent.len() as f64;
        println!(
            "{name}: 中央 {mid:.1} ms 平均 {mean:.1} ms 最短 {:.1} 最長 {:.1}",
            spent[0],
            spent[spent.len() - 1]
        );
    }
}

/// 本体の描く道で 1 枚描き、読み出して PPM で書く。
#[allow(clippy::too_many_arguments)]
fn save(
    gpu: &Gpu,
    scene: &Scene,
    cam: &Camera,
    colors: &Colors,
    [w, h]: [u32; 2],
    ao: bool,
    moving: bool,
    path: &std::path::Path,
) {
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ao-bench-shot"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let id = egui::Id::new(("ao-bench-shot", path));
    let mut target = gpu.target([w, h], SAMPLES, ao, None);
    target.resolve = tex.create_view(&Default::default());
    gpu.state.lock().unwrap().targets.insert(id, target);
    gpu.render(
        id,
        1,
        scene,
        cam,
        [w as usize, h as usize],
        1.0,
        &[],
        None,
        colors,
        Shading::ShadedEdges,
        Guides {
            grid: true,
            axes: true,
        },
        &[],
        &Behind::default(),
        moving,
    );
    let row = (w * 4).div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ao-bench-readback"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    let submitted = gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).unwrap();
    });
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submitted),
            timeout: Some(Duration::from_secs(30)),
        })
        .unwrap();
    rx.recv_timeout(Duration::from_secs(30)).unwrap().unwrap();
    let data = buffer.slice(..).get_mapped_range().unwrap().to_vec();
    let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
    for y in 0..h {
        for x in 0..w {
            let i = (y * row + x * 4) as usize;
            ppm.extend(&data[i..i + 3]);
        }
    }
    std::fs::write(path, ppm).unwrap();
}
