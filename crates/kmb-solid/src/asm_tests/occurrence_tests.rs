//! 同じ部品をいくつも置く(kmb-asm の出現の機能と操作): 置く・取り消す・元を直す・部品表で数える・
//! 独立させる・元にそろえる・入れ子の部品・保存して開き直す。

use super::*;
use kmb_asm::occurrence::{self, source_of};
use kmb_asm::{DETACH_OCCURRENCE, OCCURRENCE, PLACE_OCCURRENCE, SYNC_OCCURRENCE};

/// 読み直した形の箱(STEP の許容差の分だけ広い)を比べる。
fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-2)
}

/// 操作 `id` を引数 `args` で流し、変更なら適用して計算し直す。
fn run(t: &mut Bench, id: &str, args: serde_json::Value) -> Result<Vec<FeatureId>, String> {
    let cx = Context {
        args: &args,
        ..t.cx()
    };
    let out = AsmCommands.run(id, &cx).expect("組立の操作");
    match out {
        Ok(Outcome::Change { label, edits }) => {
            let made = t
                .doc
                .apply_edits(&label, edits)
                .map_err(|e| e.to_string())?;
            t.recompute();
            Ok(made)
        }
        Ok(Outcome::Message(m)) => Err(m),
        Ok(_) => panic!("変更でない結果"),
        Err(e) => Err(e.into()),
    }
}

/// 30×20×10 の「脚」(部品)を 1 つ持つ文書。脚の押し出しの機能を返す。
fn leg() -> (Bench, FeatureId, u64) {
    let mut b = DocBuilder::new();
    let e = block(&mut b, "脚", [0.0, 0.0, 30.0, 20.0], 10.0, 0.0);
    let mut t = Bench::new(b.finish("机"));
    let c = t.doc.alloc_group_id();
    t.doc
        .apply(Transaction::new(
            "部品",
            vec![Op::SetComponents {
                components: vec![Component {
                    id: c,
                    name: "脚".into(),
                    parent: None,
                    bodies: vec![e],
                }],
            }],
        ))
        .unwrap();
    t.recompute();
    (t, e, c)
}

fn set_height(t: &mut Bench, e: FeatureId, h: f64) {
    let mut d = t.doc.data().feature(e).unwrap().data.clone();
    d["distance"] = json!(format!("{h} mm"));
    t.doc
        .apply(Transaction::new(
            "高さ",
            vec![Op::SetFeatureData { id: e, data: d }],
        ))
        .unwrap();
    t.recompute();
}

/// 出現の部品(名前の順)。
fn occurrence_parts(t: &Bench, src: u64) -> Vec<Component> {
    let data = t.doc.data();
    occurrence::occurrences_of(data, src)
        .into_iter()
        .filter_map(|c| data.component(c).cloned())
        .collect()
}

/// 部品をいくつも置くと、元の幅と隙間ずつ横に並び、名前は「脚:2」から。元を直すと全部が変わり、
/// 部品表では 1 行で数える。置いたのは 1 回で戻る。
#[test]
fn placing_occurrences_follows_the_original_and_counts_in_the_bom() {
    let (mut t, e, c) = leg();
    let depth = t.doc.undo_depth();
    let made = run(&mut t, PLACE_OCCURRENCE, json!({"part": c, "count": 3})).unwrap();
    assert_eq!(t.doc.undo_depth(), depth + 1);
    assert_eq!(made.len(), 3);
    let parts = occurrence_parts(&t, c);
    let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["脚:2", "脚:3", "脚:4"]);
    for (i, p) in parts.iter().enumerate() {
        assert_eq!(source_of(t.doc.data(), p.id), Some(c));
        let (lo, hi) = t.bounds(p.bodies[0]);
        let x = 40.0 * (i + 1) as f64;
        assert!(
            near(lo, [x, 0.0, 0.0]) && near(hi, [x + 30.0, 20.0, 10.0]),
            "{lo:?} {hi:?}"
        );
        // 出現は形を写さず、元の形を置き直した物
        assert_eq!(
            t.doc.data().feature(p.bodies[0]).unwrap().type_id,
            OCCURRENCE
        );
    }
    let rows = bom_rows(&t.cx());
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0].names[0].as_str(), rows[0].count), ("脚", 4));
    // 元を直すと出現も
    set_height(&mut t, e, 25.0);
    for p in &parts {
        assert!((t.bounds(p.bodies[0]).1[2] - 25.0).abs() < 1e-3);
    }
    // もう 1 つ置くと続きの位置と番号
    run(&mut t, PLACE_OCCURRENCE, json!({"part": c})).unwrap();
    let last = occurrence_parts(&t, c).pop().unwrap();
    assert_eq!(last.name, "脚:5");
    assert!(near(t.bounds(last.bodies[0]).0, [160.0, 0.0, 0.0]));
    // 取り消すと 3 つに戻る
    assert!(t.doc.undo());
    t.recompute();
    assert_eq!(occurrence_parts(&t, c).len(), 3);
    // 出現の部品を選んでも、元の部品の出現を置く
    let p2 = parts[0].id;
    run(
        &mut t,
        PLACE_OCCURRENCE,
        json!({"part": p2, "x": 0, "y": 50}),
    )
    .unwrap();
    let last = occurrence_parts(&t, c).pop().unwrap();
    assert!(
        near(t.bounds(last.bodies[0]).0, [0.0, 200.0, 0.0]),
        "{:?}",
        t.bounds(last.bodies[0])
    );
}

/// 独立させると、その時の形と面の名前のまま元から切り離れる(以後は元を直しても変わらない)。
#[test]
fn a_detached_occurrence_keeps_its_shape_and_face_names() {
    let (mut t, e, c) = leg();
    run(&mut t, PLACE_OCCURRENCE, json!({"part": c})).unwrap();
    let p = occurrence_parts(&t, c)[0].clone();
    let body = p.bodies[0];
    let faces = |t: &Bench| {
        let h = t.shape(body);
        let mut f = t.k.face_names(h).unwrap();
        f.sort();
        f
    };
    let before = faces(&t);
    run(&mut t, DETACH_OCCURRENCE, json!({"part": p.id})).unwrap();
    assert_eq!(source_of(t.doc.data(), p.id), None);
    assert_eq!(faces(&t), before);
    assert!(
        close(t.bounds(body).1, [70.0, 20.0, 10.0]),
        "{:?}",
        t.bounds(body)
    );
    set_height(&mut t, e, 25.0);
    assert!(
        close(t.bounds(body).1, [70.0, 20.0, 10.0]),
        "{:?}",
        t.bounds(body)
    );
    // 部品表は別の行(形が違う)
    assert_eq!(bom_rows(&t.cx()).len(), 2);
}

/// 元の部品に後からボディを足すと、出現の部品は「元にそろえる」で同じ置き方の出現を足す。
#[test]
fn syncing_adds_bodies_added_to_the_original() {
    let mut b = DocBuilder::new();
    let a = block(&mut b, "脚", [0.0, 0.0, 30.0, 20.0], 10.0, 0.0);
    let foot = block(&mut b, "足先", [0.0, 0.0, 30.0, 5.0], 2.0, 10.0);
    let mut t = Bench::new(b.finish("机"));
    let foot = body_root(t.doc.data(), &t.reg, foot);
    let c = t.doc.alloc_group_id();
    let comp = |bodies| {
        Transaction::new(
            "部品",
            vec![Op::SetComponents {
                components: vec![Component {
                    id: c,
                    name: "脚".into(),
                    parent: None,
                    bodies,
                }],
            }],
        )
    };
    t.doc.apply(comp(vec![a])).unwrap();
    t.recompute();
    run(&mut t, PLACE_OCCURRENCE, json!({"part": c})).unwrap();
    let p = occurrence_parts(&t, c)[0].clone();
    assert_eq!(
        run(&mut t, SYNC_OCCURRENCE, json!({"part": p.id})).unwrap_err(),
        "部品「脚:2」は元の部品とそろっています"
    );
    // 元に足先を足す
    let mut list = t.doc.data().components.clone();
    list[0].bodies.push(foot);
    t.doc
        .apply(Transaction::new(
            "足す",
            vec![Op::SetComponents { components: list }],
        ))
        .unwrap();
    t.recompute();
    assert_eq!(source_of(t.doc.data(), p.id), Some(c));
    run(&mut t, SYNC_OCCURRENCE, json!({"part": p.id})).unwrap();
    let p = t.doc.data().component(p.id).unwrap().clone();
    assert_eq!(p.bodies.len(), 2);
    assert!(
        near(t.bounds(p.bodies[1]).0, [40.0, 0.0, 10.0]),
        "{:?}",
        t.bounds(p.bodies[1])
    );
}

/// 入れ子の部品の出現は、中の部品も出現の部品として同じ形の木で作る。保存して開き直しても同じ。
#[test]
fn nested_parts_are_placed_as_the_same_tree_and_survive_saving() {
    let mut b = DocBuilder::new();
    let a = block(&mut b, "板", [0.0, 0.0, 30.0, 20.0], 10.0, 0.0);
    let knob = block(&mut b, "つまみ", [10.0, 5.0, 5.0, 5.0], 4.0, 10.0);
    let mut t = Bench::new(b.finish("机"));
    let knob = body_root(t.doc.data(), &t.reg, knob);
    let (top, inner) = (t.doc.alloc_group_id(), t.doc.alloc_group_id());
    t.doc
        .apply(Transaction::new(
            "部品",
            vec![Op::SetComponents {
                components: vec![
                    Component {
                        id: top,
                        name: "引き出し".into(),
                        parent: None,
                        bodies: vec![a],
                    },
                    Component {
                        id: inner,
                        name: "取っ手".into(),
                        parent: Some(top),
                        bodies: vec![knob],
                    },
                ],
            }],
        ))
        .unwrap();
    t.recompute();
    run(&mut t, PLACE_OCCURRENCE, json!({"part": top, "count": 2})).unwrap();
    let data = t.doc.data();
    for p in occurrence::occurrences_of(data, top) {
        let kids = data.child_components(Some(p));
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].name, "取っ手");
        assert_eq!(source_of(data, kids[0].id), Some(inner));
    }
    let rows = bom_rows(&t.cx());
    let counts: Vec<(String, usize)> = rows.iter().map(|r| (r.names[0].clone(), r.count)).collect();
    assert_eq!(
        counts,
        [("引き出し".to_string(), 3), ("取っ手".to_string(), 3)]
    );
    // 保存して開き直す
    let json = t.doc.to_json();
    let mut again = Bench::new(Document::from_json(&json).unwrap());
    again.recompute();
    assert_eq!(again.doc.data().components, t.doc.data().components);
    let o = occurrence::occurrences_of(again.doc.data(), top)[1];
    let body = again.doc.data().component(o).unwrap().bodies[0];
    assert!(
        near(again.bounds(body).0, [80.0, 0.0, 0.0]),
        "{:?}",
        again.bounds(body)
    );
}
