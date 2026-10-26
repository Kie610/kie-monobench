//! 書き出しの操作: 最後に残ったボディを STL か STEP か 3MF にする(1 つのファイルか、ボディごとのファイル)。
//! 画面は保存先を選ばせ、CLI・MCP は指定の場所に書く。引数 `exclude`(機能の ID の配列)のボディは
//! 書かない(画面で目を閉じて隠したボディ。[`Context::export_bodies`])。今の視点の線の DXF・SVG は
//! kmb-io-view。

use kmb_core::{Action, Arg, Context, Extension, FeatureId, Input, Outcome, Place, ShapeHandle};
use serde_json::json;

/// STL を書き出す(引数 `tolerance` は三角形の許容差 mm、`ascii` が真なら ASCII 形式)。
pub const EXPORT_STL: &str = "io.export_stl";
/// STEP を書き出す。
pub const EXPORT_STEP: &str = "io.export_step";
/// STL を、ボディごとのファイルに分けて書き出す。
/// 選んだ名前に `-<ボディの名前>` を付けて並べる。STL・STEP の操作に引数 `each` を真で渡しても同じ
pub const EXPORT_STL_EACH: &str = "io.export_stl_each";
/// STEP を、ボディごとのファイルに分けて書き出す。
pub const EXPORT_STEP_EACH: &str = "io.export_step_each";
/// 3MF を書き出す。ボディごとに 1 つの物にし、外観(無ければ
/// 物理材料)の色と、文書の長さの単位と、設計の属性を一緒に書く。引数 `unit`(`mm`・`cm`・`m`・`in`)で
/// 単位を替えられ、`tolerance` は三角形の許容差 mm、`each` が真ならボディごとのファイル。
pub const EXPORT_3MF: &str = "io.export_3mf";
/// 書き出しの操作。
pub struct ExportCommands;

/// 書き出すボディと、その名前(ブラウザと同じ。同じ名前が重なれば後ろに (2) などを付ける)。
fn named_bodies(cx: &Context<'_>) -> Result<Vec<(String, ShapeHandle)>, String> {
    Ok(named_body_ids(cx)?
        .into_iter()
        .map(|(name, _, h)| (name, h))
        .collect())
}

/// [`named_bodies`] に、ボディの機能の ID も添えて。
fn named_body_ids(cx: &Context<'_>) -> Result<Vec<(String, FeatureId, ShapeHandle)>, String> {
    let hs = cx.export_bodies()?;
    // 番号は隠したボディも数える(ブラウザの「ボディN」と同じにする)
    let all = cx.visible_bodies();
    let ids: Vec<kmb_core::FeatureId> = all.iter().map(|(id, _)| *id).collect();
    let labels = kmb_core::body_labels(cx.data, cx.registry, &ids);
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (label, (id, h)) in labels
        .into_iter()
        .zip(all)
        .filter(|(_, (_, h))| hs.contains(h))
    {
        let mut name = label.clone();
        let mut k = 2;
        while seen.contains(&name) {
            name = format!("{label}({k})");
            k += 1;
        }
        seen.push(name.clone());
        out.push((name, id, h));
    }
    Ok(out)
}

/// 引数 `each` が真か(ボディごとのファイルに分ける)。
fn each(cx: &Context<'_>, id: &str) -> bool {
    id == EXPORT_STL_EACH
        || id == EXPORT_STEP_EACH
        || cx.arg("each").and_then(|v| v.as_bool()) == Some(true)
}

fn stl_bytes(meshes: &[kmb_core::Mesh], ascii: bool) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    if ascii {
        crate::write_ascii(meshes, "kieMonoBench", &mut out)
    } else {
        crate::write_binary(meshes, &mut out)
    }
    .map_err(|e| e.to_string())?;
    Ok(out)
}

/// ボディごとの STL。
fn stl_each(cx: &Context<'_>, tol: f64, ascii: bool) -> Result<Outcome, kmb_core::Problem> {
    let mut files = Vec::new();
    let mut triangles = 0;
    for (name, h) in named_bodies(cx)? {
        let m = cx.kernel.mesh(h, tol).map_err(|e| e.to_string())?;
        triangles += m.triangles.len();
        files.push((format!("{name}.stl"), stl_bytes(&[m], ascii)?));
    }
    log::info!("STL をボディごとに {} 個書き出す", files.len());
    Ok(Outcome::Files {
        info: json!({"bodies": files.len(), "triangles": triangles}),
        files,
    })
}

/// ボディごとの STEP。
fn step_each(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let mut files = Vec::new();
    for (name, h) in named_bodies(cx)? {
        let s = cx
            .kernel
            .export_step_named(&[h], std::slice::from_ref(&name), &name)
            .map_err(|e| e.to_string())?;
        files.push((format!("{name}.step"), s.into_bytes()));
    }
    log::info!("STEP をボディごとに {} 個書き出す", files.len());
    Ok(Outcome::Files {
        info: json!({"bodies": files.len()}),
        files,
    })
}

fn stl(cx: &Context<'_>, id: &str) -> Result<Outcome, kmb_core::Problem> {
    let tol = cx.arg_f64("tolerance")?.unwrap_or(MESH_TOLERANCE);
    if !(tol > 0.0001 && tol <= 1.0) {
        return Err("許容差は 0.0001〜1 mm にしてください".into());
    }
    let ascii = cx.arg("ascii").and_then(|v| v.as_bool()) == Some(true);
    if each(cx, id) {
        return stl_each(cx, tol, ascii);
    }
    let meshes = cx
        .export_bodies()?
        .iter()
        .map(|h| cx.kernel.mesh(*h, tol))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let out = stl_bytes(&meshes, ascii)?;
    let triangles: usize = meshes.iter().map(|m| m.triangles.len()).sum();
    Ok(Outcome::File {
        suffix: "形状.stl".into(),
        contents: out,
        info: json!({"bodies": meshes.len(), "triangles": triangles}),
    })
}

fn step(cx: &Context<'_>, id: &str) -> Result<Outcome, kmb_core::Problem> {
    if each(cx, id) {
        return step_each(cx);
    }
    // 立体の名前はブラウザのボディの名前(日本語もそのまま残す)
    let (names, hs): (Vec<String>, Vec<ShapeHandle>) = named_bodies(cx)?.into_iter().unzip();
    let s = cx
        .kernel
        .export_step_named(&hs, &names, "kieMonoBench")
        .map_err(|e| e.to_string())?;
    Ok(Outcome::File {
        suffix: "形状.step".into(),
        contents: s.into_bytes(),
        info: json!({"bodies": hs.len()}),
    })
}

/// ボディの色の名前と色: 付けた外観(部品の外観も)、無ければ物理材料(既定は鋼)。画面の色と同じ決め方。
fn body_color(cx: &Context<'_>, id: FeatureId) -> (String, [u8; 3]) {
    use kmb_look::material;
    let root = kmb_core::body_root(cx.data, cx.registry, id);
    let looks = kmb_look::appearance::of(cx.data);
    if let Some(a) = looks
        .appearance_of(cx.data, root)
        .and_then(|n| looks.resolve(n))
    {
        return (a.name, a.color);
    }
    let m = material::material_of(cx.data, root);
    let color = material::phys_material(&m).map_or([160, 160, 160], |p| p.color);
    (m, color)
}

/// 面の外観の色(面の名前 → 色の名前と色)。画面で面を塗るのと同じ外観を引く。
fn face_colors(
    cx: &Context<'_>,
    id: FeatureId,
) -> std::collections::BTreeMap<String, (String, [u8; 3])> {
    let root = kmb_core::body_root(cx.data, cx.registry, id);
    let looks = kmb_look::appearance::of(cx.data);
    looks
        .faces
        .get(&root)
        .into_iter()
        .flatten()
        .filter_map(|(face, n)| {
            let a = looks.resolve(n)?;
            Some((face.clone(), (a.name, a.color)))
        })
        .collect()
}

/// 3MF の単位: 引数 `unit`、無ければ文書の長さの単位。
fn unit_arg(cx: &Context<'_>) -> Result<kmb_core::LengthUnit, String> {
    match cx.arg("unit").and_then(|v| v.as_str()) {
        Some(u) => kmb_core::LengthUnit::parse(u)
            .ok_or_else(|| format!("単位 {u} は使えません(mm・cm・m・in・ft)")),
        None => Ok(cx.data.properties.units),
    }
}

/// 3MF に入れる設計の属性(Core 仕様の名前、値)。
fn meta_3mf(cx: &Context<'_>) -> Vec<(&'static str, String)> {
    let p = &cx.data.properties;
    vec![
        ("Title", p.title.clone()),
        ("Designer", p.designer.clone()),
        ("Description", p.description.clone()),
        ("Copyright", p.copyright.clone()),
        ("LicenseTerms", p.license.clone()),
        ("Application", "kieMonoBench".into()),
    ]
}

fn threemf(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let tol = cx.arg_f64("tolerance")?.unwrap_or(MESH_TOLERANCE);
    if !(tol > 0.0001 && tol <= 1.0) {
        return Err("許容差は 0.0001〜1 mm にしてください".into());
    }
    let unit = unit_arg(cx)?;
    let meta = meta_3mf(cx);
    let mut bodies = Vec::new();
    for (name, id, h) in named_body_ids(cx)? {
        let mesh = cx.kernel.mesh(h, tol).map_err(|e| e.to_string())?;
        let (material, color) = body_color(cx, id);
        bodies.push(crate::Body3mf {
            name,
            mesh,
            color,
            material,
            faces: face_colors(cx, id),
        });
    }
    let triangles: usize = bodies.iter().map(|b| b.mesh.triangles.len()).sum();
    let info = json!({"bodies": bodies.len(), "triangles": triangles, "unit": unit.name()});
    log::info!(
        "3MF を書き出す: ボディ {} 個、三角形 {triangles} 個、単位 {}",
        bodies.len(),
        unit.name()
    );
    if cx.arg("each").and_then(|v| v.as_bool()) == Some(true) {
        let files = bodies
            .iter()
            .map(|b| {
                (
                    format!("{}.3mf", b.name),
                    crate::write_3mf(std::slice::from_ref(b), unit, &meta),
                )
            })
            .collect();
        return Ok(Outcome::Files { info, files });
    }
    Ok(Outcome::File {
        suffix: "形状.3mf".into(),
        contents: crate::write_3mf(&bodies, unit, &meta),
        info,
    })
}

/// 三角形の網にする許容差の既定(mm)。
const MESH_TOLERANCE: f64 = 0.01;

/// 書き出しの操作が受け取る引数(`what` の操作の分)。
fn args_of(what: &str) -> Vec<Arg> {
    let exclude = Arg::list(
        "exclude",
        "書かないボディ(機能の番号の並び。画面は目を閉じたボディを渡す)",
    );
    let each = Arg::flag("each", "ボディごとに別のファイルにする");
    let mesh = Arg::length("tolerance", "三角形の網の許容差(0.0001〜1 mm)").default(MESH_TOLERANCE);
    match what {
        "stl" => vec![exclude, mesh, Arg::flag("ascii", "ASCII 形式で書く"), each],
        "3mf" => vec![
            exclude,
            mesh,
            Arg::choice(
                "unit",
                "3MF の長さの単位(省くと文書の単位)",
                &[
                    ("mm", "mm"),
                    ("cm", "cm"),
                    ("m", "m"),
                    ("in", "in"),
                    ("ft", "ft"),
                ],
            ),
            each,
        ],
        // STEP
        _ => vec![exclude, each],
    }
}

impl Extension for ExportCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let why = cx.export_bodies().err();
        let save = |title: &str, filter: &str, ext: &[&str]| Input::SaveFile {
            title: title.into(),
            filter: filter.into(),
            extensions: ext.iter().map(|e| e.to_string()).collect(),
        };
        vec![
            Action::new(EXPORT_STEP, "STEP を書き出す…", "cad")
                .menu("書き出し")
                .icon("stepout")
                .aliases("すてっぷ step stp")
                .hint("最後に残ったボディを STEP にして書き出す")
                .at(&[Place::Toolbar, Place::FileMenu])
                .input(save("STEP を書き出す", "STEP", &["step", "stp"]))
                .args(args_of("step"))
                .disabled_if(why.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES),
            Action::new(EXPORT_STL, "STL を書き出す…", "cad")
                .menu("書き出し")
                .icon("stlout")
                .aliases("えすてぃーえる stl 3d ぷりんた")
                .hint("最後に残ったボディを三角形の網にして書き出す")
                .at(&[Place::Menu, Place::FileMenu])
                .input(save("STL を書き出す", "STL", &["stl"]))
                .args(args_of("stl"))
                .disabled_if(why.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES),
            Action::new(EXPORT_STL_EACH, "STL をボディごとに書き出す…", "cad")
                .menu("書き出し")
                .icon("stlout")
                .aliases("えすてぃーえる stl 3d ぷりんた ぼでぃごと べつべつ 分けて")
                .hint("見えているボディを 1 つずつ別の STL にする。選んだ名前の後ろにボディの名前を付けて並べる(部品.stl → 部品-ボディ1.stl)")
                .at(&[Place::Menu, Place::FileMenu])
                .input(save("STL をボディごとに書き出す", "STL", &["stl"]))
                .args(args_of("stl"))
                .disabled_if(why.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES),
            Action::new(EXPORT_STEP_EACH, "STEP をボディごとに書き出す…", "cad")
                .menu("書き出し")
                .icon("stepout")
                .aliases("すてっぷ step stp ぼでぃごと べつべつ 分けて")
                .hint("見えているボディを 1 つずつ別の STEP にする。選んだ名前の後ろにボディの名前を付けて並べる")
                .at(&[Place::Menu, Place::FileMenu])
                .input(save("STEP をボディごとに書き出す", "STEP", &["step", "stp"]))
                .args(args_of("step"))
                .disabled_if(why.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES),
            // 設計のツールバーの「書き出し」は STEP を前に出し、ほかは ▾ の一覧に入れる
            //
            Action::new(EXPORT_3MF, "3MF を書き出す…", "cad")
                .menu("書き出し")
                .icon("3mfout")
                .aliases("すりーえむえふ 3mf 3d ぷりんた 3D プリンタ すらいさー 色 いろ")
                .hint("見えているボディを 3D プリンタ向けの 3MF にする。ボディごとの色(外観、無ければ材料)と、文書の長さの単位、設計の属性(部品名・作者など)も一緒に書く")
                .at(&[Place::Menu, Place::FileMenu])
                .input(save("3MF を書き出す", "3MF", &["3mf"]))
                .args(args_of("3mf"))
                .disabled_if(why.as_deref())
                .role(kmb_interchange::role::action::EXPORT_BODIES),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        match id {
            EXPORT_STL | EXPORT_STL_EACH => Some(stl(cx, id)),
            EXPORT_STEP | EXPORT_STEP_EACH => Some(step(cx, id)),
            EXPORT_3MF => Some(threemf(cx)),
            _ => None,
        }
    }
}
