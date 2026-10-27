//! 解析の研究を機能として持つ。固定・荷重・温度などは面の名前で持つので、
//! 寸法を変えると同じ面に付き直り、再計算で解き直す。研究は形を作らず、ボディの形を読むだけ
//! (同時に計算してよい [`kmb_core::CONCURRENT`] の印を付ける)。
//!
//! 力・圧力・温度などの欄は単位の無い数の式で受け取り、単位は欄の名前に書く(式の単位は長さと
//! 角度だけのため)。

use crate::boundary::Prepared;
use crate::material::{FeaMaterial, anisotropy_note, density, fea_material};
use crate::modal::{ModalInput, solve_modal};
use crate::result::{fmt, geometry};
use crate::singular;
use crate::statics::{Dir, Load, LoadKind, StaticInput, solve_static};
use crate::tetmesh::{MAX_TETS, bounds, estimate_tets, expected_tets, size_for};
use crate::thermal::{HeatTime, ThermalInput, TransientInput, solve_thermal, solve_transient};
use crate::topology::{TopologyInput, solve_topology};
use kmb_core::{
    Descriptor, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Mesh, Outputs, Payload,
    Ref, RefKind, Registry, Value, add_warnings, category, expr_inputs,
};
use kmb_study::fields::{is_empty, length, number, rename_exprs};
use kmb_study::result::Fnv;
use kmb_study::{
    Control, Derive, Frame, Mark, PLAN, Part, Plan, Quantity, Role, SimResult, Task, Values,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

pub const STATIC: &str = "fea.static@1";
pub const THERMAL: &str = "fea.thermal@1";
pub const MODAL: &str = "fea.modal@1";
pub const TOPOLOGY: &str = "fea.topology@1";

/// 研究の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Static,
    Thermal,
    Modal,
    Topology,
}

impl Kind {
    pub fn of(type_id: &str) -> Option<Kind> {
        Some(match type_id {
            STATIC => Kind::Static,
            THERMAL => Kind::Thermal,
            MODAL => Kind::Modal,
            TOPOLOGY => Kind::Topology,
            _ => return None,
        })
    }

    fn key(self) -> &'static str {
        match self {
            Kind::Static => "static",
            Kind::Thermal => "thermal",
            Kind::Modal => "modal",
            Kind::Topology => "topology",
        }
    }

    /// 要素の大きさを自動で決める時の、要素の数の目安。
    fn target(self) -> usize {
        match self {
            Kind::Static => 20_000,
            Kind::Thermal => 30_000,
            Kind::Modal => 12_000,
            Kind::Topology => 8_000,
        }
    }
}

/// 荷重の 1 行。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoadRow {
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub faces: Vec<Ref>,
    /// force(力の合計 N)か pressure(圧力 MPa)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default)]
    pub value: String,
    /// normal(面を押す)・-z・+z・-x・+x・-y・+y
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dir: String,
}

/// 面と値の 1 行(温度・熱・熱伝達係数)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValueRow {
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub faces: Vec<Ref>,
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub until: String,
}

/// 研究のデータ(種類ごとに使う項目が違う)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StudyData {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub fixed: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loads: Vec<LoadRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub temps: Vec<ValueRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heats: Vec<ValueRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub convs: Vec<ValueRow>,
    /// 残りの外の面の熱伝達係数 W/(m²·K)(空なら逃げない)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rest_h: String,
    /// まわりの空気の温度 ℃
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ambient: String,
    /// 空なら定常、transient なら時間で変わる熱。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub time: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub end: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub frames: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub initial: String,
    /// 求める固有振動の数
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modes: String,
    /// 残す体積の割合 %
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub volume: String,
    /// 要素の大きさ(空なら自動)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub size: String,
    /// 断面: xy(水平)・xz(正面)・yz(側面)。見せ方なので解き直さない
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plane: String,
    /// 断面の位置(空ならボディの真ん中)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub at: String,
}

impl StudyData {
    fn all_refs(&self) -> impl Iterator<Item = &Ref> {
        self.fixed
            .iter()
            .chain(self.loads.iter().flat_map(|r| &r.faces))
            .chain(self.temps.iter().flat_map(|r| &r.faces))
            .chain(self.heats.iter().flat_map(|r| &r.faces))
            .chain(self.convs.iter().flat_map(|r| &r.faces))
    }

    fn exprs(&self) -> Vec<&String> {
        let mut v: Vec<&String> = vec![
            &self.rest_h,
            &self.ambient,
            &self.modes,
            &self.volume,
            &self.size,
            &self.at,
        ];
        if self.time == "transient" {
            v.extend([&self.end, &self.frames, &self.initial]);
            v.extend(self.heats.iter().flat_map(|r| [&r.from, &r.until]));
        }
        v.extend(self.loads.iter().map(|r| &r.value));
        for rows in [&self.temps, &self.heats, &self.convs] {
            v.extend(rows.iter().map(|r| &r.value));
        }
        v
    }

    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut v: Vec<&mut String> = vec![
            &mut self.rest_h,
            &mut self.ambient,
            &mut self.modes,
            &mut self.volume,
            &mut self.size,
            &mut self.at,
        ];
        if self.time == "transient" {
            v.extend([&mut self.end, &mut self.frames, &mut self.initial]);
        }
        v.extend(self.loads.iter_mut().map(|r| &mut r.value));
        for rows in [&mut self.temps, &mut self.convs] {
            v.extend(rows.iter_mut().map(|r| &mut r.value));
        }
        for r in &mut self.heats {
            v.push(&mut r.value);
            if self.time == "transient" {
                v.extend([&mut r.from, &mut r.until]);
            }
        }
        v
    }
}

/// 解析の研究。
#[derive(Debug, Clone)]
pub struct Study {
    pub kind: Kind,
    pub data: StudyData,
}

fn face_names(refs: &[Ref], what: &str) -> Result<Vec<String>, EvalError> {
    refs.iter()
        .map(|r| match r {
            Ref::Face { face, .. } => Ok(face.clone()),
            _ => Err(EvalError::InvalidData(format!(
                "{what}には面を選んでください(辺・頂点・ボディは使えません)"
            ))),
        })
        .collect()
}

fn direction(s: &str) -> Dir {
    let v = match s {
        "+x" => [1.0, 0.0, 0.0],
        "-x" => [-1.0, 0.0, 0.0],
        "+y" => [0.0, 1.0, 0.0],
        "-y" => [0.0, -1.0, 0.0],
        "+z" => [0.0, 0.0, 1.0],
        "-z" => [0.0, 0.0, -1.0],
        _ => return Dir::Normal,
    };
    Dir::Fixed(v)
}

impl Study {
    /// 研究するボディ(選んだ面のボディ。どれも同じボディ)。
    fn body(&self) -> Result<FeatureId, EvalError> {
        let first = self.data.all_refs().next().ok_or_else(|| {
            EvalError::InvalidData(match self.kind {
                Kind::Thermal => "温度を決める面か熱の入る面を選んでください".into(),
                _ => "固定する面を選んでください".into(),
            })
        })?;
        if self.data.all_refs().any(|r| r.feature() != first.feature()) {
            return Err(EvalError::InvalidData(
                "面は 1 つのボディから選んでください(今は 1 つのボディだけを調べます)".into(),
            ));
        }
        Ok(first.feature())
    }

    fn loads(&self, ctx: &EvalCtx<'_>) -> Result<Vec<Load>, EvalError> {
        let mut out = Vec::new();
        for (i, r) in self.data.loads.iter().enumerate() {
            if r.faces.is_empty() {
                continue;
            }
            let what = format!("荷重 {} の値", i + 1);
            let v = number(ctx, &r.value, &what)?;
            let kind = if r.kind == "pressure" {
                LoadKind::Pressure(v)
            } else {
                LoadKind::Force(v)
            };
            out.push(Load {
                faces: face_names(&r.faces, "荷重")?,
                kind,
                dir: direction(&r.dir),
            });
        }
        Ok(out)
    }

    fn rows(
        &self,
        ctx: &EvalCtx<'_>,
        rows: &[ValueRow],
        what: &str,
    ) -> Result<Vec<(Vec<String>, f64)>, EvalError> {
        let mut out = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            if r.faces.is_empty() {
                continue;
            }
            let v = number(ctx, &r.value, &format!("{what} {}", i + 1))?;
            out.push((face_names(&r.faces, what)?, v));
        }
        Ok(out)
    }
}

impl Feature for Study {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![InputRef::Output {
            feature: self.body()?,
            slot: "body".into(),
        }];
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
        let body = match ctx.output(self.body()?, "body")? {
            Value::Shape(h) => *h,
            _ => {
                return Err(EvalError::InvalidData(
                    "選んだ面のボディがありません".into(),
                ));
            }
        };
        let mat = *fea_material(&self.data.material).ok_or_else(|| {
            EvalError::InvalidData(format!(
                "材料 {} の解析の値がありません",
                self.data.material
            ))
        })?;
        let kernel = ctx.kernel();
        let mass = kernel
            .mass(body)
            .map_err(|e| EvalError::Kernel(e.to_string()))?;
        let auto = is_empty(&self.data.size);
        let h = if auto {
            size_for(mass.volume.abs().max(1e-9), self.kind.target())
        } else {
            let h = length(ctx, &self.data.size, "要素の大きさ")?;
            if h.is_nan() || h <= 0.0 {
                return Err(EvalError::InvalidData(
                    "要素の大きさは 0 より大きくしてください".into(),
                ));
            }
            if expected_tets(mass.volume.abs(), h) > MAX_TETS as f64 {
                return Err(EvalError::InvalidData(format!(
                    "要素の大きさ {} mm では要素が多すぎます(約 {:.0} 万個。上限 {} 万個)。大きくしてください",
                    fmt(h),
                    expected_tets(mass.volume.abs(), h) / 1e4,
                    MAX_TETS / 10_000
                )));
            }
            h
        };
        // 表示用の網の弦の誤差は要素の大きさより十分に小さく(辺の中点を乗せる曲面の確かさ)。
        // 自動の大きさでは h の 0.64 倍に合わせる(結果の値を確かめた時の表示用の網の細かさ。
        // 変えると網と結果の値が変わる)
        let tol_h = if auto { 0.64 * h } else { h };
        let mesh = kernel
            .mesh(body, (tol_h / 8.0).clamp(0.005, 0.2))
            .map_err(|e| EvalError::Kernel(e.to_string()))?;
        let section = {
            let axis = match self.data.plane.as_str() {
                "xz" => 1,
                "yz" => 0,
                _ => 2,
            };
            let at = if is_empty(&self.data.at) {
                bounds(&mesh).map_or(0.0, |(lo, hi)| 0.5 * (lo[axis] + hi[axis]))
            } else {
                length(ctx, &self.data.at, "断面の位置")?
            };
            Some((axis, at))
        };
        let solve = match self.kind {
            Kind::Static => Solve::Static(StaticInput {
                e: mat.e,
                nu: mat.nu,
                fixed: face_names(&self.data.fixed, "固定")?,
                loads: self.loads(ctx)?,
            }),
            Kind::Thermal => {
                let ambient = if is_empty(&self.data.ambient) {
                    20.0
                } else {
                    number(ctx, &self.data.ambient, "まわりの温度")?
                };
                let rest_h = if is_empty(&self.data.rest_h) {
                    None
                } else {
                    Some(number(ctx, &self.data.rest_h, "残りの面の熱伝達係数")?)
                };
                let thermal = ThermalInput {
                    k: mat.k,
                    temps: self.rows(ctx, &self.data.temps, "温度")?,
                    heats: self.rows(ctx, &self.data.heats, "熱")?,
                    convs: self.rows(ctx, &self.data.convs, "熱伝達係数")?,
                    rest_h,
                    ambient,
                };
                thermal_mode(&self.data, &mat, thermal, &|s, name| number(ctx, s, name))?
            }
            Kind::Modal => {
                let count = if is_empty(&self.data.modes) {
                    6
                } else {
                    let n = number(ctx, &self.data.modes, "求める数")?.round();
                    if !(1.0..=20.0).contains(&n) {
                        return Err(EvalError::InvalidData(
                            "求める数は 1 から 20 にしてください".into(),
                        ));
                    }
                    n as usize
                };
                Solve::Modal(ModalInput {
                    e: mat.e,
                    nu: mat.nu,
                    density: density(&mat),
                    fixed: face_names(&self.data.fixed, "固定")?,
                    count,
                })
            }
            Kind::Topology => {
                let volume = if is_empty(&self.data.volume) {
                    40.0
                } else {
                    number(ctx, &self.data.volume, "残す体積")?
                };
                Solve::Topology(TopologyInput {
                    e: mat.e,
                    nu: mat.nu,
                    fixed: face_names(&self.data.fixed, "固定")?,
                    loads: self.loads(ctx)?,
                    volume: volume / 100.0,
                    // 網の大きさが決まってから 1.5 要素にする
                    radius: 0.0,
                    iterations: 40,
                })
            }
        };
        let job = Job {
            kind: self.kind,
            mat,
            h,
            auto,
            volume: mass.volume,
            mesh,
            solve,
        };
        let key = job.key();
        let steps = job.steps();
        let keeps_partial = matches!(&job.solve, Solve::Transient(_));
        kmb_study::prepare(ctx, key, Arc::new(job));
        let mut out: Outputs = [(
            PLAN.to_string(),
            Plan {
                key,
                steps,
                section,
                keeps_partial,
            }
            .to_value(),
        )]
        .into();
        let mut warnings = Vec::new();
        if let Some(n) = anisotropy_note(&mat) {
            warnings.push(n);
        }
        add_warnings(&mut out, warnings);
        Ok(out)
    }
}

/// 読み切った解く条件。
#[derive(Debug, Clone, PartialEq)]
enum Solve {
    Static(StaticInput),
    Thermal(ThermalInput),
    Transient(TransientInput),
    Modal(ModalInput),
    Topology(TopologyInput),
}

fn thermal_mode(
    data: &StudyData,
    mat: &FeaMaterial,
    thermal: ThermalInput,
    read: &dyn Fn(&str, &str) -> Result<f64, EvalError>,
) -> Result<Solve, EvalError> {
    Ok(match data.time.as_str() {
        "" => Solve::Thermal(thermal),
        "transient" => {
            let end = read(&data.end, "終わりの時刻")?;
            if !end.is_finite() || end <= 0.0 {
                return Err(EvalError::InvalidData(
                    "終わりの時刻は 0 より大きくしてください".into(),
                ));
            }
            let frames = if is_empty(&data.frames) {
                50.0
            } else {
                read(&data.frames, "コマの数")?
            };
            if !(2.0..=200.0).contains(&frames) || frames.fract() != 0.0 {
                return Err(EvalError::InvalidData(
                    "コマの数は 2 から 200 の整数にしてください".into(),
                ));
            }
            let initial = if is_empty(&data.initial) {
                thermal.ambient
            } else {
                read(&data.initial, "初めの温度")?
            };
            if !initial.is_finite() {
                return Err(EvalError::InvalidData(
                    "初めの温度は有限の数にしてください".into(),
                ));
            }
            let mut heat_times = Vec::new();
            for r in data.heats.iter().filter(|r| !r.faces.is_empty()) {
                let from = if is_empty(&r.from) {
                    0.0
                } else {
                    read(&r.from, "入れ始め")?
                };
                let until = if is_empty(&r.until) {
                    end
                } else {
                    read(&r.until, "入れ終わり")?
                };
                if !from.is_finite()
                    || !until.is_finite()
                    || from < 0.0
                    || until <= from
                    || until > end
                {
                    return Err(EvalError::InvalidData(
                        "熱の時刻は 0 ≦ 入れ始め < 入れ終わり ≦ 終わりの時刻にしてください".into(),
                    ));
                }
                heat_times.push(HeatTime { from, until });
            }
            Solve::Transient(TransientInput {
                thermal,
                density: density(mat),
                cp: mat.cp,
                end,
                frames: frames as usize,
                initial,
                heat_times,
            })
        }
        _ => return Err(EvalError::InvalidData("時間の選び方が分かりません".into())),
    })
}

/// 研究 1 つの解く仕事(評価で作り、「実行」で解く)。
struct Job {
    kind: Kind,
    mat: FeaMaterial,
    /// 要素の大きさ(mm)と、自動で決めた大きさか(自動なら、要素が多すぎる時に大きくする)
    h: f64,
    auto: bool,
    /// ボディの体積(mm³)
    volume: f64,
    mesh: Mesh,
    solve: Solve,
}

impl Job {
    /// 解いた後に中止が届いた場合も、結果を残せるかを確かめる。
    fn check_cancelled(&self, c: &Control, mut result: SimResult) -> Result<SimResult, String> {
        if c.cancelled() {
            if matches!(self.solve, Solve::Transient(_)) {
                result.partial = true;
            }
            if !result.partial {
                return Err(CANCELLED.into());
            }
        }
        Ok(result)
    }

    /// 同じ条件・同じ形なら同じ鍵。
    fn key(&self) -> u64 {
        let mut h = Fnv::default();
        h.eat(self.kind.key().as_bytes());
        h.eat(
            format!(
                "{:?}|{:?}|{}|{:?}|{}",
                self.mat, self.solve, self.h, self.auto, self.volume
            )
            .as_bytes(),
        );
        h.eat_f32s(self.mesh.positions.iter().flatten().copied());
        for t in &self.mesh.triangles {
            for i in t {
                h.eat(&i.to_le_bytes());
            }
        }
        for f in &self.mesh.triangle_faces {
            h.eat(&f.to_le_bytes());
        }
        for n in &self.mesh.face_names {
            h.eat(n.as_bytes());
        }
        h.0
    }

    fn steps(&self) -> Vec<String> {
        let solve = match self.kind {
            Kind::Static => "つり合いを解く",
            Kind::Thermal if matches!(self.solve, Solve::Transient(_)) => "時間を進める",
            Kind::Thermal => "温度を解く",
            Kind::Modal => "固有振動を解く",
            Kind::Topology => "残す所を探す",
        };
        vec!["網を作る".into(), solve.into(), "結果をまとめる".into()]
    }

    /// 自動の大きさ: 体積だけで決めた大きさ `self.h` で、大きさの場(薄い所・細い所・曲率で
    /// 小さくした大きさ)から見込んだ要素が多すぎるなら、収まるまで大きくする。薄い所の大きさは
    /// 厚さで決まり h に比例して減らないので、見込みがあまり減らなければ倍にする。
    fn auto_size(&self, c: &Control) -> Result<f64, String> {
        let limit = 0.9 * MAX_TETS as f64;
        let mut h = self.h;
        let mut prev: Option<f64> = None;
        for _ in 0..8 {
            if c.cancelled() {
                return Err(CANCELLED.into());
            }
            let est = estimate_tets(&self.mesh, h)?;
            if est <= limit {
                break;
            }
            let mut k = (est / (0.7 * MAX_TETS as f64)).cbrt();
            if prev.is_some_and(|p| est > 0.8 * p) {
                k = k.max(2.0);
            }
            prev = Some(est);
            h *= k.max(1.1);
        }
        Ok(h)
    }

    /// 網を作る(自動の大きさなら、要素が多すぎない大きさを先に決める)。
    fn prepare(&self, c: &Control) -> Result<(Prepared, Vec<String>), String> {
        let mut notes = Vec::new();
        let h = if self.auto {
            let h = self.auto_size(c)?;
            if h > self.h * 1.001 {
                notes.push(format!(
                    "自動の要素の大きさ {} mm では、薄い所・細い所・小さな丸みで要素が多くなりすぎるので、{} mm にしました",
                    fmt(self.h),
                    fmt(h)
                ));
            }
            h
        } else {
            self.h
        };
        c.progress(0.05, "");
        let p = Prepared::new(&self.mesh, h, &|| c.cancelled()).map_err(|e| {
            if c.cancelled() {
                CANCELLED.to_string()
            } else {
                e
            }
        })?;
        let off = |p: &Prepared| {
            if self.volume > 0.0 {
                ((p.mesh.stats.volume - self.volume) / self.volume).abs()
            } else {
                0.0
            }
        };
        let st = &p.mesh.stats;
        if self.volume > 0.0 && off(&p) > 0.03 {
            notes.push(format!(
                "網の体積がボディと {:.1}% 違います(要素の大きさ {} mm)。薄い所や細い所が網から抜けているかもしれません。要素の大きさを小さくしてください",
                (st.volume - self.volume) / self.volume * 100.0,
                fmt(st.h)
            ));
        }
        if st.miss_max > 0.25 * st.h {
            notes.push(format!(
                "小さな穴や角など、要素の大きさ {} mm より細かい形が網で粗くなっています(形から網へ最大 {} mm、面 {})。そこの応力を見るなら要素の大きさを小さくしてください",
                fmt(st.h),
                fmt(st.miss_max),
                st.miss_face
            ));
        }
        log::info!("解析の網: {st:?}");
        Ok((p, notes))
    }
}

/// 中止した時の誤り。
const CANCELLED: &str = "中止しました";

impl Task for Job {
    fn solve(&self, c: &Control) -> Result<SimResult, String> {
        let started = std::time::Instant::now();
        c.step(0);
        let (p, notes) = self.prepare(c)?;
        c.step(1);
        c.progress(0.2, format!("要素 {} 個", p.mesh.tets.len()));
        let r = match &self.solve {
            Solve::Static(i) => static_result(&p, &self.mat, i)?,
            Solve::Thermal(i) => thermal_result(&p, &self.mat, i)?,
            Solve::Transient(i) => transient_result(&p, &self.mat, i, c)?,
            Solve::Modal(i) => modal_result(&p, &self.mat, i)?,
            Solve::Topology(i) => topology_result(&p, &self.mat, i, c)?,
        };
        let mut r = self.check_cancelled(c, r)?;
        c.step(2);
        c.progress(0.98, "");
        let st = &p.mesh.stats;
        log::info!(
            "解析({}): 要素 {} 個・{} mm、{:.2} 秒。{}",
            self.kind.key(),
            p.mesh.tets.len(),
            fmt(st.h),
            started.elapsed().as_secs_f64(),
            r.summary
        );
        r.kind = self.kind.key().into();
        add_quality(&mut r, &p);
        r.numbers.extend(mesh_numbers(&p, self.volume));
        r.badge = format!(
            "2 次の四面体 {} mm・要素 {} 個",
            fmt(st.h),
            p.mesh.tets.len()
        );
        let mut note = notes.join("\n");
        if !r.note.is_empty() {
            if !note.is_empty() {
                note.push('\n');
            }
            note.push_str(&r.note);
        }
        if !note.is_empty() {
            note.push('\n');
        }
        note.push_str(NOTE);
        r.note = note;
        Ok(r)
    }
}

/// 節点の値を結果の点の値に(結果の点は網の節点と同じ)。
fn on_surface(nodes: &[u32], values: impl Fn(usize) -> f64) -> Vec<f32> {
    nodes.iter().map(|n| values(*n as usize) as f32).collect()
}

/// 網 1 つと、量・コマの空の結果。返す番号は結果の点ごとの節点の番号。
fn surface_result(p: &Prepared, keep: &[bool]) -> (SimResult, Vec<u32>) {
    let r = SimResult {
        parts: vec![Part {
            name: "ボディ".into(),
            geom: geometry(&p.mesh, keep),
        }],
        ..SimResult::default()
    };
    (r, (0..p.mesh.points.len() as u32).collect())
}

/// 要素の形の良さ(半径比。正四面体で 1)を量として足す(どこの要素が悪いかを色で見る)。
fn add_quality(r: &mut SimResult, p: &Prepared) {
    let q = p.mesh.node_quality();
    r.quantities.push(Quantity {
        low_is_bad: true,
        ..Quantity::scalar("網の質", "", 0)
    });
    let v = Values::exact(q.iter().map(|x| *x as f32).collect());
    for f in &mut r.frames {
        f.values.push(v.clone());
    }
}

/// 網の確かさの数値(体積の差・表面のずれ・要素の形)。
fn mesh_numbers(p: &Prepared, body_volume: f64) -> Vec<(String, String)> {
    let st = &p.mesh.stats;
    let body = if body_volume > 0.0 {
        body_volume
    } else {
        st.shape_volume
    };
    let diff = (st.volume - body) / body.max(1e-30) * 100.0;
    vec![
        (
            "網".into(),
            format!(
                "2 次の四面体 {} 個・節点 {} 個(要素の大きさ {} mm){}",
                p.mesh.tets.len(),
                p.mesh.points.len(),
                fmt(st.h),
                if st.band_points > 0 {
                    format!("・鋭い縁に沿う帯 {} 点", st.band_points)
                } else {
                    String::new()
                }
            ),
        ),
        (
            "体積の差".into(),
            format!(
                "網 {} mm³・ボディ {} mm³({diff:+.2}%)",
                fmt(st.volume),
                fmt(body)
            ),
        ),
        (
            "表面のずれ".into(),
            format!(
                "網から形へ 最大 {} mm・形から網へ 最大 {} mm(面 {})",
                fmt(st.dev_max),
                fmt(st.miss_max),
                if st.miss_face.is_empty() {
                    "—"
                } else {
                    &st.miss_face
                }
            ),
        ),
        (
            "網の質".into(),
            format!(
                "形の良い要素 {:.1}%(半径比 0.2 以上)・二面角 {:.1}°〜{:.1}°・平たい要素 {} 個",
                (st.good_share * 1000.0).floor() / 10.0,
                st.min_dihedral,
                st.max_dihedral,
                st.bad_angles
            ),
        ),
    ]
}

fn one_frame(values: Vec<Values>) -> Vec<Frame> {
    vec![Frame {
        label: String::new(),
        time: None,
        values,
    }]
}

fn static_result(
    p: &Prepared,
    mat: &FeaMaterial,
    input: &StaticInput,
) -> Result<SimResult, String> {
    let out = solve_static(p, input)?;
    let keep = vec![true; p.mesh.tets.len()];
    let (mut r, nodes) = surface_result(p, &keep);
    let disp: Vec<f32> = nodes
        .iter()
        .flat_map(|n| out.u[*n as usize].map(|v| v as f32))
        .collect();
    let max_vm = out.max_vm();
    let max_u = out.max_disp();
    let sf = if max_vm > 0.0 {
        mat.strength / max_vm
    } else {
        f64::INFINITY
    };
    // ミーゼス応力と安全率は、塗る点ごとに応力の成分(4 番目の量)を補間してから求める
    r.quantities = vec![
        Quantity {
            from: Some((3, Derive::VonMises)),
            ..Quantity::scalar("ミーゼス応力", "MPa", 0)
        },
        Quantity::vector("変位", "mm", 0, Role::Deform),
        Quantity {
            low_is_bad: true,
            cap: Some(10.0),
            from: Some((3, Derive::Over(mat.strength as f32))),
            ..Quantity::scalar("安全率", "", 0)
        },
        Quantity::tensor("応力", "MPa", 0),
    ];
    let tensor: Vec<f32> = nodes
        .iter()
        .flat_map(|n| out.node_stress[*n as usize].map(|v| v as f32))
        .collect();
    r.frames = one_frame(vec![
        Values::exact(on_surface(&nodes, |n| out.node_vm[n])),
        Values::exact(disp),
        Values::exact(on_surface(&nodes, |n| {
            let v = out.node_vm[n];
            if v > 0.0 { mat.strength / v } else { 1e6 }
        })),
        Values::exact(tensor),
    ]);
    let sf_text = if sf.is_finite() {
        fmt(sf)
    } else {
        "—".into()
    };
    // 特異点の候補: 固定した面の縁と入隅。要素 2 層より離れた所の最大も並べる(印のそばの
    // 穴の縁などの別の形は離れた側に入る)
    let fixed = p.faces_of(&input.fixed)?;
    let sing = singular::find(p, &fixed, 2);
    let away = out
        .node_vm
        .iter()
        .zip(&sing.near)
        .filter(|(_, n)| !**n)
        .map(|(v, _)| *v)
        .fold(0.0, f64::max);
    let hot = out
        .node_vm
        .iter()
        .zip(&sing.near)
        .any(|(v, n)| *n && *v > away * 1.000_001);
    if !sing.edges.is_empty() {
        r.marks.push(Mark {
            name: "特異点の候補(固定した縁・入隅)".into(),
            part: 0,
            lines: sing.edges.clone(),
        });
    }
    r.numbers = vec![
        ("最大のミーゼス応力".into(), format!("{} MPa", fmt(max_vm))),
        ("最大の変位".into(), format!("{} mm", fmt(max_u))),
        (
            "最小の安全率".into(),
            format!(
                "{sf_text}({} {} MPa ÷ 最大の応力)",
                mat.strength_kind,
                fmt(mat.strength)
            ),
        ),
        (
            "材料".into(),
            format!("{}(縦弾性係数 {} MPa)", mat.name, fmt(mat.e)),
        ),
    ];
    if hot {
        let sf_away = if away > 0.0 {
            fmt(mat.strength / away)
        } else {
            "—".into()
        };
        r.numbers.insert(
            1,
            (
                "特異点を除いた最大".into(),
                format!(
                    "{} MPa・安全率 {sf_away}(印のまわりの要素 {} 層より外。印のそばの穴の縁などは含める)",
                    fmt(away),
                    sing.layers
                ),
            ),
        );
        r.note = SINGULAR_NOTE.into();
    }
    r.summary = format!(
        "最大 {} MPa・変位 {} mm・安全率 {sf_text}",
        fmt(max_vm),
        fmt(max_u)
    );
    Ok(r)
}

fn thermal_result(
    p: &Prepared,
    mat: &FeaMaterial,
    input: &ThermalInput,
) -> Result<SimResult, String> {
    let out = solve_thermal(p, input)?;
    let keep = vec![true; p.mesh.tets.len()];
    let (mut r, nodes) = surface_result(p, &keep);
    let tmax = out.t.iter().copied().fold(f64::MIN, f64::max);
    let tmin = out.t.iter().copied().fold(f64::MAX, f64::min);
    r.quantities = vec![Quantity::scalar("温度", "℃", 0)];
    r.frames = one_frame(vec![Values::exact(on_surface(&nodes, |n| out.t[n]))]);
    r.numbers = vec![
        ("最高の温度".into(), format!("{} ℃", fmt(tmax))),
        ("最低の温度".into(), format!("{} ℃", fmt(tmin))),
        ("空気へ逃げた熱".into(), format!("{} W", fmt(out.to_air))),
        (
            "材料".into(),
            format!(
                "{}(熱伝導率 {} W/(m·K)、比熱 {} J/(kg·K))",
                mat.name,
                fmt(mat.k),
                fmt(mat.cp)
            ),
        ),
    ];
    r.summary = format!("最高 {} ℃・最低 {} ℃", fmt(tmax), fmt(tmin));
    Ok(r)
}

fn transient_result(
    p: &Prepared,
    mat: &FeaMaterial,
    input: &TransientInput,
    c: &Control,
) -> Result<SimResult, String> {
    let out = solve_transient(p, input, &|| c.cancelled(), &|f| {
        c.progress((0.2 + 0.75 * f) as f32, format!("{} s", fmt(input.end * f)))
    })?;
    let keep = vec![true; p.mesh.tets.len()];
    let (mut r, nodes) = surface_result(p, &keep);
    r.quantities = vec![Quantity::scalar("温度", "℃", 0)];
    r.frames = out
        .frames
        .iter()
        .map(|f| Frame {
            label: format!("{} s", fmt(f.time)),
            time: Some(f.time),
            values: vec![Values::exact(on_surface(&nodes, |n| f.t[n]))],
        })
        .collect();
    r.series = [("最高の温度(℃)", false), ("空気へ逃げた熱(W)", true)]
        .into_iter()
        .map(|(name, air)| kmb_core::Series {
            name: name.into(),
            x_name: "時刻 s".into(),
            note: String::new(),
            points: out
                .frames
                .iter()
                .map(|f| {
                    [
                        f.time,
                        if air {
                            f.to_air
                        } else {
                            f.t.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                        },
                    ]
                })
                .collect(),
        })
        .collect();
    let last = out.frames.last().ok_or("温度のコマがありません")?;
    let high = last.t.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low = last.t.iter().copied().fold(f64::INFINITY, f64::min);
    r.numbers = vec![
        ("最高の温度".into(), format!("{} ℃", fmt(high))),
        ("最低の温度".into(), format!("{} ℃", fmt(low))),
        ("使った刻みの数".into(), out.steps.to_string()),
        (
            "最小の刻み".into(),
            if out.steps == 0 {
                "—".into()
            } else {
                format!("{} s", fmt(out.min_step))
            },
        ),
        (
            "材料".into(),
            format!(
                "{}(熱伝導率 {} W/(m·K)、比熱 {} J/(kg·K))",
                mat.name,
                fmt(mat.k),
                fmt(mat.cp)
            ),
        ),
    ];
    r.summary = format!(
        "{} s・最高 {} ℃・最低 {} ℃",
        fmt(last.time),
        fmt(high),
        fmt(low)
    );
    r.partial = out.cancelled;
    if out.coarse {
        r.note = "網の細かさでは、初めの数刻みの急な変化を表せません(要素の大きさを小さくすると表せます)".into();
    }
    Ok(r)
}

fn modal_result(p: &Prepared, mat: &FeaMaterial, input: &ModalInput) -> Result<SimResult, String> {
    let out = solve_modal(p, input)?;
    let keep = vec![true; p.mesh.tets.len()];
    let (mut r, nodes) = surface_result(p, &keep);
    // モードごとに 1 コマ(時刻の帯でモードを送る)
    r.quantities = vec![Quantity::vector("揺れ", "相対", 0, Role::Deform)];
    r.frames = out
        .freqs
        .iter()
        .zip(&out.shapes)
        .enumerate()
        .map(|(i, (f, s))| Frame {
            label: format!("{} 次 {} Hz", i + 1, fmt(*f)),
            time: None,
            values: vec![Values::exact(
                nodes
                    .iter()
                    .flat_map(|n| s[*n as usize].map(|v| v as f32))
                    .collect(),
            )],
        })
        .collect();
    if r.frames.is_empty() {
        return Err("固有振動が見つかりませんでした".into());
    }
    let list: Vec<String> = out
        .freqs
        .iter()
        .map(|f| format!("{} Hz", fmt(*f)))
        .collect();
    let rpm = out.freqs.first().map_or(0.0, |f| f * 60.0);
    r.numbers = vec![
        ("固有振動数".into(), list.join("、")),
        (
            "1 次を回転数にすると".into(),
            format!("{} 回転/分", fmt(rpm)),
        ),
        (
            "材料".into(),
            format!(
                "{}(縦弾性係数 {} MPa、密度 {} g/cm³)",
                mat.name,
                fmt(mat.e),
                fmt(density(mat))
            ),
        ),
    ];
    r.summary = format!("1 次 {}", list.first().cloned().unwrap_or_default());
    Ok(r)
}

fn topology_result(
    p: &Prepared,
    mat: &FeaMaterial,
    input: &TopologyInput,
    c: &Control,
) -> Result<SimResult, String> {
    let input = TopologyInput {
        radius: 1.5 * p.h(),
        ..input.clone()
    };
    let total = input.iterations.max(1);
    let out = solve_topology(p, &input, |i, _| {
        c.progress(
            0.2 + 0.75 * (i as f32 / total as f32).min(1.0),
            format!("{i}/{total} 回"),
        );
    })?;
    let keep: Vec<bool> = out.density.iter().map(|d| *d >= 0.5).collect();
    let (mut r, nodes) = surface_result(p, &keep);
    // 節点の密度: まわりの要素の平均
    let mut sum = vec![0.0; p.mesh.points.len()];
    let mut cnt = vec![0u32; p.mesh.points.len()];
    for (e, el) in p.mesh.tets.iter().enumerate() {
        for n in el {
            sum[*n as usize] += out.density[e];
            cnt[*n as usize] += 1;
        }
    }
    let vol: Vec<f64> = (0..keep.len())
        .map(|e| crate::tet10::volume(&p.mesh.tet_points(e)))
        .collect();
    let kept = keep
        .iter()
        .zip(&vol)
        .filter(|(k, _)| **k)
        .map(|x| x.1)
        .sum::<f64>()
        / vol.iter().sum::<f64>().max(1e-30);
    let cv = &out.compliance;
    let gain = if cv.len() > 1 && cv[cv.len() - 1] > 0.0 {
        cv[0] / cv[cv.len() - 1]
    } else {
        1.0
    };
    r.quantities = vec![Quantity::scalar("残す所の密度", "", 0)];
    r.frames = one_frame(vec![Values::exact(on_surface(&nodes, |n| {
        sum[n] / cnt[n].max(1) as f64
    }))]);
    r.numbers = vec![
        (
            "残した体積".into(),
            format!(
                "{:.0}%(決めた割合 {:.0}%)",
                kept * 100.0,
                input.volume * 100.0
            ),
        ),
        (
            "たわみにくさ".into(),
            format!(
                "同じ体積で一様に薄くした時の {} 倍(繰り返し {} 回)",
                fmt(gain),
                cv.len()
            ),
        ),
        ("材料".into(), mat.name.into()),
    ];
    r.summary = format!("残す体積 {:.0}%(参考の形)", kept * 100.0);
    Ok(r)
}

/// 結果の見方の注意(強度と熱の研究)。
/// 最大の応力が特異点の候補にある時の注意。
pub const SINGULAR_NOTE: &str = "最大の応力は印の所(固定した縁・入隅)にあります。ここは理論上の応力が無限に上がる所で、網を細かくするほど値が上がり続けます。強さの判断は「特異点を除いた最大」か、印から離れた所の値で行ってください。除いた値は、印のまわりで応力が離れるほど下がる分だけ根元の値より低く出ます(網を細かくすると近づきます)。";
pub const NOTE: &str = "線形・小変形・接触なしの目安です。網は形に沿った 2 次の四面体で曲面も表しますが、角や穴の縁の応力は網の細かさで変わります(要素の大きさを小さくして値が落ち着くか確かめてください)。ねじ・接着・溶接は面ごと固まっているとみなします。";

const DIRS: &[(&str, &str)] = &[
    ("normal", "面を押す向き"),
    ("-z", "-Z(下)"),
    ("+z", "+Z(上)"),
    ("-x", "-X"),
    ("+x", "+X"),
    ("-y", "-Y"),
    ("+y", "+Y"),
];

fn study(type_id: &str, label: &str) -> Descriptor {
    Descriptor::new(type_id, category::STUDY, label).menu("研究")
}

fn material_field() -> Field {
    let mut choices = crate::material::choices();
    choices.sort_by_key(|c| c.0 != crate::material::DEFAULT_MATERIAL);
    Field::choice("material", "材料", &choices)
        .help("縦弾性係数・強さ・熱伝導率は一般的な値の目安(品種で変わる)")
}

fn fixed_field(label: &str) -> Field {
    Field::pick("fixed", label, &[RefKind::Face], 1, None)
        .help("動かない面(壁にねじ止めする面、万力でつかむ面など)をクリックで足す。どれも 1 つのボディの面")
}

fn loads_field() -> Field {
    Field::rows(
        "loads",
        "荷重",
        vec![
            Field::pick("faces", "面", &[RefKind::Face], 1, None)
                .help("力がかかる面をクリックで足す"),
            Field::choice(
                "kind",
                "種類",
                &[("force", "力(面全体の合計)"), ("pressure", "圧力")],
            ),
            Field::expr("value", "大きさ(N・圧力は MPa)")
                .plain("N")
                .help("力は N(1 kg の重さで約 9.8 N)、圧力は MPa(= N/mm²)。数だけで書く"),
            Field::choice("dir", "向き", DIRS)
                .only("@kind", "force", "force")
                .help("面を押す向きか、決まった向き(重さなら -Z)"),
        ],
    )
}

fn size_field() -> Field {
    Field::opt_expr("size", "要素の大きさ")
        .length()
        .help("四面体の要素の辺のおよその長さ。曲面・細い面・薄い所では自動で小さくする。空なら自動(形の体積から)。小さくすると細かく正確になるが遅い")
}

fn plane_field() -> Field {
    Field::choice(
        "plane",
        "断面",
        &[("xy", "水平(XY)"), ("xz", "正面(XZ)"), ("yz", "側面(YZ)")],
    )
    .help("「断面で切る」で形を切る面。見せ方なので、直しても解き直さない")
}

fn at_field() -> Field {
    Field::opt_expr("at", "断面の位置")
        .length()
        .help("断面の法線の向きの座標。空ならボディの真ん中。直しても解き直さない")
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        study(STATIC, "静的応力")
            .icon("fea_static")
            .order(1)
            .pinned()
            .hint("固定する面と荷重を付けて、応力・変位・安全率を色で見る(線形・小変形の目安)")
            .aliases("きょうど おうりょく たわみ あんぜんりつ 強度 応力 静解析 stress static fea")
            .field(material_field())
            .field(fixed_field("固定"))
            .field(loads_field())
            .field(size_field())
            .field(plane_field())
            .field(at_field())
            .template(json!({"material": crate::material::DEFAULT_MATERIAL, "loads": [{"kind": "force", "value": "100", "dir": "-z"}]})),
        |d| Ok(Box::new(Study { kind: Kind::Static, data: from(d)? })),
    );
    reg.register_described(
        study(THERMAL, "熱")
            .icon("fea_thermal")
            .order(2)
            .pinned()
            .hint("温度を決める面・熱が入る面・空気へ逃げる面を付けて、定常や時間で変わる温度の分布を色で見る")
            .aliases("ねつ おんど ほうねつ 熱 温度 放熱 thermal heat")
            .field(material_field())
            .field(Field::choice("time", "時間", &[("", "定常"), ("transient", "時間で変わる")]))
            .field(Field::expr("end", "終わりの時刻(s)").plain("s").only("time", "transient", ""))
            .field(Field::opt_expr("frames", "コマの数").unitless().only("time", "transient", "").help("空なら 50。2 から 200 の整数。時刻 0 のコマも加える"))
            .field(Field::opt_expr("initial", "初めの温度(℃)").plain("℃").only("time", "transient", "").help("空ならまわりの温度"))
            .field(Field::rows(
                "temps",
                "温度を決める面",
                vec![
                    Field::pick("faces", "面", &[RefKind::Face], 1, None),
                    Field::expr("value", "温度(℃)").plain("℃"),
                ],
            ))
            .field(Field::rows(
                "heats",
                "熱が入る面",
                vec![
                    Field::pick("faces", "面", &[RefKind::Face], 1, None),
                    Field::expr("value", "熱(W、面全体)").plain("W")
                        .help("LED やモータードライバーの発熱など。面全体の合計"),
                    Field::opt_expr("from", "入れ始め(s)").plain("s").only("time", "transient", "").help("時間で変わる熱だけに使う。空なら 0"),
                    Field::opt_expr("until", "入れ終わり(s)").plain("s").only("time", "transient", "").help("時間で変わる熱だけに使う。空なら終わりの時刻"),
                ],
            ))
            .field(Field::rows(
                "convs",
                "空気へ逃げる面",
                vec![
                    Field::pick("faces", "面", &[RefKind::Face], 1, None),
                    Field::expr("value", "熱伝達係数(W/(m²·K))").plain("W/(m²·K)")
                        .help("静かな空気で 5〜10、ファンで風を当てると 20〜100 ほど"),
                ],
            ))
            .field(
                Field::opt_expr("rest_h", "残りの面の熱伝達係数(W/(m²·K))").plain("W/(m²·K)")
                    .help("上で選んでいない外の面から空気へ逃げる熱。空なら逃げない"),
            )
            .field(Field::opt_expr("ambient", "まわりの温度(℃)").plain("℃").help("空なら 20 ℃"))
            .field(size_field())
            .field(plane_field())
            .field(at_field())
            .template(json!({"material": crate::material::DEFAULT_MATERIAL, "rest_h": "10", "ambient": "20", "heats": [{"value": "5"}]})),
        |d| Ok(Box::new(Study { kind: Kind::Thermal, data: from(d)? })),
    );
    reg.register_described(
        study(MODAL, "固有振動")
            .icon("fea_modal")
            .order(3)
            .pinned()
            .hint("固定する面を付けて、揺れやすい周波数(共振)と揺れの形を見る。主軸の回転(約 170〜400 Hz)と比べる")
            .aliases("こゆうしんどう きょうしん しゅうはすう 共振 振動 モード modal frequency")
            .field(material_field())
            .field(fixed_field("固定"))
            .field(Field::opt_expr("modes", "求める数").unitless().help("小さい周波数から。空なら 6"))
            .field(size_field())
            .field(plane_field())
            .field(at_field())
            .template(json!({"material": crate::material::DEFAULT_MATERIAL, "modes": "6"})),
        |d| Ok(Box::new(Study { kind: Kind::Modal, data: from(d)? })),
    );
    reg.register_described(
        study(TOPOLOGY, "軽い形の参考")
            .icon("fea_topology")
            .order(4)
            .pinned()
            .hint("固定と荷重から、力を伝えるのに要る所だけ残した形を参考に見る(形は変えない。見て肉抜きを描く)")
            .aliases("けいりょうか にくぬき トポロジー 最適化 軽量化 topology optimization simp")
            .field(material_field())
            .field(fixed_field("固定"))
            .field(loads_field())
            .field(Field::opt_expr("volume", "残す体積(%)").plain("%").help("空なら 40%"))
            .field(size_field())
            .field(plane_field())
            .field(at_field())
            .template(json!({"material": crate::material::DEFAULT_MATERIAL, "volume": "40", "loads": [{"kind": "force", "value": "100", "dir": "-z"}]})),
        |d| Ok(Box::new(Study { kind: Kind::Topology, data: from(d)? })),
    );
    for t in [STATIC, THERMAL, MODAL, TOPOLOGY] {
        reg.mark(t, kmb_core::CONCURRENT);
    }
}

fn from(d: &serde_json::Value) -> Result<StudyData, EvalError> {
    serde_json::from_value(d.clone()).map_err(|e| EvalError::InvalidData(e.to_string()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn old_thermal_document_keeps_steady_results() {
        let old = from(&json!({"rest_h":"100", "ambient":"20"})).unwrap();
        let mut filled = old.clone();
        filled.end = "invalid".into();
        filled.frames = "0".into();
        filled.initial = "invalid".into();
        filled.heats.push(ValueRow {
            from: "invalid".into(),
            until: "-1".into(),
            ..ValueRow::default()
        });
        let mut thermal = crate::validation_tests::transient_case().thermal;
        thermal.temps = vec![(vec!["x0".into()], 100.0)];
        let mat = fea_material("").unwrap();
        let read = |_: &str, _: &str| -> Result<f64, EvalError> {
            panic!("定常では時刻を読まない")
        };
        let a = thermal_mode(&old, mat, thermal.clone(), &read).unwrap();
        let b = thermal_mode(&filled, mat, thermal, &read).unwrap();
        assert_eq!(a, b);
        let p =
            Prepared::new(&crate::validation_tests::box_mesh([2.0; 3]), 1.0, &|| false).unwrap();
        let Solve::Thermal(a) = a else {
            panic!("定常を選ぶ")
        };
        let Solve::Thermal(b) = b else {
            panic!("定常を選ぶ")
        };
        let a = thermal_result(&p, mat, &a).unwrap();
        let b = thermal_result(&p, mat, &b).unwrap();
        assert_eq!(a.frames, b.frames);
        assert_eq!(a.frames.len(), 1);
        assert_eq!(a.frames[0].time, None);
    }

    /// 確かめること: 過渡の欄の既定値と入力の範囲、熱の時刻の範囲。
    #[test]
    fn transient_fields_validate_defaults_and_ranges() {
        let mat = fea_material("").unwrap();
        let thermal = crate::validation_tests::transient_case().thermal;
        let read = |s: &str, _: &str| {
            s.parse::<f64>()
                .map_err(|_| EvalError::InvalidData("数を入れてください".into()))
        };
        let mut d = from(&json!({"time":"transient", "end":"10"})).unwrap();
        let Solve::Transient(i) = thermal_mode(&d, mat, thermal.clone(), &read).unwrap() else {
            panic!("過渡を選ぶ")
        };
        assert_eq!(i.frames, 50);
        assert_eq!(i.initial, thermal.ambient);
        for end in ["", "0", "-1"] {
            d.end = end.into();
            assert!(thermal_mode(&d, mat, thermal.clone(), &read).is_err());
        }
        d.end = "10".into();
        for frames in ["1", "201", "2.5"] {
            d.frames = frames.into();
            assert!(thermal_mode(&d, mat, thermal.clone(), &read).is_err());
        }
        d.frames.clear();
        d.heats = vec![ValueRow {
            faces: vec![Ref::Face {
                feature: FeatureId(1),
                face: "x0".into(),
            }],
            value: "1".into(),
            ..Default::default()
        }];
        let mut thermal = thermal;
        thermal.heats = vec![(vec!["x0".into()], 1.0)];
        let Solve::Transient(defaults) = thermal_mode(&d, mat, thermal.clone(), &read).unwrap()
        else {
            panic!("過渡を選ぶ")
        };
        assert_eq!(
            defaults.heat_times,
            vec![HeatTime {
                from: 0.0,
                until: 10.0
            }]
        );
        for (from, until) in [
            ("-1", "5"),
            ("5", "5"),
            ("6", "5"),
            ("0", "11"),
            ("NaN", "5"),
        ] {
            d.heats[0].from = from.into();
            d.heats[0].until = until.into();
            assert!(thermal_mode(&d, mat, thermal.clone(), &read).is_err());
        }
    }

    /// 確かめること: 時刻 0 を含むコマ、共通の範囲、推移、途中結果の受け渡し。
    #[test]
    fn transient_results_have_times_series_and_partial_frames() {
        let p =
            Prepared::new(&crate::validation_tests::box_mesh([2.0; 3]), 1.0, &|| false).unwrap();
        let mat = fea_material("").unwrap();
        let mut i = crate::validation_tests::transient_case();
        i.frames = 2;
        let c = Control::new();
        let result = transient_result(&p, mat, &i, &c).unwrap();
        assert_eq!(result.frames.len(), 3);
        assert_eq!(result.frames[0].time, Some(0.0));
        assert_eq!(result.frames[2].time, Some(i.end));
        assert_eq!(result.series.len(), 2);
        assert!(result.series.iter().all(|s| s.points.len() == 3));
        let range = result.range(0, None);
        assert!((range.1 - i.initial as f32).abs() < 1e-4);
        assert!(range.0 < range.1);
        assert!(result.number("使った刻みの数").is_some());
        c.cancel();
        let partial = transient_result(&p, mat, &i, &c).unwrap();
        assert!(partial.partial);
        assert_eq!(partial.frames.len(), 1);
        assert_eq!(partial.frames[0].time, Some(0.0));
        i.thermal.k = 1.0;
        i.thermal.rest_h = Some(1e9);
        i.end = 0.01;
        let abrupt = transient_result(&p, mat, &i, &Control::new()).unwrap();
        assert!(
            abrupt
                .note
                .contains("網の細かさでは、初めの数刻みの急な変化を表せません")
        );
    }

    /// 確かめること: 解いた後の中止でも、完成した過渡のコマを捨てない。
    #[test]
    fn transient_late_cancel_keeps_completed_frames() {
        let input = crate::validation_tests::transient_case();
        let job = |solve| Job {
            kind: Kind::Thermal,
            mat: *fea_material("").unwrap(),
            h: 1.0,
            auto: false,
            volume: 8.0,
            mesh: Mesh::default(),
            solve,
        };
        let transient = job(Solve::Transient(input.clone()));
        let steady = job(Solve::Thermal(input.thermal));
        let result = SimResult {
            frames: vec![Frame {
                label: "1 s".into(),
                time: Some(1.0),
                values: vec![Values::exact(vec![25.0])],
            }],
            ..Default::default()
        };
        let c = Control::new();
        c.cancel();
        let partial = transient
            .check_cancelled(&c, result.clone())
            .expect("完成したコマを保持する");
        assert!(partial.partial);
        assert_eq!(partial.frames, result.frames);
        assert!(steady.check_cancelled(&c, result).is_err());
    }

    /// 確かめること: 過渡専用の式は入力として宣言され、時刻の列も名前変更に追従する。
    #[test]
    fn transient_field_expressions_declare_parameters_and_rename_time_columns() {
        let mut study = Study { kind: Kind::Thermal, data: from(&json!({
            "time":"transient", "end":"duration", "frames":"2 * nframes", "initial":"room + 5",
            "heats":[{"faces":{"kind":"face","feature":1,"face":"end"},"value":"power","from":"heat_start","until":"heat_end"}]
        })).unwrap() };
        let inputs = study.inputs().unwrap();
        for key in [
            "duration",
            "nframes",
            "room",
            "power",
            "heat_start",
            "heat_end",
        ] {
            assert!(
                inputs
                    .iter()
                    .any(|i| matches!(i, InputRef::Param { name } if name == key)),
                "{key}"
            );
        }
        for (old, new) in [
            ("duration", "run_time"),
            ("heat_start", "start_at"),
            ("heat_end", "finish_at"),
        ] {
            study.data = from(&study.rename_param(old, new).unwrap().unwrap()).unwrap();
        }
        assert_eq!(study.data.end, "run_time");
        assert_eq!(study.data.heats[0].from, "start_at");
        assert_eq!(study.data.heats[0].until, "finish_at");
        study.data.time.clear();
        let steady_inputs = study.inputs().unwrap();
        for key in ["run_time", "nframes", "room", "start_at", "finish_at"] {
            assert!(
                !steady_inputs
                    .iter()
                    .any(|i| matches!(i, InputRef::Param { name } if name == key)),
                "{key}"
            );
        }
        assert!(
            study
                .rename_param("run_time", "duration")
                .unwrap()
                .is_none()
        );
    }

    /// 自動の大きさ: 厚さ 1 mm の板(200×200)は、体積だけで決めた大きさでは薄い所で要素が
    /// 上限を超える(失敗した時に h の 0.8・0.64 倍と小さい方で作り直すやり方では、3 回とも
    /// 失敗する)。大きさの場から見込んで、収まる大きさまで大きくする。
    #[test]
    fn auto_size_grows_until_a_thin_plate_fits() {
        let mesh = crate::validation_tests::box_mesh([200.0, 200.0, 1.0]);
        let volume = 40_000.0;
        let h = size_for(volume, Kind::Static.target());
        let job = Job {
            kind: Kind::Static,
            mat: *fea_material("").unwrap(),
            h,
            auto: true,
            volume,
            mesh,
            solve: Solve::Static(StaticInput {
                e: 69_000.0,
                nu: 0.33,
                fixed: vec!["x0".into()],
                loads: Vec::new(),
            }),
        };
        let before = estimate_tets(&job.mesh, h).unwrap();
        assert!(
            before > MAX_TETS as f64,
            "体積だけの大きさ {h} の見込み {before}"
        );
        let grown = job.auto_size(&Control::new()).unwrap();
        let after = estimate_tets(&job.mesh, grown).unwrap();
        eprintln!("大きさ {h:.2} → {grown:.2}、見込み {before:.0} → {after:.0}");
        assert!(
            grown > h && after <= 0.9 * MAX_TETS as f64,
            "{grown} {after}"
        );
    }
}
