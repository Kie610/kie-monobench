//! 組立の動き: アニメーションの段の帯、モーション スタディ、接触セット、自動分解、モーション リンク、
//! 剛性グループ。試験の組立と道具は親(`asm_tests`)のものを使う。

use super::*;

/// ボディ `root` の動かす前の網に、段の帯の時刻 `time` の動きを掛けた物の外形の箱。
fn posed_bounds(t: &Bench, root: FeatureId, time: f64) -> ([f64; 3], [f64; 3]) {
    let data = t.doc.data();
    let (id, h) = visible_bodies(data, &t.reg, &t.report)
        .into_iter()
        .find(|(id, _)| body_root(data, &t.reg, *id) == root)
        .expect("見えているボディ");
    let mut mesh = t.k.mesh(h, 0.01).unwrap();
    if let Some(m) =
        animation::body_poses(data, &t.reg, &t.report, &animation::steps(data), time).get(&id)
    {
        mesh = animation::transform_mesh(&mesh, m);
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i] as f64);
            hi[i] = hi[i].max(p[i] as f64);
        }
    }
    (lo, hi)
}

fn near_mm(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 0.05)
}

/// アニメーションの段の帯: 蓋を開ける(ジョイントの動き)→ つまみを回す → つまみを上へ分解。
/// - 段を足す操作(分解の移動・ジョイントの動き)は、文書の段の帯の最後に足す
/// - 時刻ごとの動きを掛けた網は、ジョイントの値を本当に変えて計算し直した形と同じ所にある
/// - 分解は、蓋で動いた先からさらに平行に動かす。設計の形(再計算の結果)は変わらない
#[test]
fn storyboard_opens_the_lid_turns_the_knob_and_explodes_it() {
    let (mut t, roots, hinge, spin, _) = lid_and_knob();
    let lid0 = t.bounds(roots[1]);
    // 蓋のジョイントの動き(上下限が無いので今の値 0 から 90 度)。向きを逆に直して開く
    let args = json!({"joint": hinge.0});
    let cx = Context {
        args: &args,
        ..t.cx()
    };
    let Some(Ok(Outcome::Change { label, edits })) = AsmCommands.run(jc_asm::anim::DRIVE, &cx)
    else {
        panic!("ジョイントの動きの段を足せない")
    };
    t.doc.apply_edits(&label, edits).unwrap();
    let mut steps = animation::steps(t.doc.data());
    assert_eq!(steps.len(), 1);
    match &mut steps[0].kind {
        animation::StepKind::Drive { joint, from, to } => {
            assert_eq!((*joint, *from, *to), (hinge, 0.0, 90.0));
            *to = -90.0;
        }
        k => panic!("{k:?}"),
    }
    steps[0].seconds = 2.0;
    steps.push(animation::Step {
        kind: animation::StepKind::Drive {
            joint: spin,
            from: 0.0,
            to: 45.0,
        },
        seconds: 1.0,
        trail: false,
    });
    let op = animation::set_steps_op(t.doc.data(), steps.clone()).unwrap();
    t.doc.apply(Transaction::new("段", vec![op])).unwrap();
    // つまみを上へ 20 mm(つまみの面をクリック → Z の矢印 → 20)。時刻は 2 つの段の後
    let args = json!({"part": "f", "dir": [0.0, 0.0, 1.0], "distance": 20.0, "time": 3.0});
    let knob_face = visible_bodies(t.doc.data(), &t.reg, &t.report)
        .into_iter()
        .find(|(id, _)| body_root(t.doc.data(), &t.reg, *id) == roots[2])
        .unwrap()
        .0;
    let pick = FacePick {
        feature: knob_face,
        face: "f".into(),
    };
    let cx = Context {
        args: &args,
        picked: Some(&pick),
        ..t.cx()
    };
    let hs = jc_asm::anim::explode_handles(&cx);
    assert_eq!(hs[1].choices.len(), 6, "X・Y・Z の矢印の向き 6 つ");
    let Some(Ok(Outcome::Change { label, edits })) = AsmCommands.run(jc_asm::anim::EXPLODE, &cx)
    else {
        panic!("分解の段を足せない")
    };
    t.doc.apply_edits(&label, edits).unwrap();
    assert_eq!(animation::steps(t.doc.data()).len(), 3);
    assert_eq!(animation::duration(&animation::steps(t.doc.data())), 4.0);
    t.recompute();
    // 初め: 動かない
    assert!(near_mm(posed_bounds(&t, roots[1], 0.0).0, lid0.0));
    // 1 秒(蓋の段の真ん中): 45 度だけ開いた蓋は、値を -45 度にして計算し直した形と同じ
    let half = posed_bounds(&t, roots[1], 1.0);
    let knob_half = posed_bounds(&t, roots[2], 1.0);
    // 2 秒: 開き切る。つまみも一緒に動く
    let open = posed_bounds(&t, roots[1], 2.0);
    let knob_open = posed_bounds(&t, roots[2], 2.0);
    let knob_end = posed_bounds(&t, roots[2], 4.0);
    let box_end = posed_bounds(&t, roots[0], 4.0);
    t.set_joint(hinge, |j| j.angle = "-45 deg".into());
    assert!(near_mm(half.0, t.bounds(roots[1]).0) && near_mm(half.1, t.bounds(roots[1]).1));
    assert!(near_mm(knob_half.0, t.bounds(roots[2]).0), "{knob_half:?}");
    t.set_joint(hinge, |j| j.angle = "-90 deg".into());
    assert!(
        near_mm(open.0, [0.0, 18.0, 12.0]) && near_mm(open.1, [30.0, 20.0, 32.0]),
        "{open:?}"
    );
    assert!(near_mm(knob_open.0, t.bounds(roots[2]).0), "{knob_open:?}");
    // 終わり: つまみは蓋の上で 45 度回り(対角の幅)、さらに上へ 20 mm
    t.set_joint(spin, |j| j.angle = "45 deg".into());
    let k = t.bounds(roots[2]);
    assert!(
        near_mm(knob_end.0, [k.0[0], k.0[1], k.0[2] + 20.0])
            && near_mm(knob_end.1, [k.1[0], k.1[1], k.1[2] + 20.0]),
        "{knob_end:?} {k:?}"
    );
    assert!(near_mm(box_end.1, [30.0, 20.0, 10.0]), "箱は動かない");
    // 段は表示だけ: 値を戻すと設計の形は元のまま
    t.set_joint(spin, |j| j.angle = "0 deg".into());
    t.set_joint(hinge, |j| j.angle = "0 deg".into());
    assert!(near(t.bounds(roots[1]).0, lid0.0));
}

/// 表示しているボディ `root` の、モーション スタディをステップ `s` にした時の囲み。
fn study_bounds(t: &Bench, root: FeatureId, s: f64) -> ([f64; 3], [f64; 3]) {
    let data = t.doc.data();
    let (id, h) = visible_bodies(data, &t.reg, &t.report)
        .into_iter()
        .find(|(id, _)| body_root(data, &t.reg, *id) == root)
        .expect("見えているボディ");
    let mut mesh = t.k.mesh(h, 0.01).unwrap();
    let study = &motion::studies(data)[0];
    if let Some(m) = motion::body_poses(data, &t.reg, &t.report, study, s).get(&id) {
        mesh = animation::transform_mesh(&mesh, m);
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i] as f64);
            hi[i] = hi[i].max(p[i] as f64);
        }
    }
    (lo, hi)
}

/// モーション スタディ(#18): 操作でキーを置くと文書にスタディができ、2 つのジョイントが
/// キーの間を直線で一緒に動く。動いた網は、ジョイントの値を本当に変えて計算し直した形と同じ所に
/// ある。設計の形は変えない。キーを消す・外す・名前と長さを変える・消すも同じ操作。
#[test]
fn motion_study_moves_two_joints_together_from_its_keys() {
    let (mut t, roots, hinge, spin, _) = lid_and_knob();
    let lid0 = t.bounds(roots[1]);
    let run = |t: &mut Bench, args: serde_json::Value| -> Result<(), String> {
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        match AsmCommands
            .run(jc_asm::motion::MOTION_STUDY, &cx)
            .expect("ある操作")
        {
            Ok(Outcome::Change { label, edits }) => t
                .doc
                .apply_edits(&label, edits)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Ok(o) => Err(format!("{o:?}")),
            Err(e) => Err(e),
        }
    };
    // 引数なし: 空のスタディを作る
    run(&mut t, json!({})).unwrap();
    assert_eq!(
        motion::studies(t.doc.data())[0].name,
        "モーション スタディ1"
    );
    // 蓋: 0 で 0°、50 で -90°(開く)。つまみ: 50 で 0°、100 で 90°
    run(&mut t, json!({"joint": hinge.0, "step": 0, "value": 0.0})).unwrap();
    run(
        &mut t,
        json!({"joint": hinge.0, "step": 50, "value": -90.0}),
    )
    .unwrap();
    run(&mut t, json!({"joint": spin.0, "step": 50, "value": 0.0})).unwrap();
    run(&mut t, json!({"joint": spin.0, "step": 100, "value": 90.0})).unwrap();
    let st = &motion::studies(t.doc.data())[0];
    assert_eq!(st.tracks.len(), 2);
    t.recompute();
    assert!(
        near_mm(study_bounds(&t, roots[1], 0.0).0, lid0.0),
        "初めは動かない"
    );
    let half = study_bounds(&t, roots[1], 25.0);
    let knob_quarter = study_bounds(&t, roots[2], 75.0);
    t.set_joint(hinge, |j| j.angle = "-45 deg".into());
    assert!(near_mm(half.0, t.bounds(roots[1]).0) && near_mm(half.1, t.bounds(roots[1]).1));
    t.set_joint(hinge, |j| j.angle = "-90 deg".into());
    t.set_joint(spin, |j| j.angle = "45 deg".into());
    let k = t.bounds(roots[2]);
    assert!(
        near_mm(knob_quarter.0, k.0) && near_mm(knob_quarter.1, k.1),
        "{knob_quarter:?} {k:?}"
    );
    t.set_joint(spin, |j| j.angle = "0 deg".into());
    t.set_joint(hinge, |j| j.angle = "0 deg".into());
    assert!(near_mm(t.bounds(roots[1]).0, lid0.0), "形は変えない");
    // ステップの範囲の外・キーの無い所を消す・ジョイントでない物は断る
    assert!(run(&mut t, json!({"joint": hinge.0, "step": 101})).is_err());
    assert!(
        run(
            &mut t,
            json!({"joint": hinge.0, "step": 10, "remove": true})
        )
        .is_err()
    );
    assert!(run(&mut t, json!({"joint": roots[0].0, "step": 10})).is_err());
    // 消す・外す・名前と長さ
    run(
        &mut t,
        json!({"joint": hinge.0, "step": 50, "remove": true}),
    )
    .unwrap();
    run(&mut t, json!({"joint": spin.0, "drop": true})).unwrap();
    run(&mut t, json!({"rename": "蓋を開ける", "seconds": 2.0})).unwrap();
    let st = &motion::studies(t.doc.data())[0];
    assert_eq!(
        (st.name.as_str(), st.seconds, st.tracks.len()),
        ("蓋を開ける", 2.0, 1)
    );
    assert_eq!(st.tracks[0].keys.len(), 1);
    // 保存して読み直せ、取り消せる
    let back = Document::from_json(&t.doc.to_json()).unwrap();
    assert_eq!(motion::studies(back.data()), motion::studies(t.doc.data()));
    run(&mut t, json!({"study": "蓋を開ける", "delete": true})).unwrap();
    assert!(motion::studies(t.doc.data()).is_empty());
    t.doc.undo();
    assert_eq!(motion::studies(t.doc.data()).len(), 1);
}

/// 接触セット(#18): 箱と蓋とつまみを 1 つのセットにして有効にすると、蓋を箱へめり込む向きに
/// 引いても 0 度(載っているだけ)で止まり、開く向きには止まらない。つまみは蓋の上で回しても
/// 止まらない(面が同じ平面で触れているだけ)。セットの目や
/// 全体を切ると止めない。
#[test]
fn contact_sets_stop_a_dragged_joint_at_the_touching_value() {
    let (mut t, roots, hinge, spin, _) = lid_and_knob();
    let drag = |joint: FeatureId, motion: jc_core::DragMotion| jc_core::Drag {
        feature: joint,
        field: "angle".into(),
        motion,
        label: "引く".into(),
        dofs: Vec::new(),
        still: Vec::new(),
    };
    let turn = |axis| jc_core::DragMotion::Rotate {
        origin: [0.0; 3],
        axis,
    };
    let limit = |t: &Bench, joint, motion, from: f64, to: f64| {
        let args = json!({});
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        AsmCommands.drag_limit(&cx, &drag(joint, motion), from, to)
    };
    assert_eq!(
        limit(&t, hinge, turn([1.0, 0.0, 0.0]), 0.0, 20.0),
        None,
        "セットが無い"
    );
    // 選んだボディで新しい接触セット(全体も有効になる)
    let args = json!({"bodies": [roots[0].0, roots[1].0, roots[2].0]});
    let cx = Context {
        args: &args,
        ..t.cx()
    };
    let Some(Ok(Outcome::Change { label, edits })) =
        AsmCommands.run(jc_asm::contact::NEW_CONTACT_SET, &cx)
    else {
        panic!("接触セットを作れない")
    };
    t.doc.apply_edits(&label, edits).unwrap();
    assert!(contact::of(t.doc.data()).enabled);
    assert_eq!(contact::of(t.doc.data()).sets[0].name, "接触セット1");
    t.recompute();
    let x = turn([1.0, 0.0, 0.0]);
    let stop = limit(&t, hinge, x, 0.0, 20.0).expect("めり込む向きは止まる");
    assert!(stop.abs() < 0.01, "{stop}");
    assert_eq!(
        limit(&t, hinge, x, 0.0, -90.0),
        None,
        "開く向きは止まらない"
    );
    // 半分開いた所から閉じると、0 度の手前で止まる
    assert!(limit(&t, hinge, x, -45.0, 30.0).is_some_and(|v| v.abs() < 0.01));
    // AI・CLI からも、どこで当たるか・あといくつ動けるかを聞ける(10-20 a1)
    let reach = |t: &Bench, to: f64| {
        let args = json!({"joint": hinge.0, "to": to});
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        match AsmCommands.run(jc_asm::contact::REACH, &cx) {
            Some(Ok(Outcome::Message(m))) => m,
            Some(Err(e)) => panic!("{e}"),
            _ => panic!("答えが文でない"),
        }
    };
    let m = reach(&t, 20.0);
    assert!(m.contains("に当たります(あと 0"), "{m}");
    assert!(m.contains("「箱」"), "{m}");
    let m = reach(&t, -90.0);
    assert!(m.contains("当たりません"), "{m}");
    assert_eq!(
        limit(&t, spin, turn([0.0, 0.0, 1.0]), 0.0, 170.0),
        None,
        "つまみは蓋に載っているだけ"
    );
    // セットの目を切る・全体を切ると止めない
    let mut c = contact::of(t.doc.data());
    c.sets[0].enabled = false;
    t.doc
        .apply(Transaction::new(
            "目",
            vec![t.doc.data().section_op(&c).unwrap()],
        ))
        .unwrap();
    assert_eq!(limit(&t, hinge, x, 0.0, 20.0), None);
    t.doc.undo();
    let args = json!({});
    let cx = Context {
        args: &args,
        ..t.cx()
    };
    let Some(Ok(Outcome::Change { label, edits })) =
        AsmCommands.run(jc_asm::contact::CONTACTS, &cx)
    else {
        panic!("切れない")
    };
    t.doc.apply_edits(&label, edits).unwrap();
    assert!(!contact::of(t.doc.data()).enabled);
    assert_eq!(limit(&t, hinge, x, 0.0, 20.0), None);
    // ボディが 1 つでは作れない
    let args = json!({"bodies": [roots[0].0]});
    let cx = Context {
        args: &args,
        ..t.cx()
    };
    assert!(matches!(
        AsmCommands.run(jc_asm::contact::NEW_CONTACT_SET, &cx),
        Some(Err(_))
    ));
    // 保存して読み直せる
    let back = Document::from_json(&t.doc.to_json()).unwrap();
    assert_eq!(contact::of(back.data()), contact::of(t.doc.data()));
}
/// 自動分解: 箱(部品に入っていない)と部品「蓋」(蓋のボディと、中の部品
/// 「つまみ」)。
/// - 1 レベル: 直下の 2 つ(箱と蓋の部品)を組立の中心から離す。箱は中心より下なので -Z へ自分の
///   高さ 10、蓋の部品は +Z へ自分の高さ 6(蓋 2 + つまみ 4)
/// - すべてのレベル: さらに蓋の部品の中で、つまみは +Z へ 4、蓋のボディは -Z へ 2。中の物は外の
///   部品の動きに足して動くので、終わりは重ならない
/// - 順次は部品ごとに 1 秒の段、設計の形は変えない
#[test]
fn auto_explode_spreads_one_level_or_all_levels_without_overlap() {
    let mut b = DocBuilder::new();
    let boxed = block(&mut b, "箱", [0.0, 0.0, 30.0, 20.0], 10.0, 0.0);
    let lid = block(&mut b, "蓋", [0.0, 0.0, 30.0, 20.0], 2.0, 10.0);
    let knob = block(&mut b, "つまみ", [13.0, 8.0, 4.0, 4.0], 4.0, 12.0);
    let mut t = Bench {
        doc: b.finish("組立"),
        k: Arc::new(SmlibKernel::new()),
        report: Default::default(),
        reg: registry(),
    };
    t.recompute();
    let roots: Vec<FeatureId> = [boxed, lid, knob]
        .iter()
        .map(|f| body_root(t.doc.data(), &t.reg, *f))
        .collect();
    let (c, d) = (t.doc.alloc_group_id(), t.doc.alloc_group_id());
    let comp = |id, name: &str, parent, root| Component {
        id,
        name: name.into(),
        parent,
        bodies: vec![root],
    };
    t.doc
        .apply(Transaction::new(
            "部品",
            vec![Op::SetComponents {
                components: vec![
                    comp(c, "蓋", None, roots[1]),
                    comp(d, "つまみ", Some(c), roots[2]),
                ],
            }],
        ))
        .unwrap();
    t.recompute();
    let rest: Vec<_> = roots.iter().map(|r| t.bounds(*r)).collect();
    let run = |t: &Bench, id: &str, args: serde_json::Value| {
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        match AsmCommands.run(id, &cx) {
            Some(Ok(Outcome::Change { label, edits })) => (label, edits),
            o => panic!("{o:?}"),
        }
    };
    // 1 レベル・ワンステップ: 同時の段 1 つ
    let (label, edits) = run(&t, jc_asm::anim::AUTO_EXPLODE_ONE, json!({}));
    assert_eq!(label, "自動分解(1 レベル・ワンステップ、2 個)");
    t.doc.apply_edits(&label, edits).unwrap();
    assert_eq!(animation::steps(t.doc.data()).len(), 1);
    let up = |b: ([f64; 3], [f64; 3]), z: f64| {
        ([b.0[0], b.0[1], b.0[2] + z], [b.1[0], b.1[1], b.1[2] + z])
    };
    let end = |t: &Bench, i: usize| posed_bounds(t, roots[i], 1e9);
    assert!(
        near_mm(end(&t, 0).0, up(rest[0], -10.0).0),
        "{:?}",
        end(&t, 0)
    );
    assert!(
        near_mm(end(&t, 1).0, up(rest[1], 6.0).0),
        "{:?}",
        end(&t, 1)
    );
    assert!(
        near_mm(end(&t, 2).0, up(rest[2], 6.0).0),
        "{:?}",
        end(&t, 2)
    );
    t.doc.undo();
    // すべてのレベル・順次・目盛 1: 部品ごとに 1 秒の段が 4 つ
    let (label, edits) = run(
        &t,
        jc_asm::anim::AUTO_EXPLODE_ALL,
        json!({"sequential": true, "trail": false}),
    );
    assert_eq!(label, "自動分解(すべてのレベル・順次、4 個)");
    t.doc.apply_edits(&label, edits).unwrap();
    let steps = &animation::steps(t.doc.data());
    assert_eq!(steps.len(), 4);
    assert!(steps.iter().all(|s| s.seconds == 1.0 && !s.trail));
    assert_eq!(animation::duration(steps), 4.0);
    assert!(near_mm(end(&t, 0).0, up(rest[0], -10.0).0));
    assert!(
        near_mm(end(&t, 1).0, up(rest[1], 4.0).0),
        "{:?}",
        end(&t, 1)
    );
    assert!(
        near_mm(end(&t, 2).0, up(rest[2], 10.0).0),
        "{:?}",
        end(&t, 2)
    );
    // 終わりは重ならない(Z の向きに並ぶ)
    assert!(end(&t, 0).1[2] <= end(&t, 1).0[2] + 1e-3 && end(&t, 1).1[2] <= end(&t, 2).0[2] + 1e-3);
    // 設計の形は変えない
    t.recompute();
    assert!(near_mm(t.bounds(roots[2]).0, rest[2].0));
    // 目盛は 0 より大きく
    let cx_args = json!({"scale": 0.0});
    let cx = Context {
        args: &cx_args,
        ..t.cx()
    };
    assert!(matches!(
        AsmCommands.run(jc_asm::anim::AUTO_EXPLODE_ONE, &cx),
        Some(Err(_))
    ));
}

/// モーション リンク(#19): 蓋の蝶番とつまみの回転を「蓋 90 deg でつまみ 180 deg」でつなぐと、
/// 蓋の値を変えるだけでつまみが 2 倍回る(再計算の形も、モーション スタディの動きも)。引数の順が
/// 逆でも、タイムラインで後ろのつまみが連動する。つまみの面を引くと蓋のジョイントを動かす。
/// 反転で逆に回り、「値で動かす」に戻すと外れる。
#[test]
fn motion_link_turns_the_knob_twice_as_far_as_the_lid() {
    let (mut t, roots, hinge, spin, _) = lid_and_knob();
    let run = |t: &mut Bench, args: serde_json::Value| -> Result<(), String> {
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        match AsmCommands
            .run(jc_asm::motion_link::MOTION_LINK, &cx)
            .expect("ある操作")
        {
            Ok(Outcome::Change { label, edits }) => {
                t.doc
                    .apply_edits(&label, edits)
                    .map_err(|e| e.to_string())?;
                t.recompute();
                Ok(())
            }
            Ok(o) => Err(format!("{o:?}")),
            Err(e) => Err(e),
        }
    };
    // 同じジョイント・ジョイントでない物は断る
    assert!(run(&mut t, json!({"joint": hinge.0, "other": hinge.0})).is_err());
    assert!(run(&mut t, json!({"joint": hinge.0, "other": roots[0].0})).is_err());
    // 引数はつまみが先でも、後ろのつまみが蓋に連動する
    run(
        &mut t,
        json!({"joint": spin.0, "other": hinge.0, "lead": "180 deg", "follow": "90 deg"}),
    )
    .unwrap();
    let j = t.joint_data(spin);
    assert_eq!(j.linked(), Some(hinge));
    assert_eq!(
        (j.link_lead.as_str(), j.link_follow.as_str()),
        ("90 deg", "180 deg")
    );
    assert!(t.joint_data(hinge).linked().is_none());
    let value = |t: &Bench, id: FeatureId| animation::drive_of(&t.report, id).unwrap().value;
    // 蓋を -45 度: つまみは -90 度(欄の値 0 deg は使わない)
    t.set_joint(hinge, |j| j.angle = "-45 deg".into());
    assert!((value(&t, spin) + std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    let linked = t.bounds(roots[2]);
    // 同じ形は、連動を外してつまみを -90 度にした時と同じ所にある
    t.set_joint(spin, |j| {
        j.drive_by = jc_asm::joint::DriveBy::Value;
        j.angle = "-90 deg".into();
    });
    let own = t.bounds(roots[2]);
    assert!(
        near_mm(linked.0, own.0) && near_mm(linked.1, own.1),
        "{linked:?} {own:?}"
    );
    // 反転: 逆に回る
    t.set_joint(spin, |j| {
        j.drive_by = jc_asm::joint::DriveBy::Link;
        j.angle = "0 deg".into();
        j.link_flip = true;
    });
    assert!((value(&t, spin) - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    t.set_joint(spin, |j| j.link_flip = false);
    // 蓋を 0 度に戻し、スタディで蓋だけにキーを置くと、つまみも連動して動く
    t.set_joint(hinge, |j| j.angle = "0 deg".into());
    let cx_args = json!({"joint": hinge.0, "step": 100, "value": -45.0});
    let cx = Context {
        args: &cx_args,
        ..t.cx()
    };
    let Some(Ok(Outcome::Change { label, edits })) =
        AsmCommands.run(jc_asm::motion::MOTION_STUDY, &cx)
    else {
        panic!("スタディを作れない");
    };
    t.doc.apply_edits(&label, edits).unwrap();
    t.recompute();
    let posed = study_bounds(&t, roots[2], 100.0);
    assert!(
        near_mm(posed.0, linked.0) && near_mm(posed.1, linked.1),
        "{posed:?} {linked:?}"
    );
    // つまみの面を引くと、連動元の蓋のジョイントを動かす
    let data = t.doc.data();
    let (knob, h) = visible_bodies(data, &t.reg, &t.report)
        .into_iter()
        .find(|(id, _)| body_root(data, &t.reg, *id) == roots[2])
        .unwrap();
    let at = FacePick {
        feature: knob,
        face: t.k.mesh(h, 0.1).unwrap().face_names[0].clone(),
    };
    let cx = t.cx();
    let drag = jc_asm::joint::drag(&cx, &at).expect("引ける");
    assert_eq!(drag.feature, hinge);
    // 保存して読み直せる
    let back = Document::from_json(&t.doc.to_json()).unwrap();
    let j: Joint = serde_json::from_value(back.data().feature(spin).unwrap().data.clone()).unwrap();
    assert_eq!(j.linked(), Some(hinge));
}

/// 剛性グループ(#19): 蓋と、離れた所の札(ボディ)をグループにすると、前に作った蝶番で札も蓋と
/// 一緒に回る。後から札を地面に回転のジョイントでつなぐと、蓋とつまみも札と一緒に回る。蓋の
/// 土台の箱を同じグループに入れようとすると、輪になるので断る。
#[test]
fn rigid_group_moves_its_parts_together() {
    let (mut t, roots, hinge, _spin, _, tag) = lid_and_knob_tagged(true);
    let tag = tag.unwrap();
    let run = |t: &mut Bench, sel: &[FeatureId]| -> Result<(), String> {
        let args = json!({"bodies": sel.iter().map(|f| f.0).collect::<Vec<_>>()});
        let cx = Context {
            args: &args,
            ..t.cx()
        };
        match AsmCommands
            .run(jc_asm::rigid::NEW_RIGID_GROUP, &cx)
            .expect("ある操作")
        {
            Ok(Outcome::Change { label, edits }) => {
                t.doc
                    .apply_edits(&label, edits)
                    .map_err(|e| e.to_string())?;
                t.recompute();
                Ok(())
            }
            Ok(o) => Err(format!("{o:?}")),
            Err(e) => Err(e),
        }
    };
    let last = |t: &Bench, root: FeatureId| {
        let data = t.doc.data();
        visible_bodies(data, &t.reg, &t.report)
            .into_iter()
            .find(|(id, _)| body_root(data, &t.reg, *id) == root)
            .unwrap()
            .0
    };
    // 1 つの部品だけ・輪になる組は断る
    let lid_last = last(&t, roots[1]);
    assert!(run(&mut t, &[lid_last]).is_err());
    let box_last = last(&t, roots[0]);
    let err = run(&mut t, &[box_last, lid_last]).unwrap_err();
    assert!(err.contains("輪"), "{err}");
    let tag0 = t.bounds(tag);
    let tag_last = last(&t, tag);
    let hinge_before = t.joint_data(hinge);
    run(&mut t, &[lid_last, tag_last]).unwrap();
    // 前の蝶番は書き換えない(グループが自分で札を動かす)
    assert_eq!(t.joint_data(hinge), hinge_before);
    let group = t
        .doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == jc_asm::rigid::RIGID_GROUP)
        .unwrap()
        .id;
    // 蓋を -90 度: 札も蝶番(y = 20、z = 12 の X 軸)のまわりに立ち上がる
    t.set_joint(hinge, |j| j.angle = "-90 deg".into());
    let up = t.bounds(tag);
    assert!(up.1[2] > tag0.1[2] + 10.0, "{tag0:?} → {up:?}");
    // 動かして見せる時も札は蝶番と一緒に動く
    let d = jc_interchange::animation::drive_of(&t.report, hinge).unwrap();
    assert!(
        jc_interchange::animation::moved_bodies(t.doc.data(), &t.report, hinge, &d)
            .iter()
            .any(|b| body_root(t.doc.data(), &t.reg, *b) == tag)
    );
    // タイムラインの印をグループの前へ戻すと、札は元の位置に戻る
    let at = t.doc.data().index_of(group).unwrap();
    let rollback = |t: &mut Bench, rb: Option<usize>| {
        t.doc
            .apply(Transaction::new(
                "印",
                vec![Op::SetRollback { rollback: rb }],
            ))
            .unwrap();
        t.report = t.doc.recompute(&t.reg, &*t.k);
    };
    rollback(&mut t, Some(at));
    assert!(near_mm(t.bounds(tag).0, tag0.0), "{:?}", t.bounds(tag));
    rollback(&mut t, None);
    assert!(near_mm(t.bounds(tag).0, up.0));
    // 消すと札は元の位置に戻り、取り消すとまた一緒に動く
    t.doc
        .apply(Transaction::new(
            "消す",
            vec![Op::RemoveFeature { id: group }],
        ))
        .unwrap();
    t.recompute();
    assert!(near_mm(t.bounds(tag).0, tag0.0), "{:?}", t.bounds(tag));
    assert!(t.doc.undo());
    t.recompute();
    assert!(near_mm(t.bounds(tag).0, up.0));
    t.set_joint(hinge, |j| j.angle = "0 deg".into());
    assert!(near_mm(t.bounds(tag).0, tag0.0));
    // 後から札を地面に回転でつなぐ(札の左手前の縦の辺): 蓋とつまみも一緒に回る
    let lid0 = t.bounds(roots[1]);
    let tag_last = last(&t, tag);
    let pivot = t.joint(
        Origin {
            feature: tag_last,
            point: [40.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
        },
        None,
    );
    let moved: Vec<FeatureId> = t
        .joint_data(pivot)
        .bodies
        .iter()
        .map(|b| body_root(t.doc.data(), &t.reg, *b))
        .collect();
    assert!(moved.contains(&roots[1]) && moved.contains(&roots[2]) && !moved.contains(&roots[0]));
    t.set_joint(pivot, |j| j.angle = "180 deg".into());
    let lid = t.bounds(roots[1]);
    // 蓋(x 0〜30)は x = 40 の軸の反対側(x 50〜80)へ回る
    assert!(lid.0[0] > 45.0 && lid0.1[0] < 35.0, "{lid0:?} → {lid:?}");
    // 保存して読み直せ、取り消せる
    let back = Document::from_json(&t.doc.to_json()).unwrap();
    assert_eq!(
        back.data()
            .features
            .iter()
            .filter(|f| f.type_id == jc_asm::rigid::RIGID_GROUP)
            .count(),
        1
    );
}
