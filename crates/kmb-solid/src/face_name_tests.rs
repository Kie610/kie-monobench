//! 何度割っても面の名前が伸びない(2 回目からは「元の名前 + 最後に割った機能 + 番号」)。前の版の
//! アプリが付けた伸びた名前を指す文書は、開いた後に今の名前へ書き換えて同じ形で開く。

use super::*;
use crate::tests::*;
use kmb_core::{Document, FeatureId, FeatureStatus, Ref};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;

/// 30 × 20 × 10 の板の上面を 3 回割る: 溝(y 8〜12)で 2 つに、奥の切れ端を切り欠き(x 12〜18)で
/// 2 つに、その右の切れ端をもう一度(x 23〜25)。最後に右端の切れ端(x 25〜30、y 12〜20)を
/// 3 mm 上へ押し出す。`top` はその切れ端の名前。(板、2 回目に割った機能、3 回目に割った機能、押し出し)
fn plate_split_three_times(
    top: impl Fn(FeatureId, FeatureId) -> String,
) -> (Document, [FeatureId; 4]) {
    let mut b = DocBuilder::new();
    let rect = |b: &mut DocBuilder, name: &str, x: f64, y: f64, w: f64, h: f64| {
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(x, y);
        s.rectangle(
            o,
            &format!("{w} mm"),
            &format!("{h} mm"),
            [x, y, x + w, y + h],
        );
        b.sketch(name, s)
    };
    let sk = rect(&mut b, "板の形", 0.0, 0.0, 30.0, 20.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    // 対象は前の版のアプリと同じく、直前の機能を指す(前の版の読み方の印が要らない形)
    let cut = |b: &mut DocBuilder, name: &str, sk: FeatureId, from: f64, target: FeatureId| {
        b.feature(
            EXTRUDE,
            name,
            Extrude {
                profile: Some(sk),
                distance: "10 mm".into(),
                start: Start::Offset,
                start_offset: Some(format!("{from} mm")),
                operation: Operation::Cut,
                target: Some(target),
                ..Default::default()
            },
        )
    };
    let s1 = rect(&mut b, "溝の形", -1.0, 8.0, 32.0, 4.0);
    let first = cut(&mut b, "溝", s1, 5.0, plate);
    let s2 = rect(&mut b, "切り欠きの形", 12.0, 11.0, 6.0, 10.0);
    let second = cut(&mut b, "切り欠き", s2, 7.0, first);
    let s3 = rect(&mut b, "細い溝の形", 23.0, 11.0, 2.0, 10.0);
    let third = cut(&mut b, "細い溝", s3, 8.0, second);
    let up = b.feature(
        EXTRUDE,
        "右端の押し出し",
        Extrude {
            source: Source::Faces,
            faces: vec![Ref::Face {
                feature: third,
                face: top(second, third),
            }],
            distance: "3 mm".into(),
            operation: Operation::Join,
            target: Some(third),
            ..Default::default()
        },
    );
    (b.finish("板"), [plate, second, third, up])
}

/// 板の体積: 6000 − 溝 30 × 4 × 5 − 切り欠き 6 × 8 × 3 − 細い溝 2 × 8 × 2 + 押し出し 5 × 8 × 3。
const VOLUME: f64 = 6000.0 - 600.0 - 144.0 - 32.0 + 120.0;

fn top_names(doc: &mut Document, k: &TestKernel, plate: FeatureId) -> Vec<String> {
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    assert_ok(&report);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    let mut v: Vec<String> = k
        .face_names(bodies[0].1)
        .unwrap()
        .into_iter()
        .filter(|n| n.starts_with(&format!("f{}:end", plate.0)))
        .collect();
    v.sort();
    v
}

#[test]
fn a_face_split_three_times_keeps_a_short_name() {
    let k = TestKernel::new();
    let (mut doc, [plate, second, third, _]) =
        plate_split_three_times(|_, third| format!("f2:end~f{}#1", third.0));
    let p = plate.0;
    assert!(close(final_mass(&mut doc, &k).volume, VOLUME, 1e-6));
    let names = top_names(&mut doc, &k, plate);
    assert_eq!(
        names,
        vec![
            format!("f{p}:end#0"),
            format!("f{p}:end~f{}#0", second.0),
            format!("f{p}:end~f{}#0", third.0),
        ]
    );
}

/// 前の版の文書(版 5)の、3 回割った面(`f2:end#1#1#1`)を指す押し出しが、開いた後に同じ形になる。
/// 書き換える前は、その名前の面が無いので押し出しが止まる(前と後の対)。
#[test]
fn an_old_document_with_a_long_face_name_opens_with_the_same_shape() {
    let k = TestKernel::new();
    let reg = registry();
    let (doc, [plate, _, third, up]) = plate_split_three_times(|_, _| "f2:end#1#1#1".to_string());
    assert_eq!(plate.0, 2, "古い名前は板の番号を使う");
    let mut json: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    json["format_version"] = 5.into();
    for f in json["features"].as_array_mut().unwrap() {
        f.as_object_mut().unwrap().remove("pin_bodies");
    }
    let mut old = Document::from_json(&json.to_string()).unwrap();
    old.release_pins(&reg);

    let report = old.recompute(&reg, &*k);
    let FeatureStatus::Error { message } = report.feature(up).unwrap() else {
        panic!("書き換える前は止まるはず: {:?}", report.feature(up))
    };
    assert!(message.contains("f2:end#1#1#1"), "{message}");

    assert_eq!(old.rename_old_faces(&reg, &report), 1);
    assert!(close(final_mass(&mut old, &k).volume, VOLUME, 1e-6));
    let saved = old.to_json();
    assert!(
        saved.contains(&format!("\"f2:end~f{}#1\"", third.0)),
        "{saved}"
    );
    assert!(!saved.contains("#1#1#1"), "{saved}");
    // 書き換えは取り消しの記録にしない。開き直すと同じ形で、もう書き換える物は無い
    assert!(!old.can_undo());
    let mut again = Document::from_json(&saved).unwrap();
    let report = again.recompute(&reg, &*k);
    assert_ok(&report);
    assert_eq!(again.rename_old_faces(&reg, &report), 0);
}
