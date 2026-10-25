//! 構成表の操作。表の中身と行を当てる計算は
//! [`kmb_interchange::configuration`](文書の区画 `config.table`。このモジュールが名乗る)。ここは
//! 画面・CLI・MCP に出す操作(列と行を足す・外す、値を直す、行を当てる、行に今の値を書き込む)。

use kmb_core::{Action, Arg, Context, Extension, FeatureId, Outcome, Place, Registry};
use kmb_interchange::configuration::{self, ConfigCell, ConfigColumn, ConfigRow, Configurations};

pub const ADD_COLUMN: &str = "cad.config.add_column";
pub const REMOVE_COLUMN: &str = "cad.config.remove_column";
pub const ADD_ROW: &str = "cad.config.add_row";
pub const REMOVE_ROW: &str = "cad.config.remove_row";
pub const SET_CELL: &str = "cad.config.set";
pub const UPDATE_ROW: &str = "cad.config.update_row";
pub const ACTIVATE: &str = "cad.config.activate";

/// 構成表の操作。
pub struct ConfigCommands;

fn change(
    cx: &Context<'_>,
    label: String,
    c: Configurations,
) -> Result<Outcome, kmb_core::Problem> {
    let edit = cx.data.set_section(&c)?;
    log::info!("{label}");
    Ok(Outcome::Change {
        label,
        edits: vec![edit],
    })
}

/// 行の名前(引数 `row`。省くと当てた行)。
fn row_name(cx: &Context<'_>, c: &Configurations) -> Result<String, String> {
    let name = cx
        .arg_str("row")
        .map(str::to_string)
        .or_else(|| c.active.clone())
        .ok_or("構成(行)の名前を指定します")?;
    c.row(&name)
        .map(|r| r.name.clone())
        .ok_or_else(|| format!("構成「{name}」がありません"))
}

/// 列の番号(引数 `column`、1 から)。
fn column_index(cx: &Context<'_>, c: &Configurations) -> Result<usize, String> {
    let n = cx
        .arg_f64("column")?
        .ok_or("列の番号(1 から)を指定します")? as usize;
    (1..=c.columns.len())
        .contains(&n)
        .then(|| n - 1)
        .ok_or_else(|| format!("列 {n} はありません(列は {} 本)", c.columns.len()))
}

/// 抑制の列の値の言葉を読む。
fn suppressed_of(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "抑制" | "on" | "true" | "yes" | "1" => Some(true),
        "" | "有効" | "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn add_column(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let data = cx.data;
    let mut c = configuration::of(data);
    let col = if let Some(name) = cx.arg_str("param") {
        let p = data
            .params
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("パラメータ「{name}」がありません"))?;
        ConfigColumn::Param { param: p.id }
    } else if let Some(f) = cx.arg_f64("feature")? {
        ConfigColumn::Suppress {
            feature: FeatureId(f as u64),
        }
    } else {
        return Err("列にするパラメータの名前(param)か機能の番号(feature)を指定します".into());
    };
    c.add_column(data, col.clone())?;
    let label = format!(
        "構成表に列「{}」を足す",
        Configurations::column_label(data, &col)
    );
    change(cx, label, c)
}

fn add_row(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let data = cx.data;
    let mut c = configuration::of(data);
    let name = cx
        .arg_str("name")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(|| c.free_name(), str::to_string);
    if c.row(&name).is_some() {
        return Err(format!("構成「{name}」は既にあります").into());
    }
    let row = c.current_row(data, &name, None);
    c.rows.push(row);
    change(cx, format!("構成「{name}」を足す"), c)
}

fn set_cell(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let mut c = configuration::of(cx.data);
    let row = row_name(cx, &c)?;
    let col = column_index(cx, &c)?;
    let text = cx.arg_str("value").ok_or("値(value)を指定します")?;
    let cell = match c.columns[col] {
        ConfigColumn::Param { .. } => {
            if text.trim().is_empty() {
                return Err("パラメータの列は空にできません".into());
            }
            ConfigCell::Expr(text.trim().to_string())
        }
        ConfigColumn::Suppress { .. } => {
            ConfigCell::Suppressed(suppressed_of(text).ok_or_else(|| {
                format!("抑制の列は「抑制」か「有効」で指定します(今は「{text}」)")
            })?)
        }
    };
    let r: &mut ConfigRow = c
        .rows
        .iter_mut()
        .find(|r| r.name == row)
        .expect("row_name で確かめた");
    r.cells[col] = cell;
    let label = format!(
        "構成「{row}」の「{}」を直す",
        Configurations::column_label(cx.data, &c.columns[col])
    );
    change(cx, label, c)
}

fn update_row(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let mut c = configuration::of(cx.data);
    let row = row_name(cx, &c)?;
    let i = c.rows.iter().position(|r| r.name == row).expect("ある");
    let next = c.current_row(cx.data, &row, Some(&c.rows[i]));
    c.rows[i] = next;
    change(cx, format!("構成「{row}」に今の値を書き込む"), c)
}

impl Extension for ConfigCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        // 画面では「パラメータ」の欄の構成表から直すので、ツールバーと機能の欄には出さない。
        // 機能を選んでいる時(機能の欄・右クリック)は名乗らない(どの機能の欄にも並ばないように)
        if cx.selected.is_some() {
            return Vec::new();
        }
        let c = &configuration::of(cx.data);
        let no_rows = c.rows.is_empty().then_some("構成表に行がありません");
        let no_cols = c.columns.is_empty().then_some("構成表に列がありません");
        let places: &[Place] = &[Place::Panel];
        vec![
            Action::new(ADD_COLUMN, "構成表に列を足す", "cad")
                .args([
                    Arg::text("param", "列にするパラメータの名前"),
                    Arg::feature("feature", "抑制を切り替える機能の番号"),
                ])
                .menu("修正")
                .icon("param")
                .aliases("こうせい こうせいひょう configuration れつ")
                .hint("構成ごとに変えるパラメータか、抑制を切り替える機能を、構成表の列に足す(どの行にも今の値が入る)")
                .at(places),
            Action::new(REMOVE_COLUMN, "構成表の列を外す", "cad")
                .arg(Arg::integer("column", "外す列の番号(1 から)").required())
                .menu("修正")
                .icon("param")
                .aliases("こうせい configuration れつ")
                .hint("構成表の列を外す(パラメータや機能は消さない)")
                .at(places)
                .disabled_if(no_cols),
            Action::new(ADD_ROW, "構成を足す", "cad")
                .arg(Arg::text("name", "構成の名前(省くと「構成N」)"))
                .menu("修正")
                .icon("param")
                .aliases("こうせい configuration ぎょう")
                .hint("今の値で構成表に行を足す")
                .at(places),
            Action::new(REMOVE_ROW, "構成を消す", "cad")
                .arg(Arg::text("row", "消す構成の名前").required())
                .menu("修正")
                .icon("param")
                .aliases("こうせい configuration ぎょう")
                .hint("構成表の行を消す(今の形は変えない)")
                .at(places)
                .disabled_if(no_rows),
            Action::new(SET_CELL, "構成の値を直す", "cad")
                .args([
                    Arg::text("row", "構成の名前(省くと当てた構成)"),
                    Arg::integer("column", "列の番号(1 から)").required(),
                    Arg::text("value", "値(パラメータは式、抑制の列は「抑制」か「有効」)").required(),
                ])
                .menu("修正")
                .icon("param")
                .aliases("こうせい configuration あたい")
                .hint("構成表の 1 つの値を直す(当てるまで形は変わらない)")
                .at(places)
                .disabled_if(no_rows.or(no_cols)),
            Action::new(UPDATE_ROW, "構成に今の値を書き込む", "cad")
                .arg(Arg::text("row", "構成の名前(省くと当てた構成)"))
                .menu("修正")
                .icon("param")
                .aliases("こうせい configuration かきこむ")
                .hint("当てた後に直したパラメータの式や抑制を、構成表の行に書き込む")
                .at(places)
                .disabled_if(no_rows),
            Action::new(ACTIVATE, "構成を当てる", "cad")
                .arg(Arg::text("row", "当てる構成の名前").required())
                .menu("修正")
                .icon("param")
                .aliases("こうせい configuration きりかえ あてる")
                .hint("構成表の行の値をパラメータと抑制に当て、全部を計算し直す(1 回の取り消しで戻る)")
                .at(places)
                .disabled_if(no_rows),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        Some(match id {
            ADD_COLUMN => add_column(cx),
            REMOVE_COLUMN => {
                let mut c = configuration::of(cx.data);
                column_index(cx, &c).map_err(Into::into).and_then(|i| {
                    let label = format!(
                        "構成表の列「{}」を外す",
                        Configurations::column_label(cx.data, &c.columns[i])
                    );
                    c.remove_column(i)?;
                    change(cx, label, c)
                })
            }
            ADD_ROW => add_row(cx),
            REMOVE_ROW => {
                let mut c = configuration::of(cx.data);
                row_name(cx, &c).map_err(Into::into).and_then(|row| {
                    c.rows.retain(|r| r.name != row);
                    if c.active.as_deref() == Some(row.as_str()) {
                        c.active = None;
                    }
                    change(cx, format!("構成「{row}」を消す"), c)
                })
            }
            SET_CELL => set_cell(cx),
            UPDATE_ROW => update_row(cx),
            ACTIVATE => {
                let c = &configuration::of(cx.data);
                row_name(cx, c)
                    .map_err(kmb_core::Problem::from)
                    .and_then(|row| {
                        // 消えた列があれば断る(何も当てない)
                        let edits = c.activate_edits(cx.data, &row)?;
                        log::info!("構成「{row}」を当てる");
                        Ok(Outcome::Change {
                            label: format!("構成「{row}」を当てる"),
                            edits,
                        })
                    })
            }
            _ => return None,
        })
    }
}

fn register(reg: &mut Registry) {
    reg.declare_section::<Configurations>();
}

/// この crate の名乗り(操作だけで、機能の種類は登録しない)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-config", register).with_extension(ConfigCommands)
}

#[cfg(test)]
mod tests;
