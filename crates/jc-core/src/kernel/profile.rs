//! 平面と、その上の輪郭・領域(スケッチから押し出しなどへ渡す 2 次元の形)。

use super::*;

/// 平面上の座標系。法線は `x_axis × y_axis`。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub origin: P3,
    pub x_axis: P3,
    pub y_axis: P3,
}

impl Plane {
    /// 原点の平面の目安の四角(一辺 1)を原点から浮かせる隙間(一辺に対する割合)。原点の
    /// 平面は原点と軸に付けず、少し離して描く。画面に描く四角とクリックで選ぶ範囲の両方で使う。
    pub const ORIGIN_GAP: f64 = 0.1;

    pub const XY: Plane = Plane {
        origin: [0.0, 0.0, 0.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    };
    pub const YZ: Plane = Plane {
        origin: [0.0, 0.0, 0.0],
        x_axis: [0.0, 1.0, 0.0],
        y_axis: [0.0, 0.0, 1.0],
    };
    pub const ZX: Plane = Plane {
        origin: [0.0, 0.0, 0.0],
        x_axis: [0.0, 0.0, 1.0],
        y_axis: [1.0, 0.0, 0.0],
    };

    pub fn normal(&self) -> P3 {
        cross(self.x_axis, self.y_axis)
    }

    /// 平面上の 2D 座標を空間の点にする。
    pub fn to_world(&self, p: P2) -> P3 {
        add(
            self.origin,
            add(scale(self.x_axis, p[0]), scale(self.y_axis, p[1])),
        )
    }

    /// 空間の点を平面の 2D 座標にする(法線方向の成分は捨てる)。
    pub fn to_local(&self, p: P3) -> P2 {
        let d = sub(p, self.origin);
        [dot(d, self.x_axis), dot(d, self.y_axis)]
    }

    /// 法線方向へ平行移動した平面。
    pub fn offset(&self, d: f64) -> Plane {
        Plane {
            origin: add(self.origin, scale(self.normal(), d)),
            ..*self
        }
    }

    /// 点と法線から平面を作る。原点は世界の原点を平面へ投影した点、
    /// x 軸は法線から決まる向きにする。寸法を変えて面が動いても、
    /// 面の上のスケッチの座標系が面の形に引きずられないようにするため。
    pub fn from_point_normal(point: P3, normal: P3) -> Plane {
        let n = normalize(normal);
        let x = if n[2].abs() > 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            normalize(cross([0.0, 0.0, 1.0], n))
        };
        let y = cross(n, x);
        let origin = scale(n, dot(point, n));
        Plane {
            origin,
            x_axis: x,
            y_axis: y,
        }
    }
}

/// 輪郭の 1 区間。角度はラジアンで、円弧は `start` から `end` へ反時計回り。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
    Line {
        a: P2,
        b: P2,
    },
    Arc {
        center: P2,
        radius: f64,
        start: f64,
        end: f64,
    },
    Circle {
        center: P2,
        radius: f64,
    },
}

impl Segment {
    pub fn start_point(&self) -> P2 {
        match self {
            Segment::Line { a, .. } => *a,
            Segment::Arc {
                center,
                radius,
                start,
                ..
            } => polar(*center, *radius, *start),
            Segment::Circle { center, radius } => [center[0] + radius, center[1]],
        }
    }

    pub fn end_point(&self) -> P2 {
        match self {
            Segment::Line { b, .. } => *b,
            Segment::Arc {
                center,
                radius,
                end,
                ..
            } => polar(*center, *radius, *end),
            Segment::Circle { center, radius } => [center[0] + radius, center[1]],
        }
    }

    /// 折れ線にした時の弦と円の隔たりが `tol` 以下になる、1 周あたりの分割数(直線は 8)。
    pub fn steps_per_turn(&self, tol: f64) -> usize {
        let r = match self {
            Segment::Line { .. } => return 8,
            Segment::Arc { radius, .. } | Segment::Circle { radius, .. } => *radius,
        };
        if r <= tol {
            return 8;
        }
        let half = (1.0 - tol / r).acos();
        ((std::f64::consts::PI / half).ceil() as usize).clamp(8, 4096)
    }

    /// 2D の点 `p` との距離。円弧は張る角の外なら近い方の端までの距離。
    /// 区間に近い辺を名前で選ぶ所(カーネルの面の名前付け)が使うので、どのカーネルも同じ式にする。
    pub fn distance(&self, p: P2) -> f64 {
        match *self {
            Segment::Line { a, b } => {
                let d = [b[0] - a[0], b[1] - a[1]];
                let l2 = d[0] * d[0] + d[1] * d[1];
                let t = if l2 > 0.0 {
                    (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
            }
            Segment::Circle { center, radius } => {
                ((p[0] - center[0]).hypot(p[1] - center[1]) - radius).abs()
            }
            Segment::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let ang = (p[1] - center[1]).atan2(p[0] - center[0]);
                if arc_sweep(start, ang) <= arc_sweep(start, end) {
                    ((p[0] - center[0]).hypot(p[1] - center[1]) - radius).abs()
                } else {
                    let e0 = polar(center, radius, start);
                    let e1 = polar(center, radius, end);
                    (p[0] - e0[0])
                        .hypot(p[1] - e0[1])
                        .min((p[0] - e1[0]).hypot(p[1] - e1[1]))
                }
            }
        }
    }

    /// 区間を細かい折れ線にした点列(始点を含み、終点を含まない)。
    pub fn polyline(&self, steps_per_turn: usize) -> Vec<P2> {
        let steps = steps_per_turn.max(8);
        match self {
            Segment::Line { a, .. } => vec![*a],
            Segment::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sweep = arc_sweep(*start, *end);
                let n = ((steps as f64) * sweep / std::f64::consts::TAU).ceil() as usize;
                let n = n.max(2);
                (0..n)
                    .map(|i| polar(*center, *radius, start + sweep * i as f64 / n as f64))
                    .collect()
            }
            Segment::Circle { center, radius } => (0..steps)
                .map(|i| {
                    polar(
                        *center,
                        *radius,
                        std::f64::consts::TAU * i as f64 / steps as f64,
                    )
                })
                .collect(),
        }
    }
}

/// 反時計回りの円弧の中心角(0 より大きく 2π 以下)。
pub fn arc_sweep(start: f64, end: f64) -> f64 {
    let s = (end - start).rem_euclid(std::f64::consts::TAU);
    if s < 1e-12 { std::f64::consts::TAU } else { s }
}

fn polar(c: P2, r: f64, a: f64) -> P2 {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}

/// 名前つきの区間。名前はスケッチの要素から決まる(例: `e5`)。
/// `reversed` が真なら、区間を終点から始点へたどる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileEdge {
    pub name: String,
    pub segment: Segment,
    #[serde(default)]
    pub reversed: bool,
}

impl ProfileEdge {
    pub fn from_point(&self) -> P2 {
        if self.reversed {
            self.segment.end_point()
        } else {
            self.segment.start_point()
        }
    }

    pub fn to_point(&self) -> P2 {
        if self.reversed {
            self.segment.start_point()
        } else {
            self.segment.end_point()
        }
    }

    /// たどり始めの向き(角度)。
    pub fn leave_angle(&self) -> f64 {
        self.angle_at(!self.reversed)
    }

    /// たどり終わりの向き(角度)。
    pub fn arrive_angle(&self) -> f64 {
        self.angle_at(self.reversed)
    }

    /// 区間の始点(`at_start`)か終点での、たどる向きの角度。
    fn angle_at(&self, at_start: bool) -> f64 {
        let half = std::f64::consts::FRAC_PI_2;
        let a = match self.segment {
            Segment::Line { a, b } => (b[1] - a[1]).atan2(b[0] - a[0]),
            Segment::Arc { start, end, .. } => (if at_start { start } else { end }) + half,
            Segment::Circle { .. } => half,
        };
        if self.reversed {
            a + std::f64::consts::PI
        } else {
            a
        }
    }

    /// たどる向きに並べた折れ線(始点を含み、終点を含まない)。
    pub fn polyline(&self, steps_per_turn: usize) -> Vec<P2> {
        let mut pts = self.segment.polyline(steps_per_turn);
        if self.reversed {
            pts.push(self.segment.end_point());
            pts.reverse();
            pts.pop();
        }
        pts
    }
}

/// 閉じた輪郭。区間はつながった順に並ぶ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileLoop {
    pub edges: Vec<ProfileEdge>,
}

impl ProfileLoop {
    /// 折れ線で近似した点列。
    pub fn polyline(&self) -> Vec<P2> {
        self.edges.iter().flat_map(|e| e.polyline(64)).collect()
    }

    /// 符号つきの面積(反時計回りが正)。
    pub fn signed_area(&self) -> f64 {
        polygon_area(&self.polyline())
    }

    /// たどる向きを逆にする。
    pub fn reverse(&mut self) {
        self.edges.reverse();
        for e in &mut self.edges {
            e.reversed = !e.reversed;
        }
    }
}

pub fn polygon_area(pts: &[P2]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    for i in 0..n {
        let p = pts[i];
        let q = pts[(i + 1) % n];
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

/// 外周 1 つと穴。外周は反時計回り、穴は時計回りに揃える。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub outer: ProfileLoop,
    #[serde(default)]
    pub holes: Vec<ProfileLoop>,
}

/// 平面上の領域の集まり。押し出しや回転の入力になる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub plane: Plane,
    pub regions: Vec<Region>,
}

/// 平面の上で、端と端がつながった線の並び(スイープの経路、ロフトのガイドレール)。区間はたどる順に
/// 並び、向きは [`ProfileEdge::reversed`] で持つ。最後の区間の終わりが最初の区間の始まりに戻れば閉じた経路。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanarPath {
    pub plane: Plane,
    pub edges: Vec<ProfileEdge>,
}

/// 閉じた輪郭(領域)の出力の型の名前。スケッチが出し、押し出しや V 彫りが読む。
pub const PROFILE_TYPE: &str = "geom.profile@1";

impl Profile {
    pub fn from_value(v: &crate::Value) -> Option<Profile> {
        <Profile as crate::Payload>::from_value(v)
    }

    pub fn to_value(&self) -> crate::Value {
        crate::Payload::to_value(self)
    }
}

/// 線の交点で分けた一番小さな領域の出力の型の名前。スケッチが出し、押し出しで領域を選ぶ時に読む。
pub const PROFILE_FACES_TYPE: &str = "geom.profile_faces@1";

/// 線で囲まれた一番小さな領域の 1 つ(重ねた円と長方形なら、円だけの所・重なり・長方形だけの所)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileFace {
    /// 領域の名前。囲む区間の名前を並べたもの(例: `e3.1+e5+e6.0`)。寸法を変えても
    /// 線のつながりが同じなら変わらないので、押し出しが選んだ領域を覚えるのに使う
    pub key: String,
    pub region: Region,
}

/// スケッチの線で分けた領域の全部。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileFaces {
    pub plane: Plane,
    pub faces: Vec<ProfileFace>,
}

impl ProfileFaces {
    /// 名前 `keys` の領域を合わせた輪郭(隣り合う領域の境の線は消す)。
    pub fn profile(&self, keys: &[String]) -> Result<Profile, String> {
        let mut picked = Vec::new();
        for k in keys {
            let f = self.faces.iter().find(|f| f.key == *k).ok_or_else(|| {
                format!("選んだ領域 {k} がスケッチにありません(線を変えた時は選び直してください)")
            })?;
            picked.push(&f.region);
        }
        let regions = merge_regions(&picked);
        // 輪郭の角が 2 度出るのは、領域どうしが点で接する所(1 つの立体にならない)
        let corners: Vec<P2> = regions
            .iter()
            .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
            .flat_map(|l| &l.edges)
            .map(|e| e.from_point())
            .collect();
        for (i, a) in corners.iter().enumerate() {
            if corners[i + 1..]
                .iter()
                .any(|b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6)
            {
                return Err(format!(
                    "選んだ領域が点 ({:.2}, {:.2}) で接していて、1 つの形になりません。間の領域も選ぶか、分けて押し出してください",
                    a[0], a[1]
                ));
            }
        }
        Ok(Profile {
            plane: self.plane,
            regions,
        })
    }

    pub fn from_value(v: &crate::Value) -> Option<ProfileFaces> {
        <ProfileFaces as crate::Payload>::from_value(v)
    }

    pub fn to_value(&self) -> crate::Value {
        crate::Payload::to_value(self)
    }
}

/// 名前の並びの比べ方: 数字の所は数として比べる(`e9` < `e10`、`e3` < `e3.1`)。
pub fn natural_key(name: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut num: Option<u64> = None;
    for c in name.chars() {
        match (c.to_digit(10), num) {
            (Some(d), n) => num = Some(n.unwrap_or(0).saturating_mul(10).saturating_add(d as u64)),
            (None, Some(n)) => {
                out.push((n, std::mem::take(&mut text)));
                num = None;
                text.push(c);
            }
            (None, None) => text.push(c),
        }
    }
    out.push((num.unwrap_or(0), text));
    out
}

/// 点 `p` が閉じた折れ線 `poly` の内側か(偶奇の規則)。加工の領域で何百万回も呼ぶので、呼ぶ側の
/// crate で最適化されるよう `inline` にする。
#[inline]
pub fn point_in_polygon(p: P2, poly: &[P2]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                c = !c;
            }
        }
    }
    c
}

/// 輪郭の向きと始まりを揃える: 外周は反時計回り、穴は時計回りにし、名前のいちばん小さい
/// 区間を、順向きなら先頭、逆向きなら末尾に置く(描いた順に輪郭をたどった時と同じ並び)。
pub fn canonical_loop(mut l: ProfileLoop, ccw: bool) -> ProfileLoop {
    if (l.signed_area() > 0.0) != ccw {
        l.reverse();
    }
    let Some(m) = (0..l.edges.len())
        .min_by(|&a, &b| natural_key(&l.edges[a].name).cmp(&natural_key(&l.edges[b].name)))
    else {
        return l;
    };
    let n = l.edges.len();
    let first = if l.edges[m].reversed { (m + 1) % n } else { m };
    l.edges.rotate_left(first);
    l
}

/// 区間の名前の、分けた番号を除いた所(`e2.1` → `e2`)。分けていなければ `None`。
fn piece_base(name: &str) -> Option<&str> {
    let (base, k) = name.rsplit_once('.')?;
    k.parse::<u32>().ok().map(|_| base)
}

/// 輪郭の中で続いている、同じ線を交点で分けた区間(`e2.0` と `e2.1` など)を 1 つに戻す。
/// 合わせて円が 1 周すれば円にし、名前を元の線の名前にする(押し出しの側面が継ぎ目で割れない)。
fn fuse_pieces(mut l: ProfileLoop) -> ProfileLoop {
    let same_circle = |a: (P2, f64), b: (P2, f64)| {
        (a.0[0] - b.0[0]).hypot(a.0[1] - b.0[1]) < 1e-9 && (a.1 - b.1).abs() < 1e-9
    };
    loop {
        let n = l.edges.len();
        if n < 2 {
            break;
        }
        let mut fused = None;
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b) = (&l.edges[i], &l.edges[j]);
            let (Some(ba), Some(bb)) = (piece_base(&a.name), piece_base(&b.name)) else {
                continue;
            };
            if ba != bb || a.reversed != b.reversed {
                continue;
            }
            // たどる向きで a の後に b。逆向きなら区間の向きでは b の後に a
            let (first, second) = if a.reversed { (b, a) } else { (a, b) };
            let seg = match (&first.segment, &second.segment) {
                (
                    Segment::Arc {
                        center,
                        radius,
                        start,
                        end,
                    },
                    Segment::Arc {
                        center: c2,
                        radius: r2,
                        start: s2,
                        end: e2,
                    },
                ) if same_circle((*center, *radius), (*c2, *r2)) => {
                    let sweep = arc_sweep(*start, *end) + arc_sweep(*s2, *e2);
                    Segment::Arc {
                        center: *center,
                        radius: *radius,
                        start: *start,
                        end: start + sweep,
                    }
                }
                (Segment::Line { a: p, .. }, Segment::Line { a: q, b: r }) => {
                    let (d1, d2) = ([q[0] - p[0], q[1] - p[1]], [r[0] - q[0], r[1] - q[1]]);
                    let len = d1[0].hypot(d1[1]) * d2[0].hypot(d2[1]);
                    if (d1[0] * d2[1] - d1[1] * d2[0]).abs() > 1e-9 * len.max(1e-12) {
                        continue;
                    }
                    Segment::Line { a: *p, b: *r }
                }
                _ => continue,
            };
            let name = if natural_key(&a.name) <= natural_key(&b.name) {
                a.name.clone()
            } else {
                b.name.clone()
            };
            fused = Some((
                i,
                j,
                ProfileEdge {
                    name,
                    segment: seg,
                    reversed: a.reversed,
                },
            ));
            break;
        }
        let Some((i, j, e)) = fused else { break };
        l.edges[i] = e;
        l.edges.remove(j);
    }
    // 1 周した円弧は円に戻す
    if let [e] = &mut l.edges[..]
        && let Segment::Arc {
            center,
            radius,
            start,
            end,
        } = e.segment
        && (end - start - std::f64::consts::TAU).abs() < 1e-7
        && let Some(base) = piece_base(&e.name).map(String::from)
    {
        e.segment = Segment::Circle { center, radius };
        e.name = base;
    }
    l
}

/// 領域を合わせる。2 つの領域が共有する区間(同じ名前で向きが逆)は境なので消し、残りの区間を
/// つないで輪郭にする。反時計回りの輪郭を外周、時計回りを穴とし、穴はいちばん小さな外周に入れる。
pub fn merge_regions(regions: &[&Region]) -> Vec<Region> {
    let all: Vec<&ProfileEdge> = regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .flat_map(|l| &l.edges)
        .collect();
    let mut count: std::collections::HashMap<&str, usize> = Default::default();
    for e in &all {
        *count.entry(e.name.as_str()).or_default() += 1;
    }
    let edges: Vec<&ProfileEdge> = all
        .into_iter()
        .filter(|e| count[e.name.as_str()] == 1)
        .collect();
    // 端点でつなぐ(始点が前の終点に近い区間)
    let close = |a: P2, b: P2| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6;
    let mut used = vec![false; edges.len()];
    let mut loops: Vec<ProfileLoop> = Vec::new();
    for s in 0..edges.len() {
        if used[s] {
            continue;
        }
        used[s] = true;
        let mut chain = vec![edges[s].clone()];
        let start = edges[s].from_point();
        let mut closed = close(edges[s].to_point(), start);
        while !closed {
            let at = chain.last().expect("空でない").to_point();
            // 2 つの領域が点で接する所では、来た向きからいちばん左へ曲がる区間を取る
            // (領域を左に見てたどるので、同じ領域の輪郭が続き、8 の字にならない)
            let back = chain.last().expect("空でない").arrive_angle() + std::f64::consts::PI;
            let Some(k) = (0..edges.len())
                .filter(|&k| !used[k] && close(edges[k].from_point(), at))
                .min_by(|&a, &b| {
                    let turn = |k: usize| {
                        let t = (back - edges[k].leave_angle()).rem_euclid(std::f64::consts::TAU);
                        if t < 1e-9 { std::f64::consts::TAU } else { t }
                    };
                    turn(a).total_cmp(&turn(b))
                })
            else {
                break;
            };
            used[k] = true;
            chain.push(edges[k].clone());
            closed = close(edges[k].to_point(), start);
        }
        if closed {
            loops.push(fuse_pieces(ProfileLoop { edges: chain }));
        }
    }
    let polys: Vec<Vec<P2>> = loops.iter().map(|l| l.polyline()).collect();
    let areas: Vec<f64> = polys.iter().map(|p| polygon_area(p)).collect();
    let outers: Vec<usize> = (0..loops.len()).filter(|&i| areas[i] > 0.0).collect();
    let mut out: Vec<(usize, Region)> = outers
        .iter()
        .map(|&i| {
            (
                i,
                Region {
                    outer: canonical_loop(loops[i].clone(), true),
                    holes: Vec::new(),
                },
            )
        })
        .collect();
    for i in (0..loops.len()).filter(|&i| areas[i] < 0.0) {
        let probe = polys[i][0];
        let parent = outers
            .iter()
            .filter(|&&o| point_in_polygon(probe, &polys[o]))
            .min_by(|&&a, &&b| areas[a].total_cmp(&areas[b]));
        if let Some(&p) = parent
            && let Some((_, r)) = out.iter_mut().find(|(k, _)| *k == p)
        {
            r.holes.push(canonical_loop(loops[i].clone(), false));
        }
    }
    let first = |r: &Region| {
        r.outer
            .edges
            .iter()
            .map(|e| natural_key(&e.name))
            .min()
            .unwrap_or_default()
    };
    let mut out: Vec<Region> = out.into_iter().map(|(_, r)| r).collect();
    for r in &mut out {
        r.holes.sort_by_key(|h| {
            h.edges
                .iter()
                .map(|e| natural_key(&e.name))
                .min()
                .unwrap_or_default()
        });
    }
    out.sort_by_key(first);
    out
}

/// 線の集まりの出力の型の名前。
pub const CURVES_TYPE: &str = "geom.curves@1";

/// 平面上の線の集まり(開いた線も閉じた線も。順とつながりは問わない)。
/// 彫刻のように線をなぞる加工の入力になる。スケッチが出し、加工が読むので、コアの型にする。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curves {
    pub plane: Plane,
    pub segments: Vec<Segment>,
}

impl Curves {
    pub fn to_value(&self) -> crate::Value {
        crate::Payload::to_value(self)
    }

    pub fn from_value(v: &crate::Value) -> Option<Curves> {
        <Curves as crate::Payload>::from_value(v)
    }
}

#[cfg(test)]
mod tests {
    use super::Segment;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn distance_to_segments() {
        let line = Segment::Line {
            a: [0.0, 0.0],
            b: [10.0, 0.0],
        };
        assert!((line.distance([5.0, 3.0]) - 3.0).abs() < 1e-12);
        // 線分の外は端までの距離
        assert!((line.distance([13.0, 4.0]) - 5.0).abs() < 1e-12);
        let circle = Segment::Circle {
            center: [0.0, 0.0],
            radius: 2.0,
        };
        assert!((circle.distance([0.0, 5.0]) - 3.0).abs() < 1e-12);
        // 0°〜90° の円弧: 張る角の中は円までの距離、外は近い端までの距離
        let arc = Segment::Arc {
            center: [0.0, 0.0],
            radius: 1.0,
            start: 0.0,
            end: FRAC_PI_2,
        };
        assert!((arc.distance([2.0, 2.0]) - (8f64.sqrt() - 1.0)).abs() < 1e-12);
        assert!((arc.distance([0.0, -1.0]) - 2f64.sqrt()).abs() < 1e-12);
    }
}
