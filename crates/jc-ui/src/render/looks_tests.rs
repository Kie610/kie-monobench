//! 外観の窓の試験: 面の外観、外観を直す・複製・消す、見つからない面。

use crate::Workspace;
use crate::view3d::Pick;
use jc_cad_extrude::{EXTRUDE, Extrude};
use jc_core::{Op, Transaction};
use jc_samples::sample::{DocBuilder, SketchBuilder};
use jc_sketch::SketchPlane;
use serde_json::json;

/// 30×20×10 の板。
fn plate() -> Workspace {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "30 mm", "20 mm", [0.0, 0.0, 30.0, 20.0]);
    let sk = b.feature(jc_sketch::SKETCH, "板のスケッチ", s.build());
    b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut ws = Workspace::new(b.finish("板"), None, jc_bundle::modules());
    ws.wait();
    ws
}

/// 板の網の面のうち、名前が `end` で終わる面(上の面)の番号と名前。
fn top_face(ws: &Workspace) -> (usize, String) {
    let names = &ws.cur.scene.bodies[0].mesh.face_names;
    let k = names
        .iter()
        .position(|n| n.ends_with(":end"))
        .expect("上の面");
    (k, names[k].clone())
}

fn face_color(ws: &Workspace, k: usize) -> Option<[u8; 3]> {
    ws.cur.scene.bodies[0]
        .face_looks
        .get(k)
        .cloned()
        .flatten()
        .map(|l| l.color)
}

#[test]
fn a_face_appearance_paints_one_face_and_survives_a_new_size() {
    let mut ws = plate();
    let (last, root) = ws.body_roots()[0];
    let (k, face) = top_face(&ws);
    // 上の面を選んで、適用先を面にして赤を付ける
    ws.cur.picked = Some(Pick {
        feature: last,
        face: face.clone(),
    });
    ws.open_looks();
    assert!(ws.cur.looks.open);
    assert!(!ws.cur.looks.faces, "適用先の既定はボディ/コンポーネント");
    ws.cur.looks.faces = true;
    ws.looks_apply(Some("赤"));
    ws.wait();
    assert_eq!(
        jc_look::appearance::of(ws.cur.work.doc().data()).face(root, &face),
        Some("赤")
    );
    let red = jc_look::material::appearance("赤").unwrap().color;
    assert_eq!(face_color(&ws, k), Some(red));
    let n = ws.cur.scene.bodies[0].mesh.face_names.len();
    assert_eq!(
        (0..n).filter(|i| face_color(&ws, *i).is_some()).count(),
        1,
        "ほかの面は塗らない"
    );
    assert_eq!(ws.cur.scene.bodies[0].tint, None, "ボディの色は替えない");

    // 寸法を変えても同じ面に付いたまま(面の名前は形を作り直しても同じ)
    let ext = ws
        .cur
        .work
        .doc()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == EXTRUDE)
        .unwrap()
        .clone();
    let mut data = ext.data.clone();
    data["distance"] = json!("25 mm");
    assert!(ws.apply(Transaction::new(
        "厚さ",
        vec![Op::SetFeatureData { id: ext.id, data }]
    )));
    ws.wait();
    let (k2, face2) = top_face(&ws);
    assert_eq!(face2, face);
    assert_eq!(face_color(&ws, k2), Some(red));

    // 外すと面の外観だけが消える。取り消すと戻る
    ws.cur.picked = Some(Pick {
        feature: ws.body_roots()[0].0,
        face: face.clone(),
    });
    ws.looks_apply(None);
    ws.wait();
    assert!(
        jc_look::appearance::of(ws.cur.work.doc().data())
            .faces
            .is_empty()
    );
    assert_eq!(face_color(&ws, k2), None);
    ws.undo();
    ws.wait();
    assert_eq!(face_color(&ws, k2), Some(red));
}

#[test]
fn design_appearances_are_edited_copied_and_removed_with_their_uses() {
    let mut ws = plate();
    let (last, root) = ws.body_roots()[0];
    let (k, face) = top_face(&ws);
    ws.cur.picked = Some(Pick {
        feature: last,
        face: face.clone(),
    });
    ws.cur.looks.faces = true;
    ws.looks_apply(Some("赤"));
    // ボディ全体はアルミ
    ws.cur.picked = None;
    ws.select(last);
    ws.cur.looks.faces = false;
    ws.looks_apply(Some("アルミ"));
    ws.wait();
    assert_eq!(
        jc_look::appearance::of(ws.cur.work.doc().data())
            .bodies
            .get(&root)
            .map(String::as_str),
        Some("アルミ")
    );
    assert_eq!(
        jc_look::appearance::of(ws.cur.work.doc().data()).design_names(ws.cur.work.doc().data()),
        vec!["アルミ", "赤"]
    );

    // 赤を直して名前を替えると、付けた面も新しい名前になる
    assert!(
        ws.looks_run(json!({"edit": "赤", "color": "#102030", "roughness": 0.9, "rename": "濃紺"}))
    );
    ws.wait();
    let all = jc_look::appearance::of(ws.cur.work.doc().data());
    assert_eq!(all.face(root, &face), Some("濃紺"));
    let def = all.def("濃紺").unwrap();
    assert_eq!((def.color, def.roughness), ([16, 32, 48], 0.9));
    assert_eq!(face_color(&ws, k), Some([16, 32, 48]));
    // ライブラリと同じ名前には替えない
    assert!(!ws.looks_run(json!({"edit": "濃紺", "rename": "青"})));

    // 複製は「名前 (2)」。アルミの写しは金属のまま
    assert!(ws.looks_run(json!({"duplicate": "アルミ"})));
    let copy = jc_look::appearance::of(ws.cur.work.doc().data())
        .resolve("アルミ (2)")
        .unwrap();
    assert!(copy.custom);
    assert_eq!(copy.finish.metallic, 1.0);

    // 割り当て解除と削除: 付けた所から外して消す
    assert!(ws.looks_run(json!({"delete": "濃紺"})));
    ws.wait();
    assert!(
        jc_look::appearance::of(ws.cur.work.doc().data())
            .def("濃紺")
            .is_none()
    );
    assert!(
        jc_look::appearance::of(ws.cur.work.doc().data())
            .faces
            .is_empty()
    );
    assert!(ws.looks_run(json!({"delete": "アルミ"})));
    assert!(
        jc_look::appearance::of(ws.cur.work.doc().data())
            .bodies
            .is_empty()
    );

    // 知らない外観は付けない
    ws.select(ws.body_roots()[0].0);
    let before = ws.cur.work.doc().undo_labels().len();
    ws.looks_apply(Some("知らない外観"));
    assert_eq!(ws.cur.work.doc().undo_labels().len(), before);
    assert!(
        ws.win
            .message
            .as_deref()
            .is_some_and(|m| m.contains("知らない外観"))
    );
}

#[test]
fn a_face_that_is_gone_is_counted_and_not_moved_to_another_face() {
    let mut ws = plate();
    let (_, root) = ws.body_roots()[0];
    let mut a = jc_look::appearance::of(ws.cur.work.doc().data());
    a.faces
        .insert(root, [("f99:無い面".to_string(), "青".to_string())].into());
    let op = ws.cur.work.doc().data().section_op(&a).unwrap();
    assert!(ws.apply(Transaction::new("面の外観", vec![op])));
    ws.wait();
    assert_eq!(ws.looks_missing_faces().get("青"), Some(&1));
    let n = ws.cur.scene.bodies[0].mesh.face_names.len();
    assert!(
        (0..n).all(|i| face_color(&ws, i).is_none()),
        "ほかの面に付け替えない"
    );
    // 無い面は選べない(操作が断る)
    ws.cur.picked = Some(Pick {
        feature: ws.body_roots()[0].0,
        face: "f99:無い面".into(),
    });
    ws.cur.looks.faces = true;
    let before = ws.cur.work.doc().undo_labels().len();
    ws.looks_apply(Some("赤"));
    assert_eq!(ws.cur.work.doc().undo_labels().len(), before);
}

#[test]
fn favorites_are_kept_in_the_pc_settings() {
    let mut ws = plate();
    ws.looks_favorite("銅", true);
    ws.looks_favorite("赤", true);
    ws.looks_favorite("銅", false);
    assert_eq!(ws.win.layout.favorite_looks, vec!["赤".to_string()]);
    let back: crate::layout::Layout =
        serde_json::from_value(serde_json::to_value(&ws.win.layout).unwrap()).unwrap();
    assert_eq!(back.favorite_looks, vec!["赤".to_string()]);
}
