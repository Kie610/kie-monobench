use super::*;

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn apply(v: &[&str], tag: &str) -> Vec<String> {
    let mut n = names(v);
    for (i, new) in shorten(&n.clone(), tag) {
        n[i] = new;
    }
    n
}

/// 1 回目の分割と割れていない面はそのまま、2 回目からは「元 + 割った機能 + 番号」。
#[test]
fn a_face_split_again_gets_the_splitting_feature_once() {
    assert_eq!(
        apply(&["a:end#0", "a:end#1", "a:start"], "f9"),
        names(&["a:end#0", "a:end#1", "a:start"])
    );
    assert_eq!(
        apply(&["a:end#0", "a:end#1#1", "a:end#1#0"], "f12"),
        names(&["a:end#0", "a:end~f12#1", "a:end~f12#0"])
    );
    // 3 回目も長さは増えない
    assert_eq!(
        apply(
            &["a:end#0", "a:end~f12#0", "a:end~f12#1#0", "a:end~f12#1#1"],
            "f15"
        ),
        names(&["a:end#0", "a:end~f12#0", "a:end~f15#0", "a:end~f15#1"])
    );
    // 1 つの機能の中で何度も割った物も 1 段にする。番号は数として並べる
    let many: Vec<String> = (0..12).map(|k| format!("a:end#1#{k}#0")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let out = apply(&many, "f3");
    assert_eq!(out[2], "a:end~f3#2");
    assert_eq!(out[11], "a:end~f3#11");
    // 名前の中の別の名前(内側の面)も、末尾で数える
    assert_eq!(
        apply(&["f9:in:a:end#1#0", "f9:in:a:end#1#1"], "f9"),
        names(&["f9:in:a:end~f9#0", "f9:in:a:end~f9#1"])
    );
}

/// そろえた名前の前の名前は、名前の中に入った物(丸めの面・内側の面)も戻す。
#[test]
fn old_names_come_back_inside_other_names() {
    let mut a = FaceAliases::default();
    a.insert("f2:end~f12#1".into(), "f2:end#1#1".into());
    a.insert("f2:end~f15#0".into(), "f2:end#1#1#0".into());
    assert_eq!(a.legacy("f2:end~f12#1#0"), "f2:end#1#1#0");
    assert_eq!(
        a.legacy("f20:f2:end~f12#1|f2:side:e1"),
        "f20:f2:end#1#1|f2:side:e1"
    );
    assert_eq!(a.legacy("f2:end~f12#10"), "f2:end~f12#10");
    assert_eq!(a.legacy("f2:end#0"), "f2:end#0");

    let cur = a.current();
    assert_eq!(
        rename_old("f2:end#1#1#0", &cur).as_deref(),
        Some("f2:end~f15#0")
    );
    assert_eq!(
        rename_old("f20:f2:end#1#1|f2:side:e1", &cur).as_deref(),
        Some("f20:f2:end~f12#1|f2:side:e1")
    );
    assert_eq!(rename_old("f2:end#1#10", &cur), None);
    assert_eq!(rename_old("f2:end#0", &cur), None);
    assert_eq!(rename_old("面の名前#1", &cur), None);
}

/// 2 つのボディで同じそろえた名前が違う前の名前を持てば、どちらにも書き換えない(黙って付け替えない)。
#[test]
fn an_ambiguous_old_name_is_not_rewritten() {
    let mut a = FaceAliases::default();
    a.insert("f2:end~f12#0".into(), "f2:end#1#0".into());
    a.insert("f2:end~f12#0".into(), "f2:end#0#0".into());
    a.insert("f2:end~f12#1".into(), "f2:end#1#1".into());
    a.insert("f3:end~f12#1".into(), "f2:end#1#1".into());
    assert!(a.current().is_empty(), "{:?}", a.current());
}
