//! 今の視点から見たボディの線を SVG にする(線は [`crate::view_dxf::view_runs`] と同じ)。
//!
//! レーザー加工機のソフト(LightBurn など)にそのまま入れられる形にする:
//! - 大きさは `width`・`height` を mm で書き、`viewBox` も mm の値(1 単位 = 1 mm)。px で書くと
//!   読む側の dpi の決め方(96 か 72 か)で大きさが変わるため。
//! - 線は塗らず(`fill="none"`)に線だけ。LightBurn は線の色で層を分けるので、見える線は黒
//!   (層 00)、隠れ線は青(層 01)の決まった色にする。
//! - 端がつながる線は 1 本の `path` にまとめ、閉じた輪郭は `Z` で閉じる(閉じた形として扱われる)。
//! - y は上向きの図を、SVG の下向きの y に反転し、図の左下の角を紙の左下に置く。変換(`transform`)は使わない。

use crate::view_dxf::{View, runs_bounds, view_runs};
use jc_core::kernel::{Mesh, P3};
/// 数は小数 4 桁まで(0.1 µm より細かい差はレーザーでは意味が無い)
pub(crate) use jc_format::svg::num;
use std::fmt::Write as _;

/// 書き出しの結果(SVG の文字列と、見える線・隠れ線の `path` の本数)。
#[derive(Debug, Clone, PartialEq)]
pub struct ViewSvg {
    pub svg: String,
    pub visible: usize,
    pub hidden: usize,
}

/// 見える線の色(LightBurn の層 00)。
pub const VISIBLE_COLOR: &str = "#000000";
/// 隠れ線の色(LightBurn の層 01)。
pub const HIDDEN_COLOR: &str = "#0000FF";

/// 端のつながる折れ線を、つなげて長い折れ線にする(向きは必要なら逆にする)。`tol` は同じ点とみなす距離。
pub fn chain(mut polys: Vec<Vec<[f64; 2]>>, tol: f64) -> Vec<Vec<[f64; 2]>> {
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol;
    polys.retain(|p| p.len() >= 2);
    let mut out = Vec::new();
    while let Some(mut cur) = polys.pop() {
        loop {
            let (first, last) = (cur[0], cur[cur.len() - 1]);
            if cur.len() > 2 && near(first, last) {
                break;
            }
            let Some(k) = polys.iter().position(|p| {
                near(p[0], last)
                    || near(p[p.len() - 1], last)
                    || near(p[0], first)
                    || near(p[p.len() - 1], first)
            }) else {
                break;
            };
            let mut p = polys.swap_remove(k);
            if near(p[0], last) {
                cur.extend(p.drain(1..));
            } else if near(p[p.len() - 1], last) {
                p.reverse();
                cur.extend(p.drain(1..));
            } else {
                if near(p[0], first) {
                    p.reverse();
                }
                // p の終わりが cur の始まりにつながる
                p.pop();
                p.extend(cur);
                cur = p;
            }
        }
        out.push(cur);
    }
    out
}

/// 折れ線を線分に分け、同じ直線の上で重なる線分を 1 本にまとめる(箱を上から見ると、上の面と下の面の
/// 縁が同じ所に投影される。レーザーで同じ所を 2 度焼かないため)。`tol` は同じ点とみなす距離。
pub fn merge_overlaps(polys: &[Vec<[f64; 2]>], tol: f64) -> Vec<Vec<[f64; 2]>> {
    use std::collections::BTreeMap;
    // 直線(向きの角と原点からの距離)ごとの、線分の範囲(向きに沿った位置)
    type Line = ([f64; 2], f64, Vec<(f64, f64)>);
    let mut lines: BTreeMap<(i64, i64), Line> = BTreeMap::new();
    for p in polys {
        for w in p.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            if len <= tol {
                continue;
            }
            let mut d = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
            // 向きをそろえる(角は 0 以上 π 未満)
            if d[1] < -1e-12 || (d[1].abs() <= 1e-12 && d[0] < 0.0) {
                d = [-d[0], -d[1]];
            }
            let angle = d[1].atan2(d[0]);
            let c = d[0] * a[1] - d[1] * a[0];
            let key = (
                (angle * 1e6).round() as i64,
                (c / tol.max(1e-9)).round() as i64,
            );
            let e = lines.entry(key).or_insert((d, c, Vec::new()));
            let (t0, t1) = (e.0[0] * a[0] + e.0[1] * a[1], e.0[0] * b[0] + e.0[1] * b[1]);
            e.2.push((t0.min(t1), t0.max(t1)));
        }
    }
    let mut out = Vec::new();
    for (d, c, mut spans) in lines.into_values() {
        spans.sort_by(|x, y| x.0.total_cmp(&y.0));
        // 直線の上の点: 向き d に沿った位置 t と、法線(-d.y, d.x)に沿った距離 c
        let at = |t: f64| [d[0] * t - d[1] * c, d[1] * t + d[0] * c];
        let mut cur = spans[0];
        for s in &spans[1..] {
            if s.0 <= cur.1 + tol {
                cur.1 = cur.1.max(s.1);
            } else {
                out.push(vec![at(cur.0), at(cur.1)]);
                cur = *s;
            }
        }
        out.push(vec![at(cur.0), at(cur.1)]);
    }
    out
}

/// 折れ線を path の `d` にする(閉じていれば `Z`)。`to` は図の座標から紙の座標へ。
pub(crate) fn path_d(poly: &[[f64; 2]], tol: f64, to: impl Fn([f64; 2]) -> [f64; 2]) -> String {
    let closed = poly.len() > 2 && {
        let (a, b) = (poly[0], poly[poly.len() - 1]);
        (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
    };
    let pts = if closed {
        &poly[..poly.len() - 1]
    } else {
        poly
    };
    let mut d = String::new();
    for (i, p) in pts.iter().enumerate() {
        let q = to(*p);
        let _ = write!(
            d,
            "{}{} {}",
            if i == 0 { "M" } else { " L" },
            num(q[0]),
            num(q[1])
        );
    }
    if closed {
        d.push_str(" Z");
    }
    d
}

/// SVG の頭(大きさは mm)。`w`・`h` は図の幅と高さ mm。
pub(crate) fn header(w: f64, h: f64, title: &str) -> String {
    // 幅か高さが 0 の図(1 本の横線など)でも読めるように、少しだけ大きさを持たせる
    let (w, h) = (w.max(0.001), h.max(0.001));
    let mut s = jc_format::svg::begin_mm(w, h);
    let _ = writeln!(s, "<title>{title}</title>");
    s
}

/// 網 `meshes` とスケッチの線 `sketches` を視点 `view` から見た線を SVG にする。`hidden` が真なら
/// 隠れ線も青の破線で書く。`chord` は網にしたときの許容差 mm。
pub fn view_to_svg(
    meshes: &[Mesh],
    sketches: &[Vec<P3>],
    view: &View,
    hidden: bool,
    chord: f64,
) -> ViewSvg {
    let runs = view_runs(meshes, sketches, view, chord);
    let (lo, hi) = runs_bounds(&runs);
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
    let tol = 1e-6 * size + 1e-5;
    let to = |p: [f64; 2]| [p[0] - lo[0], hi[1] - p[1]];
    let mut s = header(hi[0] - lo[0], hi[1] - lo[1], "kieMonoBench の視点の線");
    let mut counts = [0usize; 2];
    for (seen, color, id) in [
        (true, VISIBLE_COLOR, "visible"),
        (false, HIDDEN_COLOR, "hidden"),
    ] {
        if !seen && !hidden {
            continue;
        }
        let polys: Vec<_> = runs
            .iter()
            .filter(|r| r.1 == seen)
            .map(|r| r.0.clone())
            .collect();
        let polys = chain(merge_overlaps(&polys, tol), tol);
        if polys.is_empty() {
            continue;
        }
        let dash = if seen {
            ""
        } else {
            " stroke-dasharray=\"1 0.5\""
        };
        let _ = writeln!(
            s,
            "<g id=\"{id}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"0.1\"{dash}>"
        );
        for p in &polys {
            let _ = writeln!(s, "<path d=\"{}\"/>", path_d(p, tol, to));
        }
        s.push_str("</g>\n");
        counts[usize::from(!seen)] = polys.len();
    }
    s.push_str("</svg>\n");
    ViewSvg {
        svg: s,
        visible: counts[0],
        hidden: counts[1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_lines_are_chained_into_one_closed_outline_in_either_direction() {
        // 正方形の 4 辺を、ばらばらの向きで
        let sq = vec![
            vec![[0.0, 0.0], [10.0, 0.0]],
            vec![[10.0, 10.0], [10.0, 0.0]],
            vec![[10.0, 10.0], [0.0, 10.0]],
            vec![[0.0, 0.0], [0.0, 10.0]],
            // 離れた線は別
            vec![[20.0, 0.0], [30.0, 0.0]],
        ];
        let out = chain(sq, 1e-6);
        assert_eq!(out.len(), 2);
        let ring = out.iter().find(|p| p.len() == 5).expect("つながった輪");
        assert_eq!(ring[0], ring[4]);
        assert!(path_d(ring, 1e-6, |p| p).ends_with(" Z"));
    }

    #[test]
    fn lines_on_top_of_each_other_are_cut_only_once() {
        // 同じ辺が 2 回(逆向きも)、一部だけ重なる線、続く線
        let polys = vec![
            vec![[0.0, 0.0], [10.0, 0.0]],
            vec![[10.0, 0.0], [0.0, 0.0]],
            vec![[5.0, 0.0], [15.0, 0.0]],
            vec![[0.0, 5.0], [3.0, 5.0], [6.0, 5.0]],
            vec![[1.0, 1.0], [2.0, 2.0]],
        ];
        let mut out = merge_overlaps(&polys, 1e-6);
        out.sort_by(|a, b| a[0][1].total_cmp(&b[0][1]));
        assert_eq!(out.len(), 3, "{out:?}");
        let close =
            |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9;
        assert!(
            close(out[0][0], [0.0, 0.0]) && close(out[0][1], [15.0, 0.0]),
            "{out:?}"
        );
        assert!(
            close(out[2][0], [0.0, 5.0]) && close(out[2][1], [6.0, 5.0]),
            "{out:?}"
        );
    }

    #[test]
    fn a_box_from_the_top_is_one_closed_black_path_sized_in_mm() {
        let m = crate::view_dxf::tests::boxed([0.0; 3], [40.0, 20.0, 5.0]);
        let r = view_to_svg(&[m], &[], &View::named("top").unwrap(), false, 0.01);
        assert!(
            r.svg
                .contains("width=\"40mm\" height=\"20mm\" viewBox=\"0 0 40 20\""),
            "{}",
            r.svg
        );
        assert_eq!(r.visible, 1, "{}", r.svg);
        assert_eq!(r.hidden, 0);
        assert!(r.svg.contains("stroke=\"#000000\""));
        assert!(r.svg.contains(" Z\"/>"));
        assert!(!r.svg.contains("transform"));
    }

    #[test]
    fn hidden_lines_go_to_the_blue_group_only_when_asked() {
        let m = crate::view_dxf::tests::boxed([0.0; 3], [10.0, 10.0, 10.0]);
        let v = View::named("iso").unwrap();
        let without = view_to_svg(std::slice::from_ref(&m), &[], &v, false, 0.01);
        let with = view_to_svg(&[m], &[], &v, true, 0.01);
        assert!(!without.svg.contains(HIDDEN_COLOR));
        assert!(with.hidden > 0);
        assert!(
            with.svg
                .contains("stroke=\"#0000FF\" stroke-width=\"0.1\" stroke-dasharray")
        );
    }
}
