//! パラメータの表の書き出しと読み込み(U54)。表(ビュー)ごとに CSV か JSON で出し入れする。
//! 表計算で値を管理する、同じ値を別の設計へ移すための物。新しい依存は入れず、CSV は
//! RFC 4180 の範囲(カンマ・二重引用符・改行を含む欄は二重引用符で囲む)で書き読みする。
//!
//! 読み込みの決まり(1 回の取り消しにまとめる):
//! - パラメータの行(`kind` が `param`): 同じ名前のパラメータがあれば式・単位・コメントを上書きし、
//!   なければ足す。
//! - 寸法(`dim`)と機能の値(`value`)の行: 同じ書き方(`d1`、`5:distance`)の行がこの文書にあれば
//!   式を上書きし、なければ飛ばす(別の設計の寸法は指せないため)。
//! - 式が今と同じ行は変えない。値の列(`value`)は読まない(式から計算し直す)。

use crate::params::{feature_values, key_label, key_text, parse_key};
use kmb_core::{
    Document, DocumentData, FeatureId, Op, Param, ParamKey, ParamStatus, ParamUnit,
    RecomputeReport, Registry, Transaction,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 書き出す形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Csv,
    Json,
}

impl Format {
    /// ファイルの拡張子から(大文字小文字は問わない)。
    pub fn from_extension(ext: &str) -> Option<Format> {
        match ext.to_ascii_lowercase().as_str() {
            "csv" => Some(Format::Csv),
            "json" => Some(Format::Json),
            _ => None,
        }
    }
}

/// 表の 1 行(書き出しと読み込みで同じ形)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// `param`(利用者のパラメータ)・`dim`(寸法)・`value`(機能の値)
    #[serde(default)]
    pub kind: String,
    /// 行の書き方: パラメータと寸法は名前、機能の値は `機能の ID:欄`
    pub row: String,
    /// 人が読む名前(「押し出し1 の距離」)。読み込みでは使わない
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub expr: String,
    /// 計算した値(書き出しだけ)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    /// パラメータの単位(`length`・`angle`・`unitless`・`text`・`any`)。パラメータの行だけ
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

/// JSON で書き出す時の外側。
#[derive(Debug, Serialize, Deserialize)]
struct Table {
    #[serde(default)]
    table: String,
    rows: Vec<Row>,
}

const COLUMNS: [&str; 7] = ["kind", "row", "label", "expr", "value", "unit", "comment"];

fn unit_text(u: ParamUnit) -> String {
    serde_json::to_value(u)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn status_text(s: Option<&ParamStatus>) -> String {
    match s {
        Some(ParamStatus::Ok { value }) => value.to_string(),
        Some(ParamStatus::Text { value }) => value.clone(),
        _ => String::new(),
    }
}

/// 表の行 `keys` を書き出す行にする(今ある物だけ)。
pub fn rows(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    keys: &[ParamKey],
) -> Vec<Row> {
    let values = feature_values(data, registry, report);
    keys.iter()
        .filter_map(|k| {
            let label = key_label(data, registry, report, k)?;
            let row = key_text(report, data, k);
            Some(match k {
                ParamKey::User { id } => {
                    let p = data.params.iter().find(|p| p.id == *id)?;
                    let st = report
                        .params
                        .iter()
                        .find(|r| r.id == *id)
                        .map(|r| &r.status);
                    Row {
                        kind: "param".into(),
                        row,
                        label,
                        expr: p.expr.clone(),
                        value: status_text(st),
                        unit: unit_text(p.unit),
                        comment: p.comment.clone(),
                    }
                }
                ParamKey::Model { feature, .. } => {
                    let p = report
                        .model_params
                        .iter()
                        .find(|p| p.feature == *feature && p.name == row)?;
                    Row {
                        kind: "dim".into(),
                        row,
                        label,
                        expr: p.expr.clone(),
                        value: status_text(Some(&p.status)),
                        ..Row::default()
                    }
                }
                ParamKey::Field { feature, key } => {
                    let v = values
                        .iter()
                        .find(|v| v.feature == *feature && v.key == *key)?;
                    Row {
                        kind: "value".into(),
                        row,
                        label,
                        expr: v.expr.clone(),
                        value: status_text(Some(&v.status)),
                        ..Row::default()
                    }
                }
            })
        })
        .collect()
}

/// 表 `name` の行を `format` の文字にする。
pub fn export(name: &str, rows: Vec<Row>, format: Format) -> String {
    match format {
        Format::Json => serde_json::to_string_pretty(&Table {
            table: name.to_string(),
            rows,
        })
        .expect("表の行は必ず JSON にできる"),
        Format::Csv => {
            let mut out = csv_line(COLUMNS.iter().map(|c| c.to_string()));
            for r in rows {
                out.push_str(&csv_line([
                    r.kind, r.row, r.label, r.expr, r.value, r.unit, r.comment,
                ]));
            }
            out
        }
    }
}

fn csv_line(cells: impl IntoIterator<Item = String>) -> String {
    let cells: Vec<String> = cells
        .into_iter()
        .map(|c| {
            if c.contains([',', '"', '\n', '\r']) || c.starts_with(' ') || c.ends_with(' ') {
                format!("\"{}\"", c.replace('"', "\"\""))
            } else {
                c
            }
        })
        .collect();
    format!("{}\r\n", cells.join(","))
}

/// CSV を欄の並びにする(二重引用符で囲んだ欄の中のカンマ・改行・`""` を扱う)。
fn csv_records(text: &str) -> Result<Vec<Vec<String>>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    let mut rec = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut line = 1;
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    cell.push(c);
                }
                _ => cell.push(c),
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            ',' => rec.push(std::mem::take(&mut cell)),
            '\r' => {}
            '\n' => {
                line += 1;
                rec.push(std::mem::take(&mut cell));
                if rec.iter().any(|c| !c.is_empty()) {
                    out.push(std::mem::take(&mut rec));
                }
                rec.clear();
            }
            _ => cell.push(c),
        }
    }
    if quoted {
        return Err(format!("CSV の {line} 行目で二重引用符が閉じていません"));
    }
    rec.push(cell);
    if rec.iter().any(|c| !c.is_empty()) {
        out.push(rec);
    }
    Ok(out)
}

/// 書き出したファイル(か、表計算で作った同じ列のファイル)を行にする。CSV は 1 行目が列の名前
/// (`row` と `expr` は必ず要る。`kind`・`unit`・`comment` は省ける)。
pub fn parse(text: &str, format: Format) -> Result<Vec<Row>, String> {
    match format {
        Format::Json => {
            let v: serde_json::Value =
                serde_json::from_str(text).map_err(|e| format!("JSON を読めません: {e}"))?;
            // 書き出した形({"table", "rows"})のほか、行の配列だけでも読む
            let rows = if v.is_array() { v } else { v["rows"].clone() };
            serde_json::from_value(rows).map_err(|e| format!("JSON の行を読めません: {e}"))
        }
        Format::Csv => {
            let mut recs = csv_records(text)?.into_iter();
            let head: Vec<String> = recs
                .next()
                .ok_or("CSV が空です")?
                .into_iter()
                .map(|h| h.trim().to_ascii_lowercase())
                .collect();
            let col = |name: &str| head.iter().position(|h| h == name);
            let (Some(row), Some(expr)) = (col("row"), col("expr")) else {
                return Err("CSV の 1 行目に row と expr の列がありません".into());
            };
            let (kind, unit, comment) = (col("kind"), col("unit"), col("comment"));
            let get = |r: &[String], i: Option<usize>| {
                i.and_then(|i| r.get(i)).cloned().unwrap_or_default()
            };
            Ok(recs
                .map(|r| Row {
                    kind: get(&r, kind),
                    row: get(&r, Some(row)),
                    expr: get(&r, Some(expr)),
                    unit: get(&r, unit),
                    comment: get(&r, comment),
                    ..Row::default()
                })
                .collect())
        }
    }
}

/// 読み込みの結果(画面と CLI に出す要約)。
#[derive(Debug, Default, PartialEq)]
pub struct Imported {
    pub added: usize,
    pub changed: usize,
    pub unchanged: usize,
    /// 飛ばした行と理由
    pub skipped: Vec<String>,
}

impl Imported {
    /// 「足した 2、変えた 3、同じ 1、飛ばした 1(d9: この文書にありません)」
    pub fn summary(&self) -> String {
        let mut s = format!(
            "足した {}、変えた {}、同じ {}",
            self.added, self.changed, self.unchanged
        );
        if !self.skipped.is_empty() {
            s.push_str(&format!(
                "、飛ばした {}({})",
                self.skipped.len(),
                self.skipped.join("、")
            ));
        }
        s
    }
}

/// 行の式と単位の書き方を確かめる。参照する行の存在は、変更を作る側で確かめる。
pub(crate) fn checked_row(data: &DocumentData, row: &Row) -> Result<Option<ParamUnit>, String> {
    let expr = row.expr.trim();
    if expr.is_empty() {
        return Err("式が空です".into());
    }
    let unit = match row.unit.trim() {
        "" => None,
        value => Some(
            serde_json::from_value::<ParamUnit>(serde_json::json!(value))
                .map_err(|_| format!("単位 {value} を読めません"))?,
        ),
    };
    let text = unit.or_else(|| data.param_by_name(row.row.trim()).map(|p| p.unit))
        == Some(ParamUnit::Text);
    let checked = if text {
        kmb_core::Template::parse(expr).map(|_| ())
    } else {
        kmb_core::parse_expr(expr).map(|_| ())
    };
    checked.map_err(|e| format!("式 `{expr}` の誤り: {e}"))?;
    Ok(unit)
}

/// 読んだ行 `rows` を文書に入れる変更(1 回の取り消し)。変える物が無ければ変更は `None`。
/// パラメータを足す時に番号を割り当てるので `&mut Document` を受け取る。
pub fn import(
    doc: &mut Document,
    registry: &Registry,
    report: &RecomputeReport,
    rows: &[Row],
) -> (Option<Transaction>, Imported) {
    let mut out = Imported::default();
    let mut ops = Vec::new();
    // 同じ機能の欄を何度も直す時は、直したデータに重ねる
    let mut datas: BTreeMap<FeatureId, serde_json::Value> = BTreeMap::new();
    let mut names_added: Vec<String> = Vec::new();
    for r in rows {
        let name = r.row.trim();
        let expr = r.expr.trim();
        if name.is_empty() {
            continue;
        }
        let data = doc.data();
        let unit = match checked_row(data, r) {
            Ok(unit) => unit,
            Err(why) => {
                out.skipped.push(format!("{name}: {why}"));
                continue;
            }
        };
        let kind = r.kind.trim();
        let is_user = kind == "param"
            || (kind.is_empty()
                && !name.contains(':')
                && !report.model_params.iter().any(|p| p.name == name));
        if is_user {
            if let Some(p) = data.param_by_name(name) {
                let comment = if r.comment.is_empty() && kind.is_empty() {
                    p.comment.clone()
                } else {
                    r.comment.clone()
                };
                let unit = unit.unwrap_or(p.unit);
                let mut same = true;
                let mut row_ops = Vec::new();
                if p.expr != expr {
                    row_ops.push(Op::SetParamExpr {
                        id: p.id,
                        expr: expr.to_string(),
                    });
                    same = false;
                }
                if p.unit != unit || p.comment != comment {
                    let info = Op::SetParamInfo {
                        id: p.id,
                        comment,
                        unit,
                    };
                    // 文字に替える時は単位を先に(式を差し込みの文字として確かめる)
                    if unit == ParamUnit::Text {
                        row_ops.insert(0, info);
                    } else {
                        row_ops.push(info);
                    }
                    same = false;
                }
                ops.extend(row_ops);
                if same {
                    out.unchanged += 1;
                } else {
                    out.changed += 1;
                }
            } else if names_added.iter().any(|n| n == name) {
                out.skipped
                    .push(format!("{name}: 同じ名前の行が 2 つあります"));
            } else if !kmb_core::is_valid_name(name) {
                out.skipped
                    .push(format!("{name}: パラメータの名前に使えません"));
            } else if report.model_params.iter().any(|p| p.name == name) {
                out.skipped
                    .push(format!("{name}: 寸法の名前と同じ名前は使えません"));
            } else {
                let id = doc.alloc_param_id();
                ops.push(Op::AddParam {
                    index: None,
                    param: Param {
                        comment: r.comment.clone(),
                        unit: unit.unwrap_or_default(),
                        ..Param::new(id, name, expr)
                    },
                });
                names_added.push(name.to_string());
                out.added += 1;
            }
            continue;
        }
        let key = match parse_key(data, registry, report, name) {
            Ok(k) => k,
            Err(_) => {
                out.skipped.push(format!("{name}: この文書にありません"));
                continue;
            }
        };
        match key {
            ParamKey::User { .. } => {
                // 種類が dim・value でも名前がパラメータなら、ここに来ない(上で扱う)
                out.skipped
                    .push(format!("{name}: 種類 {kind} と合いません"));
            }
            ParamKey::Model { feature, .. } => {
                let Some(now) = report
                    .model_params
                    .iter()
                    .find(|p| p.feature == feature && p.name == name)
                else {
                    out.skipped.push(format!("{name}: この文書にありません"));
                    continue;
                };
                if now.expr == expr {
                    out.unchanged += 1;
                    continue;
                }
                let Some(f) = data.feature(feature) else {
                    continue;
                };
                let cur = datas
                    .get(&feature)
                    .cloned()
                    .unwrap_or_else(|| f.data.clone());
                let new = match registry.instantiate(&f.type_id, &cur) {
                    Some(Ok(inst)) => inst.set_model_param(name, expr),
                    _ => None,
                };
                match new {
                    Some(d) => {
                        datas.insert(feature, d);
                        out.changed += 1;
                    }
                    None => out.skipped.push(format!("{name}: 寸法を直せません")),
                }
            }
            ParamKey::Field { feature, key } => {
                let Some(now) = feature_values(data, registry, report)
                    .into_iter()
                    .find(|v| v.feature == feature && v.key == key)
                else {
                    continue;
                };
                if now.expr == expr {
                    out.unchanged += 1;
                    continue;
                }
                let (Some(f), Some(field)) = (
                    data.feature(feature),
                    data.feature(feature)
                        .and_then(|f| registry.descriptor(&f.type_id))
                        .and_then(|d| d.field_at(&key)),
                ) else {
                    out.skipped.push(format!("{name}: 欄がありません"));
                    continue;
                };
                let mut cur = datas
                    .get(&feature)
                    .cloned()
                    .unwrap_or_else(|| f.data.clone());
                field.set(&mut cur, Some(serde_json::Value::String(expr.to_string())));
                datas.insert(feature, cur);
                out.changed += 1;
            }
        }
    }
    ops.extend(
        datas
            .into_iter()
            .map(|(id, data)| Op::SetFeatureData { id, data }),
    );
    let tx = (!ops.is_empty())
        .then(|| Transaction::new(format!("パラメータの表を読み込む({})", out.summary()), ops));
    (tx, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::{ParamId, ParamUnit};

    fn doc() -> Document {
        let mut d = Document::new();
        for (n, e) in [("width", "60 mm"), ("note", "2")] {
            let tx = crate::edit::add_param(&mut d, n, e);
            d.apply(tx).unwrap();
        }
        d
    }

    #[test]
    fn csv_quotes_commas_quotes_and_line_breaks() {
        let rows = vec![Row {
            kind: "param".into(),
            row: "w".into(),
            expr: "max(1 mm, 2 mm)".into(),
            comment: "幅 \"外\"\n2 行目".into(),
            ..Row::default()
        }];
        let text = export("表", rows.clone(), Format::Csv);
        assert!(text.starts_with("kind,row,label,expr,value,unit,comment\r\n"));
        assert!(text.contains("\"max(1 mm, 2 mm)\""), "{text}");
        let back = parse(&text, Format::Csv).unwrap();
        assert_eq!(back, rows);
        // 表計算が付ける BOM と、閉じていない引用符
        let bom = format!("\u{feff}{text}");
        assert_eq!(parse(&bom, Format::Csv).unwrap(), rows);
        assert!(parse("row,expr\n\"w,1", Format::Csv).is_err());
        assert!(
            parse("name,value\nw,1", Format::Csv).is_err(),
            "row と expr の列が要る"
        );
    }

    #[test]
    fn exported_rows_read_back_and_import_adds_changes_and_skips() {
        let reg = Registry::default();
        let report = RecomputeReport::default();
        let mut d = doc();
        let keys: Vec<ParamKey> = d
            .data()
            .params
            .iter()
            .map(|p| ParamKey::User { id: p.id })
            .collect();
        let rows = super::rows(d.data(), &reg, &report, &keys);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].unit, "any");
        for f in [Format::Csv, Format::Json] {
            let text = export("全部", rows.clone(), f);
            let back = parse(&text, f).unwrap();
            assert_eq!(back.len(), 2);
            // 同じ物を読むと何も変えない
            let (tx, done) = import(&mut d, &reg, &report, &back);
            assert!(tx.is_none());
            assert_eq!(done.unchanged, 2);
        }
        let text = "kind,row,expr,unit,comment\n\
                    param,width,70 mm,length,外の幅\n\
                    ,gap,3 mm,,すき間\n\
                    ,bad name,1,,\n\
                    dim,d9,1 mm,,\n\
                    value,5:distance,1 mm,,\n\
                    ,gap,4 mm,,\n";
        let rows = parse(text, Format::Csv).unwrap();
        let (tx, done) = import(&mut d, &reg, &report, &rows);
        assert_eq!((done.added, done.changed), (1, 1), "{}", done.summary());
        assert_eq!(done.skipped.len(), 4, "{:?}", done.skipped);
        d.apply(tx.unwrap()).unwrap();
        let w = d.data().param_by_name("width").unwrap();
        assert_eq!(
            (w.expr.as_str(), w.unit, w.comment.as_str()),
            ("70 mm", ParamUnit::Length, "外の幅")
        );
        assert_eq!(d.data().param_by_name("gap").unwrap().comment, "すき間");
        // 1 回で取り消せる
        assert!(d.undo());
        assert_eq!(d.data().param_by_name("width").unwrap().expr, "60 mm");
        assert!(d.data().param_by_name("gap").is_none());
        assert_eq!(d.data().params[1].id, ParamId(2));
    }

    #[test]
    fn text_params_import_as_templates() {
        let reg = Registry::default();
        let report = RecomputeReport::default();
        let mut d = doc();
        // 新しい文字のパラメータと、数のパラメータを文字に替える行(単位を先に替える)
        let text = "kind,row,expr,unit\n\
                    param,型番,M{note}-{width},text\n\
                    param,note,{{3}},text\n\
                    param,bad,M{,text\n";
        let rows = parse(text, Format::Csv).unwrap();
        let (tx, done) = import(&mut d, &reg, &report, &rows);
        assert_eq!((done.added, done.changed), (1, 1), "{}", done.summary());
        assert_eq!(done.skipped.len(), 1, "{:?}", done.skipped);
        d.apply(tx.unwrap()).unwrap();
        let t = d.data().param_by_name("型番").unwrap();
        assert_eq!(
            (t.expr.as_str(), t.unit),
            ("M{note}-{width}", ParamUnit::Text)
        );
        assert_eq!(
            d.data().param_by_name("note").unwrap().unit,
            ParamUnit::Text
        );
        let rows = super::rows(d.data(), &reg, &report, &[ParamKey::User { id: t.id }]);
        assert_eq!(rows[0].unit, "text");
    }
}
