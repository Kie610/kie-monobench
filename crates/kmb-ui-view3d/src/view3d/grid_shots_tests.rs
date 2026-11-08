//! 道具: 格子の見え方を CPU の描画で撮る(前と後を並べるため)。
//! `KMB_SHOT_OUT=<フォルダ> cargo test -p kmb-ui-view3d --lib grid_shots -- --ignored`。
//! 場面ごとに暗い画面と明るい画面の PPM を書く。

use super::*;
use kmb_core::{FeatureId, Mesh, ShapeHandle};

fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn colors(dark: bool) -> Colors {
    // 画面の既定の色(kmb-ui の theme.rs の LIGHT・DARK)
    let (ink, ink2, ink3, canvas, shade3, design, cam) = if dark {
        (
            0xe2e8e6, 0xa3b0ad, 0x6f7c79, 0x2a3231, 0x66726f, 0x4fc0ae, 0xe0915f,
        )
    } else {
        (
            0x1f2725, 0x66726f, 0x9aa4a2, 0xe6eae9, 0xdfe4e2, 0x17766a, 0xa65a2c,
        )
    };
    Colors::from_look(Palette {
        dark,
        canvas: hex(canvas),
        ink: hex(ink),
        ink2: hex(ink2),
        ink3: hex(ink3),
        err: hex(0xb3352b),
        warn: hex(0x9a6a00),
        design: hex(design),
        cam: hex(cam),
        shade3: hex(shade3),
        accent: hex(design),
        inks: if dark {
            crate::Inks::DARK
        } else {
            crate::Inks::LIGHT
        },
    })
}

/// 箱(`lo`〜`hi`)の網。
fn block(lo: [f32; 3], hi: [f32; 3]) -> Mesh {
    let mut mesh = Mesh::default();
    let c = |i: usize| {
        [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
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
    for (f, ([a, b, cc, d], n)) in faces.into_iter().enumerate() {
        let s = mesh.positions.len() as u32;
        mesh.positions.extend([c(a), c(b), c(cc), c(d)]);
        mesh.normals.extend([n; 4]);
        mesh.triangles.push([s, s + 1, s + 2]);
        mesh.triangles.push([s, s + 2, s + 3]);
        mesh.triangle_faces.extend([f as u32; 2]);
        mesh.face_names.push(format!("f{f}"));
    }
    mesh
}

#[test]
#[ignore = "道具: 格子の見え方を撮る"]
fn grid_shots() {
    let out = std::path::PathBuf::from(std::env::var("KMB_SHOT_OUT").expect("KMB_SHOT_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let mut scene = Scene::default();
    scene.set(
        &[(FeatureId(1), ShapeHandle(1))],
        vec![(FeatureId(1), block([-20.0, -15.0, 0.0], [20.0, 15.0, 12.0]))],
    );
    let (w, h) = (720usize, 450usize);
    // 名前・向き(左右・上下の度)・透視・拡大(1 mm の画素)
    let shots = [
        ("1-上から-替わり目の前", -90.0, 90.0, false, 1.385),
        ("2-上から-替わり目の後", -90.0, 90.0, false, 1.415),
        ("3-斜め-透視", -60.0, 25.0, true, 4.0),
        ("4-低く-透視-6度", -60.0, 6.0, true, 4.0),
        ("5-低く-平行-5度", -60.0, 5.0, false, 4.0),
    ];
    for (name, yaw, pitch, perspective, zoom) in shots {
        for dark in [true, false] {
            let mut cam = Camera {
                perspective,
                ..Default::default()
            };
            cam.set_view(yaw, pitch);
            cam.zoom = zoom;
            let img = render_guided(
                &scene,
                &cam,
                [w, h],
                1.0,
                &[],
                None,
                &colors(dark),
                crate::Shading::ShadedEdges,
                Guides {
                    grid: true,
                    axes: true,
                },
                &[],
                &Behind::default(),
            );
            let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
            for p in &img.pixels {
                ppm.extend([p.r(), p.g(), p.b()]);
            }
            let theme = if dark { "暗い" } else { "明るい" };
            std::fs::write(out.join(format!("{name}-{theme}.ppm")), ppm).unwrap();
        }
    }
}
