//! 投影・交差・スライスで写す物と、写す元の形。

use super::*;

/// 投影の道具で写す物。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectPick {
    /// 押した角・辺・円の中心、面なら面の縁
    Parts,
    /// 押したボディを平面へ写した外形
    Silhouette,
    /// 押したボディの辺と角すべて
    Body,
}

/// 前にできたボディの網(投影の道具で、面を選び、シルエットを求める)。
pub(crate) struct BodyMesh {
    pub feature: kmb_core::FeatureId,
    pub mesh: kmb_core::Mesh,
    /// スケッチの平面
    pub plane: kmb_core::Plane,
    /// 平面へ写した辺と円の中心(同じ位置に写る物もまとめない。面の縁を写す)
    pub edges: Vec<kmb_sketch::RefItem>,
    /// ボディを平面へ写した外形(シルエットを写す時だけ求める)
    pub silhouette: Option<Vec<kmb_sketch::RefItem>>,
}

/// 投影の道具で `w` を押すと写す物(面の縁・シルエット・ボディの辺と角)と、そのボディ。
/// `hit` は押した所にある写した物のボディ(角や辺の上なら)。面・辺・点を写す時に角や辺の
/// 上を押したなら、それ自体を写すので空を返す。
pub(crate) fn project_items(
    pick: ProjectPick,
    refs: &[kmb_sketch::RefItem],
    bodies: &[BodyMesh],
    hit: Option<(kmb_core::FeatureId, bool)>,
    w: [f64; 2],
) -> Vec<kmb_sketch::RefItem> {
    if pick == ProjectPick::Parts && hit.is_some_and(|(_, part)| part) {
        return Vec::new();
    }
    // カーソルの真上に見える面(手前の面)
    let face = bodies
        .iter()
        .filter_map(|b| {
            kmb_sketch::face_under(&b.mesh, &b.plane, w).map(|(name, h)| (b.feature, name, h))
        })
        .max_by(|a, b| a.2.total_cmp(&b.2));
    match pick {
        ProjectPick::Parts => {
            let Some((f, name, _)) = face else {
                return Vec::new();
            };
            // 面の縁の辺。同じ位置に写る辺が既に吸い付く先にあれば、そちらを使う(重ねない)
            let same = |a: &kmb_sketch::RefShape, b: &kmb_sketch::RefShape| {
                use kmb_sketch::RefShape as S;
                let d = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-6;
                match (a, b) {
                    (S::Line(p, q), S::Line(u, v)) => {
                        (d(*p, *u) && d(*q, *v)) || (d(*p, *v) && d(*q, *u))
                    }
                    (
                        S::Circle {
                            center: c,
                            radius: r,
                            arc: x,
                        },
                        S::Circle {
                            center: k,
                            radius: s,
                            arc: y,
                        },
                    ) => {
                        d(*c, *k)
                            && (r - s).abs() < 1e-6
                            && match (x, y) {
                                (None, None) => true,
                                (Some((p, q)), Some((u, v))) => d(*p, *u) && d(*q, *v),
                                _ => false,
                            }
                    }
                    _ => false,
                }
            };
            bodies
                .iter()
                .filter(|b| b.feature == f)
                .flat_map(|b| &b.edges)
                .filter(|r| {
                    matches!(&r.source, kmb_sketch::ProjectionSource::Edge { faces, .. } if faces.contains(&name))
                })
                .map(|r| {
                    refs.iter()
                        .find(|q| q.source.feature() == Some(f) && same(&q.shape, &r.shape))
                        .unwrap_or(r)
                        .clone()
                })
                .collect()
        }
        ProjectPick::Silhouette | ProjectPick::Body => {
            let Some(f) = hit.map(|(f, _)| f).or(face.map(|(f, _, _)| f)) else {
                return Vec::new();
            };
            if pick == ProjectPick::Silhouette {
                bodies
                    .iter()
                    .find(|b| b.feature == f)
                    .and_then(|b| b.silhouette.clone())
                    .unwrap_or_default()
            } else {
                refs.iter()
                    .filter(|r| r.source.feature() == Some(f))
                    .cloned()
                    .collect()
            }
        }
    }
}

/// 交差の道具で `w` を押すと写す断面の線(ボディ全体なら、そのボディの断面すべて)。
pub(crate) fn intersect_items(
    sections: &[Section],
    body: bool,
    w: [f64; 2],
    tol: f64,
) -> Vec<kmb_sketch::RefItem> {
    let Some(sec) = nearest_section(sections, w, tol) else {
        return Vec::new();
    };
    if body {
        return sec.items.clone();
    }
    sec.items
        .iter()
        .map(|r| (r.shape.distance(w), r))
        .filter(|(d, _)| *d <= tol)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, r)| vec![r.clone()])
        .unwrap_or_default()
}

/// 写す物を、仮の要素を足したスケッチ `s` に写す(既に写してある物は使い回す)。写した要素を返す。
pub(crate) fn add_items(
    s: &mut Sketch,
    items: &[kmb_sketch::RefItem],
    construction: bool,
) -> Vec<EntityId> {
    let mut ids = Vec::new();
    for it in items {
        let id = s.add_projection_as(it, construction);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// 写す物のうち、まだスケッチに無い物。投影リンクを外して写した線は元とのつながりを持たない
/// ので、同じ所をもう一度押した時は形で見分ける(見分けないと、リンクを外した交差を
/// 同じ所で押すたびに線が重なる)。リンクがあれば元で見分けるので全部返す。
pub(crate) fn unlinked_fresh(
    unlinked: bool,
    sketch: &Sketch,
    sol: &Solution,
    items: &[kmb_sketch::RefItem],
) -> Vec<kmb_sketch::RefItem> {
    items
        .iter()
        .filter(|it| !unlinked || !drawn_already(sketch, sol, &it.shape))
        .cloned()
        .collect()
}

/// 同じ形の点・線・円・円弧がスケッチにもうあるか。
pub(crate) fn drawn_already(sketch: &Sketch, sol: &Solution, shape: &kmb_sketch::RefShape) -> bool {
    use kmb_sketch::{Entity as E, RefShape as R};
    const EPS: f64 = 1e-6;
    let same = |id: &EntityId, q: [f64; 2]| {
        sol.points
            .get(id)
            .is_some_and(|p| (p[0] - q[0]).hypot(p[1] - q[1]) < EPS)
    };
    sketch.entities.iter().any(|e| match (shape, e) {
        (R::Point(q), E::Point { id, .. }) => same(id, *q),
        (R::Line(p, q), E::Line { a, b, .. }) => {
            (same(a, *p) && same(b, *q)) || (same(a, *q) && same(b, *p))
        }
        (
            R::Circle {
                center,
                radius,
                arc: None,
            },
            E::Circle {
                id,
                center: c,
                radius: r0,
                ..
            },
        ) => same(c, *center) && (sol.radii.get(id).unwrap_or(r0) - radius).abs() < EPS,
        (
            R::Circle {
                center,
                arc: Some((p, q)),
                ..
            },
            E::Arc {
                center: c,
                start,
                end,
                ..
            },
        ) => same(c, *center) && same(start, *p) && same(end, *q),
        _ => false,
    })
}

/// 参照の形を描く。
pub(crate) fn draw_ref(
    painter: &egui::Painter,
    t: &Transform,
    shape: &kmb_sketch::RefShape,
    stroke: Stroke,
) {
    match *shape {
        kmb_sketch::RefShape::Line(a, b) => {
            painter.line_segment([t.to_screen(a), t.to_screen(b)], stroke);
        }
        kmb_sketch::RefShape::Circle {
            center,
            radius,
            arc: None,
        } => {
            t.circle(painter, center, radius, stroke);
        }
        kmb_sketch::RefShape::Circle {
            center,
            arc: Some((s0, e0)),
            ..
        } => {
            let pts = arc_points(center, s0, e0)
                .into_iter()
                .map(|p| t.to_screen(p))
                .collect();
            painter.add(egui::Shape::line(pts, stroke));
        }
        kmb_sketch::RefShape::Point(p) => {
            painter.circle_filled(t.to_screen(p), stroke.width + 1.5, stroke.color);
        }
    }
}

/// スケッチに重ねて見せるもの。
pub(crate) struct Backdrop<'a> {
    /// 左上に出す題
    pub title: &'a str,
    /// 置いた面の輪郭(薄く描く)
    pub underlay: &'a Segments,
    /// 同じ平面に置いた下絵(キャンバス。世界の座標の角。格子の上・スケッチの線の下に描く)
    pub pictures: &'a [kmb_core::OverlayPicture],
    /// 既存の形(原点、見えるボディの角・辺・円の中心)をスケッチの平面へ写したもの。
    /// 薄く描き、吸い付く先と寸法の相手にする
    pub refs: &'a [kmb_sketch::RefItem],
    /// 前にできたボディの断面(スライスか交差の道具の時だけ。ほかは空)
    pub sections: &'a [Section],
    /// 前にできたボディの網(投影の道具の時だけ。ほかは空)
    pub bodies: &'a [BodyMesh],
    /// 面の上のスケッチの原点にできる所(面の角か円の中心。原点を選ぶ道具の時だけ。ほかは空)
    pub origins: &'a [([f64; 2], kmb_sketch::FaceOrigin)],
    /// スケッチの平面(世界の座標。オフセットも入れた物)。3D の視点で使う(分からなければ `None`)
    pub plane: Option<kmb_core::Plane>,
    /// 3D の視点で描く、前にできたボディ(ブラウザの目で隠した物は除く)
    pub scene: &'a kmb_ui::view3d::Scene,
    /// 3D の視点の投影(表示のメニューの平行・透視・自動。3D のビューと同じ)
    pub projection: kmb_ui::Projection,
}

/// `w` から `tol` 以内で一番近い断面。
pub(crate) fn nearest_section(sections: &[Section], w: [f64; 2], tol: f64) -> Option<&Section> {
    let dist = |[a, b]: &[[f64; 2]; 2]| {
        let (ab, aw) = ([b[0] - a[0], b[1] - a[1]], [w[0] - a[0], w[1] - a[1]]);
        let l2 = ab[0] * ab[0] + ab[1] * ab[1];
        let k = if l2 > 0.0 {
            ((aw[0] * ab[0] + aw[1] * ab[1]) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let d = [aw[0] - ab[0] * k, aw[1] - ab[1] * k];
        (d[0] * d[0] + d[1] * d[1]).sqrt()
    };
    sections
        .iter()
        .filter_map(|s| {
            let d = s.segs.iter().map(dist).fold(f64::INFINITY, f64::min);
            (d <= tol).then_some((d, s))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| s)
}
