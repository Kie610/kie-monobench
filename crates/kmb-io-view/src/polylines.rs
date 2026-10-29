//! 投影した線を、見える線と隠れ線を混ぜずに整理する。

/// 図の大きさに合わせた、端と直線を同じ所とみなす距離(mm)。
pub(crate) fn tolerance(lo: [f64; 2], hi: [f64; 2]) -> f64 {
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
    1e-6 * size + 1e-5
}

/// 同じ見え方の線だけを集め、重なりを除いてつながる端をまとめる。
pub(crate) fn clean(runs: &[(Vec<[f64; 2]>, bool)], seen: bool, tol: f64) -> Vec<Vec<[f64; 2]>> {
    let polys: Vec<_> = runs
        .iter()
        .filter(|r| r.1 == seen)
        .map(|r| r.0.clone())
        .collect();
    chain(merge_overlaps(&polys, tol), tol)
}

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
