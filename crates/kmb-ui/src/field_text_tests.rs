//! 確かめること: 欄の訳と記述子の一致、行の中の欄、専用の編集器と汎用の欄が使う文言の一致。

use crate::*;
use kmb_core::{Field, FieldKind};
use kmb_i18n::{Catalog, Lang};
use std::collections::BTreeMap;

#[test]
fn every_descriptor_field_has_texts_matching_its_module() {
    fn check(texts: &Catalog, kind: &str, fields: &[Field], parent: &str, seen: &mut BTreeMap<String, String>, wrong: &mut Vec<String>) {
        for f in fields {
            let key = if parent.is_empty() { f.key.clone() } else { format!("{parent}.{}", f.key) };
            let id = kmb_i18n::field_id(kind, &key);
            for (id, original) in [(id.clone(), &f.label), (format!("{id}-help"), &f.help)] {
                if original.is_empty() { continue; }
                if let Some(other) = seen.insert(id.clone(), original.clone()) {
                    assert_eq!(other, *original, "意味の違う欄で ID {id} が重なっている");
                }
                if !texts.has(Lang::Ja, &id) || texts.text(Lang::Ja, &id) != *original {
                    wrong.push(format!("{id} = {original}"));
                }
                if !texts.has(Lang::En, &id) { wrong.push(format!("英語に {id} が無い")); }
            }
            if let FieldKind::Rows { fields } = &f.kind {
                check(texts, kind, fields, &key, seen, wrong);
            }
        }
    }
    let texts = Catalog::with_os(Lang::Ja);
    let mut wrong = vec![];
    let mut seen = BTreeMap::new();
    for d in kmb_bundle::registry().descriptors() {
        check(&texts, &d.type_id, &d.fields, "", &mut seen, &mut wrong);
    }
    assert!(!seen.is_empty());
    assert!(wrong.is_empty(), "日本語の欄の文言を記述子とそろえる:\n{}", wrong.join("\n"));
}

#[test]
fn custom_panels_use_the_same_field_texts_and_keep_unknown_defaults() {
    let ctx = egui::Context::default();
    let mut ws = Workspace::new(kmb_samples::sample_document(), None, kmb_bundle::modules());
    let f = Field::expr("distance", "距離").help("範囲が「すべて」「対象まで」の時は使わない");
    for (lang, name) in [("en", "Distance"), ("ko", "Distance"), ("ja", "距離")] {
        ws.win.layout.appearance.language = lang.into();
        ws.win.words().share(&ctx);
        let translated = translated_field(&ctx, "solid.extrude@1", &f);
        assert_eq!(translated.label, name);
        assert_eq!(translated.key, f.key);
        assert_eq!(translated.kind, f.kind);
        assert_eq!(translated.help, ws.win.texts.text(ws.win.lang(), "field-solid-extrude-distance-help"));
        let unknown = Field::text("plugin_value", "独自の欄").help("独自の説明");
        assert_eq!(translated_field(&ctx, "plugin.custom@1", &unknown), unknown);
    }
}
