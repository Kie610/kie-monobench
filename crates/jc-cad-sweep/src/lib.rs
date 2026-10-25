//! スイープ(`solid.sweep@1`。輪郭を経路に沿って動かす)とロフト(`solid.loft@1`。断面をつなぐ)。
//!
//! 経路とガイドレールはスケッチの線(補助線も)から選ぶ。選んだ線に端のつながる線は、分かれ道に
//! 来るまで自動でたどる。曲面の作り方はカーネル
//! ([`jc_core::Kernel::sweep`]・[`jc_core::Kernel::loft`])。

use jc_cad::*;
use jc_core::kernel::{self as k, PlanarPath, ProfileEdge, Segment};
use jc_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Payload, Registry,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const SWEEP: &str = "solid.sweep@1";
pub const LOFT: &str = "solid.loft@1";
/// 輪郭 → 経路をクリックで選んでスイープを作る操作(ツールバーの「スイープ」)。
pub const SWEEP_SKETCH: &str = "cad.sweep";
/// 断面を順にクリックし、ガイドレールを選んでロフトを作る操作(ツールバーの「ロフト」)。
pub const LOFT_SKETCH: &str = "cad.loft";

/// スケッチの線(要素の番号)。`entity` を省くとスケッチの最初の線。端のつながる線もたどる。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathRef {
    pub sketch: FeatureId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<u32>,
}

/// 線の端どうしが同じ点とみなす隔たり(mm)。
const SAME_POINT: f64 = 1e-6;

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < SAME_POINT
}

/// 線 `entity` から、端のつながる線を分かれ道に来るまでたどった並び(たどる向きつき)。
/// 円はそれだけで閉じた経路。
pub fn chain(paths: &[(u32, Segment)], entity: Option<u32>) -> Result<Vec<ProfileEdge>, String> {
    let first = match entity {
        Some(id) => paths.iter().position(|(e, _)| *e == id).ok_or_else(|| {
            format!("線 {id} がスケッチにありません(線分・円弧・円を選んでください)")
        })?,
        None if paths.is_empty() => {
            return Err("スケッチに線がありません(線分・円弧・円を描いてください)".into());
        }
        None => 0,
    };
    let edge = |i: usize, reversed: bool| ProfileEdge {
        name: format!("e{}", paths[i].0),
        segment: paths[i].1.clone(),
        reversed,
    };
    if matches!(paths[first].1, Segment::Circle { .. }) {
        return Ok(vec![edge(first, false)]);
    }
    let mut used = vec![false; paths.len()];
    used[first] = true;
    let mut out = std::collections::VecDeque::from([edge(first, false)]);
    // 前へ(終わりの端から)、後ろへ(始まりの端から)たどる
    for forward in [true, false] {
        loop {
            let end = if forward {
                out.back().expect("線がある").to_point()
            } else {
                out.front().expect("線がある").from_point()
            };
            let start = out.front().expect("線がある").from_point();
            if forward && out.len() > 1 && near(end, start) {
                break; // 一周した
            }
            let next: Vec<(usize, bool)> = paths
                .iter()
                .enumerate()
                .filter(|(i, (_, s))| !used[*i] && !matches!(s, Segment::Circle { .. }))
                .filter_map(|(i, (_, s))| {
                    if near(s.start_point(), end) {
                        Some((i, !forward))
                    } else if near(s.end_point(), end) {
                        Some((i, forward))
                    } else {
                        None
                    }
                })
                .collect();
            // 分かれ道(3 本以上が出会う所)では止める
            let [(i, reversed)] = next[..] else { break };
            used[i] = true;
            if forward {
                out.push_back(edge(i, reversed));
            } else {
                out.push_front(edge(i, reversed));
            }
        }
    }
    Ok(out.into())
}

impl PathRef {
    fn inputs(&self) -> [InputRef; 2] {
        [output(self.sketch, "paths"), output(self.sketch, "profile")]
    }

    /// 選んだ線とつながる線の、平面の上の並び。
    fn resolve(&self, ctx: &EvalCtx<'_>, what: &str) -> Result<PlanarPath, EvalError> {
        let not_sketch =
            || EvalError::InvalidData(format!("{what}の {} はスケッチではありません", self.sketch));
        let paths = jc_core::Lines::from_value(ctx.output(self.sketch, "paths")?)
            .ok_or_else(not_sketch)?
            .0;
        let plane = k::Profile::from_value(ctx.output(self.sketch, "profile")?)
            .ok_or_else(not_sketch)?
            .plane;
        let edges = chain(&paths, self.entity)
            .map_err(|e| EvalError::InvalidData(format!("{what}: {e}")))?;
        Ok(PlanarPath { plane, edges })
    }
}

/// 輪郭(領域の名前を書けば、その領域だけ)。押し出し・回転と同じ選び方。
fn chosen(
    ctx: &EvalCtx<'_>,
    profile: FeatureId,
    regions: &[String],
) -> Result<k::Profile, EvalError> {
    if regions.is_empty() {
        return profile_from(ctx, profile);
    }
    jc_core::ProfileFaces::from_value(ctx.output(profile, "faces")?)
        .ok_or_else(|| {
            EvalError::InvalidData(format!("{profile} は領域を選べるスケッチではありません"))
        })?
        .profile(regions)
        .map_err(EvalError::InvalidData)
}

fn profile_inputs(profile: FeatureId, regions: &[String]) -> Vec<InputRef> {
    let mut v = vec![output(profile, "profile")];
    if !regions.is_empty() {
        v.push(output(profile, "faces"));
    }
    v
}

/// 輪郭を経路に沿って動かす。例: `{"profile": 1, "path": {"sketch": 2, "entity": 5}}`。
/// 経路の端の 1 つは輪郭の平面の上に置く(どちらの端でもよい)。輪郭は経路の曲がりに合わせて回り、
/// 直線どうしの角では角の二等分の平面で継ぐ。`twist`(角度の式)は終わりまでに輪郭を経路のまわりに
/// 回す角度、`scale`(倍率の式)は終わりの輪郭の大きさで、どちらも経路の長さに比べて一様に効く
/// (経路から離した円をねじればコイル、倍率を変えれば先の細い管)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sweep {
    pub profile: FeatureId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathRef>,
    #[serde(default)]
    pub operation: Operation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<FeatureId>,
    /// ねじれ(角度。空か 0 なら回さない)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twist: Option<String>,
    /// 終わりの大きさ(倍率。空か 1 なら変えない)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<String>,
}

impl Exprs for Sweep {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut v = Vec::new();
        v.extend(self.twist.as_mut());
        v.extend(self.scale.as_mut());
        v
    }
}

/// 終わりの大きさの上限(倍率)。
pub const MAX_SCALE: f64 = 100.0;

impl Sweep {
    /// ねじれ(ラジアン)と終わりの大きさ(倍率)。
    fn shape(&self, ctx: &EvalCtx<'_>) -> Result<(f64, f64), EvalError> {
        let filled = |s: &Option<String>| {
            s.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let twist = match filled(&self.twist) {
            Some(src) => angle(ctx, &src, "ねじれ")?,
            None => 0.0,
        };
        let scale = match filled(&self.scale) {
            Some(src) => {
                let q = ctx.expr(&src)?;
                if q.dim != jc_core::Dim::NONE {
                    return Err(EvalError::InvalidData(format!(
                        "終わりの大きさ `{src}` は単位の無い倍率にしてください(半分なら 0.5)"
                    )));
                }
                q.value
            }
            None => 1.0,
        };
        if !(scale > 0.0 && scale <= MAX_SCALE) {
            return Err(EvalError::InvalidData(format!(
                "終わりの大きさは 0 より大きく {MAX_SCALE} 倍までにしてください(今は {scale})"
            )));
        }
        Ok((twist, scale))
    }
}

impl Feature for Sweep {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = profile_inputs(self.profile, &self.regions);
        if let Some(p) = &self.path {
            v.extend(p.inputs());
        }
        v.extend(target_inputs(self.operation, self.target)?);
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        keep_target(self.operation, self.target)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let profile = chosen(ctx, self.profile, &self.regions)?;
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| {
                EvalError::InvalidData(
                    "経路をまだ選んでいません。経路にするスケッチの線を選んでください".into(),
                )
            })?
            .resolve(ctx, "経路")?;
        let (twist, scale) = self.shape(ctx)?;
        finish(
            ctx,
            Recipe::Sweep {
                operation: self.operation,
                profile,
                path,
                twist,
                scale,
            },
            self.target,
        )
    }
}

/// ロフトの断面の数の上限(手で選ぶ数より十分多く、渡し間違えの大量の断面で固まらない数)。
pub const MAX_SECTIONS: usize = 50;

/// ロフトの断面 1 つ(スケッチの輪郭か、その中の領域)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub profile: FeatureId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<String>,
}

/// 断面を順につなぐ。例: `{"sections": [{"profile": 1}, {"profile": 2}]}`。断面は閉じた輪郭 1 つずつ
/// (穴があれば、どの断面にも同じ数の穴を置くと中空のままつなぐ)で、続けて並べる断面は別の平面に置く。`rails`(2 本まで)は断面に触れる線で、形の外側を導く。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loft {
    pub sections: Vec<Section>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rails: Vec<PathRef>,
    #[serde(default)]
    pub operation: Operation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<FeatureId>,
}

impl Feature for Loft {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v: Vec<InputRef> = self
            .sections
            .iter()
            .flat_map(|s| profile_inputs(s.profile, &s.regions))
            .collect();
        for r in &self.rails {
            v.extend(r.inputs());
        }
        v.extend(target_inputs(self.operation, self.target)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        keep_target(self.operation, self.target)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        if self.sections.len() < 2 {
            return Err(EvalError::InvalidData(
                "ロフトには断面が 2 つ以上要ります(つなぐ輪郭を順に選んでください)".into(),
            ));
        }
        if self.sections.len() > MAX_SECTIONS {
            return Err(EvalError::InvalidData(format!(
                "ロフトの断面が {} 個あります。{MAX_SECTIONS} 個までにしてください(つなぐ形が長く固まるため)",
                self.sections.len()
            )));
        }
        // 同じ断面が 2 度入ると、厚さの無い所ができてカーネルが長く迷う(AI や台本が同じ輪郭を何度も渡した時)
        for (i, s) in self.sections.iter().enumerate() {
            if let Some(j) = self.sections[..i].iter().position(|t| t == s) {
                return Err(EvalError::InvalidData(format!(
                    "ロフトの断面 {} と {} が同じです({} の同じ輪郭)。1 つを外すか、別の輪郭を選んでください",
                    j + 1,
                    i + 1,
                    s.profile
                )));
            }
        }
        let sections = self
            .sections
            .iter()
            .map(|s| chosen(ctx, s.profile, &s.regions))
            .collect::<Result<Vec<_>, _>>()?;
        let rails = self
            .rails
            .iter()
            .enumerate()
            .map(|(i, r)| r.resolve(ctx, &format!("ガイドレール {}", i + 1)))
            .collect::<Result<Vec<_>, _>>()?;
        finish(
            ctx,
            Recipe::Loft {
                operation: self.operation,
                sections,
                rails,
            },
            self.target,
        )
    }
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(SWEEP, "スイープ")
            .icon("sweep")
            .hint("スケッチの輪郭を、別のスケッチの線(経路)に沿って動かす")
            .aliases("すいーぷ sweep 掃引 パイプ pipe")
            .field(Field::output("profile", "プロファイル", "profile"))
            .field(
                Field::output("path.sketch", "経路のスケッチ", "paths")
                    .help("経路にする線のあるスケッチ。線に端のつながる線は分かれ道までたどる"),
            )
            .field(
                Field::opt_integer("path.entity", "経路の線")
                    .unitless()
                    .help("空ならスケッチの最初の線"),
            )
            .field(
                Field::opt_expr("twist", "ねじれ")
                    .angle()
                    .help("終わりまでに輪郭を経路のまわりに回す角度。経路から離した円を 720° ねじると 2 巻きのコイル。空なら回さない"),
            )
            .field(
                Field::opt_expr("scale", "終わりの大きさ")
                    .ratio()
                    .help("終わりの輪郭の大きさ(始まりに比べた倍率。50% なら半分)。空なら同じ大きさ"),
            )
            .fields(operation_fields())
            .fix(
                "閉じた経路では",
                "ねじれと終わりの大きさを空にするか、経路を開いた線にする",
            )
            .fix(
                "",
                "経路の端の 1 つを輪郭の平面の上に置き、経路の曲がりが輪郭より大きいか確かめる",
            ),
        |d| Ok(Box::new(parse::<Sweep>(d)?)),
    );
    reg.register_described(
        solid(LOFT, "ロフト")
            .icon("loft")
            .hint(
                "別の平面に描いた輪郭(断面)を順になめらかにつなぐ。ガイドレールで外側の形を導ける",
            )
            .aliases("ろふと loft スキン skin 断面")
            .field(Field::rows(
                "sections",
                "断面",
                vec![Field::output("profile", "プロファイル", "profile")],
            ))
            .field(Field::rows(
                "rails",
                "ガイドレール",
                vec![
                    Field::output("sketch", "スケッチ", "paths"),
                    Field::opt_integer("entity", "線").unitless(),
                ],
            ))
            .fields(operation_fields())
            .fix(
                "同じ平面",
                "続けて並べる断面を、別の平面(スケッチの「平面からの距離」など)に描き直す",
            )
            .fix(
                "触れていません",
                "ガイドレールの線の端や途中が、断面の輪郭の上を通るように描き直す",
            )
            .fix(
                "穴が",
                "中空にする時はどの断面にも同じ数の穴を描く。中空にしない時は穴を含まない領域を選ぶ",
            )
            .fix(
                "",
                "断面が閉じた輪郭 1 つずつか、順番が合っているか確かめる",
            ),
        |d| Ok(Box::new(parse::<Loft>(d)?)),
    );
}

use jc_cad::regions;
use jc_core::{Action, Context, Extension, Gesture, Handle, HandleChoice, Outcome, Place};

/// 経路・ガイドレールを選ぶ手順の候補: 見えているスケッチの線の全部(補助線も。値
/// `{"sketch": スケッチ, "entity": 線}`)。円弧の多い線(楕円など)は 1 つの候補にまとめる。
fn line_choices(cx: &Context<'_>) -> Vec<HandleChoice> {
    let mut out = Vec::new();
    for id in cx.candidates("paths").into_iter().map(FeatureId) {
        let Some(paths) = cx
            .output(id, "paths")
            .and_then(jc_core::Lines::from_value)
            .map(|l| l.0)
        else {
            continue;
        };
        let Some(plane) = cx
            .output(id, "profile")
            .and_then(k::Profile::from_value)
            .map(|p| p.plane)
        else {
            continue;
        };
        let name = cx
            .data
            .feature(id)
            .map_or_else(|| id.to_string(), |f| f.name.clone());
        let mut seen: Vec<u32> = Vec::new();
        for (e, _) in &paths {
            if seen.contains(e) {
                continue;
            }
            seen.push(*e);
            let outline: Vec<Vec<[f64; 3]>> = paths
                .iter()
                .filter(|(x, _)| x == e)
                .map(|(_, s)| {
                    let mut pts: Vec<[f64; 3]> = s
                        .polyline(64)
                        .into_iter()
                        .map(|p| plane.to_world(p))
                        .collect();
                    let end = match s {
                        Segment::Circle { .. } => s.start_point(),
                        _ => s.end_point(),
                    };
                    pts.push(plane.to_world(end));
                    pts
                })
                .collect();
            out.push(HandleChoice {
                value: json!({"sketch": id.0, "entity": e}),
                label: format!("{name} の線 {e}"),
                outline,
                ..Default::default()
            });
        }
    }
    out
}

/// 手順の値・CLI の指定の線(`{"sketch": 2, "entity": 5}`)。
fn path_from(v: &serde_json::Value) -> Result<PathRef, String> {
    serde_json::from_value(v.clone()).map_err(|_| {
        "線は {\"sketch\": スケッチの機能の番号, \"entity\": 線の番号} で指定します".to_string()
    })
}

/// スイープを作る(輪郭と経路は手順の値か CLI・MCP の指定)。
fn add_sweep(cx: &Context<'_>) -> Result<Outcome, String> {
    let (sketch, keys) = regions::chosen_in_args(cx)
        .ok_or("動かす輪郭(閉じた輪郭のあるスケッチの領域)を選んでいません")?;
    regions::chosen_profile(cx, sketch, &keys)?;
    let path = cx
        .arg("path")
        .filter(|v| !v.is_null())
        .ok_or("経路を選んでいません(path に {\"sketch\": スケッチ, \"entity\": 線の番号})")?;
    let path = path_from(path)?;
    let twist = cx.arg_f64("twist")?.filter(|d| *d != 0.0);
    let scale = cx.arg_f64("scale")?.filter(|f| *f != 1.0);
    if scale.is_some_and(|f| !(f > 0.0 && f <= MAX_SCALE)) {
        return Err(format!(
            "終わりの大きさは 0 より大きく {MAX_SCALE} 倍までにしてください"
        ));
    }
    let data = Sweep {
        profile: sketch,
        regions: keys,
        path: Some(path),
        operation: Operation::New,
        target: None,
        twist: twist.map(|d| format!("{d} deg")),
        scale: scale.map(|f| f.to_string()),
    };
    Ok(Outcome::insert(
        "スイープを追加",
        SWEEP,
        format!("スイープ{}", cx.count_of(SWEEP) + 1),
        serde_json::to_value(data).map_err(|e| e.to_string())?,
    ))
}

/// 断面の指定(手順の値 `[{"profile": 3, "key": "e2.0+e7"}]`、CLI・MCP はスケッチの番号の並び
/// `[1, 2]` でもよい)を断面の並びに。
fn sections_from(v: &serde_json::Value) -> Result<Vec<Section>, String> {
    let bad = || {
        "断面は [{\"profile\": スケッチ, \"key\": 領域の名前}] か、スケッチの番号の並びで指定します"
            .to_string()
    };
    let list = v.as_array().ok_or_else(bad)?;
    list.iter()
        .map(|p| match p {
            serde_json::Value::Number(n) => Ok(Section {
                profile: FeatureId(n.as_u64().ok_or_else(bad)?),
                regions: vec![],
            }),
            serde_json::Value::Object(o) => Ok(Section {
                profile: FeatureId(o.get("profile").and_then(|x| x.as_u64()).ok_or_else(bad)?),
                regions: o
                    .get("key")
                    .and_then(|k| k.as_str())
                    .map(|k| vec![k.to_string()])
                    .unwrap_or_default(),
            }),
            _ => Err(bad()),
        })
        .collect()
}

/// 領域が 1 つだけのスケッチは領域の名前を覚えない(線を変えても全体を使う)。
fn tidy(cx: &Context<'_>, mut s: Section) -> Section {
    if regions::faces_of(cx, s.profile).len() <= 1 {
        s.regions.clear();
    }
    s
}

/// ロフトを作る(断面は最初の手順と次の手順の値をつないだ並び)。
fn add_loft(cx: &Context<'_>) -> Result<Outcome, String> {
    let mut sections = Vec::new();
    if let Some(v) = cx.arg("first").filter(|v| !v.is_null()) {
        sections.extend(sections_from(&serde_json::Value::Array(vec![v.clone()]))?);
    }
    if let Some(v) = cx.arg("sections").filter(|v| !v.is_null()) {
        sections.extend(sections_from(v)?);
    }
    let sections: Vec<Section> = sections.into_iter().map(|s| tidy(cx, s)).collect();
    if sections.len() < 2 {
        return Err("断面を 2 つ以上選んでください(つなぐ順にクリック)".into());
    }
    for s in &sections {
        regions::chosen_profile(cx, s.profile, &s.regions)?;
    }
    let rails = match cx.arg("rails").filter(|v| !v.is_null()) {
        Some(v) => v
            .as_array()
            .ok_or("rails は線の並び [{\"sketch\": スケッチ, \"entity\": 線}] で指定します")?
            .iter()
            .map(path_from)
            .collect::<Result<Vec<_>, _>>()?,
        None => vec![],
    };
    if rails.len() > 2 {
        return Err("ガイドレールは 2 本までです".into());
    }
    let data = Loft {
        sections,
        rails,
        operation: Operation::New,
        target: None,
    };
    Ok(Outcome::insert(
        "ロフトを追加",
        LOFT,
        format!("ロフト{}", cx.count_of(LOFT) + 1),
        serde_json::to_value(data).map_err(|e| e.to_string())?,
    ))
}

const PATH_LABEL: &str = "経路にする線をクリック(端のつながる線もたどる)";

/// スイープを作る手順: 輪郭(領域)をクリック → 経路の線をクリック。
fn sweep_handles(cx: &Context<'_>) -> Vec<Handle> {
    let sketches = match cx
        .selected
        .filter(|id| regions::profile_of(cx, *id).is_some())
    {
        Some(id) => vec![id],
        None => regions::candidates(cx),
    };
    vec![
        Handle {
            choices: regions::region_choices(cx, &sketches),
            ..Handle::new(
                "regions",
                "動かす領域をクリックすると経路へ(いくつも選ぶ時は Ctrl を押しながら)",
                Gesture::Regions,
            )
        },
        Handle {
            choices: line_choices(cx),
            ..Handle::new("path", PATH_LABEL, Gesture::Choice)
        },
    ]
}

/// ロフトを作る手順: 最初の断面 → 次の断面(3 つ以上は Ctrl を押しながら順に)→ ガイドレール
/// (無ければ Enter)。
fn loft_handles(cx: &Context<'_>) -> Vec<Handle> {
    // どのスケッチの領域も一緒に選べるよう、組を 1 つにする
    let mut choices = regions::region_choices(cx, &regions::candidates(cx));
    for c in &mut choices {
        c.group = 0;
    }
    vec![
        Handle {
            choices: choices.clone(),
            ..Handle::new("first", "最初の断面(輪郭)をクリック", Gesture::Choice)
        },
        Handle {
            choices,
            ..Handle::new(
                "sections",
                "次の断面をクリック(3 つ以上つなぐ時は Ctrl を押しながら順に。Enter で次へ)",
                Gesture::Regions,
            )
        },
        Handle {
            choices: line_choices(cx),
            optional: true,
            ..Handle::new(
                "rails",
                "ガイドレールにする線をクリック(2 本まで。無ければ Enter で作る)",
                Gesture::Regions,
            )
        },
    ]
}

/// スイープ・ロフトの操作。
pub struct SweepCommands;

impl Extension for SweepCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let none = regions::candidates(cx).is_empty();
        vec![
            Action::new(SWEEP_SKETCH, "スイープ", "cad")
                .args(regions::args())
                .arg(jc_core::Arg::value(
                    "path",
                    "経路の線 {\"sketch\": スケッチの機能の番号, \"entity\": 線の番号}(端のつながる線もたどる)",
                ))
                .arg(jc_core::Arg::angle(
                    "twist",
                    "ねじれ(終わりまでに輪郭を経路のまわりに回す角度。省けば回さない)",
                ))
                .arg(jc_core::Arg::number(
                    "scale",
                    "終わりの大きさ(始まりに比べた倍率。省けば 1)",
                    jc_core::FieldUnit::Ratio,
                ))
                .menu("作成")
                .icon("sweep")
                .aliases("すいーぷ sweep 掃引 パイプ pipe")
                .hint("押してから動かす領域をクリックし、経路にするスケッチの線をクリックする。経路の端の 1 つは輪郭の平面の上に置く")
                // スイープとロフトは帯の前には出さない(Kie 2026-10-21 の画板)
                .at(&[Place::Menu])
                .disabled_if(none.then_some("閉じた輪郭のあるスケッチがありません")),
            Action::new(LOFT_SKETCH, "ロフト", "cad")
                .arg(jc_core::Arg::value(
                    "first",
                    "最初の断面 {\"profile\": スケッチ, \"key\": 領域の名前} かスケッチの番号",
                ))
                .arg(jc_core::Arg::list(
                    "sections",
                    "続きの断面の並び(つなぐ順。[{\"profile\": スケッチ, \"key\": 領域の名前}] かスケッチの番号の並び)",
                ))
                .arg(jc_core::Arg::list(
                    "rails",
                    "ガイドレールの並び [{\"sketch\": スケッチ, \"entity\": 線の番号}](2 本まで。省いてよい)",
                ))
                .menu("作成")
                .icon("loft")
                .aliases("ろふと loft スキン skin 断面")
                .hint("押してから断面(別の平面に描いた輪郭)をつなぐ順にクリックし、ガイドレールの線を選ぶ(無ければ Enter)")
                .at(&[Place::Menu])
                .disabled_if(none.then_some("閉じた輪郭のあるスケッチがありません")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        match id {
            SWEEP_SKETCH => Some(add_sweep(cx)),
            LOFT_SKETCH => Some(add_loft(cx)),
            _ => None,
        }
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        match id {
            SWEEP_SKETCH => Some(sweep_handles(cx)),
            LOFT_SKETCH => Some(loft_handles(cx)),
            _ => None,
        }
    }
}

/// この crate の名乗り(登録する種類と操作)。組み立て(jc-bundle)が 1 回だけ並べる。
pub fn module() -> jc_core::Module {
    jc_core::Module::new("jc-cad-sweep", register).with_extension(SweepCommands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: [f64; 2], b: [f64; 2]) -> Segment {
        Segment::Line { a, b }
    }

    #[test]
    fn a_chain_follows_connected_lines_both_ways_and_stops_at_a_fork() {
        let paths = vec![
            (1, line([0.0, 0.0], [10.0, 0.0])),
            (2, line([10.0, 10.0], [10.0, 0.0])),
            (3, line([-5.0, 0.0], [0.0, 0.0])),
            (4, line([10.0, 10.0], [20.0, 10.0])),
            (5, line([10.0, 10.0], [0.0, 20.0])),
        ];
        let c = chain(&paths, Some(1)).unwrap();
        let names: Vec<&str> = c.iter().map(|e| e.name.as_str()).collect();
        // e2 の先(10, 10)は e4 と e5 に分かれるので止まる
        assert_eq!(names, ["e3", "e1", "e2"]);
        assert!(c[2].reversed && near(c[2].to_point(), [10.0, 10.0]));
        assert!(near(c[0].from_point(), [-5.0, 0.0]));
    }

    #[test]
    fn a_closed_chain_and_a_circle() {
        let paths = vec![
            (1, line([0.0, 0.0], [10.0, 0.0])),
            (2, line([10.0, 0.0], [10.0, 10.0])),
            (3, line([10.0, 10.0], [0.0, 0.0])),
            (
                4,
                Segment::Circle {
                    center: [50.0, 0.0],
                    radius: 3.0,
                },
            ),
        ];
        assert_eq!(chain(&paths, Some(2)).unwrap().len(), 3);
        assert_eq!(chain(&paths, Some(4)).unwrap().len(), 1);
        assert!(chain(&paths, Some(9)).is_err());
    }
}
