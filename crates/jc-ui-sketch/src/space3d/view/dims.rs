//! 3D スケッチのビューの拘束の印と寸法(画面へ写した位置・値の札・角度の弧)。

use super::*;

/// 円の折れ線(スケッチの座標)。
pub(super) fn circle_points(c: P3, r: f64, normal: P3) -> Vec<P3> {
    let (u, v) = basis(normal);
    (0..=48)
        .map(|k| {
            let t = std::f64::consts::TAU * k as f64 / 48.0;
            let (s, co) = t.sin_cos();
            geom::add(c, geom::add(geom::scale(u, r * co), geom::scale(v, r * s)))
        })
        .collect()
}

/// 図の上の拘束の印(寸法でない拘束。位置と文字)。
pub(super) struct Label {
    pub(super) id: ConstraintId,
    pub(super) at: Pos2,
    pub(super) text: String,
}

pub(super) fn label_rect(ui: &egui::Ui, l: &Label) -> Rect {
    let g = ui
        .painter()
        .layout_no_wrap(l.text.clone(), jc_ui::theme::prop(11.0), Color32::WHITE);
    Rect::from_center_size(l.at, g.size() + Vec2::new(8.0, 4.0))
}

/// 拘束の印の文字。
pub(super) fn mark_text(kind: &Kind3d) -> &'static str {
    match kind {
        Kind3d::Along { axis: Axis::X, .. } => "X",
        Kind3d::Along { axis: Axis::Y, .. } => "Y",
        Kind3d::Along { axis: Axis::Z, .. } => "Z",
        Kind3d::Parallel { .. } => "∥",
        Kind3d::Perpendicular { .. } => "⊥",
        Kind3d::Equal { .. } => "=",
        Kind3d::Coincident { .. } => "一致",
        Kind3d::Fix { .. } => "固定",
        Kind3d::PointOnLine { .. } => "線上",
        Kind3d::Midpoint { .. } => "中点",
        Kind3d::PointOnPlane { .. } => "面上",
        Kind3d::ParallelToPlane { .. } => "面∥",
        _ => "",
    }
}

/// 線分の画面の上の真ん中と、横へずらす向き。
pub(super) fn line_side(
    sk: &Sketch3d,
    sol: &Solution3d,
    pr: &Proj<'_>,
    l: &EntityId,
) -> Option<(Pos2, Vec2)> {
    let (a, b) = sk.line_ends(*l)?;
    let (a, b) = (sol.points.get(&a)?, sol.points.get(&b)?);
    let (sa, sb) = (pr.at(*a), pr.at(*b));
    let d = (sb - sa).normalized();
    Some((sa + (sb - sa) * 0.5, Vec2::new(-d.y, d.x)))
}

/// 拘束の印の位置(寸法でない拘束)。
pub(super) fn labels(sk: &Sketch3d, sol: &Solution3d, pr: &Proj<'_>) -> Vec<Label> {
    let p = |id: &EntityId| sol.points.get(id).copied();
    let mut count: std::collections::HashMap<EntityId, usize> = Default::default();
    let mut out = Vec::new();
    for c in &sk.constraints {
        let k = &c.kind;
        let text = mark_text(k);
        if k.value().is_some() || text.is_empty() {
            continue;
        }
        // 印は最初の相手の横に、同じ相手の印は並べる
        let first = k.refs().first().copied();
        let Some(first) = first else { continue };
        let n = count.entry(first).or_default();
        let at = match sk.entity(first) {
            Some(Entity3d::Line { .. }) => {
                line_side(sk, sol, pr, &first).map(|(m, nrm)| m + nrm * (14.0 + 15.0 * *n as f32))
            }
            Some(Entity3d::Point { .. }) => {
                p(&first).map(|q| pr.at(q) + Vec2::new(16.0 + 24.0 * *n as f32, -12.0))
            }
            Some(Entity3d::Circle { center, .. }) => {
                p(center).map(|q| pr.at(q) + Vec2::new(16.0 + 24.0 * *n as f32, 12.0))
            }
            None => None,
        };
        *n += 1;
        if let Some(at) = at {
            out.push(Label {
                id: c.id,
                at,
                text: text.into(),
            });
        }
    }
    out
}

/// 寸法の線と値の札(2D の寸法と同じ見た目で描く形。まとめてレビュー R7)。
pub(crate) struct Dim {
    pub id: ConstraintId,
    /// 札の基準の点(スケッチの座標)。札を引いて動かした量はここからのずれで覚える
    pub anchor: P3,
    /// 引いて動かした札の真ん中(画面)。なければ形から離した決まった所
    pub(super) toward: Option<Pos2>,
    pub shape: ScreenDim,
    pub(super) text: String,
    pub(super) reference: bool,
}

/// 寸法の値の字(2D と同じ。数だけの値は測った値、式なら「式 = 値」、半径は R、直径は Ø、
/// 測るだけの寸法は括弧)。
pub(super) fn dim_text(
    f: &jc_sketch::DimFormat,
    kind: &Kind3d,
    value: &str,
    now: f64,
    reference: bool,
) -> String {
    let m = if kind.is_angle() {
        f.angle(now)
    } else {
        f.length(now)
    };
    let plain = crate::view::typed_length(value).is_some() || value.trim().ends_with("deg");
    let body = if reference || plain {
        m
    } else {
        format!("{value} = {m}")
    };
    let body = match kind {
        Kind3d::Radius { .. } => format!("R{body}"),
        Kind3d::Diameter { .. } => format!("Ø{body}"),
        _ => body,
    };
    if reference { format!("({body})") } else { body }
}

/// 寸法の形(画面)。`sk` の寸法のうち、今の形で測れて描けるもの。
pub(crate) fn dims(sk: &Sketch3d, sol: &Solution3d, pr: &Proj<'_>) -> Vec<Dim> {
    let p = |id: &EntityId| sol.points.get(id).copied();
    let mid = |l: &EntityId| {
        let (a, b) = sk.line_ends(*l)?;
        Some((p(&a)?, p(&b)?))
    };
    let mid3 = |l: &EntityId| mid(l).map(|(a, b)| geom::scale(geom::add(a, b), 0.5));
    let half = |a: P3, b: P3| geom::scale(geom::add(a, b), 0.5);
    let plane_center = |r: &PlaneRef| -> Option<P3> {
        match r {
            PlaneRef::Axis { .. } => Some([0.0; 3]),
            PlaneRef::Face { lines } => sol.face(lines).map(|f| f.center),
            PlaneRef::Work { .. } => plane_at(sk, sol, r).map(|(o, _)| o),
        }
    };
    let aligned = |a: P3, b: P3| ScreenDim::Aligned(pr.at(a), pr.at(b));
    let mut out = Vec::new();
    for c in &sk.constraints {
        let k = &c.kind;
        let Some(value) = k.value() else { continue };
        let Some(now) = sol.measured.get(&c.id).copied() else {
            continue;
        };
        // 札の基準の点(形の上)と、引いて動かした札の位置
        let anchor: Option<P3> = match k {
            Kind3d::Length { line, .. } => mid3(line),
            Kind3d::Distance { a, b, .. } => p(a).zip(p(b)).map(|(a, b)| half(a, b)),
            Kind3d::AxisDistance { a, b, axis, .. } => p(a).zip(p(b)).map(|(a, b)| {
                let mut q = a;
                q[axis.index()] = b[axis.index()];
                half(a, q)
            }),
            Kind3d::Angle { a, b, .. } => mid3(a).zip(mid3(b)).map(|(m, n)| half(m, n)),
            Kind3d::PlaneAngle { line, .. } => mid3(line),
            Kind3d::FaceAngle { a, b, .. } => plane_center(a)
                .zip(plane_center(b))
                .map(|(a, b)| half(a, b)),
            Kind3d::PlaneDistance { point, plane, .. } => p(point)
                .zip(foot(sk, sol, point, plane))
                .map(|(a, b)| half(a, b)),
            Kind3d::Radius { circle, .. } | Kind3d::Diameter { circle, .. } => {
                circle_rim(sk, sol, circle)
            }
            _ => None,
        };
        let Some(anchor) = anchor else { continue };
        let toward = c.label.map(|d| pr.at(geom::add(anchor, d)));
        let nudge =
            |l: &EntityId, k: f32| line_side(sk, sol, pr, l).map_or(Vec2::ZERO, |(_, n)| n * k);
        let shape = match k {
            Kind3d::Length { line, .. } => mid(line).map(|(a, b)| aligned(a, b)),
            Kind3d::Distance { a, b, .. } => p(a).zip(p(b)).map(|(a, b)| aligned(a, b)),
            Kind3d::AxisDistance { a, b, axis, .. } => p(a).zip(p(b)).map(|(a, b)| {
                let mut q = a;
                q[axis.index()] = b[axis.index()];
                aligned(a, q)
            }),
            Kind3d::PlaneDistance { point, plane, .. } => p(point)
                .zip(foot(sk, sol, point, plane))
                .map(|(a, b)| aligned(a, b)),
            Kind3d::Radius { circle, .. } | Kind3d::Diameter { circle, .. } => {
                round_shape(sk, sol, pr, circle, matches!(k, Kind3d::Diameter { .. }))
            }
            Kind3d::Angle { a, b, .. } => angle_arc(sk, sol, pr, a, b, now, toward)
                .or_else(|| Some(ScreenDim::Text(pr.at(anchor)))),
            Kind3d::PlaneAngle { line, .. } => {
                Some(ScreenDim::Text(pr.at(anchor) + nudge(line, 22.0)))
            }
            _ => Some(ScreenDim::Text(pr.at(anchor))),
        };
        let Some(shape) = shape else { continue };
        out.push(Dim {
            id: c.id,
            anchor,
            toward,
            shape,
            text: dim_text(&pr.fmt, k, value, now, c.reference),
            reference: c.reference,
        });
    }
    out
}

/// 円の半径・直径の寸法の形(画面へ写した円周)。
pub(super) fn round_shape(
    sk: &Sketch3d,
    sol: &Solution3d,
    pr: &Proj<'_>,
    id: &EntityId,
    diameter: bool,
) -> Option<ScreenDim> {
    let Some(Entity3d::Circle { center, normal, .. }) = sk.entity(*id) else {
        return None;
    };
    let c = sol.points.get(center).copied()?;
    let r = sol.radii.get(id).copied()?;
    let rim = circle_points(c, r, *sol.normals.get(id).unwrap_or(normal))
        .into_iter()
        .map(|q| pr.at(q))
        .collect();
    Some(ScreenDim::Round {
        center: pr.at(c),
        rim,
        diameter,
    })
}

/// 2 本の線分の角度の円弧(2D と同じく、交わる所を中心に値と同じ開きの円弧と矢印)。端を共有するか
/// 交わる線分だけ。円弧の半径は置いた札の所まで広げる。
pub(super) fn angle_arc(
    sk: &Sketch3d,
    sol: &Solution3d,
    pr: &Proj<'_>,
    a: &EntityId,
    b: &EntityId,
    value: f64,
    toward: Option<Pos2>,
) -> Option<ScreenDim> {
    let ends = |l: &EntityId| {
        let (x, y) = sk.line_ends(*l)?;
        Some([sol.points.get(&x).copied()?, sol.points.get(&y).copied()?])
    };
    let (la, lb) = (ends(a)?, ends(b)?);
    let (da, db) = (geom::sub(la[1], la[0]), geom::sub(lb[1], lb[0]));
    // 2 本の直線のいちばん近い点どうし。離れていれば(ねじれ)円弧は描かない
    let (aa, bb, ab) = (geom::dot(da, da), geom::dot(db, db), geom::dot(da, db));
    let den = aa * bb - ab * ab;
    if aa < 1e-12 || bb < 1e-12 || den.abs() < 1e-12 * aa * bb {
        return None;
    }
    let w0 = geom::sub(la[0], lb[0]);
    let (d, e) = (geom::dot(da, w0), geom::dot(db, w0));
    let s = (ab * e - bb * d) / den;
    let t = (aa * e - ab * d) / den;
    let (pa, pb) = (
        geom::add(la[0], geom::scale(da, s)),
        geom::add(lb[0], geom::scale(db, t)),
    );
    let scale = geom::len(da).max(geom::len(db));
    if geom::len(geom::sub(pa, pb)) > 1e-6 * scale.max(1.0) {
        return None;
    }
    let x = pa;
    // 線分のある側へ向ける(交わる所から遠い方の端の向き)
    let away_from = |l: [P3; 2], dir: P3| {
        let k = |q: P3| geom::dot(geom::sub(q, x), dir);
        if k(l[0]).abs() > k(l[1]).abs() {
            if k(l[0]) < 0.0 {
                geom::scale(dir, -1.0)
            } else {
                dir
            }
        } else if k(l[1]) < 0.0 {
            geom::scale(dir, -1.0)
        } else {
            dir
        }
    };
    let e1 = away_from(la, geom::unit(da));
    let mut e2 = away_from(lb, geom::unit(db));
    // 寸法の値の開きの方(内角か、その外の角)
    let open = geom::dot(e1, e2).clamp(-1.0, 1.0).acos();
    if (open - value).abs() > (std::f64::consts::PI - open - value).abs() {
        e2 = geom::scale(e2, -1.0);
    }
    let w = geom::sub(e2, geom::scale(e1, geom::dot(e1, e2)));
    if geom::len(w) < 1e-9 {
        return None;
    }
    let w = geom::unit(w);
    let open = geom::dot(e1, e2).clamp(-1.0, 1.0).acos();
    let xs = pr.at(x);
    // 1 mm の画面の長さ(円弧の面の上で一番長く見える向き。斜めから見た円弧は楕円になるので、
    // 一番広い所が決めた画素になるように)
    let px = (0..8)
        .map(|i| {
            let t = std::f64::consts::PI * i as f64 / 8.0;
            let d = geom::add(geom::scale(e1, t.cos()), geom::scale(w, t.sin()));
            (pr.at(geom::add(x, d)) - xs).length()
        })
        .fold(0.0, f32::max) as f64;
    if px < 1e-3 {
        return None;
    }
    // 円弧の半径(画面で 40 px。置いた札があれば札の少し内側まで)
    let rad_px = toward.map_or(40.0, |c| ((c - xs).length() - 10.0).max(14.0)) as f64;
    let rad = rad_px / px;
    let ray = |t: f64, k: f64| {
        geom::add(
            x,
            geom::scale(
                geom::add(geom::scale(e1, t.cos()), geom::scale(w, t.sin())),
                k,
            ),
        )
    };
    let pts: Vec<Pos2> = (0..=24)
        .map(|i| pr.at(ray(open * i as f64 / 24.0, rad)))
        .collect();
    let at = toward.map_or(12, |c| {
        (1..24)
            .min_by(|i, j| (pts[*i] - c).length().total_cmp(&(pts[*j] - c).length()))
            .unwrap_or(12)
    });
    let out = (pts[at] - xs).normalized();
    // 線分が円弧まで届かない所は補助の線で延ばす
    let (gap, over) = (3.0 / px, 5.0 / px);
    let mut ext = Vec::new();
    for (l, t) in [(la, 0.0), (lb, open)] {
        let dir = ray(t, 1.0);
        let dir = geom::sub(dir, x);
        let k = |q: P3| geom::dot(geom::sub(q, x), dir);
        let (k0, k1) = (k(l[0]).min(k(l[1])), k(l[0]).max(k(l[1])));
        if rad > k1 + gap {
            ext.push((
                pr.at(geom::add(x, geom::scale(dir, k1.max(0.0) + gap))),
                pr.at(geom::add(x, geom::scale(dir, rad + over))),
            ));
        } else if rad < k0 - gap {
            ext.push((
                pr.at(geom::add(x, geom::scale(dir, (rad - over).max(0.0)))),
                pr.at(geom::add(x, geom::scale(dir, k0 - gap))),
            ));
        }
    }
    Some(ScreenDim::Arc { pts, at, out, ext })
}

/// 寸法の値の札の真ん中(画面。試験用。直前のコマで描いた札)。
#[cfg(test)]
pub(crate) fn label_center(ed: &Sketch3dEditor, id: ConstraintId) -> Option<Pos2> {
    ed.dim_rects
        .iter()
        .find(|(c, _)| *c == id)
        .map(|(_, r)| r.center())
}

/// 点から平面へ下ろした足。
pub(super) fn foot(
    sk: &Sketch3d,
    sol: &Solution3d,
    point: &EntityId,
    plane: &PlaneRef,
) -> Option<P3> {
    let q = sol.points.get(point).copied()?;
    let (o, n) = plane_at(sk, sol, plane)?;
    let d = geom::dot(geom::sub(q, o), n);
    Some(geom::sub(q, geom::scale(n, d)))
}

/// 円周の上の点(寸法の値を置く所)。
pub(super) fn circle_rim(sk: &Sketch3d, sol: &Solution3d, id: &EntityId) -> Option<P3> {
    let Some(Entity3d::Circle { center, normal, .. }) = sk.entity(*id) else {
        return None;
    };
    let c = sol.points.get(center).copied()?;
    let r = sol.radii.get(id).copied()?;
    let (u, v) = basis(*sol.normals.get(id).unwrap_or(normal));
    Some(geom::add(c, geom::scale(geom::unit(geom::add(u, v)), r)))
}
