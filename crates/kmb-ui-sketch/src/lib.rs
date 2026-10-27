//! スケッチの編集器。画面(kmb-ui)の拡張として、スケッチを選んだ時に中央のビューを描く。
//! 線・円・円弧などの道具で描き、拘束と寸法のパネルで形を決める。

mod annot;
mod auto_ui;
mod clipboard;
mod constraint_tool;
mod constraints_ui;
mod dim_choice;
mod display;
mod drag;
mod ends_ui;
mod group_ui;
mod hud;
mod infer;
mod marks;
mod menu;
mod more_tools;
mod offset_tool;
mod pattern_ui;
mod snap;
mod space3d;
mod view;

pub use space3d::{Sketch3dEditor, Tool3};
pub use view::Tool;

use kmb_core::FeatureEntry;
use kmb_sketch::{SKETCH, Sketch, Solution};
use kmb_ui::{EditorHost, EditorTool, FeatureEditor, ToolGroup};
use std::collections::HashMap;

/// 面の輪郭を作り直す目安(ボディ、面、スケッチの平面。原点を替えると座標が変わる)
type FaceKey = (kmb_core::ShapeHandle, String, kmb_core::Plane);

/// スケッチの編集器の状態(表示の移動と倍率、描いている途中の道具)。
#[derive(Default)]
pub struct SketchEditor {
    view: view::ViewState,
    editor: view::Editor,
    /// 下の帯へ出した失敗の理由。解けた時は自分が出した知らせだけを消す。
    diagnosis: Option<(kmb_core::FeatureId, String)>,
    /// 置いた面の輪郭(ボディと面の名前ごとに作り置く)
    outline: Option<(FaceKey, Vec<[[f64; 2]; 2]>)>,
    /// 既存の形をスケッチの平面へ写したもの(平面と、元のボディ・前のスケッチごとに作り置く)
    refs: Option<((RefsKey, SketchesKey), Vec<kmb_sketch::RefItem>)>,
    /// 前にできたボディの断面(スライスと交差の道具。平面と元のボディごとに作り置く)
    sections: Option<(RefsKey, Vec<view::Section>)>,
    /// 前にできたボディの網(投影の道具。平面と元のボディごとに作り置く)
    bodies: Option<(RefsKey, Vec<view::BodyMesh>)>,
    /// 写した交差とシルエットの元(元が見つかるかを見る。平面と、それを写したボディごと)
    linked: Option<((RefsKey, SketchesKey), Vec<kmb_sketch::RefItem>)>,
    /// `名前=値` と打った寸法: 先にパラメータを作ってもらい、次の更新で置くスケッチ
    deferred: Option<(kmb_core::FeatureId, Sketch)>,
    /// 面の上のスケッチの原点の候補(ボディと面の名前ごとに作り置く)
    origins: Option<((kmb_core::ShapeHandle, String), FaceOrigins)>,
    /// 3D の視点で描く、前にできたボディ(3D の視点で見ている間だけ作る。N1b)
    scene: kmb_ui::view3d::Scene,
}

/// 面の上のスケッチの原点にできる所(面の角、角が無ければ円の中心)と、面の法線から決まる平面
/// (原点は世界の原点を写した点。左下などの隅はこの向きで決める)。
#[derive(Clone)]
struct FaceOrigins {
    base: kmb_core::Plane,
    spots: Vec<kmb_sketch::FaceSpot>,
}

/// 写した形の作り置きのキー(スケッチの平面と、元のボディ)。
/// 前のスケッチと、その写せる要素(K8)。
type SketchesKey = Vec<(kmb_core::FeatureId, kmb_sketch::SketchItems)>;

type RefsKey = (
    kmb_core::Plane,
    Vec<(kmb_core::FeatureId, kmb_core::ShapeHandle)>,
);

impl SketchEditor {
    pub fn new() -> Self {
        Self::default()
    }

    fn show_diagnosis(
        &mut self,
        host: &mut EditorHost<'_>,
        id: kmb_core::FeatureId,
        reason: Option<String>,
    ) {
        let diagnosis = reason.map(|reason| (id, reason));
        if diagnosis != self.diagnosis {
            let previous = self.diagnosis.as_ref().map(|(_, reason)| reason);
            let next = diagnosis.as_ref().map(|(_, reason)| reason);
            if next.is_some() || host.message.as_ref() == previous {
                *host.message = next.cloned();
            }
            self.diagnosis = diagnosis;
        }
    }

    /// 面の上のスケッチなら、原点にできる所(作り置く)。
    fn face_origins(&mut self, host: &EditorHost<'_>, sketch: &Sketch) -> Option<FaceOrigins> {
        let kmb_sketch::SketchPlane::Face { feature, face, .. } = &sketch.plane else {
            return None;
        };
        let Some(kmb_core::Value::Shape(h)) = host.output(*feature, "body") else {
            return None;
        };
        let key = (*h, face.clone());
        if self.origins.as_ref().is_none_or(|(k, _)| *k != key) {
            let kernel = host.kernel.as_ref();
            let (base, _) =
                kmb_sketch::face_plane(kernel, *h, face, &kmb_sketch::FaceOrigin::World).ok()?;
            let spots = kmb_sketch::face_spots(kernel, *h, face);
            self.origins = Some((key, FaceOrigins { base, spots }));
        }
        self.origins.as_ref().map(|(_, o)| o.clone())
    }

    /// 描く道具を選ぶ。
    pub fn set_tool(&mut self, tool: Tool) {
        self.editor.set_tool(tool);
    }
}

/// パラメータの現在値で、スケッチの寸法の値を求める。
fn dimension_values(host: &EditorHost<'_>, sketch: &Sketch) -> Option<view::DimValues> {
    let params = host.param_values();
    kmb_sketch::dimension_values(sketch, &|n| params.get(n).copied()).ok()
}

/// 直したスケッチの寸法の値が計算できるか。式や単位の誤った寸法を置くと、スケッチの計算が
/// 失敗して、スケッチと後ろの形が全部消える。前は計算できていたのに直すと計算できなくなる
/// 変更は、置かずに断る(前から計算できないスケッチは、直すのを止めない)。
fn refuse_broken_dims(
    host: &EditorHost<'_>,
    before: &Sketch,
    after: Sketch,
    editor: &mut view::Editor,
) -> Option<Sketch> {
    refuse_broken_dims_with(host, before, after, editor, None)
}

fn refuse_broken_dims_with(
    host: &EditorHost<'_>,
    before: &Sketch,
    after: Sketch,
    editor: &mut view::Editor,
    extra: Option<(&str, kmb_core::Quantity)>,
) -> Option<Sketch> {
    let mut params = host.param_values();
    params.extend(extra);
    let lookup = |n: &str| params.get(n).copied();
    match kmb_sketch::dimension_values(&after, &lookup) {
        Err(e) if kmb_sketch::dimension_values(before, &lookup).is_ok() => {
            editor.dim_error = Some(format!("この値では置けません: {e}"));
            None
        }
        _ => match name_cycle(host, before, &after) {
            Some(e) => {
                editor.dim_error = Some(format!("この値では置けません: {e}"));
                None
            }
            None => Some(after),
        },
    }
}

/// 直したスケッチの寸法が、ほかのスケッチの寸法やパラメータと参照で輪になるか
/// (`d1 = d5` と `d5 = d1 * 2` が別のスケッチにあるなど)。輪になる寸法の言葉を返す。
fn name_cycle(host: &EditorHost<'_>, before: &Sketch, after: &Sketch) -> Option<String> {
    let mine: Vec<kmb_core::ModelParam> = after.model_params();
    let old: Vec<String> = before.model_params().into_iter().map(|p| p.name).collect();
    let mut exprs: Vec<(&str, &str)> = host
        .data
        .params
        .iter()
        .map(|p| (p.name.as_str(), p.expr.as_str()))
        .collect();
    exprs.extend(
        host.report
            .model_params
            .iter()
            .filter(|p| !old.contains(&p.name))
            .map(|p| (p.name.as_str(), p.expr.as_str())),
    );
    let first = exprs.len();
    exprs.extend(mine.iter().map(|p| (p.name.as_str(), p.expr.as_str())));
    let status = kmb_core::eval_exprs(&exprs);
    status[first..]
        .iter()
        .zip(&mine)
        .find_map(|(st, p)| match st {
            kmb_core::ParamStatus::Error { message } if message.contains("循環") => {
                Some(format!("寸法 {} の参照が循環しています", p.name))
            }
            _ => None,
        })
}

/// 寸法の値の `名前 = 式` を `名前` に直し、作るパラメータ(名前と式)を返す(
/// 寸法の欄で `w=20` と打つと w というパラメータができ、寸法は w になる)。
/// 数だけの式には寸法の単位(mm・deg)を補う。
pub(crate) fn split_new_param(sketch: &mut Sketch) -> Option<(String, String)> {
    for c in &mut sketch.constraints {
        let angle = c.kind.is_angle();
        let Some(v) = c.kind.value_mut() else {
            continue;
        };
        let Some((name, expr)) = v.split_once('=') else {
            continue;
        };
        let (name, expr) = (name.trim().to_string(), expr.trim());
        if name.is_empty() || expr.is_empty() || expr.contains('=') {
            continue;
        }
        let unit = if angle { "0 deg" } else { "0 mm" };
        let expr = kmb_ui::with_unit_of(expr, unit);
        *v = name.clone();
        return Some((name, expr));
    }
    None
}

/// `名前=式` で作るパラメータを調べ、よければ画面へパラメータを作るよう頼んで、スケッチは
/// 次の更新まで取っておく(パラメータができてから置く)。
fn defer_with_param(
    host: &mut EditorHost<'_>,
    before: &Sketch,
    mut after: Sketch,
    editor: &mut view::Editor,
    deferred: &mut Option<(kmb_core::FeatureId, Sketch)>,
    feature: kmb_core::FeatureId,
) -> Option<Sketch> {
    let Some((name, expr)) = split_new_param(&mut after) else {
        return refuse_broken_dims(host, before, after, editor);
    };
    // 単位や組み込みの名前(mm、pi など)は、パラメータがなくても値になる
    let reserved = kmb_core::eval_str(&name, &|_| None).is_ok();
    let valid = name
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !valid || reserved {
        editor.dim_error = Some(format!("「{name}」はパラメータの名前に使えません"));
        return None;
    }
    if host.name_taken(&name) {
        let what = if host.data.param_by_name(&name).is_some() {
            "パラメータ"
        } else {
            "寸法の名前"
        };
        editor.dim_error = Some(format!(
            "{what}「{name}」は既にあります(値に {name} とだけ打てば使えます)"
        ));
        return None;
    }
    let value = match host.eval(&expr) {
        Ok(v) => v,
        Err(e) => {
            editor.dim_error = Some(format!("{expr} を計算できません: {e}"));
            return None;
        }
    };
    let s = refuse_broken_dims_with(host, before, after, editor, Some((&name, value)))?;
    *host.then = Some((
        kmb_ui::APP_ADD_PARAM.into(),
        serde_json::json!({ "name": name, "expr": expr }),
    ));
    *deferred = Some((feature, s));
    None
}

/// 面の上のスケッチで、置いた面の輪郭(穴の縁も含む。スケッチの平面 `plane` の座標の線分)。
/// 面の三角形のうち、1 つの三角形にしか使われない辺が面の縁。
/// `after` で新しく置いた点(`before` に無い点。線や円に使わない点。文字の道具が置いた点)。
fn new_lone_point(before: &Sketch, after: &Sketch) -> Option<kmb_sketch::EntityId> {
    let used: std::collections::BTreeSet<kmb_sketch::EntityId> =
        after.entities.iter().flat_map(|e| e.points()).collect();
    after
        .entities
        .iter()
        .filter(|e| matches!(e, kmb_sketch::Entity::Point { .. }))
        .map(|e| e.id())
        .filter(|id| before.entity(*id).is_none() && !used.contains(id))
        .max()
}

fn face_outline(
    kernel: &dyn kmb_core::Kernel,
    body: kmb_core::ShapeHandle,
    face: &str,
    plane: &kmb_core::Plane,
) -> Vec<[[f64; 2]; 2]> {
    let Ok(mesh) = kernel.mesh(body, 0.05) else {
        return vec![];
    };
    let Some(fi) = mesh.face_names.iter().position(|n| n == face) else {
        return vec![];
    };
    // 同じ位置の点は網の中で別の番号のことがあるので、位置で数える
    type Key = (i64, i64, i64);
    let key = |i: u32| -> Key {
        let p = mesh.positions[i as usize];
        let r = |v: f32| (v as f64 * 1e4).round() as i64;
        (r(p[0]), r(p[1]), r(p[2]))
    };
    let mut uses: HashMap<(Key, Key), (u32, u32, i32)> = HashMap::new();
    for (t, _) in mesh
        .triangles
        .iter()
        .zip(&mesh.triangle_faces)
        .filter(|(_, f)| **f as usize == fi)
    {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ka, kb) = (key(a), key(b));
            let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
            uses.entry(k).or_insert((a, b, 0)).2 += 1;
        }
    }
    let local = |i: u32| {
        let p = mesh.positions[i as usize];
        let q = plane.to_local([p[0] as f64, p[1] as f64, p[2] as f64]);
        [q[0], q[1]]
    };
    uses.into_values()
        .filter(|(_, _, n)| *n == 1)
        .map(|(a, b, _)| [local(a), local(b)])
        .collect()
}

/// スケッチの平面(面の上なら、その面の今の位置と原点)。面が見つからなければ `None`。
fn sketch_plane(host: &EditorHost<'_>, plane: &kmb_sketch::SketchPlane) -> Option<kmb_core::Plane> {
    use kmb_sketch::SketchPlane as P;
    Some(match plane {
        P::Xy => kmb_core::Plane::XY,
        P::Yz => kmb_core::Plane::YZ,
        P::Zx => kmb_core::Plane::ZX,
        P::Face {
            feature,
            face,
            origin,
        } => {
            let Some(kmb_core::Value::Shape(h)) = host.output(*feature, "body") else {
                return None;
            };
            kmb_sketch::face_plane(host.kernel.as_ref(), *h, face, origin)
                .ok()?
                .0
        }
        P::Datum { feature } => {
            <kmb_interchange::DatumPlane as kmb_core::Payload>::from_value(
                host.output(*feature, kmb_interchange::DATUM_OUTPUT)?,
            )?
            .plane
        }
    })
}

/// 平面からずらす距離(mm)。式が計算できなければ 0。
fn offset_mm(host: &EditorHost<'_>, sketch: &Sketch) -> f64 {
    let Some(src) = &sketch.offset else {
        return 0.0;
    };
    offset_of(host, src).unwrap_or(0.0)
}

/// 平面からずらす距離の式を、今のパラメータで計算する。
fn offset_of(host: &EditorHost<'_>, src: &str) -> Result<f64, String> {
    let params = host.param_values();
    kmb_sketch::offset_value(src, &|s| {
        kmb_core::eval_str(s, &|n| params.get(n).copied()).map_err(|e| e.to_string())
    })
}

/// 編集パネルの上の、描く平面と平面からの距離。
/// 距離を変えたら、直したスケッチを返す。
fn plane_section(
    ui: &mut egui::Ui,
    host: &mut EditorHost<'_>,
    sketch: &Sketch,
    editor: &mut view::Editor,
    view: &mut view::ViewState,
    origins: Option<&FaceOrigins>,
) -> Option<Sketch> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.label(format!("平面: {}", plane_label(&sketch.plane)));
        // スケッチ平面を再定義: スケッチを終え、3D のビューで新しい平面をクリックする
        if ui
            .small_button("平面を再定義…")
            .on_hover_text("スケッチを終え、3D のビューで移す先の平面か平らな面をクリックする")
            .clicked()
        {
            *host.then = Some((kmb_sketch::REDEFINE_PLANE.into(), serde_json::json!({})));
        }
    });
    if let (Some(o), Some(now)) = (origins, sketch_plane(host, &sketch.plane)) {
        out = origin_row(ui, sketch, editor, o, &now).map(|(s, shift)| {
            view.follow([-shift[0], -shift[1]]);
            s
        });
    }
    let error = &mut editor.dim_error;
    ui.horizontal(|ui| {
        ui.label("平面からの距離");
        let id = ui.id().with("sketch_offset");
        let cur = sketch.offset.clone().unwrap_or_default();
        let mut text = ui
            .data(|d| d.get_temp::<String>(id))
            .unwrap_or_else(|| cur.clone());
        // 数だけなら文書の単位を付ける(候補 19)
        let unit = kmb_core::display_unit().name();
        let r = ui
            .add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(90.0)
                    .hint_text(format!("0 {unit}")),
            )
            .on_hover_text(
                "平面を法線の向きにずらして描く(負なら裏側)。パラメータの式も使える。Enter か、欄を離れると決まる",
            );
        if r.has_focus() || r.changed() {
            ui.data_mut(|d| d.insert_temp(id, text.clone()));
        }
        if r.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(id));
            let t = text.trim();
            let next = if t.is_empty() {
                None
            } else if t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') {
                Some(format!("{t} {unit}"))
            } else {
                Some(t.to_string())
            };
            let zero = next
                .as_deref()
                .is_some_and(|d| offset_of(host, d).is_ok_and(|v| v == 0.0));
            let next = next.filter(|_| !zero);
            if next != sketch.offset {
                match next.as_deref().map(|d| offset_of(host, d)) {
                    Some(Err(e)) => *error = Some(format!("この値では置けません: {e}")),
                    _ => {
                        *error = None;
                        out = Some(Sketch {
                            offset: next,
                            ..sketch.clone()
                        });
                    }
                }
            }
        }
    });
    ui.separator();
    out
}

/// 面の上のスケッチの原点の欄(面の 4 隅か円の中心、世界の原点を写した点から選ぶ。図の中で角を
/// クリックして選ぶこともできる)。替えたら、描いた形を世界の中で動かさずに原点を替えたスケッチを返す。
fn origin_row(
    ui: &mut egui::Ui,
    sketch: &Sketch,
    editor: &mut view::Editor,
    o: &FaceOrigins,
    now: &kmb_core::Plane,
) -> Option<(Sketch, [f64; 2])> {
    use kmb_sketch::{Corner, FaceOrigin, SketchPlane};
    let SketchPlane::Face { origin, .. } = &sketch.plane else {
        return None;
    };
    let corners: Vec<(Corner, &kmb_sketch::FaceSpot)> = Corner::ALL
        .iter()
        .filter_map(|c| Some((*c, kmb_sketch::corner_spot(&o.spots, &o.base, *c)?)))
        .filter(|(_, s)| matches!(s.origin, FaceOrigin::Vertex { .. }))
        .collect();
    let center = o
        .spots
        .iter()
        .find(|s| matches!(s.origin, FaceOrigin::Center { .. }));
    let current = match origin {
        FaceOrigin::World => "世界の原点を写した点".to_string(),
        _ => match corners.iter().find(|(_, s)| s.origin == *origin) {
            Some((c, _)) => c.label().to_string(),
            None if center.is_some_and(|s| s.origin == *origin) => "円の中心".into(),
            None if o.spots.iter().any(|s| s.origin == *origin) => "選んだ角".into(),
            None => "見つからない角(左下の角を使用中)".into(),
        },
    };
    let mut pick: Option<(FaceOrigin, [f64; 3])> = None;
    ui.horizontal(|ui| {
        ui.label("原点");
        egui::ComboBox::from_id_salt("sketch_origin")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (c, s) in &corners {
                    if ui
                        .selectable_label(s.origin == *origin, c.label())
                        .clicked()
                    {
                        pick = Some((s.origin.clone(), s.at));
                    }
                }
                if let Some(s) = center
                    && ui
                        .selectable_label(s.origin == *origin, "円の中心")
                        .clicked()
                {
                    pick = Some((s.origin.clone(), s.at));
                }
                if ui
                    .selectable_label(origin.is_world(), "世界の原点を写した点")
                    .on_hover_text("2026-10-18 より前に作った面のスケッチと同じ決め方")
                    .clicked()
                {
                    pick = Some((FaceOrigin::World, o.base.origin));
                }
            })
            .response
            .on_hover_text("スケッチの原点(寸法や拘束の基準)。替えても描いた形は動かない");
        if ui
            .add_enabled(
                !o.spots.is_empty(),
                egui::Button::new("角をクリック…").small(),
            )
            .on_hover_text("図の中で、原点にする面の角(角の無い面は円の中心)をクリックする")
            .clicked()
        {
            editor.set_tool(Tool::Origin);
        }
    });
    let (next, at) = pick.filter(|(n, _)| n != origin)?;
    let shift = now.to_local(at);
    Some((sketch.with_origin(next, shift)?, shift))
}

/// スケッチの平面の名前(左上の題に出す)。
fn plane_label(plane: &kmb_sketch::SketchPlane) -> &'static str {
    use kmb_sketch::SketchPlane as P;
    match plane {
        P::Xy => "XY 平面",
        P::Yz => "YZ 平面",
        P::Zx => "ZX 平面",
        P::Face { .. } => "面の上",
        P::Datum { .. } => "作業平面の上",
    }
}

/// スケッチより前にできて残っているボディ(面の上のスケッチなら、その面のボディも)。
/// ブラウザの目で隠したボディは入れない(描かず、吸い付かない)。
fn bodies_before(
    host: &EditorHost<'_>,
    entry: &FeatureEntry,
    sketch: &Sketch,
) -> Vec<(kmb_core::FeatureId, kmb_core::ShapeHandle)> {
    let mut bodies = host.bodies_before(entry.id);
    if let kmb_sketch::SketchPlane::Face { feature, .. } = &sketch.plane
        && let Some(kmb_core::Value::Shape(h)) = host.output(*feature, "body")
        && !bodies.iter().any(|(f, _)| f == feature)
        && !host.body_hidden(*feature)
    {
        bodies.push((*feature, *h));
    }
    bodies
}

/// 機能 `entry` より前の、隠していないスケッチと、その写せる要素(K8)。
fn sketches_before(host: &EditorHost<'_>, entry: &FeatureEntry) -> SketchesKey {
    host.data
        .features
        .iter()
        .take_while(|f| f.id != entry.id)
        .filter(|f| f.type_id == kmb_sketch::SKETCH && !host.hidden.contains(&f.id))
        .filter_map(|f| {
            let items = host
                .output(f.id, "items")
                .and_then(<kmb_sketch::SketchItems as kmb_core::Payload>::from_value)?;
            Some((f.id, items))
        })
        .collect()
}

/// 機能 `entry` より前のスケッチのうち `ids` のもの(隠していても)と、その写せる要素。
fn sketches_used(
    host: &EditorHost<'_>,
    entry: &FeatureEntry,
    ids: &[kmb_core::FeatureId],
) -> SketchesKey {
    host.data
        .features
        .iter()
        .take_while(|f| f.id != entry.id)
        .filter(|f| f.type_id == kmb_sketch::SKETCH && ids.contains(&f.id))
        .filter_map(|f| {
            let items = host
                .output(f.id, "items")
                .and_then(<kmb_sketch::SketchItems as kmb_core::Payload>::from_value)?;
            Some((f.id, items))
        })
        .collect()
}

/// 文書のスケッチと、解けた形(どちらかが読めなければ `None`)。
fn sketch_of(host: &EditorHost<'_>, entry: &FeatureEntry) -> Option<(Sketch, Solution)> {
    let sketch: Sketch = serde_json::from_value(entry.data.clone()).ok()?;
    let values = dimension_values(host, &sketch)?;
    let sketch = sketch.evaluated_patterns(&values).ok()?;
    let solution = host
        .output(entry.id, "sketch")
        .and_then(kmb_sketch::solution_of)?;
    Some((sketch, solution))
}

fn to_value(s: Sketch) -> serde_json::Value {
    serde_json::to_value(s).expect("JSON にできる")
}

/// リボンの「作成」「修正」の道具 1 つ。表の並びが ▾ の一覧の並び。
struct Entry {
    tool: Tool,
    label: &'static str,
    icon: &'static str,
    /// 既定で帯の前に出す順(1 から。0 は ▾ の一覧だけ)
    front: u8,
    /// ▾ の一覧で入れる親
    sub: Option<&'static str>,
    /// ▾ の一覧で前に区切りの線を引く
    sep: bool,
}

const fn e(tool: Tool, label: &'static str, icon: &'static str, front: u8) -> Entry {
    Entry {
        tool,
        label,
        icon,
        front,
        sub: None,
        sep: false,
    }
}

const fn under(parent: &'static str, x: Entry) -> Entry {
    Entry {
        sub: Some(parent),
        ..x
    }
}

const fn sep(x: Entry) -> Entry {
    Entry { sep: true, ..x }
}

/// 描く(作成メニューの分け方: 種類ごとに親を作り、作り方を子に並べる。
/// ミラーと並べる道具、投影と交差(「投影/取り込み ▸」)も作成メニューに入る)。前に出すのは
/// 線分・長方形・円・長穴・ミラー(Kie 2026-10-21 の画板)。
const DRAW: [Entry; 33] = [
    e(Tool::Line, "線分", "line", 1),
    e(Tool::MidpointLine, "中点の線分", "midline", 0),
    under("長方形", e(Tool::Rectangle, "2 点指定の長方形", "rect", 2)),
    under(
        "長方形",
        e(Tool::ThreePointRectangle, "3 点指定の長方形", "rect3", 0),
    ),
    under(
        "長方形",
        e(Tool::CenterRectangle, "中心指定の長方形", "crect", 0),
    ),
    under(
        "長方形",
        e(Tool::RoundedRectangle, "角の丸い長方形", "rrect", 0),
    ),
    under("円", e(Tool::Circle, "中心と直径の円", "circle", 3)),
    under("円", e(Tool::TwoPointCircle, "2 点の円", "circle2", 0)),
    under("円", e(Tool::ThreePointCircle, "3 点の円", "circle3", 0)),
    under(
        "円",
        e(Tool::TwoTangentCircle, "2 つに接する円", "tcircle2", 0),
    ),
    under("円", e(Tool::TangentCircle, "3 つに接する円", "tcircle", 0)),
    under("円弧", e(Tool::ThreePointArc, "3 点の円弧", "arc3", 0)),
    under("円弧", e(Tool::Arc, "中心点円弧", "arc", 0)),
    under("円弧", e(Tool::TangentArc, "接線円弧", "tarc", 0)),
    under(
        "多角形",
        e(Tool::OuterPolygon, "外接の多角形", "polyout", 0),
    ),
    under("多角形", e(Tool::Polygon, "内接の多角形", "polygon", 0)),
    under("多角形", e(Tool::EdgePolygon, "辺の多角形", "polyedge", 0)),
    e(Tool::Ellipse, "楕円", "ellipse", 0),
    under("長穴", e(Tool::Slot, "中心と中心の長穴", "slot", 4)),
    under("長穴", e(Tool::OverallSlot, "全長の長穴", "slotov", 0)),
    under("長穴", e(Tool::CenterSlot, "中心からの長穴", "slotc", 0)),
    under(
        "長穴",
        e(Tool::ThreePointArcSlot, "3 点円弧の長穴", "slotarc3", 0),
    ),
    under("長穴", e(Tool::ArcSlot, "円弧の長穴", "slotarc", 0)),
    under(
        "スプライン",
        e(Tool::FitSpline, "通る点のスプライン", "spline", 0),
    ),
    under(
        "スプライン",
        e(Tool::ControlSpline, "制御点のスプライン", "splinectl", 0),
    ),
    e(Tool::Conic, "円錐曲線", "conic", 0),
    e(Tool::Point, "点", "point", 0),
    e(Tool::Text, "文字", "textf", 0),
    sep(e(Tool::MirrorAxis, "ミラー", "skmirror", 5)),
    e(Tool::CircularPattern, "円形状パターン", "circpat", 0),
    e(Tool::LinearPattern, "矩形状パターン", "linpat", 0),
    sep(under(
        "投影/取り込み",
        e(Tool::Project, "投影", "project", 0),
    )),
    under("投影/取り込み", e(Tool::Intersect, "交差", "intersect", 0)),
];

/// 直す(修正メニューの順。移動と回転は「移動/コピー」、選んだ物とスケッチ全体の
/// 拡大縮小は「尺度」、オフセットの向きの決め打ちは「オフセット」の子にまとめる)。前に出すのは
/// トリム・フィレット・オフセット(Kie 2026-10-21 の画板)。
const MODIFY: [Entry; 10] = [
    e(Tool::Fillet, "フィレット", "skfillet", 2),
    e(Tool::Chamfer, "面取り", "skchamfer", 0),
    e(Tool::Trim, "トリム", "trim", 1),
    e(Tool::Extend, "延長", "extend", 0),
    e(Tool::Break, "破断", "skbreak", 0),
    under("尺度", e(Tool::Scale, "拡大縮小", "skscale", 0)),
    under("オフセット", e(Tool::Offset, "オフセット", "skoffset", 3)),
    e(Tool::Blend, "ブレンド曲線", "blend", 0),
    under("移動/コピー", e(Tool::Move, "移動", "skmove", 0)),
    under("移動/コピー", e(Tool::Rotate, "回転", "skrotate", 0)),
];

/// 道具の名前(右クリックの「繰り返し」に出す)。
pub(crate) fn tool_label(t: Tool) -> &'static str {
    match t {
        Tool::Select => "選ぶ",
        Tool::Dimension => "スケッチ寸法",
        Tool::Measure => "計測",
        Tool::Origin => "原点を選ぶ",
        Tool::Constraint(name) => name,
        _ => DRAW
            .iter()
            .chain(MODIFY.iter())
            .find(|d| d.tool == t)
            .map_or("道具", |d| d.label),
    }
}

/// 道具のアイコン(リボンと同じ絵。右クリックのメニューの行に使う)。
pub(crate) fn tool_icon(t: Tool) -> &'static str {
    match t {
        Tool::Select => "select",
        Tool::Dimension => "dim",
        Tool::Measure => "measure",
        Tool::Origin => "origin",
        Tool::Constraint(name) => constraint_icon(name),
        _ => DRAW
            .iter()
            .chain(MODIFY.iter())
            .find(|d| d.tool == t)
            .map_or("", |d| d.icon),
    }
}

/// 拘束の名前(帯のボタンの名前)のアイコン。知らない名前は空。
pub(crate) fn constraint_icon(name: &str) -> &'static str {
    match name {
        "水平" => return "horiz",
        "垂直" => return "vert",
        _ => {}
    }
    constraints_ui::GEOMETRIC
        .iter()
        .find(|g| g.0 == name)
        .map_or("", |g| g.1)
}

fn tool_id(t: Tool) -> String {
    format!("tool:{t:?}")
}

/// 道具を、既定で前に出す順(1 から。0 は ▾ の一覧だけ)の付いた並びから、リボンのまとまりにする。
/// ▾ の一覧は渡した並びのまま(親子の分け方もそのまま)。
fn ranked_group(name: &str, items: Vec<(EditorTool, u8)>) -> ToolGroup {
    let order = items.iter().map(|(t, _)| t.id.clone()).collect();
    let mut front: Vec<&(EditorTool, u8)> = items.iter().filter(|(_, r)| *r > 0).collect();
    front.sort_by_key(|(_, r)| *r);
    ToolGroup {
        name: name.into(),
        tools: front.into_iter().map(|(t, _)| t.clone()).collect(),
        more: items
            .iter()
            .filter(|(_, r)| *r == 0)
            .map(|(t, _)| t.clone())
            .collect(),
        order,
        ..Default::default()
    }
}

impl SketchEditor {
    /// 選んだ物のうち、まだ写していないボディの辺や角を写したスケッチと解。写す物がなければ
    /// そのまま。選びは写した後の番号へ付け替える(パネルで何も付けなければ呼ぶ側が戻す)。
    fn with_selected_refs(
        &mut self,
        sketch: &Sketch,
        sol: &Solution,
        values: Option<&view::DimValues>,
    ) -> (Sketch, Solution) {
        let same = || (sketch.clone(), sol.clone());
        let virt: Vec<kmb_sketch::EntityId> = self
            .editor
            .selection
            .iter()
            .copied()
            .filter(|id| sketch.entity(*id).is_none())
            .collect();
        let (Some((_, refs)), Some(values), false) = (self.refs.as_ref(), values, virt.is_empty())
        else {
            return same();
        };
        let aug = snap::augment(sketch, sol, refs, view::snap_wants(self.editor.tool));
        let keep: Vec<kmb_sketch::EntityId> = virt
            .into_iter()
            .filter(|id| aug.kind(*id).is_some())
            .collect();
        if keep.is_empty() {
            return same();
        }
        let (s, moved) = aug.materialize(&keep);
        let Ok(sol2) = kmb_sketch::solve(&s, values) else {
            return same();
        };
        self.editor.renumber(&moved);
        (s, sol2)
    }

    /// 元が見つからない投影の欄(網羅版 K4)。直したスケッチを返す。
    fn lost_projections_panel(
        &mut self,
        ui: &mut egui::Ui,
        sketch: &Sketch,
        sol: &Solution,
    ) -> Option<Sketch> {
        let refs = &self.refs.as_ref()?.1;
        let linked = self.linked.as_ref().map_or(&[][..], |(_, l)| l.as_slice());
        let lost: Vec<kmb_sketch::EntityId> = sketch
            .projections
            .iter()
            .filter(|p| !refs.iter().chain(linked).any(|r| r.source.same(&p.source)))
            .map(|p| p.entity)
            .collect();
        if lost.is_empty() {
            return None;
        }
        let mut out = None;
        ui.colored_label(
            kmb_ui::theme::look(ui.ctx()).pal.inks.sketch_note,
            format!("元が見つからない投影 {} 個(前の位置のまま)", lost.len()),
        );
        ui.horizontal_wrapped(|ui| {
            if ui.button("選ぶ").clicked() {
                self.editor.selection = lost.iter().copied().collect();
            }
            if ui
                .button("投影をやめる")
                .on_hover_text("元とのつながりを切り、今の位置で固定した普通の線にする")
                .clicked()
            {
                let mut s = sketch.clone();
                s.adopt_solution(sol);
                s.projections.retain(|p| !lost.contains(&p.entity));
                out = Some(menu::set_fixed(&s, &lost, true));
            }
            if ui.button("削除").clicked() {
                let mut s = sketch.clone();
                s.adopt_solution(sol);
                s.remove_entities(&lost);
                self.editor.selection.retain(|id| !lost.contains(id));
                out = Some(s);
            }
        });
        ui.separator();
        out
    }

    /// スケッチを終えて押し出しを始めるよう画面に頼む。選んだ領域があれば、それを押し出す
    /// (なければ、領域が 1 つならそれ、2 つ以上なら 3D のビューで選ぶ)。
    fn request_extrude(&mut self, host: &mut EditorHost<'_>, entry: &FeatureEntry) {
        let mut args = serde_json::json!({ "profile": entry.id.0 });
        if !self.editor.regions.is_empty() {
            args["regions"] = self
                .editor
                .regions
                .iter()
                .map(|k| serde_json::json!({ "profile": entry.id.0, "key": k }))
                .collect();
        }
        self.editor.regions.clear();
        // 押し出しの操作は名前でなく役割で頼む(名前を変えても押し出しの側だけを直せばよい)
        *host.then = Some((
            kmb_ui::APP_RUN.into(),
            serde_json::json!({ "role": kmb_interchange::role::action::EXTRUDE, "finish": true, "args": args }),
        ));
    }
}

impl FeatureEditor for SketchEditor {
    fn fresh(&self) -> Box<dyn FeatureEditor> {
        Box::new(SketchEditor::new())
    }

    fn handles(&self, type_id: &str) -> bool {
        type_id == SKETCH
    }

    fn takes_view(&self) -> bool {
        true
    }

    fn view_camera(&self) -> Option<(kmb_ui::view3d::Camera, egui::Rect)> {
        self.view.seen
    }

    fn view(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        self.editor.fmt = kmb_sketch::dim_format(host.registry);
        let Some((sketch, solution)) = sketch_of(host, entry) else {
            let why = host
                .failure(entry.id)
                .unwrap_or_else(|| "表示できるスケッチではありません".into());
            self.show_diagnosis(host, entry.id, Some(why.clone()));
            ui.colored_label(ui.visuals().error_fg_color, why);
            return None;
        };
        self.show_diagnosis(
            host,
            entry.id,
            kmb_sketch::inconsistency_reason(&sketch, &solution),
        );
        let values = dimension_values(host, &sketch);
        // 原点を選ぶ道具: 面の角(円の中心)を、今のスケッチの平面の座標で
        let origins: Vec<([f64; 2], kmb_sketch::FaceOrigin)> =
            match sketch_plane(host, &sketch.plane) {
                Some(plane) if self.editor.tool == Tool::Origin => self
                    .face_origins(host, &sketch)
                    .map(|o| {
                        o.spots
                            .into_iter()
                            .map(|s| (plane.to_local(s.at), s.origin))
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
        // 面の上なら、置いた面の輪郭を薄く下に描く(どこに描いているか分かるように)
        let on_face = match &sketch.plane {
            kmb_sketch::SketchPlane::Face { feature, face, .. } => {
                match (
                    host.output(*feature, "body"),
                    sketch_plane(host, &sketch.plane),
                ) {
                    (Some(kmb_core::Value::Shape(h)), Some(plane)) => {
                        Some((*h, face.clone(), plane))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        // 面の縁を投影してあれば(面にスケッチを作った時の自動の投影。K7)、網の縁は描かない(二重にしない)
        let projected_outline = |face: &str| {
            sketch.projections.iter().any(|p| {
                matches!(&p.source, kmb_sketch::ProjectionSource::Edge { faces, .. } if faces.iter().any(|f| f == face))
            })
        };
        let underlay = match on_face.filter(|k| !projected_outline(&k.1)) {
            Some(key) => {
                if self.outline.as_ref().is_none_or(|(k, _)| *k != key) {
                    let lines = face_outline(host.kernel.as_ref(), key.0, &key.1, &key.2);
                    self.outline = Some((key, lines));
                }
                self.outline.as_ref().map_or(&[][..], |(_, l)| l.as_slice())
            }
            None => &[],
        };
        // 原点と、前にできた形の角・辺・円の中心(吸い付く先と寸法の相手)
        let plane = sketch_plane(host, &sketch.plane).map(|p| p.offset(offset_mm(host, &sketch)));
        // 同じ平面に前から置いた下絵(キャンバス。隠した物は描かない)
        let pictures: Vec<kmb_core::OverlayPicture> = match &plane {
            Some(pl) => host
                .data
                .features
                .iter()
                .take_while(|f| f.id != entry.id)
                .filter(|f| !f.suppressed && !host.hidden.contains(&f.id))
                .filter_map(|f| host.output(f.id, kmb_core::PICTURE))
                .filter_map(|v| kmb_ui::picture_ui::overlay(ui.ctx(), v, Some(pl)))
                .collect(),
            None => Vec::new(),
        };
        let refs: &[kmb_sketch::RefItem] = match plane {
            Some(plane) => {
                let key = (
                    (plane, bodies_before(host, entry, &sketch)),
                    sketches_before(host, entry),
                );
                if self.refs.as_ref().is_none_or(|(k, _)| *k != key) {
                    let mut items = vec![kmb_sketch::origin_ref(&plane)];
                    for (f, h) in &key.0.1 {
                        items.extend(kmb_sketch::body_refs(host.kernel.as_ref(), *f, *h, &plane));
                    }
                    // 前のスケッチの線・円・点(寸法・拘束の道具で押すと写す。K8)
                    for (f, it) in &key.1 {
                        items.extend(kmb_sketch::sketch_refs(it, *f, &plane));
                    }
                    self.refs = Some((key, items));
                }
                self.refs.as_ref().map_or(&[][..], |(_, r)| r.as_slice())
            }
            None => &[],
        };
        // 写した辺(平面から遠い面の辺も)・交差・シルエットの元(元が見つからない投影の欄で使う)
        if let Some(plane) = plane {
            let wanted = |src: &kmb_sketch::ProjectionSource| {
                matches!(
                    src,
                    kmb_sketch::ProjectionSource::Section { .. }
                        | kmb_sketch::ProjectionSource::Silhouette { .. }
                )
            };
            let mut used: Vec<kmb_core::FeatureId> = sketch
                .projections
                .iter()
                .filter_map(|p| p.source.feature())
                .collect();
            used.sort();
            used.dedup();
            let used_sketches: Vec<kmb_core::FeatureId> = sketch
                .projections
                .iter()
                .filter_map(|p| p.source.sketch())
                .collect();
            // 元は目を閉じていても元のまま(計算も隠したスケッチ・ボディから取り直す)。吸い付く先の
            // refs と違い、ここは隠した物も入れる
            let key = (
                (
                    plane,
                    kmb_core::visible_bodies_before(
                        host.data,
                        host.registry,
                        host.report,
                        Some(entry.id),
                    )
                    .into_iter()
                    .filter(|(f, _)| used.contains(f))
                    .collect::<Vec<_>>(),
                ),
                sketches_used(host, entry, &used_sketches),
            );
            if self.linked.as_ref().is_none_or(|(k, _)| *k != key) {
                let mut items = Vec::new();
                for (f, h) in &key.0.1 {
                    items.extend(kmb_sketch::body_edge_refs(
                        host.kernel.as_ref(),
                        *f,
                        *h,
                        &plane,
                    ));
                    let meshed = sketch
                        .projections
                        .iter()
                        .any(|p| p.source.feature() == Some(*f) && wanted(&p.source));
                    if meshed && let Ok(mesh) = host.kernel.mesh(*h, kmb_sketch::BODY_MESH_TOL) {
                        let edges = kmb_sketch::body_refs(host.kernel.as_ref(), *f, *h, &plane);
                        items.extend(kmb_sketch::section_refs(&mesh, &plane, *f));
                        items.extend(kmb_sketch::silhouette_refs(&mesh, &plane, *f, &edges));
                    }
                }
                for (f, it) in &key.1 {
                    items.extend(kmb_sketch::sketch_refs(it, *f, &plane));
                }
                self.linked = Some((key, items));
            }
        }
        // スライス・交差の道具: 前にできたボディの断面(平面とボディが変わった時だけ作り直す)
        let wants_sections = self.editor.display.slice || self.editor.tool == Tool::Intersect;
        let sections: &[view::Section] = match plane.filter(|_| wants_sections) {
            Some(plane) => {
                let key = (plane, bodies_before(host, entry, &sketch));
                if self.sections.as_ref().is_none_or(|(k, _)| *k != key) {
                    let list = key
                        .1
                        .iter()
                        .filter_map(|(f, h)| {
                            let mesh = host.kernel.mesh(*h, kmb_sketch::BODY_MESH_TOL).ok()?;
                            let segs: Vec<[[f64; 2]; 2]> =
                                kmb_sketch::section_segments(&mesh, &plane)
                                    .into_iter()
                                    .map(|(s, _)| s)
                                    .collect();
                            (!segs.is_empty()).then(|| view::Section {
                                items: kmb_sketch::section_refs(&mesh, &plane, *f),
                                segs,
                            })
                        })
                        .collect();
                    self.sections = Some((key, list));
                }
                self.sections
                    .as_ref()
                    .map_or(&[][..], |(_, s)| s.as_slice())
            }
            None => &[],
        };
        // 投影の道具: 前にできたボディの網(面を選ぶ)と、シルエットを写す時はその外形
        let bodies: &[view::BodyMesh] = match plane.filter(|_| self.editor.tool == Tool::Project) {
            Some(plane) => {
                let key = (plane, bodies_before(host, entry, &sketch));
                if self.bodies.as_ref().is_none_or(|(k, _)| *k != key) {
                    let list = key
                        .1
                        .iter()
                        .filter_map(|(f, h)| {
                            Some(view::BodyMesh {
                                feature: *f,
                                mesh: host.kernel.mesh(*h, kmb_sketch::BODY_MESH_TOL).ok()?,
                                plane,
                                edges: kmb_sketch::body_edge_refs(
                                    host.kernel.as_ref(),
                                    *f,
                                    *h,
                                    &plane,
                                ),
                                silhouette: None,
                            })
                        })
                        .collect();
                    self.bodies = Some((key, list));
                }
                let list = &mut self.bodies.as_mut().expect("作った").1;
                if self.editor.project_pick == view::ProjectPick::Silhouette {
                    for b in list.iter_mut().filter(|b| b.silhouette.is_none()) {
                        b.silhouette = Some(kmb_sketch::silhouette_refs(
                            &b.mesh, &b.plane, b.feature, refs,
                        ));
                    }
                }
                list.as_slice()
            }
            None => &[],
        };
        let place = match sketch.offset.as_deref() {
            Some(d) => format!("{}から {d}", plane_label(&sketch.plane)),
            None => plane_label(&sketch.plane).to_string(),
        };
        let title = format!(
            "{} ・ {place} ・ {}",
            entry.name,
            view::status_text(&sketch, &solution)
        );
        self.editor.feature = Some(entry.id);
        // 前にできたボディを描く。3D の視点でも平らな図(正面の図)でも描く(ブラウザの目で隠した物は
        // 描かない。3D スケッチと同じ)。置いた面のボディを足す `bodies_before` は使わない(後の機能が
        // 受け継いだ同じボディと重なって面がちらつく)。見えているボディだけを描く
        let shown = host.bodies_before(entry.id);
        self.scene.update(host.kernel.as_ref(), &shown);
        let mut changed = view::sketch_view(
            ui,
            &mut self.view,
            &mut self.editor,
            &sketch,
            &solution,
            values.as_ref(),
            view::Backdrop {
                title: &title,
                underlay,
                pictures: &pictures,
                refs,
                sections,
                bodies,
                origins: &origins,
                plane,
                scene: &self.scene,
                projection: host.projection,
            },
        );
        if std::mem::take(&mut self.editor.wants_extrude) {
            self.request_extrude(host, entry);
        }
        // 右クリックの「元に戻す」などは画面に頼む(直すのは終えない)
        if let Some(a) = self.editor.app_ask.take() {
            *host.then = Some((a.into(), serde_json::json!({})));
        }
        if let Some(role) = self.editor.run_action.take() {
            *host.then = Some((kmb_ui::APP_RUN.into(), serde_json::json!({ "role": role })));
        }
        // 文字の道具で置いた点: スケッチを終えて、その点を基準に文字を作る(K32)
        if std::mem::take(&mut self.editor.wants_text)
            && let Some(point) = changed.as_ref().and_then(|s| new_lone_point(&sketch, s))
        {
            self.editor.set_tool(Tool::Select);
            *host.then = Some((
                kmb_ui::APP_RUN.into(),
                serde_json::json!({
                    "role": kmb_interchange::role::action::TEXT_AT_POINT,
                    "finish": true,
                    "args": { "point": point.0 },
                }),
            ));
        }
        // Delete で選んだ要素を消す(ほかの欄に入力している時は除く)
        if changed.is_none()
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
            && !ui.ctx().egui_wants_keyboard_input()
        {
            changed = view::remove_picked_constraint(&mut self.editor, &sketch, &solution).or_else(
                || constraints_ui::selection_action(&mut self.editor, &sketch, &solution, "delete"),
            );
        }
        // `名前=値` と打った寸法は、先にパラメータを作ってから次の更新で置く
        if let Some((id, s)) = self.deferred.take()
            && id == entry.id
        {
            return Some(to_value(s));
        }
        changed
            .and_then(|s| {
                defer_with_param(
                    host,
                    &sketch,
                    s,
                    &mut self.editor,
                    &mut self.deferred,
                    entry.id,
                )
            })
            .map(to_value)
    }

    fn panel(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        self.editor.fmt = kmb_sketch::dim_format(host.registry);
        let (sketch, solution) = sketch_of(host, entry)?;
        let origins = self.face_origins(host, &sketch);
        if let Some(s) = plane_section(
            ui,
            host,
            &sketch,
            &mut self.editor,
            &mut self.view,
            origins.as_ref(),
        ) {
            return Some(to_value(s));
        }
        // 自動拘束の予告の間は、その欄だけを出す(K18)
        if self.editor.auto.is_some() {
            let values = dimension_values(host, &sketch);
            return auto_ui::panel(ui, &mut self.editor, &sketch, values.as_ref()).map(to_value);
        }
        // 動かす・回す・拡大縮小: 写しを作るか、つながりを切るか
        // 投影の道具: 写し方
        if self.editor.tool == Tool::Project {
            use view::ProjectPick as P;
            ui.label("写す物");
            let pick = &mut self.editor.project_pick;
            ui.radio_value(
                pick,
                P::Parts,
                "面・辺・点(押した角・辺・円の中心、面なら面の縁)",
            );
            ui.radio_value(
                pick,
                P::Silhouette,
                "ボディのシルエット(押したボディの外形)",
            );
            ui.radio_value(pick, P::Body, "ボディの辺と角すべて");
            ui.checkbox(
                &mut self.editor.project_link,
                "投影リンク(元の形が変わると付いて行く)",
            );
            ui.separator();
        }
        // 交差の道具: 写す物とつながり
        if self.editor.tool == Tool::Intersect {
            ui.label("写す物");
            let body = &mut self.editor.intersect_body;
            ui.radio_value(body, false, "押した線(面ごとの断面)");
            ui.radio_value(body, true, "ボディ全体(押したボディの断面すべて)");
            ui.checkbox(
                &mut self.editor.intersect_link,
                "投影リンク(ボディの形が変わると付いて行く)",
            );
            ui.separator();
        }
        // 元が見つからない投影
        if let Some(s) = self.lost_projections_panel(ui, &sketch, &solution) {
            return Some(to_value(s));
        }
        // 制御点のスプライン: 次数(既定は 5)
        if self.editor.tool == Tool::ControlSpline {
            ui.horizontal(|ui| {
                ui.label("次数");
                ui.radio_value(&mut self.editor.spline_degree, 3, "3")
                    .on_hover_text("3 次: 接線までそろう");
                ui.radio_value(&mut self.editor.spline_degree, 5, "5")
                    .on_hover_text("5 次: 曲率までそろい、見た目の面に効く(制御点が 5 つ以下なら点の数 - 1 の次数)");
            });
            ui.separator();
        }
        // ブレンド曲線: つなぎ方
        if self.editor.tool == Tool::Blend {
            ui.horizontal(|ui| {
                ui.label("つなぎ方");
                ui.radio_value(&mut self.editor.blend_g1, true, "接する(G1)");
                ui.radio_value(&mut self.editor.blend_g1, false, "曲率もそろう(G2)");
            });
            ui.separator();
        }
        // オフセットの向きと角の作り方、多角形の辺の数
        group_ui::offset_tool(ui, &mut self.editor);
        group_ui::polygon_tool(ui, &mut self.editor);
        if matches!(self.editor.tool, Tool::Move | Tool::Rotate | Tool::Scale) {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.editor.copy_mode, "写しを作る(元を残す)");
                ui.add_enabled(
                    !self.editor.copy_mode,
                    egui::Checkbox::new(&mut self.editor.detach, "つながりを切って動かす"),
                );
            });
            ui.separator();
        }
        // パターンの元か写しを選んでいれば、パターンの欄(後から直せる。拘束の欄より上に出す)
        let values = dimension_values(host, &sketch);
        let params = host.param_values();
        let patterned = pattern_ui::panel(ui, &mut self.editor, &sketch, &solution, &|n| {
            params.get(n).copied()
        })
        .or_else(|| group_ui::polygon_panel(ui, &mut self.editor, &sketch, &solution))
        .and_then(|s| refuse_broken_dims(host, &sketch, s, &mut self.editor))
        .map(to_value);
        // 選んだボディの辺や角(まだ写していない物)は、ここで仮に写してパネルの相手にする。
        // パネルで何かを付けた時だけ文書に入る(網羅版 G10)
        let saved = self.editor.selection.clone();
        let (sketch, solution) = self.with_selected_refs(&sketch, &solution, values.as_ref());
        let out = constraints_ui::constraint_panel(
            ui,
            &mut self.editor,
            &sketch,
            &solution,
            values.as_ref(),
        )
        .and_then(|s| {
            defer_with_param(
                host,
                &sketch,
                s,
                &mut self.editor,
                &mut self.deferred,
                entry.id,
            )
        })
        .map(to_value);
        if out.is_none() {
            self.editor.selection = saved;
        }
        // 線種・3D スケッチの案内・図に出す物、刻みへの吸い付き、選べる物と選び方
        // 開いた端と位置だけで接する端の数、「端をつなぐ」(K25)
        let tied =
            ends_ui::row(ui, &mut self.editor, &sketch, &solution, values.as_ref()).map(to_value);
        ui.separator();
        let linetype = display::options(ui, &mut self.editor, &sketch, &solution).map(to_value);
        display::panel(ui, &mut self.editor);
        patterned.or(out).or(tied).or(linetype)
    }

    fn panel_title(&self) -> Option<(&'static str, &'static str)> {
        Some(("拘束と寸法", "dim"))
    }

    fn tools(&self, host: &EditorHost<'_>, entry: &FeatureEntry) -> Vec<ToolGroup> {
        let cur = self.editor.tool;
        let tool = |t: Tool, label: &str, icon: &str| {
            let hint = match view::tool_key(t) {
                Some(k) => format!("{}({k})", t.hint(0)),
                None => t.hint(0).to_string(),
            };
            EditorTool::new(&tool_id(t), label, icon, &hint).on(cur == t)
        };
        let entry_tool = |d: &Entry| {
            (
                tool(d.tool, d.label, d.icon).under(d.sub).sep(d.sep),
                d.front,
            )
        };
        let draw = ranked_group("作成", DRAW.iter().map(entry_tool).collect());
        let has_sel = sketch_of(host, entry)
            .is_some_and(|(s, _)| !constraints_ui::selected(&self.editor, &s).is_empty());
        let act = |id: &str, label: &str, icon: &str, hint: &str| {
            let mut t = EditorTool::new(id, label, icon, hint);
            t.enabled = has_sel;
            t
        };
        // 表の道具の間に、選んだ物への操作を修正メニューの位置で差し込む
        let mut modify: Vec<(EditorTool, u8)> = Vec::new();
        for d in &MODIFY {
            modify.push(entry_tool(d));
            match d.tool {
                Tool::Scale => modify.push((
                    EditorTool::new(
                        "sketch_scale",
                        "スケッチの尺度",
                        "skscale",
                        "スケッチ全体を拡大縮小する(全部を選んで拡大縮小の道具を始める)",
                    )
                    .under(Some("尺度")),
                    0,
                )),
                Tool::Offset => modify.extend(
                    [
                        act(
                            "act:offset_out",
                            "外へオフセット",
                            "skoffset",
                            "つながった線を、寸法の値(空なら 2 mm)だけ外へずらして写す",
                        ),
                        act(
                            "act:offset_in",
                            "内へオフセット",
                            "skoffin",
                            "つながった線を、寸法の値(空なら 2 mm)だけ内へずらして写す",
                        ),
                        act(
                            "act:offset_both",
                            "両側へオフセット",
                            "skoffboth",
                            "つながった線を、寸法の値(空なら 2 mm)だけ外と内の両側へずらして写す",
                        ),
                    ]
                    .map(|t| (t.under(Some("オフセット")), 0)),
                ),
                _ => {}
            }
        }
        modify.extend([
            (
                EditorTool::new(
                    "params",
                    "パラメータを変更",
                    "param",
                    "パラメータの一覧を開く(寸法の値に名前を使える)",
                ),
                0,
            ),
            (
                act(
                    "act:construction",
                    "補助線の切り替え",
                    "dashline",
                    "選んだ線を補助線にする(もう一度で戻す)",
                )
                .sep(true),
                0,
            ),
            (
                act("act:delete", "削除", "trash", "選んだ要素を消す(Delete)"),
                0,
            ),
        ]);
        let modify = ranked_group("修正", modify);
        let constraint = |(label, icon, what): &(&'static str, &str, &str)| {
            EditorTool::new(
                &format!("c:{label}"),
                label,
                icon,
                &format!(
                    "{label}: 押してから{what}、順にクリック(続けて付けられる)。選んでから押してもよい"
                ),
            )
            .on(cur == Tool::Constraint(label))
        };
        // 拘束: 前に 4 つ、残りの 8 つは 2 段の小さなボタン、▾ の一覧だけに 3 つと自動拘束
        // (Kie 2026-10-21 の画板。全部を大きくすると 1280 幅に入らない)
        let named = |names: &[&str]| -> Vec<EditorTool> {
            names
                .iter()
                .filter_map(|n| constraints_ui::GEOMETRIC.iter().find(|g| g.0 == *n))
                .map(constraint)
                .collect()
        };
        let mut more = named(&constraints_ui::CONSTRAINTS_MORE);
        more.push(
            EditorTool::new(
                "auto_constrain",
                "自動拘束",
                "autocons",
                "足りない拘束と寸法の候補を予告し、選んで付ける(選んだ線だけ。点を 1 つ選べば、それを位置の基準にする)",
            )
            .sep(true),
        );
        vec![
            draw,
            modify,
            ToolGroup {
                name: "拘束".into(),
                tools: named(&constraints_ui::CONSTRAINTS_FRONT),
                small: named(&constraints_ui::CONSTRAINTS_SMALL),
                more,
                ..Default::default()
            },
            ToolGroup {
                name: "寸法".into(),
                tools: vec![tool(Tool::Dimension, "スケッチ寸法", "dim")],
                more: vec![tool(Tool::Measure, "計測", "measure")],
                ..Default::default()
            },
            // スケッチのタブの「挿入」(DXF・SVG を今のスケッチへ足す)
            ToolGroup {
                name: "挿入".into(),
                tools: vec![EditorTool::new(
                    "act:insert_dxf",
                    "DXF を挿入",
                    "dxf",
                    "DXF の線・円弧・円・点を、このスケッチへ足す(ファイルの座標のまま)",
                )],
                more: vec![EditorTool::new(
                    "act:insert_svg",
                    "SVG を挿入",
                    "svg",
                    "SVG の線と曲線を、このスケッチへ足す(曲線は線分と円弧に直す)",
                )],
                ..Default::default()
            },
            ToolGroup {
                name: "立体".into(),
                tools: vec![EditorTool::new(
                    "act:extrude",
                    "押し出し",
                    "extrude",
                    "スケッチを終えて押し出す(E)。領域をクリックで選んでおくと、その領域だけを押し出す",
                )],
                more: vec![],
                ..Default::default()
            },
            // 選ぶ道具はいつも使う道具なので、描くの ▾ の中でなく、右の端に自分のまとまりで
            // 常に出す。▾ の中に置くと、選ぶ道具の時だけ前に
            // 出て、右のまとまりの位置が 1 枚分ずれた(網羅版 L9)
            ToolGroup {
                name: "選択".into(),
                tools: vec![tool(Tool::Select, "選ぶ", "select")],
                more: vec![],
                ..Default::default()
            },
        ]
    }

    fn use_tool(
        &mut self,
        id: &str,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        let all = DRAW.iter().chain(MODIFY.iter()).map(|d| d.tool).chain([
            Tool::Select,
            Tool::Dimension,
            Tool::Measure,
        ]);
        if let Some(t) = all.into_iter().find(|t| tool_id(*t) == id) {
            self.editor.set_tool(t);
            return None;
        }
        if id == "act:extrude" {
            self.request_extrude(host, entry);
            return None;
        }
        // DXF・SVG の挿入は、それぞれのモジュールの操作を役割で探して実行する
        if let Some(role) = match id {
            "act:insert_dxf" => Some(kmb_interchange::role::action::INSERT_DXF),
            "act:insert_svg" => Some(kmb_interchange::role::action::INSERT_SVG),
            _ => None,
        } {
            *host.then = Some((kmb_ui::APP_RUN.into(), serde_json::json!({ "role": role })));
            return None;
        }
        if id == "params" {
            *host.then = Some((kmb_ui::APP_PARAMS.into(), serde_json::json!({})));
            return None;
        }
        if id == "sketch_scale" {
            // スケッチの尺度(網羅版 D8): 写した形以外の全部を選び、拡大縮小の道具を始める
            let (sketch, _) = sketch_of(host, entry)?;
            self.editor.selection = sketch
                .entities
                .iter()
                .map(|e| e.id())
                .filter(|id| !sketch.projections.iter().any(|p| p.entity == *id))
                .collect();
            self.editor.set_tool(Tool::Scale);
            return None;
        }
        let (sketch, solution) = sketch_of(host, entry)?;
        if id == "auto_constrain" {
            let values = dimension_values(host, &sketch);
            auto_ui::start(&mut self.editor, &sketch, &solution, values.as_ref());
            *host.message = self.editor.notice.take();
            return None;
        }
        if let Some(a) = id.strip_prefix("act:") {
            return constraints_ui::selection_action(&mut self.editor, &sketch, &solution, a)
                .map(to_value);
        }
        let name = id.strip_prefix("c:")?;
        let sel = if name == "等しい" {
            constraints_ui::selected_in_order(&self.editor, &sketch)
        } else {
            constraints_ui::selected(&self.editor, &sketch)
        };
        // 図形の固定・図形どうしの対称・ポリゴンは、選んであればすぐ付ける
        let values = dimension_values(host, &sketch);
        if let Some(out) = constraint_tool::try_multi(
            name,
            &mut self.editor,
            &sketch,
            &solution,
            values.as_ref(),
            &sel,
            None,
        ) {
            if out.is_none() {
                *host.message = self.editor.notice.take();
            }
            return out.map(to_value);
        }
        // 水平/垂直は、選んだ線分の向きに近い方を付ける
        let as_name = constraint_tool::resolve(name, &sketch, &solution, &sel);
        let found = constraints_ui::options(&self.editor, &sketch, &solution, &sel, false)
            .into_iter()
            .find(|(n, _)| *n == as_name);
        match found {
            Some((_, k)) => {
                // ほかの拘束で既に決まっていれば足さず、どれで決まっているかを知らせる
                // (足すと「過剰な拘束」になり、何と重なったかが分からない)
                if let Some(values) = dimension_values(host, &sketch)
                    && let Some(by) = kmb_sketch::redundant_with(&sketch, &values, &k)
                {
                    let names: Vec<String> = by
                        .iter()
                        .filter_map(|id| sketch.constraints.iter().find(|c| c.id == *id))
                        .map(kmb_sketch::describe)
                        .collect();
                    *host.message = Some(if names.is_empty() {
                        format!("{as_name}: 写した形などで既に決まっているので付けません")
                    } else {
                        format!(
                            "{as_name}: {} で既に決まっているので付けません(付けると過剰な拘束になります)",
                            names.join("、")
                        )
                    });
                    return None;
                }
                let mut s = sketch;
                s.adopt_solution(&solution);
                s.add_constraint(k);
                Some(to_value(s))
            }
            None => {
                // 選んだ要素で組がそろわない: 拘束の道具を始め、図の要素を順にクリックして付ける。
                // 組の途中になる選びは残し、そうでなければ外す
                let tool = constraint_tool::tool_of(name)?;
                if !constraint_tool::usable(name, &sketch, &sel) {
                    self.editor.selection.clear();
                }
                self.editor.set_tool(tool);
                None
            }
        }
    }

    fn edit_label(&self) -> &'static str {
        "スケッチを編集"
    }

    fn undo_step(&mut self) -> bool {
        // スプラインの描きかけは最後の点だけ戻す(スケッチ全体を戻さない)
        let e = &mut self.editor;
        matches!(e.tool, Tool::FitSpline | Tool::ControlSpline) && e.spline_pts.pop().is_some()
    }

    fn status_hints(&self) -> Vec<(String, String)> {
        let e = &self.editor;
        let mut v: Vec<(&str, &str)> = match e.tool {
            Tool::Select => vec![
                ("左", "選ぶ(Shift で足す・Ctrl で足し外し)"),
                ("左ドラッグ", "動かす・囲んで選ぶ"),
                ("ダブルクリック", "つながった線を選ぶ"),
                if e.selection.is_empty() && e.regions.is_empty() && e.picked_constraint.is_none() {
                    ("Esc 2 回", "スケッチを終了")
                } else {
                    ("Esc", "選びを外す")
                },
            ],
            t if view::draws(t) => vec![
                ("左", "点を置く"),
                ("数字・Tab", "寸法を打って固定"),
                ("Enter", "作る"),
                (
                    "X",
                    if e.draw_construction {
                        "補助線で描くのをやめる"
                    } else {
                        "補助線で描く"
                    },
                ),
                ("Esc", "1 段戻す"),
            ],
            _ => vec![("左", "選ぶ・置く"), ("Esc", "道具を終える")],
        };
        v.extend([
            ("右", "メニュー"),
            ("右ドラッグ", "3D の視点で回す"),
            ("ホイール押し", "動かす"),
            ("ホイール", "拡大縮小(二度押しですべて表示)"),
        ]);
        v.into_iter()
            .map(|(k, d)| (k.to_string(), d.to_string()))
            .collect()
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod auto_ui_tests;
#[cfg(test)]
mod center_circle_tests;
#[cfg(test)]
mod commit_tests;
#[cfg(test)]
mod constraint_ui_tests;
#[cfg(test)]
mod curve_tools_tests;
#[cfg(test)]
mod dim_tests;
#[cfg(test)]
mod display_tests;
#[cfg(test)]
mod draw_more_tests;
#[cfg(test)]
mod draw_tests;
#[cfg(test)]
mod dxf_ui_tests;
#[cfg(test)]
mod group_ui_tests;
#[cfg(test)]
mod intersect_tests;
#[cfg(test)]
mod label_tests;
#[cfg(test)]
mod modify_ui_tests;
#[cfg(test)]
mod ops_tests;
#[cfg(test)]
mod origin_tests;
#[cfg(test)]
mod pattern_ui_tests;
#[cfg(test)]
mod project_tests;
#[cfg(test)]
mod redefine_tests;
#[cfg(test)]
mod sketch_aids_tests;
#[cfg(test)]
mod space_view_tests;
#[cfg(test)]
mod tests;
