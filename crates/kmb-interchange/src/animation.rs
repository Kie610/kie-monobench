//! アニメーション。
//!
//! 段を並べた時間の帯で、段は前から順に動く。段は 6 種類:
//! - 分解: 部品(かボディ)を、向きと距離で動かす。
//! - まとめた分解: いくつもの部品を、それぞれの向きと距離で同時に動かす(自動分解のワンステップ)。
//! - ジョイントの動き: 回転・スライダのジョイントの値を、始めから終わりへ動かす。
//! - ビュー: 前のビューの段の視点から、この段の視点へ動かす([`view_at`])。
//! - 表示/非表示: 部品を段の始めに見せる・隠す([`shown_at`])。
//! - 外観: 部品の外観を、段の長さをかけて替える([`appearances_at`])。
//!
//! 分解の段には、元の位置から今の位置への跡の線(トレイルライン、[`trails`])を出す印を持てる。
//! 自動分解の向きと距離の決め方は kmb-asm の `anim`(docs/避ける形.md の決まりに従う)。
//!
//! 設計の形・加工・部品表は変えない。表示(3D のビュー)と動画だけが [`body_poses`] の行列で
//! ボディを動かして描く。ジョイントの機能は今の様子([`Drive`])を出力 `drive` に出す。
//!
//! 段は文書の区画 [`Storyboard`](`asm.animation`。組立のモジュール kmb-asm が名乗る)に入る。

use kmb_core::kernel::{self as k, Matrix4, P3};
use kmb_core::{
    DocumentData, Dof, FeatureId, FeatureStatus, RecomputeReport, Registry, SectionType, Value,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// ジョイントの今の様子([`Drive`])の出力の型。
pub const DRIVE_TYPE: &str = "core.drive@1";
/// [`Drive`] を出す出力の名前。
pub const DRIVE_SLOT: &str = "drive";
/// 一緒に動く塊(剛性グループ)が前のジョイントの動きに乗せたボディ([`Ride`])の出力の型。
pub const RIDE_TYPE: &str = "core.ride@1";
/// [`Ride`] を出す出力の名前。
pub const RIDE_SLOT: &str = "ride";
/// 段の長さの既定(秒)。
pub const STEP_SECONDS: f64 = 1.0;

/// 文書の区画 `asm.animation`: ストーリーボードの段(組立のモジュール kmb-asm が名乗る)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Storyboard {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<Step>,
    /// このアプリの知らない項目(新しいアプリが足した物)。読んだまま書き戻す
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl SectionType for Storyboard {
    const KEY: &'static str = "asm.animation";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "アニメーション";

    fn check(&self) -> Result<(), String> {
        self.steps.iter().try_for_each(check_step)
    }

    fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.rest.is_empty()
    }
}

/// 文書のアニメーションの段。区画を読めなければログに残して空。
pub fn steps(data: &DocumentData) -> Vec<Step> {
    data.section::<Storyboard>()
        .map(|s| s.steps)
        .unwrap_or_else(|e| {
            log::warn!("{e}");
            Vec::new()
        })
}

/// 段を `steps` に置き換える変更(取り消しの単位に入れる形)。区画の知らない項目は残す。
pub fn set_steps_op(data: &DocumentData, steps: Vec<Step>) -> Result<kmb_core::Op, String> {
    check_looks(data, &steps)?;
    let mut board: Storyboard = data.section().map_err(|e| e.to_string())?;
    board.steps = steps;
    data.section_op(&board)
}

/// [`set_steps_op`] と同じ変更を、操作の結果の形で。
pub fn set_steps(data: &DocumentData, steps: Vec<Step>) -> Result<kmb_core::Edit, String> {
    check_looks(data, &steps)?;
    let mut board: Storyboard = data.section().map_err(|e| e.to_string())?;
    board.steps = steps;
    data.set_section(&board)
}

/// 外観の段の外観が、ライブラリかこのデザイン内で作った外観にあるか(区画だけでは文書の外観を
/// 見られないので、段を文書に入れる時に確かめる)。
fn check_looks(data: &DocumentData, steps: &[Step]) -> Result<(), String> {
    let looks = kmb_look::appearance::of(data);
    for s in steps {
        if let StepKind::Appearance { appearance, .. } = &s.kind
            && looks.resolve(appearance).is_none()
        {
            return Err(format!(
                "{}にできません: 外観「{appearance}」はありません",
                Storyboard::LABEL
            ));
        }
    }
    Ok(())
}

/// 値で動かせる関係(回転かスライダのジョイント)の、今の位置での様子。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drive {
    /// 真ならスライダ(値は mm)、偽なら回転(値はラジアン)
    pub slide: bool,
    /// 軸の通る点(世界座標、今の位置)
    pub origin: P3,
    /// 軸の向き(長さ 1。反転も掛けた物)
    pub axis: P3,
    /// 今の値
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// 一緒に動くボディ(ジョイントの手前で受け継いだ機能)
    pub bodies: Vec<FeatureId>,
    /// 動きが 2 つ以上あるジョイント(円柱・ピンスロット・平面・ボール)の、動き全部。点に掛ける順で、
    /// 軸は値が全部 0 の時の物。部品をカーソルに付いて引く([`follow`])のに使う。動きが 1 つなら空
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dofs: Vec<Dof>,
    /// ほかのジョイントに連動する時の、連動元と比。値は
    /// 連動元の値(内部の単位)× 比。アニメーションなどで連動元の値を動かすと、これも動く
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<Lead>,
}

/// ジョイントの連動元([`Drive::lead`])。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Lead {
    /// 連動元のジョイント(タイムラインで前にある)
    pub joint: FeatureId,
    /// この値(内部の単位)= 連動元の値(内部の単位)× `ratio`
    pub ratio: f64,
}

/// 値 `values` の時の動き(全部 0 の時の位置から。`dofs` の順に掛ける)。
pub fn dofs_pose(dofs: &[Dof], values: &[f64]) -> Matrix4 {
    dofs.iter()
        .zip(values)
        .fold(k::IDENTITY, |m, (d, v)| mul(&d.matrix(*v), &m))
}

/// 値 `values` の時に点 `p` にある物の、全部 0 の時の位置。
pub fn dofs_unpose(dofs: &[Dof], values: &[f64], p: P3) -> P3 {
    dofs.iter()
        .zip(values)
        .rev()
        .fold(p, |p, (d, v)| k::transform_point(&d.matrix(-*v), p))
}

/// 部品をカーソルに付いて引く: 値を `values` から変えて、点 `at`(値が全部 0 の時の位置)を、点 `o`
/// から向き `dir` の線(カーソルの視線)にいちばん近づける。`moves` が偽の動きは変えない。値は
/// `limits` の下限・上限に収める。
///
/// 減衰付きの最小二乗(Levenberg)で、今の値から少しずつ寄せる(コマごとに前の値から始めるので、
/// 回し続けても跳ばない)。動きごとの列は長さで割ってから解くので、回転(ラジアン)と移動(mm)の
/// 単位の違いが効かない。点が軸の上にあって動かない回転や、視線と同じ向きの移動は使わない。
pub fn follow(
    dofs: &[Dof],
    values: &[f64],
    moves: &[bool],
    limits: &[(Option<f64>, Option<f64>)],
    at: P3,
    o: P3,
    dir: P3,
) -> Vec<f64> {
    let dir = k::normalize(dir);
    let off = |v: &[f64]| {
        let r = k::sub(k::transform_point(&dofs_pose(dofs, v), at), o);
        k::sub(r, k::scale(dir, k::dot(r, dir)))
    };
    let clamp = |i: usize, x: f64| {
        let (lo, hi) = limits.get(i).copied().unwrap_or((None, None));
        let x = lo.map_or(x, |lo| x.max(lo));
        hi.map_or(x, |hi| x.min(hi))
    };
    let mut v: Vec<f64> = values.to_vec();
    let free: Vec<usize> = (0..dofs.len())
        .filter(|i| moves.get(*i).copied().unwrap_or(false))
        .collect();
    for _ in 0..30 {
        let r = off(&v);
        if k::dot(r, r) < 1e-12 {
            break;
        }
        // 列(値を少し変えた時の点の動き。視線に垂直な成分)と、その長さ
        let h = 1e-6;
        let mut cols: Vec<(usize, P3, f64)> = Vec::new();
        for &i in &free {
            let (mut a, mut b) = (v.clone(), v.clone());
            a[i] += h;
            b[i] -= h;
            let c = k::scale(k::sub(off(&a), off(&b)), 0.5 / h);
            let n = k::dot(c, c).sqrt();
            if n > 1e-6 {
                cols.push((i, k::scale(c, 1.0 / n), n));
            }
        }
        if cols.is_empty() {
            break;
        }
        // (JᵀJ + λ²I) y = -Jᵀr を解く
        let lambda2 = 1e-2;
        let n = cols.len();
        let mut a = vec![vec![0.0; n + 1]; n];
        for (p, (_, cp, _)) in cols.iter().enumerate() {
            for (q, (_, cq, _)) in cols.iter().enumerate() {
                a[p][q] = k::dot(*cp, *cq) + if p == q { lambda2 } else { 0.0 };
            }
            a[p][n] = -k::dot(*cp, r);
        }
        let Some(y) = solve(a) else { break };
        let mut moved = 0.0f64;
        for ((i, _, len), y) in cols.iter().zip(y) {
            // 回転は 1 回に 0.5 ラジアンまで(遠くのカーソルへ大きく跳ばない)
            let step = if dofs[*i].slide {
                y / len
            } else {
                (y / len).clamp(-0.5, 0.5)
            };
            let next = clamp(*i, v[*i] + step);
            moved = moved.max((next - v[*i]).abs() * len);
            v[*i] = next;
        }
        if moved < 1e-9 {
            break;
        }
    }
    v
}

/// 小さな連立一次方程式(拡大係数行列)をガウスの消去で解く。解けなければ `None`。
fn solve(mut a: Vec<Vec<f64>>) -> Option<Vec<f64>> {
    let n = a.len();
    for c in 0..n {
        let p = (c..n).max_by(|x, y| a[*x][c].abs().total_cmp(&a[*y][c].abs()))?;
        if a[p][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, p);
        let pivot = a[c].clone();
        for (r, row) in a.iter_mut().enumerate() {
            if r != c {
                let f = row[c] / pivot[c];
                for (x, p) in row.iter_mut().zip(&pivot).skip(c) {
                    *x -= f * p;
                }
            }
        }
    }
    Some((0..n).map(|i| a[i][n] / a[i][i]).collect())
}

impl Drive {
    /// 値を `v` にした時の、今の位置からの動き(世界座標)。上下限があれば収める。
    pub fn delta(&self, v: f64) -> Matrix4 {
        let mut v = v;
        if let Some(lo) = self.min {
            v = v.max(lo);
        }
        if let Some(hi) = self.max {
            v = v.min(hi);
        }
        let d = v - self.value;
        if self.slide {
            k::translation(k::scale(self.axis, d))
        } else {
            k::rotation(self.origin, self.axis, d)
        }
    }

    /// 段の値の単位(回転は度、スライダは mm)から内部の値へ。
    pub fn from_shown(&self, v: f64) -> f64 {
        if self.slide { v } else { v.to_radians() }
    }

    /// 内部の値から段の値の単位へ。
    pub fn to_shown(&self, v: f64) -> f64 {
        if self.slide { v } else { v.to_degrees() }
    }

    /// 新しい段の(始め, 終わり)。段の値の単位。下限から上限まで。無い方は今の値から
    /// 回転は 90 度、スライダは `span` mm(動く物の軸の向きの長さ)。
    pub fn default_range(&self, span: f64) -> (f64, f64) {
        let step = if self.slide {
            span.max(1.0)
        } else {
            std::f64::consts::FRAC_PI_2
        };
        let (lo, hi) = match (self.min, self.max) {
            (Some(lo), Some(hi)) => (lo, hi),
            (Some(lo), None) => (lo, lo.max(self.value) + step),
            (None, Some(hi)) => (hi.min(self.value) - step, hi),
            (None, None) => (self.value, self.value + step),
        };
        (self.to_shown(lo), self.to_shown(hi))
    }

    pub fn to_value(&self) -> Value {
        Value::Custom {
            type_key: DRIVE_TYPE.into(),
            data: serde_json::to_value(self).expect("数だけなので書ける"),
        }
    }
}

/// 機能 `id` の出力の [`Drive`](回転・スライダのジョイントだけが出す)。
pub fn drive_of(report: &RecomputeReport, id: FeatureId) -> Option<Drive> {
    match report.feature(id)? {
        FeatureStatus::Ok { outputs, .. } => match outputs.get(DRIVE_SLOT)? {
            Value::Custom { type_key, data } if type_key == DRIVE_TYPE => {
                serde_json::from_value(data.clone()).ok()
            }
            _ => None,
        },
        _ => None,
    }
}

/// 一緒に動く塊が、前のジョイントの動きをほかの部品にも掛けて出したボディ(出力 [`RIDE_SLOT`])。
/// ジョイントを動かして見せる時は、そのジョイントのボディと一緒にこれらも動かす。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ride {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub riders: Vec<Rider>,
}

/// [`Ride`] の 1 つのボディ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rider {
    /// 動かしたボディ(その時の最後の機能)
    pub body: FeatureId,
    /// 動きを掛けたジョイント・位置
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub joints: Vec<FeatureId>,
}

impl Ride {
    pub fn to_value(&self) -> Value {
        Value::Custom {
            type_key: RIDE_TYPE.into(),
            data: serde_json::to_value(self).unwrap_or_default(),
        }
    }
}

/// ジョイント `joint` の動きに、一緒に動く塊が乗せたボディ(最後の機能。重ならない)。
pub fn riders_of(
    data: &DocumentData,
    report: &RecomputeReport,
    joint: FeatureId,
) -> Vec<FeatureId> {
    let mut out: Vec<FeatureId> = Vec::new();
    for f in &data.features {
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(f.id) else {
            continue;
        };
        let Some(Value::Custom { type_key, data }) = outputs.get(RIDE_SLOT) else {
            continue;
        };
        if type_key != RIDE_TYPE {
            continue;
        }
        let Ok(ride) = serde_json::from_value::<Ride>(data.clone()) else {
            continue;
        };
        for r in ride.riders {
            if r.joints.contains(&joint) && !out.contains(&r.body) {
                out.push(r.body);
            }
        }
    }
    out
}

/// ジョイント `joint` を動かした時に一緒に動くボディ: ジョイントのボディ `d.bodies` と、
/// 一緒に動く塊がその動きに乗せたボディ([`riders_of`])。
pub fn moved_bodies(
    data: &DocumentData,
    report: &RecomputeReport,
    joint: FeatureId,
    d: &Drive,
) -> Vec<FeatureId> {
    let mut v = d.bodies.clone();
    for b in riders_of(data, report, joint) {
        if !v.contains(&b) {
            v.push(b);
        }
    }
    v
}

/// 分解で動かす物。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// 部品の木の部品(中の部品も一緒)
    Component { id: u64 },
    /// 部品に入っていないボディ(最初に作った機能)
    Body { root: FeatureId },
}

impl Target {
    /// 中のボディ(最初に作った機能)。
    pub fn roots(&self, data: &DocumentData) -> Vec<FeatureId> {
        match self {
            Target::Component { id } => data.component_bodies(*id),
            Target::Body { root } => vec![*root],
        }
    }

    /// 画面に出す名前(部品の名前。ボディは呼ぶ側が決める)。
    pub fn component_name<'a>(&self, data: &'a DocumentData) -> Option<&'a str> {
        match self {
            Target::Component { id } => data.component(*id).map(|c| c.name.as_str()),
            Target::Body { .. } => None,
        }
    }
}

/// まとめた分解の 1 つの動き: 動かす物と、その平行の動き。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Move {
    pub targets: Vec<Target>,
    /// 動き(mm、世界座標)
    pub offset: P3,
}

/// ビューの段の視点(名前を付けた視点と同じ量。画面のカメラの回る中心・向き・見える高さ)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewPose {
    /// 画面の真ん中に来る点(世界座標、mm)
    pub target: P3,
    /// 水平の回り(度。X 軸から反時計回り)
    pub yaw: f64,
    /// 見下ろす角度(度。-90〜90)
    pub pitch: f64,
    /// 画面の縦に見える高さ(mm)
    pub height: f64,
    /// 透視投影で見るか(偽なら平行投影)
    #[serde(default)]
    pub perspective: bool,
}

impl ViewPose {
    /// `self` から `to` へ割合 `f`(0〜1)だけ進めた視点。中心はまっすぐ、ヨーは近い回り方で、
    /// 高さは比で(寄る・引くの速さがそろう)。投影は動き始めたら行き先の物。
    pub fn lerp(&self, to: &ViewPose, f: f64) -> ViewPose {
        let f = f.clamp(0.0, 1.0);
        let dyaw = (to.yaw - self.yaw + 180.0).rem_euclid(360.0) - 180.0;
        let (h0, h1) = (self.height.max(1e-6), to.height.max(1e-6));
        ViewPose {
            target: k::add(self.target, k::scale(k::sub(to.target, self.target), f)),
            yaw: self.yaw + dyaw * f,
            pitch: self.pitch + (to.pitch - self.pitch) * f,
            height: h0 * (h1 / h0).powf(f),
            perspective: if f > 0.0 {
                to.perspective
            } else {
                self.perspective
            },
        }
    }

    /// 同じ視点とみなせるか(中心 1e-3 mm、角度 1e-3 度、高さの比 1e-4 まで)。
    pub fn near(&self, o: &ViewPose) -> bool {
        let dyaw = (o.yaw - self.yaw + 180.0).rem_euclid(360.0) - 180.0;
        k::dot(k::sub(o.target, self.target), k::sub(o.target, self.target)) < 1e-6
            && dyaw.abs() < 1e-3
            && (o.pitch - self.pitch).abs() < 1e-3
            && (o.height / self.height.max(1e-9) - 1.0).abs() < 1e-4
            && o.perspective == self.perspective
    }
}

/// 段の中身。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepKind {
    /// 動かす物を `offset`(mm、世界座標)だけ平行に動かす
    Explode { targets: Vec<Target>, offset: P3 },
    /// いくつもの物を、それぞれの動きで同時に平行に動かす(自動分解のワンステップ)。
    /// 部品とその中の部品の両方の動きがあれば、中の部品は足した分だけ動く
    ExplodeMany { moves: Vec<Move> },
    /// ジョイント `joint` の値を `from` から `to` へ(回転は度、スライダは mm)
    Drive {
        joint: FeatureId,
        from: f64,
        to: f64,
    },
    /// 前のビューの段の視点から `view` へ動かす(最初のビューの段は、その段の前もこの視点)
    View { view: ViewPose },
    /// 動かす物を段の始めに見せる(`visible`)か隠す
    Visibility { targets: Vec<Target>, visible: bool },
    /// 動かす物の外観を、段の長さをかけて `appearance`(外観の名前)へ替える
    Appearance {
        targets: Vec<Target>,
        appearance: String,
    },
}

/// ストーリーボードの段 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    #[serde(flatten)]
    pub kind: StepKind,
    /// 長さ(秒)
    #[serde(default = "default_seconds")]
    pub seconds: f64,
    /// 分解の段で、動かした物の元の位置から今の位置へ跡の線(トレイルライン)を出す
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trail: bool,
}

impl Step {
    /// 長さ `seconds` の段(跡の線なし)。
    pub fn new(kind: StepKind, seconds: f64) -> Step {
        Step {
            kind,
            seconds,
            trail: false,
        }
    }

    /// 分解の段か(距離と跡の線を持つ段)。
    pub fn is_explode(&self) -> bool {
        matches!(
            self.kind,
            StepKind::Explode { .. } | StepKind::ExplodeMany { .. }
        )
    }

    /// 分解の段の動きの並び(動かす物, 動き)。ほかの段は空。
    pub fn moves(&self) -> Vec<(&[Target], P3)> {
        match &self.kind {
            StepKind::Explode { targets, offset } => vec![(targets.as_slice(), *offset)],
            StepKind::ExplodeMany { moves } => moves
                .iter()
                .map(|m| (m.targets.as_slice(), m.offset))
                .collect(),
            StepKind::Drive { .. }
            | StepKind::View { .. }
            | StepKind::Visibility { .. }
            | StepKind::Appearance { .. } => Vec::new(),
        }
    }

    /// 表示/非表示・外観の段の、対象の物。ほかの段は空。
    pub fn targets(&self) -> &[Target] {
        match &self.kind {
            StepKind::Visibility { targets, .. } | StepKind::Appearance { targets, .. } => targets,
            _ => &[],
        }
    }
}

fn default_seconds() -> f64 {
    STEP_SECONDS
}

/// 段が使える値か(長さが正、数が有限、分解は動かす物がある)。
pub fn check_step(s: &Step) -> Result<(), String> {
    if !(s.seconds.is_finite() && s.seconds > 0.0 && s.seconds <= 600.0) {
        return Err(format!(
            "段の長さ({} 秒)は 0 より大きく 600 以下にしてください",
            s.seconds
        ));
    }
    match &s.kind {
        StepKind::Explode { targets, offset } => {
            if targets.is_empty() {
                return Err("分解の段に動かす物がありません".into());
            }
            if !offset.iter().all(|x| x.is_finite()) {
                return Err("分解の距離が数ではありません".into());
            }
        }
        StepKind::ExplodeMany { moves } => {
            if moves.is_empty() || moves.iter().any(|m| m.targets.is_empty()) {
                return Err("分解の段に動かす物がありません".into());
            }
            if !moves.iter().all(|m| m.offset.iter().all(|x| x.is_finite())) {
                return Err("分解の距離が数ではありません".into());
            }
        }
        StepKind::Drive { from, to, .. } => {
            if !(from.is_finite() && to.is_finite()) {
                return Err("ジョイントの動きの値が数ではありません".into());
            }
        }
        StepKind::View { view } => {
            let ok = view.target.iter().all(|x| x.is_finite())
                && view.yaw.is_finite()
                && view.pitch.is_finite()
                && (-90.0..=90.0).contains(&view.pitch)
                && view.height.is_finite()
                && view.height > 0.0;
            if !ok {
                return Err(
                    "ビューの段の視点が正しくありません(高さは正、見下ろす角度は -90〜90 度)"
                        .into(),
                );
            }
        }
        StepKind::Visibility { targets, .. } => {
            if targets.is_empty() {
                return Err("表示/非表示の段に部品がありません".into());
            }
        }
        StepKind::Appearance {
            targets,
            appearance,
        } => {
            if targets.is_empty() {
                return Err("外観の段に部品がありません".into());
            }
            // 名前がライブラリかこのデザイン内の外観にあるかは、文書に入れる時に確かめる
            if appearance.trim().is_empty() {
                return Err("外観の段に外観がありません".into());
            }
        }
    }
    Ok(())
}

/// 全部の段の長さの和(秒)。
pub fn duration(steps: &[Step]) -> f64 {
    // 浮動小数の和は -0.0 から始まるので、段が無い時に「-0.00 秒」と出ないよう 0.0 を足す
    steps.iter().map(|s| s.seconds).sum::<f64>() + 0.0
}

/// 段 `i` の始まりの時刻(秒)。
pub fn start_of(steps: &[Step], i: usize) -> f64 {
    steps[..i.min(steps.len())].iter().map(|s| s.seconds).sum()
}

/// 時刻 `t` に動いている段(終わった後は最後の段)。段が無ければ `None`。
pub fn step_at(steps: &[Step], t: f64) -> Option<usize> {
    let mut at = 0.0;
    for (i, s) in steps.iter().enumerate() {
        if t < at + s.seconds {
            return Some(i);
        }
        at += s.seconds;
    }
    steps.len().checked_sub(1)
}

/// ゆっくり始まり、ゆっくり止まる(0 から 1)。
fn ease(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// 時刻 `t`(秒)の、ボディ(最初に作った機能)ごとの動き。動かないボディは入らない。
///
/// ジョイントは、そのジョイントの段のうち始まった最後の物の値(途中なら間の値)で、まだどの段も
/// 始まっていなければ最初の段の始めの値。分解は、始まった段の距離を足す(終わった段は全部の距離)。
/// ジョイントの動きを先に(タイムラインの順に、前のジョイントを外側に)掛け、分解をその上に足す。
pub fn poses(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    steps: &[Step],
    t: f64,
) -> BTreeMap<FeatureId, Matrix4> {
    // ジョイントごとの値(段の値の単位)
    let mut values: BTreeMap<FeatureId, f64> = BTreeMap::new();
    let mut offsets: BTreeMap<FeatureId, P3> = BTreeMap::new();
    let mut at = 0.0;
    for s in steps {
        let f = if s.seconds > 0.0 {
            ease((t - at) / s.seconds)
        } else {
            1.0
        };
        let started = t >= at;
        match &s.kind {
            StepKind::Drive { joint, from, to } => {
                if started || !values.contains_key(joint) {
                    values.insert(*joint, from + (to - from) * f);
                }
            }
            StepKind::Explode { .. } | StepKind::ExplodeMany { .. } => {
                if f > 0.0 {
                    // 1 つの動きの中では同じボディを 1 回だけ動かす(動きどうしは足す)
                    for (targets, offset) in s.moves() {
                        let mut seen: Vec<FeatureId> = Vec::new();
                        for r in targets.iter().flat_map(|g| g.roots(data)) {
                            if seen.contains(&r) {
                                continue;
                            }
                            seen.push(r);
                            let o = offsets.entry(r).or_insert([0.0; 3]);
                            *o = k::add(*o, k::scale(offset, f));
                        }
                    }
                }
            }
            StepKind::View { .. } | StepKind::Visibility { .. } | StepKind::Appearance { .. } => {}
        }
        at += s.seconds;
    }
    let mut out = joint_poses(data, registry, report, &values);
    for (r, o) in offsets {
        let cur = out.entry(r).or_insert(k::IDENTITY);
        *cur = mul(&k::translation(o), cur);
    }
    out.retain(|_, m| !near_identity(m));
    out
}

/// 時刻 `t` の、表示しているボディ(最後の機能)ごとの動き。動かないボディは入らない。
pub fn body_poses(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    steps: &[Step],
    t: f64,
) -> BTreeMap<FeatureId, Matrix4> {
    let by_root = poses(data, registry, report, steps, t);
    bodies_of_roots(data, registry, report, &by_root)
}

/// ジョイントごとの値(回転は度、スライダは mm)にした時の、ボディ(最初に作った機能)ごとの動き。
/// タイムラインの順に、前のジョイントを外側に掛ける。値を決めていないジョイントでも、連動元
/// ([`Drive::lead`])の値が決まっていれば連動して動く。動かないボディも入る(単位行列)ことがある。
pub fn joint_poses(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    values: &BTreeMap<FeatureId, f64>,
) -> BTreeMap<FeatureId, Matrix4> {
    let mut out: BTreeMap<FeatureId, Matrix4> = BTreeMap::new();
    // 決めた値(内部の単位)。連動するジョイントは連動元の値から足していく
    let mut inner: BTreeMap<FeatureId, f64> = BTreeMap::new();
    for f in &data.features {
        let Some(d) = drive_of(report, f.id) else {
            continue;
        };
        let v = match (values.get(&f.id), d.lead) {
            (Some(v), _) => d.from_shown(*v),
            (None, Some(l)) => match inner.get(&l.joint) {
                Some(a) => a * l.ratio,
                None => continue,
            },
            (None, None) => continue,
        };
        inner.insert(f.id, v);
        let m = d.delta(v);
        let mut roots: Vec<FeatureId> = Vec::new();
        for b in &moved_bodies(data, report, f.id, &d) {
            let r = kmb_core::body_root(data, registry, *b);
            if !roots.contains(&r) {
                roots.push(r);
            }
        }
        for r in roots {
            let cur = out.entry(r).or_insert(k::IDENTITY);
            *cur = mul(cur, &m);
        }
    }
    out
}

/// ボディ(最初に作った機能)ごとの動きを、表示しているボディ(最後の機能)ごとの動きにする。
/// 動かない物は入らない。
pub fn bodies_of_roots(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    by_root: &BTreeMap<FeatureId, Matrix4>,
) -> BTreeMap<FeatureId, Matrix4> {
    if by_root.values().all(near_identity) {
        return BTreeMap::new();
    }
    kmb_core::visible_bodies(data, registry, report)
        .into_iter()
        .filter_map(|(id, _)| {
            let r = kmb_core::body_root(data, registry, id);
            by_root
                .get(&r)
                .filter(|m| !near_identity(m))
                .map(|m| (id, *m))
        })
        .collect()
}

/// 時刻 `t` の視点。ビューの段が無ければ `None`(画面の視点のまま)。最初のビューの段が
/// 始まる前はその段の視点、段の間は前のビューの段の視点からの途中、最後の段の後はその視点。
pub fn view_at(steps: &[Step], t: f64) -> Option<ViewPose> {
    let mut prev: Option<ViewPose> = None;
    let mut at = 0.0;
    for s in steps {
        if let StepKind::View { view } = &s.kind {
            let from = prev.unwrap_or(*view);
            if t < at {
                return Some(from);
            }
            if t < at + s.seconds {
                return Some(from.lerp(view, ease((t - at) / s.seconds)));
            }
            prev = Some(*view);
        }
        at += s.seconds;
    }
    prev
}

/// 時刻 `t` に、表示/非表示の段で見せる(真)・隠す(偽)と決まったボディ(最初に作った機能)。
/// 始まった段の最後の物が効く。どの段にも出てこないボディは入らない(文書の表示のまま)。
pub fn shown_at(data: &DocumentData, steps: &[Step], t: f64) -> BTreeMap<FeatureId, bool> {
    let mut out = BTreeMap::new();
    let mut at = 0.0;
    for s in steps {
        if t < at {
            break;
        }
        if let StepKind::Visibility { targets, visible } = &s.kind {
            for r in targets.iter().flat_map(|g| g.roots(data)) {
                out.insert(r, *visible);
            }
        }
        at += s.seconds;
    }
    out
}

/// 外観の段で替えている途中か替えた後の、ボディの外観。
#[derive(Debug, Clone, PartialEq)]
pub struct Blend {
    /// 替える前の外観(`None` は文書の外観)
    pub from: Option<String>,
    /// 替えた先の外観
    pub to: String,
    /// 進んだ割合(0〜1。1 なら替え終えた)
    pub f: f64,
}

impl Blend {
    /// 色を混ぜる(`from` は替える前の色)。
    pub fn mix(&self, from: [u8; 3], to: [u8; 3]) -> [u8; 3] {
        let m = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * self.f).round() as u8;
        [m(from[0], to[0]), m(from[1], to[1]), m(from[2], to[2])]
    }

    /// 見た目を決める外観の名前(半分より進んだら替えた先)。
    pub fn shown(&self) -> Option<&str> {
        if self.f >= 0.5 {
            Some(&self.to)
        } else {
            self.from.as_deref()
        }
    }
}

/// 時刻 `t` の、外観の段で替えたボディ(最初に作った機能)ごとの外観。始まっていない段は効かない。
/// 外観の段に出てこないボディは入らない(文書の外観のまま)。
pub fn appearances_at(data: &DocumentData, steps: &[Step], t: f64) -> BTreeMap<FeatureId, Blend> {
    let mut out: BTreeMap<FeatureId, Blend> = BTreeMap::new();
    let mut at = 0.0;
    for s in steps {
        if t < at {
            break;
        }
        if let StepKind::Appearance {
            targets,
            appearance,
        } = &s.kind
        {
            let f = if s.seconds > 0.0 {
                ease((t - at) / s.seconds)
            } else {
                1.0
            };
            for r in targets.iter().flat_map(|g| g.roots(data)) {
                // 前の段で替え終えていれば、その外観から替える(途中の物は替えた先とみなす)
                let from = out.get(&r).map(|b| b.to.clone());
                out.insert(
                    r,
                    Blend {
                        from,
                        to: appearance.clone(),
                        f,
                    },
                );
            }
        }
        at += s.seconds;
    }
    out
}

/// 時刻 `t` の跡の線(トレイルライン): 跡の線の印を持つ分解の段で動くボディ(最初に作った機能)
/// ごとに、元の位置の中心(`center`)から、段の区切りごとの位置を通って今の位置までの折れ線。
/// 動いていないボディの線は入らない。
pub fn trails(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    steps: &[Step],
    t: f64,
    center: impl Fn(FeatureId) -> Option<P3>,
) -> Vec<Vec<P3>> {
    let mut roots: Vec<FeatureId> = Vec::new();
    for s in steps.iter().filter(|s| s.trail) {
        for (targets, _) in s.moves() {
            for r in targets.iter().flat_map(|g| g.roots(data)) {
                if !roots.contains(&r) {
                    roots.push(r);
                }
            }
        }
    }
    if roots.is_empty() {
        return Vec::new();
    }
    // 時刻: 初め、今より前の段の区切り、今
    let mut times = vec![0.0];
    let mut at = 0.0;
    for s in steps {
        at += s.seconds;
        if at < t {
            times.push(at);
        }
    }
    times.push(t);
    let poses: Vec<BTreeMap<FeatureId, Matrix4>> = times
        .iter()
        .map(|&x| self::poses(data, registry, report, steps, x))
        .collect();
    let mut out = Vec::new();
    for r in roots {
        let Some(c) = center(r) else {
            continue;
        };
        let mut line: Vec<P3> = Vec::new();
        for p in &poses {
            let q = p.get(&r).map_or(c, |m| k::transform_point(m, c));
            if line
                .last()
                .is_none_or(|l| k::dot(k::sub(q, *l), k::sub(q, *l)) > 1e-12)
            {
                line.push(q);
            }
        }
        if line.len() >= 2 {
            out.push(line);
        }
    }
    out
}

/// 行列の積 `a × b`(`b` を先に掛ける)。
pub use kmb_core::kernel::mat_mul as mul;

fn near_identity(m: &Matrix4) -> bool {
    (0..4).all(|i| (0..4).all(|j| (m[i][j] - k::IDENTITY[i][j]).abs() < 1e-12))
}

/// 網に行列を掛けた物(点・法線・辺の線)。
pub fn transform_mesh(mesh: &kmb_core::Mesh, m: &Matrix4) -> kmb_core::Mesh {
    let p = |q: &[f32; 3]| {
        let r = k::transform_point(m, [q[0] as f64, q[1] as f64, q[2] as f64]);
        [r[0] as f32, r[1] as f32, r[2] as f32]
    };
    let n = |q: &[f32; 3]| {
        let d = [q[0] as f64, q[1] as f64, q[2] as f64];
        let r: Vec<f64> = (0..3)
            .map(|i| m[i][0] * d[0] + m[i][1] * d[1] + m[i][2] * d[2])
            .collect();
        [r[0] as f32, r[1] as f32, r[2] as f32]
    };
    kmb_core::Mesh {
        positions: mesh.positions.iter().map(p).collect(),
        normals: mesh.normals.iter().map(n).collect(),
        triangles: mesh.triangles.clone(),
        triangle_faces: mesh.triangle_faces.clone(),
        face_names: mesh.face_names.clone(),
        edges: mesh
            .edges
            .iter()
            .map(|e| e.iter().map(p).collect())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::{Component, Document, Transaction};

    fn explode(targets: Vec<Target>, offset: P3, seconds: f64) -> Step {
        Step::new(StepKind::Explode { targets, offset }, seconds)
    }

    fn view(x: f64, yaw: f64, height: f64) -> ViewPose {
        ViewPose {
            target: [x, 0.0, 0.0],
            yaw,
            pitch: 30.0,
            height,
            perspective: false,
        }
    }

    /// ビューの段: 最初の段の前はその視点、段の間は前の視点からの途中(ヨーは近い回り方、高さは比)、
    /// 最後の段の後はその視点。ビューの段が無ければ画面の視点のまま。
    #[test]
    fn view_steps_move_the_camera_from_the_previous_view() {
        let b = |r| Target::Body { root: FeatureId(r) };
        let a = view(0.0, 170.0, 100.0);
        let c = view(10.0, -170.0, 400.0);
        let steps = vec![
            explode(vec![b(1)], [0.0, 0.0, 10.0], 1.0),
            Step::new(StepKind::View { view: a }, 1.0),
            Step::new(StepKind::View { view: c }, 2.0),
        ];
        assert!(check_step(&steps[1]).is_ok());
        assert_eq!(
            view_at(&steps, 0.0),
            Some(a),
            "最初のビューの段の前はその視点"
        );
        assert_eq!(view_at(&steps, 1.5), Some(a), "最初のビューの段は動かない");
        let mid = view_at(&steps, 3.0).unwrap();
        assert!((mid.target[0] - 5.0).abs() < 1e-9);
        // 170 度から -170 度へは 180 度をまたいで 20 度だけ回る
        assert!((mid.yaw - 180.0).abs() < 1e-9, "{}", mid.yaw);
        assert!(
            (mid.height - 200.0).abs() < 1e-9,
            "高さは比で: {}",
            mid.height
        );
        assert!(view_at(&steps, 9.0).unwrap().near(&c));
        assert_eq!(view_at(&steps[..1], 0.5), None);
        let bad = Step::new(
            StepKind::View {
                view: view(0.0, 0.0, 0.0),
            },
            1.0,
        );
        assert!(check_step(&bad).is_err(), "高さ 0 は使えない");
    }

    /// 表示/非表示の段は段の始めに効き、後の段が前の段を上書きする。外観の段は段の長さをかけて
    /// 替え、前の段で替えた外観から次の段が替える。
    #[test]
    fn visibility_and_appearance_steps() {
        let b = |r| Target::Body { root: FeatureId(r) };
        let data = DocumentData::default();
        let steps = vec![
            Step::new(
                StepKind::Visibility {
                    targets: vec![b(1), b(2)],
                    visible: false,
                },
                1.0,
            ),
            Step::new(
                StepKind::Appearance {
                    targets: vec![b(2)],
                    appearance: "赤".into(),
                },
                2.0,
            ),
            Step::new(
                StepKind::Visibility {
                    targets: vec![b(1)],
                    visible: true,
                },
                1.0,
            ),
            Step::new(
                StepKind::Appearance {
                    targets: vec![b(2)],
                    appearance: "青".into(),
                },
                1.0,
            ),
        ];
        for s in &steps {
            assert!(check_step(s).is_ok());
        }
        let shown = |t| shown_at(&data, &steps, t);
        assert!(!shown(0.0)[&FeatureId(1)], "段の始めで隠す");
        assert!(!shown(2.9)[&FeatureId(1)]);
        assert!(shown(3.0)[&FeatureId(1)], "後の段で見せる");
        assert!(!shown(3.0)[&FeatureId(2)]);
        assert!(!shown(9.0).contains_key(&FeatureId(3)));
        let look = |t| appearances_at(&data, &steps, t);
        assert!(look(0.5).is_empty(), "外観の段の前は文書の外観");
        let half = &look(2.0)[&FeatureId(2)];
        assert_eq!((half.from.as_deref(), half.to.as_str()), (None, "赤"));
        assert!((half.f - 0.5).abs() < 1e-9);
        assert_eq!(half.mix([0, 0, 0], [200, 100, 50]), [100, 50, 25]);
        let blue = &look(9.0)[&FeatureId(2)];
        assert_eq!(
            (blue.from.as_deref(), blue.to.as_str(), blue.f),
            (Some("赤"), "青", 1.0)
        );
        assert_eq!(blue.shown(), Some("青"));
        let bad = Step::new(
            StepKind::Appearance {
                targets: vec![b(1)],
                appearance: "金色".into(),
            },
            1.0,
        );
        // 知らない外観は文書に入れる時に断り、このデザイン内で作った外観は受ける
        let data = DocumentData::default();
        assert!(
            set_steps(&data, vec![bad.clone()])
                .unwrap_err()
                .contains("金色")
        );
        let mut own = kmb_look::appearance::Appearances::default();
        own.defs.push(kmb_look::appearance::AppearanceDef {
            name: "金色".into(),
            category: "塗装".into(),
            color: [200, 170, 60],
            metallic: 1.0,
            roughness: 0.3,
            base: None,
        });
        let mut data = DocumentData::default();
        let Ok(kmb_core::Edit::SetSection { key, section }) = data.set_section(&own) else {
            panic!("外観の区画");
        };
        data.sections.insert(key, section.expect("空でない"));
        assert!(set_steps(&data, vec![bad]).is_ok());
    }

    /// 段の時刻: 長さの和、時刻の段(終わった後は最後)、ゆっくり始まりゆっくり止まる。
    #[test]
    fn steps_follow_one_another_and_ease_in_and_out() {
        let b = |r| Target::Body { root: FeatureId(r) };
        let steps = vec![
            explode(vec![b(1)], [0.0, 0.0, 10.0], 1.0),
            explode(vec![b(1), b(2)], [10.0, 0.0, 0.0], 2.0),
        ];
        assert_eq!(duration(&steps), 3.0);
        assert_eq!(start_of(&steps, 1), 1.0);
        assert_eq!(step_at(&steps, 0.5), Some(0));
        assert_eq!(step_at(&steps, 1.0), Some(1));
        assert_eq!(step_at(&steps, 9.0), Some(1));
        assert_eq!(step_at(&[], 0.0), None);
        assert!(duration(&[]).is_sign_positive(), "段が無い時は +0 秒");
        let data = DocumentData::default();
        let reg = Registry::new();
        let report = RecomputeReport::default();
        let at = |t| poses(&data, &reg, &report, &steps, t);
        assert!(at(0.0).is_empty(), "初めは動かない");
        // 段 1 の真ん中は半分(ゆっくり始まり止まるので真ん中は 0.5)
        assert!((at(0.5)[&FeatureId(1)][2][3] - 5.0).abs() < 1e-9);
        let end = at(3.0);
        assert_eq!(end[&FeatureId(1)][0][3], 10.0);
        assert_eq!(end[&FeatureId(1)][2][3], 10.0);
        assert_eq!(end[&FeatureId(2)][0][3], 10.0);
        assert_eq!(end[&FeatureId(2)][2][3], 0.0);
        // 段 2 の 1/4 の時刻は、ゆっくり始まるので 1/4 より手前
        let q = at(1.5)[&FeatureId(2)][0][3];
        assert!(q > 0.0 && q < 2.5, "{q}");
    }

    /// 部品を動かすと、中の部品のボディも一緒に動く。同じボディは 1 回だけ動かす。
    #[test]
    fn a_component_moves_the_bodies_of_its_children() {
        let data = DocumentData {
            components: vec![
                Component {
                    id: 10,
                    name: "引き出し".into(),
                    parent: None,
                    bodies: vec![FeatureId(1)],
                },
                Component {
                    id: 11,
                    name: "取っ手".into(),
                    parent: Some(10),
                    bodies: vec![FeatureId(2)],
                },
            ],
            ..Default::default()
        };
        let steps = vec![explode(
            vec![
                Target::Component { id: 10 },
                Target::Body { root: FeatureId(2) },
            ],
            [0.0, -30.0, 0.0],
            1.0,
        )];
        let p = poses(
            &data,
            &Registry::new(),
            &RecomputeReport::default(),
            &steps,
            1.0,
        );
        assert_eq!(p.len(), 2);
        assert_eq!(p[&FeatureId(2)][1][3], -30.0);
        assert_eq!(
            Target::Component { id: 11 }.component_name(&data),
            Some("取っ手")
        );
    }

    fn rot(field: &str, axis: P3) -> Dof {
        Dof {
            field: field.into(),
            slide: false,
            origin: [0.0; 3],
            axis,
        }
    }
    fn slide(field: &str, axis: P3) -> Dof {
        Dof {
            field: field.into(),
            slide: true,
            origin: [0.0; 3],
            axis,
        }
    }
    /// 点 `p` を通り向き `dir` の視線(カーソル)。
    fn ray_to(p: P3, dir: P3) -> (P3, P3) {
        (k::sub(p, k::scale(dir, 100.0)), dir)
    }
    fn far(a: P3, b: P3) -> f64 {
        let d = k::sub(a, b);
        k::dot(d, d).sqrt()
    }

    /// 円柱: 斜めから見て、回して軸の向きにも動かした所へカーソルを引くと、その値になる(コマごとに
    /// 前の値から寄せる)。値の点はカーソルの上に来る。
    #[test]
    fn a_cylindrical_part_follows_the_cursor_by_turning_and_sliding() {
        let dofs = [
            rot("angle", [0.0, 0.0, 1.0]),
            slide("distance", [0.0, 0.0, 1.0]),
        ];
        let at = [10.0, 0.0, 0.0];
        let dir = k::normalize([-1.0, -0.6, -0.8]);
        let mut v = vec![0.0, 0.0];
        let goal = [std::f64::consts::FRAC_PI_2, 5.0];
        for i in 1..=20 {
            let t = f64::from(i) / 20.0;
            let target = k::transform_point(&dofs_pose(&dofs, &[goal[0] * t, goal[1] * t]), at);
            let (o, d) = ray_to(target, dir);
            v = follow(&dofs, &v, &[true, true], &[(None, None); 2], at, o, d);
        }
        assert!(
            (v[0] - goal[0]).abs() < 1e-3 && (v[1] - goal[1]).abs() < 1e-3,
            "{v:?}"
        );
    }

    /// 回る面を正面から見て、カーソルで軸のまわりを 1 周すると、値も 1 周分(2π)進む(半周で跳ばない)。
    #[test]
    fn a_turn_seen_from_the_front_keeps_going_around() {
        let dofs = [
            rot("angle", [0.0, 0.0, 1.0]),
            slide("distance", [0.0, 0.0, 1.0]),
        ];
        let at = [10.0, 0.0, 0.0];
        let mut v = vec![0.0, 0.0];
        for i in 1..=36 {
            let a = f64::from(i) * 10f64.to_radians();
            let (o, d) = ray_to([10.0 * a.cos(), 10.0 * a.sin(), 0.0], [0.0, 0.0, -1.0]);
            v = follow(&dofs, &v, &[true, true], &[(None, None); 2], at, o, d);
        }
        assert!((v[0] - std::f64::consts::TAU).abs() < 1e-3, "{v:?}");
        // 視線と同じ向きの移動は使わない
        assert_eq!(v[1], 0.0);
    }

    /// 平面: 回転を止めて X・Y だけで引けば、向きを変えずにカーソルへ付く。回転も動かすと、
    /// 少しずつ回ってしまう(引いた点が届く組が多く、最小二乗が回転にも分ける)ので、平面は回さない。
    #[test]
    fn a_planar_part_slides_without_turning_when_the_turn_is_held() {
        let dofs = [
            rot("angle", [0.0, 0.0, 1.0]),
            slide("slide_x", [1.0, 0.0, 0.0]),
            slide("slide_y", [0.0, 1.0, 0.0]),
        ];
        let at = [10.0, 0.0, 0.0];
        let (o, d) = ray_to([25.0, 8.0, 0.0], [0.0, 0.0, -1.0]);
        let lim = [(None, None); 3];
        let held = follow(&dofs, &[0.0; 3], &[false, true, true], &lim, at, o, d);
        assert_eq!(held[0], 0.0);
        assert!(
            (held[1] - 15.0).abs() < 1e-6 && (held[2] - 8.0).abs() < 1e-6,
            "{held:?}"
        );
        let spun = follow(&dofs, &[0.0; 3], &[true, true, true], &lim, at, o, d);
        assert!(spun[0].abs() > 0.05, "回転も動かすと回る: {spun:?}");
    }

    /// ボール: 傾き・ひねり・角度で、球の上の点をカーソルへ寄せる。下限・上限で止まる。
    #[test]
    fn a_ball_part_turns_toward_the_cursor_and_stops_at_its_limits() {
        let z = [0.0, 0.0, 1.0];
        let dofs = [
            rot("twist", z),
            rot("tilt", [1.0, 0.0, 0.0]),
            rot("angle", z),
        ];
        let at = [0.0, 0.0, 10.0];
        // 斜め上から見て、球の上の (0, -6, 8) へ
        let dir = k::normalize([0.3, 0.2, -1.0]);
        let (o, d) = ray_to([0.0, -6.0, 8.0], dir);
        let free = [true; 3];
        let v = follow(&dofs, &[0.0; 3], &free, &[(None, None); 3], at, o, d);
        let p = k::transform_point(&dofs_pose(&dofs, &v), at);
        assert!(far(p, [0.0, -6.0, 8.0]) < 1e-3, "{v:?} {p:?}");
        // 傾きの上限 10 度で止める
        let lim = [(None, None), (None, Some(10f64.to_radians())), (None, None)];
        let v = follow(&dofs, &[0.0; 3], &free, &lim, at, o, d);
        assert!(v[1] <= 10f64.to_radians() + 1e-12, "{v:?}");
        // 全部 0 の時の位置へ戻す計算は、動きの逆
        let q = dofs_unpose(&dofs, &v, k::transform_point(&dofs_pose(&dofs, &v), at));
        assert!(far(q, at) < 1e-9, "{q:?}");
    }

    /// ジョイントの動き: 今の値から段の値への差だけ回す。上下限で止める。既定の範囲。
    #[test]
    fn a_drive_turns_by_the_difference_from_its_value_and_stops_at_the_limits() {
        let d = Drive {
            slide: false,
            origin: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            value: 0.0,
            min: Some(-std::f64::consts::FRAC_PI_2),
            max: Some(0.0),
            bodies: vec![FeatureId(3)],
            dofs: Vec::new(),
            lead: None,
        };
        let m = d.delta(d.from_shown(-90.0));
        let p = k::transform_point(&m, [1.0, 0.0, 0.0]);
        assert!((p[0]).abs() < 1e-12 && (p[1] + 1.0).abs() < 1e-12, "{p:?}");
        // 上限を越える値は上限(今の値と同じ)で止まる
        assert_eq!(d.delta(d.from_shown(45.0)), k::IDENTITY);
        assert_eq!(d.default_range(0.0), (-90.0, 0.0));
        let free = Drive {
            min: None,
            max: None,
            slide: true,
            value: 5.0,
            ..d.clone()
        };
        assert_eq!(free.default_range(40.0), (5.0, 45.0));
        let back: Drive = match free.to_value() {
            Value::Custom { type_key, data } => {
                assert_eq!(type_key, DRIVE_TYPE);
                serde_json::from_value(data).unwrap()
            }
            v => panic!("{v:?}"),
        };
        assert_eq!(back, free);
    }

    /// まとめた分解: 動きは同時に進み、部品と中の部品の動きは足される。跡の線は元の位置から
    /// 段の区切りを通って今の位置まで。
    #[test]
    fn explode_many_moves_together_and_trails_follow_the_bodies() {
        let data = DocumentData {
            components: vec![
                Component {
                    id: 10,
                    name: "箱".into(),
                    parent: None,
                    bodies: vec![FeatureId(1)],
                },
                Component {
                    id: 11,
                    name: "蓋".into(),
                    parent: Some(10),
                    bodies: vec![FeatureId(2)],
                },
            ],
            ..Default::default()
        };
        let many = Step {
            trail: true,
            ..Step::new(
                StepKind::ExplodeMany {
                    moves: vec![
                        Move {
                            targets: vec![Target::Component { id: 10 }],
                            offset: [10.0, 0.0, 0.0],
                        },
                        Move {
                            targets: vec![Target::Component { id: 11 }],
                            offset: [0.0, 0.0, 20.0],
                        },
                        Move {
                            targets: vec![Target::Body { root: FeatureId(3) }],
                            offset: [0.0, -5.0, 0.0],
                        },
                    ],
                },
                1.0,
            )
        };
        assert!(check_step(&many).is_ok());
        let reg = Registry::new();
        let report = RecomputeReport::default();
        let steps = vec![
            many.clone(),
            explode(
                vec![Target::Body { root: FeatureId(3) }],
                [0.0, 0.0, 4.0],
                1.0,
            ),
        ];
        let p = poses(&data, &reg, &report, &steps, 0.5);
        // 同時に半分ずつ
        assert!((p[&FeatureId(1)][0][3] - 5.0).abs() < 1e-9);
        assert!((p[&FeatureId(3)][1][3] + 2.5).abs() < 1e-9);
        let p = poses(&data, &reg, &report, &steps, 2.0);
        // 蓋は箱の動きと自分の動きの和
        assert_eq!(p[&FeatureId(2)][0][3], 10.0);
        assert_eq!(p[&FeatureId(2)][2][3], 20.0);
        // 跡の線: 印のある段で動く 3 つのボディ。3 は次の段(印なし)の動きも区切りを通って続く
        let lines = trails(&data, &reg, &report, &steps, 2.0, |_| Some([0.0; 3]));
        assert_eq!(lines.len(), 3);
        let l3 = lines.iter().find(|l| l[1] == [0.0, -5.0, 0.0]).unwrap();
        assert_eq!(l3, &vec![[0.0; 3], [0.0, -5.0, 0.0], [0.0, -5.0, 4.0]]);
        // 初めは線が無い。印が無ければ線は無い
        assert!(trails(&data, &reg, &report, &steps, 0.0, |_| Some([0.0; 3])).is_empty());
        let plain = vec![Step {
            trail: false,
            ..many.clone()
        }];
        assert!(trails(&data, &reg, &report, &plain, 1.0, |_| Some([0.0; 3])).is_empty());
        // 使えない段
        for bad in [
            StepKind::ExplodeMany { moves: vec![] },
            StepKind::ExplodeMany {
                moves: vec![Move {
                    targets: vec![],
                    offset: [0.0; 3],
                }],
            },
            StepKind::ExplodeMany {
                moves: vec![Move {
                    targets: vec![Target::Body { root: FeatureId(3) }],
                    offset: [f64::NAN, 0.0, 0.0],
                }],
            },
        ] {
            assert!(check_step(&Step::new(bad, 1.0)).is_err());
        }
        // 跡の線の印は保存の形に入り、無ければ書かない
        let json = serde_json::to_string(&many).unwrap();
        assert!(
            json.contains("\"trail\":true") && json.contains("explode_many"),
            "{json}"
        );
        assert!(!serde_json::to_string(&steps[1]).unwrap().contains("trail"));
        let back: Step = serde_json::from_str(&json).unwrap();
        assert_eq!(back, many);
    }

    /// 段は文書の区画に入り、保存して読み直せ、取り消せる。使えない段は断る。
    #[test]
    fn steps_are_saved_undone_and_checked() {
        let mut doc = Document::new();
        let s = explode(
            vec![Target::Body { root: FeatureId(7) }],
            [0.0, 0.0, 5.0],
            1.25,
        );
        let op = set_steps_op(doc.data(), vec![s.clone()]).unwrap();
        doc.apply(Transaction::new("段", vec![op])).unwrap();
        let saved = doc.to_json();
        assert!(saved.contains("\"asm.animation\""), "{saved}");
        let back = Document::from_json(&saved).unwrap();
        assert_eq!(steps(back.data()), vec![s.clone()]);
        // 空の文書には書かない
        assert!(!Document::new().to_json().contains("animation"));
        for bad in [
            Step {
                seconds: 0.0,
                ..s.clone()
            },
            explode(vec![], [0.0; 3], 1.0),
            explode(
                vec![Target::Body { root: FeatureId(7) }],
                [f64::NAN, 0.0, 0.0],
                1.0,
            ),
        ] {
            assert!(set_steps_op(doc.data(), vec![bad]).is_err());
        }
        assert_eq!(steps(doc.data()).len(), 1);
        doc.undo();
        assert!(steps(doc.data()).is_empty());
    }

    /// 版 4 で保存した文書(上の階に段・構成表・材料・外観)を開くと、それぞれの区画の型で同じ中身が
    /// 読め、保存し直して開いても変わらない。
    #[test]
    fn a_version_4_document_reads_through_the_sections() {
        let v4 = r#"{"format_version": 4, "next_id": 9, "params": [], "features": [],
            "body_materials": {"3": "アルミ"},
            "body_appearances": {"3": "赤"},
            "appearances": {"faces": {"3": {"f3:end": "青"}}},
            "components": [{"id": 5, "name": "蓋", "bodies": [3], "appearance": "メープル"},
                           {"id": 6, "name": "箱", "bodies": [4]}],
            "animation": [{"kind": "explode", "targets": [{"kind": "body", "root": 3}],
                           "offset": [0, 0, 5], "seconds": 1.5}],
            "configurations": {"rows": [{"name": "小", "cells": []}], "active": "小"}}"#;
        let check = |doc: &Document| {
            let data = doc.data();
            let s = steps(data);
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].seconds, 1.5);
            assert!(s[0].is_explode());
            assert_eq!(crate::configuration::of(data).active.as_deref(), Some("小"));
            let mats = kmb_look::material::of(data);
            assert_eq!(mats.material_of(FeatureId(3)), "アルミ");
            assert_eq!(
                mats.material_of(FeatureId(4)),
                kmb_look::material::DEFAULT_MATERIAL
            );
            let looks = kmb_look::appearance::of(data);
            assert_eq!(looks.appearance_of(data, FeatureId(3)), Some("赤"));
            assert_eq!(looks.appearance_of(data, FeatureId(4)), None);
            assert_eq!(
                looks.components.get(&5).map(String::as_str),
                Some("メープル")
            );
            assert_eq!(looks.face(FeatureId(3), "f3:end"), Some("青"));
        };
        let doc = Document::from_json(v4).unwrap();
        check(&doc);
        check(&Document::from_json(&doc.to_json()).unwrap());
    }
}
