//! ストック除去シミュレーション。ストックを上から見た高さの格子(Z マップ)で表し、
//! 工具の形を経路に沿って押し当てて削る。上から削る 2.5D の加工ならこれで足りる
//! (えぐれた形は表せない)。
//!
//! 部品も同じ格子の高さにして、次を調べる。
//!
//! - 食い込み: 削った後の高さが、部品の高さより低くなった所
//! - 早送りでの切削: 早送りの間に材料を削った所
//! - 削り残し: 部品より高く残った材料の体積
//! - 工具のかかり具合(係合角): 横に動いて削る間、工具の周りのうち材料に触れている角度。
//!   180° は工具の径いっぱいの溝(全溝)。荒削りの負荷の目安になる
//! - シャンクとホルダーの当たり: 刃長より上で、シャンクの半径(刃の径より細ければ刃の径)の
//!   中に材料がある所と、突き出しより上で、ホルダーの半径の中に材料がある所。突き出しを決めて
//!   いない工具は、ホルダーを調べない
//! - 固定具との当たり: 刃・シャンク・ホルダーの円柱が、固定具(クランプ)の箱に入る所。早送りも調べる
//! - 機械の形との当たり([`MachineShape`] を渡した時): 作業範囲のはみ出し、主軸の頭(丸い主軸と
//!   Z の台の箱)と材料・固定具の当たり、刃のベッドへの食い込み、門の下を通らないストックと固定具。
//!   早送りも調べる
//!
//! 動きの単位で削っていく再生は [`Player`](play.rs)。

use jc_cam_tool::Tool;
use jc_core::Mesh;
use jc_toolpath::{Move, Op, P3, Program, Toolpath};

mod play;
pub use play::{Coloring, DEVIATION_FACES, PlayState, Player, tool_mesh};

/// 上から見た高さの格子。`z[j * nx + i]` が (i, j) のセルの中心での高さ。
#[derive(Debug, Clone, PartialEq)]
pub struct ZMap {
    pub origin: [f64; 2],
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
    pub z: Vec<f64>,
}

impl ZMap {
    /// `min`〜`max` の箱のストック。
    pub fn stock(min: P3, max: P3, cell: f64) -> ZMap {
        let nx = ((max[0] - min[0]) / cell).round().max(1.0) as usize;
        let ny = ((max[1] - min[1]) / cell).round().max(1.0) as usize;
        ZMap {
            origin: [min[0], min[1]],
            cell,
            nx,
            ny,
            z: vec![max[2]; nx * ny],
        }
    }

    /// 同じ格子で、網の真上から見たいちばん高い所。部品がない所は `floor`。
    pub fn from_mesh(like: &ZMap, mesh: &Mesh, offset: P3, floor: f64) -> ZMap {
        let mut m = ZMap {
            z: vec![floor; like.z.len()],
            ..like.clone()
        };
        for t in &mesh.triangles {
            let p = t.map(|i| {
                let v = mesh.positions[i as usize];
                [
                    v[0] as f64 - offset[0],
                    v[1] as f64 - offset[1],
                    v[2] as f64 - offset[2],
                ]
            });
            let xmin = p.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min);
            let xmax = p.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max);
            let ymin = p.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min);
            let ymax = p.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max);
            let (i0, i1) = m.range(xmin, xmax, 0);
            let (j0, j1) = m.range(ymin, ymax, 1);
            for j in j0..j1 {
                for i in i0..i1 {
                    let c = m.center(i, j);
                    if let Some(z) = z_on_triangle(&p, c) {
                        let k = j * m.nx + i;
                        m.z[k] = m.z[k].max(z);
                    }
                }
            }
        }
        m
    }

    pub fn center(&self, i: usize, j: usize) -> [f64; 2] {
        [
            self.origin[0] + (i as f64 + 0.5) * self.cell,
            self.origin[1] + (j as f64 + 0.5) * self.cell,
        ]
    }

    /// 座標の範囲 [a, b] に中心が入るセルの添字の範囲。
    fn range(&self, a: f64, b: f64, axis: usize) -> (usize, usize) {
        let n = if axis == 0 { self.nx } else { self.ny };
        let o = self.origin[axis];
        let lo = ((a - o) / self.cell - 0.5).ceil().max(0.0) as usize;
        let hi = (((b - o) / self.cell - 0.5).floor() + 1.0).clamp(0.0, n as f64) as usize;
        (lo.min(n), hi.max(lo.min(n)))
    }

    /// 基準の高さ `base` より上にある体積。
    pub fn volume_above(&self, base: &ZMap) -> f64 {
        self.z
            .iter()
            .zip(&base.z)
            .map(|(a, b)| (a - b).max(0.0))
            .sum::<f64>()
            * self.cell
            * self.cell
    }

    /// 近くのセルの最小値をとった格子(縁の誤差を許すための検査用)。
    pub fn eroded(&self, cells: usize) -> ZMap {
        let mut out = self.clone();
        let c = cells as isize;
        for j in 0..self.ny {
            for i in 0..self.nx {
                let mut m = f64::INFINITY;
                for dj in -c..=c {
                    for di in -c..=c {
                        let (ii, jj) = (i as isize + di, j as isize + dj);
                        if ii < 0 || jj < 0 || ii >= self.nx as isize || jj >= self.ny as isize {
                            continue;
                        }
                        m = m.min(self.z[jj as usize * self.nx + ii as usize]);
                    }
                }
                out.z[j * self.nx + i] = m;
            }
        }
        out
    }
}

impl ZMap {
    /// 同じ範囲を大きさ `cell` のセルで並べ直した格子。新しいセルの高さは、その中心が入る
    /// 前のセルの高さ(細かくすると前のセルの高さが並び、粗くするとセルの中心の高さを拾う)。
    pub fn resampled(&self, cell: f64) -> ZMap {
        let (w, h) = (self.nx as f64 * self.cell, self.ny as f64 * self.cell);
        let nx = (w / cell).round().max(1.0) as usize;
        let ny = (h / cell).round().max(1.0) as usize;
        let mut z = Vec::with_capacity(nx * ny);
        for j in 0..ny {
            let y = (j as f64 + 0.5) * cell;
            let oj = ((y / self.cell).floor() as usize).min(self.ny - 1);
            for i in 0..nx {
                let x = (i as f64 + 0.5) * cell;
                let oi = ((x / self.cell).floor() as usize).min(self.nx - 1);
                z.push(self.z[oj * self.nx + oi]);
            }
        }
        ZMap {
            origin: self.origin,
            cell,
            nx,
            ny,
            z,
        }
    }

    /// 長い辺が `max` セル以下になるよう、`k × k` のセルをまとめた粗い格子(まとめたセルの
    /// いちばん低い高さ。細い溝も消えずに見える)。画面に途中の形を出すのに使う。
    pub fn coarse(&self, max: usize) -> ZMap {
        let k = self.nx.max(self.ny).div_ceil(max.max(1)).max(1);
        if k == 1 {
            return self.clone();
        }
        let (nx, ny) = (self.nx.div_ceil(k), self.ny.div_ceil(k));
        let mut z = vec![f64::INFINITY; nx * ny];
        for j in 0..self.ny {
            for i in 0..self.nx {
                let c = &mut z[(j / k) * nx + i / k];
                *c = c.min(self.z[j * self.nx + i]);
            }
        }
        ZMap {
            origin: self.origin,
            cell: self.cell * k as f64,
            nx,
            ny,
            z,
        }
    }

    /// 高さの格子を、底 `bottom` までの閉じた網にする(上の面はセルの中心を結ぶ三角形、
    /// 四方の側面と底)。座標は格子と同じ(セットアップの座標)。面の名前は「途中の形」の 1 つ。
    pub fn to_mesh(&self, bottom: f64) -> Mesh {
        self.to_mesh_with(bottom, &["途中の形"], |_| 0)
    }

    /// [`ZMap::to_mesh`] の、上の面を塗り分ける形。上の面の三角形は、3 つの角のセルの
    /// `class`(セルの添字 → `faces` の番号)のうち大きい方の面にする(大きい番号ほど目立つ印に
    /// すると、細い食い込みも消えない)。側面と底は面 0。
    pub fn to_mesh_with(&self, bottom: f64, faces: &[&str], class: impl Fn(usize) -> u32) -> Mesh {
        let (nx, ny) = (self.nx, self.ny);
        let mut m = Mesh {
            face_names: faces.iter().map(|f| f.to_string()).collect(),
            ..Default::default()
        };
        let mut top_faces = Vec::new();
        if nx < 2 || ny < 2 {
            return m;
        }
        let h = |i: usize, j: usize| self.z[j * nx + i].max(bottom) as f32;
        let c = |i: usize, j: usize| self.center(i, j).map(|v| v as f32);
        // 上の面: 頂点はセルの中心、法線は隣との差から
        for j in 0..ny {
            for i in 0..nx {
                let p = c(i, j);
                m.positions.push([p[0], p[1], h(i, j)]);
                let dx = h((i + 1).min(nx - 1), j) - h(i.saturating_sub(1), j);
                let dy = h(i, (j + 1).min(ny - 1)) - h(i, j.saturating_sub(1));
                let s = 2.0 * self.cell as f32;
                let n = [-dx / s, -dy / s, 1.0];
                let l = (n[0] * n[0] + n[1] * n[1] + 1.0).sqrt();
                m.normals.push([n[0] / l, n[1] / l, 1.0 / l]);
            }
        }
        let v = |i: usize, j: usize| (j * nx + i) as u32;
        for j in 0..ny - 1 {
            for i in 0..nx - 1 {
                for t in [
                    [v(i, j), v(i + 1, j), v(i + 1, j + 1)],
                    [v(i, j), v(i + 1, j + 1), v(i, j + 1)],
                ] {
                    m.triangles.push(t);
                    top_faces.push(t.iter().map(|k| class(*k as usize)).max().unwrap_or(0));
                }
            }
        }
        // 縁を反時計回り(上から見て)にたどり、側面を下ろす
        let mut ring: Vec<(usize, usize)> = (0..nx).map(|i| (i, 0)).collect();
        ring.extend((1..ny).map(|j| (nx - 1, j)));
        ring.extend((0..nx - 1).rev().map(|i| (i, ny - 1)));
        ring.extend((1..ny - 1).rev().map(|j| (0, j)));
        let b = bottom as f32;
        for w in 0..ring.len() {
            let (p, q) = (ring[w], ring[(w + 1) % ring.len()]);
            let (pp, qq) = (c(p.0, p.1), c(q.0, q.1));
            let (dx, dy) = (qq[0] - pp[0], qq[1] - pp[1]);
            let l = dx.hypot(dy).max(1e-9);
            let n = [dy / l, -dx / l, 0.0];
            let base = m.positions.len() as u32;
            for x in [
                [pp[0], pp[1], h(p.0, p.1)],
                [pp[0], pp[1], b],
                [qq[0], qq[1], b],
                [qq[0], qq[1], h(q.0, q.1)],
            ] {
                m.positions.push(x);
                m.normals.push(n);
            }
            m.triangles.push([base, base + 1, base + 2]);
            m.triangles.push([base, base + 2, base + 3]);
        }
        // 底
        let base = m.positions.len() as u32;
        for (i, j) in [(0, 0), (nx - 1, 0), (nx - 1, ny - 1), (0, ny - 1)] {
            let p = c(i, j);
            m.positions.push([p[0], p[1], b]);
            m.normals.push([0.0, 0.0, -1.0]);
        }
        m.triangles.push([base, base + 2, base + 1]);
        m.triangles.push([base, base + 3, base + 2]);
        let rest = m.triangles.len() - top_faces.len();
        top_faces.extend(std::iter::repeat_n(0, rest));
        m.triangle_faces = top_faces;
        m
    }
}

fn z_on_triangle(t: &[[f64; 3]; 3], p: [f64; 2]) -> Option<f64> {
    let [a, b, c] = *t;
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1e-12 {
        return None;
    }
    let l1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
    let l2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
    let l3 = 1.0 - l1 - l2;
    let e = -1e-9;
    (l1 >= e && l2 >= e && l3 >= e).then(|| l1 * a[2] + l2 * b[2] + l3 * c[2])
}

/// 工具のかかり具合(度)。`from` から `tip` へ横に動く時、刃の前の縁のすぐ外に材料がある
/// 部分の角度。前の縁を横の位置 s(進む向きに直角)で区切り、そのすぐ前の点に材料があれば
/// その区間の角度 asin(s / r) の幅を足す。溝は 180°、横の切り込み ae なら arccos(1 - ae / r)。
/// すでに削った壁に沿って動くだけの所は、縁の前に材料がないので数えない。
fn engaged_angle(map: &ZMap, edge: &Edge, from: P3, tip: P3) -> f64 {
    let (dx, dy) = (tip[0] - from[0], tip[1] - from[1]);
    let l = dx.hypot(dy);
    let (u, n) = ([dx / l, dy / l], [-dy / l, dx / l]);
    let lift = (2.0 * (from[2] - tip[2]).abs()).max(0.01);
    let mut angle = 0.0;
    for &(s, ahead, h, da) in &edge.parts {
        let p = [
            tip[0] + ahead * u[0] + s * n[0],
            tip[1] + ahead * u[1] + s * n[1],
        ];
        // 負の数を先に除けば、切り捨ては floor と同じ(floor は x86-64 の既定では遅い関数を呼ぶ)
        let (fi, fj) = (
            (p[0] - map.origin[0]) / map.cell,
            (p[1] - map.origin[1]) / map.cell,
        );
        if fi < 0.0 || fj < 0.0 || fi >= map.nx as f64 || fj >= map.ny as f64 {
            continue;
        }
        if map.z[fj as usize * map.nx + fi as usize] > tip[2] + h + lift {
            angle += da;
        }
    }
    // 両端は縁の手前で区切ったので、端まで材料があれば縁まで延ばす
    angle.to_degrees() * 90.0 / edge.full
}

/// [`engaged_angle`] の、工具と格子で決まる分(経路ごとに 1 回だけ計算する)。前の縁を横の位置
/// s で 36 に区切り、区間ごとに (s、縁の前の点までの距離、刃の高さ、角度の幅) を持つ。
struct Edge {
    parts: Vec<(f64, f64, f64, f64)>,
    /// 縁の手前までの角度(度)
    full: f64,
}

impl Edge {
    fn new(map: &ZMap, tool: &Tool) -> Edge {
        const N: usize = 36;
        let r = tool.radius();
        let edge = (r - 0.5 * map.cell).max(r * 0.5);
        let parts = (0..N)
            .map(|k| {
                let (s0, s1) = (
                    -edge + 2.0 * edge * k as f64 / N as f64,
                    -edge + 2.0 * edge * (k + 1) as f64 / N as f64,
                );
                let s = (s0 + s1) / 2.0;
                let ahead = (r * r - s * s).sqrt() + 0.5 * map.cell;
                let h = tool.height_at(s.abs()).unwrap_or(0.0);
                let da = (s1 / r).clamp(-1.0, 1.0).asin() - (s0 / r).clamp(-1.0, 1.0).asin();
                (s, ahead, h, da)
            })
            .collect();
        Edge {
            parts,
            full: (edge / r).asin().to_degrees(),
        }
    }
}

/// 工具の先端の位置 `tip` で削る。削った量(体積)を返す。`owner` があれば、削ったセルに
/// その番号(どの経路で最後に削ったか)を書く。`track` があれば、先端より高い材料のない
/// 塊を飛ばし、高さの変わった塊を覚える(結果は飛ばさない時と同じ)。
fn stamp(
    map: &mut ZMap,
    tool: &Tool,
    tip: P3,
    bottom: f64,
    mut owner: Option<(&mut [u16], u16)>,
    mut track: Option<&mut Track>,
) -> f64 {
    let fi = (tip[0] - map.origin[0]) / map.cell - 0.5;
    let fj = (tip[1] - map.origin[1]) / map.cell - 0.5;
    let (ci, cj) = (fi.round() as isize, fj.round() as isize);
    let r = tool.radius();
    let mut removed = 0.0;
    let reach = (r / map.cell).ceil() as isize + 1;
    // 刃の高さは負にならないので、削った後の高さはこれより低くならない
    let low = tip[2].max(bottom);
    let (nx, cell, origin) = (map.nx, map.cell, map.origin);
    for dj in -reach..=reach {
        let j = cj + dj;
        if j < 0 || j >= map.ny as isize {
            continue;
        }
        let j = j as usize;
        let dy = origin[1] + (j as f64 + 0.5) * cell - tip[1];
        // 刃の外の行は調べない(シミュレーションの時間の大半はこの繰り返し。hypot は遅いので
        // 2 乗で外を除いてから sqrt で距離を出す。2026-10-17)
        if dy * dy > r * r {
            continue;
        }
        let row = &mut map.z[j * nx..(j + 1) * nx];
        // セル i を削る。高さが変わったら true
        let mut cut = |i: usize| -> bool {
            let dx = origin[0] + (i as f64 + 0.5) * cell - tip[0];
            let d2 = dx * dx + dy * dy;
            // 半径より外なら height_at は None を返す
            if d2 > r * r {
                return false;
            }
            let d = d2.sqrt();
            // 刃の縁ちょうどのセルは削らない(数値の揺れで食い込みと見なさないため)
            let Some(h) = tool.height_at(d + 1e-6) else {
                return false;
            };
            // ストックの底より下は材料がない(捨て板に入るだけ)
            let zt = (tip[2] + h).max(bottom);
            if zt < row[i] {
                removed += row[i] - zt;
                row[i] = zt;
                if let Some((o, n)) = owner.as_mut() {
                    o[j * nx + i] = *n;
                }
                return true;
            }
            false
        };
        let (lo, hi) = ((ci - reach).max(0), (ci + reach).min(nx as isize - 1));
        if lo > hi {
            continue;
        }
        let (lo, hi) = (lo as usize, hi as usize);
        match track.as_deref_mut() {
            None => {
                for i in lo..=hi {
                    cut(i);
                }
            }
            Some(t) => {
                for b in lo / BLOCK..=hi / BLOCK {
                    let at = j * t.bx + b;
                    if !t.all && t.top[at] <= low {
                        continue;
                    }
                    let mut changed = false;
                    for i in (b * BLOCK).max(lo)..=(b * BLOCK + BLOCK - 1).min(hi) {
                        changed |= cut(i);
                    }
                    if changed && !t.dirty[at] {
                        t.dirty[at] = true;
                        t.changed.push(at);
                    }
                }
            }
        }
    }
    removed * cell * cell
}

/// [`Track`] の塊の幅(セルの数)。
const BLOCK: usize = 8;

/// シミュレーションを速くするための、格子の塊ごとの印(削る間だけ持つ)。塊は行ごとに
/// [`BLOCK`] 個ずつのセル。
struct Track {
    /// 飛ばさずに全部のセルを見る(速くする前と同じ計算。比べる試験で使う)
    all: bool,
    /// 1 行の塊の数
    bx: usize,
    /// 塊のセルのいちばん高い所以上の値(添字は `j * bx + b`)。削ると下がるだけなので、
    /// 求め直す前でも上の限りとして使える
    top: Vec<f64>,
    /// 削って高さが変わり、まだ求め直していない塊
    dirty: Vec<bool>,
    changed: Vec<usize>,
    /// 部品の高さ(食い込みを調べる高さ)より `tol` を超えて低いセル。[`gouge_near`] は
    /// これのあるセルだけを見ればよい
    deep: Vec<bool>,
    /// 行ごとの `deep` の数
    deep_rows: Vec<u32>,
}

impl Track {
    fn new(map: &ZMap, check: &ZMap, tol: f64, all: bool) -> Track {
        let bx = map.nx.div_ceil(BLOCK);
        let mut t = Track {
            all,
            bx,
            top: vec![f64::NEG_INFINITY; bx * map.ny],
            dirty: vec![false; bx * map.ny],
            changed: (0..bx * map.ny).collect(),
            deep: vec![false; map.z.len()],
            deep_rows: vec![0; map.ny],
        };
        t.refresh(map, check, tol);
        t
    }

    /// 削った塊の高さと、食い込みの印を求め直す。
    fn refresh(&mut self, map: &ZMap, check: &ZMap, tol: f64) {
        if self.all {
            self.changed.clear();
            return;
        }
        for at in self.changed.drain(..) {
            self.dirty[at] = false;
            let (j, b) = (at / self.bx, at % self.bx);
            let mut top = f64::NEG_INFINITY;
            for i in b * BLOCK..((b + 1) * BLOCK).min(map.nx) {
                let k = j * map.nx + i;
                top = top.max(map.z[k]);
                let deep = check.z[k] - map.z[k] > tol;
                if deep != self.deep[k] {
                    self.deep[k] = deep;
                    if deep {
                        self.deep_rows[j] += 1;
                    } else {
                        self.deep_rows[j] -= 1;
                    }
                }
            }
            self.top[at] = top;
        }
    }
}

/// 食い込み、早送りでの切削の 1 件。
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// 経路の名前
    pub op: String,
    /// 経路の中の何番目の動きか
    pub index: usize,
    pub at: P3,
    /// 食い込みの深さ、または早送りで削った体積
    pub amount: f64,
}

/// 経路ごとの、工具のかかり具合(係合角。度)。
#[derive(Debug, Clone, PartialEq)]
pub struct Engagement {
    pub op: String,
    /// 横に動いて削った長さ(mm)
    pub length: f64,
    /// 削った長さで重み付けした平均の角度
    pub mean: f64,
    pub max: f64,
    /// 最大の角度になった位置
    pub at_max: P3,
    /// 全溝(170° 以上)で削った長さ(mm)
    pub slot_length: f64,
}

/// 工具のどこが材料に当たったか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPart {
    /// 刃(先端から刃長まで)。固定具との当たりだけで使う
    Cutter,
    /// 刃長より上(シャンク)
    Shank,
    /// 突き出しより上(ホルダー、コレットナット)
    Holder,
}

impl ToolPart {
    pub fn label(self) -> &'static str {
        match self {
            ToolPart::Cutter => "刃",
            ToolPart::Shank => "シャンク",
            ToolPart::Holder => "ホルダー",
        }
    }
}

/// シャンクかホルダーの当たり。`event.amount` は当たる材料が、当たってよい高さ(刃の上端か
/// ホルダーの下端)より何 mm 上まであるか。
#[derive(Debug, Clone, PartialEq)]
pub struct Collision {
    pub part: ToolPart,
    pub event: Event,
}

/// 当たりを調べる間隔(経路の長さ、mm)。
const COLLISION_STEP: f64 = 0.5;

/// 工具の先端 `tip` で、高さ `limit` より上、中心から `radius` の中に材料があれば、その高さの差。
fn above(map: &ZMap, tip: P3, radius: f64, limit: f64, tol: f64) -> Option<f64> {
    let reach = (radius / map.cell).ceil() as isize + 1;
    let ci = ((tip[0] - map.origin[0]) / map.cell - 0.5).round() as isize;
    let cj = ((tip[1] - map.origin[1]) / map.cell - 0.5).round() as isize;
    let mut worst: Option<f64> = None;
    for dj in -reach..=reach {
        let j = cj + dj;
        if j < 0 || j >= map.ny as isize {
            continue;
        }
        for di in -reach..=reach {
            let i = ci + di;
            if i < 0 || i >= map.nx as isize {
                continue;
            }
            let c = map.center(i as usize, j as usize);
            let (dx, dy) = (c[0] - tip[0], c[1] - tip[1]);
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let d = map.z[j as usize * map.nx + i as usize] - limit;
            if d > tol {
                worst = Some(worst.map_or(d, |w: f64| w.max(d)));
            }
        }
    }
    worst
}

/// シャンクとホルダーが材料に当たるか(削る前の材料で)。`top` は材料のいちばん高い所で、
/// 当たってよい高さがそれより上なら細かく調べない。
fn collisions_at(map: &ZMap, tool: &Tool, tip: P3, top: f64, tol: f64) -> Vec<(ToolPart, f64)> {
    let mut out = Vec::new();
    let flute_top = tip[2] + tool.flute_length;
    // 首下長があれば、刃の上から首の上端までは刃の径の首、その上がシャンク。
    // 首の当たりもシャンクの当たりとして数える
    let neck_top = tip[2] + tool.neck_top();
    let neck = (neck_top > flute_top + 1e-9)
        .then(|| {
            (flute_top < top - tol)
                .then(|| above(map, tip, tool.diameter / 2.0, flute_top, tol))
                .flatten()
        })
        .flatten();
    let shank = (neck_top < top - tol)
        .then(|| {
            let r = if neck_top > flute_top + 1e-9 {
                tool.shank() / 2.0
            } else {
                tool.shank().max(tool.diameter) / 2.0
            };
            above(map, tip, r, neck_top, tol)
        })
        .flatten();
    if let Some(d) = match (neck, shank) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    } {
        out.push((ToolPart::Shank, d));
    }
    if let (Some(stick), Some(hd)) = (tool.stickout, tool.holder_diameter) {
        let bottom = tip[2] + stick;
        if bottom < top - tol
            && let Some(d) = above(map, tip, hd / 2.0, bottom, tol)
        {
            out.push((ToolPart::Holder, d));
        }
    }
    out
}

/// 突き出しを決めていない工具の、シャンクの長さの見込み(刃の上端から、mm)。
const SHANK_GUESS: f64 = 10.0;
/// ホルダーの長さの見込み(突き出しの上端から、mm)。
const HOLDER_LENGTH: f64 = 30.0;

/// 先端から、シャンクの上端(コレットの端)までの高さ。
fn shank_top(tool: &Tool) -> f64 {
    let flute = tool.flute_length.max(0.0);
    tool.stickout.unwrap_or(flute + SHANK_GUESS).max(flute)
}

/// 先端から主軸の鼻(コレットナットの付け根)までの高さ。突き出しとホルダーの長さの見込みから。
pub fn nose_height(tool: &Tool) -> f64 {
    shank_top(tool) + HOLDER_LENGTH
}

/// 工具の部品ごとの、先端から測った高さの範囲(下、上)と半径。
fn tool_parts(tool: &Tool) -> Vec<(ToolPart, f64, f64, f64)> {
    let flute = tool.flute_length.max(0.0);
    let shank_top = shank_top(tool);
    let mut v = vec![
        (ToolPart::Cutter, 0.0, flute, tool.radius()),
        (ToolPart::Shank, flute, shank_top, tool.shank() / 2.0),
    ];
    if let Some(hd) = tool.holder_diameter {
        v.push((
            ToolPart::Holder,
            shank_top,
            shank_top + HOLDER_LENGTH,
            hd / 2.0,
        ));
    }
    v
}

/// 先端 `tip` の工具が固定具の箱 `boxes` に入るか。入るなら、いちばん下の部品と、横に入り込んだ
/// 深さ(半径 − 箱までの横の距離)。
fn fixture_hit(
    parts: &[(ToolPart, f64, f64, f64)],
    tip: P3,
    boxes: &[(P3, P3)],
) -> Option<(ToolPart, f64)> {
    for (lo, hi) in boxes {
        let dx = (lo[0] - tip[0]).max(tip[0] - hi[0]).max(0.0);
        let dy = (lo[1] - tip[1]).max(tip[1] - hi[1]).max(0.0);
        let d = dx.hypot(dy);
        for &(part, z0, z1, r) in parts {
            if d < r - 1e-6 && tip[2] + z0 < hi[2] - 1e-6 && tip[2] + z1 > lo[2] + 1e-6 {
                return Some((part, r - d));
            }
        }
    }
    None
}

/// 模擬で見る機械の形(セットアップの座標、mm)。呼ぶ側が機械の値とストックの置き場所から作る。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MachineShape {
    /// 主軸の中心が動ける X・Y の範囲(最小、最大)
    pub travel: ([f64; 2], [f64; 2]),
    /// 主軸の鼻が動ける高さ(下、上)
    pub nose: [f64; 2],
    /// 主軸の丸い所の径と、鼻から上の長さ。径が 0 なら見ない
    pub spindle: [f64; 2],
    /// 主軸の上の Z の台の箱の幅(X)・奥行き(Y)・高さ。主軸の中心に置く。幅が 0 なら見ない
    pub carriage: [f64; 3],
    /// ベッドの箱(最小、最大)
    pub bed: Option<(P3, P3)>,
    /// 刃がベッドを削ってよい深さ。これより深い食い込みを当たりにする
    pub bed_allowance: f64,
    /// 門の下の高さ。ストックと固定具がこれより高いと通らない
    pub clearance: Option<f64>,
}

/// 主軸の頭のどこか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadPart {
    /// 丸い主軸
    Spindle,
    /// 主軸の上の Z の台
    Carriage,
}

impl HeadPart {
    pub fn label(self) -> &'static str {
        match self {
            HeadPart::Spindle => "主軸",
            HeadPart::Carriage => "Z の台",
        }
    }
}

/// 機械の形との当たりの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineKind {
    /// 作業範囲のはみ出し(軸 0〜2、+側なら `high`)。量ははみ出した長さ。Z は主軸の鼻の高さで見る
    Travel { axis: usize, high: bool },
    /// 主軸の頭が材料に当たる。量は頭の下端より何 mm 上まで材料があるか
    HeadStock(HeadPart),
    /// 主軸の頭が固定具に当たる。量は横に入り込んだ深さ
    HeadFixture(HeadPart),
    /// 刃先がベッドに、削ってよい深さより深く食い込む。量は深さ
    Bed,
    /// ストックか固定具が門の下を通らない。量は超えた高さ
    Clearance,
}

impl MachineKind {
    pub fn label(self) -> String {
        match self {
            MachineKind::Travel { axis, high } => {
                let a = ["X", "Y", "Z"][axis.min(2)];
                match (axis, high) {
                    (2, true) => "Z の作業範囲を上へはみ出す(主軸が上がりきらない)".into(),
                    (2, false) => "Z の作業範囲を下へはみ出す(主軸が下がりきらない)".into(),
                    (_, true) => format!("{a} の作業範囲を+側へはみ出す"),
                    (_, false) => format!("{a} の作業範囲を−側へはみ出す"),
                }
            }
            MachineKind::HeadStock(p) => format!("{}が材料に当たる", p.label()),
            MachineKind::HeadFixture(p) => format!("{}が固定具に当たる", p.label()),
            MachineKind::Bed => "刃がベッドに食い込む".into(),
            MachineKind::Clearance => "門の下を通らない".into(),
        }
    }

    /// 続く所を数えるための番号。
    fn slot(self) -> usize {
        match self {
            MachineKind::Travel { axis, high } => axis.min(2) * 2 + high as usize,
            MachineKind::HeadStock(p) => 6 + p as usize,
            MachineKind::HeadFixture(p) => 8 + p as usize,
            MachineKind::Bed => 10,
            MachineKind::Clearance => 11,
        }
    }
}

/// 機械の形との当たりの 1 件。
#[derive(Debug, Clone, PartialEq)]
pub struct MachineHit {
    pub kind: MachineKind,
    pub event: Event,
}

/// 高さ `limit` より上に、箱 `lo`〜`hi`(X・Y)の中の材料があれば、その高さの差。
fn above_box(map: &ZMap, lo: [f64; 2], hi: [f64; 2], limit: f64, tol: f64) -> Option<f64> {
    let (i0, i1) = map.range(lo[0], hi[0], 0);
    let (j0, j1) = map.range(lo[1], hi[1], 1);
    let mut worst: Option<f64> = None;
    for j in j0..j1 {
        for i in i0..i1 {
            let d = map.z[j * map.nx + i] - limit;
            if d > tol {
                worst = Some(worst.map_or(d, |w: f64| w.max(d)));
            }
        }
    }
    worst
}

/// 先端 `tip`(主軸の鼻はその `nose` 上)の工具で、機械の形との当たり。`top` は材料の
/// いちばん高い所。
#[allow(clippy::too_many_arguments)]
fn machine_at(
    shape: &MachineShape,
    map: &ZMap,
    fixtures: &[(P3, P3)],
    tool_radius: f64,
    nose: f64,
    tip: P3,
    top: f64,
    tol: f64,
) -> Vec<(MachineKind, f64)> {
    let mut out = Vec::new();
    let (lo, hi) = shape.travel;
    for k in 0..2 {
        if tip[k] < lo[k] - 1e-6 {
            out.push((
                MachineKind::Travel {
                    axis: k,
                    high: false,
                },
                lo[k] - tip[k],
            ));
        } else if tip[k] > hi[k] + 1e-6 {
            out.push((
                MachineKind::Travel {
                    axis: k,
                    high: true,
                },
                tip[k] - hi[k],
            ));
        }
    }
    let n = tip[2] + nose;
    if n < shape.nose[0] - 1e-6 {
        out.push((
            MachineKind::Travel {
                axis: 2,
                high: false,
            },
            shape.nose[0] - n,
        ));
    } else if n > shape.nose[1] + 1e-6 {
        out.push((
            MachineKind::Travel {
                axis: 2,
                high: true,
            },
            n - shape.nose[1],
        ));
    }
    let [sd, sl] = shape.spindle;
    if sd > 0.0 {
        let r = sd / 2.0;
        if n < top - tol
            && let Some(d) = above(map, tip, r, n, tol)
        {
            out.push((MachineKind::HeadStock(HeadPart::Spindle), d));
        }
        let parts = [(ToolPart::Holder, 0.0, sl, r)];
        if let Some((_, d)) = fixture_hit(&parts, [tip[0], tip[1], n], fixtures) {
            out.push((MachineKind::HeadFixture(HeadPart::Spindle), d));
        }
    }
    let [cw, cd, ch] = shape.carriage;
    if cw > 0.0 && cd > 0.0 {
        let bottom = n + sl.max(0.0);
        let (blo, bhi) = (
            [tip[0] - cw / 2.0, tip[1] - cd / 2.0],
            [tip[0] + cw / 2.0, tip[1] + cd / 2.0],
        );
        if bottom < top - tol
            && let Some(d) = above_box(map, blo, bhi, bottom, tol)
        {
            out.push((MachineKind::HeadStock(HeadPart::Carriage), d));
        }
        for (flo, fhi) in fixtures {
            let ox = bhi[0].min(fhi[0]) - blo[0].max(flo[0]);
            let oy = bhi[1].min(fhi[1]) - blo[1].max(flo[1]);
            if ox > 1e-6 && oy > 1e-6 && bottom < fhi[2] - 1e-6 && bottom + ch > flo[2] + 1e-6 {
                out.push((MachineKind::HeadFixture(HeadPart::Carriage), ox.min(oy)));
                break;
            }
        }
    }
    if let Some((blo, bhi)) = shape.bed {
        let inside = (0..2).all(|k| tip[k] > blo[k] - tool_radius && tip[k] < bhi[k] + tool_radius);
        if inside && tip[2] < bhi[2] - shape.bed_allowance - tol {
            out.push((MachineKind::Bed, bhi[2] - tip[2]));
        }
    }
    out
}

/// これ以上を全溝とみなす角度(度)。
pub const SLOT_ANGLE: f64 = 170.0;

#[derive(Debug, Clone, PartialEq)]
pub struct SimReport {
    pub stock_volume: f64,
    pub removed_volume: f64,
    /// 部品より高く残った材料の体積
    pub remaining_volume: f64,
    pub gouges: Vec<Event>,
    pub rapid_cuts: Vec<Event>,
    /// シャンクとホルダーの当たり(経路ごとに、当たりの続く所の始めだけ)
    pub collisions: Vec<Collision>,
    /// 固定具との当たり(当たりの続く所の始めだけ)。`amount` は横に入り込んだ深さ(mm)
    pub fixture_hits: Vec<Collision>,
    /// 機械の形との当たりとはみ出し(種類ごとに、続く所の始めの位置と、続く所でいちばん大きい量。
    /// 作業範囲のはみ出しは経路ごとに種類ごとの 1 件)。機械の形を渡さなければ空
    pub machine_hits: Vec<MachineHit>,
    /// 刃がベッドを削った深さのいちばん大きい値(削ってよい深さの内も含む。機械の形が無ければ 0)
    pub bed_depth: f64,
    /// 経路ごとの、削った体積
    pub removed_by_op: Vec<(String, f64)>,
    /// 経路ごとの、工具のかかり具合(横に動いて削った経路だけ)
    pub engagement: Vec<Engagement>,
    /// 削り残しの印(1 mm 四方ごとに最も高く残った所の、残った面の上の位置と、部品との高さの差)。
    /// 差の大きい順に最大 2000 個
    pub leftovers: Vec<(P3, f64)>,
    /// 途中の形(素材と、各経路を削った後)。粗い格子で、[`SimOptions::stage_cells`] が 0 なら空
    pub stages: Vec<(String, ZMap)>,
    pub result: ZMap,
}

impl SimReport {
    pub fn is_ok(&self) -> bool {
        self.gouges.is_empty()
            && self.rapid_cuts.is_empty()
            && self.collisions.is_empty()
            && self.fixture_hits.is_empty()
            && self.machine_hits.is_empty()
    }

    pub fn summary(&self) -> String {
        let mut s = format!(
            "削った体積 {:.1} mm³ / ストック {:.1} mm³、削り残し {:.1} mm³、食い込み {} 件、早送りでの切削 {} 件、シャンク・ホルダーの当たり {} 件、固定具との当たり {} 件、機械の当たり・はみ出し {} 件",
            self.removed_volume,
            self.stock_volume,
            self.remaining_volume,
            self.gouges.len(),
            self.rapid_cuts.len(),
            self.collisions.len(),
            self.fixture_hits.len(),
            self.machine_hits.len()
        );
        for e in self.gouges.iter().take(3) {
            s.push_str(&format!(
                "\n  食い込み: {} の {} 番目、({:.2}, {:.2}, {:.2}) で {:.3} mm",
                e.op, e.index, e.at[0], e.at[1], e.at[2], e.amount
            ));
        }
        for e in self.rapid_cuts.iter().take(3) {
            s.push_str(&format!(
                "\n  早送りでの切削: {} の {} 番目、({:.2}, {:.2}, {:.2})",
                e.op, e.index, e.at[0], e.at[1], e.at[2]
            ));
        }
        for c in self.collisions.iter().take(3) {
            let e = &c.event;
            s.push_str(&format!(
                "\n  {}の当たり: {} の {} 番目、({:.2}, {:.2}, {:.2}) で {:.2} mm 上まで材料",
                c.part.label(),
                e.op,
                e.index,
                e.at[0],
                e.at[1],
                e.at[2],
                e.amount
            ));
        }
        for c in self.fixture_hits.iter().take(3) {
            let e = &c.event;
            s.push_str(&format!(
                "\n  固定具に{}が当たる: {} の {} 番目、({:.2}, {:.2}, {:.2}) で {:.2} mm 入り込む",
                c.part.label(),
                e.op,
                e.index,
                e.at[0],
                e.at[1],
                e.at[2],
                e.amount
            ));
        }
        for h in self.machine_hits.iter().take(3) {
            let e = &h.event;
            s.push_str(&format!(
                "\n  {}: {} の {} 番目、({:.2}, {:.2}, {:.2}) で {:.2} mm",
                h.kind.label(),
                e.op,
                e.index,
                e.at[0],
                e.at[1],
                e.at[2],
                e.amount
            ));
        }
        if self.bed_depth > 0.0 && !self.machine_hits.iter().any(|h| h.kind == MachineKind::Bed) {
            s.push_str(&format!(
                "\n  ベッドを最大 {:.2} mm 削る(削ってよい深さの内)",
                self.bed_depth
            ));
        }
        for e in &self.engagement {
            s.push_str(&format!(
                "\n  工具のかかり具合: {} 平均 {:.0}°、最大 {:.0}°、全溝 {:.1} mm",
                e.op, e.mean, e.max, e.slot_length
            ));
        }
        s
    }
}

/// シミュレーションの設定。
#[derive(Debug, Clone, Copy)]
pub struct SimOptions {
    /// 格子の大きさ(mm)
    pub cell: f64,
    /// 高さの許容差(mm)。これより深い食い込みを報告する
    pub z_tolerance: f64,
    /// 横の許容差(セルの数)。部品の縁から、この数のセルまでの食い込みは許す
    pub xy_cells: usize,
    /// 削り残しに数える高さ(mm)
    pub remaining_tolerance: f64,
    /// 途中の形を残す粗い格子の、長い辺のセルの数(0 なら残さない)
    pub stage_cells: usize,
    /// 荒削りの経路を削る格子の大きさ(mm)。0 なら `cell` と同じ。どの経路を荒削りとするかは
    /// 呼ぶ側が決め、経路ごとの大きさを [`simulate_grids`] に渡す
    pub rough_cell: f64,
}

impl SimOptions {
    /// 荒削りの経路を削る格子の大きさ。
    pub fn rough(&self) -> f64 {
        if self.rough_cell > 0.0 {
            self.rough_cell
        } else {
            self.cell
        }
    }
}

impl Default for SimOptions {
    fn default() -> Self {
        SimOptions {
            cell: 0.1,
            z_tolerance: 0.02,
            xy_cells: 1,
            remaining_tolerance: 0.05,
            stage_cells: 0,
            rough_cell: 0.0,
        }
    }
}

/// ストックを経路の順に削る。`bottom` はストックの底の高さ、`part` は部品の高さ
/// (同じ格子。部品がない所は負の無限大)。
pub fn simulate(
    stock: &ZMap,
    bottom: f64,
    part: &ZMap,
    paths: &[Toolpath],
    opt: SimOptions,
) -> SimReport {
    simulate_with(stock, bottom, part, paths, opt, &[])
}

/// [`simulate`] に、固定具の箱(セットアップの座標、最小と最大)との当たりの検査を足したもの。
pub fn simulate_with(
    stock: &ZMap,
    bottom: f64,
    part: &ZMap,
    paths: &[Toolpath],
    opt: SimOptions,
    fixtures: &[(P3, P3)],
) -> SimReport {
    let cells = vec![stock.cell; paths.len()];
    simulate_grids(
        stock,
        bottom,
        &|_| part.clone(),
        paths,
        &cells,
        opt,
        fixtures,
    )
}

/// [`simulate_with`] の、経路ごとに格子の大きさを変える形(荒削りは粗く、仕上げは細かく)。
/// `cells[k]` が `paths[k]` を削る格子の大きさ(足りなければ `stock.cell`)。大きさが変わる所で
/// 削った形を新しい格子へ移し(新しいセルの中心にあたる前のセルの高さ)、部品の高さは
/// `part_at`(新しい格子と同じ並びの部品の高さを返す)で作り直す。
///
/// 移した時点で部品より低い所(粗い格子の角が部品にかかった所や、前に報告した食い込み)は、
/// 後の経路の食い込みに数え直さない(新しく深く削った分だけを数える)。結果の形と削り残しは、
/// 最後の経路の格子で出す。
pub fn simulate_grids(
    stock: &ZMap,
    bottom: f64,
    part_at: &dyn Fn(&ZMap) -> ZMap,
    paths: &[Toolpath],
    cells: &[f64],
    opt: SimOptions,
    fixtures: &[(P3, P3)],
) -> SimReport {
    run(
        stock, bottom, part_at, paths, cells, opt, fixtures, None, false,
    )
}

/// [`simulate_grids`] に、機械の形 `machine` との当たりの検査を足したもの。
#[allow(clippy::too_many_arguments)]
pub fn simulate_on_machine(
    stock: &ZMap,
    bottom: f64,
    part_at: &dyn Fn(&ZMap) -> ZMap,
    paths: &[Toolpath],
    cells: &[f64],
    opt: SimOptions,
    fixtures: &[(P3, P3)],
    machine: Option<&MachineShape>,
) -> SimReport {
    run(
        stock, bottom, part_at, paths, cells, opt, fixtures, machine, false,
    )
}

/// [`simulate_grids`] の中身。`all` なら、削る所と食い込みを調べる所で飛ばさずに全部の
/// セルを見る(結果は同じ。比べる試験で使う)。
#[allow(clippy::too_many_arguments)]
fn run(
    stock: &ZMap,
    bottom: f64,
    part_at: &dyn Fn(&ZMap) -> ZMap,
    paths: &[Toolpath],
    cells: &[f64],
    opt: SimOptions,
    fixtures: &[(P3, P3)],
    machine: Option<&MachineShape>,
    all: bool,
) -> SimReport {
    let mut map = stock.clone();
    let mut part = part_at(&map);
    let mut check = part.eroded(opt.xy_cells);
    let mut track = Track::new(&map, &check, opt.z_tolerance, all);
    let mut gouges = Vec::new();
    let mut rapid_cuts = Vec::new();
    let mut by_op = Vec::new();
    let mut total = 0.0;
    let mut engagement = Vec::new();
    let mut collisions: Vec<Collision> = Vec::new();
    let mut fixture_hits: Vec<Collision> = Vec::new();
    // 材料のいちばん高い所(削ると下がるだけなので、始めの高さで足りる)
    let top = stock.z.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut machine_hits: Vec<MachineHit> = Vec::new();
    let mut bed_depth: f64 = 0.0;
    // 門の下を通らないストックと固定具は、経路によらず 1 件
    if let Some(c) = machine.and_then(|m| m.clearance) {
        let high = fixtures.iter().map(|f| f.1[2]).fold(top, f64::max);
        if high > c + 1e-6 {
            let (i, j) = (stock.nx / 2, stock.ny / 2);
            let at = stock.center(i, j);
            machine_hits.push(MachineHit {
                kind: MachineKind::Clearance,
                event: Event {
                    op: if high > top {
                        "固定具"
                    } else {
                        "ストック"
                    }
                    .into(),
                    index: 0,
                    at: [at[0], at[1], high],
                    amount: high - c,
                },
            });
        }
    }
    let mut stages = Vec::new();
    if opt.stage_cells > 0 {
        stages.push(("素材".to_string(), map.coarse(opt.stage_cells)));
    }
    for (k, tp) in paths.iter().enumerate() {
        let cell = cells.get(k).copied().unwrap_or(stock.cell);
        if (cell - map.cell).abs() > 1e-9 {
            map = map.resampled(cell);
            part = part_at(&map);
            check = part.eroded(opt.xy_cells);
            // 移す前からの低い所は、もう数えた(か、粗い格子の角の)分なので数え直さない
            for (c, z) in check.z.iter_mut().zip(&map.z) {
                if *z < *c - opt.z_tolerance {
                    *c = *z;
                }
            }
            track = Track::new(&map, &check, opt.z_tolerance, all);
        }
        let mut removed_here = 0.0;
        let step = (cell / 2.0).min(tp.tool.radius() / 4.0);
        let mut cur: Option<P3> = None;
        let mut eng = Engagement {
            op: tp.name.clone(),
            length: 0.0,
            mean: 0.0,
            max: 0.0,
            at_max: [0.0; 3],
            slot_length: 0.0,
        };
        // 当たりは、続く所の始めだけを数える(同じ壁に沿って何百件も出さない)
        let mut hitting = [false; 2];
        let mut on_fixture = false;
        // 種類ごとに、続いている当たりの番号(量は続く所でいちばん大きい値にする)。作業範囲の
        // はみ出しは、経路ごとに種類ごとの 1 件にまとめる(往復のたびに数えない)
        let mut on_machine: [Option<usize>; 12] = [None; 12];
        let mut travel_here: [Option<usize>; 6] = [None; 6];
        let nose = nose_height(&tp.tool);
        let parts = tool_parts(&tp.tool);
        let edge = Edge::new(&map, &tp.tool);
        let mut since = f64::INFINITY;
        // レーザーは焼くだけで材料を削らない
        let moves: &[Move] = if tp.laser_power().is_some() {
            &[]
        } else {
            &tp.moves
        };
        for (index, m) in moves.iter().enumerate() {
            let Some(from) = cur else {
                cur = Some(m.to());
                continue;
            };
            let rapid = matches!(m, Move::Rapid { .. });
            let mut prev = from;
            let pts = m.points(from, step);
            let last = pts.len().saturating_sub(1);
            for (n, q) in pts.into_iter().enumerate() {
                since += (q[0] - prev[0]).hypot(q[1] - prev[1]) + (q[2] - prev[2]).abs();
                if (since >= COLLISION_STEP || n == last) && !fixtures.is_empty() {
                    let hit = fixture_hit(&parts, q, fixtures);
                    if let Some((part, amount)) = hit
                        && !on_fixture
                        && fixture_hits.len() < 100
                    {
                        fixture_hits.push(Collision {
                            part,
                            event: Event {
                                op: tp.name.clone(),
                                index,
                                at: q,
                                amount,
                            },
                        });
                    }
                    on_fixture = hit.is_some();
                }
                if let Some(shape) = machine
                    && (since >= COLLISION_STEP || n == last)
                {
                    let found = machine_at(
                        shape,
                        &map,
                        fixtures,
                        tp.tool.radius(),
                        nose,
                        q,
                        top,
                        opt.z_tolerance,
                    );
                    let mut now: [Option<usize>; 12] = [None; 12];
                    for (kind, amount) in found {
                        let k = kind.slot();
                        let open = on_machine[k].or(travel_here.get(k).copied().flatten());
                        if let Some(h) = open {
                            let e = &mut machine_hits[h].event;
                            e.amount = e.amount.max(amount);
                            now[k] = Some(h);
                        } else if machine_hits.len() < 100 {
                            now[k] = Some(machine_hits.len());
                            if let Some(t) = travel_here.get_mut(k) {
                                *t = Some(machine_hits.len());
                            }
                            machine_hits.push(MachineHit {
                                kind,
                                event: Event {
                                    op: tp.name.clone(),
                                    index,
                                    at: q,
                                    amount,
                                },
                            });
                        }
                    }
                    on_machine = now;
                    if let Some((_, bhi)) = shape.bed
                        && q[2] < bhi[2] - opt.z_tolerance
                    {
                        bed_depth = bed_depth.max(bhi[2] - q[2]);
                    }
                }
                if (since >= COLLISION_STEP || n == last) && collisions.len() < 100 {
                    since = 0.0;
                    let found = collisions_at(&map, &tp.tool, q, top, opt.z_tolerance);
                    for (k, part) in [ToolPart::Shank, ToolPart::Holder].into_iter().enumerate() {
                        let hit = found.iter().find(|f| f.0 == part);
                        if let Some(&(_, amount)) = hit
                            && !hitting[k]
                        {
                            collisions.push(Collision {
                                part,
                                event: Event {
                                    op: tp.name.clone(),
                                    index,
                                    at: q,
                                    amount,
                                },
                            });
                        }
                        hitting[k] = hit.is_some();
                    }
                }
                // 横に動く削り(下りるだけの動きは除く)なら、削る前のかかり具合を測る
                let (dxy, dz) = (
                    (q[0] - prev[0]).hypot(q[1] - prev[1]),
                    (q[2] - prev[2]).abs(),
                );
                let angle = (!rapid && dxy > 1e-9 && dxy >= dz)
                    .then(|| engaged_angle(&map, &edge, prev, q));
                prev = q;
                let removed = stamp(&mut map, &tp.tool, q, bottom, None, Some(&mut track));
                track.refresh(&map, &check, opt.z_tolerance);
                if let Some(angle) = angle.filter(|_| removed > 0.0) {
                    eng.length += dxy;
                    eng.mean += angle * dxy;
                    if angle > eng.max {
                        eng.max = angle;
                        eng.at_max = q;
                    }
                    if angle >= SLOT_ANGLE {
                        eng.slot_length += dxy;
                    }
                }
                if removed > 0.0 {
                    removed_here += removed;
                    if rapid && removed > 1e-6 && rapid_cuts.len() < 100 {
                        rapid_cuts.push(Event {
                            op: tp.name.clone(),
                            index,
                            at: q,
                            amount: removed,
                        });
                    }
                    if gouges.len() < 100
                        && let Some(depth) = gouge_near(
                            &map,
                            &mut check,
                            &mut track,
                            &tp.tool,
                            q,
                            opt.z_tolerance,
                            tp.edge_break,
                        )
                    {
                        gouges.push(Event {
                            op: tp.name.clone(),
                            index,
                            at: q,
                            amount: depth,
                        });
                    }
                }
            }
            cur = Some(m.to());
        }
        total += removed_here;
        by_op.push((tp.name.clone(), removed_here));
        if opt.stage_cells > 0 {
            stages.push((tp.name.clone(), map.coarse(opt.stage_cells)));
        }
        if eng.length > 0.0 {
            eng.mean /= eng.length;
            engagement.push(eng);
        }
    }
    // 最後の経路の格子で数える(経路がなければ始めの格子のまま)
    let base = ZMap {
        z: part
            .z
            .iter()
            .map(|z| z.max(bottom) + opt.remaining_tolerance)
            .collect(),
        ..part.clone()
    };
    let floor = ZMap {
        z: vec![bottom; stock.z.len()],
        ..stock.clone()
    };
    SimReport {
        stock_volume: stock.volume_above(&floor),
        removed_volume: total,
        remaining_volume: map.volume_above(&base),
        gouges,
        rapid_cuts,
        collisions,
        fixture_hits,
        machine_hits,
        bed_depth,
        removed_by_op: by_op,
        engagement,
        leftovers: leftover_marks(&map, &base, opt.remaining_tolerance),
        stages,
        result: map,
    }
}

/// 削り残しの印にする区画の大きさ(mm)と、印の数の上限。
const LEFT_BUCKET: f64 = 1.0;
const LEFT_MAX: usize = 2000;

/// 削り残しの印。区画ごとに、`base`(部品に許しを足した高さ)より最も高く残った所の、
/// 残った面の上の位置と、部品との高さの差。
fn leftover_marks(map: &ZMap, base: &ZMap, tol: f64) -> Vec<(P3, f64)> {
    let b = ((LEFT_BUCKET / map.cell).round() as usize).max(1);
    let mut out = Vec::new();
    for bj in (0..map.ny).step_by(b) {
        for bi in (0..map.nx).step_by(b) {
            let mut best: Option<(usize, f64)> = None;
            for j in bj..(bj + b).min(map.ny) {
                for i in bi..(bi + b).min(map.nx) {
                    let k = j * map.nx + i;
                    let d = map.z[k] - base.z[k];
                    if d > 0.0 && best.is_none_or(|w| d > w.1) {
                        best = Some((k, d));
                    }
                }
            }
            if let Some((k, d)) = best {
                let c = map.center(k % map.nx, k / map.nx);
                out.push(([c[0], c[1], map.z[k]], d + tol));
            }
        }
    }
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out.truncate(LEFT_MAX);
    out
}

/// 工具の周りのセルで、部品より深く削った所があればその深さ。
/// 工具の届く範囲で、部品より深く削ったセルの最も深い量。`allow`(面取りで意図して
/// 削る深さ)までのものは食い込みとせず、そのセルの部品の高さを削った後の高さに下げる
/// (後の経路が同じ所を通っても食い込みと見なさないため)。
///
/// 部品より `tol` を超えて低いセル([`Track::deep`])だけを見る(ほかのセルは何もしない)。
fn gouge_near(
    map: &ZMap,
    check: &mut ZMap,
    track: &mut Track,
    tool: &Tool,
    tip: P3,
    tol: f64,
    allow: f64,
) -> Option<f64> {
    let r = tool.radius();
    let reach = (r / map.cell).ceil() as isize + 1;
    let ci = ((tip[0] - map.origin[0]) / map.cell - 0.5).round() as isize;
    let cj = ((tip[1] - map.origin[1]) / map.cell - 0.5).round() as isize;
    let mut worst: Option<f64> = None;
    for dj in -reach..=reach {
        let j = cj + dj;
        if j < 0 || j >= map.ny as isize || (!track.all && track.deep_rows[j as usize] == 0) {
            continue;
        }
        let j = j as usize;
        for di in -reach..=reach {
            let i = ci + di;
            if i < 0 || i >= map.nx as isize {
                continue;
            }
            let k = j * map.nx + i as usize;
            if !track.all && !track.deep[k] {
                continue;
            }
            let d = check.z[k] - map.z[k];
            if d > tol && d <= tol + allow {
                check.z[k] = map.z[k];
                if !track.all {
                    track.deep[k] = false;
                    track.deep_rows[j] -= 1;
                }
                continue;
            }
            if d > tol {
                worst = Some(worst.map_or(d, |w: f64| w.max(d)));
            }
        }
    }
    worst
}

/// プログラムから経路を取り出す(工具交換の間の動きを 1 つの経路にまとめる)。
pub fn paths_of(p: &Program, safe_z: f64, retract_z: f64) -> Vec<Toolpath> {
    let mut out: Vec<Toolpath> = Vec::new();
    let mut name = String::new();
    let mut rpm = 0.0;
    for op in &p.ops {
        match op {
            Op::Comment { text } => name = text.clone(),
            Op::Spindle { rpm: r } => rpm = *r,
            Op::ToolChange { tool } => out.push(Toolpath {
                name: name.clone(),
                tool: (**tool).clone(),
                rpm,
                safe_z,
                retract_z,
                edge_break: 0.0,
                dwells: vec![],
                spans: vec![],
                links: Default::default(),
                moves: vec![],
            }),
            Op::Move(m) => {
                if let Some(t) = out.last_mut() {
                    t.moves.push(*m);
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests;
