//! 時間差を除いた半スペクトルと遅延の表(分けた表)の書き出しと読み込み。
//!
//! 1 つのフォルダに、いくつかの集まり(番号 d)を同じ格子で並べる。すべて little-endian。
//!
//! | ファイル | 中身 |
//! |---|---|
//! | `spectrum-<d>.f16` | 半スペクトルの格子の方向ごとに、番号 0..=F/2 の周波数の (左の実部, 左の虚部, 右の実部, 右の虚部) を float16 |
//! | `onset-<d>.f16` | 遅延の格子の方向ごとに (左, 右) の立ち上がり(sample)を float16。幅 1024 の 2 次元に並べ、余りは 0 |
//! | `rings.f32` | 2 行 × 輪の数 × (方向の数, 最初の番号) を float32。0 行目が半スペクトルの格子、1 行目が遅延の格子 |
//! | `layout.json` | 大きさ・格子の刻み・集まりの名前・共通の遅れ |
//!
//! 1 項目は、鳴らす音のスペクトル H から自分の立ち上がり τ を除いた H · e^{+j 2π k τ / F}。鳴らす時は
//! 最近傍の項目に e^{−j 2π k τ / F}(τ は遅延の表から最近傍で引いた値)を掛ける。HRIR は実数の列で
//! 持つので、ナイキストの項は小数の立ち上がりの位相を持てず実部だけになる(聞こえない 24 kHz の 1 点)。

use serde::{Deserialize, Serialize};

use crate::fft::{C, Fft};
use crate::grid::{Nearest, Rings};
use crate::half::{f16_bits, f16_value};
use crate::{Frame, HrirSet, HrtfError};

/// 遅延の表を 2 次元に並べる時の幅。
pub const ONSET_WIDTH: usize = 1024;

/// 書き出す 1 つのファイル(名前とバイト列)。置くのは呼ぶ側(壊れない書き込みを通す)。
#[derive(Clone, Debug, PartialEq)]
pub struct OutFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// 表の作り方。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSpec {
    /// FFT の長さ F。
    #[serde(default = "default_frame")]
    pub frame: usize,
    /// 半スペクトルの格子の刻み(度)。
    #[serde(default = "default_spectrum_step")]
    pub spectrum_step: f64,
    /// 遅延の格子の刻み(度)。
    #[serde(default = "default_onset_step")]
    pub onset_step: f64,
    /// 共通の遅れを引く時の、項目の立ち上がりの前に残した tap の数。
    #[serde(default)]
    pub before: f64,
    /// 共通の遅れを引く時に、鳴らす音の頭に空けておく tap の数。空なら共通の遅れを引かない。
    /// 引く量は C = max(floor(最小の τ) − before − lead, 0)(整数なので float16 の値を崩さない)。
    #[serde(default)]
    pub lead: Option<f64>,
}

pub(crate) fn default_frame() -> usize {
    512
}
pub(crate) fn default_spectrum_step() -> f64 {
    2.0
}
pub(crate) fn default_onset_step() -> f64 {
    0.5
}

impl Default for TableSpec {
    fn default() -> TableSpec {
        TableSpec {
            frame: default_frame(),
            spectrum_step: default_spectrum_step(),
            onset_step: default_onset_step(),
            before: 0.0,
            lead: None,
        }
    }
}

/// `layout.json` の中身。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub fft: usize,
    pub bins: usize,
    pub rate: f64,
    pub datasets: usize,
    pub names: Vec<String>,
    pub spectrum_rows: usize,
    pub spectrum_step: f64,
    pub onset_entries: usize,
    pub onset_step: f64,
    pub onset_width: usize,
    pub onset_height: usize,
    pub ring_width: usize,
    /// 集まりごとに遅延の表から引いた共通の遅れ(sample)。
    pub common_delay: Vec<f64>,
}

/// 1 つの集まりの表(float16 に丸める前の値)。
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    /// 方向ごと・周波数ごとの (左, 右)。
    pub spectrum: Vec<Vec<[C; 2]>>,
    /// 方向ごとの (左, 右) の立ち上がり。
    pub onset: Vec<[f64; 2]>,
    pub common_delay: f64,
}

/// 集まりの 1 方向の項目(立ち上がりを除いた半スペクトル)と立ち上がり。
fn entry(set: &HrirSet, fft: &Fft, i: usize, onset: &[[f64; 2]]) -> [Vec<C>; 2] {
    let f = fft.len();
    std::array::from_fn(|ear| {
        let mut s = fft.spectrum(&set.ir[i][ear]);
        s.truncate(f / 2 + 1);
        let o = if set.frame == Frame::Circular {
            0.0
        } else {
            onset[i][ear]
        };
        for (k, v) in s.iter_mut().enumerate() {
            let a = 2.0 * std::f64::consts::PI * k as f64 * o / f as f64;
            *v *= C::new(a.cos(), a.sin());
        }
        s
    })
}

/// 集まりから表を作る。格子の方向ごとに最も近い方向の値を取る(補間はしない)。
pub fn build(set: &HrirSet, spec: &TableSpec) -> Result<Table, HrtfError> {
    set.check()?;
    let f = spec.frame;
    if !(f.is_power_of_two() && (16..=65536).contains(&f)) {
        return Err(HrtfError::input(format!(
            "FFT の長さ {f} は 16〜65536 の 2 の累乗にする"
        )));
    }
    match set.frame {
        Frame::Circular if set.taps() != f => {
            return Err(HrtfError::input(format!(
                "輪の枠の IR の長さ {} が FFT の長さ {f} と違う",
                set.taps()
            )));
        }
        Frame::Linear if set.taps() > f => {
            return Err(HrtfError::input(format!(
                "IR の長さ {} が FFT の長さ {f} を超える(先に trim で切る)",
                set.taps()
            )));
        }
        _ => {}
    }
    let onset = set
        .onset
        .as_ref()
        .ok_or_else(|| HrtfError::input("立ち上がりが無い(先に onset の段を流す)"))?;
    let played = set.played_onset().unwrap_or_default();
    let sg = Rings::new(spec.spectrum_step)?;
    let og = Rings::new(spec.onset_step)?;
    let near = Nearest::new(&set.dirs.iter().map(|d| (d.az, d.el)).collect::<Vec<_>>());
    let fft = Fft::new(f);
    let mut spectrum = Vec::with_capacity(sg.len());
    for (az, el) in sg.dirs() {
        let i = near.find(az, el).unwrap_or(0);
        let [l, r] = entry(set, &fft, i, onset);
        spectrum.push(l.into_iter().zip(r).map(|(a, b)| [a, b]).collect());
    }
    let mut onsets: Vec<[f64; 2]> = og
        .dirs()
        .into_iter()
        .map(|(az, el)| played[near.find(az, el).unwrap_or(0)])
        .collect();
    let mut common_delay = 0.0;
    if let Some(lead) = spec.lead {
        let lo = onsets
            .iter()
            .flatten()
            .cloned()
            .fold(f64::INFINITY, f64::min);
        common_delay = ((lo - spec.before).floor() - lead).max(0.0);
        for o in onsets.iter_mut().flatten() {
            *o -= common_delay;
        }
    }
    Ok(Table {
        spectrum,
        onset: onsets,
        common_delay,
    })
}

fn push_f16(out: &mut Vec<u8>, x: f64, what: &str) -> Result<(), HrtfError> {
    let h = f16_bits(x);
    if !f16_value(h).is_finite() {
        return Err(HrtfError::compute(format!(
            "{what}の値 {x} が float16 に収まらない"
        )));
    }
    out.extend_from_slice(&h.to_le_bytes());
    Ok(())
}

/// 名前つきの集まりを同じ格子の表にして、フォルダに置くファイルの並びにする。
pub fn write(sets: &[(&str, &HrirSet)], spec: &TableSpec) -> Result<Vec<OutFile>, HrtfError> {
    let Some((_, first)) = sets.first() else {
        return Err(HrtfError::input("表にする集まりが無い"));
    };
    if let Some((name, s)) = sets.iter().find(|(_, s)| s.fs != first.fs) {
        return Err(HrtfError::input(format!(
            "集まり {name} のサンプリング周波数 {} Hz が、最初の集まりの {} Hz と違う(先にそろえる)",
            s.fs, first.fs
        )));
    }
    let sg = Rings::new(spec.spectrum_step)?;
    let og = Rings::new(spec.onset_step)?;
    let height = og.len().div_ceil(ONSET_WIDTH);
    let mut files = Vec::new();
    let mut common = Vec::new();
    for (d, (name, set)) in sets.iter().enumerate() {
        let t = build(set, spec).map_err(|e| e.at(&format!("集まり {name}")))?;
        let mut bytes = Vec::with_capacity(t.spectrum.len() * (spec.frame / 2 + 1) * 8);
        for row in &t.spectrum {
            for [l, r] in row {
                for v in [l.re, l.im, r.re, r.im] {
                    push_f16(&mut bytes, v, "半スペクトル")?;
                }
            }
        }
        files.push(OutFile {
            name: format!("spectrum-{d}.f16"),
            bytes,
        });
        let mut bytes = Vec::with_capacity(height * ONSET_WIDTH * 4);
        for o in t.onset.iter().flatten() {
            push_f16(&mut bytes, *o, "立ち上がり")?;
        }
        bytes.resize(height * ONSET_WIDTH * 4, 0);
        files.push(OutFile {
            name: format!("onset-{d}.f16"),
            bytes,
        });
        common.push(t.common_delay);
    }
    let ring_width = sg.el.len().max(og.el.len());
    let mut rings = Vec::with_capacity(2 * ring_width * 2 * 4);
    for g in [&sg, &og] {
        for r in 0..ring_width {
            let (n, first) = match (g.n.get(r), g.first.get(r)) {
                (Some(&n), Some(&first)) => (n as f32, first as f32),
                _ => (0.0, 0.0),
            };
            rings.extend_from_slice(&n.to_le_bytes());
            rings.extend_from_slice(&first.to_le_bytes());
        }
    }
    files.push(OutFile {
        name: "rings.f32".into(),
        bytes: rings,
    });
    let layout = Layout {
        fft: spec.frame,
        bins: spec.frame / 2 + 1,
        rate: first.fs,
        datasets: sets.len(),
        names: sets.iter().map(|(n, _)| n.to_string()).collect(),
        spectrum_rows: sg.len(),
        spectrum_step: spec.spectrum_step,
        onset_entries: og.len(),
        onset_step: spec.onset_step,
        onset_width: ONSET_WIDTH,
        onset_height: height,
        ring_width,
        common_delay: common,
    };
    let json = serde_json::to_vec_pretty(&layout)
        .map_err(|e| HrtfError::compute(format!("layout.json を作れない: {e}")))?;
    files.push(OutFile {
        name: "layout.json".into(),
        bytes: json,
    });
    Ok(files)
}

/// 書き出した表の 1 つの集まりを読み戻す(確かめと比べ用)。
pub fn read(layout: &Layout, spectrum: &[u8], onset: &[u8]) -> Result<Table, HrtfError> {
    let bins = layout.bins;
    if spectrum.len() != layout.spectrum_rows * bins * 8 {
        return Err(HrtfError::input(format!(
            "半スペクトルの表が {} バイトで、{} 方向 × {bins} 点 × 8 バイトと合わない",
            spectrum.len(),
            layout.spectrum_rows
        )));
    }
    if onset.len() < layout.onset_entries * 4 {
        return Err(HrtfError::input("遅延の表が方向の数より短い"));
    }
    let h = |b: &[u8], i: usize| f16_value(u16::from_le_bytes([b[2 * i], b[2 * i + 1]]));
    let spectrum = (0..layout.spectrum_rows)
        .map(|r| {
            (0..bins)
                .map(|k| {
                    let i = (r * bins + k) * 4;
                    [
                        C::new(h(spectrum, i), h(spectrum, i + 1)),
                        C::new(h(spectrum, i + 2), h(spectrum, i + 3)),
                    ]
                })
                .collect()
        })
        .collect();
    let onset = (0..layout.onset_entries)
        .map(|i| [h(onset, 2 * i), h(onset, 2 * i + 1)])
        .collect();
    Ok(Table {
        spectrum,
        onset,
        common_delay: 0.0,
    })
}

/// 表から 1 つの方向の音を鳴らす時の半スペクトル(左, 右)。引き方は格子の決まりの最近傍。
pub fn play(table: &Table, layout: &Layout, az: f64, el: f64) -> Result<[Vec<C>; 2], HrtfError> {
    let sg = Rings::new(layout.spectrum_step)?;
    let og = Rings::new(layout.onset_step)?;
    let row = &table.spectrum[sg.lookup(az, el)];
    let tau = table.onset[og.lookup(az, el)];
    let f = layout.fft as f64;
    Ok(std::array::from_fn(|ear| {
        row.iter()
            .enumerate()
            .map(|(k, v)| {
                let a = -2.0 * std::f64::consts::PI * k as f64 * tau[ear] / f;
                v[ear] * C::new(a.cos(), a.sin())
            })
            .collect()
    }))
}
