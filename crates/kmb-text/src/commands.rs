//! 文字の操作(画面・CLI・MCP に出す): 面・スケッチの点・スケッチの線に文字を置く、文字を線に
//! 分解する、選んだ文字の 3D のビューへの重ね描き(線、外接の枠)。

use crate::{TEXT, Text};
use kmb_core::{
    Action, Arg, Context, Curves, Edit, Extension, Gesture, Handle, Input, LineKind, Outcome,
    Overlay, Place,
};
use kmb_sketch::{SKETCH, Sketch, SketchPlane};

pub const TEXT_ON_FACE: &str = "cad.text_on_face";
pub const TEXT_AT_POINT: &str = "cad.text_at_point";
pub const TEXT_ON_PATH: &str = "cad.text_on_path";
/// 選んでいる文字を普通の線に分解する
pub const EXPLODE_TEXT: &str = "cad.text.explode";
/// 選んでいる文字のフォントを PC の輪郭のフォントに替える(使う字の形を文書に入れる)
pub const TEXT_FONT: &str = "cad.text.font";

const NEED_FACE: &str = "3D のビューで平らな面を選んでから使う";

/// 文字の操作。PC のフォントの一覧を持つ(初めてフォントを探す時に作る)。
#[derive(Debug, Default)]
pub struct TextCommands {
    fonts: std::sync::Mutex<crate::fonts::FontIndex>,
}

fn picked_plane(cx: &Context<'_>) -> Option<SketchPlane> {
    let p = cx.picked?;
    Some(SketchPlane::face(p.feature, p.face.clone()))
}

/// 選んでいるスケッチに単独の点(文字の基準にできる点)があれば、その ID。引数 `point` で点を
/// 決めた時(スケッチの編集器の「文字」)は、その点があればよい(角に一致させた点でもよい)。
fn sketch_with_point(cx: &Context<'_>) -> Option<kmb_core::FeatureId> {
    let id = cx.selected?;
    let sol = cx.output(id, "sketch").and_then(kmb_sketch::solution_of)?;
    match given_point(cx) {
        Some(p) => sol.points.contains_key(&p).then_some(id),
        None => (!sol.lone.is_empty()).then_some(id),
    }
}

/// 引数 `point`(文字の基準にする点の番号)。
fn given_point(cx: &Context<'_>) -> Option<kmb_sketch::EntityId> {
    let p = cx.arg("point")?.as_u64()?;
    Some(kmb_sketch::EntityId(u32::try_from(p).ok()?))
}

/// 選んでいるスケッチの最初の線(文字を沿わせる線。補助線でもよい)と、それに端のつながる線の列。
fn sketch_with_path(cx: &Context<'_>) -> Option<(kmb_core::FeatureId, crate::Path)> {
    let id = cx.selected?;
    let paths = cx.output(id, "paths").and_then(kmb_sketch::paths_of)?;
    let ((_, first), rest) = paths.split_first()?;
    let others: Vec<kmb_core::Segment> = rest.iter().map(|(_, s)| s.clone()).collect();
    let circle = matches!(first, kmb_core::Segment::Circle { .. });
    Some((id, crate::Path::chain(first.clone(), &others, circle).0))
}

/// 選んだ面の中心(面の三角形の、面積で重みをつけた重心。世界座標)。
fn face_center(cx: &Context<'_>) -> Option<[f64; 3]> {
    let p = cx.picked?;
    let Some(kmb_core::Value::Shape(body)) = cx.output(p.feature, "body") else {
        return None;
    };
    let mesh = cx.kernel.mesh(*body, 0.1).ok()?;
    let fi = mesh.face_names.iter().position(|n| *n == p.face)? as u32;
    let (mut sum, mut total) = ([0.0f64; 3], 0.0f64);
    for (t, _) in mesh
        .triangles
        .iter()
        .zip(&mesh.triangle_faces)
        .filter(|(_, f)| **f == fi)
    {
        let v = t.map(|i| mesh.positions[i as usize].map(f64::from));
        let e1 = kmb_core::kernel::sub(v[1], v[0]);
        let e2 = kmb_core::kernel::sub(v[2], v[0]);
        let c = kmb_core::kernel::cross(e1, e2);
        let area = kmb_core::kernel::dot(c, c).sqrt() / 2.0;
        for i in 0..3 {
            sum[i] += area * (v[0][i] + v[1][i] + v[2][i]) / 3.0;
        }
        total += area;
    }
    (total > 0.0).then(|| sum.map(|x| x / total))
}

/// 選んでいる文字(ID、中身)。
fn selected_text(cx: &Context<'_>) -> Option<(kmb_core::FeatureId, Text)> {
    let id = cx.selected?;
    let f = cx.data.feature(id).filter(|f| f.type_id == TEXT)?;
    Some((id, serde_json::from_value(f.data.clone()).ok()?))
}

/// 文字を線に分解する。
/// 今の文字の線(`curves`)を、同じ平面の新しいスケッチの線分・円弧にする。字の中でつながる端は
/// 1 つの点にまとめる。元の文字は抑制する(消さない。抑制を外せば戻る)。文字を使っていた機能は
/// 新しいスケッチを選び直す。
fn explode_text(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let (id, text) = selected_text(cx).ok_or("文字を選んでいません")?;
    let curves = cx
        .output(id, "curves")
        .and_then(Curves::from_value)
        .filter(|c| !c.segments.is_empty())
        .ok_or("文字の線がまだできていません(文字の誤りを直してから使う)")?;
    // 置く平面: 文字の基準の点・沿わせる線のあるスケッチの平面(ずらしも)、なければ文字の平面
    let mut sketch = match text.anchor.or(text.path) {
        Some(host) => {
            let s: Sketch = cx
                .data
                .feature(host)
                .and_then(|f| serde_json::from_value(f.data.clone()).ok())
                .ok_or("文字を置いたスケッチが読めません")?;
            Sketch {
                plane: s.plane,
                offset: s.offset,
                ..Default::default()
            }
        }
        None => Sketch {
            plane: text.plane.clone(),
            ..Default::default()
        },
    };
    let shapes: Vec<kmb_sketch::Shape> = curves
        .segments
        .iter()
        .map(|s| match *s {
            kmb_core::Segment::Line { a, b } => kmb_sketch::Shape::Line { a, b },
            kmb_core::Segment::Arc {
                center,
                radius,
                start,
                end,
            } => kmb_sketch::Shape::Arc {
                center,
                radius,
                start,
                end,
            },
            kmb_core::Segment::Circle { center, radius } => {
                kmb_sketch::Shape::Circle { center, radius }
            }
        })
        .collect();
    let added = sketch.insert_shapes(&shapes, kmb_sketch::Place::default());
    if added.is_empty() {
        return Err("文字の線が短すぎて、分解できる線がありません".into());
    }
    let name = cx
        .data
        .feature(id)
        .map_or_else(|| "文字".to_string(), |f| f.name.clone());
    Ok(Outcome::Change {
        label: "文字を線に分解".into(),
        edits: vec![
            Edit::Insert {
                type_id: SKETCH.into(),
                name: format!("{name} の線"),
                data: serde_json::to_value(&sketch).expect("JSON にできる"),
            },
            Edit::SetSuppressed {
                id,
                suppressed: true,
            },
        ],
    })
}

/// 文字 `t` を PC のフォント `face` にする(同じ書体なら入っている字を残し、足りない字を足す)。
/// フォントに無い字を返す。
pub fn use_face(t: &mut Text, face: &crate::fonts::Face) -> Result<Vec<char>, String> {
    let bytes = std::fs::read(&face.path)
        .map_err(|e| format!("フォント {} を読めません({e})", face.path.display()))?;
    let same = t
        .outline
        .as_ref()
        .is_some_and(|f| f.family == face.family && f.style == face.style);
    let mut font = match t.outline.take() {
        Some(f) if same => f,
        _ => crate::OutlineFont::open(&bytes, face.index)?,
    };
    font.family = face.family.clone();
    font.style = face.style.clone();
    font.label = face.label.clone();
    let absent = font.add_glyphs(&bytes, face.index, &t.text)?;
    t.outline = Some(font);
    Ok(absent)
}

/// 輪郭のフォントの文字に、形の入っていない字を PC の同じフォントから足す。足りる時と 1 本線の
/// 文字は何もしない(`Ok(None)`)。足した時は、フォントに無い字を返す。
pub fn refresh_glyphs(
    t: &mut Text,
    fonts: &mut crate::fonts::FontIndex,
) -> Result<Option<Vec<char>>, String> {
    let Some(f) = &t.outline else {
        return Ok(None);
    };
    if f.missing(&t.text).is_empty() {
        return Ok(None);
    }
    let face = fonts.find(&f.family, &f.style).ok_or_else(|| {
        format!(
            "フォント「{}」がこの PC にありません(新しい字の形を作れません)",
            f.label_or_family()
        )
    })?;
    use_face(t, &face).map(Some)
}

/// 選んでいる文字のフォントを替える。名前が空なら 1 本線に戻す。
fn set_font(
    cx: &Context<'_>,
    fonts: &mut crate::fonts::FontIndex,
) -> Result<Outcome, kmb_core::Problem> {
    let (id, mut text) = selected_text(cx).ok_or("文字を選んでいません")?;
    let name = cx.arg_str("name").unwrap_or("").trim();
    let style = cx.arg_str("style").unwrap_or("").trim();
    let label = if let Some(file) = cx.arg_str("file").filter(|f| !f.trim().is_empty()) {
        let index = cx.arg("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let faces = crate::fonts::faces_in(std::path::Path::new(file))?;
        let face = faces
            .into_iter()
            .find(|f| f.index == index)
            .ok_or_else(|| format!("{file} に {index} 番目の書体がありません"))?;
        let absent = use_face(&mut text, &face)?;
        font_message(&face.label, &absent)
    } else if name.is_empty() || name == "1 本線" {
        text.outline = None;
        "フォントを 1 本線に戻しました".to_string()
    } else {
        let found = fonts.find(name, style);
        let face = found.ok_or_else(|| {
            let close: Vec<String> = fonts
                .faces()
                .iter()
                .filter(|f| {
                    let low = name.to_lowercase();
                    f.family.to_lowercase().contains(&low) || f.label.contains(name)
                })
                .take(8)
                .map(|f| f.label.clone())
                .collect();
            if close.is_empty() {
                format!("フォント「{name}」がこの PC にありません")
            } else {
                format!(
                    "フォント「{name}」がこの PC にありません(近い名前: {})",
                    close.join("、")
                )
            }
        })?;
        let absent = use_face(&mut text, &face)?;
        font_message(&face.label, &absent)
    };
    Ok(Outcome::Change {
        label,
        edits: vec![Edit::SetData {
            id,
            data: serde_json::to_value(&text).expect("JSON にできる"),
        }],
    })
}

fn font_message(label: &str, absent: &[char]) -> String {
    if absent.is_empty() {
        format!("フォントを「{label}」にしました")
    } else {
        let s: String = absent.iter().take(12).collect();
        format!("フォントを「{label}」にしました(このフォントに無い字: {s})")
    }
}

fn insert<T: serde::Serialize>(label: &str, type_id: &str, name: String, data: &T) -> Outcome {
    Outcome::insert(
        label,
        type_id,
        name,
        serde_json::to_value(data).expect("JSON にできる"),
    )
}

impl Extension for TextCommands {
    /// 画面の並びは前と同じ(スケッチの「作成」の、平面の再定義より前。分解は再定義の後)。
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let face = cx.picked_plane().is_some();
        vec![
            Action::new(TEXT_AT_POINT, "スケッチの点に文字", "cad")
                .arg(kmb_core::Arg::integer("point", "文字の基準にするスケッチの点の番号(省くと単独の点)"))
                .role(kmb_interchange::role::action::TEXT_AT_POINT)
                .before(kmb_sketch::REDEFINE_PLANE)
                .menu("作成")
                .icon("textf")
                .aliases("もじ text てきすと てん point きじゅん")
                .hint("スケッチの単独の点を中心に文字を置く。点は拘束で動かせる")
                .at(if sketch_with_point(cx).is_some() {
                    &[Place::FeatureMenu]
                } else {
                    &[]
                })
                .disabled_if(
                    sketch_with_point(cx)
                        .is_none()
                        .then_some("線や円に使っていない点のあるスケッチを選んでから使う"),
                ),
            Action::new(TEXT_ON_PATH, "スケッチの線に文字", "cad")
                .before(kmb_sketch::REDEFINE_PLANE)
                .menu("作成")
                .icon("textf")
                .aliases("もじ text てきすと せん path そわせる えん circle こ arc")
                .hint("スケッチの最初の線(補助線でもよい)に沿って文字を並べる。円なら真上に外向き")
                .at(if sketch_with_path(cx).is_some() {
                    &[Place::FeatureMenu]
                } else {
                    &[]
                })
                .disabled_if(
                    sketch_with_path(cx)
                        .is_none()
                        .then_some("線分・円弧・円のあるスケッチを選んでから使う"),
                ),
            Action::new(TEXT_ON_FACE, "面に文字", "cad")
                .before(kmb_sketch::REDEFINE_PLANE)
                .menu("作成")
                .icon("textf")
                .aliases("もじ text てきすと ふぉんと font めん")
                .hint("平らな面を選んでから押すと、面の中心に 1 本線のフォントの文字を置く。彫刻でなぞるか、押し出しで彫る・浮かせる")
                // 帯の前には出さない(Kie 2026-10-21 の画板)
                .at(if face {
                    &[Place::Menu, Place::FaceMenu]
                } else {
                    &[Place::Menu]
                }),
            Action::new(EXPLODE_TEXT, "文字を線に分解", "cad")
                .menu("修正")
                .icon("textf")
                .aliases("もじ text てきすと ぶんかい explode せん ばらす")
                .hint("選んでいる文字を、同じ平面の新しいスケッチの線分にする(1 本ずつ動かして直せる)。元の文字は抑制する(消さない)")
                .at(if selected_text(cx).is_some() {
                    &[Place::FeatureMenu]
                } else {
                    &[]
                })
                .disabled_if(selected_text(cx).is_none().then_some("文字を選んでから使う")),
            Action::new(TEXT_FONT, "フォントを変更", "cad")
                .args([
                    Arg::text("name", "PC のフォントの名前(英語か日本語の書体の名前。空か「1 本線」で 1 本線に戻す)"),
                    Arg::text("style", "太さ・斜体の名前(例 Regular・Bold。省くと Regular)"),
                    Arg::file("file", "フォントのファイル(名前の代わりに。.ttf・.otf・.ttc)"),
                    Arg::integer("index", "TTC の中の書体の番号(0 から。file の時)"),
                ])
                .menu("修正")
                .icon("textf")
                .aliases("もじ text ふぉんと font しょたい にほんご")
                .hint("選んでいる文字を PC の輪郭のフォント(TrueType・OpenType)にする。使う字の形を文書に入れるので、フォントの無い PC で開いても形は変わらない")
                .at(if selected_text(cx).is_some() {
                    &[Place::FeatureMenu]
                } else {
                    &[]
                })
                .input(Input::Name {
                    title: "フォントを変更".into(),
                    label: "フォントの名前".into(),
                    default: selected_text(cx)
                        .and_then(|(_, t)| t.outline.map(|f| f.family))
                        .unwrap_or_default(),
                })
                .disabled_if(selected_text(cx).is_none().then_some("文字を選んでから使う")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        Some(match id {
            // 面の中心に、中央揃えで置く(面の平面の座標で)。面の代表点は面の外のこともあり、
            // そこに左下揃えで置くと文字が面からはみ出す
            TEXT_ON_FACE => match (picked_plane(cx), cx.picked_plane()) {
                (Some(plane), Some(f)) => {
                    let center = face_center(cx).unwrap_or(f.point);
                    let at = kmb_core::Plane::from_point_normal(f.point, f.normal).to_local(center);
                    Ok(insert(
                        "面に文字を追加",
                        TEXT,
                        format!("文字{}", cx.count_of(TEXT) + 1),
                        &Text {
                            plane,
                            x: format!("{:.2} mm", at[0]),
                            y: format!("{:.2} mm", at[1]),
                            align: crate::HAlign::Center,
                            valign: crate::VAlign::Middle,
                            ..Default::default()
                        },
                    ))
                }
                _ => Err(NEED_FACE.into()),
            },
            TEXT_AT_POINT => match sketch_with_point(cx) {
                Some(sketch) => Ok(insert(
                    "スケッチの点に文字を追加",
                    TEXT,
                    format!("文字{}", cx.count_of(TEXT) + 1),
                    &Text {
                        anchor: Some(sketch),
                        anchor_point: given_point(cx),
                        align: crate::HAlign::Center,
                        valign: crate::VAlign::Middle,
                        ..Default::default()
                    },
                )),
                None => Err("線や円に使っていない点のあるスケッチを選んでいません".into()),
            },
            // 円は真上に外向き(逆にたどる)、線分と円弧(とそれにつながる線の列)は真ん中に置く
            TEXT_ON_PATH => match sketch_with_path(cx) {
                Some((sketch, path)) => {
                    let circle = path.closed() && path.len() == 1;
                    let middle = path.length() / 2.0;
                    Ok(insert(
                        "スケッチの線に文字を追加",
                        TEXT,
                        format!("文字{}", cx.count_of(TEXT) + 1),
                        &Text {
                            path: Some(sketch),
                            reverse: circle,
                            chain: true,
                            align: crate::HAlign::Center,
                            x: if circle {
                                "0 mm".into()
                            } else {
                                format!("{middle:.3} mm")
                            },
                            ..Default::default()
                        },
                    ))
                }
                None => Err("線分・円弧・円のあるスケッチを選んでいません".into()),
            },
            EXPLODE_TEXT => explode_text(cx),
            TEXT_FONT => set_font(
                cx,
                &mut self.fonts.lock().unwrap_or_else(|p| p.into_inner()),
            ),
            _ => return None,
        })
    }

    /// 面に文字: 面を選んでいなければ、押してから面をクリックする(穴と同じ順)。
    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if cx.picked_plane().is_some() {
            return None;
        }
        if id == TEXT_ON_FACE {
            return Some(vec![Handle::new(
                "face",
                "文字を置く平らな面をクリック",
                Gesture::Face,
            )]);
        }
        None
    }

    /// JC のデータフォルダの `fonts` にもフォントを探す。
    fn set_data_dir(&self, dir: Option<&std::path::Path>) {
        if let Some(d) = dir {
            self.fonts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .add_dir(&d.join("fonts"));
        }
    }

    /// 選んでいる文字の線、外接の枠(参照用)、基準の位置。
    fn overlay(&self, cx: &Context<'_>) -> Overlay {
        let mut o = Overlay::default();
        let Some(sel) = cx.selected else { return o };
        if cx.selected_type() != Some(TEXT) {
            return o;
        }
        for (slot, kind) in [
            (
                "curves",
                LineKind::new(kmb_interchange::ink::line::SURROUND, "文字"),
            ),
            (
                "frame",
                LineKind::new(kmb_interchange::ink::line::FAINT, "文字の枠"),
            ),
        ] {
            let Some(c) = cx.output(sel, slot).and_then(Curves::from_value) else {
                continue;
            };
            for seg in &c.segments {
                let mut pts: Vec<[f64; 3]> = seg
                    .polyline(seg.steps_per_turn(0.01))
                    .into_iter()
                    .map(|q| c.plane.to_world(q))
                    .collect();
                pts.push(c.plane.to_world(seg.end_point()));
                o.lines.push((pts, kind.clone()));
            }
        }
        o
    }
}
