//! 目の開け閉め(操作「表示/非表示」と文書の区画 `cad.visibility`)の通しの試験。
//!
//! 確かめること:
//! - 区画の無い文書は、後の機能が使っているスケッチを隠した状態で読め、読むだけでは変更にならない。
//! - 目を替えると区画を直す変更が 1 つ増え(取り消せる)、形は計算し直さない。取り消しも計算し直さない。
//! - ボディはどの機能で替えても最初に作った機能で覚え、そのボディの機能が全部隠れる。
//! - 保存して開き直しても目は閉じたまま。
//! - 引数を省くと、1 つでも見えていれば隠し、全部隠れていれば出す。知らない機能は入力の誤り。

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
        let e = set(&mut h, bad.clone()).err().expect("断る");
        assert_eq!(e.kind, ProblemKind::Input, "{bad}: {}", e.message);
    }
}
