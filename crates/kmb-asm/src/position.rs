//! 部品を引いて動かした位置(`asm.position@1`)。
//!
//! ジョイントの無い部品(部品の木の部品)を 3D のビューで引くと、画面に平行に動かし、その量を
//! この機能に持つ。一緒に動く物は、部品のボディと、それを土台にしたジョイントの部品(その先も)。
//! ボディは名前(最後の機能)を変えずに動かす(コアの [`kmb_core::MOVED`]。ジョイントと同じ)。
//! 部品に入っていないボディは引かない(範囲で選ぶまま)。
//!
//! 続けて同じ部品を引いた時は、最後の機能がその部品のこの機能なら値を足して直す(タイムラインを
//! 引いた数だけ伸ばさない)。

use crate::joint::{apply_dir, moved_outputs, mul, transform_of};
use crate::part::{Part, heir_before};
use crate::{JOINT, Joint};
use kmb_cad::{Exprs, expr_refs, length, output, parse, rename};
use kmb_core::kernel::{self as k};
use kmb_core::{
    Context, Descriptor, Edit, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outcome,
    Outputs, Registry, body_root, category,
};
use serde::{Deserialize, Serialize};

pub const POSITION: &str = "asm.position@1";
/// 引いた量で位置の機能を足す・直す操作(3D のビューで部品を引いた時に画面が呼ぶ。メニューには出さない)。
/// 引数は `body`(引いた面のボディの機能)と `offset`(世界座標の mm の [x, y, z])。
pub const DRAG_POSITION: &str = "asm.drag_position";
/// 選んでいる部品を値で動かす位置の機能を足す操作(画面の M キーが、部品を選んでいる時に使う)。
/// 引数は `bodies`(ボディの機能の番号の並び。省くと選んでいる面のボディと選んでいるボディ)。
pub const MOVE_PART: &str = "asm.move_part";

fn zero_mm() -> String {
    "0 mm".into()
}

/// 例: `{"moving": {"kind": "component", "id": 12}, "bodies": [9, 10], "x": "12.5 mm", "y": "0 mm",
/// "z": "-3 mm"}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    /// 引いた部品
    pub moving: Part,
    /// 一緒に動かすボディ(この機能の手前で受け継いだ機能)
    pub bodies: Vec<FeatureId>,
    #[serde(default = "zero_mm")]
    pub x: String,
    #[serde(default = "zero_mm")]
    pub y: String,
    #[serde(default = "zero_mm")]
    pub z: String,
}

impl Exprs for Position {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.x, &mut self.y, &mut self.z]
    }
}

impl Feature for Position {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v: Vec<InputRef> = self.bodies.iter().map(|b| output(*b, "body")).collect();
        for i in expr_refs(self)? {
            if !v.contains(&i) {
                v.push(i);
            }
        }
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        if self.bodies.is_empty() {
            return Err(EvalError::InvalidData("動かすボディがありません".into()));
        }
        let m = k::translation([
            length(ctx, &self.x, "X 距離")?,
            length(ctx, &self.y, "Y 距離")?,
            length(ctx, &self.z, "Z 距離")?,
        ]);
        let mut out = Outputs::new();
        moved_outputs(ctx, &self.bodies, &m, &mut out)?;
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

pub fn descriptor() -> Descriptor {
    Descriptor::new(POSITION, category::SOLID, "位置")
        .role(kmb_interchange::role::kind::COMPONENT_POSITION)
        .icon("move")
        .hint("3D のビューで引いて動かした部品の位置(一緒に動くジョイントの部品も)。値を直すと動く")
        .aliases("いち 位置 きゃぷちゃ キャプチャ capture position ぶひん 部品 くみたて 組立")
        .field(
            Field::expr("x", "X 距離")
                .length()
                .help("引く前の位置からの距離"),
        )
        .field(
            Field::expr("y", "Y 距離")
                .length()
                .help("引く前の位置からの距離"),
        )
        .field(
            Field::expr("z", "Z 距離")
                .length()
                .help("引く前の位置からの距離"),
        )
        .no_raw_editor()
}

pub fn register(reg: &mut Registry) {
    reg.register_described(descriptor(), |d| Ok(Box::new(parse::<Position>(d)?)));
}

/// ジョイントのデータ(タイムラインの順。抑制した物は除く)。
fn joints(cx: &Context<'_>) -> Vec<Joint> {
    cx.data
        .features
        .iter()
        .filter(|f| f.type_id == JOINT && !f.suppressed)
        .filter_map(|f| serde_json::from_value::<Joint>(f.data.clone()).ok())
        .collect()
}

/// ボディ `root` を引いて自由に動かせる部品(部品の木の部品で、どのジョイントにも動かされない物)。
fn free_part(cx: &Context<'_>, root: FeatureId) -> Option<Part> {
    let part = Part::of_body(cx.data, root);
    if !matches!(part, Part::Component { .. }) {
        return None;
    }
    let roots = part.roots(cx.data);
    let moved = joints(cx).iter().any(|j| {
        j.bodies
            .iter()
            .any(|b| roots.contains(&body_root(cx.data, cx.registry, *b)))
    });
    (!moved).then_some(part)
}

/// 部品 `part` を拘束(止めていない物)が動かしているか。引いた分を足すと拘束の置いた位置が黙って
/// 崩れるので、引いて動かさない。
fn constrained(cx: &Context<'_>, part: &Part) -> bool {
    let roots = part.roots(cx.data);
    cx.data
        .features
        .iter()
        .filter(|f| f.type_id == crate::constraint::CONSTRAINT && !f.suppressed)
        .filter_map(|f| {
            serde_json::from_value::<crate::constraint::Constraint>(f.data.clone()).ok()
        })
        .any(|c| {
            c.moving == *part
                || c.bodies
                    .iter()
                    .any(|b| roots.contains(&body_root(cx.data, cx.registry, *b)))
        })
}

/// 部品 `part` と一緒に動く物(最初に作った機能): 部品のボディと、同じ剛性グループの部品と、
/// それらを土台にしたジョイントの部品(その先も)。
fn moving_set(cx: &Context<'_>, part: &Part) -> Vec<FeatureId> {
    crate::rigid::closure(cx, part.roots(cx.data))
}

/// 面 `at` を引いた時に、画面に平行に動かす物(ジョイントの無い部品)。
pub fn drag(cx: &Context<'_>, at: &kmb_core::FacePick) -> Option<kmb_core::Drag> {
    let root = body_root(cx.data, cx.registry, at.feature);
    let part = free_part(cx, root)?;
    let name = part.label(cx.data, cx.registry, cx.report);
    log::debug!("{name} を引いて動かす(面 {} から)", at.face);
    Some(kmb_core::Drag {
        feature: at.feature,
        field: DRAG_POSITION.into(),
        motion: kmb_core::DragMotion::Free,
        label: format!("{name} を動かす"),
        dofs: Vec::new(),
        still: Vec::new(),
    })
}

/// 欄の式が「数 mm」だけならその値(式やパラメータの名前なら `None`。足して直さず、新しく足す)。
/// 計算の結果は読まない: 引いている間はコマごとに戻してから入れ直すので、結果が古いことがある。
fn plain_mm(text: &str) -> Option<f64> {
    let t = text.trim();
    t.strip_suffix("mm").unwrap_or(t).trim().parse().ok()
}

fn mm(v: f64) -> String {
    // 1 μm に丸める(引いた量の細かい端数を欄に出さない)
    let v = (v * 1000.0).round() / 1000.0 + 0.0;
    format!("{v} mm")
}

/// 引いた量 `offset`(世界座標の mm)で、ボディ `body` の部品の位置の機能を足す(最後の機能が同じ
/// 部品の位置なら、その値に足して直す)。
pub fn drag_position(cx: &Context<'_>) -> Result<Outcome, String> {
    let body = cx
        .arg("body")
        .and_then(|v| v.as_u64())
        .map(FeatureId)
        .ok_or("引いたボディ(body)がありません")?;
    let offset: [f64; 3] = cx
        .arg("offset")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .ok_or("引いた量(offset)がありません")?;
    let root = body_root(cx.data, cx.registry, body);
    let part = free_part(cx, root).ok_or("ジョイントで動く部品か、部品に入っていないボディです")?;
    let name = part.label(cx.data, cx.registry, cx.report);
    if constrained(cx, &part) {
        return Err(format!(
            "{name} は拘束で置いた部品なので、引いて動かしません。動かすには拘束の値を直すか、拘束を止めてください"
        ));
    }
    let set = moving_set(cx, &part);
    let label = format!("{name} を動かす");
    if let Some(last) = cx.data.features.last().filter(|e| {
        e.type_id == POSITION
            && !e.suppressed
            && serde_json::from_value::<Position>(e.data.clone()).is_ok_and(|p| {
                p.moving == part
                    && p.bodies
                        .iter()
                        .map(|b| body_root(cx.data, cx.registry, *b))
                        .eq(set.iter().copied())
            })
    }) && let Ok(mut p) = serde_json::from_value::<Position>(last.data.clone())
        && let (Some(x), Some(y), Some(z)) = (plain_mm(&p.x), plain_mm(&p.y), plain_mm(&p.z))
    {
        let to = k::add([x, y, z], offset);
        [p.x, p.y, p.z] = to.map(mm);
        log::debug!("{} を直す: {to:?}", last.name);
        return Ok(Outcome::Change {
            label,
            edits: vec![Edit::SetData {
                id: last.id,
                data: serde_json::to_value(&p).map_err(|e| e.to_string())?,
            }],
        });
    }
    let bodies = set
        .iter()
        .map(|r| heir_before(cx.data, cx.registry, cx.report, *r, None))
        .collect();
    let [x, y, z] = offset.map(mm);
    let p = Position {
        moving: part,
        bodies,
        x,
        y,
        z,
    };
    let n = cx.count_of(POSITION) + 1;
    log::debug!("位置{n} を足す: {offset:?}(ボディ {} 個)", p.bodies.len());
    Ok(Outcome::Change {
        label,
        edits: vec![Edit::Insert {
            type_id: POSITION.into(),
            name: format!("位置{n}"),
            data: serde_json::to_value(&p).map_err(|e| e.to_string())?,
        }],
    })
}

/// ボディ `roots`(最初に作った機能)がみな入る、値で動かせる部品。部品に入っていない・別々の部品・
/// ジョイントや拘束で置いた部品なら理由。
pub fn move_target(cx: &Context<'_>, roots: &[FeatureId]) -> Result<Part, String> {
    let first = roots.first().ok_or("部品のボディを選んでください")?;
    let part = Part::of_body(cx.data, *first);
    if !matches!(part, Part::Component { .. }) {
        return Err("部品に入っていないボディです(ボディを動かすには移動/コピー)".into());
    }
    if roots.iter().any(|r| Part::of_body(cx.data, *r) != part) {
        return Err("別々の部品のボディを選んでいます(部品は 1 つずつ動かす)".into());
    }
    let name = part.label(cx.data, cx.registry, cx.report);
    if free_part(cx, *first).is_none() {
        return Err(format!(
            "{name} はジョイントで動く部品なので、ジョイントの値で動かします"
        ));
    }
    if constrained(cx, &part) {
        return Err(format!(
            "{name} は拘束で置いた部品なので、拘束の値で動かします"
        ));
    }
    Ok(part)
}

/// 部品 `part` を値で動かす位置の機能(0 mm から)を足す。いつも新しく足すので、作っている途中で
/// やめれば足す前に戻る。
pub fn move_part(cx: &Context<'_>, part: Part) -> Result<Outcome, String> {
    let name = part.label(cx.data, cx.registry, cx.report);
    let bodies = moving_set(cx, &part)
        .iter()
        .map(|r| heir_before(cx.data, cx.registry, cx.report, *r, None))
        .collect();
    let p = Position {
        moving: part,
        bodies,
        x: zero_mm(),
        y: zero_mm(),
        z: zero_mm(),
    };
    let n = cx.count_of(POSITION) + 1;
    log::debug!(
        "{name} を値で動かす位置{n} を足す(ボディ {} 個)",
        p.bodies.len()
    );
    Ok(Outcome::Change {
        label: format!("{name} を動かす"),
        edits: vec![Edit::Insert {
            type_id: POSITION.into(),
            name: format!("位置{n}"),
            data: serde_json::to_value(&p).map_err(|e| e.to_string())?,
        }],
    })
}

/// ジョイントの引く軸 `d` に、そのジョイントより後で部品を動かした位置の機能の動きを掛ける
/// (引いて動かした後の部品のジョイントも、見えている所の軸で回す)。
pub fn after_positions(cx: &Context<'_>, mut d: kmb_core::Drag, at: FeatureId) -> kmb_core::Drag {
    let root = body_root(cx.data, cx.registry, at);
    let Some(from) = cx.data.index_of(d.feature) else {
        return d;
    };
    let mut pose = k::IDENTITY;
    for e in cx.data.features.iter().skip(from + 1) {
        if e.type_id != POSITION || e.suppressed {
            continue;
        }
        let Ok(p) = serde_json::from_value::<Position>(e.data.clone()) else {
            continue;
        };
        if !p
            .bodies
            .iter()
            .any(|b| body_root(cx.data, cx.registry, *b) == root)
        {
            continue;
        }
        if let Some(m) = cx.output(e.id, "transform").and_then(transform_of) {
            pose = mul(&m, &pose);
        }
    }
    if pose == k::IDENTITY {
        return d;
    }
    d.motion = match d.motion {
        kmb_core::DragMotion::Rotate { origin, axis } => kmb_core::DragMotion::Rotate {
            origin: k::transform_point(&pose, origin),
            axis: apply_dir(&pose, axis),
        },
        kmb_core::DragMotion::Slide { axis } => kmb_core::DragMotion::Slide {
            axis: apply_dir(&pose, axis),
        },
        m => m,
    };
    for f in &mut d.dofs {
        f.origin = k::transform_point(&pose, f.origin);
        f.axis = apply_dir(&pose, f.axis);
    }
    d
}
