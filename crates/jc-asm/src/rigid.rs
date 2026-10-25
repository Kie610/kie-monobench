//! 剛性グループ(`asm.rigid_group@1`)。2 つ以上の部品を
//! 1 つの塊にして、どれを動かしても全部が一緒に動くようにする。
//!
//! グループは前の機能を書き換えず、自分の位置で動かす。作る時に、グループの部品のどれかを動かす
//! 前のジョイント・位置(`follows`)と、一緒に動く物(グループの部品と、それを土台にした部品。
//! `riders`)を自分のデータに持つ。計算では、前のジョイント・位置の動きを順に掛けた物を、
//! まだその動きを受けていないボディに掛けて出す(コアの [`jc_core::MOVED`]。ジョイントと同じ)。
//! - グループの部品のどれかを動かす前のジョイントの土台がグループと一緒に動くなら、輪になるので
//!   断る。
//! - 後から作るジョイント・位置は、グループの部品をまとめて動かす([`closure`])。グループに乗る
//!   部品を土台にするジョイントは、グループの動き(出力 `transform`)を軸に掛ける。
//! - アニメーション・引く操作は、グループが前のジョイントの動きに乗せたボディ(出力
//!   [`anim::RIDE_SLOT`])も、そのジョイントと一緒に動かす。
//!
//! 消すか、タイムラインの印をグループの前へ戻すと、部品は元の動きに戻る。

use crate::joint::{TRANSFORM_TYPE, mul, rigid_inverse, transform_of};
use crate::part::{Part, heir_before};
use crate::position::{POSITION, Position};
use crate::{JOINT, Joint};
use jc_cad::{body_of, kernel_err, output, parse};
use jc_core::kernel::{self as k, Matrix4};
use jc_core::{
    Context, Descriptor, Edit, EvalCtx, EvalError, Feature, FeatureId, InputRef, Outcome, Outputs,
    Registry, Value, body_root, category, moved_slot,
};
use jc_interchange::animation::{self as anim, Ride};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const RIGID_GROUP: &str = "asm.rigid_group@1";
/// 選んでいる部品を剛性グループにする操作。
pub const NEW_RIGID_GROUP: &str = "asm.new_rigid_group";

/// 例: `{"members": [{"kind": "component", "id": 12}, {"kind": "body", "root": 3}],
/// "follows": [20], "riders": [{"body": 7, "with": [20]}, {"body": 9}]}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigidGroup {
    /// 一緒に動く部品(2 つ以上)
    pub members: Vec<Part>,
    /// 部品のどれかを動かす前のジョイント・位置(タイムラインの順)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follows: Vec<FeatureId>,
    /// 一緒に動くボディ(作った時の最後の機能)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub riders: Vec<Rider>,
}

/// グループと一緒に動くボディ 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rider {
    pub body: FeatureId,
    /// `follows` のうち、もうこのボディを動かしている物
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub with: Vec<FeatureId>,
}

impl RigidGroup {
    /// `follows` のうち `pick` の物の動きを、タイムラインの順に掛けた物。
    fn pose(
        &self,
        ctx: &EvalCtx<'_>,
        pick: impl Fn(&FeatureId) -> bool,
    ) -> Result<Matrix4, EvalError> {
        let mut pose = k::IDENTITY;
        for j in self.follows.iter().filter(|j| pick(j)) {
            let m = transform_of(ctx.output(*j, "transform")?).ok_or_else(|| {
                EvalError::InvalidData(format!("{j} はジョイントか位置ではありません"))
            })?;
            pose = mul(&m, &pose);
        }
        Ok(pose)
    }
}

impl Feature for RigidGroup {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v: Vec<InputRef> = self
            .follows
            .iter()
            .map(|j| output(*j, "transform"))
            .collect();
        v.extend(self.riders.iter().map(|r| output(r.body, "body")));
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        if self.members.len() < 2 {
            return Err(EvalError::InvalidData("部品が 2 つ以上いります".into()));
        }
        // 前の動きを全部受けた位置へ: もう受けた動きを戻してから、全部を掛ける
        let all = self.pose(ctx, |_| true)?;
        let mut out = Outputs::new();
        let mut ride = Ride::default();
        for r in &self.riders {
            let done = self.pose(ctx, |j| r.with.contains(j))?;
            let m = mul(&all, &rigid_inverse(&done));
            if !near_identity(&m) {
                let h = ctx
                    .kernel()
                    .transform(body_of(ctx, r.body)?, &m, "")
                    .map_err(kernel_err)?;
                out.insert(moved_slot(r.body), Value::Shape(h));
            }
            ride.riders.push(anim::Rider {
                body: r.body,
                joints: self
                    .follows
                    .iter()
                    .filter(|j| !r.with.contains(j))
                    .copied()
                    .collect(),
            });
        }
        out.insert(
            "transform".into(),
            Value::Custom {
                type_key: TRANSFORM_TYPE.into(),
                data: json!(all),
            },
        );
        out.insert(anim::RIDE_SLOT.into(), ride.to_value());
        Ok(out)
    }
}

fn near_identity(m: &Matrix4) -> bool {
    (0..4).all(|i| (0..4).all(|j| (m[i][j] - k::IDENTITY[i][j]).abs() < 1e-12))
}

pub fn descriptor() -> Descriptor {
    Descriptor::new(RIGID_GROUP, category::SOLID, "剛性グループ")
        .menu("組立")
        .icon("rigidgroup")
        .hint("一緒に動く部品の塊。どの部品をジョイントや引く操作で動かしても、全部が一緒に動く")
        .aliases(
            "ごうせい 剛性 ぐるーぷ グループ rigid group かたまり 塊 いっしょ 一緒 くみたて 組立",
        )
        .no_raw_editor()
}

pub fn register(reg: &mut Registry) {
    reg.register_described(descriptor(), |d| Ok(Box::new(parse::<RigidGroup>(d)?)));
}

/// 文書の剛性グループ(抑制していない物)の、部品のボディ(最初に作った機能)の組。
pub fn groups(cx: &Context<'_>) -> Vec<Vec<FeatureId>> {
    cx.data
        .features
        .iter()
        .filter(|f| f.type_id == RIGID_GROUP && !f.suppressed)
        .filter_map(|f| serde_json::from_value::<RigidGroup>(f.data.clone()).ok())
        .map(|g| g.members.iter().flat_map(|m| m.roots(cx.data)).collect())
        .collect()
}

/// 文書の剛性グループ(抑制していない物)とそのデータ(タイムラインの順)。
pub fn group_data(cx: &Context<'_>) -> Vec<(FeatureId, RigidGroup)> {
    cx.data
        .features
        .iter()
        .filter(|f| f.type_id == RIGID_GROUP && !f.suppressed)
        .filter_map(|f| {
            Some((
                f.id,
                serde_json::from_value::<RigidGroup>(f.data.clone()).ok()?,
            ))
        })
        .collect()
}

/// ボディ(最初に作った機能)`roots` を今の位置へ動かしている前のジョイントと剛性グループ
/// (タイムラインの順)。グループに乗るボディは、グループが動きをまとめて掛け直すので、その
/// グループの `follows` のジョイントは入れない。
pub fn movers(cx: &Context<'_>, roots: &[FeatureId]) -> Vec<FeatureId> {
    let gs: Vec<(FeatureId, RigidGroup)> = group_data(cx)
        .into_iter()
        .filter(|(_, g)| {
            !g.follows.is_empty()
                && g.riders
                    .iter()
                    .any(|r| roots.contains(&body_root(cx.data, cx.registry, r.body)))
        })
        .collect();
    let mut v: Vec<FeatureId> = joints(cx)
        .into_iter()
        .filter(|(id, j)| {
            roots_of(cx, &j.bodies).iter().any(|r| roots.contains(r))
                && !gs.iter().any(|(_, g)| g.follows.contains(id))
        })
        .map(|(id, _)| id)
        .collect();
    v.extend(gs.iter().map(|(id, _)| *id));
    v.sort_by_key(|id| cx.data.index_of(*id).unwrap_or(usize::MAX));
    v
}

fn joints(cx: &Context<'_>) -> Vec<(FeatureId, Joint)> {
    cx.data
        .features
        .iter()
        .filter(|f| f.type_id == JOINT && !f.suppressed)
        .filter_map(|f| Some((f.id, serde_json::from_value::<Joint>(f.data.clone()).ok()?)))
        .collect()
}

fn roots_of(cx: &Context<'_>, bodies: &[FeatureId]) -> Vec<FeatureId> {
    let mut v: Vec<FeatureId> = Vec::new();
    for b in bodies {
        let r = body_root(cx.data, cx.registry, *b);
        if !v.contains(&r) {
            v.push(r);
        }
    }
    v
}

/// `set` に、それと一緒に動く物を足す: 入っている部品の剛性グループのほかの部品と、`set` の部品を
/// 土台にしたジョイントの部品(その先も)。
pub fn closure(cx: &Context<'_>, mut set: Vec<FeatureId>) -> Vec<FeatureId> {
    let js = joints(cx);
    let gs = groups(cx);
    loop {
        let mut add: Vec<FeatureId> = Vec::new();
        for g in &gs {
            if g.iter().any(|r| set.contains(r)) {
                add.extend(g.iter().copied());
            }
        }
        for (_, j) in &js {
            let Some(b) = &j.base else { continue };
            if b.roots(cx.data).iter().any(|r| set.contains(r)) {
                add.extend(roots_of(cx, &j.bodies));
            }
        }
        let before = set.len();
        for r in add {
            if !set.contains(&r) {
                set.push(r);
            }
        }
        if set.len() == before {
            return set;
        }
    }
}

/// 選んでいるボディ(`roots`、最初に作った機能)の部品を剛性グループにする。
pub fn new_group(cx: &Context<'_>, roots: &[FeatureId]) -> Result<Outcome, String> {
    let data = cx.data;
    let mut members: Vec<Part> = Vec::new();
    for r in roots {
        let p = Part::of_body(data, *r);
        if !members.contains(&p) {
            members.push(p);
        }
    }
    if members.len() < 2 {
        return Err("部品を 2 つ以上選んでください(同じ部品のボディは 1 つに数えます)".into());
    }
    let core: Vec<FeatureId> = members.iter().flat_map(|m| m.roots(data)).collect();
    // グループの部品(前のグループとつながる物も)と、それを土台にした部品
    let whole = closure(cx, core.clone());
    let mut core = core;
    for g in groups(cx) {
        if g.iter().any(|r| core.contains(r)) {
            for r in g {
                if !core.contains(&r) {
                    core.push(r);
                }
            }
        }
    }
    let name_of = |id: FeatureId| data.feature(id).map_or(String::new(), |e| e.name.clone());
    let at = |id: FeatureId| data.index_of(id).unwrap_or(usize::MAX);
    // グループの部品のどれかを動かす前のジョイント・位置と、それが動かしているボディ
    let mut follows: Vec<(FeatureId, Vec<FeatureId>)> = Vec::new();
    for (id, j) in joints(cx) {
        let moved = roots_of(cx, &j.bodies);
        if !moved.iter().any(|r| core.contains(r)) {
            continue;
        }
        if let Some(b) = &j.base
            && b.roots(data).iter().any(|r| whole.contains(r))
        {
            return Err(format!(
                "{} で動く部品と、その土台が同じグループで一緒に動きます(ジョイントが輪になります)",
                name_of(id)
            ));
        }
        follows.push((id, moved));
    }
    for f in data
        .features
        .iter()
        .filter(|f| f.type_id == POSITION && !f.suppressed)
    {
        let Ok(p) = serde_json::from_value::<Position>(f.data.clone()) else {
            continue;
        };
        let moved = roots_of(cx, &p.bodies);
        if moved.iter().any(|r| core.contains(r)) {
            follows.push((f.id, moved));
        }
    }
    follows.sort_by_key(|(id, _)| at(*id));
    // 前のグループに乗っているボディは、そのグループの前のジョイントの動きをもう受けている
    let earlier = group_data(cx);
    let ridden = |r: FeatureId, j: FeatureId| {
        earlier.iter().any(|(_, g)| {
            g.follows.contains(&j)
                && g.riders
                    .iter()
                    .any(|x| body_root(data, cx.registry, x.body) == r)
        })
    };
    let riders: Vec<Rider> = whole
        .iter()
        .map(|r| Rider {
            body: heir_before(data, cx.registry, cx.report, *r, None),
            with: follows
                .iter()
                .filter(|(id, moved)| moved.contains(r) || ridden(*r, *id))
                .map(|(id, _)| *id)
                .collect(),
        })
        .collect();
    let name = format!("剛性グループ{}", cx.count_of(RIGID_GROUP) + 1);
    log::info!(
        "{name} を作る(部品 {}、一緒に動くボディ {}、前のジョイント・位置 {} 個の動きを掛ける)",
        members.len(),
        riders.len(),
        follows.len()
    );
    let g = RigidGroup {
        members,
        follows: follows.into_iter().map(|(id, _)| id).collect(),
        riders,
    };
    Ok(Outcome::Change {
        label: format!("{name} を作成"),
        edits: vec![Edit::Insert {
            type_id: RIGID_GROUP.into(),
            name: name.clone(),
            data: serde_json::to_value(g).map_err(|e| e.to_string())?,
        }],
    })
}
