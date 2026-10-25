//! 動きの単位の再生。経路を時間の軸に並べ、決めた時刻まで粗い格子のストックを削る。
//!
//! 動きの時間は時間の目安と同じ見積もり([`Toolpath::move_seconds`])で、時間の軸の長さは
//! 時間の目安の合計になる。前へ進む時は今の形から続けて削る。後ろへ戻る時は、一定の数の動き
//! ごとに取っておいた形の写しから削り直す(格子のセルに命令の番号を結び付けて戻す作りに
//! しない。docs/避ける形.md)。
//!
//! 動きの途中の時刻では、動きの始めからその時刻の位置までを削った形を見せる(続けて削る形には
//! 入れない)。

use crate::{ZMap, stamp};
use jc_cam_tool::{Tool, ToolKind};
use jc_core::Mesh;
use jc_toolpath::{Motion, Move, P3, Toolpath};

/// 写しを取る数の上限(格子 1 枚ぶんの大きさ × この数だけ覚える)。
const MAX_SNAPSHOTS: usize = 100;
/// 写しの間の動きの数の下限。
const MIN_SNAPSHOT_GAP: usize = 50;

/// 時間の軸の上の 1 つの動き。
#[derive(Debug, Clone)]
struct Step {
    path: usize,
    index: usize,
    from: P3,
    t0: f64,
    t1: f64,
}

/// ある時刻の工具の様子。
#[derive(Debug, Clone, PartialEq)]
pub struct PlayState {
    /// 時刻(秒、始めから)
    pub at: f64,
    /// 工具の先端の位置(セットアップの座標)
    pub tip: P3,
    /// 経路の番号と、経路の中の何番目の動きか(終わりまで進めたら最後の動き)
    pub path: usize,
    pub index: usize,
    /// 早送りか
    pub rapid: bool,
    /// 送り(mm/min)。早送りなら機械の早送りの速さ
    pub feed: f64,
}

/// ストックの色の付け方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Coloring {
    /// 材料の色 1 つ
    #[default]
    Material,
    /// 最後に削った操作ごとの色
    Operation,
    /// 部品と比べる: 許容内・削り不足・削り過ぎ
    Deviation,
}

/// 比べる時の面の名前(この順の番号を [`Player::mesh`] が面に付ける)。
pub const DEVIATION_FACES: [&str; 4] = ["途中の形", "許容内", "削り不足", "削り過ぎ"];

/// 動きの単位の再生。
pub struct Player {
    start: ZMap,
    bottom: f64,
    part: ZMap,
    paths: Vec<Toolpath>,
    rapid_feed: f64,
    steps: Vec<Step>,
    total: f64,
    /// (済んだ動きの数、その時の形、最後に削った経路の番号 + 1)
    snaps: Vec<(usize, ZMap, Vec<u16>)>,
    gap: usize,
    map: ZMap,
    owner: Vec<u16>,
    done: usize,
    /// 見せる形(済んだ動きに、今の動きの途中までを足したもの)
    shown: ZMap,
    shown_owner: Vec<u16>,
    state: PlayState,
}

impl Player {
    /// `start` は素材の格子(粗くてよい)、`part` は同じ格子の部品の高さ(部品がない所は負の無限大)。
    pub fn new(
        start: ZMap,
        bottom: f64,
        part: ZMap,
        paths: Vec<Toolpath>,
        motion: &Motion,
    ) -> Player {
        let mut steps = Vec::new();
        let mut t = 0.0;
        for (pi, tp) in paths.iter().enumerate() {
            let secs = tp.move_seconds(motion);
            let mut cur: Option<P3> = None;
            for (index, m) in tp.moves.iter().enumerate() {
                let from = cur.unwrap_or(m.to());
                let d = secs.get(index).copied().unwrap_or(0.0);
                steps.push(Step {
                    path: pi,
                    index,
                    from,
                    t0: t,
                    t1: t + d,
                });
                t += d;
                cur = Some(m.to());
            }
        }
        let gap = (steps.len() / MAX_SNAPSHOTS).max(MIN_SNAPSHOT_GAP);
        let owner = vec![0; start.z.len()];
        let state = PlayState {
            at: 0.0,
            tip: steps.first().map_or([0.0; 3], |s| s.from),
            path: 0,
            index: 0,
            rapid: true,
            feed: motion.rapid_feed,
        };
        Player {
            snaps: vec![(0, start.clone(), owner.clone())],
            map: start.clone(),
            shown: start.clone(),
            shown_owner: owner.clone(),
            owner,
            start,
            bottom,
            part,
            paths,
            rapid_feed: motion.rapid_feed,
            steps,
            total: t,
            gap,
            done: 0,
            state,
        }
    }

    /// 全体の時間(秒)。
    pub fn total(&self) -> f64 {
        self.total
    }

    pub fn paths(&self) -> &[Toolpath] {
        &self.paths
    }

    pub fn state(&self) -> &PlayState {
        &self.state
    }

    /// 経路ごとの始まりの時刻(秒)。
    pub fn starts(&self) -> Vec<f64> {
        let mut out = vec![f64::NAN; self.paths.len()];
        for s in self.steps.iter().rev() {
            out[s.path] = s.t0;
        }
        out
    }

    /// 動きの数(全部の経路)と、今の動きの通しの番号。
    pub fn move_count(&self) -> usize {
        self.steps.len()
    }

    /// 今の時刻の動きの通しの番号。
    pub fn current_step(&self) -> usize {
        self.steps
            .partition_point(|s| s.t1 <= self.state.at)
            .min(self.steps.len().saturating_sub(1))
    }

    /// 通しの番号 `k` の動きが終わる時刻。
    pub fn step_end(&self, k: usize) -> f64 {
        self.steps.get(k).map_or(self.total, |s| s.t1)
    }

    /// 通しの番号 `k` の動きが始まる時刻。
    pub fn step_start(&self, k: usize) -> f64 {
        self.steps.get(k).map_or(0.0, |s| s.t0)
    }

    fn cut_step(&mut self, k: usize, upto: Option<f64>, shown: bool) {
        let s = &self.steps[k];
        let tp = &self.paths[s.path];
        if s.index == 0 {
            return;
        }
        let m = tp.moves[s.index];
        let step = (self.start.cell / 2.0).min(tp.tool.radius() / 4.0);
        let pts = m.points(s.from, step);
        let n = match upto {
            Some(f) => ((pts.len() as f64) * f).floor() as usize,
            None => pts.len(),
        };
        let tag = s.path as u16 + 1;
        let (map, owner) = if shown {
            (&mut self.shown, &mut self.shown_owner)
        } else {
            (&mut self.map, &mut self.owner)
        };
        for q in pts.into_iter().take(n) {
            stamp(map, &tp.tool, q, self.bottom, Some((owner, tag)), None);
        }
    }

    /// 時刻 `t`(秒)まで進める(戻す)。
    pub fn seek(&mut self, t: f64) -> &PlayState {
        let t = t.clamp(0.0, self.total);
        // 済ませる動きの数(終わる時刻が t 以下の動き)
        let k = self.steps.partition_point(|s| s.t1 <= t);
        if k < self.done {
            let (n, map, owner) = self
                .snaps
                .iter()
                .rev()
                .find(|s| s.0 <= k)
                .cloned()
                .unwrap_or((0, self.start.clone(), vec![0; self.start.z.len()]));
            self.done = n;
            self.map = map;
            self.owner = owner;
        }
        while self.done < k {
            self.cut_step(self.done, None, false);
            self.done += 1;
            if self.done.is_multiple_of(self.gap)
                && self.snaps.last().is_some_and(|s| s.0 < self.done)
            {
                self.snaps
                    .push((self.done, self.map.clone(), self.owner.clone()));
            }
        }
        self.shown.z.clone_from(&self.map.z);
        self.shown_owner.clone_from(&self.owner);
        let (path, index, tip, rapid, feed) = match self.steps.get(k) {
            Some(s) => {
                let f = if s.t1 > s.t0 {
                    ((t - s.t0) / (s.t1 - s.t0)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                self.cut_step(k, Some(f), true);
                let s = &self.steps[k];
                let m = self.paths[s.path].moves[s.index];
                let tip = at_fraction(&m, s.from, f);
                let (rapid, feed) = kind(&m, self.rapid_feed);
                (s.path, s.index, tip, rapid, feed)
            }
            None => match self.steps.last() {
                Some(s) => {
                    let m = self.paths[s.path].moves[s.index];
                    let (rapid, feed) = kind(&m, self.rapid_feed);
                    (s.path, s.index, m.to(), rapid, feed)
                }
                None => (0, 0, [0.0; 3], true, self.rapid_feed),
            },
        };
        self.state = PlayState {
            at: t,
            tip,
            path,
            index,
            rapid,
            feed,
        };
        &self.state
    }

    /// 見せる形(済んだ動きと、今の動きの途中まで)。
    pub fn shown(&self) -> &ZMap {
        &self.shown
    }

    /// 見せる形の網(セットアップの座標)。色の付け方 `coloring` で上の面を面に分ける。
    /// `tol` は比べる時の許容差(mm)。面の名前は、材料なら「途中の形」1 つ、操作ごとなら
    /// 「途中の形」と経路の名前、比べるなら [`DEVIATION_FACES`]。
    pub fn mesh(&self, coloring: Coloring, tol: f64) -> Mesh {
        let map = &self.shown;
        match coloring {
            Coloring::Material => map.to_mesh(self.bottom),
            Coloring::Operation => {
                let names: Vec<&str> = std::iter::once("途中の形")
                    .chain(self.paths.iter().map(|p| p.name.as_str()))
                    .collect();
                map.to_mesh_with(self.bottom, &names, |k| self.shown_owner[k] as u32)
            }
            Coloring::Deviation => map.to_mesh_with(self.bottom, &DEVIATION_FACES, |k| {
                let (z, p) = (map.z[k], self.part.z[k]);
                if p.is_finite() && p - z > tol {
                    3
                } else if z - p.max(self.bottom) > tol {
                    2
                } else {
                    1
                }
            }),
        }
    }
}

/// 動きが早送りか、と送り。
fn kind(m: &Move, rapid_feed: f64) -> (bool, f64) {
    match *m {
        Move::Rapid { .. } => (true, rapid_feed),
        Move::Linear { feed, .. } | Move::Arc { feed, .. } => (false, feed),
    }
}

/// 動き `m` の、始まり `from` から長さの割合 `f` の位置。
fn at_fraction(m: &Move, from: P3, f: f64) -> P3 {
    if f >= 1.0 {
        return m.to();
    }
    let pts = m.points(from, 0.1);
    let mut all = Vec::with_capacity(pts.len() + 1);
    all.push(from);
    all.extend(pts);
    let len: f64 = all.windows(2).map(|w| jc_toolpath::dist(w[0], w[1])).sum();
    let mut want = len * f;
    for w in all.windows(2) {
        let d = jc_toolpath::dist(w[0], w[1]);
        if d >= want && d > 0.0 {
            let u = want / d;
            return [0, 1, 2].map(|i| w[0][i] + (w[1][i] - w[0][i]) * u);
        }
        want -= d;
    }
    m.to()
}

/// 工具の網(セットアップの座標、先端が `tip`)。面は「刃」「シャンク」「ホルダー」(ホルダーの
/// 径を決めていない工具は描かない)。突き出しを決めていなければ、シャンクは刃の上 10 mm まで。
pub fn tool_mesh(tool: &Tool, tip: P3) -> Mesh {
    const N: usize = 24;
    let r = tool.radius();
    let flute = tool.flute_length.max(r * 0.5);
    // 刃の外形(半径, 先端からの高さ)を下から
    let mut cutter: Vec<(f64, f64)> = vec![(0.0, 0.0)];
    match tool.kind {
        ToolKind::FlatEnd => cutter.push((r, 0.0)),
        // ねじ切りミルは歯の山と首(上の端も首の径)
        ToolKind::ThreadMill => {
            for [h, rr] in tool.thread_outline().unwrap_or_default() {
                cutter.push((rr, h));
            }
        }
        _ => {
            if tool.kind != ToolKind::BallEnd && tool.tip_radius() > 0.0 {
                cutter.push((tool.tip_radius(), 0.0));
            }
            for k in 1..=8 {
                let d = tool.tip_radius() + (r - tool.tip_radius()) * k as f64 / 8.0;
                if let Some(h) = tool.height_at(d.min(r)) {
                    cutter.push((d, h.min(flute)));
                }
            }
        }
    }
    if tool.kind != ToolKind::ThreadMill {
        cutter.push((r, flute));
    }
    let shank_top = tool.stickout.unwrap_or(flute + 10.0).max(flute);
    let rs = tool.shank() / 2.0;
    let shank = vec![(rs, flute), (rs, shank_top), (0.0, shank_top)];
    let mut parts = vec![("刃", cutter), ("シャンク", shank)];
    if let Some(hd) = tool.holder_diameter {
        let top = shank_top + 30.0;
        parts.push((
            "ホルダー",
            vec![(hd / 2.0, shank_top), (hd / 2.0, top), (0.0, top)],
        ));
    }
    let mut m = Mesh {
        face_names: parts.iter().map(|p| p.0.to_string()).collect(),
        ..Default::default()
    };
    for (face, (_, prof)) in parts.iter().enumerate() {
        for w in prof.windows(2) {
            let ((r0, z0), (r1, z1)) = (w[0], w[1]);
            // 輪の帯。法線は外形の線の向きに直角(外向き)
            let (dr, dz) = (r1 - r0, z1 - z0);
            let l = dr.hypot(dz).max(1e-9);
            let (nr, nz) = (dz / l, -dr / l);
            let base = m.positions.len() as u32;
            for k in 0..=N {
                let a = std::f64::consts::TAU * k as f64 / N as f64;
                let (c, s) = (a.cos(), a.sin());
                for (rr, zz) in [(r0, z0), (r1, z1)] {
                    m.positions.push([
                        (tip[0] + rr * c) as f32,
                        (tip[1] + rr * s) as f32,
                        (tip[2] + zz) as f32,
                    ]);
                    m.normals
                        .push([(nr * c) as f32, (nr * s) as f32, nz as f32]);
                }
            }
            for k in 0..N as u32 {
                let (a, b, c, d) = (
                    base + 2 * k,
                    base + 2 * k + 1,
                    base + 2 * k + 2,
                    base + 2 * k + 3,
                );
                m.triangles.push([a, c, d]);
                m.triangles.push([a, d, b]);
                m.triangle_faces.extend([face as u32, face as u32]);
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SimOptions, simulate};
    use jc_cam_tool::ToolLibrary;

    fn grooves() -> Vec<Toolpath> {
        let tool = ToolLibrary::starter().tools[0].clone();
        let mk = |name: &str, y: f64, z: f64| Toolpath {
            name: name.into(),
            tool: tool.clone(),
            rpm: 10000.0,
            safe_z: 5.0,
            retract_z: 1.0,
            edge_break: 0.0,
            dwells: vec![],
            links: Default::default(),
            moves: vec![
                Move::Rapid { to: [-5.0, y, 5.0] },
                Move::Rapid { to: [-5.0, y, 1.0] },
                Move::Linear {
                    to: [-5.0, y, z],
                    feed: 200.0,
                },
                Move::Linear {
                    to: [25.0, y, z],
                    feed: 600.0,
                },
                Move::Rapid { to: [25.0, y, 5.0] },
            ],
            spans: vec![],
        };
        vec![mk("溝 1", 5.0, -2.0), mk("溝 2", 14.0, -3.0)]
    }

    fn player() -> Player {
        let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.25);
        let part = ZMap {
            z: vec![f64::NEG_INFINITY; stock.z.len()],
            ..stock.clone()
        };
        Player::new(stock, -10.0, part, grooves(), &Motion::simple(3000.0))
    }

    #[test]
    fn the_end_matches_the_whole_simulation_and_going_back_matches_going_forward() {
        let mut p = player();
        // 時間の軸は時間の目安の合計
        let want: f64 = grooves()
            .iter()
            .map(|t| t.minutes(&Motion::simple(3000.0)) * 60.0)
            .sum();
        assert!((p.total() - want).abs() < 1e-9, "{} {want}", p.total());
        let end = p.total();
        p.seek(end);
        let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.25);
        let part = ZMap {
            z: vec![f64::NEG_INFINITY; stock.z.len()],
            ..stock.clone()
        };
        let r = simulate(
            &stock,
            -10.0,
            &part,
            &grooves(),
            SimOptions {
                cell: 0.25,
                ..SimOptions::default()
            },
        );
        assert_eq!(p.shown().z, r.result.z);
        // 途中へ戻ると、そこまで前から削った形と同じ
        let mid = p.starts()[1] + 1.0;
        let back = p.seek(mid).clone();
        let mut fresh = player();
        fresh.seek(mid);
        assert_eq!(p.shown().z, fresh.shown().z);
        assert_eq!(back.path, 1);
        // 2 本目の溝はまだ途中: 溝 1 は削れ、溝 2 の先は残っている
        let s = p.shown();
        let at = |x: f64, y: f64| {
            let i = ((x - s.origin[0]) / s.cell) as usize;
            let j = ((y - s.origin[1]) / s.cell) as usize;
            s.z[j * s.nx + i]
        };
        assert!((at(15.0, 5.0) + 2.0).abs() < 1e-9);
        assert_eq!(at(18.0, 14.0), 0.0);
    }

    #[test]
    fn the_state_tells_where_the_tool_is_and_how_it_moves() {
        let mut p = player();
        // 溝 1 の横の切削の真ん中: 送り 600、(10, 5, -2) 付近
        let s0 = p.starts()[0];
        let secs = grooves()[0].move_seconds(&Motion::simple(3000.0));
        let t = s0 + secs[1] + secs[2] + secs[3] / 2.0;
        let st = p.seek(t).clone();
        assert!(!st.rapid && st.feed == 600.0, "{st:?}");
        assert_eq!((st.path, st.index), (0, 3));
        assert!(
            (st.tip[0] - 10.0).abs() < 0.2 && st.tip[2] == -2.0,
            "{st:?}"
        );
        // 操作ごとの色: 溝 1 で削った所は 1 番、溝 2 の所は 2 番
        let end = p.total();
        p.seek(end);
        let m = p.mesh(Coloring::Operation, 0.05);
        assert_eq!(m.face_names, ["途中の形", "溝 1", "溝 2"]);
        assert!(m.triangle_faces.contains(&1) && m.triangle_faces.contains(&2));
        assert_eq!(m.triangle_faces.len(), m.triangles.len());
    }

    #[test]
    fn deviation_colors_show_leftover_and_gouge() {
        let stock = ZMap::stock([0.0, 0.0, -10.0], [20.0, 20.0, 0.0], 0.25);
        // 部品の上面は -2.5: 溝 1(-2)は削り不足、溝 2(-3)は削り過ぎ、ほかは削り不足
        let part = ZMap {
            z: vec![-2.5; stock.z.len()],
            ..stock.clone()
        };
        let mut p = Player::new(stock, -10.0, part, grooves(), &Motion::simple(3000.0));
        let end = p.total();
        p.seek(end);
        let m = p.mesh(Coloring::Deviation, 0.05);
        assert!(m.triangle_faces.contains(&2) && m.triangle_faces.contains(&3));
        assert!(!m.triangle_faces.contains(&1));
    }

    #[test]
    fn the_tool_mesh_stands_on_the_tip() {
        let mut t = ToolLibrary::starter().tools[0].clone();
        t.holder_diameter = Some(20.0);
        t.stickout = Some(25.0);
        let m = tool_mesh(&t, [1.0, 2.0, 3.0]);
        assert_eq!(m.face_names, ["刃", "シャンク", "ホルダー"]);
        let zmin = m.positions.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
        let zmax = m.positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        assert!(
            (zmin - 3.0).abs() < 1e-6 && (zmax - 58.0).abs() < 1e-4,
            "{zmin} {zmax}"
        );
        assert_eq!(m.triangles.len(), m.triangle_faces.len());
        let xmax = m.positions.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        assert!((xmax - 11.0).abs() < 1e-4);
    }
}
