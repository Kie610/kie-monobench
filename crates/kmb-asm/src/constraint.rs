//! 組立の拘束(`asm.constraint@1`)。部品 1 の面を部品 2 の面に
//! 合わせる条件(行)を表に重ね、全部の行を同時に満たす位置へ部品 1 を動かす。
//!
//! 行の種類:
//! - 位置合わせ: 平面と平面は向かい合わせ(反転で同じ向き = 面一)にし、間をオフセットにする。軸と軸
//!   (円筒の面)は同心にする。平面と軸は、軸を平面の上(オフセットの所)に寝かせる。
//! - 角度: 2 つの面の向き(平面は法線、円筒は軸)の間を角度にする。
//! - 中央: 部品 1 の面(2 つ選べばその真ん中)を、部品 2 の平行な 2 面の真ん中に置く。
//! - 接線: 平面と円筒、円筒と円筒(外から接する)を接させる。平面と平面は向かい合わせと同じ。
//!
//! 面のほかに、角の点(頂点)とまっすぐな辺も選べる。辺は円筒の軸と同じく線として扱う(線と平面は
//! 線を平面の上に、線と線は一直線に)。点は、平面の上(オフセットの所)・線の上・点に重ねる。部品 2 の
//! 相手は原点と原点の XY・YZ・ZX 平面にもできる(採用分 10-20 a3・10-22 a3)。点には向きが無いので、
//! 角度の行には使えない。
//!
//! Kie の決定(2026-10-22): 同時に解くのは 1 つの拘束の機能の中の行だけ。機能どうしは、ジョイント・位置と
//! 同じくタイムラインの順に 1 つずつ当てる(前の拘束で動いた位置から、次の拘束が動かす)。
//!
//! 解き方: 部品 1 の動き(回転 3 + 平行移動 3)を、行の食い違いの 2 乗の和が 0 になるように
//! レーベンバーグ・マーカート法で探す。向かい合わせのように半回転が要る時のため、回さない所と
//! 半回転・4 分の 1 回転をいくつか始めの位置にし、満たした中でいちばん動きの小さい物を採る(条件が
//! 足りない時も、今の位置からの動きが小さい所に置く)。満たせなければ、いちばん合わない行を誤りに出す。
//!
//! 一緒に動く物は位置の機能と同じ: 部品 1 のボディと、同じ剛性グループの部品と、それらを土台にした
//! ジョイントの部品(作る時に決めて `bodies` に持つ)。

use crate::joint::moved_outputs;
use crate::part::{Part, heir_before};
use kmb_cad::{Exprs, body_of, expr_refs, kernel_err, output, parse, rename};
use kmb_core::kernel::{self as k, Matrix4, P3};
use kmb_core::{
    Context, Descriptor, Edit, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outcome,
    Outputs, Ref, RefKind, Registry, body_root, category,
};
use serde::{Deserialize, Serialize};

pub const CONSTRAINT: &str = "asm.constraint@1";
/// 選んでいる面(部品 1 の面、部品 2 の面)から拘束を作る操作。
pub const NEW_CONSTRAINT: &str = "asm.new_constraint";

/// 行の種類の値と名前。
pub const KINDS: &[(&str, &str)] = &[
    ("align", "位置合わせ"),
    ("angle", "角度"),
    ("center", "中央"),
    ("tangent", "接線"),
];

fn align() -> String {
    "align".into()
}

/// 拘束の行 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    /// 種類([`KINDS`] の値)
    #[serde(default = "align")]
    pub kind: String,
    /// 部品 1 の面(中央では 2 つ選ぶとその真ん中)
    #[serde(default, with = "kmb_core::one_or_many")]
    pub a: Vec<Ref>,
    /// 部品 2 の面(中央では平行な 2 面)
    #[serde(default, with = "kmb_core::one_or_many")]
    pub b: Vec<Ref>,
    /// オフセットの距離(角度の行では使わない)。空なら 0
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub offset: String,
    /// 角度の行の角度。空なら 0
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub angle: String,
    /// 反転(位置合わせの平面は同じ向き、軸は逆の向き。角度は部品 1 の向きを逆に。接線の円筒どうしは
    /// 内から接する)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// 部品 2 の相手([`TARGETS`] の値)。`part` なら `b` の面・辺・点、ほかは原点か原点の平面
    #[serde(default = "part", skip_serializing_if = "is_part")]
    pub b_to: String,
}

/// 部品 2 の相手の値と名前。
pub const TARGETS: &[(&str, &str)] = &[
    ("part", "部品 2 の形"),
    ("origin", "原点"),
    ("origin_xy", "原点の XY 平面"),
    ("origin_yz", "原点の YZ 平面"),
    ("origin_zx", "原点の ZX 平面"),
];

fn part() -> String {
    "part".into()
}

fn is_part(s: &String) -> bool {
    s == "part"
}

/// 例: `{"moving": {"kind": "component", "id": 12}, "base": {"kind": "body", "root": 3},
/// "bodies": [9], "rows": [{"kind": "align", "a": {"kind": "face", "feature": 9, "face": "f9:start"},
/// "b": {"kind": "face", "feature": 3, "face": "f3:end"}}]}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    /// 動かす部品(部品 1)
    pub moving: Part,
    /// 相手の部品(部品 2)
    pub base: Part,
    /// 一緒に動かすボディ(この機能の手前で受け継いだ機能)
    pub bodies: Vec<FeatureId>,
    pub rows: Vec<Row>,
}

impl Exprs for Constraint {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        // 空の欄は 0 として読むので式に数えない
        self.rows
            .iter_mut()
            .flat_map(|r| [&mut r.offset, &mut r.angle])
            .filter(|e| !e.trim().is_empty())
            .collect()
    }
}

fn bad(s: impl Into<String>) -> EvalError {
    EvalError::InvalidData(s.into())
}

/// 面・辺・点から読んだ形: 平面(点と外向きの法線)か、円筒の軸かまっすぐな辺(点・向き・半径。
/// 辺は半径 0)か、点。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Geom {
    Plane { p: P3, n: P3 },
    Axis { p: P3, d: P3, r: f64 },
    Point { p: P3 },
}

impl Geom {
    fn moved(self, m: &Matrix4) -> Geom {
        match self {
            Geom::Plane { p, n } => Geom::Plane {
                p: k::transform_point(m, p),
                n: crate::joint::apply_dir(m, n),
            },
            Geom::Axis { p, d, r } => Geom::Axis {
                p: k::transform_point(m, p),
                d: crate::joint::apply_dir(m, d),
                r,
            },
            Geom::Point { p } => Geom::Point {
                p: k::transform_point(m, p),
            },
        }
    }

    fn point(self) -> P3 {
        match self {
            Geom::Plane { p, .. } | Geom::Axis { p, .. } | Geom::Point { p } => p,
        }
    }

    /// 平面の法線か軸の向き(点は 0)。
    fn dir(self) -> P3 {
        match self {
            Geom::Plane { n, .. } => n,
            Geom::Axis { d, .. } => d,
            Geom::Point { .. } => [0.0; 3],
        }
    }
}

/// 部品 2 の相手の原点・原点の平面(`part` は `None`)。
fn origin_geom(to: &str, who: &str) -> Result<Option<Geom>, EvalError> {
    let p = [0.0; 3];
    Ok(Some(match to {
        "part" => return Ok(None),
        "origin" => Geom::Point { p },
        "origin_xy" => Geom::Plane {
            p,
            n: [0.0, 0.0, 1.0],
        },
        "origin_yz" => Geom::Plane {
            p,
            n: [1.0, 0.0, 0.0],
        },
        "origin_zx" => Geom::Plane {
            p,
            n: [0.0, 1.0, 0.0],
        },
        other => return Err(bad(format!("{who}の相手 {other} は知らない相手です"))),
    }))
}

/// 面 `r` の形(平面か円筒)、まっすぐな辺の線、頂点の点。
fn geom_of(ctx: &EvalCtx<'_>, r: &Ref, who: &str) -> Result<Geom, EvalError> {
    let (feature, face) = match r {
        Ref::Face { feature, face } => (feature, face),
        Ref::Edge { feature, faces } => {
            let shape = body_of(ctx, *feature)?;
            let edges = ctx.kernel().edges(shape).map_err(kernel_err)?;
            let Some(e) = edges
                .iter()
                .find(|e| e.faces == *faces || (e.faces[0] == faces[1] && e.faces[1] == faces[0]))
            else {
                return Err(bad(format!(
                    "{who}の辺 {}|{} がボディに無いか、まっすぐな辺ではありません。選び直してください",
                    faces[0], faces[1]
                )));
            };
            let d = k::sub(e.b, e.a);
            if k::dot(d, d) < 1e-18 {
                return Err(bad(format!("{who}の辺の長さが 0 です")));
            }
            return Ok(Geom::Axis {
                p: e.a,
                d: k::normalize(d),
                r: 0.0,
            });
        }
        Ref::Vertex { feature, vertex } => {
            let shape = body_of(ctx, *feature)?;
            return kmb_cad::stop_at::vertex_at(ctx.kernel(), shape, vertex)
                .map(|p| Geom::Point { p })
                .map_err(|_| {
                    bad(format!(
                        "{who}の頂点 {vertex} がボディにありません。頂点を選び直してください"
                    ))
                });
        }
        Ref::Body(_) => {
            return Err(bad(format!("{who}は面・辺・頂点を選んでください")));
        }
    };
    let shape = body_of(ctx, *feature)?;
    let faces = ctx.kernel().faces(shape).map_err(kernel_err)?;
    let Some(f) = faces.iter().find(|f| f.name == *face) else {
        // 面が無くなったら、別の面に付け替えずに止める(黙って壊さない)
        return Err(bad(format!(
            "{who}の面 {face} がボディにありません。面を選び直してください"
        )));
    };
    if let Some(pf) = f.planar {
        return Ok(Geom::Plane {
            p: pf.point,
            n: k::normalize(pf.normal),
        });
    }
    let arcs = ctx.kernel().arc_edges(shape).map_err(kernel_err)?;
    match arcs
        .iter()
        .filter(|e| e.faces.contains(face))
        .find_map(|e| e.arc)
    {
        Some(a) => Ok(Geom::Axis {
            p: a.center,
            d: k::normalize(a.axis),
            r: a.radius,
        }),
        None => Err(bad(format!(
            "{who}の面 {face} は平面でも円筒でもありません(拘束は平面と円筒の面で作ります)"
        ))),
    }
}

/// 平行な 2 平面の真ん中の平面(法線は 1 つ目の向き)。
fn middle(g1: Geom, g2: Geom, who: &str) -> Result<Geom, EvalError> {
    match (g1, g2) {
        (Geom::Plane { p, n }, Geom::Plane { p: q, n: m }) => {
            if k::dot(n, m).abs() < 1.0 - 1e-6 {
                return Err(bad(format!("{who}の 2 つの面が平行ではありません")));
            }
            Ok(Geom::Plane {
                p: k::add(p, k::scale(n, k::dot(k::sub(q, p), n) / 2.0)),
                n,
            })
        }
        _ => Err(bad(format!("{who}の 2 つの面は平面にしてください"))),
    }
}

/// 向きの食い違いを長さの食い違いと比べられるようにする重み(mm。1 rad の食い違いを 100 mm と見る)。
const W: f64 = 100.0;

/// 1 行を解く形にした物(部品 1 の形は動かす前の位置)。
#[derive(Debug, Clone, Copy)]
struct Goal {
    kind: Kind,
    a: Geom,
    b: Geom,
    offset: f64,
    flip: bool,
    /// 軸と軸の向きをそろえる向き(作った位置で近い方。反転で逆)
    sign: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Align,
    Angle,
    Center,
    Tangent,
}

fn perp(v: P3, d: P3) -> P3 {
    k::sub(v, k::scale(d, k::dot(v, d)))
}

impl Goal {
    /// 動き `m` の時の食い違い(0 なら満たす)を `out` に足す。
    fn residuals(&self, m: &Matrix4, out: &mut Vec<f64>) {
        let a = self.a.moved(m);
        let b = self.b;
        let push3 = |out: &mut Vec<f64>, v: P3, w: f64| out.extend(v.map(|x| x * w));
        match (self.kind, a, b) {
            (Kind::Angle, ..) => {
                let ua = if self.flip {
                    k::scale(a.dir(), -1.0)
                } else {
                    a.dir()
                };
                out.push(W * (k::dot(ua, b.dir()) - self.offset.cos()));
            }
            // 平面と平面: 向かい合わせ(反転で同じ向き)、間はオフセット
            (
                Kind::Align | Kind::Tangent | Kind::Center,
                Geom::Plane { p, n },
                Geom::Plane { p: q, n: m2 },
            ) => {
                let s = if self.kind == Kind::Center {
                    // 中央は向きをそろえるだけ(どちら向きでも真ん中)
                    if k::dot(n, m2) >= 0.0 { -1.0 } else { 1.0 }
                } else if self.flip {
                    -1.0
                } else {
                    1.0
                };
                if self.kind == Kind::Center {
                    push3(out, k::cross(n, m2), W);
                } else {
                    push3(out, k::add(n, k::scale(m2, s)), W);
                }
                out.push(k::dot(k::sub(p, q), m2) - self.offset);
            }
            // 軸と軸: 同心(接線は外から接する)
            (
                Kind::Align | Kind::Tangent,
                Geom::Axis { p, d, r },
                Geom::Axis { p: q, d: e, r: r2 },
            ) => {
                if self.kind == Kind::Align {
                    push3(out, k::sub(d, k::scale(e, self.sign)), W);
                    push3(out, perp(k::sub(p, q), e), 1.0);
                } else {
                    push3(out, k::cross(d, e), W);
                    let want = if self.flip { (r2 - r).abs() } else { r + r2 } + self.offset;
                    let gap = perp(k::sub(p, q), e);
                    out.push(k::dot(gap, gap).sqrt() - want);
                }
            }
            // 平面と軸: 軸を平面の上に寝かせる(接線は半径だけ離す)
            (
                Kind::Align | Kind::Tangent | Kind::Center,
                Geom::Plane { p, n },
                Geom::Axis { p: q, d: e, r },
            ) => {
                out.push(W * k::dot(n, e));
                let lift = if self.kind == Kind::Tangent { r } else { 0.0 };
                out.push(k::dot(k::sub(q, p), n) - (lift + self.offset));
            }
            (
                Kind::Align | Kind::Tangent | Kind::Center,
                Geom::Axis { p, d, r },
                Geom::Plane { p: q, n },
            ) => {
                out.push(W * k::dot(d, n));
                let lift = if self.kind == Kind::Tangent { r } else { 0.0 };
                out.push(k::dot(k::sub(p, q), n) - (lift + self.offset));
            }
            // 点と平面: 点を平面の上(オフセットの所)に
            (_, Geom::Point { p }, Geom::Plane { p: q, n })
            | (_, Geom::Plane { p: q, n }, Geom::Point { p }) => {
                out.push(k::dot(k::sub(p, q), n) - self.offset);
            }
            // 点と線: 点を線の上に(オフセットがあれば線からその距離に)
            (_, Geom::Point { p }, Geom::Axis { p: q, d: e, .. })
            | (_, Geom::Axis { p: q, d: e, .. }, Geom::Point { p }) => {
                let gap = perp(k::sub(p, q), e);
                if self.offset == 0.0 {
                    push3(out, gap, 1.0);
                } else {
                    out.push(k::dot(gap, gap).sqrt() - self.offset);
                }
            }
            // 点と点: 重ねる(オフセットがあればその距離に)
            (_, Geom::Point { p }, Geom::Point { p: q }) => {
                let gap = k::sub(p, q);
                if self.offset == 0.0 {
                    push3(out, gap, 1.0);
                } else {
                    out.push(k::dot(gap, gap).sqrt() - self.offset);
                }
            }
            (Kind::Center, Geom::Axis { .. }, Geom::Axis { .. }) => {
                // 中央の部品 2 は 2 平面の真ん中なので、ここへは来ない
                out.push(0.0);
            }
        }
    }
}

/// 回転ベクトル `w`(軸 × 角度)の回転行列(点 `pivot` のまわり)と、平行移動 `t`。
fn pose(x: &[f64; 6], pivot: P3) -> Matrix4 {
    let w = [x[0], x[1], x[2]];
    let angle = k::dot(w, w).sqrt();
    let rot = if angle < 1e-15 {
        k::IDENTITY
    } else {
        k::rotation(pivot, k::scale(w, 1.0 / angle), angle)
    };
    crate::joint::mul(&k::translation([x[3], x[4], x[5]]), &rot)
}

fn residuals(goals: &[Goal], x: &[f64; 6], pivot: P3) -> Vec<f64> {
    let m = pose(x, pivot);
    let mut out = Vec::new();
    for g in goals {
        g.residuals(&m, &mut out);
    }
    out
}

fn norm2(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum()
}

/// 6 元の連立 1 次方程式(部分ピボットのガウスの消去)。
fn solve6(mut a: [[f64; 6]; 6], mut b: [f64; 6]) -> Option<[f64; 6]> {
    for c in 0..6 {
        let p = (c..6).max_by(|i, j| a[*i][c].abs().total_cmp(&a[*j][c].abs()))?;
        if a[p][c].abs() < 1e-300 {
            return None;
        }
        a.swap(c, p);
        b.swap(c, p);
        let (top, rest) = a.split_at_mut(c + 1);
        let pivot = top[c];
        for (k, row) in rest.iter_mut().enumerate() {
            let f = row[c] / pivot[c];
            for (v, p) in row[c..].iter_mut().zip(&pivot[c..]) {
                *v -= f * p;
            }
            b[c + 1 + k] -= f * b[c];
        }
    }
    let mut x = [0.0; 6];
    for c in (0..6).rev() {
        let s: f64 = (c + 1..6).map(|j| a[c][j] * x[j]).sum();
        x[c] = (b[c] - s) / a[c][c];
    }
    Some(x)
}

/// レーベンバーグ・マーカート法で、`x` から食い違いの 2 乗の和を小さくする。
fn levenberg(goals: &[Goal], mut x: [f64; 6], pivot: P3) -> [f64; 6] {
    let mut r = residuals(goals, &x, pivot);
    let mut cost = norm2(&r);
    let mut lambda = 1e-3;
    for _ in 0..200 {
        if cost < 1e-24 {
            break;
        }
        // 数値の微分(前進差分)
        let h = 1e-7;
        let cols: Vec<Vec<f64>> = (0..6)
            .map(|j| {
                let mut y = x;
                y[j] += h;
                residuals(goals, &y, pivot)
                    .iter()
                    .zip(&r)
                    .map(|(a, b)| (a - b) / h)
                    .collect()
            })
            .collect();
        let mut jtj = [[0.0; 6]; 6];
        let mut jtr = [0.0; 6];
        for i in 0..6 {
            for j in 0..6 {
                jtj[i][j] = cols[i].iter().zip(&cols[j]).map(|(a, b)| a * b).sum();
            }
            jtr[i] = -cols[i].iter().zip(&r).map(|(a, b)| a * b).sum::<f64>();
        }
        let mut better = false;
        for _ in 0..12 {
            let mut a = jtj;
            for (i, row) in a.iter_mut().enumerate() {
                row[i] += lambda * (jtj[i][i] + 1e-9);
            }
            let Some(dx) = solve6(a, jtr) else {
                lambda *= 10.0;
                continue;
            };
            let mut y = x;
            for i in 0..6 {
                y[i] += dx[i];
            }
            let ry = residuals(goals, &y, pivot);
            let cy = norm2(&ry);
            if cy < cost {
                x = y;
                r = ry;
                let step: f64 = dx.iter().map(|v| v * v).sum::<f64>().sqrt();
                better = (cost - cy) > 1e-30 && step > 1e-16;
                cost = cy;
                lambda = (lambda / 10.0).max(1e-12);
                break;
            }
            lambda *= 10.0;
        }
        if !better {
            break;
        }
    }
    x
}

/// 行 `goals` を全部満たす部品 1 の動き。満たせなければ、いちばん合わない行の番号(0 から)。
fn solve(goals: &[Goal], pivot: P3) -> Result<Matrix4, usize> {
    let h = std::f64::consts::PI;
    let q = h / 2.0;
    let starts: [[f64; 3]; 10] = [
        [0.0; 3],
        [h, 0.0, 0.0],
        [0.0, h, 0.0],
        [0.0, 0.0, h],
        [q, 0.0, 0.0],
        [-q, 0.0, 0.0],
        [0.0, q, 0.0],
        [0.0, -q, 0.0],
        [0.0, 0.0, q],
        [0.0, 0.0, -q],
    ];
    let mut best: Option<(f64, [f64; 6])> = None;
    let mut worst_row = 0;
    let mut worst_cost = f64::MAX;
    for s in starts {
        let x = levenberg(goals, [s[0], s[1], s[2], 0.0, 0.0, 0.0], pivot);
        let r = residuals(goals, &x, pivot);
        let cost = norm2(&r);
        if cost < 1e-12 {
            // 動きの大きさ: 回した角度を 100 mm、平行移動を mm で見る
            let angle = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
            let size = W * angle + (x[3] * x[3] + x[4] * x[4] + x[5] * x[5]).sqrt();
            if best.as_ref().is_none_or(|(b, _)| size < *b - 1e-9) {
                best = Some((size, x));
            }
        } else if cost < worst_cost {
            worst_cost = cost;
            let m = pose(&x, pivot);
            worst_row = goals
                .iter()
                .enumerate()
                .map(|(i, g)| {
                    let mut v = Vec::new();
                    g.residuals(&m, &mut v);
                    (i, norm2(&v))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(0, |(i, _)| i);
        }
    }
    best.map(|(_, x)| pose(&x, pivot)).ok_or(worst_row)
}

impl Constraint {
    /// 行を解く形にする(部品 1 の面は動かす前、部品 2 の面は今の位置)。
    fn goals(&self, ctx: &EvalCtx<'_>) -> Result<Vec<Goal>, EvalError> {
        let mut goals = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            let n = i + 1;
            let kind = match row.kind.as_str() {
                "align" => Kind::Align,
                "angle" => Kind::Angle,
                "center" => Kind::Center,
                "tangent" => Kind::Tangent,
                other => return Err(bad(format!("{n} 行目の種類 {other} は知らない種類です"))),
            };
            let one = |refs: &[Ref], who: &str| -> Result<Geom, EvalError> {
                match refs {
                    [r] => geom_of(ctx, r, who),
                    [r1, r2] if kind == Kind::Center => {
                        middle(geom_of(ctx, r1, who)?, geom_of(ctx, r2, who)?, who)
                    }
                    [] => Err(bad(format!("{who}の面を選んでください"))),
                    _ if kind == Kind::Center => Err(bad(format!("{who}の面は 1 つか 2 つです"))),
                    _ => Err(bad(format!("{who}の面は 1 つだけ選んでください"))),
                }
            };
            let who1 = format!("{n} 行目の部品 1");
            let who2 = format!("{n} 行目の部品 2");
            for r in &row.a {
                if !self.bodies.contains(&r.feature()) {
                    return Err(bad(format!("{who1}の面が、動かす部品の面ではありません")));
                }
            }
            let origin = origin_geom(&row.b_to, &who2)?;
            if origin.is_some() && !row.b.is_empty() {
                return Err(bad(format!(
                    "{who2}は原点か原点の平面にしたので、部品 2 の面・辺・点は外してください"
                )));
            }
            for r in &row.b {
                if self.bodies.contains(&r.feature()) {
                    return Err(bad(format!(
                        "{who2}の面が、動かす部品(か一緒に動く部品)の面です"
                    )));
                }
            }
            let a = one(&row.a, &who1)?;
            if kind == Kind::Center && origin.is_none() && row.b.len() != 2 {
                return Err(bad(format!("{who2}は平行な 2 つの面を選んでください")));
            }
            let b = match origin {
                Some(g) => g,
                None => one(&row.b, &who2)?,
            };
            if kind == Kind::Angle
                && let (Geom::Point { .. }, _) | (_, Geom::Point { .. }) = (a, b)
            {
                return Err(bad(format!(
                    "{n} 行目は角度の行なので、点ではなく面か辺を選んでください"
                )));
            }
            let offset = match kind {
                Kind::Angle if !row.angle.trim().is_empty() => {
                    kmb_cad::angle(ctx, &row.angle, &format!("{n} 行目の角度"))?
                }
                Kind::Angle => 0.0,
                _ if !row.offset.trim().is_empty() => {
                    kmb_cad::length(ctx, &row.offset, &format!("{n} 行目のオフセット"))?
                }
                _ => 0.0,
            };
            if kind == Kind::Center
                && let Geom::Axis { .. } | Geom::Point { .. } = b
            {
                return Err(bad(format!("{who2}は平面にしてください")));
            }
            let near = k::dot(a.dir(), b.dir()) >= 0.0;
            let sign = if near != row.flip { 1.0 } else { -1.0 };
            goals.push(Goal {
                kind,
                a,
                b,
                offset,
                flip: row.flip,
                sign,
            });
        }
        Ok(goals)
    }
}

impl Feature for Constraint {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v: Vec<InputRef> = self.bodies.iter().map(|b| output(*b, "body")).collect();
        for r in self.rows.iter().flat_map(|r| r.a.iter().chain(&r.b)) {
            let i = output(r.feature(), "body");
            if !v.contains(&i) {
                v.push(i);
            }
        }
        for i in expr_refs(self)? {
            if !v.contains(&i) {
                v.push(i);
            }
        }
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        if self.bodies.is_empty() {
            return Err(bad("動かすボディがありません"));
        }
        if self.rows.is_empty() {
            return Err(bad("拘束の行がありません(面の組を足してください)"));
        }
        let goals = self.goals(ctx)?;
        // 回す中心は部品 1 の面の点の真ん中(平行移動を小さく保つ)
        let pivot = {
            let s = goals.iter().fold([0.0; 3], |s, g| k::add(s, g.a.point()));
            k::scale(s, 1.0 / goals.len() as f64)
        };
        let m = solve(&goals, pivot).map_err(|i| {
            bad(format!(
                "拘束を全部満たす位置が見つかりません({} 行目がいちばん合いません。行どうしが食い違っていないか見てください)",
                i + 1
            ))
        })?;
        log::debug!("拘束の動き: {m:?}");
        let mut out = Outputs::new();
        moved_outputs(ctx, &self.bodies, &m, &mut out)?;
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

/// 行の相手に選べる物(面・まっすぐな辺・頂点)。
const PICKS: &[RefKind] = &[RefKind::Face, RefKind::Edge, RefKind::Vertex];

pub fn descriptor() -> Descriptor {
    Descriptor::new(CONSTRAINT, category::SOLID, "拘束")
        .menu("組立")
        .icon("joint")
        .hint("部品 1 の面を部品 2 の面に合わせる条件を表に重ね、全部を同時に満たす所へ部品 1 を動かす")
        .aliases("こうそく 拘束 constraint いちあわせ 位置合わせ めんいち 面一 どうしん 同心 かくど 角度 ちゅうおう 中央 せっせん 接線 くみたて 組立")
        .field(Field::rows(
            "rows",
            "面の組",
            vec![
                Field::choice("kind", "種類", KINDS).help(
                    "位置合わせ: 平面は向かい合わせ(反転で面一)、円筒は同心。角度: 2 つの面の向きの間の角度。中央: 部品 2 の平行な 2 面の真ん中に置く。接線: 平面と円筒、円筒どうしを接させる",
                ),
                Field::pick("a", "部品 1 の面", PICKS, 1, Some(2))
                    .help("動かす部品の平面か円筒の面、まっすぐな辺、角の点(中央では 2 つ選ぶとその真ん中)"),
                Field::choice("b_to", "部品 2 の相手", TARGETS)
                    .help("部品 2 の形(面・辺・点)に合わせるか、原点・原点の平面に合わせるか"),
                Field::pick("b", "部品 2 の面", PICKS, 1, Some(2))
                    .only("@b_to", "part", "part")
                    .help("相手の部品の平面か円筒の面、まっすぐな辺、角の点(中央では平行な 2 面)"),
                offset_field(),
                Field::opt_expr("angle", "角度")
                    .angle()
                    .only("@kind", "angle", "align")
                    .help("2 つの面の向きの間の角度。空なら 0"),
                Field::bool("flip", "反転").help(
                    "位置合わせの平面は同じ向き(面一)、円筒は軸を逆向きに。角度は部品 1 の向きを逆に。接線の円筒どうしは内から接する",
                ),
            ],
        ).help("行ごとに部品 1 の面と部品 2 の面の条件を持つ。この拘束の行は全部を同時に満たす(前の拘束・ジョイントはタイムラインの順に当たった後の位置)"))
        .no_raw_editor()
}

/// オフセットの欄(角度の行の外で見せる)。
fn offset_field() -> Field {
    let mut f = Field::opt_expr("offset", "オフセット")
        .length()
        .unless("@kind", "angle")
        .help("面の間の距離(中央は真ん中からのずれ、接線は接する所からの離れ)。空なら 0");
    if let Some(w) = f.when.as_mut() {
        w.default = "align".into();
    }
    f
}

pub fn register(reg: &mut Registry) {
    reg.register_described(descriptor(), |d| Ok(Box::new(parse::<Constraint>(d)?)));
}

/// 選んでいる面(先に選んだ面が部品 1、後の面が部品 2。中央は部品 2 の面を 2 つ)か、引数 `a`・`b` から
/// 拘束を作る。
pub fn new_constraint(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let kind = cx
        .arg("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("align")
        .to_string();
    if !KINDS.iter().any(|(k, _)| *k == kind) {
        return Err(
            format!("種類 {kind} は知らない種類です(align・angle・center・tangent)").into(),
        );
    }
    let refs = |key: &str| -> Result<Option<Vec<Ref>>, String> {
        match cx.arg(key) {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("{key} の面: {e}")))
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(v) => serde_json::from_value(v.clone())
                .map(|r| Some(vec![r]))
                .map_err(|e| format!("{key} の面: {e}")),
        }
    };
    let b_to = cx
        .arg("b_to")
        .and_then(|v| v.as_str())
        .unwrap_or("part")
        .to_string();
    origin_geom(&b_to, "部品 2").map_err(|e| e.to_string())?;
    let (a, b) = match (refs("a")?, refs("b")?) {
        (Some(a), Some(b)) if b_to == "part" => (a, b),
        (Some(_), Some(_)) => {
            return Err("部品 2 を原点か原点の平面にする時は、b を渡さないでください".into());
        }
        (Some(a), None) if b_to != "part" => (a, Vec::new()),
        (None, None) => {
            let picked: Vec<Ref> = cx
                .picked_faces()
                .into_iter()
                .map(|p| Ref::from(p.clone()))
                .collect();
            match picked.split_first() {
                Some((first, _)) if b_to != "part" => (vec![first.clone()], Vec::new()),
                Some((first, rest)) if !rest.is_empty() => (vec![first.clone()], rest.to_vec()),
                _ => {
                    return Err(
                        "部品 1 の面と部品 2 の面を選んでください(1 つ目の面をクリックし、Ctrl を押しながら相手の部品の面をクリック)"
                            .into(),
                    );
                }
            }
        }
        _ => return Err("部品 1 の面(a)と部品 2 の面(b)は両方渡してください".into()),
    };
    let data = cx.data;
    let root_of = |r: &Ref| body_root(data, cx.registry, r.feature());
    let moving = Part::of_body(data, root_of(&a[0]));
    // 原点に合わせる時の部品 2 は、動かす部品そのもの(相手の部品が無い)
    let base = match b.first() {
        Some(r) => Part::of_body(data, root_of(r)),
        None => moving.clone(),
    };
    let set = crate::rigid::closure(cx, moving.roots(data));
    if b.iter().any(|r| set.contains(&root_of(r))) {
        return Err(
            "部品 2 の面が、部品 1 と一緒に動く部品の面です(同じ部品か同じ剛性グループ、またはジョイントでつながっています)"
                .into(),
        );
    }
    if a.iter().any(|r| !set.contains(&root_of(r))) {
        return Err("部品 1 の面は同じ部品から選んでください".into());
    }
    let bodies: Vec<FeatureId> = set
        .iter()
        .map(|r| heir_before(data, cx.registry, cx.report, *r, None))
        .collect();
    // 面は動かす前の、今の最後の機能で指す(この拘束の手前の形)
    let fix = |r: Ref| -> Ref {
        let root = root_of(&r);
        let last = heir_before(data, cx.registry, cx.report, root, None);
        match r {
            Ref::Face { face, .. } => Ref::Face {
                feature: last,
                face,
            },
            Ref::Edge { faces, .. } => Ref::Edge {
                feature: last,
                faces,
            },
            Ref::Vertex { vertex, .. } => Ref::Vertex {
                feature: last,
                vertex,
            },
            other => other,
        }
    };
    let offset = cx
        .arg("offset")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let (offset, angle) = if kind == "angle" {
        (String::new(), offset)
    } else {
        (offset, String::new())
    };
    let row = Row {
        kind: kind.clone(),
        a: a.into_iter().map(fix).collect(),
        b: b.into_iter().map(fix).collect(),
        offset,
        angle,
        flip: cx.arg("flip").and_then(|v| v.as_bool()).unwrap_or(false),
        b_to,
    };
    let c = Constraint {
        moving,
        base,
        bodies,
        rows: vec![row],
    };
    let name = format!("拘束{}", cx.count_of(CONSTRAINT) + 1);
    log::info!("{name} を作る({kind}、動かすボディ {} 個)", c.bodies.len());
    Ok(Outcome::Change {
        label: format!("{name} を作成"),
        edits: vec![Edit::Insert {
            type_id: CONSTRAINT.into(),
            name,
            data: serde_json::to_value(&c).map_err(|e| e.to_string())?,
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(p: P3, n: P3) -> Geom {
        Geom::Plane { p, n }
    }

    fn goal(kind: Kind, a: Geom, b: Geom, offset: f64, flip: bool) -> Goal {
        let near = k::dot(a.dir(), b.dir()) >= 0.0;
        Goal {
            kind,
            a,
            b,
            offset,
            flip,
            sign: if near != flip { 1.0 } else { -1.0 },
        }
    }

    fn close(a: P3, b: P3) -> bool {
        k::dot(k::sub(a, b), k::sub(a, b)).sqrt() < 1e-5
    }

    /// 上を向いた面を、上を向いた相手の面に向かい合わせると半回転して重なる。オフセットで離れる。
    #[test]
    fn two_faces_meet_face_to_face_and_flush_with_flip() {
        let a = plane([5.0, 0.0, 10.0], [0.0, 0.0, 1.0]);
        let b = plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let m = solve(&[goal(Kind::Align, a, b, 2.0, false)], a.point()).unwrap();
        let moved = a.moved(&m);
        assert!(close(moved.dir(), [0.0, 0.0, -1.0]), "{moved:?}");
        assert!((moved.point()[2] - 2.0).abs() < 1e-6, "{moved:?}");
        // 反転(面一): 回さずに下ろすだけ
        let m = solve(&[goal(Kind::Align, a, b, 0.0, true)], a.point()).unwrap();
        let moved = a.moved(&m);
        assert!(close(moved.dir(), [0.0, 0.0, 1.0]));
        assert!(close(moved.point(), [5.0, 0.0, 0.0]), "{moved:?}");
    }

    /// 円筒の軸どうしは同心になり、2 つの平面の行と一緒に解ける(ピンを穴に挿して底に当てる)。
    #[test]
    fn a_pin_goes_into_a_hole_and_down_to_its_floor() {
        let pin = Geom::Axis {
            p: [30.0, 20.0, 5.0],
            d: [0.0, 0.0, 1.0],
            r: 3.0,
        };
        let pin_end = plane([30.0, 20.0, 0.0], [0.0, 0.0, -1.0]);
        let hole = Geom::Axis {
            p: [0.0, 0.0, -10.0],
            d: [0.0, 0.0, -1.0],
            r: 3.0,
        };
        let floor = plane([0.0, 0.0, -10.0], [0.0, 0.0, 1.0]);
        let goals = [
            goal(Kind::Align, pin, hole, 0.0, false),
            goal(Kind::Align, pin_end, floor, 0.0, false),
        ];
        let m = solve(&goals, pin.point()).unwrap();
        let p = pin.moved(&m);
        assert!(close(p.dir(), [0.0, 0.0, 1.0]), "回さない: {p:?}");
        assert!(
            (p.point()[0]).abs() < 1e-6 && (p.point()[1]).abs() < 1e-6,
            "{p:?}"
        );
        assert!((pin_end.moved(&m).point()[2] + 10.0).abs() < 1e-6);
    }

    /// 角度・中央・接線。
    #[test]
    fn angle_center_and_tangent_rows() {
        // 角度: 上向きの面を、相手の上向きの面から 30 度に傾ける
        let a = plane([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]);
        let b = plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let m = solve(
            &[goal(Kind::Angle, a, b, 30f64.to_radians(), false)],
            a.point(),
        )
        .unwrap();
        let c = k::dot(a.moved(&m).dir(), b.dir());
        assert!((c - 30f64.to_radians().cos()).abs() < 1e-6, "{c}");
        // 中央: 溝の 2 面(x = 0 と x = 10)の真ん中に、板の面を置く
        let slot = middle(
            plane([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            plane([10.0, 0.0, 0.0], [-1.0, 0.0, 0.0]),
            "部品 2",
        )
        .unwrap();
        let tab = plane([42.0, 3.0, 1.0], [1.0, 0.0, 0.0]);
        let m = solve(&[goal(Kind::Center, tab, slot, 0.0, false)], tab.point()).unwrap();
        assert!((tab.moved(&m).point()[0] - 5.0).abs() < 1e-6);
        // 接線: 円筒を平面の上に転がして接させる(軸と平面の間が半径)
        let roll = Geom::Axis {
            p: [0.0, 0.0, 20.0],
            d: [1.0, 0.0, 0.0],
            r: 4.0,
        };
        let m = solve(&[goal(Kind::Tangent, roll, b, 0.0, false)], roll.point()).unwrap();
        assert!((roll.moved(&m).point()[2] - 4.0).abs() < 1e-6);
    }

    /// 満たせない行の組は、いちばん合わない行を返す。
    #[test]
    fn rows_that_disagree_are_reported() {
        let a = plane([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]);
        let b = plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let c = plane([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        // 同じ面を、上向きの面と横向きの面の両方に面一にはできない
        let goals = [
            goal(Kind::Align, a, b, 0.0, true),
            goal(Kind::Align, a, c, 0.0, true),
        ];
        assert!(solve(&goals, a.point()).is_err());
    }
}
