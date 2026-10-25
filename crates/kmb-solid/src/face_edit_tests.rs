//! 勾配・面を置換・面を削除。板の体積を手計算と比べる。

use super::tests::{TestKernel, close, rect_sketch, registry};
use super::*;
use kmb_cad_modify::{DELETE_FACES, DRAFT, REPLACE_FACE};
use kmb_core::{Document, FeatureId, FeatureStatus, Kernel, Value};
use kmb_samples::sample::DocBuilder;
use kmb_sketch::SketchPlane;
use serde_json::json;

/// (x, 0) から 30×20 の長方形を、高さ `h` に押し出した板。
fn plate(b: &mut DocBuilder, x: f64, h: f64) -> FeatureId {
    let s = rect_sketch(b, SketchPlane::Xy, x, 0.0, 30.0, 20.0);
    b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            distance: format!("{h} mm"),
            ..Default::default()
        },
    )
}

/// 文書の最後に機能を足す。
fn push(doc: &mut Document, type_id: &str, data: serde_json::Value) -> FeatureId {
    doc.apply_edits(
        type_id,
        vec![kmb_core::Edit::Insert {
            type_id: type_id.into(),
            name: type_id.into(),
            data,
        }],
    )
    .unwrap()[0]
}

/// 機能 `id` の計算の結果(ボディか誤りの文)。
fn result(
    doc: &mut Document,
    k: &dyn Kernel,
    id: FeatureId,
) -> Result<kmb_core::ShapeHandle, String> {
    let report = doc.recompute(&registry(), k);
    match report.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => match outputs.get("body") {
            Some(Value::Shape(h)) => Ok(*h),
            v => panic!("{v:?}"),
        },
        other => Err(format!("{other:?}")),
    }
}

/// 外向きの法線が `n` の平らな面の名前。
fn face(k: &dyn Kernel, h: kmb_core::ShapeHandle, n: [f64; 3]) -> String {
    k.faces(h)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.planar
                .is_some_and(|p| kmb_core::kernel::dot(p.normal, n) > 0.999)
        })
        .unwrap_or_else(|| panic!("法線 {n:?} の面が無い"))
        .name
}

fn face_ref(body: FeatureId, name: &str) -> serde_json::Value {
    json!({"kind": "face", "feature": body.0, "face": name})
}

/// 板の +X の面に 10 度の勾配: 下の辺(XY 平面との交わり)を軸に上が細り、三角の柱の分だけ軽くなる。
/// 反転で引き出し方向を逆にすると同じだけ重くなる。隣り合う +Y の面と一緒に付けると角が合う
/// (2 つの三角の柱から、重なる角の分を引いた体積)。固定の平面を高さ 4 の別の板の上の面にすると、
/// その高さを軸に上が削れ下が足される。引き出し方向に垂直な上の面は断る。
#[test]
fn draft_tilts_side_faces_about_the_neutral_plane() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は面の置き換えに使う箱の積を作れない");
    }
    let mut b = DocBuilder::new();
    let p = plate(&mut b, 0.0, 10.0);
    let low = plate(&mut b, 80.0, 4.0);
    let mut doc = b.finish("勾配");
    let h = result(&mut doc, &*k, p).unwrap();
    let (px, py, top) = (
        face(&*k, h, [1.0, 0.0, 0.0]),
        face(&*k, h, [0.0, 1.0, 0.0]),
        face(&*k, h, [0.0, 0.0, 1.0]),
    );
    let t = 10f64.to_radians().tan();
    let wedge = 0.5 * 10.0 * 10.0 * t;
    let d = push(
        &mut doc,
        DRAFT,
        json!({"faces": [face_ref(p, &px)], "plane_ref": {"kind": "origin_xy"}, "angle": "10 deg"}),
    );
    let r = result(&mut doc, &*k, d).unwrap();
    let m = k.mass(r).unwrap();
    assert!(close(m.volume, 6000.0 - wedge * 20.0, 1e-6), "{}", m.volume);
    assert!(
        (m.max[0] - 30.0).abs() < 1e-6,
        "下の辺は動かない: {:?}",
        m.max
    );

    let flip = json!({"faces": [face_ref(p, &px)], "plane_ref": {"kind": "origin_xy"}, "angle": "10 deg", "flip": true});
    doc.apply(super::modify_tests::set(d, flip)).unwrap();
    let r = result(&mut doc, &*k, d).unwrap();
    let m = k.mass(r).unwrap();
    assert!(close(m.volume, 6000.0 + wedge * 20.0, 1e-6), "{}", m.volume);
    assert!((m.max[0] - (30.0 + 10.0 * t)).abs() < 1e-6, "{:?}", m.max);

    let both = json!({"faces": [face_ref(p, &px), face_ref(p, &py)], "plane_ref": {"kind": "origin_xy"}, "angle": "10 deg"});
    doc.apply(super::modify_tests::set(d, both)).unwrap();
    let r = result(&mut doc, &*k, d).unwrap();
    let corner = t * t * 1000.0 / 3.0;
    let want = 6000.0 - wedge * (20.0 + 30.0) + corner;
    assert!(
        close(k.volume(r).unwrap(), want, 1e-6),
        "{} と {want}",
        k.volume(r).unwrap()
    );

    let hl = result(&mut doc, &*k, low).unwrap();
    let mid = json!({"faces": [face_ref(p, &px)], "plane_ref": {"kind": "face", "feature": low.0, "face": face(&*k, hl, [0.0, 0.0, 1.0])}, "angle": "10 deg"});
    doc.apply(super::modify_tests::set(d, mid)).unwrap();
    let r = result(&mut doc, &*k, d).unwrap();
    let want = 6000.0 - 0.5 * (36.0 - 16.0) * t * 20.0;
    assert!(
        close(k.volume(r).unwrap(), want, 1e-6),
        "{} と {want}",
        k.volume(r).unwrap()
    );

    let flat = json!({"faces": [face_ref(p, &top)], "plane_ref": {"kind": "origin_xy"}, "angle": "10 deg"});
    doc.apply(super::modify_tests::set(d, flat)).unwrap();
    let e = result(&mut doc, &*k, d).unwrap_err();
    assert!(e.contains("垂直な面には勾配を付けられません"), "{e}");
}

/// 板の上の面を、別の板の上の面(高い・低い)に置き換えると、まわりの面が延びて・縮んで高さが合う。
#[test]
fn replace_face_moves_a_face_onto_another_plane() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は面の置き換えに使う箱の積を作れない");
    }
    let mut b = DocBuilder::new();
    let p = plate(&mut b, 0.0, 10.0);
    let high = plate(&mut b, 40.0, 15.0);
    let low = plate(&mut b, 80.0, 4.0);
    let mut doc = b.finish("面を置換");
    let h = result(&mut doc, &*k, p).unwrap();
    let top = face(&*k, h, [0.0, 0.0, 1.0]);
    let up = |id: FeatureId, doc: &mut Document| {
        let t = result(doc, &*k, id).unwrap();
        face(&*k, t, [0.0, 0.0, 1.0])
    };
    let target = up(high, &mut doc);
    let r = push(
        &mut doc,
        REPLACE_FACE,
        json!({"faces": [face_ref(p, &top)], "target": [face_ref(high, &target)]}),
    );
    let m = k.mass(result(&mut doc, &*k, r).unwrap()).unwrap();
    assert!(close(m.volume, 9000.0, 1e-6), "{}", m.volume);
    assert!((m.max[2] - 15.0).abs() < 1e-6);
    let target = up(low, &mut doc);
    doc.apply(super::modify_tests::set(
        r,
        json!({"faces": [face_ref(p, &top)], "target": [face_ref(low, &target)]}),
    ))
    .unwrap();
    let m = k.mass(result(&mut doc, &*k, r).unwrap()).unwrap();
    assert!(close(m.volume, 2400.0, 1e-6), "{}", m.volume);
}

/// 面取りした辺の斜めの面を消すと、上の面と横の面が延びて角に戻る。
#[test]
fn delete_face_closes_a_chamfer_back_into_a_corner() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は面の置き換えに使う箱の積を作れない");
    }
    let mut b = DocBuilder::new();
    let p = plate(&mut b, 0.0, 10.0);
    let mut doc = b.finish("面を削除");
    let h = result(&mut doc, &*k, p).unwrap();
    let (top, side) = (face(&*k, h, [0.0, 0.0, 1.0]), face(&*k, h, [1.0, 0.0, 0.0]));
    let ch = push(
        &mut doc,
        CHAMFER,
        json!({"target": p.0, "edges": [[top, side]], "distance": "2 mm"}),
    );
    let hc = result(&mut doc, &*k, ch).unwrap();
    assert!(close(k.volume(hc).unwrap(), 6000.0 - 40.0, 1e-6));
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let slant = face(&*k, hc, [s, 0.0, s]);
    let del = push(
        &mut doc,
        DELETE_FACES,
        json!({"faces": [face_ref(ch, &slant)]}),
    );
    let m = k.mass(result(&mut doc, &*k, del).unwrap()).unwrap();
    assert!(close(m.volume, 6000.0, 1e-6), "{}", m.volume);
    assert!((m.max[0] - 30.0).abs() < 1e-6 && (m.max[2] - 10.0).abs() < 1e-6);

    // 丸めた辺の隣の面は、まわりに曲がった面があるので断る(黙って形を崩さない)
    let fil = push(
        &mut doc,
        FILLET,
        json!({"target": p.0, "edges": [[face(&*k, h, [0.0, 0.0, 1.0]), face(&*k, h, [0.0, 1.0, 0.0])]], "radius": "2 mm"}),
    );
    let hf = result(&mut doc, &*k, fil).unwrap();
    let e = {
        let id = push(
            &mut doc,
            DELETE_FACES,
            json!({"faces": [face_ref(fil, &face(&*k, hf, [0.0, 1.0, 0.0]))]}),
        );
        result(&mut doc, &*k, id).unwrap_err()
    };
    assert!(e.contains("曲がった面"), "{e}");
}

/// フィレットの丸めの面を消すと、上の面と横の面が延びて鋭い角に戻る(SMLib の丸めの面の削除)。
/// 丸めでない曲面(見本の部品のボスの円筒の側面)は断る。
#[test]
fn delete_face_removes_a_fillet_back_into_a_corner() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は丸めの面を消せない");
    }
    let mut b = DocBuilder::new();
    let p = plate(&mut b, 0.0, 10.0);
    let mut doc = b.finish("丸めの削除");
    let h = result(&mut doc, &*k, p).unwrap();
    let (top, side) = (face(&*k, h, [0.0, 0.0, 1.0]), face(&*k, h, [1.0, 0.0, 0.0]));
    let fil = push(
        &mut doc,
        FILLET,
        json!({"target": p.0, "edges": [[top, side]], "radius": "2 mm"}),
    );
    let hf = result(&mut doc, &*k, fil).unwrap();
    let lost = (4.0 - std::f64::consts::PI) * 20.0;
    assert!(close(k.volume(hf).unwrap(), 6000.0 - lost, 1e-4));
    let round = k
        .faces(hf)
        .unwrap()
        .into_iter()
        .find(|f| f.planar.is_none())
        .expect("丸めの面")
        .name;
    let del = push(
        &mut doc,
        DELETE_FACES,
        json!({"faces": [face_ref(fil, &round)]}),
    );
    let hd = result(&mut doc, &*k, del).unwrap();
    let m = k.mass(hd).unwrap();
    assert!(close(m.volume, 6000.0, 1e-6), "{}", m.volume);
    assert!(
        k.faces(hd).unwrap().iter().all(|f| f.planar.is_some()),
        "曲面が残らない"
    );
    assert_eq!(k.faces(hd).unwrap().len(), 6);

    let part = kmb_samples::reference_part();
    let mut doc = part.doc;
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let (body, hp) = *kmb_core::visible_bodies_before(doc.data(), &reg, &report, None)
        .last()
        .unwrap();
    let boss = k
        .faces(hp)
        .unwrap()
        .into_iter()
        .find(|f| f.name.starts_with("f12:side"))
        .unwrap()
        .name;
    let id = push(
        &mut doc,
        DELETE_FACES,
        json!({"faces": [face_ref(body, &boss)]}),
    );
    let e = result(&mut doc, &*k, id).unwrap_err();
    assert!(e.contains("丸めの面ではない"), "{e}");
}

/// 置き換えた面に元の面の名前が残り、その面に置いたスケッチが、置き換え先を高い面から低い面に
/// 変えても外れずに付いて行く(前は面の名前が足した箱と削った箱の面で替わり、スケッチが外れた)。
/// 勾配を付けた面も元の名前のまま。
#[test]
fn edited_faces_keep_their_names_so_sketches_stay_on_them() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は面の置き換えに使う箱の積を作れない");
    }
    let mut b = DocBuilder::new();
    let p = plate(&mut b, 0.0, 10.0);
    let high = plate(&mut b, 40.0, 15.0);
    let low = plate(&mut b, 80.0, 4.0);
    let mut doc = b.finish("面の名前");
    let h = result(&mut doc, &*k, p).unwrap();
    let (top, px) = (
        face(&*k, h, [0.0, 0.0, 1.0]),
        face(&*k, h, [1.0, 0.0, 0.0]),
    );
    let up = |id: FeatureId, doc: &mut Document| {
        let t = result(doc, &*k, id).unwrap();
        face(&*k, t, [0.0, 0.0, 1.0])
    };
    let target = up(high, &mut doc);
    let r = push(
        &mut doc,
        REPLACE_FACE,
        json!({"faces": [face_ref(p, &top)], "target": [face_ref(high, &target)]}),
    );
    let h = result(&mut doc, &*k, r).unwrap();
    assert_eq!(face(&*k, h, [0.0, 0.0, 1.0]), top);
    let mut sk = kmb_samples::sample::SketchBuilder::new(SketchPlane::face(r, top.clone()));
    sk.fixed_point(5.0, 5.0);
    let s = push(
        &mut doc,
        kmb_sketch::SKETCH,
        serde_json::to_value(sk.build()).unwrap(),
    );
    let z = |doc: &mut Document| {
        let report = doc.recompute(&registry(), &*k);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(s) else {
            panic!("スケッチが外れた: {:?}", report.feature(s));
        };
        let pts = <kmb_core::Points as kmb_core::Payload>::from_value(&outputs["points"]).expect("点");
        let q = pts.points.values().next().expect("点が 1 つ");
        pts.plane.to_world(*q)[2]
    };
    assert!((z(&mut doc) - 15.0).abs() < 1e-6);
    let target = up(low, &mut doc);
    doc.apply(super::modify_tests::set(
        r,
        json!({"faces": [face_ref(p, &top)], "target": [face_ref(low, &target)]}),
    ))
    .unwrap();
    assert!((z(&mut doc) - 4.0).abs() < 1e-6);

    let d = push(
        &mut doc,
        DRAFT,
        json!({"faces": [face_ref(p, &px)], "plane_ref": {"kind": "origin_xy"}, "angle": "10 deg"}),
    );
    let h = result(&mut doc, &*k, d).unwrap();
    let tilted = k
        .faces(h)
        .unwrap()
        .into_iter()
        .find(|f| f.name == px)
        .and_then(|f| f.planar)
        .expect("勾配を付けた面が元の名前で残る");
    assert!(tilted.normal[2] > 0.1, "{:?}", tilted.normal);
}
