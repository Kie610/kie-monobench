//! 形を変えた後の面の名前の決め方。SMLib が引き継いだ面のタグを名前に戻し、タグを失った面や
//! 新しくできた面(シェルの内側・丸めの面)には、元の面との位置の関係から名前を付ける。

use crate::ffi::{self, Brep, JcFace};
use crate::{Store, failed};
use kmb_core::kernel::{self as k, KernelError, P3, ShapeHandle, dedupe_names};
use std::collections::HashMap;

/// シェルの後の面の名前。元の名前を引き継いだ面のうち、元の面と同じ所にある 1 枚だけが元の名前を
/// 持ち、ほかの同じ名前の面(内側へずらした面)は `{tag}:in:{元の名前}` にする。名前の無い面は
/// `{tag}:face`。最後に同じ名前を `#0`、`#1` … で分ける。
pub(crate) fn shell_names(
    store: &Store,
    faces: &[JcFace],
    src_faces: &[JcFace],
    src_names: &[String],
    tag: &str,
) -> Vec<String> {
    let base: Vec<Option<String>> = faces
        .iter()
        .map(|f| {
            (f.tag >= 0)
                .then(|| store.tag_names.get(f.tag as usize).cloned())
                .flatten()
        })
        .collect();
    let d2 = |a: P3, b: P3| (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>();
    let mut names = vec![String::new(); faces.len()];
    let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, b) in base.iter().enumerate() {
        match b {
            Some(n) => groups.entry(n.as_str()).or_default().push(i),
            None => names[i] = format!("{tag}:face"),
        }
    }
    for (n, idx) in groups {
        // 元の面の代表点に一番近い面が元の面
        let keep = src_names
            .iter()
            .position(|m| m == n)
            .and_then(|j| src_faces.get(j))
            .and_then(|sf| {
                idx.iter().copied().min_by(|&a, &b| {
                    d2(faces[a].point, sf.point).total_cmp(&d2(faces[b].point, sf.point))
                })
            });
        for i in idx {
            names[i] = if Some(i) == keep {
                n.to_string()
            } else {
                format!("{tag}:in:{n}")
            };
        }
    }
    dedupe_names(names, &points(faces))
}

/// 面のオフセットの板の面の名前。動かした面は、動かした先の面が元の名前を受け継ぐ(後の機能が
/// その名前で指し続けられるように)。元の位置の面は `{tag}:offset:base:{名前}`。開けた面から
/// できた縁の面は、シェルと同じく元の面に近い方が元の名前、ほかは `{tag}:in:{名前}`。
pub(crate) fn offset_slab_names(
    store: &Store,
    faces: &[JcFace],
    src_faces: &[JcFace],
    src_names: &[String],
    moved: &[String],
    tag: &str,
) -> Vec<String> {
    let mut names = shell_names(store, faces, src_faces, src_names, tag);
    for n in moved {
        let inner = format!("{tag}:in:{n}");
        if names.iter().any(|m| m == &inner) {
            for m in names.iter_mut() {
                if m == n {
                    *m = format!("{tag}:offset:base:{n}");
                } else if *m == inner {
                    *m = n.clone();
                }
            }
        }
    }
    dedupe_names(names, &points(faces))
}

pub(crate) fn points(faces: &[JcFace]) -> Vec<P3> {
    faces.iter().map(|f| f.point).collect()
}

/// 平面の面の、平面の式(外向きの法線、原点からの距離)。
pub(crate) fn plane_key(f: &JcFace) -> Option<(P3, f64)> {
    (f.planar != 0).then(|| (f.normal, k::dot(f.normal, f.plane_point)))
}

/// ブーリアンの結果の面に名前を付ける。タグがあればその名前、無ければ入力の同じ平面の名前。
pub(crate) fn adopt(
    store: &Store,
    faces: &[JcFace],
    sources: &[(&[JcFace], &[String])],
) -> Vec<String> {
    let names: Vec<String> = faces
        .iter()
        .map(|f| {
            if f.tag >= 0
                && let Some(n) = store.tag_names.get(f.tag as usize)
            {
                return n.clone();
            }
            if let Some((n, d)) = plane_key(f) {
                for (src, src_names) in sources {
                    for (g, name) in src.iter().zip(src_names.iter()) {
                        if let Some((m, e)) = plane_key(g)
                            && k::dot(n, m) > 1.0 - 1e-9
                            && (d - e).abs() < 1e-6
                        {
                            return name.clone();
                        }
                    }
                }
            }
            "unnamed".into()
        })
        .collect();
    dedupe_names(names, &points(faces))
}

/// 同じ平面で重なる面が 1 枚にまとまった時、SMLib はどちらの面の印を残すか決まっていない
/// (実行ごとに変わる)。道具の面の名前が残った平面の面が、元の形の同じ平面の面を含んでいれば、
/// 元の形の名前にそろえる(後の機能がその面の名前で参照し続けられるように)。
pub(crate) fn prefer_target_names(
    s: &Store,
    result: &Brep,
    faces: &[JcFace],
    names: &mut [String],
    target: ShapeHandle,
    tool: ShapeHandle,
) -> Result<(), KernelError> {
    let (t, u) = (s.get(target)?, s.get(tool)?);
    let mut target_faces: Option<Vec<JcFace>> = None;
    for (i, f) in faces.iter().enumerate() {
        let Some((n, d)) = plane_key(f) else {
            continue;
        };
        if !u.names.contains(&names[i]) || t.names.contains(&names[i]) {
            continue;
        }
        if target_faces.is_none() {
            target_faces = Some(
                t.brep
                    .faces(ffi::FACE_INNER_POINT)
                    .map_err(failed("面の列挙"))?,
            );
        }
        for (g, name) in target_faces.iter().flatten().zip(&t.names) {
            let Some((m, e)) = plane_key(g) else {
                continue;
            };
            if k::dot(n, m) > 1.0 - 1e-9
                && (d - e).abs() < 1e-6
                && !names.contains(name)
                && result
                    .face_distance(i, g.point)
                    .is_ok_and(|dist| dist < 1e-6)
            {
                names[i] = name.clone();
                break;
            }
        }
    }
    Ok(())
}

/// 点 `p` から折れ線 `line` までの距離と、一番近い所から折れ線の近い方の端までの長さ。
pub(crate) fn polyline_distance(line: &[P3], p: P3) -> (f64, f64) {
    let len = |a: P3, b: P3| {
        let d = k::sub(b, a);
        k::dot(d, d).sqrt()
    };
    if line.len() == 1 {
        return (len(p, line[0]), 0.0);
    }
    let total: f64 = line.windows(2).map(|w| len(w[0], w[1])).sum();
    let (mut best, mut at, mut run) = (f64::INFINITY, 0.0, 0.0);
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = k::sub(b, a);
        let l2 = k::dot(ab, ab);
        let t = if l2 > 0.0 {
            (k::dot(k::sub(p, a), ab) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let d = len(p, k::add(a, k::scale(ab, t)));
        if d < best {
            (best, at) = (d, run + t * l2.sqrt());
        }
        run += l2.sqrt();
    }
    (best, at.min(total - at))
}

/// 丸めの後の面の名前。タグのある面はその名前、無い面(丸めの面)は一番近い丸めた辺から
/// `{tag}:{面 a}|{面 b}`、辺の集まる角の面は `{tag}:corner`。
pub(crate) fn blend_names(
    store: &Store,
    faces: &[JcFace],
    blended: &[(&[String; 2], Vec<P3>, f64)],
    tag: &str,
) -> Vec<String> {
    let names: Vec<String> = faces
        .iter()
        .map(|f| {
            if f.tag >= 0
                && let Some(n) = store.tag_names.get(f.tag as usize)
            {
                return n.clone();
            }
            // 丸めの面の内側の点は、辺から半径ほど(直角の辺で 0.3〜0.8 倍)。近い辺が 2 本以上あり、
            // 一番近い辺の端のそば(半径の 1.5 倍まで)なら、辺の集まる角の面
            let mut close: Vec<(&[String; 2], f64, f64)> = blended
                .iter()
                .map(|(faces, line, size)| {
                    let (d, from_end) = polyline_distance(line, f.point);
                    let size = size.max(1e-9);
                    (*faces, d / size, from_end / size)
                })
                .filter(|x| x.1 < 1.2)
                .collect();
            close.sort_by(|a, b| a.1.total_cmp(&b.1));
            match close.as_slice() {
                [] => format!("{tag}:corner"),
                [first, _, ..] if first.2 < 1.5 => format!("{tag}:corner"),
                [first, ..] => format!("{tag}:{}|{}", first.0[0], first.0[1]),
            }
        })
        .collect();
    dedupe_names(names, &points(faces))
}
