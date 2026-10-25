//! へこんだ辺の柱が、足元の穴やポケットの上を埋めないようにする。

use super::*;

/// 埋める柱の足元を調べる板の厚さ(mm)。面の下のこの厚さに穴やポケットがあれば、その上は埋めない
const SLAB: f64 = 0.2;
/// 板を面からわずかに沈める量(mm)。面と同じ平面に板の面を置くとブーリアンが失敗するため
const SINK: f64 = 0.001;
/// 板を柱の幅より両側へ広げる量(mm)
const MARGIN: f64 = 0.5;
/// 足元の空きの輪郭を網から取る時の、面からのずれの上限(mm)
pub(super) const OUTLINE_TOL: f64 = 0.002;
/// 空きの輪郭を外へ広げる量(mm)。輪郭は折れ線なので、穴の壁と重ねずに少し材料の側へ置く
/// (同じ面で接するとブーリアンが失敗するため)。丸めはここで穴の縁からこの幅だけ手前で止まる
const OUTLINE_GROW: f64 = 0.01;

/// へこんだ辺の、面(外向きの法線 `n`、相手の面の法線 `other`)の側の足元が空いている所
/// (穴やポケット)の上を、柱から除くための道具。`width` は柱が面に乗る幅。足元が詰まって
/// いるか、調べる途中で失敗した時は `None`(柱のまま埋める)。
///
/// 面のすぐ下の薄い板とボディの差(足元の空き)を作り、その上面の輪郭を網から取り出して、
/// 面の法線の向きへ柱の高さまで押し出す。カーネルに形を伸び縮みさせる移動がないため、
/// 輪郭は折れ線で近似し、`OUTLINE_GROW` だけ外へ広げる。
#[allow(clippy::too_many_arguments)]
pub(super) fn shadow(
    kernel: &dyn k::Kernel,
    body: k::ShapeHandle,
    e: &EdgeInfo,
    n: P3,
    other: P3,
    width: f64,
    start: f64,
    end: f64,
    tag: &str,
) -> Option<k::ShapeHandle> {
    let d = k::normalize(k::sub(e.b, e.a));
    let v = k::cross(d, n);
    // 面の上を辺から離れる向き(へこんだ辺では相手の面の法線の側)
    let (v0, v1) = if k::dot(v, other) > 0.0 {
        (-MARGIN, width + MARGIN)
    } else {
        (-width - MARGIN, MARGIN)
    };
    let (u0, u1) = (-SLAB, -SINK);
    let lp = ProfileLoop {
        edges: vec![
            line("s1", [u0, v0], [u1, v0]),
            line("s2", [u1, v0], [u1, v1]),
            line("s3", [u1, v1], [u0, v1]),
            line("s4", [u0, v1], [u0, v0]),
        ],
    };
    let profile = Profile {
        plane: Plane {
            origin: e.a,
            x_axis: n,
            y_axis: v,
        },
        regions: vec![Region {
            outer: lp,
            holes: vec![],
        }],
    };
    let slab = kernel.extrude(&profile, start, end, tag).ok()?;
    let full = kernel.volume(slab).ok()?;
    // truck は面が近いとブーリアンに失敗する。その時は調べずに埋める
    let solid = kernel.boolean(BooleanOp::Intersect, slab, body).ok()?;
    if full - kernel.volume(solid).ok()? < 1e-6 * full.max(1.0) {
        return None;
    }
    let gap = kernel.boolean(BooleanOp::Subtract, slab, body).ok()?;
    // 空きの上面(u = -SINK)の輪郭を、面の平面の 2 次元(x = v、y = n × v)で取り出す
    let plane = Plane {
        origin: k::add(e.a, k::scale(n, -SINK)),
        x_axis: v,
        y_axis: k::cross(n, v),
    };
    let loops = top_outline(&kernel.mesh(gap, OUTLINE_TOL).ok()?, &plane, n)?;
    let mut regions = Vec::new();
    let mut holes = Vec::new();
    for pts in loops {
        // 向きは広げる前に決める(小さな島は縮めると裏返るため)
        let outer = k::polygon_area(&pts) > 0.0;
        let grown = grow(&pts, OUTLINE_GROW);
        let lp = ProfileLoop {
            edges: (0..grown.len())
                .map(|i| line(&format!("g{i}"), grown[i], grown[(i + 1) % grown.len()]))
                .collect(),
        };
        if outer {
            regions.push(Region {
                outer: lp,
                holes: vec![],
            });
        } else if lp.signed_area() < 0.0 {
            // 空きの中の材料の島(右回りのまま穴にする)。縮めて消えた島は捨てる
            holes.push(lp);
        }
    }
    // 島は、それを囲む外側の輪郭に入れる
    for h in holes {
        let p = h.polyline()[0];
        if let Some(r) = regions.iter_mut().find(|r| inside(&r.outer.polyline(), p)) {
            r.holes.push(h);
        }
    }
    if regions.is_empty() {
        return None;
    }
    // 柱の点は辺から `width` より遠くない
    kernel
        .extrude(&Profile { plane, regions }, SINK - SLAB, width + 1.0, tag)
        .ok()
}

/// 網のうち、法線が `n` で平面 `plane` の上にある三角形の集まりの輪郭(左回りが外側、
/// 右回りが島)。三角形は網の元の向きのまま使い、元の面ごとに `n` から見て左回りへそろえる
/// (多角形を扇形に分けた網では、向きの逆な三角形が打ち消し合う)。輪郭が閉じなければ `None`。
fn top_outline(mesh: &k::Mesh, plane: &Plane, n: P3) -> Option<Vec<Vec<[f64; 2]>>> {
    use std::collections::HashMap;
    let to2 = |p: [f32; 3]| -> ([f64; 2], f64) {
        let q = k::sub([p[0] as f64, p[1] as f64, p[2] as f64], plane.origin);
        (
            [k::dot(q, plane.x_axis), k::dot(q, plane.y_axis)],
            k::dot(q, n),
        )
    };
    // 同じ位置の頂点を 1 つにまとめる(面ごとに頂点を持つ網もある)
    let key = |p: [f64; 2]| ((p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64);
    let mut ids: HashMap<(i64, i64), usize> = HashMap::new();
    let mut pts: Vec<[f64; 2]> = Vec::new();
    // 元の面ごとの、向き付きの辺と符号付きの面積
    let mut by_face: HashMap<u32, (Vec<(usize, usize)>, f64)> = HashMap::new();
    for (t, &f) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        let c: Vec<([f64; 2], f64)> = t.iter().map(|&i| to2(mesh.positions[i as usize])).collect();
        if c.iter().any(|(_, h)| h.abs() > 1e-4) {
            continue;
        }
        let (a, b, cc) = (c[0].0, c[1].0, c[2].0);
        let area = (b[0] - a[0]) * (cc[1] - a[1]) - (b[1] - a[1]) * (cc[0] - a[0]);
        let mut id = |p: [f64; 2]| -> usize {
            *ids.entry(key(p)).or_insert_with(|| {
                pts.push(p);
                pts.len() - 1
            })
        };
        let (ia, ib, ic) = (id(a), id(b), id(cc));
        let g = by_face.entry(f).or_default();
        g.0.extend([(ia, ib), (ib, ic), (ic, ia)]);
        g.1 += area;
    }
    // 向き付きの辺を数え、逆向きの辺と打ち消し合って残ったものが輪郭
    let mut count: HashMap<(usize, usize), i32> = HashMap::new();
    for (edges, area) in by_face.into_values() {
        for (a, b) in edges {
            let (a, b) = if area >= 0.0 { (a, b) } else { (b, a) };
            *count.entry((a, b)).or_default() += 1;
            *count.entry((b, a)).or_default() -= 1;
        }
    }
    let mut next: HashMap<usize, usize> = HashMap::new();
    for (&(a, b), &c) in &count {
        // 同じ点から 2 本出る(8 の字)か、重なった三角形がある
        if c > 0 && (c != 1 || next.insert(a, b).is_some()) {
            return None;
        }
    }
    let mut loops = Vec::new();
    while let Some(&s) = next.keys().next() {
        let mut lp = vec![pts[s]];
        let mut cur = next.remove(&s)?;
        while cur != s {
            lp.push(pts[cur]);
            cur = next.remove(&cur)?;
        }
        if lp.len() >= 3 {
            loops.push(lp);
        }
    }
    Some(loops)
}

/// 閉じた折れ線を、囲む向き(左回りなら外へ、右回りなら内へ)に `delta` だけ広げる。
fn grow(pts: &[[f64; 2]], delta: f64) -> Vec<[f64; 2]> {
    let m = pts.len();
    let mut out = Vec::with_capacity(m);
    for i in 0..m {
        let (p0, p1, p2) = (pts[(i + m - 1) % m], pts[i], pts[(i + 1) % m]);
        let nrm = |a: [f64; 2], b: [f64; 2]| {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = (dx * dx + dy * dy).sqrt().max(1e-12);
            [dy / l, -dx / l]
        };
        let (n1, n2) = (nrm(p0, p1), nrm(p1, p2));
        let bis = [n1[0] + n2[0], n1[1] + n2[1]];
        let c = (bis[0] * n1[0] + bis[1] * n1[1]).max(0.2);
        out.push([p1[0] + bis[0] * delta / c, p1[1] + bis[1] * delta / c]);
    }
    out
}

/// 点が多角形の中にあるか(偶奇の数え方)。
fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    let m = poly.len();
    for i in 0..m {
        let (a, b) = (poly[i], poly[(i + 1) % m]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if x > p[0] {
                odd = !odd;
            }
        }
    }
    odd
}

/// 辺の柱を、辺の端 `p` から向き `dir` へどれだけ延ばすか。
///
/// 端が外向きの面(法線が `dir` 側)に乗っているとき: 出っ張った辺なら先は空なので
/// 大きく延ばし、へこんだ辺ならその面の手前で止める。
/// そうでないとき(端の先に材料が続く角): へこんだ辺は隣の壁の中身へ柱の大きさの 2 倍まで
/// 延ばす。出っ張った辺(穴やポケットの縁の角)は柱の大きさだけ延ばす。隣の辺の柱と
/// 合わせると、2 つの面取り面(丸め面)が角で交わる形になり、それより先へ延ばすと
/// 上面に溝を彫ってしまう。
pub(super) fn reach(faces: &[FaceInfo], e: &EdgeInfo, p: P3, dir: P3, size: f64) -> f64 {
    match (e.convex, outward(faces, e, p, dir)) {
        (true, true) => EXTEND,
        (true, false) => size.min(EXTEND),
        (false, true) => -INSET,
        (false, false) => (2.0 * size).min(EXTEND),
    }
}

/// 辺の端 `p` が、向き `dir` を外へ向いた平面(辺の両側の面でない)に乗っているか。
fn outward(faces: &[FaceInfo], e: &EdgeInfo, p: P3, dir: P3) -> bool {
    faces.iter().any(|f| {
        !e.faces.contains(&f.name)
            && f.planar.is_some_and(|pl| {
                k::dot(k::sub(p, pl.point), pl.normal).abs() < 1e-6 && k::dot(pl.normal, dir) > 0.5
            })
    })
}

/// 出っ張った辺の端の先で、材料が角から離れていく(先に掛けた丸めや面取りで下がっていく)とき、
/// 柱をその先の材料を削り終えるまで延ばす長さ。端の先に材料が続く角(ポケットの縁の角など)
/// なら `None`。
///
/// 辺を端 `p` から向き `dir` へ延ばした線を、角の内側へわずかに入れて網の中か調べる。柱の
/// 大きさの 1.5 倍先で外なら、角から離れていく。そこから先へ、柱の断面の点(角と、両側の面の
/// 上で辺から `width` までの点)がすべて材料の外になる所まで延ばす。線が一度外へ出てから材料に
/// 戻ったら(別の出っ張りなど)、その手前で止める。大きさだけ延ばして止めると、丸めの途中に
/// 削り残しの三角ができていた。
pub(super) fn run_out(
    mesh: &k::Mesh,
    p: P3,
    dir: P3,
    na: P3,
    nb: P3,
    width: f64,
    size: f64,
) -> Option<f64> {
    let bis = k::normalize(k::add(na, nb));
    // 面の上を辺から離れる向き(出っ張った辺では相手の面の法線の逆)
    let away = |other: P3, n: P3| {
        let t = k::sub(other, k::scale(n, k::dot(other, n)));
        k::scale(k::normalize(k::sub(t, k::scale(dir, k::dot(t, dir)))), -1.0)
    };
    let (wa, wb) = (away(nb, na), away(na, nb));
    let at = |t: f64, off: P3| k::add(p, k::add(k::scale(dir, t), off));
    let corner = k::scale(bis, -RUN_OUT_PROBE);
    if in_mesh(mesh, at(1.5 * size, corner)) {
        return None;
    }
    let mut section = vec![corner];
    for f in [0.5, 1.0] {
        section.push(k::add(
            k::scale(wa, f * width),
            k::scale(na, -RUN_OUT_PROBE),
        ));
        section.push(k::add(
            k::scale(wb, f * width),
            k::scale(nb, -RUN_OUT_PROBE),
        ));
    }
    // 形の外へ出るまでの長さ(網の点を延ばす向きへ測った最大)
    let far = mesh
        .positions
        .iter()
        .map(|v| k::dot(k::sub([v[0] as f64, v[1] as f64, v[2] as f64], p), dir))
        .fold(0.0, f64::max);
    let step = (size / 4.0).max(0.02);
    let mut left = false;
    let mut t = step;
    while t < far + step {
        let corner_in = in_mesh(mesh, at(t, corner));
        if left && corner_in {
            // 材料に戻る所は削らない
            return Some((t - step).max(size));
        }
        left |= !corner_in && t >= 1.5 * size;
        if section.iter().all(|o| !in_mesh(mesh, at(t, *o))) {
            return Some(t + step);
        }
        t += step;
    }
    Some(far + EXTEND)
}

/// 点が網の囲む中にあるか(半直線と三角形の交わりの数の偶奇)。
fn in_mesh(mesh: &k::Mesh, q: P3) -> bool {
    // 三角形の辺や頂点にちょうど当たりにくい、半端な向き
    let ray = k::normalize([0.5773, 0.5821, 0.5717]);
    let pos = |i: u32| {
        let v = mesh.positions[i as usize];
        [v[0] as f64, v[1] as f64, v[2] as f64]
    };
    let mut odd = false;
    for t in &mesh.triangles {
        let (a, b, c) = (pos(t[0]), pos(t[1]), pos(t[2]));
        let (e1, e2) = (k::sub(b, a), k::sub(c, a));
        let h = k::cross(ray, e2);
        let det = k::dot(e1, h);
        if det.abs() < 1e-14 {
            continue;
        }
        let s = k::sub(q, a);
        let u = k::dot(s, h) / det;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let qv = k::cross(s, e1);
        let v = k::dot(ray, qv) / det;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        if k::dot(e2, qv) / det > 0.0 {
            odd = !odd;
        }
    }
    odd
}
