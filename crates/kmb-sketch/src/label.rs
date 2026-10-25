//! 寸法の札の位置。図形に対する相対の座標で持つので、図形を動かす・回す・寸法を変えると、札も
//! 一緒に付いて来る(Kie のレビュー K5)。座標の取り方は寸法の種類ごと:
//!
//! - 長さ・距離・点と線の距離など: 測る 2 点の真ん中を原点に、線(測る向き)に沿う軸と、
//!   それに直角な軸の (沿う量, 離れる量)。横・縦の距離は軸が X・Y のまま
//! - 半径・直径: 中心から見た向き(rad)と、円周から外への距離(半径を変えても札は円の外のまま)
//! - 角度: 2 本の線の交点から見た向き(1 本目の線の向きからの rad)と、交点からの距離
//!
//! 古いファイルの `at`(平面の絶対座標)は、保存されている図形の位置について読み替える。

use crate::model::{Constraint, ConstraintKind as K, Entity, EntityId, Sketch};
use crate::solver::Solution;

/// 要素の位置の読み方(解いた位置か、保存されている位置)。
struct Geom<'a> {
    sketch: &'a Sketch,
    sol: Option<&'a Solution>,
}

impl Geom<'_> {
    fn pt(&self, id: EntityId) -> Option<[f64; 2]> {
        if let Some(s) = self.sol {
            return s.points.get(&id).copied();
        }
        match self.sketch.entity(id)? {
            Entity::Point { x, y, .. } => Some([*x, *y]),
            _ => None,
        }
    }

    fn line(&self, id: EntityId) -> Option<([f64; 2], [f64; 2])> {
        match self.sketch.entity(id)? {
            Entity::Line { a, b, .. } => Some((self.pt(*a)?, self.pt(*b)?)),
            _ => None,
        }
    }

    /// 円・円弧の中心と半径。
    fn round(&self, id: EntityId) -> Option<([f64; 2], f64)> {
        match self.sketch.entity(id)? {
            Entity::Circle { center, radius, .. } => {
                let r = match self.sol {
                    Some(s) => *s.radii.get(&id)?,
                    None => *radius,
                };
                Some((self.pt(*center)?, r))
            }
            Entity::Arc { center, start, .. } => {
                let (c, s) = (self.pt(*center)?, self.pt(*start)?);
                Some((c, (s[0] - c[0]).hypot(s[1] - c[1])))
            }
            _ => None,
        }
    }

    /// 点か、線分なら真ん中、円なら中心(円との距離の相手)。
    fn anchor(&self, id: EntityId) -> Option<[f64; 2]> {
        match self.sketch.entity(id)? {
            Entity::Point { .. } => self.pt(id),
            Entity::Line { .. } => {
                let (a, b) = self.line(id)?;
                Some(mid(a, b))
            }
            _ => self.round(id).map(|(c, _)| c),
        }
    }
}

/// 札の座標の取り方。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Frame {
    /// 原点と、沿う軸の向き(単位)。相対の座標は (沿う量, 直角に離れる量)
    Line { origin: [f64; 2], u: [f64; 2] },
    /// 円の中心と半径。相対の座標は (向き, 円周から外への距離)
    Round { center: [f64; 2], r: f64 },
    /// 交点と、基準の向き(rad)。相対の座標は (基準からの向き, 交点からの距離)
    Turn { center: [f64; 2], base: f64 },
}

impl Frame {
    fn to_world(self, l: [f64; 2]) -> [f64; 2] {
        match self {
            Frame::Line { origin, u } => [
                origin[0] + l[0] * u[0] - l[1] * u[1],
                origin[1] + l[0] * u[1] + l[1] * u[0],
            ],
            Frame::Round { center, r } => {
                let d = (r + l[1]).max(0.0);
                [center[0] + d * l[0].cos(), center[1] + d * l[0].sin()]
            }
            Frame::Turn { center, base } => {
                let a = base + l[0];
                [center[0] + l[1] * a.cos(), center[1] + l[1] * a.sin()]
            }
        }
    }

    fn to_local(self, w: [f64; 2]) -> [f64; 2] {
        match self {
            Frame::Line { origin, u } => {
                let d = [w[0] - origin[0], w[1] - origin[1]];
                [d[0] * u[0] + d[1] * u[1], -d[0] * u[1] + d[1] * u[0]]
            }
            Frame::Round { center, r } => {
                let d = [w[0] - center[0], w[1] - center[1]];
                [d[1].atan2(d[0]), d[0].hypot(d[1]) - r]
            }
            Frame::Turn { center, base } => {
                let d = [w[0] - center[0], w[1] - center[1]];
                [d[1].atan2(d[0]) - base, d[0].hypot(d[1])]
            }
        }
    }
}

fn mid(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

/// a から b への単位の向き(重なっていれば X 軸)。
fn unit(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]);
    if l < 1e-12 {
        [1.0, 0.0]
    } else {
        [d[0] / l, d[1] / l]
    }
}

/// 点 `x` から直線 a-b へ下ろした足。
fn foot(x: [f64; 2], a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let d = [b[0] - a[0], b[1] - a[1]];
    let dd = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
    let k = ((x[0] - a[0]) * d[0] + (x[1] - a[1]) * d[1]) / dd;
    [a[0] + d[0] * k, a[1] + d[1] * k]
}

/// 2 本の線(延ばした直線)の交点。平行なら `None`。
pub fn line_crossing(p: [f64; 2], q: [f64; 2], r: [f64; 2], u: [f64; 2]) -> Option<[f64; 2]> {
    let (d1, d2) = ([q[0] - p[0], q[1] - p[1]], [u[0] - r[0], u[1] - r[1]]);
    let den = d1[0] * d2[1] - d1[1] * d2[0];
    let scale = d1[0].hypot(d1[1]) * d2[0].hypot(d2[1]);
    if den.abs() <= 1e-9 * scale.max(1e-12) {
        return None;
    }
    let k = ((r[0] - p[0]) * d2[1] - (r[1] - p[1]) * d2[0]) / den;
    Some([p[0] + d1[0] * k, p[1] + d1[1] * k])
}

const HORIZONTAL: [f64; 2] = [1.0, 0.0];

fn frame(g: &Geom, kind: &K) -> Option<Frame> {
    let line = |origin, u| Some(Frame::Line { origin, u });
    match kind {
        K::Length { line: l, .. } => {
            let (a, b) = g.line(*l)?;
            line(mid(a, b), unit(a, b))
        }
        K::Distance { a, b, .. } => {
            let (a, b) = (g.pt(*a)?, g.pt(*b)?);
            line(mid(a, b), unit(a, b))
        }
        K::HorizontalDistance { a, b, .. } | K::VerticalDistance { a, b, .. } => {
            line(mid(g.pt(*a)?, g.pt(*b)?), HORIZONTAL)
        }
        K::PointLineDistance { point, line: l, .. } => {
            let x = g.pt(*point)?;
            let (a, b) = g.line(*l)?;
            line(mid(x, foot(x, a, b)), unit(a, b))
        }
        K::LinearDiameter { point, line: l, .. } => {
            let x = g.pt(*point)?;
            let (a, b) = g.line(*l)?;
            line(foot(x, a, b), unit(a, b))
        }
        K::CircleDistance { circle, other, .. } => {
            let (c, _) = g.round(*circle)?;
            let o = g.anchor(*other)?;
            line(mid(c, o), unit(c, o))
        }
        K::Radius { circle, .. } | K::Diameter { circle, .. } => {
            let (center, r) = g.round(*circle)?;
            Some(Frame::Round { center, r })
        }
        K::Angle { a, b, .. } => {
            let ((p, q), (r, u)) = (g.line(*a)?, g.line(*b)?);
            let d = unit(p, q);
            match line_crossing(p, q, r, u) {
                Some(center) => Some(Frame::Turn {
                    center,
                    base: d[1].atan2(d[0]),
                }),
                // 平行(角が決まらない): 2 本の真ん中の間を原点に
                None => line(mid(mid(p, q), mid(r, u)), d),
            }
        }
        _ => None,
    }
}

impl Sketch {
    /// 寸法 `c` の札の位置(平面の座標)。置いた位置がなければ `None`(形の外側に出す)。
    pub fn label_at(&self, c: &Constraint, sol: &Solution) -> Option<[f64; 2]> {
        let now = frame(
            &Geom {
                sketch: self,
                sol: Some(sol),
            },
            &c.kind,
        )?;
        if let Some(l) = c.label {
            return Some(now.to_world(l));
        }
        // 古い絶対座標: 保存されている図形の位置について読み替える
        let at = c.at?;
        let stored = frame(
            &Geom {
                sketch: self,
                sol: None,
            },
            &c.kind,
        );
        Some(match stored {
            Some(f) => now.to_world(f.to_local(at)),
            None => at,
        })
    }

    /// 寸法 `id` の札を平面の点 `w` に置く(解いた形 `sol` に対する相対の座標で覚える)。
    pub fn place_label(&mut self, id: crate::model::ConstraintId, w: [f64; 2], sol: &Solution) {
        let Some(i) = self.constraints.iter().position(|c| c.id == id) else {
            return;
        };
        let f = frame(
            &Geom {
                sketch: self,
                sol: Some(sol),
            },
            &self.constraints[i].kind,
        );
        let c = &mut self.constraints[i];
        match f {
            Some(f) => {
                c.label = Some(f.to_local(w));
                c.at = None;
            }
            None => c.at = Some(w),
        }
    }

    /// 古い絶対座標の札(`at`)を、保存されている図形の位置について相対の座標へ移す。
    /// 保存されている位置を書き換える前([`Sketch::adopt_solution`])に呼ぶ。
    pub(crate) fn anchor_labels(&mut self) {
        let g = Geom {
            sketch: self,
            sol: None,
        };
        let moved: Vec<(usize, [f64; 2])> = self
            .constraints
            .iter()
            .enumerate()
            .filter(|(_, c)| c.label.is_none())
            .filter_map(|(i, c)| Some((i, frame(&g, &c.kind)?.to_local(c.at?))))
            .collect();
        for (i, l) in moved {
            self.constraints[i].label = Some(l);
            self.constraints[i].at = None;
        }
    }
}

/// 寸法の式を `k` 倍にする。「10」「10 mm」のような数だけなら数を直し、式なら括弧でくるむ。
pub fn scaled(value: &str, k: f64) -> String {
    let v = value.trim();
    let (num, unit) = match v.strip_suffix("mm") {
        Some(n) => (n.trim(), " mm"),
        None => (v, ""),
    };
    match num.parse::<f64>() {
        Ok(n) if n.is_finite() => format!("{}{unit}", n * k),
        _ if k >= 1.0 => format!("({v}) * {k}"),
        _ => format!("({v}) / {}", 1.0 / k),
    }
}

impl Sketch {
    /// 半径の寸法を直径の寸法に、直径を半径に変える(値は同じ大きさの円になるよう直す。札の
    /// 位置もそのまま)。半径・直径の寸法でなければ偽。Kie のレビュー K9。
    pub fn toggle_radius(&mut self, id: crate::model::ConstraintId) -> bool {
        let Some(c) = self.constraints.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        c.kind = match &c.kind {
            K::Radius { circle, value } => K::Diameter {
                circle: *circle,
                value: scaled(value, 2.0),
            },
            K::Diameter { circle, value } => K::Radius {
                circle: *circle,
                value: scaled(value, 0.5),
            },
            _ => return false,
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn frames_round_trip() {
        let fs = [
            Frame::Line {
                origin: [3.0, -2.0],
                u: unit([0.0, 0.0], [3.0, 4.0]),
            },
            Frame::Round {
                center: [1.0, 1.0],
                r: 5.0,
            },
            Frame::Turn {
                center: [-4.0, 2.0],
                base: 0.7,
            },
        ];
        for f in fs {
            let w = [7.5, -3.25];
            assert!(close(f.to_world(f.to_local(w)), w), "{f:?}");
        }
    }

    #[test]
    fn radius_and_diameter_values_are_converted() {
        assert_eq!(scaled("10", 2.0), "20");
        assert_eq!(scaled("12.5 mm", 0.5), "6.25 mm");
        assert_eq!(scaled("w + 2", 2.0), "(w + 2) * 2");
        assert_eq!(scaled("d1", 0.5), "(d1) / 2");
    }

    /// 古いファイルの絶対座標の札は、保存されている形について読み替え、形が動けば付いて来る。
    /// 解いた位置を写す時(編集の前)に相対の座標へ移る。新しく置いた札も同じく付いて来る。
    #[test]
    fn old_absolute_labels_are_read_relative_to_the_stored_shape() {
        use crate::model::{ConstraintId, ConstraintKind};
        use std::collections::HashMap;
        let mut s = Sketch::default();
        let (a, b) = (s.add_point(0.0, 0.0), s.add_point(10.0, 0.0));
        let line = s.add_line(a, b);
        let mut c = Constraint::new(
            ConstraintId(1),
            ConstraintKind::Length {
                line,
                value: "10".into(),
            },
        );
        c.at = Some([5.0, 3.0]);
        s.constraints.push(c);
        let v = HashMap::from([(ConstraintId(1), 10.0)]);
        let sol = crate::solve(&s, &v).unwrap();
        assert!(close(
            s.label_at(&s.constraints[0], &sol).unwrap(),
            [5.0, 3.0]
        ));
        // 形を右へ 7 動かした解: 札も 7 動く
        let mut moved = sol.clone();
        for p in moved.points.values_mut() {
            p[0] += 7.0;
        }
        assert!(close(
            s.label_at(&s.constraints[0], &moved).unwrap(),
            [12.0, 3.0]
        ));
        // 編集の前に解を写すと、相対の座標へ移る(絶対座標は消える)
        let mut t = s.clone();
        t.adopt_solution(&moved);
        assert!(t.constraints[0].at.is_none() && t.constraints[0].label.is_some());
        assert!(close(
            t.label_at(&t.constraints[0], &moved).unwrap(),
            [12.0, 3.0]
        ));
        // 新しく置く: 相対で覚える
        t.place_label(ConstraintId(1), [12.0, -4.0], &moved);
        assert!(close(
            t.label_at(&t.constraints[0], &moved).unwrap(),
            [12.0, -4.0]
        ));
        assert!(close(
            t.label_at(&t.constraints[0], &sol).unwrap(),
            [5.0, -4.0]
        ));
    }
}
