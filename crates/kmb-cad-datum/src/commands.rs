//! 作業平面の操作(画面・CLI・MCP に出す)と、3D のビューに描く灰色の四角。
//!
//! 元は選んでいる物から決める: 平らな面 2 つなら中間平面、角 3 つなら 3 点を通る平面、平らな面か
//! 作業平面 1 つなら浮かせた平面(辺も選んでいれば、その辺のまわりに傾けた平面)。何も選んで
//! いなければ、押してから原点の平面・作業平面・ボディの平らな面をクリックして選ぶ。
use crate::{Axis, Base, DATUM_PLANE, DatumPlaneFeature, Method, Point};
use kmb_core::{Action, Arg, Context, Extension, Gesture, Handle, HandleChoice, Outcome, Overlay, Place};
use serde_json::json;

pub const ADD_DATUM_PLANE: &str = "cad.add.datum_plane";

/// 作業平面の操作。
#[derive(Default)]
pub struct DatumCommands;

/// 指定 `plane`(xy・yz・zx・face か作業平面の機能の番号)を元の平面にする。face は選んでいる面。
fn base_of(cx: &Context<'_>, v: &serde_json::Value) -> Result<Base, String> {
    if let Some(n) = v.as_u64() {
        let id = kmb_core::FeatureId(n);
        if kmb_interchange::datum_planes(cx).iter().all(|(f, ..)| *f != id) {
            return Err(format!("機能 {n} は作業平面ではありません"));
        }
        return Ok(Base::Datum { feature: id });
    }
    Ok(match v.as_str() {
        Some("xy") => Base::Xy,
        Some("yz") => Base::Yz,
        Some("zx") => Base::Zx,
        Some("face") => picked_face(cx).ok_or("元にする平らな面をクリックしてください")?,
        _ => return Err(format!("平面 {v} はありません(xy・yz・zx・face か作業平面の機能の番号)")),
    })
}

fn picked_face(cx: &Context<'_>) -> Option<Base> {
    cx.picked_plane()?;
    let p = cx.picked?;
    Some(Base::Face {
        feature: p.feature,
        face: p.face.clone(),
    })
}

/// 選んでいる平らな面の全部(一緒に選んだ面が先)。
fn picked_planar_faces(cx: &Context<'_>) -> Vec<Base> {
    cx.picked_faces()
        .into_iter()
        .filter(|p| kmb_cad::plane::has_planar_face(cx, p.feature, &p.face))
        .map(|p| Base::Face {
            feature: p.feature,
            face: p.face.clone(),
        })
        .collect()
}

fn method_arg(cx: &Context<'_>) -> Option<Method> {
    cx.arg_str("method")
        .and_then(|m| serde_json::from_value(json!(m)).ok())
}

fn build(cx: &Context<'_>) -> Result<DatumPlaneFeature, String> {
    let faces = picked_planar_faces(cx);
    let method = method_arg(cx).unwrap_or(if cx.vertices.len() == 3 {
        Method::ThreePoints
    } else if faces.len() >= 2 {
        Method::Midplane
    } else if !cx.edges.is_empty() {
        Method::Angle
    } else {
        Method::Offset
    });
    let base = || -> Result<Base, String> {
        if let Some(v) = cx.arg("plane") {
            return base_of(cx, v);
        }
        if let Some(b) = faces.last() {
            return Ok(b.clone());
        }
        if let Some(id) = kmb_interchange::selected_datum(cx) {
            return Ok(Base::Datum { feature: id });
        }
        Err("元にする平面(XY・YZ・ZX の四角か作業平面)か、ボディの平らな面をクリックしてください".into())
    };
    let mut f = DatumPlaneFeature {
        method,
        ..Default::default()
    };
    match method {
        Method::Offset => {
            f.base = Some(base()?);
            f.distance = cx.arg_str("distance").unwrap_or("10 mm").to_string();
        }
        Method::Angle => {
            f.base = Some(base()?);
            f.axis = Some(match cx.arg_str("axis") {
                Some("x") => Axis::X,
                Some("y") => Axis::Y,
                Some("z") => Axis::Z,
                Some(other) => return Err(format!("軸 {other} はありません(x・y・z か、選んだ辺)")),
                None => {
                    let e = cx.edges.first().ok_or("傾ける軸にするまっすぐな辺を選んでください")?;
                    Axis::Edge {
                        feature: e.feature,
                        faces: e.faces.clone(),
                    }
                }
            });
            f.angle = cx.arg_str("angle").unwrap_or("30 deg").to_string();
        }
        Method::Midplane => {
            let [a, b] = match faces.as_slice() {
                [.., a, b] => [a.clone(), b.clone()],
                _ => return Err("中間平面にする平行な平らな面を 2 つ選んでください".into()),
            };
            f.base = Some(a);
            f.other = Some(b);
        }
        Method::ThreePoints => {
            if cx.vertices.len() != 3 {
                return Err(format!(
                    "3 点を通る平面には、ボディの角を 3 つ選んでください(今は {} つ)",
                    cx.vertices.len()
                ));
            }
            f.points = cx
                .vertices
                .iter()
                .map(|v| Point::Vertex {
                    feature: v.feature,
                    vertex: v.vertex.clone(),
                })
                .collect();
        }
    }
    Ok(f)
}

/// 何も選ばずに押した時に、元の平面を選ぶ候補(原点の平面と作業平面)。
fn plane_choices(cx: &Context<'_>) -> Vec<HandleChoice> {
    let mut v: Vec<HandleChoice> = ["xy", "yz", "zx"]
        .iter()
        .enumerate()
        .map(|(k, key)| HandleChoice::origin_plane(k, json!(key)))
        .collect();
    v.extend(kmb_interchange::datum_choices(cx));
    v
}

impl Extension for DatumCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        vec![
            Action::new(ADD_DATUM_PLANE, "作業平面", "cad")
                .args([
                    Arg::choice(
                        "method",
                        "作り方(省くと選んでいる物から: 平らな面 2 つなら中間、角 3 つなら 3 点、辺もあれば傾ける、ほかは浮かせる)",
                        &[
                            ("offset", "オフセット平面"),
                            ("angle", "傾けた平面"),
                            ("midplane", "中間平面"),
                            ("three_points", "3 点を通る平面"),
                        ],
                    ),
                    Arg::value("plane", "元の平面: xy・yz・zx・face(選んでいる平らな面)か作業平面の機能の番号(省くと選んでいる面か作業平面)"),
                    Arg::text("distance", "浮かせる距離(式可。負なら裏の側)").default("10 mm"),
                    Arg::choice("axis", "傾ける軸(省くと選んでいる辺)", &[("x", "X"), ("y", "Y"), ("z", "Z")]),
                    Arg::text("angle", "傾ける角度(式可)").default("30 deg"),
                ])
                .menu("構築")
                .icon("datumPlane")
                .aliases("さぎょうへいめん こうちく へいめん datum plane construction offset plane")
                .hint("スケッチや機能の基準にする平面を作る。面か平面を選んで押すと浮かせた平面、平らな面 2 つで中間平面、角 3 つで 3 点を通る平面")
                .at(&[Place::Toolbar, Place::FaceMenu]),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        if id != ADD_DATUM_PLANE {
            return None;
        }
        Some(build(cx).map(|f| {
            Outcome::insert(
                "作業平面を作成",
                DATUM_PLANE,
                format!("作業平面{}", kmb_interchange::datum_planes(cx).len() + 1),
                serde_json::to_value(&f).expect("JSON にできる"),
            )
        }))
    }

    /// 何も選ばずに押した時は、元の平面をクリックして選ぶ。
    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if id != ADD_DATUM_PLANE
            || cx.picked_plane().is_some()
            || !cx.vertices.is_empty()
            || kmb_interchange::selected_datum(cx).is_some()
        {
            return None;
        }
        Some(vec![Handle {
            choices: plane_choices(cx),
            faces: true,
            ..Handle::new(
                "plane",
                "元にする平面(XY・YZ・ZX の四角か作業平面)か、ボディの平らな面をクリック",
                Gesture::Choice,
            )
        }])
    }

    /// 作業平面を灰色の半透明の四角で描く(名前の札つき)。
    fn overlay(&self, cx: &Context<'_>) -> Overlay {
        let mut o = Overlay::default();
        for (_, name, d) in kmb_interchange::datum_planes(cx) {
            o.planes.push(kmb_core::OverlayPlane {
                corners: d.corners(),
                ink: kmb_interchange::ink::plane::DATUM,
                label: name,
            });
        }
        o
    }
}
