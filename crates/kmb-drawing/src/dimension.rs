//! 名前付きの辺を現在のビューへ解き、寸法の値と線の配置を求める。

use crate::{DimKind, DimRef, RefAt};
use kmb_core::FeatureId;
use kmb_core::kernel::{EdgeInfo, P3, dot};
use kmb_interchange::drawing::{EdgeKey, Path, ViewLine};
use kmb_view_lines::View;
use std::collections::{BTreeMap, BTreeSet};

type P2 = [f64; 2];
#[derive(Clone)]
pub(super) struct ViewGeometry {
    pub frame: View,
    pub at: P2,
    pub scale: f64,
    pub edges: BTreeMap<EdgeKey, EdgeInfo>,
    pub aliases: BTreeMap<FeatureId, FeatureId>,
    pub bodies: BTreeSet<FeatureId>,
    pub lines: Vec<ViewLine>,
}
impl ViewGeometry {
    pub fn canonical_key(&self, key: &EdgeKey) -> EdgeKey {
        EdgeKey {
            body: self.aliases.get(&key.body).copied().unwrap_or(key.body),
            ..key.clone()
        }
    }
    pub fn point(&self, p: P3) -> (P2, f64) {
        let q = self.frame.project(p);
        (
            [
                self.at[0] + self.scale * q[0],
                self.at[1] + self.scale * q[1],
            ],
            q[2],
        )
    }
}

pub(super) struct Resolved<'a> {
    pub space: &'a ViewGeometry,
    pub edge: &'a EdgeInfo,
    pub point: P2,
    pub depth: f64,
}

pub(super) fn resolve<'a>(
    reference: &DimRef,
    views: &'a BTreeMap<String, ViewGeometry>,
) -> Result<Resolved<'a>, String> {
    let space = views
        .get(&reference.view)
        .ok_or("参照したビューがありません。ビューを選び直してください")?;
    let edge = space
        .edges
        .get(&space.canonical_key(&reference.key))
        .ok_or("参照した辺がありません。寸法の辺を選び直してください")?;
    let p = match reference.at {
        RefAt::A => edge.a,
        RefAt::B => edge.b,
        RefAt::Midpoint | RefAt::Line => {
            if let Some(a) = edge.arc {
                a.point(edge.a, a.sweep * 0.5)
            } else if edge.curve.len() > 1 {
                edge.curve[edge.curve.len() / 2]
            } else {
                std::array::from_fn(|i| 0.5 * (edge.a[i] + edge.b[i]))
            }
        }
        RefAt::Center => {
            edge.arc
                .ok_or("この辺は円や円弧ではありません。中心を持つ辺を選んでください")?
                .center
        }
    };
    let (point, depth) = space.point(p);
    Ok(Resolved {
        space,
        edge,
        point,
        depth,
    })
}

pub(super) struct DimensionGeometry {
    pub value: f64,
    pub ends: [P2; 2],
    pub origins: [P2; 2],
    pub text: P2,
    pub arc: Option<Path>,
    pub angle: bool,
    pub prefix: &'static str,
}

pub(super) fn dimension(
    kind: DimKind,
    refs: &[DimRef],
    at: P2,
    views: &BTreeMap<String, ViewGeometry>,
) -> Result<DimensionGeometry, String> {
    if !at.iter().all(|v| v.is_finite()) {
        return Err("寸法の位置は有限の数にしてください".into());
    }
    let circular = matches!(kind, DimKind::Radius | DimKind::Diameter);
    if refs.len() != if circular { 1 } else { 2 } {
        return Err("この寸法に必要な数の参照を指定してください".into());
    }
    if refs.iter().any(|r| r.view != refs[0].view) {
        return Err("寸法の参照は同じビューから選んでください".into());
    }
    let a = resolve(&refs[0], views)?;
    let b = if circular {
        None
    } else {
        Some(resolve(&refs[1], views)?)
    };
    let p = a.point;
    let q = b.as_ref().map_or(p, |b| b.point);
    let mut out = DimensionGeometry {
        value: 0.0,
        ends: [p, q],
        origins: [p, q],
        text: at,
        arc: None,
        angle: false,
        prefix: "",
    };
    match kind {
        DimKind::Horizontal | DimKind::OrdinateX => {
            out.value = (q[0] - p[0]) / a.space.scale;
            if kind == DimKind::Horizontal {
                out.value = out.value.abs();
            }
            out.ends = [[p[0], at[1]], [q[0], at[1]]];
            out.text = [0.5 * (p[0] + q[0]), at[1]];
        }
        DimKind::Vertical | DimKind::OrdinateY => {
            out.value = (q[1] - p[1]) / a.space.scale;
            if kind == DimKind::Vertical {
                out.value = out.value.abs();
            }
            out.ends = [[at[0], p[1]], [at[0], q[1]]];
            out.text = [at[0], 0.5 * (p[1] + q[1])];
        }
        DimKind::Parallel => {
            if (a.depth - b.as_ref().unwrap().depth).abs() > 1e-6 {
                return Err(
                    "このビューでは実長が出ません。辺に平行なビューを選んでください".into(),
                );
            }
            let length = norm(sub(q, p));
            let direction = if length > 1e-12 {
                scale(sub(q, p), 1.0 / length)
            } else {
                [1.0, 0.0]
            };
            let normal = [-direction[1], direction[0]];
            let offset = dot2(sub(at, p), normal);
            out.ends = [add(p, scale(normal, offset)), add(q, scale(normal, offset))];
            out.text = scale(add(out.ends[0], out.ends[1]), 0.5);
            out.value = length / a.space.scale;
        }
        DimKind::Angle => {
            let b = b.as_ref().unwrap();
            for r in [&a, b] {
                if r.edge.arc.is_some() || !r.edge.curve.is_empty() {
                    return Err("角度には2本の直線の辺を選んでください".into());
                }
                if (r.space.point(r.edge.a).1 - r.space.point(r.edge.b).1).abs() > 1e-6 {
                    return Err(
                        "このビューでは実長が出ません。2本の線に平行なビューを選んでください"
                            .into(),
                    );
                }
            }
            let (a0, a1) = (a.space.point(a.edge.a).0, a.space.point(a.edge.b).0);
            let (b0, b1) = (b.space.point(b.edge.a).0, b.space.point(b.edge.b).0);
            let (d, e) = (sub(a1, a0), sub(b1, b0));
            if norm(d) < 1e-12 || norm(e) < 1e-12 {
                return Err("長さの無い辺では角度を測れません".into());
            }
            let determinant = cross(d, e);
            out.angle = true;
            if determinant.abs() < 1e-10 * norm(d) * norm(e) {
                out.value = 0.0;
                return Ok(out);
            }
            let center = add(a0, scale(d, cross(sub(b0, a0), e) / determinant));
            let toward = |mut v: P2| {
                v = scale(v, 1.0 / norm(v));
                if dot2(v, sub(at, center)) < 0.0 {
                    v = scale(v, -1.0);
                }
                v
            };
            let (u, v) = (toward(d), toward(e));
            let theta = cross(u, v).atan2(dot2(u, v));
            let radius = norm(sub(at, center));
            if radius < 1e-9 {
                return Err("角度の寸法線は交点から離して置いてください".into());
            }
            let start = u[1].atan2(u[0]);
            out.value = theta.abs().to_degrees();
            out.ends = [add(center, scale(u, radius)), add(center, scale(v, radius))];
            out.origins = [center; 2];
            out.arc = Some(Path::Arc {
                c: center,
                r: radius,
                start_deg: start.to_degrees(),
                sweep_deg: theta.to_degrees(),
            });
            out.text = add(
                center,
                scale(
                    [(start + theta * 0.5).cos(), (start + theta * 0.5).sin()],
                    radius,
                ),
            );
        }
        DimKind::Radius | DimKind::Diameter => {
            let arc = a
                .edge
                .arc
                .ok_or("半径と直径には円か円弧の辺を選んでください")?;
            if dot(a.space.frame.eye, arc.axis).abs() < 1.0 - 1e-6 {
                return Err(
                    "このビューでは円の実長が出ません。円を正面から見るビューを選んでください"
                        .into(),
                );
            }
            let center = a.space.point(arc.center).0;
            let delta = sub(at, center);
            if norm(delta) < 1e-9 {
                return Err("寸法の文字は円の中心から離して置いてください".into());
            }
            let r = scale(delta, arc.radius * a.space.scale / norm(delta));
            out.ends = [
                if kind == DimKind::Radius {
                    center
                } else {
                    sub(center, r)
                },
                add(center, r),
            ];
            out.origins = out.ends;
            out.value = arc.radius * if kind == DimKind::Radius { 1.0 } else { 2.0 };
            out.prefix = if kind == DimKind::Radius { "R" } else { "⌀" };
        }
    }
    Ok(out)
}

pub(super) fn add(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}
pub(super) fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}
pub(super) fn scale(a: P2, s: f64) -> P2 {
    [a[0] * s, a[1] * s]
}
pub(super) fn norm(a: P2) -> f64 {
    dot2(a, a).sqrt()
}
pub(super) fn dot2(a: P2, b: P2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
pub(super) fn cross(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::{FeatureId, kernel::Seam};
    fn fixture() -> (BTreeMap<String, ViewGeometry>, Vec<DimRef>) {
        let key = EdgeKey {
            body: FeatureId(1),
            faces: ["front".into(), "top".into()],
            index: 0,
        };
        let edge = EdgeInfo {
            a: [0.0, 0.0, 0.0],
            b: [20.0, 0.0, 0.0],
            faces: key.faces.clone(),
            convex: true,
            seam: Seam::Corner,
            arc: None,
            curve: vec![],
        };
        let view = ViewGeometry {
            frame: View::named("front").unwrap(),
            at: [10.0, 20.0],
            scale: 0.5,
            edges: BTreeMap::from([(key.clone(), edge)]),
            aliases: BTreeMap::from([(FeatureId(1), FeatureId(1)), (FeatureId(2), FeatureId(1))]),
            bodies: BTreeSet::from([FeatureId(1)]),
            lines: vec![],
        };
        let refs = vec![
            DimRef {
                view: "v".into(),
                key: key.clone(),
                at: RefAt::A,
            },
            DimRef {
                view: "v".into(),
                key,
                at: RefAt::B,
            },
        ];
        (BTreeMap::from([("v".into(), view)]), refs)
    }
    #[test]
    fn dimensions_read_the_current_geometry_and_never_retarget_a_lost_edge() {
        let (mut views, refs) = fixture();
        assert_eq!(
            dimension(DimKind::Horizontal, &refs, [20.0, 40.0], &views)
                .unwrap()
                .value,
            20.0
        );
        let mut later = refs.clone();
        for r in &mut later {
            r.key.body = FeatureId(2);
        }
        assert_eq!(
            dimension(DimKind::Horizontal, &later, [20.0, 40.0], &views)
                .unwrap()
                .value,
            20.0
        );
        let edge = views
            .get_mut("v")
            .unwrap()
            .edges
            .values_mut()
            .next()
            .unwrap();
        edge.b[0] = 30.0;
        assert_eq!(
            dimension(DimKind::Horizontal, &refs, [20.0, 40.0], &views)
                .unwrap()
                .value,
            30.0
        );
        views.get_mut("v").unwrap().edges.clear();
        assert!(
            dimension(DimKind::Horizontal, &refs, [20.0, 40.0], &views)
                .err()
                .unwrap()
                .contains("辺がありません")
        );
    }
    #[test]
    fn parallel_dimensions_reject_depth_and_horizontal_stays_projected() {
        let (mut views, refs) = fixture();
        views
            .get_mut("v")
            .unwrap()
            .edges
            .values_mut()
            .next()
            .unwrap()
            .b[1] = 3.0;
        assert!(
            dimension(DimKind::Parallel, &refs, [20.0, 40.0], &views)
                .err()
                .unwrap()
                .contains("実長")
        );
        assert_eq!(
            dimension(DimKind::Horizontal, &refs, [20.0, 40.0], &views)
                .unwrap()
                .value,
            20.0
        );
    }
    #[test]
    fn angles_circle_sizes_and_signed_ordinates_use_model_geometry() {
        let (mut views, refs) = fixture();
        assert_eq!(
            dimension(
                DimKind::OrdinateX,
                &[refs[1].clone(), refs[0].clone()],
                [20.0, 40.0],
                &views
            )
            .unwrap()
            .value,
            -20.0
        );
        let mut key = refs[0].key.clone();
        key.index = 1;
        let mut edge = views["v"].edges[&refs[0].key].clone();
        edge.b = [0.0, 0.0, 10.0];
        views.get_mut("v").unwrap().edges.insert(key.clone(), edge);
        let lines = [
            DimRef {
                at: RefAt::Line,
                ..refs[0].clone()
            },
            DimRef {
                key: key.clone(),
                at: RefAt::Line,
                ..refs[0].clone()
            },
        ];
        let angle = dimension(DimKind::Angle, &lines, [15.0, 25.0], &views).unwrap();
        assert!((angle.value - 90.0).abs() < 1e-9);
        assert!(matches!(angle.arc, Some(Path::Arc { .. })));
        views.get_mut("v").unwrap().edges.get_mut(&key).unwrap().b[1] = 1.0;
        assert!(
            dimension(DimKind::Angle, &lines, [15.0, 25.0], &views)
                .err()
                .unwrap()
                .contains("実長")
        );
        let edge = views
            .get_mut("v")
            .unwrap()
            .edges
            .get_mut(&refs[0].key)
            .unwrap();
        edge.a = [5.0, 0.0, 0.0];
        edge.b = edge.a;
        edge.arc = Some(kmb_core::kernel::ArcEdge {
            center: [0.0; 3],
            axis: [0.0, -1.0, 0.0],
            radius: 5.0,
            sweep: std::f64::consts::TAU,
            inside: true,
        });
        let circle = [DimRef {
            at: RefAt::Center,
            ..refs[0].clone()
        }];
        assert_eq!(
            dimension(DimKind::Radius, &circle, [20.0, 20.0], &views)
                .unwrap()
                .value,
            5.0
        );
        assert_eq!(
            dimension(DimKind::Diameter, &circle, [20.0, 20.0], &views)
                .unwrap()
                .value,
            10.0
        );
    }
}
