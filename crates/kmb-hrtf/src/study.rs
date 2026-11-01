//! 研究「頭の HRTF」(`hrtf.compute@1`)。選んだ頭のボディと左右の耳の入口の点から、全方向の
//! HRTF を境界要素法で計算する([`crate::compute`] の画面の入口)。
//!
//! - 評価は頭の表示用の網と読み切った条件をそろえて出力 `plan` を出すだけにし、重い計算は「実行」の
//!   時に kmb-study が別のスレッドで回す。進み具合と中止は [`Control`] を解き方の [`Watch`] に渡す。
//!   中止すると、両耳とも解き終えた周波数までを結果に残す(HRIR にはしない)。
//! - 頭の向きは、左の耳 − 右の耳を左(SOFA の y)、欄の「前の向き」をそれに直交させた物を前(x)、
//!   その外積を上(z)とする。頭の中心は両耳の入口の真ん中。
//! - 網は耳からの距離で作り直し(既定は耳ごとに、耳の近く 1 mm・遠く 15 mm)、作り直した網を
//!   辺の長さの色で見せる。
//! - 結果は方向の球(頭の横に描く。向きは頭と同じ)の上の、周波数ごとの左・右の大きさと両耳の
//!   音量差、全部の周波数が解けた時は HRIR の立ち上がりから測った両耳の時間差。コマは周波数。

use std::collections::HashMap;
use std::sync::Arc;

use kmb_core::{
    Descriptor, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Mesh, Outputs, Payload,
    Ref, RefKind, Registry, Value, category,
};
use kmb_study::fields::{is_empty, length, number, rename_exprs};
use kmb_study::result::{Fnv, fmt};
use kmb_study::{
    Control, Frame, Geometry, PLAN, Part, Plan, Quantity, SimResult, Start, Task, Values,
};
use kmb_wave::{BemMesh, Medium, Watch, icosphere};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::compute::{HeadJob, Transfer, check, head_meshes, to_hrir, transfer_on};
use crate::dsp::OnsetFinder;
use crate::grid::Nearest;
use crate::head::Grading;
use crate::{Directions, HrtfError};

/// 研究の種類の id。
pub const COMPUTE: &str = "hrtf.compute@1";

/// 解く網の三角形の数の上限(耳 1 つ分)。行列はおよそ N² × 16 バイト(16000 で約 4 GB)。
const MAX_TRIS: usize = 16_000;

/// 方向の球の分け方の回数(2562 点)。
const BALLOON: u32 = 4;

/// 研究のデータ(欄の値。数は式の文字)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeadData {
    /// 頭のボディ
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub bodies: Vec<Ref>,
    /// 左の耳の穴の入口(mm、世界の座標)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<[f64; 3]>,
    /// 右の耳の穴の入口(mm、世界の座標)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<[f64; 3]>,
    /// 顔の向き(+x・-x・+y・-y・+z・-z)。空なら +x
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub front: String,
    /// 網の作り直し: per_ear(耳ごと)・both(両耳で 1 つ)・none(そのまま)。空なら per_ear
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub regrade: String,
    /// 耳の近くの辺の長さ(mm)。空なら 1
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub near: String,
    /// いちばん大きい辺の長さ(mm)。空なら 15
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub far: String,
    /// 耳の入口として振動させる範囲の半径(mm)。空なら 2
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ear_radius: String,
    /// サンプリング周波数(48000・44100)。空なら 48000
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rate: String,
    /// HRIR の長さ(128・256・512)。空なら 256
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub taps: String,
    /// 解く周波数の上限(Hz)。空なら 20000
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub max_hz: String,
    /// 方向の格子: gl(89 × 180)・r5(5° の輪)・r10(10° の輪)。空なら gl
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub directions: String,
    /// 音源までの距離(mm)。空なら遠方
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub distance: String,
    /// 空気の温度(℃)。空なら 20
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub temperature: String,
}

impl HeadData {
    fn exprs(&self) -> [&String; 6] {
        [
            &self.near,
            &self.far,
            &self.ear_radius,
            &self.max_hz,
            &self.distance,
            &self.temperature,
        ]
    }

    fn exprs_mut(&mut self) -> [&mut String; 6] {
        [
            &mut self.near,
            &mut self.far,
            &mut self.ear_radius,
            &mut self.max_hz,
            &mut self.distance,
            &mut self.temperature,
        ]
    }
}

/// 研究「頭の HRTF」。
#[derive(Debug, Clone)]
pub struct HeadStudy {
    pub data: HeadData,
}

/// 読み切った条件(長さは mm、世界の座標)。
#[derive(Debug, Clone, PartialEq)]
pub struct Conditions {
    pub left: [f64; 3],
    pub right: [f64; 3],
    /// 頭の前・左・上の向き(世界の単位ベクトル)
    pub axes: [[f64; 3]; 3],
    /// 作り直し(無ければそのまま)
    pub grading: Option<Grading>,
    pub ear_radius: f64,
    pub fs: f64,
    pub taps: usize,
    pub max_hz: f64,
    pub directions: Directions,
    pub distance: Option<f64>,
    pub temperature: f64,
}

/// 向きの文字(+x など)の単位ベクトル。
fn axis(s: &str) -> [f64; 3] {
    match s {
        "-x" => [-1.0, 0.0, 0.0],
        "+y" => [0.0, 1.0, 0.0],
        "-y" => [0.0, -1.0, 0.0],
        "+z" => [0.0, 0.0, 1.0],
        "-z" => [0.0, 0.0, -1.0],
        _ => [1.0, 0.0, 0.0],
    }
}

impl HeadStudy {
    fn body(&self) -> Result<FeatureId, EvalError> {
        self.data
            .bodies
            .first()
            .map(Ref::feature)
            .ok_or_else(|| EvalError::InvalidData("頭のボディを選んでください".into()))
    }

    fn opt(
        &self,
        ctx: &EvalCtx<'_>,
        src: &str,
        what: &str,
        default: f64,
        len: bool,
    ) -> Result<f64, EvalError> {
        if is_empty(src) {
            Ok(default)
        } else if len {
            length(ctx, src, what)
        } else {
            number(ctx, src, what)
        }
    }

    fn conditions(&self, ctx: &EvalCtx<'_>) -> Result<Conditions, EvalError> {
        let bad = |m: String| EvalError::InvalidData(m);
        let d = &self.data;
        let (Some(left), Some(right)) = (d.left, d.right) else {
            return Err(bad("左と右の耳の入口の点を入れてください".into()));
        };
        let axes = head_axes(left, right, axis(&d.front)).map_err(bad)?;
        let near = self.opt(ctx, &d.near, "耳の近くの辺の長さ", 1.0, true)?;
        let far = self.opt(ctx, &d.far, "遠くの辺の長さ", 15.0, true)?;
        if !(near.is_finite() && near > 0.0 && far.is_finite() && far >= near) {
            return Err(bad(
                "辺の長さは 0 より大きく、遠くを耳の近く以上にしてください".into(),
            ));
        }
        let grading = match d.regrade.as_str() {
            "none" => None,
            r => Some(Grading {
                near: near * 1e-3,
                far: far * 1e-3,
                grade: None,
                per_ear: r != "both",
            }),
        };
        let ear_radius = self.opt(ctx, &d.ear_radius, "耳の入口の半径", 2.0, true)?;
        if !(ear_radius.is_finite() && ear_radius >= 0.0) {
            return Err(bad("耳の入口の半径は 0 以上にしてください".into()));
        }
        let fs = match d.rate.as_str() {
            "44100" => 44_100.0,
            _ => 48_000.0,
        };
        let taps = match d.taps.as_str() {
            "128" => 128,
            "512" => 512,
            _ => 256,
        };
        let max_hz = self.opt(ctx, &d.max_hz, "周波数の上限", 20_000.0, false)?;
        let directions = match d.directions.as_str() {
            "r5" => Directions::Rings { step: 5.0 },
            "r10" => Directions::Rings { step: 10.0 },
            _ => Directions::default(), // Gauss–Legendre 89 × 180
        };
        let distance = if is_empty(&d.distance) {
            None
        } else {
            let r = length(ctx, &d.distance, "音源までの距離")?;
            if !(r.is_finite() && r > 0.0) {
                return Err(bad("音源までの距離は 0 より大きくしてください".into()));
            }
            Some(r)
        };
        let temperature = self.opt(ctx, &d.temperature, "温度", 20.0, false)?;
        Ok(Conditions {
            left,
            right,
            axes,
            grading,
            ear_radius,
            fs,
            taps,
            max_hz,
            directions,
            distance,
            temperature,
        })
    }
}

/// 両耳の入口と顔の向きから、頭の前・左・上の向き(世界の単位ベクトル)。
pub fn head_axes(
    left: [f64; 3],
    right: [f64; 3],
    front: [f64; 3],
) -> Result<[[f64; 3]; 3], String> {
    let span = sub(left, right);
    if norm(span) < 1.0 {
        return Err("左と右の耳の入口の点が近すぎます(1 mm 未満)".into());
    }
    let y = scale(span, 1.0 / norm(span));
    let x0 = sub(front, scale(y, dot(front, y)));
    if norm(x0) < 0.3 {
        return Err("顔の向きが、耳を結ぶ向きとほぼ同じです。別の向きを選んでください".into());
    }
    let x = scale(x0, 1.0 / norm(x0));
    Ok([x, y, cross(x, y)])
}

impl Feature for HeadStudy {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        kmb_study::fields::inputs(vec![self.body()?], self.data.exprs())
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut d = self.data.clone();
        let changed = rename_exprs(d.exprs_mut(), old, new)?;
        Ok(changed.then(|| serde_json::to_value(&d).expect("研究のデータは JSON にできる")))
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let id = self.body()?;
        let conditions = self.conditions(ctx)?;
        let h = match ctx.output(id, "body")? {
            Value::Shape(h) => *h,
            _ => return Err(EvalError::InvalidData("選んだボディがありません".into())),
        };
        let mesh = ctx
            .kernel()
            .mesh(h, 0.2)
            .map_err(|e| EvalError::Kernel(e.to_string()))?;
        let job = HeadTask {
            mesh,
            body: id.0,
            conditions,
        };
        job.head_job()
            .and_then(|j| check(&j))
            .map_err(|e| EvalError::InvalidData(e.message))?;
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
const STEPS: [&str; 3] = ["網を作り直す", "境界要素法で解く", "HRIR にしてまとめる"];

const CANCELLED: &str = "中止しました";

/// 研究 1 つの解く仕事。
pub struct HeadTask {
    /// 頭の表示用の網(mm、世界の座標)
    pub mesh: Mesh,
    /// 頭のボディの機能の番号
    pub body: u64,
    pub conditions: Conditions,
}

impl HeadTask {
    fn key(&self) -> u64 {
        let mut h = Fnv::default();
        h.eat(format!("{:?}", self.conditions).as_bytes());
        h.eat_f32s(self.mesh.positions.iter().flatten().copied());
        for t in &self.mesh.triangles {
            for i in t {
                h.eat(&i.to_le_bytes());
            }
        }
        h.0
    }

    /// 頭の中心(両耳の入口の真ん中、mm)。
    fn center(&self) -> [f64; 3] {
        scale(add(self.conditions.left, self.conditions.right), 0.5)
    }

    /// 世界の点(mm)を頭の座標(SOFA の向き、中心が原点、m)へ。
    fn to_head(&self, p: [f64; 3]) -> [f64; 3] {
        let d = sub(p, self.center());
        self.conditions.axes.map(|a| dot(a, d) * 1e-3)
    }

    /// 頭の座標(m)を世界の点(mm)へ。
    fn to_world(&self, p: [f64; 3]) -> [f64; 3] {
        let [x, y, z] = self.conditions.axes;
        let c = self.center();
        [0, 1, 2].map(|a| c[a] + 1e3 * (x[a] * p[0] + y[a] * p[1] + z[a] * p[2]))
    }

    /// 解き方の仕事(頭の座標の網。作り直しの前)。
    pub fn head_job(&self) -> Result<HeadJob, HrtfError> {
        let mesh = weld(&self.mesh, |p| self.to_head(p));
        if mesh.triangles.is_empty() {
            return Err(HrtfError::input("頭のボディの網が空です"));
        }
        let c = &self.conditions;
        Ok(HeadJob {
            mesh,
            center: [0.0; 3],
            ears: [self.to_head(c.left), self.to_head(c.right)],
            ear_radius: c.ear_radius * 1e-3,
            grading: c.grading.clone(),
            directions: c.directions.clone(),
            distance: c.distance.map(|r| r * 1e-3),
            fs: c.fs,
            taps: c.taps,
            max_hz: c.max_hz,
            lead: 24.0,
            medium: Medium::from_temperature(c.temperature),
            solver: Default::default(),
        })
    }
}

/// 表示用の網の同じ位置の頂点を 1 つにまとめ、`map` で座標を移した解き方の網にする。
fn weld(m: &Mesh, map: impl Fn([f64; 3]) -> [f64; 3]) -> BemMesh {
    let mut index: HashMap<[i64; 3], u32> = HashMap::new();
    let mut vertices = Vec::new();
    let mut remap = Vec::with_capacity(m.positions.len());
    for p in &m.positions {
        // 1 µm の升目でそろえる
        let k = p.map(|v| (f64::from(v) * 1e3).round() as i64);
        let i = *index.entry(k).or_insert_with(|| {
            vertices.push(map(p.map(f64::from)));
            (vertices.len() - 1) as u32
        });
        remap.push(i);
    }
    let triangles = m
        .triangles
        .iter()
        .map(|t| t.map(|i| remap[i as usize]))
        .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
        .collect();
    BemMesh {
        vertices,
        triangles,
        groups: Vec::new(),
    }
}

struct Progress<'a>(&'a Control);

impl Watch for Progress<'_> {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }

    fn progress(&self, fraction: f64, note: &str) {
        self.0.progress(0.1 + 0.85 * fraction as f32, note);
    }
}

impl Task for HeadTask {
    fn solve(&self, c: &Control) -> Result<SimResult, String> {
        let started = std::time::Instant::now();
        c.step(0);
        c.progress(0.0, "頭の網を作り直しています");
        let job = self.head_job().map_err(|e| e.message)?;
        let meshes = head_meshes(&job, &|| c.cancelled()).map_err(|e| {
            if c.cancelled() {
                CANCELLED.to_string()
            } else {
                e.message
            }
        })?;
        if c.cancelled() {
            return Err(CANCELLED.into());
        }
        for (m, side) in meshes.iter().zip(["左", "右"]) {
            if m.triangles.len() > MAX_TRIS {
                return Err(format!(
                    "{side}の耳の網の三角形が {} 個になり多すぎます(上限 {MAX_TRIS}。行列が約 {:.1} GB)。\
                     耳の近く・遠くの辺の長さを大きくしてください",
                    m.triangles.len(),
                    (m.triangles.len() as f64).powi(2) * 16.0 / 1e9
                ));
            }
        }
        c.step(1);
        let t = transfer_on(&job, &meshes, &Progress(c)).map_err(|e| e.message)?;
        if t.frequencies.is_empty() {
            return Err(CANCELLED.into());
        }
        c.step(2);
        let itd = if t.cancelled {
            None
        } else {
            Some(itd(&job, &t).map_err(|e| e.message)?)
        };
        let seconds = started.elapsed().as_secs_f64();
        let mut r = self.result(&job, &meshes, &t, itd.as_deref(), seconds);
        r.bodies = vec![self.body];
        log::info!("頭の HRTF: {seconds:.1} 秒。{}", r.summary);
        Ok(r)
    }
}

/// 方向ごとの両耳の時間差(左 − 右、µs)。HRIR の立ち上がりで測る。
fn itd(job: &HeadJob, t: &Transfer) -> Result<Vec<f64>, HrtfError> {
    let set = to_hrir(job, t)?;
    let finder = OnsetFinder::new(set.taps().next_power_of_two(), set.fs);
    Ok(set
        .ir
        .iter()
        .zip(&set.delay)
        .map(|(ir, d)| {
            let on = [0, 1].map(|e| finder.find(&ir[e]) + d[e]);
            (on[0] - on[1]) / set.fs * 1e6
        })
        .collect())
}

/// 頂点ごとの、付いている辺の長さの平均(mm)。
fn edge_lengths(m: &BemMesh) -> Vec<f32> {
    let mut sum = vec![0.0f64; m.vertices.len()];
    let mut count = vec![0u32; m.vertices.len()];
    for t in &m.triangles {
        for k in 0..3 {
            let (a, b) = (t[k] as usize, t[(k + 1) % 3] as usize);
            let l = norm(sub(m.vertices[a], m.vertices[b])) * 1e3;
            for i in [a, b] {
                sum[i] += l;
                count[i] += 1;
            }
        }
    }
    sum.iter()
        .zip(&count)
        .map(|(s, &n)| {
            if n == 0 {
                0.0
            } else {
                (s / f64::from(n)) as f32
            }
        })
        .collect()
}

fn db(v: kmb_wave::Complex64) -> f32 {
    (20.0 * v.norm().max(1e-12).log10()) as f32
}

/// 周波数の書き方(「188 Hz」「1.50 kHz」)。
fn hz_label(f: f64) -> String {
    if f >= 1000.0 {
        format!("{:.2} kHz", f / 1000.0)
    } else {
        format!("{f:.0} Hz")
    }
}

impl HeadTask {
    /// 解いた結果を研究の結果の形にする。
    fn result(
        &self,
        job: &HeadJob,
        meshes: &[BemMesh; 2],
        t: &Transfer,
        itd: Option<&[f64]>,
        seconds: f64,
    ) -> SimResult {
        let shared = meshes[0] == meshes[1];
        // 頭の大きさ(中心から一番遠い頂点、mm)
        let half = meshes[0]
            .vertices
            .iter()
            .map(|p| norm(*p) * 1e3)
            .fold(1.0, f64::max);
        let y = self.conditions.axes[1];
        let mut parts = Vec::new();
        let mut lengths = Vec::new();
        let mut put = |name: &str, m: &BemMesh, shift: f64| {
            let s = scale(y, shift);
            parts.push(Part {
                name: name.into(),
                geom: Geometry::Surface {
                    points: m
                        .vertices
                        .iter()
                        .map(|p| add(self.to_world(*p), s).map(|v| v as f32))
                        .collect(),
                    tris: m.triangles.clone(),
                },
            });
            lengths.push(edge_lengths(m));
        };
        if shared {
            put("解いた網", &meshes[0], 0.0);
        } else {
            put("左の耳の網", &meshes[0], 0.0);
            // 右の耳の網は頭の右へずらして並べる
            put("右の耳の網", &meshes[1], -2.4 * half);
        }
        // 方向の球は頭の左に並べる(向きは頭と同じ)
        let balloon = icosphere(1.0, BALLOON);
        let rd = 0.6 * half;
        let at = add(self.center(), scale(y, half + 1.4 * rd));
        let [ax, ay, az] = self.conditions.axes;
        let ball_points: Vec<[f32; 3]> = balloon
            .vertices
            .iter()
            .map(|v| {
                [0, 1, 2]
                    .map(|a| (at[a] + rd * (ax[a] * v[0] + ay[a] * v[1] + az[a] * v[2])) as f32)
            })
            .collect();
        let index = Nearest::new(&t.dirs);
        let pick: Vec<usize> = balloon
            .vertices
            .iter()
            .map(|v| {
                let az = v[1].atan2(v[0]).to_degrees();
                let el = v[2].clamp(-1.0, 1.0).asin().to_degrees();
                index.find(az, el).unwrap_or(0)
            })
            .collect();
        let ball = parts.len();
        parts.push(Part {
            name: "方向の球".into(),
            geom: Geometry::Surface {
                points: ball_points,
                tris: balloon.triangles,
            },
        });
        let mut quantities = vec![
            Quantity::scalar("左の耳の大きさ", "dB", ball),
            Quantity::scalar("右の耳の大きさ", "dB", ball),
            Quantity::scalar("両耳の音量差(左 − 右)", "dB", ball),
        ];
        if itd.is_some() {
            quantities.push(Quantity::scalar("両耳の時間差(左 − 右)", "µs", ball));
        }
        for (i, p) in parts.iter().enumerate().take(ball) {
            quantities.push(Quantity::scalar(&format!("辺の長さ({})", p.name), "mm", i));
        }
        let itd_values: Option<Vec<f32>> = itd.map(|v| pick.iter().map(|&d| v[d] as f32).collect());
        let mut frames = Vec::new();
        for (k, &f) in t.frequencies.iter().enumerate() {
            let l: Vec<f32> = pick.iter().map(|&d| db(t.h[0][k][d])).collect();
            let r: Vec<f32> = pick.iter().map(|&d| db(t.h[1][k][d])).collect();
            let ild: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
            let mut values = vec![Values::exact(l), Values::exact(r), Values::exact(ild)];
            if let Some(v) = &itd_values {
                values.push(Values::exact(v.clone()));
            }
            values.extend(lengths.iter().map(|v| Values::exact(v.clone())));
            frames.push(Frame {
                label: hz_label(f),
                time: None,
                values,
            });
        }
        let c = &self.conditions;
        let tris = if shared {
            format!("{}", meshes[0].triangles.len())
        } else {
            format!(
                "左の耳 {}・右の耳 {}",
                meshes[0].triangles.len(),
                meshes[1].triangles.len()
            )
        };
        let worst = t
            .notes
            .iter()
            .map(|n| n.elements_per_wavelength)
            .fold(f64::INFINITY, f64::min);
        let iterations = t.notes.iter().map(|n| n.iterations).max().unwrap_or(0);
        let first = t.frequencies[0];
        let last = t.frequencies[t.frequencies.len() - 1];
        let mut numbers = vec![
            (
                "網".into(),
                match &c.grading {
                    Some(g) => format!(
                        "三角形 {tris}・耳の近く {} mm・遠く {} mm",
                        fmt(g.near * 1e3),
                        fmt(g.far * 1e3)
                    ),
                    None => format!("三角形 {tris}(作り直していない)"),
                },
            ),
            (
                "耳の入口".into(),
                format!(
                    "振動させた三角形 左 {}・右 {}",
                    t.ear_triangles[0].len(),
                    t.ear_triangles[1].len()
                ),
            ),
            (
                "方向".into(),
                format!(
                    "{} 方向・{}",
                    t.dirs.len(),
                    match c.distance {
                        None => "遠方".to_string(),
                        Some(r) => format!("距離 {} mm", fmt(r)),
                    }
                ),
            ),
            (
                "周波数".into(),
                format!(
                    "{}〜{}・{} 点({} Hz 刻み)・{} tap・{} Hz",
                    hz_label(first),
                    hz_label(last),
                    t.frequencies.len(),
                    fmt(job.fs / job.taps as f64),
                    job.taps,
                    fmt(job.fs)
                ),
            ),
            (
                "空気".into(),
                format!(
                    "{} ℃・音速 {} m/s",
                    fmt(c.temperature),
                    fmt(job.medium.speed_of_sound)
                ),
            ),
            (
                "1 波長の要素".into(),
                format!("いちばん少ない所で {:.1} 個", worst),
            ),
            ("反復の最大".into(), format!("{iterations} 回")),
        ];
        if let Some(v) = itd {
            let max = v.iter().map(|x| x.abs()).fold(0.0, f64::max);
            numbers.push(("両耳の時間差の最大".into(), format!("{max:.0} µs")));
        }
        numbers.push(("かかった時間".into(), format!("{seconds:.1} 秒")));
        let mut note = vec![NOTE.to_string()];
        if worst < 6.0 {
            note.push(format!(
                "1 波長に入る要素が 6 個より少ない周波数があり({:.1} 個)、高い周波数の値が粗くなります。耳の近く・遠くの辺の長さを小さくしてください",
                worst
            ));
        }
        if t.cancelled {
            note.push(format!(
                "中止したので、{} までの {} 点だけです(両耳の時間差は全部の周波数が要るので出していません)",
                hz_label(last),
                t.frequencies.len()
            ));
        }
        SimResult {
            kind: "hrtf".into(),
            parts,
            quantities,
            frames,
            numbers,
            summary: format!(
                "{} 方向・{}〜{}・{} 点・三角形 {tris}",
                t.dirs.len(),
                hz_label(first),
                hz_label(last),
                t.frequencies.len()
            ),
            note: note.join("\n"),
            badge: format!("境界要素法・三角形 {tris}"),
            partial: t.cancelled,
            start: Start {
                quantity: 0,
                ..Start::default()
            },
            ..SimResult::default()
        }
    }
}

/// 結果の見方の注意。
pub const NOTE: &str = "境界要素法で、耳の入口を振動させて外へ出る音を方向ごとに読んだ値です(音の通り道は向きを逆にしても同じ)。大きさは、頭が無い時に頭の中心へ同じ音源を置いた時との比です。頭の表面は全部が剛く音を通さないとしています。方向の球は見やすいように頭の横に描いています(向きは頭と同じで、球の上の点がその方向から来た音の値)。";

const FRONTS: &[(&str, &str)] = &[
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
        Descriptor::new(COMPUTE, category::STUDY, "頭の HRTF")
            .menu("音")
            .icon("hrtf_compute")
            .order(22)
            .hint("頭のボディと左右の耳の入口から、全方向の頭部伝達関数(HRTF)を境界要素法で計算し、方向ごとの左右の大きさと両耳の差を見る")
            .aliases("hrtf とうぶでんたつかんすう でんたつかんすう みみ あたま 頭部伝達関数 頭 耳 両耳 立体音響 バイノーラル binaural head ear sofa")
            .field(
                Field::pick("bodies", "頭のボディ", &[RefKind::Body], 1, Some(1))
                    .help("頭(と肩)の閉じたボディをクリック。読み込んだ網のボディも使える"),
            )
            .field(
                Field::vector("left", "左の耳の入口")
                    .length()
                    .help("左の耳の穴の入口の点(X, Y, Z)。頭の表面の上に置く"),
            )
            .field(
                Field::vector("right", "右の耳の入口")
                    .length()
                    .help("右の耳の穴の入口の点(X, Y, Z)。頭の表面の上に置く"),
            )
            .field(
                Field::choice("front", "顔の向き", FRONTS)
                    .help("顔が向いている向き。耳を結ぶ向きに直交させて、前の向きにする"),
            )
            .field(
                Field::choice(
                    "regrade",
                    "網の作り直し",
                    &[
                        ("per_ear", "耳ごとに作る"),
                        ("both", "両耳で 1 つ"),
                        ("none", "そのまま"),
                    ],
                )
                .help("耳の近くだけ細かく、離れるほど粗く網を作り直す。耳ごとに作ると、解く耳の反対側は粗くなり速い"),
            )
            .field(
                Field::opt_expr("near", "耳の近くの辺の長さ")
                    .length()
                    .help("耳の入口での三角形の 1 辺。空なら 1 mm")
                    .unless("regrade", "none"),
            )
            .field(
                Field::opt_expr("far", "遠くの辺の長さ")
                    .length()
                    .help("いちばん大きい三角形の 1 辺。空なら 15 mm")
                    .unless("regrade", "none"),
            )
            .field(
                Field::opt_expr("ear_radius", "耳の入口の半径")
                    .length()
                    .help("入口として振動させる範囲。重心がこの中にある三角形を振動させる。空なら 2 mm"),
            )
            .field(Field::choice(
                "rate",
                "サンプリング周波数",
                &[("48000", "48 kHz"), ("44100", "44.1 kHz")],
            ))
            .field(
                Field::choice(
                    "taps",
                    "HRIR の長さ",
                    &[("128", "128 tap"), ("256", "256 tap"), ("512", "512 tap")],
                )
                .help("解く周波数の刻みはサンプリング周波数 ÷ 長さ(48 kHz・256 tap なら 187.5 Hz)"),
            )
            .field(
                Field::opt_expr("max_hz", "周波数の上限(Hz)")
                    .plain("Hz")
                    .help("ここまでを解き、その上はなだらかに下げる。空なら 20000 Hz。下げると速い"),
            )
            .field(Field::choice(
                "directions",
                "方向の格子",
                &[
                    ("gl", "89 × 180(16020 方向)"),
                    ("r5", "5° の輪(約 1650 方向)"),
                    ("r10", "10° の輪(約 410 方向)"),
                ],
            ))
            .field(
                Field::opt_expr("distance", "音源までの距離")
                    .length()
                    .help("頭の中心から音源までの距離。空なら遠方"),
            )
            .field(
                Field::opt_expr("temperature", "温度(℃)")
                    .plain("℃")
                    .help("空気の温度。音速と密度が決まる。空なら 20 ℃"),
            )
            .template(json!({
                "left": [0.0, 70.0, 0.0],
                "right": [0.0, -70.0, 0.0],
                "front": "+x",
                "regrade": "per_ear",
                "rate": "48000",
                "taps": "256",
                "directions": "gl"
            })),
        |d| {
            Ok(Box::new(HeadStudy {
                data: serde_json::from_value(d.clone())
                    .map_err(|e| EvalError::InvalidData(e.to_string()))?,
            }))
        },
    );
    reg.mark(COMPUTE, kmb_core::CONCURRENT);
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|v| v * s)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

#[cfg(test)]
mod tests;
