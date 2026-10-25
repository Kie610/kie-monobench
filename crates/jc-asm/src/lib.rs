//! 組立。
//!
//! - 部品の木の操作: 選んだボディから新しい部品を作る(`asm.new_component`)。部品の木そのものは
//!   文書(`DocumentData::components`)にあり、名前・色・ボディの出し入れは画面が直接変える。
//! - ジョイント(`asm.joint@1`。[`joint`]): 固定・回転・スライダ・円柱・ピンスロット・平面・ボール。
//!   部品 1 の原点を部品 2 の原点に合わせて動かす作り方(`asm.snap_joint`。原点の候補は [`snap`])と、
//!   置いた位置のまま結ぶ作り方(`asm.joint`)がある。値を変えると動く。
//! - ジョイントの無い部品を引いて動かした位置(`asm.position@1`。[`position`])。
//! - 干渉の確認(`asm.interference`。表と、重なった所を 3D に色で。引数 `coincident` で面で接する組も)と部品表(`asm.bom`・`asm.bom_csv`。[`inspect`])。
//! - 別ファイルの部品をリンクで組み込む(`asm.link@1`。[`link`])。
//! - アニメーションの段を足す操作(`anim.explode`・`anim.drive`。[`anim`])。作業空間「アニメーション」に出す。
//! - 規格部品(ねじ・ナット・座金・玉軸受)を穴に挿す(`asm.fastener@1`。[`fastener`])。
//! - 選んだ面の組から位置を解く拘束(`asm.constraint@1`。[`constraint`])、部品を 1 つの塊にする
//!   剛性グループ(`asm.rigid_group@1`。[`rigid`])、2 つのジョイントを比でつなぐモーション リンク
//!   ([`motion_link`])。
//! - モーション スタディ([`motion`])と接触セット([`contact`])。中身は文書の区画に持つ(形は変えない)。

pub mod anim;
pub mod constraint;
pub mod contact;
pub mod fastener;
pub mod inspect;
pub mod joint;
pub mod link;
pub mod motion;
pub mod motion_link;
pub mod part;
pub mod position;
pub mod rigid;
pub mod snap;

pub use fastener::{FASTENER, FASTENER_BODY, Fastener, FastenerBody};
pub use inspect::{
    BomRow, Clash, Unchecked, bom_csv, bom_rows, bom_table, interference_table, interferences,
    interferences_with,
};
pub use joint::{Frame, How, JOINT, Joint, JointKind, Origin, Snap, TRANSFORM_TYPE};
pub use link::{LINK, LINK_BODY, Link, LinkBody, LinkCommands};
pub use part::{Part, heir_before};
pub use position::{DRAG_POSITION, MOVE_PART, POSITION, Position};
pub use snap::SnapPick;

use jc_core::kernel::{self as k};
use jc_core::{
    Action, Arg, Component, Context, Edit, Extension, FeatureId, Gesture, Handle, HandleChoice,
    Input, Outcome, Place, Registry, body_root,
};
use serde_json::json;

/// 選んでいるボディを新しい部品にまとめる操作(名前を打たせる)。
pub const NEW_COMPONENT: &str = "asm.new_component";
/// 置いた位置のままジョイントを作る操作(動かす部品の面 → 軸 → 土台の部品の面)。
pub const NEW_JOINT: &str = "asm.joint";
/// 部品 1 を部品 2 に合わせてジョイントを作る操作(部品 1 の面 → 原点 → 部品 2 の面 → 原点)。
pub const NEW_SNAP_JOINT: &str = "asm.snap_joint";
/// 見えているボディの組の重なりを調べる操作。
pub const INTERFERENCE: &str = "asm.interference";
/// ジョイントの一覧を表に出す操作(何と何をつないでいるか。MCP の AI が組立を読む入口)。
pub const JOINTS: &str = "asm.joints";
/// 部品表を窓に出す操作。
pub const BOM: &str = "asm.bom";
/// 部品表を CSV に書き出す操作。
pub const BOM_CSV: &str = "asm.bom_csv";
/// 規格部品を穴に挿す操作(穴の口の面 → 穴の口の円)。
pub const NEW_FASTENER: &str = "asm.fastener";

pub fn register(reg: &mut Registry) {
    // 文書の区画(モーション スタディと接触セット。中身の型は jc-interchange)
    reg.declare_section::<jc_interchange::motion::MotionStudies>();
    reg.declare_section::<jc_interchange::contact::Contacts>();
    reg.declare_section::<jc_interchange::animation::Storyboard>();
    joint::register(reg);
    link::register(reg);
    fastener::register(reg);
    position::register(reg);
    rigid::register(reg);
    constraint::register(reg);
}

/// 規格部品の手順: 穴の口の面 → 穴の口の円(クリックで挿す。いくつも挿す時は Ctrl で選んで Enter)。
fn fastener_handles(cx: &Context<'_>) -> Vec<Handle> {
    let mut holes = Handle::new(
        "holes",
        "挿す穴の口の円をクリックすると挿す(いくつも選ぶ時は Ctrl を押しながら。Ctrl で選んだ後は Enter でも挿せる)",
        Gesture::Regions,
    );
    if cx.arg("face").is_some() {
        holes.choices = fastener::mouth_choices(cx);
    }
    vec![
        Handle::new(
            "face",
            "穴の口の平らな面か、穴の内側の面をクリック",
            Gesture::AnyFace,
        ),
        holes,
    ]
}

/// 規格部品を挿す: 規格部品の機能と、穴ごとのボディの機能を足し、ボディを新しい部品にまとめる
/// (穴のボディが部品に入っていれば、その中に。ジョイントで一緒に動く)。
fn new_fastener(cx: &Context<'_>) -> Result<Outcome, String> {
    let mouths: Vec<fastener::HoleMouth> = cx
        .arg("holes")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    if mouths.is_empty() {
        return Err("挿す穴の口の円を選んでいません".into());
    }
    let kind = match cx.arg_str("kind") {
        Some(k) => {
            serde_json::from_value(json!(k)).map_err(|_| format!("種類 `{k}` が分かりません"))?
        }
        // 皿の穴の縁なら皿小ねじ
        None if mouths[0].countersink => fastener::Kind::FlatHead,
        None => fastener::Kind::default(),
    };
    let parent = cx
        .picked
        .and_then(|p| {
            cx.data
                .component_of(body_root(cx.data, cx.registry, p.feature))
        })
        .map(|c| c.id);
    let mut f = fastener::new_fastener(kind, &mouths);
    // 穴のあるボディを覚え、穴の径が後で呼びと合わなくなったら知らせる(合わせ直しはしない)。
    // 同じ穴の口に座金・ナットが挿してあれば、その上に重ねる
    for (p, m) in f.places.iter_mut().zip(&mouths) {
        p.body = cx.picked.map(|p| p.feature);
        p.on = fastener::stack_on(cx.data, kind, m);
    }
    let n = cx.count_of(fastener::FASTENER) + 1;
    let name = format!("規格部品{n}");
    log::info!(
        "{name}: {} を {} か所に挿す",
        f.designation(None),
        mouths.len()
    );
    let mut edits = vec![Edit::Insert {
        type_id: fastener::FASTENER.into(),
        name: name.clone(),
        data: serde_json::to_value(&f).map_err(|e| e.to_string())?,
    }];
    for i in 0..mouths.len() {
        edits.push(Edit::Insert {
            type_id: fastener::FASTENER_BODY.into(),
            name: format!("{name} の穴{}", i + 1),
            data: json!({"fastener": {"$new": 0}, "index": i}),
        });
    }
    edits.push(Edit::AddBodies {
        component: None,
        name: name.clone(),
        parent,
        bodies: (0..mouths.len())
            .map(|i| (i + 1, format!("穴{}", i + 1)))
            .collect(),
    });
    Ok(Outcome::Change {
        label: format!("{name} を挿す"),
        edits,
    })
}

/// 選んでいる面のボディと選んでいるボディ(最初に作った機能。重ねない)。指定 `bodies` があれば
/// それだけ。
fn picked_roots(cx: &Context<'_>) -> Vec<FeatureId> {
    let shown: Vec<FeatureId> = cx.visible_bodies().iter().map(|b| b.0).collect();
    let listed: Option<Vec<FeatureId>> = cx
        .arg("bodies")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_u64().map(FeatureId)).collect());
    let ids: Vec<FeatureId> = match listed {
        Some(l) => l,
        None => cx
            .picked
            .into_iter()
            .chain(cx.also_picked.iter())
            .map(|p| p.feature)
            .chain(cx.selected)
            .chain(cx.also_selected.iter().copied())
            .collect(),
    };
    unique_roots(
        cx,
        ids.into_iter().filter(|id| {
            shown.contains(id)
                || shown
                    .iter()
                    .any(|s| body_root(cx.data, cx.registry, *s) == *id)
        }),
    )
}

/// 選んでいる見えているボディ(最初に作った機能。重ねない)。名前を打たせた操作は指定 `selection`
/// (ボディの最後の機能でも最初に作った機能でもよい)。
fn selected_roots(cx: &Context<'_>) -> Vec<FeatureId> {
    let bodies: Vec<FeatureId> = cx.visible_bodies().iter().map(|b| b.0).collect();
    let shown = |id: &FeatureId| {
        bodies.contains(id)
            || bodies
                .iter()
                .any(|b| body_root(cx.data, cx.registry, *b) == *id)
    };
    let mut ids: Vec<FeatureId> = match cx.arg("selection").and_then(|v| v.as_array()) {
        Some(list) => list
            .iter()
            .filter_map(|v| v.as_u64().map(FeatureId))
            .collect(),
        None => cx
            .selected
            .into_iter()
            .chain(cx.also_selected.iter().copied())
            .collect(),
    };
    ids.retain(|id| shown(id));
    unique_roots(cx, ids)
}

/// ボディの最初に作った機能(順はそのまま、重ねない)。
fn unique_roots(cx: &Context<'_>, ids: impl IntoIterator<Item = FeatureId>) -> Vec<FeatureId> {
    let mut roots: Vec<FeatureId> = Vec::new();
    for id in ids {
        let r = body_root(cx.data, cx.registry, id);
        if !roots.contains(&r) {
            roots.push(r);
        }
    }
    roots
}

/// 新しい部品の名前の既定(「部品N」。使っていない番号)。
fn next_component_name(cx: &Context<'_>) -> String {
    (1..)
        .map(|n| format!("部品{n}"))
        .find(|n| !cx.data.components.iter().any(|c| c.name == *n))
        .expect("番号はいつか空く")
}

fn new_component(cx: &Context<'_>) -> Result<Outcome, String> {
    let name = cx
        .arg_str("name")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(|| next_component_name(cx), str::to_string);
    let roots = selected_roots(cx);
    let parent = match cx.arg("parent").and_then(|v| v.as_u64()) {
        Some(p) if cx.data.components.iter().any(|c| c.id == p) => Some(p),
        Some(p) => return Err(format!("親にする部品 {p} がありません")),
        // 選んだボディがみな同じ部品に入っていれば、その中に作る
        None => {
            let parents: Vec<Option<u64>> = roots
                .iter()
                .map(|r| cx.data.component_of(*r).map(|c| c.id))
                .collect();
            match parents.first() {
                Some(p) if parents.iter().all(|q| q == p) => *p,
                _ => None,
            }
        }
    };
    let mut list = cx.data.components_moving(&roots, None);
    list.push(Component {
        id: 0,
        name: name.clone(),
        parent,
        bodies: roots.clone(),
    });
    log::info!("部品「{name}」を作る(ボディ {} 個)", roots.len());
    Ok(Outcome::Change {
        label: format!("部品「{name}」を作成"),
        edits: vec![Edit::SetComponents { components: list }],
    })
}

/// ジョイントの軸の候補: 動かす部品をクリックした面のボディの、円の辺(中心と軸)とまっすぐな辺。
fn origin_choices(cx: &Context<'_>) -> Vec<HandleChoice> {
    let Some(p) = cx.picked else {
        return Vec::new();
    };
    let Some(jc_core::Value::Shape(h)) = cx.output(p.feature, "body") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen: Vec<(k::P3, k::P3)> = Vec::new();
    let near = |a: k::P3, b: k::P3| k::dot(k::sub(a, b), k::sub(a, b)) < 1e-8;
    for e in cx.kernel.arc_edges(*h).unwrap_or_default() {
        let Some(arc) = e.arc else { continue };
        if seen
            .iter()
            .any(|(c, a)| near(*c, arc.center) && near(*a, arc.axis))
        {
            continue;
        }
        seen.push((arc.center, arc.axis));
        let o = Origin {
            feature: p.feature,
            point: arc.center,
            axis: arc.axis,
        };
        out.push(HandleChoice {
            value: json!(o),
            label: format!("円の中心(半径 {:.2} mm)", arc.radius),
            outline: vec![e.polyline(48)],
            group: 0,
            sized: false,
            color: None,
        });
    }
    for e in cx.kernel.edges(*h).unwrap_or_default() {
        let d = k::sub(e.b, e.a);
        if k::dot(d, d) < 1e-12 {
            continue;
        }
        let o = Origin {
            feature: p.feature,
            point: e.a,
            axis: k::normalize(d),
        };
        out.push(HandleChoice {
            value: json!(o),
            label: "辺(蝶番の軸・スライドの向き)".into(),
            outline: vec![vec![e.a, e.b]],
            group: 0,
            sized: false,
            color: None,
        });
    }
    out
}

/// ジョイントの手順: 動かす部品の面 → 軸(円か辺)→ 土台の部品の面(Enter で地面)。
fn joint_handles(cx: &Context<'_>) -> Vec<Handle> {
    let mut origin = Handle::new(
        "origin",
        "軸にする円の辺か、まっすぐな辺をクリック(円は中心を通る軸、辺は辺の向き)",
        Gesture::Choice,
    );
    if cx.arg("moving").is_some() {
        origin.choices = origin_choices(cx);
    }
    vec![
        Handle::new("moving", "動かす部品の面をクリック", Gesture::AnyFace),
        origin,
        Handle {
            optional: true,
            ..Handle::new(
                "base",
                "土台の部品の面をクリック(Enter だけなら地面に固定)",
                Gesture::AnyFace,
            )
        },
    ]
}

/// 合わせるジョイントの手順: 部品 1 の面 → その面の原点 → 部品 2 の面 → その面の原点。
fn snap_joint_handles(cx: &Context<'_>) -> Vec<Handle> {
    let label = "原点をクリック(面の中心・円の中心・頂点・辺の中点・2 面の間・2 辺の交点)";
    let mut from = Handle::new("from", &format!("部品 1 の{label}"), Gesture::Choice);
    let mut to = Handle::new("to", &format!("部品 2 の{label}"), Gesture::Choice);
    if let Some(p) = cx.picked {
        if cx.arg("base").is_some() {
            to.choices = snap::choices(cx, p);
        } else if cx.arg("moving").is_some() {
            from.choices = snap::choices(cx, p);
        }
    }
    vec![
        Handle::new(
            "moving",
            "部品 1(動かす部品)の面をクリック",
            Gesture::AnyFace,
        ),
        from,
        Handle::new("base", "部品 2(相手の部品)の面をクリック", Gesture::AnyFace),
        to,
    ]
}

/// ジョイントのデータ(ボディを受け継いだ機能の順)。
fn joints_of(cx: &Context<'_>) -> Vec<(FeatureId, Joint)> {
    cx.data
        .features
        .iter()
        .filter(|f| f.type_id == JOINT && !f.suppressed)
        .filter_map(|f| Some((f.id, serde_json::from_value::<Joint>(f.data.clone()).ok()?)))
        .collect()
}

/// ジョイントの一覧の表。行はタイムラインの順で、何と何をつないでいるかを
/// 名前で出す(機能の番号も付けるので、AI はそのまま `get_feature` などに渡せる)。
pub fn joint_table(cx: &Context<'_>) -> jc_core::Table {
    use jc_core::{Table, TableColumn as C};
    // ボディは「ボディ1」のような名前だけでは見分けにくいので、作った機能の名前と番号も付ける
    let body = |r: FeatureId| {
        let made = cx.data.feature(r).map_or("", |f| f.name.as_str());
        format!(
            "{}(「{made}」#{})",
            part::body_label(cx.data, cx.registry, cx.report, r),
            r.0
        )
    };
    let label = |p: &Part| match p {
        Part::Body { root } => body(*root),
        Part::Component { .. } => p.label(cx.data, cx.registry, cx.report),
    };
    let xyz = |v: k::P3| format!("{:.3}, {:.3}, {:.3}", v[0], v[1], v[2]);
    // 止めたジョイントも出す(AI が組立の決まりを読む時に抜けないように)
    let rows = cx
        .data
        .features
        .iter()
        .filter(|f| f.type_id == JOINT)
        .filter_map(|f| {
            Some((
                f.id,
                f.suppressed,
                serde_json::from_value::<Joint>(f.data.clone()).ok()?,
            ))
        })
        .map(|(id, suppressed, j)| {
            let name = cx
                .data
                .feature(id)
                .map_or_else(String::new, |f| f.name.clone());
            let kind = joint::KINDS
                .iter()
                .find(|(k, _)| serde_json::to_value(j.kind).ok() == Some(json!(k)))
                .map_or("?", |(_, l)| l);
            let bodies: Vec<String> = moved_roots(cx, &j).into_iter().map(body).collect();
            let k = j.kind;
            // 動きごとに (使う, 名前, 値, 下限, 上限, 戻る位置)。値はどれも打った式のまま出す
            let moves = [
                (
                    k.turns(),
                    "角度",
                    &j.angle,
                    &j.angle_min,
                    &j.angle_max,
                    &j.angle_rest,
                ),
                (
                    k.slides(),
                    "距離",
                    &j.distance,
                    &j.distance_min,
                    &j.distance_max,
                    &j.distance_rest,
                ),
                (
                    k.slides_x(),
                    "X",
                    &j.slide_x,
                    &j.slide_x_min,
                    &j.slide_x_max,
                    &j.slide_x_rest,
                ),
                (
                    k.slides_y(),
                    "Y",
                    &j.slide_y,
                    &j.slide_y_min,
                    &j.slide_y_max,
                    &j.slide_y_rest,
                ),
                (
                    k.tilts(),
                    "傾き",
                    &j.tilt,
                    &j.tilt_min,
                    &j.tilt_max,
                    &j.tilt_rest,
                ),
                (
                    k.tilts(),
                    "ひねり",
                    &j.twist,
                    &j.twist_min,
                    &j.twist_max,
                    &j.twist_rest,
                ),
            ];
            let moves: Vec<_> = moves.into_iter().filter(|m| m.0).collect();
            let value: Vec<String> = moves.iter().map(|m| format!("{} {}", m.1, m.2)).collect();
            let limits: Vec<String> = moves
                .iter()
                .filter(|m| !m.3.is_empty() || !m.4.is_empty())
                .map(|m| {
                    let end = |v: &String| {
                        if v.is_empty() {
                            "制限なし".to_string()
                        } else {
                            v.clone()
                        }
                    };
                    format!("{} {}〜{}", m.1, end(m.3), end(m.4))
                })
                .collect();
            let rest: Vec<String> = moves
                .iter()
                .filter(|m| !m.5.is_empty())
                .map(|m| format!("{} {}", m.1, m.5))
                .collect();
            let link = match (j.drive_by, j.link_joint) {
                (joint::DriveBy::Link, Some(from)) => {
                    let from_name = cx.data.feature(from).map_or("", |f| f.name.as_str());
                    format!(
                        "「{from_name}」#{} が {} 動く時に {}{}",
                        from.0,
                        j.link_lead,
                        j.link_follow,
                        if j.link_flip { "(逆向き)" } else { "" }
                    )
                }
                _ => String::new(),
            };
            vec![
                id.0.to_string(),
                name,
                kind.to_string(),
                label(&j.moving),
                j.base
                    .as_ref()
                    .map_or_else(|| "地面(動かない)".into(), label),
                bodies.join("、"),
                xyz(j.origin),
                xyz(if j.flip {
                    [-j.axis[0], -j.axis[1], -j.axis[2]]
                } else {
                    j.axis
                }),
                value.join("・"),
                limits.join("・"),
                rest.join("・"),
                link,
                if suppressed { "止めている" } else { "" }.into(),
            ]
        })
        .collect();
    Table {
        key: JOINTS.into(),
        columns: vec![
            C::new("id", "機能の番号", true, true),
            C::new("name", "名前", false, true),
            C::new("kind", "種類", false, true),
            C::new("moving", "動く部品", false, true),
            C::new("base", "土台", false, true),
            C::new("bodies", "一緒に動くボディ", false, true),
            C::new("origin", "軸の点 mm(作った時)", false, true),
            C::new("axis", "軸の向き(作った時)", false, true),
            C::new("value", "今の値(式)", false, true),
            C::new("limits", "動く範囲", false, true),
            C::new("rest", "戻る位置", false, true),
            C::new("link", "連動", false, true),
            C::new("suppressed", "止めた", false, true),
        ],
        rows,
        note:
            "軸の点と向きは作った時の世界の座標です(土台を動かす前)。値・範囲・戻る位置は打った式のままです。止めたジョイントも「止めた」の列に印を付けて出します(動きには効きません)。"
                .into(),
        empty: "ジョイントがありません".into(),
        ..Table::default()
    }
}

/// ジョイントが動かしているボディ(最初に作った機能)。
fn moved_roots(cx: &Context<'_>, j: &Joint) -> Vec<FeatureId> {
    j.bodies
        .iter()
        .map(|b| body_root(cx.data, cx.registry, *b))
        .collect()
}

/// 新しいジョイントがつなぐ物: 一緒に動くボディ、土台を動かす前のジョイント(`follows`)と、
/// それらのデータの直し。
struct Links {
    bodies: Vec<FeatureId>,
    follows: Vec<FeatureId>,
    edits: Vec<Edit>,
}

impl Links {
    /// 前のジョイントの動きを順に掛けた物(`ids` の中の、タイムラインの順)。
    fn pose(cx: &Context<'_>, ids: impl Iterator<Item = FeatureId>) -> k::Matrix4 {
        let mut pose = k::IDENTITY;
        for id in ids {
            if let Some(m) = cx.output(id, "transform").and_then(joint::transform_of) {
                pose = joint::mul(&m, &pose);
            }
        }
        pose
    }
}

/// 部品 `moving` を部品 `base`(なければ地面)につなぐ時の [`Links`]。
fn links(cx: &Context<'_>, moving: &Part, base: Option<&Part>) -> Result<Links, String> {
    let data = cx.data;
    let base_roots = base.map_or_else(Vec::new, |b| b.roots(data));
    let existing = joints_of(cx);
    // 一緒に動く物: 動かす部品と、同じ剛性グループの部品と、それらを土台にしたジョイントの部品
    // (その先も)
    let set = rigid::closure(cx, moving.roots(data));
    if set.iter().any(|r| base_roots.contains(r)) {
        return Err(
            "土台が、動かす部品と一緒に動く部品です(同じ部品か同じ剛性グループ、またはジョイントが輪になります)".into(),
        );
    }
    let mut edits = Vec::new();
    // 土台を動かしている前の物(ジョイントと剛性グループ。グループが動きをまとめて掛ける
    // ジョイントは入らない)
    let follows = rigid::movers(cx, &base_roots);
    for (id, j) in &existing {
        if !follows.contains(id) {
            continue;
        }
        let roots = moved_roots(cx, j);
        // 土台を動かすジョイントは、新しく一緒に動く物も動かす
        let at = data.index_of(*id).unwrap_or(usize::MAX);
        let mut j2 = j.clone();
        for r in &set {
            if roots.contains(r) || data.index_of(*r).is_none_or(|i| i > at) {
                continue;
            }
            j2.bodies
                .push(heir_before(data, cx.registry, cx.report, *r, Some(*id)));
        }
        if j2 != *j {
            edits.push(Edit::SetData {
                id: *id,
                data: serde_json::to_value(&j2).map_err(|e| e.to_string())?,
            });
        }
    }
    // 土台を動かす剛性グループも、新しく一緒に動く物を乗せる
    for (id, g) in rigid::group_data(cx) {
        if !follows.contains(&id) {
            continue;
        }
        let at = data.index_of(id).unwrap_or(usize::MAX);
        let riding: Vec<FeatureId> = g
            .riders
            .iter()
            .map(|r| body_root(data, cx.registry, r.body))
            .collect();
        let mut g2 = g.clone();
        for r in &set {
            if riding.contains(r) || data.index_of(*r).is_none_or(|i| i > at) {
                continue;
            }
            g2.riders.push(rigid::Rider {
                body: heir_before(data, cx.registry, cx.report, *r, Some(id)),
                with: Vec::new(),
            });
        }
        if g2 != g {
            edits.push(Edit::SetData {
                id,
                data: serde_json::to_value(&g2).map_err(|e| e.to_string())?,
            });
        }
    }
    let bodies = set
        .iter()
        .map(|r| heir_before(data, cx.registry, cx.report, *r, None))
        .collect();
    Ok(Links {
        bodies,
        follows,
        edits,
    })
}

/// ジョイント `j` を足す変更(前のジョイントの直し `edits` も一緒に)。
fn insert_joint(cx: &Context<'_>, j: &Joint, mut edits: Vec<Edit>) -> Result<Outcome, String> {
    let name = format!("ジョイント{}", cx.count_of(JOINT) + 1);
    log::info!(
        "{name} を作る({:?}・{:?}、動かすボディ {} 個、前のジョイント {} 個を直す)",
        j.how,
        j.kind,
        j.bodies.len(),
        edits.len()
    );
    edits.insert(
        0,
        Edit::Insert {
            type_id: JOINT.into(),
            name: name.clone(),
            data: serde_json::to_value(j).map_err(|e| e.to_string())?,
        },
    );
    Ok(Outcome::Change {
        label: format!("{name} を作成"),
        edits,
    })
}

fn new_joint(cx: &Context<'_>) -> Result<Outcome, String> {
    let origin: Origin = serde_json::from_value(
        cx.arg("origin")
            .cloned()
            .ok_or("軸(origin)を選んでいません")?,
    )
    .map_err(|e| {
        // 何を渡すかを示す(10-24 m5。合わせるジョイントの原点と同じ)
        format!(
            "軸の値が読めません({e})。円の辺(中心を通る軸)かまっすぐな辺(辺の向き)の軸を、{} の形で渡してください",
            r#"{"feature": 面のボディの機能の番号, "point": [x, y, z], "axis": [x, y, z]}"#
        )
    })?;
    let data = cx.data;
    let moving_root = body_root(data, cx.registry, origin.feature);
    let moving = Part::of_body(data, moving_root);
    let base = match cx.arg("base") {
        Some(v) if !v.is_null() => {
            let p = cx.picked.ok_or("土台の部品の面を選んでいません")?;
            Some(Part::of_body(data, body_root(data, cx.registry, p.feature)))
        }
        _ => None,
    };
    let l = links(cx, &moving, base.as_ref())?;
    // クリックした軸は、動かす部品の今の位置にある: その部品を動かしている前のジョイントの動きを
    // 戻して、作った位置の軸にする(動いていなければそのまま。土台の動きは `follows` で掛け直す)
    let pose = Links::pose(cx, rigid::movers(cx, &[moving_root]).into_iter());
    let back = joint::rigid_inverse(&pose);
    let origin = Origin {
        point: k::transform_point(&back, origin.point),
        axis: joint::apply_dir(&back, origin.axis),
        ..origin
    };
    let mut j = joint::new_joint(moving, base, l.bodies, &origin);
    j.follows = l.follows;
    insert_joint(cx, &j, l.edits)
}

/// 合わせるジョイントの原点の欄に読めない値が来た時の理由。何を渡すかを示す(AI や台本が面を
/// そのまま渡すなど。採用分 10-24 m5): 面を渡したなら、その面の上の原点の候補の名前と値を並べる。
fn snap_pick_problem(
    cx: &Context<'_>,
    v: &serde_json::Value,
    what: &str,
    e: &serde_json::Error,
) -> String {
    let shape = r#"{"feature": 面のボディの機能の番号, "frame": {"origin": [x, y, z], "z": [..], "x": [..]}, "label": 取り方の名前}"#;
    let face = v["face"].as_str().zip(v["feature"].as_u64());
    let Some((face, feature)) = face else {
        return format!(
            "{what}の原点の値が読めません({e})。面の上の原点(面の中心・円の中心・頂点・辺の中点・2 面の間・2 辺の交点)を 1 つ、{shape} の形で渡してください"
        );
    };
    let at = jc_core::FacePick {
        feature: FeatureId(feature),
        face: face.to_string(),
    };
    let choices = snap::choices(cx, &at);
    if choices.is_empty() {
        return format!(
            "{what}の原点に面 {face} を渡しています。原点は面ではなく、面の上の取り方(円の中心・角など)を {shape} の形で渡してください(この面には候補がありません。ボディの機能と面の名前を確かめてください)"
        );
    }
    let list: Vec<String> = choices
        .iter()
        .map(|c| format!("{}: {}", c.label, c.value))
        .collect();
    format!(
        "{what}の原点に面 {face} を渡しています。原点は面ではなく、面の上の取り方を 1 つ選んで渡してください。この面の候補: {}",
        list.join(" / ")
    )
}

/// 部品 1 の原点を部品 2 の原点に合わせるジョイントを作る(種類は固定。後で変える)。
fn new_snap_joint(cx: &Context<'_>) -> Result<Outcome, String> {
    let pick = |key: &str, what: &str| -> Result<SnapPick, String> {
        let v = cx
            .arg(key)
            .cloned()
            .ok_or(format!("{what}の原点を選んでいません"))?;
        serde_json::from_value(v.clone()).map_err(|e| snap_pick_problem(cx, &v, what, &e))
    };
    let (from, to) = (pick("from", "部品 1")?, pick("to", "部品 2")?);
    let data = cx.data;
    let moving_root = body_root(data, cx.registry, from.feature);
    let moving = Part::of_body(data, moving_root);
    let base = Part::of_body(data, body_root(data, cx.registry, to.feature));
    if moving
        .roots(data)
        .iter()
        .any(|r| base.roots(data).contains(r))
    {
        return Err("部品 1 と部品 2 が同じ部品です(別の部品の面をクリック)".into());
    }
    if let Some(id) = rigid::movers(cx, &[moving_root]).first() {
        let name = data
            .feature(*id)
            .map_or("前のジョイント", |e| e.name.as_str());
        return Err(format!(
            "部品 1 は {name} で動く部品なので、合わせて動かせません(位置のままのジョイントでつなぐ)"
        ));
    }
    let l = links(cx, &moving, Some(&base))?;
    // 部品 2 の原点は今の位置にある: 土台を動かしている前のジョイントの動きを戻す
    let back = joint::rigid_inverse(&Links::pose(cx, l.follows.iter().copied()));
    let to_frame = Frame {
        origin: k::transform_point(&back, to.frame.origin),
        z: joint::apply_dir(&back, to.frame.z),
        x: joint::apply_dir(&back, to.frame.x),
    };
    let mut j = joint::new_snap_joint(
        moving,
        Some(base),
        l.bodies,
        (from.frame, from.label),
        (to_frame, to.label),
    );
    if let Some((a, flip)) = joint::touching_alignment(&from.frame, &to.frame) {
        j.align_angle = format!("{a} deg");
        j.align_flip = flip;
    }
    j.follows = l.follows;
    insert_joint(cx, &j, l.edits)
}

/// 組立の操作。
pub struct AsmCommands;

impl Extension for AsmCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let bodies = cx.visible_bodies();
        let none = bodies.is_empty().then_some("ボディがありません");
        let few = (bodies.len() < 2).then_some("ボディが 2 つ以上いります");
        let move_why = position::move_target(cx, &picked_roots(cx)).err();
        // 接触セットの区画(読めなければ空として出し、操作の実行で理由を返す)
        let contacts: jc_interchange::contact::Contacts = cx.data.section().unwrap_or_default();
        vec![
            Action::new(NEW_COMPONENT, "新しい部品", "cad")
                .args([
                    Arg::text("name", "部品の名前(省くと 部品1 のような空いた名前)"),
                    Arg::list("selection", "部品にするボディの機能の番号の並び(省くと選んでいるボディ。空の並びなら空の部品)"),
                    Arg::integer("parent", "入れる親の部品の番号(省くと、選んだボディがみな同じ部品に入っていればその中、ほかは文書の直下)"),
                ])
                .role(jc_interchange::role::action::NEW_COMPONENT)
                .menu("組立")
                .icon("component")
                .aliases("ぶひん 部品 こんぽーねんと コンポーネント component くみたて 組立")
                .hint("選んでいるボディをまとめて部品にする(選んでいなければ空の部品)。ブラウザの部品の木に出て、名前は右クリックの「名前変更」で変える"),
            Action::new(NEW_SNAP_JOINT, "ジョイント", "cad")
                .args([
                    Arg::value("moving", "部品 1(動かす部品)の面"),
                    Arg::value("from", "部品 1 の原点"),
                    Arg::value("base", "部品 2(相手の部品)の面"),
                    Arg::value("to", "部品 2 の原点"),
                ])
                .menu("組立")
                .icon("joint")
                .aliases("じょいんと ジョイント joint ちょうつがい 蝶番 すらいだ スライダ あわせる 合わせる げんてん 原点")
                .hint("部品 1 を動かして部品 2 に合わせてつなぐ: 部品 1 の面 → 原点 → 部品 2 の面 → 原点。種類・角度・ずらしは後で変える")
                .at(&[Place::Toolbar])
                .disabled_if(few),
            Action::new(NEW_JOINT, "位置のままのジョイント", "cad")
                .args([
                    Arg::value("moving", "動かす部品の面"),
                    Arg::value("origin", "軸(円か辺。手順の値)").required(),
                    Arg::value("base", "土台の部品の面(省くと土台なし)"),
                ])
                .menu("組立")
                .icon("joint")
                .aliases("じょいんと ジョイント joint ちょうつがい 蝶番 すらいだ スライダ いちのまま 位置のまま げんじょう 現状")
                .hint("置いた位置のまま部品をつなぐ: 動かす部品の面 → 軸(円か辺)→ 土台の部品の面。値で動かせる")
                .disabled_if(none),
            Action::new(NEW_FASTENER, "規格部品", "cad")
                .args([
                    Arg::value("face", "穴の口の面"),
                    Arg::list("holes", "穴の口の円(手順の値の並び)"),
                    Arg::text("kind", "種類(省くとねじ)"),
                ])
                .menu("組立")
                .icon("fastener")
                .aliases("きかくぶひん 規格部品 ねじ ネジ ぼると ボルト なっと ナット ざがね 座金 わっしゃ ワッシャ べありんぐ ベアリング じくうけ 軸受 fastener bolt nut washer bearing")
                .hint("穴にねじ・ナット・座金・玉軸受を挿す: 穴の口の面 → 穴の口の円(いくつでも)→ Enter。呼びは穴の径から選び、種類と長さは後で変えられる。部品表に数が出る")
                .disabled_if(none),
            Action::new(INTERFERENCE, "干渉", "cad")
                .arg(Arg::flag("coincident", "面で接するだけの組も出す"))
                .menu("検査")
                .icon("interference")
                .aliases("かんしょう 干渉 interference ぶつかり 重なり かさなり")
                .hint("見えているボディどうしの重なりを調べ、組ごとの体積を表に、重なった所を 3D に赤で出す(ジョイントで動かした位置のまま)。「一致した面を含める」で、面で接するだけの組も出す")
                .disabled_if(few),
            Action::new(motion::MOTION_STUDY, "モーション スタディ", "cad")
                .args([
                    Arg::text("study", "スタディの名前(省くと最初のスタディ。無ければ作る)"),
                    Arg::feature("joint", "キーを置くジョイントの機能の番号(回転かスライダ)"),
                    Arg::number("step", "キーのステップ(0〜100。省くと 0)", jc_core::FieldUnit::Unitless),
                    Arg::number("value", "キーの値(回転は度、スライダは mm。省くとジョイントの今の値)", jc_core::FieldUnit::Unitless),
                    Arg::flag("remove", "ステップのキーを消す"),
                    Arg::flag("drop", "ジョイントをスタディから外す"),
                    Arg::number("seconds", "0 から 100 まで再生する長さ(秒)", jc_core::FieldUnit::Plain("s".into())),
                    Arg::text("rename", "スタディの新しい名前"),
                    Arg::flag("delete", "スタディを消す"),
                ])
                .menu("組立")
                .icon("joint")
                .aliases("もーしょん モーション すたでぃ スタディ motion study きこう 機構 うごき 動き きー キー")
                .hint("いくつものジョイントの値を、0〜100 のステップの帯のキーで決めて一緒に動かし、機構の動きを確かめる。形と値は変えない。スタディは文書に残る")
                .role(jc_interchange::role::action::MOTION_STUDY)
                .disabled_if(
                    (!cx.data.features.iter().any(|f| f.type_id == JOINT
                        && jc_interchange::animation::drive_of(cx.report, f.id).is_some()))
                    .then_some("回転かスライダのジョイントがありません(「組立」の「ジョイント」で作る)"),
                ),
            Action::new(MOVE_PART, "部品を移動", "cad")
                .arg(Arg::list("bodies", "動かす部品のボディの機能の番号の並び(省くと選んでいる面のボディと選んでいるボディ)"))
                .menu("組立")
                .icon("movecopy")
                .aliases("ぶひん 部品 いどう 移動 うごかす 動かす move component")
                .hint("選んだ部品を中のボディごと値で動かす(位置の機能を足す)。部品を選んで M キーでも始まる")
                .role(jc_interchange::role::action::MOVE_COMPONENT)
                .disabled_if(move_why.as_deref()),
            Action::new(rigid::NEW_RIGID_GROUP, "剛性グループ", "cad")
                .arg(Arg::list("bodies", "まとめるボディの機能の番号の並び(省くと選んでいる面のボディと選んでいるボディ。2 つ以上の部品)"))
                .menu("組立")
                .icon("rigidgroup")
                .aliases("ごうせい 剛性 ぐるーぷ グループ rigid group かたまり 塊 いっしょ 一緒 まとめる")
                .hint("選んでいる部品(2 つ以上)を 1 つの塊にする。どの部品をジョイントや引く操作で動かしても、全部が一緒に動く")
                .disabled_if(few),
            Action::new(constraint::NEW_CONSTRAINT, "拘束", "cad")
                .args([
                    Arg::choice("kind", "種類(省くと位置合わせ)", constraint::KINDS),
                    Arg::value("a", "部品 1 の面・辺・頂点(省くと先に選んだ面)"),
                    Arg::value("b", "部品 2 の面・辺・頂点(省くと後から選んだ面。中央は 2 つ)"),
                    Arg::choice(
                        "b_to",
                        "部品 2 の相手(省くと部品 2 の形。原点・原点の平面にする時は b を渡さない)",
                        constraint::TARGETS,
                    ),
                    Arg::text("offset", "オフセットの距離(式。角度の行では角度)"),
                    Arg::flag("flip", "反転"),
                ])
                .menu("組立")
                .icon("joint")
                .aliases("こうそく 拘束 constraint いちあわせ 位置合わせ めんいち 面一 どうしん 同心 かくど 角度 ちゅうおう 中央 せっせん 接線")
                .hint("部品 1 の面を部品 2 の面に合わせて部品 1 を動かす: 部品 1 の面をクリックし、Ctrl を押しながら部品 2 の面をクリックしてから押す。行(面の組)を足すと全部を同時に満たす")
                .disabled_if(few),
            Action::new(motion_link::MOTION_LINK, "モーション リンク", "cad")
                .args([
                    Arg::feature("joint", "1 つ目のジョイントの機能の番号(回転かスライダ)").required(),
                    Arg::feature("other", "2 つ目のジョイントの機能の番号").required(),
                    Arg::text("lead", "1 つ目の動きの量(式。省くと回転は 360 deg、スライダは 10 mm)"),
                    Arg::text("follow", "2 つ目の動きの量(式。省き方は同じ)"),
                    Arg::flag("flip", "連動する向きを逆にする"),
                ])
                .menu("組立")
                .icon("joint")
                .aliases("もーしょん モーション りんく リンク motion link れんどう 連動 つなぐ ぎや ギヤ 歯車 ねじ")
                .hint("2 つのジョイントの動きを比でつなぐ(例: ハンドルが 360 deg 回ると、あごが 10 mm 動く)。タイムラインで後ろのジョイントが前のジョイントに連動し、比と向きは後ろのジョイントの編集で直す")
                .disabled_if(
                    (cx.data.features.iter().filter(|f| f.type_id == JOINT
                        && jc_interchange::animation::drive_of(cx.report, f.id).is_some()).count() < 2)
                    .then_some("回転かスライダのジョイントが 2 つ以上いります"),
                ),
            Action::new(contact::NEW_CONTACT_SET, "新しい接触セット", "cad")
                .args([
                    Arg::list("bodies", "ぶつかり合うボディの機能の番号の並び(省くと選んでいるボディ。2 つ以上)"),
                    Arg::text("name", "名前(省くと 接触セット1 のような空いた名前)"),
                ])
                .menu("組立")
                .icon("interference")
                .aliases("せっしょく 接触 せっと セット contact ぶつかる 当たる あたる とまる 止まる")
                .hint("選んだボディ(2 つ以上)を接触セットにする。接触セットを有効にしている間、ジョイントの部品を引くと、同じセットの相手に食い込む手前で止まる")
                .disabled_if(few),
            Action::new(
                contact::CONTACTS,
                if contacts.enabled {
                    "接触セットを無効にする"
                } else {
                    "接触セットを有効にする"
                },
                "cad",
            )
            .arg(Arg::flag("enabled", "入れるか(省くと今の逆)"))
            .menu("組立")
            .icon("interference")
            .aliases("せっしょく 接触 せっと セット contact ゆうこう 有効 むこう 無効")
            .hint("ジョイントの部品を引く時に、接触セットの相手で止めるかを切り替える(ブラウザの「接触セット」の目でセットごとにも切れる)")
            .disabled_if(
                contacts
                    .sets
                    .is_empty()
                    .then_some("接触セットがありません(「新しい接触セット」で作る)"),
            ),
            Action::new(contact::REACH, "当たるまでの値", "cad")
                .args([
                    Arg::feature("joint", "動かすジョイントの機能の番号(回転かスライダ)").required(),
                    Arg::number("to", "動かす先の値(回転は deg、スライダは mm)", jc_core::FieldUnit::Unitless).required(),
                ])
                .aliases("せっしょく 接触 あたる 当たる ぶつかる とまる 止まる よゆう 余裕 contact reach clearance")
                .hint("ジョイントを今の値から動かす先まで動かすと、接触セットの相手にどの値で当たるかと、あといくつ動けるかを答える(形は変えない)")
                .disabled_if(
                    (!contacts.enabled)
                        .then_some("接触セットが無効です(接触セットを有効にする)"),
                ),
            Action::new(JOINTS, "ジョイントの一覧", "cad")
                .aliases("じょいんと ジョイント joint いちらん 一覧 つながり くみたて 組立")
                .hint("ジョイントごとに、種類・動く部品・土台・一緒に動くボディ・軸・今の値を表に出す(AI が組立を読む時にも使う)"),
            Action::new(BOM, "部品表", "cad")
                .menu("組立")
                .icon("table")
                .aliases("ぶひんひょう 部品表 bom かっとりすと カットリスト cutlist")
                .hint("部品ごとの数・材料・寸法(長さ×幅×厚さ)・体積・質量の表。出す列は表の「列」で選ぶ。同じ寸法と材料の物はまとめて数える")
                .disabled_if(none),
            Action::new(BOM_CSV, "部品表の書き出し…", "cad")
                .menu("組立")
                .icon("export")
                .aliases("ぶひんひょう 部品表 csv かきだし 書き出し bom")
                .hint("部品表を CSV(表計算ソフトで開ける)に書き出す")
                .at(&[Place::FileMenu])
                .input(Input::SaveFile {
                    title: "部品表の書き出し".into(),
                    filter: "CSV".into(),
                    extensions: vec!["csv".into()],
                })
                .disabled_if(none),
            Action::new(anim::EXPLODE, "分解の移動", "anim")
                .args([
                    Arg::value("part", "動かす部品の面"),
                    Arg::vector("dir", "向き").required(),
                    Arg::length("distance", "距離(mm)").required(),
                    Arg::list("targets", "動かす物の並び(省くと選んだ面の部品)"),
                    Arg::number("time", "段の帯の今の時刻(秒。画面が渡す)", jc_core::FieldUnit::Plain("s".into())),
                ])
                .menu("変換")
                .icon("move")
                .aliases("ぶんかい 分解 分解図 explode ばらす いどう 移動 へんかん 変換")
                .hint("部品を向きと距離で動かす段を、段の帯の最後に足す: 部品の面 → 向きの矢印 → 距離。設計の形は変えない")
                .at(&[Place::Toolbar])
                .disabled_if(none),
            Action::new(anim::AUTO_EXPLODE_ONE, "自動分解: 1 レベル", "anim")
                .args([
                    Arg::flag("sequential", "順次(部品ごとに 1 秒)。偽ならワンステップ").default(false),
                    Arg::flag("trail", "トレイルラインを出す").default(true),
                    Arg::number("scale", "分解の目盛(0 より大きい倍率)", jc_core::FieldUnit::Unitless).default(1.0),
                    Arg::number("time", "段の帯の今の時刻(秒。画面が渡す)", jc_core::FieldUnit::Plain("s".into())),
                ])
                .menu("変換")
                .icon("explode")
                .aliases("じどうぶんかい 自動分解 ぶんかい 分解 分解図 auto explode one level 1れべる いちれべる")
                .hint("部品を組立の中心から自動で離す(直下の部品だけ。部品の中は動かさない)。ワンステップ(全部同時)か順次(部品ごとに 1 秒)、トレイルライン、分解の目盛を選んで OK")
                .at(&[Place::Toolbar])
                .role(jc_interchange::role::action::AUTO_EXPLODE)
                .disabled_if(few),
            Action::new(anim::AUTO_EXPLODE_ALL, "自動分解: すべてのレベル", "anim")
                .args([
                    Arg::flag("sequential", "順次(部品ごとに 1 秒)。偽ならワンステップ").default(false),
                    Arg::flag("trail", "トレイルラインを出す").default(true),
                    Arg::number("scale", "分解の目盛(0 より大きい倍率)", jc_core::FieldUnit::Unitless).default(1.0),
                    Arg::number("time", "段の帯の今の時刻(秒。画面が渡す)", jc_core::FieldUnit::Plain("s".into())),
                ])
                .menu("変換")
                .icon("explode")
                .aliases("じどうぶんかい 自動分解 ぶんかい 分解 分解図 auto explode all levels すべて")
                .hint("部品を組立の中心から自動で離し、部品の中の部品もその部品の中心から離す。ワンステップか順次、トレイルライン、分解の目盛を選んで OK")
                .role(jc_interchange::role::action::AUTO_EXPLODE)
                .disabled_if(few),
            Action::new(anim::VISIBILITY, "表示/非表示", "anim")
                .args([
                    Arg::value("part", "部品の面"),
                    Arg::list("targets", "見せる・隠す物の並び(省くと選んだ面かブラウザで選んだボディの部品)"),
                    Arg::flag("visible", "見せるなら真、隠すなら偽(省くと今の様子の逆)"),
                    Arg::number("time", "段の帯の今の時刻(秒。画面が渡す)", jc_core::FieldUnit::Plain("s".into())),
                ])
                .menu("変換")
                .icon("eye")
                .aliases("ひょうじ 表示 ひひょうじ 非表示 かくす 隠す みせる 見せる show hide visibility")
                .hint("選んだ部品を段の始めに隠す(隠れていれば見せる)段を、段の帯の最後に足す。部品を選んでから押すか、押してから面をクリック")
                .at(&[Place::Toolbar])
                .disabled_if(none),
            Action::new(anim::APPEARANCE, "外観", "anim")
                .args([
                    Arg::value("part", "部品の面"),
                    Arg::list("targets", "外観を替える物の並び(省くと選んだ面かブラウザで選んだボディの部品)"),
                    Arg::text("appearance", "外観の名前(省くと今の外観と違う最初の塗装の色)"),
                    Arg::number("time", "段の帯の今の時刻(秒。画面が渡す)", jc_core::FieldUnit::Plain("s".into())),
                ])
                .menu("変換")
                .icon("appearance")
                .aliases("がいかん 外観 いろ 色 appearance color")
                .hint("選んだ部品の外観を、段の長さをかけて替える段を足す(設計の外観は変えない)。外観は段の帯で選び直す")
                .at(&[Place::Toolbar])
                .disabled_if(none),
            Action::new(anim::VIEW, "ビュー", "anim")
                .args([
                    Arg::object("view", "視点 {target, yaw, pitch, height, perspective}(画面が今の視点を渡す)").required(),
                    Arg::number("time", "段の帯の今の時刻(秒。画面が渡す)", jc_core::FieldUnit::Plain("s".into())),
                ])
                .menu("ビュー")
                .icon("camera")
                .aliases("びゅー ビュー してん 視点 かめら カメラ きろく 記録 view camera record")
                .hint("今の視点へ動かす段を、段の帯の最後に足す(前のビューの段の視点から動く)。段の帯の「記録」を入れると、視点を動かし終えるたびに足す")
                .at(&[Place::Toolbar])
                .role(jc_interchange::role::action::ANIMATION_VIEW)
                .disabled_if(none),
            Action::new(anim::DRIVE, "ジョイントの動き", "anim")
                .args([
                    Arg::feature("joint", "ジョイントの機能の番号").required(),
                    Arg::number("from", "始めの値(回転は度、スライダは mm。省くと下限)", jc_core::FieldUnit::Unitless),
                    Arg::number("to", "終わりの値(省くと上限)", jc_core::FieldUnit::Unitless),
                    Arg::number("time", "段の帯の今の時刻(秒。画面が渡す)", jc_core::FieldUnit::Plain("s".into())),
                ])
                .menu("変換")
                .icon("joint")
                .aliases("じょいんと ジョイント うごき 動き アニメーション animate drive")
                .hint("ジョイントの値を下限から上限へ動かす段を足す(上下限が無ければ回転は 90 度、スライダは部品の長さ)。値と長さは段の帯で直す")
                .at(&[Place::Toolbar])
                .disabled_if(
                    (!cx.data.features.iter().any(|f| f.type_id == JOINT
                        && jc_interchange::animation::drive_of(cx.report, f.id).is_some()))
                    .then_some("回転かスライダのジョイントがありません(設計の「組立」で作る)"),
                ),
        ]
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        match id {
            NEW_JOINT => Some(joint_handles(cx)),
            NEW_SNAP_JOINT => Some(snap_joint_handles(cx)),
            NEW_FASTENER => Some(fastener_handles(cx)),
            anim::EXPLODE => Some(anim::explode_handles(cx)),
            anim::DRIVE => Some(anim::drive_handles(cx)),
            motion_link::MOTION_LINK => Some(motion_link::handles(cx)),
            anim::VISIBILITY | anim::APPEARANCE => Some(anim::part_handles(cx)),
            _ => None,
        }
    }

    fn unlisted(&self, id: &str) -> Option<Vec<Arg>> {
        // 部品を 3D のビューで引いて動かす(画面のドラッグが呼ぶ)
        (id == DRAG_POSITION).then(|| {
            vec![
                Arg::feature("body", "引いたボディの機能の番号").required(),
                Arg::vector("offset", "引いた量(世界座標の mm)").required(),
            ]
        })
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        Some(match id {
            NEW_COMPONENT => new_component(cx),
            NEW_JOINT => new_joint(cx),
            NEW_SNAP_JOINT => new_snap_joint(cx),
            NEW_FASTENER => new_fastener(cx),
            anim::EXPLODE => anim::explode(cx),
            anim::DRIVE => anim::drive(cx),
            motion::MOTION_STUDY => motion::run(cx),
            contact::NEW_CONTACT_SET => contact::new_set(cx),
            contact::CONTACTS => contact::toggle(cx),
            contact::REACH => contact::reach(cx),
            motion_link::MOTION_LINK => motion_link::run(cx),
            rigid::NEW_RIGID_GROUP => rigid::new_group(cx, &picked_roots(cx)),
            constraint::NEW_CONSTRAINT => constraint::new_constraint(cx),
            anim::VIEW => anim::view(cx),
            anim::VISIBILITY => anim::visibility(cx),
            anim::APPEARANCE => anim::appearance(cx),
            anim::AUTO_EXPLODE_ONE => anim::auto_explode(cx, false),
            anim::AUTO_EXPLODE_ALL => anim::auto_explode(cx, true),
            DRAG_POSITION => position::drag_position(cx),
            MOVE_PART => position::move_target(cx, &picked_roots(cx))
                .and_then(|p| position::move_part(cx, p)),
            INTERFERENCE => inspect::interference_outcome(cx),
            JOINTS => Ok(Outcome::Table {
                title: "ジョイントの一覧".into(),
                table: joint_table(cx),
            }),
            BOM => Ok(Outcome::Table {
                title: "部品表".into(),
                table: bom_table(&bom_rows(cx)),
            }),
            BOM_CSV => {
                let rows = bom_rows(cx);
                Ok(Outcome::File {
                    suffix: "部品表.csv".into(),
                    contents: bom_csv(&rows).into_bytes(),
                    info: json!({"rows": rows.len()}),
                })
            }
            _ => return None,
        })
    }

    fn summary(&self, cx: &Context<'_>, id: FeatureId) -> Option<String> {
        let e = cx.data.feature(id)?;
        if e.type_id == fastener::FASTENER || e.type_id == fastener::FASTENER_BODY {
            let (gid, f) = match e.type_id.as_str() {
                fastener::FASTENER => {
                    (id, serde_json::from_value::<Fastener>(e.data.clone()).ok()?)
                }
                _ => fastener::fastener_of(cx.data, id)?,
            };
            return Some(format!(
                "{}、穴 {} か所",
                fastener::name_of(cx, gid, &f),
                f.places.len()
            ));
        }
        if e.type_id == constraint::CONSTRAINT {
            let c: constraint::Constraint = serde_json::from_value(e.data.clone()).ok()?;
            let kinds: Vec<&str> = c
                .rows
                .iter()
                .map(|r| {
                    constraint::KINDS
                        .iter()
                        .find(|(k, _)| *k == r.kind)
                        .map_or("?", |(_, l)| *l)
                })
                .collect();
            return Some(format!(
                "{}。部品 1: {} / 部品 2: {}",
                kinds.join("・"),
                c.moving.label(cx.data, cx.registry, cx.report),
                // 原点に合わせた拘束は、部品 2 を部品 1 と同じにしている
                if c.base == c.moving {
                    "原点".to_string()
                } else {
                    c.base.label(cx.data, cx.registry, cx.report)
                }
            ));
        }
        let e = Some(e).filter(|e| e.type_id == JOINT)?;
        let j: Joint = serde_json::from_value(e.data.clone()).ok()?;
        let moving = j.moving.label(cx.data, cx.registry, cx.report);
        let base = j.base.as_ref().map_or_else(
            || "地面".to_string(),
            |b| b.label(cx.data, cx.registry, cx.report),
        );
        let kind = joint::KINDS
            .iter()
            .find(|(k, _)| json!(j.kind) == json!(k))
            .map_or("", |(_, l)| l);
        Some(match &j.snap {
            Some(s) if j.how == joint::How::Snap => format!(
                "{kind}。部品 1: {moving}({})/ 部品 2: {base}({})",
                s.from_label, s.to_label
            ),
            _ => format!(
                "{kind}。動かす部品: {moving}(ボディ {} 個)/ 土台: {base}",
                j.bodies.len()
            ),
        })
    }

    fn drag_limit(
        &self,
        cx: &Context<'_>,
        drag: &jc_core::Drag,
        from: f64,
        to: f64,
    ) -> Option<f64> {
        contact::limit(cx, drag, from, to)
    }

    fn drag(&self, cx: &Context<'_>, at: &jc_core::FacePick) -> Option<jc_core::Drag> {
        match joint::drag(cx, at) {
            Some(d) => Some(position::after_positions(cx, d, at.feature)),
            None => position::drag(cx, at),
        }
    }

    fn overlay(&self, cx: &Context<'_>) -> jc_core::Overlay {
        // 選んでいるジョイントの軸を、作った位置の線で見せる
        let mut o = jc_core::Overlay::default();
        let Some(e) = cx
            .selected
            .and_then(|s| cx.data.feature(s))
            .filter(|e| e.type_id == JOINT)
        else {
            return o;
        };
        let Ok(j) = serde_json::from_value::<Joint>(e.data.clone()) else {
            return o;
        };
        let a = k::scale(k::normalize(j.axis), 30.0);
        o.lines.push((
            vec![k::sub(j.origin, a), k::add(j.origin, a)],
            jc_core::LineKind::new(jc_interchange::ink::line::CONSTRUCTION, "ジョイントの軸"),
        ));
        o.points.push(j.origin);
        o
    }
}

/// この crate の名乗り(登録する種類と操作)。組み立て(jc-bundle)が 1 回だけ並べる。
pub fn module() -> jc_core::Module {
    jc_core::Module::new("jc-asm", register)
        .with_extension(AsmCommands)
        .with_extension(link::LinkCommands::default())
}

#[cfg(test)]
mod tests;
