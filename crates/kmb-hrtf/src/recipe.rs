//! 処理の手順。JSON の段の並びを、名前つきの集まりに上から順にかける。画面・CLI・MCP は同じ手順を
//! 同じ入口(`run`)で流す。知らない段・知らない欄は誤りにする(黙って飛ばさない)。
//!
//! ```json
//! {"steps": [
//!   {"op": "onset", "set": "b"},
//!   {"op": "onset", "set": "a"},
//!   {"op": "combine", "magnitude": "a", "phase": "b", "out": "ab"},
//!   {"op": "shift", "set": "ab", "from": "b", "add": 3},
//!   {"op": "trim", "set": "ab", "before": 4, "after": 128, "rolloff_hz": 20000},
//!   {"op": "table", "sets": ["ab"], "lead": 4, "before": 4}
//! ]}
//! ```
//!
//! 格子とサンプリング周波数のそろえ方と SOFA の書き出し:
//!
//! ```json
//! {"steps": [
//!   {"op": "resample", "set": "d1", "fs": 48000},
//!   {"op": "onset", "set": "d1"},
//!   {"op": "regrid", "set": "d1", "to": {"kind": "gauss_legendre", "el": 89, "az": 180}},
//!   {"op": "write", "set": "d1", "name": "d1-48k.sofa"}
//! ]}
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dsp::{MinPhase, OnsetFinder, linear_from_circular, rolloff, trim_window};
use crate::fft::{C, Fft, advance};
use crate::grid::Rings;
use crate::regrid::{Method, Pole, Target, gauss_legendre, regrid, resample};
use crate::sofa::{DelayMode, SofaSpec};
use crate::table::{
    OutFile, TableSpec, default_frame, default_onset_method, default_onset_step,
    default_spectrum_step,
};
use crate::{Dir, Frame, Grid, HrirSet, HrtfError};

/// 処理の手順。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// 読む物(名前 → ファイルのパス)。読むのは呼ぶ側(操作)で、手順を流す `run` は使わない。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, String>,
    pub steps: Vec<Step>,
}

fn yes() -> bool {
    true
}

fn default_cutoff() -> f64 {
    0.95
}

/// 処理の 1 段。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// 立ち上がりを見つける。`remove` なら IR から外して遅れへ移し、長さ `frame` の輪の枠に置き直す
    /// (立ち上がりを 0 番に置き、前の分は枠の終わりへ回る)。
    Onset {
        set: String,
        #[serde(default = "yes")]
        remove: bool,
        #[serde(default = "default_frame")]
        frame: usize,
    },
    /// 大きさを `magnitude` から、時間(超過位相と遅れ)を `phase` から取って 1 つにし、`out` に置く。
    /// 結果のスペクトルは、大きさが |A|、位相が「A の最小位相の位相 + B の位相 − B の最小位相の位相」
    /// (A = `magnitude`、B = `phase`。最小位相は長さ F/2 に切った物)。方向の並びとサンプリング周波数が
    /// 違えば誤り。
    Combine {
        magnitude: String,
        phase: String,
        out: String,
        #[serde(default = "default_frame")]
        frame: usize,
    },
    /// 鳴らす時の立ち上がりを決め直す。`from` があればその集まりの立ち上がり + `add`、無ければ今の
    /// 遅れに `add` を足す(sample)。
    Shift {
        set: String,
        #[serde(default)]
        from: Option<String>,
        #[serde(default)]
        add: f64,
    },
    /// 輪の枠の IR を、立ち上がりの前 `before`・後ろ `after` tap だけ残す。端は半ハン窓でならし
    /// (既定は前 min(before, 8) 点・後ろ max(after / 8, 4) 点)、`rolloff_hz` があればそこから
    /// ナイキストまで高い所を 0 へならす。
    Trim {
        set: String,
        before: usize,
        after: usize,
        #[serde(default)]
        fade_before: Option<usize>,
        #[serde(default)]
        fade_after: Option<usize>,
        #[serde(default)]
        rolloff_hz: Option<f64>,
    },
    /// 方向の格子を替える(`regrid` の説明を見る)。`out` が無ければ `set` を置き換える。
    Regrid {
        set: String,
        to: Target,
        #[serde(default)]
        method: Method,
        /// 輪の双線形での極の取り方(`regrid::Pole`)。
        #[serde(default)]
        pole: Pole,
        #[serde(default)]
        out: Option<String>,
    },
    /// サンプリング周波数を `fs` Hz に替える。帯域は低い方のナイキストの `cutoff` 倍まで残す。
    /// 輪の枠には使えない(onset の段より前に置く)。
    Resample {
        set: String,
        fs: f64,
        #[serde(default = "default_cutoff")]
        cutoff: f64,
    },
    /// SOFA(SimpleFreeFieldHRIR)に書き出す。`delay` は遅れを IR に入れる(`bake`、既定)か、方向ごとの
    /// `Data.Delay` に分ける(`keep`)か。名前が無ければ「集まりの名前.sofa」。
    Write {
        set: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        delay: DelayMode,
    },
    /// 時間差を除いた半スペクトルと遅延の表(分けた表)を書き出す。
    /// 欄の意味は `TableSpec` と同じ。
    Table {
        sets: Vec<String>,
        #[serde(default = "default_frame")]
        frame: usize,
        #[serde(default = "default_spectrum_step")]
        spectrum_step: f64,
        #[serde(default = "default_onset_step")]
        onset_step: f64,
        #[serde(default)]
        before: f64,
        #[serde(default)]
        lead: Option<f64>,
        #[serde(default = "default_onset_method")]
        onset_method: Method,
    },
}

impl Step {
    fn name(&self) -> &'static str {
        match self {
            Step::Onset { .. } => "onset",
            Step::Combine { .. } => "combine",
            Step::Shift { .. } => "shift",
            Step::Trim { .. } => "trim",
            Step::Table { .. } => "table",
            Step::Regrid { .. } => "regrid",
            Step::Resample { .. } => "resample",
            Step::Write { .. } => "write",
        }
    }
}

impl Recipe {
    /// JSON の文字から読む。
    pub fn from_json(text: &str) -> Result<Recipe, HrtfError> {
        serde_json::from_str(text)
            .map_err(|e| HrtfError::input(format!("手順の JSON を読めない: {e}")))
    }
}

fn frame_ok(frame: usize) -> Result<(), HrtfError> {
    if frame.is_power_of_two() && (16..=65536).contains(&frame) {
        Ok(())
    } else {
        Err(HrtfError::input(format!(
            "枠の長さ {frame} は 16〜65536 の 2 の累乗にする"
        )))
    }
}

fn get<'a>(sets: &'a BTreeMap<String, HrirSet>, name: &str) -> Result<&'a HrirSet, HrtfError> {
    let s = sets
        .get(name)
        .ok_or_else(|| HrtfError::input(format!("集まり {name} が無い")))?;
    s.check().map_err(|e| e.at(&format!("集まり {name}")))?;
    Ok(s)
}

/// 手順の外から渡す値。処理は時計を読まないので、書き出しの日時は呼ぶ側が渡す。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunOptions {
    /// 書き出しの日時(UTC、`yyyy-mm-dd HH:MM:SS`)。
    pub now: Option<String>,
}

/// 手順を流す。集まりは名前で置き換わり、書き出すファイルを返す(置くのは呼ぶ側)。
pub fn run(
    recipe: &Recipe,
    sets: &mut BTreeMap<String, HrirSet>,
) -> Result<Vec<OutFile>, HrtfError> {
    run_with(recipe, sets, &RunOptions::default())
}

/// `run` に、手順の外から渡す値を足した物。
pub fn run_with(
    recipe: &Recipe,
    sets: &mut BTreeMap<String, HrirSet>,
    opts: &RunOptions,
) -> Result<Vec<OutFile>, HrtfError> {
    let mut files = Vec::new();
    for (i, step) in recipe.steps.iter().enumerate() {
        let place = format!("段 {}({})", i + 1, step.name());
        apply(step, sets, &mut files, opts).map_err(|e| e.at(&place))?;
    }
    Ok(files)
}

fn apply(
    step: &Step,
    sets: &mut BTreeMap<String, HrirSet>,
    files: &mut Vec<OutFile>,
    opts: &RunOptions,
) -> Result<(), HrtfError> {
    match step {
        Step::Regrid {
            set,
            to,
            method,
            pole,
            out,
        } => {
            let s = get(sets, set)?;
            let dist = s.dirs[0].dist;
            let (dirs, grid) = match to {
                Target::GaussLegendre { el, az } => (
                    gauss_legendre(*el, *az)?
                        .into_iter()
                        .map(|(az, el)| Dir { az, el, dist })
                        .collect(),
                    Grid::GaussLegendre { el: *el, az: *az },
                ),
                Target::Rings { step } => (
                    Rings::new(*step)?
                        .dirs()
                        .into_iter()
                        .map(|(az, el)| Dir { az, el, dist })
                        .collect(),
                    Grid::Rings { step: *step },
                ),
                Target::Like { set: other } => {
                    let o = get(sets, other)?;
                    (o.dirs.clone(), o.grid)
                }
            };
            let r = regrid(s, &dirs, grid, *method, *pole)?;
            sets.insert(out.clone().unwrap_or_else(|| set.clone()), r);
        }
        Step::Resample { set, fs, cutoff } => {
            let r = resample(get(sets, set)?, *fs, *cutoff)?;
            sets.insert(set.clone(), r);
        }
        Step::Write { set, name, delay } => {
            let s = get(sets, set)?;
            let spec = SofaSpec {
                delay: *delay,
                now: opts.now.clone(),
            };
            let name = name.clone().unwrap_or_else(|| format!("{set}.sofa"));
            if name.contains(['/', '\\']) || name.starts_with('.') {
                return Err(HrtfError::input(format!(
                    "書き出しの名前「{name}」にフォルダの区切りや頭の . を使わない"
                )));
            }
            files.push(OutFile {
                name,
                bytes: crate::sofa::write(s, &spec)?,
            });
        }
        Step::Onset { set, remove, frame } => {
            frame_ok(*frame)?;
            let s = get(sets, set)?;
            if s.taps() > *frame {
                return Err(HrtfError::input(format!(
                    "IR の長さ {} が枠の長さ {frame} を超える",
                    s.taps()
                )));
            }
            let finder = OnsetFinder::new(*frame, s.fs);
            let fft = Fft::new(*frame);
            let mut out = s.clone();
            let found: Vec<[f64; 2]> =
                s.ir.iter()
                    .map(|pair| [finder.find(&pair[0]), finder.find(&pair[1])])
                    .collect();
            if *remove {
                for ((ir, d), t) in out.ir.iter_mut().zip(out.delay.iter_mut()).zip(&found) {
                    for ear in 0..2 {
                        ir[ear] = advance(&fft, &ir[ear], t[ear]);
                        d[ear] += t[ear];
                    }
                }
                out.onset = Some(vec![[0.0, 0.0]; found.len()]);
                out.frame = Frame::Circular;
                out.history
                    .push(format!("onset: 立ち上がりを外して遅れへ移した(枠 {frame})"));
            } else {
                out.onset = Some(found);
                out.history.push("onset: 立ち上がりを見つけた".into());
            }
            sets.insert(set.clone(), out);
        }
        Step::Combine {
            magnitude,
            phase,
            out,
            frame,
        } => {
            frame_ok(*frame)?;
            let a = get(sets, magnitude)?;
            let b = get(sets, phase)?;
            if a.fs != b.fs {
                return Err(HrtfError::input(format!(
                    "サンプリング周波数が違う({magnitude} は {} Hz、{phase} は {} Hz。先にそろえる)",
                    a.fs, b.fs
                )));
            }
            if !a.same_dirs(b) {
                return Err(HrtfError::input(format!(
                    "{magnitude} と {phase} の方向の並びが違う(先に格子をそろえる)"
                )));
            }
            if a.taps() > *frame || b.taps() > *frame {
                return Err(HrtfError::input(format!(
                    "IR の長さが枠の長さ {frame} を超える"
                )));
            }
            let mp = MinPhase::new(*frame);
            let fft = Fft::new(*frame);
            let keep = frame / 2;
            let mut ir = Vec::with_capacity(a.ir.len());
            for (pa, pb) in a.ir.iter().zip(&b.ir) {
                let pair: [Vec<f64>; 2] = std::array::from_fn(|ear| {
                    let min_phase = |s: &HrirSet, ir: &[f64]| {
                        let lin = if s.frame == Frame::Circular {
                            linear_from_circular(ir)
                        } else {
                            ir.to_vec()
                        };
                        fft.spectrum(&mp.ir(&lin, keep))
                    };
                    let ma = min_phase(a, &pa[ear]);
                    let mb = min_phase(b, &pb[ear]);
                    let ha = fft.spectrum(&pa[ear]);
                    let hb = fft.spectrum(&pb[ear]);
                    let h: Vec<C> = (0..*frame)
                        .map(|k| {
                            let phase = ma[k].arg() + hb[k].arg() - mb[k].arg();
                            C::from_polar(ha[k].norm(), phase)
                        })
                        .collect();
                    fft.real(h)
                });
                ir.push(pair);
            }
            let mut c = a.clone();
            c.ir = ir;
            c.delay = b.delay.clone();
            c.onset = b.onset.clone();
            c.frame = if b.taps() == *frame {
                b.frame
            } else {
                Frame::Linear
            };
            let title = |s: &HrirSet| s.meta.get("Title").cloned().unwrap_or_default();
            c.history.push(format!(
                "combine: 大きさは {magnitude}({})、超過位相と遅れは {phase}({}、許諾 {})",
                title(a),
                title(b),
                b.meta.get("License").map_or("不明", |s| s)
            ));
            sets.insert(out.clone(), c);
        }
        Step::Shift { set, from, add } => {
            if !add.is_finite() {
                return Err(HrtfError::input("add が数でない"));
            }
            let s = get(sets, set)?;
            let mut out = s.clone();
            match from {
                Some(f) => {
                    let src = get(sets, f)?;
                    if !s.same_dirs(src) {
                        return Err(HrtfError::input(format!("{set} と {f} の方向の並びが違う")));
                    }
                    let target = src.played_onset().ok_or_else(|| {
                        HrtfError::input(format!("{f} に立ち上がりが無い(先に onset の段を流す)"))
                    })?;
                    let own = s.onset.as_ref().ok_or_else(|| {
                        HrtfError::input(format!("{set} に立ち上がりが無い(先に onset の段を流す)"))
                    })?;
                    for ((d, t), o) in out.delay.iter_mut().zip(&target).zip(own) {
                        for ear in 0..2 {
                            d[ear] = t[ear] + add - o[ear];
                        }
                    }
                    out.history.push(format!(
                        "shift: 立ち上がりを {f} の立ち上がり + {add} sample にした"
                    ));
                }
                None => {
                    for d in out.delay.iter_mut().flatten() {
                        *d += add;
                    }
                    out.history
                        .push(format!("shift: 遅れに {add} sample 足した"));
                }
            }
            sets.insert(set.clone(), out);
        }
        Step::Trim {
            set,
            before,
            after,
            fade_before,
            fade_after,
            rolloff_hz,
        } => {
            let s = get(sets, set)?;
            if s.frame != Frame::Circular {
                return Err(HrtfError::input(
                    "輪の枠でない(先に onset の段で立ち上がりを外す)",
                ));
            }
            let f = s.taps();
            frame_ok(f)?;
            if *after == 0 || before + after > f {
                return Err(HrtfError::input(format!(
                    "残す長さ(前 {before}・後ろ {after})は後ろを 1 以上、合わせて枠の長さ {f} 以下にする"
                )));
            }
            let fb = fade_before.unwrap_or((*before).min(8));
            let fa = fade_after.unwrap_or((after / 8).max(4));
            let w = trim_window(f, *before, *after, fb, fa);
            let ro = match rolloff_hz {
                Some(hz) if !(hz.is_finite() && *hz > 0.0 && *hz < s.fs / 2.0) => {
                    return Err(HrtfError::input(format!(
                        "rolloff_hz {hz} は 0 より大きくナイキスト {} Hz より小さくする",
                        s.fs / 2.0
                    )));
                }
                Some(hz) => Some(rolloff(f, s.fs, *hz)),
                None => None,
            };
            let fft = Fft::new(f);
            let mut out = s.clone();
            for pair in out.ir.iter_mut() {
                for ir in pair.iter_mut() {
                    let g: Vec<f64> = ir.iter().zip(&w).map(|(a, b)| a * b).collect();
                    *ir = match &ro {
                        None => g,
                        Some(ro) => {
                            let mut sp = fft.spectrum(&g);
                            for (k, v) in sp.iter_mut().enumerate() {
                                *v *= ro[if k <= f / 2 { k } else { f - k }];
                            }
                            fft.real(sp)
                        }
                    };
                }
            }
            out.history.push(format!(
                "trim: 立ち上がりの前 {before}・後ろ {after} tap を残した(縁 {fb}・{fa} 点{})",
                rolloff_hz.map_or(String::new(), |hz| format!("、{hz} Hz から上をならした"))
            ));
            sets.insert(set.clone(), out);
        }
        Step::Table {
            sets: names,
            frame,
            spectrum_step,
            onset_step,
            before,
            lead,
            onset_method,
        } => {
            let spec = TableSpec {
                frame: *frame,
                spectrum_step: *spectrum_step,
                onset_step: *onset_step,
                before: *before,
                lead: *lead,
                onset_method: *onset_method,
            };
            let list: Vec<(&str, &HrirSet)> = names
                .iter()
                .map(|n| get(sets, n).map(|s| (n.as_str(), s)))
                .collect::<Result<_, _>>()?;
            files.extend(crate::table::write(&list, &spec)?);
        }
    }
    Ok(())
}
