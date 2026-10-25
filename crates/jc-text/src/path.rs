//! 文字を線(線分・円弧の列か円)に沿わせて並べる。

use super::*;

/// 文字を沿わせる線: 端のつながった線分・円弧の列(1 本でもよい)か、円。円弧と円は
/// 反時計回りに進み、逆にたどれば逆に進む。文字は進む向きに並び、字の上は進む向きの左になる。
/// つまり反時計回りなら字の上は円の中心を向き(円の下側で正立)、逆にたどれば外を向く
/// (円の上側で正立)。円の始点は真上。
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// たどる順の区間と、その区間を逆向きにたどるか
    pieces: Vec<(Segment, bool)>,
    /// 終点が始点に戻る(円、または輪になった列)
    closed: bool,
}

/// 端が重なるとみなす距離
const JOIN: f64 = 1e-6;

/// 区間の、本来の向き(円弧は反時計回り)の始点と終点。円は真上から一周。
fn ends(seg: &Segment) -> ([f64; 2], [f64; 2]) {
    match *seg {
        Segment::Line { a, b } => (a, b),
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let at = |t: f64| [center[0] + radius * t.cos(), center[1] + radius * t.sin()];
            (at(start), at(end))
        }
        Segment::Circle { center, radius } => {
            let top = [center[0], center[1] + radius];
            (top, top)
        }
    }
}

/// 円弧と円の中心、半径、本来の向きの始まりの角度、回る角度。
fn sweep(seg: &Segment) -> Option<([f64; 2], f64, f64, f64)> {
    use std::f64::consts::TAU;
    match *seg {
        Segment::Line { .. } => None,
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let mut sweep = (end - start).rem_euclid(TAU);
            if sweep < 1e-12 {
                sweep = TAU;
            }
            Some((center, radius, start, sweep))
        }
        // 円は真上から始める(逆にたどれば、位置 0 の中央揃えで真上に正立する)
        Segment::Circle { center, radius } => Some((center, radius, TAU / 4.0, TAU)),
    }
}

fn seg_length(seg: &Segment) -> f64 {
    match *seg {
        Segment::Line { a, b } => (b[0] - a[0]).hypot(b[1] - a[1]),
        _ => {
            let (_, r, _, sweep) = sweep(seg).expect("円弧か円");
            r * sweep
        }
    }
}

/// 区間の始点から長さ `s` の点、進む向き(長さ 1)、曲がり(左へ曲がれば正、1/半径)。
/// 区間の外は延長の上。
fn seg_at(seg: &Segment, flip: bool, s: f64) -> ([f64; 2], [f64; 2], f64) {
    match *seg {
        Segment::Line { a, b } => {
            let len = seg_length(seg);
            let u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
            if flip {
                ([b[0] - u[0] * s, b[1] - u[1] * s], [-u[0], -u[1]], 0.0)
            } else {
                ([a[0] + u[0] * s, a[1] + u[1] * s], u, 0.0)
            }
        }
        _ => {
            let (c, r, start, sweep) = sweep(seg).expect("円弧か円");
            let (t, sign) = if flip {
                (start + sweep - s / r, -1.0)
            } else {
                (start + s / r, 1.0)
            };
            let (sin, cos) = t.sin_cos();
            (
                [c[0] + r * cos, c[1] + r * sin],
                [-sin * sign, cos * sign],
                sign / r,
            )
        }
    }
}

fn near(p: [f64; 2], q: [f64; 2]) -> bool {
    (p[0] - q[0]).hypot(p[1] - q[1]) <= JOIN
}

impl Path {
    /// 1 本の線。`reverse` なら逆にたどる。
    pub fn new(segment: Segment, reverse: bool) -> Path {
        let closed = matches!(segment, Segment::Circle { .. });
        Path {
            pieces: vec![(segment, reverse)],
            closed,
        }
    }

    /// `first` から、端のつながる線分・円弧(`others` の中)を両側へたどった列。
    /// 向きは `first` の本来の向き(`reverse` なら逆)。3 本以上が出会う所ではそこで止め、
    /// その時は 2 つ目の値が真になる。円はつながない。
    pub fn chain(first: Segment, others: &[Segment], reverse: bool) -> (Path, bool) {
        if matches!(first, Segment::Circle { .. }) {
            return (Path::new(first, reverse), false);
        }
        let mut used = vec![false; others.len()];
        let mut pieces: std::collections::VecDeque<(Segment, bool)> =
            std::collections::VecDeque::from([(first, false)]);
        let end_of = |p: &(Segment, bool)| {
            let (a, b) = ends(&p.0);
            if p.1 { a } else { b }
        };
        let start_of = |p: &(Segment, bool)| {
            let (a, b) = ends(&p.0);
            if p.1 { b } else { a }
        };
        let mut branched = false;
        // その点で出会う、まだ使っていない区間(番号と、その点が本来の始点か)
        let meeting = |at: [f64; 2], used: &[bool]| -> Vec<(usize, bool)> {
            others
                .iter()
                .enumerate()
                .filter(|(i, s)| !used[*i] && !matches!(s, Segment::Circle { .. }))
                .filter_map(|(i, s)| {
                    let (a, b) = ends(s);
                    if near(a, at) {
                        Some((i, true))
                    } else if near(b, at) {
                        Some((i, false))
                    } else {
                        None
                    }
                })
                .collect()
        };
        let mut closed = false;
        // 前へ
        loop {
            let at = end_of(pieces.back().expect("1 本はある"));
            if near(at, start_of(pieces.front().expect("1 本はある"))) {
                closed = true;
                break;
            }
            match meeting(at, &used)[..] {
                [(i, at_start)] => {
                    used[i] = true;
                    // 始点で出会えば本来の向き、終点で出会えば逆向き
                    pieces.push_back((others[i].clone(), !at_start));
                }
                [] => break,
                _ => {
                    branched = true;
                    break;
                }
            }
        }
        // 後ろへ(輪になっていれば要らない)
        if !closed {
            loop {
                let at = start_of(pieces.front().expect("1 本はある"));
                match meeting(at, &used)[..] {
                    [(i, at_start)] => {
                        used[i] = true;
                        // 終点で出会えば本来の向き、始点で出会えば逆向き
                        pieces.push_front((others[i].clone(), at_start));
                    }
                    [] => break,
                    _ => {
                        branched = true;
                        break;
                    }
                }
            }
        }
        let mut pieces: Vec<(Segment, bool)> = pieces.into();
        if reverse {
            pieces.reverse();
            for p in &mut pieces {
                p.1 = !p.1;
            }
        }
        (Path { pieces, closed }, branched)
    }

    pub fn length(&self) -> f64 {
        self.pieces.iter().map(|p| seg_length(&p.0)).sum()
    }

    pub fn closed(&self) -> bool {
        self.closed
    }

    /// 区間の数。
    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// 区間の曲がり(左へ曲がれば正)。
    fn kappa(&self, i: usize) -> f64 {
        let (seg, flip) = &self.pieces[i];
        seg_at(seg, *flip, 0.0).2
    }

    /// 始点から長さ `s` の位置の点、進む向き(長さ 1)、曲がり。
    /// 線の外は端の区間の延長の上(閉じた線は一周して戻る)。
    pub(super) fn at(&self, s: f64) -> ([f64; 2], [f64; 2], f64) {
        let len = self.length();
        let mut s = if self.closed && len > 0.0 {
            s.rem_euclid(len)
        } else {
            s
        };
        let n = self.pieces.len();
        for (i, (seg, flip)) in self.pieces.iter().enumerate() {
            let l = seg_length(seg);
            if s <= l || i + 1 == n || (i == 0 && s < 0.0) {
                return seg_at(seg, *flip, s);
            }
            s -= l;
        }
        unreachable!("区間は 1 本以上ある")
    }

    /// 線から `h` 離れた線の上での長さ(始点から線の上の長さ `s` まで)。
    /// 離れた線の上で字の間を測るのに使う。円の中心を越える区間があれば誤り。
    pub(super) fn offset_length(&self, s: f64, h: f64) -> Result<f64, String> {
        let total = self.length();
        let f = |i: usize| -> Result<f64, String> {
            let f = 1.0 - self.kappa(i) * h;
            if f <= 1e-6 {
                Err("文字が円の中心を越えます(線からの距離か文字の高さを小さくしてください)".into())
            } else {
                Ok(f)
            }
        };
        let n = self.pieces.len();
        let (laps, mut s) = if self.closed && total > 0.0 {
            ((s / total).floor(), s.rem_euclid(total))
        } else {
            (0.0, s)
        };
        let mut full = 0.0;
        if laps != 0.0 {
            for i in 0..n {
                full += seg_length(&self.pieces[i].0) * f(i)?;
            }
        }
        if s < 0.0 {
            return Ok(s * f(0)?);
        }
        let mut out = laps * full;
        for i in 0..n {
            let l = seg_length(&self.pieces[i].0);
            if s <= l || i + 1 == n {
                return Ok(out + s * f(i)?);
            }
            out += l * f(i)?;
            s -= l;
        }
        unreachable!("区間は 1 本以上ある")
    }

    /// `offset_length` の逆: 離れた線の上での長さ `x` に当たる、線の上の長さ。
    pub(super) fn along_offset(&self, x: f64, h: f64) -> Result<f64, String> {
        let f = |i: usize| -> Result<f64, String> {
            let f = 1.0 - self.kappa(i) * h;
            if f <= 1e-6 {
                Err("文字が円の中心を越えます(線からの距離か文字の高さを小さくしてください)".into())
            } else {
                Ok(f)
            }
        };
        let n = self.pieces.len();
        let (laps, mut x) = if self.closed {
            let full = self.offset_length(self.length(), h)?;
            if full <= 0.0 {
                return Err("沿わせる線の長さが 0 です".into());
            }
            ((x / full).floor(), x.rem_euclid(full))
        } else {
            (0.0, x)
        };
        if x < 0.0 {
            return Ok(x / f(0)?);
        }
        let mut s = laps * self.length();
        for i in 0..n {
            let l = seg_length(&self.pieces[i].0);
            let fi = f(i)?;
            if x <= l * fi || i + 1 == n {
                return Ok(s + x / fi);
            }
            x -= l * fi;
            s += l;
        }
        unreachable!("区間は 1 本以上ある")
    }
}

/// 文字列を線に沿わせる。`offset` の x は線の始点から揃えの点までの長さ、y は線から離す長さ
/// (進む向きの左が正)。字は形を保ったまま、字の中心の位置で線の向きに回す。
/// 円の内側・外側に離した行は、その行の半径で字の間を測る。`style.angle` は使わない。
pub fn lay_along(text: &str, style: &Style, path: &Path, offset: [f64; 2]) -> Result<Laid, String> {
    lay_along_with(text, style, None, path, offset)
}

/// [`lay_along`] の、輪郭のフォント `outline` も使える版。
pub fn lay_along_with(
    text: &str,
    style: &Style,
    outline: Option<&OutlineFont>,
    path: &Path,
    offset: [f64; 2],
) -> Result<Laid, String> {
    let len = path.length();
    if len <= 1e-9 {
        return Err("沿わせる線の長さが 0 です".into());
    }
    let sh = shape(text, style, outline);
    // 線から h だけ離れた所で、揃えの点から x 進むときの、線の上での長さ
    // (曲がった区間では、離れた線の上で字の間を測る)
    let along = |x: f64, h: f64| -> Result<f64, String> {
        path.along_offset(path.offset_length(offset[0], h)? + x, h)
    };
    let map = |x: f64, y: f64| -> Result<[f64; 2], String> {
        let h = y + offset[1];
        let (p, t, _) = path.at(along(x, h)?);
        Ok([p[0] - t[1] * h, p[1] + t[0] * h])
    };
    let mut segments = Vec::new();
    let mut rings = Vec::new();
    for g in &sh.glyphs {
        let h = g.base + offset[1];
        let (p, t, _) = path.at(along(g.cx, h)?);
        let n = [-t[1], t[0]];
        let place = |q: [f64; 2]| {
            let (u, v) = (q[0] - g.cx, q[1] + offset[1]);
            [p[0] + t[0] * u + n[0] * v, p[1] + t[1] * u + n[1] * v]
        };
        for (a, b) in &g.strokes {
            segments.push(Segment::Line {
                a: place(*a),
                b: place(*b),
            });
        }
        let theta = t[1].atan2(t[0]);
        for r in &g.rings {
            let moved: Vec<(Segment, bool)> = r
                .iter()
                .map(|(s, back)| (glyph::rigid(s, &place, theta), *back))
                .collect();
            segments.extend(moved.iter().map(|(s, _)| s.clone()));
            rings.push(moved);
        }
    }
    let mut frame = Vec::new();
    let mut warnings = Vec::new();
    if !segments.is_empty() {
        // 枠: 外接の四角形を線に沿って曲げたもの
        let (lo, hi) = (sh.lo, sh.hi);
        const N: usize = 32;
        let mut ring = Vec::new();
        for i in 0..=N {
            ring.push(map(lo[0] + (hi[0] - lo[0]) * i as f64 / N as f64, lo[1])?);
        }
        for i in 0..=N {
            ring.push(map(hi[0] - (hi[0] - lo[0]) * i as f64 / N as f64, hi[1])?);
        }
        for i in 0..ring.len() {
            frame.push(Segment::Line {
                a: ring[i],
                b: ring[(i + 1) % ring.len()],
            });
        }
        let base = |x: f64| along(x, offset[1]);
        let (s0, s1) = (base(lo[0])?, base(hi[0])?);
        if path.closed() {
            if s1 - s0 > len + 1e-9 {
                warnings.push("文字が一周より長く、重なっています".into());
            }
        } else if s0 < -1e-9 || s1 > len + 1e-9 {
            warnings
                .push("文字が線の端をはみ出しています(はみ出た分は線の延長に置きました)".into());
        }
    }
    Ok(Laid {
        segments,
        frame,
        missing: sh.missing,
        warnings,
        rings,
    })
}
