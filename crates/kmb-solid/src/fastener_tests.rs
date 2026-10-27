//! 組立(kmb-asm)の規格部品: 穴の口を選んでボルトを挿す、呼びと長さを穴から選ぶ、種類を
//! 変える、部品表に 1 個ずつ数える、干渉しない。

use super::tests::{assert_ok, rect_sketch, registry};
use super::*;
use kmb_asm::fastener::{HoleMouth, Kind};
use kmb_asm::{AsmCommands, FASTENER, Fastener, bom_rows, interferences};
use kmb_core::{
    Context, Document, Extension, FacePick, FeatureId, Kernel, Op, Outcome, RecomputeReport,
    Transaction, body_root, visible_bodies,
};
use kmb_kernel_smlib::SmlibKernel;
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use serde_json::json;
use std::sync::Arc;

#[test]
fn expanded_thread_fasteners_include_large_metric_sizes() {
    for name in ["M14", "M16", "M20", "M24", "M30"] {
        let size = kmb_asm::fastener::metric(name).unwrap();
        let hole = kmb_cad_hole::thread(name).unwrap();
        assert_eq!(
            (size.d, size.pitch, size.tap),
            (hole.major, hole.pitch, hole.tap_drill)
        );
        assert_eq!(size.clearance, hole.clearance);
        assert!(hole.counterbore.0 > size.socket[0]);
        assert!(hole.counterbore.1 > size.socket[1]);
    }
}

#[test]
fn expanded_thread_large_bolts_nuts_and_washers_make_closed_bodies() {
    for name in ["M24", "M30"] {
        for kind in ["socket_head", "hex_bolt", "hex_nut", "washer"] {
            let mut b = DocBuilder::new();
            let fastener = b.feature(
                FASTENER,
                "規格部品",
                json!({
                    "kind":kind,"size":name,"length":"40 mm",
                    "places":[{"center":[0,0,0],"normal":[0,0,1]}]
                }),
            );
            b.feature(
                kmb_asm::fastener::FASTENER_BODY,
                "ボディ",
                json!({"fastener":fastener.0,"index":0}),
            );
            let mut doc = b.finish("大きい規格部品");
            let reg = registry();
            let k = SmlibKernel::new();
            let report = doc.recompute(&reg, &k);
            assert_ok(&report);
            let bodies = visible_bodies(doc.data(), &reg, &report);
            assert_eq!(bodies.len(), 1);
            let body = bodies[0].1;
            assert_eq!(k.check(body).unwrap(), (true, 1, 0), "{name} {kind}");
            assert!(k.mass(body).unwrap().volume > 0.0);
            let table = kmb_asm::fastener::metric(name).unwrap();
            let expected_height = match kind {
                "socket_head" => 40.0 + table.socket[1],
                "hex_bolt" => 40.0 + table.hex_head[1],
                "hex_nut" => table.nut[1],
                _ => table.washer[2],
            };
            let (min, max) = k.bounds(body).unwrap();
            assert!(
                (max[2] - min[2] - expected_height).abs() < 1e-3,
                "{name} {kind}: {min:?} {max:?}"
            );
        }
    }
}

struct Bench {
    doc: Document,
    reg: kmb_core::Registry,
    k: Arc<dyn Kernel>,
    report: RecomputeReport,
}

impl Bench {
    fn new(doc: Document) -> Self {
        let mut b = Bench {
            doc,
            reg: registry(),
            k: Arc::new(SmlibKernel::new()),
            report: RecomputeReport::default(),
        };
        b.recompute();
        b
    }

    fn recompute(&mut self) {
        self.report = self.doc.recompute(&self.reg, &*self.k);
        assert_ok(&self.report);
    }

    fn cx<'a>(&'a self, args: &'a serde_json::Value, pick: Option<&'a FacePick>) -> Context<'a> {
        Context {
            args,
            picked: pick,
            ..Context::new(self.doc.data(), &self.report, &self.reg, &self.k)
        }
    }

    /// 見えているボディ(機能と最初の機能と形)。
    fn bodies(&self) -> Vec<(FeatureId, FeatureId, kmb_core::ShapeHandle)> {
        let data = self.doc.data();
        visible_bodies(data, &self.reg, &self.report)
            .into_iter()
            .map(|(id, h)| (id, body_root(data, &self.reg, id), h))
            .collect()
    }

    fn bounds_of(&self, root: FeatureId) -> ([f64; 3], [f64; 3]) {
        let (_, _, h) = self
            .bodies()
            .into_iter()
            .find(|(_, r, _)| *r == root)
            .expect("見えているボディ");
        self.k.bounds(h).unwrap()
    }

    /// ボディ `feature` の、法線が `n` の平らな面(いちばん `n` の向きにある物)。
    fn face(&self, feature: FeatureId, n: [f64; 3]) -> FacePick {
        let (_, _, h) = self
            .bodies()
            .into_iter()
            .find(|(id, _, _)| *id == feature)
            .expect("見えているボディ");
        let face = self
            .k
            .faces(h)
            .unwrap()
            .into_iter()
            .filter(|f| {
                f.planar
                    .is_some_and(|p| (0..3).all(|i| (p.normal[i] - n[i]).abs() < 1e-6))
            })
            .max_by(|a, b| {
                let d = |f: &kmb_core::kernel::FaceInfo| {
                    let p = f.planar.unwrap().point;
                    p[0] * n[0] + p[1] * n[1] + p[2] * n[2]
                };
                d(a).total_cmp(&d(b))
            })
            .expect("その向きの面")
            .name;
        FacePick { feature, face }
    }

    fn run(&mut self, id: &str, args: serde_json::Value, pick: Option<FacePick>) -> Vec<FeatureId> {
        let cx = self.cx(&args, pick.as_ref());
        let Some(Ok(Outcome::Change { label, edits })) = AsmCommands.run(id, &cx) else {
            panic!("{id} を実行できない");
        };
        let added = self.doc.apply_edits(&label, edits).unwrap();
        self.recompute();
        added
    }

    fn set_data(&mut self, id: FeatureId, data: serde_json::Value) {
        self.doc
            .apply(Transaction::new(
                "値",
                vec![Op::SetFeatureData { id, data }],
            ))
            .unwrap();
        self.recompute();
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// 30×20×8 の板に、M3 の通し穴(直径 3.4)を 2 つ(中心 (8, 10) と (22, 10))。
fn plate() -> (Document, FeatureId) {
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 20.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            distance: "8 mm".into(),
            ..Default::default()
        },
    );
    let mut hs = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = hs.fixed_point(0.0, 0.0);
    let p1 = hs.point_from(o, "8 mm", "10 mm", [8.0, 10.0]);
    let p2 = hs.point_from(o, "22 mm", "10 mm", [22.0, 10.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "穴の位置", hs.build());
    let hole = b.feature(
        HOLE,
        "穴",
        Hole {
            points: vec![kmb_core::PointId(p1.0), kmb_core::PointId(p2.0)],
            diameter: "3.4 mm".into(),
            ..Hole::new(plate, sk)
        },
    );
    (b.finish("板とボルト"), hole)
}

/// 板の上の面をクリックすると、穴の口の円が 2 つ候補に出て、M3・長さ 8 mm のボルトが挿さる。
/// - ボルトは頭が上の面に載り(z = 8〜11)、首の下が板の厚さ(z = 0)まで
/// - 部品「規格部品1」にボディ 2 つが入り、部品表は板 1 行とボルト 1 行(2 個、鋼の質量つき)
/// - 通し穴の径(3.4)より細い(3.0)ので、板とは干渉しない
/// - 種類をナットにすると、穴の口の面の上に高さ 2.4 の六角が載る。座金は厚さ 0.5
#[test]
fn bolts_go_into_the_clicked_holes_and_are_counted() {
    let (doc, hole) = plate();
    let mut b = Bench::new(doc);
    let top = b.face(hole, [0.0, 0.0, 1.0]);
    let args = json!({"face": top.face});
    let handles = AsmCommands
        .handles(kmb_asm::NEW_FASTENER, &b.cx(&args, Some(&top)))
        .expect("手順がある");
    let holes: Vec<HoleMouth> = handles[1]
        .choices
        .iter()
        .map(|c| serde_json::from_value(c.value.clone()).unwrap())
        .collect();
    assert_eq!(holes.len(), 2, "{holes:?}");
    for m in &holes {
        assert!(near(m.diameter, 3.4) && m.hole, "{m:?}");
        assert!(near(m.normal[2], 1.0) && near(m.center[2], 8.0), "{m:?}");
        assert!(near(m.depth, 8.0), "{m:?}");
    }
    let values: Vec<serde_json::Value> =
        handles[1].choices.iter().map(|c| c.value.clone()).collect();
    let added = b.run(
        kmb_asm::NEW_FASTENER,
        json!({"face": top.face, "holes": values}),
        Some(top.clone()),
    );
    assert_eq!(added.len(), 3);
    let group = added[0];
    let f: Fastener =
        serde_json::from_value(b.doc.data().feature(group).unwrap().data.clone()).unwrap();
    assert_eq!(
        (f.kind, f.size.as_str(), f.length.as_str()),
        (Kind::SocketHead, "M3", "8 mm")
    );
    let c = b
        .doc
        .data()
        .components
        .iter()
        .find(|c| c.name == "規格部品1")
        .expect("部品");
    assert_eq!(c.bodies, vec![added[1], added[2]]);

    let (lo, hi) = b.bounds_of(added[1]);
    assert!(near(lo[2], 0.0) && near(hi[2], 11.0), "{lo:?} {hi:?}");
    let x = (lo[0] + hi[0]) / 2.0;
    assert!((x - 8.0).abs() < 1e-3 || (x - 22.0).abs() < 1e-3, "{x}");
    assert!(near(hi[0] - lo[0], 5.5), "頭の径: {lo:?} {hi:?}");

    let empty = json!({});
    let ctx = b.cx(&empty, None);
    let rows = bom_rows(&ctx);
    assert_eq!(rows.len(), 2, "{rows:?}");
    let bolt = rows
        .iter()
        .find(|r| r.names == ["六角穴付きボルト M3×8(JIS B 1176)"])
        .expect("ボルトの行");
    assert_eq!((bolt.count, bolt.material.as_str()), (2, "鋼"));
    assert!(bolt.mass.is_some_and(|g| g > 0.3 && g < 1.5), "{bolt:?}");
    let (clashes, failed) = interferences(&ctx);
    assert!(
        clashes.is_empty() && failed.is_empty(),
        "{clashes:?} {failed:?}"
    );

    // 種類を変える(呼びと穴はそのまま)
    let mut data = b.doc.data().feature(group).unwrap().data.clone();
    data["kind"] = json!("hex_nut");
    b.set_data(group, data.clone());
    let (lo, hi) = b.bounds_of(added[1]);
    assert!(near(lo[2], 8.0) && near(hi[2], 10.4), "{lo:?} {hi:?}");
    data["kind"] = json!("washer");
    b.set_data(group, data);
    let (lo, hi) = b.bounds_of(added[2]);
    assert!(
        near(lo[2], 8.0) && near(hi[2], 8.5) && near(hi[0] - lo[0], 7.0),
        "{lo:?} {hi:?}"
    );
    let rows = bom_rows(&b.cx(&empty, None));
    assert!(
        rows.iter()
            .any(|r| r.names == ["平座金 M3(JIS B 1256)"] && r.count == 2),
        "{rows:?}"
    );
}

/// 挿した後に穴の径を変えても、呼びは追いかけない(自動の合わせ直しはしない。docs/避ける形.md)。
/// 合わなくなったら、規格部品とその穴のボディの機能に注意が付く(タイムラインとブラウザに ⚠)。
/// 戻すと注意は消える。
#[test]
fn a_changed_hole_warns_but_keeps_the_size() {
    let (doc, hole) = plate();
    let mut b = Bench::new(doc);
    let top = b.face(hole, [0.0, 0.0, 1.0]);
    let args = json!({"face": top.face});
    let handles = AsmCommands
        .handles(kmb_asm::NEW_FASTENER, &b.cx(&args, Some(&top)))
        .expect("手順がある");
    let values: Vec<serde_json::Value> =
        handles[1].choices.iter().map(|c| c.value.clone()).collect();
    let added = b.run(
        kmb_asm::NEW_FASTENER,
        json!({"face": top.face, "holes": values}),
        Some(top.clone()),
    );
    let warnings = |b: &Bench, id: FeatureId| match b.report.feature(id) {
        Some(kmb_core::FeatureStatus::Ok { outputs, .. }) => kmb_core::warnings_of(outputs),
        s => panic!("{s:?}"),
    };
    let f: Fastener =
        serde_json::from_value(b.doc.data().feature(added[0]).unwrap().data.clone()).unwrap();
    assert!(f.places.iter().all(|p| p.body == Some(hole)), "{f:?}");
    assert!(warnings(&b, added[0]).is_empty());

    let mut h = b.doc.data().feature(hole).unwrap().data.clone();
    h["diameter"] = json!("4.5 mm");
    b.set_data(hole, h.clone());
    let w = warnings(&b, added[0]);
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(
        w[0].contains("穴1 の径 4.5 mm に呼び M3 が合いません(合うのは M4)"),
        "{w:?}"
    );
    assert_eq!(warnings(&b, added[1]).len(), 1, "穴のボディにも注意");
    let f: Fastener =
        serde_json::from_value(b.doc.data().feature(added[0]).unwrap().data.clone()).unwrap();
    assert_eq!(f.size, "M3", "呼びは変えない");

    h["diameter"] = json!("3.4 mm");
    b.set_data(hole, h);
    assert!(warnings(&b, added[0]).is_empty() && warnings(&b, added[1]).is_empty());
}

/// 呼びと穴の径の合い具合: ねじは下穴から粗い通し穴まで、軸受は外径(軸なら内径)。
#[test]
fn fits_follow_the_iso_hole_ranges() {
    use kmb_asm::fastener::fits;
    assert_eq!(fits(Kind::SocketHead, "M3", 3.4, true), Some(true));
    assert_eq!(fits(Kind::SocketHead, "M3", 2.5, true), Some(true)); // 下穴
    assert_eq!(fits(Kind::SocketHead, "M3", 4.5, true), Some(false));
    assert_eq!(fits(Kind::HexNut, "M5", 5.0, false), Some(true)); // 軸
    assert_eq!(fits(Kind::Bearing, "608", 22.0, true), Some(true));
    assert_eq!(fits(Kind::Bearing, "608", 24.0, true), Some(false));
    assert_eq!(fits(Kind::SocketHead, "M7", 7.0, true), None);
}

/// 穴の径から呼びを選ぶ: 下穴・通し穴・呼び径のいちばん近い物。軸受は穴なら外径。
#[test]
fn sizes_follow_the_hole_diameter() {
    use kmb_asm::fastener::size_for;
    assert_eq!(size_for(Kind::SocketHead, 3.4, true), "M3");
    assert_eq!(size_for(Kind::SocketHead, 4.2, true), "M5"); // M5 の下穴
    assert_eq!(size_for(Kind::HexNut, 6.6, true), "M6");
    assert_eq!(size_for(Kind::Bearing, 22.0, true), "608");
    assert_eq!(size_for(Kind::Bearing, 8.0, false), "688");
    assert_eq!(kmb_asm::fastener::length_for(8.0), 8.0);
    assert_eq!(kmb_asm::fastener::length_for(9.0), 10.0);
}

/// 玉軸受は穴の口から穴の中へ、幅の分だけ入る(608 は外径 22・幅 7)。
#[test]
fn a_bearing_sits_in_its_bore() {
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 40.0, 40.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut hs = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = hs.fixed_point(0.0, 0.0);
    let p = hs.point_from(o, "20 mm", "20 mm", [20.0, 20.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "穴の位置", hs.build());
    let hole = b.feature(
        HOLE,
        "軸受の穴",
        Hole {
            points: vec![kmb_core::PointId(p.0)],
            diameter: "22 mm".into(),
            ..Hole::new(plate, sk)
        },
    );
    let mut bench = Bench::new(b.finish("軸受"));
    let top = bench.face(hole, [0.0, 0.0, 1.0]);
    let args = json!({"face": top.face});
    let handles = AsmCommands
        .handles(kmb_asm::NEW_FASTENER, &bench.cx(&args, Some(&top)))
        .unwrap();
    let mouth = handles[1].choices[0].value.clone();
    let added = bench.run(
        kmb_asm::NEW_FASTENER,
        json!({"face": top.face, "holes": [mouth], "kind": "bearing"}),
        Some(top),
    );
    let f: Fastener =
        serde_json::from_value(bench.doc.data().feature(added[0]).unwrap().data.clone()).unwrap();
    assert_eq!(f.size, "608");
    let (lo, hi) = bench.bounds_of(added[1]);
    assert!(
        near(lo[2], 3.0) && near(hi[2], 10.0) && near(hi[0] - lo[0], 22.0),
        "{lo:?} {hi:?}"
    );
    assert_eq!(
        bench.doc.data().feature(added[0]).unwrap().type_id,
        FASTENER
    );
}

/// 板の 2 つの穴に座金を挿してから、同じ穴の口にボルトを挿すと、座金の上に重なる(頭の下が
/// z = 8.5)。座金の呼びを M4(厚さ 0.8)に変えると、ボルトも上がる。座金の上にナットも重なる。
#[test]
fn a_bolt_stacks_on_the_washer_in_the_same_hole() {
    let (doc, hole) = plate();
    let mut b = Bench::new(doc);
    let top = b.face(hole, [0.0, 0.0, 1.0]);
    let args = json!({"face": top.face});
    let mouths = |b: &Bench| -> Vec<serde_json::Value> {
        let handles = AsmCommands
            .handles(kmb_asm::NEW_FASTENER, &b.cx(&args, Some(&top)))
            .unwrap();
        handles[1].choices.iter().map(|c| c.value.clone()).collect()
    };
    let values = mouths(&b);
    let washers = b.run(
        kmb_asm::NEW_FASTENER,
        json!({"face": top.face, "holes": values, "kind": "washer"}),
        Some(top.clone()),
    );
    let values = mouths(&b);
    assert_eq!(values.len(), 2, "座金があっても穴の口は 2 つ");
    let bolts = b.run(
        kmb_asm::NEW_FASTENER,
        json!({"face": top.face, "holes": values}),
        Some(top.clone()),
    );
    let f: Fastener =
        serde_json::from_value(b.doc.data().feature(bolts[0]).unwrap().data.clone()).unwrap();
    assert!(
        f.places
            .iter()
            .all(|p| p.on.is_some_and(|s| s.fastener == washers[0])),
        "{f:?}"
    );
    for body in &bolts[1..] {
        let (lo, hi) = b.bounds_of(*body);
        assert!(near(hi[2], 11.5) && near(lo[2], 0.5), "{lo:?} {hi:?}");
    }

    let mut w = b.doc.data().feature(washers[0]).unwrap().data.clone();
    w["size"] = json!("M4");
    b.set_data(washers[0], w);
    let (lo, hi) = b.bounds_of(bolts[1]);
    assert!(near(hi[2], 11.8) && near(lo[2], 0.8), "{lo:?} {hi:?}");

    // 座金の上にナットも重ねられる(座金 → ナットの順で、ナットの下は座金の上の端)
    let values = mouths(&b);
    let nuts = b.run(
        kmb_asm::NEW_FASTENER,
        json!({"face": top.face, "holes": [values[0].clone()], "kind": "hex_nut"}),
        Some(top.clone()),
    );
    let (lo, _) = b.bounds_of(nuts[1]);
    assert!(near(lo[2], 8.8), "{lo:?}");

    // 座金を消すと、上に重ねたボルトはほかの参照と同じく理由つきで止まる(消す前の確かめに出る)
    let ops = washers
        .iter()
        .rev()
        .map(|id| Op::RemoveFeature { id: *id })
        .collect();
    b.doc.apply(Transaction::new("座金を消す", ops)).unwrap();
    b.report = b.doc.recompute(&b.reg, &*b.k);
    match b.report.feature(bolts[0]) {
        Some(kmb_core::FeatureStatus::Error { message, .. }) => {
            assert!(message.contains("参照先の機能"), "{message}")
        }
        s => panic!("{s:?}"),
    }
}

/// JIS の名前: 並目はピッチを省き、細目は「M10×1.25」。細目は六角ボルトとナットの候補に出る。
/// 細目の表に無いピッチと、表に無い呼びは読まない。細目の下穴は呼び径からピッチを引いた径。
#[test]
fn names_follow_the_jis_thread_designation() {
    use kmb_asm::fastener::{fits, size_name, thread};
    assert_eq!(size_name("M10x1.25"), "M10×1.25");
    assert_eq!(size_name("M10×1.5"), "M10", "並目のピッチは省く");
    assert_eq!(size_name("m3"), "M3");
    assert!(thread("M10×1.75").is_none(), "M10 に 1.75 の細目は無い");
    assert!(thread("M27").is_none());
    assert!(thread("M1.6").is_some() && thread("M20×1.5").is_some());
    assert_eq!(fits(Kind::HexNut, "M10×1.25", 8.75, true), Some(true));
    assert_eq!(fits(Kind::HexNut, "M10", 8.75, true), Some(true));
    assert_eq!(fits(Kind::HexNut, "M10", 8.2, true), Some(false));

    let f = |kind: &str, size: &str| -> Fastener {
        serde_json::from_value(json!({
            "kind": kind, "size": size, "length": "30 mm",
            "places": [{"center": [0, 0, 0], "normal": [0, 0, 1]}]
        }))
        .unwrap()
    };
    assert_eq!(
        f("hex_bolt", "M10x1.25").designation(Some(30.0)),
        "六角ボルト M10×1.25×30(JIS B 1180)"
    );
    assert_eq!(
        f("socket_head", "M12").designation(Some(20.0)),
        "六角穴付きボルト M12×20(JIS B 1176)"
    );
    assert_eq!(
        f("hex_nut", "M8×1").designation(None),
        "六角ナット M8×1(JIS B 1181)"
    );
    assert_eq!(
        f("bearing", "608").designation(None),
        "玉軸受 608(JIS B 1521)"
    );
    assert_eq!(Kind::Washer.label(), "平座金");
    assert_eq!(Kind::Washer.standard(), "JIS B 1256");
}

/// 皿の穴(皿の径 6.3、90°)の口の円に挿すと、種類は皿小ねじになり、皿の径から M3 を選び、頭の円すいが皿に当たる
/// 高さに沈む(頭の径 6 なので、頭の上は面から 0.15 下の z = 7.85)。長さは頭を含む全長で、皿の下の通し穴の奥(板の厚さ 8)から 8 mm。板とは
/// 干渉しない。なべ小ねじは同じ板の上の面に頭が載る(高さ 2.0)。部品表は JIS B 1111 の名前。
#[test]
fn small_screws_sit_on_or_in_the_hole() {
    let (doc, hole) = plate();
    let mut b = Bench::new(doc);
    let mut h = b.doc.data().feature(hole).unwrap().data.clone();
    h["kind"] = json!("countersink");
    h["countersink_diameter"] = json!("6.3 mm");
    b.set_data(hole, h);
    let top = b.face(hole, [0.0, 0.0, 1.0]);
    let args = json!({"face": top.face});
    let handles = AsmCommands
        .handles(kmb_asm::NEW_FASTENER, &b.cx(&args, Some(&top)))
        .unwrap();
    let mouths: Vec<HoleMouth> = handles[1]
        .choices
        .iter()
        .map(|c| serde_json::from_value(c.value.clone()).unwrap())
        .collect();
    assert_eq!(mouths.len(), 2, "{mouths:?}");
    assert!(
        mouths
            .iter()
            .all(|m| near(m.diameter, 6.3) && near(m.through, 8.0) && m.hole),
        "{mouths:?}"
    );
    let values: Vec<serde_json::Value> =
        handles[1].choices.iter().map(|c| c.value.clone()).collect();
    let flat = b.run(
        kmb_asm::NEW_FASTENER,
        json!({"face": top.face, "holes": values}),
        Some(top.clone()),
    );
    let f: Fastener =
        serde_json::from_value(b.doc.data().feature(flat[0]).unwrap().data.clone()).unwrap();
    assert_eq!((f.size.as_str(), f.length.as_str()), ("M3", "8 mm"));
    assert!(f.places.iter().all(|p| p.on.is_none()));
    let (lo, hi) = b.bounds_of(flat[1]);
    assert!(
        near(hi[2], 7.85) && near(lo[2], -0.15) && near(hi[0] - lo[0], 6.0),
        "{lo:?} {hi:?}"
    );
    let empty = json!({});
    let ctx = b.cx(&empty, None);
    let (clashes, failed) = interferences(&ctx);
    assert!(
        clashes.is_empty() && failed.is_empty(),
        "{clashes:?} {failed:?}"
    );
    assert!(
        bom_rows(&ctx)
            .iter()
            .any(|r| r.names == ["皿小ねじ M3×8(JIS B 1111)"] && r.count == 2),
        "{:?}",
        bom_rows(&ctx)
    );

    // なべ小ねじに変えると、頭が面の上に載る
    let mut data = b.doc.data().feature(flat[0]).unwrap().data.clone();
    data["kind"] = json!("pan_head");
    b.set_data(flat[0], data);
    let (lo, hi) = b.bounds_of(flat[1]);
    assert!(
        near(hi[2], 10.0) && near(lo[2], 0.0) && near(hi[0] - lo[0], 5.5),
        "{lo:?} {hi:?}"
    );
    use kmb_asm::fastener::{fits, size_for};
    assert_eq!(size_for(Kind::FlatHead, 6.3, true), "M3");
    assert_eq!(
        size_for(Kind::PanHead, 8.8, true),
        "M6",
        "M8 は小ねじの表に無い"
    );
    assert_eq!(fits(Kind::FlatHead, "M3", 6.3, true), Some(true));
    assert_eq!(fits(Kind::PanHead, "M3", 6.3, true), Some(false));
    assert_eq!(fits(Kind::PanHead, "M8", 9.0, true), None);
}
