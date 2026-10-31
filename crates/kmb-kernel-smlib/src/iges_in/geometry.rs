//! IGES の曲線・曲面を、面を組む共通の内部表現へ直す。
use super::*;
use crate::step_in::{self, Frame, Nurbs, NurbsSurf, Surf};
use std::f64::consts::TAU;
use std::sync::Arc;

const MAX_CONTROL: usize = 262_144;
const MAX_DEGREE: usize = 16;

pub(super) struct Surface {
    pub geo: Surf,
    pub place: k::Matrix4,
    pub domain: Option<[f64; 4]>,
    pub boundary: u32,
    pub parameterized: bool,
}
pub(super) fn checked(p: P3) -> Result<P3, String> {
    if p.iter().any(|v| !v.is_finite() || v.abs() > 1e30) {
        Err("座標が計算できる範囲を超えています。単位と変換を確かめてください".into())
    } else {
        Ok(p)
    }
}
fn unit(v: P3) -> Result<P3, String> {
    let length = v[0].hypot(v[1]).hypot(v[2]);
    if length == 0. || !length.is_finite() {
        Err("向きを示すベクトルが0か大きすぎます".into())
    } else {
        Ok(k::scale(v, 1. / length))
    }
}
fn frame(o: P3, z: P3, x: Option<P3>) -> Result<Frame, String> {
    let z = unit(z)?;
    let x = x.unwrap_or_else(|| {
        let index = (0..3)
            .min_by(|a, b| z[*a].abs().total_cmp(&z[*b].abs()))
            .unwrap_or(0);
        let mut axis = [0.; 3];
        axis[index] = 1.;
        axis
    });
    let x = unit(k::sub(x, k::scale(z, k::dot(x, z))))?;
    Ok(Frame {
        o,
        x,
        y: k::cross(z, x),
        z,
    })
}
fn vectors(p: &mut Params<'_>, count: usize) -> Result<Vec<f64>, String> {
    (0..count).map(|_| p.number()).collect()
}
fn knots(values: &[f64], degree: usize, count: usize) -> Result<(), String> {
    if degree == 0
        || degree > MAX_DEGREE
        || count <= degree
        || values.len() != count + degree + 1
        || values.windows(2).any(|p| p[0] > p[1])
        || values[count] - values[degree] <= 1e-12
    {
        return Err("B スプラインの次数・制御点・ノットの範囲が不正です".into());
    }
    Ok(())
}
fn weights(values: &mut [f64]) -> Result<(), String> {
    let max = values.iter().copied().fold(0., f64::max);
    if max <= 0. || values.iter().any(|w| *w <= 0.) {
        return Err("B スプラインの重みは正の値にしてください".into());
    }
    for w in values {
        *w /= max;
        if *w == 0. {
            return Err("B スプラインの重みの比が大きすぎます".into());
        }
    }
    Ok(())
}
pub(super) fn line(a: P3, b: P3) -> Result<Nurbs, String> {
    if k::sub(a, b).iter().all(|d| d.abs() < 1e-12) {
        return Err("長さ0の直線は縁に使えません".into());
    }
    Ok(Nurbs {
        degree: 1,
        knots: vec![0., 0., 1., 1.],
        pts: vec![[a[0], a[1], a[2], 1.], [b[0], b[1], b[2], 1.]],
    })
}
pub(super) fn moved(c: &Nurbs, m: &k::Matrix4) -> Result<Nurbs, String> {
    let mut c = c.clone();
    for p in &mut c.pts {
        let q = checked(k::transform_point(m, [p[0], p[1], p[2]]))?;
        p[..3].copy_from_slice(&q);
    }
    Ok(c)
}
/// 切り出す前にノットの尺度をそろえる。ノット差の絶対値で切り出し位置を取り違えない。
pub(super) fn portion(c: &Nurbs, from: f64, to: f64) -> Result<Nurbs, String> {
    let lo = c.knots[c.degree];
    let hi = c.knots[c.pts.len()];
    let width = hi - lo;
    if !width.is_finite()
        || width <= 0.
        || from < lo - 1e-10 * width
        || to > hi + 1e-10 * width
        || to - from <= width * 1e-10
    {
        return Err("曲線の始まりと終わりがノットの範囲に合いません".into());
    }
    let mut normalized = c.clone();
    for t in &mut normalized.knots {
        *t = (*t - lo) / width;
    }
    let a = ((from - lo) / width).clamp(0., 1.);
    let b = ((to - lo) / width).clamp(0., 1.);
    Ok(step_in::sub(&normalized, a, b))
}
fn rectangle(a: f64, b: f64, c: f64, d: f64) -> Result<Vec<Nurbs>, String> {
    let points = [[a, c, 0.], [b, c, 0.], [b, d, 0.], [a, d, 0.]];
    (0..4)
        .map(|i| line(points[i], points[(i + 1) % 4]))
        .collect()
}

impl Data {
    pub(super) fn point(&self, id: u32) -> Result<P3, String> {
        self.with(id, |e| {
            if e.kind != 116 {
                return Err("点の要素116を参照してください".into());
            }
            let mut p = Params::new(e);
            let at = checked(k::scale(p.point()?, self.file.global.mm))?;
            checked(k::transform_point(&self.matrix(e.transform, false)?, at))
        })
    }
    fn direction(&self, id: u32) -> Result<P3, String> {
        self.with(id, |e| {
            if e.kind != 123 {
                return Err("方向の要素123を参照してください".into());
            }
            let mut p = Params::new(e);
            let at = p.point()?;
            let mut m = self.matrix(e.transform, false)?;
            for row in &mut m[..3] {
                row[3] = 0.;
            }
            unit(k::transform_point(&m, at))
        })
    }
    pub(super) fn curve(&self, id: u32, parameter: bool) -> Result<Vec<Nurbs>, String> {
        self.with(id, |e| {
            let mut p = Params::new(e);
            let scale = if parameter { 1. } else { self.file.global.mm };
            let curves = match e.kind {
                110 => {
                    if e.form != 0 {
                        return Err("無限の直線は、頂点で区切った辺として指定してください".into());
                    }
                    let a = checked(k::scale(p.point()?, scale))?;
                    let b = checked(k::scale(p.point()?, scale))?;
                    vec![line(a, b)?]
                }
                100 => {
                    let z = p.number()? * scale;
                    let center = [p.number()? * scale, p.number()? * scale, z];
                    let start = [p.number()? * scale, p.number()? * scale, z];
                    let end = [p.number()? * scale, p.number()? * scale, z];
                    checked(center)?;
                    checked(start)?;
                    checked(end)?;
                    let a = k::sub(start, center);
                    let b = k::sub(end, center);
                    let r = a[0].hypot(a[1]);
                    if r <= 1e-12
                        || (b[0].hypot(b[1]) - r).abs()
                            > (if parameter {
                                1e-9_f64.max(r * 1e-7)
                            } else {
                                self.file.global.tolerance_mm.max(r * 1e-7)
                            })
                    {
                        return Err("円弧の始点と終点の半径が合いません".into());
                    }
                    let begin = a[1].atan2(a[0]);
                    let mut sweep = (b[1].atan2(b[0]) - begin).rem_euclid(TAU);
                    if sweep < 1e-12 {
                        sweep = TAU;
                    }
                    let f = frame(center, [0., 0., 1.], Some([1., 0., 0.]))?;
                    vec![step_in::arc(&f, r, r, begin, sweep)]
                }
                126 => {
                    let count = p
                        .count(MAX_CONTROL - 1)?
                        .checked_add(1)
                        .ok_or("制御点が多すぎます")?;
                    let degree = p.count(MAX_DEGREE)?;
                    for _ in 0..4 {
                        p.flag()?;
                    }
                    let values = vectors(&mut p, count + degree + 1)?;
                    knots(&values, degree, count)?;
                    let mut w = vectors(&mut p, count)?;
                    weights(&mut w)?;
                    let mut pts = Vec::with_capacity(count);
                    for weight in w {
                        let at = checked(k::scale(p.point()?, scale))?;
                        pts.push([at[0], at[1], at[2], weight]);
                    }
                    let from = p.number()?;
                    let to = p.number()?;
                    vec![portion(
                        &Nurbs {
                            degree,
                            knots: values,
                            pts,
                        },
                        from,
                        to,
                    )?]
                }
                102 => {
                    let n = p.count(100_000)?;
                    let mut out = Vec::new();
                    for _ in 0..n {
                        out.extend(self.curve(p.integer()?, parameter)?);
                        if out.len() > 100_000 {
                            return Err("複合の曲線が多すぎます".into());
                        }
                    }
                    out
                }
                kind => return Err(format!("曲線の種類 {kind} には対応していません")),
            };
            self.charge(curves.iter().map(|c| c.pts.len()).sum())?;
            let m = self.matrix(e.transform, parameter)?;
            curves.iter().map(|c| moved(c, &m)).collect()
        })
    }
    pub(super) fn surface(&self, id: u32) -> Result<Arc<Surface>, String> {
        if let Some(found) = self.surfaces.borrow().get(&id).cloned() {
            return Ok(found);
        }
        let surface=self.with(id,|e|{
            let mut p=Params::new(e);let mm=self.file.global.mm;
            let mut domain=None;let mut boundary=0;
            let geo=match e.kind {
                108=>{
                    let n=p.point()?;let distance=p.number()?*mm;boundary=p.integer()?;
                    if e.form == -1 { return Err("平面の外側を残す形式は無限に広がるため読めません。境のある面で保存してください".into()); }
                    if !matches!(e.form,0|1) || (e.form==0 && boundary!=0) || (e.form==1 && boundary==0) { return Err("平面の形式と境の指定が合いません".into()); }
                    let length=n[0].hypot(n[1]).hypot(n[2]);let z=unit(n)?;
                    Surf::Plane(frame(checked(k::scale(z,distance/length))?,z,None)?)
                }
                128=>{
                    let nu=p.count(MAX_CONTROL-1)?+1;let nv=p.count(MAX_CONTROL-1)?+1;
                    let size=nu.checked_mul(nv).filter(|n|*n<=MAX_CONTROL).ok_or("曲面の制御点が多すぎます")?;
                    let du=p.count(MAX_DEGREE)?;let dv=p.count(MAX_DEGREE)?;for _ in 0..5 {p.flag()?;}
                    let ku=vectors(&mut p,nu+du+1)?;let kv=vectors(&mut p,nv+dv+1)?;
                    knots(&ku,du,nu)?;knots(&kv,dv,nv)?;
                    let mut w=vectors(&mut p,size)?;weights(&mut w)?;
                    let mut points=vec![[0.;4];size];
                    for j in 0..nv {for i in 0..nu {let at=checked(k::scale(p.point()?,mm))?;points[i*nv+j]=[at[0],at[1],at[2],w[j*nu+i]];}}
                    let u0=p.number()?;let u1=p.number()?;let v0=p.number()?;let v1=p.number()?;
                    if u0<ku[du] || u1>ku[nu] || v0<kv[dv] || v1>kv[nv] || u1-u0<=1e-12 || v1-v0<=1e-12 {return Err("曲面の範囲がノットと合いません".into());}
                    domain=Some([u0,u1,v0,v1]);
                    Surf::BSpline(NurbsSurf{degree_u:du,degree_v:dv,knots_u:ku,knots_v:kv,pts:points})
                }
                190|192|194|196|198=>{
                    if !matches!(e.form,0|1) {return Err("解析曲面の形式は0か1にしてください".into());}
                    let origin=self.point(p.integer()?)?;
                    let (z,radius)=if e.kind==196 {let r=p.number()?*mm;let z=if e.form==1 {self.direction(p.integer()?)?}else{[0.,0.,1.]};(z,r)}else{
                        let z=self.direction(p.integer()?)?;let r=if e.kind==190{0.}else{p.number()?*mm};(z,r)
                    };
                    let second=if e.kind==194 {p.number()?.to_radians()}else if e.kind==198 {p.number()?*mm}else{0.};
                    let x=if e.form==1 {Some(self.direction(p.integer()?)?)}else{None};
                    let f=frame(origin,z,x)?;
                    if e.kind!=190 && (!radius.is_finite() || radius<0. || (radius==0. && e.kind!=194)) {return Err("曲面の半径が不正です".into());}
                    match e.kind {
                        190=>Surf::Plane(f),192=>Surf::Cylinder(f,radius),
                        194=>{if !(0. ..std::f64::consts::FRAC_PI_2).contains(&second) || second==0. {return Err("円すいの半頂角は0度より大きく90度より小さくしてください".into());}Surf::Cone(f,radius,second)},
                        196=>Surf::Sphere(f,radius),
                        _=>{if second<=0. || second>=radius {return Err("円環の小さい半径は0より大きく、大きい半径より小さくしてください".into());}Surf::Torus(f,radius,second)},
                    }
                }
                kind=>return Err(format!("曲面の種類 {kind} には対応していません")),
            };
            Ok(Arc::new(Surface{geo,place:self.matrix(e.transform,false)?,domain,boundary,parameterized:e.kind==128 || (e.kind!=108 && e.form==1)}))
        })?;
        self.surfaces.borrow_mut().insert(id, surface.clone());
        Ok(surface)
    }
}

/// 有理曲面のパラメータを1つ固定して、曲線の制御点と重みを求める。
fn iso(s: &NurbsSurf, u_running: bool, fixed: f64, from: f64, to: f64) -> Result<Nurbs, String> {
    let nu = s.knots_u.len() - s.degree_u - 1;
    let nv = s.knots_v.len() - s.degree_v - 1;
    let (knots, degree, count) = if u_running {
        (&s.knots_v, s.degree_v, nv)
    } else {
        (&s.knots_u, s.degree_u, nu)
    };
    let span = crate::skin::find_span(knots, degree, count, fixed);
    let values = crate::skin::basis(knots, degree, span, fixed);
    let n = if u_running { nu } else { nv };
    let mut points = Vec::with_capacity(n);
    for i in 0..n {
        let mut h = [0.; 4];
        for (j, value) in values.iter().enumerate() {
            let k = span - degree + j;
            let p = if u_running {
                s.pts[i * nv + k]
            } else {
                s.pts[k * nv + i]
            };
            for axis in 0..3 {
                h[axis] += value * p[axis] * p[3];
            }
            h[3] += value * p[3];
        }
        if h[3] <= 0. || !h[3].is_finite() {
            return Err("曲面の縁の重みを求められません".into());
        }
        let at = checked([h[0] / h[3], h[1] / h[3], h[2] / h[3]])?;
        points.push([at[0], at[1], at[2], h[3]]);
    }
    let c = Nurbs {
        degree: if u_running { s.degree_u } else { s.degree_v },
        knots: if u_running {
            s.knots_u.clone()
        } else {
            s.knots_v.clone()
        },
        pts: points,
    };
    portion(&c, from, to)
}
impl Surface {
    pub(super) fn natural(&self, uv: bool) -> Result<Vec<Nurbs>, String> {
        let Some([u0, u1, v0, v1]) = self.domain else {
            return Err(
                "無限の曲面には境の線が必要です。面の境を付けて保存し直してください".into(),
            );
        };
        if uv {
            return rectangle(u0, u1, v0, v1);
        }
        let Surf::BSpline(s) = &self.geo else {
            return Err("曲面のパラメータの境を読めません".into());
        };
        let lines = vec![
            iso(s, true, v0, u0, u1)?,
            iso(s, false, u1, v0, v1)?,
            step_in::reverse(&iso(s, true, v1, u0, u1)?),
            step_in::reverse(&iso(s, false, u0, v0, v1)?),
        ];
        lines.iter().map(|c| moved(c, &self.place)).collect()
    }
    pub(super) fn lift_plane(&self, c: &Nurbs, mm: f64) -> Result<Nurbs, String> {
        let Surf::Plane(f) = &self.geo else {
            return Err("この曲面のパラメータの境だけの読み込みには対応していません。3D の境を付けて保存してください".into());
        };
        let mut c = c.clone();
        for p in &mut c.pts {
            if p[2].abs() > 1e-9 {
                return Err("面のパラメータの線は2次元で指定してください".into());
            }
            let at = checked(k::transform_point(
                &self.place,
                f.at(p[0] * mm, p[1] * mm, 0.),
            ))?;
            p[..3].copy_from_slice(&at);
        }
        Ok(c)
    }
}
