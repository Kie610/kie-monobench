//! 作業平面・作業軸・作業点の操作(画面・CLI・MCP に出す)と、3D のビューに描く灰色の四角・線・点。
//!
//! 作業平面の元は選んでいる物から決める: 平らな面 2 つなら中間平面、角 3 つなら 3 点を通る平面、
//! 平らな面か作業平面 1 つなら浮かせた平面(辺も選んでいれば、その辺のまわりに傾けた平面)。何も
//! 選んでいなければ、押してから原点の平面・作業平面・ボディの平らな面をクリックして選ぶ。
//! 作業軸は、角か作業点 2 つなら 2 点を通る軸、辺なら辺に沿う軸、曲がった面なら円筒の軸、平らな面
//! 2 つなら交わり。何も選んでいなければ、押してからボディの辺をクリックして選ぶ。
//! 作業点は、角なら角、円の辺なら中心、何も選んでいなければ原点の座標(編集パネルで直す)。
use crate::axis::{AxisMethod, EdgeRef, FaceRef};
use crate::{
    Axis, Base, DATUM_AXIS, DATUM_PLANE, DATUM_POINT, DatumAxisFeature, DatumPlaneFeature,
    DatumPointFeature, Method, Point,
};
use kmb_core::{
    Action, Arg, Context, Extension, Gesture, Handle, HandleChoice, Outcome, Overlay, Place,
};
use serde_json::json;

pub const ADD_DATUM_PLANE: &str = "cad.add.datum_plane";
pub const ADD_DATUM_AXIS: &str = "cad.add.datum_axis";
pub const ADD_DATUM_POINT: &str = "cad.add.datum_point";

/// 作業平面の操作。
#[derive(Default)]
pub struct DatumCommands;

/// 指定 `plane`(xy・yz・zx・face か作業平面の機能の番号)を元の平面にする。face は選んでいる面。
fn base_of(cx: &Context<'_>, v: &serde_json::Value) -> Result<Base, String> {
    if let Some(n) = v.as_u64() {
        let id = kmb_core::FeatureId(n);
        if kmb_interchange::datum_planes(cx)
            .iter()
            .all(|(f, ..)| *f != id)
        {
            return Err(format!("機能 {n} は作業平面ではありません"));
        }
        return Ok(Base::Datum { feature: id });
    }
    Ok(match v.as_str() {
        Some("xy") => Base::Xy,
        Some("yz") => Base::Yz,
        Some("zx") => Base::Zx,
        Some("face") => picked_face(cx).ok_or("元にする平らな面をクリックしてください")?,
        _ => {
            return Err(format!(
                "平面 {v} はありません(xy・yz・zx・face か作業平面の機能の番号)"
            ));
        }
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
        Err(
            "元にする平面(XY・YZ・ZX の四角か作業平面)か、ボディの平らな面をクリックしてください"
                .into(),
        )
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
                Some(other) => {
                    return Err(format!("軸 {other} はありません(x・y・z か、選んだ辺)"));
                }
                None => {
                    let e = cx
                        .edges
                        .first()
                        .ok_or("傾ける軸にするまっすぐな辺を選んでください")?;
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
            f.points = picked_points(cx);
        }
    }
    Ok(f)
}

/// 選んでいる角と作業点(角が先)。
fn picked_points(cx: &Context<'_>) -> Vec<Point> {
    let mut v: Vec<Point> = cx
        .vertices
        .iter()
        .map(|v| Point::Vertex {
            feature: v.feature,
            vertex: v.vertex.clone(),
        })
        .collect();
    let points = kmb_interchange::datum_points(cx, None);
    for id in cx.selected.iter().chain(cx.also_selected) {
        if points.iter().any(|(f, ..)| f == id) && !v.contains(&Point::Datum { feature: *id }) {
            v.push(Point::Datum { feature: *id });
        }
    }
    v
}

/// 選んでいる曲がった面(円筒の面の軸に使う)。
fn picked_curved_face(cx: &Context<'_>) -> Option<FaceRef> {
    cx.picked_faces()
        .into_iter()
        .find(|p| !kmb_cad::plane::has_planar_face(cx, p.feature, &p.face))
        .map(|p| FaceRef {
            feature: p.feature,
            face: p.face.clone(),
        })
}

fn axis_method_arg(cx: &Context<'_>) -> Option<AxisMethod> {
    cx.arg_str("method")
        .and_then(|m| serde_json::from_value(json!(m)).ok())
}

fn build_axis(cx: &Context<'_>) -> Result<DatumAxisFeature, String> {
    let points = picked_points(cx);
    let faces = picked_planar_faces(cx);
    let edge = match cx.arg("edge") {
        // 辺をクリックして選ぶ手順の値({"feature": 番号, "edge": [面, 面]})
        Some(v) => match kmb_cad::AxisRef::from_choice(v, cx)? {
            kmb_cad::AxisRef::Edge { feature, faces } => Some(EdgeRef { feature, faces }),
            _ => return Err("作業軸にするボディの辺をクリックしてください".into()),
        },
        None => cx.edges.first().map(|e| EdgeRef {
            feature: e.feature,
            faces: e.faces.clone(),
        }),
    };
    let curved = picked_curved_face(cx);
    let method = axis_method_arg(cx).unwrap_or(if points.len() >= 2 {
        AxisMethod::TwoPoints
    } else if edge.is_some() {
        AxisMethod::Edge
    } else if curved.is_some() {
        AxisMethod::Cylinder
    } else {
        AxisMethod::TwoPlanes
    });
    let mut f = DatumAxisFeature {
        method,
        ..Default::default()
    };
    match method {
        AxisMethod::TwoPoints => {
            if points.len() != 2 {
                return Err(format!(
                    "2 点を通る軸には、ボディの角か作業点を 2 つ選んでください(今は {} つ)",
                    points.len()
                ));
            }
            f.points = points;
        }
        AxisMethod::Edge => f.edge = Some(edge.ok_or("作業軸にするボディの辺を選んでください")?),
        AxisMethod::Cylinder => f.face = Some(curved.ok_or("軸を取る円筒の面を選んでください")?),
        AxisMethod::TwoPlanes => {
            let given = |key: &str| cx.arg(key).map(|v| base_of(cx, v)).transpose();
            let (a, b) = match (given("plane")?, given("other")?) {
                (Some(a), Some(b)) => (a, b),
                _ => match faces.as_slice() {
                    [.., a, b] => (a.clone(), b.clone()),
                    _ => {
                        return Err(
                            "作業軸にするボディの辺を選ぶか、交わる平らな面を 2 つ選んでください"
                                .into(),
                        );
                    }
                },
            };
            f.base = Some(a);
            f.other = Some(b);
        }
    }
    Ok(f)
}

fn build_point(cx: &Context<'_>) -> Result<DatumPointFeature, String> {
    if let Some(v) = cx.arg("at") {
        let at: [f64; 3] =
            serde_json::from_value(v.clone()).map_err(|_| "at は [x, y, z](mm)で指定します")?;
        return Ok(DatumPointFeature {
            point: Point::At { at },
        });
    }
    if let Some(v) = cx.vertices.first() {
        return Ok(DatumPointFeature {
            point: Point::Vertex {
                feature: v.feature,
                vertex: v.vertex.clone(),
            },
        });
    }
    if let Some(e) = cx.edges.first() {
        if !kmb_cad::axis::is_arc_edge(cx, e.feature, &e.faces) {
            return Err(
                "作業点にできるのは円の辺(その中心)です。まっすぐな辺なら端の角を選んでください"
                    .into(),
            );
        }
        return Ok(DatumPointFeature {
            point: Point::Center {
                feature: e.feature,
                faces: e.faces.clone(),
            },
        });
    }
    Ok(DatumPointFeature::default())
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
            Action::new(ADD_DATUM_AXIS, "作業軸", "cad")
                .args([
                    Arg::choice(
                        "method",
                        "作り方(省くと選んでいる物から: 角か作業点 2 つなら 2 点、辺なら辺、曲がった面なら円筒の軸、平らな面 2 つなら交わり)",
                        crate::axis::AXIS_METHODS,
                    ),
                    Arg::value("edge", "辺: {\"feature\": 機能の番号, \"edge\": [面, 面]}(省くと選んでいる辺)"),
                    Arg::value("plane", "交わりの 1 つ目の平面: xy・yz・zx か作業平面の機能の番号(省くと選んでいる平らな面)"),
                    Arg::value("other", "交わりの 2 つ目の平面(plane と同じ書き方)"),
                ])
                .menu("構築")
                .icon("datumAxis")
                .aliases("さぎょうじく こうちく じく datum axis construction")
                .hint("回転やパターンの軸にする直線を作る。辺を選んで押すと辺に沿う軸、円筒の面で円筒の軸、角 2 つで 2 点を通る軸、平らな面 2 つで交わり")
                .at(&[Place::Toolbar, Place::FaceMenu]),
            Action::new(ADD_DATUM_POINT, "作業点", "cad")
                .args([Arg::value("at", "座標 [x, y, z](mm。省くと選んでいる角か円の辺の中心、何も選んでいなければ原点)")])
                .menu("構築")
                .icon("datumPoint")
                .aliases("さぎょうてん こうちく てん datum point construction")
                .hint("作業平面や作業軸の基準にする点を作る。角を選んで押すと角、円の辺で中心、何も選ばずに押すと座標の点")
                .at(&[Place::Toolbar, Place::FaceMenu]),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        let insert = |what: &str, kind: &str, n: usize, data: serde_json::Value| {
            Outcome::insert(
                &format!("{what}を作成"),
                kind,
                format!("{what}{}", n + 1),
                data,
            )
        };
        let json = |v: Result<serde_json::Value, serde_json::Error>| v.expect("JSON にできる");
        Some(match id {
            ADD_DATUM_PLANE => build(cx)
                .map(|f| {
                    insert(
                        "作業平面",
                        DATUM_PLANE,
                        kmb_interchange::datum_planes(cx).len(),
                        json(serde_json::to_value(&f)),
                    )
                })
                .map_err(Into::into),
            ADD_DATUM_AXIS => build_axis(cx)
                .map(|f| {
                    insert(
                        "作業軸",
                        DATUM_AXIS,
                        kmb_interchange::datum_axes(cx, None).len(),
                        json(serde_json::to_value(&f)),
                    )
                })
                .map_err(Into::into),
            ADD_DATUM_POINT => build_point(cx)
                .map(|f| {
                    insert(
                        "作業点",
                        DATUM_POINT,
                        kmb_interchange::datum_points(cx, None).len(),
                        json(serde_json::to_value(&f)),
                    )
                })
                .map_err(Into::into),
            _ => return None,
        })
    }

    /// 何も選ばずに押した時は、元の平面(作業軸はボディの辺)をクリックして選ぶ。
    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if id == ADD_DATUM_AXIS {
            if !cx.edges.is_empty()
                || !cx.picked_faces().is_empty()
                || picked_points(cx).len() >= 2
                || cx.arg("plane").is_some()
            {
                return None;
            }
            // 文書に無い番号を渡すと、全部の機能の後(見えている全部のボディの辺)
            let all = kmb_core::FeatureId(u64::MAX);
            let edges: Vec<HandleChoice> =
                kmb_cad::axis::axis_choices_where(cx, all, None, |_, _| true)
                    .into_iter()
                    .filter(|c| c.value.get("edge").is_some())
                    .collect();
            return Some(vec![Handle {
                choices: edges,
                ..Handle::new(
                    "edge",
                    "作業軸にするボディの辺(まっすぐな辺か円の辺)をクリック",
                    Gesture::Choice,
                )
            }]);
        }
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
        // 作業軸は灰色の線、作業点は灰色の点(基準面と同じ色)
        const DATUM: kmb_core::LineKind =
            kmb_core::LineKind::new(kmb_interchange::ink::line::SURROUND, "作業軸・作業点");
        for (_, _, d) in kmb_interchange::datum_axes(cx, None) {
            o.lines.push((d.ends().to_vec(), DATUM));
        }
        for (_, _, d) in kmb_interchange::datum_points(cx, None) {
            o.lines.push((vec![d.at], DATUM));
        }
        o
    }
}
