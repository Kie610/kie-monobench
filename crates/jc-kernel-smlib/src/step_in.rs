//! STEP の読み込み(M56)。AP203・AP214・AP242 の B-rep(面・輪・辺・頂点と、その曲面と曲線)を
//! 自分で読み、SMLib の面(平面・円筒・円すい・球・円環・B スプライン)を辺の曲線で切り抜いて縫い、
//! 足し引き・穴・フィレットに使える立体にする(ffi::step_solid)。
//!
//! 読むのは形だけ(色・名前・アセンブリの置き方は読まない)。長さは truck の読み込みと同じく、
//! ファイルの数をそのまま mm として使う(単位を見ない)。角度の単位(円すいの半頂角)は見る。
//! 対応していない曲面や曲線があれば理由を返し、呼ぶ側が今までの読み方(網)に戻す。
//!
//! 面の名前は truck の読み込みと同じ `step{殻の番号}:f{殻の中の面の番号}`(殻は CLOSED_SHELL・
//! OPEN_SHELL を番号の順に数える)。前に読み込んだ文書の加工や測る道具が面を名前で指しているため。

use jc_core::kernel::{self as k, P3};
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

// ---- Part 21 の字句と構文 ----

/// 引数 1 つ。
#[derive(Debug, Clone)]
enum P {
    Ref(u64),
    Num(f64),
    /// 文字列(名前など)。形には使わないが、引用符の読み方を試験で確かめる。
    Str(#[cfg_attr(not(test), allow(dead_code))] String),
    Enum(String),
    Null,
    List(Vec<P>),
    /// 型つきの値(`LENGTH_MEASURE(0.01)` など)。型の名前は見ない。
    Typed(Vec<P>),
}

/// 記録 1 つ(名前と引数)。
#[derive(Debug, Clone)]
struct Rec {
    name: String,
    params: Vec<P>,
}

/// 実体 1 つ。複合の実体(有理 B スプラインなど)は記録の並び。
#[derive(Debug)]
enum Inst {
    Simple(Rec),
    Complex(Vec<Rec>),
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn ws(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"/*") {
                match find(&self.s[self.i + 2..], b"*/") {
                    Some(k) => self.i += k + 4,
                    None => self.i = self.s.len(),
                }
            } else {
                return;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("{} 文字目に「{}」がありません", self.i, c as char))
        }
    }

    fn word(&mut self) -> String {
        self.ws();
        let st = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_')
        {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[st..self.i]).to_ascii_uppercase()
    }

    fn number(&mut self) -> Result<f64, String> {
        let st = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_digit() || b"+-.eE".contains(&self.s[self.i]))
        {
            self.i += 1;
        }
        let t = std::str::from_utf8(&self.s[st..self.i]).unwrap_or("");
        t.parse::<f64>()
            .map_err(|_| format!("{st} 文字目の数「{t}」を読めません"))
    }

    fn params(&mut self) -> Result<Vec<P>, String> {
        self.expect(b'(')?;
        let mut out = Vec::new();
        if self.peek() == Some(b')') {
            self.i += 1;
            return Ok(out);
        }
        loop {
            out.push(self.param()?);
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b')') => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => return Err(format!("{} 文字目で引数の並びが閉じていません", self.i)),
            }
        }
    }

    fn param(&mut self) -> Result<P, String> {
        match self.peek().ok_or("ファイルが途中で終わっています")? {
            b'#' => {
                self.i += 1;
                Ok(P::Ref(self.number()? as u64))
            }
            b'\'' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    match self.s.get(self.i) {
                        None => return Err("文字列が閉じていません".into()),
                        Some(b'\'') if self.s.get(self.i + 1) == Some(&b'\'') => {
                            v.push(b'\'');
                            self.i += 2;
                        }
                        Some(b'\'') => {
                            self.i += 1;
                            break;
                        }
                        Some(&c) => {
                            v.push(c);
                            self.i += 1;
                        }
                    }
                }
                Ok(P::Str(String::from_utf8_lossy(&v).into_owned()))
            }
            b'"' => {
                self.i += 1;
                while self.s.get(self.i).is_some_and(|c| *c != b'"') {
                    self.i += 1;
                }
                self.i += 1;
                Ok(P::Null)
            }
            b'.' => {
                self.i += 1;
                let w = self.word();
                self.expect(b'.')?;
                Ok(P::Enum(w))
            }
            b'$' | b'*' => {
                self.i += 1;
                Ok(P::Null)
            }
            b'(' => Ok(P::List(self.params()?)),
            c if c.is_ascii_digit() || c == b'-' || c == b'+' => Ok(P::Num(self.number()?)),
            c if c.is_ascii_alphabetic() => {
                self.word();
                Ok(P::Typed(self.params()?))
            }
            c => Err(format!("{} 文字目の「{}」を読めません", self.i, c as char)),
        }
    }

    fn record(&mut self) -> Result<Rec, String> {
        let name = self.word();
        if name.is_empty() {
            return Err(format!("{} 文字目に実体の名前がありません", self.i));
        }
        Ok(Rec {
            name,
            params: self.params()?,
        })
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// DATA の節の実体を全部読む。
fn parse(text: &str) -> Result<HashMap<u64, Inst>, String> {
    let s = text.as_bytes();
    let mut out = HashMap::new();
    let mut at = 0;
    while let Some(k) = find(&s[at..], b"DATA") {
        let mut lx = Lexer { s, i: at + k + 4 };
        // DATA の後の「;」(AP242 は DATA('名前', (...)); もある)
        if lx.peek() == Some(b'(') {
            lx.params()?;
        }
        if lx.peek() != Some(b';') {
            at += k + 4;
            continue;
        }
        lx.i += 1;
        while lx.peek() == Some(b'#') {
            lx.i += 1;
            let id = lx.number()? as u64;
            lx.expect(b'=')?;
            let inst = if lx.peek() == Some(b'(') {
                lx.i += 1;
                let mut recs = Vec::new();
                while lx.peek() != Some(b')') {
                    recs.push(lx.record()?);
                }
                lx.i += 1;
                Inst::Complex(recs)
            } else {
                Inst::Simple(lx.record()?)
            };
            lx.expect(b';')?;
            out.insert(id, inst);
        }
        if lx.word() != "ENDSEC" {
            return Err(format!(
                "{} 文字目で DATA の節が ENDSEC で終わっていません",
                lx.i
            ));
        }
        at = lx.i;
    }
    if out.is_empty() {
        return Err("STEP に DATA の節がありません".into());
    }
    Ok(out)
}

// ---- 実体を形に読む ----

/// 座標の向き(原点と 3 本の軸)。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame {
    pub o: P3,
    pub x: P3,
    pub y: P3,
    pub z: P3,
}

impl Frame {
    fn at(&self, a: f64, b: f64, c: f64) -> P3 {
        k::add(
            self.o,
            k::add(
                k::add(k::scale(self.x, a), k::scale(self.y, b)),
                k::scale(self.z, c),
            ),
        )
    }

    fn local(&self, p: P3) -> P3 {
        let d = k::sub(p, self.o);
        [k::dot(d, self.x), k::dot(d, self.y), k::dot(d, self.z)]
    }
}

/// 曲面。
#[derive(Debug, Clone)]
pub(crate) enum Surf {
    Plane(Frame),
    Cylinder(Frame, f64),
    /// 半径と半頂角(ラジアン)
    Cone(Frame, f64, f64),
    Sphere(Frame, f64),
    Torus(Frame, f64, f64),
    BSpline(NurbsSurf),
}

/// 有理 B スプライン曲線(制御点はユークリッド座標と重み、ノットは重複を並べた物)。
#[derive(Debug, Clone)]
pub(crate) struct Nurbs {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub pts: Vec<[f64; 4]>,
}

/// 有理 B スプライン曲面(制御点は u ごとに v の並び)。
#[derive(Debug, Clone)]
pub(crate) struct NurbsSurf {
    pub degree_u: usize,
    pub degree_v: usize,
    pub knots_u: Vec<f64>,
    pub knots_v: Vec<f64>,
    pub pts: Vec<[f64; 4]>,
}

/// 面 1 枚(輪は外の輪が先。輪の曲線は輪を回る向きにそろえた物)。
#[derive(Debug, Clone)]
pub(crate) struct StepFace {
    pub surface: Surf,
    pub same_sense: bool,
    pub loops: Vec<Vec<Nurbs>>,
    /// 1 点だけの輪(円すいの頂点)
    pub points: Vec<P3>,
    pub name: String,
}

/// 3D の曲線(辺の曲線の元)。
#[derive(Debug, Clone)]
enum Curve {
    /// 直線(辺は両端の頂点で決まるので、点と向きは読んで確かめるだけ)
    Line,
    Circle(Frame, f64),
    Ellipse(Frame, f64, f64),
    BSpline(Nurbs),
}

struct Data {
    recs: HashMap<u64, Inst>,
    /// 角度の単位をラジアンに直す倍率
    angle: f64,
}

fn num(p: &P) -> Result<f64, String> {
    match p {
        P::Num(v) => Ok(*v),
        P::Typed(v) if v.len() == 1 => num(&v[0]),
        _ => Err(format!("数のはずの所が {p:?}")),
    }
}

fn rid(p: &P) -> Result<u64, String> {
    match p {
        P::Ref(i) => Ok(*i),
        _ => Err(format!("参照のはずの所が {p:?}")),
    }
}

fn list(p: &P) -> Result<&[P], String> {
    match p {
        P::List(v) => Ok(v),
        _ => Err(format!("並びのはずの所が {p:?}")),
    }
}

fn flag(p: &P) -> bool {
    !matches!(p, P::Enum(e) if e == "F")
}

fn arg(r: &Rec, i: usize) -> Result<&P, String> {
    r.params
        .get(i)
        .ok_or_else(|| format!("{} の引数 {} がありません", r.name, i + 1))
}

fn norm(v: P3) -> P3 {
    let l = k::dot(v, v).sqrt();
    if l < 1e-300 { v } else { k::scale(v, 1.0 / l) }
}

impl Data {
    fn inst(&self, id: u64) -> Result<&Inst, String> {
        self.recs
            .get(&id)
            .ok_or_else(|| format!("#{id} がありません"))
    }

    fn rec(&self, id: u64) -> Result<&Rec, String> {
        match self.inst(id)? {
            Inst::Simple(r) => Ok(r),
            Inst::Complex(_) => Err(format!("#{id} は複合の実体です")),
        }
    }

    /// 複合の実体の中の名前 `name` の記録(単純な実体ならそれ自身の名前が合う時)。
    fn part(&self, id: u64, name: &str) -> Option<&Rec> {
        match self.recs.get(&id)? {
            Inst::Simple(r) => (r.name == name).then_some(r),
            Inst::Complex(v) => v.iter().find(|r| r.name == name),
        }
    }

    fn name_of(&self, id: u64) -> String {
        match self.recs.get(&id) {
            Some(Inst::Simple(r)) => r.name.clone(),
            Some(Inst::Complex(v)) => v
                .iter()
                .map(|r| r.name.as_str())
                .filter(|n| n.contains("B_SPLINE") || n.contains("RATIONAL"))
                .collect::<Vec<_>>()
                .join("+"),
            None => "?".into(),
        }
    }

    fn triple(&self, id: u64, what: &str) -> Result<P3, String> {
        let r = self.rec(id)?;
        if r.name != what {
            return Err(format!("#{id} は {what} ではなく {}", r.name));
        }
        let v = list(arg(r, 1)?)?;
        let c = |i: usize| v.get(i).map_or(Ok(0.0), num);
        Ok([c(0)?, c(1)?, c(2)?])
    }

    fn point(&self, id: u64) -> Result<P3, String> {
        self.triple(id, "CARTESIAN_POINT")
    }

    fn direction(&self, id: u64) -> Result<P3, String> {
        Ok(norm(self.triple(id, "DIRECTION")?))
    }

    fn frame(&self, id: u64) -> Result<Frame, String> {
        let r = self.rec(id)?;
        if r.name != "AXIS2_PLACEMENT_3D" {
            return Err(format!("#{id} は AXIS2_PLACEMENT_3D ではなく {}", r.name));
        }
        let o = self.point(rid(arg(r, 1)?)?)?;
        let z = match arg(r, 2)? {
            P::Ref(i) => self.direction(*i)?,
            _ => [0.0, 0.0, 1.0],
        };
        // 参照の向きが無ければ X 軸(Z が X に沿う時は Y 軸。ISO 10303-42 の first_proj_axis と同じ)
        let r0 = match r.params.get(3) {
            Some(P::Ref(i)) => self.direction(*i)?,
            _ if z[0].abs() > 1.0 - 1e-9 => [0.0, 1.0, 0.0],
            _ => [1.0, 0.0, 0.0],
        };
        let x = norm(k::sub(r0, k::scale(z, k::dot(r0, z))));
        Ok(Frame {
            o,
            x,
            y: k::cross(z, x),
            z,
        })
    }

    fn surface(&self, id: u64) -> Result<Surf, String> {
        if let Some(s) = self.bspline_surface(id)? {
            return Ok(Surf::BSpline(s));
        }
        let r = self.rec(id)?;
        let f = || self.frame(rid(arg(r, 1)?)?);
        Ok(match r.name.as_str() {
            "PLANE" => Surf::Plane(f()?),
            "CYLINDRICAL_SURFACE" => Surf::Cylinder(f()?, num(arg(r, 2)?)?),
            "CONICAL_SURFACE" => Surf::Cone(f()?, num(arg(r, 2)?)?, num(arg(r, 3)?)? * self.angle),
            "SPHERICAL_SURFACE" => Surf::Sphere(f()?, num(arg(r, 2)?)?),
            "TOROIDAL_SURFACE" => Surf::Torus(f()?, num(arg(r, 2)?)?, num(arg(r, 3)?)?),
            other => return Err(format!("曲面 {other}(#{id})には対応していません")),
        })
    }

    /// B スプラインの曲面(単純な実体と、有理の複合の実体)。B スプラインでなければ `None`。
    fn bspline_surface(&self, id: u64) -> Result<Option<NurbsSurf>, String> {
        let (head, with_knots, weights) = match self.inst(id)? {
            Inst::Simple(r) if r.name == "B_SPLINE_SURFACE_WITH_KNOTS" => {
                (&r.params[1..8], &r.params[8..], None)
            }
            Inst::Complex(_) => {
                let (Some(b), Some(kn)) = (
                    self.part(id, "B_SPLINE_SURFACE"),
                    self.part(id, "B_SPLINE_SURFACE_WITH_KNOTS"),
                ) else {
                    return Ok(None);
                };
                let w = self
                    .part(id, "RATIONAL_B_SPLINE_SURFACE")
                    .map(|r| r.params.first().cloned());
                (b.params.as_slice(), kn.params.as_slice(), w.flatten())
            }
            _ => return Ok(None),
        };
        let du = num(&head[0])? as usize;
        let dv = num(&head[1])? as usize;
        let rows = list(&head[2])?;
        let wrows: Option<Vec<&[P]>> = match &weights {
            Some(p) => Some(list(p)?.iter().map(list).collect::<Result<_, _>>()?),
            None => None,
        };
        let mut pts = Vec::new();
        let mut cv = 0;
        for (i, row) in rows.iter().enumerate() {
            let row = list(row)?;
            cv = row.len();
            for (j, q) in row.iter().enumerate() {
                let p = self.point(rid(q)?)?;
                let w = match &wrows {
                    Some(w) => num(w[i].get(j).ok_or("重みが足りません")?)?,
                    None => 1.0,
                };
                pts.push([p[0], p[1], p[2], w]);
            }
        }
        let knots_u = expand(list(&with_knots[0])?, list(&with_knots[2])?)?;
        let knots_v = expand(list(&with_knots[1])?, list(&with_knots[3])?)?;
        if knots_u.len() != rows.len() + du + 1 || knots_v.len() != cv + dv + 1 {
            return Err(format!("B スプラインの曲面 #{id} のノットの数が合いません"));
        }
        Ok(Some(NurbsSurf {
            degree_u: du,
            degree_v: dv,
            knots_u,
            knots_v,
            pts,
        }))
    }

    fn curve(&self, id: u64) -> Result<Curve, String> {
        if let Some(n) = self.bspline_curve(id)? {
            return Ok(Curve::BSpline(n));
        }
        let r = self.rec(id)?;
        Ok(match r.name.as_str() {
            "LINE" => {
                self.point(rid(arg(r, 1)?)?)?;
                let v = self.rec(rid(arg(r, 2)?)?)?;
                self.direction(rid(arg(v, 1)?)?)?;
                Curve::Line
            }
            "CIRCLE" => Curve::Circle(self.frame(rid(arg(r, 1)?)?)?, num(arg(r, 2)?)?),
            "ELLIPSE" => Curve::Ellipse(
                self.frame(rid(arg(r, 1)?)?)?,
                num(arg(r, 2)?)?,
                num(arg(r, 3)?)?,
            ),
            // 面の上の曲線は 3D の曲線だけを使う
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" => {
                self.curve(rid(arg(r, 1)?)?)?
            }
            "TRIMMED_CURVE" => self.curve(rid(arg(r, 1)?)?)?,
            "POLYLINE" => {
                let pts: Vec<P3> = list(arg(r, 1)?)?
                    .iter()
                    .map(|q| self.point(rid(q)?))
                    .collect::<Result<_, _>>()?;
                let n = pts.len();
                let mut knots = vec![0.0];
                knots.extend((0..n).map(|i| i as f64));
                knots.push((n - 1) as f64);
                Curve::BSpline(Nurbs {
                    degree: 1,
                    knots,
                    pts: pts.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
                })
            }
            other => return Err(format!("曲線 {other}(#{id})には対応していません")),
        })
    }

    fn bspline_curve(&self, id: u64) -> Result<Option<Nurbs>, String> {
        let (head, with_knots, weights) = match self.inst(id)? {
            Inst::Simple(r) if r.name == "B_SPLINE_CURVE_WITH_KNOTS" => {
                (&r.params[1..6], &r.params[6..], None)
            }
            Inst::Complex(_) => {
                let (Some(b), Some(kn)) = (
                    self.part(id, "B_SPLINE_CURVE"),
                    self.part(id, "B_SPLINE_CURVE_WITH_KNOTS"),
                ) else {
                    return Ok(None);
                };
                let w = self
                    .part(id, "RATIONAL_B_SPLINE_CURVE")
                    .map(|r| r.params.first().cloned());
                (b.params.as_slice(), kn.params.as_slice(), w.flatten())
            }
            _ => return Ok(None),
        };
        let degree = num(&head[0])? as usize;
        let cps = list(&head[1])?;
        let ws: Option<&[P]> = match &weights {
            Some(p) => Some(list(p)?),
            None => None,
        };
        let mut pts = Vec::new();
        for (i, q) in cps.iter().enumerate() {
            let p = self.point(rid(q)?)?;
            let w = match ws {
                Some(w) => num(w.get(i).ok_or("重みが足りません")?)?,
                None => 1.0,
            };
            pts.push([p[0], p[1], p[2], w]);
        }
        let knots = expand(list(&with_knots[0])?, list(&with_knots[1])?)?;
        if knots.len() != pts.len() + degree + 1 {
            return Err(format!("B スプラインの曲線 #{id} のノットの数が合いません"));
        }
        Ok(Some(Nurbs { degree, knots, pts }))
    }

    fn vertex(&self, id: u64) -> Result<P3, String> {
        let r = self.rec(id)?;
        if r.name != "VERTEX_POINT" {
            return Err(format!("#{id} は VERTEX_POINT ではなく {}", r.name));
        }
        self.point(rid(arg(r, 1)?)?)
    }

    /// 辺 `id` を、始まりの頂点から終わりの頂点へ向かう曲線にする(`forward` が偽なら逆向き)。
    fn edge(&self, id: u64, forward: bool) -> Result<Nurbs, String> {
        let r = self.rec(id)?;
        if r.name != "EDGE_CURVE" {
            return Err(format!("辺 {}(#{id})には対応していません", r.name));
        }
        let a = self.vertex(rid(arg(r, 1)?)?)?;
        let b = self.vertex(rid(arg(r, 2)?)?)?;
        let cid = rid(arg(r, 3)?)?;
        let same = flag(arg(r, 4)?);
        let c = self
            .curve(cid)
            .map_err(|e| format!("辺 #{id} の曲線: {e}"))?;
        let n = edge_curve(&c, a, b, same)
            .map_err(|e| format!("辺 #{id}({}): {e}", self.name_of(cid)))?;
        Ok(if forward { n } else { reverse(&n) })
    }

    fn face(&self, id: u64, name: String) -> Result<StepFace, String> {
        let r = self.rec(id)?;
        if r.name != "ADVANCED_FACE" && r.name != "FACE_SURFACE" {
            return Err(format!("面 {}(#{id})には対応していません", r.name));
        }
        let sid = rid(arg(r, 2)?)?;
        let surface = self.surface(sid).map_err(|e| format!("面 #{id}: {e}"))?;
        let same_sense = flag(arg(r, 3)?);
        let mut outer: Option<Vec<Nurbs>> = None;
        let mut loops = Vec::new();
        let mut points = Vec::new();
        for b in list(arg(r, 1)?)? {
            let br = self.rec(rid(b)?)?;
            let lid = rid(arg(br, 1)?)?;
            let orient = flag(arg(br, 2)?);
            let lr = self.rec(lid)?;
            match lr.name.as_str() {
                "VERTEX_LOOP" => {
                    points.push(self.vertex(rid(arg(lr, 1)?)?)?);
                    continue;
                }
                "EDGE_LOOP" => {}
                other => return Err(format!("輪 {other}(#{lid})には対応していません")),
            }
            let mut curves = Vec::new();
            for oe in list(arg(lr, 1)?)? {
                let o = self.rec(rid(oe)?)?;
                if o.name != "ORIENTED_EDGE" {
                    return Err(format!("輪の辺 {}(#{lid})には対応していません", o.name));
                }
                curves.push(self.edge(rid(arg(o, 3)?)?, flag(arg(o, 4)?))?);
            }
            if !orient {
                curves = curves.iter().rev().map(reverse).collect();
            }
            if br.name == "FACE_OUTER_BOUND" && outer.is_none() {
                outer = Some(curves);
            } else {
                loops.push(curves);
            }
        }
        // 外の輪の印が無ければ、平面は面の表から見て左回りで一番大きい輪を外にする
        let outer = match outer {
            Some(o) => Some(o),
            None => match &surface {
                Surf::Plane(f) if loops.len() > 1 => {
                    let n = if same_sense { f.z } else { k::scale(f.z, -1.0) };
                    let k = (0..loops.len())
                        .max_by(|&a, &b| {
                            loop_area(&loops[a], n).total_cmp(&loop_area(&loops[b], n))
                        })
                        .unwrap_or(0);
                    Some(loops.remove(k))
                }
                _ => None,
            },
        };
        let mut all = Vec::new();
        all.extend(outer);
        all.extend(loops);
        if all.is_empty() && points.is_empty() {
            return Err(format!("面 #{id} に輪がありません"));
        }
        Ok(StepFace {
            surface,
            same_sense,
            loops: all,
            points,
            name,
        })
    }
}

/// 重みつきの数の並び `mults` と値 `knots` を、重複を並べたノットにする。
fn expand(mults: &[P], knots: &[P]) -> Result<Vec<f64>, String> {
    let mut out = Vec::new();
    for (m, v) in mults.iter().zip(knots) {
        let v = num(v)?;
        for _ in 0..num(m)? as usize {
            out.push(v);
        }
    }
    Ok(out)
}

/// 輪の面積(法線 `n` に対して左回りなら正)。曲線は端の点だけで近似する(外の輪を選ぶだけなので
/// 粗くてよいが、円 1 本の輪のために曲線の途中の点も入れる)。
fn loop_area(curves: &[Nurbs], n: P3) -> f64 {
    let mut pts = Vec::new();
    for c in curves {
        let (t0, t1) = (c.knots[c.degree], c.knots[c.knots.len() - c.degree - 1]);
        for i in 0..8 {
            pts.push(eval(c, t0 + (t1 - t0) * i as f64 / 8.0));
        }
    }
    let mut s = [0.0; 3];
    for i in 0..pts.len() {
        s = k::add(s, k::cross(pts[i], pts[(i + 1) % pts.len()]));
    }
    0.5 * k::dot(s, n)
}

// ---- 曲線の計算 ----

fn hom(p: [f64; 4]) -> [f64; 4] {
    [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
}

fn euc(q: [f64; 4]) -> [f64; 4] {
    [q[0] / q[3], q[1] / q[3], q[2] / q[3], q[3]]
}

/// 曲線の点。
pub(crate) fn eval(c: &Nurbs, t: f64) -> P3 {
    let n = c.pts.len();
    let span = crate::skin::find_span(&c.knots, c.degree, n, t);
    let b = crate::skin::basis(&c.knots, c.degree, span, t);
    let mut q = [0.0; 4];
    for (j, bj) in b.iter().enumerate() {
        let h = hom(c.pts[span - c.degree + j]);
        for i in 0..4 {
            q[i] += bj * h[i];
        }
    }
    [q[0] / q[3], q[1] / q[3], q[2] / q[3]]
}

/// 曲線を逆向きにする。
pub(crate) fn reverse(c: &Nurbs) -> Nurbs {
    let (a, b) = (c.knots[0], c.knots[c.knots.len() - 1]);
    Nurbs {
        degree: c.degree,
        knots: c.knots.iter().rev().map(|t| a + b - t).collect(),
        pts: c.pts.iter().rev().copied().collect(),
    }
}

/// ノット `t` を 1 回差す(Boehm)。
fn insert(c: &Nurbs, t: f64) -> Nurbs {
    let p = c.degree;
    let n = c.pts.len();
    let span = crate::skin::find_span(&c.knots, p, n, t);
    let h: Vec<[f64; 4]> = c.pts.iter().map(|q| hom(*q)).collect();
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let q = if i + p <= span {
            h[i]
        } else if i > span {
            h[i - 1]
        } else {
            let a = (t - c.knots[i]) / (c.knots[i + p] - c.knots[i]);
            let mut q = [0.0; 4];
            for j in 0..4 {
                q[j] = (1.0 - a) * h[i - 1][j] + a * h[i][j];
            }
            q
        };
        out.push(euc(q));
    }
    let mut knots = c.knots.clone();
    knots.insert(span + 1, t);
    Nurbs {
        degree: p,
        knots,
        pts: out,
    }
}

/// 曲線の `t0..t1` の部分(t0 < t1)。両端のノットを次数の数だけ差してから、その間の制御点を取る。
fn sub(c: &Nurbs, t0: f64, t1: f64) -> Nurbs {
    let p = c.degree;
    let mut c = c.clone();
    let same = |a: f64, b: f64| (a - b).abs() < 1e-12;
    for t in [t0, t1] {
        let have = c.knots.iter().filter(|k| same(**k, t)).count();
        for _ in have..p {
            c = insert(&c, t);
        }
    }
    let i0 = c.knots.iter().rposition(|k| same(*k, t0)).unwrap_or(p);
    let i1 = c
        .knots
        .iter()
        .position(|k| same(*k, t1))
        .unwrap_or(c.pts.len());
    let mut knots = vec![t0; p + 1];
    knots.extend(c.knots[i0 + 1..i1].iter().copied());
    knots.extend(std::iter::repeat_n(t1, p + 1));
    Nurbs {
        degree: p,
        knots,
        pts: c.pts[i0 - p..i1].to_vec(),
    }
}

/// 曲線の上で点 `q` に一番近い値。
fn param_of(c: &Nurbs, q: P3) -> f64 {
    let (t0, t1) = (c.knots[c.degree], c.knots[c.knots.len() - c.degree - 1]);
    let n = 64 * c.pts.len().max(4);
    let d2 = |t: f64| {
        let d = k::sub(eval(c, t), q);
        k::dot(d, d)
    };
    let mut best = (t0, d2(t0));
    for i in 1..=n {
        let t = t0 + (t1 - t0) * i as f64 / n as f64;
        let d = d2(t);
        if d < best.1 {
            best = (t, d);
        }
    }
    // 黄金分割で詰める
    let h = (t1 - t0) / n as f64;
    let (mut a, mut b) = ((best.0 - h).max(t0), (best.0 + h).min(t1));
    let g = 0.618_033_988_749_895;
    for _ in 0..80 {
        let x1 = b - g * (b - a);
        let x2 = a + g * (b - a);
        if d2(x1) < d2(x2) {
            b = x2;
        } else {
            a = x1;
        }
    }
    0.5 * (a + b)
}

/// 円(楕円)の `phi0` から `delta`(正なら左回り)の弧を、90 度以下の有理 2 次の区間で表す。
fn arc(f: &Frame, ra: f64, rb: f64, phi0: f64, delta: f64) -> Nurbs {
    let spans = ((delta.abs() / FRAC_PI_2) - 1e-9).ceil().max(1.0) as usize;
    let d = delta / spans as f64;
    let w = (d / 2.0).cos();
    let at = |phi: f64, s: f64| f.at(ra * phi.cos() * s, rb * phi.sin() * s, 0.0);
    let mut pts = Vec::new();
    for i in 0..spans {
        let a = phi0 + d * i as f64;
        let p0 = at(a, 1.0);
        if i == 0 {
            pts.push([p0[0], p0[1], p0[2], 1.0]);
        }
        let m = at(a + d / 2.0, 1.0 / w);
        pts.push([m[0], m[1], m[2], w]);
        let p1 = at(a + d, 1.0);
        pts.push([p1[0], p1[1], p1[2], 1.0]);
    }
    let mut knots = vec![0.0; 3];
    for i in 1..spans {
        let t = i as f64 / spans as f64;
        knots.push(t);
        knots.push(t);
    }
    knots.extend([1.0; 3]);
    Nurbs {
        degree: 2,
        knots,
        pts,
    }
}

/// 辺の曲線 `c` の、頂点 `a` から `b` までの部分(`same` が偽なら曲線の向きと逆にたどる)。
fn edge_curve(c: &Curve, a: P3, b: P3, same: bool) -> Result<Nurbs, String> {
    let closed = k::dot(k::sub(a, b), k::sub(a, b)).sqrt() < 1e-7;
    match c {
        Curve::Line => {
            if closed {
                return Err("両端が同じ直線の辺".into());
            }
            Ok(Nurbs {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                pts: vec![[a[0], a[1], a[2], 1.0], [b[0], b[1], b[2], 1.0]],
            })
        }
        Curve::Circle(f, r) | Curve::Ellipse(f, r, _) => {
            let rb = match c {
                Curve::Ellipse(_, _, rb) => *rb,
                _ => *r,
            };
            let ang = |p: P3| {
                let l = f.local(p);
                (l[1] / rb).atan2(l[0] / r)
            };
            let (pa, pb) = (ang(a), ang(b));
            // 曲線の向き(左回り)にたどるなら a から b へ正の角、逆なら負の角
            let mut delta = if same { pb - pa } else { pa - pb };
            while delta <= 1e-9 {
                delta += TAU;
            }
            if closed {
                delta = TAU;
            }
            Ok(arc(f, *r, rb, pa, if same { delta } else { -delta }))
        }
        Curve::BSpline(n) => {
            let (t0, t1) = (n.knots[n.degree], n.knots[n.knots.len() - n.degree - 1]);
            let (ta, tb) = if closed {
                (t0, t1)
            } else {
                (param_of(n, a), param_of(n, b))
            };
            let piece = if same {
                if tb <= ta {
                    return Err("曲線の向きと頂点の並びが合いません".into());
                }
                sub(n, ta, tb)
            } else {
                if ta <= tb {
                    return Err("曲線の向きと頂点の並びが合いません".into());
                }
                reverse(&sub(n, tb, ta))
            };
            Ok(piece)
        }
    }
}

// ---- まとめ ----

/// 読んだ殻と面。
pub(crate) struct Model {
    pub faces: Vec<StepFace>,
    /// 空洞の殻の面の名前の頭(`step{n}:`)
    pub voids: Vec<String>,
}

/// STEP を読み、殻ごとの面を返す。
pub(crate) fn read(text: &str) -> Result<Model, String> {
    let recs = parse(text)?;
    // 角度の単位: 「度」の換算の単位があれば度、ほかはラジアン
    let degrees = recs.values().any(|i| match i {
        Inst::Complex(v) => {
            v.iter().any(|r| r.name == "PLANE_ANGLE_UNIT")
                && v.iter().any(|r| r.name == "CONVERSION_BASED_UNIT")
        }
        _ => false,
    });
    let data = Data {
        recs,
        angle: if degrees { PI / 180.0 } else { 1.0 },
    };
    let mut shells: Vec<u64> = data
        .recs
        .iter()
        .filter(|(_, i)| matches!(i, Inst::Simple(r) if r.name == "CLOSED_SHELL" || r.name == "OPEN_SHELL"))
        .map(|(id, _)| *id)
        .collect();
    shells.sort();
    if shells.is_empty() {
        return Err("STEP に形(殻)がありません".into());
    }
    let mut voids = Vec::new();
    for i in data.recs.values() {
        if let Inst::Simple(r) = i
            && r.name == "BREP_WITH_VOIDS"
            && let Some(P::List(v)) = r.params.get(2)
        {
            for p in v {
                let Ok(o) = rid(p).and_then(|id| data.rec(id)) else {
                    continue;
                };
                if let Some(Ok(s)) = o.params.get(2).map(rid)
                    && let Some(n) = shells.iter().position(|x| *x == s)
                {
                    voids.push(format!("step{n}:"));
                }
            }
        }
    }
    let mut faces = Vec::new();
    for (n, sid) in shells.iter().enumerate() {
        let s = data.rec(*sid)?;
        if s.name == "OPEN_SHELL" {
            return Err("閉じていない殻(OPEN_SHELL)は立体にできません".into());
        }
        for (i, f) in list(arg(s, 1)?)?.iter().enumerate() {
            faces.push(data.face(rid(f)?, format!("step{n}:f{i}"))?);
        }
    }
    Ok(Model { faces, voids })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: f64, b: f64) -> Nurbs {
        Nurbs {
            degree: 3,
            knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
            pts: (0..5)
                .map(|i| {
                    let x = a + (b - a) * i as f64 / 4.0;
                    [x, (x * 0.3).sin(), 0.0, 1.0 + 0.1 * i as f64]
                })
                .collect(),
        }
    }

    #[test]
    fn a_part_of_a_curve_keeps_its_shape() {
        let c = line(0.0, 10.0);
        let s = sub(&c, 0.2, 0.7);
        assert_eq!(s.knots.len(), s.pts.len() + s.degree + 1);
        for i in 0..=10 {
            let t = 0.2 + 0.5 * i as f64 / 10.0;
            let (p, q) = (eval(&c, t), eval(&s, t));
            assert!(
                k::dot(k::sub(p, q), k::sub(p, q)).sqrt() < 1e-9,
                "{t}: {p:?} {q:?}"
            );
        }
        let r = reverse(&s);
        let (p, q) = (eval(&r, 0.2), eval(&c, 0.7));
        assert!(k::dot(k::sub(p, q), k::sub(p, q)).sqrt() < 1e-9);
        assert!((param_of(&c, eval(&c, 0.33)) - 0.33).abs() < 1e-7);
    }

    #[test]
    fn an_arc_follows_the_circle_in_both_directions() {
        let f = Frame {
            o: [1.0, 2.0, 3.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        };
        let c = Curve::Circle(f, 5.0);
        let (a, b) = ([6.0, 2.0, 3.0], [1.0, 7.0, 3.0]);
        let fwd = edge_curve(&c, a, b, true).unwrap();
        let mid = eval(&fwd, 0.5);
        // 左回りに 90 度: 中ほどは 45 度の所
        assert!(
            (mid[0] - (1.0 + 5.0 * 0.5f64.sqrt())).abs() < 1e-9,
            "{mid:?}"
        );
        let back = edge_curve(&c, a, b, false).unwrap();
        let mid = eval(&back, 0.5);
        // 右回りに 270 度: 中ほどは -135 度の所
        assert!(
            (mid[0] - (1.0 - 5.0 * 0.5f64.sqrt())).abs() < 1e-9,
            "{mid:?}"
        );
        assert!(
            (mid[1] - (2.0 - 5.0 * 0.5f64.sqrt())).abs() < 1e-9,
            "{mid:?}"
        );
        let full = edge_curve(&c, a, a, true).unwrap();
        assert_eq!(full.pts.len(), 9);
    }

    #[test]
    fn complex_entities_and_comments_are_read() {
        let t = "ISO-10303-21;\nHEADER;\nFILE_NAME('a;b','',(''),(''),'','','');\nENDSEC;\nDATA;\n\
                 #1=CARTESIAN_POINT('',(0.,0.,0.));/* c */\n#2=( LENGTH_UNIT() NAMED_UNIT(*)\nSI_UNIT(.MILLI.,.METRE.) );\n\
                 #3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.01),#2,'it''s','');\nENDSEC;\nEND-ISO-10303-21;\n";
        let r = parse(t).unwrap();
        assert_eq!(r.len(), 3);
        assert!(matches!(&r[&2], Inst::Complex(v) if v.len() == 3));
        let Inst::Simple(u) = &r[&3] else { panic!() };
        assert!(matches!(&u.params[2], P::Str(s) if s == "it's"));
    }
}
