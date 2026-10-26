//! 目の開け閉め(操作「表示/非表示」と文書の区画 `cad.visibility`)の通しの試験。
//!
//! 確かめること:
//! - 区画の無い文書は、後の機能が使っているスケッチを隠した状態で読め、読むだけでは変更にならない。
//! - 目を替えると区画を直す変更が 1 つ増え(取り消せる)、形は計算し直さない。取り消しも計算し直さない。
//! - ボディはどの機能で替えても最初に作った機能で覚え、そのボディの機能が全部隠れる。
//! - 保存して開き直しても目は閉じたまま。
//! - 引数を省くと、1 つでも見えていれば隠し、全部隠れていれば出す。知らない機能は入力の誤り。
//! - 機能を足すか直して、スケッチと作業平面を初めて使うと同じ変更で隠し、取り消せば目も戻る。目を
//!   開けた後に別の機能が使っても隠さない。区画の無い文書は、初めて使った時に区画を書き始める。
//! - 作る途中(OK の前)に輪郭を選び替えると、その途中で隠したスケッチを同じ変更で出し直す。OK の後と、
//!   目を自分で替えた物と、ほかの機能が使っている物は出し直さない。

use kmb_core::{FeatureId, ProblemKind, category};
use kmb_interchange::visibility::Visibility;
use kmb_session::{Headless, Ran};
use serde_json::json;

fn all_of(h: &Headless, cat: &str) -> Vec<FeatureId> {
    let reg = &h.modules().registry;
    h.doc()
        .data()
        .features
        .iter()
        .filter(|f| reg.category(&f.type_id) == Some(cat))
        .map(|f| f.id)
        .collect()
}

fn set(h: &mut Headless, args: serde_json::Value) -> Result<Ran, kmb_core::Problem> {
    let id = h
        .action_for_role(&kmb_interchange::role::action::SET_VISIBILITY)
        .unwrap();
    h.run(&id, None, None, &args)
}

#[test]
fn eyes_are_saved_undone_and_never_recompute_shapes() {
    let mut h = Headless::with_document(
        kmb_bundle::modules(),
        kmb_samples::reference_document(),
        None,
    );
    assert_eq!(h.report().error_count(), 0);
    // 見本のスケッチは、どれも後の機能が使っている
    let sketches = all_of(&h, category::SKETCH);
    let body = all_of(&h, category::SOLID)[0];
    let ids = |v: &[FeatureId]| json!(v.iter().map(|f| f.0).collect::<Vec<_>>());
    let with_body: std::collections::BTreeSet<FeatureId> =
        sketches.iter().copied().chain([body]).collect();
    let last = h.doc().data().features.last().unwrap().id;

    // 区画の無い文書: 使われているスケッチが隠れている
    assert_eq!(h.hidden_features(), sketches);
    assert!(!h.doc().can_undo(), "読むだけで変更にしない");

    // ボディの最後の機能で隠すと、最初に作った機能で覚え、ボディの機能が全部隠れる
    assert!(matches!(
        set(&mut h, json!({"features": [last.0]})),
        Ok(Ran::Changed(_))
    ));
    assert_eq!(h.report().evaluated, 0, "目を替えて計算し直した");
    assert!(h.doc().can_undo());
    assert_eq!(h.visibility().hidden, with_body);
    let hidden = h.hidden_features();
    assert!(
        hidden.contains(&body) && hidden.contains(&last),
        "{hidden:?}"
    );
    let st = h.status();
    let row = |id: FeatureId| {
        st["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == id.0)
            .unwrap()
            .clone()
    };
    assert_eq!(row(body)["hidden"], true, "status に隠した印が出る");
    assert!(row(last).get("hidden").is_some());

    // 保存して開き直しても閉じたまま
    let dir = std::env::temp_dir().join(format!("kmb-visibility-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = h
        .save(Some(
            &dir.join(format!("目.{}", kmb_core::DOCUMENT_EXTENSION)),
        ))
        .unwrap();
    let again = Headless::open(kmb_bundle::modules(), &path).unwrap();
    assert_eq!(again.visibility().hidden, with_body);
    assert_eq!(again.hidden_features(), hidden);
    drop(again);
    let _ = std::fs::remove_dir_all(&dir);

    // 取り消すと区画の無い状態へ戻り、計算し直さない
    h.undo().unwrap();
    assert_eq!(h.report().evaluated, 0, "取り消しで計算し直した");
    assert!(!h.doc().data().sections.contains_key("cad.visibility"));
    assert_eq!(h.hidden_features(), sketches);

    // 省くと、全部隠れていれば出す。見せた目も区画に残り、開き直しても使ったスケッチを隠し直さない
    set(&mut h, json!({"features": ids(&sketches)})).unwrap();
    assert_eq!(h.visibility(), Visibility::default());
    assert!(h.hidden_features().is_empty());
    assert!(h.doc().data().sections.contains_key("cad.visibility"));
    // もう見えている物を見せても変更にしない
    let depth = h.doc().undo_depth();
    assert!(matches!(
        set(&mut h, json!({"features": ids(&sketches), "show": true})),
        Ok(Ran::Message(_))
    ));
    assert_eq!(h.doc().undo_depth(), depth);

    // まとまりの目はスケッチを全部隠し、行の目は変えない。原点の目も同じ操作で替える
    set(&mut h, json!({"folder": "sketches"})).unwrap();
    assert_eq!(h.hidden_features(), sketches);
    assert!(h.visibility().hidden.is_empty());
    set(&mut h, json!({"origin": true})).unwrap();
    assert!(h.visibility().origin_shown);
    assert_eq!(h.report().evaluated, 0);

    // 知らない機能・まとまりと、対象の無い呼び出しは入力の誤り
    for bad in [
        json!({"features": [99999]}),
        json!({"folder": "nothing"}),
        json!({}),
    ] {
        let Err(e) = set(&mut h, bad.clone()) else {
            panic!("{bad}: 断らなかった");
        };
        assert_eq!(e.kind, ProblemKind::Input, "{bad}: {}", e.message);
    }
}

/// 行の目を閉じた機能。
fn hidden_of(h: &Headless) -> std::collections::BTreeSet<FeatureId> {
    h.visibility().hidden
}

#[test]
fn a_feature_hides_what_it_first_uses_in_the_same_undo_step() {
    let mut h = Headless::with_document(
        kmb_bundle::modules(),
        kmb_samples::reference_document(),
        None,
    );
    let sketch = all_of(&h, category::SKETCH)[0];
    let data = |h: &Headless, id: FeatureId| h.doc().data().feature(id).unwrap().clone();
    let s = data(&h, sketch);
    let e = h
        .doc()
        .data()
        .features
        .iter()
        .find(|f| f.data["profile"] == json!(sketch.0))
        .unwrap()
        .clone();
    let extrude = |profile: FeatureId| {
        let mut d = e.data.clone();
        d["profile"] = json!(profile.0);
        d
    };

    // 区画の無い文書(新しい文書も): 初めて使った時に、区画の無い時の見え方で区画を書き始める。
    // 後で使うのをやめても出し直さない(区画が無いままだと、使われなくなった物が出てしまう)
    let s2 = h.add_feature(&s.type_id, None, s.data.clone()).unwrap();
    let ex = h.add_feature(&e.type_id, None, extrude(s2)).unwrap();
    assert!(h.doc().data().sections.contains_key("cad.visibility"));
    assert!(hidden_of(&h).contains(&s2) && hidden_of(&h).contains(&sketch));
    h.apply_edits(
        "輪郭を戻す",
        vec![kmb_core::Edit::SetData {
            id: ex,
            data: extrude(sketch),
        }],
    )
    .unwrap();
    assert!(hidden_of(&h).contains(&s2), "使うのをやめたら出し直した");
    h.undo().unwrap();
    h.undo().unwrap();
    // 取り消せば区画の無い文書へ戻る
    assert!(!h.doc().data().sections.contains_key("cad.visibility"));
    h.undo().unwrap();

    // 使っているスケッチの目を開ける。別の機能が同じスケッチを使っても、もう隠さない
    set(&mut h, json!({"features": [sketch.0], "show": true})).unwrap();
    h.add_feature(&e.type_id, None, extrude(sketch)).unwrap();
    assert!(!hidden_of(&h).contains(&sketch), "目を開けた後に隠した");

    // 初めて使うスケッチは、機能を足す変更と同じ 1 手で隠れる
    let s3 = h.add_feature(&s.type_id, None, s.data.clone()).unwrap();
    assert!(!hidden_of(&h).contains(&s3), "使う前に隠した");
    let depth = h.doc().undo_depth();
    h.add_feature(&e.type_id, None, extrude(s3)).unwrap();
    assert_eq!(h.doc().undo_depth(), depth + 1, "変更が 2 手に分かれた");
    assert!(hidden_of(&h).contains(&s3));
    assert_eq!(h.report().error_count(), 0);
    // 取り消せば機能と一緒に目も戻り、やり直せばまた隠れる
    h.undo().unwrap();
    assert!(!hidden_of(&h).contains(&s3));
    h.redo().unwrap();
    assert!(hidden_of(&h).contains(&s3));

    // 1 回の変更でスケッチと、それを使う機能を足しても隠れる(操作の結果の変更と同じ道)
    let added = h
        .apply_edits(
            "スケッチと押し出し",
            vec![
                kmb_core::Edit::Insert {
                    type_id: s.type_id.clone(),
                    name: "s4".into(),
                    data: s.data.clone(),
                },
                kmb_core::Edit::Insert {
                    type_id: e.type_id.clone(),
                    name: "e4".into(),
                    data: {
                        let mut d = e.data.clone();
                        d["profile"] = json!({"$new": 0});
                        d
                    },
                },
            ],
        )
        .unwrap();
    assert!(hidden_of(&h).contains(&added[0]));

    // 足した機能を直して、別のスケッチを初めて使った時も隠れる(画面は足してから輪郭を選ぶ)。
    // 前に使っていたスケッチは隠したまま(道具は目を出し直さない)
    let s5 = h.add_feature(&s.type_id, None, s.data.clone()).unwrap();
    let depth = h.doc().undo_depth();
    h.apply_edits(
        "輪郭を替える",
        vec![kmb_core::Edit::SetData {
            id: added[1],
            data: extrude(s5),
        }],
    )
    .unwrap();
    assert_eq!(h.doc().undo_depth(), depth + 1);
    assert!(hidden_of(&h).contains(&s5));
    assert!(hidden_of(&h).contains(&added[0]));
    h.undo().unwrap();
    assert!(!hidden_of(&h).contains(&s5));

    // 作業平面も、スケッチを置いて初めて使った時に隠れる
    let plane = h
        .add_feature(
            kmb_cad_datum::DATUM_PLANE,
            None,
            json!({"method": "offset", "base": {"kind": "xy"}, "distance": "10 mm"}),
        )
        .unwrap();
    assert!(!hidden_of(&h).contains(&plane));
    let mut on_plane = s.data.clone();
    on_plane["plane"] = json!({"kind": "datum", "feature": plane.0});
    h.add_feature(&s.type_id, None, on_plane).unwrap();
    assert!(hidden_of(&h).contains(&plane));
    assert_eq!(h.report().error_count(), 0);
}

#[test]
fn re_picking_while_creating_shows_again_what_the_creation_hid() {
    let mut h = Headless::with_document(
        kmb_bundle::modules(),
        kmb_samples::reference_document(),
        None,
    );
    let sketch = all_of(&h, category::SKETCH)[0];
    let s = h.doc().data().feature(sketch).unwrap().clone();
    let e = h
        .doc()
        .data()
        .features
        .iter()
        .find(|f| f.data["profile"] == json!(sketch.0))
        .unwrap()
        .clone();
    let extrude = |profile: FeatureId| {
        let mut d = e.data.clone();
        d["profile"] = json!(profile.0);
        d
    };
    let pick = |h: &mut Headless, id: FeatureId, profile: FeatureId| {
        h.apply_edits(
            "輪郭を替える",
            vec![kmb_core::Edit::SetData {
                id,
                data: extrude(profile),
            }],
        )
        .unwrap();
    };
    let s2 = h.add_feature(&s.type_id, None, s.data.clone()).unwrap();
    let s3 = h.add_feature(&s.type_id, None, s.data.clone()).unwrap();

    // 区画の無い文書で足して作る途中にする。選び替えると前のスケッチが同じ 1 手で出る
    let ex = h.add_feature(&e.type_id, None, extrude(s2)).unwrap();
    h.begin_creation(ex);
    assert!(hidden_of(&h).contains(&s2));
    let depth = h.doc().undo_depth();
    pick(&mut h, ex, s3);
    assert_eq!(h.doc().undo_depth(), depth + 1, "変更が 2 手に分かれた");
    assert!(hidden_of(&h).contains(&s3));
    assert!(
        !hidden_of(&h).contains(&s2),
        "選び替えた前のスケッチが隠れたまま"
    );
    h.undo().unwrap();
    assert!(hidden_of(&h).contains(&s2) && !hidden_of(&h).contains(&s3));
    h.redo().unwrap();

    // ほかの機能が使っているスケッチは、選んでから外しても隠したまま
    pick(&mut h, ex, sketch);
    pick(&mut h, ex, s3);
    assert!(
        hidden_of(&h).contains(&sketch),
        "ほかの機能が使う物を出した"
    );

    // 作る途中でも、目を自分で替えた物は出し直さない
    set(&mut h, json!({"features": [s3.0], "show": true})).unwrap();
    set(&mut h, json!({"features": [s3.0], "show": false})).unwrap();
    pick(&mut h, ex, s2);
    assert!(hidden_of(&h).contains(&s3), "目を自分で閉じた物を出した");

    // OK の後に選び替えても出し直さない
    h.end_creation();
    pick(&mut h, ex, s3);
    assert!(hidden_of(&h).contains(&s2), "OK の後に出し直した");
    assert_eq!(h.report().error_count(), 0);
}
