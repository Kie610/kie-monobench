//! 結果の見せ方。[`SimResult`] だけを読み、3D のビューに重ねる色分けの形([`OverlayShape`])を作る。
//!
//! - 表面の色: 表面の網の量を、点の値から三角形の中で補間して塗る。色の帯の境(等値線)で三角形を
//!   切り分け、切った小片を 1 色で塗るので、描く側は面ごとの色のままで、境のまっすぐな帯になる。
//!   帯を細かく(既定 40 段)すると滑らかな色に見える。
//! - 断面: 格子の量を、平面の上の点で格子の 8 つ(2D は 4 つ)の流体のセルから補間して塗る。
//!   物体の中は塗らない。物体との境は、物体の表面の網(本当の形)を平面で切った線で切る。
//!   物体は隠さない(断面の穴から物体が通って見える)。
//! - 流線: 格子の速度を上流の点から追い、細い管で描いて速さで塗る。
//!
//! 色の範囲は既定で全部のコマで決める(時刻を送っても色の意味が変わらない)。

use crate::result::{Derive, Geometry, Grid, Role, SimResult, Tets, Values, fmt, nice};
use kmb_core::{Mesh, OverlayShape, ShapeInk};

mod section;
mod stream;
#[cfg(test)]
mod tests;
mod tets;

pub use section::sample;
use section::section_mesh;
pub use stream::{Streamline, streamlines};
use stream::{segment, tube};
use tets::{Src, deform_factor, tets_mesh};

/// 帯で塗る時の帯の数。
pub const BANDS: usize = 10;
/// 滑らかに塗る時の段の数。
pub const SMOOTH: usize = 40;
/// 流線の色の帯の数。
const STREAM_BANDS: usize = 10;

/// 値の無い所・色を塗らない物体の灰色。
pub const NO_COLOR: [u8; 3] = [150, 152, 158];
/// 印(特異点の候補など)の色。色の段(青〜赤)に無い色
pub const MARK_COLOR: [u8; 3] = [214, 0, 190];

/// 見せ方の選び。
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// 今のコマ
    pub frame: usize,
    /// 表面を塗る量(`None` なら表面は灰色)
    pub surface: Option<usize>,
    /// 断面を塗る量と、平面(世界の軸と位置 mm)
    pub section: Option<(usize, usize, f64)>,
    /// 流線を出すか(速度の量があれば)
    pub streamlines: bool,
    /// 滑らかに塗る(偽なら帯)
    pub smooth: bool,
    /// 変形を出す(変位の量があれば)
    pub deform: bool,
    /// 色の範囲を全部のコマで決める(偽なら今のコマだけ)
    pub legend_all: bool,
    /// 結果が古い(見出しに書く)
    pub stale: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            frame: 0,
            surface: None,
            section: None,
            streamlines: false,
            smooth: true,
            deform: true,
            legend_all: true,
            stale: false,
        }
    }
}

/// 低い値(青)から高い値(赤)への色。`t` は 0〜1。
pub fn ramp(t: f32) -> [u8; 3] {
    const STOPS: [[f32; 3]; 6] = [
        [40.0, 70.0, 200.0],
        [30.0, 150.0, 230.0],
        [40.0, 190.0, 160.0],
        [150.0, 210.0, 60.0],
        [245.0, 190.0, 40.0],
        [220.0, 50.0, 40.0],
    ];
    let t = t.clamp(0.0, 1.0) * (STOPS.len() - 1) as f32;
    let a = (t.floor() as usize).min(STOPS.len() - 2);
    let f = t - a as f32;
    let c = |k: usize| (STOPS[a][k] * (1.0 - f) + STOPS[a + 1][k] * f).round() as u8;
    [c(0), c(1), c(2)]
}

/// 値から色の段への写し。
#[derive(Debug, Clone, Copy)]
struct Scale {
    lo: f32,
    span: f32,
    levels: usize,
    reverse: bool,
    cap: Option<f32>,
    /// この色の塊の最初の面の番号
    base: u32,
}

impl Scale {
    /// 段の座標(0〜levels)。
    fn t(&self, v: f32) -> f64 {
        let v = self.cap.map_or(v, |c| v.min(c));
        (((v - self.lo) / self.span).clamp(0.0, 1.0) * self.levels as f32) as f64
    }

    fn face(&self, band: usize) -> u32 {
        self.base + band.min(self.levels - 1) as u32
    }

    fn color(&self, band: usize) -> [u8; 3] {
        let t = band as f32 / (self.levels - 1).max(1) as f32;
        ramp(if self.reverse { 1.0 - t } else { t })
    }

    /// 凡例(上が高い値)。帯は範囲、滑らかは値の目盛り。
    fn legend(&self, unit: &str, banded: bool, top_open: bool) -> Vec<(ShapeInk, String)> {
        let with_unit = |s: String| {
            if unit.is_empty() {
                s
            } else {
                format!("{s} {unit}")
            }
        };
        let mut out = Vec::new();
        if banded {
            for b in (0..self.levels).rev() {
                let a = self.lo + self.span * b as f32 / self.levels as f32;
                let z = self.lo + self.span * (b + 1) as f32 / self.levels as f32;
                let text = if b == self.levels - 1 && top_open {
                    format!("{} 以上", fmt(a as f64))
                } else {
                    format!("{}〜{}", fmt(a as f64), fmt(z as f64))
                };
                out.push((ShapeInk::Rgb(self.color(b)), with_unit(text)));
            }
        } else {
            const TICKS: usize = 9;
            for k in (0..TICKS).rev() {
                let t = k as f32 / (TICKS - 1) as f32;
                let v = self.lo + self.span * t;
                let band = ((t * self.levels as f32) as usize).min(self.levels - 1);
                let text = if k == TICKS - 1 && top_open {
                    format!("{} 以上", fmt(v as f64))
                } else {
                    fmt(v as f64)
                };
                out.push((ShapeInk::Rgb(self.color(band)), with_unit(text)));
            }
        }
        out
    }
}

/// 網を組み立てる。
#[derive(Default)]
struct Builder {
    mesh: Mesh,
}

/// 三角形の向きの単位法線(面積が無ければ None)。
fn facet(p: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
    let w = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
    unit(cross3(u, w))
}

fn cross3(u: [f64; 3], w: [f64; 3]) -> [f64; 3] {
    [
        u[1] * w[2] - u[2] * w[1],
        u[2] * w[0] - u[0] * w[2],
        u[0] * w[1] - u[1] * w[0],
    ]
}

/// 単位の長さにした物(長さが無い・NaN なら None)。
fn unit(n: [f64; 3]) -> Option<[f64; 3]> {
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    (l.is_finite() && l > 1e-18).then(|| n.map(|x| x / l))
}

impl Builder {
    /// 平らに塗る三角形(3 つの点の法線は三角形の向き)。
    fn tri(&mut self, p: [[f64; 3]; 3], face: u32) {
        self.tri_n(p, [[0.0; 3]; 3], face);
    }

    /// 点ごとの陰の法線 `n` で塗る三角形。長さの無い法線の点は三角形の向きにする。
    fn tri_n(&mut self, p: [[f64; 3]; 3], n: [[f64; 3]; 3], face: u32) {
        let Some(f) = facet(p) else {
            return;
        };
        let base = self.mesh.positions.len() as u32;
        for (q, n) in p.iter().zip(n) {
            let n = unit(n).unwrap_or(f);
            self.mesh.positions.push(q.map(|x| x as f32));
            self.mesh.normals.push(n.map(|x| x as f32));
        }
        self.mesh.triangles.push([base, base + 1, base + 2]);
        self.mesh.triangle_faces.push(face);
    }

    /// 凸な多角形を扇に分けて足す(点と陰の法線)。
    fn polygon(&mut self, p: &[([f64; 3], [f64; 3])], face: u32) {
        for k in 1..p.len().saturating_sub(1) {
            self.tri_n(
                [p[0].0, p[k].0, p[k + 1].0],
                [p[0].1, p[k].1, p[k + 1].1],
                face,
            );
        }
    }

    /// 三角形を色の段の境で切り分けて、平らに塗って足す。`t` は頂点の段の座標。
    fn banded(&mut self, p: [[f64; 3]; 3], t: [f64; 3], s: &Scale) {
        self.banded_n(p, [[0.0; 3]; 3], t, s);
    }

    /// 三角形を色の段の境で切り分けて足す。`n` は頂点の陰の法線(切り口の点では補間する)。
    fn banded_n(&mut self, p: [[f64; 3]; 3], n: [[f64; 3]; 3], t: [f64; 3], s: &Scale) {
        let band = |x: f64| (x.floor().max(0.0) as usize).min(s.levels - 1);
        let (b0, b1) = (
            band(t[0].min(t[1]).min(t[2])),
            band(t[0].max(t[1]).max(t[2])),
        );
        if b0 == b1 {
            self.tri_n(p, n, s.face(b0));
            return;
        }
        let verts: Vec<ClipV> = (0..3).map(|i| (p[i], n[i], t[i])).collect();
        for b in b0..=b1 {
            let mut poly = verts.clone();
            if b > b0 {
                poly = clip(&poly, b as f64, true);
            }
            if b < b1 {
                poly = clip(&poly, (b + 1) as f64, false);
            }
            if poly.len() >= 3 {
                let pts: Vec<([f64; 3], [f64; 3])> = poly.iter().map(|v| (v.0, v.1)).collect();
                self.polygon(&pts, s.face(b));
            }
        }
    }
}

/// 段で切る多角形の点(位置、陰の法線、段の座標)。
type ClipV = ([f64; 3], [f64; 3], f64);

/// 多角形を段の座標 `level` で切る(`keep_above` なら level 以上を残す)。
fn clip(poly: &[ClipV], level: f64, keep_above: bool) -> Vec<ClipV> {
    let inside = |t: f64| if keep_above { t >= level } else { t <= level };
    let mut out = Vec::with_capacity(poly.len() + 2);
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        let (ia, ib) = (inside(a.2), inside(b.2));
        if ia {
            out.push(a);
        }
        if ia != ib {
            let f = (level - a.2) / (b.2 - a.2);
            let p = [0, 1, 2].map(|i| a.0[i] + (b.0[i] - a.0[i]) * f);
            let n = [0, 1, 2].map(|i| a.1[i] + (b.1[i] - a.1[i]) * f);
            out.push((p, n, level));
        }
    }
    out
}

/// 結果 `r` を見せ方 `look` で描いた形。`key` は画面の使い回しの番号。
pub fn draw(r: &SimResult, look: &Look, key: u64) -> OverlayShape {
    let frame = look.frame.min(r.frames.len().saturating_sub(1));
    let levels = if look.smooth { SMOOTH } else { BANDS };
    let range_frame = (!look.legend_all).then_some(frame);
    let mut face_names: Vec<String> = Vec::new();
    let mut face_colors: Vec<Option<ShapeInk>> = Vec::new();
    let mut add_block = |name: &str, s: &Scale| {
        for b in 0..s.levels {
            face_names.push(format!("{name}{b}"));
            face_colors.push(Some(ShapeInk::Rgb(s.color(b))));
        }
    };
    let scale_of = |q: usize, levels: usize, base: u32| {
        let def = &r.quantities[q];
        let (lo, hi) = r.range(q, range_frame);
        Scale {
            lo,
            span: (hi - lo).max(1e-30),
            levels,
            reverse: def.low_is_bad,
            cap: def.cap,
            base,
        }
    };
    let mut base = 0u32;
    let surface = look.surface.filter(|q| *q < r.quantities.len()).map(|q| {
        let s = scale_of(q, levels, base);
        base += levels as u32;
        add_block("表面", &s);
        (q, s)
    });
    let section = look
        .section
        .filter(|(q, _, _)| *q < r.quantities.len())
        .map(|(q, axis, at)| {
            // 表面と同じ量なら同じ色の段と凡例を使う(凡例を 2 つ並べない)
            if let Some((_, s)) = surface.filter(|(sq, _)| *sq == q) {
                return (q, axis, at, s);
            }
            let s = scale_of(q, levels, base);
            base += levels as u32;
            add_block("断面", &s);
            (q, axis, at, s)
        });
    let velocity = look
        .streamlines
        .then(|| {
            r.quantities
                .iter()
                .position(|q| q.role == Role::Velocity && q.comps == 3)
        })
        .flatten();
    let stream = velocity.map(|q| {
        let s = scale_of(q, STREAM_BANDS, base);
        base += STREAM_BANDS as u32;
        add_block("流線", &s);
        (q, s)
    });
    let mark_face = base;
    face_names.push("印".into());
    face_colors.push(Some(ShapeInk::Rgb(MARK_COLOR)));
    // 灰色はいつも最後(解いていないボディを足す時に使う)
    let gray = base + 1;
    face_names.push("色なし".into());
    face_colors.push(Some(ShapeInk::Rgb(NO_COLOR)));

    let mut b = Builder::default();
    // 表面(色を塗る網と灰色の網)
    let deform = look
        .deform
        .then(|| {
            r.quantities
                .iter()
                .position(|q| q.role == Role::Deform && q.comps == 3)
        })
        .flatten();
    let mut deform_scale = None;
    for (pi, part) in r.parts.iter().enumerate() {
        let (points, tets) = match &part.geom {
            Geometry::Surface { points, .. } => (points, None),
            Geometry::Tets(t) => (&t.points, Some(t)),
            Geometry::Grid(_) => continue,
        };
        let painted = surface.filter(|(q, _)| r.quantities[*q].part == pi);
        let disp = deform
            .filter(|q| r.quantities[*q].part == pi)
            .and_then(|q| Some((q, r.values(frame, q)?)));
        let k = disp.map(|(q, v)| {
            let k = deform_factor(r, q, points);
            deform_scale = Some(k);
            (v, k)
        });
        let painted = painted.and_then(|(q, s)| {
            let def = &r.quantities[q];
            Some((r.values(frame, q)?, def.comps, s))
        });
        let Some(t) = tets else {
            let Geometry::Surface { tris, .. } = &part.geom else {
                continue;
            };
            let at = |i: u32| -> [f64; 3] {
                let mut p = points[i as usize].map(f64::from);
                if let Some((v, k)) = &k {
                    let d = v.vec3(i as usize);
                    for a in 0..3 {
                        p[a] += k * d[a] as f64;
                    }
                }
                p
            };
            for t in tris {
                let p = t.map(at);
                match &painted {
                    Some((vals, comps, s)) => {
                        let v = t.map(|i| vals.magnitude(i as usize, *comps));
                        if v.iter().any(|x| !x.is_finite()) {
                            b.tri(p, gray);
                        } else {
                            b.banded(p, v.map(|x| s.t(x)), s);
                        }
                    }
                    None => b.tri(p, gray),
                }
            }
            continue;
        };
        // 応力から求める量は、成分を補間してから求める
        let src = |q: usize| -> Option<Src> {
            let def = &r.quantities[q];
            if let Some((tq, d)) = def.from
                && r.quantities.get(tq).is_some_and(|x| x.comps == 6)
                && let Some(v) = r.values(frame, tq)
            {
                return Some(Src::Tensor(v, d));
            }
            Some(Src::Plain(r.values(frame, q)?, def.comps))
        };
        let paint = surface
            .filter(|(q, _)| r.quantities[*q].part == pi)
            .and_then(|(q, s)| Some((src(q)?, s)));
        // 断面で切る(切る量がこの網の時)
        let cut = section.as_ref().and_then(|(q, axis, at, s)| {
            (r.quantities[*q].part == pi).then_some(())?;
            Some((*axis, *at, src(*q)?, *s))
        });
        tets_mesh(
            &mut b,
            t,
            k.as_ref().map(|(v, k)| (*v, *k)),
            paint.as_ref().map(|(v, s)| (*v, s)),
            cut.as_ref().map(|(a, at, v, s)| (*a, *at, *v, s)),
            gray,
        );
    }
    // 印(変形の表示に合わせて動かす)
    for m in r.marks.iter().filter(|m| !m.lines.is_empty()) {
        let Some(part) = r.parts.get(m.part) else {
            continue;
        };
        let points = match &part.geom {
            Geometry::Surface { points, .. } => points,
            Geometry::Tets(t) => &t.points,
            Geometry::Grid(_) => continue,
        };
        let disp = deform
            .filter(|q| r.quantities[*q].part == m.part)
            .and_then(|q| Some((r.values(frame, q)?, deform_factor(r, q, points))));
        let at = |i: u32| -> [f64; 3] {
            let mut p = points[i as usize].map(f64::from);
            if let Some((v, k)) = disp {
                let d = v.vec3(i as usize);
                for a in 0..3 {
                    p[a] += k * d[a] as f64;
                }
            }
            p
        };
        let size = size_of(points);
        // 網を断面で切っているなら、取り除いた側の印も描かない(網と同じ平面で切る)
        let plane = section
            .as_ref()
            .filter(|(q, ..)| {
                r.quantities[*q].part == m.part && matches!(part.geom, Geometry::Tets(_))
            })
            .map(|(_, axis, at, _)| (*axis, *at + 1.37e-6 * size));
        for l in &m.lines {
            let (mut a, mut c) = (at(l[0]), at(l[1]));
            if let Some((axis, cut)) = plane {
                let (da, dc) = (a[axis] - cut, c[axis] - cut);
                if da > 0.0 && dc > 0.0 {
                    continue;
                }
                if da > 0.0 || dc > 0.0 {
                    let t = da / (da - dc);
                    let x: [f64; 3] = std::array::from_fn(|k| a[k] + t * (c[k] - a[k]));
                    if da > 0.0 { a = x } else { c = x }
                }
            }
            segment(&mut b, a, c, 0.004 * size, mark_face);
        }
    }
    // 断面
    if let Some((q, axis, at, s)) = &section {
        let def = &r.quantities[*q];
        if let Some(Geometry::Grid(g)) = r.parts.get(def.part).map(|p| &p.geom)
            && let Some(vals) = r.values(frame, *q)
        {
            let body = g.body.and_then(|i| match &r.parts.get(i)?.geom {
                Geometry::Surface { points, tris } => Some((points, tris)),
                _ => None,
            });
            section_mesh(&mut b, g, vals, def.comps, *axis, *at, body, s);
        }
    }
    // 流線
    if let Some((q, s)) = &stream {
        let def = &r.quantities[*q];
        if let Some(Geometry::Grid(g)) = r.parts.get(def.part).map(|p| &p.geom)
            && let Some(vals) = r.values(frame, *q)
        {
            let body = g.body.and_then(|i| match &r.parts.get(i)?.geom {
                Geometry::Surface { points, .. } => Some(points.as_slice()),
                _ => None,
            });
            for line in streamlines(g, vals, body, look.section.map(|(_, a, v)| (a, v))) {
                tube(&mut b, g, &line, s);
            }
        }
    }
    let mut mesh = b.mesh;
    mesh.face_names = face_names;
    // 見出しと凡例(最初の凡例は表面、無ければ断面)
    let mut legends: Vec<(String, Vec<(ShapeInk, String)>)> = Vec::new();
    let top_open = |q: usize, s: &Scale| {
        r.quantities[q]
            .cap
            .is_some_and(|c| s.lo + s.span >= c - 1e-6)
    };
    if let Some((q, s)) = &surface {
        let d = &r.quantities[*q];
        legends.push((d.title(), s.legend(&d.unit, !look.smooth, top_open(*q, s))));
    }
    if let Some((q, _, _, s)) = section
        .filter(|(q, ..)| surface.is_none_or(|(sq, _)| sq != *q))
        .as_ref()
    {
        let d = &r.quantities[*q];
        legends.push((
            format!("断面 {}", d.title()),
            s.legend(&d.unit, !look.smooth, top_open(*q, s)),
        ));
    }
    if let Some((q, s)) = &stream {
        let d = &r.quantities[*q];
        legends.push((
            format!("流線 {}", d.title()),
            s.legend(&d.unit, true, false),
        ));
    }
    for m in r.marks.iter().filter(|m| !m.lines.is_empty()) {
        legends.push((
            "印".into(),
            vec![(ShapeInk::Rgb(MARK_COLOR), m.name.clone())],
        ));
    }
    let (mut label, legend) = if legends.is_empty() {
        (String::new(), Vec::new())
    } else {
        legends.remove(0)
    };
    if let Some(k) = deform_scale {
        // 倍率は 1・2・5 の倍数なので、1 以上は整数で書く
        let k = if k >= 1.0 { format!("{k:.0}") } else { fmt(k) };
        label.push_str(&format!("・変形 ×{k}"));
    }
    if let Some((q, axis, at, _)) = &section
        && matches!(
            r.parts.get(r.quantities[*q].part).map(|p| &p.geom),
            Some(Geometry::Tets(_))
        )
    {
        label.push_str(&format!(
            "・断面 {} = {} mm",
            ["X", "Y", "Z"][*axis],
            fmt(*at)
        ));
    }
    if r.frames.len() > 1 {
        label.push_str(&format!("・{}", r.frames[frame].label));
    }
    if look.stale {
        label.push_str("・古い");
    }
    let mut badge = r.badge.clone();
    if r.partial {
        badge.push_str("・中止までの結果");
    }
    OverlayShape {
        key,
        label,
        mesh,
        face_colors,
        legend,
        more_legends: legends,
        badge,
    }
}

/// 三角の網(点と三角)。
type Tris<'a> = (&'a Vec<[f32; 3]>, &'a Vec<[u32; 3]>);

/// 点の外接の箱の、いちばん長い辺。
fn size_of(points: &[[f32; 3]]) -> f64 {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for p in points {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a] as f64);
            hi[a] = hi[a].max(p[a] as f64);
        }
    }
    (0..3).map(|a| hi[a] - lo[a]).fold(1e-9, f64::max)
}
