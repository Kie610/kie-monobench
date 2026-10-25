use crate::document::{
    Component, DocumentData, FeatureEntry, Group, NamedView, Param, ParamView, Properties,
};
use crate::{FeatureId, ParamId, ParamKey};
use serde::{Deserialize, Serialize};
use std::fmt;

/// 文書モデルへの最小単位の変更。取り消しは、逆向きの `Op` を適用して行う。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    AddParam {
        index: Option<usize>,
        param: Param,
    },
    RemoveParam {
        id: ParamId,
    },
    SetParamExpr {
        id: ParamId,
        expr: String,
    },
    /// コメントと単位を変える(パラメータの表の列)
    SetParamInfo {
        id: ParamId,
        comment: String,
        unit: crate::ParamUnit,
    },
    /// 名前だけを変える。参照する式も書き換えるには [`crate::Document::rename_param`] を使う
    RenameParam {
        id: ParamId,
        name: String,
    },
    /// `index` が None ならロールバック位置(なければ末尾)に入れ、有効な機能にする
    InsertFeature {
        index: Option<usize>,
        entry: FeatureEntry,
    },
    RemoveFeature {
        id: FeatureId,
    },
    MoveFeature {
        id: FeatureId,
        to: usize,
    },
    SetFeatureData {
        id: FeatureId,
        data: serde_json::Value,
    },
    RenameFeature {
        id: FeatureId,
        name: String,
    },
    SetSuppressed {
        id: FeatureId,
        suppressed: bool,
    },
    SetRollback {
        rollback: Option<usize>,
    },
    /// タイムラインのグループを丸ごと置き換える
    SetGroups {
        groups: Vec<Group>,
    },
    /// パラメータの表を丸ごと置き換える
    SetParamViews {
        views: Vec<ParamView>,
    },
    /// 消したパラメータの一覧を丸ごと置き換える(消す時は [`Op::RemoveParam`] と組にする)
    SetRemovedParams {
        params: Vec<Param>,
    },
    /// ボディ(最初に作った機能 `root` で指す)の名前を変える。`None` なら「ボディN」に戻す
    RenameBody {
        root: FeatureId,
        name: Option<String>,
    },
    /// 設計の属性を丸ごと置き換える(U13。ほかの操作より大きいので箱に入れる)
    SetProperties {
        properties: Box<Properties>,
    },
    /// 名前を付けた視点を丸ごと置き換える(U20)
    SetNamedViews {
        views: Vec<NamedView>,
    },
    /// 部品の木を丸ごと置き換える
    SetComponents {
        components: Vec<Component>,
    },
    /// モジュールの区画 `key` を置き換える(`None` なら外す。[`crate::section`])
    SetSection {
        key: String,
        section: Option<crate::section::Section>,
    },
}

/// まとめて適用し、まとめて取り消す変更。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    pub label: String,
    pub ops: Vec<Op>,
}

impl Transaction {
    pub fn new(label: impl Into<String>, ops: Vec<Op>) -> Self {
        Transaction {
            label: label.into(),
            ops,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TxError {
    InvalidName(String),
    DuplicateName(String),
    DuplicateId(u64),
    NoSuchParam(ParamId),
    NoSuchFeature(FeatureId),
    BadExpr {
        expr: String,
        message: String,
    },
    IndexOutOfRange(usize),
    EmptyTypeId,
    IdNotAllocated(u64),
    /// グループが正しくない(理由)
    BadGroup(String),
    /// パラメータの表が正しくない(理由)
    BadParamView(String),
    /// 名前を付けた視点が正しくない(理由)
    BadNamedView(String),
    /// 部品の木が正しくない(理由)
    BadComponent(String),
    BadSection(String),
}

impl fmt::Display for TxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TxError::InvalidName(n) if kmb_expr::UNIT_NAMES.contains(&n.as_str()) => write!(
                f,
                "パラメータ名に使えません: {n}(単位の名前と同じで、式の中では単位として読みます。\
                 別の名前に替えてください)"
            ),
            TxError::InvalidName(n) if kmb_expr::BUILTIN_NAMES.contains(&n.as_str()) => write!(
                f,
                "パラメータ名に使えません: {n}(式の関数・定数の名前と同じです。別の名前に替えてください)"
            ),
            TxError::InvalidName(n) => write!(f, "パラメータ名に使えません: {n}"),
            TxError::DuplicateName(n) => write!(f, "同じ名前のパラメータがあります: {n}"),
            TxError::DuplicateId(i) => write!(f, "ID {i} が重複しています"),
            TxError::NoSuchParam(i) => write!(f, "パラメータ {i} がありません"),
            TxError::NoSuchFeature(i) => write!(f, "機能 {i} がありません"),
            TxError::BadExpr { expr, message } => write!(f, "式 `{expr}` の誤り: {message}"),
            TxError::IndexOutOfRange(i) => write!(f, "位置 {i} は範囲外です"),
            TxError::EmptyTypeId => write!(f, "機能の種類が空です"),
            TxError::IdNotAllocated(i) => write!(f, "ID {i} は割り当て前の値です"),
            TxError::BadGroup(why) => write!(f, "グループにできません: {why}"),
            TxError::BadParamView(why) => write!(f, "パラメータの表にできません: {why}"),
            TxError::BadNamedView(why) => write!(f, "視点を保存できません: {why}"),
            TxError::BadComponent(why) => write!(f, "部品にできません: {why}"),
            TxError::BadSection(why) => write!(f, "文書の区画にできません: {why}"),
        }
    }
}

impl std::error::Error for TxError {}

/// `tx` を適用し、取り消し用のトランザクションを返す。失敗したら元に戻す。
pub(crate) fn apply(data: &mut DocumentData, tx: Transaction) -> Result<Transaction, TxError> {
    let mut groups: Vec<Vec<Op>> = Vec::new();
    for op in tx.ops {
        match apply_op(data, op) {
            Ok(inv) => groups.push(inv),
            Err(e) => {
                for inv in groups.into_iter().rev() {
                    for op in inv {
                        apply_op(data, op).expect("逆操作は必ず適用できる");
                    }
                }
                return Err(e);
            }
        }
    }
    let ops = groups.into_iter().rev().flatten().collect();
    Ok(Transaction {
        label: tx.label,
        ops,
    })
}

/// 機能のデータで行の並び(`並び.行番号.欄` の鍵で指す配列)の行を消したり足したりした時、
/// パラメータの表(手で入れた行・外した行・お気に入り)が指す行番号を、同じ行を指すように付け替える
/// 変更を足す。消した行を指していた鍵は外す。行の中身も変えた時は、どの行が残ったか決められない
/// ので付け替えない。取り消しとやり直しは足した変更をそのまま戻すので、ここは利用者の変更でだけ通す。
pub(crate) fn follow_rows(data: &DocumentData, tx: Transaction) -> Transaction {
    if data.param_views.is_empty() {
        return tx;
    }
    let mut views = data.param_views.clone();
    let mut now: std::collections::HashMap<FeatureId, serde_json::Value> = Default::default();
    let mut ops = Vec::with_capacity(tx.ops.len());
    for op in tx.ops {
        match &op {
            Op::SetParamViews { views: v } => views = v.clone(),
            Op::SetFeatureData { id, data: new } => {
                let old = now.get(id).cloned().or_else(|| {
                    data.features
                        .iter()
                        .find(|f| f.id == *id)
                        .map(|f| f.data.clone())
                });
                now.insert(*id, new.clone());
                if let Some(old) = old {
                    let moved = move_row_keys(&views, *id, &old, new);
                    if moved != views {
                        views = moved.clone();
                        ops.push(op);
                        ops.push(Op::SetParamViews { views: moved });
                        continue;
                    }
                }
            }
            _ => {}
        }
        ops.push(op);
    }
    Transaction {
        label: tx.label,
        ops,
    }
}

/// 古い並び `old` の各行が、新しい並び `new` の何行目になったか(消えた行は `None`)。
/// 片方がもう片方から行を抜いただけの時だけ決まる。
fn row_moves(old: &[serde_json::Value], new: &[serde_json::Value]) -> Option<Vec<Option<usize>>> {
    let (short, long, shrink) = if new.len() < old.len() {
        (new, old, true)
    } else if new.len() > old.len() {
        (old, new, false)
    } else {
        return None;
    };
    // 長い方の各行が、短い方の何行目か
    let mut at = vec![None; long.len()];
    let mut j = 0;
    for (i, row) in long.iter().enumerate() {
        if j < short.len() && short[j] == *row {
            at[i] = Some(j);
            j += 1;
        }
    }
    if j != short.len() {
        return None;
    }
    if shrink {
        return Some(at);
    }
    let mut back = vec![None; short.len()];
    for (i, k) in at.iter().enumerate() {
        if let Some(k) = k {
            back[*k] = Some(i);
        }
    }
    Some(back)
}

fn move_row_keys(
    views: &[ParamView],
    feature: FeatureId,
    old: &serde_json::Value,
    new: &serde_json::Value,
) -> Vec<ParamView> {
    let moves = |rows: &str| -> Option<Vec<Option<usize>>> {
        row_moves(old.get(rows)?.as_array()?, new.get(rows)?.as_array()?)
    };
    let mut cache: std::collections::HashMap<String, Option<Vec<Option<usize>>>> =
        Default::default();
    let mut move_key = |k: &ParamKey| -> Option<Option<ParamKey>> {
        let ParamKey::Field { feature: f, key } = k else {
            return None;
        };
        if *f != feature {
            return None;
        }
        let mut parts = key.splitn(3, '.');
        let (rows, row, rest) = (parts.next()?, parts.next()?, parts.next()?);
        let row: usize = row.parse().ok()?;
        let m = cache
            .entry(rows.to_string())
            .or_insert_with(|| moves(rows))
            .as_ref()?;
        Some(m.get(row).copied().flatten().map(|to| ParamKey::Field {
            feature,
            key: format!("{rows}.{to}.{rest}"),
        }))
    };
    let mut fix = |keys: &[ParamKey]| -> Vec<ParamKey> {
        keys.iter()
            .filter_map(|k| match move_key(k) {
                None => Some(k.clone()),
                Some(to) => to,
            })
            .collect()
    };
    views
        .iter()
        .map(|v| ParamView {
            members: fix(&v.members),
            hidden: fix(&v.hidden),
            ..v.clone()
        })
        .collect()
}

fn check_expr(expr: &str) -> Result<(), TxError> {
    kmb_expr::parse(expr)
        .map(|_| ())
        .map_err(|e| TxError::BadExpr {
            expr: expr.into(),
            message: e.to_string(),
        })
}

fn check_name(data: &DocumentData, name: &str, except: Option<ParamId>) -> Result<(), TxError> {
    if !kmb_expr::is_valid_name(name) {
        return Err(TxError::InvalidName(name.into()));
    }
    if data
        .params
        .iter()
        .any(|p| p.name == name && Some(p.id) != except)
    {
        return Err(TxError::DuplicateName(name.into()));
    }
    Ok(())
}

fn id_in_use(data: &DocumentData, id: u64) -> bool {
    data.params.iter().any(|p| p.id.0 == id)
        || data.features.iter().any(|f| f.id.0 == id)
        || data.groups.iter().any(|g| g.id == id)
        || data.param_views.iter().any(|v| v.id == id)
        || data.named_views.iter().any(|v| v.id == id)
        || data.removed_params.iter().any(|p| p.id.0 == id)
}

/// 消したパラメータの一覧が正しいか: 番号が割り当て済みで、互いにも今のパラメータ・機能などとも
/// 重ならない。名前は今のパラメータと同じでもよい(戻す時に断る)。
fn check_removed(data: &DocumentData, params: &[Param]) -> Result<(), TxError> {
    for (k, p) in params.iter().enumerate() {
        let id = p.id.0;
        if id >= data.next_id {
            return Err(TxError::IdNotAllocated(id));
        }
        if params[..k].iter().any(|q| q.id == p.id)
            || data.params.iter().any(|q| q.id == p.id)
            || data.features.iter().any(|f| f.id.0 == id)
            || data.groups.iter().any(|g| g.id == id)
            || data.param_views.iter().any(|v| v.id == id)
            || data.named_views.iter().any(|v| v.id == id)
        {
            return Err(TxError::DuplicateId(id));
        }
    }
    Ok(())
}

/// 名前を付けた視点の一覧が正しいか: 番号が割り当て済みで重ならない、名前が空でない、
/// 数が有限で、見える高さが正、ピッチが ±90 度の中。
fn check_named_views(data: &DocumentData, views: &[NamedView]) -> Result<(), TxError> {
    for (k, v) in views.iter().enumerate() {
        if v.id >= data.next_id {
            return Err(TxError::IdNotAllocated(v.id));
        }
        if views[..k].iter().any(|w| w.id == v.id)
            || data.params.iter().any(|p| p.id.0 == v.id)
            || data.features.iter().any(|f| f.id.0 == v.id)
            || data.groups.iter().any(|g| g.id == v.id)
            || data.param_views.iter().any(|w| w.id == v.id)
        {
            return Err(TxError::DuplicateId(v.id));
        }
        if v.name.trim().is_empty() {
            return Err(TxError::BadNamedView("名前が空です".into()));
        }
        let finite = v
            .target
            .iter()
            .chain([&v.yaw, &v.pitch, &v.roll, &v.height])
            .all(|x| x.is_finite());
        if !finite || v.height <= 0.0 || v.pitch.abs() > 90.0 {
            return Err(TxError::BadNamedView(format!(
                "「{}」の向きか大きさが正しくありません",
                v.name
            )));
        }
    }
    Ok(())
}

/// パラメータの表の一覧が正しいか: 番号が割り当て済みで重ならない、名前が空でない。
/// 行の指す先は確かめない(消えた物を指す行は表に出さないだけ)。
fn check_views(data: &DocumentData, views: &[ParamView]) -> Result<(), TxError> {
    for (k, v) in views.iter().enumerate() {
        if v.id >= data.next_id {
            return Err(TxError::IdNotAllocated(v.id));
        }
        if views[..k].iter().any(|w| w.id == v.id)
            || data.params.iter().any(|p| p.id.0 == v.id)
            || data.features.iter().any(|f| f.id.0 == v.id)
            || data.groups.iter().any(|g| g.id == v.id)
            || data.named_views.iter().any(|w| w.id == v.id)
        {
            return Err(TxError::DuplicateId(v.id));
        }
        if v.name.trim().is_empty() {
            return Err(TxError::BadParamView("名前が空です".into()));
        }
        if v.favorites && views[..k].iter().any(|w| w.favorites) {
            return Err(TxError::BadParamView(
                "お気に入りの表は 1 つだけです".into(),
            ));
        }
    }
    Ok(())
}

/// グループの一覧が正しいか: 番号が割り当て済みで重ならない、両端の機能があり順に並ぶ、
/// 2 つのグループは離れているか一方が他方を含む。
fn check_groups(data: &DocumentData, groups: &[Group]) -> Result<(), TxError> {
    let mut ranges = Vec::new();
    for (k, g) in groups.iter().enumerate() {
        if g.id >= data.next_id {
            return Err(TxError::IdNotAllocated(g.id));
        }
        if groups[..k].iter().any(|h| h.id == g.id)
            || data.params.iter().any(|p| p.id.0 == g.id)
            || data.features.iter().any(|f| f.id.0 == g.id)
        {
            return Err(TxError::DuplicateId(g.id));
        }
        let a = feature_index(data, g.first)?;
        let b = feature_index(data, g.last)?;
        if a > b {
            return Err(TxError::BadGroup(format!(
                "「{}」の先頭が末尾より後ろにあります",
                g.name
            )));
        }
        ranges.push((a, b, &g.name));
    }
    for (i, x) in ranges.iter().enumerate() {
        for y in &ranges[..i] {
            let apart = x.1 < y.0 || y.1 < x.0;
            let nested = (x.0 <= y.0 && y.1 <= x.1) || (y.0 <= x.0 && x.1 <= y.1);
            if !apart && !nested {
                return Err(TxError::BadGroup(format!(
                    "「{}」と「{}」が一部だけ重なります",
                    x.2, y.2
                )));
            }
        }
    }
    Ok(())
}

/// 機能 `id`(位置 `idx`)がグループの端なら、端を隣の中身へ寄せたグループの一覧
/// (中身がその機能だけのグループは、`drop_single` なら除く)。変わらなければ `None`。
fn groups_without(
    data: &DocumentData,
    id: FeatureId,
    idx: usize,
    drop_single: bool,
) -> Option<Vec<Group>> {
    let mut changed = false;
    let mut out = Vec::new();
    for g in &data.groups {
        let mut g = g.clone();
        if g.first == id && g.last == id {
            if drop_single {
                changed = true;
                continue;
            }
        } else if g.first == id {
            g.first = data.features[idx + 1].id;
            changed = true;
        } else if g.last == id {
            g.last = data.features[idx - 1].id;
            changed = true;
        }
        out.push(g);
    }
    changed.then_some(out)
}

fn check_new_id(data: &DocumentData, id: u64) -> Result<(), TxError> {
    if id >= data.next_id {
        return Err(TxError::IdNotAllocated(id));
    }
    if id_in_use(data, id) {
        return Err(TxError::DuplicateId(id));
    }
    Ok(())
}

fn param_index(data: &DocumentData, id: ParamId) -> Result<usize, TxError> {
    data.params
        .iter()
        .position(|p| p.id == id)
        .ok_or(TxError::NoSuchParam(id))
}

fn feature_index(data: &DocumentData, id: FeatureId) -> Result<usize, TxError> {
    data.features
        .iter()
        .position(|f| f.id == id)
        .ok_or(TxError::NoSuchFeature(id))
}

/// 1 つの操作を適用し、それを打ち消す操作の列(この順に適用する)を返す。
fn apply_op(data: &mut DocumentData, op: Op) -> Result<Vec<Op>, TxError> {
    match op {
        Op::AddParam { index, param } => {
            check_new_id(data, param.id.0)?;
            check_name(data, &param.name, None)?;
            check_expr(&param.expr)?;
            let idx = index.unwrap_or(data.params.len());
            if idx > data.params.len() {
                return Err(TxError::IndexOutOfRange(idx));
            }
            let id = param.id;
            data.params.insert(idx, param);
            Ok(vec![Op::RemoveParam { id }])
        }
        Op::RemoveParam { id } => {
            let idx = param_index(data, id)?;
            let param = data.params.remove(idx);
            Ok(vec![Op::AddParam {
                index: Some(idx),
                param,
            }])
        }
        Op::SetParamExpr { id, expr } => {
            let idx = param_index(data, id)?;
            check_expr(&expr)?;
            let old = std::mem::replace(&mut data.params[idx].expr, expr);
            Ok(vec![Op::SetParamExpr { id, expr: old }])
        }
        Op::SetParamInfo { id, comment, unit } => {
            let idx = param_index(data, id)?;
            let p = &mut data.params[idx];
            let old = Op::SetParamInfo {
                id,
                comment: std::mem::replace(&mut p.comment, comment),
                unit: std::mem::replace(&mut p.unit, unit),
            };
            Ok(vec![old])
        }
        Op::RenameParam { id, name } => {
            let idx = param_index(data, id)?;
            check_name(data, &name, Some(id))?;
            let old = std::mem::replace(&mut data.params[idx].name, name);
            Ok(vec![Op::RenameParam { id, name: old }])
        }
        Op::InsertFeature { index, entry } => {
            check_new_id(data, entry.id.0)?;
            if entry.type_id.is_empty() {
                return Err(TxError::EmptyTypeId);
            }
            let len = data.features.len();
            let old_rb = data.rollback;
            let idx = match index {
                Some(i) if i > len => return Err(TxError::IndexOutOfRange(i)),
                Some(i) => i,
                None => data.active_len(),
            };
            if let Some(n) = data.rollback
                && (idx < n || (index.is_none() && idx == n))
            {
                data.rollback = Some(n + 1);
            }
            let id = entry.id;
            data.features.insert(idx, entry);
            Ok(vec![
                Op::RemoveFeature { id },
                Op::SetRollback { rollback: old_rb },
            ])
        }
        Op::RemoveFeature { id } => {
            let idx = feature_index(data, id)?;
            let old_rb = data.rollback;
            // 端の機能を消すグループは、端を隣へ寄せる(中身がなくなれば消える)
            let regrouped = groups_without(data, id, idx, true);
            let entry = data.features.remove(idx);
            if let Some(n) = data.rollback
                && idx < n
            {
                data.rollback = Some(n - 1);
            }
            let mut inv = vec![
                Op::InsertFeature {
                    index: Some(idx),
                    entry,
                },
                Op::SetRollback { rollback: old_rb },
            ];
            if let Some(g) = regrouped {
                inv.push(Op::SetGroups {
                    groups: std::mem::replace(&mut data.groups, g),
                });
            }
            Ok(inv)
        }
        Op::MoveFeature { id, to } => {
            let from = feature_index(data, id)?;
            if to >= data.features.len() {
                return Err(TxError::IndexOutOfRange(to));
            }
            // 端の機能を動かすグループは、端を隣の中身へ寄せる(1 つだけのグループは一緒に動く)
            let regrouped = groups_without(data, id, from, false);
            let entry = data.features.remove(from);
            data.features.insert(to, entry);
            let mut inv = vec![Op::MoveFeature { id, to: from }];
            if let Some(g) = regrouped {
                if let Err(e) = check_groups(data, &g) {
                    let entry = data.features.remove(to);
                    data.features.insert(from, entry);
                    return Err(e);
                }
                inv.push(Op::SetGroups {
                    groups: std::mem::replace(&mut data.groups, g),
                });
            }
            Ok(inv)
        }
        Op::SetFeatureData { id, data: new } => {
            let idx = feature_index(data, id)?;
            let old = std::mem::replace(&mut data.features[idx].data, new);
            Ok(vec![Op::SetFeatureData { id, data: old }])
        }
        Op::RenameFeature { id, name } => {
            let idx = feature_index(data, id)?;
            let old = std::mem::replace(&mut data.features[idx].name, name);
            Ok(vec![Op::RenameFeature { id, name: old }])
        }
        Op::SetSuppressed { id, suppressed } => {
            let idx = feature_index(data, id)?;
            let old = std::mem::replace(&mut data.features[idx].suppressed, suppressed);
            Ok(vec![Op::SetSuppressed {
                id,
                suppressed: old,
            }])
        }
        Op::SetRollback { rollback } => {
            if let Some(n) = rollback
                && n > data.features.len()
            {
                return Err(TxError::IndexOutOfRange(n));
            }
            let old = std::mem::replace(&mut data.rollback, rollback);
            Ok(vec![Op::SetRollback { rollback: old }])
        }
        Op::SetGroups { groups } => {
            check_groups(data, &groups)?;
            let old = std::mem::replace(&mut data.groups, groups);
            Ok(vec![Op::SetGroups { groups: old }])
        }
        Op::SetParamViews { views } => {
            check_views(data, &views)?;
            let old = std::mem::replace(&mut data.param_views, views);
            Ok(vec![Op::SetParamViews { views: old }])
        }
        Op::SetRemovedParams { params } => {
            check_removed(data, &params)?;
            let old = std::mem::replace(&mut data.removed_params, params);
            Ok(vec![Op::SetRemovedParams { params: old }])
        }
        Op::RenameBody { root, name } => {
            feature_index(data, root)?;
            // 空の名前は「ボディN」に戻すのと同じ
            let old = match name.map(|n| n.trim().to_string()) {
                Some(n) if !n.is_empty() => data.body_names.insert(root, n),
                _ => data.body_names.remove(&root),
            };
            Ok(vec![Op::RenameBody { root, name: old }])
        }
        Op::SetProperties { properties } => {
            let old = std::mem::replace(&mut data.properties, *properties);
            Ok(vec![Op::SetProperties {
                properties: Box::new(old),
            }])
        }
        Op::SetNamedViews { views } => {
            check_named_views(data, &views)?;
            let old = std::mem::replace(&mut data.named_views, views);
            Ok(vec![Op::SetNamedViews { views: old }])
        }
        Op::SetComponents { components } => {
            check_components(data, &components)?;
            let old = std::mem::replace(&mut data.components, components);
            Ok(vec![Op::SetComponents { components: old }])
        }
        Op::SetSection { key, section } => {
            // 中身の決まりは名乗るモジュールが書く前に確かめる(コアは中身を知らない)
            crate::section::check_key(&key).map_err(TxError::BadSection)?;
            let old = match section {
                Some(s) => data.sections.insert(key.clone(), s),
                None => data.sections.remove(&key),
            };
            Ok(vec![Op::SetSection { key, section: old }])
        }
    }
}

/// 部品の木の決まり: 番号は割り当て済みで重ならない、名前は空でない、親はある部品で輪にならない、
/// 1 つのボディは 1 つの部品にだけ入る。指すボディが今あるかは問わない(取り消しで戻るため)。
fn check_components(data: &DocumentData, list: &[Component]) -> Result<(), TxError> {
    let bad = |why: String| Err(TxError::BadComponent(why));
    let mut ids = std::collections::HashSet::new();
    let mut bodies = std::collections::HashSet::new();
    for c in list {
        if c.id == 0 || c.id >= data.next_id {
            return Err(TxError::IdNotAllocated(c.id));
        }
        if !ids.insert(c.id) {
            return Err(TxError::DuplicateId(c.id));
        }
        if c.name.trim().is_empty() {
            return bad("名前が空です".into());
        }
        for b in &c.bodies {
            if !bodies.insert(*b) {
                return bad(format!("ボディ {b} が 2 つの部品に入っています"));
            }
        }
    }
    for c in list {
        let mut cur = c.parent;
        for _ in 0..=list.len() {
            let Some(p) = cur else { break };
            if p == c.id {
                return bad(format!("部品「{}」が自分の中に入っています", c.name));
            }
            match list.iter().find(|x| x.id == p) {
                Some(x) => cur = x.parent,
                None => return bad(format!("部品「{}」の親 {p} がありません", c.name)),
            }
        }
        if cur.is_some() {
            return bad(format!("部品「{}」の親が輪になっています", c.name));
        }
    }
    Ok(())
}

/// 読み込んだ文書が、編集時と同じ規則を満たすか確かめる。
pub(crate) fn validate(data: &DocumentData) -> Result<(), TxError> {
    let mut rebuilt = DocumentData {
        next_id: data.next_id,
        ..DocumentData::default()
    };
    for p in &data.params {
        apply_op(
            &mut rebuilt,
            Op::AddParam {
                index: None,
                param: p.clone(),
            },
        )?;
    }
    for f in &data.features {
        apply_op(
            &mut rebuilt,
            Op::InsertFeature {
                index: None,
                entry: f.clone(),
            },
        )?;
    }
    apply_op(
        &mut rebuilt,
        Op::SetRollback {
            rollback: data.rollback,
        },
    )?;
    apply_op(
        &mut rebuilt,
        Op::SetGroups {
            groups: data.groups.clone(),
        },
    )?;
    apply_op(
        &mut rebuilt,
        Op::SetParamViews {
            views: data.param_views.clone(),
        },
    )?;
    apply_op(
        &mut rebuilt,
        Op::SetNamedViews {
            views: data.named_views.clone(),
        },
    )?;
    apply_op(
        &mut rebuilt,
        Op::SetRemovedParams {
            params: data.removed_params.clone(),
        },
    )?;
    apply_op(
        &mut rebuilt,
        Op::SetComponents {
            components: data.components.clone(),
        },
    )?;
    for (key, section) in &data.sections {
        apply_op(
            &mut rebuilt,
            Op::SetSection {
                key: key.clone(),
                section: Some(section.clone()),
            },
        )?;
    }
    Ok(())
}
