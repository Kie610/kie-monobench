//! 重ねて描いた輪郭が、領域と切り取りの道具を増やさないことを確かめる。
use crate::tests::{TestKernel, assert_ok, close, rect_sketch, registry};
use crate::{Direction, EXTRUDE, Extrude, Operation};
use kmb_core::kernel::{BooleanOp, FaceInfo, KernelError, MassProps, Mesh, ShapeHandle};
use kmb_core::{Document, FeatureId, FeatureStatus, Kernel, Profile, ProfileFaces, visible_bodies};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// この試験の押し出しに渡る領域数と、切り取りの呼び出しを数える。
struct MeasuredKernel {
    inner: TestKernel,
    extrusions: Mutex<Vec<(usize, Duration)>>,
    booleans: Mutex<Vec<Duration>>,
}

impl MeasuredKernel {
    fn new() -> Self {
        Self {
            inner: TestKernel::new(),
            extrusions: Mutex::new(Vec::new()),
            booleans: Mutex::new(Vec::new()),
        }
    }
}

impl Kernel for MeasuredKernel {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn extrude(
        &self,
        p: &Profile,
        from: f64,
        to: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let start = Instant::now();
        let result = self.inner.extrude(p, from, to, tag);
        self.extrusions
            .lock()
            .unwrap()
            .push((p.regions.len(), start.elapsed()));
        result
    }
    fn boolean(
        &self,
        op: BooleanOp,
        target: ShapeHandle,
        tool: ShapeHandle,
    ) -> Result<ShapeHandle, KernelError> {
        let start = Instant::now();
        let result = self.inner.boolean(op, target, tool);
        self.booleans.lock().unwrap().push(start.elapsed());
        result
    }
    fn faces(&self, h: ShapeHandle) -> Result<Vec<FaceInfo>, KernelError> {
        self.inner.faces(h)
    }
    fn face_names(&self, h: ShapeHandle) -> Result<Vec<String>, KernelError> {
        self.inner.face_names(h)
    }
    fn mass(&self, h: ShapeHandle) -> Result<MassProps, KernelError> {
        self.inner.mass(h)
    }
    fn mesh(&self, h: ShapeHandle, tolerance: f64) -> Result<Mesh, KernelError> {
        self.inner.mesh(h, tolerance)
    }
    fn retain(&self, live: &std::collections::HashSet<ShapeHandle>) {
        self.inner.retain(live);
    }
}

fn circle_grid(duplicate: bool, count: usize) -> (Document, FeatureId) {
    let mut b = DocBuilder::new();
    let outline = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 60.0, 50.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(outline),
            distance: "5 mm".into(),
            ..Default::default()
        },
    );
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let centers: Vec<_> = (0..count)
        .map(|i| [10.0 + (i % 5) as f64 * 10.0, 10.0 + (i / 5) as f64 * 10.0])
        .map(|p| s.fixed_point(p[0], p[1]))
        .collect();
    for p in &centers {
        s.circle(*p, "4 mm", 2.0);
    }
    if duplicate {
        for p in &centers {
            s.circle(*p, "4 mm", 2.0);
        }
    }
    let sketch = b.feature(kmb_sketch::SKETCH, "穴の円", s.build());
    b.feature(
        EXTRUDE,
        "穴",
        Extrude {
            profile: Some(sketch),
            distance: "5 mm".into(),
            direction: Direction::Normal,
            operation: Operation::Cut,
            target: Some(plate),
            ..Default::default()
        },
    );
    (b.finish("円の格子"), sketch)
}

#[test]
fn duplicate_circles_do_not_multiply_the_grid_cut_or_change_its_shape() {
    let kernel = MeasuredKernel::new();
    let reg = registry();
    let mut measured = Vec::new();
    for duplicate in [false, true] {
        kernel.extrusions.lock().unwrap().clear();
        kernel.booleans.lock().unwrap().clear();
        let (mut doc, sketch) = circle_grid(duplicate, 20);
        let before = doc.data().feature(sketch).unwrap().data.clone();
        let start = Instant::now();
        let report = doc.recompute(&reg, &kernel);
        let elapsed = start.elapsed();
        let tool_calls: Vec<_> = kernel
            .extrusions
            .lock()
            .unwrap()
            .iter()
            .map(|(n, _)| *n)
            .collect();
        let boolean_calls = kernel.booleans.lock().unwrap().len();
        eprintln!(
            "二重={duplicate}: 再計算={elapsed:?}, 押し出し(領域数・時間)={:?}, boolean(時間)={:?}",
            kernel.extrusions.lock().unwrap(),
            kernel.booleans.lock().unwrap()
        );
        let after = &doc.data().feature(sketch).unwrap().data;
        assert_eq!(
            before["entities"], after["entities"],
            "描いた要素は消さない"
        );
        assert_eq!(
            before["constraints"], after["constraints"],
            "拘束は変えない"
        );
        let saved = doc.to_json();
        doc.recompute(&reg, &kernel);
        assert_eq!(doc.to_json(), saved, "再計算後の保存は安定する");
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(sketch) else {
            panic!("{report:?}")
        };
        let profile = Profile::from_value(&outputs["profile"]).unwrap();
        let faces = ProfileFaces::from_value(&outputs["faces"]).unwrap();
        let curves = kmb_core::Curves::from_value(&outputs["curves"]).unwrap();
        assert_eq!(
            curves.segments.len(),
            if duplicate { 40 } else { 20 },
            "描画用の線は残す"
        );
        eprintln!(
            "二重={duplicate}: 再計算={elapsed:?}, 領域={}, 選べる領域={}",
            profile.regions.len(),
            faces.faces.len()
        );
        assert_ok(&report);
        let bodies = visible_bodies(doc.data(), &reg, &report);
        assert_eq!(bodies.len(), 1);
        let mass = kernel.mass(bodies[0].1).unwrap();
        let face_info = kernel.faces(bodies[0].1).unwrap();
        let face_count = face_info.len();
        eprintln!("二重={duplicate}: 体積={}, 面数={face_count}", mass.volume);
        let mut names: Vec<_> = face_info.into_iter().map(|f| f.name).collect();
        names.sort();
        measured.push((
            elapsed,
            profile.regions.len(),
            faces.faces.len(),
            mass.volume,
            face_count,
            names,
            tool_calls,
            boolean_calls,
        ));
    }
    let (single, double) = (&measured[0], &measured[1]);
    assert_eq!(double.1, 20, "同じ円板の道具を重ねて作らない");
    assert_eq!(double.2, 20, "重ねた円の領域を二重に選ばせない");
    assert_eq!(single.1, double.1);
    assert!(close(single.3, double.3, 1e-7), "{measured:?}");
    assert_eq!(single.4, double.4, "{measured:?}");
    assert_eq!(single.5, double.5, "押し出した面の名前を保つ");
    assert_eq!(single.6, vec![1, 20], "板と穴を各一つの道具で作る");
    assert_eq!(single.6, double.6, "道具に渡る領域を増やさない");
    assert_eq!(single.7, 1, "切り取りは一回");
    assert_eq!(single.7, double.7, "切り取りの試行を増やさない");
    let expected_holes = 20.0 * std::f64::consts::PI * 4.0 * 5.0;
    // 網から求める体積には曲面の近似誤差を許す。重複の有無の同値判定とは分ける。
    let theory_rel = if kernel.inner.is_truck() { 2e-3 } else { 1e-6 };
    for result in [single, double] {
        let removed = 60.0 * 50.0 * 5.0 - result.3;
        eprintln!(
            "穴の体積={removed}, 理論={expected_holes}, 差={}",
            removed - expected_holes
        );
        assert!(close(removed, expected_holes, theory_rel), "{measured:?}");
    }
    // 短い基準時間の揺れを 2 秒まで許し、長い場合でも 8 倍以内に抑える。
    assert!(
        double.0 <= (single.0 * 8).max(Duration::from_secs(2)),
        "{measured:?}"
    );
}

#[test]
fn named_regions_of_duplicate_circles_extrude_and_cut_the_same_volume() {
    let kernel = MeasuredKernel::new();
    let reg = registry();
    let mut original_keys = None;
    let mut original_shapes: Vec<(f64, Vec<String>)> = Vec::new();
    for duplicate in [false, true] {
        let (mut doc, sketch) = circle_grid(duplicate, 3);
        let report = doc.recompute(&reg, &kernel);
        assert_ok(&report);
        let FeatureStatus::Ok { outputs, .. } = report.feature(sketch).unwrap() else {
            panic!()
        };
        let faces = ProfileFaces::from_value(&outputs["faces"]).unwrap();
        let keys = vec![faces.faces[0].key.clone(), faces.faces[2].key.clone()];
        if let Some(want) = &original_keys {
            assert_eq!(&keys, want);
        } else {
            original_keys = Some(keys.clone());
        }
        let id = doc.data().features.last().unwrap().id;
        let saved = doc.data().feature(id).unwrap().data.clone();
        for (case, operation) in [Operation::New, Operation::Cut].into_iter().enumerate() {
            let mut extrude: Extrude = serde_json::from_value(saved.clone()).unwrap();
            extrude.regions = keys.clone();
            extrude.operation = operation;
            if operation == Operation::New {
                extrude.target = None;
            }
            doc.apply(kmb_core::Transaction::new(
                "領域を選ぶ",
                vec![kmb_core::Op::SetFeatureData {
                    id,
                    data: serde_json::to_value(extrude).unwrap(),
                }],
            ))
            .unwrap();
            let report = doc.recompute(&reg, &kernel);
            assert_ok(&report);
            let FeatureStatus::Ok { outputs, .. } = report.feature(id).unwrap() else {
                panic!()
            };
            let kmb_core::Value::Shape(body) = outputs["body"] else {
                panic!()
            };
            let v = 2.0 * std::f64::consts::PI * 4.0 * 5.0;
            let actual = kernel.mass(body).unwrap().volume;
            let volume = if operation == Operation::Cut {
                60.0 * 50.0 * 5.0 - actual
            } else {
                actual
            };
            // 小さな円の工具は、既存の円穴の試験と同じ網の近似誤差を許す。
            let theory_rel = if kernel.inner.is_truck() { 1e-2 } else { 1e-6 };
            eprintln!(
                "二重={duplicate}, 操作={operation:?}: 体積={actual}, 工具体積={volume}, 理論={v}, 差={}",
                volume - v
            );
            assert!(close(volume, v, theory_rel), "{volume} vs {v}");
            let mut names: Vec<_> = kernel
                .faces(body)
                .unwrap()
                .into_iter()
                .map(|f| f.name)
                .collect();
            names.sort();
            if duplicate {
                let (want, want_names) = &original_shapes[case];
                assert!(close(actual, *want, 1e-7), "{actual} vs {want}");
                assert_eq!(&names, want_names, "選んだ領域の面数と面名を保つ");
            } else {
                original_shapes.push((actual, names));
            }
        }
    }
}
