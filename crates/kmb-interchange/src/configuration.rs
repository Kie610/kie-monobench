//! 構成表。
//!
//! 寸法違いの部品を 1 つの文書で持つ。列は変えるパラメータか、抑制を切り替える機能。行は構成の
//! 名前と、列ごとの値(パラメータの式か、抑制するか)。行を替えると、その行の値を設計に当てて
//! (パラメータの式と機能の抑制を書き換えて)全部を計算し直す。1 回の取り消しで戻る。行ごとの
//! 形の作り置きは持たない。
//!
//! 列はパラメータと機能を番号で指す(名前を変えても付いて行く)。指す物が消えた列は残し、
//! 行を替える時に「その列は当てられない」と知らせる(黙って飛ばさない)。

use kmb_core::{DocumentData, Edit, FeatureId, Op, ParamId, SectionType};
use serde::{Deserialize, Serialize};

/// 1 つの文書に持てる行と列の数の上限。
const MAX_ROWS: usize = 500;
const MAX_COLUMNS: usize = 200;

/// 列(何を変えるか)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigColumn {
    /// パラメータの式
    Param { param: ParamId },
    /// 機能の抑制
    Suppress { feature: FeatureId },
}

/// 行の値 1 つ。列の種類と組になる(パラメータの列は式、抑制の列は抑制するか)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigCell {
    Expr(String),
    Suppressed(bool),
}

/// 行(構成 1 つ)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigRow {
    pub name: String,
    /// 列ごとの値(列と同じ数)
    pub cells: Vec<ConfigCell>,
}

/// 文書の区画 `config.table`: 構成表。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Configurations {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ConfigColumn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<ConfigRow>,
    /// 最後に当てた行の名前
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    /// このアプリの知らない項目(新しいアプリが足した物)。読んだまま書き戻す
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl SectionType for Configurations {
    const KEY: &'static str = "config.table";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "構成表";

    fn check(&self) -> Result<(), String> {
        check_configurations(self)
    }

    fn is_empty(&self) -> bool {
        self.columns.is_empty()
            && self.rows.is_empty()
            && self.active.is_none()
            && self.rest.is_empty()
    }
}

/// 文書の構成表。区画を読めなければログに残して空。
pub fn of(data: &DocumentData) -> Configurations {
    data.section().unwrap_or_else(|e| {
        log::warn!("{e}");
        Configurations::default()
    })
}

/// 行を当てる時に変える物(パラメータの式・機能の抑制・当てた行を覚えた表)。
struct Activation {
    exprs: Vec<(ParamId, String)>,
    suppress: Vec<(FeatureId, bool)>,
    table: Configurations,
}

impl Configurations {
    pub fn row(&self, name: &str) -> Option<&ConfigRow> {
        self.rows.iter().find(|r| r.name == name)
    }

    /// まだ使っていない行の名前(「構成1」「構成2」…)。
    pub fn free_name(&self) -> String {
        (1..)
            .map(|i| format!("構成{i}"))
            .find(|n| self.row(n).is_none())
            .expect("どこかで空く")
    }

    /// 列 `col` の、今の文書の値(指す物が消えていれば `None`)。
    pub fn current_cell(data: &DocumentData, col: &ConfigColumn) -> Option<ConfigCell> {
        match col {
            ConfigColumn::Param { param } => data
                .params
                .iter()
                .find(|p| p.id == *param)
                .map(|p| ConfigCell::Expr(p.expr.clone())),
            ConfigColumn::Suppress { feature } => data
                .feature(*feature)
                .map(|f| ConfigCell::Suppressed(f.suppressed)),
        }
    }

    /// 列の名前(パラメータの名前か、機能の名前と「の抑制」。消えていれば「(消えた…)」)。
    pub fn column_label(data: &DocumentData, col: &ConfigColumn) -> String {
        match col {
            ConfigColumn::Param { param } => data
                .params
                .iter()
                .find(|p| p.id == *param)
                .map_or_else(|| "(消えたパラメータ)".into(), |p| p.name.clone()),
            ConfigColumn::Suppress { feature } => data
                .feature(*feature)
                .map_or_else(|| "(消えた機能)".into(), |f| format!("{} の抑制", f.name)),
        }
    }

    /// 今の文書の値を並べた行(新しい行を足す時と、行に今の値を書き込む時に使う)。消えた列は
    /// 前の値 `old` のまま(無ければ空の式か、抑制しない)。
    pub fn current_row(
        &self,
        data: &DocumentData,
        name: &str,
        old: Option<&ConfigRow>,
    ) -> ConfigRow {
        let cells = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                Self::current_cell(data, c)
                    .or_else(|| old.and_then(|r| r.cells.get(i).cloned()))
                    .unwrap_or_else(|| blank(c))
            })
            .collect();
        ConfigRow {
            name: name.into(),
            cells,
        }
    }

    /// 行 `name` の値が今の文書と同じか(当てた後に式や抑制を直したかを見る)。消えた列は見ない。
    pub fn matches(&self, data: &DocumentData, name: &str) -> bool {
        let Some(row) = self.row(name) else {
            return false;
        };
        self.columns
            .iter()
            .zip(&row.cells)
            .all(|(c, v)| Self::current_cell(data, c).is_none_or(|now| now == *v))
    }

    /// 列を足す(どの行にも今の文書の値を入れる)。同じ物を指す列が既にあれば断る。
    pub fn add_column(&mut self, data: &DocumentData, col: ConfigColumn) -> Result<(), String> {
        if self.columns.contains(&col) {
            return Err(format!(
                "「{}」は既に列にあります",
                Self::column_label(data, &col)
            ));
        }
        let now = Self::current_cell(data, &col)
            .ok_or_else(|| "列にするパラメータか機能がありません".to_string())?;
        for r in &mut self.rows {
            r.cells.push(now.clone());
        }
        self.columns.push(col);
        Ok(())
    }

    /// 列 `index` を外す。
    pub fn remove_column(&mut self, index: usize) -> Result<(), String> {
        if index >= self.columns.len() {
            return Err(format!("列 {} はありません", index + 1));
        }
        self.columns.remove(index);
        for r in &mut self.rows {
            if index < r.cells.len() {
                r.cells.remove(index);
            }
        }
        Ok(())
    }

    /// 行 `row` を当てる変更(パラメータの式と機能の抑制を行の値にし、当てた行を覚える)。
    /// 指す物が消えた列があれば断る(黙って飛ばさない)。
    pub fn activate_ops(&self, data: &DocumentData, row: &str) -> Result<Vec<Op>, String> {
        let a = self.activation(data, row)?;
        let mut ops: Vec<Op> = a
            .exprs
            .into_iter()
            .map(|(id, expr)| Op::SetParamExpr { id, expr })
            .chain(
                a.suppress
                    .into_iter()
                    .map(|(id, suppressed)| Op::SetSuppressed { id, suppressed }),
            )
            .collect();
        ops.push(data.section_op(&a.table)?);
        Ok(ops)
    }

    /// [`Configurations::activate_ops`] と同じ変更を、操作の結果([`Edit`])の形で。
    pub fn activate_edits(&self, data: &DocumentData, row: &str) -> Result<Vec<Edit>, String> {
        let a = self.activation(data, row)?;
        let mut edits: Vec<Edit> = a
            .exprs
            .into_iter()
            .map(|(id, expr)| Edit::SetParamExpr { id, expr })
            .chain(
                a.suppress
                    .into_iter()
                    .map(|(id, suppressed)| Edit::SetSuppressed { id, suppressed }),
            )
            .collect();
        edits.push(data.set_section(&a.table)?);
        Ok(edits)
    }

    fn activation(&self, data: &DocumentData, row: &str) -> Result<Activation, String> {
        let r = self
            .row(row)
            .ok_or_else(|| format!("構成「{row}」がありません"))?;
        let mut exprs = Vec::new();
        let mut suppress = Vec::new();
        for (c, v) in self.columns.iter().zip(&r.cells) {
            if Self::current_cell(data, c).is_none() {
                return Err(format!(
                    "構成「{row}」を当てられません: 列「{}」の指す物が消えています(列を外してください)",
                    Self::column_label(data, c)
                ));
            }
            match (c, v) {
                (ConfigColumn::Param { param }, ConfigCell::Expr(e)) => {
                    let now = data.params.iter().find(|p| p.id == *param);
                    if now.is_some_and(|p| p.expr != *e) {
                        exprs.push((*param, e.clone()));
                    }
                }
                (ConfigColumn::Suppress { feature }, ConfigCell::Suppressed(s)) => {
                    if data.feature(*feature).is_some_and(|f| f.suppressed != *s) {
                        suppress.push((*feature, *s));
                    }
                }
                _ => return Err(format!("構成「{row}」の値の種類が列と合いません")),
            }
        }
        let mut table = self.clone();
        table.active = Some(row.into());
        Ok(Activation {
            exprs,
            suppress,
            table,
        })
    }
}

/// 列の空の値(式は空、抑制はしない)。
fn blank(col: &ConfigColumn) -> ConfigCell {
    match col {
        ConfigColumn::Param { .. } => ConfigCell::Expr(String::new()),
        ConfigColumn::Suppress { .. } => ConfigCell::Suppressed(false),
    }
}

/// 構成表の形を確かめる(行の名前が空でなく重ならない、値の数と種類が列に合う、当てた行がある)。
/// 列が指す物が残っているかは見ない(消した後でも文書は開ける。当てる時に知らせる)。
fn check_configurations(c: &Configurations) -> Result<(), String> {
    if c.rows.len() > MAX_ROWS || c.columns.len() > MAX_COLUMNS {
        return Err(format!("行は {MAX_ROWS} まで、列は {MAX_COLUMNS} までです"));
    }
    for (i, r) in c.rows.iter().enumerate() {
        if r.name.trim().is_empty() {
            return Err("構成の名前が空です".into());
        }
        if c.rows[..i].iter().any(|o| o.name == r.name) {
            return Err(format!("構成の名前「{}」が重なっています", r.name));
        }
        if r.cells.len() != c.columns.len() {
            return Err(format!(
                "構成「{}」の値の数 {} が列の数 {} と合いません",
                r.name,
                r.cells.len(),
                c.columns.len()
            ));
        }
        for (col, v) in c.columns.iter().zip(&r.cells) {
            let ok = matches!(
                (col, v),
                (ConfigColumn::Param { .. }, ConfigCell::Expr(_))
                    | (ConfigColumn::Suppress { .. }, ConfigCell::Suppressed(_))
            );
            if !ok {
                return Err(format!("構成「{}」の値の種類が列と合いません", r.name));
            }
        }
    }
    if let Some(a) = &c.active
        && c.row(a).is_none()
    {
        return Err(format!("当てた構成「{a}」がありません"));
    }
    for (i, col) in c.columns.iter().enumerate() {
        if c.columns[..i].contains(col) {
            return Err("同じ物を指す列が重なっています".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::{Document, FeatureEntry, Param, Transaction};

    /// パラメータ「幅」(40 mm)と機能「穴」(抑制していない)の文書。
    fn doc() -> (Document, ParamId, FeatureId) {
        let mut d = Document::new();
        let p = d.alloc_param_id();
        let f = d.alloc_feature_id();
        d.apply(Transaction::new(
            "見本",
            vec![
                Op::AddParam {
                    index: None,
                    param: Param::new(p, "幅", "40 mm"),
                },
                Op::InsertFeature {
                    index: None,
                    entry: FeatureEntry {
                        id: f,
                        type_id: "test.box@1".into(),
                        name: "穴".into(),
                        data: serde_json::json!({}),
                        suppressed: false,
                        pin_bodies: false,
                    },
                },
            ],
        ))
        .unwrap();
        (d, p, f)
    }

    fn table(d: &Document, p: ParamId, f: FeatureId) -> Configurations {
        let data = d.data();
        let mut c = Configurations::default();
        c.add_column(data, ConfigColumn::Param { param: p })
            .unwrap();
        c.add_column(data, ConfigColumn::Suppress { feature: f })
            .unwrap();
        c.rows.push(c.current_row(data, "小", None));
        c.rows.push(ConfigRow {
            name: "大".into(),
            cells: vec![
                ConfigCell::Expr("80 mm".into()),
                ConfigCell::Suppressed(true),
            ],
        });
        c
    }

    // 確かめること: 行を当てるとパラメータの式と抑制が行の値になり、1 回の取り消しで戻る。
    // 当てた行を覚え、値を直すと「行と違う」と分かる。
    #[test]
    fn activating_a_row_sets_params_and_suppression_and_undoes_once() {
        let (mut d, p, f) = doc();
        let c = table(&d, p, f);
        let set = d.data().set_section(&c).unwrap();
        d.apply_edits("構成表", vec![set]).unwrap();
        let edits = of(d.data()).activate_edits(d.data(), "大").unwrap();
        d.apply_edits("構成「大」", edits).unwrap();
        let data = d.data();
        assert_eq!(data.params[0].expr, "80 mm");
        assert!(data.feature(f).unwrap().suppressed);
        assert_eq!(of(data).active.as_deref(), Some("大"));
        assert!(of(data).matches(data, "大"));
        assert!(!of(data).matches(data, "小"));
        assert!(d.undo());
        let data = d.data();
        assert_eq!(data.params[0].expr, "40 mm");
        assert!(!data.feature(f).unwrap().suppressed);
        assert_eq!(of(data).active, None);
    }

    #[test]
    fn a_row_pointing_at_a_removed_param_is_refused_not_skipped() {
        let (mut d, p, f) = doc();
        let c = table(&d, p, f);
        let set = d.data().section_op(&c).unwrap();
        d.apply(Transaction::new("表", vec![set])).unwrap();
        d.apply(Transaction::new("消す", vec![Op::RemoveParam { id: p }]))
            .unwrap();
        let err = of(d.data()).activate_ops(d.data(), "大").unwrap_err();
        assert!(err.contains("消えたパラメータ"), "{err}");
        assert!(!d.data().feature(f).unwrap().suppressed, "何も当てない");
    }

    #[test]
    fn the_table_shape_is_checked() {
        let (d, p, f) = doc();
        let mut c = table(&d, p, f);
        assert!(
            c.add_column(d.data(), ConfigColumn::Param { param: p })
                .is_err()
        );
        c.rows[1].name = "小".into();
        assert!(check_configurations(&c).is_err(), "名前の重なり");
        c.rows[1].name = "大".into();
        c.rows[1].cells.pop();
        assert!(check_configurations(&c).is_err(), "値の数");
        let mut c = table(&d, p, f);
        c.active = Some("無い".into());
        assert!(check_configurations(&c).is_err());
        c.active = None;
        c.remove_column(0).unwrap();
        assert_eq!(c.rows[1].cells, vec![ConfigCell::Suppressed(true)]);
        check_configurations(&c).unwrap();
        // 保存して読み戻しても同じ
        let back: Configurations =
            serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}
