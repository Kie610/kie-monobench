//! 設定の細かい項目(候補 19)の試験: 文書の長さの単位と、新しい文書の既定の値。

use super::*;
use kmb_core::{Document, Field, LengthUnit};

fn reg() -> kmb_core::Modules {
    kmb_bundle::modules()
}

fn inch(ws: &mut Workspace) {
    let mut p = ws.document().data().properties.clone();
    p.units = LengthUnit::In;
    assert!(ws.set_properties(p));
}

#[test]
fn new_documents_get_the_defaults_but_opened_ones_do_not() {
    let mut ws = Workspace::new(Document::new(), None, reg());
    {
        let nd = &mut ws.layout_mut().new_doc;
        nd.units = LengthUnit::In;
        nd.designer = "Kie".into();
        nd.license = "CC BY 4.0".into();
    }
    // 空の無題のタブは使い回し、既定の値を入れる。取り消しの履歴には入れず、変更にもしない
    ws.new_document();
    let p = &ws.document().data().properties;
    assert_eq!(
        (p.units, p.designer.as_str(), p.license.as_str()),
        (LengthUnit::In, "Kie", "CC BY 4.0")
    );
    assert!(!ws.document().can_undo() && !ws.is_modified());
    assert_eq!(ws.length_unit(), LengthUnit::In);
    // 見本(開いた文書)には入れない
    let mut ws = Workspace::new(kmb_samples::sample_document(), None, reg());
    ws.layout_mut().new_doc.units = LengthUnit::In;
    assert_eq!(ws.length_unit(), LengthUnit::Mm);
    // 中身のある文書から新しい文書を作ると新しいタブに入る
    ws.new_document();
    assert_eq!(ws.win.doc_tabs.len(), 2);
    assert_eq!(ws.length_unit(), LengthUnit::In);
    ws.switch_tab(0);
    assert_eq!(ws.length_unit(), LengthUnit::Mm);
}

#[test]
fn typed_numbers_are_read_in_the_document_unit_and_values_shown_in_it() {
    let mut ws = Workspace::new(Document::new(), None, reg());
    let f = Field::expr("distance", "距離").length();
    // mm の文書は mm のまま
    assert_eq!(feature_ui::typed_expr(&ws, &f, "10 mm", "0.5"), "0.5 mm");
    inch(&mut ws);
    assert_eq!(feature_ui::typed_expr(&ws, &f, "10 mm", "0.5"), "0.5 in");
    assert_eq!(feature_ui::typed_expr(&ws, &f, "", "2"), "2 in");
    // 単位を書けばそのまま、角度・式・個数は変えない
    assert_eq!(feature_ui::typed_expr(&ws, &f, "10 mm", "3 mm"), "3 mm");
    assert_eq!(
        feature_ui::typed_expr(&ws, &Field::expr("angle", "角度").angle(), "", "30"),
        "30 deg"
    );
    assert_eq!(
        feature_ui::typed_expr(&ws, &Field::expr("count", "個数").unitless(), "", "4"),
        "4"
    );
    // 値はインチで出す(形と保存は mm のまま)
    let q = ws.eval_expr("0.5 in").unwrap();
    assert!((q.value - 12.7).abs() < 1e-9);
    assert_eq!(ws.shown(q), "0.5 in");
    assert_eq!(ws.shown(kmb_core::Quantity::deg(30.0)), "30 deg");
    assert_eq!(ws.in_doc_unit("3"), "3 in");
    assert_eq!(ws.in_doc_unit("w / 2"), "w / 2");
    assert_eq!(ws.param_unit_label(kmb_core::ParamUnit::Length), "in");
    // 単位を変えるのは取り消せる
    ws.undo();
    assert_eq!(ws.length_unit(), LengthUnit::Mm);
    assert_eq!(ws.param_unit_label(kmb_core::ParamUnit::Length), "mm");
}

#[test]
fn the_new_document_defaults_are_remembered_with_the_layout() {
    let mut l = Layout::default();
    l.new_doc.units = LengthUnit::Cm;
    l.new_doc.organization = "工房".into();
    let back: Layout = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
    assert_eq!(back.new_doc, l.new_doc);
    // 配置を戻しても既定の値は残す
    let mut r = l.clone();
    r.reset_arrangement();
    assert_eq!(r.new_doc, l.new_doc);
    // 前の layout.json(項目なし)は既定のまま読める
    let old: Layout = serde_json::from_str("{}").unwrap();
    assert_eq!(old.new_doc, layout::NewDoc::default());
    let p = l.new_doc.properties();
    assert_eq!((p.units, p.organization.as_str()), (LengthUnit::Cm, "工房"));
}

/// 組み込みの機能・設定の数と式の欄は、みな既定の単位(長さ・角度・割合・式に書けない単位)か、
/// 例外の印(単位の無い数・式そのもの)を持つ。持たない欄は数だけを打っても単位を補わないので、
/// 足し忘れるとこの試験が欄を挙げて落ちる(欄の名前からは推し量らない)。
#[test]
fn every_number_field_declares_its_default_unit_or_an_exception() {
    use kmb_core::FieldKind;
    fn walk(owner: &str, fields: &[Field], out: &mut Vec<String>) {
        for f in fields {
            match &f.kind {
                // 点の欄は長さ、向きの欄は単位の無い数(前は点の欄が宣言を持たず mm のままだった)
                FieldKind::Expr { .. } | FieldKind::Number { .. } | FieldKind::Vector
                    if f.unit.is_none() =>
                {
                    out.push(format!("{owner} の「{}」({})", f.label, f.key));
                }
                FieldKind::Rows { fields } => walk(&format!("{owner} の {}", f.label), fields, out),
                _ => {}
            }
        }
    }
    let m = reg();
    let mut missing = Vec::new();
    let mut count = 0;
    for d in m.registry.descriptors() {
        count += d.fields.len();
        walk(&d.type_id, &d.fields, &mut missing);
    }
    for s in m.extensions.settings() {
        walk(&format!("設定 {}", s.id), &s.fields, &mut missing);
    }
    assert!(count > 300, "欄を数えられていない: {count}");
    assert!(
        missing.is_empty(),
        "既定の単位も例外の印も無い欄 {} 個:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

/// 長さの欄の既定の単位は、設定(文書の長さの単位)に従う。ほかの量は数のまま。
#[test]
fn length_fields_follow_the_unit_setting_and_other_quantities_keep_theirs() {
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, reg());
    let size = Field::opt_expr("size", "要素の大きさ").length();
    let temp = Field::opt_expr("ambient", "まわりの温度(℃)").plain("℃");
    let angle = Field::expr("angle", "角度").angle();
    for (unit, want) in [
        (LengthUnit::Mm, "2 mm"),
        (LengthUnit::Cm, "2 cm"),
        (LengthUnit::M, "2 m"),
        (LengthUnit::In, "2 in"),
        (LengthUnit::Ft, "2 ft"),
    ] {
        let mut p = ws.document().data().properties.clone();
        p.units = unit;
        assert!(ws.set_properties(p) || unit == LengthUnit::Mm);
        // 今の値の単位(前に打った cm など)には引きずられない
        assert_eq!(feature_ui::typed_expr(&ws, &size, "10 cm", "2"), want);
        assert_eq!(feature_ui::typed_expr(&ws, &temp, "20", "25"), "25");
        assert_eq!(feature_ui::typed_expr(&ws, &angle, "", "30"), "30 deg");
    }
}

/// パラメータの表の「機能の値」と MCP・コマンドの set_value も、編集パネルと同じく欄の既定の
/// 単位で補う。前は今の値の表示(長さはいつも mm)の単位を補い、インチの文書で「0.5」が
/// 0.5 mm になった(編集パネルなら 0.5 in で、同じ値が欄によって 25.4 倍ずれた)。
#[test]
fn the_parameter_table_reads_bare_numbers_in_the_field_unit_too() {
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, reg());
    ws.wait();
    inch(&mut ws);
    ws.wait();
    let m = reg();
    let vals = kmb_session::params::feature_values(ws.document().data(), &m.registry, ws.report());
    let lengths: Vec<_> = vals
        .iter()
        .filter(|v| {
            matches!(&v.status, kmb_core::ParamStatus::Ok { value } if value.dim == kmb_core::Dim::LENGTH)
        })
        .collect();
    assert!(!lengths.is_empty());
    for v in lengths {
        let tx = kmb_session::params::set_feature_value(
            ws.document().data(),
            &m.registry,
            ws.report(),
            v.feature,
            &v.key,
            "0.5",
        )
        .unwrap();
        assert!(tx.label.contains("0.5 in"), "{}", tx.label);
    }
}

/// % の欄(残す体積・出力)に「40%」と打つと 40 にする(読む側は数だけを受ける)。割合の欄は 0.4。
#[test]
fn a_percent_sign_is_taken_off_in_percent_fields() {
    let ws = Workspace::new(Document::new(), None, reg());
    let f = Field::expr("volume", "残す体積(%)").plain("%");
    assert_eq!(feature_ui::typed_expr(&ws, &f, "", "40%"), "40");
    assert_eq!(feature_ui::typed_expr(&ws, &f, "", "40"), "40");
    let r = Field::expr("ratio", "割合").ratio();
    assert_eq!(feature_ui::typed_expr(&ws, &r, "", "30%"), "0.3");
}

/// 長さの欄の名前の「(mm…)」は文書の単位で出す。送りの欄はインチの文書で in/min と読む。
#[test]
fn length_field_names_show_the_document_unit() {
    let mut ws = Workspace::new(Document::new(), None, reg());
    let feed = Field::opt_expr("feed", "切削送り(mm/min)").length();
    let temp = Field::expr("t", "温度(℃)").plain("℃");
    assert_eq!(feature_ui::field_label(&ws, &feed), "切削送り(mm/min)");
    inch(&mut ws);
    assert_eq!(feature_ui::field_label(&ws, &feed), "切削送り(in/min)");
    assert_eq!(feature_ui::field_label(&ws, &temp), "温度(℃)");
}
