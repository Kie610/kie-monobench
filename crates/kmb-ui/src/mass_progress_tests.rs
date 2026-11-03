//! 質量の計算と、計測を開く画面の同期処理の時間を手で測る。

use crate::*;
use kmb_core::kernel::{Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
use kmb_measure::Meshes;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn grid_step(side: usize) -> String {
    let modules = crate::tests::reg();
    let mut shapes = Vec::new();
    let mut names = Vec::new();
    // 独立した箱を同じSTEPへ書き、結合の中間形状を測定データの準備で保持しない。
    for y in 0..side {
        for x in 0..side {
            let points = [[0.0, 0.0], [8.0, 0.0], [8.0, 6.0], [0.0, 6.0]];
            let profile = Profile {
                plane: Plane {
                    origin: [12.0 * x as f64, 10.0 * y as f64, 0.0],
                    x_axis: [1.0, 0.0, 0.0],
                    y_axis: [0.0, 1.0, 0.0],
                },
                regions: vec![Region {
                    outer: ProfileLoop {
                        edges: (0..4)
                            .map(|i| ProfileEdge {
                                name: format!("edge{i}"),
                                segment: Segment::Line {
                                    a: points[i],
                                    b: points[(i + 1) % 4],
                                },
                                reversed: false,
                            })
                            .collect(),
                    },
                    holes: vec![],
                }],
            };
            let shape = modules
                .kernel
                .extrude(&profile, 0.0, 4.0, "fixture")
                .unwrap();
            assert!(
                (modules.kernel.mass(shape).unwrap().volume - 192.0).abs() < 1e-6,
                "箱の体積"
            );
            shapes.push(shape);
            names.push(format!("箱{}", shapes.len()));
        }
    }
    assert_eq!(shapes.len(), side * side);
    modules
        .kernel
        .export_step_named(&shapes, &names, "質量の測り")
        .unwrap()
}

fn sphere_step(radius: f64) -> String {
    let modules = crate::tests::reg();
    let profile = Profile {
        plane: Plane {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
        },
        regions: vec![Region {
            outer: ProfileLoop {
                edges: vec![
                    ProfileEdge {
                        name: "arc".into(),
                        segment: Segment::Arc {
                            center: [0.0; 2],
                            radius,
                            start: -std::f64::consts::FRAC_PI_2,
                            end: std::f64::consts::FRAC_PI_2,
                        },
                        reversed: false,
                    },
                    ProfileEdge {
                        name: "axis".into(),
                        segment: Segment::Line {
                            a: [0.0, radius],
                            b: [0.0, -radius],
                        },
                        reversed: false,
                    },
                ],
            },
            holes: vec![],
        }],
    };
    let shape = modules
        .kernel
        .revolve(
            &profile,
            [0.0; 3],
            [0.0, 0.0, 1.0],
            std::f64::consts::TAU,
            "mass_fixture",
        )
        .unwrap();
    let expected = 4.0 * std::f64::consts::PI * radius.powi(3) / 3.0;
    assert!(
        (modules.kernel.mass(shape).unwrap().volume - expected).abs() < 1e-6 * expected,
        "球が閉じていることを体積で確かめる"
    );
    modules
        .kernel
        .export_step_named(&[shape], &["球".into()], "質量の測り")
        .unwrap()
}

fn measure_case(step: &str, label: &str) -> Value {
    let modules = crate::tests::reg();
    let shape = modules.kernel.import_step(step).unwrap();
    let before = Instant::now();
    let cold = modules.kernel.mass(shape).unwrap();
    let cold_ms = before.elapsed().as_secs_f64() * 1000.0;
    let before = Instant::now();
    let warm = modules.kernel.mass(shape).unwrap();
    let warm_ms = before.elapsed().as_secs_f64() * 1000.0;
    assert!(cold.volume > 0.0);
    assert_eq!(cold, warm);
    // 計測は別のカーネルと読み込みから始め、上の質量のキャッシュを使わない。
    let mut b = kmb_samples::sample::DocBuilder::new();
    let id = b.feature(
        kmb_cad_import_step::IMPORT_STEP,
        "測る部品",
        json!({"file":"measure.step","step":step,"upright":true}),
    );
    let mut ws = Workspace::new(b.finish("質量の測り"), None, crate::tests::reg());
    assert_eq!(
        ws.cur.work.report().error_count(),
        0,
        "{:?}",
        ws.cur.work.report().features
    );
    let mesh = ws.cur.scene.mesh(id).expect("開いた部品の網");
    let mut counts = vec![0usize; mesh.face_names.len()];
    for face in &mesh.triangle_faces {
        counts[*face as usize] += 1;
    }
    let face = counts
        .iter()
        .enumerate()
        .max_by_key(|(_, n)| **n)
        .unwrap()
        .0;
    let faces = mesh.face_names.len();
    let triangles = mesh.triangles.len();
    let selected_triangles = counts[face];
    let face_name = mesh.face_names[face].clone();
    ws.cur.picked = Some(view3d::Pick {
        feature: id,
        face: face_name,
    });
    let before = Instant::now();
    ws.toggle_measure();
    let open_ms = before.elapsed().as_secs_f64() * 1000.0;
    assert!(ws.is_measuring());
    assert!(
        ws.cur
            .measure
            .as_ref()
            .unwrap()
            .report
            .rows
            .iter()
            .any(|(k, _)| k == "ボディの体積")
    );
    let before = Instant::now();
    ws.measure_refresh();
    let refresh_ms = before.elapsed().as_secs_f64() * 1000.0;
    let record = json!({"kernel":modules.kernel.name(),"case":label,"profile":if cfg!(debug_assertions){"debug"}else{"release"},"step_bytes":step.len(),"faces":faces,"triangles":triangles,"selected_face_triangles":selected_triangles,"mass_cold_ms":cold_ms,"mass_warm_ms":warm_ms,"measure_open_ms":open_ms,"measure_refresh_ms":refresh_ms,"volume_mm3":cold.volume});
    println!("KMB_MASS_PROGRESS {record}");
    record
}

fn mesh_child(path: &Path, label: &str) {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "mass_progress_tests::mass_progress_measurement",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("JC_KERNEL", "truck")
        .env("KMB_MASS_PROGRESS_CHILD_STEP", path)
        .env("KMB_MASS_PROGRESS_CHILD_LABEL", label)
        .output()
        .unwrap();
    println!("{}", String::from_utf8_lossy(&child.stdout));
    assert!(
        child.status.success(),
        "網としての読み込みと測り: {}",
        String::from_utf8_lossy(&child.stderr)
    );
}

/// cargo test -p kmb-ui --release --lib mass_progress_measurement -- --ignored --nocapture --test-threads=1
#[test]
#[ignore = "大きなSTEPと網の質量と画面を止める時間を、他の検査を止めて手で測る"]
fn mass_progress_measurement() {
    if let Ok(path) = std::env::var("KMB_MASS_PROGRESS_CHILD_STEP") {
        let label = std::env::var("KMB_MASS_PROGRESS_CHILD_LABEL").unwrap();
        let step = std::fs::read_to_string(path).unwrap();
        measure_case(&step, &label);
    } else {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = Scratch(std::env::temp_dir().join(format!(
            "kmb-mass-measurement-{}-{nonce}",
            std::process::id()
        )));
        std::fs::create_dir_all(&dir.0).unwrap();
        for side in [16, 32] {
            let step = grid_step(side);
            measure_case(&step, &format!("step-grid-{}", side * side));
        }
        let radius = 100.0;
        let step = sphere_step(radius);
        let label = format!("mesh-sphere-r{radius}");
        let path = dir.0.join(format!("sphere-{radius}.step"));
        std::fs::write(&path, &step).unwrap();
        mesh_child(&path, &label);
    }
}

/// cargo test -p kmb-ui --release --lib mass_progress_background_measurement -- --ignored --nocapture --test-threads=1
#[test]
#[ignore = "大きなSTEPを測る間の画面の時間を、他の検査を止めて手で測る"]
fn mass_progress_background_measurement() {
    use egui_kittest::{Harness, kittest::Queryable};
    use std::time::Duration;
    let step = grid_step(32);
    let mut b = kmb_samples::sample::DocBuilder::new();
    let id = b.feature(
        kmb_cad_import_step::IMPORT_STEP,
        "1024個の箱",
        json!({"file":"measure.step","step":step,"upright":true}),
    );
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = Scratch(std::env::temp_dir().join(format!(
        "kmb-mass-background-{}-{nonce}",
        std::process::id()
    )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let mut ws = Workspace::open(b.finish("質量の進み"), None, true, crate::tests::reg())
        .with_data_dir(dir.0.clone());
    ws.wait();
    assert_eq!(ws.cur.work.report().error_count(), 0);
    ws.layout_mut().ai_live = true;
    let mesh = ws.cur.scene.mesh(id).unwrap();
    let top = mesh
        .triangles
        .iter()
        .enumerate()
        .find(|(_, t)| {
            let p = t.map(|i| mesh.positions[i as usize].map(f64::from));
            kmb_core::kernel::cross(
                kmb_core::kernel::sub(p[1], p[0]),
                kmb_core::kernel::sub(p[2], p[0]),
            )[2] > 1e-6
        })
        .map(|(i, _)| mesh.face_names[mesh.triangle_faces[i] as usize].clone())
        .expect("上向きの面");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let request = |dir: &Path, tool: &str, args: Value| {
        let (dir, tool) = (dir.to_owned(), tool.to_owned());
        std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args))
    };
    for target in ["頂点", "辺"] {
        let change = request(&dir.0, "ui_click", json!({"target":target}));
        let before = Instant::now();
        while !change.is_finished() {
            h.step();
            std::thread::sleep(Duration::from_millis(2));
            assert!(before.elapsed() < Duration::from_secs(30));
        }
        assert_eq!(change.join().unwrap().unwrap()["ok"], true);
        h.run_steps(4);
    }
    assert_eq!(h.state().win.pick_filter, PickFilter::FACE, "面だけを選ぶ");
    let pick = request(&dir.0, "ui_pick", json!({"face":top}));
    let before = Instant::now();
    while !pick.is_finished() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(before.elapsed() < Duration::from_secs(30));
    }
    assert_eq!(pick.join().unwrap().unwrap()["ok"], true);
    h.run_steps(4);
    assert_eq!(h.state().picked_faces().len(), 1, "測定前に面を選ぶ");
    assert_eq!(h.state().picked_faces()[0].feature, id);
    let data = h.state().document().data().clone();
    let depth = h.state().document().undo_depth();
    let key = request(&dir.0, "ui_key", json!({"keys":"I"}));
    let began = Instant::now();
    let mut frames = Vec::new();
    let mut progress_frames = 0;
    while !key.is_finished()
        || !h
            .state()
            .cur
            .measure
            .as_ref()
            .is_some_and(|m| m.report.rows.iter().any(|(k, _)| k == "ボディの体積"))
    {
        let frame = Instant::now();
        h.step();
        frames.push(frame.elapsed().as_secs_f64() * 1000.0);
        progress_frames += usize::from(h.query_by_label("体積などを計算中…").is_some());
        std::thread::sleep(Duration::from_millis(2));
        if began.elapsed() >= Duration::from_secs(30) {
            let ws = h.state();
            eprintln!(
                "MASS_BACKGROUND_DIAGNOSTIC key_finished={} measuring={} document_job={} busy={} body_job={} generation={} data_equal={} picks={:?} report={:?} scene={:?} message={:?} features={:?} max_frame_ms={:?}",
                key.is_finished(),
                ws.is_measuring(),
                ws.cur.work.is_recomputing(),
                ws.is_busy(),
                ws.cur.measure_job.is_some(),
                ws.cur.scene.generation(),
                ws.document().data() == &data,
                ws.cur.measure.as_ref().map(|m| &m.picks),
                ws.cur.measure.as_ref().map(|m| &m.report),
                ws.cur.scene.key(),
                ws.win.message,
                ws.cur.work.report().features,
                frames.iter().copied().reduce(f64::max)
            );
            panic!("測定の結果の時間切れ");
        }
    }
    assert_eq!(key.join().unwrap().unwrap()["ok"], true);
    assert!(progress_frames > 0, "計算中の表示");
    assert_eq!(h.state().document().data(), &data);
    assert_eq!(h.state().document().undo_depth(), depth);
    let volume = &h
        .state()
        .cur
        .measure
        .as_ref()
        .unwrap()
        .report
        .rows
        .iter()
        .find(|(k, _)| k == "ボディの体積")
        .unwrap()
        .1;
    assert!(volume.starts_with("196608"), "{volume}");
    frames.sort_by(f64::total_cmp);
    let n = frames.len();
    println!(
        "KMB_MASS_BACKGROUND {}",
        json!({"case":"step-grid-1024","profile":if cfg!(debug_assertions){"debug"}else{"release"},"frames":n,"progress_frames":progress_frames,"frame_max_ms":frames[n-1],"frame_p95_ms":frames[(n-1)*95/100],"frame_median_ms":frames[n/2],"until_result_ms":began.elapsed().as_secs_f64()*1000.0,"data_unchanged":true,"undo_unchanged":true})
    );
}
