//! 境界要素法の仕事と結果の型。どれも JSON で往復できる(複素数は `[実部, 虚部]`)。

use num_complex::Complex64;
use serde::{Deserialize, Serialize};

use crate::mesh::BemMesh;

/// 外の空間の音を解く仕事 1 つ。周波数ごとに独立に解く。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BemJob {
    /// 物体の表面(閉じた三角形の網、m、法線は外向き)
    pub mesh: BemMesh,
    /// 面の組ごとの境界の種類。三角形の組は [`BemMesh::groups`] の番号で引く。
    /// 足りない組と、組の無い網は剛い面
    #[serde(default)]
    pub boundaries: Vec<Boundary>,
    /// 外から来る音(散乱を解く時)。無ければ振動する面からの放射だけ
    #[serde(default)]
    pub sources: Vec<Source>,
    /// 解く周波数(Hz、正の数)
    pub frequencies: Vec<f64>,
    /// 音圧を出す所
    #[serde(default)]
    pub receivers: Receivers,
    /// `receivers` とは別に音圧を出す外の空間の点(m。断面の格子など)。値は来た音と散った音の和
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_points: Vec<[f64; 3]>,
    /// 空気の値
    #[serde(default)]
    pub medium: Medium,
    /// 解き方の設定
    #[serde(default)]
    pub solver: Solver,
}

/// 面の境界の種類。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Boundary {
    /// 動かず音を通さない面
    Rigid,
    /// 法線の向き(外向き)に振動する面。速さは複素の振幅(m/s)
    Velocity { velocity: Complex64 },
    /// 音を吸う面(局所作用)。比音響アドミタンス(空気の ρc で割った、単位の無い数)。
    /// 1 なら面に向かう平面波を返さない
    Admittance { admittance: Complex64 },
}

/// 外から来る音。音圧の振幅は複素数(Pa)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// 平面波。`direction` は波の進む向き(長さは問わない)、振幅は原点での音圧
    PlaneWave {
        direction: [f64; 3],
        amplitude: Complex64,
    },
    /// 点音源(単極)。振幅は 1 m 離れた所の音圧
    Point {
        position: [f64; 3],
        amplitude: Complex64,
    },
}

/// 音圧を出す所。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Receivers {
    /// 表面の音圧だけ
    #[default]
    None,
    /// 外の空間の点(m)。音圧は来た音と散った音の和
    Points { points: Vec<[f64; 3]> },
    /// 原点から見た方向(長さは問わない)。`distance` が無ければ遠方で、値は散った音・出した音の
    /// 遠方の振幅 F(Pa·m。音圧は F·e^{−jkr}/r)。距離があればその点の音圧(来た音との和)
    Directions {
        directions: Vec<[f64; 3]>,
        #[serde(default)]
        distance: Option<f64>,
    },
}

/// 空気の値。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Medium {
    /// 音速(m/s)
    pub speed_of_sound: f64,
    /// 密度(kg/m³)
    pub density: f64,
}

impl Default for Medium {
    fn default() -> Self {
        Self {
            speed_of_sound: 343.0,
            density: 1.204,
        }
    }
}

impl Medium {
    /// 温度(℃)と 1 気圧の乾いた空気から作る。
    pub fn from_temperature(celsius: f64) -> Self {
        let kelvin = celsius + 273.15;
        Self {
            speed_of_sound: 331.3 * (kelvin / 273.15).sqrt(),
            density: 101_325.0 / (287.058 * kelvin),
        }
    }
}

/// 解き方の設定。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Solver {
    /// 内部の共振の周波数で解が乱れるのを、法線で微分した式との結合(Burton–Miller)で消す。
    /// 外すのは確かめの時だけ
    pub burton_miller: bool,
    /// 反復の許す残り(右辺の大きさに対する比)
    pub tolerance: f64,
    /// 反復の上限(再始動を含めた全部)
    pub max_iterations: usize,
    /// 再始動までの反復の数
    pub restart: usize,
    /// 行列に使ってよいメモリ(バイト)。超えるなら解き始める前に止める
    pub max_matrix_bytes: u64,
}

impl Default for Solver {
    fn default() -> Self {
        Self {
            burton_miller: true,
            tolerance: 1e-4,
            max_iterations: 1000,
            restart: 100,
            max_matrix_bytes: 2 << 30,
        }
    }
}

/// 仕事の結果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BemOut {
    /// 周波数ごとの結果(仕事の並びの順)
    pub frequencies: Vec<FrequencyOut>,
    /// 中止されて、途中の周波数までで終えたか
    pub cancelled: bool,
}

/// 周波数 1 つの結果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrequencyOut {
    /// 周波数(Hz)
    pub frequency: f64,
    /// 三角形ごとの表面の音圧(Pa、来た音との和)
    pub surface: Vec<Complex64>,
    /// 受け取る所ごとの値([`Receivers`] の並びの順)
    pub receivers: Vec<Complex64>,
    /// [`BemJob::field_points`] の点ごとの音圧(Pa、同じ並び)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field: Vec<Complex64>,
    /// 反復の回数
    pub iterations: usize,
    /// 終わった時の残り(右辺の大きさに対する比)
    pub residual: f64,
    /// 一番長い辺の 1 本に入る波長の割合の逆数(1 波長に入る要素の数の目安)。6 を下回ると粗い
    pub elements_per_wavelength: f64,
    /// かかった時間(s)
    pub seconds: f64,
}

/// 誤りの種類。研究の機能が `Problem` の種類へ写す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveErrorKind {
    /// 仕事の中身が正しくない(閉じていない網、負の周波数など)
    Input,
    /// 解けなかった(反復が収まらない)
    Compute,
    /// この大きさは解けない(メモリの上限を超える)
    Unavailable,
}

/// 種類つきの誤り。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaveError {
    pub kind: WaveErrorKind,
    pub message: String,
}

impl WaveError {
    pub fn input(message: impl Into<String>) -> Self {
        Self {
            kind: WaveErrorKind::Input,
            message: message.into(),
        }
    }
    pub fn compute(message: impl Into<String>) -> Self {
        Self {
            kind: WaveErrorKind::Compute,
            message: message.into(),
        }
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: WaveErrorKind::Unavailable,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for WaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WaveError {}
