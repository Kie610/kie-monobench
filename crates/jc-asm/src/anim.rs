//! アニメーションの段を足す操作(作業空間「アニメーション」の部品の変換とジョイントの動き)。
//! 段の中身と動かし方は [`jc_interchange::animation`]。
//!
//! - 分解の移動(`anim.explode`): 動かす部品の面 → 向き(X・Y・Z の矢印)→ 距離。分解の段を足す。
//!   向きと距離は利用者が決める(自動で決めるのは下の自動分解)。
//! - ジョイントの動き(`anim.drive`): 軸の線をクリックしたジョイントの値を、下限から上限へ動かす段を足す。
//! - 自動分解(`anim.auto_explode_one`・`anim.auto_explode_all`。1 レベルとすべてのレベル): 部品の外形の箱の中心が組立の中心からどちらにずれているかで向きを決め、
//!   同じ向きへ離す物は近い順に大きさを積んで距離にする([`auto_moves`])。ワンステップ(全部同時の
//!   段 1 つ)か順次(部品ごとに 1 秒の段)で足す。
//!
//! - ビュー(`anim.view`): 画面の今の視点(引数 `view`)へ動かす段を足す。
//!   画面の段の帯の「記録」を入れている間は、視点を動かし終えるたびに画面がこの操作を呼ぶ。
//! - 表示/非表示(`anim.visibility`): 選んだ部品を段の始めに隠す(隠れていれば見せる)段を足す。
//! - 外観(`anim.appearance`): 選んだ部品の外観を段の長さをかけて替える段を足す。外観は段の帯で選び直す。
//!
//! 分解の移動とジョイントの動きは段の帯の今の時刻(引数 `time`、秒)の位置で矢印・軸を描く。
//! 足した段は帯の最後に付ける。
//!
//! 自動分解の決め方は、docs/避ける形.md の次の形を使わない: 視点から見た隠れ、
//! 部品を少しずつ動かしてぶつかりを試す分解、分解の道筋の候補を並べて選ばせる。外形の箱だけで決める。

use crate::joint::{JOINT, apply_dir};
use crate::part::Part;
use jc_core::kernel::{self as k, P3};
use jc_core::{Context, FeatureId, Gesture, Handle, HandleChoice, Outcome, body_root};
use jc_interchange::animation::{self as anim, Step, StepKind, Target};
use serde_json::json;

/// 分解の段を足す操作。
pub const EXPLODE: &str = "anim.explode";
/// ジョイントの動きの段を足す操作。
pub const DRIVE: &str = "anim.drive";
/// 自動分解(1 レベル)の操作。
pub const AUTO_EXPLODE_ONE: &str = "anim.auto_explode_one";
/// 自動分解(すべてのレベル)の操作。
pub const AUTO_EXPLODE_ALL: &str = "anim.auto_explode_all";
/// ビューの段を足す操作。
pub const VIEW: &str = "anim.view";
/// 表示/非表示の段を足す操作。
pub const VISIBILITY: &str = "anim.visibility";
/// 外観の段を足す操作。
pub const APPEARANCE: &str = "anim.appearance";
/// ジョイントの動きの段の長さの既定(秒)。
const DRIVE_SECONDS: f64 = 2.0;
/// 表示/非表示の段の長さの既定(秒。段の始めに替わり、残りは次の段までの間)。
const VISIBILITY_SECONDS: f64 = 0.5;

/// 段の帯の今の時刻(秒)。
fn time(cx: &Context<'_>) -> f64 {
    cx.arg("time").and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// 今の時刻の、ボディ(最初に作った機能)の動き。
fn pose_of(cx: &Context<'_>, root: FeatureId) -> k::Matrix4 {
    anim::poses(
        cx.data,
        cx.registry,
        cx.report,
        &anim::steps(cx.data),
        time(cx),
    )
    .get(&root)
    .copied()
    .unwrap_or(k::IDENTITY)
}

/// 部品のボディを包む箱(動かす前の位置、世界座標)。
fn part_bounds(cx: &Context<'_>, roots: &[FeatureId]) -> Option<(P3, P3)> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (id, h) in cx.visible_bodies() {
        if !roots.contains(&body_root(cx.data, cx.registry, id)) {
            continue;
        }
        let Ok((a, b)) = cx.kernel.bounds(h) else {
            continue;
        };
        for i in 0..3 {
            lo[i] = lo[i].min(a[i]);
            hi[i] = hi[i].max(b[i]);
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

/// クリックした面の部品(と中心・大きさ。今の時刻の位置)。
fn picked_part(cx: &Context<'_>) -> Option<(Part, P3, f64)> {
    let p = cx.picked?;
    let root = body_root(cx.data, cx.registry, p.feature);
    let part = Part::of_body(cx.data, root);
    let roots = part.roots(cx.data);
    let (lo, hi) = part_bounds(cx, &roots)?;
    let center = k::transform_point(&pose_of(cx, root), k::scale(k::add(lo, hi), 0.5));
    let size = (0..3).map(|i| hi[i] - lo[i]).fold(0.0, f64::max).max(1.0);
    Some((part, center, size))
}

const AXES: [(&str, P3, [u8; 3]); 6] = [
    ("X の向き", [1.0, 0.0, 0.0], [220, 60, 60]),
    ("-X の向き", [-1.0, 0.0, 0.0], [220, 60, 60]),
    ("Y の向き", [0.0, 1.0, 0.0], [60, 170, 60]),
    ("-Y の向き", [0.0, -1.0, 0.0], [60, 170, 60]),
    ("Z の向き", [0.0, 0.0, 1.0], [60, 100, 230]),
    ("-Z の向き", [0.0, 0.0, -1.0], [60, 100, 230]),
];

/// 矢印の線(軸と、先の 2 本のはね)。
fn arrow(from: P3, dir: P3, len: f64) -> Vec<Vec<P3>> {
    let tip = k::add(from, k::scale(dir, len));
    // はねは、向きに直角な軸のうち、向きといちばん離れた物の向きに開く
    let side = if dir[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let back = k::scale(dir, -0.18 * len);
    let s = k::scale(side, 0.1 * len);
    vec![
        vec![from, tip],
        vec![tip, k::add(k::add(tip, back), s)],
        vec![tip, k::sub(k::add(tip, back), s)],
    ]
}

/// 分解の移動の手順: 部品の面 → 向き → 距離。
pub fn explode_handles(cx: &Context<'_>) -> Vec<Handle> {
    let mut dir = Handle::new(
        "dir",
        "動かす向きの矢印をクリック(X は赤、Y は緑、Z は青)",
        Gesture::Choice,
    );
    let mut distance = Handle::new(
        "distance",
        "マウスで距離を決めてクリック(数字を打って Enter でもよい)",
        Gesture::Distance {
            origin: [0.0; 3],
            dir: [0.0, 0.0, 1.0],
        },
    );
    if cx.arg("part").is_some()
        && let Some((_, center, size)) = picked_part(cx)
    {
        let len = size * 0.6 + 10.0;
        dir.choices = AXES
            .iter()
            .map(|(label, d, color)| HandleChoice {
                value: json!(d),
                label: (*label).into(),
                outline: arrow(center, *d, len),
                group: 0,
                sized: false,
                color: Some(*color),
            })
            .collect();
        if let Some(d) = cx.arg("dir").and_then(vec3) {
            distance.gesture = Gesture::Distance {
                origin: center,
                dir: d,
            };
            distance.outline = vec![vec![center, k::add(center, k::scale(d, len))]];
        }
    }
    vec![
        Handle::new("part", "動かす部品の面をクリック", Gesture::AnyFace),
        dir,
        distance,
    ]
}

fn vec3(v: &serde_json::Value) -> Option<P3> {
    let a = v.as_array()?;
    let c: Vec<f64> = a.iter().filter_map(|x| x.as_f64()).collect();
    (c.len() == 3 && c.iter().all(|x| x.is_finite())).then(|| [c[0], c[1], c[2]])
}

fn with_step(cx: &Context<'_>, step: Step, label: String) -> Result<Outcome, String> {
    anim::check_step(&step)?;
    let mut steps = anim::steps(cx.data);
    steps.push(step);
    Ok(Outcome::Change {
        label,
        edits: vec![anim::set_steps(cx.data, steps)?],
    })
}

/// 分解の段を足す。引数 `dir`(向き)と `distance`(mm)、動かす物は選んだ面の部品か `targets`。
pub fn explode(cx: &Context<'_>) -> Result<Outcome, String> {
    let dir = cx
        .arg("dir")
        .and_then(vec3)
        .ok_or("向き(dir)を選んでいません")?;
    if k::dot(dir, dir) < 1e-18 {
        return Err("向きが 0 です".into());
    }
    let d = cx
        .arg("distance")
        .and_then(|v| v.as_f64())
        .ok_or("距離(distance)を決めていません")?;
    if d.abs() < 1e-9 {
        return Err("距離が 0 なので、段を足しませんでした".into());
    }
    let targets: Vec<Target> = match cx.arg("targets") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("targets: {e}"))?,
        None => {
            let (part, _, _) = picked_part(cx).ok_or("動かす部品の面を選んでいません")?;
            vec![match part {
                Part::Component { id } => Target::Component { id },
                Part::Body { root } => Target::Body { root },
            }]
        }
    };
    let name = targets
        .first()
        .map(|t| target_label(cx, t))
        .unwrap_or_default();
    let offset = k::scale(k::normalize(dir), d);
    log::info!("分解の段を足す: {name} を {offset:?} mm");
    with_step(
        cx,
        Step::new(StepKind::Explode { targets, offset }, anim::STEP_SECONDS),
        format!("分解の移動({name})"),
    )
}

/// 表示/非表示・外観の対象: 引数 `targets`、無ければクリックした面(Ctrl で足した面も)の部品、
/// 無ければブラウザで選んだボディの部品。同じ物は 1 回だけ。
fn chosen_targets(cx: &Context<'_>) -> Result<Vec<Target>, String> {
    if let Some(v) = cx.arg("targets") {
        return serde_json::from_value(v.clone()).map_err(|e| format!("targets: {e}"));
    }
    let mut roots: Vec<FeatureId> = cx
        .picked
        .into_iter()
        .chain(cx.also_picked.iter())
        .map(|p| p.feature)
        .collect();
    if roots.is_empty() {
        roots = cx
            .selected
            .into_iter()
            .chain(cx.also_selected.iter().copied())
            .filter(|f| cx.visible_bodies().iter().any(|(b, _)| b == f))
            .collect();
    }
    let mut out: Vec<Target> = Vec::new();
    for f in roots {
        let t = match Part::of_body(cx.data, body_root(cx.data, cx.registry, f)) {
            Part::Component { id } => Target::Component { id },
            Part::Body { root } => Target::Body { root },
        };
        if !out.contains(&t) {
            out.push(t);
        }
    }
    Ok(out)
}

/// 表示/非表示・外観の手順: 部品を選んでいなければ、部品の面をクリックさせる。
pub fn part_handles(cx: &Context<'_>) -> Vec<Handle> {
    if cx.arg("part").is_some() || chosen_targets(cx).is_ok_and(|t| !t.is_empty()) {
        return Vec::new();
    }
    vec![Handle::new(
        "part",
        "部品の面をクリック(Ctrl で足す。隠れている部品はブラウザで選ぶ)",
        Gesture::AnyFace,
    )]
}

fn names(cx: &Context<'_>, targets: &[Target]) -> String {
    let n: Vec<String> = targets.iter().map(|t| target_label(cx, t)).collect();
    if n.len() > 3 {
        format!("{} ほか", n[..3].join("・"))
    } else {
        n.join("・")
    }
}

/// ビューの段を足す。引数 `view`(視点 {target, yaw, pitch, height, perspective}。画面が今の視点を渡す)。
pub fn view(cx: &Context<'_>) -> Result<Outcome, String> {
    let v = cx.arg("view").ok_or("視点(view)がありません")?;
    let view: anim::ViewPose =
        serde_json::from_value(v.clone()).map_err(|e| format!("view: {e}"))?;
    // 前のビューの段と同じ視点なら足さない(記録を入れたままクリックしただけの時)
    let same = anim::steps(cx.data)
        .into_iter()
        .rev()
        .find_map(|s| match &s.kind {
            StepKind::View { view } => Some(*view),
            _ => None,
        });
    if same.is_some_and(|p| p.near(&view)) {
        return Err("前のビューの段と同じ視点なので、段を足しませんでした".into());
    }
    log::info!("ビューの段を足す: {view:?}");
    with_step(
        cx,
        Step::new(StepKind::View { view }, anim::STEP_SECONDS),
        "ビュー".into(),
    )
}

/// 表示/非表示の段を足す。引数 `visible`(省くと、最初の物が今の時刻に見えていれば隠し、
/// 隠れていれば見せる)。
pub fn visibility(cx: &Context<'_>) -> Result<Outcome, String> {
    let targets = chosen_targets(cx)?;
    if targets.is_empty() {
        return Err("見せる・隠す部品を選んでいません".into());
    }
    let visible = match cx.arg("visible").and_then(|v| v.as_bool()) {
        Some(v) => v,
        None => {
            let now = anim::shown_at(cx.data, &anim::steps(cx.data), time(cx));
            let first = targets[0].roots(cx.data);
            !first.iter().all(|r| now.get(r).copied().unwrap_or(true))
        }
    };
    let name = names(cx, &targets);
    log::info!(
        "表示/非表示の段を足す: {name} を {}",
        if visible { "見せる" } else { "隠す" }
    );
    with_step(
        cx,
        Step::new(
            StepKind::Visibility { targets, visible },
            VISIBILITY_SECONDS,
        ),
        format!("{}({name})", if visible { "表示" } else { "非表示" }),
    )
}

/// 外観の段を足す。引数 `appearance`(外観の名前。省くと、今の外観と違う最初の塗装の色)。
pub fn appearance(cx: &Context<'_>) -> Result<Outcome, String> {
    let targets = chosen_targets(cx)?;
    if targets.is_empty() {
        return Err("外観を替える部品を選んでいません".into());
    }
    let appearance = match cx.arg("appearance").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => {
            let now = anim::appearances_at(cx.data, &anim::steps(cx.data), time(cx));
            let r = targets[0].roots(cx.data).first().copied();
            let cur = r.and_then(|r| {
                now.get(&r)
                    .map(|b| b.to.clone())
                    .or_else(|| jc_look::appearance::appearance_of(cx.data, r))
            });
            jc_look::material::PAINTS
                .iter()
                .find(|p| cur.as_deref() != Some(p.name))
                .map_or("赤", |p| p.name)
                .to_string()
        }
    };
    let name = names(cx, &targets);
    log::info!("外観の段を足す: {name} を {appearance}");
    with_step(
        cx,
        Step::new(
            StepKind::Appearance {
                targets,
                appearance: appearance.clone(),
            },
            anim::STEP_SECONDS,
        ),
        format!("外観({name} を {appearance})"),
    )
}

/// 動かす物の名前(部品の名前か、ボディの名前)。
pub fn target_label(cx: &Context<'_>, t: &Target) -> String {
    match t {
        Target::Component { id } => Part::Component { id: *id },
        Target::Body { root } => Part::Body { root: *root },
    }
    .label(cx.data, cx.registry, cx.report)
}

/// 動かせるジョイント(回転・スライダ。タイムラインの順)と今の様子。
pub(crate) fn drives(cx: &Context<'_>) -> Vec<(FeatureId, String, anim::Drive)> {
    cx.data
        .features
        .iter()
        .filter(|f| f.type_id == JOINT && !f.suppressed)
        .filter_map(|f| Some((f.id, f.name.clone(), anim::drive_of(cx.report, f.id)?)))
        .collect()
}

/// ジョイントの動きの手順: ジョイントの軸の線をクリック。
pub fn drive_handles(cx: &Context<'_>) -> Vec<Handle> {
    let mut h = Handle::new(
        "joint",
        "動かすジョイントの軸の線をクリック(回転とスライダのジョイント)",
        Gesture::Choice,
    );
    for (id, name, d) in drives(cx) {
        // 軸は今の時刻の位置に描く(前の段で動いた部品の上)
        let root = d
            .bodies
            .first()
            .map(|b| body_root(cx.data, cx.registry, *b));
        let base = root.map_or(k::IDENTITY, |r| pose_of(cx, r));
        let o = k::transform_point(&base, d.origin);
        let a = apply_dir(&base, d.axis);
        let half = k::scale(a, 40.0);
        h.choices.push(HandleChoice {
            value: json!(id.0),
            label: format!("{name}({})", if d.slide { "スライダ" } else { "回転" }),
            outline: vec![vec![k::sub(o, half), k::add(o, half)]],
            group: 0,
            sized: false,
            color: Some([230, 140, 30]),
        });
    }
    vec![h]
}

/// ジョイントの動きの段を足す。引数 `joint`(ジョイントの番号)、`from`・`to`(度か mm。省くと
/// 下限から上限、無ければ今の値から 90 度か動く部品の長さ)。
pub fn drive(cx: &Context<'_>) -> Result<Outcome, String> {
    let id = FeatureId(
        cx.arg("joint")
            .and_then(|v| v.as_u64())
            .ok_or("ジョイント(joint)を選んでいません")?,
    );
    let (_, name, d) = drives(cx)
        .into_iter()
        .find(|(j, _, _)| *j == id)
        .ok_or("回転かスライダのジョイントではありません")?;
    // スライダの既定の動き: 動く部品の軸の向きの長さ
    let roots: Vec<FeatureId> = d
        .bodies
        .iter()
        .map(|b| body_root(cx.data, cx.registry, *b))
        .collect();
    let span = part_bounds(cx, &roots).map_or(10.0, |(lo, hi)| {
        k::dot(
            k::sub(hi, lo),
            [d.axis[0].abs(), d.axis[1].abs(), d.axis[2].abs()],
        )
    });
    let (lo, hi) = d.default_range(span);
    let from = cx.arg("from").and_then(|v| v.as_f64()).unwrap_or(lo);
    let to = cx.arg("to").and_then(|v| v.as_f64()).unwrap_or(hi);
    log::info!("ジョイントの動きの段を足す: {name} を {from} から {to} へ");
    with_step(
        cx,
        Step::new(
            StepKind::Drive {
                joint: id,
                from,
                to,
            },
            DRIVE_SECONDS,
        ),
        format!("ジョイントの動き({name})"),
    )
}

/// 自動分解で一緒に動く物(部品 1 つか、部品に入っていないボディ 1 つか、部品の直下のボディ)。
struct Unit {
    targets: Vec<Target>,
    lo: P3,
    hi: P3,
}

/// 部品 `parent`(`None` は設計の全体)の直下で、分けて動かす物。部品の直下のボディは 1 つにまとめ、
/// 設計の全体の直下(部品に入っていない)のボディは 1 つずつ。見えている物だけ。
fn units_under(cx: &Context<'_>, parent: Option<u64>) -> Vec<Unit> {
    let data = cx.data;
    let mut groups: Vec<Vec<Target>> = data
        .components
        .iter()
        .filter(|c| c.parent == parent)
        .map(|c| vec![Target::Component { id: c.id }])
        .collect();
    match parent {
        Some(p) => {
            if let Some(c) = data.component(p)
                && !c.bodies.is_empty()
            {
                groups.push(c.bodies.iter().map(|r| Target::Body { root: *r }).collect());
            }
        }
        None => {
            let mut loose: Vec<FeatureId> = Vec::new();
            for (id, _) in cx.visible_bodies() {
                let r = body_root(data, cx.registry, id);
                if data.component_of(r).is_none() && !loose.contains(&r) {
                    loose.push(r);
                }
            }
            groups.extend(loose.into_iter().map(|root| vec![Target::Body { root }]));
        }
    }
    groups
        .into_iter()
        .filter_map(|targets| {
            let roots: Vec<FeatureId> = targets.iter().flat_map(|t| t.roots(data)).collect();
            let (lo, hi) = part_bounds(cx, &roots)?;
            Some(Unit { targets, lo, hi })
        })
        .collect()
}

/// 並んだ物を組立の中心から離す動き(`units` と同じ順)。
///
/// 向き: 物の外形の箱の中心が、全部を包む箱の中心からいちばん大きくずれている軸の、ずれている側
/// (同じくらいなら Z・Y・X の順に選ぶ。中心に重なる物は +Z)。距離: 同じ向きへ離す物を中心に近い順に
/// 並べ、その向きの長さを積んだ物に `scale` を掛ける(近い物の分だけ遠い物が先へ出るので重ならない)。
/// 距離は 0.01 mm に丸める。
pub fn spread(boxes: &[(P3, P3)], scale: f64) -> Vec<P3> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (a, b) in boxes {
        for i in 0..3 {
            lo[i] = lo[i].min(a[i]);
            hi[i] = hi[i].max(b[i]);
        }
    }
    let center = k::scale(k::add(lo, hi), 0.5);
    let size = (0..3).map(|i| hi[i] - lo[i]).fold(0.0, f64::max).max(1.0);
    let tol = size * 1e-6;
    // (軸, 向きの符号, 中心からのずれ)
    let dirs: Vec<(usize, f64, f64)> = boxes
        .iter()
        .map(|(a, b)| {
            let c = k::scale(k::add(*a, *b), 0.5);
            let v = k::sub(c, center);
            let mut best = 2;
            for i in [1, 0] {
                if v[i].abs() > v[best].abs() + tol {
                    best = i;
                }
            }
            if v[best].abs() <= tol {
                (2, 1.0, 0.0)
            } else {
                (best, v[best].signum(), v[best].abs())
            }
        })
        .collect();
    let mut out = vec![[0.0; 3]; boxes.len()];
    for axis in 0..3 {
        for sign in [1.0, -1.0] {
            let mut group: Vec<usize> = (0..boxes.len())
                .filter(|&i| dirs[i].0 == axis && dirs[i].1 == sign)
                .collect();
            group.sort_by(|&a, &b| dirs[a].2.total_cmp(&dirs[b].2).then(a.cmp(&b)));
            let mut sum = 0.0;
            for i in group {
                let (a, b) = boxes[i];
                sum += (b[axis] - a[axis]).max(1.0);
                // 0.01 mm に丸める(段の名前と欄が「30 mm」と読めるように。外形の箱は少し大きめに出る)
                out[i][axis] = (sign * sum * scale * 100.0).round() / 100.0;
            }
        }
    }
    out
}

/// 自動分解の動きの並び(動かす物, 動き)。`all` が偽なら 1 レベル(直下の物だけ。直下が 1 つしか
/// なければ 2 つ以上ある所まで下りる)、真ならすべてのレベル(部品の中も、その部品の中心から離す。
/// 中の物は外の部品の動きに足して動く)。並びは部品の木の上から順。
pub fn auto_moves(cx: &Context<'_>, all: bool, scale: f64) -> Vec<(Vec<Target>, P3)> {
    let mut out = Vec::new();
    let mut queue: std::collections::VecDeque<Option<u64>> = [None].into();
    while let Some(parent) = queue.pop_front() {
        let units = units_under(cx, parent);
        let children = units.iter().filter_map(|u| match u.targets.as_slice() {
            [Target::Component { id }] => Some(Some(*id)),
            _ => None,
        });
        if units.len() >= 2 {
            let boxes: Vec<(P3, P3)> = units.iter().map(|u| (u.lo, u.hi)).collect();
            for (u, o) in units.iter().zip(spread(&boxes, scale)) {
                out.push((u.targets.clone(), o));
            }
            if !all {
                break;
            }
            queue.extend(children);
        } else {
            // 分けられる物が 1 つだけなら、その中へ下りる(1 レベルでも)
            queue.extend(children);
        }
    }
    out
}

/// 自動分解の段を足す。引数 `sequential`(真なら部品ごとに 1 秒の段、偽なら全部同時の段 1 つ)・
/// `trail`(跡の線。既定は真)・`scale`(分解の目盛。距離の倍率、既定 1)。
pub fn auto_explode(cx: &Context<'_>, all: bool) -> Result<Outcome, String> {
    let sequential = cx
        .arg("sequential")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let trail = cx.arg("trail").and_then(|v| v.as_bool()).unwrap_or(true);
    let scale = cx.arg("scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
    if !(scale.is_finite() && scale > 0.0) {
        return Err(format!("分解の目盛({scale})は 0 より大きくしてください"));
    }
    let moves = auto_moves(cx, all, scale);
    if moves.is_empty() {
        return Err(
            "分けて動かせる物が 2 つ以上ありません(部品か、部品に入っていないボディが 2 つ以上要ります)"
                .into(),
        );
    }
    let level = if all {
        "すべてのレベル"
    } else {
        "1 レベル"
    };
    let how = if sequential {
        "順次"
    } else {
        "ワンステップ"
    };
    log::info!(
        "自動分解({level}・{how}・目盛 {scale}・跡の線 {trail}): {} 個",
        moves.len()
    );
    let mut steps = anim::steps(cx.data);
    if sequential {
        for (targets, offset) in moves.iter().cloned() {
            let step = Step {
                trail,
                ..Step::new(StepKind::Explode { targets, offset }, anim::STEP_SECONDS)
            };
            anim::check_step(&step)?;
            steps.push(step);
        }
    } else {
        let step = Step {
            trail,
            ..Step::new(
                StepKind::ExplodeMany {
                    moves: moves
                        .iter()
                        .cloned()
                        .map(|(targets, offset)| anim::Move { targets, offset })
                        .collect(),
                },
                anim::STEP_SECONDS,
            )
        };
        anim::check_step(&step)?;
        steps.push(step);
    }
    Ok(Outcome::Change {
        label: format!("自動分解({level}・{how}、{} 個)", moves.len()),
        edits: vec![anim::set_steps(cx.data, steps)?],
    })
}
