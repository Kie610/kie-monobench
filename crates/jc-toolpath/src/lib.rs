//! 機械に依存しない経路の中間表現。
//!
//! 単位は mm と mm/min に固定する。座標はセットアップの作業座標系(WCS)で表す。
//! 円弧は XY 平面(G17)だけを使い、Z が変わる円弧はらせん(ヘリカル)として扱う。
//!
//! 加工の機能は [`Toolpath`] を出し、[`Program::build`] がそれを順に並べて、
//! 工具交換、主軸の起動と停止、退避を足したプログラムにする。ポストはこれを文字列にするだけで、
//! 経路を変えない。

use jc_cam_tool::Tool;
use serde::{Deserialize, Serialize};

pub type P3 = [f64; 3];

pub const TOOLPATH_TYPE: &str = "cam.toolpath@1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArcDir {
    /// 時計回り(G2)
    Cw,
    /// 反時計回り(G3)
    Ccw,
}

/// 工具の動き。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Move {
    /// 早送り。材料の外(ストックの上)だけで使う
    Rapid { to: P3 },
    /// 直線の切削送り
    Linear { to: P3, feed: f64 },
    /// XY 平面の円弧。`center` は絶対座標で、Z は使わない。Z が変わればらせん
    Arc {
        to: P3,
        center: [f64; 2],
        dir: ArcDir,
        feed: f64,
    },
}

impl Move {
    pub fn to(&self) -> P3 {
        match *self {
            Move::Rapid { to } | Move::Linear { to, .. } | Move::Arc { to, .. } => to,
        }
    }

    /// `from` から、この動きに沿って `step` 以下の間隔で点を並べる(`from` は含めない)。
    pub fn points(&self, from: P3, step: f64) -> Vec<P3> {
        match *self {
            Move::Rapid { to } | Move::Linear { to, .. } => {
                let d = dist(from, to);
                let n = ((d / step).ceil() as usize).max(1);
                (1..=n)
                    .map(|i| lerp(from, to, i as f64 / n as f64))
                    .collect()
            }
            Move::Arc {
                to, center, dir, ..
            } => {
                let sweep = arc_sweep(from, to, center, dir);
                let r = ((from[0] - center[0]).hypot(from[1] - center[1])
                    + (to[0] - center[0]).hypot(to[1] - center[1]))
                    / 2.0;
                let len = (sweep.abs() * r).hypot(to[2] - from[2]);
                let n = ((len / step).ceil() as usize).max(1);
                let a0 = (from[1] - center[1]).atan2(from[0] - center[0]);
                let r0 = (from[0] - center[0]).hypot(from[1] - center[1]);
                let r1 = (to[0] - center[0]).hypot(to[1] - center[1]);
                (1..=n)
                    .map(|i| {
                        let t = i as f64 / n as f64;
                        if i == n {
                            return to;
                        }
                        let a = a0 + sweep * t;
                        let rr = r0 + (r1 - r0) * t;
                        [
                            center[0] + rr * a.cos(),
                            center[1] + rr * a.sin(),
                            from[2] + (to[2] - from[2]) * t,
                        ]
                    })
                    .collect()
            }
        }
    }
}

/// 円弧の回転角(反時計回りが正)。始点と終点が同じなら一周。
pub fn arc_sweep(from: P3, to: P3, center: [f64; 2], dir: ArcDir) -> f64 {
    use std::f64::consts::TAU;
    let a0 = (from[1] - center[1]).atan2(from[0] - center[0]);
    let a1 = (to[1] - center[1]).atan2(to[0] - center[0]);
    let mut s = a1 - a0;
    match dir {
        ArcDir::Ccw => {
            while s <= 1e-9 {
                s += TAU;
            }
            while s > TAU + 1e-9 {
                s -= TAU;
            }
        }
        ArcDir::Cw => {
            while s >= -1e-9 {
                s -= TAU;
            }
            while s < -TAU - 1e-9 {
                s += TAU;
            }
        }
    }
    s
}

pub fn dist(a: P3, b: P3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn lerp(a: P3, b: P3, t: f64) -> P3 {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// 1 つの加工操作の経路。最初の動きは安全高さへの早送り、最後は安全高さへの退避にする。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Toolpath {
    /// 加工操作の名前(G-code のコメントに使う)
    pub name: String,
    pub tool: Tool,
    pub rpm: f64,
    /// 横に早送りしてよい最も低い高さ
    pub safe_z: f64,
    /// 下へ早送りしてよい最も低い高さ(材料の上面より上)
    pub retract_z: f64,
    pub moves: Vec<Move>,
    /// 部品の縁を意図して削る深さ(mm)。面取り加工の経路は形にない面取りを削るので、
    /// シミュレーションはこの深さまでの食い込みを許す。0 なら許さない
    #[serde(default, skip_serializing_if = "is_zero")]
    pub edge_break: f64,
    /// 送りの動きの終わりで止まる所(穴の底での停留)。動きの番号ではなく位置で結び付けるので、
    /// 円弧のあてはめや固定具を越える持ち上げで動きの数が変わっても外れない
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dwells: Vec<Dwell>,
    /// 動きの役目の印(入り・リード・つなぎ)。画面で色を分けるのに使い、G-code は変えない。
    /// 印のない動きは、早送りか切削
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<Span>,
    /// 削っていない間の動き(つなぎと空の動き)。`jc-toolpath-moves` のつなぎが付ける
    #[serde(default, skip_serializing_if = "LinkInfo::is_empty")]
    pub links: LinkInfo,
}

/// 送りの動きで `at` に着いた所で、`seconds` 秒止まる。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dwell {
    pub at: P3,
    pub seconds: f64,
}

impl Toolpath {
    /// レーザーの経路なら、その出力(0〜1)。レーザーの工具では `rpm` に出力(%)を入れる。
    pub fn laser_power(&self) -> Option<f64> {
        (self.tool.kind == jc_cam_tool::ToolKind::Laser).then(|| (self.rpm / 100.0).clamp(0.0, 1.0))
    }

    /// 動き `m` の終わりで止まる秒数(止まらなければ 0)。早送りでは止まらない。
    pub fn dwell_after(&self, m: &Move) -> f64 {
        if matches!(m, Move::Rapid { .. }) {
            return 0.0;
        }
        let to = m.to();
        self.dwells
            .iter()
            .filter(|d| dist(d.at, to) < 1e-6)
            .map(|d| d.seconds)
            .sum()
    }
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// 動きの役目。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// 材料へ下りる動き(らせん・斜め・真下・下穴・ストックの外から)
    Entry,
    /// 壁に沿い始める・離れる動き(円弧や直線のリード)
    Lead,
    /// 削らずに送りで移る・下げる動き(前の段の高さまでの下げ、削った所の中の移動)
    Link,
}

/// `moves[start..end]` の役目。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub role: Role,
}

/// つなぎ(削り終えた所から次に削り始める所への動き)の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// 削った所の中を、下げたまま切削の送りで移る
    StayDown,
    /// 通り道の材料と部品の上へ少し持ち上げ、切削の送りで移る
    Lift,
    /// 通り道に合わせた、安全高さより低い高さで早送りする
    Low,
    /// 安全高さまで上げて早送りする(全退避)
    Full,
}

/// 1 つのつなぎ。`start..end` は経路の動きの番号。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub kind: LinkKind,
    /// 横に動く高さ
    pub z: f64,
}

/// 経路の、削っていない間の動きの記録。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkInfo {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    /// 材料を削らない動き(早送りと空を切る送り)の番号の範囲 `[始め, 終わり)`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub air: Vec<[usize; 2]>,
}

impl LinkInfo {
    pub fn is_empty(&self) -> bool {
        self.links.is_empty() && self.air.is_empty()
    }

    /// 種類 `kind` のつなぎの数。
    pub fn count(&self, kind: LinkKind) -> usize {
        self.links.iter().filter(|l| l.kind == kind).count()
    }

    /// 動き `i` を含むつなぎ。
    pub fn link_at(&self, i: usize) -> Option<&Link> {
        self.links.iter().find(|l| l.start <= i && i < l.end)
    }

    /// 動きの番号を付け替える(`map(i)` は元の動き `i` の新しい番号。列の終わり `len` も渡る)。
    /// 動きを足したり分けたりする後処理が、記録を動きに合わせて保つために使う。
    pub fn remap(&mut self, map: impl Fn(usize) -> usize) {
        for l in &mut self.links {
            l.start = map(l.start);
            l.end = map(l.end);
        }
        for r in &mut self.air {
            *r = [map(r[0]), map(r[1])];
        }
    }

    /// 動き `len` 個の経路を `copies` 回つないだ経路の記録(写しごとに番号をずらす)。
    pub fn repeat(&self, len: usize, copies: usize) -> LinkInfo {
        let mut out = LinkInfo::default();
        for k in 0..copies {
            let d = k * len;
            out.links.extend(self.links.iter().map(|l| Link {
                start: l.start + d,
                end: l.end + d,
                ..*l
            }));
            out.air
                .extend(self.air.iter().map(|r| [r[0] + d, r[1] + d]));
        }
        out
    }
}

impl Toolpath {
    /// `moves[start..]`(今の終わりまで)に役目 `role` の印を付ける。動きがなければ何もしない。
    pub fn mark(&mut self, start: usize, role: Role) {
        let end = self.moves.len();
        if start < end {
            self.spans.push(Span { start, end, role });
        }
    }

    /// 動きごとの役目(印のない動きは `None`)。印が動きの数を超えていれば、その分は無視する。
    pub fn roles(&self) -> Vec<Option<Role>> {
        let mut out = vec![None; self.moves.len()];
        for s in &self.spans {
            for r in out.iter_mut().take(s.end).skip(s.start) {
                *r = Some(s.role);
            }
        }
        out
    }

    /// 動きの列を作り直した後に、印を付け直す。`starts[i]` は元の `i` 番目の動きが新しい列で
    /// 始まる位置で、長さは元の動きの数 + 1(最後は新しい列の長さ)。
    pub fn remap_spans(&mut self, starts: &[usize]) {
        let at = |i: usize| starts[i.min(starts.len() - 1)];
        if starts.is_empty() {
            self.spans.clear();
            return;
        }
        self.spans = self
            .spans
            .iter()
            .map(|s| Span {
                start: at(s.start),
                end: at(s.end),
                role: s.role,
            })
            .filter(|s| s.end > s.start)
            .collect();
    }

    pub fn to_value_data(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("経路は JSON にできる")
    }

    /// 切削の送りで動いた長さと、早送りの長さ。
    pub fn lengths(&self) -> (f64, f64) {
        let mut cur = match self.moves.first() {
            Some(m) => m.to(),
            None => return (0.0, 0.0),
        };
        let (mut cut, mut rapid) = (0.0, 0.0);
        for m in &self.moves[1..] {
            let len: f64 = {
                let mut p = cur;
                let mut l = 0.0;
                for q in m.points(cur, 0.5) {
                    l += dist(p, q);
                    p = q;
                }
                l
            };
            match m {
                Move::Rapid { .. } => rapid += len,
                _ => cut += len,
            }
            cur = m.to();
        }
        (cut, rapid)
    }

    /// 送りの時間の目安(分)。`motion` の加速度が 0 なら、長さを送りの速さで割るだけ。
    pub fn minutes(&self, motion: &Motion) -> f64 {
        let Some(first) = self.moves.first() else {
            return 0.0;
        };
        let mut pl = Planner::new(motion);
        let mut cur = first.to();
        for m in &self.moves[1..] {
            pl.add(cur, m);
            cur = m.to();
            let wait = self.dwell_after(m);
            if wait > 0.0 {
                pl.flush();
                pl.seconds += wait;
            }
        }
        pl.flush();
        pl.seconds / 60.0
    }

    /// 材料を削らない動き([`LinkInfo::air`])の時間の目安(分)。範囲ごとに止まってから
    /// 止まるまでで見積もる(範囲の境目の速さは数えないので、少し長めに出る)。
    pub fn air_minutes(&self, motion: &Motion) -> f64 {
        let mut pl = Planner::new(motion);
        for &[s, e] in &self.links.air {
            let e = e.min(self.moves.len());
            if s == 0 || s >= e {
                continue;
            }
            let mut cur = self.moves[s - 1].to();
            for m in &self.moves[s..e] {
                pl.add(cur, m);
                cur = m.to();
            }
            pl.flush();
        }
        pl.seconds / 60.0
    }

    /// 動きごとの時間(秒)。[`Toolpath::minutes`] と同じ見積もりで、合計はそれの 60 倍。
    /// 最初の動き(安全高さへの早送り)は、どこから来るか分からないので 0。
    pub fn move_seconds(&self, motion: &Motion) -> Vec<f64> {
        let mut out = vec![0.0; self.moves.len()];
        let Some(first) = self.moves.first() else {
            return out;
        };
        let mut pl = Planner::new(motion);
        pl.per_move = Some(std::mem::take(&mut out));
        let mut cur = first.to();
        for (k, m) in self.moves.iter().enumerate().skip(1) {
            pl.current = k;
            pl.add(cur, m);
            cur = m.to();
        }
        pl.flush();
        pl.per_move.unwrap_or_default()
    }
}

/// 時間の見積もりに使う、機械の動きの性質。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    /// 早送りの速さ(mm/min)
    pub rapid_feed: f64,
    /// 加速度(mm/s²)。0 なら加減速を見込まない
    pub acceleration: f64,
    /// 角を回る時に許すずれ(mm、Grbl・Smoothieware の junction deviation)。角の速さを決める
    pub junction_deviation: f64,
    /// 軸ごとの最高速(mm/min)。0 の軸は制限しない
    pub max_rate: [f64; 3],
}

impl Motion {
    /// 加減速を見込まない(長さを送りの速さで割るだけ)。
    pub fn simple(rapid_feed: f64) -> Motion {
        Motion {
            rapid_feed,
            acceleration: 0.0,
            junction_deviation: 0.0,
            max_rate: [0.0; 3],
        }
    }
}

/// 直線の区間を順にため、止まってから止まるまでの時間を、加減速と角の速さを見込んで
/// 求める(Grbl・Smoothieware の先読みと同じ考え方: 角の速さの上限を junction deviation
/// から決め、後ろからと前からの 2 回で各区間の入りの速さを決め、区間を台形の速さで動く)。
struct Planner<'a> {
    m: &'a Motion,
    /// 長さ(mm)、向き、最高速(mm/s)、どの動きの区間か
    segs: Vec<(f64, [f64; 3], f64, usize)>,
    seconds: f64,
    /// 動きごとの時間を数える時の入れ物と、今足している動きの番号
    per_move: Option<Vec<f64>>,
    current: usize,
}

impl<'a> Planner<'a> {
    fn new(m: &'a Motion) -> Planner<'a> {
        Planner {
            m,
            segs: Vec::new(),
            seconds: 0.0,
            per_move: None,
            current: 0,
        }
    }

    /// `from` から `to` へ送り `feed`(mm/min)で動く区間を足す。
    fn push(&mut self, from: P3, to: P3, feed: f64) {
        let len = dist(from, to);
        if len < 1e-9 || feed <= 0.0 {
            return;
        }
        let u = [
            (to[0] - from[0]) / len,
            (to[1] - from[1]) / len,
            (to[2] - from[2]) / len,
        ];
        if self.m.acceleration <= 0.0 {
            let t = len / (feed / 60.0);
            self.seconds += t;
            if let Some(v) = self.per_move.as_mut().and_then(|v| v.get_mut(self.current)) {
                *v += t;
            }
            return;
        }
        let mut v = feed / 60.0;
        for (axis, &rate) in self.m.max_rate.iter().enumerate() {
            if rate > 0.0 && u[axis].abs() > 1e-12 {
                v = v.min(rate / 60.0 / u[axis].abs());
            }
        }
        self.segs.push((len, u, v, self.current));
    }

    /// ためた区間の時間を足し、止まった所から次を始める。
    fn flush(&mut self) {
        let a = self.m.acceleration;
        let n = self.segs.len();
        if n == 0 {
            return;
        }
        // 区間 k の入りの速さの上限(角の速さ)
        let mut entry = vec![0.0f64; n + 1];
        for (k, w) in self.segs.windows(2).enumerate() {
            let ((_, p, vp, _), (_, u, v, _)) = (w[0], w[1]);
            let cos = -(p[0] * u[0] + p[1] * u[1] + p[2] * u[2]);
            entry[k + 1] = if cos >= 0.999_999 {
                0.0
            } else if cos <= -0.999_999 {
                vp.min(v)
            } else {
                let sin_half = (0.5 * (1.0 - cos)).sqrt();
                (a * self.m.junction_deviation * sin_half / (1.0 - sin_half))
                    .sqrt()
                    .min(vp.min(v))
            };
        }
        // 後ろから: 止まれる速さに抑える。前から: 加速して届く速さに抑える
        for k in (0..n).rev() {
            entry[k] = entry[k].min((entry[k + 1].powi(2) + 2.0 * a * self.segs[k].0).sqrt());
        }
        for k in 0..n {
            entry[k + 1] = entry[k + 1].min((entry[k].powi(2) + 2.0 * a * self.segs[k].0).sqrt());
        }
        for (k, &(len, _, v, mv)) in self.segs.iter().enumerate() {
            let (v0, v1) = (entry[k], entry[k + 1]);
            let peak = ((2.0 * a * len + v0 * v0 + v1 * v1) / 2.0).sqrt();
            let t = if peak <= v {
                (2.0 * peak - v0 - v1) / a
            } else {
                let cruise = len - (2.0 * v * v - v0 * v0 - v1 * v1) / (2.0 * a);
                (2.0 * v - v0 - v1) / a + cruise / v
            };
            self.seconds += t;
            if let Some(m) = self.per_move.as_mut().and_then(|p| p.get_mut(mv)) {
                *m += t;
            }
        }
        self.segs.clear();
    }

    /// 動き `m` を足す。円弧は短い弦に分ける。
    fn add(&mut self, cur: P3, m: &Move) {
        let feed = match *m {
            Move::Rapid { .. } => self.m.rapid_feed,
            Move::Linear { feed, .. } | Move::Arc { feed, .. } => feed,
        };
        match m {
            Move::Arc { .. } => {
                let mut p = cur;
                for q in m.points(cur, 0.5) {
                    self.push(p, q, feed);
                    p = q;
                }
            }
            _ => self.push(cur, m.to(), feed),
        }
    }
}

/// 経路の作り方の検査で見つかった問題。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    pub message: String,
    /// 何番目の動きか
    pub index: usize,
}

/// 経路だけで分かる誤りを探す。
///
/// - 早送りは安全高さより下で横に動かない(下への早送りは退避高さまで)
/// - 送りは正の有限値
/// - 円弧の始点と終点は中心から同じ距離
pub fn check(tp: &Toolpath) -> Vec<Problem> {
    let mut out = Vec::new();
    let eps = 1e-6;
    let mut cur: Option<P3> = None;
    for (i, m) in tp.moves.iter().enumerate() {
        let to = m.to();
        if to.iter().any(|v| !v.is_finite()) {
            out.push(Problem {
                message: "座標が数ではありません".into(),
                index: i,
            });
            continue;
        }
        match *m {
            Move::Rapid { to } => {
                if let Some(c) = cur {
                    let horizontal = (to[0] - c[0]).hypot(to[1] - c[1]) > eps;
                    // 通り道に合わせて低くしたつなぎは、その高さ(退避高さより上)までよい
                    let floor = match tp.links.link_at(i) {
                        Some(l) if l.kind == LinkKind::Low && l.z >= tp.retract_z - eps => l.z,
                        _ => tp.safe_z,
                    };
                    if horizontal && (to[2] < floor - eps || c[2] < floor - eps) {
                        out.push(Problem {
                            message: format!(
                                "安全高さ Z{} より下で横に早送りしています",
                                tp.safe_z
                            ),
                            index: i,
                        });
                    } else if !horizontal && to[2] < tp.retract_z - eps && to[2] < c[2] {
                        out.push(Problem {
                            message: format!("退避高さ Z{} より下へ早送りしています", tp.retract_z),
                            index: i,
                        });
                    }
                } else if to[2] < tp.safe_z - eps {
                    out.push(Problem {
                        message: "最初の早送りが安全高さより下です".into(),
                        index: i,
                    });
                }
            }
            Move::Linear { feed, .. } | Move::Arc { feed, .. } => {
                if !(feed > 0.0 && feed.is_finite()) {
                    out.push(Problem {
                        message: "送りが正の値ではありません".into(),
                        index: i,
                    });
                }
                if cur.is_none() {
                    out.push(Problem {
                        message: "最初の動きが早送りではありません".into(),
                        index: i,
                    });
                }
            }
        }
        if let (Move::Arc { center, .. }, Some(c)) = (m, cur) {
            let r0 = (c[0] - center[0]).hypot(c[1] - center[1]);
            let r1 = (to[0] - center[0]).hypot(to[1] - center[1]);
            if (r0 - r1).abs() > 1e-3 || r0 < 1e-6 {
                out.push(Problem {
                    message: format!("円弧の半径が始点と終点で違います({r0:.4} と {r1:.4})"),
                    index: i,
                });
            }
        }
        cur = Some(to);
    }
    if let Some(c) = cur
        && c[2] < tp.safe_z - eps
    {
        out.push(Problem {
            message: "最後に安全高さへ戻っていません".into(),
            index: tp.moves.len().saturating_sub(1),
        });
    }
    out
}

/// プログラムの命令。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Comment {
        text: String,
    },
    /// 工具は大きい(形・切削条件・ねじ切りミルの山)ので箱に入れ、ほかの小さい命令の並びを軽くする
    ToolChange {
        tool: Box<Tool>,
    },
    Spindle {
        rpm: f64,
    },
    SpindleStop,
    Dwell {
        seconds: f64,
    },
    /// Z だけを早送りで上げる(XY はそのまま)
    Retract {
        z: f64,
    },
    Move(Move),
    /// 止まって利用者を待つ(プログラム停止。手動 NC)。`optional` なら任意停止。
    /// [`Program::build_with`] は、この前で主軸を止めて退避の高さへ上げ、後の加工の前で
    /// 主軸を回し直して待つ。古いポストのプラグインはこの命令を知らないので渡さない
    /// (`jc-plugin-host` が誤りにする)
    Stop {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        optional: bool,
    },
    /// 操作する人への知らせ(手動 NC)。画面のない機械や知らせの命令が分からない機械では、
    /// ポストがコメントにする。古いポストのプラグインにはコメントとして渡す
    Message {
        text: String,
    },
    /// レーザーで焼き始める。`power` は出力(0〜1)。焼くのは送りの動きの間だけで、早送りでは焼かない。
    /// 古いポストのプラグインはこの命令を知らないので渡さない
    LaserOn {
        power: f64,
    },
    /// レーザーで焼くのをやめる
    LaserOff,
}

/// 加工の間に入れる手動の命令(手動 NC。機能 `cam.manual_nc@1` の出力)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Manual {
    /// 止まって待つ。`text` は止まる理由(停止の前にコメントで書く)
    Stop {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        optional: bool,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
    },
    /// G-code のコメントだけ
    Comment { text: String },
    /// その場で待つ(秒)。主軸はそのまま
    Dwell { seconds: f64 },
    /// 操作する人への知らせ
    Message { text: String },
}

impl Manual {
    /// 短い説明(段取り表や要約に使う)。
    pub fn describe(&self) -> String {
        let with = |head: &str, text: &str| {
            if text.is_empty() {
                head.to_string()
            } else {
                format!("{head}: {text}")
            }
        };
        match self {
            Manual::Stop {
                optional: false,
                text,
            } => with("停止", text),
            Manual::Stop {
                optional: true,
                text,
            } => with("任意停止", text),
            Manual::Comment { text } => with("コメント", text),
            Manual::Dwell { seconds } => format!("停留 {seconds} 秒"),
            Manual::Message { text } => with("知らせ", text),
        }
    }
}

/// XY の写し方(平行移動・回転・鏡写し)。Z は変えない。加工のパターンに使う。
/// 点 `p` を `m · p + t` に写す。`m` の行列式が負なら鏡写しで、円弧の回る向きが逆になる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Xform2 {
    pub m: [[f64; 2]; 2],
    pub t: [f64; 2],
}

impl Xform2 {
    pub fn translate(dx: f64, dy: f64) -> Xform2 {
        Xform2 {
            m: [[1.0, 0.0], [0.0, 1.0]],
            t: [dx, dy],
        }
    }

    /// 点 `c` のまわりに `angle`(ラジアン、反時計回りが正)回す。
    pub fn rotate(c: [f64; 2], angle: f64) -> Xform2 {
        let (s, k) = angle.sin_cos();
        let m = [[k, -s], [s, k]];
        Xform2 {
            m,
            t: [
                c[0] - (m[0][0] * c[0] + m[0][1] * c[1]),
                c[1] - (m[1][0] * c[0] + m[1][1] * c[1]),
            ],
        }
    }

    /// 直線 x = `at`(`x_axis` が偽)か y = `at`(真)に対する鏡写し。
    /// `x_axis` が偽なら左右が入れ替わり、真なら手前と奥が入れ替わる。
    pub fn mirror(x_axis: bool, at: f64) -> Xform2 {
        if x_axis {
            Xform2 {
                m: [[1.0, 0.0], [0.0, -1.0]],
                t: [0.0, 2.0 * at],
            }
        } else {
            Xform2 {
                m: [[-1.0, 0.0], [0.0, 1.0]],
                t: [2.0 * at, 0.0],
            }
        }
    }

    /// 鏡写しか(円弧の回る向きと、ダウンカット・アップカットが逆になる)。
    pub fn mirrors(&self) -> bool {
        self.m[0][0] * self.m[1][1] - self.m[0][1] * self.m[1][0] < 0.0
    }

    pub fn point2(&self, p: [f64; 2]) -> [f64; 2] {
        [
            self.m[0][0] * p[0] + self.m[0][1] * p[1] + self.t[0],
            self.m[1][0] * p[0] + self.m[1][1] * p[1] + self.t[1],
        ]
    }

    pub fn point(&self, p: P3) -> P3 {
        let q = self.point2([p[0], p[1]]);
        [q[0], q[1], p[2]]
    }

    /// 動きを写す。円弧は中心も写し、鏡写しなら回る向きを逆にする。
    pub fn apply(&self, m: &Move) -> Move {
        match *m {
            Move::Rapid { to } => Move::Rapid { to: self.point(to) },
            Move::Linear { to, feed } => Move::Linear {
                to: self.point(to),
                feed,
            },
            Move::Arc {
                to,
                center,
                dir,
                feed,
            } => Move::Arc {
                to: self.point(to),
                center: self.point2(center),
                dir: match (dir, self.mirrors()) {
                    (d, false) => d,
                    (ArcDir::Cw, true) => ArcDir::Ccw,
                    (ArcDir::Ccw, true) => ArcDir::Cw,
                },
                feed,
            },
        }
    }
}

/// 1 つのセットアップの、機械に送る前のプログラム。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub name: String,
    /// 作業座標系の番号(1 = G54)
    pub wcs: u32,
    /// 最後に戻る高さ(WCS)
    pub clearance_z: f64,
    pub ops: Vec<Op>,
    /// プログラム番号(O 番号)。書けない機械ではポストがコメントにする
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// 削る前にプローブで表面の高さを測って補正する格子(機械の命令で測る時だけ)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leveling: Option<LevelGrid>,
}

/// 高さの補正で測る格子(WCS)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LevelGrid {
    /// 測る範囲の左手前の角
    pub x: f64,
    pub y: f64,
    /// 範囲の幅(X)と奥行き(Y)
    pub width: f64,
    pub length: f64,
    /// 格子の点の数(X と Y)
    pub nx: u32,
    pub ny: u32,
    /// 点の間を動く時の高さ(材料の表面から)
    pub height: f64,
}

impl Program {
    /// 経路を順に並べる。工具が変わる時は主軸を止めて交換し、回り始めを待ってから動く。
    pub fn build(name: &str, wcs: u32, clearance_z: f64, paths: &[Toolpath]) -> Program {
        Program::build_with(name, wcs, clearance_z, paths, &[])
    }

    /// [`Program::build`] に、手動 NC の命令を足す。`manual` の `(i, m)` は、`paths[i]` の前に
    /// `m` を入れる(`i` が経路の数なら最後の加工の後)。同じ `i` は並べた順に入れる。
    ///
    /// 停止(任意停止も)は安全側に組む: 主軸が回っていれば退避の高さ `clearance_z` へ上げて
    /// 主軸を止めてから止まり、次の加工の前で主軸を回し直して、回り始めを待つ。
    /// 停留は、その場(前の加工の最後の安全高さ)で主軸を回したまま待つ。
    pub fn build_with(
        name: &str,
        wcs: u32,
        clearance_z: f64,
        paths: &[Toolpath],
        manual: &[(usize, Manual)],
    ) -> Program {
        let mut ops = vec![Op::Comment {
            text: name.to_string(),
        }];
        let mut tool: Option<u32> = None;
        let mut rpm: Option<f64> = None;
        let insert = |ops: &mut Vec<Op>, rpm: &mut Option<f64>, at: usize| {
            for (_, m) in manual.iter().filter(|(i, _)| *i == at) {
                match m {
                    Manual::Stop { optional, text } => {
                        ops.push(Op::Retract { z: clearance_z });
                        if rpm.is_some() {
                            ops.push(Op::SpindleStop);
                            *rpm = None;
                        }
                        ops.push(Op::Comment { text: m.describe() });
                        if !text.is_empty() {
                            ops.push(Op::Message { text: text.clone() });
                        }
                        ops.push(Op::Stop {
                            optional: *optional,
                        });
                    }
                    Manual::Comment { text } => ops.push(Op::Comment { text: text.clone() }),
                    Manual::Dwell { seconds } => ops.push(Op::Dwell { seconds: *seconds }),
                    Manual::Message { text } => ops.push(Op::Message { text: text.clone() }),
                }
            }
        };
        for (k, tp) in paths.iter().enumerate() {
            insert(&mut ops, &mut rpm, k);
            if tp.moves.is_empty() {
                continue;
            }
            ops.push(Op::Comment {
                text: tp.name.clone(),
            });
            if tool != Some(tp.tool.number) {
                // 工具交換の前に、交換できる高さへ上げてから主軸を止める
                if tool.is_some() {
                    ops.push(Op::Retract { z: clearance_z });
                }
                if rpm.is_some() {
                    ops.push(Op::SpindleStop);
                    rpm = None;
                }
                ops.push(Op::ToolChange {
                    tool: Box::new(tp.tool.clone()),
                });
                tool = Some(tp.tool.number);
            }
            if let Some(power) = tp.laser_power() {
                // レーザーは主軸を回さず、経路の間だけ焼く。役目が「つなぎ」の送り(塗りの行の外の
                // はみ出し)は出力 0 で同じ速さのまま通る。早送りでは焼かない
                let mut now: Option<f64> = None;
                let link = |i: usize| {
                    tp.spans
                        .iter()
                        .any(|s| s.role == Role::Link && (s.start..s.end).contains(&i))
                };
                for (i, m) in tp.moves.iter().enumerate() {
                    if !matches!(m, Move::Rapid { .. }) {
                        let want = if link(i) { 0.0 } else { power };
                        // 焦点の高さへ下りる出力 0 の送りにも S0 を書く(S の無い送りの行で前の
                        // 出力を使う機械があり得るため、焼かない送りは必ず 0 と書く)
                        if now != Some(want) {
                            ops.push(Op::LaserOn { power: want });
                            now = Some(want);
                        }
                    }
                    ops.push(Op::Move(*m));
                    // 貫通: 出力 0 で焼き始めの所へ着き、次に焼く時だけ、止まったまま焼く
                    let wait = tp.dwell_after(m);
                    let burns_next = tp
                        .moves
                        .get(i + 1)
                        .is_some_and(|n| !matches!(n, Move::Rapid { .. }) && !link(i + 1));
                    if wait > 0.0 && link(i) && burns_next {
                        ops.push(Op::LaserOn { power });
                        now = Some(power);
                        ops.push(Op::Dwell { seconds: wait });
                    }
                }
                ops.push(Op::LaserOff);
                continue;
            }
            if rpm != Some(tp.rpm) {
                ops.push(Op::Spindle { rpm: tp.rpm });
                ops.push(Op::Dwell { seconds: 2.0 });
                rpm = Some(tp.rpm);
            }
            for m in &tp.moves {
                ops.push(Op::Move(*m));
                let wait = tp.dwell_after(m);
                if wait > 0.0 {
                    ops.push(Op::Dwell { seconds: wait });
                }
            }
        }
        insert(&mut ops, &mut rpm, paths.len());
        if rpm.is_some() {
            ops.push(Op::Retract { z: clearance_z });
            ops.push(Op::SpindleStop);
        } else if matches!(ops.last(), Some(Op::LaserOff)) {
            ops.push(Op::Retract { z: clearance_z });
        }
        Program {
            name: name.into(),
            wcs,
            clearance_z,
            ops,
            number: None,
            leveling: None,
        }
    }

    /// 複数のセットアップのプログラム `parts` を、1 本 `name` にまとめる。2 本目からは、前の
    /// セットアップの終わり(退避して主軸を止めた所)の後に、付け替えを促すコメントと知らせと
    /// 停止を入れる。各セットアップの初めには工具交換が必ず入る(付け替えの後に工具長を測り直す)。
    /// 作業座標系が違うセットアップは 1 本にできない(座標系を切り替える命令を持たないため)。
    pub fn chain(name: &str, parts: &[Program]) -> Result<Program, String> {
        let first = parts.first().ok_or("まとめるセットアップがありません")?;
        if let Some(p) = parts.iter().find(|p| p.wcs != first.wcs) {
            return Err(format!(
                "{} の作業座標系 G{} が {} の G{} と違うので、1 本にまとめられません",
                p.name,
                53 + p.wcs,
                first.name,
                53 + first.wcs
            ));
        }
        let clearance_z = parts
            .iter()
            .map(|p| p.clearance_z)
            .fold(f64::NEG_INFINITY, f64::max);
        let mut ops = vec![Op::Comment {
            text: name.to_string(),
        }];
        for (k, p) in parts.iter().enumerate() {
            if k > 0 {
                let text = format!(
                    "{} へ付け替え、原点を合わせ直してから再開する(停止中に原点復帰しない)",
                    p.name
                );
                ops.push(Op::Retract { z: p.clearance_z });
                ops.push(Op::Comment {
                    text: format!("付け替え: {text}"),
                });
                ops.push(Op::Message { text });
                ops.push(Op::Stop { optional: false });
            }
            ops.extend(p.ops.iter().cloned());
        }
        Ok(Program {
            name: name.into(),
            wcs: first.wcs,
            clearance_z,
            ops,
            number: first.number,
            leveling: first.leveling,
        })
    }

    /// 動きの範囲。
    pub fn extents(&self) -> Option<(P3, P3)> {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let mut any = false;
        for op in &self.ops {
            match op {
                Op::Move(m) => {
                    for (i, v) in m.to().iter().enumerate() {
                        min[i] = min[i].min(*v);
                        max[i] = max[i].max(*v);
                    }
                    any = true;
                }
                Op::Retract { z } => {
                    min[2] = min[2].min(*z);
                    max[2] = max[2].max(*z);
                }
                _ => {}
            }
        }
        any.then_some((min, max))
    }

    /// プログラム全体の時間の目安(分)。工具交換の後の最初の動きは、交換した位置が
    /// 分からないので数えない。工具交換と工具長の測定にかかる時間は含まない。
    pub fn minutes(&self, motion: &Motion) -> f64 {
        let mut pl = Planner::new(motion);
        let mut cur: Option<P3> = None;
        for op in &self.ops {
            match op {
                Op::Move(m) => {
                    if let Some(c) = cur {
                        pl.add(c, m);
                    }
                    cur = Some(m.to());
                }
                Op::Retract { z } => {
                    if let Some(c) = cur {
                        let to = [c[0], c[1], *z];
                        pl.add(c, &Move::Rapid { to });
                        cur = Some(to);
                    }
                }
                Op::Dwell { seconds } => {
                    pl.flush();
                    pl.seconds += seconds;
                }
                Op::ToolChange { .. } => {
                    pl.flush();
                    cur = None;
                }
                // 停止は人を待つので、待つ時間は数えない
                Op::Spindle { .. } | Op::SpindleStop | Op::Stop { .. } => pl.flush(),
                Op::Comment { .. } | Op::Message { .. } | Op::LaserOn { .. } | Op::LaserOff => {}
            }
        }
        pl.flush();
        pl.seconds / 60.0
    }

    pub fn tools(&self) -> Vec<&Tool> {
        let mut v: Vec<&Tool> = Vec::new();
        for op in &self.ops {
            if let Op::ToolChange { tool } = op
                && !v.iter().any(|t| t.number == tool.number)
            {
                v.push(tool);
            }
        }
        v
    }
}

#[cfg(test)]
mod tests;
