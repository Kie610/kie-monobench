//! 面の上のスケッチの原点([`FaceOrigin`])。面の角(角が無ければ円の辺の中心)を探し、
//! スケッチの平面を決める。X・Y の向きは面の法線から決まり([`Plane::from_point_normal`])、
//! 原点だけを動かす。
//!
//! 新しく作る面のスケッチは、面の左下の角を原点にする(まとめてレビューの R2、2026-10-18 Kie)。
//! 左下は、面を外から見たスケッチの向き(右が X、上が Y)で決める。

use crate::model::{FaceOrigin, Sketch, SketchPlane};
use kmb_core::{Kernel, Plane, ShapeHandle};
use std::collections::{BTreeMap, BTreeSet};

type P3 = [f64; 3];

/// 原点にできる面の 1 か所(角か円の中心。世界座標)と、その指し方。
#[derive(Debug, Clone, PartialEq)]
pub struct FaceSpot {
    pub at: P3,
    pub origin: FaceOrigin,
}

/// 面の 4 隅(スケッチの向きで見て)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    LowerLeft,
    LowerRight,
    UpperRight,
    UpperLeft,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::LowerLeft,
        Corner::LowerRight,
        Corner::UpperRight,
        Corner::UpperLeft,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Corner::LowerLeft => "左下の角",
            Corner::LowerRight => "右下の角",
            Corner::UpperRight => "右上の角",
            Corner::UpperLeft => "左上の角",
        }
    }

    /// X・Y の向き(左・下なら -1)
    fn signs(self) -> (f64, f64) {
        match self {
            Corner::LowerLeft => (-1.0, -1.0),
            Corner::LowerRight => (1.0, -1.0),
            Corner::UpperRight => (1.0, 1.0),
            Corner::UpperLeft => (-1.0, 1.0),
        }
    }
}

/// 位置を数えるための丸め(0.1 µm。投影の角と同じ)
fn key(p: P3) -> (i64, i64, i64) {
    let r = |v: f64| (v * 1e4).round() as i64;
    (r(p[0]), r(p[1]), r(p[2]))
}

/// 面 `face` の角(そこで交わる面の名前で指す。投影の角と同じ数え方)。角が無ければ、面の縁の
/// 円の中心(大きい円から)。
pub fn face_spots(kernel: &dyn Kernel, shape: ShapeHandle, face: &str) -> Vec<FaceSpot> {
    let mut edges = kernel.edges(shape).unwrap_or_default();
    edges.extend(kernel.arc_edges(shape).unwrap_or_default());
    let full = |e: &kmb_core::EdgeInfo| {
        e.arc
            .is_some_and(|a| (a.sweep.abs() - std::f64::consts::TAU).abs() < 1e-6)
    };
    let mut corners: BTreeMap<(i64, i64, i64), (P3, BTreeSet<String>)> = BTreeMap::new();
    for e in edges.iter().filter(|e| !full(e)) {
        for p in [e.a, e.b] {
            let c = corners.entry(key(p)).or_insert((p, BTreeSet::new()));
            c.1.extend(e.faces.iter().cloned());
        }
    }
    let spots: Vec<FaceSpot> = corners
        .into_values()
        .filter(|(_, faces)| faces.contains(face))
        .map(|(at, faces)| FaceSpot {
            at,
            origin: FaceOrigin::Vertex {
                faces: faces.into_iter().collect(),
            },
        })
        .collect();
    if !spots.is_empty() {
        return spots;
    }
    let mut circles: Vec<(f64, FaceSpot)> = edges
        .iter()
        .filter(|e| full(e) && e.faces[0] == face)
        .filter_map(|e| {
            let arc = e.arc?;
            Some((
                arc.radius,
                FaceSpot {
                    at: arc.center,
                    origin: FaceOrigin::Center {
                        faces: e.faces.clone(),
                    },
                },
            ))
        })
        .collect();
    circles.sort_by(|a, b| b.0.total_cmp(&a.0));
    circles.into_iter().map(|(_, s)| s).collect()
}

/// `spots` のうち、平面 `plane` の向きで見て `corner` の隅にいちばん近いもの(角の囲みの隅からの
/// 距離。同じなら、下の隅なら下の物、次に左の隅なら左の物)。
pub fn corner_spot<'a>(
    spots: &'a [FaceSpot],
    plane: &Plane,
    corner: Corner,
) -> Option<&'a FaceSpot> {
    let local: Vec<[f64; 2]> = spots.iter().map(|s| plane.to_local(s.at)).collect();
    let (sx, sy) = corner.signs();
    let pick = |i: usize, s: f64| {
        local
            .iter()
            .map(|q| q[i] * s)
            .fold(f64::NEG_INFINITY, f64::max)
            * s
    };
    let target = [pick(0, sx), pick(1, sy)];
    let score = |q: &[f64; 2]| {
        let d = (q[0] - target[0]).hypot(q[1] - target[1]);
        ((d * 1e6).round() as i64, -sy * q[1], -sx * q[0])
    };
    (0..spots.len())
        .min_by(|&a, &b| {
            let (da, ya, xa) = score(&local[a]);
            let (db, yb, xb) = score(&local[b]);
            da.cmp(&db).then(ya.total_cmp(&yb)).then(xa.total_cmp(&xb))
        })
        .map(|i| &spots[i])
}

/// 平面 `base` の原点を点 `at` へ移した平面(向きはそのまま)。
fn moved_to(base: Plane, at: P3) -> Plane {
    Plane {
        origin: base.to_world(base.to_local(at)),
        ..base
    }
}

/// 面 `face` の平らな面の情報(無い・平らでなければ誤りの文)。
fn planar(
    kernel: &dyn Kernel,
    shape: ShapeHandle,
    face: &str,
) -> Result<kmb_core::kernel::PlanarFace, String> {
    let faces = kernel.faces(shape).map_err(|e| e.to_string())?;
    let f = faces
        .iter()
        .find(|f| f.name == face)
        .ok_or_else(|| format!("面 {face} が見つかりません(参照が切れています)"))?;
    f.planar
        .ok_or_else(|| format!("面 {face} は平らではありません"))
}

/// 新しく面にスケッチを作る時の原点(面の左下の角。角が無ければ円の中心、それも無ければ世界の
/// 原点を写した点)。
pub fn default_origin(kernel: &dyn Kernel, shape: ShapeHandle, face: &str) -> FaceOrigin {
    let Ok(p) = planar(kernel, shape, face) else {
        return FaceOrigin::World;
    };
    let base = Plane::from_point_normal(p.point, p.normal);
    let spots = face_spots(kernel, shape, face);
    corner_spot(&spots, &base, Corner::LowerLeft).map_or(FaceOrigin::World, |s| s.origin.clone())
}

/// 面 `face` の上のスケッチの平面(向きは面の法線から、原点は `origin` の所)と、原点の角が
/// 見つからなかった時の知らせ(その時は今の面の左下の角を原点にする)。
pub fn face_plane(
    kernel: &dyn Kernel,
    shape: ShapeHandle,
    face: &str,
    origin: &FaceOrigin,
) -> Result<(Plane, Option<String>), String> {
    let p = planar(kernel, shape, face)?;
    let base = Plane::from_point_normal(p.point, p.normal);
    if origin.is_world() {
        return Ok((base, None));
    }
    let spots = face_spots(kernel, shape, face);
    if let Some(s) = spots.iter().find(|s| s.origin == *origin) {
        return Ok((moved_to(base, s.at), None));
    }
    let what = match origin {
        FaceOrigin::Vertex { faces } => format!("面 {} が交わる角", faces.join("・")),
        FaceOrigin::Center { faces } => format!("面 {} の円の中心", faces.join("・")),
        FaceOrigin::World => unreachable!(),
    };
    Ok(match corner_spot(&spots, &base, Corner::LowerLeft) {
        Some(s) => (
            moved_to(base, s.at),
            Some(format!(
                "スケッチの原点({what})が見つかりません。面の左下の角を原点にしています(スケッチのパネルの「原点」で選び直せます)"
            )),
        ),
        None => (
            base,
            Some(format!(
                "スケッチの原点({what})が見つかりません。世界の原点を面へ写した点を原点にしています"
            )),
        ),
    })
}

impl Sketch {
    /// 面の上のスケッチの原点を `origin` に替える。`shift` は新しい原点の、今の平面の座標
    /// (mm)。描いた形は世界の中で動かさない(座標を `shift` だけ戻す)。面の上でなければ何も
    /// しない。
    pub fn with_origin(&self, origin: FaceOrigin, shift: [f64; 2]) -> Option<Sketch> {
        let SketchPlane::Face { feature, face, .. } = &self.plane else {
            return None;
        };
        let mut s = self.clone();
        s.plane = SketchPlane::Face {
            feature: *feature,
            face: face.clone(),
            origin,
        };
        let ids: Vec<_> = s.entities.iter().map(|e| e.id()).collect();
        s.move_entities(&ids, [-shift[0], -shift[1]]);
        for p in &mut s.patterns {
            if let crate::PatternDef::Circular(c) = &mut p.def
                && let crate::PatternCenter::At(at) = &mut c.center
            {
                *at = [at[0] - shift[0], at[1] - shift[1]];
            }
        }
        Some(s)
    }
}
