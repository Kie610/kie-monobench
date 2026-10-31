//! 包みの中だけを解決する 3MF の読み手。実体宣言・外部参照を展開しない。
use super::{Geometry, Object, Unit, check_count, clean};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    children: Vec<Node>,
    text: String,
}
impl Node {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|x| x.name == name)
    }
    fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |x| x.name == name)
    }
}

fn unescape(text: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let end = rest.find(';').ok_or("XML の実体参照が途中で切れています")?;
        let entity = &rest[..end];
        let c = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let (digits, radix) = entity
                    .strip_prefix("#x")
                    .map(|x| (x, 16))
                    .or_else(|| entity.strip_prefix('#').map(|x| (x, 10)))
                    .ok_or("XML に読めない実体参照があります")?;
                char::from_u32(
                    u32::from_str_radix(digits, radix).map_err(|_| "XML の文字番号が不正です")?,
                )
                .filter(|c| *c != '\0')
                .ok_or("XML の文字番号が不正です")?
            }
        };
        out.push(c);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn xml(bytes: &[u8]) -> Result<Node, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "3MF の XML が UTF-8 ではありません")?
        .trim_start_matches('\u{feff}');
    let mut stack = vec![Node::default()];
    let mut rest = text;
    let mut nodes = 0;
    while !rest.is_empty() {
        let i = rest.find('<').unwrap_or(rest.len());
        stack
            .last_mut()
            .expect("root")
            .text
            .push_str(&unescape(&rest[..i])?);
        rest = &rest[i..];
        if rest.is_empty() {
            break;
        }
        if rest.starts_with("<!--") {
            let n = rest.find("-->").ok_or("XML のコメントが閉じていません")?;
            rest = &rest[n + 3..];
            continue;
        }
        if rest.starts_with("<?") {
            let n = rest.find("?>").ok_or("XML の宣言が閉じていません")?;
            rest = &rest[n + 2..];
            continue;
        }
        if rest.starts_with("<!") {
            return Err("3MF の XML の実体宣言や特殊な宣言は読めません".into());
        }
        // 引用符の中の > は、属性値の一部として読む。
        let mut quote = None;
        let mut end = None;
        for (i, c) in rest.char_indices().skip(1) {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else if c == '\'' || c == '"' {
                quote = Some(c);
            } else if c == '>' {
                end = Some(i);
                break;
            }
        }
        let n = end.ok_or("XML の要素が閉じていません")?;
        let tag = &rest[1..n];
        rest = &rest[n + 1..];
        if let Some(close) = tag.strip_prefix('/') {
            if stack.len() == 1 {
                return Err("XML の閉じる要素が余っています".into());
            }
            let node = stack.pop().expect("node");
            if node.name != close.trim().rsplit(':').next().unwrap_or("") {
                return Err("XML の要素の対応が違います".into());
            }
            stack.last_mut().expect("parent").children.push(node);
            continue;
        }
        nodes += 1;
        if nodes > 25_000_000 || stack.len() > 64 {
            return Err("3MF の XML が大きすぎるか、入れ子が深すぎます".into());
        }
        let empty = tag.trim_end().ends_with('/');
        let tag = if empty {
            tag.trim_end().trim_end_matches('/')
        } else {
            tag
        };
        let n = tag.find(char::is_whitespace).unwrap_or(tag.len());
        let mut node = Node {
            name: tag[..n].rsplit(':').next().unwrap_or("").into(),
            ..Node::default()
        };
        if node.name.is_empty() {
            return Err("XML の要素名がありません".into());
        }
        let mut attrs = tag[n..].trim();
        while !attrs.is_empty() {
            let eq = attrs.find('=').ok_or("XML の属性が不正です")?;
            let name = attrs[..eq].trim();
            if name.is_empty() || name.chars().any(char::is_whitespace) {
                return Err("XML の属性名が不正です".into());
            }
            attrs = attrs[eq + 1..].trim_start();
            let q = attrs
                .chars()
                .next()
                .filter(|c| *c == '\'' || *c == '"')
                .ok_or("XML の属性値に引用符がありません")?;
            let n = attrs[1..].find(q).ok_or("XML の属性値が閉じていません")? + 1;
            if node
                .attrs
                .insert(name.into(), unescape(&attrs[1..n])?)
                .is_some()
            {
                return Err("XML の属性が重複しています".into());
            }
            attrs = attrs[n + 1..].trim_start();
        }
        if empty {
            stack.last_mut().expect("parent").children.push(node);
        } else {
            stack.push(node);
        }
    }
    if stack.len() != 1 {
        return Err("3MF の XML が途中で切れています".into());
    }
    let mut root = stack.pop().expect("root");
    if root.children.len() != 1 || !root.text.trim().is_empty() {
        return Err("XML の根の要素が不正です".into());
    }
    Ok(root.children.remove(0))
}

type Matrix = [[f64; 4]; 4];
fn identity() -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1.0 } else { 0.0 }))
}
fn matrix(value: Option<&str>) -> Result<Matrix, String> {
    let Some(value) = value else {
        return Ok(identity());
    };
    let v = value
        .split_whitespace()
        .map(|x| {
            x.parse::<f64>()
                .map_err(|_| "3MF の変換行列が不正です".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if v.len() != 12 || v.iter().any(|x| !x.is_finite()) {
        return Err("3MF の変換行列には有限の数が 12 個必要です".into());
    }
    let mut m = identity();
    for i in 0..3 {
        for j in 0..4 {
            m[i][j] = v[j * 3 + i];
        }
    }
    Ok(m)
}
fn compose(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..4).map(|k| a[i][k] * b[k][j]).sum()))
}
fn unit(model: &Node) -> Result<Unit, String> {
    match model.attr("unit").unwrap_or("millimeter") {
        "micron" => Ok(Unit::Micron),
        "millimeter" => Ok(Unit::Millimeter),
        "centimeter" => Ok(Unit::Centimeter),
        "meter" => Ok(Unit::Meter),
        "inch" => Ok(Unit::Inch),
        "foot" => Ok(Unit::Foot),
        x => Err(format!("3MF の単位 {x} は読めません")),
    }
}

fn part_path(base: &str, target: &str) -> Result<String, String> {
    if target.contains(['\\', '\0', '?', '#']) || target.contains("://") {
        return Err("3MF の参照先は包みの中のファイルだけ使えます".into());
    }
    let path = if target.starts_with('/') {
        target.trim_start_matches('/').to_string()
    } else {
        format!(
            "{}{target}",
            base.rsplit_once('/')
                .map(|(p, _)| format!("{p}/"))
                .unwrap_or_default()
        )
    };
    let mut components = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                components.pop().ok_or("3MF の参照先が包みの外です")?;
            }
            _ => components.push(p),
        }
    }
    Ok(components.join("/"))
}
struct Package {
    models: BTreeMap<String, Node>,
    skipped: BTreeMap<String, usize>,
}
impl Package {
    fn object(&self, path: &str, id: &str) -> Result<&Node, String> {
        self.models
            .get(path)
            .and_then(|n| n.child("resources"))
            .and_then(|n| n.children("object").find(|x| x.attr("id") == Some(id)))
            .ok_or_else(|| format!("3MF の物 {path}#{id} がありません"))
    }
    fn append(
        &self,
        path: &str,
        id: &str,
        place: Matrix,
        g: &mut Geometry,
        seen: &mut BTreeSet<(String, String)>,
        notes: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if seen.len() >= 64 || !seen.insert((path.into(), id.into())) {
            return Err("3MF の部品の参照が循環するか、入れ子が深すぎます".into());
        }
        let node = self.object(path, id)?;
        let kind = node.attr("type").unwrap_or("model");
        if kind != "model" {
            notes.insert(format!("3MF の物の種類 {kind} は読みませんでした"));
        } else if let Some(mesh) = node.child("mesh") {
            let base = g.vertices.len() as u32;
            let verts = mesh
                .child("vertices")
                .ok_or("3MF の vertices がありません")?;
            for v in verts.children("vertex") {
                let mut p = [0.0; 3];
                for (i, key) in ["x", "y", "z"].iter().enumerate() {
                    p[i] = v
                        .attr(key)
                        .ok_or("3MF の座標がありません")?
                        .parse::<f64>()
                        .map_err(|_| "3MF の座標が不正です")?;
                }
                let p = std::array::from_fn(|i| {
                    ((0..3).map(|j| place[i][j] * p[j]).sum::<f64>() + place[i][3]) as f32
                });
                if p.iter().any(|x| !x.is_finite()) {
                    return Err("3MF の変換後の座標が有限の数ではありません".into());
                }
                g.vertices.push(p);
                if g.vertices.len() > super::MAX_TRIANGLES * 3 {
                    return Err("3MF の頂点が多すぎます".into());
                }
            }
            let count = g.vertices.len() - base as usize;
            for t in mesh
                .child("triangles")
                .ok_or("3MF の triangles がありません")?
                .children("triangle")
            {
                check_count(g.triangles.len() + 1)?;
                let mut ids = [0; 3];
                for (i, key) in ["v1", "v2", "v3"].iter().enumerate() {
                    let n = t
                        .attr(key)
                        .ok_or("3MF の頂点番号がありません")?
                        .parse::<usize>()
                        .map_err(|_| "3MF の頂点番号が不正です")?;
                    if n >= count {
                        return Err("3MF の頂点番号が範囲の外です".into());
                    }
                    ids[i] = base + n as u32;
                }
                g.triangles.push(ids);
            }
        } else if let Some(components) = node.child("components") {
            for c in components.children("component") {
                let target = c
                    .attrs
                    .iter()
                    .find(|(k, _)| k.rsplit(':').next() == Some("path"))
                    .map(|(_, v)| part_path(path, v))
                    .transpose()?
                    .unwrap_or_else(|| path.into());
                let child_unit = unit(
                    self.models
                        .get(&target)
                        .ok_or("3MF の参照先の model がありません")?,
                )?
                .mm();
                let parent_unit = unit(self.models.get(path).expect("current model"))?.mm();
                let mut scale = identity();
                for (i, row) in scale.iter_mut().enumerate().take(3) {
                    row[i] = child_unit / parent_unit;
                }
                let m = compose(place, compose(matrix(c.attr("transform"))?, scale));
                self.append(
                    &target,
                    c.attr("objectid").ok_or("3MF の部品番号がありません")?,
                    m,
                    g,
                    seen,
                    notes,
                )?;
            }
        } else {
            notes.insert("3MF の三角形の網でない物は読みませんでした".into());
        }
        seen.remove(&(path.into(), id.into()));
        Ok(())
    }
}

/// build の物ごとに、入れ子の部品の変換を掛けて 1 つの網にまとめる。
pub fn read_3mf(bytes: &[u8]) -> Result<Vec<Object>, String> {
    let files = crate::zip::read(bytes)?;
    let model_path = if let Some(rels) = files.get("_rels/.rels") {
        let root = xml(rels)?;
        let r = root
            .children("Relationship")
            .find(|r| r.attr("Type").is_some_and(|t| t.ends_with("/3dmodel")))
            .ok_or("3MF の 3D model の参照がありません")?;
        if r.attr("TargetMode") == Some("External") {
            return Err("3MF の外部の model は読めません".into());
        }
        part_path(
            "",
            r.attr("Target")
                .ok_or("3MF の model の参照先がありません")?,
        )?
    } else {
        "3D/3dmodel.model".into()
    };
    let mut package = Package {
        models: BTreeMap::new(),
        skipped: BTreeMap::new(),
    };
    for (path, data) in &files {
        if path.ends_with(".model") {
            let n = xml(data)?;
            if n.name != "model" {
                return Err("3MF の model の根が違います".into());
            }
            unit(&n)?;
            if let Some(resources) = n.child("resources") {
                let mut ids = BTreeSet::new();
                for r in &resources.children {
                    if r.name == "object" {
                        let id = r.attr("id").ok_or("3MF の物の番号がありません")?;
                        if !ids.insert(id) {
                            return Err("3MF の物の番号が重複しています".into());
                        }
                        if r.child("mesh").is_some() && r.child("components").is_some() {
                            return Err("3MF の物に mesh と components が両方あります".into());
                        }
                        let kind = r.attr("type").unwrap_or("model");
                        if kind != "model" {
                            *package
                                .skipped
                                .entry(format!("物の種類 {kind}"))
                                .or_default() += 1;
                        }
                    }
                    if r.name != "object" {
                        *package.skipped.entry(r.name.clone()).or_default() += 1;
                    }
                }
            }
            package.models.insert(path.clone(), n);
        }
    }
    let model = package
        .models
        .get(&model_path)
        .ok_or("3MF の model がありません")?;
    let mut out = Vec::new();
    let mut total = 0;
    for item in model
        .child("build")
        .ok_or("3MF の build がありません")?
        .children("item")
    {
        let id = item
            .attr("objectid")
            .ok_or("3MF の build の物の番号がありません")?;
        let object = package.object(&model_path, id)?;
        let name = object
            .attr("name")
            .map(str::to_string)
            .or_else(|| {
                object
                    .children("metadata")
                    .chain(model.children("metadata"))
                    .find(|n| n.attr("name") == Some("Title"))
                    .map(|n| n.text.clone())
            })
            .unwrap_or_default();
        let mut geometry = Geometry::default();
        let mut notes = package
            .skipped
            .iter()
            .map(|(kind, n)| format!("3MF の {kind} {n} 件は読みませんでした"))
            .collect::<BTreeSet<_>>();
        package.append(
            &model_path,
            id,
            matrix(item.attr("transform"))?,
            &mut geometry,
            &mut BTreeSet::new(),
            &mut notes,
        )?;
        total += geometry.triangles.len();
        check_count(total)?;
        if !geometry.triangles.is_empty() {
            out.push(clean(Object {
                name,
                unit: unit(model)?,
                geometry,
                notes: notes.into_iter().collect(),
            })?);
        }
    }
    if out.is_empty() {
        return Err("3MF に読み込める三角形の網がありません".into());
    }
    Ok(out)
}
