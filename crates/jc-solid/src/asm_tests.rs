//! 組立(jc-asm): ジョイントで動かす、動いた部品について動く、干渉、部品表、拘束。動きの試験は `motion_tests`。

use super::tests::{assert_ok, rect_sketch, registry};
use super::*;
use jc_asm::{
    AsmCommands, BOM, DRAG_POSITION, INTERFERENCE, JOINT, Joint, JointKind, MOVE_PART,
    NEW_COMPONENT, NEW_JOINT, NEW_SNAP_JOINT, Origin, POSITION, SnapPick, bom_rows, interferences,
    interferences_with,
};
use jc_cad_modify::MOVE;
use jc_core::{
    Component, Context, Document, Extension, FacePick, FeatureId, Kernel, Op, Outcome,
    RecomputeReport, Transaction, body_root, visible_bodies,
};
use jc_interchange::{animation, contact, motion};
use jc_kernel_smlib::SmlibKernel;
use jc_samples::sample::DocBuilder;
use jc_sketch::SketchPlane;
use serde_json::json;
use std::sync::Arc;

mod motion_tests;

/// 長方形 `[x, y, w, h]` を高さ `t` に押し出し、`z` だけ上げた箱。最後の機能を返す。
fn block(b: &mut DocBuilder, name: &str, [x, y, w, h]: [f64; 4], t: f64, z: f64) -> FeatureId {
    let s = rect_sketch(b, SketchPlane::Xy, x, y, w, h);
    let e = b.feature(
        EXTRUDE,
        name,
        Extrude {
            profile: Some(s),
            distance: format!("{t} mm"),
            ..Default::default()
        },
    );
    if z == 0.0 {
        return e;
    }
    b.feature(
        MOVE,
        &format!("{name}を上げる"),
        json!({"body": e.0, "z": format!("{z} mm")}),
    )
}

struct Bench {
    doc: Document,
    reg: jc_core::Registry,
    k: Arc<dyn Kernel>,
    report: RecomputeReport,
}

impl Bench {
    fn recompute(&mut self) {
        self.report = self.doc.recompute(&self.reg, &*self.k);
        assert_ok(&self.report);
    }

    /// ボディ `root` の今の外形の箱。
    fn bounds(&self, root: FeatureId) -> ([f64; 3], [f64; 3]) {
        let data = self.doc.data();
        let (_, h) = visible_bodies(data, &self.reg, &self.report)
            .into_iter()
            .find(|(id, _)| body_root(data, &self.reg, *id) == root)
            .expect("見えているボディ");
        self.k.bounds(h).unwrap()
    }

    /// ジョイントの操作を、動かすボディの軸の候補と土台のボディ(なければ地面)で実行する。
    fn joint(&mut self, origin: Origin, base: Option<FeatureId>) -> FeatureId {
        let args = json!({"moving": "f", "origin": origin, "base": base.map(|_| "f")});
        let pick = base.map(|f| FacePick {
            feature: f,
            face: "f".into(),
        });
        let cx = Context {
            args: &args,
            picked: pick.as_ref(),
            ..Context::new(self.doc.data(), &self.report, &self.reg, &self.k)
        };
        let Some(Ok(Outcome::Change { label, edits })) = AsmCommands.run(NEW_JOINT, &cx) else {
            panic!("ジョイントを作れない");
        };
        let added = self.doc.apply_edits(&label, edits).unwrap();
        self.recompute();
        added[0]
    }

    fn set_joint(&mut self, id: FeatureId, f: impl FnOnce(&mut Joint)) {
        let mut j: Joint =
            serde_json::from_value(self.doc.data().feature(id).unwrap().data.clone()).unwrap();
        f(&mut j);
        self.doc
            .apply(Transaction::new(
                "ジョイントの値",
                vec![Op::SetFeatureData {
                    id,
                    data: serde_json::to_value(j).unwrap(),
                }],
            ))
            .unwrap();
        self.recompute();
    }

    fn joint_data(&self, id: FeatureId) -> Joint {
        serde_json::from_value(self.doc.data().feature(id).unwrap().data.clone()).unwrap()
    }

    fn cx(&self) -> Context<'_> {
        Context::new(self.doc.data(), &self.report, &self.reg, &self.k)
    }
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3)
}

fn center(b: ([f64; 3], [f64; 3])) -> [f64; 3] {
    [0, 1, 2].map(|i| (b.0[i] + b.1[i]) / 2.0)
}

/// 試験の組立: 箱(30×20×10)の上に蓋(厚さ 2。部品「蓋」)、蓋の上につまみ(4×4×4)。蓋は奥の上の
/// 辺を蝶番に箱へ、つまみは自分の中心の縦軸で蓋へ、回転のジョイントでつなぐ。
/// (台, ボディの最初の機能 [箱, 蓋, つまみ], 蓋のジョイント, つまみのジョイント,
/// 形の最後の機能 [箱, 蓋, つまみ](面を選ぶ時に使う))。
fn lid_and_knob() -> (Bench, Vec<FeatureId>, FeatureId, FeatureId, [FeatureId; 3]) {
    let (t, roots, hinge, spin, last, _) = lid_and_knob_tagged(false);
    (t, roots, hinge, spin, last)
}

/// [`lid_and_knob`] に、`tag` なら箱の右に離して置いた札(5×5×2 のボディ、部品に入れない)を足した物。
/// 最後は札の最初の機能。
#[allow(clippy::type_complexity)]
fn lid_and_knob_tagged(
    tag: bool,
) -> (
    Bench,
    Vec<FeatureId>,
    FeatureId,
    FeatureId,
    [FeatureId; 3],
    Option<FeatureId>,
) {
    let mut b = DocBuilder::new();
    let boxed = block(&mut b, "箱", [0.0, 0.0, 30.0, 20.0], 10.0, 0.0);
    let lid = block(&mut b, "蓋", [0.0, 0.0, 30.0, 20.0], 2.0, 10.0);
    let knob = block(&mut b, "つまみ", [13.0, 8.0, 4.0, 4.0], 4.0, 12.0);
    let extra = tag.then(|| block(&mut b, "札", [40.0, 0.0, 5.0, 5.0], 2.0, 0.0));
    let doc = b.finish("組立");
    let reg = registry();
    let mut t = Bench {
        doc,
        k: Arc::new(SmlibKernel::new()),
        report: Default::default(),
        reg,
    };
    t.recompute();
    let roots: Vec<FeatureId> = [boxed, lid, knob]
        .iter()
        .map(|f| body_root(t.doc.data(), &t.reg, *f))
        .collect();
    let c = t.doc.alloc_group_id();
    t.doc
        .apply(Transaction::new(
            "部品",
            vec![Op::SetComponents {
                components: vec![Component {
                    id: c,
                    name: "蓋".into(),
                    parent: None,
                    bodies: vec![roots[1]],
                }],
            }],
        ))
        .unwrap();
    t.recompute();
    // 蓋: 奥の上の辺(y = 20、z = 12)を X の向きの蝶番に、箱を土台に
    let hinge = t.joint(
        Origin {
            feature: lid,
            point: [0.0, 20.0, 12.0],
            axis: [1.0, 0.0, 0.0],
        },
        Some(boxed),
    );
    // つまみ: 中心の縦軸、蓋を土台に
    let spin = t.joint(
        Origin {
            feature: knob,
            point: [15.0, 10.0, 12.0],
            axis: [0.0, 0.0, 1.0],
        },
        Some(lid),
    );
    let extra = extra.map(|f| body_root(t.doc.data(), &t.reg, f));
    (t, roots, hinge, spin, [boxed, lid, knob], extra)
}

/// 新しい部品の操作の親(画面の「新しい部品にする」「中に新しい部品」も同じ操作を流す): 引数 `parent`
/// があればその中(空の部品も作れる)、無い部品の番号は誤り。省くと、選んだボディがみな同じ部品に
/// 入っていればその中、別々なら文書の直下。`selection` はボディの最初に作った機能でもよい。
#[test]
fn a_new_component_goes_into_the_given_parent_or_the_common_one() {
    let (t, roots, ..) = lid_and_knob();
    let lid_part = t.doc.data().components[0].id;
    let made = |args: serde_json::Value| -> Result<Component, String> {
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        match AsmCommands.run(NEW_COMPONENT, &cx) {
            Some(Ok(Outcome::Change { edits, .. })) => match edits.into_iter().last() {
                Some(jc_core::Edit::SetComponents { components }) => {
                    Ok(components.last().cloned().unwrap())
                }
                e => panic!("部品の木の変更でない: {e:?}"),
            },
            Some(Err(e)) => Err(e),
            _ => panic!("部品の操作が無い"),
        }
    };
    let c = made(json!({"selection": [], "parent": lid_part})).unwrap();
    assert_eq!(
        (c.name.as_str(), c.parent, c.bodies.len()),
        ("部品1", Some(lid_part), 0)
    );
    let e = made(json!({"selection": [], "parent": 999})).unwrap_err();
    assert!(e.contains("999"), "{e}");
    // 蓋だけ(部品「蓋」の中)ならその中、箱(直下)と一緒なら直下
    let c = made(json!({"selection": [roots[1].0]})).unwrap();
    assert_eq!(
        (c.parent, c.bodies.clone()),
        (Some(lid_part), vec![roots[1]])
    );
    let c = made(json!({"selection": [roots[0].0, roots[1].0], "name": "箱と蓋"})).unwrap();
    assert_eq!((c.name.as_str(), c.parent), ("箱と蓋", None));
    assert_eq!(c.bodies.len(), 2);
}

/// 箱(30×20×10)の上に蓋(厚さ 2)、蓋の上につまみ(4×4×4)。蓋は奥の上の辺を蝶番に、つまみは
/// 自分の中心の縦軸で回す。
/// - 蓋を -90 度にすると立ち上がり、つまみも一緒に動く(後から作ったつまみのジョイントが、
///   蓋のジョイントの動かす物に入る)。箱は動かない
/// - つまみを 45 度回すと、蓋で動いた先の自分の軸のまわりに回る(中心は同じ)
/// - 蓋を 10 度(箱へめり込む向き)にすると干渉が 2 組(蓋とつまみ)。0 度(載っているだけ)は無い
/// - 部品表は 3 行(箱・蓋・つまみ)で、長さ・幅・厚さの順

#[test]
fn hinge_moves_the_lid_and_the_knob_on_it_and_finds_interference() {
    let (mut t, roots, hinge, spin, [boxed, lid, knob]) = lid_and_knob();
    let lid0 = t.bounds(roots[1]);
    let knob0 = t.bounds(roots[2]);
    let hj: Joint =
        serde_json::from_value(t.doc.data().feature(hinge).unwrap().data.clone()).unwrap();
    assert_eq!(hj.bodies.len(), 2, "蓋のジョイントがつまみも動かす: {hj:?}");
    let sj: Joint =
        serde_json::from_value(t.doc.data().feature(spin).unwrap().data.clone()).unwrap();
    assert_eq!(sj.follows, vec![hinge]);
    assert_eq!(t.doc.data().feature(hinge).unwrap().type_id, JOINT);
    // 値 0 では動かない
    assert!(near(t.bounds(roots[1]).0, lid0.0) && near(t.bounds(roots[2]).1, knob0.1));

    t.set_joint(hinge, |j| j.angle = "-90 deg".into());
    let l = t.bounds(roots[1]);
    assert!(
        near(l.0, [0.0, 18.0, 12.0]) && near(l.1, [30.0, 20.0, 32.0]),
        "{l:?}"
    );
    let k1 = t.bounds(roots[2]);
    // つまみ(y 8〜12、z 12〜16)は蝶番のまわりに: y = 20 + dz、z = 12 - dy
    assert!(
        near(k1.0, [13.0, 20.0, 20.0]) && near(k1.1, [17.0, 24.0, 24.0]),
        "{k1:?}"
    );
    assert!(
        near(t.bounds(roots[0]).1, [30.0, 20.0, 10.0]),
        "箱は動かない"
    );

    // 3D のビューで引いて動かす物: つまみはつまみのジョイント(動かすボディのいちばん少ない物)で、
    // 軸は蓋で立ち上がった先(上を向いていた軸が +Y に)。蓋は蝶番、箱(土台)は動かせない
    let face = |feature| FacePick {
        feature,
        face: "f".into(),
    };
    let d = AsmCommands
        .drag(&t.cx(), &face(knob))
        .expect("つまみは回せる");
    assert_eq!((d.feature, d.field.as_str()), (spin, "angle"));
    let jc_core::DragMotion::Rotate { origin, axis } = d.motion else {
        panic!("{d:?}")
    };
    assert!(
        near(origin, [15.0, 20.0, 22.0]) && near(axis, [0.0, 1.0, 0.0]),
        "{d:?}"
    );
    let d = AsmCommands.drag(&t.cx(), &face(lid)).expect("蓋は回せる");
    assert_eq!(d.feature, hinge);
    assert!(AsmCommands.drag(&t.cx(), &face(boxed)).is_none());

    t.set_joint(spin, |j| j.angle = "45 deg".into());
    let k2 = t.bounds(roots[2]);
    assert!(near(center(k2), center(k1)), "{k2:?} {k1:?}");
    let w = k2.1[0] - k2.0[0];
    assert!((w - 4.0 * 2f64.sqrt()).abs() < 1e-3, "45 度で対角の幅: {w}");

    // 干渉: 載っているだけなら無い、めり込めば 1 組
    t.set_joint(spin, |j| j.angle = "0 deg".into());
    t.set_joint(hinge, |j| j.angle = "0 deg".into());
    assert!(interferences(&t.cx()).0.is_empty());
    t.set_joint(hinge, |j| j.angle = "10 deg".into());
    let (clash, failed) = interferences(&t.cx());
    assert!(failed.is_empty());
    // 蓋と箱、蓋と一緒に傾いたつまみの角も箱にわずかに入る(y = 8 の下の角が z = 9.92)
    assert_eq!(clash.len(), 2, "{clash:?}");
    assert!(
        clash[0].volume > 100.0 && clash[1].volume < 1.0,
        "{clash:?}"
    );
    let Some(Ok(Outcome::Table { table, .. })) = AsmCommands.run(INTERFERENCE, &t.cx()) else {
        panic!()
    };
    assert_eq!(table.rows.len(), 2, "{table:?}");
    assert_eq!(table.marks.len(), 2);
    assert!(
        table.marks.iter().all(|m| !m.tris.is_empty()),
        "重なった所を 3D で見せる立体"
    );

    // 範囲の外の値は上限で止める
    t.set_joint(hinge, |j| {
        j.angle = "-200 deg".into();
        j.angle_min = "-110 deg".into();
    });
    let warn = jc_core::warnings_of(match t.report.feature(hinge).unwrap() {
        jc_core::FeatureStatus::Ok { outputs, .. } => outputs,
        s => panic!("{s:?}"),
    });
    assert_eq!(warn.len(), 1, "{warn:?}");

    // 戻る位置が範囲の外・下限が上限より大きい時も注意を出す
    let warns = |t: &Bench| {
        jc_core::warnings_of(match t.report.feature(hinge).unwrap() {
            jc_core::FeatureStatus::Ok { outputs, .. } => outputs,
            s => panic!("{s:?}"),
        })
    };
    t.set_joint(hinge, |j| {
        j.angle = "-30 deg".into();
        j.angle_max = "0 deg".into();
        j.angle_rest = "20 deg".into();
    });
    let w = warns(&t);
    assert!(w.len() == 1 && w[0].contains("戻る位置"), "{w:?}");
    t.set_joint(hinge, |j| {
        j.angle_min = "10 deg".into();
        j.angle_rest = String::new();
    });
    let w = warns(&t);
    assert!(
        w.iter().any(|w| w.contains("下限が上限より大きい")),
        "{w:?}"
    );
    t.set_joint(hinge, |j| {
        j.angle_min = "-110 deg".into();
        j.angle_max = String::new();
    });

    // 部品表: 3 行(どれも 1 個)。蓋は部品の名前、寸法は長い順
    t.set_joint(hinge, |j| j.angle = "0 deg".into());
    let rows = bom_rows(&t.cx());
    assert_eq!(rows.len(), 3, "{rows:?}");
    let lid_row = rows.iter().find(|r| r.names == ["蓋"]).unwrap();
    assert!(near(lid_row.size, [30.0, 20.0, 2.0]), "{lid_row:?}");
    // 材料を付けていない蓋は既定の鋼(7.85 g/cm³)で、30×20×2 mm = 1.2 cm³ → 9.42 g
    assert!(
        lid_row.mass.is_some_and(|g| (g - 9.42).abs() < 0.01),
        "{lid_row:?}"
    );
    assert!(matches!(
        AsmCommands.run(BOM, &t.cx()),
        Some(Ok(Outcome::Table { table, .. })) if table.rows.len() == 3
    ));
}

/// 「一致した面を含める」: 閉じた蓋は箱に、つまみは蓋に平らな面で接する(体積 0、接する面積と
/// 塗る面)。めり込めば接する組ではなく重なりとして出す。
#[test]
fn coincident_faces_are_listed_when_asked() {
    let (mut t, _, hinge, _, _) = lid_and_knob();
    assert!(interferences(&t.cx()).0.is_empty(), "既定では数えない");
    let (touch, failed) = interferences_with(&t.cx(), true);
    assert!(failed.is_empty());
    assert_eq!(touch.len(), 2, "{touch:?}");
    assert!(touch.iter().all(|c| c.touch && c.volume == 0.0));
    let mut areas: Vec<f64> = touch.iter().map(|c| c.area).collect();
    areas.sort_by(f64::total_cmp);
    assert!(
        (areas[0] - 16.0).abs() < 1e-6 && (areas[1] - 600.0).abs() < 1e-6,
        "つまみの底 4×4 と蓋の底 30×20: {areas:?}"
    );
    assert!(
        touch.iter().all(|c| !c.mark.tris.is_empty()),
        "接する面を塗る"
    );
    // 引数で同じことを頼める(画面の切り替え・CLI・MCP)
    let args = serde_json::json!({"coincident": true});
    let cx = jc_core::Context {
        args: &args,
        ..t.cx()
    };
    let Some(Ok(Outcome::Table { table, .. })) = AsmCommands.run(INTERFERENCE, &cx) else {
        panic!()
    };
    assert_eq!(table.rows.len(), 2);
    assert!(table.options[0].on);
    assert_eq!(table.rows[0][4], "面の一致");
    t.set_joint(hinge, |j| j.angle = "10 deg".into());
    let (c, _) = interferences_with(&t.cx(), true);
    assert!(c.iter().any(|c| !c.touch && c.volume > 100.0), "{c:?}");
    assert!(
        !c.iter()
            .any(|c| c.touch && c.names[0].contains("箱") && c.names[1].contains("蓋")),
        "めり込んだ組は接する組に出さない: {c:?}"
    );
}

/// ジョイントの無い部品を引くと、画面に平行に動かす「位置」の機能が入り、その部品を土台にした部品
/// (蓋とつまみ)も一緒に動く。続けて引くと同じ機能の値に足す(タイムラインを伸ばさない)。部品に
/// 入っていないボディは自由に引けず、ジョイントで動く蓋はジョイントで引く。位置の後も、蓋の
/// ジョイントは動いた先の蝶番のまわりに回り、引く軸もそこにある。
#[test]
fn a_free_component_is_dragged_with_the_parts_on_it() {
    let (mut t, roots, hinge, _spin, [boxed, lid, _knob]) = lid_and_knob();
    let face = |feature| FacePick {
        feature,
        face: "f".into(),
    };
    assert!(
        AsmCommands.drag(&t.cx(), &face(boxed)).is_none(),
        "部品に入っていないボディは引かない"
    );
    let mut list = t.doc.data().components.clone();
    list.push(Component {
        id: t.doc.alloc_group_id(),
        name: "箱".into(),
        parent: None,
        bodies: vec![roots[0]],
    });
    t.doc
        .apply(Transaction::new(
            "部品",
            vec![Op::SetComponents { components: list }],
        ))
        .unwrap();
    t.recompute();
    let d = AsmCommands
        .drag(&t.cx(), &face(boxed))
        .expect("箱の部品は自由に引ける");
    assert_eq!(
        (d.motion, d.field.as_str()),
        (jc_core::DragMotion::Free, DRAG_POSITION)
    );
    let before: Vec<_> = roots.iter().map(|r| t.bounds(*r)).collect();
    let drag_by = |t: &mut Bench, offset: [f64; 3]| {
        let args = json!({"body": boxed.0, "offset": offset});
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        let Some(Ok(Outcome::Change { label, edits })) = AsmCommands.run(&d.field, &cx) else {
            panic!("引いて動かせない");
        };
        t.doc.apply_edits(&label, edits).unwrap();
        t.recompute();
    };
    let positions = |t: &Bench| -> Vec<jc_asm::Position> {
        t.doc
            .data()
            .features
            .iter()
            .filter(|e| e.type_id == POSITION)
            .map(|e| serde_json::from_value(e.data.clone()).unwrap())
            .collect()
    };
    drag_by(&mut t, [5.0, 0.0, 0.0]);
    drag_by(&mut t, [0.0, 3.0, 0.0]);
    let p = positions(&t);
    assert_eq!(p.len(), 1, "続けて引くと同じ機能を直す: {p:?}");
    assert_eq!((p[0].x.as_str(), p[0].y.as_str()), ("5 mm", "3 mm"));
    assert_eq!(p[0].bodies.len(), 3, "箱と、箱を土台にした蓋とつまみ");
    for (r, b) in roots.iter().zip(&before) {
        let now = t.bounds(*r);
        assert!(
            near(now.0, [b.0[0] + 5.0, b.0[1] + 3.0, b.0[2]]),
            "{now:?} {b:?}"
        );
    }
    // 蓋はジョイントで引く。軸は動いた先の蝶番
    let d = AsmCommands.drag(&t.cx(), &face(lid)).expect("蓋は回せる");
    assert_eq!(d.feature, hinge);
    let jc_core::DragMotion::Rotate { origin, axis } = d.motion else {
        panic!("{d:?}")
    };
    assert!(
        near(origin, [5.0, 23.0, 12.0]) && near(axis, [1.0, 0.0, 0.0]),
        "{d:?}"
    );
    t.set_joint(hinge, |j| j.angle = "-90 deg".into());
    let l = t.bounds(roots[1]);
    assert!(
        near(l.0, [5.0, 21.0, 12.0]) && near(l.1, [35.0, 23.0, 32.0]),
        "{l:?}"
    );
}

/// 部品を値で動かす操作(M キーの道): 部品ならボディと土台にした部品ごと 0 mm の位置を足す。
/// 部品に入っていないボディ・ジョイントで動く部品は理由を返す。
#[test]
fn moving_a_part_by_value_adds_a_position_for_the_whole_part() {
    let (mut t, roots, _hinge, _spin, [boxed, lid, _knob]) = lid_and_knob();
    // 蓋はもう部品(ジョイントで箱に付く)。箱も部品にする
    let mut list = t.doc.data().components.clone();
    list.push(Component {
        id: t.doc.alloc_group_id(),
        name: "箱".into(),
        parent: None,
        bodies: vec![roots[0]],
    });
    t.doc
        .apply(Transaction::new(
            "部品",
            vec![Op::SetComponents { components: list }],
        ))
        .unwrap();
    t.recompute();
    let run = |t: &Bench, body: FeatureId| {
        let args = json!({"bodies": [body.0]});
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        AsmCommands.run(MOVE_PART, &cx).expect("部品を動かす操作")
    };
    let Ok(Outcome::Change { edits, .. }) = run(&t, boxed) else {
        panic!("箱の部品は値で動かせる");
    };
    let [jc_core::Edit::Insert { type_id, data, .. }] = edits.as_slice() else {
        panic!("{edits:?}");
    };
    assert_eq!(type_id, POSITION);
    let p: jc_asm::Position = serde_json::from_value(data.clone()).unwrap();
    assert_eq!(
        (p.x.as_str(), p.y.as_str(), p.z.as_str()),
        ("0 mm", "0 mm", "0 mm")
    );
    assert_eq!(p.bodies.len(), 3, "箱と、箱を土台にした蓋とつまみ");
    let err = run(&t, lid).expect_err("蓋はジョイントで動く");
    assert!(err.contains("ジョイント"), "{err}");
    let err = run(&t, roots[2]).expect_err("つまみは部品に入っていない");
    assert!(err.contains("部品に入っていない"), "{err}");
}

/// 同じ寸法の板 2 枚は部品表で 1 行・2 個にまとまり、材料があれば質量を出す。
#[test]
fn bom_counts_equal_boards_and_weighs_them() {
    let mut b = DocBuilder::new();
    let a = block(&mut b, "棚板", [0.0, 0.0, 300.0, 200.0], 18.0, 0.0);
    let c = block(&mut b, "棚板", [0.0, 0.0, 300.0, 200.0], 18.0, 100.0);
    let mut doc = b.finish("棚");
    let reg = registry();
    let k: Arc<dyn Kernel> = Arc::new(SmlibKernel::new());
    let report = doc.recompute(&reg, &*k);
    let roots: Vec<FeatureId> = [a, c]
        .iter()
        .map(|f| body_root(doc.data(), &reg, *f))
        .collect();
    let mut mats = jc_look::material::BodyMaterials::default();
    for r in &roots {
        mats.set(*r, Some("アルミ"));
    }
    let op = doc.data().section_op(&mats).unwrap();
    doc.apply(Transaction::new("材料", vec![op])).unwrap();
    let rows = bom_rows(&Context::new(doc.data(), &report, &reg, &k));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].count, 2);
    assert!(near(rows[0].size, [300.0, 200.0, 18.0]));
    let density = jc_look::material::phys_material("アルミ").unwrap().density;
    let g = rows[0].mass.unwrap();
    assert!(
        (g - 300.0 * 200.0 * 18.0 * density / 1000.0).abs() < 1.0,
        "{g}"
    );
}

/// 10-24 k11: 表に無い材料名(手で直したファイル)のボディは、どこでも「重さなし」でそろう。
/// 部品表はその名前のまま重さを出さず、属性の重さ(jc-session の `body_material` と状態の質量)が
/// 引く物理材料の表にも無い。解析の材料の表(jc-fea)は物理材料の表の部分なので、そこにも無い。
#[test]
fn an_unknown_material_name_gives_no_mass_anywhere() {
    let mut b = DocBuilder::new();
    let a = block(&mut b, "板", [0.0, 0.0, 30.0, 20.0], 5.0, 0.0);
    let mut doc = b.finish("知らない材料");
    let reg = registry();
    let k: Arc<dyn Kernel> = Arc::new(SmlibKernel::new());
    let report = doc.recompute(&reg, &*k);
    let root = body_root(doc.data(), &reg, a);
    let mut mats = jc_look::material::BodyMaterials::default();
    mats.set(root, Some("謎の合金"));
    let op = doc.data().section_op(&mats).unwrap();
    doc.apply(Transaction::new("材料", vec![op])).unwrap();
    let name = jc_look::material::material_of(doc.data(), root);
    assert_eq!(name, "謎の合金");
    assert!(jc_look::material::phys_material(&name).is_none());
    let rows = bom_rows(&Context::new(doc.data(), &report, &reg, &k));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].material, "謎の合金");
    assert_eq!(rows[0].mass, None, "{rows:?}");
}

/// 台(40×30×5)と、離れた所に置いた柱(4×4×10、x = 100)。柱は部品「柱」。
/// (台, ボディの最初の機能 [台, 柱], 形の最後の機能 [台, 柱])。
fn plate_and_post() -> (Bench, Vec<FeatureId>, [FeatureId; 2]) {
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "台", [0.0, 0.0, 40.0, 30.0], 5.0, 0.0);
    let post = block(&mut b, "柱", [100.0, 0.0, 4.0, 4.0], 10.0, 0.0);
    let doc = b.finish("組立");
    let mut t = Bench {
        doc,
        k: Arc::new(SmlibKernel::new()),
        report: Default::default(),
        reg: registry(),
    };
    t.recompute();
    let roots: Vec<FeatureId> = [plate, post]
        .iter()
        .map(|f| body_root(t.doc.data(), &t.reg, *f))
        .collect();
    let c = t.doc.alloc_group_id();
    t.doc
        .apply(Transaction::new(
            "部品",
            vec![Op::SetComponents {
                components: vec![Component {
                    id: c,
                    name: "柱".into(),
                    parent: None,
                    bodies: vec![roots[1]],
                }],
            }],
        ))
        .unwrap();
    t.recompute();
    (t, roots, [plate, post])
}

impl Bench {
    /// ボディ `feature` の、外向きが `n` の平らな面の名前。
    fn face_facing(&self, feature: FeatureId, n: [f64; 3]) -> String {
        let Some(jc_core::Value::Shape(h)) = self.cx().output(feature, "body") else {
            panic!("ボディが無い");
        };
        self.k
            .faces(*h)
            .unwrap()
            .into_iter()
            .find(|f| f.planar.is_some_and(|p| near(p.normal, n)))
            .expect("その向きの面")
            .name
    }

    /// 合わせるジョイントの原点の候補(面 `face` の上)。
    fn snap_choices(&self, key: &str, face: &FacePick) -> Vec<SnapPick> {
        let args = json!({"moving": "f"});
        let args2 = json!({"moving": "f", "base": "f"});
        let cx = Context {
            args: if key == "from" { &args } else { &args2 },
            picked: Some(face),
            ..self.cx()
        };
        let hs = AsmCommands.handles(NEW_SNAP_JOINT, &cx).unwrap();
        let h = hs.iter().find(|h| h.key == key).unwrap();
        h.choices
            .iter()
            .map(|c| serde_json::from_value(c.value.clone()).unwrap())
            .collect()
    }

    fn snap_joint(&mut self, from: &SnapPick, to: &SnapPick) -> Result<FeatureId, String> {
        let args = json!({"moving": "f", "from": from, "base": "f", "to": to});
        let cx = Context {
            args: &args,
            ..self.cx()
        };
        let (label, edits) = match AsmCommands.run(NEW_SNAP_JOINT, &cx) {
            Some(Ok(Outcome::Change { label, edits })) => (label, edits),
            Some(Err(e)) => return Err(e),
            _ => panic!("合わせるジョイントの操作が無い"),
        };
        let added = self.doc.apply_edits(&label, edits).unwrap();
        self.recompute();
        Ok(added[0])
    }
}

/// 採用分 10-24 m5: AI や台本が合わせるジョイントの原点に面をそのまま渡すと、面の上の原点の候補
/// (名前と、そのまま渡せる値)を並べて断る。読めない値なら、渡す形と取り方を示す。
#[test]
fn a_face_given_as_a_snap_origin_is_refused_with_the_origins_on_it() {
    let (t, _, [plate, post]) = plate_and_post();
    let top = FacePick {
        feature: plate,
        face: t.face_facing(plate, [0.0, 0.0, 1.0]),
    };
    let to = t.snap_choices("to", &top);
    let run = |from: serde_json::Value| {
        let args = json!({"moving": "f", "from": from, "base": "f", "to": to[0]});
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        match AsmCommands.run(NEW_SNAP_JOINT, &cx) {
            Some(Err(e)) => e,
            _ => panic!("断る"),
        }
    };
    let bottom = t.face_facing(post, [0.0, 0.0, -1.0]);
    let e = run(json!({"feature": post.0, "face": bottom}));
    assert!(
        e.contains("部品 1の原点に面") && e.contains("面の中心: {"),
        "{e}"
    );
    assert!(e.contains("頂点") && e.contains("\"frame\""), "{e}");
    let e = run(json!(12));
    assert!(e.contains("円の中心") && e.contains("\"frame\""), "{e}");
}

/// 合わせるジョイント: 柱の下の面の中心を、台の上の面の中心に合わせると、
/// 柱が台の真ん中に立つ(Z は向かい合わせ)。候補には 3 つの取り方(単純・2 面の間・2 辺の交点)
/// が出る。ずらし・角度・反転で合わせ方を直せ、種類を変えると動きが 7 種のとおりになる。
#[test]
fn snap_joint_puts_the_post_on_the_plate_and_kinds_move_it() {
    let (mut t, roots, [plate, post]) = plate_and_post();
    let top = FacePick {
        feature: plate,
        face: t.face_facing(plate, [0.0, 0.0, 1.0]),
    };
    let bottom = FacePick {
        feature: post,
        face: t.face_facing(post, [0.0, 0.0, -1.0]),
    };
    let to = t.snap_choices("to", &top);
    let labels: Vec<&str> = to.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels[0], "面の中心");
    assert!(
        labels.contains(&"頂点") && labels.contains(&"辺の中点"),
        "{labels:?}"
    );
    let mid = to
        .iter()
        .find(|c| c.label.starts_with("2 面の間"))
        .expect("2 面の間");
    assert!(near(mid.frame.origin, [20.0, 15.0, 2.5]), "{mid:?}");
    assert!(near(to[0].frame.origin, [20.0, 15.0, 5.0]));
    assert!(near(to[0].frame.z, [0.0, 0.0, 1.0]));
    let from = t.snap_choices("from", &bottom);
    assert!(near(from[0].frame.origin, [102.0, 2.0, 0.0]));
    assert!(near(from[0].frame.z, [0.0, 0.0, -1.0]));
    // 同じ部品どうしは断る
    assert!(t.snap_joint(&from[0], &from[1]).is_err());
    let j = t.snap_joint(&from[0], &to[0]).unwrap();
    let b = t.bounds(roots[1]);
    assert!(
        near(b.0, [18.0, 13.0, 5.0]) && near(b.1, [22.0, 17.0, 15.0]),
        "柱は台の真ん中に立つ: {b:?}"
    );
    assert!(
        near(t.bounds(roots[0]).1, [40.0, 30.0, 5.0]),
        "台は動かない"
    );
    // ずらし(Z は面から離す)・反転(同じ向き: 台にめり込む)
    t.set_joint(j, |j| {
        j.offset_x = "5 mm".into();
        j.offset_z = "1 mm".into();
    });
    let b = t.bounds(roots[1]);
    assert!(near(b.0, [23.0, 13.0, 6.0]), "{b:?}");
    t.set_joint(j, |j| {
        j.offset_x = "0 mm".into();
        j.offset_z = "0 mm".into();
        j.align_flip = true;
    });
    let b = t.bounds(roots[1]);
    assert!(
        near(b.0, [18.0, 13.0, -5.0]) && near(b.1, [22.0, 17.0, 5.0]),
        "{b:?}"
    );
    t.set_joint(j, |j| j.align_flip = false);
    // 種類: 円柱は回って軸(台の上の面の Z)の向きにも動く
    t.set_joint(j, |j| {
        j.kind = JointKind::Cylindrical;
        j.distance = "3 mm".into();
        j.angle = "45 deg".into();
    });
    let b = t.bounds(roots[1]);
    let half = 2.0 * 2f64.sqrt();
    assert!(
        near(b.0, [20.0 - half, 15.0 - half, 8.0]) && near(b.1, [20.0 + half, 15.0 + half, 18.0]),
        "{b:?}"
    );
    // 平面: X・Y に滑り、軸のまわりに回る。引くと X・Y でカーソルに付いてくる(回さない)
    t.set_joint(j, |j| {
        j.kind = JointKind::Planar;
        j.angle = "0 deg".into();
        j.slide_x = "4 mm".into();
        j.slide_y = "-2 mm".into();
    });
    let b = t.bounds(roots[1]);
    let (_, x, y) = jc_asm::joint::basis([0.0, 0.0, 1.0], t.joint_data(j).x_dir);
    let c = [
        20.0 + 4.0 * x[0] - 2.0 * y[0],
        15.0 + 4.0 * x[1] - 2.0 * y[1],
        10.0,
    ];
    assert!(near(center(b), c), "{b:?} {c:?}");
    let d = AsmCommands.drag(&t.cx(), &bottom).expect("引ける");
    assert_eq!(d.motion, jc_core::DragMotion::Follow);
    let fields: Vec<&str> = d.dofs.iter().map(|f| f.field.as_str()).collect();
    assert_eq!(fields, ["angle", "slide_x", "slide_y"]);
    assert_eq!(d.still, ["angle"]);
    // ピンスロット: X だけに動く。距離の範囲の外は端に寄せて知らせる
    t.set_joint(j, |j| {
        j.kind = JointKind::PinSlot;
        j.slide_x_max = "2 mm".into();
    });
    let b = t.bounds(roots[1]);
    assert!(
        near(center(b), [20.0 + 2.0 * x[0], 15.0 + 2.0 * x[1], 10.0]),
        "{b:?}"
    );
    let warned = format!("{:?}", t.report.feature(j).unwrap());
    assert!(warned.contains("X 距離の値が上限より大きい"), "{warned}");
    // ボール: 傾けると柱が倒れる(原点 = 柱の下の面の中心のまわり)
    t.set_joint(j, |j| {
        j.kind = JointKind::Ball;
        j.tilt = "90 deg".into();
    });
    let b = t.bounds(roots[1]);
    assert!(
        (b.1[2] - b.0[2] - 4.0).abs() < 1e-3,
        "倒れて高さは柱の幅: {b:?}"
    );
    let d = jc_interchange::animation::drive_of(&t.report, j).expect("ボールは角度で動かす");
    assert!(!d.slide && near(d.origin, [20.0, 15.0, 5.0]));
}

/// 土台が前のジョイントで動いていれば、合わせる原点は動かす前の位置に直して覚え、部品 1 は土台に
/// ついて動く。ジョイントで動く部品は、もう一度合わせて動かせない(動きがぶつかるのを防ぐ)。
#[test]
fn snap_joint_follows_a_moved_base_and_refuses_a_moved_part() {
    let (mut t, roots, [plate, post]) = plate_and_post();
    // 台を手前の下の辺(y = 0、z = 0)のまわりに地面へつなぎ、90 度起こす(上の面は -Y を向く)
    let hinge = t.joint(
        Origin {
            feature: plate,
            point: [0.0, 0.0, 0.0],
            axis: [1.0, 0.0, 0.0],
        },
        None,
    );
    t.set_joint(hinge, |j| j.angle = "90 deg".into());
    let top = FacePick {
        feature: plate,
        face: t.face_facing(plate, [0.0, -1.0, 0.0]),
    };
    let bottom = FacePick {
        feature: post,
        face: t.face_facing(post, [0.0, 0.0, -1.0]),
    };
    let to = t.snap_choices("to", &top);
    assert!(near(to[0].frame.origin, [20.0, -5.0, 15.0]), "{:?}", to[0]);
    let from = t.snap_choices("from", &bottom);
    let j = t.snap_joint(&from[0], &to[0]).unwrap();
    let b = t.bounds(roots[1]);
    assert!(
        near(b.0, [18.0, -15.0, 13.0]) && near(b.1, [22.0, -5.0, 17.0]),
        "起こした台の面から横に出る: {b:?}"
    );
    t.set_joint(hinge, |j| j.angle = "0 deg".into());
    let b = t.bounds(roots[1]);
    assert!(
        near(b.0, [18.0, 13.0, 5.0]) && near(b.1, [22.0, 17.0, 15.0]),
        "台を戻すと真ん中に立つ: {b:?}"
    );
    assert_eq!(t.joint_data(j).follows, vec![hinge]);
    // 台はジョイント1 で動くので、柱に合わせて動かせない
    let top = FacePick {
        feature: plate,
        face: t.face_facing(plate, [0.0, 0.0, 1.0]),
    };
    let from = t.snap_choices("from", &top);
    let to = t.snap_choices("to", &bottom);
    let e = t.snap_joint(&from[0], &to[0]).unwrap_err();
    assert!(e.contains("ジョイント1"), "{e}");
}

/// 10-22 k5: ジョイントで動かした柱で後から台を切り取ると、動かした後の(見えている)所が削れる。
/// 柱を反転で台の真ん中に半分(深さ 5 のうち 5)めり込ませ、柱を道具に台を切り取る。元の位置
/// (x = 100)は台に当たらないので、動かす前の形を使えば何も削れず止まる。
#[test]
fn a_cut_by_a_part_moved_by_a_joint_removes_where_it_is_shown() {
    let (mut t, roots, [plate, post]) = plate_and_post();
    let top = FacePick {
        feature: plate,
        face: t.face_facing(plate, [0.0, 0.0, 1.0]),
    };
    let bottom = FacePick {
        feature: post,
        face: t.face_facing(post, [0.0, 0.0, -1.0]),
    };
    let to = t.snap_choices("to", &top);
    let from = t.snap_choices("from", &bottom);
    let j = t.snap_joint(&from[0], &to[0]).unwrap();
    t.set_joint(j, |j| j.align_flip = true);
    let b = t.bounds(roots[1]);
    assert!(near(b.0, [18.0, 13.0, -5.0]), "{b:?}");
    let id = t.doc.alloc_feature_id();
    let cut = BooleanFeature {
        target: plate,
        tool: vec![post],
        operation: Operation::Cut,
        keep_tools: false,
    };
    t.doc
        .apply(Transaction::new(
            "切り取り",
            vec![Op::InsertFeature {
                index: None,
                entry: jc_core::FeatureEntry {
                    id,
                    type_id: BOOLEAN.into(),
                    name: "柱で切る".into(),
                    data: serde_json::to_value(cut).unwrap(),
                    suppressed: false,
                },
            }],
        ))
        .unwrap();
    t.recompute();
    let data = t.doc.data();
    let (_, h) = visible_bodies(data, &t.reg, &t.report)
        .into_iter()
        .find(|(id, _)| body_root(data, &t.reg, *id) == roots[0])
        .expect("台");
    let v = t.k.volume(h).unwrap();
    assert!(
        (v - (40.0 * 30.0 * 5.0 - 4.0 * 4.0 * 5.0)).abs() < 1e-6,
        "{v}"
    );
    // 削れたのは真ん中(柱の今の所)。外形は台のまま
    let (lo, hi) = t.k.bounds(h).unwrap();
    assert!(near(lo, [0.0, 0.0, 0.0]) && near(hi, [40.0, 30.0, 5.0]));
}

/// ジョイントの一覧の表は、ジョイントごとに何と何をつないでいるかを名前で
/// 出す(MCP の AI は run_action で読む)。蓋は部品「蓋」で土台は箱のボディ、つまみは蓋が土台。
#[test]
fn the_joint_table_says_what_each_joint_connects() {
    let (mut t, _roots, hinge, spin, _) = lid_and_knob();
    t.set_joint(hinge, |j| {
        j.angle = "-30 deg".into();
        j.angle_min = "-90 deg".into();
        j.angle_rest = "0 deg".into();
    });
    // 止めたジョイントも表に出す(AI が組立の決まりを読み落とさない)
    t.doc
        .apply(Transaction::new(
            "止める",
            vec![Op::SetSuppressed {
                id: spin,
                suppressed: true,
            }],
        ))
        .unwrap();
    // 表は文書の印を読む(止めた機能は再計算で「止めた」になるので、計算し直さずに読む)

    // 画面・CLI・MCP と同じ操作の入口で読む
    let Some(Ok(Outcome::Table { table, .. })) = AsmCommands.run(jc_asm::JOINTS, &t.cx()) else {
        panic!("ジョイントの一覧が表にならない")
    };
    assert_eq!(table.rows.len(), 2, "{table:?}");
    let col = |id: &str| table.columns.iter().position(|c| c.id == id).unwrap();
    let row = |f: FeatureId| {
        table
            .rows
            .iter()
            .find(|r| r[col("id")] == f.0.to_string())
            .unwrap_or_else(|| panic!("{f:?} の行が無い: {table:?}"))
    };
    let h = row(hinge);
    assert_eq!(h[col("kind")], "回転");
    assert_eq!(h[col("moving")], "蓋");
    assert!(h[col("base")].contains("箱"), "{h:?}");
    assert!(
        h[col("bodies")].contains("つまみ"),
        "蓋と一緒につまみも動く: {h:?}"
    );
    assert_eq!(h[col("value")], "角度 -30 deg");
    assert_eq!(h[col("limits")], "角度 -90 deg〜制限なし");
    assert_eq!(h[col("rest")], "角度 0 deg");
    assert_eq!(h[col("suppressed")], "");
    assert_eq!(h[col("axis")], "1.000, 0.000, 0.000");
    let s = row(spin);
    assert!(s[col("moving")].contains("つまみ"), "{s:?}");
    assert_eq!(s[col("base")], "蓋");
    assert_eq!(s[col("suppressed")], "止めている");
}

/// 干渉で、足し引きできない形(STEP から読み込んだ形だけのボディ)との組は
/// 「干渉なし」にせず、「調べていません」と理由を表に出す。
#[test]
fn interference_checks_a_step_body_that_smlib_reads_as_a_solid() {
    // SMLib は STEP を立体に組む(M56)ので、読み込んだ形も干渉を調べる(重なり 15 × 20 × 10)
    let mut b = DocBuilder::new();
    block(&mut b, "箱", [0.0, 0.0, 30.0, 20.0], 10.0, 0.0);
    let k: Arc<dyn Kernel> =
        Arc::new(SmlibKernel::new().with_step_import(jc_kernel_truck::TruckKernel::new()));
    let other = {
        let mut d = DocBuilder::new();
        block(&mut d, "元", [15.0, 0.0, 30.0, 20.0], 10.0, 0.0);
        let mut doc = d.finish("元");
        let reg = registry();
        let report = doc.recompute(&reg, &*k);
        let (_, h) = visible_bodies(doc.data(), &reg, &report)[0];
        k.export_step(&[h]).unwrap()
    };
    b.feature(
        crate::IMPORT_STEP,
        "読み込み",
        crate::ImportStep {
            file: "元.step".into(),
            step: other,
            stand_up: false,
            upright: false,
        },
    );
    let mut t = Bench {
        doc: b.finish("干渉"),
        k,
        report: Default::default(),
        reg: registry(),
    };
    t.recompute();
    let (clashes, unchecked) = interferences(&t.cx());
    assert!(unchecked.is_empty(), "{unchecked:?}");
    assert_eq!(clashes.len(), 1, "{clashes:?}");
    assert!(
        (clashes[0].volume - 3000.0).abs() < 1e-6,
        "{}",
        clashes[0].volume
    );
}

#[test]
fn interference_lists_a_shape_only_body_as_unchecked_with_the_reason() {
    let mut b = DocBuilder::new();
    let block_id = block(&mut b, "箱", [0.0, 0.0, 30.0, 20.0], 10.0, 0.0);
    // 箱と半分重なる形を STEP にして読み込む。SMLib は STEP を立体に組む(M56)ので、網で読む
    // truck のカーネルで、形だけのボディ(足し引きできない)を作る
    let k: Arc<dyn Kernel> = Arc::new(jc_kernel_truck::TruckKernel::new());
    let other = {
        let mut d = DocBuilder::new();
        block(&mut d, "元", [15.0, 0.0, 30.0, 20.0], 10.0, 0.0);
        let mut doc = d.finish("元");
        let reg = registry();
        let report = doc.recompute(&reg, &*k);
        let (_, h) = visible_bodies(doc.data(), &reg, &report)[0];
        k.export_step(&[h]).unwrap()
    };
    b.feature(
        crate::IMPORT_STEP,
        "読み込み",
        crate::ImportStep {
            file: "元.step".into(),
            step: other,
            stand_up: false,
            upright: false,
        },
    );
    let _ = block_id;
    let mut t = Bench {
        doc: b.finish("干渉"),
        k,
        report: Default::default(),
        reg: registry(),
    };
    t.recompute();
    let (clashes, unchecked) = interferences(&t.cx());
    assert!(clashes.is_empty(), "{clashes:?}");
    assert_eq!(unchecked.len(), 1, "{unchecked:?}");
    assert!(unchecked[0].reason.contains("STEP"), "{unchecked:?}");
    let Some(Ok(Outcome::Table { table, .. })) = AsmCommands.run(INTERFERENCE, &t.cx()) else {
        panic!()
    };
    assert!(table.note.contains("調べていません"), "{}", table.note);
}

/// 拘束の相手に角の点・まっすぐな辺・原点・原点の平面も選べる(採用分 10-20 a3・10-22 a3)。
#[test]
fn a_constraint_takes_corners_edges_and_the_origin() {
    use jc_core::Ref;
    let (mut t, roots, [plate, post]) = plate_and_post();
    let face = |t: &Bench, f: FeatureId, n: [f64; 3]| FacePick {
        feature: f,
        face: t.face_facing(f, n),
    };
    let bottom = face(&t, post, [0.0, 0.0, -1.0]);
    let top = face(&t, plate, [0.0, 0.0, 1.0]);
    let row = |a: Ref, b: Option<Ref>, to: &str| jc_asm::constraint::Row {
        kind: "align".into(),
        a: vec![a],
        b: b.into_iter().collect(),
        offset: String::new(),
        angle: String::new(),
        flip: false,
        b_to: to.into(),
    };
    // 柱の下の面を台の上に向かい合わせ、柱の左手前の下の角を台の右奥の上の角に重ねる
    let c = t
        .constrain(&[bottom.clone(), top.clone()], json!({}))
        .unwrap();
    let a_feature = t.constraint_data(c).rows[0].a[0].feature();
    let corner = |t: &Bench, f: FeatureId, at: [f64; 3]| -> Ref {
        let Some(jc_core::Value::Shape(h)) = t.cx().output(f, "body") else {
            panic!("ボディが無い");
        };
        let all = jc_core::vertices(&*t.k, *h);
        let v = all
            .iter()
            .find(|v| near(v.at, at))
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "その位置の角 {at:?}: {:?}",
                    all.iter().map(|v| v.at).collect::<Vec<_>>()
                )
            });
        Ref::Vertex {
            feature: f,
            vertex: v.name,
        }
    };
    // 名前は動かす前と同じなので、今の位置(台の上に立った柱)の左手前の下の角で探す
    let post_corner = corner(&t, a_feature, t.bounds(roots[1]).0);
    let plate_corner = corner(&t, plate, [40.0, 30.0, 5.0]);
    // 回りを止める角度の行(柱の左の面を台の左の面と同じ向きに)。点と面だけでは縦の軸まわりに回れる
    let turn = jc_asm::constraint::Row {
        kind: "angle".into(),
        ..row(
            Ref::from(FacePick {
                feature: a_feature,
                face: t.face_facing(post, [-1.0, 0.0, 0.0]),
            }),
            Some(Ref::from(face(&t, plate, [-1.0, 0.0, 0.0]))),
            "part",
        )
    };
    t.set_constraint(c, |c| {
        c.rows
            .push(row(post_corner.clone(), Some(plate_corner), "part"));
        c.rows.push(turn.clone());
    });
    assert_ok(&t.report);
    let b = t.bounds(roots[1]);
    assert!(
        near(b.0, [40.0, 30.0, 5.0]) && near(b.1, [44.0, 34.0, 15.0]),
        "角に重なる: {b:?}"
    );
    // 角の代わりに辺: 柱の左手前の縦の辺を、台の左手前の縦の辺と一直線に
    let edge = |t: &Bench, f: FeatureId, x: f64, y: f64| -> Ref {
        let Some(jc_core::Value::Shape(h)) = t.cx().output(f, "body") else {
            panic!("ボディが無い");
        };
        let e = t
            .k
            .edges(*h)
            .unwrap()
            .into_iter()
            .find(|e| {
                near([e.a[0], e.a[1], 0.0], [x, y, 0.0]) && near([e.b[0], e.b[1], 0.0], [x, y, 0.0])
            })
            .expect("その縦の辺");
        Ref::Edge {
            feature: f,
            faces: e.faces,
        }
    };
    let lo = t.bounds(roots[1]).0;
    let post_edge = edge(&t, a_feature, lo[0], lo[1]);
    let plate_edge = edge(&t, plate, 0.0, 0.0);
    t.set_constraint(c, |c| c.rows[1] = row(post_edge, Some(plate_edge), "part"));
    assert_ok(&t.report);
    let b = t.bounds(roots[1]);
    assert!(
        near(b.0, [0.0, 0.0, 5.0]) && near(b.1, [4.0, 4.0, 15.0]),
        "辺が一直線: {b:?}"
    );
    // 原点と原点の平面: 下の面を XY 平面に、角を原点に
    t.set_constraint(c, |c| {
        c.rows = vec![
            row(c.rows[0].a[0].clone(), None, "origin_xy"),
            row(post_corner.clone(), None, "origin"),
            turn.clone(),
        ];
    });
    assert_ok(&t.report);
    let b = t.bounds(roots[1]);
    assert!(
        near(b.0, [0.0, 0.0, 0.0]) && near(b.1, [4.0, 4.0, 10.0]),
        "原点に: {b:?}"
    );
    // 点に向きは無いので角度の行には使えない。原点にしたのに部品 2 の形も残す行は断る
    t.set_constraint(c, |c| {
        c.rows[1].kind = "angle".into();
    });
    let msg = format!("{:?}", t.report.feature(c).unwrap());
    assert!(msg.contains("点ではなく面か辺"), "{msg}");
    t.set_constraint(c, |c| {
        c.rows[1].kind = "align".into();
        c.rows[1].b = vec![Ref::from(top.clone())];
    });
    let msg = format!("{:?}", t.report.feature(c).unwrap());
    assert!(msg.contains("外してください"), "{msg}");
    // 操作でも原点の平面を相手にできる(部品 2 の面は渡さない)
    let args = json!({"a": Ref::from(bottom.clone()), "b_to": "origin_zx"});
    let cx = Context {
        args: &args,
        ..t.cx()
    };
    assert!(matches!(
        AsmCommands.run(jc_asm::constraint::NEW_CONSTRAINT, &cx),
        Some(Ok(Outcome::Change { .. }))
    ));
}

impl Bench {
    /// 選んだ面(`faces` の先が部品 1、後が部品 2)と引数で拘束を作る。
    fn constrain(
        &mut self,
        faces: &[FacePick],
        args: serde_json::Value,
    ) -> Result<FeatureId, String> {
        let (picked, also) = faces.split_last().expect("面");
        let cx = Context {
            args: &args,
            picked: Some(picked),
            also_picked: also,
            ..self.cx()
        };
        let (label, edits) = match AsmCommands.run(jc_asm::constraint::NEW_CONSTRAINT, &cx) {
            Some(Ok(Outcome::Change { label, edits })) => (label, edits),
            Some(Err(e)) => return Err(e),
            _ => panic!("拘束の操作が無い"),
        };
        let added = self.doc.apply_edits(&label, edits).unwrap();
        self.report = self.doc.recompute(&self.reg, &*self.k);
        Ok(added[0])
    }

    fn constraint_data(&self, id: FeatureId) -> jc_asm::constraint::Constraint {
        serde_json::from_value(self.doc.data().feature(id).unwrap().data.clone()).unwrap()
    }

    fn set_constraint(
        &mut self,
        id: FeatureId,
        f: impl FnOnce(&mut jc_asm::constraint::Constraint),
    ) {
        let mut c = self.constraint_data(id);
        f(&mut c);
        self.doc
            .apply(Transaction::new(
                "拘束の値",
                vec![Op::SetFeatureData {
                    id,
                    data: serde_json::to_value(c).unwrap(),
                }],
            ))
            .unwrap();
        self.report = self.doc.recompute(&self.reg, &*self.k);
    }
}

/// 拘束: 柱の下の面を台の上の面に向かい合わせ、柱の左の面と手前の面を台の左と手前の
/// 面に面一にする 3 行を 1 つの拘束で同時に解くと、柱は台の左手前の角に立つ。オフセットで離れ、
/// 角度・中央の行も同じ表で解ける。満たせない組は、いちばん合わない行を誤りに出す。
#[test]
fn a_constraint_solves_its_rows_together_and_moves_the_post() {
    let (mut t, roots, [plate, post]) = plate_and_post();
    let face = |t: &Bench, f: FeatureId, n: [f64; 3]| FacePick {
        feature: f,
        face: t.face_facing(f, n),
    };
    let (top, bottom) = (
        face(&t, plate, [0.0, 0.0, 1.0]),
        face(&t, post, [0.0, 0.0, -1.0]),
    );
    // 同じ部品の面どうしは断る
    let side = face(&t, post, [-1.0, 0.0, 0.0]);
    assert!(
        t.constrain(&[bottom.clone(), side.clone()], json!({}))
            .is_err()
    );
    // 拘束の前は柱を引いて動かせる。拘束で置いた後は、引いても動かさずに理由を返す(F1)
    let drag = |t: &Bench| {
        let args = json!({"body": post.0, "offset": [5.0, 0.0, 0.0]});
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        AsmCommands.run(DRAG_POSITION, &cx).expect("引く操作")
    };
    assert!(drag(&t).is_ok());
    let c = t
        .constrain(&[bottom.clone(), top.clone()], json!({}))
        .unwrap();
    assert_ok(&t.report);
    let e = drag(&t).expect_err("拘束で置いた部品は引かない");
    assert!(e.contains("拘束で置いた部品"), "{e}");
    let b = t.bounds(roots[1]);
    assert!(
        (b.0[2] - 5.0).abs() < 1e-3 && (b.1[2] - 15.0).abs() < 1e-3,
        "台の上に立つ: {b:?}"
    );
    assert!(
        near(t.bounds(roots[0]).1, [40.0, 30.0, 5.0]),
        "台は動かない"
    );
    // 行を足す: 左の面と手前の面を面一に(同時に解く)
    let row = |a: &FacePick, b: &FacePick, flip: bool, offset: &str| jc_asm::constraint::Row {
        kind: "align".into(),
        a: vec![jc_core::Ref::from(a.clone())],
        b: vec![jc_core::Ref::from(b.clone())],
        offset: offset.into(),
        angle: String::new(),
        flip,
        b_to: "part".into(),
    };
    let (pl, pf) = (
        face(&t, plate, [-1.0, 0.0, 0.0]),
        face(&t, plate, [0.0, -1.0, 0.0]),
    );
    let (sl, sf) = (
        face(&t, post, [-1.0, 0.0, 0.0]),
        face(&t, post, [0.0, -1.0, 0.0]),
    );
    let first = t.constraint_data(c).rows[0].clone();
    let fix = |p: &FacePick| FacePick {
        feature: first.a[0].feature(),
        face: p.face.clone(),
    };
    t.set_constraint(c, |c| {
        c.rows.push(row(&fix(&sl), &pl, true, ""));
        c.rows.push(row(&fix(&sf), &pf, true, "3 mm"));
    });
    assert_ok(&t.report);
    let b = t.bounds(roots[1]);
    assert!(
        near(b.0, [0.0, -3.0, 5.0]) && near(b.1, [4.0, 1.0, 15.0]),
        "左手前の角(オフセットは台の面の外向きに 3 mm): {b:?}"
    );
    // 向かい合わせのオフセット: 台の上から 2 mm 浮く
    t.set_constraint(c, |c| c.rows[0].offset = "2 mm".into());
    assert!((t.bounds(roots[1]).0[2] - 7.0).abs() < 1e-3);
    // 中央: 柱の左右の面の真ん中を、台の左右の面の真ん中に(手前の行はそのまま)
    let pr = face(&t, plate, [1.0, 0.0, 0.0]);
    let sr = face(&t, post, [1.0, 0.0, 0.0]);
    t.set_constraint(c, |c| {
        c.rows[1] = jc_asm::constraint::Row {
            kind: "center".into(),
            a: vec![fix(&sl).into(), fix(&sr).into()],
            b: vec![pl.clone().into(), pr.clone().into()],
            offset: String::new(),
            angle: String::new(),
            flip: false,
            b_to: "part".into(),
        };
    });
    assert_ok(&t.report);
    let b = t.bounds(roots[1]);
    assert!((center(b)[0] - 20.0).abs() < 1e-3, "左右の真ん中: {b:?}");
    // 満たせない組: 同じ下の面を、台の上の面と左の面の両方に向かい合わせる
    t.set_constraint(c, |c| {
        c.rows.truncate(1);
        c.rows.push(row(&fix(&bottom), &pl, false, ""));
    });
    let msg = format!("{:?}", t.report.feature(c).unwrap());
    assert!(
        msg.contains("拘束を全部満たす位置が見つかりません"),
        "{msg}"
    );
    // 面が無くなった行は付け替えずに止める
    t.set_constraint(c, |c| {
        c.rows.truncate(1);
        c.rows[0].b = vec![jc_core::Ref::Face {
            feature: plate,
            face: "f999:end".into(),
        }];
    });
    let msg = format!("{:?}", t.report.feature(c).unwrap());
    assert!(msg.contains("がボディにありません"), "{msg}");
    // 保存して読み直せる
    let back = Document::from_json(&t.doc.to_json()).unwrap();
    assert!(
        back.data()
            .features
            .iter()
            .any(|f| f.type_id == jc_asm::constraint::CONSTRAINT)
    );
}
