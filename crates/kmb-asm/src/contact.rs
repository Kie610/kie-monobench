//! 接触セット。
//! 中身の型は kmb_interchange::contact(文書の区画 `asm.contacts`)。ここは操作と、引いている部品が止まる値の計算。
//!
//! 止まる値: 引いているジョイントの値を少しずつ進め、動く網が相手の網に食い込んだら、その手前と
//! の間を二分して、食い込まないいちばん先の値を探す。食い込みは、片方の三角形の辺がもう片方の
//! 三角形の中を突き抜けるかで見る。面どうしが同じ平面で触れているだけ(置いてあるだけ)は
//! 食い込みとみなさない。網は形の許容差 [`TOLERANCE`] で作るので、曲面ではその分だけ手前か先で
//! 止まる。

use crate::anim::drives;
use kmb_core::kernel::{self as k, Matrix4, P3};
use kmb_core::{Context, Drag, DragMotion, FeatureId, Outcome, body_root};
use kmb_interchange::animation as anim;
use kmb_interchange::contact::{ContactSet, Contacts};

/// 新しい接触セットを作る操作の ID。
pub const NEW_CONTACT_SET: &str = "asm.new_contact_set";
/// 接触セット全体を入れる・切る操作の ID。
pub const CONTACTS: &str = "asm.contacts";
/// ジョイントを動かすとどこで当たるかを答える操作の ID。
pub const REACH: &str = "asm.contact_reach";
/// 網を作る許容差(mm)。
const TOLERANCE: f64 = 0.05;
/// 進める刻みの上限(回転は度、スライダは mm)。薄い物を飛び越さないように細かく見る
const STEP_DEG: f64 = 2.0;
const STEP_MM: f64 = 0.5;
/// 1 回に見る刻みの数の上限。
const MAX_STEPS: usize = 90;
/// 食い込みとみなす深さの下限(三角形の中の割合と、辺の上の割合)
const EPS: f64 = 1e-6;

/// 三角形 1 つと、その囲みの箱。
#[derive(Clone, Copy)]
struct Tri {
    p: [P3; 3],
    lo: P3,
    hi: P3,
}

impl Tri {
    fn new(p: [P3; 3]) -> Tri {
        let mut lo = p[0];
        let mut hi = p[0];
        for q in &p[1..] {
            for i in 0..3 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
            }
        }
        Tri { p, lo, hi }
    }

    fn moved(&self, m: &Matrix4) -> Tri {
        Tri::new(self.p.map(|q| k::transform_point(m, q)))
    }
}

fn boxes_meet(a: &Tri, b: &Tri) -> bool {
    (0..3).all(|i| a.lo[i] <= b.hi[i] + 1e-9 && b.lo[i] <= a.hi[i] + 1e-9)
}

/// 線分 `a`→`b` が三角形 `t` の中を突き抜けるか(端と縁と、三角形の平面に沿う線分は除く)。
fn segment_pierces(a: P3, b: P3, t: &[P3; 3]) -> bool {
    let e1 = k::sub(t[1], t[0]);
    let e2 = k::sub(t[2], t[0]);
    let d = k::sub(b, a);
    let h = k::cross(d, e2);
    let det = k::dot(e1, h);
    let scale = k::dot(d, d).sqrt() * k::dot(e1, e1).sqrt() * k::dot(e2, e2).sqrt();
    if det.abs() <= 1e-9 * scale.max(1e-30) {
        return false;
    }
    let inv = 1.0 / det;
    let s = k::sub(a, t[0]);
    let u = k::dot(s, h) * inv;
    if u <= EPS || u >= 1.0 - EPS {
        return false;
    }
    let q = k::cross(s, e1);
    let v = k::dot(d, q) * inv;
    if v <= EPS || u + v >= 1.0 - EPS {
        return false;
    }
    let f = k::dot(e2, q) * inv;
    f > EPS && f < 1.0 - EPS
}

/// 2 つの三角形が食い込んでいるか(どちらかの辺が、もう片方の中を突き抜ける)。
fn tris_cross(a: &Tri, b: &Tri) -> bool {
    let pierce = |x: &Tri, y: &Tri| (0..3).any(|i| segment_pierces(x.p[i], x.p[(i + 1) % 3], &y.p));
    pierce(a, b) || pierce(b, a)
}

/// 網の三角形(世界座標)。
fn tris_of(mesh: &kmb_core::Mesh) -> Vec<Tri> {
    let p = |i: u32| mesh.positions[i as usize].map(f64::from);
    mesh.triangles
        .iter()
        .map(|t| Tri::new([p(t[0]), p(t[1]), p(t[2])]))
        .collect()
}

/// 動く網を `m` だけ動かした時に、相手の網に食い込むか。
fn overlaps(moving: &[Tri], others: &[Tri], m: &Matrix4) -> bool {
    let moved: Vec<Tri> = moving.iter().map(|t| t.moved(m)).collect();
    let Some(all) = moved.iter().copied().reduce(|a, b| {
        let mut t = a;
        for i in 0..3 {
            t.lo[i] = t.lo[i].min(b.lo[i]);
            t.hi[i] = t.hi[i].max(b.hi[i]);
        }
        t
    }) else {
        return false;
    };
    let near: Vec<&Tri> = others.iter().filter(|o| boxes_meet(o, &all)).collect();
    moved
        .iter()
        .any(|a| near.iter().any(|b| boxes_meet(a, b) && tris_cross(a, b)))
}

/// ジョイントの部品を欄の値 `from` から `to` へ引く時に、接触セットの相手に食い込む手前で止まる
/// 値。止めなければ `None`(接触セットを切っている、相手が無い、`to` まで食い込まない、`from` で
/// もう食い込んでいる)。
pub fn limit(cx: &Context<'_>, drag: &Drag, from: f64, to: f64) -> Option<f64> {
    let step = match drag.motion {
        DragMotion::Rotate { .. } => STEP_DEG,
        DragMotion::Slide { .. } => STEP_MM,
        DragMotion::Free | DragMotion::Follow => return None,
    };
    match sweep(cx, drag.feature, step, from, to) {
        Ok(Swept::Stopped { at, .. }) => {
            log::debug!("接触: {} を {at:.3} で止める(引いた先 {to:.3})", drag.label);
            Some(at)
        }
        Ok(_) => None,
        Err(e) => {
            log::debug!("接触: {e}");
            None
        }
    }
}

/// [`sweep`] の結果。
enum Swept {
    /// `to` まで食い込まない
    Free,
    /// 値 `at` で、ボディ `hit` に当たる手前で止まる
    Stopped { at: f64, hit: Vec<FeatureId> },
    /// `from` でもう食い込んでいる(ボディ `hit` に)
    Already { hit: Vec<FeatureId> },
}

/// ジョイント `joint` の値(段の値の単位)を `from` から `to` へ刻み `step` で進め、接触セットの
/// 相手に食い込む手前を探す。見られない時(接触セットを切っている、相手が無いなど)は理由。
fn sweep(
    cx: &Context<'_>,
    joint: FeatureId,
    step: f64,
    from: f64,
    to: f64,
) -> Result<Swept, String> {
    let contacts: Contacts = cx.data.section().map_err(|e| e.to_string())?;
    if !contacts.enabled {
        return Err("接触セットが無効です(接触セットを有効にする)".into());
    }
    let (_, _, d) = drives(cx)
        .into_iter()
        .find(|(j, _, _)| *j == joint)
        .ok_or_else(|| format!("{joint} は動く回転かスライダのジョイントではありません"))?;
    // 剛性グループがこのジョイントの動きに乗せた部品も一緒に動く
    let moving: Vec<FeatureId> = anim::moved_bodies(cx.data, cx.report, joint, &d)
        .iter()
        .map(|b| body_root(cx.data, cx.registry, *b))
        .collect();
    let obstacles = contacts.obstacles(&moving);
    if obstacles.is_empty() {
        return Err("動く部品と同じ接触セットに入った相手のボディがありません".into());
    }
    let mut mov: Vec<Tri> = Vec::new();
    let mut others: Vec<(FeatureId, Vec<Tri>)> = Vec::new();
    for (id, h) in cx.visible_bodies() {
        let r = body_root(cx.data, cx.registry, id);
        let mesh = |h| match cx.kernel.mesh(h, TOLERANCE) {
            Ok(m) => tris_of(&m),
            Err(e) => {
                log::warn!("接触: ボディ {id} の網を作れない: {e}");
                Vec::new()
            }
        };
        if moving.contains(&r) {
            mov.extend(mesh(h));
        } else if obstacles.contains(&r) {
            others.push((id, mesh(h)));
        }
    }
    let all: Vec<Tri> = others.iter().flat_map(|(_, t)| t.iter().cloned()).collect();
    if mov.is_empty() || all.is_empty() {
        return Err("見えている動く部品か相手のボディがありません".into());
    }
    let at = |v: f64| d.delta(d.from_shown(v));
    let hits = |v: f64| overlaps(&mov, &all, &at(v));
    // 値 `v` で食い込む相手
    let hit = |v: f64| {
        others
            .iter()
            .filter(|(_, t)| overlaps(&mov, t, &at(v)))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
    };
    if hits(from) {
        // もう食い込んでいる所からは止めない(離す向きにも引けるように)
        return Ok(Swept::Already { hit: hit(from) });
    }
    if (to - from).abs() < 1e-12 {
        return Ok(Swept::Free);
    }
    let n = (((to - from).abs() / step).ceil() as usize).clamp(1, MAX_STEPS);
    let mut free = from;
    for i in 1..=n {
        let v = from + (to - from) * i as f64 / n as f64;
        if !hits(v) {
            free = v;
            continue;
        }
        // free と v の間を二分する
        let mut bad = v;
        for _ in 0..24 {
            let mid = 0.5 * (free + bad);
            if hits(mid) {
                bad = mid;
            } else {
                free = mid;
            }
        }
        return Ok(Swept::Stopped {
            at: free,
            hit: hit(bad),
        });
    }
    Ok(Swept::Free)
}

/// ジョイントを今の値から `to` へ動かすと、接触セットの相手にどこで当たるか(AI・CLI が聞く。
/// 10-20 a1)。引数 `joint`(回転かスライダのジョイントの機能の番号)・`to`(動かす先。回転は deg、
/// スライダは mm)。形は変えない。
pub fn reach(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let joint = cx
        .arg("joint")
        .and_then(|v| v.as_u64())
        .map(FeatureId)
        .ok_or("joint にジョイントの機能の番号を渡します")?;
    let to = cx.arg_f64("to")?.ok_or("to に動かす先の値を渡します")?;
    let name = cx
        .data
        .feature(joint)
        .map_or_else(|| joint.to_string(), |f| f.name.clone());
    let (_, _, d) = drives(cx)
        .into_iter()
        .find(|(j, _, _)| *j == joint)
        .ok_or_else(|| format!("{name} は動く回転かスライダのジョイントではありません"))?;
    if !d.dofs.is_empty() {
        return Err(
            format!("{name} は動きが 2 つ以上あるので、当たる値を 1 つに決められません").into(),
        );
    }
    let (step, unit) = if d.slide {
        (STEP_MM, "mm")
    } else {
        (STEP_DEG, "deg")
    };
    let now = d.to_shown(d.value);
    let names = |ids: &[FeatureId]| {
        ids.iter()
            .map(|b| {
                cx.data
                    .feature(*b)
                    .map_or_else(|| b.to_string(), |f| format!("「{}」", f.name))
            })
            .collect::<Vec<_>>()
            .join("・")
    };
    let n = |v: f64| format!("{} {unit}", (v * 1e3).round() / 1e3 + 0.0);
    // 動きの範囲を越えた先は、範囲の端までを見る
    let end = to.clamp(
        d.min.map_or(f64::MIN, |m| d.to_shown(m)),
        d.max.map_or(f64::MAX, |m| d.to_shown(m)),
    );
    let past = if end != to {
        format!("(動きの範囲の端 {} まで)", n(end))
    } else {
        String::new()
    };
    Ok(Outcome::Message(match sweep(cx, joint, step, now, end)? {
        Swept::Free => format!(
            "{name} を {} から {} へ動かしても当たりません{past}",
            n(now),
            n(to)
        ),
        Swept::Stopped { at, hit } => format!(
            "{name} を {} から {} へ動かすと、{} で {} に当たります(あと {} 動けます){past}",
            n(now),
            n(to),
            n(at),
            names(&hit),
            n((at - now).abs())
        ),
        Swept::Already { hit } => format!(
            "{name} は今の {} で、もう {} に食い込んでいます",
            n(now),
            names(&hit)
        ),
    }))
}

/// 新しい接触セット。引数 `bodies`(ボディの機能の番号の並び)と `name`。`bodies` を省くと、
/// 選んでいる面のボディと選んでいるボディ(ボディでない物は飛ばす)。
/// 接触セットを切っていれば入れる。
pub fn new_set(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let shown: Vec<FeatureId> = cx.visible_bodies().iter().map(|b| b.0).collect();
    let is_body = |id: FeatureId| {
        shown.contains(&id)
            || shown
                .iter()
                .any(|s| body_root(cx.data, cx.registry, *s) == id)
    };
    let ids: Vec<FeatureId> = match cx.arg("bodies").and_then(|v| v.as_array()) {
        Some(list) => {
            let ids: Vec<FeatureId> = list
                .iter()
                .filter_map(|v| v.as_u64().map(FeatureId))
                .collect();
            if let Some(bad) = ids.iter().find(|id| !is_body(**id)) {
                return Err(format!("機能 {bad} は見えているボディではありません").into());
            }
            ids
        }
        None => cx
            .picked
            .into_iter()
            .chain(cx.also_picked.iter())
            .map(|p| p.feature)
            .chain(cx.selected)
            .chain(cx.also_selected.iter().copied())
            .filter(|id| is_body(*id))
            .collect(),
    };
    let mut bodies: Vec<FeatureId> = Vec::new();
    for id in ids {
        let r = body_root(cx.data, cx.registry, id);
        if !bodies.contains(&r) {
            bodies.push(r);
        }
    }
    if bodies.len() < 2 {
        return Err(
            "ぶつかり合うボディを 2 つ以上選んでください(面を Ctrl を押しながら選ぶ)".into(),
        );
    }
    let mut c: Contacts = cx.data.section().map_err(|e| e.to_string())?;
    let name = cx
        .arg_str("name")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(|| c.free_name(), str::to_string);
    c.sets.push(ContactSet {
        name: name.clone(),
        bodies,
        enabled: true,
    });
    c.enabled = true;
    let edit = cx.data.set_section(&c)?;
    log::info!("接触セット「{name}」を作る");
    Ok(Outcome::Change {
        label: format!("接触セット「{name}」を作成"),
        edits: vec![edit],
    })
}

/// 接触セット全体を入れる・切る。引数 `enabled`(省くと今の逆)。
pub fn toggle(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let mut c: Contacts = cx.data.section().map_err(|e| e.to_string())?;
    c.enabled = cx
        .arg("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(!c.enabled);
    let label = if c.enabled {
        "接触セットを有効にする"
    } else {
        "接触セットを無効にする"
    };
    log::info!("{label}");
    Ok(Outcome::Change {
        label: label.into(),
        edits: vec![cx.data.set_section(&c)?],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(p: [[f64; 3]; 3]) -> Tri {
        Tri::new(p)
    }

    #[test]
    fn crossing_triangles_meet_but_touching_ones_do_not() {
        let floor = tri([[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]]);
        // 床を縦に突き抜ける
        let wall = tri([[2.0, 2.0, -1.0], [2.0, 2.0, 1.0], [3.0, 1.0, 0.5]]);
        assert!(tris_cross(&floor, &wall));
        // 床の上に立つだけ(下の点が床の上)
        let stand = tri([[2.0, 2.0, 0.0], [2.0, 2.0, 1.0], [3.0, 1.0, 1.0]]);
        assert!(!tris_cross(&floor, &stand));
        // 同じ平面で重なる(置いてあるだけ)
        let flat = tri([[1.0, 1.0, 0.0], [4.0, 1.0, 0.0], [1.0, 4.0, 0.0]]);
        assert!(!tris_cross(&floor, &flat));
        // 離れている
        let away = tri([[2.0, 2.0, 1.0], [2.0, 2.0, 2.0], [3.0, 1.0, 1.5]]);
        assert!(!tris_cross(&floor, &away));
    }
}
