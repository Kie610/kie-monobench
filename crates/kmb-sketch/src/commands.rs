//! スケッチの操作(画面・CLI・MCP に出す): スケッチの追加、面の上のスケッチ、スケッチ平面の
//! 再定義。文字(kmb-text)と DXF・SVG(kmb-io-dxf・kmb-io-svg)の操作は、それぞれの crate が持ち、
//! ここの [`selected_sketch`]・[`insert_file`] を使う。

use crate::{Imported, SKETCH, Sketch, SketchPlane};
use kmb_core::{Action, Context, Edit, Extension, Gesture, Handle, HandleChoice, Outcome, Place};

pub const ADD_SKETCH: &str = "cad.add.sketch";
pub const SKETCH_ON_FACE: &str = "cad.sketch_on_face";
/// 選んでいるスケッチを別の平面・面へ移す
pub const REDEFINE_PLANE: &str = "cad.sketch.redefine_plane";
/// 選んでいるスケッチに、足りない拘束と寸法を付ける
pub const AUTO_CONSTRAIN: &str = "cad.sketch.auto_constrain";

const NEED_FACE: &str = "3D のビューで平らな面を選んでから使う";

/// スケッチの操作。
#[derive(Default)]
pub struct SketchCommands {
    /// PC 全体の設定(寸法の見せ方・面の縁を投影するか)。組み立てが共有の値としても置く
    pub(crate) store: std::sync::Arc<crate::display::SketchSettings>,
}

impl SketchCommands {
    /// 設定の入れ物(試験で今の値を見る)。
    #[cfg(test)]
    pub(crate) fn settings_store(&self) -> &crate::display::SketchSettings {
        &self.store
    }
}

/// 選んだ面に新しく描く平面。原点は面の左下の角(まとめてレビューの R2。[`crate::FaceOrigin`])。
pub fn picked_sketch_plane(cx: &Context<'_>) -> Option<SketchPlane> {
    cx.picked_plane()?;
    let p = cx.picked?;
    let origin = match cx.output(p.feature, "body") {
        Some(kmb_core::Value::Shape(h)) => crate::default_origin(cx.kernel.as_ref(), *h, &p.face),
        _ => crate::FaceOrigin::World,
    };
    Some(SketchPlane::Face {
        feature: p.feature,
        face: p.face.clone(),
        origin,
    })
}

/// 面の上の新しいスケッチに、面の縁を投影する(Kie の決定 2026-10-19: 補助線にせず、押し出せる
/// 輪郭として写す)。設定「面にスケッチを作る時、面の縁を
/// 投影する」か、引数 `project_edges: false` で切れる。辺は両側の面の名前で指すので、元の寸法を変えると
/// 付いて行く(投影の道具で面を押した時と同じ)。円の縁の中心は写さない。
fn with_face_edges(cx: &Context<'_>, mut sketch: Sketch) -> Sketch {
    let on = cx
        .arg("project_edges")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_else(|| {
            cx.registry
                .service::<crate::display::SketchSettings>()
                .is_none_or(|s| s.project_face_edges())
        });
    let SketchPlane::Face {
        feature,
        face,
        origin,
    } = &sketch.plane
    else {
        return sketch;
    };
    let Some(kmb_core::Value::Shape(h)) = cx.output(*feature, "body") else {
        return sketch;
    };
    if !on {
        return sketch;
    }
    let Ok((plane, _)) = crate::face_plane(cx.kernel.as_ref(), *h, face, origin) else {
        return sketch;
    };
    let items: Vec<crate::RefItem> =
        crate::body_edge_refs(cx.kernel.as_ref(), *feature, *h, &plane)
            .into_iter()
            .filter(|r| {
                matches!(&r.source, crate::ProjectionSource::Edge { faces, .. } if faces.contains(face))
            })
            .collect();
    for r in &items {
        sketch.add_projection_as(r, false);
    }
    sketch
}

/// 「スケッチを作成」で選べる基準の平面(指定の値、名前、平面)。
const BASE_PLANES: [(&str, &str, kmb_core::Plane); 3] = [
    ("xy", "XY 平面", kmb_core::Plane::XY),
    ("yz", "YZ 平面", kmb_core::Plane::YZ),
    ("zx", "ZX 平面", kmb_core::Plane::ZX),
];

fn base_plane(key: &str) -> Option<SketchPlane> {
    Some(match key {
        "xy" => SketchPlane::Xy,
        "yz" => SketchPlane::Yz,
        "zx" => SketchPlane::Zx,
        _ => return None,
    })
}

/// 指定 `plane` の平面: xy・yz・zx か作業平面の機能の番号(face は選んでいる面なので、ここでは
/// 読まない)。知らない値は誤り。
fn plane_arg(cx: &Context<'_>) -> Result<Option<SketchPlane>, String> {
    let Some(v) = cx.arg("plane") else {
        return Ok(None);
    };
    if let Some(n) = v.as_u64() {
        let id = kmb_core::FeatureId(n);
        if kmb_interchange::datum_planes(cx).iter().all(|(f, ..)| *f != id) {
            return Err(format!("機能 {n} は作業平面ではありません"));
        }
        return Ok(Some(SketchPlane::Datum { feature: id }));
    }
    match v.as_str() {
        Some("face") => Ok(None),
        Some(key) => base_plane(key)
            .map(Some)
            .ok_or_else(|| format!("平面 {key} はありません(xy・yz・zx・face か作業平面の機能の番号)")),
        None => Err(format!("平面 {v} はありません(xy・yz・zx・face か作業平面の機能の番号)")),
    }
}

/// 選んでいる作業平面の上の平面。
fn selected_datum_plane(cx: &Context<'_>) -> Option<SketchPlane> {
    kmb_interchange::selected_datum(cx).map(|feature| SketchPlane::Datum { feature })
}

/// 選んでいるスケッチ(ID、中身)。
pub fn selected_sketch(cx: &Context<'_>) -> Option<(kmb_core::FeatureId, Sketch)> {
    let id = cx.selected?;
    let f = cx.data.feature(id).filter(|f| f.type_id == SKETCH)?;
    Some((id, serde_json::from_value(f.data.clone()).ok()?))
}

/// ファイル(DXF・SVG)の挿入: `read` で読んだ形を、選んでいるスケッチへ足す(なければ XY 平面の
/// 新しいスケッチ)。置く位置はファイルの座標のまま。
/// `kind` はファイルの種類の名前(知らせに使う)。
pub fn insert_file(
    cx: &Context<'_>,
    kind: &str,
    read: fn(&str) -> Result<Imported, String>,
) -> Result<Outcome, String> {
    let path = cx
        .file
        .ok_or_else(|| format!("{kind} のファイルを選んでから使う"))?;
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    let read = read(&text)?;
    if read.shapes.is_empty() {
        return Err(format!("{kind} に読める形がありません"));
    }
    let label = format!("{kind} を挿入");
    let (target, mut sketch) = match selected_sketch(cx) {
        Some((id, s)) => (Some(id), s),
        None => (None, Sketch::default()),
    };
    if sketch
        .insert_shapes(&read.shapes, crate::Place::default())
        .is_empty()
    {
        return Err(format!(
            "{kind} の形が小さすぎるか長さ 0 で、足せる線がありません"
        ));
    }
    let data = serde_json::to_value(&sketch).expect("JSON にできる");
    Ok(match target {
        Some(id) => Outcome::Change {
            label,
            edits: vec![Edit::SetData { id, data }],
        },
        None => Outcome::insert(
            &label,
            SKETCH,
            format!("スケッチ{}", cx.count_of(SKETCH) + 1),
            data,
        ),
    })
}

/// 自動拘束: 選んでいるスケッチに、候補を全部付ける(画面では候補を予告して種類ごとに外せる)。
fn auto_constrain(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, sketch) = selected_sketch(cx).ok_or("スケッチを選んでから使う")?;
    let mut sol = cx
        .output(id, "sketch")
        .and_then(crate::solution_of)
        .ok_or("スケッチが解けていないので使えません(先に誤りを直す)")?;
    let dims = cx
        .arg("dimensions")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let mut s = sketch.clone();
    let values = crate::measured_values(&s, &sol);
    let datum = dims.then(|| crate::with_origin(&mut s, &mut sol));
    let opts = crate::AutoOptions {
        gap: cx.arg_f64("gap")?.unwrap_or(crate::AUTO_GAP),
        angle: cx
            .arg_f64("angle")?
            .unwrap_or(crate::AUTO_ANGLE_DEG)
            .to_radians(),
        dimensions: dims,
        datum,
        ..Default::default()
    };
    let r = crate::auto_constrain(&s, &sol, &values, &opts).map_err(|e| e.to_string())?;
    if r.suggestions.is_empty() {
        return Err(format!(
            "付けられる拘束がありません(自由度 {}。今の位置で成り立つ物は既に決まっています)",
            r.dof_before
        ));
    }
    let out = crate::apply_suggestions(&s, &sol, &r.suggestions);
    Ok(Outcome::Change {
        label: format!(
            "自動拘束({} 個、自由度 {} → {})",
            r.suggestions.len(),
            r.dof_before,
            r.dof_after
        ),
        edits: vec![Edit::SetData {
            id,
            data: serde_json::to_value(&out).expect("JSON にできる"),
        }],
    })
}

/// スケッチ平面を再定義: 選んでいるスケッチの平面を、クリックした面か XY・YZ・ZX に替える。
/// 線と拘束はそのまま(平面の座標で同じ位置)、平面からの距離も残す。スケッチより後にできた
/// ボディの面は選べない(計算の順が逆になる)。
fn redefine_plane(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, mut sketch) = selected_sketch(cx).ok_or("スケッチを選んでから使う")?;
    let plane = match picked_sketch_plane(cx) {
        Some(p) => p,
        None => plane_arg(cx)?
            .ok_or("新しい平面(XY・YZ・ZX の四角か作業平面、ボディの平らな面)をクリックして選ぶ")?,
    };
    if let Some(feature) = &plane.source() {
        let at = |f: kmb_core::FeatureId| cx.data.features.iter().position(|e| e.id == f);
        if *feature == id || at(*feature) > at(id) {
            return Err(
                "このスケッチより後にできた面や作業平面には移せません(計算の順が逆になる)".into(),
            );
        }
    }
    if plane == sketch.plane {
        return Err("今と同じ平面です".into());
    }
    sketch.plane = plane;
    Ok(Outcome::Change {
        label: "スケッチ平面を再定義".into(),
        edits: vec![Edit::SetData {
            id,
            data: serde_json::to_value(&sketch).expect("JSON にできる"),
        }],
    })
}

/// 機能 `type_id` を 1 つ足す変更(名前 `name`、データ `data`)。
pub fn insert_feature<T: serde::Serialize>(
    label: &str,
    type_id: &str,
    name: String,
    data: &T,
) -> Outcome {
    Outcome::insert(
        label,
        type_id,
        name,
        serde_json::to_value(data).expect("JSON にできる"),
    )
}

/// 拘束の解の状態を言葉で(CLI・MCP の再計算の結果に出す)。
pub(crate) fn status_text(s: &crate::Solution) -> String {
    use crate::SolveStatus as S;
    let ids = |v: &[crate::ConstraintId]| {
        v.iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    if s.is_blank() {
        return crate::NO_SHAPE.into();
    }
    match &s.status {
        S::WellConstrained => "完全拘束".into(),
        S::UnderConstrained { dof } => format!("拘束不足、自由度 {dof}"),
        S::OverConstrained { redundant } => format!("過剰な拘束 {}", ids(redundant)),
        S::Inconsistent { conflicting } => format!("満たせない拘束 {}", ids(conflicting)),
    }
}

impl Extension for SketchCommands {
    fn describe(&self, v: &kmb_core::Value) -> Option<String> {
        if let Some(s) = crate::solution_of(v) {
            return Some(format!("スケッチ({})", status_text(&s)));
        }
        crate::profile_value(v).map(|p| format!("輪郭 {} 個", p.regions.len()))
    }

    fn settings(&self) -> Vec<kmb_core::Settings> {
        vec![
            crate::display::settings(&self.store),
            crate::display::create_settings(&self.store),
        ]
    }

    fn set_settings(&self, id: &str, value: &serde_json::Value) -> Option<Result<(), String>> {
        if id == crate::display::CREATE_SETTINGS {
            return Some(crate::display::set_create_settings(&self.store, value));
        }
        (id == crate::DISPLAY_SETTINGS).then(|| crate::display::set_settings(&self.store, value))
    }

    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let face = cx.picked_plane().is_some();
        let on_face: &[Place] = if face { &[Place::FaceMenu] } else { &[] };
        vec![
            Action::new(ADD_SKETCH, "スケッチを作成", "cad")
                .args([kmb_core::Arg::value("plane", "描く平面: xy・yz・zx・face(手順でクリックした面)か作業平面の機能の番号(省くと選んでいる面か作業平面、なければ XY)"), kmb_core::Arg::flag("project_edges", "面の外周の辺を投影する(省くと設定の「面の辺を投影する」)")])
                .menu("作成")
                .icon("sketch")
                .aliases("すけっち sketch")
                .hint("空のスケッチを足す。3D のビューで平らな面を選んでいればその面に、なければ押してから XY・YZ・ZX の平面をクリックして選ぶ")
                .at(&[Place::Toolbar]),
            Action::new(SKETCH_ON_FACE, "面にスケッチ", "cad")
                .arg(kmb_core::Arg::flag("project_edges", "面の外周の辺を投影する(省くと設定の「面の辺を投影する」)"))
                .menu("作成")
                .icon("sketchFace")
                .aliases("めん すけっち sketch face")
                .hint("3D のビューで平らな面を選んでから押す")
                .at(on_face)
                .disabled_if((!face).then_some(NEED_FACE)),
            Action::new(REDEFINE_PLANE, "スケッチ平面を再定義", "cad")
                .arg(kmb_core::Arg::value("plane", "新しい平面: xy・yz・zx・face(手順でクリックした面)か作業平面の機能の番号(省くと選んでいる面)"))
                .menu("作成")
                .icon("replane")
                .aliases("へいめん さいていぎ redefine plane めん うつす")
                .hint("選んでいるスケッチを、クリックした面か XY・YZ・ZX の平面へ移す(線と拘束はそのまま)")
                .at(if selected_sketch(cx).is_some() {
                    &[Place::FeatureMenu]
                } else {
                    &[]
                })
                .disabled_if(selected_sketch(cx).is_none().then_some("スケッチを選んでから使う")),
            Action::new(AUTO_CONSTRAIN, "自動拘束", "cad")
                .args([
                    kmb_core::Arg::flag("dimensions", "寸法(直径・半径・長さと、原点からの横と縦の距離)も付ける").default(true),
                    kmb_core::Arg::length("gap", "同じ位置・線の上とみなす距離(mm)").default(crate::AUTO_GAP),
                    kmb_core::Arg::angle("angle", "水平・垂直・接する・平行・直角とみなす角度(deg)").default(crate::AUTO_ANGLE_DEG),
                ])
                .menu("作成")
                .icon("autocons")
                .aliases("じどう こうそく auto constrain fully define かんぜんこうそく")
                .hint("選んでいるスケッチの、今の位置で既に成り立っている一致・水平・垂直・接するなどと寸法を、足りない分だけ付ける(等しいは推測しない)")
                .at(if selected_sketch(cx).is_some() {
                    &[Place::FeatureMenu]
                } else {
                    &[]
                })
                .disabled_if(selected_sketch(cx).is_none().then_some("スケッチを選んでから使う")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        Some(match id {
            // 平らな面を選んでいれば、その面に(面を選んでから押せばそこに描く)
            // 平面を選ぶ手順で選んだ平面(`plane` が xy・yz・zx)。指定がなければ XY
            // 選んでいる作業平面か、手順で選んだ作業平面(`plane` が機能の番号)にも描ける
            ADD_SKETCH => plane_arg(cx).map(|given| {
                insert_feature(
                    "スケッチを作成",
                    SKETCH,
                    format!("スケッチ{}", cx.count_of(SKETCH) + 1),
                    &with_face_edges(
                        cx,
                        Sketch {
                            plane: picked_sketch_plane(cx)
                                .or(given)
                                .or_else(|| selected_datum_plane(cx))
                                .unwrap_or_default(),
                            ..Default::default()
                        },
                    ),
                )
            }),
            SKETCH_ON_FACE => match picked_sketch_plane(cx) {
                Some(plane) => Ok(insert_feature(
                    "面にスケッチを追加",
                    SKETCH,
                    format!("スケッチ{}", cx.count_of(SKETCH) + 1),
                    &with_face_edges(
                        cx,
                        Sketch {
                            plane,
                            ..Default::default()
                        },
                    ),
                )),
                None => Err(NEED_FACE.into()),
            },
            REDEFINE_PLANE => redefine_plane(cx),
            AUTO_CONSTRAIN => auto_constrain(cx),
            _ => return None,
        })
    }

    /// 面を選ばずに「スケッチを作成」を押した時は、XY・YZ・ZX の平面をクリックして選ぶ。
    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if cx.picked_plane().is_some() {
            return None;
        }
        if id != ADD_SKETCH && id != REDEFINE_PLANE {
            return None;
        }
        if id == ADD_SKETCH && selected_datum_plane(cx).is_some() {
            return None;
        }
        // 軸の正の向きへ広がる正方形を原点から少し浮かせる(描く四角と同じ)。
        // 大きさは画面の上で一定(ビューが原点の目安の大きさに伸ばす。長さ 1 がその大きさ)
        let (a, b) = (kmb_core::Plane::ORIGIN_GAP, 1.0 + kmb_core::Plane::ORIGIN_GAP);
        let choices = BASE_PLANES
            .iter()
            .map(|(key, label, plane)| {
                let corner = |u: f64, v: f64| {
                    let w = plane.to_world([u, v]);
                    // 平面の座標の向きによらず、世界の軸の正の側に置く
                    w.map(f64::abs)
                };
                HandleChoice {
                    value: serde_json::json!(key),
                    label: (*label).into(),
                    outline: vec![vec![
                        corner(a, a),
                        corner(b, a),
                        corner(b, b),
                        corner(a, b),
                        corner(a, a),
                    ]],
                    sized: true,
                    ..Default::default()
                }
            })
            .chain(kmb_interchange::datum_choices(cx))
            .collect();
        // 原点の平面と作業平面のほかに、ボディの平らな面もそのままクリックで選べる
        Some(vec![Handle {
            choices,
            faces: true,
            frame: None,
            ..Handle::new(
                "plane",
                if id == REDEFINE_PLANE {
                    "スケッチを移す平面(XY・YZ・ZX の四角)か、ボディの平らな面をクリック"
                } else {
                    "スケッチを描く平面(XY・YZ・ZX の四角)か、ボディの平らな面をクリック"
                },
                Gesture::Choice,
            )
        }])
    }
}
