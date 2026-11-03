//! 操作「歯車」と「歯車の輪郭」。押してから平面(原点の平面の四角・作業平面・ボディの平らな面)→
//! 中心の順にクリックすると、歯車の輪郭(`cad.gear@1`)を置き、「歯車」はそれを押し出すボディ
//! (`cad.gear_body@1`)も同じ取り消し単位で足す。足した後は編集パネルで輪郭の欄(種類・モジュール・
//! 歯数…)を直し、OK か Enter で決める(Esc でやめる)。「歯車の輪郭」はボディを作らない(レーザーや
//! 2D の加工にそのまま使う)。画面・CLI・MCP・コマンド欄は同じ引数の宣言を通る。

use crate::{GEAR, GEAR_BODY, kinds};
use kmb_cad::plane::PlaneRef;
use kmb_core::kernel::P3;
use kmb_core::{
    Action, Arg, Context, Edit, Extension, FeatureId, Gesture, Handle, Outcome, Payload, Place,
    Plane, Problem, Value,
};
use kmb_interchange::GearKind;
use serde_json::json;

/// 歯車の輪郭とボディを足す操作。
pub const GEAR_ADD: &str = "cad.gear";
/// 歯車の輪郭だけを足す操作。
pub const GEAR_OUTLINE_ADD: &str = "cad.gear_outline";

/// まだ文書に無い機能(これから足す歯車)。平面の候補と面は、文書のどの機能より後として探す。
const NEW: FeatureId = FeatureId(u64::MAX);

const PLANE_STEP: &str =
    "歯車を置く平面(XY・YZ・ZX の四角か作業平面)か、ボディの平らな面をクリック";
const POINT_STEP: &str = "歯車の中心をクリック(数字で x, y も打てる)。歯数などは後の欄で直す";

/// 操作が受け取る引数。平面は [`kmb_cad::plane::args`] と同じ(省くと選んでいる平らな面、
/// それも無ければ原点の XY 平面)。
fn args(body: bool) -> Vec<Arg> {
    let kinds: Vec<(&str, &str)> = GearKind::ALL
        .iter()
        .filter(|k| kinds::available(**k))
        .map(|k| (k.key(), k.label()))
        .collect();
    let mut v = kmb_cad::plane::args();
    v.extend([
        Arg::vector("point", "中心(世界座標の mm。省くと平面の原点か面の代表点)"),
        Arg::choice("kind", "歯車の種類", &kinds).default(GearKind::External.key()),
        Arg::length("module", "モジュール(ピッチ円の径 = モジュール × 歯数)").default(1.0),
        Arg::integer("teeth", "歯数(平歯車は 3 以上、ラックは 1 以上)").default(20),
        Arg::choice(
            "rack_size",
            "ラックの長さの指定",
            &[("teeth", "歯数"), ("length", "長さ")],
        )
        .default("teeth"),
        Arg::length("rack_length", "ラックの全長(長さ指定の時に必須)"),
        Arg::length("rack_back", "ラックの背の厚さ(省くとモジュールの 2 倍)"),
        Arg::angle("phase", "回す角(ラックは進む向き、歯車は 1 枚目の歯の向き)"),
        Arg::angle("pressure_angle", "圧力角").default(20.0),
        Arg::length("bore", "軸の穴の径(省くと穴なし)"),
    ]);
    if body {
        v.push(Arg::length("width", "歯の幅(ボディの厚さ)").default(10.0));
    }
    v
}

/// 置く平面と、その平面の座標の向き(輪郭の機能が使うのと同じ決め方)。
fn place(cx: &Context<'_>) -> Result<(PlaneRef, Plane), String> {
    let plane = if cx.arg("plane").is_some() {
        PlaneRef::from_choice(cx, NEW, "歯車を置く平面")?
    } else if let (Some(p), Some(_)) = (cx.picked, cx.picked_plane()) {
        PlaneRef::Face {
            feature: Some(p.feature),
            face: Some(p.face.clone()),
        }
    } else {
        PlaneRef::OriginXy
    };
    let frame = match &plane {
        PlaneRef::OriginXy => Plane::XY,
        PlaneRef::OriginYz => Plane::YZ,
        PlaneRef::OriginZx => Plane::ZX,
        PlaneRef::Datum { feature } => cx
            .output(*feature, kmb_interchange::DATUM_OUTPUT)
            .and_then(kmb_interchange::DatumPlane::from_value)
            .map(|d| d.plane)
            .ok_or(format!("機能 {} は作業平面ではありません", feature.0))?,
        PlaneRef::Face {
            feature: Some(f),
            face: Some(name),
        } => {
            let Some(Value::Shape(h)) = cx.output(*f, "body") else {
                return Err("面を持つボディがまだ計算できていません".into());
            };
            let p = cx
                .kernel
                .faces(*h)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|x| &x.name == name)
                .and_then(|x| x.planar)
                .ok_or(format!("面 {name} は平らな面ではありません"))?;
            Plane::from_point_normal(p.point, p.normal)
        }
        _ => return Err("歯車を置く平面を選んでください".into()),
    };
    Ok((plane, frame))
}

/// 数を式の文字にする(小さな端数を落とす)。
fn mm(v: f64) -> String {
    format!("{} mm", (v * 1e4).round() / 1e4)
}

fn add(cx: &Context<'_>, body: bool) -> Result<Outcome, Problem> {
    let (plane, frame) = place(cx)?;
    let center = match cx.arg("point") {
        Some(v) => {
            let p: P3 = serde_json::from_value(v.clone())
                .map_err(|_| "point は [x, y, z] の数で指定します".to_string())?;
            frame.to_local(p)
        }
        None => [0.0, 0.0],
    };
    let kind_key = cx.arg_str("kind").unwrap_or(GearKind::External.key());
    let kind = GearKind::ALL
        .iter()
        .copied()
        .find(|k| k.key() == kind_key)
        .ok_or(format!("歯車の種類 {kind_key} はありません"))?;
    if !kinds::available(kind) {
        return Err(format!("「{}」の形はまだ作れません", kind.label()).into());
    }
    let module = cx.arg_f64("module")?.unwrap_or(1.0);
    if module <= 0.0 {
        return Err("モジュールは 0 より大きくしてください".into());
    }
    let teeth = cx.arg("teeth").and_then(|v| v.as_u64()).unwrap_or(20);
    if teeth < if kind == GearKind::Rack { 1 } else { 3 } {
        return Err("歯数は平歯車で 3 以上、ラックで 1 以上にしてください".into());
    }
    let pressure = cx.arg_f64("pressure_angle")?.unwrap_or(20.0);
    let module_text = if kind == GearKind::Rack {
        format!("{module} mm")
    } else {
        mm(module)
    };
    let mut gear = json!({
        "plane": serde_json::to_value(&plane).expect("JSON にできる"),
        "module": module_text,
        "teeth": teeth.to_string(),
        "pressure_angle": format!("{pressure} deg"),
        "x": mm(center[0]),
        "y": mm(center[1]),
    });
    if kind != GearKind::default() {
        gear["kind"] = json!(kind.key());
    }
    if let Some(b) = cx.arg_f64("bore")?.filter(|b| *b > 0.0) {
        gear["bore"] = json!(mm(b));
    }
    if let Some(phase) = cx.arg_f64("phase")? {
        gear["phase"] = json!(format!("{phase} deg"));
    }
    let rack_size = cx.arg_str("rack_size").unwrap_or("teeth");
    if kind == GearKind::Rack {
        gear["rack_size"] = json!(rack_size);
        if rack_size == "length" {
            let value = cx
                .arg_f64("rack_length")?
                .ok_or("ラックの長さを入力してください")?;
            kinds::rack::teeth_for_length(module, value)?;
            gear["rack_length"] = json!(format!("{value} mm"));
        }
        if let Some(back) = cx.arg_f64("rack_back")? {
            if back <= 0.0 {
                return Err("背の厚さは 0 より大きくしてください".into());
            }
            gear["rack_back"] = json!(format!("{back} mm"));
        }
        if cx.arg_f64("bore")?.unwrap_or(0.0) != 0.0 {
            return Err("ラックに軸の穴は付けられません".into());
        }
    } else if rack_size != "teeth"
        || cx.arg("rack_length").is_some()
        || cx.arg("rack_back").is_some()
    {
        return Err("ラックの長さと背の厚さは種類がラックの時だけ指定できます".into());
    }
    let n = cx.count_of(GEAR) + 1;
    let mut edits = vec![Edit::Insert {
        type_id: GEAR.into(),
        name: format!("歯車{n}"),
        data: gear,
    }];
    if body {
        let width = cx.arg_f64("width")?.unwrap_or(10.0);
        if width <= 0.0 {
            return Err("歯の幅は 0 より大きくしてください".into());
        }
        edits.push(Edit::Insert {
            type_id: GEAR_BODY.into(),
            name: format!("歯車のボディ{}", cx.count_of(GEAR_BODY) + 1),
            data: json!({"gear": {"$new": 0}, "width": mm(width)}),
        });
    }
    Ok(Outcome::Change {
        label: if body {
            "歯車を追加".into()
        } else {
            "歯車の輪郭を追加".into()
        },
        edits,
    })
}

/// 操作「歯車」「歯車の輪郭」。
pub struct GearCommands;

impl Extension for GearCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        vec![
            Action::new(GEAR_ADD, "歯車", "cad")
                .args(args(true))
                .menu("作成")
                .icon("gears")
                .aliases("はぐるま 歯車 gear ぎあ へいはぐるま spur いんぼりゅーと involute")
                .hint("押してから、平面 → 中心の順にクリックすると歯車の輪郭とボディができる。種類・モジュール・歯数などは欄で直し、OK か Enter で決める。Esc でやめる")
                .at(&[Place::Menu]),
            Action::new(GEAR_OUTLINE_ADD, "歯車の輪郭", "cad")
                .args(args(false))
                .menu("作成")
                .icon("gears")
                .aliases("はぐるま 歯車 gear ぎあ りんかく 輪郭 outline れーざー laser")
                .hint("歯車の輪郭だけを置く(ボディは作らない)。レーザーや 2D 輪郭の加工、押し出しにそのまま使える")
                .at(&[Place::Menu]),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        match id {
            GEAR_ADD => Some(add(cx, true)),
            GEAR_OUTLINE_ADD => Some(add(cx, false)),
            _ => None,
        }
    }

    /// 平面を選んでいなければ平面の候補から、平らな面を選んでいれば中心から始める。
    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if id != GEAR_ADD && id != GEAR_OUTLINE_ADD {
            return None;
        }
        let plane_step = || Handle {
            choices: kmb_cad::plane::plane_choices(cx, NEW),
            faces: true,
            ..Handle::new("plane", PLANE_STEP, Gesture::Choice)
        };
        let given = cx.arg("plane").is_some();
        if !given && cx.picked_plane().is_none() {
            // 中心の手順は平面を選んでから出し直す
            return Some(vec![
                plane_step(),
                Handle::new("point", POINT_STEP, Gesture::Face),
            ]);
        }
        let (_, frame) = place(cx).ok()?;
        let point = Handle::new(
            "point",
            POINT_STEP,
            Gesture::Point {
                origin: frame.origin,
                normal: frame.normal(),
                snap: 0.5,
            },
        );
        Some(if given {
            vec![plane_step(), point]
        } else {
            vec![point]
        })
    }
}
