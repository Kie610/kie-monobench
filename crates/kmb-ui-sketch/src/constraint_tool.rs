//! 拘束の道具: 帯の拘束(水平・平行など)を先に押し、図の要素を順にクリックして付ける。
//! 付けた後も道具は続き、次の組を選べる(Esc か右クリックで終える)。要素を選んでから押す
//! 順でも付けられる(lib.rs の `use_tool`)。

use crate::constraints_ui::{self, Kind};
use crate::view::{DimValues, Editor, Tool};
use kmb_sketch::{EntityId, Sketch, Solution};

/// 拘束の名前ごとの、選ぶ要素の組(順は問わない)と、その組で付ける拘束の名前
/// (一致は、点と線分なら線上、点と円なら円周上になる)。
fn patterns(name: &str) -> &'static [(&'static [Kind], &'static str)] {
    use Kind::{Curve as C, Line as L, Point as P, Round as R};
    match name {
        // 線分の向き・2 点(円は中心)の並びに近い方を付ける([`resolve`])
        constraints_ui::HV => &[
            (&[L], constraints_ui::HV),
            (&[P, P], constraints_ui::HV),
            (&[R, R], constraints_ui::HV),
            (&[P, R], constraints_ui::HV),
        ],
        "一致" => &[
            (&[P, P], "一致"),
            (&[P, L], "線上"),
            (&[P, R], "円周上"),
            (&[P, C], "曲線上"),
        ],
        "平行" => &[(&[L, L], "平行")],
        "等しい" => &[(&[L, L], "等しい"), (&[R, R], "等しい")],
        "直交" => &[(&[L, L], "直交")],
        // 図形の固定(線分・円・曲線の点を全部固定する)
        "固定" => &[
            (&[P], "固定"),
            (&[L], FIX_SHAPE),
            (&[R], FIX_SHAPE),
            (&[C], FIX_SHAPE),
        ],
        "線上" => &[(&[P, L], "線上")],
        "中点" => &[(&[P, L], "中点")],
        "円周上" => &[(&[P, R], "円周上")],
        "接線" => &[
            (&[L, R], "接線"),
            (&[R, R], "接線"),
            (&[C, L], "接線"),
            (&[C, R], "接線"),
            (&[C, C], "接線"),
        ],
        "曲率" => &[(&[C, L], "曲率"), (&[C, R], "曲率"), (&[C, C], "曲率")],
        // 閉じた線分の輪の 1 本を押すと、輪を正多角形にする
        "ポリゴン" => &[(&[L], POLYGON)],
        "同心円" => &[(&[R, R], "同心円")],
        "同一直線上" => &[(&[L, L], "同一直線上")],
        // 図形どうしの対称(線分 2 本と軸、円 2 つと軸)
        "対称" => &[
            (&[P, P, L], "対称"),
            (&[L, L, L], SYM_SHAPES),
            (&[R, R, L], SYM_SHAPES),
        ],
        _ => &[],
    }
}

/// 1 つの拘束では表せず、いくつかを付ける組(下の [`apply_multi`])。
const FIX_SHAPE: &str = "固定(図形)";
const SYM_SHAPES: &str = "対称(図形)";
const POLYGON: &str = "ポリゴン";

/// 図形の固定・図形どうしの対称・ポリゴンを、いくつかの拘束(と補助の円)で付ける。
/// 付けられなければ `false`(スケッチは変えない)。
pub(crate) fn apply_multi(as_name: &str, s: &mut Sketch, sol: &Solution, sel: &[EntityId]) -> bool {
    use kmb_sketch::{ConstraintKind as K, Entity};
    let pt = |id: EntityId| sol.points.get(&id).copied();
    fn ends(s: &Sketch, id: EntityId) -> Option<(EntityId, EntityId)> {
        match s.entity(id) {
            Some(kmb_sketch::Entity::Line { a, b, .. }) => Some((*a, *b)),
            _ => None,
        }
    }
    fn dist(p: [f64; 2], q: [f64; 2]) -> f64 {
        (p[0] - q[0]).hypot(p[1] - q[1])
    }
    match as_name {
        FIX_SHAPE => {
            // 円は円そのものの固定で大きさも止まる(半径の寸法は足さない。余分になるため)
            let before = s.constraints.len();
            *s = crate::menu::set_fixed(s, sel, true);
            s.constraints.len() > before
        }
        SYM_SHAPES => {
            let lines: Vec<EntityId> = sel
                .iter()
                .copied()
                .filter(|id| matches!(s.entity(*id), Some(Entity::Line { .. })))
                .collect();
            let rounds: Vec<EntityId> = sel
                .iter()
                .copied()
                .filter(|id| {
                    matches!(
                        s.entity(*id),
                        Some(Entity::Circle { .. } | Entity::Arc { .. })
                    )
                })
                .collect();
            // 鏡に写した点
            let mirror = |p: [f64; 2], a: [f64; 2], b: [f64; 2]| {
                let d = [b[0] - a[0], b[1] - a[1]];
                let dd = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
                let k = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / dd;
                let f = [a[0] + d[0] * k, a[1] + d[1] * k];
                [2.0 * f[0] - p[0], 2.0 * f[1] - p[1]]
            };
            if rounds.len() == 2 && lines.len() == 1 {
                let center = |id| match s.entity(id) {
                    Some(Entity::Circle { center, .. } | Entity::Arc { center, .. }) => {
                        Some(*center)
                    }
                    _ => None,
                };
                let (Some(c1), Some(c2)) = (center(rounds[0]), center(rounds[1])) else {
                    return false;
                };
                s.add_constraint(K::Symmetric {
                    a: c1,
                    b: c2,
                    line: lines[0],
                });
                s.add_constraint(K::Equal {
                    a: rounds[0],
                    b: rounds[1],
                });
                return true;
            }
            if lines.len() != 3 {
                return false;
            }
            // どれが軸か: 残りの 2 本が鏡に写り合う(端の点が近い)ものを軸にする。中心線があれば中心線
            let mut best: Option<(f64, usize, bool)> = None;
            for (i, axis) in lines.iter().enumerate() {
                let others: Vec<EntityId> = lines.iter().copied().filter(|x| x != axis).collect();
                let (Some((aa, ab)), Some((p1, q1)), Some((p2, q2))) =
                    (ends(s, *axis), ends(s, others[0]), ends(s, others[1]))
                else {
                    continue;
                };
                let (Some(a), Some(b), Some(p1), Some(q1), Some(p2), Some(q2)) =
                    (pt(aa), pt(ab), pt(p1), pt(q1), pt(p2), pt(q2))
                else {
                    continue;
                };
                let (m1, n1) = (mirror(p1, a, b), mirror(q1, a, b));
                let straight = dist(m1, p2) + dist(n1, q2);
                let crossed = dist(m1, q2) + dist(n1, p2);
                let mut err = straight.min(crossed);
                if s.is_centerline(*axis) {
                    err *= 0.5;
                }
                if best.is_none_or(|(e, ..)| err < e) {
                    best = Some((err, i, crossed < straight));
                }
            }
            let Some((_, i, crossed)) = best else {
                return false;
            };
            let axis = lines[i];
            let others: Vec<EntityId> = lines.iter().copied().filter(|x| *x != axis).collect();
            let (Some((p1, q1)), Some((p2, q2))) = (ends(s, others[0]), ends(s, others[1])) else {
                return false;
            };
            let (p2, q2) = if crossed { (q2, p2) } else { (p2, q2) };
            s.add_constraint(K::Symmetric {
                a: p1,
                b: p2,
                line: axis,
            });
            s.add_constraint(K::Symmetric {
                a: q1,
                b: q2,
                line: axis,
            });
            true
        }
        POLYGON => {
            let Some(first) = sel.first() else {
                return false;
            };
            let loop_ids = s.chain_of(*first);
            // 線分だけで閉じた輪(点がどれもちょうど 2 本に使われる)
            let mut uses: std::collections::BTreeMap<EntityId, usize> = Default::default();
            for id in &loop_ids {
                match ends(s, *id) {
                    Some((a, b)) => {
                        *uses.entry(a).or_default() += 1;
                        *uses.entry(b).or_default() += 1;
                    }
                    None => return false,
                }
            }
            if loop_ids.len() < 3 || uses.values().any(|n| *n != 2) {
                return false;
            }
            let pts: Vec<[f64; 2]> = uses.keys().filter_map(|p| pt(*p)).collect();
            let n = pts.len() as f64;
            let c = [
                pts.iter().map(|p| p[0]).sum::<f64>() / n,
                pts.iter().map(|p| p[1]).sum::<f64>() / n,
            ];
            let r = pts.iter().map(|p| dist(*p, c)).sum::<f64>() / n;
            let center = s.add_point(c[0], c[1]);
            let ring = s.add_circle(center, r);
            s.toggle_construction(ring);
            // 1 つのポリゴンの拘束(頂点が補助線の円の上、辺の長さが等しい)
            s.add_constraint(K::Polygon {
                circle: ring,
                sides: loop_ids,
            });
            true
        }
        _ => false,
    }
}

/// `have` が、`want` から要素を除いた残りになっているか(順は問わない)。
fn fits(have: &[Kind], want: &[Kind]) -> bool {
    let mut rest = want.to_vec();
    have.iter().all(|k| match rest.iter().position(|x| x == k) {
        Some(i) => {
            rest.remove(i);
            true
        }
        None => false,
    })
}

fn kinds(sketch: &Sketch, ids: &[EntityId]) -> Vec<Kind> {
    ids.iter()
        .filter_map(|id| constraints_ui::kind(sketch, *id))
        .collect()
}

/// 今の選び `sel` に `id` を足しても、拘束 `name` の組になり得るか(ホバーで光らせる・選ぶ)。
pub(crate) fn accepts(name: &str, sketch: &Sketch, sel: &[EntityId], id: EntityId) -> bool {
    if sel.contains(&id) {
        return true;
    }
    let Some(k) = constraints_ui::kind(sketch, id) else {
        return false;
    };
    let mut have = kinds(sketch, sel);
    have.push(k);
    patterns(name).iter().any(|(want, _)| fits(&have, want))
}

/// 選んだ要素のうち、拘束 `name` の組の途中として使えるもの(道具を始める時に残す選び)。
pub(crate) fn usable(name: &str, sketch: &Sketch, sel: &[EntityId]) -> bool {
    let have = kinds(sketch, sel);
    have.len() == sel.len() && patterns(name).iter().any(|(want, _)| fits(&have, want))
}

/// 選びがそろったら付ける拘束(名前と拘束)。
fn complete(
    name: &str,
    editor: &Editor,
    sketch: &Sketch,
    sol: &Solution,
    sel: &[EntityId],
) -> Option<(&'static str, kmb_sketch::ConstraintKind)> {
    let have = kinds(sketch, sel);
    let (_, as_name) = patterns(name)
        .iter()
        .find(|(want, _)| want.len() == have.len() && fits(&have, want))?;
    let as_name = resolve(as_name, sketch, sol, sel);
    constraints_ui::options(editor, sketch, sol, sel, false)
        .into_iter()
        .find(|(n, _)| *n == as_name)
}

/// 水平/垂直の道具を、選んだ 1 本の線分の向きに近い方(水平か垂直)の名前にする。ほかの名前と、
/// 線分 1 本の選びでない時はそのまま。
pub(crate) fn resolve<'a>(
    name: &'a str,
    sketch: &Sketch,
    sol: &Solution,
    sel: &[EntityId],
) -> &'a str {
    match sel {
        [line] if name == constraints_ui::HV => {
            constraints_ui::horizontal_or_vertical(sketch, sol, *line).unwrap_or(name)
        }
        [_, _] if name == constraints_ui::HV => {
            constraints_ui::points_horizontal_or_vertical(sketch, sol, sel).unwrap_or(name)
        }
        _ => name,
    }
}

/// 右クリックに出す拘束(2 つの要素を選んだ時。1 本の線分の水平・垂直はメニューが別に出す)。
/// 帯の拘束の並びのうち、選んだ組にそのまま付けられる物だけ。
const MENU: [&str; 7] = [
    "一致",
    "平行",
    "直交",
    "等しい",
    "接線",
    "同心円",
    "同一直線上",
];

/// 選んだ要素 `sel` にそのまま付けられる拘束(帯のボタンの名前と、付く拘束の名前)。
/// 一致は、点と線分なら線上、点と円なら円周上になる。
pub(crate) fn fitting(sketch: &Sketch, sel: &[EntityId]) -> Vec<(&'static str, &'static str)> {
    let have = kinds(sketch, sel);
    if have.len() != sel.len() || have.len() != 2 {
        return Vec::new();
    }
    MENU.iter()
        .filter_map(|name| {
            patterns(name)
                .iter()
                .find(|(want, _)| want.len() == have.len() && fits(&have, want))
                .map(|(_, as_name)| (*name, *as_name))
        })
        .collect()
}

/// 選んだ要素に拘束 `name`(帯のボタンの名前)を付けたスケッチ。付けると過剰になる・既に決まっている
/// なら付けず、理由を `editor.notice` に出す。
pub(crate) fn apply_selected(
    name: &str,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
    sel: &[EntityId],
) -> Option<Sketch> {
    let Some((label, k)) = complete(name, editor, sketch, sol, sel) else {
        editor.notice = Some(format!("{name}を付けられません"));
        return None;
    };
    if let Some(v) = values
        && let Some(by) = kmb_sketch::redundant_with(sketch, v, &k)
    {
        let names: Vec<String> = by
            .iter()
            .filter_map(|id| sketch.constraints.iter().find(|c| c.id == *id))
            .map(kmb_sketch::describe)
            .collect();
        editor.notice = Some(if names.is_empty() {
            format!("{label}: 写した形などで既に決まっているので付けません")
        } else {
            format!(
                "{label}: {} で既に決まっているので付けません(付けると過剰な拘束になります)",
                names.join("、")
            )
        });
        return None;
    }
    log::info!("右クリックから拘束を付ける: {label}");
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    s.add_constraint(k);
    Some(s)
}

/// 案内の文。
pub(crate) fn hint(name: &str, editor: &Editor) -> String {
    let what = constraints_ui::GEOMETRIC
        .iter()
        .find(|g| g.0 == name)
        .map_or("要素", |g| g.2);
    let extra = match name {
        "一致" => "(点と線・円・曲線なら、その上に載せる)",
        "固定" => "(線分・円を押せば図形ごと固定)",
        "対称" => "(点 2 つか、線分 2 本か円 2 つと、軸の線分)",
        _ => "",
    };
    let picked = editor.selection.len();
    if picked == 0 {
        format!("{what}、順にクリック{extra}。付けた後も続く(Esc で終わる)")
    } else {
        format!("残りをクリック({picked} つ選んだ。Esc で終わる)")
    }
}

/// クリックした要素 `id` を選びに足し、組がそろえば拘束を付けたスケッチを返す。
/// 既にほかの拘束で決まっていれば付けず、知らせる(`editor.notice`)。
pub(crate) fn pick(
    name: &str,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
    id: Option<EntityId>,
) -> Option<kmb_sketch::Sketch> {
    editor.notice = None;
    let Some(id) = id else {
        // 何もない所: 選びをやめて最初から
        editor.selection.clear();
        return None;
    };
    let sel: Vec<EntityId> = editor.selection.iter().copied().collect();
    if !accepts(name, sketch, &sel, id) {
        editor.notice = Some(format!(
            "{name}には使えない要素です({})",
            constraints_ui::GEOMETRIC
                .iter()
                .find(|g| g.0 == name)
                .map_or("", |g| g.2)
        ));
        return None;
    }
    if !editor.selection.remove(&id) {
        editor.selection.insert(id);
    }
    let sel: Vec<EntityId> = editor.selection.iter().copied().collect();
    if let Some(out) = try_multi(name, editor, sketch, sol, values, &sel) {
        return out;
    }
    let (label, k) = complete(name, editor, sketch, sol, &sel)?;
    editor.selection.clear();
    if let Some(v) = values
        && let Some(by) = kmb_sketch::redundant_with(sketch, v, &k)
    {
        let names: Vec<String> = by
            .iter()
            .filter_map(|id| sketch.constraints.iter().find(|c| c.id == *id))
            .map(kmb_sketch::describe)
            .collect();
        editor.notice = Some(if names.is_empty() {
            format!("{label}: 写した形などで既に決まっているので付けません")
        } else {
            format!(
                "{label}: {} で既に決まっているので付けません(付けると過剰な拘束になります)",
                names.join("、")
            )
        });
        return None;
    }
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    s.add_constraint(k);
    Some(s)
}

/// いくつかの拘束で付ける組なら付ける(付けられなければ `Some(None)`)。その組でなければ `None`。
pub(crate) fn try_multi(
    name: &str,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
    sel: &[EntityId],
) -> Option<Option<Sketch>> {
    // いくつかの拘束で付ける組(図形の固定・図形どうしの対称・ポリゴン)
    let have = kinds(sketch, sel);
    if let Some((_, as_name)) = patterns(name)
        .iter()
        .find(|(want, _)| want.len() == have.len() && fits(&have, want))
        && [FIX_SHAPE, SYM_SHAPES, POLYGON].contains(as_name)
    {
        editor.selection.clear();
        let mut s = sketch.clone();
        s.adopt_solution(sol);
        if !apply_multi(as_name, &mut s, sol, sel) {
            editor.notice = Some(match *as_name {
                POLYGON => "ポリゴンにできません: 線分だけで閉じた輪の 1 本を押してください".into(),
                _ => format!("{as_name}を付けられません"),
            });
            return Some(None);
        }
        // 付けると形が決まりすぎる(過剰)なら付けない(前から過剰なスケッチは止めない)
        let bad = |s: &Sketch, v: &DimValues| {
            kmb_sketch::solve(s, v).is_ok_and(|r| {
                matches!(
                    r.status,
                    kmb_sketch::SolveStatus::OverConstrained { .. }
                        | kmb_sketch::SolveStatus::Inconsistent { .. }
                )
            })
        };
        // 図形の固定は右クリックの「固定する」と同じく調べずに付ける(固定した図形の水平などは
        // 余分になるが、固定を外せば戻る)
        if let Some(v) = values
            && *as_name != FIX_SHAPE
            && bad(&s, v)
            && !bad(sketch, v)
        {
            editor.notice = Some(format!(
                "{as_name}: ほかの拘束と重なるので付けません(付けると過剰な拘束になります)"
            ));
            return Some(None);
        }
        return Some(Some(s));
    }
    None
}

/// 道具の名前(帯のボタンの名前と同じ)。
pub(crate) fn tool_of(name: &str) -> Option<Tool> {
    constraints_ui::GEOMETRIC
        .iter()
        .find(|g| g.0 == name)
        .map(|g| Tool::Constraint(g.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Kind::{Line as L, Point as P, Round as R};

    #[test]
    fn partial_picks_fit_the_pattern_in_any_order() {
        assert!(fits(&[L], &[P, L]));
        assert!(fits(&[L, P], &[P, L]));
        assert!(!fits(&[L, L], &[P, L]));
        assert!(fits(&[P, L], &[P, P, L]));
        assert!(!fits(&[R], &[P, L]));
    }
}
