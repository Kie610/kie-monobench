//! 確かめること: STEP の読み込み中の中止で、次の読み込みが待ったままにならない。

use super::*;
use kmb_core::FeatureStatus;
use kmb_core::kernel::{
    KernelError, MassProps, Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment,
};
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(10);

/// 最初の読み込みの入口で待ち合わせる。中止の要求を、計算の終了より先に確実に届ける。
struct ImportGate {
    inner: Arc<dyn Kernel>,
    calls: AtomicUsize,
    successful: AtomicUsize,
    entered: SyncSender<()>,
    release: Mutex<Receiver<()>>,
}

impl Kernel for ImportGate {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn import_step(&self, step: &str) -> Result<ShapeHandle, KernelError> {
        if self.calls.fetch_add(1, Ordering::Relaxed) == 0 {
            self.entered.send(()).expect("読み込みの開始を知らせる");
            self.release
                .lock()
                .unwrap()
                .recv_timeout(LIMIT)
                .expect("中止の要求の後で読み込みを進める");
        }
        let result = self.inner.import_step(step);
        if result.is_ok() {
            self.successful.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    fn mass(&self, shape: ShapeHandle) -> Result<MassProps, KernelError> {
        self.inner.mass(shape)
    }
    fn retain(&self, live: &HashSet<ShapeHandle>) {
        self.inner.retain(live);
    }
}

fn box_shape(
    kernel: &dyn Kernel,
    x: f64,
    width: f64,
    depth: f64,
    height: f64,
    tag: &str,
) -> ShapeHandle {
    let points = [[x, 0.0], [x + width, 0.0], [x + width, depth], [x, depth]];
    let outer = ProfileLoop {
        edges: (0..4)
            .map(|i| ProfileEdge {
                name: format!("e{i}"),
                reversed: false,
                segment: Segment::Line {
                    a: points[i],
                    b: points[(i + 1) % 4],
                },
            })
            .collect(),
    };
    kernel
        .extrude(
            &Profile {
                plane: Plane::XY,
                regions: vec![Region {
                    outer,
                    holes: vec![],
                }],
            },
            0.0,
            height,
            tag,
        )
        .unwrap()
}

fn poll_before_timeout(h: &mut Headless) -> Polled {
    let deadline = Instant::now() + LIMIT;
    loop {
        match h.poll_recompute() {
            Polled::Running => {
                assert!(
                    Instant::now() < deadline,
                    "裏の読み込みが時間内に終わらない"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            result => return result,
        }
    }
}

#[test]
fn a_cancelled_step_import_drops_its_result_and_the_next_import_completes() {
    let kernel = kmb_bundle::kernel();
    let shapes: Vec<_> = (0..16)
        .map(|i| {
            box_shape(
                kernel.as_ref(),
                i as f64 * 20.0,
                10.0,
                10.0,
                2.0,
                &format!("box{i}"),
            )
        })
        .collect();
    let large = kernel.export_step(&shapes).unwrap();
    let small = kernel
        .export_step(&[box_shape(kernel.as_ref(), 0.0, 8.0, 6.0, 4.0, "small")])
        .unwrap();
    assert!(
        large.len() > small.len() * 8,
        "最初の STEP は複数の立体を持つ"
    );
    let dir = std::env::temp_dir().join(format!("step-cancel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (first, second) = (dir.join("large.step"), dir.join("small.step"));
    std::fs::write(&first, large).unwrap();
    std::fs::write(&second, small).unwrap();
    let (entered, started) = sync_channel(1);
    let (release, proceed) = sync_channel(1);
    let gate = Arc::new(ImportGate {
        inner: kernel,
        calls: AtomicUsize::new(0),
        successful: AtomicUsize::new(0),
        entered,
        release: Mutex::new(proceed),
    });
    let modules = Modules::assemble(kmb_bundle::all(), gate.clone());
    let mut h = Headless::in_background(modules, Document::new(), None);
    let action = h
        .action_for_role(&kmb_interchange::role::action::IMPORT_STEP)
        .unwrap();
    h.run(&action, None, Some(&first), &serde_json::Value::Null)
        .unwrap();
    let first_id = h.doc().data().features[0].id;
    h.start_recompute(
        Box::new(|_, _| panic!("中止した計算の続きの仕事をしない")),
        None,
    );
    started
        .recv_timeout(LIMIT)
        .expect("最初の STEP の読み込みが始まる");
    h.supersede_recompute();
    assert!(h.is_superseded());
    h.undo().unwrap();
    h.run(&action, None, Some(&second), &serde_json::Value::Null)
        .unwrap();
    let second_id = h.doc().data().features[0].id;
    assert_ne!(first_id, second_id);
    // 中止した計算が残る間は、次の計算を二重に始めない。
    h.start_recompute(
        Box::new(|_, _| panic!("走っている間は次の仕事を始めない")),
        None,
    );
    assert_eq!(gate.calls.load(Ordering::Relaxed), 1);
    release.send(()).unwrap();
    assert!(matches!(poll_before_timeout(&mut h), Polled::Dropped));
    assert!(!h.is_recomputing());
    assert!(h.needs_recompute());
    assert!(
        h.report().feature(first_id).is_none(),
        "中止した結果は画面へ出さない"
    );
    h.start_recompute(Box::new(|_, _| None), None);
    assert!(matches!(poll_before_timeout(&mut h), Polled::Done(_)));
    assert!(!h.is_recomputing());
    assert!(!h.needs_recompute());
    assert_eq!(gate.calls.load(Ordering::Relaxed), 2);
    assert_eq!(
        gate.successful.load(Ordering::Relaxed),
        2,
        "両方の STEP が実際に読める"
    );
    assert!(matches!(
        h.report().feature(second_id),
        Some(FeatureStatus::Ok { .. })
    ));
    let bodies = h.bodies().unwrap();
    assert_eq!(bodies.len(), 1);
    assert!((h.kernel().mass(bodies[0]).unwrap().volume - 192.0).abs() < 1e-5);
    std::fs::remove_dir_all(dir).unwrap();
}
