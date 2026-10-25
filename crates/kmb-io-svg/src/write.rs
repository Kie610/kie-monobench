//! スケッチを SVG に書き出す(レーザー加工機のソフト LightBurn などで、そのまま同じ大きさで開ける形)。
//!
//! - 大きさは `width`・`height` を mm で書き、`viewBox` も mm の値(1 単位 = 1 mm)。px で書くと、
//!   読む側の dpi の決め方(96 か 72 か)で大きさが変わるため。
//! - 線は塗らず(`fill="none"`)に線だけ。LightBurn は線の色で層を分けるので、決まった色にする:
//!   通常の線は黒(層 00)、補助線を書く時は青(層 01)の破線。
//! - 端がつながる線と円弧は 1 本の `path` にまとめ、閉じた輪郭は `Z` で閉じる(閉じた形として扱われ、
//!   塗りつぶしの彫刻やオフセットに使える)。円は `circle`。
//! - y は上向きのスケッチを SVG の下向きの y に反転し、図の左下の角を紙の左下に置く。
//!   変換(`transform`)は使わない。読み込み([`crate::from_svg`])で読み直すと同じ形に戻る。

use kmb_core::kernel::{P2, Segment, arc_sweep};
use kmb_format::svg::{esc, num};
use kmb_sketch::{EntityId, Sketch, Solution, entity_segments};
use std::f64::consts::{PI, TAU};
use std::fmt::Write as _;

/// 書き出しの選び方。
#[derive(Debug, Clone, PartialEq)]
pub struct SvgOptions {
    /// 補助線も書く(既定は書かない)
    pub construction: bool,
    /// 図の名前(`title`)
    pub title: String,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions {
            construction: false,
            title: "スケッチ".into(),
        }
    }
}

/// 通常の線の色(LightBurn の層 00)。
pub const LINE_COLOR: &str = "#000000";
/// 補助線の色(LightBurn の層 01)。
pub const CONSTRUCTION_COLOR: &str = "#0000FF";

/// つなげる 1 区間(線か円弧。円弧は反時計回りに `sweep` だけ回る。逆にたどると時計回り)。
#[derive(Debug, Clone, Copy)]
struct Piece {
    a: P2,
    b: P2,
    /// 円弧なら(半径、回る角。正は反時計回り)
    arc: Option<(f64, f64)>,
}

impl Piece {
    fn reversed(self) -> Piece {
        Piece {
            a: self.b,
            b: self.a,
            arc: self.arc.map(|(r, s)| (r, -s)),
        }
    }
}

/// 区間を区切りに分ける(円は丸ごと、半周を超える円弧も 1 つの円弧として)。
fn pieces(segs: &[Segment]) -> (Vec<Piece>, Vec<(P2, f64)>) {
    let mut out = Vec::new();
    let mut circles = Vec::new();
    for s in segs {
        match *s {
            Segment::Line { a, b } => out.push(Piece { a, b, arc: None }),
            Segment::Circle { center, radius } => circles.push((center, radius)),
            Segment::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sweep = arc_sweep(start, end);
                if sweep >= TAU - 1e-9 {
                    circles.push((center, radius));
                    continue;
                }
                let at = |t: f64| [center[0] + radius * t.cos(), center[1] + radius * t.sin()];
                out.push(Piece {
                    a: at(start),
                    b: at(start + sweep),
                    arc: Some((radius, sweep)),
                });
            }
        }
    }
    (out, circles)
}

/// 端がつながる区間を、つなげた列にする。`tol` は同じ点とみなす距離。
fn chain(mut ps: Vec<Piece>, tol: f64) -> Vec<Vec<Piece>> {
    let near = |p: P2, q: P2| (p[0] - q[0]).abs() <= tol && (p[1] - q[1]).abs() <= tol;
    let mut out = Vec::new();
    while let Some(first) = ps.pop() {
        let mut cur = vec![first];
        loop {
            let (start, end) = (cur[0].a, cur[cur.len() - 1].b);
            if near(start, end) {
                break;
            }
            if let Some(k) = ps.iter().position(|p| near(p.a, end) || near(p.b, end)) {
                let p = ps.swap_remove(k);
                cur.push(if near(p.a, end) { p } else { p.reversed() });
            } else if let Some(k) = ps.iter().position(|p| near(p.b, start) || near(p.a, start)) {
                let p = ps.swap_remove(k);
                cur.insert(0, if near(p.b, start) { p } else { p.reversed() });
            } else {
                break;
            }
        }
        out.push(cur);
    }
    out
}

/// 列を path の `d` にする。`to` はスケッチの座標から紙の座標へ(y を反転する)。
fn path_d(list: &[Piece], tol: f64, to: &dyn Fn(P2) -> P2) -> String {
    let mut d = String::new();
    let p0 = to(list[0].a);
    let _ = write!(d, "M{} {}", num(p0[0]), num(p0[1]));
    let closed = list.len() > 1 && {
        let (a, b) = (list[0].a, list[list.len() - 1].b);
        (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
    };
    for (i, p) in list.iter().enumerate() {
        // 閉じた列の最後の線は Z で書く(円弧は終わりまで書いてから Z)
        let last = i + 1 == list.len();
        let q = to(if closed && last { list[0].a } else { p.b });
        match p.arc {
            None if closed && last => {}
            None => {
                let _ = write!(d, " L{} {}", num(q[0]), num(q[1]));
            }
            Some((r, sweep)) => {
                // y を反転するので、反時計回り(正の角)は SVG の負の向き(sweep-flag 0)になる
                let large = u8::from(sweep.abs() > PI);
                let flag = u8::from(sweep < 0.0);
                let _ = write!(
                    d,
                    " A{} {} 0 {large} {flag} {} {}",
                    num(r),
                    num(r),
                    num(q[0]),
                    num(q[1])
                );
            }
        }
    }
    if closed {
        d.push_str(" Z");
    }
    d
}

/// 区間の囲み(最小の角と最大の角)。円弧は膨らんだ所も含める。
fn bounds(segs: &[Segment]) -> Option<(P2, P2)> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut add = |p: P2| {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    };
    for s in segs {
        match *s {
            Segment::Line { a, b } => {
                add(a);
                add(b);
            }
            Segment::Circle { center, radius } => {
                add([center[0] - radius, center[1] - radius]);
                add([center[0] + radius, center[1] + radius]);
            }
            Segment::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sweep = arc_sweep(start, end);
                let at = |t: f64| [center[0] + radius * t.cos(), center[1] + radius * t.sin()];
                add(at(start));
                add(at(start + sweep));
                // 間に入る 0°・90°・180°・270°
                for k in 0..4 {
                    let t = f64::from(k) * PI / 2.0;
                    if (t - start).rem_euclid(TAU) <= sweep {
                        add(at(t));
                    }
                }
            }
        }
    }
    lo[0].is_finite().then_some((lo, hi))
}

/// 解いたスケッチを SVG の文字列にする(書く線が無ければ `None`)。ファイルに書くのは呼び出し側。
pub fn to_svg(sketch: &Sketch, sol: &Solution, opts: &SvgOptions) -> Option<String> {
    let construction: Vec<EntityId> = sketch
        .entities
        .iter()
        .filter(|e| e.is_construction())
        .map(|e| e.id())
        .collect();
    let segs: Vec<(EntityId, Segment)> = entity_segments(sketch, sol, opts.construction)
        .into_iter()
        .filter(|(_, s)| finite(s))
        .collect();
    let all: Vec<Segment> = segs.iter().map(|(_, s)| s.clone()).collect();
    let (lo, hi) = bounds(&all)?;
    let (w, h) = ((hi[0] - lo[0]).max(0.001), (hi[1] - lo[1]).max(0.001));
    let size = w.max(h).max(1.0);
    let tol = 1e-6 * size + 1e-6;
    let to = |p: P2| [p[0] - lo[0], hi[1] - p[1]];

    let mut s = kmb_format::svg::begin_mm(w, h);
    let _ = writeln!(s, "<title>{}</title>", esc(&opts.title));
    for (cons, color, id) in [
        (false, LINE_COLOR, "lines"),
        (true, CONSTRUCTION_COLOR, "construction"),
    ] {
        let mine: Vec<Segment> = segs
            .iter()
            .filter(|(e, _)| construction.contains(e) == cons)
            .map(|(_, s)| s.clone())
            .collect();
        if mine.is_empty() {
            continue;
        }
        let dash = if cons {
            " stroke-dasharray=\"1 0.5\""
        } else {
            ""
        };
        let _ = writeln!(
            s,
            "<g id=\"{id}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"0.1\"{dash}>"
        );
        let (ps, circles) = pieces(&mine);
        for list in chain(ps, tol) {
            let _ = writeln!(s, "<path d=\"{}\"/>", path_d(&list, tol, &to));
        }
        for (c, r) in circles {
            let q = to(c);
            let _ = writeln!(
                s,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                num(q[0]),
                num(q[1]),
                num(r)
            );
        }
        s.push_str("</g>\n");
    }
    s.push_str("</svg>\n");
    Some(s)
}

fn finite(s: &Segment) -> bool {
    match *s {
        Segment::Line { a, b } => a.iter().chain(&b).all(|v| v.is_finite()),
        Segment::Circle { center, radius } => {
            center.iter().all(|v| v.is_finite()) && radius.is_finite() && radius > 0.0
        }
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => {
            center.iter().all(|v| v.is_finite())
                && radius.is_finite()
                && radius > 0.0
                && start.is_finite()
                && end.is_finite()
        }
    }
}
