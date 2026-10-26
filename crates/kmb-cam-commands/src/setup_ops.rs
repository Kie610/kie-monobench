//! セットアップを 3D のビューで直す操作: 原点を点で選ぶ、一緒に削るボディを足す・外す、
//! 固定具を 1 つずつ動かす・大きさを変える・外す。
//!
//! 原点は、ストックの箱の点(角・辺の中点・面の中心)、部品の外形の箱の点、部品の稜線の端の点に
//! 吸い付く。吸い付いた点の種類で、ストックや部品の大きさが変わっても同じ所に付いて行くように
//! 覚える([`OriginAt`])。

use crate::commands::{picked_setup, round_um, setup_of};
use crate::{Setup, SetupInfo};
use kmb_cam_setup::OriginAt;
use kmb_core::{
    Action, Arg, Context, Edit, FeatureId, Gesture, Handle, HandleChoice, Outcome, Place, Value,
};

pub const PICK_ORIGIN: &str = "cam.setup.origin_pick";
pub const RESET_ORIGIN: &str = "cam.setup.origin_reset";
pub const ADD_BODY: &str = "cam.setup.add_body";
pub const CLEAR_BODIES: &str = "cam.setup.clear_bodies";
pub const MOVE_FIXTURE: &str = "cam.fixture.move";
pub const RESIZE_FIXTURE: &str = "cam.fixture.resize";
pub const REMOVE_FIXTURE: &str = "cam.fixture.remove";

pub const IDS: &[&str] = &[
    PICK_ORIGIN,
    RESET_ORIGIN,
    ADD_BODY,
    CLEAR_BODIES,
    MOVE_FIXTURE,
    RESIZE_FIXTURE,
    REMOVE_FIXTURE,
];

/// 箱の点の割合(軸ごとに 0・0.5・1)の 27 通り。
const FRACS: [f64; 3] = [0.0, 0.5, 1.0];

/// 箱 `lo`〜`hi`(WCS)の 27 点(世界座標)と、その割合。
fn box_points(i: &SetupInfo, lo: [f64; 3], hi: [f64; 3]) -> Vec<([f64; 3], [f64; 3])> {
    let mut v = Vec::with_capacity(27);
    for fz in FRACS {
        for fy in FRACS {
            for fx in FRACS {
                let f = [fx, fy, fz];
                let p = [0, 1, 2].map(|k| lo[k] + (hi[k] - lo[k]) * f[k]);
                v.push((i.to_world(p), f));
            }
        }
    }
    v
}

/// 部品の稜線の端の点(世界座標、重なりは除く)。多すぎる時は間引く。
fn part_vertices(cx: &Context<'_>, i: &SetupInfo) -> Vec<[f64; 3]> {
    let Ok(mesh) = i.part_mesh(cx.kernel.as_ref()) else {
        return vec![];
    };
    let mut seen = std::collections::HashSet::new();
    let mut v = Vec::new();
    for e in &mesh.edges {
        for p in [e.first(), e.last()].into_iter().flatten() {
            let key = p.map(|c| (c as f64 * 1000.0).round() as i64);
            if seen.insert(key) {
                v.push(p.map(|c| c as f64));
            }
        }
    }
    const MAX: usize = 2000;
    if v.len() > MAX {
        let step = v.len().div_ceil(MAX);
        v = v.into_iter().step_by(step).collect();
    }
    v
}

fn same(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6)
}

/// クリックで決まった点 `w`(世界座標)を、覚える原点にする。
pub fn origin_at(i: &SetupInfo, w: [f64; 3]) -> OriginAt {
    if let Some((_, f)) = box_points(i, i.stock_min, i.stock_max)
        .into_iter()
        .find(|(p, _)| same(*p, w))
    {
        return OriginAt::Stock { at: f };
    }
    if let Some((_, f)) = box_points(i, i.part_min, i.part_max)
        .into_iter()
        .find(|(p, _)| same(*p, w))
    {
        return OriginAt::Model { at: f };
    }
    OriginAt::Point {
        at: w.map(|c| (c * 1000.0).round() / 1000.0),
    }
}

/// 箱の上面の四角(世界座標の閉じた線)。
fn top_rect(i: &SetupInfo, lo: [f64; 3], hi: [f64; 3]) -> Vec<[f64; 3]> {
    let z = hi[2];
    [
        [lo[0], lo[1], z],
        [hi[0], lo[1], z],
        [hi[0], hi[1], z],
        [lo[0], hi[1], z],
        [lo[0], lo[1], z],
    ]
    .into_iter()
    .map(|p| i.to_world(p))
    .collect()
}

/// 部品の世界座標の箱の上面の四角。
fn world_top_rect(lo: [f64; 3], hi: [f64; 3]) -> Vec<[f64; 3]> {
    let z = hi[2];
    vec![
        [lo[0], lo[1], z],
        [hi[0], lo[1], z],
        [hi[0], hi[1], z],
        [lo[0], hi[1], z],
        [lo[0], lo[1], z],
    ]
}

/// セットアップに足せるボディ(見えていて、まだ削る部品に入っていないもの)。
fn addable_bodies(cx: &Context<'_>, s: &Setup) -> Vec<(FeatureId, kmb_core::ShapeHandle)> {
    cx.visible_bodies()
        .into_iter()
        .filter(|(id, _)| *id != s.body && !s.more_bodies.contains(id) && Some(*id) != s.stock.body)
        .collect()
}

pub fn actions(cx: &Context<'_>, info: Option<&SetupInfo>) -> Vec<Action> {
    let Some(id) = picked_setup(cx) else {
        return vec![];
    };
    let Some(s) = setup_of(cx, id) else {
        return vec![];
    };
    let why = info
        .is_none()
        .then_some("セットアップの計算ができていません");
    let mut v = vec![
        Action::new(PICK_ORIGIN, "原点を 3D で選ぶ", "cam")
            .arg(Arg::vector("point", "原点にする点(世界座標の mm)").required())
            .menu("セットアップ")
            .icon("pin")
            .aliases("げんてん wcs origin てん")
            .hint("ストックの箱の角・辺の中点・面の中心か、部品の外形の箱の点、部品の角をクリックして、作業座標系の原点にする")
            .at(&[Place::Panel, Place::FeatureMenu])
            .disabled_if(why),
    ];
    if s.origin_at.is_some() {
        v.push(
            Action::new(RESET_ORIGIN, "原点を一覧の選び方に戻す", "cam")
                .menu("セットアップ")
                .icon("pin")
                .hint("3D で選んだ原点をやめ、「原点(XY)」「原点(Z)」の欄で決める")
                .at(&[Place::Panel, Place::FeatureMenu]),
        );
    }
    v.push(
        Action::new(ADD_BODY, "削るボディを足す", "cam")
            .arg(Arg::feature("body", "足すボディ(機能の番号)").required())
            .menu("セットアップ")
            .icon("body")
            .aliases("ぼでぃ ふくすう body add multiple")
            .hint("1 つのセットアップで、ほかのボディも一緒に削る(1 枚の板から部品を何個も削る時など)")
            .at(&[Place::Panel, Place::FeatureMenu])
            .disabled_if(
                why.or(addable_bodies(cx, &s)
                    .is_empty()
                    .then_some("足せるボディがありません")),
            ),
    );
    if !s.more_bodies.is_empty() {
        v.push(
            Action::new(
                CLEAR_BODIES,
                &format!("足したボディを外す({} 個)", s.more_bodies.len()),
                "cam",
            )
            .menu("セットアップ")
            .icon("body")
            .hint("「部品」の欄のボディだけを削る")
            .at(&[Place::Panel, Place::FeatureMenu]),
        );
    }
    if !s.fixtures.is_empty() {
        for (aid, label, hint) in [
            (
                MOVE_FIXTURE,
                "固定具を動かす",
                "動かす固定具をクリックし、置く所をクリックする(ネジ穴の近くでは穴に吸い付く)",
            ),
            (
                RESIZE_FIXTURE,
                "固定具の大きさを変える",
                "固定具をクリックし、幅・奥行・高さを打つ(Enter で次へ)",
            ),
            (
                REMOVE_FIXTURE,
                "固定具を 1 つ外す",
                "外す固定具をクリックする",
            ),
        ] {
            v.push(
                Action::new(aid, label, "cam")
                    .args(fixture_args(aid))
                    .menu("セットアップ")
                    .icon("lock")
                    .aliases("こていぐ くらんぷ fixture clamp")
                    .hint(hint)
                    .at(&[Place::FeatureMenu])
                    .disabled_if(why),
            );
        }
    }
    v
}

/// 固定具を直す操作 `aid` の引数。
fn fixture_args(aid: &str) -> Vec<Arg> {
    let mut v = vec![Arg::integer("fixture", "固定具の番号(0 から)").required()];
    match aid {
        MOVE_FIXTURE => v.push(Arg::vector("point", "置く所(世界座標の mm)").required()),
        RESIZE_FIXTURE => v.extend(
            [("width", "幅"), ("depth", "奥行"), ("height", "高さ")]
                .map(|(k, h)| Arg::length(k, h).required()),
        ),
        _ => {}
    }
    v
}

fn fixture_choices(i: &SetupInfo, s: &Setup) -> Vec<HandleChoice> {
    s.fixtures
        .iter()
        .enumerate()
        .zip(&i.fixtures)
        .map(|((k, f), (lo, hi))| HandleChoice {
            value: serde_json::json!(k),
            label: if f.name.is_empty() {
                format!("固定具 {}", k + 1)
            } else {
                f.name.clone()
            },
            outline: vec![top_rect(i, *lo, *hi)],
            group: 0,
            ..Default::default()
        })
        .collect()
}

pub fn handles(
    id: &str,
    cx: &Context<'_>,
    info: Option<&SetupInfo>,
    bed: Option<&kmb_post::Bed>,
) -> Option<Vec<Handle>> {
    if !IDS.contains(&id) || id == RESET_ORIGIN || id == CLEAR_BODIES {
        return None;
    }
    let i = info?;
    let sid = picked_setup(cx)?;
    let s = setup_of(cx, sid)?;
    let top = i.to_world([0.0, 0.0, i.stock_max[2]]);
    match id {
        PICK_ORIGIN => {
            let mut snap: Vec<[f64; 3]> = box_points(i, i.stock_min, i.stock_max)
                .into_iter()
                .chain(box_points(i, i.part_min, i.part_max))
                .map(|(p, _)| p)
                .collect();
            snap.extend(part_vertices(cx, i));
            let mut h = Handle::new(
                "point",
                "原点にする点をクリック(ストックと部品の箱の点、部品の角に吸い付く)",
                Gesture::Point {
                    origin: top,
                    normal: [0.0, 0.0, 1.0],
                    snap: 0.5,
                },
            );
            h.snap_to = snap;
            Some(vec![h])
        }
        ADD_BODY => {
            let choices = addable_bodies(cx, &s)
                .into_iter()
                .filter_map(|(fid, h)| {
                    let m = cx.kernel.mesh(h, i.tolerance).ok()?;
                    let mm = kmb_cam_geom::MachiningModel::from_mesh(&m, [0.0; 3]);
                    let name = cx
                        .data
                        .feature(fid)
                        .map(|f| f.name.clone())
                        .unwrap_or_default();
                    Some(HandleChoice {
                        value: serde_json::json!(fid.0),
                        label: name,
                        outline: vec![world_top_rect(mm.min, mm.max)],
                        group: 0,
                        ..Default::default()
                    })
                })
                .collect();
            let mut h = Handle::new(
                "body",
                "一緒に削るボディをクリック(上面の四角の中)",
                Gesture::Choice,
            );
            h.choices = choices;
            Some(vec![h])
        }
        MOVE_FIXTURE | RESIZE_FIXTURE | REMOVE_FIXTURE => {
            let mut pick = Handle::new("fixture", "固定具をクリック", Gesture::Choice);
            pick.choices = fixture_choices(i, &s);
            let mut v = vec![pick];
            let z = i.stock_min[2];
            let c = i.to_world([
                (i.stock_min[0] + i.stock_max[0]) / 2.0,
                (i.stock_min[1] + i.stock_max[1]) / 2.0,
                z,
            ]);
            if id == MOVE_FIXTURE {
                let mut to = Handle::new(
                    "point",
                    "置く所をクリック(ネジ穴の近くでは穴に吸い付く)",
                    Gesture::Point {
                        origin: i.to_world([0.0, 0.0, z]),
                        normal: [0.0, 0.0, 1.0],
                        snap: 1.0,
                    },
                );
                to.snap_to = bed
                    .map(|b| crate::fixtures::bed_holes(b, bed_corner(i)))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| i.to_world(p))
                    .collect();
                v.push(to);
            } else if id == RESIZE_FIXTURE {
                v.push(Handle::new(
                    "width",
                    "幅(X)を打つ",
                    Gesture::Diameter {
                        center: c,
                        normal: [0.0, 0.0, 1.0],
                    },
                ));
                v.push(Handle::new(
                    "depth",
                    "奥行(Y)を打つ",
                    Gesture::Diameter {
                        center: c,
                        normal: [0.0, 0.0, 1.0],
                    },
                ));
                v.push(Handle::new(
                    "height",
                    "高さ(ストックの底から)を打つ",
                    Gesture::Distance {
                        origin: c,
                        dir: [0.0, 0.0, 1.0],
                    },
                ));
            }
            Some(v)
        }
        _ => None,
    }
}

fn arg<T: serde::de::DeserializeOwned>(
    cx: &Context<'_>,
    key: &str,
    what: &str,
) -> Result<T, String> {
    cx.arg(key)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| format!("{what}({key})がありません"))
}

pub fn run(
    id: &str,
    cx: &Context<'_>,
    info: Option<&SetupInfo>,
) -> Option<Result<Outcome, kmb_core::Problem>> {
    if !IDS.contains(&id) {
        return None;
    }
    Some(
        (|| {
            let sid = picked_setup(cx).ok_or("セットアップを選んでいません")?;
            let mut s = setup_of(cx, sid).ok_or("セットアップのデータが読めません")?;
            let need_info = || info.ok_or("セットアップの計算ができていません");
            let fixture = |s: &Setup| -> Result<usize, String> {
                let k: usize = arg(cx, "fixture", "固定具")?;
                (k < s.fixtures.len())
                    .then_some(k)
                    .ok_or_else(|| format!("固定具 {} がありません", k + 1))
            };
            let label = match id {
                PICK_ORIGIN => {
                    let w: [f64; 3] = arg(cx, "point", "原点")?;
                    let o = origin_at(need_info()?, w);
                    s.origin_note = o.describe();
                    s.origin_at = Some(o);
                    "原点を 3D で選ぶ"
                }
                RESET_ORIGIN => {
                    s.origin_at = None;
                    s.origin_note.clear();
                    "原点を一覧の選び方に戻す"
                }
                ADD_BODY => {
                    let b: u64 = arg(cx, "body", "ボディ")?;
                    let b = FeatureId(b);
                    if !matches!(cx.output(b, "body"), Some(Value::Shape(_))) {
                        return Err(format!("{b} はボディではありません"));
                    }
                    if b != s.body && !s.more_bodies.contains(&b) {
                        s.more_bodies.push(b);
                    }
                    "削るボディを足す"
                }
                CLEAR_BODIES => {
                    s.more_bodies.clear();
                    "足したボディを外す"
                }
                MOVE_FIXTURE => {
                    let k = fixture(&s)?;
                    let w: [f64; 3] = arg(cx, "point", "置く所")?;
                    let q = need_info()?.to_setup(w);
                    s.fixtures[k].at = [round_um(q[0]), round_um(q[1])];
                    "固定具を動かす"
                }
                RESIZE_FIXTURE => {
                    let k = fixture(&s)?;
                    let w: f64 = arg(cx, "width", "幅")?;
                    let d: f64 = arg(cx, "depth", "奥行")?;
                    let h: f64 = arg(cx, "height", "高さ")?;
                    if !(w > 0.0 && d > 0.0 && h > 0.0) {
                        return Err("固定具の幅・奥行・高さは 0 より大きくしてください".into());
                    }
                    s.fixtures[k].size = [round_um(w), round_um(d), round_um(h)];
                    "固定具の大きさを変える"
                }
                REMOVE_FIXTURE => {
                    let k = fixture(&s)?;
                    s.fixtures.remove(k);
                    "固定具を 1 つ外す"
                }
                _ => unreachable!("IDS で絞った"),
            };
            Ok(Outcome::Change {
                label: label.into(),
                edits: vec![Edit::SetData {
                    id: sid,
                    data: serde_json::to_value(&s).expect("JSON にできる"),
                }],
            })
        })()
        .map_err(Into::into),
    )
}

/// L 字の基準の角(セットアップの座標)。ストックの手前左の下の角から、ベッドの上の置き場所だけ戻した所。
pub fn bed_corner(i: &SetupInfo) -> [f64; 3] {
    i.fence_corner()
}
