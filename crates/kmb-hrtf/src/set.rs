//! HRIR の集まりの型。座標は SOFA の球座標の決まり 1 つにそろえる(方位は前から左回りで 90° が左、
//! 仰角は上が正、距離は m)。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::HrtfError;

/// 音源の方向。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dir {
    /// 方位(度、前から左回り)。
    pub az: f64,
    /// 仰角(度、上が正)。
    pub el: f64,
    /// 距離(m)。遠方の時は測った距離か 0。
    pub dist: f64,
}

/// 方向の格子の種類。処理は方向の並びだけで動き、種類は記録と書き出しの説明に使う。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Grid {
    /// 輪の格子(`crate::grid::Rings`)。
    Rings { step: f64 },
    /// 仰角が Gauss–Legendre の点、方位が等間隔の格子。
    GaussLegendre { el: usize, az: usize },
    /// 決まりの無い並び。
    Any,
}

/// IR の置き方。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frame {
    /// 普通の時間の並び。立ち上がりより前は IR の頭にある。
    Linear,
    /// 長さ F の輪の枠。立ち上がりを 0 番に置き、それより前の分は枠の終わりへ回っている。
    /// 半スペクトルの表の 1 項目と同じ置き方。
    Circular,
}

/// 左右の HRIR の集まり。鳴らす時の音は「`ir` を `delay` sample 遅らせた物」。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HrirSet {
    /// サンプリング周波数(Hz)。
    pub fs: f64,
    /// 方向ごとの音源の位置。
    pub dirs: Vec<Dir>,
    /// 方向ごとの左・右の HRIR。長さはすべて同じ。
    pub ir: Vec<[Vec<f64>; 2]>,
    /// 方向ごとの左・右の遅れ(sample、小数も可、負も可)。
    pub delay: Vec<[f64; 2]>,
    /// 方向ごとの左・右の立ち上がりの位置(IR の中の sample)。まだ見つけていなければ `None`。
    pub onset: Option<Vec<[f64; 2]>>,
    pub frame: Frame,
    pub grid: Grid,
    /// 元のデータの属性(題・出典・作者・許諾など)。知らない鍵も捨てずに保ち、書き出しに写す。
    pub meta: BTreeMap<String, String>,
    /// 通した処理の記録(1 段ごとに 1 行)。
    pub history: Vec<String>,
}

impl HrirSet {
    /// IR の長さ(tap)。
    pub fn taps(&self) -> usize {
        self.ir.first().map_or(0, |e| e[0].len())
    }

    /// 長さと値の決まりを確かめる。処理の段は入る時にこれを通す。
    pub fn check(&self) -> Result<(), HrtfError> {
        if !(self.fs.is_finite() && self.fs > 0.0) {
            return Err(HrtfError::input(format!(
                "サンプリング周波数 {} Hz が正の数でない",
                self.fs
            )));
        }
        let m = self.dirs.len();
        if m == 0 {
            return Err(HrtfError::input("方向が 1 つも無い"));
        }
        if self.ir.len() != m || self.delay.len() != m {
            return Err(HrtfError::input(format!(
                "方向 {m} 個に対して IR が {} 組、遅れが {} 組ある",
                self.ir.len(),
                self.delay.len()
            )));
        }
        if let Some(o) = &self.onset
            && o.len() != m
        {
            return Err(HrtfError::input(format!(
                "方向 {m} 個に対して立ち上がりが {} 組ある",
                o.len()
            )));
        }
        let taps = self.taps();
        if taps == 0 {
            return Err(HrtfError::input("IR の長さが 0"));
        }
        for (i, pair) in self.ir.iter().enumerate() {
            for ear in pair {
                if ear.len() != taps {
                    return Err(HrtfError::input(format!(
                        "方向 {i} の IR の長さ {} が、最初の方向の長さ {taps} と違う",
                        ear.len()
                    )));
                }
                if ear.iter().any(|v| !v.is_finite()) {
                    return Err(HrtfError::input(format!(
                        "方向 {i} の IR に無限大か非数がある"
                    )));
                }
            }
        }
        let finite = |v: &[[f64; 2]]| v.iter().flatten().all(|x| x.is_finite());
        if !finite(&self.delay) || !self.onset.as_deref().is_none_or(finite) {
            return Err(HrtfError::input("遅れか立ち上がりに無限大か非数がある"));
        }
        for d in &self.dirs {
            if !(d.az.is_finite() && d.el.is_finite() && (-90.0..=90.0).contains(&d.el)) {
                return Err(HrtfError::input(format!(
                    "方向(方位 {}°、仰角 {}°)が球座標の範囲に無い",
                    d.az, d.el
                )));
            }
        }
        Ok(())
    }

    /// 鳴らす時の立ち上がり(遅れ + IR の中の立ち上がり)。立ち上がりが無ければ `None`。
    pub fn played_onset(&self) -> Option<Vec<[f64; 2]>> {
        let o = self.onset.as_ref()?;
        Some(
            self.delay
                .iter()
                .zip(o)
                .map(|(d, o)| [d[0] + o[0], d[1] + o[1]])
                .collect(),
        )
    }

    /// 2 つの集まりの方向が同じ並びか(方位は 360° で回して比べる)。
    pub fn same_dirs(&self, other: &HrirSet) -> bool {
        self.dirs.len() == other.dirs.len()
            && self.dirs.iter().zip(&other.dirs).all(|(a, b)| {
                // 極では方位に意味が無いので仰角だけで比べる。
                let pole = a.el.abs() >= 90.0 - 1e-9;
                let da = (a.az - b.az).rem_euclid(360.0);
                (pole || da.min(360.0 - da) < 1e-6) && (a.el - b.el).abs() < 1e-6
            })
    }
}
