//! 同じ部品をいくつも置く(出現)。
//!
//! - 出現の機能 `asm.occurrence@1`([`Occurrence`]): 元のボディの形を写さず、置き方の行列で別の位置に
//!   置いたボディ。コアへは [`kmb_core::Feature::instance_of`] で宣言し、元を後ろで直すと出現も
//!   付いて来る(読み方はコア)。出現の形を変える機能はコアが誤りにする。
//! - 部品の木: 出現の 1 つは 1 つの部品。元の部品の全部のボディに出現の機能を 1 つずつ作り、
//!   新しい部品「元の名前:2」に入れる(入れ子の部品も同じ形の木で作る)。部品が出現かは、名前でなく
//!   中のボディの機能のデータで決める([`source_of`])。
//! - 操作: 部品をもう 1 つ置く(`asm.place_occurrence`)、独立した部品にする(`asm.occurrence_detach`。
//!   その時の形を写して持ち、以後は元を直しても変わらない)、元にそろえる(`asm.occurrence_sync`)。

use kmb_cad::solid;
use kmb_core::kernel::{self as k, Matrix4};
use kmb_core::{
    BodyId, Context, DocumentData, Edit, EvalCtx, EvalError, Feature, FeatureId, FeatureStatus,
    Field, InputRef, Outcome, Outputs, Problem, Registry, Value, body_labels, body_root,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const OCCURRENCE: &str = "asm.occurrence@1";
/// 部品をもう 1 つ置く操作(選んでいる部品か、引数 `part`)。
pub const PLACE_OCCURRENCE: &str = "asm.place_occurrence";
/// 出現の部品を独立した部品にする操作。
pub const DETACH_OCCURRENCE: &str = "asm.occurrence_detach";
/// 出現の部品のボディを元の部品にそろえる操作。
pub const SYNC_OCCURRENCE: &str = "asm.occurrence_sync";
/// 横に並べる時の隙間(mm)。
pub const GAP: f64 = 10.0;

/// 出現の機能のデータ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Occurrence {
    /// 元のボディ
    pub of: BodyId,
    /// 置き方: 元の定義の座標から、この出現の置き場所への行列(剛体と鏡映。4×4 を行の順に 16 個)
    pub place: [f64; 16],
    /// 独立させた時の形(あれば元を読まない)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detached: Option<Detached>,
}

/// 独立させた出現が持つ、その時の形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detached {
    /// 置いた位置の形(STEP)
    pub step: String,
    /// 面の名前(形の面の順)。読み直した形に付け直し、面を参照する後ろの機能が外れないようにする
    pub faces: Vec<String>,
}

/// 行の順の 16 個を行列にする。
pub fn matrix(place: &[f64; 16]) -> Matrix4 {
    std::array::from_fn(|r| std::array::from_fn(|c| place[r * 4 + c]))
}

/// 行列を行の順の 16 個にする。
pub fn flat(m: &Matrix4) -> [f64; 16] {
    std::array::from_fn(|i| m[i / 4][i % 4])
}

/// ボディを入れる出力の名前。
fn slot(id: BodyId) -> String {
    match id.part {
        0 => "body".into(),
        n => format!("part:{n}"),
    }
}

impl Occurrence {
    pub fn new(of: BodyId, place: &Matrix4) -> Self {
        Occurrence {
            of,
            place: flat(place),
            detached: None,
        }
    }

    fn source(&self) -> InputRef {
        kmb_cad::output(self.of.root, &slot(self.of))
    }
}

impl Feature for Occurrence {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(match self.detached {
            Some(_) => vec![],
            None => vec![self.source()],
        })
    }

    fn instance_of(&self) -> Option<InputRef> {
        self.detached.is_none().then(|| self.source())
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let kernel = ctx.kernel();
        let kernel_err = |e: kmb_core::KernelError| EvalError::Kernel(e.to_string());
        let h = match &self.detached {
            Some(d) => {
                let h = kernel.import_step(&d.step).map_err(kernel_err)?;
                let now = kernel.face_names(h).map_err(kernel_err)?;
                if now.len() != d.faces.len() {
                    return Err(EvalError::InvalidData(format!(
                        "独立させた形を読み直すと面の数が違います(持っている名前 {}、読んだ面 {})",
                        d.faces.len(),
                        now.len()
                    )));
                }
                let renames: Vec<(String, String)> = now
                    .into_iter()
                    .zip(d.faces.iter().cloned())
                    .filter(|(a, b)| a != b)
                    .collect();
                if renames.is_empty() {
                    h
                } else {
                    kernel.rename_faces(h, &renames).map_err(kernel_err)?
                }
            }
            None => {
                let m = matrix(&self.place);
                if let Some(why) = k::placement_problem(&m) {
                    return Err(EvalError::InvalidData(format!("置き方の行列: {why}")));
                }
                let src = match ctx.output(self.of.root, &slot(self.of)) {
                    Ok(Value::Shape(h)) => *h,
                    _ => {
                        return Err(EvalError::Other(format!(
                            "出現の元のボディ {} がありません",
                            self.of
                        )));
                    }
                };
                kernel.instance(src, &m).map_err(kernel_err)?
            }
        };
        Ok([("body".to_string(), Value::Shape(h))].into())
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(OCCURRENCE, "出現")
            .role(kmb_interchange::role::kind::OCCURRENCE)
            .icon("component")
            .hint("元の部品のボディを、形を写さずに別の位置に置いた物。元を直すと全部の出現が変わる。形は元の部品で直す")
            .aliases("しゅつげん 出現 どうじぶひん 同じ部品 いんすたんす インスタンス occurrence instance")
            .field(Field::info("of", "元のボディ"))
            .no_raw_editor(),
        |d| Ok(Box::new(kmb_cad::parse::<Occurrence>(d)?)),
    );
}

/// 機能 `id` が独立していない出現なら、そのデータ。
pub fn occurrence(data: &DocumentData, id: FeatureId) -> Option<Occurrence> {
    let e = data.feature(id)?;
    if e.type_id != OCCURRENCE {
        return None;
    }
    serde_json::from_value::<Occurrence>(e.data.clone())
        .ok()
        .filter(|o| o.detached.is_none())
}

/// 部品 `id` が出現の部品なら、元の部品。中のボディ(直下)が 1 つ以上あり、全部が同じ元の部品の
/// ボディの出現の時。
pub fn source_of(data: &DocumentData, id: u64) -> Option<u64> {
    let c = data.component(id)?;
    let mut src: Option<u64> = None;
    for b in &c.bodies {
        let o = occurrence(data, *b)?;
        let s = data.component_of(o.of.root)?.id;
        if s == id || src.is_some_and(|x| x != s) {
            return None;
        }
        src = Some(s);
    }
    src
}

/// 部品 `id` を元にした出現の部品(直下の部品について。並びは部品の木の順)。
pub fn occurrences_of(data: &DocumentData, id: u64) -> Vec<u64> {
    data.components
        .iter()
        .filter(|c| source_of(data, c.id) == Some(id))
        .map(|c| c.id)
        .collect()
}

/// 出現を置く部品(引数 `part` か、選んでいる面のボディ・選んでいるボディの部品)。
fn target(cx: &Context<'_>) -> Result<u64, String> {
    if let Some(v) = cx.arg("part") {
        let id = v.as_u64().ok_or("part は部品の番号で指定します")?;
        return cx
            .data
            .component(id)
            .map(|c| c.id)
            .ok_or_else(|| format!("部品 {id} がありません"));
    }
    let picked = cx
        .picked
        .map(|p| p.feature)
        .or(cx.selected)
        .ok_or("部品を選んでいません")?;
    let root = body_root(cx.data, cx.registry, picked);
    cx.data
        .component_of(root)
        .map(|c| c.id)
        .ok_or_else(|| "選んだボディは部品に入っていません(先に「新しい部品」でまとめる)".into())
}

/// 元の部品の名前と、次の出現の番号(「元の名前:2」の 2。文書の中で同じ元の通し番号)。
fn next_number(data: &DocumentData, src: u64) -> usize {
    let name = data.component(src).map(|c| c.name.as_str()).unwrap_or("");
    let taken: Vec<usize> = data
        .components
        .iter()
        .filter_map(|c| c.name.strip_prefix(name)?.strip_prefix(':')?.parse().ok())
        .collect();
    (2..).find(|n| !taken.contains(n)).unwrap_or(2)
}

/// ボディ `root` を動かした分(ジョイント・移動。定義の座標の形から今の形への行列)。
fn moved(cx: &Context<'_>, root: FeatureId) -> Matrix4 {
    cx.visible_bodies()
        .into_iter()
        .find(|(b, _)| body_root(cx.data, cx.registry, *b) == root)
        .and_then(|(_, h)| cx.kernel.instance_of(h))
        .map_or(k::IDENTITY, |(_, m)| m)
}

/// 部品 `src` の出現を、元の位置から `offsets` だけずらして 1 つずつ置く変更。入れ子の部品も同じ
/// 形の木で作る。新しい部品の番号は適用する側が割り当てるので、[`Edit`] を並べた順に数えて
/// 親の番号を決める(機能を足すたびと、部品を作るたびに 1 つ進む)。
pub fn place_edits(cx: &Context<'_>, src: u64, offsets: &[[f64; 3]]) -> Result<Vec<Edit>, String> {
    let data = cx.data;
    let top = data
        .component(src)
        .cloned()
        .ok_or_else(|| format!("部品 {src} がありません"))?;
    if source_of(data, src).is_some() {
        return Err(format!(
            "部品「{}」は出現なので、元の部品を選んで置いてください",
            top.name
        ));
    }
    let tree = data.component_tree(src);
    let visible = cx.visible_bodies();
    let shown: Vec<FeatureId> = visible.iter().map(|b| b.0).collect();
    let roots: Vec<FeatureId> = shown
        .iter()
        .map(|b| body_root(data, cx.registry, *b))
        .collect();
    let labels = body_labels(data, cx.registry, &shown);
    // 出現は元の今の位置(動かした後)の横に置く
    let moved = |root: FeatureId| moved(cx, root);
    let label_of = |root: FeatureId| {
        roots
            .iter()
            .position(|r| *r == root)
            .map(|i| labels[i].clone())
            .or_else(|| data.body_names.get(&root).cloned())
            .unwrap_or_else(|| format!("ボディ({root})"))
    };
    if tree
        .iter()
        .filter_map(|c| data.component(*c))
        .all(|c| c.bodies.is_empty())
    {
        return Err(format!("部品「{}」にはボディがありません", top.name));
    }
    let mut number = next_number(data, src);
    let mut edits = Vec::new();
    let mut next_id = data.next_id;
    for d in offsets {
        let m = k::translation(*d);
        let mut made: Vec<(u64, u64)> = Vec::new();
        for c in tree.iter().filter_map(|c| data.component(*c)) {
            let first = edits
                .iter()
                .filter(|e| matches!(e, Edit::Insert { .. }))
                .count();
            for root in &c.bodies {
                let o = Occurrence::new(BodyId::new(*root), &k::mat_mul(&m, &moved(*root)));
                edits.push(Edit::Insert {
                    type_id: OCCURRENCE.into(),
                    name: format!("出現({})", label_of(*root)),
                    data: serde_json::to_value(&o).map_err(|e| e.to_string())?,
                });
                next_id += 1;
            }
            let name = if c.id == src {
                format!("{}:{number}", c.name)
            } else {
                c.name.clone()
            };
            let parent = if c.id == src {
                c.parent
            } else {
                c.parent
                    .and_then(|p| made.iter().find(|(o, _)| *o == p).map(|(_, n)| *n))
            };
            edits.push(Edit::AddBodies {
                component: None,
                name,
                parent,
                bodies: c
                    .bodies
                    .iter()
                    .enumerate()
                    .map(|(i, r)| (first + i, label_of(*r)))
                    .collect(),
            });
            made.push((c.id, next_id));
            next_id += 1;
        }
        number += 1;
        while data
            .components
            .iter()
            .any(|c| c.name == format!("{}:{number}", top.name))
        {
            number += 1;
        }
    }
    Ok(edits)
}

/// 部品 `src` の外形の X の幅(見えているボディの箱。測れなければ 0)。
fn width(cx: &Context<'_>, src: u64) -> f64 {
    let roots = cx.data.component_bodies(src);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (b, h) in cx.visible_bodies() {
        if !roots.contains(&body_root(cx.data, cx.registry, b)) {
            continue;
        }
        if let Ok((a, z)) = cx.kernel.bounds(h) {
            lo = lo.min(a[0]);
            hi = hi.max(z[0]);
        }
    }
    if hi > lo { hi - lo } else { 0.0 }
}

/// 選んでいるボディの部品(引数を読まない。操作の一覧の使えない理由に使う)。
fn picked_part(cx: &Context<'_>) -> Option<u64> {
    let f = cx.picked.map(|p| p.feature).or(cx.selected)?;
    cx.data
        .component_of(body_root(cx.data, cx.registry, f))
        .map(|c| c.id)
}

/// 部品をもう 1 つ置けない理由。
pub fn place_why(cx: &Context<'_>) -> Option<&'static str> {
    cx.data
        .components
        .iter()
        .all(|c| c.bodies.is_empty())
        .then_some("ボディの入った部品がありません(先に「新しい部品」でまとめる)")
}

/// 独立させられない理由(選んでいる部品が出現でない時。何も選んでいなければ引数で渡せる)。
pub fn detach_why(cx: &Context<'_>) -> Option<&'static str> {
    if cx
        .data
        .components
        .iter()
        .all(|c| source_of(cx.data, c.id).is_none())
    {
        return Some("出現の部品がありません(「部品をもう 1 つ置く」で置く)");
    }
    match picked_part(cx) {
        Some(id) if source_of(cx.data, id).is_none() => Some("選んでいる部品は出現ではありません"),
        _ => None,
    }
}

/// 元にそろえられない理由。
pub fn sync_why(cx: &Context<'_>) -> Option<String> {
    if let Some(why) = detach_why(cx) {
        return Some(why.into());
    }
    let id = picked_part(cx)?;
    let src = source_of(cx.data, id)?;
    missing(cx.data, id, src)
        .is_empty()
        .then(|| "選んでいる部品は元の部品とそろっています".into())
}

/// 部品をもう 1 つ置く。
pub fn place(cx: &Context<'_>) -> Result<Outcome, Problem> {
    let src = target(cx)?;
    let src = source_of(cx.data, src).unwrap_or(src);
    let count = match cx.arg("count") {
        None | Some(serde_json::Value::Null) => 1,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=1000).contains(n))
            .ok_or("count は 1〜1000 の整数で指定します")?,
    } as usize;
    let given = [cx.arg_f64("x")?, cx.arg_f64("y")?, cx.arg_f64("z")?];
    let step = if given.iter().all(Option::is_none) {
        // 既定は元の幅と隙間だけ横へ(重ねて置かない)
        [((width(cx, src) + GAP) * 10.0).round() / 10.0, 0.0, 0.0]
    } else {
        given.map(|v| v.unwrap_or(0.0))
    };
    let already = occurrences_of(cx.data, src).len();
    let offsets: Vec<[f64; 3]> = (1..=count)
        .map(|i| step.map(|s| s * (already + i) as f64))
        .collect();
    let edits = place_edits(cx, src, &offsets)?;
    let name = cx
        .data
        .component(src)
        .map_or(String::new(), |c| c.name.clone());
    log::info!("部品「{name}」の出現を {count} 個置く(ずらし {step:?})");
    Ok(Outcome::Change {
        label: format!("部品「{name}」をもう {count} 個置く"),
        edits,
    })
}

/// 出現の部品(引数 `part` か選んでいる部品。出現でなければ誤り)。
fn occurrence_part(cx: &Context<'_>) -> Result<(u64, u64), String> {
    let id = target(cx)?;
    let name = cx.data.component(id).map_or("", |c| c.name.as_str());
    source_of(cx.data, id)
        .map(|s| (id, s))
        .ok_or_else(|| format!("部品「{name}」は出現ではありません"))
}

/// 独立した部品にする: 中の出現の機能(入れ子も)に、その時の形を写して持たせる。機能の番号は
/// 変えないので、ジョイントや参照はそのまま。
pub fn detach(cx: &Context<'_>) -> Result<Outcome, Problem> {
    let (id, _) = occurrence_part(cx)?;
    let mut edits = Vec::new();
    for root in cx.data.component_bodies(id) {
        let Some(mut o) = occurrence(cx.data, root) else {
            continue;
        };
        let h = match cx.report.feature(root) {
            Some(FeatureStatus::Ok { outputs, .. }) => match outputs.get("body") {
                Some(Value::Shape(h)) => *h,
                _ => return Err(format!("出現 {root} の形がありません").into()),
            },
            _ => {
                return Err(
                    format!("出現 {root} が計算できていないので、独立させられません").into(),
                );
            }
        };
        let step = cx.kernel.export_step(&[h]).map_err(|e| e.to_string())?;
        let faces = cx.kernel.face_names(h).map_err(|e| e.to_string())?;
        o.detached = Some(Detached { step, faces });
        edits.push(Edit::SetData {
            id: root,
            data: serde_json::to_value(&o).map_err(|e| e.to_string())?,
        });
    }
    let name = cx.data.component(id).map_or("", |c| c.name.as_str());
    log::info!("部品「{name}」を独立させる(出現 {} 個)", edits.len());
    Ok(Outcome::Change {
        label: format!("部品「{name}」を独立した部品にする"),
        edits,
    })
}

/// 出現の部品 `id` に無い、元の部品 `src` のボディ。
pub fn missing(data: &DocumentData, id: u64, src: u64) -> Vec<FeatureId> {
    let have: Vec<FeatureId> = data
        .component(id)
        .map(|c| {
            c.bodies
                .iter()
                .filter_map(|b| occurrence(data, *b).map(|o| o.of.root))
                .collect()
        })
        .unwrap_or_default();
    data.component(src)
        .map(|c| {
            c.bodies
                .iter()
                .filter(|b| !have.contains(b))
                .copied()
                .collect()
        })
        .unwrap_or_default()
}

/// 元にそろえる: 元の部品にあって出現の部品に無いボディの出現を、同じ置き方で足す。
pub fn sync(cx: &Context<'_>) -> Result<Outcome, Problem> {
    let (id, src) = occurrence_part(cx)?;
    let data = cx.data;
    let lack = missing(data, id, src);
    let name = data.component(id).map_or("", |c| c.name.as_str());
    if lack.is_empty() {
        return Ok(Outcome::Message(format!(
            "部品「{name}」は元の部品とそろっています"
        )));
    }
    // 出現の部品の置き方(元の今の位置から): 置いてある出現の 1 つの置き方から、その元を動かした分を除く
    let first = data
        .component(id)
        .and_then(|c| c.bodies.iter().find_map(|b| occurrence(data, *b)))
        .ok_or("出現の置き方が分かりません")?;
    let shift = k::mat_mul(
        &matrix(&first.place),
        &k::rigid_inverse(&moved(cx, first.of.root)),
    );
    let shown: Vec<FeatureId> = cx.visible_bodies().iter().map(|b| b.0).collect();
    let labels = body_labels(data, cx.registry, &shown);
    let label_of = |root: FeatureId| {
        shown
            .iter()
            .position(|b| body_root(data, cx.registry, *b) == root)
            .map(|i| labels[i].clone())
            .unwrap_or_else(|| format!("ボディ({root})"))
    };
    let mut edits = Vec::new();
    for root in &lack {
        let o = Occurrence::new(BodyId::new(*root), &k::mat_mul(&shift, &moved(cx, *root)));
        edits.push(Edit::Insert {
            type_id: OCCURRENCE.into(),
            name: format!("出現({})", label_of(*root)),
            data: json!(o),
        });
    }
    edits.push(Edit::AddBodies {
        component: Some(id),
        name: String::new(),
        parent: None,
        bodies: lack
            .iter()
            .enumerate()
            .map(|(i, r)| (i, label_of(*r)))
            .collect(),
    });
    log::info!("部品「{name}」を元にそろえる(足す出現 {} 個)", lack.len());
    Ok(Outcome::Change {
        label: format!("部品「{name}」を元にそろえる"),
        edits,
    })
}
