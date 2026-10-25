//! 工具を部品に触れるまで下ろした高さ(ドロップカッター)。3D の加工の戦略が共通で使う。
//!
//! 部品の高さは格子の節点で調べ、セルの高さはその 4 隅のいちばん高い値とする。工具は、
//! 工具の中心からセルの最も近い点までの距離で刃の高さを求めて当てる。どちらも安全側
//! (削りすぎない側)に倒しているので、残る量は格子の幅と斜面の傾きの積ほどになる。
//!
//! [`DropField`] は格子のすべての節点で下ろした高さを求めておき、[`DropField::contours`]
//! でその等高線(工具の中心がその高さまで下りられる所の境目)を取り出す。こちらは格子でなく
//! 元の三角形へ直接下ろす([`exact`])。格子は縦の壁の際で最大でセル 1 つ分外へずれ、輪が
//! 揺れ、ランプ(3D)の縁が折れるため。
//!
//! 3D の残す厚さを面から離れる向きに残す時は、工具を残す厚さだけ太らせた形([`Cutter`]、
//! 工具と球のミンコフスキー和)を下ろす。[`region`] は節点の印から輪と領域を作る道具、
//! [`limits`] は 3D の加工に共通の欄(径方向の残す厚さ・加工の範囲・傾斜の範囲)。

use jc_cam_geom::z_on_triangle;
use jc_cam_tool::{Tool, ToolKind};

pub mod exact;
pub mod limits;
pub mod region;

pub use limits::{Boundary, Contain, Limits, Resolved};
pub use region::{
    Grid, Region, areas_from_loops, dilate, level_loops, mask_loops, simplify_closed,
};

/// 工具の形(中心からの距離ごとの、刃の先端からの高さ)。
pub trait Profile {
    fn radius(&self) -> f64;
    /// 中心から距離 `d` の所で、刃が先端よりどれだけ上にあるか。刃の外なら `None`。
    /// 負にはならない(工具を下ろす計算は、これを使って高さの足りないセルを飛ばす)。
    fn height_at(&self, d: f64) -> Option<f64>;
    /// 式で解ける形か([`exact`] が速く解く)。ほかの形は数で探す。
    fn shape(&self) -> Shape {
        Shape::Other
    }
}

/// 工具の形のうち、三角形に触れる高さを式で解けるもの。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// 半径 r の球の先
    Ball(f64),
    /// 半径 r の平らな先
    Flat(f64),
    Other,
}

impl Profile for Tool {
    fn radius(&self) -> f64 {
        Tool::radius(self)
    }
    fn height_at(&self, d: f64) -> Option<f64> {
        Tool::height_at(self, d)
    }
    fn shape(&self) -> Shape {
        match self.kind {
            ToolKind::BallEnd => Shape::Ball(self.radius()),
            ToolKind::FlatEnd | ToolKind::Laser => Shape::Flat(self.radius()),
            _ => Shape::Other,
        }
    }
}

/// 工具を `grow` だけ太らせた形(工具と半径 `grow` の球のミンコフスキー和)。先端は元の工具の
/// 先端より `grow` 下にある。これを部品に下ろして先端の高さに `grow` を足すと、面のどの向きにも
/// `grow` の厚さが残る(ボールは半径 r + grow のボール、平は角の丸みが grow の形になる)。
#[derive(Debug, Clone, PartialEq)]
pub struct Cutter {
    pub tool: Tool,
    pub grow: f64,
}

impl Cutter {
    pub fn new(tool: Tool, grow: f64) -> Cutter {
        Cutter {
            tool,
            grow: grow.max(0.0),
        }
    }

    pub fn radius(&self) -> f64 {
        self.tool.radius() + self.grow
    }

    pub fn height_at(&self, d: f64) -> Option<f64> {
        let (r, g) = (self.tool.radius(), self.grow);
        if g <= 1e-12 {
            return self.tool.height_at(d);
        }
        if d > r + g {
            return None;
        }
        let arc = |x: f64| g - (g * g - x * x).max(0.0).sqrt();
        Some(match self.tool.kind {
            ToolKind::BallEnd => {
                let big = r + g;
                big - (big * big - d * d).max(0.0).sqrt()
            }
            ToolKind::FlatEnd => {
                if d <= r {
                    0.0
                } else {
                    arc(d - r)
                }
            }
            // ほかの形は、球の中心を刃の上に動かした時のいちばん低い所を数で求める。
            // 見本の間で低くなる分を、見本の間隔の傾きの上限で引いて安全側にする
            _ => {
                let (lo, hi) = ((d - g).max(0.0), (d + g).min(r));
                let n = 64;
                let mut best = f64::INFINITY;
                for k in 0..=n {
                    let d0 = lo + (hi - lo) * k as f64 / n as f64;
                    if let Some(h) = self.tool.height_at(d0) {
                        best = best.min(h + arc(d - d0));
                    }
                }
                (best - (hi - lo) / n as f64).max(0.0)
            }
        })
    }
}

impl Profile for Cutter {
    fn radius(&self) -> f64 {
        Cutter::radius(self)
    }
    fn height_at(&self, d: f64) -> Option<f64> {
        Cutter::height_at(self, d)
    }
    fn shape(&self) -> Shape {
        match self.tool.shape() {
            Shape::Ball(_) => Shape::Ball(self.radius()),
            s if self.grow <= 1e-12 => s,
            _ => Shape::Other,
        }
    }
}

impl From<Tool> for Cutter {
    fn from(tool: Tool) -> Cutter {
        Cutter::new(tool, 0.0)
    }
}

/// 部品の高さ(格子の節点)。部品がない所は `floor`。
#[derive(Clone)]
pub struct Heights {
    pub origin: [f64; 2],
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
    pub floor: f64,
    z: Vec<f64>,
    /// セル (i, j) のいちばん高い所([`Heights::cell_top`])を先に求めたもの(添字は節点と同じ
    /// `j * nx + i`。最後の行と列は使わない)。工具を下ろす計算はこれを何度も読む
    tops: Vec<f64>,
    /// セルの行ごとに、[`BLOCK`] 個ずつのセルの `tops` のいちばん高い所(添字は `j * bx + b`)。
    /// 工具を下ろす計算で、先端の高さを上げられない行をまとめて飛ばすのに使う
    blocks: Vec<f64>,
    /// 1 行の塊の数
    bx: usize,
    /// 元の三角形(工具を直接下ろす時に使う)
    mesh: std::sync::Arc<exact::Mesh>,
}

/// [`Heights`] の塊の幅(セルの数)。
const BLOCK: usize = 8;

impl Heights {
    pub fn new(
        min: [f64; 2],
        max: [f64; 2],
        cell: f64,
        floor: f64,
        tris: &[[[f64; 3]; 3]],
    ) -> Self {
        let nx = ((max[0] - min[0]) / cell).ceil() as usize + 1;
        let ny = ((max[1] - min[1]) / cell).ceil() as usize + 1;
        let mut h = Heights {
            origin: min,
            cell,
            nx,
            ny,
            floor,
            z: vec![floor; nx * ny],
            tops: Vec::new(),
            blocks: Vec::new(),
            bx: 0,
            mesh: std::sync::Arc::new(exact::Mesh::new(tris)),
        };
        for t in tris {
            let lo = |k: usize| t.iter().map(|v| v[k]).fold(f64::INFINITY, f64::min);
            let hi = |k: usize| t.iter().map(|v| v[k]).fold(f64::NEG_INFINITY, f64::max);
            let i0 = (((lo(0) - min[0]) / cell).ceil().max(0.0)) as usize;
            let i1 = (((hi(0) - min[0]) / cell).floor() + 1.0).clamp(0.0, nx as f64) as usize;
            let j0 = (((lo(1) - min[1]) / cell).ceil().max(0.0)) as usize;
            let j1 = (((hi(1) - min[1]) / cell).floor() + 1.0).clamp(0.0, ny as f64) as usize;
            for j in j0..j1 {
                for i in i0..i1 {
                    let p = [min[0] + i as f64 * cell, min[1] + j as f64 * cell];
                    if let Some(z) = z_on_triangle(t, p) {
                        let k = j * nx + i;
                        h.z[k] = h.z[k].max(z);
                    }
                }
            }
        }
        h.tops = vec![floor; nx * ny];
        for j in 0..ny.saturating_sub(1) {
            for i in 0..nx.saturating_sub(1) {
                h.tops[j * nx + i] = h
                    .node(i, j)
                    .max(h.node(i + 1, j))
                    .max(h.node(i, j + 1))
                    .max(h.node(i + 1, j + 1));
            }
        }
        // セルは各行 nx - 1 個
        h.bx = nx.saturating_sub(1).div_ceil(BLOCK);
        h.blocks = vec![floor; h.bx * ny];
        for j in 0..ny.saturating_sub(1) {
            for b in 0..h.bx {
                let row = &h.tops[j * nx..j * nx + nx - 1];
                h.blocks[j * h.bx + b] = row[b * BLOCK..((b + 1) * BLOCK).min(nx - 1)]
                    .iter()
                    .copied()
                    .fold(floor, f64::max);
            }
        }
        h
    }

    /// セルの行 `j` の、`lo` から `hi` まで(どちらも含む)のセルのいちばん高い所以上の値
    /// (塊ごとの値なので、範囲の外の同じ塊のセルも入ることがある)。
    fn span_top(&self, j: usize, lo: usize, hi: usize) -> f64 {
        self.blocks[j * self.bx + lo / BLOCK..=j * self.bx + hi / BLOCK]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
    }

    pub fn node(&self, i: usize, j: usize) -> f64 {
        self.z[j * self.nx + i]
    }

    /// 格子の形(原点・幅・節点の数)。
    pub fn grid(&self) -> Grid {
        Grid {
            origin: self.origin,
            cell: self.cell,
            nx: self.nx,
            ny: self.ny,
        }
    }

    /// 節点の位置。
    pub fn at(&self, i: usize, j: usize) -> [f64; 2] {
        [
            self.origin[0] + i as f64 * self.cell,
            self.origin[1] + j as f64 * self.cell,
        ]
    }

    /// 位置にいちばん近い節点の番号。
    pub fn index(&self, v: f64, axis: usize) -> usize {
        ((v - self.origin[axis]) / self.cell).round().max(0.0) as usize
    }

    /// セル (i, j)(節点 (i, j) から (i+1, j+1) まで)のいちばん高い所。
    pub fn cell_top(&self, i: usize, j: usize) -> f64 {
        debug_assert!(i + 1 < self.nx && j + 1 < self.ny);
        self.tops[j * self.nx + i]
    }

    /// 工具の下になるセル(工具の中心の節点からのずれ)と、そこでの刃の高さ。
    pub fn footprint<P: Profile + ?Sized>(&self, tool: &P) -> Footprint {
        let cell = self.cell;
        let n = (tool.radius() / cell).ceil() as isize + 1;
        // 区間 [d, d+1] × cell の、0 に最も近い点
        let near = |d: isize| {
            if d >= 0 {
                d as f64 * cell
            } else {
                -(d + 1) as f64 * cell
            }
        };
        let mut rows = Vec::new();
        for dj in -n..n {
            let cells: Vec<(isize, f64)> = (-n..n)
                .filter_map(|di| cut_height(tool, near(di).hypot(near(dj))).map(|h| (di, h)))
                .collect();
            let (Some(&(lo, _)), Some(&(hi, _))) = (cells.first(), cells.last()) else {
                continue;
            };
            // 刃のない所(間にはないはず)は高さを無限にして、当たらないようにする
            let mut ks = vec![f64::INFINITY; (hi - lo + 1) as usize];
            for &(di, k) in &cells {
                ks[(di - lo) as usize] = k;
            }
            rows.push(FootprintRow {
                dj,
                lo,
                hi,
                low: cells.iter().map(|c| c.1).fold(f64::INFINITY, f64::min),
                ks,
            });
        }
        // 刃の低い行(中心に近い行)から当てると、先端が早く高くなり、後の行を飛ばしやすい
        rows.sort_by(|a, b| a.low.total_cmp(&b.low));
        Footprint(rows)
    }

    /// 任意の点 `p` に工具の中心を置いて、格子でなく元の三角形へ直接下ろした先端の高さ
    /// ([`exact`])。[`drop_at`](Self::drop_at) 以下で、ずれは三角形にした誤差だけ。
    pub fn exact_at<P: Profile + ?Sized>(&self, tool: &P, p: [f64; 2]) -> f64 {
        self.mesh.drop_at(tool, p, self.floor)
    }

    /// 節点 (i, j) に工具の中心を置いた時の、先端の高さ。
    ///
    /// 塊([`BLOCK`] 個のセル)のいちばん高い所から、その行の刃のいちばん低い所を引いても
    /// 今の先端より高くならない塊は、セルを 1 つずつ見ずに飛ばす(最大を取るだけなので、
    /// 結果は全部のセルを見た時と同じ)。
    pub fn drop_node(&self, fp: &Footprint, i: usize, j: usize) -> f64 {
        let mut z = self.floor;
        let (i, j) = (i as isize, j as isize);
        // セルの番号の範囲は 0..=nx-2、0..=ny-2
        let (last_i, last_j) = (self.nx as isize - 2, self.ny as isize - 2);
        for row in &fp.0 {
            let cj = j + row.dj;
            if cj < 0 || cj > last_j {
                continue;
            }
            let (lo, hi) = ((i + row.lo).max(0), (i + row.hi).min(last_i));
            if lo > hi {
                continue;
            }
            let (lo, hi, cj) = (lo as usize, hi as usize, cj as usize);
            let tops = &self.tops[cj * self.nx..];
            let blocks = &self.blocks[cj * self.bx..];
            // 塊の中のセル ci の刃の高さは ks[ci - first]
            let first = i + row.lo;
            let b0 = lo / BLOCK;
            for (n, &top) in blocks[b0..=hi / BLOCK].iter().enumerate() {
                if top - row.low <= z {
                    continue;
                }
                let b = b0 + n;
                let (c0, c1) = ((b * BLOCK).max(lo), (b * BLOCK + BLOCK - 1).min(hi));
                let k0 = (c0 as isize - first) as usize;
                for (t, k) in tops[c0..=c1].iter().zip(&row.ks[k0..]) {
                    z = z.max(t - k);
                }
            }
        }
        z
    }

    /// 任意の点 `p` に工具の中心を置いた時の、先端の高さ。
    pub fn drop_at<P: Profile + ?Sized>(&self, tool: &P, p: [f64; 2]) -> f64 {
        let (r, cell) = (tool.radius(), self.cell);
        let range = |v: f64, axis: usize, n: usize| {
            let lo = ((v - r - self.origin[axis]) / cell).floor().max(0.0) as usize;
            let hi = (((v + r - self.origin[axis]) / cell).floor() as isize + 1)
                .clamp(0, n as isize - 1) as usize;
            lo..hi
        };
        let mut z = self.floor;
        let (rows, cols) = (range(p[1], 1, self.ny), range(p[0], 0, self.nx));
        if cols.is_empty() {
            return z;
        }
        // 刃の高さは負にならないので、セルのいちばん高い所が今の先端より低い行とセルは
        // 先端を上げない(飛ばしても結果は同じ)。中心に近い行から当てて、早く高くする
        let center = (((p[1] - self.origin[1]) / cell).floor().max(0.0) as usize)
            .clamp(rows.start, rows.end.max(rows.start + 1) - 1);
        let order = (center..rows.end).chain((rows.start..center).rev());
        for cj in order {
            if self.span_top(cj, cols.start, cols.end - 1) <= z {
                continue;
            }
            let y0 = self.origin[1] + cj as f64 * cell;
            let dy = (y0 - p[1]).max(p[1] - (y0 + cell)).max(0.0);
            for ci in cols.clone() {
                let top = self.cell_top(ci, cj);
                if top <= z {
                    continue;
                }
                let x0 = self.origin[0] + ci as f64 * cell;
                let dx = (x0 - p[0]).max(p[0] - (x0 + cell)).max(0.0);
                if let Some(k) = cut_height(tool, dx.hypot(dy)) {
                    z = z.max(top - k);
                }
            }
        }
        z
    }
}

/// 工具の中心から `d` の所の刃の高さ。ちょうど半径の所は、丸めの誤差で節点ごとと
/// 任意の点とで結果が分かれないよう、少し外まで刃があるとみなす。
fn cut_height<P: Profile + ?Sized>(tool: &P, d: f64) -> Option<f64> {
    (d <= tool.radius() + 1e-9)
        .then(|| tool.height_at(d.min(tool.radius())))
        .flatten()
}

/// 工具の下になるセルの一覧([`Heights::footprint`])。行ごとに、刃の低い行から並べる。
pub struct Footprint(Vec<FootprintRow>);

/// [`Footprint`] の 1 行。
struct FootprintRow {
    /// 工具の中心の節点からの行のずれ
    dj: isize,
    /// 列のずれのいちばん小さい・大きい所
    lo: isize,
    hi: isize,
    /// 列のずれ `lo..=hi` の刃の高さ
    ks: Vec<f64>,
    /// 行の刃のいちばん低い所
    low: f64,
}

/// `items` の各要素に `f` を当てた結果を、同じ順で返す。手の空いたスレッドから次の要素を
/// 取るので、要素ごとの重さが違っても偏らない。
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 16)
        .min(items.len().max(1));
    let next = AtomicUsize::new(0);
    let mut parts: Vec<Vec<(usize, R)>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(k) else { break };
                        out.push((k, f(item)));
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    let mut all: Vec<(usize, R)> = parts.drain(..).flatten().collect();
    all.sort_by_key(|(k, _)| *k);
    all.into_iter().map(|(_, r)| r).collect()
}

/// 等高線で「高さ z まで下りられる」とみなす許し。平らな面がちょうど z にある時、
/// 丸めの誤差で面の上に細かな輪がたくさんできないようにする。
const FLAT: f64 = 1e-6;

/// すべての節点で工具を下ろした高さ。
pub struct DropField {
    pub heights: Heights,
    pub tool: Cutter,
    drop: Vec<f64>,
}

impl DropField {
    /// 格子の全節点で下ろした高さを求める(行ごとにスレッドへ分ける)。`tool` は工具か、
    /// 太らせた工具([`Cutter`])。
    pub fn new(heights: Heights, tool: impl Into<Cutter>) -> DropField {
        let tool = tool.into();
        let fp = heights.footprint(&tool);
        // 式で解ける形(ボール・平)は、節点も三角形へ直接下ろす(格子の 2〜3 倍の時間)。ほかの形は
        // 数で探すので 30 倍ほどかかる。節点は格子で安全側に求め、境目を詰める所だけ直接下ろす
        let exact = tool.shape() != Shape::Other;
        let (nx, ny) = (heights.nx, heights.ny);
        let mut drop = vec![heights.floor; nx * ny];
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(1, 16);
        let rows_per = ny.div_ceil(threads).max(1);
        std::thread::scope(|s| {
            for (c, chunk) in drop.chunks_mut(rows_per * nx).enumerate() {
                let (h, fp, tool) = (&heights, &fp, &tool);
                s.spawn(move || {
                    for (k, v) in chunk.iter_mut().enumerate() {
                        let (i, j) = (k % nx, c * rows_per + k / nx);
                        *v = if exact {
                            h.exact_at(tool, h.at(i, j))
                        } else {
                            h.drop_node(fp, i, j)
                        };
                    }
                });
            }
        });
        DropField {
            heights,
            tool,
            drop,
        }
    }

    pub fn node(&self, i: usize, j: usize) -> f64 {
        self.drop[j * self.heights.nx + i]
    }

    /// 任意の点に工具の中心を置いて下ろした先端の高さ(元の三角形へ直接下ろす)。
    pub fn drop_at(&self, p: [f64; 2]) -> f64 {
        self.heights.exact_at(&self.tool, p)
    }

    /// 工具の中心が高さ `z` まで下りられる所の境目。部品(下りられない所)を進む向きの
    /// 右に見るので、島の周りは時計回り、穴の中は反時計回りになる(主軸が時計回りの時の
    /// ダウンカット)。境目の点は、実際に下ろした高さが `z` 以下になるまで下りられる側へ
    /// 寄せる。周の長さが `min_length` より短い輪は捨てる。
    pub fn contours(&self, z: f64, min_length: f64) -> Vec<Vec<[f64; 2]>> {
        let h = &self.heights;
        let free = |i: usize, j: usize| self.node(i, j) <= z + FLAT;
        // 鞍点: 真ん中が下りられないなら、下りられる隅を 1 つずつ切り取る
        let center = |i: usize, j: usize| {
            let mid = (self.node(i, j)
                + self.node(i + 1, j)
                + self.node(i + 1, j + 1)
                + self.node(i, j + 1))
                / 4.0;
            mid <= z + FLAT
        };
        region::trace(
            h.nx,
            h.ny,
            &free,
            &center,
            &mut |a, b| self.crossing(z, a, b),
            min_length,
        )
    }

    /// [`contours`](Self::contours) と同じ向きの輪で、工具の中心が高さ `z` まで下りられて、かつ
    /// 印 `allowed`(節点ごと、`heights` の格子の並び)の付いた所の境目。下りられないことによる
    /// 境目は安全側に寄せ、印による境目は節点の間の真ん中に置く。
    pub fn free_loops(&self, z: f64, allowed: &[bool], min_length: f64) -> Vec<Vec<[f64; 2]>> {
        let h = &self.heights;
        let nx = h.nx;
        let down = |i: usize, j: usize| self.node(i, j) <= z + FLAT;
        let free = |i: usize, j: usize| down(i, j) && allowed[j * nx + i];
        // 鞍点: 下りられないことだけで分かれる時は高さの平均で決め、印で分かれる時は切り離す
        let center = |i: usize, j: usize| {
            let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            if corners.iter().all(|&(a, b)| allowed[b * nx + a]) {
                let mid = corners.iter().map(|&(a, b)| self.node(a, b)).sum::<f64>() / 4.0;
                mid <= z + FLAT
            } else {
                false
            }
        };
        let mut crossing = |a: (usize, usize), b: (usize, usize)| {
            // a が中、b が外
            if !down(b.0, b.1) {
                self.crossing(z, a, b)
            } else {
                let (pa, pb) = (h.at(a.0, a.1), h.at(b.0, b.1));
                [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0]
            }
        };
        region::trace(h.nx, h.ny, &free, &center, &mut crossing, min_length)
    }

    /// 節点 (i, j) での、下ろした高さの面の傾き(ラジアン、0 が水平)。前後 `span` 節点の差で測る。
    /// ボールエンドミルなら、工具が当たる所の部品の面の傾きと同じ。
    pub fn slope(&self, i: usize, j: usize, span: usize) -> f64 {
        let h = &self.heights;
        let d = |a: usize, b: usize, axis: usize| {
            let (lo, hi) = (a.saturating_sub(span), (a + span).min(b - 1));
            if hi == lo {
                return 0.0;
            }
            let (z0, z1) = if axis == 0 {
                (self.node(lo, j), self.node(hi, j))
            } else {
                (self.node(i, lo), self.node(i, hi))
            };
            (z1 - z0) / ((hi - lo) as f64 * h.cell)
        };
        d(i, h.nx, 0).hypot(d(j, h.ny, 1)).atan()
    }

    /// 任意の点に近い節点での傾き([`slope`](Self::slope))。
    pub fn slope_at(&self, p: [f64; 2], span: usize) -> f64 {
        let h = &self.heights;
        let i = h.index(p[0], 0).min(h.nx - 1);
        let j = h.index(p[1], 1).min(h.ny - 1);
        self.slope(i, j, span)
    }

    /// 節点 `a`(下りられる)と `b`(下りられない)の間の境目を、安全側に求める。
    fn crossing(&self, z: f64, a: (usize, usize), b: (usize, usize)) -> [f64; 2] {
        let (a, b) = if self.node(a.0, a.1) > z + FLAT {
            (b, a)
        } else {
            (a, b)
        };
        let (va, vb) = (self.node(a.0, a.1), self.node(b.0, b.1));
        let (pa, pb) = (self.heights.at(a.0, a.1), self.heights.at(b.0, b.1));
        let lerp = |t: f64| [pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t];
        let ok = |t: f64| self.drop_at(lerp(t)) <= z + FLAT;
        // 節点の間を直線で補った点から始め、下りられる側と下りられない側の間を半分ずつ詰める。
        // 縦の壁では節点の値が跳ぶので、直線で補った点は本当の境目から最大で格子 1 つ分
        // 下りられる側へずれる(安全だが、輪が格子の幅ほどぎざぎざになる)。どちらへずれていても
        // 本当の境目まで寄せる
        let t = ((z - va) / (vb - va)).clamp(0.0, 1.0);
        let (mut lo, mut hi) = if ok(t) { (t, 1.0) } else { (0.0, t) };
        for _ in 0..8 {
            let m = (lo + hi) / 2.0;
            if ok(m) { lo = m } else { hi = m }
        }
        lerp(lo)
    }
}

/// 一直線に並ぶ点を間引く(閉じた輪)。
fn simplify_loop(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let open: Vec<[f64; 3]> = pts.iter().map(|p| [p[0], p[1], 0.0]).collect();
    let mut s = simplify(&open);
    // 始点をはさむ 3 点も確かめる
    while s.len() > 3 {
        let (a, b, c) = (s[s.len() - 1], s[0], s[1]);
        if collinear(a, b, c) {
            s.remove(0);
        } else {
            break;
        }
    }
    s.into_iter().map(|p| [p[0], p[1]]).collect()
}

fn collinear(a: [f64; 3], b: [f64; 3], q: [f64; 3]) -> bool {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let aq = [q[0] - a[0], q[1] - a[1], q[2] - a[2]];
    let cross = [
        ab[1] * aq[2] - ab[2] * aq[1],
        ab[2] * aq[0] - ab[0] * aq[2],
        ab[0] * aq[1] - ab[1] * aq[0],
    ];
    let len = (aq[0] * aq[0] + aq[1] * aq[1] + aq[2] * aq[2]).sqrt();
    let dot = ab[0] * aq[0] + ab[1] * aq[1] + ab[2] * aq[2];
    let off = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    dot > 0.0 && off <= 1e-6 * len.max(1.0)
}

/// 一直線に並ぶ点を間引く。
pub fn simplify(pts: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let mut out: Vec<[f64; 3]> = Vec::with_capacity(pts.len());
    for &q in pts {
        if out.last() == Some(&q) {
            continue;
        }
        if out.len() >= 2 && collinear(out[out.len() - 2], out[out.len() - 1], q) {
            out.pop();
        }
        out.push(q);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_cam_tool::ToolLibrary;

    /// 半径 5、高さ 4 の円柱(上面だけの三角形で十分)。
    fn cylinder() -> Vec<[[f64; 3]; 3]> {
        let n = 64;
        (0..n)
            .map(|k| {
                let a = |k: usize| std::f64::consts::TAU * k as f64 / n as f64;
                [
                    [0.0, 0.0, 4.0],
                    [5.0 * a(k).cos(), 5.0 * a(k).sin(), 4.0],
                    [5.0 * a(k + 1).cos(), 5.0 * a(k + 1).sin(), 4.0],
                ]
            })
            .collect()
    }

    fn signed_area(l: &[[f64; 2]]) -> f64 {
        (0..l.len())
            .map(|k| {
                let (a, b) = (l[k], l[(k + 1) % l.len()]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            / 2.0
    }

    #[test]
    fn a_flat_endmill_circles_an_island_clockwise_at_its_radius() {
        let tool = ToolLibrary::starter().tools[0].clone(); // 平 6 mm
        let h = Heights::new([-12.0, -12.0], [12.0, 12.0], 0.1, 0.0, &cylinder());
        let f = DropField::new(h, tool.clone());
        let loops = f.contours(2.0, 1.0);
        assert_eq!(loops.len(), 1);
        let l = &loops[0];
        assert!(signed_area(l) < 0.0, "時計回り");
        for p in l {
            let d = p[0].hypot(p[1]);
            // 島(64 角形。辺の真ん中は半径 5 cos(π/64))の半径 + 工具の半径 3
            let inner = 5.0 * (std::f64::consts::PI / 64.0).cos() + 3.0;
            assert!((inner - 1e-6..8.0 + 1e-3).contains(&d), "{d}");
            assert!(f.drop_at(*p) <= 2.0 + 1e-9);
        }
    }

    #[test]
    fn a_ring_beside_a_vertical_wall_is_not_jagged_by_a_cell() {
        // 縦の壁の際では、壁の内と外の節点の値が跳ぶ(床と上面)。節点の間を直線で補った点は
        // 本当の境目から格子 1 つ分ほど離れうるので、輪が格子の幅でぎざぎざになっていた
        // (ランプ(3D)の穴の縁の折れ)。高さの格子(セルの中のいちばん高い所をセル全体の高さと
        // みなす)で下ろすと、境目まで詰めても格子 0.1 mm で 0.09 mm 揺れた(詰めない時は 0.14 mm)。
        // 三角形へ直接下ろすと、残る揺れは 64 角形の辺の真ん中と頂点の差(0.006 mm)だけ
        let n = 64;
        let a = |k: usize| std::f64::consts::TAU * k as f64 / n as f64;
        let rim = |k: usize, z: f64| [5.0 * a(k).cos(), 5.0 * a(k).sin(), z];
        let mut tris = cylinder();
        for k in 0..n {
            tris.push([rim(k, 0.0), rim(k + 1, 0.0), rim(k + 1, 4.0)]);
            tris.push([rim(k, 0.0), rim(k + 1, 4.0), rim(k, 4.0)]);
        }
        let ball = ToolLibrary::starter().tools[3].clone();
        let h = Heights::new([-12.0, -12.0], [12.0, 12.0], 0.1, 0.0, &tris);
        let f = DropField::new(h, ball.clone());
        let l = &f.contours(1.0, 1.0)[0];
        let d: Vec<f64> = l.iter().map(|p| p[0].hypot(p[1])).collect();
        let lo = d.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = d.iter().copied().fold(0.0, f64::max);
        let inner = 5.0 * (std::f64::consts::PI / n as f64).cos() + ball.radius();
        assert!(
            lo >= inner - 1e-6 && hi <= 5.0 + ball.radius() + 1e-3,
            "{lo} {hi}"
        );
        let jump = (0..d.len())
            .map(|k| (d[(k + 1) % d.len()] - d[k]).abs())
            .fold(0.0, f64::max);
        assert!(
            hi - lo < 0.007 && jump < 0.007,
            "揺れ {:.3}({lo:.3}〜{hi:.3})、隣との差 {jump:.3}",
            hi - lo
        );
    }

    #[test]
    fn a_floor_at_the_level_gives_no_crumbs_of_rings() {
        // 高さ 0.3 の床(扇形に細かく分けた三角形で、節点の高さが丸めの誤差で揺れる)に島
        let n = 97;
        let mut tris = cylinder();
        for k in 0..n {
            let a = |k: usize| 0.37 + std::f64::consts::TAU * k as f64 / n as f64;
            let p = |k: usize| [0.3 + 20.0 * a(k).cos(), 0.7 + 20.0 * a(k).sin(), 0.1 + 0.2];
            tris.push([[0.3, 0.7, 0.3], p(k), p(k + 1)]);
        }
        let tool = ToolLibrary::starter().tools[0].clone();
        let h = Heights::new([-12.0, -12.0], [12.0, 12.0], 0.1, -1.0, &tris);
        let f = DropField::new(h, tool);
        let loops = f.contours(0.3, 0.4);
        assert_eq!(loops.len(), 1, "島の周りの 1 つだけ");
    }

    #[test]
    fn a_bull_nose_and_a_taper_ball_drop_by_their_own_shapes() {
        use jc_cam_tool::ToolKind;
        // 高さ 4 の円板(半径 5)の縁 (5, 0) の外に工具を置く
        let h = Heights::new([-12.0, -12.0], [12.0, 12.0], 0.02, -10.0, &cylinder());
        let mut bull = ToolLibrary::starter().tools[0].clone(); // 径 6
        bull.kind = ToolKind::BullNose;
        bull.corner_radius = Some(1.0);
        // 縁から 2.5 mm: 平らな所(2 mm)の外で、角 R の上に縁が乗る
        let want = 4.0 - (1.0 - 0.75f64.sqrt());
        let z = h.drop_at(&bull, [7.5, 0.0]);
        assert!((z - want).abs() < 0.03, "{z} {want}");
        // 平らな所の中なら上面の高さ
        assert!((h.drop_at(&bull, [6.9, 0.0]) - 4.0).abs() < 1e-9);
        let mut tb = bull.clone();
        tb.kind = ToolKind::TaperBall;
        tb.tip_diameter = Some(1.0);
        tb.tip_angle = 10.0;
        // 先の球の上(中心から 0.3 mm)と、テーパーの側面の上(中心から 1 mm)
        let z = h.drop_at(&tb, [5.3, 0.0]);
        assert!((z - (4.0 - (0.5 - 0.16f64.sqrt()))).abs() < 0.03, "{z}");
        // 側面(全体の角 10°、片側 5°)は先の球に接する所 (r cos5°, r − r sin5°) から立ち上がる。
        // 中心から 1 mm では 0.5 − 0.5 sin5° + (1 − 0.5 cos5°) / tan5° ≈ 6.19 mm 上。
        // 側面は急なので、円板の縁の網の分(0.02 mm)の違いが高さでは 10 倍ほどになる
        let half = 5f64.to_radians();
        let rise = 0.5 - 0.5 * half.sin() + (1.0 - 0.5 * half.cos()) / half.tan();
        assert!((tb.height_at(1.0).unwrap() - rise).abs() < 1e-9);
        let z = h.drop_at(&tb, [6.0, 0.0]);
        let want = 4.0 - rise;
        assert!((z - want).abs() < 0.3, "{z} {want}");
        assert!(z < 0.0);
    }

    #[test]
    fn par_map_keeps_the_order() {
        let v: Vec<u64> = (0..1000).collect();
        assert_eq!(
            par_map(&v, |x| x * 2),
            v.iter().map(|x| x * 2).collect::<Vec<_>>()
        );
        assert!(par_map(&Vec::<u64>::new(), |x| *x).is_empty());
    }

    #[test]
    fn nothing_is_traced_above_the_part() {
        let tool = ToolLibrary::starter().tools[0].clone();
        let h = Heights::new([-12.0, -12.0], [12.0, 12.0], 0.2, 0.0, &cylinder());
        assert!(DropField::new(h, tool).contours(4.5, 1.0).is_empty());
    }

    /// 飛ばさずに全部のセルを見る、前からの下ろし方(比べるため)。
    fn drop_node_all<P: Profile + ?Sized>(h: &Heights, tool: &P, i: usize, j: usize) -> f64 {
        let n = (tool.radius() / h.cell).ceil() as isize + 1;
        let near = |d: isize| {
            if d >= 0 {
                d as f64 * h.cell
            } else {
                -(d + 1) as f64 * h.cell
            }
        };
        let mut z = h.floor;
        for dj in -n..n {
            for di in -n..n {
                let Some(k) = cut_height(tool, near(di).hypot(near(dj))) else {
                    continue;
                };
                let (ci, cj) = (i as isize + di, j as isize + dj);
                if ci < 0 || cj < 0 || ci as usize + 1 >= h.nx || cj as usize + 1 >= h.ny {
                    continue;
                }
                z = z.max(h.cell_top(ci as usize, cj as usize) - k);
            }
        }
        z
    }

    fn drop_at_all<P: Profile + ?Sized>(h: &Heights, tool: &P, p: [f64; 2]) -> f64 {
        let (r, cell) = (tool.radius(), h.cell);
        let range = |v: f64, axis: usize, n: usize| {
            let lo = ((v - r - h.origin[axis]) / cell).floor().max(0.0) as usize;
            let hi = (((v + r - h.origin[axis]) / cell).floor() as isize + 1)
                .clamp(0, n as isize - 1) as usize;
            lo..hi
        };
        let mut z = h.floor;
        for cj in range(p[1], 1, h.ny) {
            let y0 = h.origin[1] + cj as f64 * cell;
            let dy = (y0 - p[1]).max(p[1] - (y0 + cell)).max(0.0);
            for ci in range(p[0], 0, h.nx) {
                let x0 = h.origin[0] + ci as f64 * cell;
                let dx = (x0 - p[0]).max(p[0] - (x0 + cell)).max(0.0);
                if let Some(k) = cut_height(tool, dx.hypot(dy)) {
                    z = z.max(h.cell_top(ci, cj) - k);
                }
            }
        }
        z
    }

    #[test]
    fn skipping_low_cells_gives_the_same_heights_as_looking_at_every_cell() {
        use jc_cam_tool::ToolKind;
        // でこぼこの面(斜めの三角形を重ねる)と円柱。格子の端にかかる所も比べる
        let mut tris = cylinder();
        for k in 0..40 {
            let (x, y) = (-11.0 + 0.53 * k as f64, -9.0 + 0.47 * (k * 7 % 40) as f64);
            let z = (k as f64 * 0.37).sin() * 3.0;
            tris.push([
                [x, y, z],
                [x + 2.3, y + 0.4, z + 1.1],
                [x + 0.6, y + 1.9, z - 0.8],
            ]);
        }
        let h = Heights::new([-12.0, -12.0], [12.0, 12.0], 0.13, -5.0, &tris);
        let base = ToolLibrary::starter().tools[0].clone();
        let mut tools: Vec<Cutter> = Vec::new();
        for (kind, corner) in [
            (ToolKind::FlatEnd, None),
            (ToolKind::BallEnd, None),
            (ToolKind::BullNose, Some(1.0)),
            (ToolKind::VBit, None),
        ] {
            let mut t = base.clone();
            t.kind = kind;
            t.corner_radius = corner;
            if kind == ToolKind::VBit {
                t.tip_angle = 60.0;
            }
            tools.push(Cutter::new(t.clone(), 0.0));
            tools.push(Cutter::new(t, 0.4));
        }
        for tool in &tools {
            let fp = h.footprint(tool);
            for j in (0..h.ny).step_by(7) {
                for i in (0..h.nx).step_by(5) {
                    assert_eq!(h.drop_node(&fp, i, j), drop_node_all(&h, tool, i, j));
                }
            }
            for k in 0..400 {
                let p = [
                    -13.0 + 0.0657 * k as f64,
                    -12.5 + 0.0611 * (k * 13 % 400) as f64,
                ];
                assert_eq!(h.drop_at(tool, p), drop_at_all(&h, tool, p));
            }
        }
    }
}
