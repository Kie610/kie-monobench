//! 流れと空力の研究(機能)。ボディは機能で指すので、寸法を変えると同じボディで解き直す。
//! 評価は解く準備(ボディの網と読み切った条件)だけをして出力 `plan` を出し、解くのは「実行」の時。
//! 結果は強度と熱の研究と同じ形([`kmb_study::SimResult`])で出し、見せ方(断面・流線・表面の圧力・
//! 時刻のコマ)を共有する。流れは時刻を進めて解き、決まった間隔の状態をコマとして残す。

use crate::flow::{self, FlowSpec, Watch};
use crate::geom::{Frame, Shape};
use crate::lbm::is_solid;
use crate::vlm;
use crate::wing;
use kmb_core::{
    Descriptor, Dim, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Mesh, Outputs,
    Payload, Ref, RefKind, Registry, Series, Value, category, expr_inputs,
};
use kmb_study::fields::{is_empty, length, number, rename_exprs};
use kmb_study::result::{Fnv, fmt};
use kmb_study::{
    Control, Frame as StudyFrame, Geometry, Grid, PLAN, Part, Plan, Quantity, Role, SimResult,
    Start, Task, Values,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

pub const WING: &str = "flow.wing@1";
pub const SECTION: &str = "flow.section@1";
pub const VOLUME: &str = "flow.volume@1";

/// 研究の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Wing,
    Section,
    Volume,
}

impl Kind {
    pub fn of(type_id: &str) -> Option<Kind> {
        Some(match type_id {
            WING => Kind::Wing,
            SECTION => Kind::Section,
            VOLUME => Kind::Volume,
            _ => return None,
        })
    }
}

/// 研究のデータ。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FlowData {
    /// 調べるボディ(翼なら主翼・尾翼など)
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub bodies: Vec<Ref>,
    /// 流れの向き(+x・-x・+y・-y・+z・-z)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dir: String,
    /// 速さ m/s
    #[serde(default)]
    pub speed: String,
    /// air・water
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fluid: String,
    /// 断面: xy(水平)・xz(正面)・yz(側面)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plane: String,
    /// 断面の位置(空ならボディの真ん中)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub at: String,
    /// 格子の大きさ(空なら自動)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub size: String,
    /// 迎え角(翼)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alpha: String,
    /// 重心の位置(流れの軸の座標、翼)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cg: String,
    /// 終わりの時刻(s、流れ)。空なら抗力が落ち着くまで
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub end: String,
    /// 残すコマの数(流れ)。空なら 50
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub frames: String,
}

impl FlowData {
    fn exprs(&self) -> [&String; 7] {
        [
            &self.speed,
            &self.at,
            &self.size,
            &self.alpha,
            &self.cg,
            &self.end,
            &self.frames,
        ]
    }

    fn exprs_mut(&mut self) -> [&mut String; 7] {
        [
            &mut self.speed,
            &mut self.at,
            &mut self.size,
            &mut self.alpha,
            &mut self.cg,
            &mut self.end,
            &mut self.frames,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct Study {
    pub kind: Kind,
    pub data: FlowData,
}

/// 角度(rad)。単位の無い値は deg。
fn angle(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if q.dim == Dim::NONE {
        return Ok(q.value.to_radians());
    }
    if q.dim != Dim::ANGLE {
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は角度にしてください(値: {q})"
        )));
    }
    Ok(q.value)
}

/// 流れの向き → (軸、符号)。
fn direction(s: &str) -> (usize, f64) {
    match s {
        "-x" => (0, -1.0),
        "+y" => (1, 1.0),
        "-y" => (1, -1.0),
        "+z" => (2, 1.0),
        "-z" => (2, -1.0),
        _ => (0, 1.0),
    }
}

const AXES: [&str; 3] = ["X", "Y", "Z"];

impl Study {
    fn bodies(&self) -> Result<Vec<FeatureId>, EvalError> {
        let mut out: Vec<FeatureId> = Vec::new();
        for r in &self.data.bodies {
            let f = r.feature();
            if !out.contains(&f) {
                out.push(f);
            }
        }
        if out.is_empty() {
            return Err(EvalError::InvalidData(match self.kind {
                Kind::Wing => "翼のボディを選んでください(主翼と尾翼を選ぶと両方で解く)".into(),
                _ => "流れの中に置くボディを選んでください".into(),
            }));
        }
        Ok(out)
    }

    /// 選んだボディの網(名前と一緒に)。
    fn meshes(&self, ctx: &EvalCtx<'_>) -> Result<Vec<(String, Mesh)>, EvalError> {
        let kernel = ctx.kernel();
        let mut out = Vec::new();
        let what = if self.kind == Kind::Wing {
            "翼"
        } else {
            "ボディ"
        };
        for (i, id) in self.bodies()?.into_iter().enumerate() {
            let h = match ctx.output(id, "body")? {
                Value::Shape(h) => *h,
                _ => return Err(EvalError::InvalidData("選んだボディがありません".into())),
            };
            let name = format!("{what} {}", i + 1);
            let mesh = kernel
                .mesh(h, 0.2)
                .map_err(|e| EvalError::Kernel(e.to_string()))?;
            out.push((name, mesh));
        }
        Ok(out)
    }

    fn speed(&self, ctx: &EvalCtx<'_>) -> Result<f64, EvalError> {
        let v = if is_empty(&self.data.speed) {
            10.0
        } else {
            number(ctx, &self.data.speed, "速さ")?
        };
        if !(v > 0.0 && v < 340.0 * 0.3) {
            return Err(EvalError::InvalidData(
                "速さは 0 より大きく、音速の 3 割(約 100 m/s)より小さくしてください".into(),
            ));
        }
        Ok(v)
    }
}

impl Feature for Study {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v: Vec<InputRef> = self
            .bodies()?
            .into_iter()
            .map(|feature| InputRef::Output {
                feature,
                slot: "body".into(),
            })
            .collect();
        for e in self.data.exprs() {
            if is_empty(e) {
                continue;
            }
            for i in expr_inputs(e)? {
                if !v.contains(&i) {
                    v.push(i);
                }
            }
        }
        Ok(v)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut d = self.data.clone();
        let changed = rename_exprs(d.exprs_mut(), old, new)?;
        Ok(changed.then(|| serde_json::to_value(&d).expect("研究のデータは JSON にできる")))
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let meshes = self.meshes(ctx)?;
        let bodies = self.bodies()?.into_iter().map(|f| f.0).collect();
        let speed = self.speed(ctx)?;
        let (axis, sign) = direction(if is_empty(&self.data.dir) {
            "+x"
        } else {
            &self.data.dir
        });
        let (solve, section) = match self.kind {
            Kind::Wing => {
                if axis == 2 {
                    return Err(EvalError::InvalidData(
                        "翼の流れの向きは X か Y にしてください(上は +Z)".into(),
                    ));
                }
                let alpha = if is_empty(&self.data.alpha) {
                    4f64.to_radians()
                } else {
                    angle(ctx, &self.data.alpha, "迎え角")?
                };
                if alpha.abs() > 20f64.to_radians() {
                    return Err(EvalError::InvalidData(
                        "迎え角は ±20° までにしてください(大きい角は失速して渦格子法の外です)"
                            .into(),
                    ));
                }
                let cg = if is_empty(&self.data.cg) {
                    None
                } else {
                    Some(length(ctx, &self.data.cg, "重心の位置")?)
                };
                (
                    Solve::Wing {
                        axis,
                        sign,
                        speed,
                        alpha,
                        cg,
                    },
                    None,
                )
            }
            _ => self.flow_solve(ctx, &meshes, axis, sign, speed)?,
        };
        let job = Job {
            kind: self.kind,
            meshes,
            bodies,
            solve,
        };
        let key = job.key();
        let plan = Plan {
            key,
            steps: job.steps(),
            section,
            keeps_partial: self.kind != Kind::Wing,
            ..Plan::default()
        };
        kmb_study::prepare(ctx, key, Arc::new(job));
        Ok([(PLAN.to_string(), plan.to_value())].into())
    }
}

impl Study {
    /// 流れの研究の条件と、3D の見せる断面(世界の軸と位置)。
    fn flow_solve(
        &self,
        ctx: &EvalCtx<'_>,
        meshes: &[(String, Mesh)],
        axis: usize,
        sign: f64,
        speed: f64,
    ) -> Result<(Solve, Option<(usize, f64)>), EvalError> {
        let three = self.kind == Kind::Volume;
        let plane = match self.data.plane.as_str() {
            "xz" => 1,
            "yz" => 0,
            _ => 2,
        };
        // 3D は解く向きを断面に関わらず決める(断面は見せ方なので、直しても解き直さない)
        let normal = if !three {
            plane
        } else if axis == 2 {
            1
        } else {
            2
        };
        if !three && axis == normal {
            return Err(EvalError::InvalidData(format!(
                "流れの向き {} は断面の中にしてください(断面の法線が {} です)",
                AXES[axis], AXES[normal]
            )));
        }
        let frame = Frame::new(axis, sign, normal);
        let ms: Vec<&Mesh> = meshes.iter().map(|(_, m)| m).collect();
        let shape = Shape::new(&ms, &frame)
            .ok_or_else(|| EvalError::InvalidData("ボディの網が空です".into()))?;
        // 断面の位置(世界の座標)。空ならボディの真ん中
        let mid = {
            let (lo, hi) = (frame.to_world(shape.lo), frame.to_world(shape.hi));
            0.5 * (lo[plane] + hi[plane])
        };
        let at = if is_empty(&self.data.at) {
            mid
        } else {
            length(ctx, &self.data.at, "断面の位置")?
        };
        let h = if is_empty(&self.data.size) {
            None
        } else {
            Some(length(ctx, &self.data.size, "格子の大きさ")?)
        };
        let frames = if is_empty(&self.data.frames) {
            FRAMES
        } else {
            let n = number(ctx, &self.data.frames, "コマの数")?.round();
            if !(2.0..=200.0).contains(&n) {
                return Err(EvalError::InvalidData(
                    "コマの数は 2 から 200 にしてください".into(),
                ));
            }
            n as usize
        };
        let end = if is_empty(&self.data.end) {
            None
        } else {
            let t = number(ctx, &self.data.end, "終わりの時刻")?;
            if t.is_nan() || t <= 0.0 {
                return Err(EvalError::InvalidData(
                    "終わりの時刻は 0 より大きくしてください".into(),
                ));
            }
            Some(t)
        };
        let spec = FlowSpec {
            dims: if three { 3 } else { 2 },
            // 2D の断面は流れの座標の z(法線の向きは +)
            plane_z: if three { 0.0 } else { at },
            h,
            speed,
            fluid: flow::fluid(&self.data.fluid),
            frames,
            end,
        };
        Ok((
            Solve::Flow {
                axis,
                sign,
                normal,
                spec,
            },
            three.then_some((plane, at)),
        ))
    }
}

/// 既定のコマの数。
const FRAMES: usize = 50;

/// 読み切った解く条件。
#[derive(Debug, Clone)]
enum Solve {
    Wing {
        axis: usize,
        sign: f64,
        speed: f64,
        alpha: f64,
        /// 重心の位置(世界の座標 mm)
        cg: Option<f64>,
    },
    Flow {
        axis: usize,
        sign: f64,
        normal: usize,
        spec: FlowSpec,
    },
}

/// 研究 1 つの解く仕事。
struct Job {
    kind: Kind,
    meshes: Vec<(String, Mesh)>,
    bodies: Vec<u64>,
    solve: Solve,
}

impl Job {
    fn key(&self) -> u64 {
        let mut h = Fnv::default();
        h.eat(format!("{:?}|{:?}", self.kind, self.solve).as_bytes());
        for (name, m) in &self.meshes {
            h.eat(name.as_bytes());
            h.eat_f32s(m.positions.iter().flatten().copied());
            for t in &m.triangles {
                for i in t {
                    h.eat(&i.to_le_bytes());
                }
            }
        }
        h.0
    }

    fn steps(&self) -> Vec<String> {
        match self.solve {
            Solve::Wing { .. } => vec!["翼を板に分ける".into(), "渦格子法で解く".into()],
            Solve::Flow { .. } => vec![
                "格子を作る".into(),
                "流れを解く".into(),
                "結果をまとめる".into(),
            ],
        }
    }
}

impl Task for Job {
    fn solve(&self, c: &Control) -> Result<SimResult, String> {
        let started = std::time::Instant::now();
        let mut r = match &self.solve {
            Solve::Wing {
                axis,
                sign,
                speed,
                alpha,
                cg,
            } => solve_wing(&self.meshes, *axis, *sign, *speed, *alpha, *cg, c)?,
            Solve::Flow {
                axis,
                sign,
                normal,
                spec,
            } => solve_flow(self.kind, &self.meshes, *axis, *sign, *normal, spec, c)?,
        };
        log::info!(
            "流れ({:?}): {:.2} 秒。{}",
            self.kind,
            started.elapsed().as_secs_f64(),
            r.summary
        );
        r.bodies = self.bodies.clone();
        Ok(r)
    }
}

pub const WING_NOTE: &str = "薄い翼の理論(渦格子法)の目安です。翼型の摩擦や形の抗力、失速(迎え角が大きい所)、胴体の影響は入りません。揚抗比は誘導抗力だけで出しています。";

pub const FLOW_NOTE: &str = "格子ボルツマン法の目安です。格子は立方体なので壁は階段状に近く、格子で表せないレイノルズ数は下げて解きます(結果の数値の「計算した Re」)。流れの形と、形を変えた時の大小の比べに使い、抗力の絶対値は ±数十% と考えてください。風は止まった所から滑らかに上げ、全部の速さになってからの時刻をコマに残します。圧力は入口のすぐ内側の平均との差です。";

fn solve_wing(
    meshes: &[(String, Mesh)],
    axis: usize,
    sign: f64,
    speed: f64,
    alpha: f64,
    cg: Option<f64>,
    c: &Control,
) -> Result<SimResult, String> {
    c.step(0);
    let frame = Frame::new(axis, sign, 2);
    let air = flow::AIR;
    let mut surfaces = Vec::new();
    for (name, m) in meshes {
        let shape = Shape::new(&[m], &frame).ok_or_else(|| format!("{name} の網が空です"))?;
        surfaces.push(wing::surface(&shape, name, 40, 6)?);
    }
    // 基準は面積のいちばん大きい翼(主翼)
    let main = (0..surfaces.len())
        .max_by(|a, b| {
            surfaces[*a]
                .area_and_mac()
                .0
                .total_cmp(&surfaces[*b].area_and_mac().0)
        })
        .ok_or("翼がありません")?;
    let (area, mac) = surfaces[main].area_and_mac();
    let span = surfaces[main].span();
    if area <= 0.0 {
        return Err("主翼の面積が 0 です".into());
    }
    c.step(1);
    c.progress(0.3, "");
    let sol = vlm::solve(&surfaces, alpha, speed, air.rho)?;
    c.progress(0.65, "");
    let sol2 = vlm::solve(&surfaces, alpha + 1f64.to_radians(), speed, air.rho)?;
    let q = 0.5 * air.rho * speed * speed;
    let cl = sol.lift / (q * area);
    let cdi = sol.induced_drag / (q * area);
    let ar = span * span / area;
    let e = if cdi > 1e-12 {
        cl * cl / (std::f64::consts::PI * ar * cdi)
    } else {
        f64::NAN
    };
    let cla = (sol2.lift - sol.lift) / (q * area) / 1f64.to_radians();
    let np = vlm::neutral_point(&sol, &sol2);
    let mut warnings = Vec::new();
    // 色: 板ごとの圧力差の係数(揚力の分布)と、帯ごとの揚力係数(翼幅の分布)
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    let mut dcp = Vec::new();
    let mut local = Vec::new();
    let mut strip_cl = std::collections::HashMap::new();
    for (p, f) in sol.panels.iter().zip(&sol.force) {
        *strip_cl.entry((p.surface, p.strip)).or_insert(0.0) += f[2];
    }
    for (p, f) in sol.panels.iter().zip(&sol.force) {
        let s = &surfaces[p.surface];
        let (a, b) = (&s.points[p.strip], &s.points[p.strip + 1]);
        let c = 0.5 * ((a[a.len() - 1][0] - a[0][0]) + (b[b.len() - 1][0] - b[0][0]));
        let dy = b[0][1] - a[0][1];
        let lc = strip_cl[&(p.surface, p.strip)] / (q * c * dy).max(1e-12);
        let d = (f[0] * p.normal[0] + f[1] * p.normal[1] + f[2] * p.normal[2])
            / (q * p.area).max(1e-12);
        let base = verts.len() as u32;
        for c in p.corners {
            let w = frame.to_world(c.map(|v| v * 1000.0));
            verts.push(w.map(|v| v as f32));
            dcp.push(d as f32);
            local.push(lc as f32);
        }
        tris.push([base, base + 1, base + 2]);
        tris.push([base, base + 2, base + 3]);
    }
    let world = |x: f64| -> f64 { x * 1000.0 * sign };
    let mut numbers = vec![
        ("揚力係数 CL".into(), fmt(cl)),
        (
            "揚力".into(),
            format!(
                "{} N(約 {} g を持ち上げる)",
                fmt(sol.lift),
                fmt(sol.lift / 9.80665 * 1000.0)
            ),
        ),
        ("誘導抗力係数".into(), fmt(cdi)),
        ("誘導抗力".into(), format!("{} N", fmt(sol.induced_drag))),
        (
            "揚抗比(誘導抗力だけ)".into(),
            if cdi > 1e-12 {
                fmt(cl / cdi)
            } else {
                "—".into()
            },
        ),
        ("揚力傾斜".into(), format!("{} /rad", fmt(cla))),
        ("翼の効率 e".into(), fmt(e)),
        (
            "主翼".into(),
            format!(
                "{}(面積 {} cm²・翼幅 {} mm・平均空力翼弦 {} mm・縦横比 {})",
                surfaces[main].name,
                fmt(area * 1e4),
                fmt(span * 1000.0),
                fmt(mac * 1000.0),
                fmt(ar)
            ),
        ),
    ];
    if let Some(x) = np {
        numbers.push((
            "中立点".into(),
            format!("{} = {} mm", AXES[axis], fmt(world(x))),
        ));
        if let Some(cg_world) = cg {
            let cg = cg_world * sign / 1000.0;
            let margin = (x - cg) / mac.max(1e-9) * 100.0;
            let cm = vlm::pitching_moment(&sol, cg) / (q * area * mac.max(1e-9));
            numbers.push((
                "静安定余裕".into(),
                format!(
                    "{:.0}%(重心が中立点より平均空力翼弦の {:.0}% 前。5〜15% が目安)",
                    margin, margin
                ),
            ));
            numbers.push(("重心まわりの縦揺れモーメント係数".into(), fmt(cm)));
            if margin < 0.0 {
                warnings.push(format!(
                    "重心が中立点より後ろです(静安定余裕 {margin:.0}%)。重心を前へ移すか、尾翼を大きくしてください"
                ));
            }
        }
    }
    let re = speed * mac / air.nu;
    numbers.push((
        "条件".into(),
        format!(
            "迎え角 {}°・{} m/s・空気(レイノルズ数 約 {})",
            fmt(alpha.to_degrees()),
            fmt(speed),
            fmt(re)
        ),
    ));
    let summary = format!(
        "CL {}・揚力 {} N・誘導抗力 {} N",
        fmt(cl),
        fmt(sol.lift),
        fmt(sol.induced_drag)
    );
    warnings.push(WING_NOTE.into());
    Ok(SimResult {
        kind: "wing".into(),
        parts: vec![Part {
            name: "翼".into(),
            geom: Geometry::Surface {
                points: verts,
                tris,
            },
        }],
        quantities: vec![
            Quantity::scalar("揚力の分布(圧力差の係数)", "", 0),
            Quantity::scalar("翼幅の揚力係数", "", 0),
        ],
        frames: vec![StudyFrame {
            label: String::new(),
            time: None,
            values: vec![Values::exact(dcp), Values::exact(local)],
        }],
        numbers,
        summary,
        note: warnings.join("\n"),
        badge: format!("渦格子法・板 {} 枚", sol.panels.len()),
        ..SimResult::default()
    })
}

/// 時刻の書き方(1 秒より短ければ ms)。
fn time_label(t: f64) -> String {
    if t < 1.0 {
        format!("{} ms", fmt(t * 1000.0))
    } else {
        format!("{} s", fmt(t))
    }
}

/// 3 桁ごとに区切った整数(レイノルズ数など)。
fn grouped(v: f64) -> String {
    let s = format!("{:.0}", v.abs());
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if v < 0.0 { format!("-{out}") } else { out }
}

/// 流れを解きながら、コマ・抗力の推移・進み具合を集める。
struct Collect<'a> {
    c: &'a Control,
    frame: Frame,
    three: bool,
    /// 動圧(Pa)
    q: f64,
    grid: Option<Grid>,
    /// ボディの表面の点(流れの座標)と、圧力を読む点(壁から 1 セル外)
    body: Vec<[f64; 3]>,
    probe: Vec<[f64; 3]>,
    frames: Vec<StudyFrame>,
    drag: Vec<[f64; 2]>,
    max_speed: f32,
}

impl Watch for Collect<'_> {
    fn cancelled(&self) -> bool {
        self.c.cancelled()
    }

    fn progress(&mut self, fraction: f64, time: f64) {
        self.c.progress(
            0.05 + 0.9 * fraction as f32,
            format!("t = {}", time_label(time)),
        );
    }

    fn drag(&mut self, time: f64, cd: f64) {
        self.drag.push([time, cd]);
        self.c.series(Series {
            name: "抗力係数(目安)".into(),
            x_name: "時刻(s)".into(),
            points: self.drag.clone(),
            note: "落ち着いたら解き終わる".into(),
        });
    }

    fn start(&mut self, n: [usize; 3], origin: [f64; 3], h: f64, cell: &[u8]) {
        self.c.step(1);
        let g = Grid {
            n,
            origin,
            h,
            axes: self.frame.axes,
            signs: self.frame.signs,
            solid: cell.iter().map(|c| is_solid(*c)).collect(),
            body: Some(0),
        };
        if self.three {
            // 表面の圧力を読む点: 三角形の法線の向きに 1 セル外(外が物体なら逆の向き)
            let fluid = |p: [f64; 3]| {
                let i = [0, 1, 2].map(|a| ((p[a] - origin[a]) / h).floor());
                (0..3).all(|a| i[a] >= 0.0 && (i[a] as usize) < n[a])
                    && !g.solid[g.index(i[0] as usize, i[1] as usize, i[2] as usize)]
            };
            self.probe = self
                .body
                .chunks(3)
                .flat_map(|t| {
                    let u = [0, 1, 2].map(|a| t[1][a] - t[0][a]);
                    let v = [0, 1, 2].map(|a| t[2][a] - t[0][a]);
                    let nn = [
                        u[1] * v[2] - u[2] * v[1],
                        u[2] * v[0] - u[0] * v[2],
                        u[0] * v[1] - u[1] * v[0],
                    ];
                    let l = (nn[0] * nn[0] + nn[1] * nn[1] + nn[2] * nn[2])
                        .sqrt()
                        .max(1e-12);
                    let nn = nn.map(|x| x / l * h);
                    t.iter()
                        .map(|p| {
                            let out = [0, 1, 2].map(|a| p[a] + nn[a]);
                            if fluid(out) {
                                out
                            } else {
                                [0, 1, 2].map(|a| p[a] - nn[a])
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
        }
        self.grid = Some(g);
    }

    fn frame(&mut self, time: f64, vel: &[[f32; 3]], pressure: &[f32]) {
        let Some(g) = &self.grid else { return };
        let flat: Vec<f32> = vel.iter().flatten().copied().collect();
        let p = Values::packed(pressure);
        let mut w = Vec::with_capacity(vel.len());
        let cell: Vec<u8> = g.solid.iter().map(|s| if *s { 1 } else { 0 }).collect();
        let mut top = 0.0f32;
        for k in 0..g.n[2] {
            for j in 0..g.n[1] {
                for i in 0..g.n[0] {
                    let x = g.index(i, j, k);
                    if g.solid[x] {
                        w.push(f32::NAN);
                        continue;
                    }
                    let u = vel[x];
                    top = top.max((u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt());
                    w.push(flow::vorticity(g.n, g.h, &cell, vel, i, j, k));
                }
            }
        }
        self.max_speed = top;
        let mut values = vec![Values::packed(&flat), p.clone(), Values::packed(&w)];
        if self.three {
            let sp: Vec<f32> = self
                .probe
                .iter()
                .map(|q| kmb_study::view::sample(g, &p, 1, *q).map_or(f32::NAN, |v| v[0]))
                .collect();
            let cp: Vec<f32> = sp.iter().map(|v| (*v as f64 / self.q) as f32).collect();
            values.insert(0, Values::packed(&cp));
            values.insert(0, Values::packed(&sp));
        }
        self.frames.push(StudyFrame {
            label: time_label(time),
            time: Some(time),
            values,
        });
    }
}

/// 三角形を、辺が `h` ほどになるまで 4 つずつに分ける(点は 3 つずつ並べて返す)。
/// 分けすぎないよう、全部で 40 万個ほどで止める。
fn refine(tris: &[[[f64; 3]; 3]], h: f64) -> Vec<[f64; 3]> {
    const LIMIT: usize = 400_000;
    let long = |t: &[[f64; 3]; 3]| {
        (0..3)
            .map(|i| {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
            })
            .fold(0.0, f64::max)
    };
    let mut cur: Vec<[[f64; 3]; 3]> = tris.to_vec();
    for _ in 0..8 {
        let split = cur.iter().filter(|t| long(t) > h).count();
        if split == 0 || cur.len() + 3 * split > LIMIT {
            break;
        }
        let mut next = Vec::with_capacity(cur.len() + 3 * split);
        for t in cur {
            if long(&t) <= h {
                next.push(t);
                continue;
            }
            let mid = |a: [f64; 3], b: [f64; 3]| [0, 1, 2].map(|k| 0.5 * (a[k] + b[k]));
            let (ab, bc, ca) = (mid(t[0], t[1]), mid(t[1], t[2]), mid(t[2], t[0]));
            next.push([t[0], ab, ca]);
            next.push([ab, t[1], bc]);
            next.push([ca, bc, t[2]]);
            next.push([ab, bc, ca]);
        }
        cur = next;
    }
    cur.into_iter().flatten().collect()
}

#[allow(clippy::too_many_arguments)]
fn solve_flow(
    kind: Kind,
    meshes: &[(String, Mesh)],
    axis: usize,
    sign: f64,
    normal: usize,
    spec: &FlowSpec,
    c: &Control,
) -> Result<SimResult, String> {
    c.step(0);
    let three = kind == Kind::Volume;
    let frame = Frame::new(axis, sign, normal);
    let ms: Vec<&Mesh> = meshes.iter().map(|(_, m)| m).collect();
    let shape = Shape::new(&ms, &frame).ok_or("ボディの網が空です")?;
    // 3D は表面の圧力を塗るので、三角形を格子ほどの細かさに分ける(大きな面の真ん中の値も出す)
    let body: Vec<[f64; 3]> = if three {
        let h = spec.h.unwrap_or_else(|| {
            let ext = [0, 1, 2].map(|a| shape.hi[a] - shape.lo[a]);
            ext[1].max(ext[2]) / 12.0
        });
        refine(&shape.tris, h)
    } else {
        shape.tris.iter().flatten().copied().collect()
    };
    let mut col = Collect {
        c,
        frame,
        three,
        q: 0.5 * spec.fluid.rho * spec.speed * spec.speed,
        grid: None,
        body,
        probe: Vec::new(),
        frames: Vec::new(),
        drag: Vec::new(),
        max_speed: 0.0,
    };
    let out = flow::run(&shape, spec, &mut col)?;
    c.step(2);
    let grid = col.grid.take().ok_or("格子を作れませんでした")?;
    let n = out.n;
    let axes = |a: usize| AXES[frame.axes[a]];
    let unit_f = if three {
        "N"
    } else {
        "N/m(奥行き 1 m あたり)"
    };
    let mut numbers = vec![
        (
            "抗力係数".into(),
            format!(
                "{}(流れの向き {}{}、正面の{} {})",
                fmt(out.coef[0]),
                if frame.signs[0] > 0.0 { "+" } else { "-" },
                axes(0),
                if three { "面積" } else { "幅" },
                if three {
                    format!("{} mm²", fmt(out.area))
                } else {
                    format!("{} mm", fmt(out.area))
                }
            ),
        ),
        ("抗力".into(), format!("{} {unit_f}", fmt(out.force[0]))),
        (
            format!("横の力({})", axes(1)),
            format!("{} {unit_f}(係数 {})", fmt(out.force[1]), fmt(out.coef[1])),
        ),
    ];
    if three {
        numbers.push((
            format!("縦の力({})", axes(2)),
            format!("{} N(係数 {})", fmt(out.force[2]), fmt(out.coef[2])),
        ));
    }
    numbers.push((
        "いちばん速い所".into(),
        format!("{} m/s", fmt(col.max_speed as f64)),
    ));
    numbers.push((
        "レイノルズ数".into(),
        format!(
            "実際 {}・計算した Re {}",
            fmt(out.re_real),
            fmt(out.re_calc)
        ),
    ));
    let grid_text = if three {
        format!("{}×{}×{}", n[0], n[1], n[2])
    } else {
        format!("{}×{}", n[0], n[1])
    };
    numbers.push((
        "格子".into(),
        format!("{grid_text}(1 辺 {} mm)、{} 刻み", fmt(out.h), out.steps),
    ));
    numbers.push((
        "時刻".into(),
        format!(
            "{} まで解いた(1 刻み {}、コマ {} 枚){}",
            time_label(out.time),
            time_label(out.dt),
            col.frames.len(),
            if out.cancelled {
                "。中止したので途中まで"
            } else {
                ""
            }
        ),
    ));
    numbers.push((
        "条件".into(),
        format!("{}・{} m/s", spec.fluid.name, fmt(spec.speed)),
    ));
    let mut note = Vec::new();
    if out.re_calc < out.re_real * 0.8 {
        note.push(format!(
            "レイノルズ数 {} は格子で表せないので {} で解きました(流れの形の目安。格子を小さくすると近づく)",
            fmt(out.re_real),
            fmt(out.re_calc)
        ));
    }
    note.push(FLOW_NOTE.into());
    let summary = format!(
        "抗力係数 {}・抗力 {} {}{}",
        fmt(out.coef[0]),
        fmt(out.force[0]),
        if three { "N" } else { "N/m" },
        if out.cancelled { "(途中まで)" } else { "" }
    );
    // 表面の網(世界の座標)。三角形ごとに点を持つ(3D の表面の圧力は三角形の外側で読む)
    let points: Vec<[f32; 3]> = col
        .body
        .iter()
        .map(|p| frame.to_world(*p).map(|v| v as f32))
        .collect();
    let tris = (0..points.len() as u32 / 3)
        .map(|t| [3 * t, 3 * t + 1, 3 * t + 2])
        .collect();
    let mut quantities = vec![
        Quantity::vector("速さ", "m/s", 1, Role::Velocity),
        Quantity::scalar("圧力(入口との差)", "Pa", 1),
        Quantity::scalar(
            if three {
                "渦の強さ"
            } else {
                "渦(回る速さ)"
            },
            "1/s",
            1,
        ),
    ];
    if three {
        quantities.insert(0, Quantity::scalar("圧力係数 Cp", "", 0));
        quantities.insert(0, Quantity::scalar("表面の圧力", "Pa", 0));
    }
    let series = Series {
        name: "抗力係数(目安)".into(),
        x_name: "時刻(s)".into(),
        points: col.drag.clone(),
        note: String::new(),
    };
    Ok(SimResult {
        kind: if three { "flow3d" } else { "flow2d" }.into(),
        parts: vec![
            Part {
                name: "ボディ".into(),
                geom: Geometry::Surface { points, tris },
            },
            Part {
                name: "格子".into(),
                geom: Geometry::Grid(grid),
            },
        ],
        quantities,
        frames: std::mem::take(&mut col.frames),
        numbers,
        summary,
        note: note.join("\n"),
        badge: format!(
            "{}・格子 {} mm・Re {}",
            if three { "3D" } else { "2D" },
            fmt(out.h),
            grouped(out.re_real)
        ),
        series: if series.points.is_empty() {
            Vec::new()
        } else {
            vec![series]
        },
        partial: out.cancelled,
        start: Start {
            quantity: 0,
            section: !three,
            streamlines: three,
        },
        ..SimResult::default()
    })
}

const DIRS: &[(&str, &str)] = &[
    ("+x", "+X"),
    ("-x", "-X"),
    ("+y", "+Y"),
    ("-y", "-Y"),
    ("+z", "+Z"),
    ("-z", "-Z"),
];

fn bodies_field(label: &str, help: &str) -> Field {
    Field::pick("bodies", label, &[RefKind::Body], 1, None).help(help)
}

fn speed_field() -> Field {
    Field::opt_expr("speed", "速さ(m/s)")
        .plain("m/s")
        .help("流れの速さ。空なら 10 m/s")
}

fn plane_field() -> Field {
    Field::choice(
        "plane",
        "断面",
        &[("xy", "水平(XY)"), ("xz", "正面(XZ)"), ("yz", "側面(YZ)")],
    )
    .help("流れを見る面。流れの向きはこの面の中にする")
}

fn end_field() -> Field {
    Field::opt_expr("end", "終わりの時刻(s)")
        .plain("s")
        .help("流れ始めからこの時刻まで解く。空なら抗力が落ち着くまで")
}

fn frames_field() -> Field {
    Field::opt_expr("frames", "コマの数")
        .unitless()
        .help("時刻の帯で送って見る状態の数(終わりまでを等しい間隔で)。空なら 50。多いと記憶を使う")
}

fn study(type_id: &str, label: &str) -> Descriptor {
    Descriptor::new(type_id, category::STUDY, label).menu("流れ")
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        study(WING, "翼の揚力")
            .icon("flow_wing")
            .order(11)
            .pinned()
            .hint("翼のボディ(主翼・尾翼)の揚力・誘導抗力・中立点を渦格子法で出す(上は +Z)")
            .aliases(
                "よく ようりょく ちゅうりつてん 翼 揚力 抗力 中立点 重心 ラジコン wing lift vlm",
            )
            .field(bodies_field(
                "翼のボディ",
                "主翼をクリック。尾翼も足すと一緒に解き、中立点に効く",
            ))
            .field(
                Field::choice("dir", "流れの向き", &DIRS[..4])
                    .help("空気が来る向き。前縁がこの向きの手前"),
            )
            .field(
                Field::opt_expr("alpha", "迎え角")
                    .angle()
                    .help("翼に当たる風の角度。空なら 4°"),
            )
            .field(speed_field())
            .field(
                Field::opt_expr("cg", "重心の位置(流れの軸の座標)")
                    .length()
                    .help("重心の X(流れが Y なら Y)の座標。入れると静安定余裕を出す"),
            )
            .template(json!({"dir": "+x", "alpha": "4", "speed": "10"})),
        |d| {
            Ok(Box::new(Study {
                kind: Kind::Wing,
                data: from(d)?,
            }))
        },
    );
    reg.register_described(
        study(SECTION, "2D の流れ")
            .icon("flow_2d")
            .order(12)
            .pinned()
            .hint("ボディを断面で切り、まわりの流れ(速さ・圧力・渦)と抗力を見る(格子ボルツマン法)")
            .aliases("りゅうたい ながれ だくと 流れ 流体 ダクト 断面 抗力 cfd flow 2d")
            .field(bodies_field("ボディ", "流れの中に置くボディをクリック"))
            .field(plane_field())
            .field(
                Field::opt_expr("at", "断面の位置")
                    .length()
                    .help("断面の法線の向きの座標。空ならボディの真ん中"),
            )
            .field(Field::choice("dir", "流れの向き", DIRS))
            .field(speed_field())
            .field(Field::choice(
                "fluid",
                "流体",
                &[("air", "空気"), ("water", "水")],
            ))
            .field(
                Field::opt_expr("size", "格子の大きさ")
                    .length()
                    .help("格子の 1 辺。空なら自動(約 6 万セル)"),
            )
            .field(end_field())
            .field(frames_field())
            .template(json!({"plane": "xy", "dir": "+x", "speed": "10", "fluid": "air"})),
        |d| {
            Ok(Box::new(Study {
                kind: Kind::Section,
                data: from(d)?,
            }))
        },
    );
    reg.register_described(
        study(VOLUME, "3D の流れ")
            .icon("flow_3d")
            .order(13)
            .pinned()
            .hint("ボディのまわりの 3 次元の流れと抗力を解き、表面の圧力と流線、断面の速さ・圧力・渦を見る(1 分ほどかかる)")
            .aliases("りゅうたい ながれ くうりき 流れ 流体 空力 抗力 3d cfd flow")
            .field(bodies_field("ボディ", "流れの中に置くボディをクリック"))
            .field(Field::choice("dir", "流れの向き", DIRS))
            .field(speed_field())
            .field(Field::choice("fluid", "流体", &[("air", "空気"), ("water", "水")]))
            .field(plane_field().help("断面を出す時に見る面(解くのは 3 次元。直しても解き直さない)"))
            .field(Field::opt_expr("at", "断面の位置").length().help("空ならボディの真ん中。直しても解き直さない"))
            .field(Field::opt_expr("size", "格子の大きさ").length().help("格子の 1 辺。空なら自動(約 22 万セル)"))
            .field(end_field())
            .field(frames_field())
            .template(json!({"plane": "xy", "dir": "+x", "speed": "10", "fluid": "air"})),
        |d| Ok(Box::new(Study { kind: Kind::Volume, data: from(d)? })),
    );
    for t in [WING, SECTION, VOLUME] {
        reg.mark(t, kmb_core::CONCURRENT);
    }
}

fn from(d: &serde_json::Value) -> Result<FlowData, EvalError> {
    serde_json::from_value(d.clone()).map_err(|e| EvalError::InvalidData(e.to_string()))
}
