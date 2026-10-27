//! DXF の書き出しと読み込み。
//!
//! 書き出しは R2000(AC1015)。幾何は共通の書き手を使い、R2000 のハンドルと所有関係を補う。
//! 単位は mm として `$INSUNITS = 4` と `$MEASUREMENT = 1` を書く。

use kmb_core::kernel::{P2, Segment, arc_sweep};
use kmb_format::dxf;
use kmb_sketch::{
    Affine, Imported, Shape, Sketch, Solution, entity_segments, shapes_bounds, transform_shape,
};
use std::collections::{BTreeSet, HashMap};
use std::f64::consts::TAU;

/// 書き出しの選び方。
#[derive(Debug, Clone, PartialEq)]
pub struct DxfOptions {
    /// 補助線も書く(既定は書かない)
    pub construction: bool,
    /// 単独の点(穴の位置など)を POINT で書く(既定は書かない)
    pub points: bool,
    /// 通常の線の画層
    pub layer: String,
    /// 補助線の画層(破線、灰色)
    pub construction_layer: String,
}

impl Default for DxfOptions {
    fn default() -> Self {
        DxfOptions {
            construction: false,
            points: false,
            layer: "SKETCH".into(),
            construction_layer: "CONSTRUCTION".into(),
        }
    }
}

/// 数を短く、往復で値が変わらない形で書く(`-0` は `0` にする)
fn num(v: f64) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{s}.0")
    }
}

/// 解いたスケッチを DXF の文字列にする。ファイルに書くのは呼び出し側。
pub fn to_dxf(sketch: &Sketch, sol: &Solution, opts: &DxfOptions) -> String {
    let construction: BTreeSet<_> = sketch
        .entities
        .iter()
        .filter(|e| e.is_construction())
        .map(|e| e.id())
        .collect();
    let segs: Vec<_> = entity_segments(sketch, sol, opts.construction)
        .into_iter()
        .filter(|(_, s)| segment_finite(s))
        .collect();
    let points: Vec<P2> = if opts.points {
        sol.lone
            .iter()
            .filter_map(|id| sol.points.get(id).copied())
            .filter(|p| p[0].is_finite() && p[1].is_finite())
            .collect()
    } else {
        Vec::new()
    };
    let shapes: Vec<Shape> = segs
        .iter()
        .map(|(_, s)| segment_shape(s))
        .chain(points.iter().map(|p| Shape::Point { p: *p }))
        .collect();
    let (lo, hi) = shapes_bounds(&shapes).unwrap_or(([0.0; 2], [0.0; 2]));

    // 同じ名前の画層は 1 つにする(既定の画層 0 に描く選び方もできる)
    let mut layers: Vec<(&str, i32, &str)> = Vec::new();
    for l in [
        ("0", 7, dxf::CONTINUOUS),
        (opts.layer.as_str(), 7, dxf::CONTINUOUS),
        (opts.construction_layer.as_str(), 8, dxf::DASHED),
    ] {
        match layers.iter_mut().find(|x| x.0 == l.0) {
            Some(x) => *x = l,
            None => layers.push(l),
        }
    }
    let mut o = dxf::R12::begin(num, lo, hi, &layers);
    for (id, seg) in &segs {
        let layer = if construction.contains(id) {
            &opts.construction_layer
        } else {
            &opts.layer
        };
        match seg {
            Segment::Line { a, b } => o.line(layer, *a, *b),
            Segment::Circle { center, radius } => o.circle(layer, *center, *radius),
            Segment::Arc {
                center,
                radius,
                start,
                end,
            } => o.arc(layer, *center, *radius, *start, *end),
        }
    }
    for p in &points {
        o.point(&opts.layer, *p);
    }
    crate::r2000::upgrade(&o.finish())
}

fn segment_finite(s: &Segment) -> bool {
    segment_shape(s).is_finite()
}

fn segment_shape(s: &Segment) -> Shape {
    match *s {
        Segment::Line { a, b } => Shape::Line { a, b },
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => Shape::Arc {
            center,
            radius,
            start,
            end,
        },
        Segment::Circle { center, radius } => Shape::Circle { center, radius },
    }
}

// ---- 読み込み ----

/// 要素 1 つ(種類と、その組の並び)。POLYLINE は続く VERTEX を持つ。
struct Ent<'a> {
    kind: &'a str,
    codes: Vec<(i32, &'a str)>,
    verts: Vec<Vec<(i32, &'a str)>>,
}

impl Ent<'_> {
    fn get(&self, code: i32) -> Option<&str> {
        self.codes.iter().find(|c| c.0 == code).map(|c| c.1)
    }
}

/// 数を読む。無ければ `default`、読めなければ `None`。
fn f(codes: &[(i32, &str)], code: i32, default: f64) -> Option<f64> {
    match codes.iter().find(|c| c.0 == code) {
        None => Some(default),
        Some((_, v)) => v.trim().parse::<f64>().ok().filter(|x| x.is_finite()),
    }
}

fn int(codes: &[(i32, &str)], code: i32) -> i64 {
    codes
        .iter()
        .find(|c| c.0 == code)
        .and_then(|(_, v)| {
            let v = v.trim();
            v.parse::<i64>()
                .ok()
                .or_else(|| v.parse::<f64>().ok().map(|x| x as i64))
        })
        .unwrap_or(0)
}

struct Block<'a> {
    base: P2,
    ents: Vec<Ent<'a>>,
}

/// 組の並びから要素を読む。`ENDSEC`・`ENDBLK` か終わりで止まり、その位置を返す。
fn read_entities<'a>(pairs: &[(i32, &'a str)], mut i: usize) -> (Vec<Ent<'a>>, usize) {
    let mut ents: Vec<Ent<'a>> = Vec::new();
    while i < pairs.len() {
        let (code, kind) = pairs[i];
        if code != 0 {
            i += 1;
            continue;
        }
        if kind == "ENDSEC" || kind == "ENDBLK" || kind == "EOF" {
            break;
        }
        i += 1;
        let start = i;
        while i < pairs.len() && pairs[i].0 != 0 {
            i += 1;
        }
        let codes = pairs[start..i].to_vec();
        match kind {
            "VERTEX" => {
                if let Some(last) = ents.last_mut()
                    && last.kind == "POLYLINE"
                {
                    last.verts.push(codes);
                }
            }
            "SEQEND" | "ATTRIB" => {}
            _ => ents.push(Ent {
                kind,
                codes,
                verts: Vec::new(),
            }),
        }
    }
    (ents, i)
}

/// `$INSUNITS` の値を mm への倍率にする(0 や知らない値は mm)
fn unit_scale(code: i64) -> f64 {
    match code {
        1 => 25.4,
        2 => 304.8,
        3 => 1_609_344.0,
        5 => 10.0,
        6 => 1000.0,
        7 => 1e6,
        8 => 25.4e-6,
        9 => 0.0254,
        10 => 914.4,
        11 => 1e-7,
        12 => 1e-6,
        13 => 1e-3,
        14 => 100.0,
        15 => 1e4,
        16 => 1e5,
        17 => 1e12,
        18 => 1.495_978_707e14,
        19 => 9.460_730_472_580_8e18,
        20 => 3.085_677_581e19,
        _ => 1.0,
    }
}

/// 一度に展開する形の数の上限(入れ子の INSERT で爆発しないように)
const MAX_SHAPES: usize = 2_000_000;
const MAX_DEPTH: usize = 16;
const MAX_WORK: usize = 4_000_000;

/// DXF(ASCII)を読む。座標は `$INSUNITS` で mm に直す(無ければ mm とみなす)。
/// LINE・CIRCLE・ARC・LWPOLYLINE・POLYLINE・ELLIPSE・SPLINE・POINT と、BLOCK の INSERT
/// (位置・倍率・回転・行列の配置)を読む。読めなかった要素は種類ごとに数える。
pub fn from_dxf(text: &str) -> Result<Imported, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.starts_with("AutoCAD Binary DXF") {
        return crate::from_dxf_bytes(text.as_bytes());
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut pairs: Vec<(i32, &str)> = Vec::with_capacity(lines.len() / 2);
    for (k, ch) in lines.chunks(2).enumerate() {
        if ch.len() < 2 {
            break;
        }
        let code = ch[0].trim();
        if code.is_empty() && ch[1].trim().is_empty() {
            continue;
        }
        let Ok(code) = code.parse::<i32>() else {
            return Err(format!(
                "DXF ではありません({} 行目のグループコードが数でない)",
                2 * k + 1
            ));
        };
        if code == 999 {
            continue;
        }
        pairs.push((code, ch[1].trim()));
    }
    from_pairs(&pairs)
}

pub(crate) fn from_pairs(pairs: &[(i32, &str)]) -> Result<Imported, String> {
    if !pairs.iter().any(|p| p.0 == 0 && p.1 == "SECTION") {
        return Err("DXF ではありません(SECTION がない)".into());
    }

    let mut insunits = 0i64;
    let mut entities: Vec<Ent> = Vec::new();
    let mut blocks: HashMap<&str, Block> = HashMap::new();
    let mut i = 0;
    while i < pairs.len() {
        if pairs[i] != (0, "SECTION") {
            if pairs[i] == (0, "EOF") {
                break;
            }
            i += 1;
            continue;
        }
        let name = pairs.get(i + 1).map(|p| p.1).unwrap_or("");
        i += 2;
        match name {
            "HEADER" => {
                while i < pairs.len() && pairs[i] != (0, "ENDSEC") {
                    if pairs[i] == (9, "$INSUNITS")
                        && let Some((70, v)) = pairs.get(i + 1)
                    {
                        insunits = v.parse().unwrap_or(0);
                    }
                    i += 1;
                }
            }
            "ENTITIES" => {
                let (e, j) = read_entities(pairs, i);
                entities.extend(e);
                i = j;
            }
            "BLOCKS" => {
                while i < pairs.len() && pairs[i] != (0, "ENDSEC") {
                    if pairs[i] != (0, "BLOCK") {
                        i += 1;
                        continue;
                    }
                    i += 1;
                    let start = i;
                    while i < pairs.len() && pairs[i].0 != 0 {
                        i += 1;
                    }
                    let head = &pairs[start..i];
                    let (ents, j) = read_entities(pairs, i);
                    i = j;
                    // ENDBLK とその組を飛ばす
                    if i < pairs.len() && pairs[i] == (0, "ENDBLK") {
                        i += 1;
                    }
                    let name = head.iter().find(|c| c.0 == 2).map(|c| c.1);
                    if let Some(name) = name {
                        let base = [
                            f(head, 10, 0.0).unwrap_or(0.0),
                            f(head, 20, 0.0).unwrap_or(0.0),
                        ];
                        blocks.insert(name, Block { base, ents });
                    }
                }
            }
            _ => {
                while i < pairs.len() && pairs[i] != (0, "ENDSEC") {
                    i += 1;
                }
            }
        }
    }

    let mut cx = Ctx {
        blocks: &blocks,
        shapes: Vec::new(),
        skipped: HashMap::new(),
        work: 0,
    };
    for e in &entities {
        if int(&e.codes, 67) == 1 {
            cx.skip("ペーパー空間の要素");
            continue;
        }
        cx.entity(e, &Affine::IDENTITY, 0);
    }
    let k = unit_scale(insunits);
    let scale = Affine::scale(k, k);
    let shapes = cx
        .shapes
        .iter()
        .map(|s| {
            if k == 1.0 {
                s.clone()
            } else {
                transform_shape(s, &scale)
            }
        })
        .filter(Shape::is_finite)
        .collect();
    Ok(Imported::new(shapes, cx.skipped))
}

struct Ctx<'b, 'a> {
    blocks: &'b HashMap<&'a str, Block<'a>>,
    shapes: Vec<Shape>,
    skipped: HashMap<String, usize>,
    /// 展開した回数(空のブロックの入れ子で止まらなくならないように)
    work: usize,
}

impl Ctx<'_, '_> {
    fn skip(&mut self, what: &str) {
        *self.skipped.entry(what.to_string()).or_default() += 1;
    }

    fn push(&mut self, s: Shape, m: &Affine) {
        if self.shapes.len() < MAX_SHAPES {
            self.shapes.push(transform_shape(&s, m));
        }
    }

    /// 要素の座標系(OCS)から WCS へ。押し出しが -Z なら x を反転、傾いていれば `None`。
    fn ocs(e: &Ent) -> Option<Affine> {
        let z = f(&e.codes, 230, 1.0)?;
        let (x, y) = (f(&e.codes, 210, 0.0)?, f(&e.codes, 220, 0.0)?);
        let l = (x * x + y * y + z * z).sqrt();
        if l == 0.0 || x.abs() > 1e-9 * l || y.abs() > 1e-9 * l {
            return None;
        }
        Some(if z > 0.0 {
            Affine::IDENTITY
        } else {
            Affine::scale(-1.0, 1.0)
        })
    }

    fn entity(&mut self, e: &Ent, m: &Affine, depth: usize) {
        self.work += 1;
        if !self.try_entity(e, m, depth) {
            self.skip(e.kind);
        }
    }

    /// 読めたら真
    fn try_entity(&mut self, e: &Ent, m: &Affine, depth: usize) -> bool {
        let c = &e.codes;
        let xy = |cx: i32| -> Option<P2> { Some([f(c, cx, 0.0)?, f(c, cx + 10, 0.0)?]) };
        match e.kind {
            "LINE" => {
                let (Some(a), Some(b)) = (xy(10), xy(11)) else {
                    return false;
                };
                self.push(Shape::Line { a, b }, m);
            }
            "POINT" => {
                let Some(p) = xy(10) else { return false };
                self.push(Shape::Point { p }, m);
            }
            "CIRCLE" | "ARC" => {
                let (Some(center), Some(radius), Some(o)) = (xy(10), f(c, 40, 0.0), Self::ocs(e))
                else {
                    return false;
                };
                if radius.is_nan() || radius <= 0.0 {
                    return false;
                }
                let m = m.then_after(&o);
                if e.kind == "CIRCLE" {
                    self.push(Shape::Circle { center, radius }, &m);
                } else {
                    let (Some(s), Some(t)) = (f(c, 50, 0.0), f(c, 51, 360.0)) else {
                        return false;
                    };
                    let start = s.to_radians();
                    let end = start + arc_sweep(start, t.to_radians());
                    self.push(
                        Shape::Arc {
                            center,
                            radius,
                            start,
                            end,
                        },
                        &m,
                    );
                }
            }
            "ELLIPSE" => {
                let (Some(center), Some(major), Some(ratio), Some(s), Some(t)) =
                    (xy(10), xy(11), f(c, 40, 1.0), f(c, 41, 0.0), f(c, 42, TAU))
                else {
                    return false;
                };
                if ratio.is_nan() || ratio <= 0.0 || major == [0.0, 0.0] {
                    return false;
                }
                let sweep = {
                    let d = (t - s).rem_euclid(TAU);
                    if d < 1e-12 { TAU } else { d }
                };
                // 押し出しが -Z なら、パラメータは WCS で時計回り
                let z = f(c, 230, 1.0).unwrap_or(1.0);
                let (start, end) = if z < 0.0 {
                    (-(s + sweep), -s)
                } else {
                    (s, s + sweep)
                };
                self.push(
                    Shape::Ellipse {
                        center,
                        major,
                        ratio,
                        start,
                        end,
                    },
                    m,
                );
            }
            "LWPOLYLINE" => {
                let Some(o) = Self::ocs(e) else { return false };
                let mut verts: Vec<(P2, f64)> = Vec::new();
                for (code, v) in c {
                    let Some(x) = v.trim().parse::<f64>().ok().filter(|x| x.is_finite()) else {
                        if matches!(code, 10 | 20 | 42) {
                            return false;
                        }
                        continue;
                    };
                    match code {
                        10 => verts.push(([x, 0.0], 0.0)),
                        20 => {
                            if let Some(last) = verts.last_mut() {
                                last.0[1] = x;
                            }
                        }
                        42 => {
                            if let Some(last) = verts.last_mut() {
                                last.1 = x;
                            }
                        }
                        _ => {}
                    }
                }
                let closed = int(c, 70) & 1 == 1;
                self.polyline(&verts, closed, &m.then_after(&o));
            }
            "POLYLINE" => {
                let flags = int(c, 70);
                if flags & (16 | 64) != 0 {
                    return false;
                }
                // 3D の折れ線は WCS、2D は OCS
                let o = if flags & 8 != 0 {
                    Affine::IDENTITY
                } else {
                    let Some(o) = Self::ocs(e) else { return false };
                    o
                };
                let mut verts = Vec::new();
                for v in &e.verts {
                    // 16 はスプラインの枠の点(線の上に無い)
                    if int(v, 70) & 16 != 0 {
                        continue;
                    }
                    let (Some(x), Some(y), Some(b)) = (f(v, 10, 0.0), f(v, 20, 0.0), f(v, 42, 0.0))
                    else {
                        return false;
                    };
                    verts.push(([x, y], b));
                }
                self.polyline(&verts, flags & 1 == 1, &m.then_after(&o));
            }
            "SPLINE" => return self.spline(e, m),
            "INSERT" => return self.insert(e, m, depth),
            _ => return false,
        }
        true
    }

    /// 頂点と膨らみ(bulge)の並びを線分と円弧にする
    fn polyline(&mut self, verts: &[(P2, f64)], closed: bool, m: &Affine) {
        let n = verts.len();
        let count = if closed { n } else { n.saturating_sub(1) };
        for k in 0..count {
            let (p, b) = verts[k];
            let q = verts[(k + 1) % n].0;
            if p == q {
                continue;
            }
            self.push(bulge_shape(p, q, b), m);
        }
    }

    fn spline(&mut self, e: &Ent, m: &Affine) -> bool {
        let c = &e.codes;
        let degree = int(c, 71);
        let mut control: Vec<P2> = Vec::new();
        let mut knots = Vec::new();
        let mut weights = Vec::new();
        for (code, v) in c {
            let parse = || v.trim().parse::<f64>().ok().filter(|x| x.is_finite());
            match code {
                10 | 20 | 40 | 41 => {
                    let Some(x) = parse() else { return false };
                    match code {
                        10 => control.push([x, 0.0]),
                        20 => {
                            if let Some(p) = control.last_mut() {
                                p[1] = x;
                            }
                        }
                        40 => knots.push(x),
                        _ => weights.push(x),
                    }
                }
                _ => {}
            }
        }
        if !(1..=32).contains(&degree) {
            return false;
        }
        let degree = degree as usize;
        let n = control.len();
        if n < degree + 1 {
            if n == 0 && c.iter().any(|x| x.0 == 11) {
                self.skip("SPLINE(通過点だけ)");
                return true;
            }
            return false;
        }
        if knots.len() != n + degree + 1 {
            // 節点が無いか数が合わなければ、両端を固定した等間隔の節点にする
            let inner = n - degree;
            knots = (0..n + degree + 1)
                .map(|k| (k.saturating_sub(degree).min(inner)) as f64)
                .collect();
        }
        if weights.len() != n || weights.iter().all(|w| *w == 1.0) {
            weights.clear();
        }
        let s = Shape::BSpline {
            degree,
            control,
            knots,
            weights,
        };
        // 正しくない節点などは読まない
        if kmb_sketch::approximate_tol(&s, 1.0).is_empty() {
            return false;
        }
        self.push(s, m);
        true
    }

    fn insert(&mut self, e: &Ent, m: &Affine, depth: usize) -> bool {
        let c = &e.codes;
        let Some(name) = e.get(2) else { return false };
        let Some(block) = self.blocks.get(name) else {
            return false;
        };
        if depth >= MAX_DEPTH {
            return false;
        }
        let (Some(px), Some(py), Some(sx), Some(sy), Some(rot), Some(dc), Some(dr), Some(o)) = (
            f(c, 10, 0.0),
            f(c, 20, 0.0),
            f(c, 41, 1.0),
            f(c, 42, 1.0),
            f(c, 50, 0.0),
            f(c, 44, 0.0),
            f(c, 45, 0.0),
            Self::ocs(e),
        ) else {
            return false;
        };
        let cols = int(c, 70).clamp(1, 1000);
        let rows = int(c, 71).clamp(1, 1000);
        let base = block.base;
        for r in 0..rows {
            for k in 0..cols {
                self.work += 1;
                if self.shapes.len() >= MAX_SHAPES || self.work > MAX_WORK {
                    self.skip("INSERT(形が多すぎる)");
                    return true;
                }
                let t = m
                    .then_after(&o)
                    .then_after(&Affine::translate(px, py))
                    .then_after(&Affine::rotate(rot.to_radians()))
                    .then_after(&Affine::translate(k as f64 * dc, r as f64 * dr))
                    .then_after(&Affine::scale(sx, sy))
                    .then_after(&Affine::translate(-base[0], -base[1]));
                for be in &block.ents {
                    self.entity(be, &t, depth + 1);
                }
            }
        }
        true
    }
}

/// 点 `p` から `q` への、膨らみ `b`(中心角の 4 分の 1 の正接。正なら反時計回り)の区間
pub(crate) fn bulge_shape(p: P2, q: P2, b: f64) -> Shape {
    if b.abs() < 1e-12 {
        return Shape::Line { a: p, b: q };
    }
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    let chord = dx.hypot(dy);
    let m = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
    // 弦の左向きの単位ベクトル
    let n = [-dy / chord, dx / chord];
    let h = chord / 2.0 * (1.0 - b * b) / (2.0 * b);
    let center = [m[0] + n[0] * h, m[1] + n[1] * h];
    let radius = chord * (1.0 + b * b) / (4.0 * b.abs());
    let ang = |v: P2| (v[1] - center[1]).atan2(v[0] - center[0]);
    let (from, to) = if b > 0.0 { (p, q) } else { (q, p) };
    let start = ang(from);
    Shape::Arc {
        center,
        radius,
        start,
        end: start + arc_sweep(start, ang(to)),
    }
}
