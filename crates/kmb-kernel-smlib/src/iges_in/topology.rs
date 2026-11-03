//! 位相の頂点・辺・輪・殻を、保存された参照で読む。
use super::*;
use crate::step_in::{self, Nurbs};

impl Data {
    fn vertex(&self, id: u32, index: u32) -> Result<P3, String> {
        let cached = self.vertices.borrow().get(&id).cloned();
        let list = if let Some(found) = cached {
            found
        } else {
            let points = self.with(id, |e| {
                if e.kind != 502 {
                    return Err("頂点リスト502を参照してください".into());
                }
                let mut p = Params::new(e);
                let count = p.count(100_000)?;
                let matrix = self.matrix(e.transform, false)?;
                let mut points = Vec::with_capacity(count);
                for _ in 0..count {
                    let at = checked(k::scale(p.point()?, self.file.global.mm))?;
                    points.push(checked(k::transform_point(&matrix, at))?);
                }
                Ok(Arc::new(points))
            })?;
            self.vertices.borrow_mut().insert(id, points.clone());
            points
        };
        index
            .checked_sub(1)
            .and_then(|n| list.get(n as usize))
            .copied()
            .ok_or_else(|| format!("頂点リスト {id} に頂点 {index} がありません"))
    }
    fn edge(&self, id: u32, index: u32) -> Result<Vec<Nurbs>, String> {
        self.with(id, |e| {
            if e.kind != 504 {
                return Err("辺リスト504を参照してください".into());
            }
            let mut p = Params::new(e);
            let count = p.count(100_000)?;
            if index == 0 || index as usize > count {
                return Err(format!("辺リストに辺 {index} がありません"));
            }
            let mut selected = None;
            for i in 1..=count {
                let tuple = [
                    p.integer()?,
                    p.integer()?,
                    p.integer()?,
                    p.integer()?,
                    p.integer()?,
                ];
                if i == index as usize {
                    selected = Some(tuple);
                }
            }
            let [curve, starts, start, ends, end] = selected.ok_or("辺の定義がありません")?;
            let a = self.vertex(starts, start)?;
            let b = self.vertex(ends, end)?;
            let definition = self
                .file
                .entries
                .get(&curve)
                .ok_or_else(|| format!("辺の曲線 {curve} がありません"))?;
            let parts = if definition.kind == 110 {
                self.with(curve, |e| {
                    let mut p = Params::new(e);
                    let matrix = self.matrix(e.transform, false)?;
                    let x = checked(k::transform_point(
                        &matrix,
                        k::scale(p.point()?, self.file.global.mm),
                    ))?;
                    let y = checked(k::transform_point(
                        &matrix,
                        k::scale(p.point()?, self.file.global.mm),
                    ))?;
                    let d = k::sub(y, x);
                    let l = k::dot(d, d);
                    if !l.is_finite() || l <= 1e-24 {
                        return Err("辺の直線の向きが不正です".into());
                    }
                    let position = |q: P3| {
                        let t = k::dot(k::sub(q, x), d) / l;
                        let off = k::sub(q, k::add(x, k::scale(d, t)));
                        (t, off[0].hypot(off[1]).hypot(off[2]))
                    };
                    let (ta, da) = position(a);
                    let (tb, db) = position(b);
                    let tolerance = self.file.global.tolerance_mm.max(1e-7);
                    if da > tolerance
                        || db > tolerance
                        || ta >= tb
                        || (e.form == 0 && (ta < -1e-8 || tb > 1. + 1e-8))
                        || (e.form == 1 && ta < -1e-8)
                        || !matches!(e.form, 0..=2)
                    {
                        return Err("辺の頂点と直線の範囲・向きが合いません".into());
                    }
                    Ok(vec![geometry::line(a, b)?])
                })?
            } else {
                let curves = self.curve(curve, false)?;
                trim_chain(&curves, a, b, self.file.global.tolerance_mm.max(1e-7))?
            };
            let matrix = self.matrix(e.transform, false)?;
            parts.iter().map(|c| moved(c, &matrix)).collect()
        })
    }
    pub(super) fn topology_loop(&self, id: u32) -> Result<Boundary, String> {
        self.with(id, |e| {
            if e.kind != 508 {
                return Err("輪の要素508を参照してください".into());
            }
            let mut p = Params::new(e);
            let count = p.count(100_000)?;
            let mut curves = Vec::new();
            let mut points = Vec::new();
            for _ in 0..count {
                let kind = p.integer()?;
                let source = p.integer()?;
                let index = p.integer()?;
                let agrees = p.flag()?;
                let parameter_count = p.count(100_000)?;
                for _ in 0..parameter_count {
                    p.flag()?;
                    self.mark_curve(p.integer()?, 0);
                }
                match kind {
                    0 => {
                        let mut edge = self.edge(source, index)?;
                        if !agrees {
                            edge.reverse();
                            edge = edge.iter().map(step_in::reverse).collect();
                        }
                        curves.extend(edge);
                    }
                    1 => points.push(self.vertex(source, index)?),
                    _ => return Err("輪の辺の種類は辺か頂点で指定してください".into()),
                }
            }
            if !points.is_empty() && (!curves.is_empty() || points.len() != 1) {
                return Err("頂点だけの輪は1点で指定してください".into());
            }
            let matrix = self.matrix(e.transform, false)?;
            Ok(Boundary {
                curves: curves
                    .iter()
                    .map(|c| moved(c, &matrix))
                    .collect::<Result<_, _>>()?,
                points: points
                    .into_iter()
                    .map(|p| checked(k::transform_point(&matrix, p)))
                    .collect::<Result<_, _>>()?,
                uv: false,
            })
        })
    }
    pub(super) fn shell(
        &self,
        id: u32,
        same: bool,
        parent: k::Matrix4,
        numbers: &BTreeMap<u32, usize>,
        model: &mut Model,
    ) -> Result<(), String> {
        self.with(id, |e| {
            if e.kind != 514 {
                return Err("殻の要素514を参照してください".into());
            }
            let mut p = Params::new(e);
            let count = p.count(100_000)?;
            let number = numbers.get(&id).ok_or("殻の番号がありません")?;
            let place = k::mat_mul(&parent, &self.matrix(e.transform, false)?);
            for ordinal in 1..=count {
                let face = p.integer()?;
                let orientation = p.flag()?;
                let name = format!("iges{number}:f{ordinal}");
                match self.face(face, same == orientation, place, name.clone()) {
                    Ok(face) => model.faces.push(face),
                    Err(why) => {
                        model.missing_faces += 1;
                        model
                            .warnings
                            .push(format!("面 {name} を読めませんでした: {why}"));
                    }
                }
            }
            Ok(())
        })
    }
}
fn distance(a: P3, b: P3) -> f64 {
    let d = k::sub(a, b);
    d[0].hypot(d[1]).hypot(d[2])
}
/// 位相の頂点は、曲線の増える向きにたどる。閉じた曲線の継ぎ目だけは2区間に分ける。
fn trim_chain(curves: &[Nurbs], a: P3, b: P3, tolerance: f64) -> Result<Vec<Nurbs>, String> {
    if curves.is_empty() {
        return Err("辺の曲線がありません".into());
    }
    let endpoint = |c: &Nurbs, last: bool| {
        step_in::eval(
            c,
            if last {
                c.knots[c.pts.len()]
            } else {
                c.knots[c.degree]
            },
        )
    };
    for pair in curves.windows(2) {
        if distance(endpoint(&pair[0], true), endpoint(&pair[1], false)) > tolerance {
            return Err("複合曲線の途中に隙間があります".into());
        }
    }
    let locate = |point: P3| -> Result<(usize, f64), String> {
        let mut found = Vec::new();
        for (i, c) in curves.iter().enumerate() {
            let t = step_in::param_of(c, point);
            if distance(step_in::eval(c, t), point) <= tolerance {
                found.push((i, t));
            }
        }
        found
            .first()
            .copied()
            .ok_or_else(|| format!("頂点 {point:?} が辺の曲線上にありません"))
    };
    let (ia, ta) = locate(a)?;
    let (ib, tb) = locate(b)?;
    let closed = distance(
        endpoint(&curves[0], false),
        endpoint(curves.last().ok_or("辺の曲線がありません")?, true),
    ) <= tolerance;
    let wrap = ib < ia || (ia == ib && tb <= ta + 1e-10);
    if wrap && !closed {
        return Err("辺の頂点の順が曲線の向きと合いません".into());
    }
    let mut out = Vec::new();
    let mut i = ia;
    let mut start = ta;
    for step in 0..=curves.len() {
        let c = &curves[i];
        let final_part = i == ib && (!wrap || step > 0);
        let finish = if final_part { tb } else { c.knots[c.pts.len()] };
        if finish - start > 1e-10 * (c.knots[c.pts.len()] - c.knots[c.degree]) {
            out.push(geometry::portion(c, start, finish)?);
        }
        if final_part {
            break;
        }
        i = (i + 1) % curves.len();
        start = curves[i].knots[curves[i].degree];
    }
    if out.is_empty() {
        return Err("辺を区切る範囲がありません".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_closed_edge_crossing_the_parameter_seam_keeps_both_parts() {
        let frame = step_in::Frame {
            o: [0.; 3],
            x: [1., 0., 0.],
            y: [0., 1., 0.],
            z: [0., 0., 1.],
        };
        let c = step_in::arc(&frame, 5., 5., 0., std::f64::consts::TAU);
        let a = step_in::eval(&c, 0.75);
        let b = step_in::eval(&c, 0.25);
        let parts = trim_chain(&[c], a, b, 1e-6).unwrap();
        assert_eq!(parts.len(), 2);
        assert!(distance(step_in::eval(&parts[0], parts[0].knots[parts[0].degree]), a) < 1e-6);
        assert!(
            distance(
                step_in::eval(&parts[1], parts[1].knots[parts[1].pts.len()]),
                b
            ) < 1e-6
        );
    }
}
