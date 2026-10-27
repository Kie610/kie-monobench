//! 確かめること: 軸と穴のすき間・重なり・円筒の面の一致と、組立を調べ直す計算量。

use super::*;
use crate::joint::{Joint, Origin, moved_outputs, new_joint, transform_of};
use crate::{JOINT, Part};
use kmb_core::kernel::{
    EdgeInfo, FaceInfo, MassProps, Matrix4, Profile, ProfileEdge, ProfileLoop, Region, Segment,
};
use kmb_core::{
    Descriptor, Document, EvalCtx, EvalError, Feature, FeatureEntry, InputRef, Op, Outputs,
    RecomputeReport, Registry, Transaction, Value,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BODY: &str = "test.cylinder_body@1";
const MATRIX: &str = "test.joint_matrix@1";

/// 計測は呼び出しの前後に時計を読み、終了後だけ錠を取る。内側のカーネルの錠を延ばさない。
#[derive(Default)]
struct Timings(Mutex<BTreeMap<&'static str, (usize, Duration)>>);

impl Timings {
    fn record(&self, name: &'static str, elapsed: Duration) {
        let mut all = self.0.lock().unwrap();
        let entry = all.entry(name).or_default();
        entry.0 += 1;
        entry.1 += elapsed;
    }

    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }

    fn report(&self) -> String {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(name, (n, t))| format!("{name} {n}回/{:.3}ms", t.as_secs_f64() * 1000.0))
            .collect::<Vec<_>>()
            .join("・")
    }
}

struct MeasuredKernel {
    inner: Arc<dyn Kernel>,
    timings: Arc<Timings>,
}

impl MeasuredKernel {
    fn timed<T>(&self, name: &'static str, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let result = f();
        self.timings.record(name, start.elapsed());
        result
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
        self.inner.extrude(p, from, to, tag)
    }
    fn boolean(
        &self,
        op: BooleanOp,
        a: ShapeHandle,
        b: ShapeHandle,
    ) -> Result<ShapeHandle, KernelError> {
        self.timed("boolean", || self.inner.boolean(op, a, b))
    }
    fn bounds(&self, h: ShapeHandle) -> Result<(P3, P3), KernelError> {
        self.timed("bounds", || self.inner.bounds(h))
    }
    fn mass(&self, h: ShapeHandle) -> Result<MassProps, KernelError> {
        self.inner.mass(h)
    }
    fn volume(&self, h: ShapeHandle) -> Result<f64, KernelError> {
        self.timed("volume", || self.inner.volume(h))
    }
    fn faces(&self, h: ShapeHandle) -> Result<Vec<FaceInfo>, KernelError> {
        self.timed("faces", || self.inner.faces(h))
    }
    fn face_names(&self, h: ShapeHandle) -> Result<Vec<String>, KernelError> {
        self.timed("face_names", || self.inner.face_names(h))
    }
    fn arc_edges(&self, h: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
        self.timed("arc_edges", || self.inner.arc_edges(h))
    }
    fn mesh(&self, h: ShapeHandle, tolerance: f64) -> Result<Mesh, KernelError> {
        self.timed("mesh", || self.inner.mesh(h, tolerance))
    }
    fn transform(
        &self,
        h: ShapeHandle,
        m: &Matrix4,
        suffix: &str,
    ) -> Result<ShapeHandle, KernelError> {
        self.timed("transform", || self.inner.transform(h, m, suffix))
    }
    fn instance(&self, h: ShapeHandle, m: &Matrix4) -> Result<ShapeHandle, KernelError> {
        self.timed("instance", || self.inner.instance(h, m))
    }
    fn instance_of(&self, h: ShapeHandle) -> Option<(ShapeHandle, Matrix4)> {
        self.inner.instance_of(h)
    }
    fn retain(&self, live: &HashSet<ShapeHandle>) {
        self.timed("retain", || self.inner.retain(live));
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Cylinder {
    outer: f64,
    inner: f64,
    plane: Plane,
    from: f64,
    to: f64,
}

impl Feature for Cylinder {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }

    fn evaluate(&self, cx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let circle = |radius, name: &str| ProfileLoop {
            edges: vec![ProfileEdge {
                name: name.into(),
                segment: Segment::Circle {
                    center: [0.0; 2],
                    radius,
                },
                reversed: false,
            }],
        };
        let p = Profile {
            plane: self.plane,
            regions: vec![Region {
                outer: circle(self.outer, "outer"),
                holes: if self.inner > 0.0 {
                    vec![circle(self.inner, "inner")]
                } else {
                    vec![]
                },
            }],
        };
        let h = cx
            .kernel()
            .extrude(&p, self.from, self.to, &cx.feature_id().to_string())
            .map_err(kmb_cad::kernel_err)?;
        Ok([("body".into(), Value::Shape(h))].into())
    }
}

/// 行列の計算だけを別の文書で測る。製品の評価は呼び直さず、動かした形も作らない。
struct MatrixProbe(Joint);

impl Feature for MatrixProbe {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        self.0.inputs()
    }
    fn evaluate(&self, cx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let start = Instant::now();
        let (m, _, _) = self.0.motion_and_drive(cx)?;
        cx.service::<Timings>()
            .unwrap()
            .record("joint_matrix", start.elapsed());
        let mut out = Outputs::new();
        moved_outputs(cx, &[], &m, &mut out)?;
        Ok(out)
    }
}

struct Bench {
    doc: Document,
    registry: Registry,
    kernel: Arc<dyn Kernel>,
    direct: Arc<dyn Kernel>,
    timings: Arc<Timings>,
    report: RecomputeReport,
}

impl Bench {
    fn new() -> Self {
        let timings = Arc::new(Timings::default());
        let direct = kmb_bundle::kernel();
        let kernel: Arc<dyn Kernel> = Arc::new(MeasuredKernel {
            inner: direct.clone(),
            timings: timings.clone(),
        });
        let mut registry = Registry::new();
        crate::register(&mut registry);
        registry.register_described(
            Descriptor::new(BODY, kmb_core::category::SOLID, "試験の円筒"),
            |v| Ok(Box::new(kmb_cad::parse::<Cylinder>(v)?)),
        );
        registry.register(MATRIX, |v| {
            Ok(Box::new(MatrixProbe(kmb_cad::parse::<Joint>(v)?)))
        });
        registry.provide(timings.clone());
        Self {
            doc: Document::new(),
            registry,
            kernel,
            direct,
            timings,
            report: RecomputeReport::default(),
        }
    }

    fn add(&mut self, kind: &str, data: impl Serialize) -> FeatureId {
        let id = self.doc.alloc_feature_id();
        let entry = FeatureEntry {
            id,
            type_id: kind.into(),
            name: format!("部品 {id}"),
            data: serde_json::to_value(data).unwrap(),
            suppressed: false,
            pin_bodies: false,
        };
        self.doc
            .apply(Transaction::new(
                "試験の組立",
                vec![Op::InsertFeature { index: None, entry }],
            ))
            .unwrap();
        id
    }

    fn recompute(&mut self) {
        self.report = self.doc.recompute(&self.registry, self.kernel.as_ref());
        assert_eq!(self.report.error_count(), 0, "{:?}", self.report);
    }

    fn cx(&self) -> Context<'_> {
        Context::new(self.doc.data(), &self.report, &self.registry, &self.kernel)
    }

    fn fits(shaft: f64, hole: f64, from: f64, to: f64) -> Self {
        let mut b = Self::new();
        b.add(
            BODY,
            Cylinder {
                outer: 6.0,
                inner: hole / 2.0,
                plane: Plane::XY,
                from: 0.0,
                to: 10.0,
            },
        );
        b.add(
            BODY,
            Cylinder {
                outer: shaft / 2.0,
                inner: 0.0,
                plane: Plane::XY,
                from,
                to,
            },
        );
        b.recompute();
        b
    }

    fn change_angle(&mut self, id: FeatureId, value: f64) {
        let mut data = self.doc.data().feature(id).unwrap().data.clone();
        data["angle"] = format!("{value} deg").into();
        self.doc
            .apply(Transaction::new(
                "角度を変える",
                vec![Op::SetFeatureData { id, data }],
            ))
            .unwrap();
    }
}

#[test]
fn a_shaft_with_clearance_is_neither_overlap_nor_coincident() {
    let b = Bench::fits(5.0, 5.05, -2.0, 12.0);
    for coincident in [false, true] {
        let (clashes, failed) = interferences_with(&b.cx(), coincident);
        assert!(clashes.is_empty(), "{clashes:?}");
        assert!(failed.is_empty(), "{failed:?}");
    }
}

#[test]
fn an_oversized_shaft_has_the_annular_overlap_volume() {
    let b = Bench::fits(5.05, 5.0, -2.0, 12.0);
    let (clashes, failed) = interferences_with(&b.cx(), true);
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(clashes.len(), 1, "{clashes:?}");
    let want = std::f64::consts::PI * (2.525_f64.powi(2) - 2.5_f64.powi(2)) * 10.0;
    assert!(!clashes[0].touch);
    assert!(
        (clashes[0].volume / want - 1.0).abs() < 0.01,
        "{} / {want}",
        clashes[0].volume
    );
}

fn check_contact(from: f64, to: f64, length: f64) {
    let b = Bench::fits(5.0, 5.0, from, to);
    let (off, failed) = interferences_with(&b.cx(), false);
    assert!(off.is_empty(), "{off:?}");
    assert!(failed.is_empty(), "{failed:?}");
    let (clashes, failed) = interferences_with(&b.cx(), true);
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(clashes.len(), 1, "{clashes:?}");
    let c = &clashes[0];
    assert!(c.touch && c.volume == 0.0, "{c:?}");
    let want = std::f64::consts::PI * 5.0 * length;
    assert!((c.area - want).abs() < 1e-5, "{} / {want}", c.area);
    assert!(!c.mark.tris.is_empty(), "一致する円筒を塗る");
    let (lo, hi) = (from.max(0.0) as f32, to.min(10.0) as f32);
    for p in c.mark.tris.iter().flatten() {
        assert!(p[2] >= lo - 1e-4 && p[2] <= hi + 1e-4, "{p:?}, {lo}..{hi}");
    }
}

#[test]
fn an_exact_shaft_reports_the_cylindrical_face_as_coincident() {
    check_contact(-2.0, 12.0, 10.0);
}

#[test]
fn a_half_inserted_shaft_marks_only_the_inserted_length() {
    check_contact(5.0, 15.0, 5.0);
}

#[test]
#[ignore = "道具: 干渉の速さを手で測る"]
fn speed_cylinder_interferences() {
    let calibration = Timings::default();
    let now = Instant::now();
    for _ in 0..10_000 {
        let start = Instant::now();
        std::hint::black_box(());
        calibration.record("timer", start.elapsed());
    }
    eprintln!(
        "時計と記録の上限目安: {:.3} us/回",
        now.elapsed().as_secs_f64() * 1e6 / 10_000.0
    );
    for n in [10, 30, 100] {
        let mut b = Bench::new();
        for i in 0..n {
            let plane = Plane {
                origin: [20.0 * i as f64, 0.0, 0.0],
                ..Plane::XY
            };
            b.add(
                BODY,
                Cylinder {
                    outer: 6.0,
                    inner: 2.525,
                    plane,
                    from: 0.0,
                    to: 10.0,
                },
            );
            b.add(
                BODY,
                Cylinder {
                    outer: 2.5,
                    inner: 0.0,
                    plane,
                    from: -2.0,
                    to: 12.0,
                },
            );
        }
        b.recompute();
        b.timings.clear();
        let start = Instant::now();
        let (clashes, failed) = interferences_with(&b.cx(), true);
        let elapsed = start.elapsed();
        eprintln!(
            "干渉 {n}組/{}ボディ: {:.3} ms・検出{}・未検査{} | {}",
            n * 2,
            elapsed.as_secs_f64() * 1000.0,
            clashes.len(),
            failed.len(),
            b.timings.report()
        );
        let direct_cx = Context {
            kernel: &b.direct,
            ..b.cx()
        };
        let start = Instant::now();
        let (plain, unchecked) = interferences_with(&direct_cx, true);
        eprintln!(
            "記録を挟まない内側への直接呼出し: {:.3}ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        assert_eq!(
            (plain.len(), unchecked.len()),
            (clashes.len(), failed.len())
        );
        let bodies = b.cx().visible_bodies();
        let boxes: Vec<_> = bodies
            .iter()
            .map(|(_, h)| b.kernel.bounds(*h).unwrap())
            .collect();
        let start = Instant::now();
        let mut pairs = vec![];
        for i in 0..bodies.len() {
            for j in i + 1..bodies.len() {
                if boxes_meet(&boxes[i], &boxes[j], 1e-6) {
                    pairs.push((i, j));
                }
            }
        }
        let boxes_time = start.elapsed();
        assert_eq!(pairs.len(), n);
        let start = Instant::now();
        for (i, j) in pairs {
            let _ = contact(b.kernel.as_ref(), bodies[i].1, bodies[j].1).unwrap();
        }
        eprintln!(
            "独立した段の測定: 箱の組判定 {:.3} ms・面の一致 {:.3} ms",
            boxes_time.as_secs_f64() * 1000.0,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}

fn joint_chain(n: usize, matrices_only: bool) -> (Bench, Vec<FeatureId>) {
    let mut b = Bench::new();
    let bodies: Vec<_> = (0..n)
        .map(|i| {
            b.add(
                BODY,
                Cylinder {
                    outer: 2.5,
                    inner: 0.0,
                    plane: Plane {
                        origin: [10.0 * i as f64, 0.0, 0.0],
                        ..Plane::XY
                    },
                    from: 0.0,
                    to: 3.0,
                },
            )
        })
        .collect();
    let mut joints = vec![];
    for i in 0..n {
        let mut joint = new_joint(
            Part::Body { root: bodies[i] },
            i.checked_sub(1).map(|p| Part::Body { root: bodies[p] }),
            bodies[i..].to_vec(),
            &Origin {
                feature: bodies[i],
                point: [10.0 * i as f64, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
            },
        );
        joint.follows = joints.clone();
        joint.angle = "1 deg".into();
        joints.push(b.add(if matrices_only { MATRIX } else { JOINT }, joint));
    }
    b.recompute();
    (b, joints)
}

#[test]
#[ignore = "道具: ジョイントの再計算の速さを手で測る"]
fn speed_joint_chain_recomputation() {
    for n in [20, 50] {
        let (mut b, joints) = joint_chain(n, false);
        let (mut matrices, probes) = joint_chain(n, true);
        for (round, angle) in [2.0, 3.0, 2.0].into_iter().enumerate() {
            b.change_angle(joints[0], angle);
            b.timings.clear();
            let start = Instant::now();
            b.recompute();
            eprintln!(
                "ジョイント{n}個・変更{}回目: {:.3}ms・評価{}・再利用{} | {}",
                round + 1,
                start.elapsed().as_secs_f64() * 1000.0,
                b.report.evaluated,
                b.report.reused,
                b.timings.report()
            );
            matrices.change_angle(probes[0], angle);
            matrices.timings.clear();
            matrices.recompute();
            for (&a, &p) in joints.iter().zip(&probes) {
                let original = transform_of(b.cx().output(a, "transform").unwrap()).unwrap();
                let probe = transform_of(matrices.cx().output(p, "transform").unwrap()).unwrap();
                assert_eq!(original, probe, "行列の単独測定は同じ出力を作る");
            }
            eprintln!("行列だけの独立測定: {}", matrices.timings.report());
        }
    }
}
