//! 式の欄の名前の変更が漏れないか。
//!
//! 機能は式の欄を `inputs`(依存)と `rename_param`(パラメータの名前の変更)に別々に書くので、
//! 欄を足した時に片方を書き忘れると、パラメータの名前を変えた時にその欄だけ古い名前のまま残り、
//! 文書が開けなくなる。見本の文書の全部の機能について、
//! 記述子の式の欄に「パラメータ p を使う式」を入れ、`rename_param("p", "q")` が全部を書き換え、
//! `inputs` が p を挙げるかを確かめる。

use kmb_core::{Document, FieldKind, InputRef, Registry};
use serde_json::Value;
use std::collections::BTreeSet;

const OLD: &str = "contract_p";
const NEW: &str = "contract_q";

/// 見本の文書の全部。
fn documents() -> Vec<Document> {
    let mut v: Vec<Document> = kmb_bundle::Sample::ALL.iter().map(|s| s.make()()).collect();
    v.extend([
        kmb_samples::sample_document(),
        kmb_samples::sketch_dims_document(),
        kmb_samples::sketch_tools_document(),
        kmb_samples::fillet_then_chamfer_document(),
    ]);
    v
}

/// データ `d` の式の欄(記述子の `fields`。行の欄は行ごと)の JSON の場所。
fn expr_pointers(fields: &[kmb_core::Field], d: &Value, prefix: &str, out: &mut Vec<String>) {
    for f in fields {
        let at = format!("{prefix}/{}", f.key.replace('.', "/"));
        match &f.kind {
            FieldKind::Expr { .. } => {
                // 親が無い入れ子の欄は、その機能では使っていない
                let parent = at.rsplit_once('/').map_or("", |(p, _)| p);
                if d.pointer(parent).is_some_and(Value::is_object) {
                    out.push(at);
                }
            }
            FieldKind::Rows { fields } => {
                if let Some(rows) = d.pointer(&at).and_then(Value::as_array) {
                    for i in 0..rows.len() {
                        expr_pointers(fields, d, &format!("{at}/{i}"), out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn check(reg: &Registry, type_id: &str, data: &Value) -> Result<usize, String> {
    let Some(desc) = reg.descriptor(type_id) else {
        return Ok(0);
    };
    let mut at = vec![];
    expr_pointers(&desc.fields, data, "", &mut at);
    if at.is_empty() {
        return Ok(0);
    }
    let mut d = data.clone();
    for p in &at {
        let (parent, key) = p.rsplit_once('/').expect("欄の場所");
        let o = d
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .expect("親は物");
        o.insert(key.to_string(), Value::String(format!("{OLD} * 1")));
    }
    // 雛形のデータは選び(面・ボディ)が空のことがあり、読めないか inputs が選びを求めて誤りになる。
    // 読めなければ確かめられない(数えない)。inputs が誤りなら名前の変更だけを確かめる
    let Ok(f) = reg.instantiate(type_id, &d).expect("登録されている") else {
        return Ok(0);
    };
    if let Ok(inputs) = f.inputs()
        && !inputs.contains(&InputRef::Param { name: OLD.into() })
    {
        return Err(format!("inputs が {OLD} を挙げない(式の欄 {at:?})"));
    }
    let renamed = f
        .rename_param(OLD, NEW)
        .map_err(|e| format!("rename_param が誤り: {e:?}"))?
        .ok_or_else(|| format!("rename_param が何も書き換えない(式の欄 {at:?})"))?;
    let left: Vec<&String> = at
        .iter()
        .filter(|p| renamed.pointer(p).and_then(Value::as_str) != Some(&format!("{NEW} * 1")))
        .collect();
    if !left.is_empty() {
        return Err(format!("名前の変更で書き換わらない式の欄: {left:?}"));
    }
    Ok(at.len())
}

/// 加工の戦略の初めのデータを作る時の、決まった選び(セットアップ 1・工具ライブラリ 2・線の元 3)。
struct Fixed;

impl kmb_cam_setup::Picks for Fixed {
    fn base(&self) -> kmb_cam_setup::OpBase {
        kmb_cam_setup::OpBase::new(kmb_core::FeatureId(1), kmb_core::FeatureId(2), 1)
    }
    fn tool_of_kind(&self, _: kmb_cam_tool::ToolKind) -> Option<u32> {
        Some(1)
    }
    fn smallest_flat(&self) -> Option<u32> {
        Some(1)
    }
    fn planar_face(&self) -> Option<String> {
        Some("f1:end".into())
    }
    fn wall_face(&self) -> Option<String> {
        Some("f1:side".into())
    }
    fn floor_faces(&self) -> Result<(String, Vec<String>), String> {
        Ok(("f1:end".into(), vec![]))
    }
    fn selected_profile(&self) -> Option<kmb_core::FeatureId> {
        Some(kmb_core::FeatureId(3))
    }
    fn sketch_depth(&self, _: kmb_core::FeatureId) -> Option<String> {
        Some("1 mm".into())
    }
    fn source(&self, _: &str) -> Option<kmb_core::FeatureId> {
        Some(kmb_core::FeatureId(3))
    }
    fn setup_data(&self) -> Option<Value> {
        None
    }
}

/// 見本の文書に無い種類のデータ: 加工の戦略の初めの値と、記述子の雛形。
fn more_data(reg: &Registry) -> Vec<(String, Value)> {
    let mut v: Vec<(String, Value)> = kmb_bundle::strategies()
        .iter()
        .filter_map(|s| Some((s.type_id.to_string(), (s.make)(&Fixed).ok()?)))
        .collect();
    v.extend(
        reg.descriptors()
            .into_iter()
            .filter_map(|d| Some((d.type_id.clone(), d.template.clone()?))),
    );
    v
}

#[test]
fn every_expression_field_follows_a_parameter_rename() {
    let reg = kmb_bundle::registry();
    let mut seen = BTreeSet::new();
    let mut bad = vec![];
    let mut fields = 0;
    for doc in documents() {
        for e in &doc.data().features {
            if !seen.insert(e.type_id.clone()) {
                continue;
            }
            match check(&reg, &e.type_id, &e.data) {
                Ok(n) => fields += n,
                Err(m) => bad.push(format!("{}: {m}", e.type_id)),
            }
        }
    }
    for (t, d) in more_data(&reg) {
        if !seen.insert(t.clone()) {
            continue;
        }
        match check(&reg, &t, &d) {
            Ok(n) => fields += n,
            Err(m) => bad.push(format!("{t}: {m}")),
        }
    }
    eprintln!(
        "名前の変更を確かめた種類 {}・式の欄 {fields}(登録 {})",
        seen.len(),
        reg.type_ids().count()
    );
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
