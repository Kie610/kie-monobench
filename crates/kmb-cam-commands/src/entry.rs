//! 加工に「進入位置」を 3D のビューで置く操作(ポケット・負荷制御粗取り・輪郭・等高線・往復・3D 粗取り)。
//!
//! 置いた位置は加工の `entry`(セットアップの座標の XY)に入る。経路はその近くから入る
//! (ポケットと負荷一定はそこで下りる。らせんならそこを中心に回る。輪郭はそこに最も近い所から、
//! 等高線は段ごとにそこに近い所から、往復は端のうちそこに近い方から)。削る所の外や、らせんが
//! 入らない所に置いた時は、加工の計算が注意を出して自動の入り方に戻す(経路を壊さない)。
//! マウスの所に出す座標と打つ座標は、セットアップの座標(原点と裏返しを含む)。

use crate::SetupInfo;
use crate::commands::round_um;
use kmb_cam_setup::Strategy;
use kmb_core::{Action, Context, Edit, Frame, Gesture, Handle, Outcome, Place};
use serde_json::Value;

pub const PLACE_ENTRY: &str = "cam.entry.place";
pub const AUTO_ENTRY: &str = "cam.entry.auto";

/// 選んでいる加工の戦略とデータ(入る位置を置ける加工だけ。どれが置けるかは戦略が名乗る)。
fn selected_op<'s>(cx: &Context<'_>, all: &'s [Strategy]) -> Option<(&'s Strategy, Value)> {
    let id = cx.selected?;
    let f = cx.data.feature(id)?;
    let s = all
        .iter()
        .find(|s| s.type_id == f.type_id && s.entry.is_some())?;
    Some((s, f.data.clone()))
}

/// 世界座標の点を、セットアップの座標にする。
pub fn to_setup(i: &SetupInfo, p: [f64; 3]) -> [f64; 3] {
    i.to_setup(p)
}

/// セットアップの座標系(マウスの所の座標と、打つ座標に使う)。
pub fn setup_frame(i: &SetupInfo) -> Frame {
    frame_of(|p| i.to_world(p))
}

/// 座標を世界座標にする写し方(回すか裏返すだけ)から、座標系を作る。
fn frame_of(to_world: impl Fn([f64; 3]) -> [f64; 3]) -> Frame {
    let o = to_world([0.0, 0.0, 0.0]);
    let axis = |e: [f64; 3]| {
        let w = to_world(e);
        [w[0] - o[0], w[1] - o[1], w[2] - o[2]]
    };
    Frame {
        origin: o,
        axes: [
            axis([1.0, 0.0, 0.0]),
            axis([0.0, 1.0, 0.0]),
            axis([0.0, 0.0, 1.0]),
        ],
    }
}

pub fn actions(cx: &Context<'_>, info: Option<&SetupInfo>, all: &[Strategy]) -> Vec<Action> {
    let Some((s, data)) = selected_op(cx, all) else {
        return vec![];
    };
    let menu = s.menu;
    let mut v = vec![
        Action::new(PLACE_ENTRY, "進入位置を置く", "cam")
            .arg(kmb_core::Arg::vector("point", "入る位置(世界座標の mm)").required())
            .menu(menu)
            .icon("pin")
            .aliases("しんにゅう はいるいち すいしょうしんにゅういち entry plunge")
            .hint(s.entry.unwrap_or_default())
            .at(&[Place::Panel, Place::FeatureMenu])
            .disabled_if(
                info.is_none()
                    .then_some("セットアップの計算ができていません"),
            ),
    ];
    if data.get("entry").is_some_and(|e| !e.is_null()) {
        v.push(
            Action::new(AUTO_ENTRY, "進入位置を自動に戻す", "cam")
                .menu(menu)
                .icon("pin")
                .hint("置いた入る位置を消し、自動で決めた所から入る")
                .at(&[Place::Panel, Place::FeatureMenu]),
        );
    }
    v
}

pub fn handles(
    id: &str,
    cx: &Context<'_>,
    info: Option<&SetupInfo>,
    all: &[Strategy],
) -> Option<Vec<Handle>> {
    if id != PLACE_ENTRY {
        return None;
    }
    let i = info?;
    selected_op(cx, all)?;
    let op = cx.selected?;
    // 入る位置を置ける所(ポケットと負荷一定)。計算できていれば塗って見せる
    let area = cx
        .output(op, kmb_cam_setup::ENTRY_AREA)
        .and_then(kmb_cam_setup::entry_area_of);
    let outline: Vec<Vec<[f64; 3]>> = area
        .as_ref()
        .map(|a| {
            a.rings
                .iter()
                .map(|l| l.iter().map(|q| i.to_world([q[0], q[1], a.z])).collect())
                .collect()
        })
        .unwrap_or_default();
    let label = match &area {
        Some(a) if a.helix => {
            "入る位置をクリック(塗った所なら、らせんが小さくならずに入る。座標はセットアップの座標)"
        }
        Some(_) => "入る位置をクリック(塗った所が工具の中心が入れる所。座標はセットアップの座標)",
        None => "入る位置をクリック(削る所の内側に置く。座標はセットアップの座標)",
    };
    let top = i.to_world([0.0, 0.0, i.stock_max[2]]);
    // ストックの上面の高さの平面(裏返していれば、世界座標では下を向く)
    let up = i.to_world([0.0, 0.0, i.stock_max[2] + 1.0]);
    let normal = [up[0] - top[0], up[1] - top[1], up[2] - top[2]];
    Some(vec![Handle {
        key: "point".into(),
        label: label.into(),
        gesture: Gesture::Point {
            origin: top,
            normal,
            snap: 0.5,
        },
        outline,
        snap_to: vec![],
        choices: vec![],
        optional: false,
        faces: false,
        frame: Some(setup_frame(i)),
        fill: true,
        picked: vec![],
        ops: None,
        show_origin: false,
        shape: None,
        chain: false,
        vertices: false,
        stop_key: None,
    }])
}

pub fn run(
    id: &str,
    cx: &Context<'_>,
    info: Option<&SetupInfo>,
    all: &[Strategy],
) -> Option<Result<Outcome, kmb_core::Problem>> {
    if id != PLACE_ENTRY && id != AUTO_ENTRY {
        return None;
    }
    Some((|| {
        let sel = cx.selected.ok_or("加工を選んでいません")?;
        let (_, mut data) = selected_op(cx, all).ok_or("入る位置を置ける加工を選んでいません")?;
        let obj = data.as_object_mut().ok_or("加工のデータが読めません")?;
        let label = if id == AUTO_ENTRY {
            obj.remove("entry");
            "進入位置を自動に戻す"
        } else {
            let i = info.ok_or("セットアップの計算ができていません")?;
            let w: [f64; 3] = cx
                .arg("point")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .ok_or("入る位置(point)がありません")?;
            let q = to_setup(i, w);
            obj.insert(
                "entry".into(),
                serde_json::json!([round_um(q[0]), round_um(q[1])]),
            );
            "進入位置を置く"
        };
        Ok(Outcome::Change {
            label: label.into(),
            edits: vec![Edit::SetData { id: sel, data }],
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setup_frame_reads_points_in_setup_coordinates() {
        // SetupInfo::to_world と同じ写し方(原点を足してから裏返す)
        let (o, c) = ([10.0, 20.0, 5.0], [0.0, 0.0, 2.0]);
        let flip = crate::Flip::AroundX;
        let to_world = |p: [f64; 3]| flip.point([p[0] + o[0], p[1] + o[1], p[2] + o[2]], c);
        let f = frame_of(to_world);
        for p in [[1.0, 2.0, 3.0], [-4.0, 0.5, 0.0]] {
            let back = f.to_local(to_world(p));
            let again = f.to_world(p);
            let w = to_world(p);
            for k in 0..3 {
                assert!((back[k] - p[k]).abs() < 1e-9, "{back:?} {p:?}");
                assert!((again[k] - w[k]).abs() < 1e-9);
            }
        }
    }
}
