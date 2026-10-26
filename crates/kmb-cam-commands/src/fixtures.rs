//! 固定具(クランプ)を置く・外す操作と、その重ね描き。
//!
//! 固定具は 3D のビューでクリックして置き、ベッドのネジ穴の近くでは穴に吸い付く
//! (ベッドは素材の左手前の下の角を L 字の基準の角に当てて置いた場合の位置)。
//! 経路が固定具を避けるのはセットアップの計算(`kmb_cam_setup::avoid`)。

use crate::commands::{picked_setup, setup_of};
use crate::{Fixture, Setup, SetupInfo};
use kmb_core::{Action, Context, Edit, Gesture, Handle, LineKind, Outcome, Place};

pub const ADD_FIXTURE: &str = "cam.fixture.add";
pub const CLEAR_FIXTURES: &str = "cam.fixture.clear";

/// ベッドのネジ穴の位置(セットアップの座標)。
pub fn bed_holes(bed: &kmb_post::Bed, corner: [f64; 3]) -> Vec<[f64; 3]> {
    let (bx, by) = (corner[0] - bed.origin[0], corner[1] - bed.origin[1]);
    bed.holes
        .iter()
        .map(|h| [bx + h[0], by + h[1], corner[2]])
        .collect()
}

pub fn actions(cx: &Context<'_>, info: Option<&SetupInfo>) -> Vec<Action> {
    // セットアップを選んでいない時も灰色で出す(出たり消えたりすると、リボンの後ろのボタンがずれる)
    let id = picked_setup(cx);
    let n = id
        .and_then(|id| setup_of(cx, id))
        .map_or(0, |s| s.fixtures.len());
    let why = if id.is_none() {
        Some("セットアップを選ぶと使えます")
    } else {
        info.is_none()
            .then_some("セットアップの計算ができていません")
    };
    let mut v = vec![
        Action::new(ADD_FIXTURE, "固定具を置く", "cam")
            .arg(kmb_core::Arg::vector("point", "置く位置(世界座標の mm)").required())
            .menu("セットアップ")
            .icon("fixture")
            .aliases("こていぐ くらんぷ fixture clamp")
            .hint("クリックで固定具(クランプ)を置く。ネジ穴の近くでは穴に吸い付く。ストックに重なる所では、いちばん近い辺の外へずらす。経路は固定具のまわり 2 mm を避ける")
            // 前には出さない(Kie 2026-10-21 の画板)
            .at(&[Place::Menu])
            // 欄と右クリックには、セットアップを選んでいる時だけ出す(ほかの機能の欄に混ぜない)
            .at(if id.is_some() {
                &[Place::Panel, Place::FeatureMenu]
            } else {
                &[]
            })
            .disabled_if(why),
    ];
    if n > 0 {
        v.push(
            Action::new(CLEAR_FIXTURES, &format!("固定具を外す({n} 個)"), "cam")
                .menu("セットアップ")
                .icon("lock")
                .hint("このセットアップの固定具をすべて外す")
                .at(&[Place::Panel, Place::FeatureMenu]),
        );
    }
    v
}

pub fn handles(
    id: &str,
    cx: &Context<'_>,
    info: Option<&SetupInfo>,
    bed: Option<&kmb_post::Bed>,
) -> Option<Vec<Handle>> {
    if id != ADD_FIXTURE {
        return None;
    }
    let i = info?;
    picked_setup(cx)?;
    let z = i.stock_min[2];
    let snap_to = bed
        .map(|b| bed_holes(b, crate::setup_ops::bed_corner(i)))
        .unwrap_or_default()
        .into_iter()
        .map(|p| i.to_world(p))
        .collect();
    Some(vec![Handle {
        key: "point".into(),
        label: "固定具を置く所をクリック(ネジ穴の近くでは穴に吸い付き、ストックに重なればその外へずらす)".into(),
        gesture: Gesture::Point {
            origin: i.to_world([0.0, 0.0, z]),
            normal: [0.0, 0.0, 1.0],
            snap: 1.0,
        },
        outline: vec![],
        snap_to,
        choices: vec![],
        optional: false,
        faces: false,
        frame: None,
        fill: false,
        picked: vec![],
        ops: None,
        show_origin: false,
        shape: None,
        chain: false,
        vertices: false,
    }])
}

pub fn run(
    id: &str,
    cx: &Context<'_>,
    info: Option<&SetupInfo>,
) -> Option<Result<Outcome, kmb_core::Problem>> {
    if id != ADD_FIXTURE && id != CLEAR_FIXTURES {
        return None;
    }
    Some((|| {
        let sid = picked_setup(cx).ok_or("セットアップを選んでいません")?;
        let mut s: Setup = setup_of(cx, sid).ok_or("セットアップのデータが読めません")?;
        let label = if id == CLEAR_FIXTURES {
            s.fixtures.clear();
            "固定具を外す"
        } else {
            let i = info.ok_or("セットアップの計算ができていません")?;
            let p: [f64; 3] = cx
                .arg("point")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .ok_or("置く所(point)がありません")?;
            let q = crate::entry::to_setup(i, p);
            // ストックに重なる所なら、いちばん近い辺の外へずらす
            let (at, size) = kmb_cam_setup::place_clamp([q[0], q[1]], i.stock_min, i.stock_max);
            s.fixtures.push(Fixture {
                at,
                size,
                name: format!("固定具 {}", s.fixtures.len() + 1),
            });
            "固定具を置く"
        };
        let data = serde_json::to_value(&s).expect("JSON にできる");
        Ok(Outcome::Change {
            label: label.into(),
            edits: vec![Edit::SetData { id: sid, data }],
        })
    })())
}

/// 固定具の箱(実線)と避ける範囲(案内の線)。世界座標。
pub fn lines(i: &SetupInfo, out: &mut Vec<(Vec<[f64; 3]>, LineKind)>) {
    for (boxes, kind) in [
        (
            &i.fixtures,
            LineKind::new(kmb_interchange::ink::line::SURROUND, "固定具"),
        ),
        (
            &i.keep_out,
            LineKind::new(kmb_interchange::ink::line::FAINT, "避ける範囲"),
        ),
    ] {
        for (lo, hi) in boxes {
            for e in box_edges(*lo, *hi) {
                out.push((e.iter().map(|p| i.to_world(*p)).collect(), kind.clone()));
            }
        }
    }
}

fn box_edges(lo: [f64; 3], hi: [f64; 3]) -> Vec<Vec<[f64; 3]>> {
    let ring = |z: f64| {
        vec![
            [lo[0], lo[1], z],
            [hi[0], lo[1], z],
            [hi[0], hi[1], z],
            [lo[0], hi[1], z],
            [lo[0], lo[1], z],
        ]
    };
    let mut v = vec![ring(lo[2]), ring(hi[2])];
    for (x, y) in [
        (lo[0], lo[1]),
        (hi[0], lo[1]),
        (hi[0], hi[1]),
        (lo[0], hi[1]),
    ] {
        v.push(vec![[x, y, lo[2]], [x, y, hi[2]]]);
    }
    v
}
