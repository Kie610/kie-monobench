//! 研究「音の放射・散乱」(`wave.radiate@1`)。選んだボディのまわりの音を境界要素法で解く。
//!
//! - 振動する面(選んだ面が同じ速さ・同じ位相で法線の向きに動く)から外へ出る音と、平面波が
//!   ボディに当たって散る音を、周波数ごとに解く。
//! - 評価は解く準備(ボディの表示用の網と読み切った条件)だけをして出力 `plan` を出し、重い計算は
//!   「実行」の時に kmb-study が別のスレッドで回す。進み具合と中止は [`Control`] を解き方の
//!   [`Watch`] に渡して受ける。中止すると、解き終えた周波数までを結果に残す。
//! - 網は表示用の網を要素の大きさ(既定は一番高い周波数の波長の 1/6)に張り直して作る
//!   (kmb-fea の表面の張り直し。面の境を保つので、振動する面の三角形はそのまま分かる)。
//! - 結果は表面(音圧レベルと位相)と、受け取りの球(方向ごとの音圧レベル。指向性)の 2 つの網。
//!   受け取りの球はボディの横に並べて描く(向きはボディと同じ)。コマは周波数ごと。

use crate::{
    BemJob, BemMesh, BemOut, Boundary, Medium, Receivers, Solver, Source, Watch, icosphere,
    solve_bem,
};
use kmb_core::{
    Descriptor, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Mesh, Outputs, Payload,
    Ref, RefKind, Registry, Value, category,
};
use kmb_fea::surfmesh::{SizeRule, reference, remesh};
use kmb_study::fields::{is_empty, length, number, rename_exprs};
use kmb_study::result::{Fnv, fmt};
use kmb_study::{
    Control, Frame, Geometry, PLAN, Part, Plan, Quantity, SimResult, Start, Task, Values,
};
use num_complex::Complex64 as C;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// 研究の種類の id。
pub const RADIATE: &str = "wave.radiate@1";

/// 解く三角形の数の上限。自動の大きさがこれを超えるなら大きさを上げ、知らせる。
/// 指定の大きさで超えるなら解く前に止める(行列は約 N² × 8 バイト)。
const MAX_TRIS: usize = 6000;

/// 自動の大きさで少なくとも作る三角形の数(低い周波数だけでも表面の音圧の分布が見えるように)。
const MIN_TRIS: usize = 600;

/// 周波数の数の上限。
const MAX_FREQS: usize = 40;

/// 音圧レベルの基準(Pa、実効値)。
const P_REF: f64 = 20e-6;

/// 平面波の振幅(Pa)。実効値 1 Pa(94 dB)。
const PLANE_AMPLITUDE: f64 = std::f64::consts::SQRT_2;

/// 研究のデータ(欄の値。数は式の文字)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RadiateData {
    /// 音の場に置くボディ
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub bodies: Vec<Ref>,
    /// 振動する面
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub faces: Vec<Ref>,
    /// 振動の速さ(mm/s、振幅)。空なら 1 mm/s
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub velocity: String,
    /// 外から来る音: none・plane
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// 平面波の進む向き(+x・-x・+y・-y・+z・-z)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dir: String,
    /// 始めの周波数(Hz)。空なら 250
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub f_start: String,
    /// 終わりの周波数(Hz)。空なら 4000
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub f_end: String,
    /// 1 オクターブの点の数。空なら 3
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub per_octave: String,
    /// 受け取りの球の半径(mm)。空なら遠方
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub radius: String,
    /// 受け取りの方向の細かさ: coarse・normal・fine
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// 空気の温度(℃)。空なら 20
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub temperature: String,
    /// 要素の大きさ(mm)。空なら一番高い周波数の波長の 1/6
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub size: String,
}

impl RadiateData {
    fn exprs(&self) -> [&String; 7] {
        [
            &self.velocity,
            &self.f_start,
            &self.f_end,
            &self.per_octave,
            &self.radius,
            &self.temperature,
            &self.size,
        ]
    }

    fn exprs_mut(&mut self) -> [&mut String; 7] {
        [
            &mut self.velocity,
            &mut self.f_start,
            &mut self.f_end,
            &mut self.per_octave,
            &mut self.radius,
            &mut self.temperature,
            &mut self.size,
        ]
    }
}

/// 研究「音の放射・散乱」。
#[derive(Debug, Clone)]
pub struct Radiate {
    pub data: RadiateData,
}

/// 平面波の進む向き。
fn direction(s: &str) -> [f64; 3] {
    match s {
        "-x" => [-1.0, 0.0, 0.0],
        "+y" => [0.0, 1.0, 0.0],
        "-y" => [0.0, -1.0, 0.0],
        "+z" => [0.0, 0.0, 1.0],
        "-z" => [0.0, 0.0, -1.0],
        _ => [1.0, 0.0, 0.0],
    }
}

/// 始め・終わり・1 オクターブの点の数から、周波数の並び(Hz)。
pub fn frequencies(start: f64, end: f64, per_octave: f64) -> Result<Vec<f64>, String> {
    if !(start > 0.0 && start.is_finite()) {
        return Err("始めの周波数は 0 より大きくしてください".into());
    }
    if !(end >= start && end.is_finite()) {
        return Err("終わりの周波数は始めの周波数以上にしてください".into());
    }
    if !(1.0..=24.0).contains(&per_octave) {
        return Err("1 オクターブの点の数は 1 から 24 にしてください".into());
    }
    let n = ((end / start).log2() * per_octave + 1e-9).floor() as usize + 1;
    if n > MAX_FREQS {
        return Err(format!(
            "周波数が {n} 点になり多すぎます(上限 {MAX_FREQS})。範囲を狭めるか、1 オクターブの点の数を減らしてください"
        ));
    }
    Ok((0..n)
        .map(|i| start * 2f64.powf(i as f64 / per_octave))
        .collect())
}

/// 周波数の書き方(「250 Hz」「1.00 kHz」)。
pub fn hz_label(f: f64) -> String {
    if f >= 1000.0 {
        format!("{:.2} kHz", f / 1000.0)
    } else {
        format!("{f:.0} Hz")
    }
}

impl Radiate {
    fn bodies(&self) -> Result<Vec<FeatureId>, EvalError> {
        let mut out: Vec<FeatureId> = Vec::new();
        for r in &self.data.bodies {
            let f = r.feature();
            if !out.contains(&f) {
                out.push(f);
            }
        }
        if out.is_empty() {
            return Err(EvalError::InvalidData(
                "音の場に置くボディを選んでください".into(),
            ));
        }
        Ok(out)
    }

    /// 振動する面(ボディの番号と面の名前)。
    fn faces(&self, bodies: &[FeatureId]) -> Result<Vec<(usize, String)>, EvalError> {
        self.data
            .faces
            .iter()
            .map(|r| match r {
                Ref::Face { feature, face } => bodies
                    .iter()
                    .position(|b| b == feature)
                    .map(|i| (i, face.clone()))
                    .ok_or_else(|| {
                        EvalError::InvalidData(
                            "振動する面は、選んだボディの面にしてください".into(),
                        )
                    }),
                _ => Err(EvalError::InvalidData(
                    "振動する面には面を選んでください(辺・頂点・ボディは使えません)".into(),
                )),
            })
            .collect()
    }

    fn opt(
        &self,
        ctx: &EvalCtx<'_>,
        src: &str,
        what: &str,
        default: f64,
    ) -> Result<f64, EvalError> {
        if is_empty(src) {
            Ok(default)
        } else {
            number(ctx, src, what)
        }
    }

    fn conditions(&self, ctx: &EvalCtx<'_>) -> Result<Conditions, EvalError> {
        let bad = |m: String| EvalError::InvalidData(m);
        let velocity = self.opt(ctx, &self.data.velocity, "振動の速さ", 1.0)?;
        if !(velocity.is_finite() && velocity > 0.0) {
            return Err(bad("振動の速さは 0 より大きくしてください".into()));
        }
        let freqs = frequencies(
            self.opt(ctx, &self.data.f_start, "始めの周波数", 250.0)?,
            self.opt(ctx, &self.data.f_end, "終わりの周波数", 4000.0)?,
            self.opt(ctx, &self.data.per_octave, "1 オクターブの点の数", 3.0)?
                .round(),
        )
        .map_err(bad)?;
        let radius = if is_empty(&self.data.radius) {
            None
        } else {
            let r = length(ctx, &self.data.radius, "受け取りの球の半径")?;
            if !(r.is_finite() && r > 0.0) {
                return Err(bad("受け取りの球の半径は 0 より大きくしてください".into()));
            }
            Some(r)
        };
        let temperature = self.opt(ctx, &self.data.temperature, "温度", 20.0)?;
        if !(-50.0..=100.0).contains(&temperature) {
            return Err(bad("温度は −50 から 100 ℃ にしてください".into()));
        }
        let size = if is_empty(&self.data.size) {
            None
        } else {
            let h = length(ctx, &self.data.size, "要素の大きさ")?;
            if !(h.is_finite() && h > 0.0) {
                return Err(bad("要素の大きさは 0 より大きくしてください".into()));
            }
            Some(h)
        };
        let plane = (self.data.source == "plane").then(|| direction(&self.data.dir));
        if self.data.faces.is_empty() && plane.is_none() {
            return Err(bad(
                "振動する面を選ぶか、外から来る音を平面波にしてください".into(),
            ));
        }
        let subdivisions = match self.data.detail.as_str() {
            "coarse" => 2,
            "fine" => 4,
            _ => 3,
        };
        Ok(Conditions {
            velocity: velocity * 1e-3,
            plane,
            freqs,
            radius,
            temperature,
            size,
            subdivisions,
        })
    }
}

impl Feature for Radiate {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        kmb_study::fields::inputs(self.bodies()?, self.data.exprs())
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut d = self.data.clone();
        let changed = rename_exprs(d.exprs_mut(), old, new)?;
        Ok(changed.then(|| serde_json::to_value(&d).expect("研究のデータは JSON にできる")))
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let ids = self.bodies()?;
        let faces = self.faces(&ids)?;
        let conditions = self.conditions(ctx)?;
        let kernel = ctx.kernel();
        let mut meshes = Vec::new();
        for id in &ids {
            let h = match ctx.output(*id, "body")? {
                Value::Shape(h) => *h,
                _ => return Err(EvalError::InvalidData("選んだボディがありません".into())),
            };
            meshes.push(
                kernel
                    .mesh(h, 0.2)
                    .map_err(|e| EvalError::Kernel(e.to_string()))?,
            );
        }
        for (b, name) in &faces {
            if !meshes[*b].face_names.iter().any(|n| n == name) {
                return Err(EvalError::InvalidData(format!(
                    "振動する面 `{name}` がボディにありません(形が変わって面が無くなりました)"
                )));
            }
        }
        let job = RadiateJob {
            meshes,
            faces,
            bodies: ids.iter().map(|f| f.0).collect(),
            conditions,
        };
        let key = job.key();
        let plan = Plan {
            key,
            steps: STEPS.iter().map(|s| s.to_string()).collect(),
            section: None,
            keeps_partial: true,
        };
        kmb_study::prepare(ctx, key, Arc::new(job));
        Ok([(PLAN.to_string(), plan.to_value())].into())
    }
}

/// 計算の段。
const STEPS: [&str; 3] = ["網を整える", "境界要素法で解く", "結果をまとめる"];

/// 読み切った条件。
#[derive(Debug, Clone, PartialEq)]
pub struct Conditions {
    /// 振動の速さ(m/s、振幅)
    pub velocity: f64,
    /// 平面波の進む向き(無ければ放射だけ)
    pub plane: Option<[f64; 3]>,
    /// 周波数(Hz)
    pub freqs: Vec<f64>,
    /// 受け取りの球の半径(mm)。無ければ遠方
    pub radius: Option<f64>,
    /// 温度(℃)
    pub temperature: f64,
    /// 要素の大きさ(mm)。無ければ自動
    pub size: Option<f64>,
    /// 受け取りの方向の球の分け方の回数
    pub subdivisions: u32,
}

/// 研究 1 つの解く仕事。
pub struct RadiateJob {
    /// ボディごとの表示用の網(mm、面の名前つき)
    pub meshes: Vec<Mesh>,
    /// 振動する面(ボディの番号と面の名前)
    pub faces: Vec<(usize, String)>,
    /// ボディの機能の番号
    pub bodies: Vec<u64>,
    pub conditions: Conditions,
}

impl RadiateJob {
    fn key(&self) -> u64 {
        let mut h = Fnv::default();
        h.eat(format!("{:?}|{:?}", self.conditions, self.faces).as_bytes());
        for m in &self.meshes {
            h.eat_f32s(m.positions.iter().flatten().copied());
            for t in &m.triangles {
                for i in t {
                    h.eat(&i.to_le_bytes());
                }
            }
            for f in &m.triangle_faces {
                h.eat(&f.to_le_bytes());
            }
            for n in &m.face_names {
                h.eat(n.as_bytes());
            }
        }
        h.0
    }

    /// 要素の大きさ(mm)と、それを選んだ理由。
    fn element_size(&self, medium: &Medium) -> Result<(f64, String), String> {
        let fmax = self.conditions.freqs.iter().copied().fold(0.0, f64::max);
        let wavelength = medium.speed_of_sound / fmax * 1000.0;
        let area: f64 = self.meshes.iter().map(mesh_area).sum();
        // 正三角形に近い網の三角形の数の見込み
        let count = |h: f64| area / (0.433 * h * h);
        if let Some(h) = self.conditions.size {
            if count(h) > MAX_TRIS as f64 * 1.2 {
                return Err(format!(
                    "要素の大きさ {} mm では三角形が約 {:.0} 個になり多すぎます(上限 {MAX_TRIS})。大きくしてください",
                    fmt(h),
                    count(h)
                ));
            }
            return Ok((h, format!("指定の {} mm", fmt(h))));
        }
        let h = wavelength / 6.0;
        let floor = (area / (0.433 * MAX_TRIS as f64)).sqrt();
        let fine = (area / (0.433 * MIN_TRIS as f64)).sqrt();
        if h > fine {
            Ok((
                fine,
                format!(
                    "{} mm(三角形を {MIN_TRIS} 個ほどにした。{} の波長の 1/6 は {} mm)",
                    fmt(fine),
                    hz_label(fmax),
                    fmt(h)
                ),
            ))
        } else if h < floor {
            Ok((
                floor,
                format!(
                    "{} mm(三角形の数の上限 {MAX_TRIS} に合わせた。{} の波長の 1/6 は {} mm)",
                    fmt(floor),
                    hz_label(fmax),
                    fmt(h)
                ),
            ))
        } else {
            Ok((h, format!("{} mm({} の波長の 1/6)", fmt(h), hz_label(fmax))))
        }
    }

    /// 表示用の網を張り直して、解く網(m、中心を原点へ)を作る。
    fn surface(&self, h: f64, c: &Control) -> Result<Surface, String> {
        let rule = SizeRule {
            min_share: 0.25,
            ..SizeRule::default()
        };
        let mut points: Vec<[f64; 3]> = Vec::new();
        let mut tris: Vec<[u32; 3]> = Vec::new();
        let mut moving: Vec<bool> = Vec::new();
        for (b, m) in self.meshes.iter().enumerate() {
            let (r, s0) = reference(m, h, rule)?;
            let s = remesh(&r, s0, &|| c.cancelled())?;
            if c.cancelled() {
                return Err(CANCELLED.into());
            }
            let faces: Vec<u32> = self
                .faces
                .iter()
                .filter(|(i, _)| *i == b)
                .filter_map(|(_, n)| m.face_names.iter().position(|x| x == n))
                .map(|i| i as u32)
                .collect();
            let base = points.len() as u32;
            points.extend(s.points.iter().copied());
            tris.extend(s.tris.iter().map(|t| t.map(|i| i + base)));
            moving.extend(s.label.iter().map(|l| faces.contains(l)));
            c.progress(
                0.08 * (b + 1) as f32 / self.meshes.len() as f32,
                format!("三角形 {}", tris.len()),
            );
        }
        if tris.len() > MAX_TRIS * 3 / 2 {
            return Err(format!(
                "張り直した網の三角形が {} 個になり多すぎます(上限 {MAX_TRIS})。要素の大きさを大きくしてください",
                tris.len()
            ));
        }
        let (lo, hi) = bounds(&points);
        let center = [0, 1, 2].map(|a| 0.5 * (lo[a] + hi[a]));
        Ok(Surface {
            points,
            tris,
            moving,
            center,
            half: 0.5 * dist(lo, hi),
        })
    }

    fn bem_job(&self, s: &Surface, medium: Medium) -> BemJob {
        let to_m = |p: &[f64; 3]| [0, 1, 2].map(|a| (p[a] - s.center[a]) * 1e-3);
        let directions = icosphere(1.0, self.conditions.subdivisions).vertices;
        BemJob {
            mesh: BemMesh {
                vertices: s.points.iter().map(to_m).collect(),
                triangles: s.tris.clone(),
                groups: s.moving.iter().map(|m| *m as u32).collect(),
            },
            boundaries: vec![
                Boundary::Rigid,
                Boundary::Velocity {
                    velocity: C::new(self.conditions.velocity, 0.0),
                },
            ],
            sources: self
                .conditions
                .plane
                .map(|d| Source::PlaneWave {
                    direction: d,
                    amplitude: C::new(PLANE_AMPLITUDE, 0.0),
                })
                .into_iter()
                .collect(),
            frequencies: self.conditions.freqs.clone(),
            receivers: Receivers::Directions {
                directions,
                distance: self.conditions.radius.map(|r| r * 1e-3),
            },
            medium,
            solver: Solver::default(),
        }
    }
}

/// 中止の時の誤り(実行は中止を誤りにしない)。
const CANCELLED: &str = "中止しました";

/// 張り直した網(mm)。
struct Surface {
    points: Vec<[f64; 3]>,
    tris: Vec<[u32; 3]>,
    /// 三角形ごとの、振動する面か
    moving: Vec<bool>,
    center: [f64; 3],
    /// 外接の箱の対角の半分(mm)
    half: f64,
}

fn mesh_area(m: &Mesh) -> f64 {
    m.triangles
        .iter()
        .map(|t| {
            let p = t.map(|i| m.positions[i as usize].map(f64::from));
            0.5 * norm(cross(sub(p[1], p[0]), sub(p[2], p[0])))
        })
        .sum()
}

fn bounds(p: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for q in p {
        for a in 0..3 {
            lo[a] = lo[a].min(q[a]);
            hi[a] = hi[a].max(q[a]);
        }
    }
    (lo, hi)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(sub(a, b))
}

/// 振幅(Pa)の音圧レベル(dB、実効値で)。0 は数でない値(色を塗らない)。
fn level(p: f64) -> f32 {
    if p > 0.0 {
        (20.0 * (p / std::f64::consts::SQRT_2 / P_REF).log10()) as f32
    } else {
        f32::NAN
    }
}

/// 解き方の様子を研究の進み具合へ写す(段 1 の 0.1〜0.95)。
struct Progress<'a>(&'a Control);

impl Watch for Progress<'_> {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }

    fn progress(&self, fraction: f64, note: &str) {
        self.0.progress(0.1 + 0.85 * fraction as f32, note);
    }
}

impl Task for RadiateJob {
    fn solve(&self, c: &Control) -> Result<SimResult, String> {
        let started = std::time::Instant::now();
        c.step(0);
        let medium = Medium::from_temperature(self.conditions.temperature);
        let (h, why) = self.element_size(&medium)?;
        let s = self.surface(h, c)?;
        c.step(1);
        let job = self.bem_job(&s, medium);
        let out = solve_bem(&job, &Progress(c)).map_err(|e| e.message)?;
        if out.frequencies.is_empty() {
            return Err(CANCELLED.into());
        }
        c.step(2);
        let mut r = self.result(&s, &job, &out, &why);
        r.bodies = self.bodies.clone();
        log::info!(
            "音の放射・散乱: {:.2} 秒。{}",
            started.elapsed().as_secs_f64(),
            r.summary
        );
        Ok(r)
    }
}

impl RadiateJob {
    /// 解いた結果を研究の結果の形にする。
    fn result(&self, s: &Surface, job: &BemJob, out: &BemOut, why: &str) -> SimResult {
        let far = self.conditions.radius.is_none();
        let dirs = match &job.receivers {
            Receivers::Directions { directions, .. } => directions.clone(),
            _ => Vec::new(),
        };
        let balloon = icosphere(1.0, self.conditions.subdivisions);
        // 受け取りの球はボディの横に、ボディと同じくらいの大きさで描く
        let rd = (0.5 * s.half).max(1.0);
        let at = [s.center[0] + s.half + 1.2 * rd, s.center[1], s.center[2]];
        let ball_points: Vec<[f32; 3]> = balloon
            .vertices
            .iter()
            .map(|v| [0, 1, 2].map(|a| (at[a] + rd * v[a]) as f32))
            .collect();
        let axis = self.axis(s);
        let mut frames = Vec::new();
        let mut numbers = Vec::new();
        let mut coarse = Vec::new();
        for f in &out.frequencies {
            let (lv, ph) = vertex_values(s, &f.surface);
            let dir_level: Vec<f32> = f.receivers.iter().map(|p| level(p.norm())).collect();
            frames.push(Frame {
                label: hz_label(f.frequency),
                time: None,
                values: vec![
                    Values::exact(lv),
                    Values::exact(ph),
                    Values::exact(dir_level.clone()),
                ],
            });
            numbers.push((hz_label(f.frequency), on_axis(&dirs, &dir_level, axis)));
            if f.elements_per_wavelength < 6.0 {
                coarse.push(hz_label(f.frequency));
            }
        }
        let iterations = out
            .frequencies
            .iter()
            .map(|f| f.iterations)
            .max()
            .unwrap_or(0);
        numbers.push((
            "網".into(),
            format!("三角形 {}・要素の大きさ {why}", s.tris.len()),
        ));
        numbers.push((
            "空気".into(),
            format!(
                "{} ℃・音速 {} m/s・密度 {} kg/m³",
                fmt(self.conditions.temperature),
                fmt(job.medium.speed_of_sound),
                fmt(job.medium.density)
            ),
        ));
        numbers.push((
            "条件".into(),
            [
                (!self.faces.is_empty()).then(|| {
                    format!(
                        "振動する面 {}・速さ {} mm/s",
                        self.faces.len(),
                        fmt(self.conditions.velocity * 1e3)
                    )
                }),
                self.conditions
                    .plane
                    .map(|_| "平面波 1 Pa(94 dB)".to_string()),
                Some(match self.conditions.radius {
                    Some(r) => format!("受け取り 半径 {} mm の球", fmt(r)),
                    None => "受け取り 遠方(1 m に換えた値)".into(),
                }),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("・"),
        ));
        numbers.push(("反復の最大".into(), format!("{iterations} 回")));
        let mut note = vec![NOTE.to_string()];
        if !coarse.is_empty() {
            note.push(format!(
                "{} は 1 波長に入る要素が 6 個より少なく、値が粗くなります。要素の大きさを小さくしてください",
                coarse.join("・")
            ));
        }
        if out.cancelled {
            note.push(format!(
                "中止したので、{} までの {} 点だけです",
                out.frequencies
                    .last()
                    .map(|f| hz_label(f.frequency))
                    .unwrap_or_default(),
                out.frequencies.len()
            ));
        }
        let first = out.frequencies.first().map(|f| f.frequency).unwrap_or(0.0);
        let last = out.frequencies.last().map(|f| f.frequency).unwrap_or(0.0);
        SimResult {
            kind: "radiate".into(),
            parts: vec![
                Part {
                    name: "表面".into(),
                    geom: Geometry::Surface {
                        points: s.points.iter().map(|p| p.map(|v| v as f32)).collect(),
                        tris: s.tris.clone(),
                    },
                },
                Part {
                    name: "受け取りの球".into(),
                    geom: Geometry::Surface {
                        points: ball_points,
                        tris: balloon.triangles,
                    },
                },
            ],
            quantities: vec![
                Quantity::scalar("表面の音圧レベル", "dB", 0),
                Quantity::scalar("表面の音圧の位相", "°", 0),
                Quantity::scalar(
                    if far {
                        "指向性(1 m の音圧レベル)"
                    } else {
                        "受け取りの球の音圧レベル"
                    },
                    "dB",
                    1,
                ),
            ],
            frames,
            numbers,
            summary: format!(
                "{}〜{}・{} 点・三角形 {}",
                hz_label(first),
                hz_label(last),
                out.frequencies.len(),
                s.tris.len()
            ),
            note: note.join("\n"),
            badge: format!("境界要素法・三角形 {}", s.tris.len()),
            partial: out.cancelled,
            start: Start {
                quantity: 2,
                ..Start::default()
            },
            ..SimResult::default()
        }
    }

    /// 軸の向き: 振動する面の法線の平均(放射)か、平面波の進む向き(散乱)。
    fn axis(&self, s: &Surface) -> [f64; 3] {
        let mut n = [0.0; 3];
        for (t, m) in s.tris.iter().zip(&s.moving) {
            if *m {
                let p = t.map(|i| s.points[i as usize]);
                let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                for a in 0..3 {
                    n[a] += c[a];
                }
            }
        }
        let l = norm(n);
        if l > 0.0 {
            return n.map(|v| v / l);
        }
        self.conditions.plane.unwrap_or([0.0, 0.0, 1.0])
    }
}

pub const NOTE: &str = "境界要素法(区分一定の三角形・Burton–Miller の結合)の値です。空気は止まっていて、壁は振動する面のほか動かず音を通しません。振動する面は全体が同じ速さ・同じ位相で動くとしています。受け取りの球は見やすいようにボディの横に描いています(向きはボディと同じ)。";

/// 軸の上の音圧レベルと、軸から最初に 6 dB 下がる角度。
fn on_axis(dirs: &[[f64; 3]], levels: &[f32], axis: [f64; 3]) -> String {
    let unit = |d: [f64; 3]| {
        let l = norm(d);
        d.map(|v| v / l)
    };
    let angle = |d: [f64; 3]| dot(unit(d), axis).clamp(-1.0, 1.0).acos().to_degrees();
    let Some(i) = (0..dirs.len().min(levels.len()))
        .min_by(|a, b| angle(dirs[*a]).total_cmp(&angle(dirs[*b])))
    else {
        return "—".into();
    };
    let on = levels[i];
    let drop = (0..dirs.len().min(levels.len()))
        .filter(|j| levels[*j] < on - 6.0)
        .map(|j| angle(dirs[j]))
        .fold(f64::INFINITY, f64::min);
    if drop.is_finite() {
        format!("軸の上 {:.1} dB・−6 dB の角度 {:.0}°", on, drop)
    } else {
        format!("軸の上 {on:.1} dB・どの向きも 6 dB 以内")
    }
}

/// 三角形ごとの音圧を点へ移す(面積で重みを付けた平均)。レベル(dB)と位相(°)。
fn vertex_values(s: &Surface, p: &[C]) -> (Vec<f32>, Vec<f32>) {
    let n = s.points.len();
    let mut mag = vec![0.0; n];
    let mut sum = vec![C::new(0.0, 0.0); n];
    let mut w = vec![0.0; n];
    for (t, v) in s.tris.iter().zip(p) {
        let q = t.map(|i| s.points[i as usize]);
        let a = 0.5 * norm(cross(sub(q[1], q[0]), sub(q[2], q[0])));
        for &i in t {
            mag[i as usize] += a * v.norm();
            sum[i as usize] += v * a;
            w[i as usize] += a;
        }
    }
    let lv = (0..n)
        .map(|i| {
            if w[i] > 0.0 {
                level(mag[i] / w[i])
            } else {
                f32::NAN
            }
        })
        .collect();
    let ph = (0..n)
        .map(|i| {
            if w[i] > 0.0 {
                sum[i].arg().to_degrees() as f32
            } else {
                f32::NAN
            }
        })
        .collect();
    (lv, ph)
}

const DIRS: &[(&str, &str)] = &[
    ("+x", "+X"),
    ("-x", "-X"),
    ("+y", "+Y"),
    ("-y", "-Y"),
    ("+z", "+Z"),
    ("-z", "-Z"),
];

/// 研究の種類を登録する。
pub fn register(reg: &mut Registry) {
    reg.register_described(
        Descriptor::new(RADIATE, category::STUDY, "音の放射・散乱")
            .menu("音")
            .icon("wave_radiate")
            .order(21)
            .pinned()
            .hint("振動する面から外へ出る音と、平面波がボディに当たって散る音を周波数ごとに解き、表面の音圧と指向性を見る(境界要素法)")
            .aliases("おと おんきょう ほうしゃ さんらん しこうせい 音 音響 放射 散乱 指向性 スピーカー 回折 バッフル 境界要素法 bem acoustics radiation scattering directivity")
            .field(
                Field::pick("bodies", "ボディ", &[RefKind::Body], 1, None)
                    .help("音の場に置くボディをクリック(閉じたボディ)"),
            )
            .field(
                Field::pick("faces", "振動する面", &[RefKind::Face], 0, None)
                    .help("法線の向きに同じ速さで振動する面(スピーカーの振動板など)。無ければ平面波の散乱だけ"),
            )
            .field(
                Field::opt_expr("velocity", "振動の速さ(mm/s)")
                    .plain("mm/s")
                    .help("振動する面の速さの振幅。空なら 1 mm/s"),
            )
            .field(Field::choice(
                "source",
                "外から来る音",
                &[("none", "なし"), ("plane", "平面波")],
            ).help("平面波は実効値 1 Pa(94 dB)で、ボディに当たって散る音を解く"))
            .field(Field::choice("dir", "平面波の向き", DIRS).help("平面波が進む向き"))
            .field(
                Field::opt_expr("f_start", "始めの周波数(Hz)")
                    .plain("Hz")
                    .help("空なら 250 Hz"),
            )
            .field(
                Field::opt_expr("f_end", "終わりの周波数(Hz)")
                    .plain("Hz")
                    .help("空なら 4000 Hz。高いほど網が細かくなり時間がかかる"),
            )
            .field(
                Field::opt_expr("per_octave", "1 オクターブの点の数")
                    .unitless()
                    .help("周波数を倍にする間に解く点の数。空なら 3"),
            )
            .field(
                Field::opt_expr("radius", "受け取りの球の半径")
                    .length()
                    .help("ボディの真ん中からこの距離の音圧を方向ごとに出す。空なら遠方(1 m に換えた値)"),
            )
            .field(Field::choice(
                "detail",
                "受け取りの細かさ",
                &[
                    ("coarse", "粗い(162 方向)"),
                    ("normal", "普通(642 方向)"),
                    ("fine", "細かい(2562 方向)"),
                ],
            ))
            .field(
                Field::opt_expr("temperature", "温度(℃)")
                    .plain("℃")
                    .help("空気の温度。音速と密度が決まる。空なら 20 ℃"),
            )
            .field(
                Field::opt_expr("size", "要素の大きさ")
                    .length()
                    .help("表面の三角形の 1 辺。空なら一番高い周波数の波長の 1/6 と、三角形が 600 個ほどになる大きさの小さい方"),
            )
            .template(json!({"source": "none", "dir": "+x", "detail": "normal"})),
        |d| {
            Ok(Box::new(Radiate {
                data: serde_json::from_value(d.clone())
                    .map_err(|e| EvalError::InvalidData(e.to_string()))?,
            }))
        },
    );
    reg.mark(RADIATE, kmb_core::CONCURRENT);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 半径 a(mm)の球の表示用の網(面は 1 つ "s")。
    fn sphere(a: f64, sub: u32) -> Mesh {
        let s = icosphere(a, sub);
        Mesh {
            positions: s.vertices.iter().map(|v| v.map(|x| x as f32)).collect(),
            normals: s
                .vertices
                .iter()
                .map(|v| v.map(|x| (x / a) as f32))
                .collect(),
            triangles: s.triangles.clone(),
            triangle_faces: vec![0; s.triangles.len()],
            face_names: vec!["s".into()],
            edges: Vec::new(),
        }
    }

    fn job(faces: Vec<(usize, String)>, plane: Option<[f64; 3]>, freqs: Vec<f64>) -> RadiateJob {
        RadiateJob {
            meshes: vec![sphere(87.5, 3)],
            faces,
            bodies: vec![1],
            conditions: Conditions {
                velocity: 1e-3,
                plane,
                freqs,
                radius: None,
                temperature: 20.0,
                size: Some(18.0),
                subdivisions: 2,
            },
        }
    }

    #[test]
    fn frequencies_follow_octaves() {
        let f = frequencies(250.0, 4000.0, 3.0).unwrap();
        assert_eq!(f.len(), 13);
        assert!((f[3] - 500.0).abs() < 1e-9 && (f[12] - 4000.0).abs() < 1e-6);
        assert_eq!(frequencies(1000.0, 1000.0, 3.0).unwrap(), vec![1000.0]);
        assert!(frequencies(0.0, 100.0, 3.0).is_err());
        assert!(frequencies(500.0, 100.0, 3.0).is_err());
        assert!(frequencies(10.0, 20000.0, 24.0).is_err(), "多すぎる");
        assert_eq!(hz_label(250.0), "250 Hz");
        assert_eq!(hz_label(1000.0), "1.00 kHz");
    }

    /// 脈動する球は向きによらず同じ音を出し、遠方の振幅は単極の解析解に合う。
    #[test]
    fn pulsating_sphere_radiates_evenly_and_matches_the_monopole() {
        let j = job(vec![(0, "s".into())], None, vec![500.0]);
        let r = j.solve(&Control::new()).expect("解ける");
        assert_eq!(r.kind, "radiate");
        assert_eq!(r.frames.len(), 1);
        assert!(!r.partial);
        assert_eq!(r.bodies, vec![1]);
        let lv: Vec<f32> = match &r.frames[0].values[2] {
            Values::F32(v) => v.to_vec(),
            _ => panic!("値の形"),
        };
        assert_eq!(lv.len(), 162);
        let (lo, hi) = lv
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        assert!(hi - lo < 0.05, "向きによる差 {} dB", hi - lo);
        // 単極: |F| = ρ c v k a² / √(1 + (ka)²)
        let m = Medium::from_temperature(20.0);
        let k = 2.0 * std::f64::consts::PI * 500.0 / m.speed_of_sound;
        let a = 0.0875;
        let f = m.density * m.speed_of_sound * 1e-3 * k * a * a / (1.0 + (k * a).powi(2)).sqrt();
        let exact = level(f);
        assert!(
            (lv[0] - exact).abs() < 0.2,
            "遠方 {} dB、解析解 {exact} dB",
            lv[0]
        );
        // 表面の値は点の数だけある
        assert_eq!(r.parts.len(), 2);
        assert_eq!(r.quantities[0].part, 0);
        assert_eq!(r.quantities[2].part, 1);
        let n0 = r.parts[0].geom.len();
        assert!(matches!(&r.frames[0].values[0], Values::F32(v) if v.len() == n0));
    }

    /// 一辺 a(mm)の立方体の表示用の網(面ごとに点を持つ。面は +x・-x・+y・-y・+z・-z)。
    fn cube(a: f32) -> Mesh {
        let mut m = Mesh::default();
        let h = a / 2.0;
        for (f, (axis, sign)) in [
            (0, 1.0),
            (0, -1.0),
            (1, 1.0),
            (1, -1.0),
            (2, 1.0),
            (2, -1.0),
        ]
        .into_iter()
        .enumerate()
        {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let base = m.positions.len() as u32;
            for (du, dv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let mut p = [0.0; 3];
                p[axis] = sign * h;
                p[u] = du * h;
                p[v] = dv * h;
                m.positions.push(p);
                let mut n = [0.0; 3];
                n[axis] = sign;
                m.normals.push(n);
            }
            let quad = if sign > 0.0 {
                [[0, 1, 2], [0, 2, 3]]
            } else {
                [[0, 2, 1], [0, 3, 2]]
            };
            for t in quad {
                m.triangles.push(t.map(|i| base + i));
                m.triangle_faces.push(f as u32);
            }
            m.face_names
                .push(["+x", "-x", "+y", "-y", "+z", "-z"][f].into());
        }
        m
    }

    /// 立方体の 1 面だけを振動させると、その面の向きの音が後ろより大きい。
    #[test]
    fn one_vibrating_face_of_a_box_sends_sound_forward() {
        let mut j = job(vec![(0, "+x".into())], None, vec![2000.0]);
        j.meshes = vec![cube(100.0)];
        let r = j.solve(&Control::new()).expect("解ける");
        let lv: Vec<f32> = match &r.frames[0].values[2] {
            Values::F32(v) => v.to_vec(),
            _ => panic!("値の形"),
        };
        let dirs = icosphere(1.0, 2).vertices;
        let toward = |d: [f64; 3]| {
            (0..dirs.len())
                .max_by(|a, b| dot(dirs[*a], d).total_cmp(&dot(dirs[*b], d)))
                .unwrap()
        };
        let (front, back) = (lv[toward([1.0, 0.0, 0.0])], lv[toward([-1.0, 0.0, 0.0])]);
        assert!(front > back + 3.0, "前 {front} dB、後ろ {back} dB");
        assert!(r.numbers[0].1.starts_with("軸の上"), "{:?}", r.numbers[0]);
    }

    /// 中止を頼まれていたら、解かずに中止で返す(実行は中止を誤りにしない)。
    #[test]
    fn cancel_stops_before_solving() {
        let j = job(Vec::new(), Some([1.0, 0.0, 0.0]), vec![300.0, 600.0]);
        let c = Control::new();
        c.cancel();
        assert_eq!(j.solve(&c).unwrap_err(), CANCELLED);
    }

    /// 途中で中止した結果は、解き終えた周波数までのコマを持ち、途中の印が付く。
    #[test]
    fn partial_result_keeps_solved_frequencies() {
        let j = job(Vec::new(), Some([1.0, 0.0, 0.0]), vec![300.0, 600.0]);
        let c = Control::new();
        let medium = Medium::from_temperature(20.0);
        let (h, why) = j.element_size(&medium).unwrap();
        let s = j.surface(h, &c).unwrap();
        let mut bem = j.bem_job(&s, medium);
        bem.frequencies.truncate(1);
        let mut out = solve_bem(&bem, &()).unwrap();
        out.cancelled = true;
        let r = j.result(&s, &bem, &out, &why);
        assert!(r.partial);
        assert_eq!(r.frames.len(), 1);
        assert_eq!(r.frames[0].label, "300 Hz");
        assert!(r.note.contains("中止"));
        // 散乱だけなら軸は平面波の向き
        assert_eq!(j.axis(&s), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn too_fine_elements_stop_before_meshing() {
        let mut j = job(Vec::new(), Some([1.0, 0.0, 0.0]), vec![300.0]);
        j.conditions.size = Some(0.5);
        let e = j.solve(&Control::new()).unwrap_err();
        assert!(e.contains("多すぎます"), "{e}");
    }

    #[test]
    fn data_round_trips_through_json() {
        let d = RadiateData {
            bodies: vec![Ref::Body(FeatureId(3))],
            source: "plane".into(),
            dir: "-y".into(),
            f_end: "2000".into(),
            ..RadiateData::default()
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(serde_json::from_value::<RadiateData>(v).unwrap(), d);
    }
}
