//! 確かめること: 接する円筒の切り口に、相手の面の色が混じらない。
use super::*;
use crate::view3d::{Palette, Section};
use kmb_core::{FeatureId, Mesh, ShapeHandle};
use std::time::Duration;

/// カーネルを使わず、向きのそろった閉じた円筒か筒を作る。
fn tube(inner: f32, outer: f32, count: usize, phase: f32) -> Mesh {
    let mut mesh = Mesh {
        face_names: ["outer", "inner", "top", "bottom"].map(String::from).into(),
        ..Default::default()
    };
    let at = |radius: f32, i: usize, z: f32| {
        let a = std::f32::consts::TAU * (i as f32 + phase) / count as f32;
        [radius * a.cos(), radius * a.sin(), z]
    };
    let mut tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3], face| {
        let sub = |p: [f32; 3], q: [f32; 3]| std::array::from_fn::<_, 3, _>(|i| p[i] - q[i]);
        let u = sub(b, a);
        let v = sub(c, a);
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = n.iter().map(|v| v * v).sum::<f32>().sqrt();
        let n = n.map(|v| v / length);
        let start = mesh.positions.len() as u32;
        mesh.positions.extend([a, b, c]);
        mesh.normals.extend([n; 3]);
        mesh.triangles.push([start, start + 1, start + 2]);
        mesh.triangle_faces.push(face);
    };
    for i in 0..count {
        let j = (i + 1) % count;
        let [a, b, c, d] = [
            at(outer, i, 0.0),
            at(outer, j, 0.0),
            at(outer, j, 30.0),
            at(outer, i, 30.0),
        ];
        tri(a, b, c, 0);
        tri(a, c, d, 0);
        if inner == 0.0 {
            tri([0.0, 0.0, 30.0], d, c, 2);
            tri([0.0, 0.0, 0.0], b, a, 3);
        } else {
            let [e, f, g, h] = [
                at(inner, i, 0.0),
                at(inner, j, 0.0),
                at(inner, j, 30.0),
                at(inner, i, 30.0),
            ];
            tri(e, h, g, 1);
            tri(e, g, f, 1);
            tri(h, d, c, 2);
            tri(h, c, g, 2);
            tri(e, f, b, 3);
            tri(e, b, a, 3);
        }
    }
    mesh
}

fn colors(dark: bool) -> Colors {
    let ink = if dark {
        crate::Inks::DARK
    } else {
        crate::Inks::LIGHT
    };
    let gray = egui::Color32::from_gray;
    Colors::from_look(Palette {
        dark,
        canvas: gray(if dark { 28 } else { 240 }),
        ink: gray(150),
        ink2: gray(120),
        ink3: gray(90),
        err: ink.gouge,
        warn: ink.leftover,
        design: ink.path_entry,
        cam: ink.path_lead,
        shade3: gray(80),
        accent: ink.profile,
        inks: ink,
    })
}

/// 本体の描く道を使い、試験用の読み出せる絵だけに差し替える。
fn picture(gpu: &Gpu, scene: &Scene, cam: &Camera, colors: &Colors) -> Vec<u8> {
    let size = 256;
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("section-contact-test"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let id = egui::Id::new("section-contact-test");
    let mut target = gpu.target([size, size], SAMPLES, false, None);
    target.resolve = tex.create_view(&Default::default());
    gpu.state.lock().unwrap().targets.insert(id, target);
    gpu.render(
        id,
        1,
        scene,
        cam,
        [size as usize; 2],
        1.0,
        &[],
        None,
        colors,
        Shading::Shaded,
        Guides::default(),
        &[],
        &Behind::default(),
    );
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("section-contact-readback"),
        size: u64::from(size * size * 4),
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
                bytes_per_row: Some(size * 4),
                rows_per_image: Some(size),
            },
        },
        wgpu::Extent3d {
            width: size,
            height: size,
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
            timeout: Some(Duration::from_secs(10)),
        })
        .unwrap();
    rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    let pixels = buffer.slice(..).get_mapped_range().unwrap().to_vec();
    buffer.unmap();
    pixels
}

#[test]
#[ignore = "道具: GPU の実描画で、接する円筒の切り口を確かめる"]
fn touching_cylinders_keep_their_own_cap_colors_on_the_gpu() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let rs = pollster_block(egui_wgpu::RenderState::create(
        &Default::default(),
        &instance,
        None,
        Default::default(),
    ))
    .expect("GPU の描画装置が要ります");
    let gpu = Gpu::new(&rs).expect("GPU の描く管を作る");
    let settings = crate::render::RenderSettings {
        ao: false,
        ..Default::default()
    };
    crate::render::set_current(&settings);
    let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(256.0, 256.0));
    let mut checked = 0;
    for reverse in [false, true] {
        let mut bodies = vec![
            (FeatureId(1), tube(10.0, 20.0, 96, 0.0)),
            (FeatureId(2), tube(0.0, 10.0, 80, 0.3)),
        ];
        if reverse {
            bodies.reverse();
        }
        let keys: Vec<_> = bodies
            .iter()
            .map(|(id, _)| (*id, ShapeHandle(id.0)))
            .collect();
        let mut scene = Scene::default();
        scene.set(&keys, bodies);
        for dark in [false, true] {
            let colors = colors(dark);
            for perspective in [false, true] {
                let mut cam = Camera::default();
                cam.yaw = -90f32.to_radians();
                cam.pitch = 20f32.to_radians();
                cam.perspective = perspective;
                cam.fit_box([-20.0, -20.0, 0.0], [20.0, 20.0, 30.0], rect);
                for y in [-2.3, 0.0, 2.3] {
                    scene.section = Some(Section {
                        normal: [0.0, -1.0, 0.0],
                        offset: -y,
                        by_body: true,
                        outside: false,
                    });
                    let pixels = picture(&gpu, &scene, &cam, &colors);
                    for x in [
                        -17.0_f32, -14.0, -11.5, -8.5, -5.0, 0.0, 5.0, 8.5, 11.5, 14.0, 17.0,
                    ] {
                        for z in [6.0, 12.0, 18.0, 24.0] {
                            let id = FeatureId(if x * x + y * y < 100.0 { 2 } else { 1 });
                            let index = scene.bodies.iter().position(|b| b.feature == id).unwrap();
                            let expected = scene.cap_color(index, &colors).to_array();
                            let p = cam.project([x, y, z], rect).0;
                            let start = (p.y.floor() as usize * 256 + p.x.floor() as usize) * 4;
                            let actual = &pixels[start..start + 3];
                            let ratio = (0..3)
                                .map(|i| f32::from(actual[i]) * f32::from(expected[i]))
                                .sum::<f32>()
                                / expected[..3]
                                    .iter()
                                    .map(|v| f32::from(*v).powi(2))
                                    .sum::<f32>();
                            assert!(
                                (0.57..1.03).contains(&ratio)
                                    && (0..3).all(|i| (f32::from(actual[i])
                                        - ratio * f32::from(expected[i]))
                                    .abs()
                                        < 4.0),
                                "相手の面の色が混じる: reverse={reverse}, dark={dark}, perspective={perspective}, at={:?}, actual={actual:?}, expected={expected:?}",
                                [x, y, z]
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 1056);
    println!("GPU: {}、切り口の色 {checked} 点を確認", gpu.adapter);
}
