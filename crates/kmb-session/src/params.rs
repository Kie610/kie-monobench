//! パラメータの表(U50)。機能の値(押し出しの距離など、記述子の式の欄)の一覧と、表(ビュー)ごとに
//! 出す行の決め方。画面(kmb-ui のパラメータの窓)と CLI・MCP が同じ計算を使う。
//!
//! 表は同じパラメータを見る窓で、写しは持たない([`kmb_core::ParamView`])。行は ID と鍵で指す
//! ([`ParamKey`])ので、名前を変えてもずれない。指す先が消えた行は黙って出さない。
//! 「ボディに関わる」行: そのボディを作った機能と、ボディを受け継いだ機能(`body` を読む機能)、
//! それらが入力に使う機能(スケッチなど)をたどり、その寸法と機能の値、それらの式が使う
//! 利用者のパラメータ(パラメータの式が使うパラメータも)を入れる。

use kmb_core::{
    Document, DocumentData, FeatureId, FieldKind, FieldUnit, InputRef, LengthUnit, Op, ParamId,
    ParamKey, ParamStatus, ParamView, RecomputeReport, Registry, Transaction, category,
};
use std::collections::{BTreeSet, HashSet};

/// 機能の値 1 つ(記述子の式の欄)。
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureValue {
    pub feature: FeatureId,
    /// 欄の名前(行の並びの欄は `並び.行番号.欄`)
    pub key: String,
    /// 欄の見出し(行の並びは「半径 2」のように行の番号を添える)
    pub label: String,
    pub expr: String,
    pub status: ParamStatus,
}

/// 表に出す機能の値か(設計の機能だけ。加工の操作の送りなどは出さない)。
fn shows_values(category: &str) -> bool {
    category == category::SKETCH || category == category::SOLID
}

/// 機能の値の一覧(タイムラインの順。空の欄と、今のデータで使わない欄は出さない)。
pub fn feature_values(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
) -> Vec<FeatureValue> {
    let values = report.param_values();
    let eval = |expr: &str, unit: Option<&FieldUnit>| match kmb_core::eval_str(expr, &|n| {
        values.get(n).copied()
    }) {
        Ok(value) => match unit_mismatch(unit, value.dim) {
            Some(message) => ParamStatus::Error {
                message: message.into(),
            },
            None => ParamStatus::Ok { value },
        },
        Err(e) => ParamStatus::Error {
            message: e.to_string(),
        },
    };
    let mut out = Vec::new();
    for f in &data.features {
        let Some(d) = registry.descriptor(&f.type_id) else {
            continue;
        };
        if !shows_values(&d.category) {
            continue;
        }
        let mut push = |key: String,
                        label: String,
                        v: Option<&serde_json::Value>,
                        unit: Option<&FieldUnit>| {
            if let Some(expr) = v.and_then(|v| v.as_str())
                && !expr.trim().is_empty()
            {
                out.push(FeatureValue {
                    feature: f.id,
                    key,
                    label,
                    expr: expr.to_string(),
                    status: eval(expr, unit),
                });
            }
        };
        for field in &d.fields {
            if !field.applies(&f.data) {
                continue;
            }
            match &field.kind {
                FieldKind::Expr { .. } => push(
                    field.key.clone(),
                    field.label.clone(),
                    field.get(&f.data),
                    field.unit.as_ref(),
                ),
                FieldKind::Rows { fields } => {
                    let n = field.row_count(&f.data);
                    for row in 0..n {
                        for sub in fields {
                            if !matches!(sub.kind, FieldKind::Expr { .. }) {
                                continue;
                            }
                            let at = field.row_field(row, sub);
                            if !at.applies(&f.data) {
                                continue;
                            }
                            let label = if n > 1 {
                                format!("{} {}", sub.label, row + 1)
                            } else {
                                sub.label.clone()
                            };
                            push(at.key.clone(), label, at.get(&f.data), at.unit.as_ref());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// 欄に打った字 `typed` を、入れる式にする。数だけを打った時は、欄が宣言した既定の単位を補う:
/// 長さは文書の長さの単位 `length`(インチの文書で「0.5」は 0.5 in)、角度は deg。割合の欄の
/// 「30%」は 0.3、% の欄(残す体積など)の「40%」は 40 にする。式に単位を書けない量と単位の無い
/// 数は数のまま。編集パネル・値の箱・パラメータの表・MCP とコマンドの set_value が同じ規則を使う。
pub fn typed_expr(unit: Option<&FieldUnit>, typed: &str, length: LengthUnit) -> String {
    let t = typed.trim();
    let bare = t.parse::<f64>().is_ok();
    let percent = || t.strip_suffix('%').map(|n| n.trim().parse::<f64>());
    match unit {
        Some(FieldUnit::Length) if bare => format!("{t} {}", length.name()),
        Some(FieldUnit::Angle) if bare => format!("{t} deg"),
        Some(FieldUnit::Ratio) => match percent() {
            Some(Ok(n)) => format!("{}", n / 100.0),
            _ => t.to_string(),
        },
        Some(FieldUnit::Plain(u)) if u == "%" => match percent() {
            Some(Ok(n)) => format!("{n}"),
            _ => t.to_string(),
        },
        _ => t.to_string(),
    }
}

/// 欄の単位と式の値の量が食い違う時の理由(長さの欄に角度、角度の欄に長さ)。打った時に欄の横へ
/// 出し、計算まで待たずに知らせる(U48)。送りの速さのように長さの式で別の量を書く欄があるので、
/// 長さと角度の取り違えだけを見る
pub fn unit_mismatch(unit: Option<&FieldUnit>, dim: kmb_core::Dim) -> Option<&'static str> {
    match unit {
        Some(FieldUnit::Length) if dim == kmb_core::Dim::ANGLE => {
            Some("長さの欄に角度を書いています")
        }
        Some(FieldUnit::Angle) if dim == kmb_core::Dim::LENGTH => {
            Some("角度の欄に長さを書いています")
        }
        _ => None,
    }
}

/// 単位を宣言していない欄(外のモジュールの欄など)で数だけを打った時は、今の値を表に出す時の
/// 単位(長さは文書の単位 `length`)を付ける。インチの文書で表の「0.5 in」を見て「0.5」と打てば
/// 0.5 in になる。
fn with_unit_of_value(new: &str, now: &ParamStatus, length: LengthUnit) -> String {
    let new = new.trim();
    match now {
        ParamStatus::Ok { value } if new.parse::<f64>().is_ok() => {
            let s = length.show(*value);
            match s.split_once(' ') {
                Some((n, u)) if n.parse::<f64>().is_ok() && u.chars().all(char::is_alphabetic) => {
                    format!("{new} {u}")
                }
                _ => new.to_string(),
            }
        }
        _ => new.to_string(),
    }
}

/// 機能 `feature` の値 `key` の式を変える(取り消しは 1 回)。数だけなら欄の既定の単位を補う([`typed_expr`])。
pub fn set_feature_value(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    feature: FeatureId,
    key: &str,
    expr: &str,
) -> Result<Transaction, String> {
    let f = data
        .feature(feature)
        .ok_or_else(|| format!("機能 {feature} がありません"))?;
    let now = feature_values(data, registry, report)
        .into_iter()
        .find(|v| v.feature == feature && v.key == key)
        .ok_or_else(|| {
            format!(
                "{} に値「{key}」がありません(パラメータの表の機能の値だけを直せます)",
                f.name
            )
        })?;
    let d = registry
        .descriptor(&f.type_id)
        .ok_or_else(|| format!("{} は欄で直せません", f.name))?;
    let field = d
        .field_at(key)
        .ok_or_else(|| format!("{} に「{key}」の欄がありません", f.name))?;
    let expr = match &field.unit {
        Some(u) => typed_expr(Some(u), expr, data.properties.units),
        None => with_unit_of_value(expr, &now.status, data.properties.units),
    };
    let refs = kmb_core::expr_inputs(&expr).map_err(|e| format!("式 `{expr}` の誤り: {e}"))?;
    // 知らない名前は、機能を止めてしまうので入れる前に断る
    for r in refs {
        if let kmb_core::InputRef::Param { name } = r
            && data.param_by_name(&name).is_none()
            && !report.model_params.iter().any(|p| p.name == name)
        {
            return Err(format!(
                "式 `{expr}` の {name} というパラメータはありません"
            ));
        }
    }
    let mut new = f.data.clone();
    field.set(&mut new, Some(serde_json::Value::String(expr.clone())));
    Ok(Transaction::new(
        format!("{}の{}を {expr} に変更", f.name, now.label),
        vec![Op::SetFeatureData {
            id: feature,
            data: new,
        }],
    ))
}

/// 寸法の行の鍵(機能が鍵を出さなければ名前)。
fn model_key(p: &kmb_core::ModelParamResult) -> String {
    if p.key.is_empty() {
        p.name.clone()
    } else {
        p.key.clone()
    }
}

/// 文書の今の行すべて(「全部」の表)。利用者のパラメータ、寸法、機能の値の順。
pub fn all_keys(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
) -> Vec<ParamKey> {
    let mut out: Vec<ParamKey> = data
        .params
        .iter()
        .map(|p| ParamKey::User { id: p.id })
        .collect();
    out.extend(report.model_params.iter().map(|p| ParamKey::Model {
        feature: p.feature,
        key: model_key(p),
    }));
    out.extend(
        feature_values(data, registry, report)
            .into_iter()
            .map(|v| ParamKey::Field {
                feature: v.feature,
                key: v.key,
            }),
    );
    out
}

/// ボディ `body`(最初に作った機能)に関わる行(タイムラインの順)。
pub fn body_keys(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    body: FeatureId,
) -> Vec<ParamKey> {
    // ボディの機能と、それらが入力に使う機能
    let mut features: HashSet<FeatureId> = data
        .features
        .iter()
        .filter(|f| kmb_core::body_root(data, registry, f.id) == body)
        .map(|f| f.id)
        .collect();
    let mut names: HashSet<String> = HashSet::new();
    let mut todo: Vec<FeatureId> = features.iter().copied().collect();
    while let Some(id) = todo.pop() {
        let Some(f) = data.feature(id) else { continue };
        let Some(Ok(feature)) = registry.instantiate(&f.type_id, &f.data) else {
            continue;
        };
        for i in feature.inputs().unwrap_or_default() {
            match i {
                InputRef::Output { feature, .. } => {
                    if features.insert(feature) {
                        todo.push(feature);
                    }
                }
                InputRef::Param { name } => {
                    names.insert(name);
                }
                InputRef::Units => {}
            }
        }
    }
    // 式が使う名前を、パラメータと寸法の式の中までたどる
    let expr_of = |n: &str| {
        data.param_by_name(n)
            .map(|p| p.completed_expr())
            .or_else(|| {
                report
                    .model_params
                    .iter()
                    .find(|p| p.name == n)
                    .map(|p| p.expr.clone())
            })
    };
    let mut todo: Vec<String> = names.iter().cloned().collect();
    // 関わる機能の寸法の式が使う名前も
    for p in &report.model_params {
        if features.contains(&p.feature) {
            todo.push(p.name.clone());
        }
    }
    while let Some(n) = todo.pop() {
        names.insert(n.clone());
        if let Some(e) = expr_of(&n) {
            // 文字のパラメータの式は差し込みの文字
            let refs = kmb_core::parse_expr(&e)
                .map(|ast| ast.references())
                .or_else(|_| kmb_core::Template::parse(&e).map(|t| t.references()))
                .unwrap_or_default();
            for r in refs {
                if !names.contains(&r) {
                    todo.push(r);
                }
            }
        }
    }
    all_keys(data, registry, report)
        .into_iter()
        .filter(|k| match k {
            ParamKey::User { id } => data
                .params
                .iter()
                .any(|p| p.id == *id && names.contains(&p.name)),
            ParamKey::Model { feature, key } => {
                features.contains(feature)
                    || report.model_params.iter().any(|p| {
                        p.feature == *feature && model_key(p) == *key && names.contains(&p.name)
                    })
            }
            ParamKey::Field { feature, .. } => features.contains(feature),
        })
        .collect()
}

/// 表 `view` に出す行(今ある物だけ)。手で入れた行を入れた順に先に、決まりで入る行を後に並べ、
/// 外した行を除く。
pub fn view_keys(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    view: &ParamView,
) -> Vec<ParamKey> {
    let all: BTreeSet<ParamKey> = all_keys(data, registry, report).into_iter().collect();
    let mut seen = HashSet::new();
    let ruled = view
        .body
        .map(|b| body_keys(data, registry, report, b))
        .unwrap_or_default();
    view.members
        .iter()
        .cloned()
        .chain(ruled)
        .filter(|k| all.contains(k) && !view.hidden.contains(k) && seen.insert(k.clone()))
        .collect()
}

/// 行を人が読む名前で(「width」「スケッチ1 の d1」「押し出し1 の距離」)。消えた物は `None`。
pub fn key_label(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    key: &ParamKey,
) -> Option<String> {
    let fname = |id: FeatureId| data.feature(id).map(|f| f.name.clone());
    match key {
        ParamKey::User { id } => data
            .params
            .iter()
            .find(|p| p.id == *id)
            .map(|p| p.name.clone()),
        ParamKey::Model { feature, key } => report
            .model_params
            .iter()
            .find(|p| p.feature == *feature && model_key(p) == *key)
            .map(|p| format!("{} の {}", fname(*feature).unwrap_or_default(), p.name)),
        ParamKey::Field { feature, key } => feature_values(data, registry, report)
            .into_iter()
            .find(|v| v.feature == *feature && v.key == *key)
            .map(|v| format!("{} の{}", fname(*feature).unwrap_or_default(), v.label)),
    }
}

/// 行の書き方(CLI・MCP): 利用者のパラメータか寸法の名前(`width`、`d1`)か、
/// 機能の値の `機能の ID:欄`(`5:distance`)。
pub fn parse_key(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    s: &str,
) -> Result<ParamKey, String> {
    let s = s.trim();
    if let Some((f, key)) = s.split_once(':')
        && let Ok(f) = f.trim().parse::<u64>()
    {
        let k = ParamKey::Field {
            feature: FeatureId(f),
            key: key.trim().to_string(),
        };
        return if key_label(data, registry, report, &k).is_some() {
            Ok(k)
        } else {
            Err(format!("機能 {f} に値「{}」がありません", key.trim()))
        };
    }
    if let Some(p) = data.param_by_name(s) {
        return Ok(ParamKey::User { id: p.id });
    }
    if let Some(p) = report.model_params.iter().find(|p| p.name == s) {
        return Ok(ParamKey::Model {
            feature: p.feature,
            key: model_key(p),
        });
    }
    Err(format!(
        "行 {s} がありません(パラメータか寸法の名前、機能の値は「機能の ID:欄」)"
    ))
}

/// [`parse_key`] の逆。
pub fn key_text(report: &RecomputeReport, data: &DocumentData, key: &ParamKey) -> String {
    match key {
        ParamKey::User { id } => data
            .params
            .iter()
            .find(|p| p.id == *id)
            .map_or_else(|| format!("param:{}", id.0), |p| p.name.clone()),
        ParamKey::Model { feature, key } => report
            .model_params
            .iter()
            .find(|p| p.feature == *feature && model_key(p) == *key)
            .map_or_else(|| format!("{}:{key}", feature.0), |p| p.name.clone()),
        ParamKey::Field { feature, key } => format!("{}:{key}", feature.0),
    }
}

fn views_tx(label: String, views: Vec<ParamView>) -> Transaction {
    Transaction::new(label, vec![Op::SetParamViews { views }])
}

fn view_index(data: &DocumentData, id: u64) -> Result<usize, String> {
    data.param_views
        .iter()
        .position(|v| v.id == id)
        .ok_or_else(|| format!("パラメータの表 {id} がありません"))
}

/// 新しい表(`body` があればそのボディに関わる行を決まりで入れる)。番号も返す。
pub fn add_view(
    doc: &mut Document,
    name: &str,
    body: Option<FeatureId>,
    members: Vec<ParamKey>,
) -> Result<(Transaction, u64), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("表の名前が空です".into());
    }
    let id = doc.alloc_view_id();
    let mut views = doc.data().param_views.clone();
    views.push(ParamView {
        id,
        name: name.to_string(),
        members,
        body,
        hidden: Vec::new(),
        favorites: false,
    });
    Ok((
        views_tx(format!("パラメータの表「{name}」を作成"), views),
        id,
    ))
}

/// 表の名前を変える。
pub fn rename_view(data: &DocumentData, id: u64, name: &str) -> Result<Transaction, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("表の名前が空です".into());
    }
    let i = view_index(data, id)?;
    let mut views = data.param_views.clone();
    let old = std::mem::replace(&mut views[i].name, name.to_string());
    Ok(views_tx(
        format!("パラメータの表「{old}」を「{name}」に変更"),
        views,
    ))
}

/// 表を消す(中のパラメータは消さない)。
pub fn remove_view(data: &DocumentData, id: u64) -> Result<Transaction, String> {
    let i = view_index(data, id)?;
    let mut views = data.param_views.clone();
    let v = views.remove(i);
    Ok(views_tx(
        format!("パラメータの表「{}」を削除", v.name),
        views,
    ))
}

/// 表に行を入れる(外していた行は戻す)。
pub fn add_to_view(data: &DocumentData, id: u64, keys: &[ParamKey]) -> Result<Transaction, String> {
    let i = view_index(data, id)?;
    let mut views = data.param_views.clone();
    let v = &mut views[i];
    for k in keys {
        v.hidden.retain(|h| h != k);
        if !v.members.contains(k) {
            v.members.push(k.clone());
        }
    }
    let name = v.name.clone();
    Ok(views_tx(
        format!("パラメータの表「{name}」に {} 行を入れる", keys.len()),
        views,
    ))
}

/// 表から行を外す(パラメータは消さない)。決まりで入る行は外した行として覚える。
pub fn remove_from_view(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    id: u64,
    keys: &[ParamKey],
) -> Result<Transaction, String> {
    let i = view_index(data, id)?;
    let ruled = data.param_views[i]
        .body
        .map(|b| body_keys(data, registry, report, b))
        .unwrap_or_default();
    let mut views = data.param_views.clone();
    let v = &mut views[i];
    for k in keys {
        v.members.retain(|m| m != k);
        if ruled.contains(k) && !v.hidden.contains(k) {
            v.hidden.push(k.clone());
        }
    }
    let name = v.name.clone();
    Ok(views_tx(
        format!("パラメータの表「{name}」から {} 行を外す", keys.len()),
        views,
    ))
}

/// ボディ `body`(最初に作った機能)の表の名前(「ボディ1 のパラメータ」。名前を付けたボディはその名前)。
pub fn body_view_name(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    body: FeatureId,
) -> String {
    let vis: Vec<FeatureId> = kmb_core::visible_bodies(data, registry, report)
        .into_iter()
        .map(|(f, _)| f)
        .collect();
    let labels = kmb_core::body_labels(data, registry, &vis);
    let label = vis
        .iter()
        .position(|b| kmb_core::body_root(data, registry, *b) == body)
        .map(|i| labels[i].clone())
        .or_else(|| data.body_names.get(&body).cloned())
        .unwrap_or_else(|| "ボディ".into());
    format!("{label} のパラメータ")
}

/// 利用者のパラメータの行の ID(画面が利用者の行を絞るのに使う)。
pub fn user_ids(keys: &[ParamKey]) -> HashSet<ParamId> {
    keys.iter()
        .filter_map(|k| match k {
            ParamKey::User { id } => Some(*id),
            _ => None,
        })
        .collect()
}

/// お気に入りの表(U52)の行 `key` を入れる(`on`)・外す。表がまだなければ作る(番号を割り当てる)。
pub fn set_favorite(doc: &mut Document, key: &ParamKey, on: bool) -> Transaction {
    let mut views = doc.data().param_views.clone();
    let i = match views.iter().position(|v| v.favorites) {
        Some(i) => i,
        None => {
            let id = doc.alloc_view_id();
            views.push(ParamView {
                id,
                name: FAVORITES.into(),
                members: Vec::new(),
                body: None,
                hidden: Vec::new(),
                favorites: true,
            });
            views.len() - 1
        }
    };
    let v = &mut views[i];
    v.members.retain(|m| m != key);
    if on {
        v.members.push(key.clone());
    }
    let label = if on {
        "お気に入りに入れる"
    } else {
        "お気に入りから外す"
    };
    views_tx(label.into(), views)
}

/// お気に入りの表のタブの名前。
pub const FAVORITES: &str = "お気に入り";

/// 行 `key` がお気に入りか。
pub fn is_favorite(data: &DocumentData, key: &ParamKey) -> bool {
    data.favorites_view()
        .is_some_and(|v| v.members.contains(key))
}

/// 表 `id` の中で行 `key` を同じ種類(パラメータ・寸法・機能の値)の 1 つ上(`up`)か下の行と
/// 入れ替える(U52 の並べ替え)。今出ている行を手で並べた順として覚えるので、決まりで入った行も
/// 並べた後は手で入れた行になる。端の行なら `None`。
pub fn move_in_view(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    id: u64,
    key: &ParamKey,
    up: bool,
) -> Result<Option<Transaction>, String> {
    let i = view_index(data, id)?;
    let mut order = view_keys(data, registry, report, &data.param_views[i]);
    let same = |a: &ParamKey, b: &ParamKey| std::mem::discriminant(a) == std::mem::discriminant(b);
    let Some(at) = order.iter().position(|k| k == key) else {
        return Err("この行は表にありません".into());
    };
    let other = if up {
        order[..at].iter().rposition(|k| same(k, key))
    } else {
        order[at + 1..]
            .iter()
            .position(|k| same(k, key))
            .map(|j| at + 1 + j)
    };
    let Some(other) = other else {
        return Ok(None);
    };
    order.swap(at, other);
    let mut views = data.param_views.clone();
    let v = &mut views[i];
    // 今は出ていない行(消えた物を指す行)は、取り消しで戻った時のために後ろに残す
    let rest: Vec<ParamKey> = v
        .members
        .iter()
        .filter(|m| !order.contains(m))
        .cloned()
        .collect();
    v.members = order.into_iter().chain(rest).collect();
    let name = v.name.clone();
    Ok(Some(views_tx(
        format!("パラメータの表「{name}」の行を並べ替える"),
        views,
    )))
}
