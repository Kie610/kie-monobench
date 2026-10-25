//! 測る道具の計算。表示用の網(どのカーネルの形でも、STEP の形でも同じ)から点・辺・面を取り出し、
//! 1 つなら座標・長さ・径・面積、2 つなら最短の距離・ΔX/ΔY/ΔZ・角度を出す。
//! 画面(kmb-ui の測る道具)と、画面を持たない作業(kmb-session の `Headless`、MCP の `measure`)が
//! 同じ計算を使う。egui にもカーネルにも依存しない(網は [`Meshes`] で受け取る)。
//!
//! 平らな面と直線の辺は網が形そのものなので、値は形の値と同じ(f32 の丸めの分だけずれる)。
//! 曲面と円の辺の距離は網の細かさ(表示の許容差 0.05 mm)の分だけ大きめに出ることがあるので、
//! 円の辺は網の点から円を求め直して径・中心を出す(網の点は円の上にある)。
//! 円柱の面(穴の壁)も網の点から軸と径を求め、穴は中心(円の辺)か軸(円柱の面)から測る。

use kmb_core::kernel::{cross, dot};
use kmb_core::{FacePick as Pick, FeatureId, Kernel, MassProps, Mesh, ShapeHandle};
use serde::Serialize;

mod closest;
mod fit;
mod report;
mod slot;

pub use closest::*;
pub use fit::*;
pub use report::*;
pub use slot::*;

pub type P = [f64; 3];

/// 表示用の網を作る時の許容差(mm)。画面と MCP が同じ網で測るように、ここで決める。
pub const MESH_TOLERANCE: f64 = 0.05;

/// ボディの網を機能ごとに探す所(画面の場面、または [`meshes`] で作った網の並び)。
pub trait Meshes {
    fn mesh(&self, feature: FeatureId) -> Option<&Mesh>;
}

impl Meshes for [(FeatureId, Mesh)] {
    fn mesh(&self, feature: FeatureId) -> Option<&Mesh> {
        self.iter().find(|(f, _)| *f == feature).map(|(_, m)| m)
    }
}

impl Meshes for Vec<(FeatureId, Mesh)> {
    fn mesh(&self, feature: FeatureId) -> Option<&Mesh> {
        self.as_slice().mesh(feature)
    }
}

/// 表示用の網を作る([`MESH_TOLERANCE`])。網にできないボディは飛ばす。別のスレッドからも呼べる。
pub fn meshes(kernel: &dyn Kernel, bodies: &[(FeatureId, ShapeHandle)]) -> Vec<(FeatureId, Mesh)> {
    bodies
        .iter()
        .filter_map(|(f, h)| Some((*f, kernel.mesh(*h, MESH_TOLERANCE).ok()?)))
        .collect()
}

/// 選んだもの(画面の網の中の位置で指す。網を作り直したら選び直す)。
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// 辺の端(頂点)
    Vertex {
        feature: FeatureId,
        at: P,
    },
    /// 円・円弧の辺の中心(`edge` は網の稜線の何本目か)
    Center {
        feature: FeatureId,
        edge: usize,
    },
    /// 面の境目の辺(網の稜線の何本目か)
    Edge {
        feature: FeatureId,
        edge: usize,
    },
    Face(Pick),
}

/// 円・円弧(網の点から求めたもの)。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Circle {
    pub center: P,
    pub normal: P,
    pub radius: f64,
    /// 一周しているか
    pub closed: bool,
    /// 円弧の角度(ラジアン。一周なら 2π)
    pub sweep: f64,
}

/// 選んだものの形。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Kind {
    Point(P),
    /// まっすぐな辺
    Line {
        a: P,
        b: P,
    },
    Circle(Circle),
    /// その他の辺(曲線)
    Curve {
        length: f64,
    },
    /// 平らな面(外向きの法線)
    Plane {
        normal: P,
        point: P,
        area: f64,
    },
    /// 円柱の面(穴の壁・丸い柱の側面)
    Cylinder(Cylinder),
    /// 曲面
    Surface {
        area: f64,
    },
}

/// 円柱の面(網の点から求めたもの)。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Cylinder {
    /// 軸の上の点(面の高さの真ん中)
    pub center: P,
    /// 軸の向き(長さ 1)
    pub axis: P,
    pub radius: f64,
    /// 軸の向きの長さ
    pub length: f64,
    /// 一周しているか
    pub closed: bool,
    /// 回った角度(ラジアン。一周なら 2π)
    pub sweep: f64,
    /// 一周していない時、軸から弧の真ん中へ向かう向き(一周なら 0)
    pub bulge: P,
}

/// 長穴(両端が半円の形)。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Slot {
    pub width: f64,
    /// 両端の半円の中心
    pub centers: (P, P),
}

impl Slot {
    /// 中心間の長さ
    pub fn pitch(&self) -> f64 {
        dist(self.centers.0, self.centers.1)
    }

    /// 全長(中心間 + 幅)
    pub fn overall(&self) -> f64 {
        self.pitch() + self.width
    }
}

/// 2 つ選んだ時の距離の測り方。丸い物(円・円柱)が入っていれば中心か軸から測るのが既定で、
/// 最短・最長に切り替えられる。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Basis {
    #[default]
    Center,
    Min,
    Max,
}

impl Basis {
    pub fn label(self) -> &'static str {
        match self {
            Basis::Center => "中心",
            Basis::Min => "最短",
            Basis::Max => "最長",
        }
    }
}

/// 距離を測るための、点・線分・三角形の集まりと、形の種類。
#[derive(Clone, Debug, PartialEq)]
pub struct Geo {
    pub kind: Kind,
    pub points: Vec<P>,
    pub segs: Vec<[P; 2]>,
    pub tris: Vec<[P; 3]>,
}

fn f64s(p: [f32; 3]) -> P {
    p.map(|v| v as f64)
}

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: P, s: f64) -> P {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn len(a: P) -> f64 {
    dot(a, a).sqrt()
}

pub fn dist(a: P, b: P) -> f64 {
    len(sub(a, b))
}

fn normalize(a: P) -> P {
    let l = len(a);
    if l < 1e-300 { a } else { scale(a, 1.0 / l) }
}

/// 網の稜線(折れ線)。
fn edge_line(scene: &(impl Meshes + ?Sized), feature: FeatureId, edge: usize) -> Option<Vec<P>> {
    Some(
        scene
            .mesh(feature)?
            .edges
            .get(edge)?
            .iter()
            .map(|p| f64s(*p))
            .collect(),
    )
}

/// 選んだものの形を、今の網から取り出す(網が変わって見つからなければ `None`)。
pub fn resolve(scene: &(impl Meshes + ?Sized), t: &Target) -> Option<Geo> {
    match t {
        Target::Vertex { at, .. } => Some(Geo {
            kind: Kind::Point(*at),
            points: vec![*at],
            segs: vec![],
            tris: vec![],
        }),
        Target::Center { feature, edge } => {
            let c = fit_circle(&edge_line(scene, *feature, *edge)?)?;
            Some(Geo {
                kind: Kind::Point(c.center),
                points: vec![c.center],
                segs: vec![],
                tris: vec![],
            })
        }
        Target::Edge { feature, edge } => {
            let line = edge_line(scene, *feature, *edge)?;
            let segs: Vec<[P; 2]> = line.windows(2).map(|w| [w[0], w[1]]).collect();
            let straight = line.len() == 2 || {
                let (a, b) = (line[0], line[line.len() - 1]);
                let d = normalize(sub(b, a));
                dist(a, b) > 1e-9
                    && line.iter().all(|p| {
                        let v = sub(*p, a);
                        len(sub(v, scale(d, dot(v, d)))) < 1e-5 * (1.0 + dist(a, b))
                    })
            };
            let mut segs = segs;
            let kind = if straight {
                Kind::Line {
                    a: line[0],
                    b: line[line.len() - 1],
                }
            } else if let Some(c) = fit_circle(&line) {
                // 網の弦は円の内側を通るので、求めた円を細かく(一周 2048 区間)刻み直して測る
                let n = ((c.sweep.abs() / std::f64::consts::TAU) * 2048.0)
                    .ceil()
                    .max(8.0) as usize;
                let r0 = sub(line[0], c.center);
                let pts: Vec<P> = (0..=n)
                    .map(|i| {
                        let a = c.sweep * i as f64 / n as f64;
                        let (sn, cs) = a.sin_cos();
                        add(c.center, add(scale(r0, cs), scale(cross(c.normal, r0), sn)))
                    })
                    .collect();
                segs = pts.windows(2).map(|w| [w[0], w[1]]).collect();
                Kind::Circle(c)
            } else {
                Kind::Curve {
                    length: segs.iter().map(|s| dist(s[0], s[1])).sum(),
                }
            };
            Some(Geo {
                kind,
                points: vec![],
                segs,
                tris: vec![],
            })
        }
        Target::Face(p) => {
            let m = scene.mesh(p.feature)?;
            let k = m.face_names.iter().position(|n| *n == p.face)? as u32;
            let tris: Vec<[P; 3]> = m
                .triangles
                .iter()
                .zip(&m.triangle_faces)
                .filter(|(_, f)| **f == k)
                .map(|(t, _)| t.map(|i| f64s(m.positions[i as usize])))
                .collect();
            if tris.is_empty() {
                return None;
            }
            let area: f64 = tris
                .iter()
                .map(|t| 0.5 * len(cross(sub(t[1], t[0]), sub(t[2], t[0]))))
                .sum();
            // 平らな面か: どの三角形の向きも、いちばん大きい三角形の向きと同じ
            let big = tris
                .iter()
                .max_by(|a, b| {
                    let s = |t: &[P; 3]| len(cross(sub(t[1], t[0]), sub(t[2], t[0])));
                    s(a).total_cmp(&s(b))
                })
                .expect("空でない");
            let normal = normalize(cross(sub(big[1], big[0]), sub(big[2], big[0])));
            let size = tris
                .iter()
                .flatten()
                .map(|p| dist(*p, big[0]))
                .fold(0.0, f64::max);
            let flat = tris
                .iter()
                .flatten()
                .all(|p| dot(sub(*p, big[0]), normal).abs() < 1e-5 * (1.0 + size));
            let kind = if flat {
                Kind::Plane {
                    normal,
                    point: big[0],
                    area,
                }
            } else if let Some(c) = fit_cylinder(&tris) {
                // 網の弦は円柱の内側を通るので、求めた円柱を細かく刻み直して測る(円の辺と同じ)
                return Some(Geo {
                    kind: Kind::Cylinder(c),
                    points: vec![],
                    segs: vec![],
                    tris: cylinder_tris(&c),
                });
            } else {
                Kind::Surface { area }
            };
            Some(Geo {
                kind,
                points: vec![],
                segs: vec![],
                tris,
            })
        }
    }
}

#[cfg(test)]
mod tests;
