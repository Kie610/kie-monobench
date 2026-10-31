//! 元の網を文書に埋め、単位と折れ目の角度から表示・加工の網を作る。
use kmb_cad::{Exprs, angle, expr_refs, kernel_err, parse, rename, solid};
use kmb_core::kernel::{self as k, Mesh};
use kmb_core::{
    Action, Arg, Context, Edit, EvalCtx, EvalError, Extension, Feature, Field, Input, InputRef,
    Outcome, Outputs, Place, Problem, ProblemKind, Registry, Value,
};
use kmb_format::mesh::{self as format, Geometry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const IMPORT_MESH: &str = "solid.import_mesh@1";
pub const IMPORT_MESH_FILE: &str = "cad.import_mesh";
const UNITS: &[(&str, &str)] = &[
    ("um", "µm"),
    ("mm", "mm"),
    ("cm", "cm"),
    ("m", "m"),
    ("in", "in"),
    ("ft", "ft"),
];
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshFormat {
    Stl,
    #[serde(rename = "3mf")]
    ThreeMf,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MeshUnit {
    #[serde(rename = "um")]
    Micron,
    #[default]
    #[serde(rename = "mm")]
    Millimeter,
    #[serde(rename = "cm")]
    Centimeter,
    #[serde(rename = "m")]
    Meter,
    #[serde(rename = "in")]
    Inch,
    #[serde(rename = "ft")]
    Foot,
}
impl MeshUnit {
    fn format(self) -> format::Unit {
        match self {
            Self::Micron => format::Unit::Micron,
            Self::Millimeter => format::Unit::Millimeter,
            Self::Centimeter => format::Unit::Centimeter,
            Self::Meter => format::Unit::Meter,
            Self::Inch => format::Unit::Inch,
            Self::Foot => format::Unit::Foot,
        }
    }
    fn from_format(u: format::Unit) -> Self {
        match u {
            format::Unit::Micron => Self::Micron,
            format::Unit::Millimeter => Self::Millimeter,
            format::Unit::Centimeter => Self::Centimeter,
            format::Unit::Meter => Self::Meter,
            format::Unit::Inch => Self::Inch,
            format::Unit::Foot => Self::Foot,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportMesh {
    pub file: String,
    pub format: MeshFormat,
    pub object: String,
    pub unit: MeshUnit,
    pub mesh: String,
    pub triangles: u32,
    pub crease_angle: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}
impl Exprs for ImportMesh {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.crease_angle]
    }
}
impl Feature for ImportMesh {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        expr_refs(self)
    }
    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let g = format::decode(&self.mesh).map_err(EvalError::InvalidData)?;
        if g.triangles.len() != self.triangles as usize {
            return Err(EvalError::InvalidData(
                "網の三角形数が保存した情報と合いません".into(),
            ));
        }
        let crease = angle(ctx, &self.crease_angle, "折れ目の角度")?;
        if !crease.is_finite() || !(0.0..=std::f64::consts::PI).contains(&crease) {
            return Err(EvalError::InvalidData(
                "折れ目の角度は 0〜180 deg にしてください".into(),
            ));
        }
        let mesh =
            display_mesh(&g, self.unit.format().mm(), crease).map_err(EvalError::InvalidData)?;
        let shape = ctx
            .kernel()
            .import_mesh(&mesh, &format!("f{}", ctx.feature_id().0))
            .map_err(kernel_err)?;
        let mut out: Outputs = [("body".into(), Value::Shape(shape))].into();
        if !self.notes.is_empty() {
            kmb_core::add_warnings(&mut out, self.notes.clone());
        }
        Ok(out)
    }
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}
/// 辺を共有する三角形だけをつなぎ、折れ目をまたがない頂点の法線を面積で平均する。
pub fn display_mesh(g: &Geometry, mm: f64, crease: f64) -> Result<Mesh, String> {
    format::properties(g)?;
    let points = g
        .vertices
        .iter()
        .map(|p| p.map(|x| (f64::from(x) * mm) as f32))
        .collect::<Vec<_>>();
    if points.iter().flatten().any(|x| !x.is_finite()) {
        return Err("単位を直した網の座標が大きすぎます".into());
    }
    let normals = g
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| points[i as usize].map(f64::from));
            k::cross(k::sub(b, a), k::sub(c, a))
        })
        .collect::<Vec<_>>();
    let mut edges = BTreeMap::<(u32, u32), Vec<(usize, usize)>>::new();
    for (i, t) in g.triangles.iter().enumerate() {
        for j in 0..3 {
            let (a, b) = (t[j], t[(j + 1) % 3]);
            edges.entry((a.min(b), a.max(b))).or_default().push((i, j));
        }
    }
    let mut parents = (0..g.triangles.len() * 3).collect::<Vec<_>>();
    let cos = crease.cos();
    for sides in edges.values() {
        if let [(a, ae), (b, be)] = sides.as_slice() {
            if k::dot(k::normalize(normals[*a]), k::normalize(normals[*b])) + 1e-12 >= cos {
                for ac in [*ae, (*ae + 1) % 3] {
                    for bc in [*be, (*be + 1) % 3] {
                        if g.triangles[*a][ac] == g.triangles[*b][bc] {
                            let x = root(&mut parents, a * 3 + ac);
                            let y = root(&mut parents, b * 3 + bc);
                            parents[y] = x;
                        }
                    }
                }
            }
        }
    }
    let mut sums = BTreeMap::<usize, [f64; 3]>::new();
    for (i, n) in normals.iter().enumerate() {
        for j in 0..3 {
            let r = root(&mut parents, i * 3 + j);
            let s = sums.entry(r).or_default();
            *s = k::add(*s, *n);
        }
    }
    let mut out = Mesh::default();
    let mut ids = BTreeMap::new();
    for (i, t) in g.triangles.iter().enumerate() {
        let mut triangle = [0; 3];
        for j in 0..3 {
            let r = root(&mut parents, i * 3 + j);
            triangle[j] = *ids.entry(r).or_insert_with(|| {
                let id = out.positions.len() as u32;
                out.positions.push(points[t[j] as usize]);
                out.normals.push(k::normalize(sums[&r]).map(|x| x as f32));
                id
            });
        }
        out.triangles.push(triangle);
        out.triangle_faces.push(0);
    }
    out.face_names.push("mesh".into());
    Ok(out)
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(IMPORT_MESH, "網の読み込み")
            .icon("mesh-import")
            .hint("STL・3MF の三角形の網を読み込み、表示・計測・加工に使う")
            .field(Field::info("file", "ファイル"))
            .field(Field::info("object", "物の名前"))
            .field(Field::info("triangles", "三角形の数"))
            .field(Field::choice("unit", "単位", UNITS))
            .field(
                Field::expr("crease_angle", "折れ目の角度")
                    .angle()
                    .help("隣り合う三角形の角度がこの値以下なら、なめらかに見せる"),
            )
            .field(Field::info("notes", "注記"))
            .no_raw_editor(),
        |d| Ok(Box::new(parse::<ImportMesh>(d)?)),
    );
}
pub struct ImportMeshCommands;
impl Extension for ImportMeshCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        vec![
            Action::new(IMPORT_MESH_FILE, "STL・3MF を読み込む…", "cad")
                .role(kmb_interchange::role::action::IMPORT_MESH)
                .menu("挿入")
                .icon("mesh-import")
                .at(&[Place::Toolbar])
                .aliases("網 よみこみ import mesh stl 3mf")
                .hint("STL・3MF の三角形の網を読み込み、表示・計測・加工に使う")
                .args(vec![
                    Arg::file("path", "読み込む STL・3MF のファイル"),
                    Arg::choice("unit", "STL の頂点の単位(3MF はファイルの単位)", UNITS)
                        .default("mm"),
                ])
                .input(Input::OpenFile {
                    title: "STL・3MF を読み込む".into(),
                    filter: "STL・3MF".into(),
                    extensions: ["stl", "STL", "3mf", "3MF"].map(String::from).to_vec(),
                }),
        ]
    }
    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        if id == IMPORT_MESH_FILE {
            Some(import(cx))
        } else {
            None
        }
    }
}
fn input(s: impl Into<String>) -> Problem {
    Problem::new(ProblemKind::Input, s)
}
fn import(cx: &Context<'_>) -> Result<Outcome, Problem> {
    let arg_path = cx
        .arg("path")
        .and_then(|x| x.as_str())
        .map(std::path::Path::new);
    let path = arg_path
        .or(cx.file)
        .ok_or_else(|| input("STL・3MF のファイルを選んでください"))?;
    let path = cx.open_path(path)?;
    let bytes = std::fs::read(&path).map_err(|e| {
        Problem::new(
            ProblemKind::File,
            format!("{} を読めません: {e}", path.display()),
        )
    })?;
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let format = if ext == "3mf" {
        MeshFormat::ThreeMf
    } else if ext == "stl" {
        MeshFormat::Stl
    } else {
        return Err(input("STL または 3MF のファイルを選んでください"));
    };
    let unit: MeshUnit = serde_json::from_value(
        cx.arg("unit")
            .cloned()
            .unwrap_or_else(|| serde_json::json!("mm")),
    )
    .map_err(|_| input("STL の単位が不正です"))?;
    let objects = if format == MeshFormat::ThreeMf {
        format::read_3mf(&bytes)
    } else {
        format::read_stl(&bytes, unit.format())
    }
    .map_err(input)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "読み込んだ部品".into());
    let file = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let roots = cx
        .visible_bodies()
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    let mut used = kmb_core::body_labels(cx.data, cx.registry, &roots);
    let mut edits = Vec::new();
    let mut bodies = Vec::new();
    for (i, o) in objects.into_iter().enumerate() {
        let label = if o.name.is_empty() {
            stem.clone()
        } else {
            o.name.clone()
        };
        let mut name = label.clone();
        let mut k = 2;
        while used.contains(&name) {
            name = format!("{label}({k})");
            k += 1;
        }
        used.push(name.clone());
        let data = ImportMesh {
            file: file.clone(),
            format,
            object: o.name,
            unit: MeshUnit::from_format(o.unit),
            mesh: format::encode(&o.geometry).map_err(input)?,
            triangles: o.geometry.triangles.len() as u32,
            crease_angle: "30 deg".into(),
            notes: o.notes,
        };
        edits.push(Edit::Insert {
            type_id: IMPORT_MESH.into(),
            name: name.clone(),
            data: serde_json::to_value(data).expect("serializable mesh"),
        });
        bodies.push((i, name));
    }
    if bodies.len() == 1 {
        edits.push(Edit::NameBodies { bodies });
    } else {
        edits.push(Edit::AddBodies {
            component: None,
            name: stem,
            parent: None,
            bodies,
        });
    }
    Ok(Outcome::Change {
        label: "STL・3MF を読み込む".into(),
        edits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crease_splits_normals_without_changing_triangles_or_units() {
        let g = Geometry {
            vertices: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            triangles: vec![[0, 1, 2], [0, 3, 1]],
        };
        let sharp = display_mesh(&g, 25.4, 30f64.to_radians()).unwrap();
        let smooth = display_mesh(&g, 25.4, 120f64.to_radians()).unwrap();
        assert_eq!(sharp.positions.len(), 6);
        assert_eq!(smooth.positions.len(), 4);
        assert_eq!(sharp.triangles.len(), 2);
        assert_eq!(smooth.triangles.len(), 2);
        assert!(sharp.positions.contains(&[25.4, 0., 0.]));
        assert_eq!(
            format::properties(&Geometry {
                vertices: sharp.positions,
                triangles: sharp.triangles
            })
            .unwrap()
            .2,
            format::properties(&Geometry {
                vertices: smooth.positions,
                triangles: smooth.triangles
            })
            .unwrap()
            .2
        );
    }
}
