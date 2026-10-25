//! 形状カーネルの接続口。
//!
//! ここに出てくる型は、どのカーネルにも依存しない純データである。機能は
//! [`Kernel`] の操作を呼んで [`ShapeHandle`] を受け取り、出力のスロットに入れる。
//! 面には生成元から決まる名前(例: `f3:side:e5`、`f3:end`)を付け、ブーリアンや
//! パターンの後も引き継ぐ。寸法を変えても名前が変わらないので、下流の機能は
//! 名前で面を参照し続けられる。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

/// カーネルが管理する形状への参照。値そのものは文書に保存しない。
/// JSON では `{"handle": 3}` になる([`crate::Value`] の内部タグと両立させるため)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "HandleRepr", into = "HandleRepr")]
pub struct ShapeHandle(pub u64);

#[derive(Serialize, Deserialize)]
struct HandleRepr {
    handle: u64,
}

impl From<HandleRepr> for ShapeHandle {
    fn from(r: HandleRepr) -> Self {
        ShapeHandle(r.handle)
    }
}

impl From<ShapeHandle> for HandleRepr {
    fn from(h: ShapeHandle) -> Self {
        HandleRepr { handle: h.0 }
    }
}

pub type P2 = [f64; 2];
pub type P3 = [f64; 3];

mod math;
mod profile;

pub use math::*;
pub use profile::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BooleanOp {
    Union,
    Subtract,
    Intersect,
}

/// 面の情報。平面なら、面上の点と外向きの法線を持つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceInfo {
    pub name: String,
    pub planar: Option<PlanarFace>,
    pub area: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlanarFace {
    pub point: P3,
    pub normal: P3,
}

/// 2 つの平面の境目の、まっすぐな辺([`Kernel::edges`])か、平面とそれに垂直な円筒の境目の
/// 円の辺([`Kernel::arc_edges`]、`arc` が `Some`)。辺は両側の面の名前の組で指す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeInfo {
    /// 円の辺では、平面の面が先、円筒の面が後
    pub faces: [String; 2],
    /// 両端の点(一周の円なら同じ点)
    pub a: P3,
    pub b: P3,
    /// 出っ張った辺(外側の角)か。へこんだ辺(内側の角)なら偽
    pub convex: bool,
    /// 両側の面が辺に沿って角を作るか、接していて角の無いなめらかな継ぎ目か(平面と、それに接する
    /// 円筒の境目など)。カーネルが分からなければ [`Seam::Unknown`]。丸める角の無い継ぎ目へは、接して
    /// つながる辺のたどり(CAD の層)が進まない
    #[serde(default, skip_serializing_if = "Seam::is_unknown")]
    pub seam: Seam,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<ArcEdge>,
    /// 曲がった辺([`Kernel::curve_edges`])の折れ線(`a` から `b` へ)。まっすぐな辺と円の辺では空
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curve: Vec<P3>,
}

/// 辺の両側の面のつながり方。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Seam {
    /// カーネルが求めていない。たどりなどは角とも継ぎ目とも決めつけずに止まる
    #[default]
    Unknown,
    /// 両側の面が角を作る(丸められる辺)
    Corner,
    /// 両側の面が辺に沿って接している、角の無いなめらかな継ぎ目
    Smooth,
}

/// 両側の面の法線の角度がこれ以下なら、なめらかな継ぎ目と見なす(度)。
pub const SEAM_DEG: f64 = 2.0;

impl Seam {
    pub fn is_unknown(&self) -> bool {
        *self == Seam::Unknown
    }

    /// 辺の上の 1 点での両側の面の外向きの法線 `na`・`nb`(長さ 1)から求める。
    pub fn from_normals(na: P3, nb: P3) -> Seam {
        if dot(na, nb) >= SEAM_DEG.to_radians().cos() {
            Seam::Smooth
        } else {
            Seam::Corner
        }
    }
}

/// カーネルの丸め([`Kernel::blend_edges`])に渡す辺 1 本。辺は両側の面の名前と、辺の近くの点で指す。
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeBlend {
    pub faces: [String; 2],
    pub point: P3,
    /// 丸めの半径。面取り(`flat`)では、両側の面に接する円の半径(直角の辺なら面取りの距離と同じ)。
    /// 弦の長さ([`BlendLaw::Chord`])では弦の長さ、途中で変わる半径では使わない
    pub size: f64,
    pub flat: bool,
    /// 半径の決め方(M16。面取りでは一定だけ)
    pub law: BlendLaw,
    /// 断面を曲率までなめらかにつなぐ(G2)。偽なら円の断面(G1)
    pub g2: bool,
    /// G2 の断面のふくらみ(1 が既定)。カーネルは 1 回の丸めで 1 つの
    /// 値しか使えないので、G2 の辺の最初の値を使う
    pub weight: f64,
}

impl EdgeBlend {
    /// 一定の半径(`flat` なら面取り)で、円の断面の丸め。
    pub fn new(faces: [String; 2], point: P3, size: f64, flat: bool) -> Self {
        EdgeBlend {
            faces,
            point,
            size,
            flat,
            law: BlendLaw::Constant,
            g2: false,
            weight: 1.0,
        }
    }
}

/// 丸めの半径の決め方([`EdgeBlend::law`])。
#[derive(Debug, Clone, PartialEq, Default)]
pub enum BlendLaw {
    /// 辺に沿って同じ半径 `size`
    #[default]
    Constant,
    /// 断面の両端(両側の面に接する所)の間の距離を `size` にそろえる
    Chord,
    /// 辺に沿って半径を変える。`radii` は(位置, 半径)の組で、位置は `start` に近い辺の端を 0、
    /// 反対の端を 1 とした割合(0 と 1 を含み、位置の順)
    Variable { start: P3, radii: Vec<(f64, f64)> },
}

impl BlendLaw {
    pub fn is_constant(&self) -> bool {
        *self == BlendLaw::Constant
    }
}

/// 円の辺の形。`a` から軸 `axis`(平面の面の外向きの法線)まわりに `sweep` ラジアン
/// (反時計回りが正、一周なら ±2π)回ると `b`。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ArcEdge {
    pub center: P3,
    pub axis: P3,
    pub radius: f64,
    pub sweep: f64,
    /// 平面の面が円の内側にあるか(ボスの上面なら真、穴の縁なら偽)
    pub inside: bool,
}

impl EdgeInfo {
    /// 辺を折れ線にする(まっすぐな辺は両端の 2 点、円の辺は `segments` 区間)。
    pub fn polyline(&self, segments: usize) -> Vec<P3> {
        if self.curve.len() >= 2 {
            return self.curve.clone();
        }
        let Some(arc) = self.arc else {
            return vec![self.a, self.b];
        };
        let n = segments.max(1);
        (0..=n)
            .map(|i| arc.point(self.a, arc.sweep * i as f64 / n as f64))
            .collect()
    }

    /// 辺の端 `p` での、辺に沿って辺の内へ向かう向き(`p` は `a` か `b`)。
    pub fn tangent_into(&self, at_b: bool) -> P3 {
        if self.curve.len() >= 2 {
            let n = self.curve.len();
            let d = if at_b {
                sub(self.curve[n - 2], self.curve[n - 1])
            } else {
                sub(self.curve[1], self.curve[0])
            };
            let l = dot(d, d).sqrt();
            return if l > 0.0 { scale(d, 1.0 / l) } else { d };
        }
        match self.arc {
            None => {
                let d = if at_b {
                    sub(self.a, self.b)
                } else {
                    sub(self.b, self.a)
                };
                let l = dot(d, d).sqrt();
                scale(d, 1.0 / l)
            }
            Some(arc) => {
                let p = if at_b { self.b } else { self.a };
                let r = sub(p, arc.center);
                let t = cross(arc.axis, r);
                let l = dot(t, t).sqrt();
                // a からは sweep の向き、b からは逆向き
                let s = if (arc.sweep > 0.0) != at_b { 1.0 } else { -1.0 };
                scale(t, s / l)
            }
        }
    }
}

impl ArcEdge {
    /// 円の上の点 `from` を軸まわりに `angle` 回した点。
    pub fn point(&self, from: P3, angle: f64) -> P3 {
        let r = sub(from, self.center);
        let k = self.axis;
        let (s, c) = angle.sin_cos();
        // ロドリゲスの回転(r は軸に垂直)
        add(self.center, add(scale(r, c), scale(cross(k, r), s)))
    }
}

/// 表示や STL 出力に使う三角形の網。`triangle_faces[i]` は三角形 i が属する面の番号
/// (`face_names` の添字)。`edges` は面の境目(稜線)を折れ線にしたもの。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub triangle_faces: Vec<u32>,
    pub face_names: Vec<String>,
    pub edges: Vec<Vec<[f32; 3]>>,
}

impl Mesh {
    /// 点 `p` が網で囲まれた中にあるか(閉じた網として、半直線と三角形の交わりの数の偶奇で見る)。
    /// 面の上の点は、どちらにもなり得る。半直線は軸からずらした向きにし、辺や頂点をかすめにくくする。
    pub fn contains(&self, p: P3) -> bool {
        let d = normalize([0.5773, 0.6131, 0.5392]);
        let at = |i: u32| self.positions[i as usize].map(f64::from);
        let mut n = 0;
        for t in &self.triangles {
            let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
            // Möller–Trumbore
            let (e1, e2) = (sub(b, a), sub(c, a));
            let h = cross(d, e2);
            let det = dot(e1, h);
            if det.abs() < 1e-12 {
                continue;
            }
            let s = sub(p, a);
            let u = dot(s, h) / det;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = cross(s, e1);
            let v = dot(d, q) / det;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            if dot(e2, q) / det > 1e-9 {
                n += 1;
            }
        }
        n % 2 == 1
    }
}

/// 体積、表面積、外形(軸に平行な箱)、重心。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MassProps {
    pub volume: f64,
    pub area: f64,
    pub min: P3,
    pub max: P3,
    /// 重心(密度が一様な時。M47)。求められないカーネルは `None`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub centroid: Option<P3>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KernelError {
    Unsupported(String),
    UnknownShape(ShapeHandle),
    Failed(String),
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelError::Unsupported(op) => write!(f, "このカーネルは {op} に対応していません"),
            KernelError::UnknownShape(h) => write!(f, "形状 {} が見つかりません", h.0),
            KernelError::Failed(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for KernelError {}

/// 同じ名前が複数あれば、代表点の位置の順(x、y、z の辞書順)に `#0`、`#1` … を付ける。
/// どのカーネルも同じ規則で面の名前を分ける。座標は 1 µm より近ければ同じとみなし、次の座標で
/// 比べる。格子に丸めて比べると、丸めの境にある x(0.4999 µm と 0.5001 µm など)のわずかな揺れで、
/// y で並ぶはずの番号が入れ替わる。
pub fn dedupe_names(names: Vec<String>, reps: &[P3]) -> Vec<String> {
    let mut groups: std::collections::HashMap<&str, Vec<usize>> = Default::default();
    for (i, n) in names.iter().enumerate() {
        groups.entry(n.as_str()).or_default().push(i);
    }
    let mut out = names.clone();
    for (name, idx) in groups {
        if idx.len() < 2 {
            continue;
        }
        let at = |i: usize| reps.get(i).copied().unwrap_or([0.0; 3]);
        for (k, i) in order_by_position(idx, &at, 0).into_iter().enumerate() {
            out[i] = format!("{name}#{k}");
        }
    }
    out
}

/// 位置の近さを見る幅(mm)。
const SAME_POSITION: f64 = 1e-3;

/// `idx` を、座標 `axis` から順に並べる。座標が `SAME_POSITION` より近く続く物は一つの組にし、
/// 組の中を次の座標で並べる(比べ方が推移的でない「近ければ同じ」を、並べ替えに直接使わない)。
fn order_by_position(mut idx: Vec<usize>, at: &dyn Fn(usize) -> P3, axis: usize) -> Vec<usize> {
    if axis == 3 || idx.len() < 2 {
        // 3 つとも同じ位置なら、元の並び(呼ぶ側の順)
        idx.sort_unstable();
        return idx;
    }
    idx.sort_by(|&a, &b| at(a)[axis].total_cmp(&at(b)[axis]));
    let mut out = Vec::with_capacity(idx.len());
    let mut group = vec![idx[0]];
    for w in idx.windows(2) {
        if at(w[1])[axis] - at(w[0])[axis] > SAME_POSITION {
            out.extend(order_by_position(std::mem::take(&mut group), at, axis + 1));
        }
        group.push(w[1]);
    }
    out.extend(order_by_position(group, at, axis + 1));
    out
}

/// 形状カーネルの接続口。操作の既定の実装は「対応していない」を返すので、
/// 形状を扱わない機能だけなら [`NullKernel`] で動く。
///
/// `tag` は面の名前の接頭辞で、ふつうは機能の ID(例: `f3`)を渡す。
pub trait Kernel: Send + Sync {
    fn name(&self) -> &str;

    /// 輪郭を法線方向に `from` から `to` まで押し出す(`from < to`)。
    fn extrude(
        &self,
        profile: &Profile,
        from: f64,
        to: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (profile, from, to, tag);
        Err(KernelError::Unsupported("押し出し".into()))
    }

    /// 輪郭を法線方向に `from` から `to` まで、壁を `taper` ラジアン傾けて押し出す(`from < to`、
    /// `from <= 0 <= to`)。輪郭は高さ 0 での断面で、正の `taper` は 0 から離れるほど細くなる
    /// (穴は逆に太くなる)。
    fn extrude_taper(
        &self,
        profile: &Profile,
        from: f64,
        to: f64,
        taper: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (profile, from, to, taper, tag);
        Err(KernelError::Unsupported("テーパーの付いた押し出し".into()))
    }

    /// ボディ `shape` の平らな面 `faces`(名前、同じ平面)の縁をそのまま、面の外向きの法線へ
    /// `from` から `to` まで押し出す(`from < to`)。縁に直線と円弧でない線(丸めと斜めの面の境の
    /// 楕円など)がある面を押し出す時、輪郭に近い形にせず面の縁の曲線を使うので、もとのボディの辺と
    /// ずれない。面は `{tag}:start`・`{tag}:end`・`{tag}:side:s1` のように名付ける。
    fn extrude_faces(
        &self,
        shape: ShapeHandle,
        faces: &[String],
        from: f64,
        to: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (shape, faces, from, to, tag);
        Err(KernelError::Unsupported("面の縁のままの押し出し".into()))
    }

    /// 輪郭を軸まわりに `angle` ラジアン回す。2π 以上なら一周。
    fn revolve(
        &self,
        profile: &Profile,
        axis_origin: P3,
        axis_dir: P3,
        angle: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (profile, axis_origin, axis_dir, angle, tag);
        Err(KernelError::Unsupported("回転".into()))
    }

    fn boolean(
        &self,
        op: BooleanOp,
        target: ShapeHandle,
        tool: ShapeHandle,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (op, target, tool);
        Err(KernelError::Unsupported("ブーリアン".into()))
    }

    /// 形状を動かした複製を作る。面の名前の末尾に `suffix` を足す。
    fn transform(
        &self,
        shape: ShapeHandle,
        m: &Matrix4,
        suffix: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (shape, m, suffix);
        Err(KernelError::Unsupported("移動".into()))
    }

    fn faces(&self, shape: ShapeHandle) -> Result<Vec<FaceInfo>, KernelError> {
        let _ = shape;
        Err(KernelError::Unsupported("面の列挙".into()))
    }

    /// 2 つの平面の境目の、まっすぐな辺の一覧。
    fn edges(&self, shape: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
        let _ = shape;
        Err(KernelError::Unsupported("辺の列挙".into()))
    }

    /// 平面とそれに垂直な円筒の境目の、円の辺の一覧(`arc` が `Some`)。
    fn arc_edges(&self, shape: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
        let _ = shape;
        Ok(vec![])
    }

    /// [`Kernel::edges`] と [`Kernel::arc_edges`] に無い、2 つの面の境目の辺(曲がった辺など。
    /// `curve` に折れ線)。
    fn curve_edges(&self, shape: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
        let _ = shape;
        Ok(vec![])
    }

    /// 辺を丸める・面取りする(両側の面にちょうど接するカーネルの丸め)。新しくできた面は、辺ごとに
    /// `{tag}:{面 a}|{面 b}`、辺の集まる角は `{tag}:corner` と名付ける(同じ名前は番号で分ける)。
    fn blend_edges(
        &self,
        shape: ShapeHandle,
        blends: &[EdgeBlend],
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (shape, blends, tag);
        Err(KernelError::Unsupported("辺の丸め".into()))
    }

    fn mass(&self, shape: ShapeHandle) -> Result<MassProps, KernelError> {
        let _ = shape;
        Err(KernelError::Unsupported("体積の計算".into()))
    }

    /// 体積だけ。表面積と外形の箱も求める [`Kernel::mass`] より軽くできるカーネルは置き換える
    /// (ブーリアンの結果を確かめる時など、体積しか使わない所で使う)。
    fn volume(&self, shape: ShapeHandle) -> Result<f64, KernelError> {
        self.mass(shape).map(|m| m.volume)
    }

    /// 外形の箱(少し大きめでよい)。体積を求めるより軽い。
    fn bounds(&self, shape: ShapeHandle) -> Result<(P3, P3), KernelError> {
        self.mass(shape).map(|m| (m.min, m.max))
    }

    /// 三角形の網にする。`tolerance` は面からのずれの上限(mm)。
    fn mesh(&self, shape: ShapeHandle, tolerance: f64) -> Result<Mesh, KernelError> {
        let _ = (shape, tolerance);
        Err(KernelError::Unsupported("テッセレーション".into()))
    }

    fn export_step(&self, shapes: &[ShapeHandle]) -> Result<String, KernelError> {
        let _ = shapes;
        Err(KernelError::Unsupported("STEP 出力".into()))
    }

    /// 名前つきで STEP にする。`names` は形ごとの名前(立体の名前に書く。数が合わなければ使わない)、
    /// `product` は部品の名前。日本語などの ASCII 以外の字も STEP の書き方で残す。名前を書けない
    /// カーネルは [`Kernel::export_step`] と同じ物を返す。
    fn export_step_named(
        &self,
        shapes: &[ShapeHandle],
        names: &[String],
        product: &str,
    ) -> Result<String, KernelError> {
        let _ = (names, product);
        self.export_step(shapes)
    }

    /// STEP の文字列から形状を作る。カーネルによっては、足し引きには使えない形になる。
    fn import_step(&self, step: &str) -> Result<ShapeHandle, KernelError> {
        let _ = step;
        Err(KernelError::Unsupported("STEP の読み込み".into()))
    }

    /// 立体をくり抜いて殻にする(シェル)。面 `open`(名前)は取り除いて開ける(空なら閉じた空洞)。
    /// `thickness` が正なら今の外形の内側へ、負なら外側へ厚みを付ける。新しくできた内側の面は
    /// `{tag}:in:{元の面の名前}` と名付ける。
    fn shell(
        &self,
        shape: ShapeHandle,
        open: &[String],
        thickness: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (shape, open, thickness, tag);
        Err(KernelError::Unsupported("シェル".into()))
    }

    /// 面 `faces`(名前)を、それぞれの外向きの法線の向きへ `distance` だけ動かす。正なら外へ肉を足し、負なら内へ削る(穴の壁を負にすると穴が広がる)。
    /// 面の名前はそのまま残し、足し引きでできた面は `{tag}:offset` と名付ける。
    fn offset_faces(
        &self,
        shape: ShapeHandle,
        faces: &[String],
        distance: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (shape, faces, distance, tag);
        Err(KernelError::Unsupported("面のオフセット".into()))
    }

    /// 輪郭 `profile` を経路 `path` に沿って動かした立体(スイープ)。輪郭は経路の向きの変わりに合わせて
    /// 回り、直線どうしの角では角の二等分の平面で継ぐ。経路の端の 1 つは輪郭の平面の上に置く。
    /// 側面は `{tag}:side:{輪郭の区間}:{経路の区間}`、端の面は `{tag}:start`・`{tag}:end`。
    /// `twist`(ラジアン)は終わりまでに輪郭を経路のまわりに回す角度、`scale` は終わりの輪郭の大きさの倍率で、
    /// どちらも経路の長さに比べて一様に効かせる(回す中心と大きさの中心は経路の点)。0 と 1 なら回さず変えない。
    fn sweep(
        &self,
        profile: &Profile,
        path: &PlanarPath,
        twist: f64,
        scale: f64,
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (profile, path, twist, scale, tag);
        Err(KernelError::Unsupported("スイープ".into()))
    }

    /// 断面 `sections`(順に並べた、閉じた輪郭 1 つずつ)をなめらかにつないだ立体(ロフト)。
    /// 断面に穴があれば、どの断面にも同じ数の穴を置き、穴どうしもつないで中空にする(前の断面の穴に
    /// いちばん近い穴を同じ穴とみなす)。`rails`(0〜2 本)は、断面に触れながら形の外側を導くガイドレール。
    /// 側面は `{tag}:side:{最初の断面の区間}`(穴の側面も穴の区間の名前)、端の面は `{tag}:start`・`{tag}:end`。
    fn loft(
        &self,
        sections: &[Profile],
        rails: &[PlanarPath],
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (sections, rails, tag);
        Err(KernelError::Unsupported("ロフト".into()))
    }

    /// 丸めの面 `faces`(名前)を消し、両側の面を延ばして鋭い辺に戻す。残る面の名前はそのまま、つなぎ直した面は `{tag}:healed` と名付ける。
    /// 丸めでない面は断る。
    fn remove_blend_faces(
        &self,
        shape: ShapeHandle,
        faces: &[String],
        tag: &str,
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (shape, faces, tag);
        Err(KernelError::Unsupported("丸めの面の削除".into()))
    }

    /// 面の名前を付け替えた形(形は同じ)。`renames` は (今の名前, 新しい名前) の組。無い名前は断る。
    /// 面を直した後に、直した面へ元の面の名前を戻し、その面を参照する後の機能が外れないようにする。
    fn rename_faces(
        &self,
        shape: ShapeHandle,
        renames: &[(String, String)],
    ) -> Result<ShapeHandle, KernelError> {
        let _ = (shape, renames);
        Err(KernelError::Unsupported("面の名前の付け替え".into()))
    }

    /// `live` にない形状を捨ててよいことを伝える。再計算のたびに呼ばれる。
    fn retain(&self, live: &HashSet<ShapeHandle>) {
        let _ = live;
    }
}

/// 形状を扱わない機能だけで動かすためのカーネル。
#[derive(Debug, Default)]
pub struct NullKernel;

impl Kernel for NullKernel {
    fn name(&self) -> &str {
        "null"
    }
}
