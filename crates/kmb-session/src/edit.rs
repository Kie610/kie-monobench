//! 文書の変更の組み立て。画面(kmb-ui)も CLI・MCP([`crate::Headless`])も、ここで作った
//! [`Transaction`] を文書に適用する。変更の決まり(取り消しの名前、グループの扱いなど)を
//! 1 か所にまとめ、画面だけが別の経路で文書を変えることがないようにする。
//!
//! どれも文書を読むだけで変えない。番号を割り当てる変更だけ `&mut Document` を受け取る。

use kmb_core::{
    Document, DocumentData, FeatureEntry, FeatureId, Group, InputRef, NamedView, Op, Param,
    ParamId, Properties, Registry, Transaction,
};
use std::collections::{HashMap, HashSet};

fn feature_name(data: &DocumentData, id: FeatureId) -> String {
    data.feature(id)
        .map_or_else(|| "機能".to_string(), |f| f.name.clone())
}

fn index(data: &DocumentData, id: FeatureId) -> Result<usize, String> {
    data.index_of(id)
        .ok_or_else(|| format!("機能 {id} がありません"))
}

/// パラメータの式を変える。
pub fn set_param_expr(data: &DocumentData, id: ParamId, expr: &str) -> Transaction {
    let label = match data.params.iter().find(|p| p.id == id) {
        Some(p) => format!("{} を {expr} に変更", p.name),
        None => "パラメータを変更".to_string(),
    };
    Transaction::new(
        label,
        vec![Op::SetParamExpr {
            id,
            expr: expr.into(),
        }],
    )
}

/// パラメータを最後に足す。
pub fn add_param(doc: &mut Document, name: &str, expr: &str) -> Transaction {
    add_param_with(doc, name, expr, kmb_core::ParamUnit::Any)
}

/// 単位つきでパラメータを最後に足す。
pub fn add_param_with(
    doc: &mut Document,
    name: &str,
    expr: &str,
    unit: kmb_core::ParamUnit,
) -> Transaction {
    let id = doc.alloc_param_id();
    Transaction::new(
        "パラメータを追加",
        vec![Op::AddParam {
            index: None,
            param: Param {
                unit,
                ..Param::new(id, name, expr)
            },
        }],
    )
}

/// パラメータのコメントと単位(表の列)。省いた物は今のまま。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParamInfo {
    pub comment: Option<String>,
    pub unit: Option<kmb_core::ParamUnit>,
}

impl ParamInfo {
    /// 画面・CLI・MCP・コマンド欄の引数から作る。単位は length・angle・unitless・any。
    pub fn parse(comment: Option<&str>, unit: Option<&str>) -> Result<Self, String> {
        let unit = match unit.map(str::trim) {
            None => None,
            Some(u) => Some(
                serde_json::from_value(serde_json::Value::String(u.to_string())).map_err(|_| {
                    format!("パラメータの単位 {u} は使えません(length・angle・unitless・any)")
                })?,
            ),
        };
        Ok(ParamInfo {
            comment: comment.map(str::to_string),
            unit,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.comment.is_none() && self.unit.is_none()
    }

    /// パラメータ `id` のコメントと単位を変える操作(変える物が無ければ無し)。
    pub fn op(&self, data: &DocumentData, id: ParamId) -> Option<Op> {
        let p = data.params.iter().find(|p| p.id == id)?;
        if self.is_empty() {
            return None;
        }
        Some(Op::SetParamInfo {
            id,
            comment: self.comment.clone().unwrap_or_else(|| p.comment.clone()),
            unit: self.unit.unwrap_or(p.unit),
        })
    }
}

/// コメントと単位つきでパラメータを最後に足す。
pub fn add_param_info(doc: &mut Document, name: &str, expr: &str, info: &ParamInfo) -> Transaction {
    let mut tx = add_param_with(doc, name, expr, info.unit.unwrap_or_default());
    if let Some(Op::AddParam { param, .. }) = tx.ops.first_mut() {
        param.comment = info.comment.clone().unwrap_or_default();
    }
    tx
}

/// パラメータ `id` を式で使っているパラメータと機能の名前(古い順)。使われていれば消せない。
pub fn param_users(data: &DocumentData, registry: &Registry, id: ParamId) -> Vec<String> {
    let Some(name) = data
        .params
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
    else {
        return Vec::new();
    };
    name_users(data, registry, &name, Some(id))
}

/// 名前 `name`(パラメータか寸法の名前)を式で使っている物の名前(パラメータ、寸法、機能の古い順)。
/// 寸法は「スケッチ1 の d3」の形で出し、寸法で使っている機能はその機能の名前を重ねて出さない。
/// `except` はそのパラメータ自身を数えないため。
pub fn name_users(
    data: &DocumentData,
    registry: &Registry,
    name: &str,
    except: Option<ParamId>,
) -> Vec<String> {
    NameUsers::new(data, registry).of(name, except)
}

/// パラメータと寸法の名前ごとの、使っている物の名前([`name_users`] と同じ並び)。パラメータは自身を
/// 数えない。パラメータの表が行ごとに [`name_users`] を呼ぶと、寸法の一覧と機能の読み直しを行の数だけ
/// 繰り返して、大きな設計で表が重くなった。こちらは 1 回の読み直しで全部の名前を数える
pub fn all_name_users(data: &DocumentData, registry: &Registry) -> HashMap<String, Vec<String>> {
    let scan = NameUsers::new(data, registry);
    let mut all: HashMap<String, Vec<String>> = HashMap::new();
    for p in &data.params {
        all.insert(p.name.clone(), scan.of(&p.name, Some(p.id)));
    }
    for (_, p) in &scan.model {
        if !all.contains_key(&p.name) {
            all.insert(p.name.clone(), scan.of(&p.name, None));
        }
    }
    all
}

/// 機能の番号・名前と、読み直した機能(読めない機能は `None`)。
type ReadFeature<'a> = (
    kmb_core::FeatureId,
    &'a str,
    Option<Box<dyn kmb_core::Feature>>,
);

/// 名前を使っている物を探すための、寸法の一覧と読み直した機能(名前ごとに作り直さない)。
struct NameUsers<'a> {
    data: &'a DocumentData,
    model: Vec<(kmb_core::FeatureId, kmb_core::ModelParam)>,
    features: Vec<ReadFeature<'a>>,
}

impl<'a> NameUsers<'a> {
    fn new(data: &'a DocumentData, registry: &Registry) -> Self {
        let features = data
            .features
            .iter()
            .map(|f| {
                let made = match registry.instantiate(&f.type_id, &f.data) {
                    Some(Ok(feature)) => Some(feature),
                    _ => None,
                };
                (f.id, f.name.as_str(), made)
            })
            .collect();
        NameUsers {
            data,
            model: kmb_core::model_params(data, registry),
            features,
        }
    }

    fn of(&self, name: &str, except: Option<ParamId>) -> Vec<String> {
        let data = self.data;
        // 名前を仮の名前に替えてみて、式が変わるものが使っている物
        let probe = "__jc_param_probe__";
        let uses =
            |expr: &str| kmb_core::rename_reference(expr, name, probe).is_ok_and(|e| e != expr);
        let mut users: Vec<String> = data
            .params
            .iter()
            .filter(|p| Some(p.id) != except)
            .filter(|p| uses(&p.expr))
            .map(|p| p.name.clone())
            .collect();
        let feature_name = |id: kmb_core::FeatureId| {
            data.feature(id)
                .map_or_else(String::new, |f| f.name.clone())
        };
        // 寸法の名前なら、その寸法を持つ機能は名前を変えると自分の寸法の名前も変わるので数えない
        let owners: Vec<kmb_core::FeatureId> = self
            .model
            .iter()
            .filter(|(_, p)| p.name == name)
            .map(|(f, _)| *f)
            .collect();
        let dims: Vec<(kmb_core::FeatureId, &str)> = self
            .model
            .iter()
            .filter(|(_, p)| p.name != name && uses(&p.expr))
            .map(|(f, p)| (*f, p.name.as_str()))
            .collect();
        users.extend(
            dims.iter()
                .map(|(f, n)| format!("{} の {n}", feature_name(*f))),
        );
        users.extend(
            self.features
                .iter()
                .filter(|(id, _, _)| !dims.iter().any(|(d, _)| d == id) && !owners.contains(id))
                .filter(|(_, _, made)| {
                    made.as_ref()
                        .is_some_and(|f| matches!(f.rename_param(name, probe), Ok(Some(_))))
                })
                .map(|(_, n, _)| n.to_string()),
        );
        users
    }
}

/// パラメータ `id` のコメントと単位を変える(パラメータの表の列。取り消せる)。
pub fn set_param_info(
    data: &DocumentData,
    id: ParamId,
    comment: &str,
    unit: kmb_core::ParamUnit,
) -> Transaction {
    let label = match data.params.iter().find(|p| p.id == id) {
        Some(p) => format!("パラメータ {} の説明を変更", p.name),
        None => "パラメータの説明を変更".to_string(),
    };
    Transaction::new(
        label,
        vec![Op::SetParamInfo {
            id,
            comment: comment.to_string(),
            unit,
        }],
    )
}

/// パラメータを消す(取り消せる)。使われているかは [`param_users`] で先に確かめる。
/// 消した物は文書の消したパラメータの一覧に残し、[`restore_param`] で戻せる。
pub fn remove_param(data: &DocumentData, id: ParamId) -> Transaction {
    let Some(p) = data.params.iter().find(|p| p.id == id) else {
        return Transaction::new("パラメータを削除", vec![Op::RemoveParam { id }]);
    };
    let mut removed = data.removed_params.clone();
    removed.push(p.clone());
    Transaction::new(
        format!("パラメータ {} を削除", p.name),
        vec![
            Op::RemoveParam { id },
            Op::SetRemovedParams { params: removed },
        ],
    )
}

/// 消したパラメータ `id` を戻す(取り消せる。表の最後に入る)。同じ名前のパラメータか寸法が
/// あれば理由を返す。
pub fn restore_param(
    data: &DocumentData,
    model_names: &[String],
    id: ParamId,
) -> Result<Transaction, String> {
    let p = data
        .removed_param(id)
        .ok_or_else(|| format!("消したパラメータ {} がありません", id.0))?;
    if data.param_by_name(&p.name).is_some() || model_names.contains(&p.name) {
        return Err(format!(
            "同じ名前 {} が使われているので戻せません(今ある方の名前を変えると戻せます)",
            p.name
        ));
    }
    let removed = data
        .removed_params
        .iter()
        .filter(|q| q.id != id)
        .cloned()
        .collect();
    Ok(Transaction::new(
        format!("パラメータ {} を戻す", p.name),
        vec![
            Op::SetRemovedParams { params: removed },
            Op::AddParam {
                index: None,
                param: p.clone(),
            },
        ],
    ))
}

/// 機能をロールバック位置(なければ最後)に足す。足す機能の番号も返す。
pub fn insert_feature(
    doc: &mut Document,
    label: &str,
    type_id: &str,
    name: &str,
    data: serde_json::Value,
) -> (Transaction, FeatureId) {
    let id = doc.alloc_feature_id();
    let tx = Transaction::new(
        label,
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: type_id.into(),
                name: name.into(),
                data,
                suppressed: false,
                pin_bodies: false,
            },
        }],
    );
    (tx, id)
}

/// 機能のデータを置き換える。
pub fn set_feature_data(data: &DocumentData, id: FeatureId, new: serde_json::Value) -> Transaction {
    Transaction::new(
        format!("{}を変更", feature_name(data, id)),
        vec![Op::SetFeatureData { id, data: new }],
    )
}

/// 選んだ機能 `ids` の欄 `key`(記述子の欄の名前。`a.b` で入れ子)を、まとめて `value` にする
/// (画面で複数の加工の操作を一度に直す時)。欄のない機能があれば変えずに理由を返す。
pub fn set_field(
    data: &DocumentData,
    registry: &Registry,
    ids: &[FeatureId],
    key: &str,
    value: Option<serde_json::Value>,
) -> Result<Transaction, String> {
    let mut ops = Vec::new();
    for &id in ids {
        let f = data
            .feature(id)
            .ok_or_else(|| format!("機能 {id} がありません"))?;
        let d = registry
            .descriptor(&f.type_id)
            .ok_or_else(|| format!("{} は欄で直せません", f.name))?;
        let field = d
            .fields
            .iter()
            .find(|x| x.key == key)
            .ok_or_else(|| format!("{} に「{key}」の欄がありません", f.name))?;
        let mut new = f.data.clone();
        field.set(&mut new, value.clone());
        if new != f.data {
            ops.push(Op::SetFeatureData { id, data: new });
        }
    }
    Ok(Transaction::new(
        format!("{} 個の機能を変更", ids.len()),
        ops,
    ))
}

/// 機能の名前を変える。
pub fn rename_feature(data: &DocumentData, id: FeatureId, name: &str) -> Transaction {
    Transaction::new(
        format!("{}の名前を {name} に変更", feature_name(data, id)),
        vec![Op::RenameFeature {
            id,
            name: name.into(),
        }],
    )
}

/// ボディ(最初に作った機能 `root`)の名前を変える。空なら「ボディN」に戻す。
pub fn rename_body(root: FeatureId, name: &str) -> Transaction {
    let name = name.trim();
    Transaction::new(
        if name.is_empty() {
            "ボディの名前を戻す".to_string()
        } else {
            format!("ボディの名前を {name} に変更")
        },
        vec![Op::RenameBody {
            root,
            name: (!name.is_empty()).then(|| name.to_string()),
        }],
    )
}

/// ボディ(最初に作った機能 `root`)に物理材料を付ける(M26)。空なら外す。名前は表の名前にそろえる
/// (別名は表の名前に。表に無い名前はそのまま)。
pub fn set_body_material(
    data: &kmb_core::DocumentData,
    root: FeatureId,
    material: &str,
) -> Result<Transaction, String> {
    set_body_materials(data, &[root], material)
}

/// いくつものボディ(最初に作った機能)に同じ物理材料を付ける(1 回の取り消し単位)。
pub fn set_body_materials(
    data: &kmb_core::DocumentData,
    roots: &[FeatureId],
    material: &str,
) -> Result<Transaction, String> {
    let name = material.trim();
    let name = kmb_look::material::phys_material(name).map_or(name, |m| m.name);
    let mut all: kmb_look::material::BodyMaterials = data.section().map_err(|e| e.to_string())?;
    for r in roots {
        all.set(*r, Some(name));
    }
    Ok(Transaction::new(
        if name.is_empty() {
            "ボディの材料を外す".to_string()
        } else {
            format!("ボディの材料を {name} に")
        },
        vec![data.section_op(&all)?],
    ))
}

/// ボディ(最初に作った機能 `root`)の外観を替える(M26)。空なら材料の色に戻す。
pub fn set_body_appearance(
    data: &kmb_core::DocumentData,
    root: FeatureId,
    appearance: &str,
) -> Result<Transaction, String> {
    let name = appearance.trim();
    let name = kmb_look::material::appearance(name).map_or(name, |a| a.name);
    let mut all: kmb_look::appearance::Appearances = data.section().map_err(|e| e.to_string())?;
    all.set_body(root, Some(name));
    Ok(Transaction::new(
        if name.is_empty() {
            "ボディの外観を材料の色に戻す".to_string()
        } else {
            format!("ボディの外観を {name} に")
        },
        vec![data.section_op(&all)?],
    ))
}

/// 機能をまとめて抑制する(または戻す)。後ろの機能は、参照が切れれば理由つきで止まる。
pub fn set_suppressed(ids: &[FeatureId], suppressed: bool) -> Transaction {
    let label = if suppressed {
        "抑制"
    } else {
        "抑制を解除"
    };
    Transaction::new(
        label,
        ids.iter()
            .map(|&id| Op::SetSuppressed { id, suppressed })
            .collect(),
    )
}

/// 機能の名前を「」で囲んで並べる(多い時は「ほか N 個」)。取り消しの履歴や確かめの文に使う。
pub fn names(data: &DocumentData, ids: &[FeatureId]) -> String {
    let shown: Vec<String> = ids
        .iter()
        .take(3)
        .map(|&id| format!("「{}」", feature_name(data, id)))
        .collect();
    match ids.len() {
        n if n > 3 => format!("{}ほか {} 個", shown.join(""), n - 3),
        _ => shown.join(""),
    }
}

/// 機能を消す(取り消せる)。使っていた機能は理由つきで止まる。
/// 取り消しの履歴でどれを消したか分かるよう、名前を入れる。
pub fn remove_features(data: &DocumentData, ids: &[FeatureId]) -> Transaction {
    Transaction::new(
        format!("{}を消す", names(data, ids)),
        ids.iter().map(|&id| Op::RemoveFeature { id }).collect(),
    )
}

/// `gone` の機能が消えると、入っているボディが全部なくなる部品(消す前の確かめに名前を出す)。
/// 子の部品が残る部品と、もともとボディの無い部品(入れ物)は数えない。
pub fn components_emptied<'a>(
    data: &'a DocumentData,
    gone: &[FeatureId],
) -> Vec<&'a kmb_core::Component> {
    let empty: Vec<u64> = data
        .components
        .iter()
        .filter(|c| !c.bodies.is_empty() && c.bodies.iter().all(|b| gone.contains(b)))
        .map(|c| c.id)
        .collect();
    data.components
        .iter()
        .filter(|c| empty.contains(&c.id))
        .filter(|c| {
            !data
                .components
                .iter()
                .any(|k| k.parent == Some(c.id) && !empty.contains(&k.id))
        })
        .collect()
}

/// 機能 `ids` を、それを使う機能(たどった先も)と、中身がなくなる部品と一緒に消す(1 回の取り消し
/// 単位)。挿入した部品を、つないだジョイントや空になる部品の箱を残さずに片付ける(採用分 10-24 a1)。
pub fn remove_features_with_users(
    data: &DocumentData,
    registry: &Registry,
    ids: &[FeatureId],
) -> Transaction {
    let mut all = ids.to_vec();
    all.extend(dependents(data, registry, ids));
    let mut ops: Vec<Op> = all.iter().map(|&id| Op::RemoveFeature { id }).collect();
    let emptied: Vec<u64> = components_emptied(data, &all)
        .into_iter()
        .map(|c| c.id)
        .collect();
    if !emptied.is_empty() {
        ops.push(Op::SetComponents {
            components: data
                .components
                .iter()
                .filter(|c| !emptied.contains(&c.id))
                .cloned()
                .collect(),
        });
    }
    Transaction::new(format!("{}と使っていた物を消す", names(data, ids)), ops)
}

/// `ids` を消すと中身がなくなって一緒に消えるグループ(消す前の確かめに名前を出す)。
pub fn groups_emptied<'a>(data: &'a DocumentData, ids: &[FeatureId]) -> Vec<&'a kmb_core::Group> {
    data.groups
        .iter()
        .filter(|g| {
            data.group_range(g)
                .is_some_and(|(a, b)| data.features[a..=b].iter().all(|f| ids.contains(&f.id)))
        })
        .collect()
}

/// 機能 `ids` を使っている機能(`ids` 自身を除く。タイムラインの順)。消す前の確かめに使う。
pub fn users(data: &DocumentData, registry: &Registry, ids: &[FeatureId]) -> Vec<FeatureId> {
    data.features
        .iter()
        .map(|f| f.id)
        .filter(|f| !ids.contains(f))
        .filter(|&f| uses(data, registry, f).iter().any(|u| ids.contains(u)))
        .collect()
}

/// `ids` を消すと計算できなくなる機能(使うものを使う機能もたどる。タイムラインの順)。
pub fn dependents(data: &DocumentData, registry: &Registry, ids: &[FeatureId]) -> Vec<FeatureId> {
    let mut gone: Vec<FeatureId> = ids.to_vec();
    let mut out = Vec::new();
    for f in data.features.iter().map(|f| f.id) {
        if !gone.contains(&f) && uses(data, registry, f).iter().any(|u| gone.contains(u)) {
            gone.push(f);
            out.push(f);
        }
    }
    out
}

/// 機能 `id` をタイムラインの `to` 番目へ動かすと、使う機能より前に来てしまうなら、その理由。
pub fn move_conflict(
    data: &DocumentData,
    registry: &Registry,
    id: FeatureId,
    to: usize,
) -> Option<String> {
    let from = data.index_of(id)?;
    let mut order: Vec<FeatureId> = data.features.iter().map(|f| f.id).collect();
    let moved = order.remove(from);
    order.insert(to.min(order.len()), moved);
    let pos = |f: FeatureId| order.iter().position(|x| *x == f);
    let me = pos(id)?;
    // 動かす機能が使うものが後ろに来る
    for u in uses(data, registry, id) {
        if pos(u).is_some_and(|p| p > me) {
            return Some(format!(
                "{}は{}を使うので、それより前へは動かせません",
                names(data, &[id]),
                names(data, &[u])
            ));
        }
    }
    // 動かす機能を使うものが前に来る
    for f in users(data, registry, &[id]) {
        if pos(f).is_some_and(|p| p < me) {
            return Some(format!(
                "{}は{}を使うので、それより後ろへは動かせません",
                names(data, &[f]),
                names(data, &[id])
            ));
        }
    }
    None
}

/// 機能をタイムラインで `delta` だけ動かす(`-1` で 1 つ前)。動かせなければ `None`。
pub fn move_feature(data: &DocumentData, id: FeatureId, delta: i32) -> Option<Transaction> {
    let i = data.index_of(id)?;
    let to = i as i64 + delta as i64;
    if to < 0 || to as usize >= data.features.len() {
        return None;
    }
    Some(Transaction::new(
        format!("{}を動かす", names(data, &[id])),
        vec![Op::MoveFeature {
            id,
            to: to as usize,
        }],
    ))
}

/// 設計の属性を置き換える(U13。取り消せる)。
pub fn set_properties(properties: Properties) -> Transaction {
    Transaction::new(
        "設計の属性を変える",
        vec![Op::SetProperties {
            properties: Box::new(properties),
        }],
    )
}

/// 今の視点を名前を付けて保存する(U20。`view` の番号は使わず、新しく割り当てる)。
/// 名前が空なら「視点N」。同じ名前があれば後ろに番号を足す。
pub fn save_named_view(doc: &mut Document, mut view: NamedView) -> (Transaction, u64) {
    let data = doc.data();
    let taken = |n: &str| data.named_views.iter().any(|v| v.name == n);
    let base = match view.name.trim() {
        "" => format!("視点{}", data.named_views.len() + 1),
        n => n.to_string(),
    };
    let name = kmb_core::unique_name(&base, taken);
    let mut views = data.named_views.clone();
    view.id = doc.alloc_view_id();
    view.name = name.clone();
    let id = view.id;
    views.push(view);
    (
        Transaction::new(
            format!("視点「{name}」を保存"),
            vec![Op::SetNamedViews { views }],
        ),
        id,
    )
}

/// 名前を付けた視点を 1 つ変える(名前を変える・今の視点で上書きする)。
pub fn change_named_view(
    data: &DocumentData,
    id: u64,
    label: &str,
    f: impl FnOnce(&mut NamedView),
) -> Result<Transaction, String> {
    let mut views = data.named_views.clone();
    let v = views
        .iter_mut()
        .find(|v| v.id == id)
        .ok_or_else(|| format!("視点 {id} がありません"))?;
    f(v);
    Ok(Transaction::new(label, vec![Op::SetNamedViews { views }]))
}

/// 名前を付けた視点を消す。
pub fn remove_named_view(data: &DocumentData, id: u64) -> Result<Transaction, String> {
    let v = data
        .named_view(id)
        .ok_or_else(|| format!("視点 {id} がありません"))?;
    let label = format!("視点「{}」を消す", v.name);
    let views = data
        .named_views
        .iter()
        .filter(|v| v.id != id)
        .cloned()
        .collect();
    Ok(Transaction::new(label, vec![Op::SetNamedViews { views }]))
}

/// 機能を貼った結果([`paste_features`])。
pub struct Pasted {
    pub tx: Transaction,
    /// 貼った機能の番号(写した順)
    pub ids: Vec<FeatureId>,
    /// 空にした選ぶ欄の数(貼る先の面や辺を選び直す数)
    pub cleared: usize,
}

/// 写した設計の機能 `entries` を、タイムラインの今の位置(ロールバックしていればそこ)に貼る
/// 変更を組む(U42。画面の貼り付けと CLI・MCP の写しが同じ道を通る)。値と式はそのまま写し、
/// 面・辺などの選ぶ欄は空にする(貼る先は使う人が選び直す。似た面を探して勧めない)。
/// 前の機能の出力を指す欄は、指す機能も一緒に写していれば貼った写しへ付け替える(まとめて写すと、
/// 貼った方が元の形に効いてしまわないように)。一緒に写していないボディは `tip` が返す機能
/// (今のボディの最後の機能)に付け替える。返さなければそのまま。
pub fn paste_features(
    doc: &mut Document,
    registry: &Registry,
    entries: &[FeatureEntry],
    tip: &dyn Fn(FeatureId) -> Option<FeatureId>,
) -> Pasted {
    let mut names: Vec<String> = doc.data().features.iter().map(|f| f.name.clone()).collect();
    let new_ids: Vec<FeatureId> = entries.iter().map(|_| doc.alloc_feature_id()).collect();
    let copied = |old: FeatureId| entries.iter().position(|e| e.id == old).map(|i| new_ids[i]);
    let mut ops = Changes::new();
    let mut cleared = 0;
    for (e, &id) in entries.iter().zip(&new_ids) {
        let mut d = e.data.clone();
        if let Some(desc) = registry.descriptor(&e.type_id) {
            // 古い形のデータは今の形に直してから選ぶ欄を探す(フィレットの辺の行など)
            d = desc.upgraded(&d);
            for f in desc.pick_fields(&d) {
                if !f.refs(&d).is_empty() {
                    cleared += 1;
                }
                f.set_refs(&mut d, &[]);
            }
            for f in &desc.fields {
                let kmb_core::FieldKind::Output { slot, .. } = &f.kind else {
                    continue;
                };
                let Some(old) = d.get(&f.key).and_then(|v| v.as_u64()).map(FeatureId) else {
                    continue;
                };
                let to = match copied(old) {
                    Some(new) => Some(new),
                    None if slot == "body" && doc.data().feature(old).is_some() => {
                        tip(old).filter(|t| *t != old)
                    }
                    None => None,
                };
                if let Some(to) = to {
                    d[f.key.as_str()] = serde_json::json!(to.0);
                }
            }
        }
        let name = kmb_core::unique_name(&e.name, |t| names.iter().any(|x| x == t));
        names.push(name.clone());
        // 位置を決めずに入れると、ロールバックの位置(なければ最後)に入り、有効な機能になる
        ops.insert_feature(
            None,
            FeatureEntry {
                id,
                type_id: e.type_id.clone(),
                name,
                data: d,
                suppressed: false,
                pin_bodies: false,
            },
        );
    }
    let shown: Vec<String> = entries
        .iter()
        .take(3)
        .map(|e| format!("「{}」", e.name))
        .collect();
    let label = match entries.len() {
        n if n > 3 => format!("{}ほか {} 個を貼り付け", shown.join(""), n - 3),
        _ => format!("{}を貼り付け", shown.join("")),
    };
    Pasted {
        tx: ops.into_transaction(label),
        ids: new_ids,
        cleared,
    }
}

/// 選んだ機能 `ids` をまとめて、機能 `target` の前(`after` なら後ろ)へ動かす(U45)。
/// 動かす機能どうしの順は保ち、続いていなくても 1 か所に並べる。1 回で取り消せる。
/// 使う機能より前に来てしまう時は理由を返す。動かす先が今と同じなら `Ok(None)`。
pub fn move_features(
    data: &DocumentData,
    registry: &Registry,
    ids: &[FeatureId],
    target: FeatureId,
    after: bool,
) -> Result<Option<Transaction>, String> {
    let moved: Vec<FeatureId> = data
        .features
        .iter()
        .map(|f| f.id)
        .filter(|f| ids.contains(f))
        .collect();
    if moved.is_empty() || moved.contains(&target) {
        return Ok(None);
    }
    let mut order: Vec<FeatureId> = data
        .features
        .iter()
        .map(|f| f.id)
        .filter(|f| !moved.contains(f))
        .collect();
    let at = order
        .iter()
        .position(|f| *f == target)
        .ok_or_else(|| format!("機能 {target} がありません"))?
        + usize::from(after);
    order.splice(at..at, moved.iter().copied());
    let now: Vec<FeatureId> = data.features.iter().map(|f| f.id).collect();
    if order == now {
        return Ok(None);
    }
    let pos = |f: FeatureId| order.iter().position(|x| *x == f);
    for &id in &moved {
        let me = pos(id).expect("動かす機能は並びにある");
        if let Some(u) = uses(data, registry, id)
            .into_iter()
            .filter(|u| pos(*u).is_some_and(|p| p > me))
            .min_by_key(|u| pos(*u))
        {
            return Err(format!(
                "{}は{}を使うので、それより前へは動かせません",
                names(data, &[id]),
                names(data, &[u])
            ));
        }
        if let Some(f) = users(data, registry, &[id])
            .into_iter()
            .find(|f| pos(*f).is_some_and(|p| p < me))
        {
            return Err(format!(
                "{}は{}を使うので、それより後ろへは動かせません",
                names(data, &[f]),
                names(data, &[id])
            ));
        }
    }
    // 先頭から順に、あるべき機能をその位置へ動かす(動かした後の位置で次を数える)
    let mut cur = now;
    let mut ops = Vec::new();
    for (k, &id) in order.iter().enumerate() {
        let from = cur.iter().position(|x| *x == id).expect("同じ機能の並び");
        if from != k {
            let f = cur.remove(from);
            cur.insert(k, f);
            ops.push(Op::MoveFeature { id, to: k });
        }
    }
    Ok(Some(Transaction::new(
        format!("{}を動かす", names(data, &moved)),
        ops,
    )))
}

/// ロールバック位置を機能 `id` の後ろにする(`None` なら最後まで進める)。
pub fn roll_back_after(data: &DocumentData, id: Option<FeatureId>) -> Result<Transaction, String> {
    let (label, rollback) = match id {
        Some(id) => ("ここまで戻す", Some(index(data, id)? + 1)),
        None => ("最後まで進める", None),
    };
    Ok(Transaction::new(label, vec![Op::SetRollback { rollback }]))
}

/// ロールバックの位置を 1 つ前(`-1`)か後ろ(`1`)へ動かす。動かせなければ `None`。
pub fn step_rollback(data: &DocumentData, delta: i32) -> Option<Transaction> {
    let n = data.features.len() as i64;
    let now = data.active_len() as i64;
    let to = (now + delta as i64).clamp(0, n);
    if to == now {
        return None;
    }
    let rollback = (to < n).then_some(to as usize);
    Some(Transaction::new(
        if delta < 0 {
            "ここまで戻す"
        } else {
            "1 つ進める"
        },
        vec![Op::SetRollback { rollback }],
    ))
}

/// グループにできない理由。
#[derive(Debug, Clone, PartialEq)]
pub enum GroupError {
    /// 選んだものが続いていない。間に挟まった機能
    Gaps(Vec<FeatureId>),
    /// 寄せると、使う機能が使われる機能より前に来る(使う側、使われる側)
    Order(Vec<(FeatureId, FeatureId)>),
    /// その他(選んでいない、ほかのグループと一部だけ重なる)
    Other(String),
}

impl GroupError {
    /// 利用者に見せる文。
    pub fn describe(&self, data: &DocumentData) -> String {
        let name = |id| feature_name(data, id);
        match self {
            GroupError::Gaps(g) => format!(
                "選んだ機能が続いていません(間に {} があります)",
                g.iter().map(|&i| name(i)).collect::<Vec<_>>().join("、")
            ),
            GroupError::Order(v) => format!(
                "寄せると順番の決まりが壊れます: {}",
                v.iter()
                    .map(|&(a, b)| format!("{} は {} の後に要る", name(a), name(b)))
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            GroupError::Other(s) => s.clone(),
        }
    }
}

/// 選んだ機能をグループにする時の扱い。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupMode {
    /// 続いた範囲だけまとめる(間があれば [`GroupError::Gaps`])
    Exact,
    /// 最初から最後までの間のものも含める
    IncludeGaps,
    /// 選んだものを最初の機能の後ろへ寄せてからまとめる
    Gather,
}

/// 機能 `id` が使う、前の機能。
/// 機能 `id` が使う機能(タイムラインの順)。右クリックの「スケッチを編集」に使う。
pub fn used_features(data: &DocumentData, registry: &Registry, id: FeatureId) -> Vec<FeatureId> {
    let used = uses(data, registry, id);
    data.features
        .iter()
        .map(|f| f.id)
        .filter(|f| *f != id && used.contains(f))
        .collect()
}

fn uses(data: &DocumentData, registry: &Registry, id: FeatureId) -> HashSet<FeatureId> {
    let Some(f) = data.feature(id) else {
        return HashSet::new();
    };
    let Some(Ok(feature)) = registry.instantiate(&f.type_id, &f.data) else {
        return HashSet::new();
    };
    feature
        .inputs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| match r {
            InputRef::Output { feature, .. } => Some(feature),
            InputRef::Param { .. } | InputRef::Units => None,
        })
        .collect()
}

/// 選んだ機能 `ids` を名前 `name` のグループにする。
pub fn make_group(
    doc: &mut Document,
    registry: &Registry,
    ids: &[FeatureId],
    name: &str,
    mode: GroupMode,
) -> Result<Transaction, GroupError> {
    let data = doc.data().clone();
    let mut idx: Vec<usize> = ids.iter().filter_map(|&id| data.index_of(id)).collect();
    idx.sort_unstable();
    idx.dedup();
    let (Some(&a), Some(&b)) = (idx.first(), idx.last()) else {
        return Err(GroupError::Other("まとめる機能を選んでください".into()));
    };
    let gaps: Vec<FeatureId> = (a..=b)
        .filter(|i| !idx.contains(i))
        .map(|i| data.features[i].id)
        .collect();
    let mut ops = Vec::new();
    let (first, last) = match mode {
        _ if gaps.is_empty() => (data.features[a].id, data.features[b].id),
        GroupMode::Exact => return Err(GroupError::Gaps(gaps)),
        GroupMode::IncludeGaps => (data.features[a].id, data.features[b].id),
        GroupMode::Gather => {
            // 間の機能を、選んだものの後ろへ出す。出す機能が選んだ機能を使っていれば問題ないが、
            // 選んだ機能が間の機能を使っていると、使う側が前に来てしまう
            let picked: Vec<FeatureId> = idx.iter().map(|&i| data.features[i].id).collect();
            let mut broken = Vec::new();
            for &p in &picked {
                let pi = data.index_of(p).unwrap_or(0);
                for g in &gaps {
                    if data.index_of(*g).unwrap_or(0) < pi && uses(&data, registry, p).contains(g) {
                        broken.push((p, *g));
                    }
                }
            }
            if !broken.is_empty() {
                return Err(GroupError::Order(broken));
            }
            // 間の機能を、選んだものの最後の後ろへ 1 つずつ動かす(前のものから。間の順は保つ)
            for g in &gaps {
                ops.push(Op::MoveFeature { id: *g, to: b });
            }
            (picked[0], *picked.last().unwrap_or(&picked[0]))
        }
    };
    let mut groups = data.groups.clone();
    let id = doc.alloc_group_id();
    groups.push(Group {
        id,
        name: name.into(),
        first,
        last,
    });
    // 外側(広い)ものが先になるよう並べ直すのは画面の仕事にせず、ここでは足すだけ
    ops.push(Op::SetGroups { groups });
    Ok(Transaction::new(format!("グループ「{name}」を作る"), ops))
}

fn without_group(data: &DocumentData, gid: u64) -> Result<(Group, Vec<Group>), String> {
    let g = data
        .group(gid)
        .cloned()
        .ok_or_else(|| "グループがありません".to_string())?;
    let rest = data
        .groups
        .iter()
        .filter(|x| x.id != gid)
        .cloned()
        .collect();
    Ok((g, rest))
}

/// グループを解く(中身は残す)。
pub fn ungroup(data: &DocumentData, gid: u64) -> Result<Transaction, String> {
    let (g, rest) = without_group(data, gid)?;
    Ok(Transaction::new(
        format!("グループ「{}」を解く", g.name),
        vec![Op::SetGroups { groups: rest }],
    ))
}

/// グループの中身の機能(古い順)。
pub fn group_members(data: &DocumentData, gid: u64) -> Vec<FeatureId> {
    let Some((a, b)) = data.group(gid).and_then(|g| data.group_range(g)) else {
        return Vec::new();
    };
    data.features[a..=b].iter().map(|f| f.id).collect()
}

/// グループと中身を消す(取り消せる)。
pub fn delete_group(data: &DocumentData, gid: u64) -> Result<Transaction, String> {
    let (g, rest) = without_group(data, gid)?;
    // 中のグループも一緒に消えるので、先にグループを外してから中身を消す
    let members = group_members(data, gid);
    let inner: HashSet<u64> = rest
        .iter()
        .filter(|x| members.contains(&x.first))
        .map(|x| x.id)
        .collect();
    let rest = rest
        .into_iter()
        .filter(|x| !inner.contains(&x.id))
        .collect();
    let mut ops = vec![Op::SetGroups { groups: rest }];
    ops.extend(members.into_iter().map(|id| Op::RemoveFeature { id }));
    Ok(Transaction::new(
        format!("グループ「{}」と中身を消す", g.name),
        ops,
    ))
}

/// グループの名前を変える。
pub fn rename_group(data: &DocumentData, gid: u64, name: &str) -> Result<Transaction, String> {
    let mut groups = data.groups.clone();
    let g = groups
        .iter_mut()
        .find(|g| g.id == gid)
        .ok_or_else(|| "グループがありません".to_string())?;
    g.name = name.into();
    Ok(Transaction::new(
        format!("グループの名前を {name} に変更"),
        vec![Op::SetGroups { groups }],
    ))
}

/// グループの端の機能を、グループから出す(中ほどの機能は、出すと 2 つに分かれるので出せない)。
pub fn take_out_of_group(
    data: &DocumentData,
    gid: u64,
    id: FeatureId,
) -> Result<Transaction, String> {
    let mut groups = data.groups.clone();
    let k = groups
        .iter()
        .position(|g| g.id == gid)
        .ok_or_else(|| "グループがありません".to_string())?;
    let g = &groups[k];
    let (a, b) = data
        .group_range(g)
        .ok_or_else(|| "グループの端の機能がありません".to_string())?;
    let i = index(data, id)?;
    let name = g.name.clone();
    if a == b {
        groups.remove(k);
    } else if i == a {
        groups[k].first = data.features[a + 1].id;
    } else if i == b {
        groups[k].last = data.features[b - 1].id;
    } else {
        return Err("グループの端の機能だけ出せます(中ほどの機能は先に端へ動かします)".into());
    }
    Ok(Transaction::new(
        format!("{}を「{name}」から出す", feature_name(data, id)),
        vec![Op::SetGroups { groups }],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc_with(n: usize) -> (Document, Vec<FeatureId>) {
        let mut doc = Document::new();
        let mut ids = Vec::new();
        for i in 0..n {
            let (tx, id) = insert_feature(&mut doc, "足す", "x@1", &format!("f{i}"), json!({}));
            doc.apply(tx).unwrap();
            ids.push(id);
        }
        (doc, ids)
    }

    #[test]
    fn removing_with_users_also_drops_the_parts_left_empty() {
        // 採用分 10-24 a1: 中身が全部なくなる部品だけを一緒に消す(子の部品が残る部品は残す)
        let (mut doc, ids) = doc_with(3);
        let part = |id: u64, name: &str, parent: Option<u64>, bodies: Vec<FeatureId>| {
            kmb_core::Component {
                id,
                name: name.into(),
                parent,
                bodies,
            }
        };
        let [a, b, c] = [(); 3].map(|_| doc.alloc_group_id());
        doc.apply(set_components(
            "部品",
            vec![
                part(a, "台", None, vec![ids[0]]),
                part(b, "腕", None, vec![ids[1]]),
                part(c, "台の中", Some(a), vec![ids[2]]),
            ],
        ))
        .unwrap();
        let names = |cs: Vec<&kmb_core::Component>| -> Vec<String> {
            cs.into_iter().map(|c| c.name.clone()).collect()
        };
        assert_eq!(
            names(components_emptied(doc.data(), &[ids[1], ids[2]])),
            ["腕", "台の中"]
        );
        assert_eq!(names(components_emptied(doc.data(), &[ids[1]])), ["腕"]);
        // 台は子の「台の中」が残るので、台のボディだけ消えても残す
        assert_eq!(
            names(components_emptied(doc.data(), &[ids[0]])),
            Vec::<String>::new()
        );
        let reg = Registry::new();
        let tx = remove_features_with_users(doc.data(), &reg, &[ids[1], ids[2]]);
        doc.apply(tx).unwrap();
        assert_eq!(doc.data().features.len(), 1);
        let left: Vec<&str> = doc
            .data()
            .components
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(left, ["台"]);
        // 1 回の取り消しで機能も部品も戻る
        doc.undo();
        assert_eq!(doc.data().features.len(), 3);
        assert_eq!(doc.data().components.len(), 3);
    }

    #[test]
    fn grouping_a_gap_explains_and_can_include_or_gather() {
        let reg = Registry::new();
        let (mut doc, ids) = doc_with(4);
        let picked = [ids[0], ids[2]];
        let e = make_group(&mut doc, &reg, &picked, "土台", GroupMode::Exact).unwrap_err();
        assert_eq!(e, GroupError::Gaps(vec![ids[1]]));
        assert!(e.describe(doc.data()).contains("f1"));
        // 間も含める
        let tx = make_group(&mut doc, &reg, &picked, "土台", GroupMode::IncludeGaps).unwrap();
        doc.apply(tx).unwrap();
        assert_eq!(group_members(doc.data(), doc.data().groups[0].id), ids[..3]);
        doc.undo();
        // 寄せる: f1 が後ろへ出て、f0・f2 がまとまる(1 回の取り消し)
        let tx = make_group(&mut doc, &reg, &picked, "土台", GroupMode::Gather).unwrap();
        doc.apply(tx).unwrap();
        let order: Vec<_> = doc.data().features.iter().map(|f| f.id).collect();
        assert_eq!(order, vec![ids[0], ids[2], ids[1], ids[3]]);
        let g = doc.data().groups[0].id;
        assert_eq!(group_members(doc.data(), g), vec![ids[0], ids[2]]);
        doc.undo();
        assert!(doc.data().groups.is_empty());
        assert_eq!(doc.data().features[1].id, ids[1]);
    }

    #[test]
    fn ungroup_keeps_and_delete_removes_the_contents() {
        let reg = Registry::new();
        let (mut doc, ids) = doc_with(3);
        let tx = make_group(&mut doc, &reg, &ids[..2], "穴", GroupMode::Exact).unwrap();
        doc.apply(tx).unwrap();
        let g = doc.data().groups[0].id;
        doc.apply(ungroup(doc.data(), g).unwrap()).unwrap();
        assert_eq!(doc.data().features.len(), 3);
        doc.undo();
        doc.apply(delete_group(doc.data(), g).unwrap()).unwrap();
        assert_eq!(doc.data().features.len(), 1);
        assert!(doc.data().groups.is_empty());
        doc.undo();
        assert_eq!(doc.data().features.len(), 3);
        // 端だけ出せる
        doc.apply(take_out_of_group(doc.data(), g, ids[1]).unwrap())
            .unwrap();
        assert_eq!(group_members(doc.data(), g), vec![ids[0]]);
    }

    #[test]
    fn several_features_move_together_and_keep_their_order() {
        let reg = Registry::new();
        let (mut doc, ids) = doc_with(5);
        // 続いていない f1・f3 を f4 の後ろへ: 順は保って末尾に並ぶ
        let tx = move_features(doc.data(), &reg, &[ids[3], ids[1]], ids[4], true)
            .unwrap()
            .unwrap();
        assert!(tx.label.contains("f1") && tx.label.contains("f3"));
        doc.apply(tx).unwrap();
        let order: Vec<_> = doc.data().features.iter().map(|f| f.id).collect();
        assert_eq!(order, vec![ids[0], ids[2], ids[4], ids[1], ids[3]]);
        // 1 回で戻る
        doc.undo();
        let order: Vec<_> = doc.data().features.iter().map(|f| f.id).collect();
        assert_eq!(order, ids);
        // 先頭の前へ
        let tx = move_features(doc.data(), &reg, &[ids[2], ids[3]], ids[0], false)
            .unwrap()
            .unwrap();
        doc.apply(tx).unwrap();
        let order: Vec<_> = doc.data().features.iter().map(|f| f.id).collect();
        assert_eq!(order, vec![ids[2], ids[3], ids[0], ids[1], ids[4]]);
        // 動かした中の機能へ落としても、今と同じ所でも何もしない
        assert!(
            move_features(doc.data(), &reg, &[ids[2], ids[3]], ids[3], true)
                .unwrap()
                .is_none()
        );
        assert!(
            move_features(doc.data(), &reg, &[ids[2], ids[3]], ids[0], false)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn named_views_are_saved_renamed_and_removed_with_undo() {
        let (mut doc, _) = doc_with(1);
        let view = NamedView {
            id: 0,
            name: String::new(),
            target: [0.0; 3],
            yaw: -60.0,
            pitch: 30.0,
            roll: 0.0,
            height: 100.0,
            perspective: false,
        };
        let (tx, a) = save_named_view(&mut doc, view.clone());
        doc.apply(tx).unwrap();
        assert_eq!(doc.data().named_view(a).unwrap().name, "視点1");
        // 同じ名前は番号を足す
        let (tx, b) = save_named_view(
            &mut doc,
            NamedView {
                name: "視点1".into(),
                ..view.clone()
            },
        );
        doc.apply(tx).unwrap();
        assert_eq!(doc.data().named_view(b).unwrap().name, "視点1 (2)");
        let tx = change_named_view(doc.data(), b, "名前", |v| v.name = "正面".into()).unwrap();
        doc.apply(tx).unwrap();
        assert_eq!(doc.data().named_view(b).unwrap().name, "正面");
        doc.apply(remove_named_view(doc.data(), a).unwrap())
            .unwrap();
        assert_eq!(doc.data().named_views.len(), 1);
        doc.undo();
        assert_eq!(doc.data().named_views.len(), 2);
        assert!(remove_named_view(doc.data(), 999).is_err());
    }

    #[test]
    fn the_rollback_steps_one_feature_at_a_time() {
        let (mut doc, _) = doc_with(3);
        doc.apply(step_rollback(doc.data(), -1).unwrap()).unwrap();
        assert_eq!(doc.data().rollback, Some(2));
        doc.apply(step_rollback(doc.data(), 1).unwrap()).unwrap();
        assert_eq!(doc.data().rollback, None);
        assert!(step_rollback(doc.data(), 1).is_none());
    }
}

/// 部品の木を丸ごと置き換える。
pub fn set_components(label: &str, components: Vec<kmb_core::Component>) -> Transaction {
    Transaction::new(label, vec![Op::SetComponents { components }])
}

/// 部品 `id` の外観を替える(`None` なら外す。外観の無い中のボディをこの色で塗る)。
pub fn set_component_appearance(
    data: &kmb_core::DocumentData,
    label: &str,
    id: u64,
    appearance: Option<&str>,
) -> Result<Transaction, String> {
    let mut all: kmb_look::appearance::Appearances = data.section().map_err(|e| e.to_string())?;
    all.set_component(id, appearance);
    Ok(Transaction::new(label, vec![data.section_op(&all)?]))
}

/// アニメーションの段を丸ごと置き換える。
pub fn set_animation(
    data: &kmb_core::DocumentData,
    label: &str,
    steps: Vec<kmb_interchange::animation::Step>,
) -> Result<Transaction, String> {
    Ok(Transaction::new(
        label,
        vec![kmb_interchange::animation::set_steps_op(data, steps)?],
    ))
}

/// モジュールの区画 `T` を `value` に置き換える(1 回の取り消し単位)。
pub fn set_section<T: kmb_core::SectionType>(
    data: &kmb_core::DocumentData,
    label: &str,
    value: &T,
) -> Result<Transaction, String> {
    Ok(Transaction::new(label, vec![data.section_op(value)?]))
}

/// 1 回の取り消し単位にまとめる、いくつかの変更(貼り付け・複製・並べ替えのように、画面が
/// 選んだ物から順に組み立てる変更)。画面は [`Op`] を直に組まず、これに積んで
/// [`Changes::into_transaction`] で文書へ渡す。
#[derive(Debug, Default)]
pub struct Changes {
    ops: Vec<Op>,
}

impl Changes {
    pub fn new() -> Self {
        Self::default()
    }

    /// 機能を入れる。`index` が `None` ならロールバックの位置(なければ最後)。
    pub fn insert_feature(&mut self, index: Option<usize>, entry: FeatureEntry) {
        self.ops.push(Op::InsertFeature { index, entry });
    }

    /// 機能のデータを置き換える。
    pub fn set_feature_data(&mut self, id: FeatureId, data: serde_json::Value) {
        self.ops.push(Op::SetFeatureData { id, data });
    }

    /// 機能を `to` 番目へ動かす。
    pub fn move_feature(&mut self, id: FeatureId, to: usize) {
        self.ops.push(Op::MoveFeature { id, to });
    }

    /// ボディ(最初に作った機能 `root`)の名前(`None` は外す)。
    pub fn rename_body(&mut self, root: FeatureId, name: Option<String>) {
        self.ops.push(Op::RenameBody { root, name });
    }

    /// モジュールの区画 `T` を `value` に置き換える(同じ区画を何度も変える時は、変え終えた値で 1 回)。
    pub fn set_section<T: kmb_core::SectionType>(
        &mut self,
        data: &kmb_core::DocumentData,
        value: &T,
    ) -> Result<(), String> {
        self.ops.push(data.section_op(value)?);
        Ok(())
    }

    /// 部品の木を丸ごと置き換える。
    pub fn set_components(&mut self, components: Vec<kmb_core::Component>) {
        self.ops.push(Op::SetComponents { components });
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn into_transaction(self, label: impl Into<String>) -> Transaction {
        Transaction::new(label, self.ops)
    }
}
