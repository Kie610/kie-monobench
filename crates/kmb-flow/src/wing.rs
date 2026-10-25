//! 描いた翼のボディから、渦格子法の揚力面を作る。翼幅の向き(y)に断面を切り、断面ごとに
//! 前縁(x の最小)・後縁(x の最大)と、上の面と下の面の真ん中(反りの面)の高さを読む。
//! 反り・ねじり・上反角・後退角は、そのまま面の形に入る。座標は流れの座標の mm。

use crate::geom::{P3, Shape};
use crate::vlm::Surface;

/// 断面 y = const で切った線分(x, z)。
fn section(shape: &Shape, y: f64) -> Vec<[[f64; 2]; 2]> {
    let mut out = Vec::new();
    for t in &shape.tris {
        let mut pts = Vec::with_capacity(2);
        for e in 0..3 {
            let (p, q) = (t[e], t[(e + 1) % 3]);
            if (p[1] - y) * (q[1] - y) < 0.0 {
                let s = (y - p[1]) / (q[1] - p[1]);
                pts.push([p[0] + s * (q[0] - p[0]), p[2] + s * (q[2] - p[2])]);
            }
        }
        if pts.len() == 2 {
            out.push([pts[0], pts[1]]);
        }
    }
    out
}

/// 断面の前縁から後縁までの `nc + 1` 個の点(反りの面の上、mm)。
fn chord_points(seg: &[[[f64; 2]; 2]], y: f64, nc: usize) -> Option<Vec<P3>> {
    let mut le = [f64::MAX, 0.0];
    let mut te = [f64::MIN, 0.0];
    for s in seg {
        for p in s {
            if p[0] < le[0] {
                le = *p;
            }
            if p[0] > te[0] {
                te = *p;
            }
        }
    }
    if te[0] <= le[0] || te[0].is_nan() {
        return None;
    }
    // 前縁・後縁が縦の面(板の端)なら、その面の高さの真ん中
    let eps = 1e-6 * (te[0] - le[0]);
    let mid_at = |x: f64| {
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for s in seg {
            for p in s {
                if (p[0] - x).abs() <= eps {
                    lo = lo.min(p[1]);
                    hi = hi.max(p[1]);
                }
            }
        }
        0.5 * (lo + hi)
    };
    le[1] = mid_at(le[0]);
    te[1] = mid_at(te[0]);
    let mut out = Vec::with_capacity(nc + 1);
    for m in 0..=nc {
        let x = le[0] + (te[0] - le[0]) * m as f64 / nc as f64;
        let z = if m == 0 {
            le[1]
        } else if m == nc {
            te[1]
        } else {
            let (mut lo, mut hi) = (f64::MAX, f64::MIN);
            for s in seg {
                let (a, b) = (s[0], s[1]);
                if (a[0] - x) * (b[0] - x) <= 0.0 && (a[0] - b[0]).abs() > 1e-12 {
                    let z = a[1] + (x - a[0]) / (b[0] - a[0]) * (b[1] - a[1]);
                    lo = lo.min(z);
                    hi = hi.max(z);
                }
            }
            if lo > hi {
                le[1] + (te[1] - le[1]) * m as f64 / nc as f64
            } else {
                0.5 * (lo + hi)
            }
        };
        out.push([x, y, z]);
    }
    Some(out)
}

/// 翼のボディの形から揚力面(m)。`ns` は翼幅の帯の数、`nc` は翼弦の板の数。
pub fn surface(shape: &Shape, name: &str, ns: usize, nc: usize) -> Result<Surface, String> {
    let (y0, y1) = (shape.lo[1], shape.hi[1]);
    let span = y1 - y0;
    if span <= 0.0 {
        return Err(format!("{name} に翼幅がありません"));
    }
    let mut points = Vec::with_capacity(ns + 1);
    // 端の帯の幅。端の断面は、隣の断面を越えない範囲で内へずらして探す
    let gap = span * 0.5 * (1.0 - (std::f64::consts::PI / ns as f64).cos());
    for k in 0..=ns {
        let t = k as f64 / ns as f64;
        // 端を細かく(余弦の間隔)。端そのものは端の面の上で切れないので少し内へ
        let inward = if k == 0 { 1.0 } else { -1.0 };
        let mut y = y0 + span * 0.5 * (1.0 - (std::f64::consts::PI * t).cos());
        if k == 0 || k == ns {
            y += inward * gap * 0.01;
        }
        let mut tries = 0;
        let row = loop {
            if let Some(r) = chord_points(&section(shape, y), y, nc) {
                break r;
            }
            tries += 1;
            if tries > 10 || k > 0 && k < ns {
                return Err(format!(
                    "{name} の翼幅の {:.1} mm の所で断面が切れません(翼は流れの向きに直角に、横へ伸ばしてください)",
                    y - y0
                ));
            }
            y += inward * gap * 0.05;
        };
        points.push(row.into_iter().map(|p| p.map(|v| v / 1000.0)).collect());
    }
    Ok(Surface {
        name: name.into(),
        points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Frame;
    use crate::geom::tests::box_mesh;

    #[test]
    fn a_flat_plate_becomes_a_rectangle_of_points() {
        // 翼弦 200 mm(X)、翼幅 1000 mm(Y)、厚さ 4 mm
        let m = box_mesh([100.0, 0.0, 10.0], [200.0, 1000.0, 4.0]);
        let s = Shape::new(&[&m], &Frame::new(0, 1.0, 2)).unwrap();
        let w = surface(&s, "翼", 40, 4).unwrap();
        assert_eq!(w.points.len(), 41);
        let (area, mac) = w.area_and_mac();
        assert!((area - 0.2).abs() < 1e-3, "面積 {area}");
        assert!((mac - 0.2).abs() < 1e-3);
        // 端の帯も幅が正(断面が隣を越えない)
        assert!(w.points.windows(2).all(|p| p[1][0][1] > p[0][0][1]));
        // 反りの面は厚さの真ん中
        assert!(w.points[20].iter().all(|p| (p[2] - 0.010).abs() < 1e-9));
    }
}
