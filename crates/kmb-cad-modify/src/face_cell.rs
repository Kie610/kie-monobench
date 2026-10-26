//! 平らな面を別の平面へ置き換える計算(勾配・面の置換・面の削除が使う)。SMLib にも truck にも、
//! ある面だけを傾ける・置き換える関数は無い。そこで、まわりの平らな面の半空間で面を囲んだ凸の立体
//! (セル)を考え、元の面と新しい平面の間の分を、半空間の箱の積で組んで足し引きする。まわりの面は
//! 延びる・縮むだけで、向きは変わらない。
//!
//! 使えるのは、面もまわりの面もどれも平らで、面がまわりの面に対して凸(面の頂点がどのまわりの面の
//! 平面に対しても片側にある)時だけ。曲がった面と接する面、へこんだ角を持つ面は理由つきで断る
//! (別の面に付け替えたり、形を崩したりして黙って進めない)。

use crate::split::half_space;
use kmb_core::BooleanOp;
use kmb_core::kernel::{self as k, Kernel, KernelError, P3, ShapeHandle};

/// 半空間 `dot(x - p, n) <= 0`(`n` は単位の長さ)。`label` は箱の面の名前に入れる。
#[derive(Debug, Clone)]
pub(crate) struct Half {
    pub p: P3,
    pub n: P3,
    pub label: String,
}

impl Half {
    pub fn new(p: P3, n: P3, label: &str) -> Half {
        Half {
            p,
            n: k::normalize(n),
            label: label.into(),
        }
    }

    fn flip(&self, label: &str) -> Half {
        Half::new(self.p, k::scale(self.n, -1.0), label)
    }

    fn dist(&self, x: P3) -> f64 {
        k::dot(k::sub(x, self.p), self.n)
    }
}

/// 平らな面と、それを囲むまわりの面の半空間(面のある側)。
pub(crate) struct Cell {
    /// 面の平面(`n` は外向きの法線。半空間はボディの内側)
    pub face: Half,
    sides: Vec<Half>,
    /// ボディの外形の箱を大きく広げた範囲(ここに届く立体は「囲めない」)
    bounds: (P3, P3),
}

/// 長さの許し(ボディの外形の大きさに比べて)。
fn tol(c: &Cell) -> f64 {
    let d = k::sub(c.bounds.1, c.bounds.0);
    k::dot(d, d).sqrt() * 1e-9 + 1e-7
}

/// ボディ `body` の平らな面 `face` のセル。
pub(crate) fn cell(kernel: &dyn Kernel, body: ShapeHandle, face: &str) -> Result<Cell, String> {
    let faces = kernel.faces(body).map_err(|e| e.to_string())?;
    let me = faces
        .iter()
        .find(|f| f.name == face)
        .ok_or_else(|| format!("面 {face} がボディにありません"))?;
    let pf = me
        .planar
        .ok_or_else(|| format!("面 {face} は平らではありません(平らな面だけを選んでください)"))?;
    let mut edges = kernel.edges(body).map_err(|e| e.to_string())?;
    edges.extend(kernel.arc_edges(body).map_err(|e| e.to_string())?);
    edges.extend(kernel.curve_edges(body).map_err(|e| e.to_string())?);
    let mut verts = Vec::new();
    let mut around: Vec<String> = Vec::new();
    for e in edges.iter().filter(|e| e.faces.iter().any(|n| n == face)) {
        let other = if e.faces[0] == face {
            &e.faces[1]
        } else {
            &e.faces[0]
        };
        verts.extend([e.a, e.b]);
        verts.extend(e.curve.iter().copied());
        if other != face && !around.contains(other) {
            around.push(other.clone());
        }
    }
    if around.is_empty() {
        return Err(format!("面 {face} のまわりの面が見つかりません"));
    }
    let (lo, hi) = kernel.bounds(body).map_err(|e| e.to_string())?;
    let d = k::sub(hi, lo);
    let m = k::dot(d, d).sqrt() + 10.0;
    let mut c = Cell {
        face: Half::new(pf.point, pf.normal, "face"),
        sides: Vec::new(),
        bounds: (
            [lo[0] - m, lo[1] - m, lo[2] - m],
            [hi[0] + m, hi[1] + m, hi[2] + m],
        ),
    };
    let t = tol(&c);
    for (i, name) in around.iter().enumerate() {
        let a = faces
            .iter()
            .find(|f| &f.name == name)
            .ok_or_else(|| format!("まわりの面 {name} が見つかりません"))?;
        let pa = a.planar.ok_or_else(|| {
            format!(
                "面 {face} は曲がった面 {name} と接しています。まわりの面がどれも平らな面だけに使えます"
            )
        })?;
        let h = Half::new(pa.point, pa.normal, &format!("s{i}"));
        let (lo, hi) = verts.iter().fold((0.0f64, 0.0f64), |(lo, hi), v| {
            let d = h.dist(*v);
            (lo.min(d), hi.max(d))
        });
        if lo < -t && hi > t {
            return Err(format!(
                "面 {face} はまわりの面 {name} との間にへこんだ角があります。へこんだ角の無い面だけに使えます"
            ));
        }
        if hi > t {
            c.sides.push(h.flip(&format!("s{i}")));
        } else if lo < -t {
            c.sides.push(h);
        }
    }
    Ok(c)
}

/// 半空間の積の頂点(どれも空なら空)。外形の箱を広げた範囲の面に頂点が乗れば、囲めていない。
fn vertices(c: &Cell, halves: &[Half]) -> (Vec<P3>, bool) {
    let (lo, hi) = c.bounds;
    let mut all: Vec<Half> = halves.to_vec();
    for i in 0..3 {
        let mut n = [0.0; 3];
        n[i] = 1.0;
        all.push(Half::new(hi, n, "box"));
        all.push(Half::new(lo, k::scale(n, -1.0), "box"));
    }
    let t = tol(c);
    let mut out: Vec<P3> = Vec::new();
    let mut open = false;
    let n = all.len();
    for a in 0..n {
        for b in a + 1..n {
            for d in b + 1..n {
                let (p, q, r) = (&all[a], &all[b], &all[d]);
                let det = k::dot(p.n, k::cross(q.n, r.n));
                if det.abs() < 1e-12 {
                    continue;
                }
                // 3 つの平面の交点(n・x = n・p の連立)
                let (hp, hq, hr) = (k::dot(p.n, p.p), k::dot(q.n, q.p), k::dot(r.n, r.p));
                let x = k::scale(
                    k::add(
                        k::add(
                            k::scale(k::cross(q.n, r.n), hp),
                            k::scale(k::cross(r.n, p.n), hq),
                        ),
                        k::scale(k::cross(p.n, q.n), hr),
                    ),
                    1.0 / det,
                );
                if all.iter().all(|h| h.dist(x) <= t) {
                    if all[halves.len()..].iter().any(|h| h.dist(x).abs() <= t) {
                        open = true;
                    }
                    if !out
                        .iter()
                        .any(|y| k::dot(k::sub(*y, x), k::sub(*y, x)) < t * t)
                    {
                        out.push(x);
                    }
                }
            }
        }
    }
    (out, open)
}

/// 頂点の組が体積を持つか(4 点以上で、同じ平面に乗らない)。
fn solid_enough(v: &[P3], t: f64) -> bool {
    if v.len() < 4 {
        return false;
    }
    let o = v[0];
    for i in 1..v.len() {
        for j in i + 1..v.len() {
            for l in j + 1..v.len() {
                let vol = k::dot(k::sub(v[i], o), k::cross(k::sub(v[j], o), k::sub(v[l], o)));
                if vol.abs() > t * 1e3 {
                    return true;
                }
            }
        }
    }
    false
}

/// セルと `extra` の半空間の積を、半空間の箱の積で作る。空なら `None`。
fn region(
    kernel: &dyn Kernel,
    c: &Cell,
    extra: &[Half],
    tag: &str,
) -> Result<Option<ShapeHandle>, String> {
    let mut halves = c.sides.clone();
    halves.extend(extra.iter().cloned());
    let (v, open) = vertices(c, &halves);
    if !solid_enough(&v, tol(c)) {
        return Ok(None);
    }
    if open {
        return Err(
            "まわりの面を延ばしても閉じません(まわりの面が面を囲んでいないか、平面が離れすぎています)"
                .into(),
        );
    }
    let t = tol(c);
    let center = k::scale(
        v.iter().fold([0.0; 3], |s, p| k::add(s, *p)),
        1.0 / v.len() as f64,
    );
    let reach = v
        .iter()
        .map(|p| k::dot(k::sub(*p, center), k::sub(*p, center)).sqrt())
        .fold(0.0, f64::max)
        * 4.0
        + 10.0;
    let mut out: Option<ShapeHandle> = None;
    // 立体の面にならない半空間(どの頂点も乗らない物)は箱を作らない
    for h in halves
        .iter()
        .filter(|h| v.iter().filter(|p| h.dist(**p).abs() <= t).count() >= 3)
    {
        let foot = k::sub(center, k::scale(h.n, h.dist(center)));
        let b = half_space(
            kernel,
            foot,
            k::scale(h.n, -1.0),
            reach,
            &format!("{tag}:{}", h.label),
        )
        .map_err(|e| e.to_string())?;
        out = Some(match out {
            None => b,
            Some(a) => kernel
                .boolean(BooleanOp::Intersect, a, b)
                .map_err(|e| format!("囲む立体を作れません({e})"))?,
        });
    }
    Ok(out)
}

/// 面 `face` を平面 `q`(`n` は新しい面の外向きの法線)に置き換えた形。まわりの面は延びるか縮む。
/// 元の面と平面の間の、面の内側にある分を削り、外側にある分を足す(傾けた面では両方ある)。
pub(crate) fn replace(
    kernel: &dyn Kernel,
    body: ShapeHandle,
    face: &str,
    q: &Half,
    tag: &str,
) -> Result<ShapeHandle, String> {
    let c = cell(kernel, body, face)?;
    if k::dot(q.n, c.face.n) <= 1e-6 {
        return Err(format!(
            "面 {face} を、向きが反対か直交する平面には置き換えられません"
        ));
    }
    let (old, new) = (
        Half::new(c.face.p, c.face.n, "old"),
        Half::new(q.p, q.n, "new"),
    );
    let cut = region(
        kernel,
        &c,
        &[old.clone(), new.flip("new")],
        &format!("{tag}:cut"),
    )?;
    let add = region(kernel, &c, &[old.flip("old"), new], &format!("{tag}:add"))?;
    let mut b = body;
    if let Some(r) = cut {
        b = kernel
            .boolean(BooleanOp::Subtract, b, r)
            .map_err(|e| format!("面の内側を削れません({e})"))?;
    }
    if let Some(r) = add {
        b = kernel
            .boolean(BooleanOp::Union, b, r)
            .map_err(|e| format!("面の外側を足せません({e})"))?;
    }
    keep_name(kernel, body, b, face, q)
}

/// 直した面(平面 `q` に乗る、元の形に無い名前の面のうちいちばん広い物)に、元の面 `face` の名前を
/// 戻す。その面に置いたスケッチ・穴・止め面が、面を直した後も同じ面を指し続けられるように。
/// 元の形に既にある名前の面(同じ平面のほかの面とまとまった物)は付け替えない。名前を付け替え
/// られないカーネルでは、道具の面の名前のまま返す(後の機能はその面を見つけられず誤りになる)。
fn keep_name(
    kernel: &dyn Kernel,
    before: ShapeHandle,
    after: ShapeHandle,
    face: &str,
    q: &Half,
) -> Result<ShapeHandle, String> {
    let old: Vec<String> = kernel
        .faces(before)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|f| f.name)
        .collect();
    let faces = kernel.faces(after).map_err(|e| e.to_string())?;
    if faces.iter().any(|f| f.name == face) {
        return Ok(after);
    }
    let Some(new) = faces
        .into_iter()
        .filter(|f| !old.contains(&f.name))
        .filter(|f| {
            f.planar
                .is_some_and(|p| k::dot(p.normal, q.n) > 1.0 - 1e-9 && q.dist(p.point).abs() < 1e-6)
        })
        .max_by(|a, b| a.area.total_cmp(&b.area))
    else {
        return Ok(after);
    };
    match kernel.rename_faces(after, &[(new.name.clone(), face.to_string())]) {
        Ok(h) => Ok(h),
        Err(KernelError::Unsupported(_)) => {
            log::warn!(
                "面 {} に元の名前 {face} を戻せません(カーネルが対応していません)",
                new.name
            );
            Ok(after)
        }
        Err(e) => Err(format!("面に元の名前を戻せません({e})")),
    }
}

/// 平らな面 `face` を消し、まわりの平らな面を延ばしてふさいだ形(面取りの面を消すと角に戻る)。
pub(crate) fn remove(
    kernel: &dyn Kernel,
    body: ShapeHandle,
    face: &str,
    tag: &str,
) -> Result<ShapeHandle, String> {
    let c = cell(kernel, body, face)?;
    let add =
        region(kernel, &c, &[c.face.flip("old")], &format!("{tag}:fill"))?.ok_or_else(|| {
            format!("面 {face} を消してふさぐ所がありません(まわりの面が面の外で交わりません)")
        })?;
    kernel
        .boolean(BooleanOp::Union, body, add)
        .map_err(|e| format!("面を消した所をふさげません({e})"))
}

/// 前の手順で名前が替わった面を、元の平面(点と外向きの法線)で探し直す(いちばん広い面)。
pub(crate) fn find_on_plane(
    kernel: &dyn Kernel,
    body: ShapeHandle,
    p: P3,
    n: P3,
) -> Option<String> {
    let faces = kernel.faces(body).ok()?;
    faces
        .into_iter()
        .filter(|f| {
            f.planar.is_some_and(|q| {
                k::dot(q.normal, n) > 1.0 - 1e-9 && k::dot(k::sub(q.point, p), n).abs() < 1e-6
            })
        })
        .max_by(|a, b| a.area.total_cmp(&b.area))
        .map(|f| f.name)
}
