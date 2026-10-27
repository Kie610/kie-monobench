use super::*;
use std::collections::BTreeSet;

fn ids_of(lang: Lang) -> BTreeSet<&'static str> {
    FILES
        .iter()
        .filter(|(l, _, _)| *l == lang)
        .flat_map(|(_, _, src)| ids(src))
        .collect()
}

#[test]
fn every_file_parses() {
    for (lang, area, src) in FILES {
        if let Err((_, errs)) = FluentResource::try_new(src.to_string()) {
            panic!("{}/{area}.ftl: {errs:?}", lang.code());
        }
    }
}

#[test]
fn japanese_and_english_have_the_same_ids_and_others_add_none() {
    let ja = ids_of(Lang::Ja);
    let en = ids_of(Lang::En);
    assert!(!ja.is_empty());
    assert_eq!(ja, en, "日本語と英語は同じ ID の組を持つ");
    for lang in [Lang::ZhHans, Lang::ZhHant, Lang::Ko] {
        let extra: Vec<_> = ids_of(lang).difference(&en).copied().collect();
        assert!(
            extra.is_empty(),
            "{} に英語に無い ID: {extra:?}",
            lang.code()
        );
    }
}

#[test]
fn ids_are_lowercase_with_the_area_prefix() {
    for (lang, area, src) in FILES {
        for id in ids(src) {
            assert!(
                id.starts_with(&format!("{area}-"))
                    && id
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{}/{area}.ftl の ID {id} は小文字とハイフンで、頭が「{area}-」",
                lang.code()
            );
        }
    }
}

#[test]
fn every_file_in_the_folders_is_built_in() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    for lang in Lang::ALL {
        let dir = root.join(lang.code());
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter_map(|e| {
                e.file_name()
                    .to_str()?
                    .strip_suffix(".ftl")
                    .map(String::from)
            })
            .collect();
        on_disk.sort();
        let mut built: Vec<String> = FILES
            .iter()
            .filter(|(l, _, _)| *l == lang)
            .map(|(_, a, _)| a.to_string())
            .collect();
        built.sort();
        assert_eq!(
            on_disk,
            built,
            "{} のファイルを FILES に並べる",
            lang.code()
        );
    }
}

#[test]
fn missing_text_falls_back_to_english_then_japanese_then_the_id() {
    let c = Catalog::with_os(Lang::Ja);
    assert_eq!(c.text(Lang::Ja, "settings-window-title"), "設定");
    assert_eq!(c.text(Lang::En, "settings-window-title"), "Settings");
    assert!(!c.has(Lang::Ko, "settings-window-title"));
    assert_eq!(c.text(Lang::Ko, "settings-window-title"), "Settings");
    assert_eq!(c.text(Lang::En, "no-such-id"), "no-such-id");
}

#[test]
fn placeholders_and_plurals() {
    let c = Catalog::with_os(Lang::En);
    let found = |lang, n: usize| {
        c.text_with(
            lang,
            "settings-search-found",
            &[("query", "font".into()), ("count", n.into())],
        )
    };
    assert_eq!(found(Lang::En, 1), "1 page matches \"font\"");
    assert_eq!(found(Lang::En, 3), "3 pages match \"font\"");
    assert_eq!(found(Lang::Ja, 3), "「font」に当たる設定: 3 頁");
}

#[test]
fn os_locale_and_setting_values() {
    assert_eq!(Lang::from_locale("ja-JP"), Lang::Ja);
    assert_eq!(Lang::from_locale("zh-CN"), Lang::ZhHans);
    assert_eq!(Lang::from_locale("zh-Hans-SG"), Lang::ZhHans);
    assert_eq!(Lang::from_locale("zh-TW"), Lang::ZhHant);
    assert_eq!(Lang::from_locale("zh_HK.UTF-8"), Lang::ZhHant);
    assert_eq!(Lang::from_locale("ko-KR"), Lang::Ko);
    assert_eq!(Lang::from_locale("de-DE"), Lang::En);
    let c = Catalog::with_os(Lang::Ko);
    assert_eq!(c.resolve(Choice::parse("auto")), Lang::Ko);
    assert_eq!(c.resolve(Choice::parse("知らない")), Lang::Ko);
    assert_eq!(Choice::parse("zh-Hant"), Choice::Fixed(Lang::ZhHant));
    for ch in Choice::all() {
        assert_eq!(Choice::parse(ch.code()), ch);
    }
}

#[test]
fn field_ids_use_the_feature_kind_and_the_full_field_path() {
    assert_eq!(field_id("solid.extrude@1", "start_offset"), "field-solid-extrude-start-offset");
    assert_eq!(field_id("solid.edge@2", "edges.radius"), "field-solid-edge-edges-radius");
    assert_ne!(field_id("solid.edge@1", "radius"), field_id("solid.edge@1", "edges.radius"));
}

#[test]
fn untranslated_field_languages_use_english() {
    let c = Catalog::with_os(Lang::Ja);
    let id = "field-solid-extrude-distance";
    assert_eq!(c.text(Lang::Ja, id), "距離");
    for lang in [Lang::En, Lang::ZhHans, Lang::ZhHant, Lang::Ko] {
        assert_eq!(c.text(lang, id), "Distance");
    }
}
