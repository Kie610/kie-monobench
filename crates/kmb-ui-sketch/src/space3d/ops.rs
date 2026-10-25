//! 3D スケッチの選びから拘束と寸法を作る(画面に依らない部分)。
//! 道具は、先に選んでから押す(選んでいなければ理由を返す)。

use kmb_sketch::{ConstraintId, EntityId, SolveStatus};
use kmb_sketch_3d::{
    Axis, Entity3d, FaceRef, Kind3d, P3, PlaneRef, Sketch3d, Solution3d, geom, measure, solve3d,
};
use std::collections::HashMap;

/// 選んだ物(要素か面か作図面)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Sel {
    Ent(EntityId),
    Face(FaceRef),
    /// 作図面
    Plane(u32),
    /// ボディから求めた線(交差・交差曲線・サーフェスに投影)
    Derived(u32),
}

/// 選んだ物を種類ごとに分けたもの(選んだ順)。
#[derive(Default, Debug)]
pub(crate) struct Picked {
    pub points: Vec<EntityId>,
    pub lines: Vec<EntityId>,
    pub circles: Vec<EntityId>,
    /// 面と作図面(平面の拘束・寸法の相手)
    pub faces: Vec<PlaneRef>,
}

impl Picked {
    fn only(&self, points: usize, lines: usize, circles: usize, faces: usize) -> bool {
        self.points.len() == points
            && self.lines.len() == lines
            && self.circles.len() == circles
            && self.faces.len() == faces
    }

    pub fn is_empty(&self) -> bool {
        self.only(0, 0, 0, 0)
    }
}

pub(crate) fn classify(sk: &Sketch3d, sel: &[Sel]) -> Picked {
    let mut p = Picked::default();
    for s in sel {
        match s {
            Sel::Face(f) => p.faces.push(PlaneRef::Face { lines: f.clone() }),
            Sel::Plane(id) => p.faces.push(PlaneRef::Work { id: *id }),
            Sel::Derived(_) => {}
            Sel::Ent(id) => match sk.entity(*id) {
                Some(Entity3d::Point { .. }) => p.points.push(*id),
                Some(Entity3d::Line { .. }) => p.lines.push(*id),
                Some(Entity3d::Circle { .. }) => p.circles.push(*id),
                None => {}
            },
        }
    }
    p
}

/// 拘束の道具(名前、アイコン、選ぶ物)。
pub(crate) const CONSTRAINTS: [(&str, &str, &str); 11] = [
    ("X に沿う", "alongx", "線分(何本でも)"),
    ("Y に沿う", "alongy", "線分(何本でも)"),
    ("Z に沿う", "alongz", "線分(何本でも)"),
    ("平行", "parallel", "線分を 2 本"),
    ("直交", "perp", "線分を 2 本"),
    ("等しい", "equal", "線分を 2 本以上か、円を 2 つ以上"),
    (
        "一致",
        "coinc",
        "点を 2 つ(点と線分なら線の上、点と面なら面の上)",
    ),
    ("中点", "midpoint", "点と線分"),
    ("固定", "fix", "点・線分・円"),
    (
        "平面の上",
        "onplane",
        "点か線分と、面(面を選ばなければ相手の平面)",
    ),
    (
        "平面に平行",
        "parplane",
        "線分と、面(面を選ばなければ相手の平面)",
    ),
];

/// 寸法の道具(名前、アイコン、選ぶ物)。
pub(crate) const DIMENSIONS: [(&str, &str, &str); 7] = [
    (
        "寸法",
        "dim",
        "線分(長さ)・点 2 つ(距離)・円(直径)・線分 2 本(角度)・線分と面(平面との角度)・面 2 つ(面の角度)・点と面(距離)",
    ),
    ("X の差", "axisdist", "点を 2 つか、線分を 1 本"),
    ("Y の差", "axisdist", "点を 2 つか、線分を 1 本"),
    ("Z の差", "axisdist", "点を 2 つか、線分を 1 本"),
    (
        "平面との距離",
        "pdist",
        "点と、面(面を選ばなければ相手の平面)",
    ),
    (
        "平面との角度",
        "pangle",
        "線分と、面(面を選ばなければ相手の平面)",
    ),
    (
        "面の角度",
        "fangle",
        "面を 2 つか、面を 1 つ(相手の平面との角度)",
    ),
];

fn need(name: &str, what: &str) -> String {
    format!("{name}: {what}を選んでから押す")
}

/// 選んだ面か作図面。選んでいなければ相手の平面。
fn plane_of(p: &Picked, target: &PlaneRef) -> PlaneRef {
    p.faces.first().cloned().unwrap_or_else(|| target.clone())
}

/// 線分の両端も含めた点(固定や平面の上で、線分を選んだ時)。
fn points_with_ends(sk: &Sketch3d, p: &Picked) -> Vec<EntityId> {
    let mut out = p.points.clone();
    for l in &p.lines {
        if let Some((a, b)) = sk.line_ends(*l) {
            for q in [a, b] {
                if !out.contains(&q) {
                    out.push(q);
                }
            }
        }
    }
    out
}

/// 形の拘束を作る。
pub(crate) fn constraint_kinds(
    name: &str,
    sk: &Sketch3d,
    sel: &[Sel],
    target: &PlaneRef,
) -> Result<Vec<Kind3d>, String> {
    let p = classify(sk, sel);
    let what = CONSTRAINTS
        .iter()
        .find(|c| c.0 == name)
        .map_or("物", |c| c.2);
    let err = || need(name, what);
    let axis = match name {
        "X に沿う" => Some(Axis::X),
        "Y に沿う" => Some(Axis::Y),
        "Z に沿う" => Some(Axis::Z),
        _ => None,
    };
    if let Some(axis) = axis {
        if p.lines.is_empty() || !p.points.is_empty() || !p.circles.is_empty() {
            return Err(err());
        }
        return Ok(p
            .lines
            .iter()
            .map(|l| Kind3d::Along { line: *l, axis })
            .collect());
    }
    Ok(match name {
        "平行" if p.only(0, 2, 0, 0) => vec![Kind3d::Parallel {
            a: p.lines[0],
            b: p.lines[1],
        }],
        "直交" if p.only(0, 2, 0, 0) => vec![Kind3d::Perpendicular {
            a: p.lines[0],
            b: p.lines[1],
        }],
        "等しい" if p.lines.len() >= 2 && p.only(0, p.lines.len(), 0, 0) => p.lines[1..]
            .iter()
            .map(|b| Kind3d::Equal {
                a: p.lines[0],
                b: *b,
            })
            .collect(),
        "等しい" if p.circles.len() >= 2 && p.only(0, 0, p.circles.len(), 0) => p.circles[1..]
            .iter()
            .map(|b| Kind3d::Equal {
                a: p.circles[0],
                b: *b,
            })
            .collect(),
        "一致" if p.only(2, 0, 0, 0) => vec![Kind3d::Coincident {
            a: p.points[0],
            b: p.points[1],
        }],
        "一致" if p.only(1, 1, 0, 0) => vec![Kind3d::PointOnLine {
            point: p.points[0],
            line: p.lines[0],
        }],
        "一致" if p.only(1, 0, 0, 1) => vec![Kind3d::PointOnPlane {
            point: p.points[0],
            plane: plane_of(&p, target),
        }],
        "中点" if p.only(1, 1, 0, 0) => vec![Kind3d::Midpoint {
            point: p.points[0],
            line: p.lines[0],
        }],
        "固定" if p.faces.is_empty() && !p.is_empty() => {
            let mut v: Vec<Kind3d> = points_with_ends(sk, &p)
                .into_iter()
                .map(|point| Kind3d::Fix { point })
                .collect();
            v.extend(p.circles.iter().map(|c| Kind3d::Fix { point: *c }));
            v
        }
        "平面の上" if p.circles.is_empty() && p.faces.len() <= 1 && !p.is_empty() => {
            let plane = plane_of(&p, target);
            let pts = points_with_ends(sk, &p);
            if pts.is_empty() {
                return Err(err());
            }
            pts.into_iter()
                .map(|point| Kind3d::PointOnPlane {
                    point,
                    plane: plane.clone(),
                })
                .collect()
        }
        "平面に平行"
            if !p.lines.is_empty()
                && p.points.is_empty()
                && p.circles.is_empty()
                && p.faces.len() <= 1 =>
        {
            let plane = plane_of(&p, target);
            p.lines
                .iter()
                .map(|l| Kind3d::ParallelToPlane {
                    line: *l,
                    plane: plane.clone(),
                })
                .collect()
        }
        _ => return Err(err()),
    })
}

/// 長さ(mm)と角度(ラジアン)を、寸法の式に書く(表示の桁で丸める)。
pub(crate) fn value_text(f: &kmb_sketch::DimFormat, angle: bool, v: f64) -> String {
    if angle {
        format!("{:.*} deg", f.angle_digits as usize, v.to_degrees())
    } else {
        // 文書の単位で書く(インチの文書は in。候補 19)
        let u = kmb_core::display_unit();
        format!("{:.*} {}", f.length_digits as usize, u.from_mm(v), u.name())
    }
}

/// 寸法を作る(値は今の形を測った値)。
pub(crate) fn dimension_kind(
    fmt: &kmb_sketch::DimFormat,
    name: &str,
    sk: &Sketch3d,
    sol: &Solution3d,
    sel: &[Sel],
    target: &PlaneRef,
) -> Result<Kind3d, String> {
    let p = classify(sk, sel);
    let what = DIMENSIONS
        .iter()
        .find(|c| c.0 == name)
        .map_or("物", |c| c.2);
    let err = || need(name, what);
    let v = String::new;
    let axis = match name {
        "X の差" => Some(Axis::X),
        "Y の差" => Some(Axis::Y),
        "Z の差" => Some(Axis::Z),
        _ => None,
    };
    let mut kind = if let Some(axis) = axis {
        let (a, b) = if p.only(2, 0, 0, 0) {
            (p.points[0], p.points[1])
        } else if p.only(0, 1, 0, 0) {
            sk.line_ends(p.lines[0]).ok_or_else(err)?
        } else {
            return Err(err());
        };
        // 正の値になる向きにする
        let (pa, pb) = (sol.points.get(&a), sol.points.get(&b));
        let (a, b) = match (pa, pb) {
            (Some(pa), Some(pb)) if pb[axis.index()] < pa[axis.index()] => (b, a),
            _ => (a, b),
        };
        Kind3d::AxisDistance {
            a,
            b,
            axis,
            value: v(),
            flip: false,
        }
    } else {
        match name {
            "寸法" if p.only(0, 1, 0, 0) => Kind3d::Length {
                line: p.lines[0],
                value: v(),
            },
            "寸法" if p.only(2, 0, 0, 0) => Kind3d::Distance {
                a: p.points[0],
                b: p.points[1],
                value: v(),
            },
            "寸法" if p.only(0, 0, 1, 0) => Kind3d::Diameter {
                circle: p.circles[0],
                value: v(),
            },
            "寸法" if p.only(0, 2, 0, 0) => Kind3d::Angle {
                a: p.lines[0],
                b: p.lines[1],
                value: v(),
            },
            "寸法" | "平面との角度"
                if p.only(0, 1, 0, 1) || (name != "寸法" && p.only(0, 1, 0, 0)) =>
            {
                Kind3d::PlaneAngle {
                    line: p.lines[0],
                    plane: plane_of(&p, target),
                    value: v(),
                }
            }
            "寸法" | "面の角度" if p.only(0, 0, 0, 2) => Kind3d::FaceAngle {
                a: p.faces[0].clone(),
                b: p.faces[1].clone(),
                value: v(),
            },
            "寸法" | "面の角度" if p.only(0, 0, 0, 1) && p.faces[0] != *target => {
                Kind3d::FaceAngle {
                    a: p.faces[0].clone(),
                    b: target.clone(),
                    value: v(),
                }
            }
            "寸法" | "平面との距離" if p.only(1, 0, 0, 1) || p.only(1, 0, 0, 0) => {
                Kind3d::PlaneDistance {
                    point: p.points[0],
                    plane: plane_of(&p, target),
                    value: v(),
                }
            }
            _ => return Err(err()),
        }
    };
    let now = measure(sk, sol, &kind).ok_or_else(err)?;
    let angle = kind.is_angle();
    if let Some(val) = kind.value_mut() {
        *val = value_text(fmt, angle, now.abs());
    }
    Ok(kind)
}

/// 拘束を足したスケッチ。足すと解けなくなる・余分になる寸法は、測るだけ(被駆動)にする。
/// 形の拘束で解けなくなる時は足さずに理由を返す。返す番号は足した拘束。
pub(crate) fn add_constraints(
    sk: &Sketch3d,
    kinds: Vec<Kind3d>,
    values: &HashMap<ConstraintId, f64>,
    eval: &dyn Fn(&str) -> Option<f64>,
) -> Result<(Sketch3d, Vec<ConstraintId>, Option<String>), String> {
    let mut s = sk.clone();
    let mut values = values.clone();
    let mut ids = Vec::new();
    let mut note = None;
    for kind in kinds {
        // 同じ拘束が既にあれば足さない
        if s.constraints.iter().any(|c| c.kind == kind) {
            continue;
        }
        let dim = kind.value().map(|v| v.to_string());
        let label = kind.label();
        let id = s.add_constraint(kind);
        if let Some(src) = &dim
            && let Some(v) = eval(src)
        {
            values.insert(id, v);
        }
        let blame = |st: &SolveStatus| match st {
            SolveStatus::Inconsistent { conflicting } => conflicting.contains(&id),
            SolveStatus::OverConstrained { redundant } => redundant.contains(&id),
            _ => false,
        };
        let bad = match solve3d(&s, &values) {
            Ok(sol) => blame(&sol.status),
            Err(_) => true,
        };
        if bad {
            if dim.is_some() {
                if let Some(c) = s.constraints.iter_mut().find(|c| c.id == id) {
                    c.reference = true;
                }
                values.remove(&id);
                note = Some(
                    "ほかの拘束で決まっているので、測るだけの寸法(被駆動)にしました".to_string(),
                );
            } else {
                return Err(format!(
                    "「{label}」を付けると、ほかの拘束と同時に満たせないか余分になります"
                ));
            }
        }
        ids.push(id);
    }
    if ids.is_empty() && note.is_none() {
        return Err("同じ拘束が既に付いています".into());
    }
    // 解いた位置を保存して、次の描き足しの始めの値にする
    if let Ok(sol) = solve3d(&s, &values) {
        s.adopt(&sol);
    }
    Ok((s, ids, note))
}

/// 描く平面(平面の上の点と法線と、平面の上の 2 本の向き)。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawPlane {
    pub origin: P3,
    pub n: P3,
    pub u: P3,
    pub v: P3,
    /// 平面の上の 2 本の向きが座標の軸なら、その軸(引いた線に軸に沿う拘束を付ける)
    pub axes: Option<(Axis, Axis)>,
    /// 作図面なら、その番号(描いた点を作図面に乗せる)
    pub work: Option<u32>,
    /// `u` の向きの線分(傾けた作図面の軸の線など)。`u` に沿う線はこれに平行、`v` に沿う線は
    /// これに直角の拘束を付ける
    pub u_line: Option<EntityId>,
}

impl DrawPlane {
    /// 平面の上の点(平面へ下ろした足)。
    pub fn project(&self, p: P3) -> P3 {
        let d = geom::dot(geom::sub(p, self.origin), self.n);
        geom::sub(p, geom::scale(self.n, d))
    }

    pub fn at(&self, from: P3, du: f64, dv: f64) -> P3 {
        geom::add(
            from,
            geom::add(geom::scale(self.u, du), geom::scale(self.v, dv)),
        )
    }

    /// 向きを推して拘束を付けられるか(座標の軸か、軸の線があるか)。
    pub fn infers(&self) -> bool {
        self.axes.is_some() || self.u_line.is_some()
    }

    /// `u`(0)か `v`(1)の向きに引いた線分 `line` に付ける拘束。
    pub fn dir_kind(&self, k: usize, line: EntityId) -> Option<Kind3d> {
        if let Some((a, b)) = self.axes {
            return Some(Kind3d::Along {
                line,
                axis: if k == 0 { a } else { b },
            });
        }
        let other = self.u_line?;
        Some(if k == 0 {
            Kind3d::Parallel { a: line, b: other }
        } else {
            Kind3d::Perpendicular { a: line, b: other }
        })
    }

    /// 向きの名前(描きかけの横に出す)。
    pub fn dir_label(&self, k: usize) -> String {
        match (self.axes, k) {
            (Some((a, _)), 0) => format!("{} に沿う", a.label()),
            (Some((_, b)), _) => format!("{} に沿う", b.label()),
            (None, 0) => "軸の線に平行".into(),
            (None, _) => "軸の線に直角".into(),
        }
    }

    /// 平面の上で `from` から `to` へ引く線が、`u` か `v` に近ければ(4° 以内)その向き
    /// (0 か 1)。近ければ `to` をその向きの上へ寄せる。拘束を付けられない平面では推さない。
    pub fn infer(&self, from: P3, to: &mut P3) -> Option<usize> {
        if !self.infers() {
            return None;
        }
        let d = geom::sub(*to, from);
        let (du, dv) = (geom::dot(d, self.u), geom::dot(d, self.v));
        let l = du.hypot(dv);
        if l < 1e-9 {
            return None;
        }
        let tol = 4f64.to_radians().sin();
        if dv.abs() / l < tol {
            *to = self.at(from, du, 0.0);
            Some(0)
        } else if du.abs() / l < tol {
            *to = self.at(from, 0.0, dv);
            Some(1)
        } else {
            None
        }
    }
}

/// 作図面を作る道具で選ぶ物。
pub(crate) const WORK_PLANE_HINT: &str = "点を 3 つ(3 点を通る)・線分(と面か作図面。線を通り傾ける)・面(面の上)・点を 1 つ(その点を通るようにずらす)のどれかを選んでから押す。何も選ばなければ今の作図面の位置で作る";

/// 選びから作図面の決め方を作る(選んだ物で決め方が決まる)。
/// `base` は今の作図面の向きの座標の平面、`at` は今の作図面の点、`target` は相手の平面。
pub(crate) fn work_plane_def(
    sk: &Sketch3d,
    sol: &Solution3d,
    sel: &[Sel],
    base: kmb_sketch_3d::AxisPlane,
    at: P3,
    target: &PlaneRef,
) -> Result<kmb_sketch_3d::PlaneDef, String> {
    use kmb_sketch_3d::PlaneDef;
    let p = classify(sk, sel);
    let err = || format!("作図面を作る: {WORK_PLANE_HINT}");
    let offset = |q: P3| PlaneDef::Offset {
        base,
        offset: q[base.normal().index()],
    };
    if p.only(3, 0, 0, 0) {
        let q: Vec<P3> = p
            .points
            .iter()
            .map(|id| sol.points.get(id).copied())
            .collect::<Option<_>>()
            .ok_or_else(err)?;
        let n = geom::cross(geom::sub(q[1], q[0]), geom::sub(q[2], q[0]));
        if geom::len(n) < 1e-9 {
            return Err("作図面を作る: 3 点が一直線に並んでいて平面が決まりません".into());
        }
        return Ok(PlaneDef::ThreePoints {
            a: p.points[0],
            b: p.points[1],
            c: p.points[2],
        });
    }
    if p.lines.len() == 1 && p.points.is_empty() && p.circles.is_empty() && p.faces.len() <= 1 {
        return Ok(PlaneDef::Tilted {
            line: p.lines[0],
            base: plane_of(&p, target),
            angle: 30f64.to_radians(),
            flip: false,
        });
    }
    if p.only(0, 0, 0, 1)
        && let PlaneRef::Face { lines } = &p.faces[0]
    {
        return Ok(PlaneDef::Face {
            lines: lines.clone(),
        });
    }
    if p.only(1, 0, 0, 0) {
        return Ok(offset(
            sol.points.get(&p.points[0]).copied().ok_or_else(err)?,
        ));
    }
    if p.is_empty() {
        return Ok(offset(at));
    }
    Err(err())
}
