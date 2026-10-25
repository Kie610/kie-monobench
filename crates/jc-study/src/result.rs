//! 解き方と見せ方の間で受け渡す結果の共通の形([`SimResult`])。
//!
//! 結果は「網(点と、点をつなぐ形)」「量(網の点ごとの値。スカラーかベクトル)」「コマ(時刻や
//! モードごとの値の組)」の 3 つでできている。解き方(強度・熱・流れ…)はこの形を作って返すだけで、
//! 色の塗り方・断面・流線・時刻の帯などの見せ方([`crate::view`])は、この形だけを読む。
//! だから、どの解き方の結果にも、網と量の種類が合う見せ方なら組み合わせられる
//! (表面の色はどの表面の量にも、断面と流線はどの格子の量にも)。
//!
//! 網の種類は 3 つ: 三角形の表面([`Geometry::Surface`])、等間隔の直交格子
//! ([`Geometry::Grid`]。流れ)、形に沿った 2 次の四面体の網([`Geometry::Tets`]。強度と熱)。

use std::sync::Arc;

/// 解いた結果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SimResult {
    /// 解き方の種類(static・thermal・modal・topology・wing・flow2d・flow3d)
    pub kind: String,
    /// 網(表面・格子)
    pub parts: Vec<Part>,
    /// 量の定義(値はコマごとに [`Frame::values`] に同じ順で入る)
    pub quantities: Vec<Quantity>,
    /// コマ(時刻・モード)。少なくとも 1 つ
    pub frames: Vec<Frame>,
    /// 要約の行(名前と値)
    pub numbers: Vec<(String, String)>,
    /// 一行の要約(設定欄とタイムラインのヒントに出す)
    pub summary: String,
    /// 結果の見方の注意
    pub note: String,
    /// 解き方の札(例: 「3D・格子 1.6 mm・Re 13,000」)。ビューの右上に出す
    pub badge: String,
    /// 推移のグラフ(抗力の推移など)
    pub series: Vec<jc_core::Series>,
    /// 解いたボディ(機能の番号)。画面はほかの見えるボディを灰色で一緒に描く
    pub bodies: Vec<u64>,
    /// 中止して、途中までのコマだけの結果か
    pub partial: bool,
    /// 最初に出す見せ方
    pub start: Start,
    /// 網の上の印(特異点の候補など)
    pub marks: Vec<Mark>,
}

/// 網の上に線で出す印。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mark {
    /// 凡例の名前
    pub name: String,
    /// どの網の点か
    pub part: usize,
    /// 点の番号の対(線分)
    pub lines: Vec<[u32; 2]>,
}

/// 最初に出す見せ方(利用者が切り替えるまで)。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Start {
    /// 色で出す量
    pub quantity: usize,
    /// 格子の断面を出すか(格子の量を選ぶと、いつも出す)
    pub section: bool,
    /// 流線を出すか
    pub streamlines: bool,
}

/// 網 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub geom: Geometry,
}

/// 網の形。
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// 三角形の表面。点は世界座標(mm)
    Surface {
        points: Vec<[f32; 3]>,
        tris: Vec<[u32; 3]>,
    },
    /// 等間隔の直交格子。点はセルの中心
    Grid(Grid),
    /// 2 次の四面体の網。点は節点(世界座標 mm)
    Tets(Tets),
}

impl Geometry {
    /// 点の数(量の値の数はこれ × 成分の数)。
    pub fn len(&self) -> usize {
        match self {
            Geometry::Surface { points, .. } => points.len(),
            Geometry::Grid(g) => g.len(),
            Geometry::Tets(t) => t.points.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 2 次(10 節点)の四面体の網。節点の順は角 0〜3、辺の中点 (0,1)・(1,2)・(0,2)・(0,3)・
/// (1,3)・(2,3)。表面の三角形は 6 節点で、角 a・b・c(外から見て反時計回り)、辺の中点
/// ab・bc・ca の順。辺の中点が曲面に乗るので、要素の面は曲がる。
#[derive(Debug, Clone, PartialEq)]
pub struct Tets {
    pub points: Vec<[f32; 3]>,
    pub tets: Vec<[u32; 10]>,
    /// 外の面(色を塗る表面)
    pub faces: Vec<[u32; 6]>,
    /// 外の面ごとの、形の面の番号(`faces` と同じ順。分からない面は `u32::MAX`、空なら全部
    /// 分からない)。表示の陰の法線を、形の面の中だけで滑らかにつなぐのに使う
    pub face_ids: Vec<u32>,
}

impl Tets {
    /// 表面の 6 節点の三角形を、辺の中点で 4 つの三角形に分けた物(外向き)。
    pub fn surface_tris(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        self.faces.iter().flat_map(|f| {
            [
                [f[0], f[3], f[5]],
                [f[3], f[1], f[4]],
                [f[5], f[4], f[2]],
                [f[3], f[4], f[5]],
            ]
        })
    }

    /// 要素を辺の中点で 8 つの 1 次の四面体に分けた物(断面を切るため)。
    pub fn linear(&self, e: usize) -> [[u32; 4]; 8] {
        let t = self.tets[e];
        [
            [t[0], t[4], t[6], t[7]],
            [t[4], t[1], t[5], t[8]],
            [t[6], t[5], t[2], t[9]],
            [t[7], t[8], t[9], t[3]],
            // 中の八面体を対角 (0,2)–(1,3) で 4 つに
            [t[6], t[8], t[4], t[5]],
            [t[6], t[8], t[5], t[9]],
            [t[6], t[8], t[9], t[7]],
            [t[6], t[8], t[7], t[4]],
        ]
    }
}

/// 等間隔の直交格子。格子の座標(ローカル)から世界座標へは、軸の入れ替えと向きだけで移る
/// (`world[axes[a]] = local[a] * signs[a]`)。
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// セルの数(2D の流れは z が 1)
    pub n: [usize; 3],
    /// セル (0,0,0) の角(ローカル、mm)。セル (i,j,k) の中心は origin + (i+0.5, j+0.5, k+0.5)·h
    /// (z が 1 つの時は z = origin\[2\])
    pub origin: [f64; 3],
    /// セルの 1 辺(mm)
    pub h: f64,
    pub axes: [usize; 3],
    pub signs: [f64; 3],
    /// 物体の中のセル(値は使わない)
    pub solid: Vec<bool>,
    /// 物体の表面の網([`SimResult::parts`] の番号)。断面の切り口を本当の外形で切るのに使う
    pub body: Option<usize>,
}

impl Grid {
    pub fn len(&self) -> usize {
        self.n[0] * self.n[1] * self.n[2]
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn index(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.n[0] * (j + self.n[1] * k)
    }

    /// セルの中心(ローカル)。
    pub fn center(&self, i: usize, j: usize, k: usize) -> [f64; 3] {
        let c = |a: usize, v: usize| {
            if self.n[a] == 1 && a == 2 {
                self.origin[2]
            } else {
                self.origin[a] + (v as f64 + 0.5) * self.h
            }
        };
        [c(0, i), c(1, j), c(2, k)]
    }

    pub fn to_world(&self, p: [f64; 3]) -> [f64; 3] {
        let mut w = [0.0; 3];
        for a in 0..3 {
            w[self.axes[a]] = p[a] * self.signs[a];
        }
        w
    }

    pub fn to_local(&self, w: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|a| w[self.axes[a]] * self.signs[a])
    }

    /// 世界の軸 `axis` に当たるローカルの軸。
    pub fn local_axis(&self, axis: usize) -> usize {
        (0..3).find(|a| self.axes[*a] == axis).unwrap_or(2)
    }

    /// 平らな(2D の)格子か。
    pub fn flat(&self) -> bool {
        self.n[2] == 1
    }
}

/// 量 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Quantity {
    pub name: String,
    pub unit: String,
    /// どの網の点の値か
    pub part: usize,
    /// 成分の数(1 = スカラー、3 = ベクトル。ベクトルは大きさで色を塗る)
    pub comps: usize,
    /// 小さいほど危ない値(安全率)。色の向きを逆にする
    pub low_is_bad: bool,
    /// 色の上限(安全率の「10 以上」など)
    pub cap: Option<f32>,
    pub role: Role,
    /// 応力の成分(量の番号)から求める量。四面体の網では、塗る点ごとに成分を補間してから求める
    /// (節点のミーゼス応力を補間すると、応力の向きが変わる所で網の模様が出るため)
    pub from: Option<(usize, Derive)>,
}

/// 応力の成分から求める量。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Derive {
    /// ミーゼス応力
    VonMises,
    /// 強さ ÷ ミーゼス応力(安全率)。応力が 0 なら 1e6
    Over(f32),
}

impl Derive {
    /// Voigt の順(xx, yy, zz, xy, yz, zx)の応力から。
    pub fn of(self, s: &[f64; 6]) -> f64 {
        let vm = (0.5 * ((s[0] - s[1]).powi(2) + (s[1] - s[2]).powi(2) + (s[2] - s[0]).powi(2))
            + 3.0 * (s[3] * s[3] + s[4] * s[4] + s[5] * s[5]))
            .max(0.0)
            .sqrt();
        // f64::max は NaN を捨てるので、値の無い所が 0(いちばん安全)に塗られないよう先に返す
        if s.iter().any(|v| !v.is_finite()) {
            return f64::NAN;
        }
        match self {
            Derive::VonMises => vm,
            Derive::Over(k) if vm > 0.0 => k as f64 / vm,
            Derive::Over(_) => 1e6,
        }
    }
}

impl Quantity {
    pub fn scalar(name: &str, unit: &str, part: usize) -> Self {
        Quantity {
            name: name.into(),
            unit: unit.into(),
            part,
            comps: 1,
            low_is_bad: false,
            cap: None,
            role: Role::Plain,
            from: None,
        }
    }

    /// 応力の 6 成分(Voigt の順)。量の一覧には出さず、[`Quantity::from`] が使う。
    pub fn tensor(name: &str, unit: &str, part: usize) -> Self {
        Quantity {
            comps: 6,
            role: Role::Tensor,
            ..Quantity::scalar(name, unit, part)
        }
    }

    pub fn vector(name: &str, unit: &str, part: usize, role: Role) -> Self {
        Quantity {
            comps: 3,
            role,
            ..Quantity::scalar(name, unit, part)
        }
    }

    /// 凡例の見出し(「速さ(m/s)」)。
    pub fn title(&self) -> String {
        if self.unit.is_empty() {
            self.name.clone()
        } else {
            format!("{}({})", self.name, self.unit)
        }
    }
}

/// 量の使い道(見せ方が探す)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Plain,
    /// 表面の点の変位・揺れの形(変形の表示に使う。世界座標の向き)
    Deform,
    /// 格子の流れの速度(流線に使う。格子のローカルの向き)
    Velocity,
    /// 応力の 6 成分(一覧に出さない)
    Tensor,
}

/// コマ 1 つ(時刻かモード)。
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// 時刻の帯に出す名前(「0.21 s」「1 次 123 Hz」)
    pub label: String,
    /// 物理の時刻(s)。モードのコマは `None`
    pub time: Option<f64>,
    /// 量ごとの値([`SimResult::quantities`] と同じ順。点ごとに成分を並べる)
    pub values: Vec<Values>,
}

/// 点ごとの値の列。大きな格子は 16 ビットに縮めて持つ(色を塗るには十分。最大・最小などの数値は
/// 解き方が縮める前に出す)。NaN は値の無い所。
#[derive(Debug, Clone, PartialEq)]
pub enum Values {
    F32(Arc<[f32]>),
    Q16 { lo: f32, step: f32, q: Arc<[u16]> },
}

const NONE16: u16 = u16::MAX;

impl Values {
    pub fn exact(v: Vec<f32>) -> Self {
        Values::F32(v.into())
    }

    /// 16 ビットに縮める。
    pub fn packed(v: &[f32]) -> Self {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for x in v.iter().filter(|x| x.is_finite()) {
            lo = lo.min(*x);
            hi = hi.max(*x);
        }
        if lo > hi {
            lo = 0.0;
            hi = 0.0;
        }
        let step = ((hi - lo) / (NONE16 - 1) as f32).max(f32::MIN_POSITIVE);
        let q = v
            .iter()
            .map(|x| {
                if x.is_finite() {
                    (((x - lo) / step).round() as u32).min(NONE16 as u32 - 1) as u16
                } else {
                    NONE16
                }
            })
            .collect();
        Values::Q16 { lo, step, q }
    }

    pub fn len(&self) -> usize {
        match self {
            Values::F32(v) => v.len(),
            Values::Q16 { q, .. } => q.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn at(&self, i: usize) -> f32 {
        match self {
            Values::F32(v) => v.get(i).copied().unwrap_or(f32::NAN),
            Values::Q16 { lo, step, q } => match q.get(i) {
                Some(&x) if x != NONE16 => lo + step * x as f32,
                _ => f32::NAN,
            },
        }
    }

    /// 点 `p` の値(`comps` が 3 なら大きさ)。
    #[inline]
    pub fn magnitude(&self, p: usize, comps: usize) -> f32 {
        if comps == 1 {
            self.at(p)
        } else {
            let v = self.vec3(p);
            (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
        }
    }

    /// 点 `p` の 6 成分。
    #[inline]
    pub fn vec6(&self, p: usize) -> [f64; 6] {
        std::array::from_fn(|k| self.at(6 * p + k) as f64)
    }

    #[inline]
    pub fn vec3(&self, p: usize) -> [f32; 3] {
        [self.at(3 * p), self.at(3 * p + 1), self.at(3 * p + 2)]
    }
}

impl SimResult {
    /// 量 `q` のコマ `f` の値。
    pub fn values(&self, f: usize, q: usize) -> Option<&Values> {
        self.frames.get(f)?.values.get(q)
    }

    /// 量 `q` の、コマ `frame` だけ(`None` なら全部のコマ)の値の範囲(上限のある量は上限まで)。
    pub fn range(&self, q: usize, frame: Option<usize>) -> (f32, f32) {
        let Some(def) = self.quantities.get(q) else {
            return (0.0, 1.0);
        };
        let n = self.parts.get(def.part).map_or(0, |p| p.geom.len());
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        let frames: Vec<usize> = match frame {
            Some(f) => vec![f.min(self.frames.len().saturating_sub(1))],
            None => (0..self.frames.len()).collect(),
        };
        let solid = match self.parts.get(def.part).map(|p| &p.geom) {
            Some(Geometry::Grid(g)) => Some(&g.solid),
            _ => None,
        };
        for f in frames {
            let Some(v) = self.values(f, q) else { continue };
            for p in 0..n {
                if solid.is_some_and(|s| s.get(p).copied().unwrap_or(false)) {
                    continue;
                }
                let x = v.magnitude(p, def.comps);
                if !x.is_finite() {
                    continue;
                }
                let x = def.cap.map_or(x, |c| x.min(c));
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        if lo > hi {
            return (0.0, 1.0);
        }
        if let Some(c) = def.cap {
            let lo = if c - lo < 1e-6 * c.abs().max(1.0) {
                0.0
            } else {
                lo
            };
            return (lo, c);
        }
        (lo, hi)
    }

    /// 中身から作る番号(同じ結果なら同じ。画面が描いた絵を使い回すかの判断)。
    pub fn stamp(&self) -> u64 {
        let mut h = Fnv::default();
        h.eat(self.kind.as_bytes());
        h.eat(self.summary.as_bytes());
        h.eat(&(self.frames.len() as u64).to_le_bytes());
        for p in &self.parts {
            h.eat(&(p.geom.len() as u64).to_le_bytes());
        }
        for f in &self.frames {
            for v in &f.values {
                let step = (v.len() / 64).max(1);
                for i in (0..v.len()).step_by(step) {
                    h.eat(&v.at(i).to_le_bytes());
                }
            }
        }
        h.0
    }

    /// 量の番号を名前で探す。
    pub fn quantity(&self, name: &str) -> Option<usize> {
        self.quantities.iter().position(|q| q.name == name)
    }

    /// 要約の値を名前で探す。
    pub fn number(&self, key: &str) -> Option<&str> {
        self.numbers
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// 番号を作る(FNV-1a)。
#[derive(Debug, Clone, Copy)]
pub struct Fnv(pub u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    pub fn eat(&mut self, b: &[u8]) {
        for x in b {
            self.0 ^= *x as u64;
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }

    pub fn eat_f32s(&mut self, v: impl IntoIterator<Item = f32>) {
        for x in v {
            self.eat(&x.to_le_bytes());
        }
    }
}

/// 数を短く(有効数字 3 桁ほど)。
pub fn fmt(v: f64) -> String {
    let a = v.abs();
    if a == 0.0 {
        "0".into()
    } else if !a.is_finite() {
        "—".into()
    } else if a >= 1000.0 {
        format!("{v:.0}")
    } else if a >= 100.0 {
        format!("{v:.1}")
    } else if a >= 1.0 {
        format!("{v:.2}")
    } else if a >= 0.01 {
        format!("{v:.3}")
    } else {
        format!("{v:.2e}")
    }
}

/// 1・2・5 の倍数のきりの良い数に丸める。
pub fn nice(v: f64) -> f64 {
    if v.is_nan() || v <= 0.0 || !v.is_finite() {
        return 1.0;
    }
    let p = 10f64.powf(v.log10().floor());
    let m = v / p;
    let n = if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    };
    n * p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 値の無い成分から 0(いちばん安全な色)を作らず、灰色に塗る NaN を返す。
    #[test]
    fn a_missing_stress_component_gives_nan_not_zero() {
        let s = [f64::NAN, 1.0, 2.0, 0.0, 0.0, 0.0];
        assert!(Derive::VonMises.of(&s).is_nan());
        assert!(Derive::Over(100.0).of(&s).is_nan());
        assert_eq!(Derive::VonMises.of(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0]), 1.0);
    }

    #[test]
    fn packed_values_keep_three_digits_and_nan() {
        let v = vec![0.0, 1.0, f32::NAN, 1234.5, -7.25];
        let p = Values::packed(&v);
        for (i, x) in v.iter().enumerate() {
            if x.is_nan() {
                assert!(p.at(i).is_nan());
            } else {
                assert!((p.at(i) - x).abs() < 1242.0 / 60000.0, "{i}: {}", p.at(i));
            }
        }
    }

    #[test]
    fn nice_numbers() {
        assert_eq!(nice(0.9), 1.0);
        assert_eq!(nice(230.0), 200.0);
        assert_eq!(nice(4.0), 5.0);
    }
}
