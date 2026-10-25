//! ソリッドの切れ([`Part`])どうしのブーリアン。truck には殻 1 つのソリッドどうしだけを渡し、
//! 空洞は分けて持って後から引く。結果の殻は、つながりごとの切れに分けて面の名前を入力から引き継ぐ。

use crate::{
    COARSE_TOLERANCE, Part, bounds, face_meshes, faces_of, pt, representative_points,
    signed_volume, surface_key, t_and, t_or, triangles,
};
use kmb_core::kernel::{self as k, KernelError, P3, dedupe_names};
use std::collections::HashMap;
use std::sync::Arc;
use truck_meshalgo::prelude::*;
use truck_modeling as tm;

impl Part {
    /// 向きを外向きに揃え、外形の箱を求める。
    pub(crate) fn new(solid: tm::Solid, names: Vec<String>) -> Part {
        let meshes = face_meshes(&solid, COARSE_TOLERANCE);
        Part::with_meshes(solid, names, meshes)
    }

    /// 求めておいた網(`COARSE_TOLERANCE`)を使って作る。
    pub(crate) fn with_meshes(
        mut solid: tm::Solid,
        names: Vec<String>,
        mut meshes: Vec<Option<PolygonMesh>>,
    ) -> Part {
        let mut volume = signed_volume(&meshes);
        if volume < 0.0 {
            solid.not();
            meshes = face_meshes(&solid, COARSE_TOLERANCE);
            volume = -volume;
        }
        let (mut min, mut max) = bounds(&meshes);
        for i in 0..3 {
            min[i] -= COARSE_TOLERANCE;
            max[i] += COARSE_TOLERANCE;
        }
        Part {
            solid,
            names,
            min,
            max,
            volume,
            meshes: Arc::new(meshes),
        }
    }

    pub(crate) fn overlaps(&self, other: &Part) -> bool {
        (0..3).all(|i| self.min[i] <= other.max[i] && other.min[i] <= self.max[i])
    }
}

/// ブーリアンの結果の殻を、中身のある殻(外側の殻)と空洞(内側の殻)に分け、面の名前を入力から
/// 引き継ぐ。どちらも殻 1 つのソリッドで、空洞は中身のある向き(空洞そのものを中身とする)にする。
pub(crate) fn shells_of(result: tm::Solid, sources: &[&Part]) -> (Vec<Part>, Vec<Part>) {
    // 曲面ごとの元の面(名前と網)。同じ平面の面がいくつもあることがある(前に分かれた面の切れ端)
    let mut by_key: HashMap<String, Vec<(&String, Option<&PolygonMesh>)>> = HashMap::new();
    for src in sources {
        for ((f, n), m) in faces_of(&src.solid)
            .iter()
            .zip(&src.names)
            .zip(src.meshes.iter())
        {
            by_key
                .entry(surface_key(f))
                .or_default()
                .push((n, m.as_ref()));
        }
    }
    let mut outer = Vec::new();
    let mut voids = Vec::new();
    for shell in result.boundaries() {
        let Ok(solid) = tm::Solid::try_new(vec![shell.clone()]) else {
            continue;
        };
        let faces = faces_of(&solid);
        let meshes = face_meshes(&solid, COARSE_TOLERANCE);
        let reps = representative_points(&meshes);
        let names: Vec<String> = faces
            .iter()
            .zip(&reps)
            .map(
                |(f, &p)| match by_key.get(&surface_key(f)).map(Vec::as_slice) {
                    None | Some([]) => "unnamed".into(),
                    Some([(n, _)]) => (*n).clone(),
                    // 同じ平面の元の面が複数: 結果の面の代表点を含む元の面の名前を継ぐ(SMLib が面の
                    // 印で継ぐのと同じ結果)。以前は先に見つけた名前にしたので、殻の並び(実行ごとに
                    // 変わる)で切れ端の名前が入れ替わった
                    Some(all) => all
                        .iter()
                        .min_by(|a, b| {
                            mesh_distance(a.1, p)
                                .total_cmp(&mesh_distance(b.1, p))
                                .then_with(|| a.0.cmp(b.0))
                        })
                        .map(|(n, _)| (*n).clone())
                        .unwrap_or_else(|| "unnamed".into()),
                },
            )
            .collect();
        let names = dedupe_names(names, &reps);
        if signed_volume(&meshes) >= 0.0 {
            outer.push(Part::with_meshes(solid, names, meshes));
        } else {
            voids.push(Part::with_meshes(solid, names, meshes));
        }
    }
    (outer, voids)
}

/// 点 `p` から面の網 `m` までの距離(網が無ければ無限大)。
pub(crate) fn mesh_distance(m: Option<&PolygonMesh>, p: P3) -> f64 {
    m.map_or(f64::INFINITY, |m| {
        triangles(m)
            .map(|t| triangle_distance(&t, p))
            .fold(f64::INFINITY, f64::min)
    })
}

/// 点 `p` から三角形 `t` までの距離(Ericson「Real-Time Collision Detection」5.1.5 の最近点)。
pub(crate) fn triangle_distance(t: &[P3; 3], p: P3) -> f64 {
    let [a, b, c] = *t;
    let (ab, ac, ap) = (k::sub(b, a), k::sub(c, a), k::sub(p, a));
    let (d1, d2) = (k::dot(ab, ap), k::dot(ac, ap));
    let near = if d1 <= 0.0 && d2 <= 0.0 {
        a
    } else {
        let bp = k::sub(p, b);
        let (d3, d4) = (k::dot(ab, bp), k::dot(ac, bp));
        let cp = k::sub(p, c);
        let (d5, d6) = (k::dot(ab, cp), k::dot(ac, cp));
        let (vc, vb, va) = (d1 * d4 - d3 * d2, d5 * d2 - d1 * d6, d3 * d6 - d5 * d4);
        if d3 >= 0.0 && d4 <= d3 {
            b
        } else if d6 >= 0.0 && d5 <= d6 {
            c
        } else if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            k::add(a, k::scale(ab, d1 / (d1 - d3)))
        } else if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            k::add(a, k::scale(ac, d2 / (d2 - d6)))
        } else if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
            k::add(
                b,
                k::scale(k::sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))),
            )
        } else {
            let den = 1.0 / (va + vb + vc);
            k::add(a, k::add(k::scale(ab, vb * den), k::scale(ac, vc * den)))
        }
    };
    let d = k::sub(p, near);
    k::dot(d, d).sqrt()
}

/// 光線の向き(長さは 1 でなくてよい)。軸や 45 度に沿わない向きにして、網の辺や頂点をかすめにくくする。
const RAY: [f64; 3] = [0.334_1, 0.262_9, 0.905_3];

/// 点が、`p` の殻(網)に囲まれた所にあるか。光線が網を外から入る数と出る数の差で数える。
pub(crate) fn encloses(p: &Part, point: P3) -> bool {
    let dir = tm::Vector3::new(RAY[0], RAY[1], RAY[2]);
    let count: isize = p
        .meshes
        .iter()
        .flatten()
        .map(|m| m.signed_crossing_faces(pt(point), dir))
        .sum();
    count >= 1
}

/// 外側の殻 `outer`(空洞なし)に、空洞(中身のある向きの殻 1 つのソリッド)を足した物。
pub(crate) fn with_voids(outer: Part, voids: Vec<Part>) -> Result<Part, KernelError> {
    if voids.is_empty() {
        return Ok(outer);
    }
    let mut shells = outer.solid.boundaries().clone();
    let mut names = outer.names;
    for v in voids {
        let mut inv = v.solid;
        inv.not();
        shells.extend(inv.boundaries().iter().cloned());
        names.extend(v.names);
    }
    let solid = tm::Solid::try_new(shells).map_err(|_| boolean_failed())?;
    // 体積と網は、空洞を含めて数え直す(外側の殻の体積のままだと、結果の検査が誤る)
    Ok(Part::new(solid, names))
}

/// ブーリアンの結果を、つながりごとのソリッドに分け、面の名前を入力から引き継ぐ。
/// 空洞は、それを囲む外側の殻のうち最も内側の物へ足す(外形の箱では見分けない。L 字の残りの
/// ように、離れた空洞を外形の箱に含む物がある)。
pub(crate) fn adopt(result: tm::Solid, sources: &[&Part]) -> Result<Vec<Part>, KernelError> {
    let (outer, voids) = shells_of(result, sources);
    let mut held: Vec<Vec<Part>> = vec![Vec::new(); outer.len()];
    for v in voids {
        // 空洞の殻の上の点(殻どうしは交わらないので、どの点でも同じ答え)
        let Some(point) = v
            .solid
            .boundaries()
            .first()
            .and_then(|s| s.vertex_iter().next())
            .map(|vx| vx.point())
        else {
            continue;
        };
        let point = [point.x, point.y, point.z];
        let at = (0..outer.len())
            .filter(|&i| encloses(&outer[i], point))
            .min_by(|&i, &j| outer[i].volume.total_cmp(&outer[j].volume))
            .ok_or_else(boolean_failed)?;
        held[at].push(v);
    }
    outer
        .into_iter()
        .zip(held)
        .map(|(o, v)| with_voids(o, v))
        .collect()
}

pub(crate) fn boolean_failed() -> KernelError {
    KernelError::Failed(
        "ブーリアンに失敗しました。同じ平面や同じ円筒で接している面があると失敗します。\
         少しずらすか、はみ出すように作り直してください"
            .into(),
    )
}

/// 外側の殻だけのソリッドと、空洞(内側の殻)を中身のある向きにしたソリッドに分ける。
///
/// truck には、殻 1 つのソリッドどうしだけを渡す。truck は 2 つ目より後ろの殻を、それまでの
/// 結果(つながりの違う殻をまとめて持つ)と 1 つずつ畳む。その時、まとめた殻の中に相手と交わらない
/// 閉じた殻(手を付けない空洞)があり、別の殻が相手と交わると、truck-shapeops の
/// `integrate_by_component` が境界の無い殻の境界の最初の辺を読み、panic する(空洞が 2 つある形で
/// 片方の空洞だけを開ける時。殻の並びが「交わらない空洞が先」の時だけ起きるので、回ごとに揺れる)。
/// また OR で畳むと空洞の意味が崩れる(空洞が埋まる)。そのため空洞はここで分け、[`remove_region`]
/// で 1 つずつ扱う。
pub(crate) fn split_cavities(p: &Part) -> (Part, Vec<Part>) {
    if p.solid.boundaries().len() < 2 {
        return (p.clone(), Vec::new());
    }
    // 殻の面の名前は、元の面(同じ面を共有する)の名前
    let all = faces_of(&p.solid);
    let names_of = |solid: &tm::Solid| -> Vec<String> {
        faces_of(solid)
            .iter()
            .map(|f| {
                all.iter()
                    .position(|g| g.id() == f.id())
                    .map(|i| p.names[i].clone())
                    .unwrap_or_else(|| "unnamed".into())
            })
            .collect()
    };
    let mut outer = Vec::new();
    let mut cavities = Vec::new();
    for shell in p.solid.boundaries() {
        let Ok(solid) = tm::Solid::try_new(vec![shell.clone()]) else {
            continue;
        };
        let meshes = face_meshes(&solid, COARSE_TOLERANCE);
        if signed_volume(&meshes) >= 0.0 {
            outer.push(shell.clone());
        } else {
            let names = names_of(&solid);
            cavities.push(Part::with_meshes(solid, names, meshes));
        }
    }
    let solid = tm::Solid::try_new(outer).unwrap_or_else(|_| p.solid.clone());
    let names = names_of(&solid);
    (Part::new(solid, names), cavities)
}

/// `p` から、殻 1 つのソリッド `v`(空洞なし)の中身を取り除く。`p` の空洞は、外側の殻を削った
/// 後に 1 つずつ削り直す(truck には殻 1 つどうしだけを渡す。[`split_cavities`] の説明)。
pub(crate) fn remove_region(
    p: &Part,
    v: &Part,
    sources: &[&Part],
) -> Result<Vec<Part>, KernelError> {
    if !p.overlaps(v) {
        return Ok(vec![p.clone()]);
    }
    let (outer, cavities) = split_cavities(p);
    let mut inv = v.solid.clone();
    inv.not();
    let r = t_and(&outer.solid, &inv).ok_or_else(boolean_failed)?;
    if r.boundaries().is_empty() {
        // 全部削れたのか、実は触れていないのかを、共通部分で見分ける
        let common = t_and(&outer.solid, &v.solid);
        return Ok(if common.is_none_or(|c| c.boundaries().is_empty()) {
            vec![p.clone()]
        } else {
            Vec::new()
        });
    }
    let (pieces, holes) = shells_of(r, sources);
    match (pieces.len(), holes.len()) {
        (_, 0) => {
            // 外側の殻を削った(または触れていない)。空洞を、残った塊ごとに削り直す。空洞が
            // 削った所とつながれば開き、塊の中に収まれば空洞として残る
            let mut pieces = pieces;
            for c in &cavities {
                let mut next = Vec::new();
                for q in &pieces {
                    next.extend(remove_region(q, c, sources)?);
                }
                pieces = next;
            }
            Ok(pieces)
        }
        (1, 1) => {
            // `v` は外側の殻に触れずに中にある(新しい空洞)。元の空洞と重なれば 1 つにまとめる
            let mut hole = holes.into_iter().next().ok_or_else(boolean_failed)?;
            let mut rest = Vec::new();
            for c in cavities {
                if !hole.overlaps(&c) {
                    rest.push(c);
                    continue;
                }
                let u = t_or(&hole.solid, &c.solid).ok_or_else(boolean_failed)?;
                let (mut merged, inner) = shells_of(u, sources);
                match (merged.len(), inner.len()) {
                    (1, 0) => hole = merged.remove(0),
                    (2, 0) => rest.push(c),
                    // 2 つの空洞が中身を囲む(空洞の中の島)。まれなので扱わない
                    _ => return Err(boolean_failed()),
                }
            }
            rest.push(hole);
            let outer = pieces.into_iter().next().ok_or_else(boolean_failed)?;
            Ok(vec![with_voids(outer, rest)?])
        }
        _ => Err(boolean_failed()),
    }
}

/// `p` から `s` を引く。`s` の空洞の中にある `p` の中身は残る。
pub(crate) fn subtract_part(p: &Part, s: &Part) -> Result<Vec<Part>, KernelError> {
    if !p.overlaps(s) {
        return Ok(vec![p.clone()]);
    }
    let (outer, cavities) = split_cavities(s);
    let mut out = remove_region(p, &outer, &[p, s])?;
    for c in &cavities {
        out.extend(intersect_part(p, c, &[p, s])?);
    }
    Ok(out)
}

/// `t` と `s` の共通部分。外側の殻どうしの共通部分から、両方の空洞を 1 つずつ削る。
/// 名前は `sources` から引き継ぐ。
pub(crate) fn intersect_part(
    t: &Part,
    s: &Part,
    sources: &[&Part],
) -> Result<Vec<Part>, KernelError> {
    if !t.overlaps(s) {
        return Ok(Vec::new());
    }
    let (to, tc) = split_cavities(t);
    let (so, sc) = split_cavities(s);
    let r = t_and(&to.solid, &so.solid).ok_or_else(boolean_failed)?;
    let mut pieces = adopt(r, sources)?;
    for c in tc.iter().chain(&sc) {
        let mut next = Vec::new();
        for q in &pieces {
            next.extend(remove_region(q, c, sources)?);
        }
        pieces = next;
    }
    Ok(pieces)
}

/// `a` と `b` を足す。外側の殻どうしを足し、空洞のうち相手に埋まらない所を後から引く。
/// つながらなければ `None`(外側の殻が離れている時と、片方がもう片方の空洞の中に浮いている時)。
pub(crate) fn union_part(a: &Part, b: &Part) -> Result<Option<Part>, KernelError> {
    let (ao, ac) = split_cavities(a);
    let (bo, bc) = split_cavities(b);
    let r = t_or(&ao.solid, &bo.solid).ok_or_else(boolean_failed)?;
    let mut pieces = adopt(r, &[a, b])?;
    if pieces.len() != 1 {
        return Ok(None);
    }
    // 和の空洞 = (A の空洞 − B) ∪ (B の空洞 − A の外側の殻)。B の空洞から A 全体を引くと、
    // 空洞どうしの重なりを 2 回引くことになり、同じ面で接して失敗する
    let mut voids = Vec::new();
    for c in &ac {
        voids.extend(subtract_part(c, b)?);
    }
    for c in &bc {
        voids.extend(subtract_part(c, &ao)?);
    }
    for v in &voids {
        let mut next = Vec::new();
        for q in &pieces {
            next.extend(subtract_part(q, v)?);
        }
        pieces = next;
    }
    // 1 つにならなければ、A と B は触れていない(空洞の中に浮いた島)。どちらもそのまま残す
    if pieces.len() != 1 {
        return Ok(None);
    }
    Ok(pieces.pop())
}
