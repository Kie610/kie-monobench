//! 外観の操作。
//!
//! ボディ・部品・面に外観を付ける・外す、「このデザイン内」の外観の色・金属・粗さを直す・複製する・
//! 消す、外観の一覧とライブラリの表を出す。画面の外観の窓も CLI・MCP も、この 1 つの操作を
//! 引数を替えて流す。文書の外観の決まりは [`kmb_look::appearance`]。

use kmb_core::{
    Action, Arg, Context, FeatureId, FieldUnit, Outcome, Ref, RefKind, Table, TableColumn,
    body_root,
};
use kmb_look::appearance::{self as looks, AppearanceDef, Appearances};
use kmb_look::material::{self, CATEGORIES, LIBRARY};
use serde_json::Value;
use std::collections::BTreeMap;

pub const APPEARANCE: &str = "render.appearance";

/// 外観の操作の宣言。
pub fn action() -> Action {
    let categories: Vec<String> = CATEGORIES.iter().map(|c| c.to_string()).collect();
    Action::new(APPEARANCE, "外観", "cad")
        .menu("修正")
        .icon("swatch")
        .role(kmb_interchange::role::action::APPEARANCE)
        .aliases("がいかん 外観 appearance いろ 色 ぬる 塗る 面の色 しつかん 質感 らいぶらり ライブラリ")
        .hint("ボディ・部品・面に外観(色・金属・粗さ)を付ける窓を開く。ライブラリを分類と名前で探し、このデザイン内の外観の色と粗さを直す")
        .args([
            Arg::text("apply", "付ける外観の名前(このデザイン内の外観か、ライブラリの外観)"),
            Arg::flag("remove", "選んだ物の外観を外す(面は面の外観だけを外し、ボディの外観に戻す)"),
            Arg::choice(
                "target",
                "適用先(既定はボディ/コンポーネント)",
                &[("body", "ボディ/コンポーネント"), ("face", "面")],
            ),
            Arg::list("bodies", "ボディの機能の番号の並び(省くと選んでいる面のボディと選んでいるボディ)"),
            Arg::list("components", "部品の番号の並び"),
            Arg::pick("faces", "面(省くと選んでいる面)", &[RefKind::Face]),
            Arg::text("edit", "直す外観の名前(ライブラリの外観なら、このデザイン内に写して直す)"),
            Arg::text("rename", "直す外観の新しい名前(使っている所も新しい名前にする)"),
            Arg::text("color", "直す外観の色(#c86440 か 200,100,64)"),
            Arg::number("metallic", "直す外観の金属の度合い(0〜1。1 は金属)", FieldUnit::Ratio),
            Arg::number("roughness", "直す外観の粗さ(0〜1。0 は鏡、1 はつやなし)", FieldUnit::Ratio),
            Arg::text("duplicate", "複製する外観の名前(「名前 (2)」の名前でこのデザイン内に足す)"),
            Arg::text("delete", "割り当てを外して消す外観の名前(使っているボディ・部品・面から外す)"),
            Arg::flag("list", "このデザイン内の外観の表を出す"),
            Arg::flag("library", "ライブラリの表を出す(`query`・`category` で絞る)"),
            Arg::text("query", "ライブラリを探す言葉(名前と分類の一部)"),
            Arg::text("category", &format!("ライブラリの分類({})", categories.join("・"))),
        ])
}

/// 外観の操作を流す。
pub fn run(cx: &Context<'_>) -> Result<Outcome, String> {
    let text = |k: &str| {
        cx.arg_str(k)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let flag = |k: &str| cx.arg(k).and_then(Value::as_bool).unwrap_or(false);
    // 表は先に読む(読まない鍵は宣言の点検で誤りにならないが、渡した物は全部見る)
    let list = flag("list");
    let library = flag("library");
    let query = text("query");
    let category = text("category");
    if list {
        return Ok(Outcome::Table {
            title: "このデザイン内の外観".into(),
            table: design_table(cx),
        });
    }
    if library {
        return Ok(Outcome::Table {
            title: "外観のライブラリ".into(),
            table: library_table(query.as_deref(), category.as_deref())?,
        });
    }
    if let Some(name) = text("delete") {
        return delete(cx, &name);
    }
    if let Some(name) = text("duplicate") {
        return duplicate(cx, &name);
    }
    if let Some(name) = text("edit") {
        return edit(cx, &name);
    }
    let remove = flag("remove");
    match text("apply") {
        Some(name) if !remove => apply(cx, Some(&name)),
        None if remove => apply(cx, None),
        Some(_) => Err("apply と remove は一緒に渡せません".into()),
        None => Err(
            "何をするかを渡します(apply・remove・edit・duplicate・delete・list・library)".into(),
        ),
    }
}

/// 外観 `name` を付ける(`None` なら外す)。
fn apply(cx: &Context<'_>, name: Option<&str>) -> Result<Outcome, String> {
    let name = match name {
        Some(n) => Some(
            looks::of(cx.data)
                .resolve(n)
                .ok_or_else(|| format!("外観「{n}」はありません"))?
                .name,
        ),
        None => None,
    };
    let what = name.as_deref().unwrap_or("");
    let face = match cx.arg_str("target").unwrap_or("body") {
        "face" => true,
        "body" => false,
        t => return Err(format!("適用先 {t} は使えません(body か face)")),
    };
    if face {
        let faces = picked_faces(cx)?;
        if faces.is_empty() {
            return Err(if name.is_some() {
                "外観を付ける面を選んでください".into()
            } else {
                "外観を外す面を選んでください".into()
            });
        }
        let before = looks::of(cx.data);
        let mut all = before.clone();
        for (root, f) in &faces {
            match &name {
                Some(n) => {
                    all.faces
                        .entry(*root)
                        .or_default()
                        .insert(f.clone(), n.clone());
                }
                None => {
                    if let Some(m) = all.faces.get_mut(root) {
                        m.remove(f);
                        if m.is_empty() {
                            all.faces.remove(root);
                        }
                    }
                }
            }
        }
        if all == before {
            return Err("外観の付いた面がありません".into());
        }
        log::info!("面 {} 個の外観を「{what}」に", faces.len());
        return Ok(Outcome::Change {
            label: match &name {
                Some(n) => format!("面 {} 個の外観を {n} に", faces.len()),
                None => format!("面 {} 個の外観を外す", faces.len()),
            },
            edits: vec![cx.data.set_section(&all)?],
        });
    }
    let roots = picked_roots(cx)?;
    let comps: Vec<u64> = cx
        .arg("components")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();
    if roots.is_empty() && comps.is_empty() {
        return Err("外観を付けるボディか部品を選んでください".into());
    }
    let mut all = looks::of(cx.data);
    for r in &roots {
        all.set_body(*r, name.as_deref());
    }
    for id in &comps {
        if cx.data.component(*id).is_none() {
            return Err(format!("部品 {id} はありません"));
        }
        all.set_component(*id, name.as_deref());
    }
    let edits = vec![cx.data.set_section(&all)?];
    log::info!(
        "ボディ {} 個・部品 {} 個の外観を「{what}」に",
        roots.len(),
        comps.len()
    );
    Ok(Outcome::Change {
        label: match &name {
            Some(n) => format!("外観を {n} に"),
            None => "外観を外す".into(),
        },
        edits,
    })
}

/// 引数 `bodies`、無ければ選んでいる面のボディと選んでいるボディ(最初に作った機能。重ねない)。
fn picked_roots(cx: &Context<'_>) -> Result<Vec<FeatureId>, String> {
    let shown: Vec<FeatureId> = cx.visible_bodies().iter().map(|b| b.0).collect();
    let roots_shown: Vec<FeatureId> = shown
        .iter()
        .map(|s| body_root(cx.data, cx.registry, *s))
        .collect();
    let ids: Vec<FeatureId> = match cx.arg("bodies").and_then(Value::as_array) {
        Some(l) => l.iter().filter_map(|v| v.as_u64().map(FeatureId)).collect(),
        None => cx
            .picked_faces()
            .into_iter()
            .map(|p| p.feature)
            .chain(cx.selected)
            .chain(cx.also_selected.iter().copied())
            .filter(|id| shown.contains(id) || roots_shown.contains(id))
            .collect(),
    };
    let mut out: Vec<FeatureId> = Vec::new();
    for id in ids {
        let r = body_root(cx.data, cx.registry, id);
        if !roots_shown.contains(&r) {
            return Err(format!("{} はボディではありません", id.0));
        }
        if !out.contains(&r) {
            out.push(r);
        }
    }
    Ok(out)
}

/// 引数 `faces`、無ければ選んでいる面の(ボディ、面の名前)。今のボディに無い面は誤り。
fn picked_faces(cx: &Context<'_>) -> Result<Vec<(FeatureId, String)>, String> {
    let refs: Vec<Ref> = match cx.arg("faces") {
        Some(v @ Value::Array(_)) => {
            serde_json::from_value(v.clone()).map_err(|e| e.to_string())?
        }
        Some(v @ Value::Object(_)) => {
            vec![serde_json::from_value(v.clone()).map_err(|e| e.to_string())?]
        }
        _ => cx
            .picked_faces()
            .into_iter()
            .map(|p| Ref::Face {
                feature: p.feature,
                face: p.face.clone(),
            })
            .collect(),
    };
    let bodies = cx.visible_bodies();
    let mut out: Vec<(FeatureId, String)> = Vec::new();
    for r in refs {
        let Ref::Face { feature, face } = r else {
            return Err("面でない物は外観の面に選べません".into());
        };
        let root = body_root(cx.data, cx.registry, feature);
        let names = bodies
            .iter()
            .find(|(b, _)| body_root(cx.data, cx.registry, *b) == root)
            .and_then(|(_, h)| cx.kernel.faces(*h).ok())
            .ok_or_else(|| format!("{} はボディではありません", feature.0))?;
        if !names.iter().any(|f| f.name == face) {
            return Err(format!("面 {face} はボディにありません"));
        }
        if !out.iter().any(|(r, f)| *r == root && *f == face) {
            out.push((root, face));
        }
    }
    Ok(out)
}

/// 色の文字(`#c86440` か `200,100,64`)。
fn parse_color(s: &str) -> Result<[u8; 3], String> {
    let s = s.trim();
    let bad = || format!("色 {s} は #c86440 か 200,100,64 の形で書きます");
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 {
            return Err(bad());
        }
        let v = u32::from_str_radix(hex, 16).map_err(|_| bad())?;
        return Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8]);
    }
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(bad());
    }
    let mut c = [0u8; 3];
    for (k, p) in parts.iter().enumerate() {
        c[k] = p.parse().map_err(|_| bad())?;
    }
    Ok(c)
}

/// 色を `#rrggbb` の文字に。
fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// このデザイン内の外観 `name` を直す(ライブラリの外観なら写して直す)。
fn edit(cx: &Context<'_>, name: &str) -> Result<Outcome, String> {
    let before = looks::of(cx.data);
    let r = before
        .resolve(name)
        .ok_or_else(|| format!("外観「{name}」はありません"))?;
    let mut all = before.clone();
    let mut def = match all.def(&r.name) {
        Some(d) => d.clone(),
        None => AppearanceDef {
            name: r.name.clone(),
            category: r.category.clone(),
            color: r.color,
            metallic: r.finish.metallic,
            roughness: r.finish.roughness,
            base: None,
        },
    };
    if let Some(c) = cx.arg_str("color") {
        def.color = parse_color(c)?;
    }
    let unit = |k: &str| -> Result<Option<f32>, String> {
        match cx.arg_f64(k)? {
            Some(v) if (0.0..=1.0).contains(&v) => Ok(Some(v as f32)),
            Some(v) => Err(format!("{k} は 0〜1 です({v})")),
            None => Ok(None),
        }
    };
    if let Some(v) = unit("metallic")? {
        def.metallic = v;
    }
    if let Some(v) = unit("roughness")? {
        def.roughness = v;
    }
    let old = def.name.clone();
    if let Some(new) = cx
        .arg_str("rename")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        && new != old
    {
        if all.def(new).is_some() || material::library_look(new).is_some() {
            return Err(format!("外観「{new}」はもうあります(別の名前にします)"));
        }
        // 名前を替えたライブラリの写しは、木目を元のライブラリの外観から受け継ぐ
        if def.base.is_none() && material::library_look(&old).is_some() {
            def.base = Some(old.clone());
        }
        def.name = new.to_string();
        rename_users(&mut all, &old, new);
    }
    match all.defs.iter_mut().find(|d| d.name == old) {
        Some(d) => *d = def.clone(),
        None => all.defs.push(def.clone()),
    }
    if all == before {
        return Err(format!("外観「{old}」は変わりません"));
    }
    log::info!(
        "外観「{old}」を直す: 名前 {} 色 {} 金属 {} 粗さ {}",
        def.name,
        hex(def.color),
        def.metallic,
        def.roughness
    );
    Ok(Outcome::Change {
        label: format!("外観 {} を直す", def.name),
        edits: vec![cx.data.set_section(&all)?],
    })
}

/// 外観 `old` を使っているボディ・部品・面を `new` に替える。
fn rename_users(all: &mut Appearances, old: &str, new: &str) {
    let uses = all
        .faces
        .values_mut()
        .flat_map(|m| m.values_mut())
        .chain(all.bodies.values_mut())
        .chain(all.components.values_mut());
    for a in uses {
        if a == old {
            *a = new.to_string();
        }
    }
}

/// 外観 `name` を「名前 (2)」の名前で複製する。
fn duplicate(cx: &Context<'_>, name: &str) -> Result<Outcome, String> {
    let mut all = looks::of(cx.data);
    let r = all
        .resolve(name)
        .ok_or_else(|| format!("外観「{name}」はありません"))?;
    let base = all
        .def(&r.name)
        .and_then(|d| d.base.clone())
        .or_else(|| material::library_look(&r.name).map(|l| l.name.to_string()));
    let new = all.free_name(&r.name);
    all.defs.push(AppearanceDef {
        name: new.clone(),
        category: r.category,
        color: r.color,
        metallic: r.finish.metallic,
        roughness: r.finish.roughness,
        base,
    });
    log::info!("外観「{}」を「{new}」に複製", r.name);
    Ok(Outcome::Change {
        label: format!("外観 {} を複製", r.name),
        edits: vec![cx.data.set_section(&all)?],
    })
}

/// 外観 `name` の割り当てを全部外し、このデザイン内から消す。
fn delete(cx: &Context<'_>, name: &str) -> Result<Outcome, String> {
    let mut all = looks::of(cx.data);
    let (bodies, comps, faces) = all.users(cx.data, name);
    let had = all.def(name).is_some();
    all.defs.retain(|d| d.name != name);
    for m in all.faces.values_mut() {
        m.retain(|_, a| a != name);
    }
    all.faces.retain(|_, m| !m.is_empty());
    all.bodies.retain(|_, a| a != name);
    all.components.retain(|_, a| a != name);
    if !had && bodies.is_empty() && comps.is_empty() && faces.is_empty() {
        return Err(format!("外観「{name}」はこのデザインにありません"));
    }
    let edits = vec![cx.data.set_section(&all)?];
    log::info!(
        "外観「{name}」を消す(ボディ {}・部品 {}・面 {} から外す)",
        bodies.len(),
        comps.len(),
        faces.len()
    );
    Ok(Outcome::Change {
        label: format!("外観 {name} を消す"),
        edits,
    })
}

/// 外観の面のうち、今のボディに無い面の数(外観の名前ごと)。ボディが無くなった面も数える。
pub fn missing_faces(cx: &Context<'_>) -> BTreeMap<String, usize> {
    let mut names: BTreeMap<FeatureId, Vec<String>> = BTreeMap::new();
    for (b, h) in cx.visible_bodies() {
        let root = body_root(cx.data, cx.registry, b);
        if let Ok(fs) = cx.kernel.faces(h) {
            names
                .entry(root)
                .or_default()
                .extend(fs.into_iter().map(|f| f.name));
        }
    }
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for (root, m) in &looks::of(cx.data).faces {
        for (face, a) in m {
            if !names.get(root).is_some_and(|n| n.contains(face)) {
                *out.entry(a.clone()).or_default() += 1;
            }
        }
    }
    out
}

fn design_table(cx: &Context<'_>) -> Table {
    let missing = missing_faces(cx);
    let col = |id: &str, label: &str, numeric: bool| TableColumn::new(id, label, numeric, true);
    let all = looks::of(cx.data);
    let rows = all
        .design_names(cx.data)
        .into_iter()
        .filter_map(|n| {
            let r = all.resolve(&n)?;
            let (b, c, f) = all.users(cx.data, &n);
            Some(vec![
                r.name.clone(),
                r.category,
                hex(r.color),
                format!("{:.2}", r.finish.metallic),
                format!("{:.2}", r.finish.roughness),
                b.len().to_string(),
                c.len().to_string(),
                f.len().to_string(),
                missing.get(&n).copied().unwrap_or(0).to_string(),
                if r.custom { "直した" } else { "" }.into(),
            ])
        })
        .collect();
    Table {
        key: APPEARANCE.into(),
        columns: vec![
            col("name", "名前", false),
            col("category", "分類", false),
            col("color", "色", false),
            col("metallic", "金属", true),
            col("roughness", "粗さ", true),
            col("bodies", "ボディ", true),
            col("components", "部品", true),
            col("faces", "面", true),
            col("missing", "見つからない面", true),
            col("custom", "直した", false),
        ],
        rows,
        empty: "このデザインで使っている外観はありません".into(),
        ..Default::default()
    }
}

/// ライブラリの外観のうち、言葉 `query` と分類 `category` に合う物(並びはライブラリの順)。
pub fn search_library(
    query: Option<&str>,
    category: Option<&str>,
) -> Vec<&'static material::LibraryLook> {
    let q = query
        .map(|q| q.trim().to_lowercase())
        .filter(|q| !q.is_empty());
    LIBRARY
        .iter()
        .filter(|l| category.is_none_or(|c| l.category == c))
        .filter(|l| {
            q.as_ref().is_none_or(|q| {
                l.name.to_lowercase().contains(q.as_str())
                    || l.category.contains(q.as_str())
                    || material::same_material(l.name, q)
            })
        })
        .collect()
}

fn library_table(query: Option<&str>, category: Option<&str>) -> Result<Table, String> {
    if let Some(c) = category
        && !CATEGORIES.contains(&c)
    {
        return Err(format!("分類 {c} はありません({})", CATEGORIES.join("・")));
    }
    let col = |id: &str, label: &str, numeric: bool| TableColumn::new(id, label, numeric, true);
    Ok(Table {
        key: format!("{APPEARANCE}.library"),
        columns: vec![
            col("name", "名前", false),
            col("category", "分類", false),
            col("color", "色", false),
            col("metallic", "金属", true),
            col("roughness", "粗さ", true),
        ],
        rows: search_library(query, category)
            .into_iter()
            .map(|l| {
                vec![
                    l.name.to_string(),
                    l.category.to_string(),
                    hex(l.color),
                    format!("{:.2}", l.finish.metallic),
                    format!("{:.2}", l.finish.roughness),
                ]
            })
            .collect(),
        empty: "合う外観はありません".into(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_read_in_two_forms() {
        assert_eq!(parse_color("#c86440"), Ok([200, 100, 64]));
        assert_eq!(parse_color(" 200, 100,64 "), Ok([200, 100, 64]));
        assert!(parse_color("赤").is_err());
        assert!(parse_color("#c8644").is_err());
        assert!(parse_color("1,2,300").is_err());
        assert_eq!(hex([200, 100, 64]), "#c86440");
    }

    #[test]
    fn the_library_is_searched_by_name_alias_and_category() {
        let names = |q: Option<&str>, c: Option<&str>| -> Vec<&str> {
            search_library(q, c).into_iter().map(|l| l.name).collect()
        };
        assert!(names(Some("アルミ"), None).contains(&"アルミ - 艶消し"));
        // 別名(英語の名前)でも探せる
        assert_eq!(names(Some("aluminum"), None), vec!["アルミ"]);
        assert!(names(None, Some("木")).contains(&"ウォルナット"));
        assert!(names(None, Some("木")).iter().all(|n| *n != "赤"));
        assert!(names(Some("金属"), None).contains(&"銅"));
        assert!(names(Some("ないもの"), None).is_empty());
        assert!(library_table(None, Some("ガラス")).is_err());
    }
}
