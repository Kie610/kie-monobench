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
const EQUAL: &str = "等しい";

/// 等しい長さか半径として比べられる、同じ種類だけの選び。
fn equal_kinds(have: &[Kind]) -> bool {
    matches!(have.first(), Some(Kind::Line | Kind::Round))
        && have.iter().all(|k| Some(k) == have.first())
}

/// 等しい・図形の固定・図形どうしの対称・ポリゴンを、いくつかの拘束(と補助の円)で付ける。
/// 付けられなければ `false`(スケッチは変えない)。`last` は道具で最後に押した要素(図形どうしの
/// 対称では、それが線分なら軸にする。前から選んであった時は `None` で、形から軸を推す)。
pub(crate) fn apply_multi(
    as_name: &str,
    s: &mut Sketch,
    sol: &Solution,
    sel: &[EntityId],
    last: Option<EntityId>,
) -> bool {
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
        EQUAL => {
            if sel.len() < 2 || !usable(EQUAL, s, sel) {
                return false;
            }
            for b in &sel[1..] {
                s.add_constraint(K::Equal { a: sel[0], b: *b });
            }
            true
        }
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
            // どれが軸か: 最後に押した線分。前から選んであった時は、残りの 2 本が鏡に写り合う
            // (端の点が近い)ものを軸にする。中心線があれば中心線。長方形の縦 2 辺は付ける前は
            // 軸から離れているので、形からは辺の 1 本を軸と取り違える(10-24 t4)
            let mut best: Option<(f64, usize, bool)> = None;
            for (i, axis) in lines.iter().enumerate() {
                if last.is_some_and(|l| lines.contains(&l) && l != *axis) {
                    continue;
                }
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
            // 2 組の点の対称を 1 つの拘束にする(長方形の縦 2 辺のように一部が既に決まっていても、
            // 新しく決める分があれば過剰にしない。10-24 t4)
            s.add_constraint(K::SymmetricPairs {
                a1: p1,
                b1: p2,
                a2: q1,
                b2: q2,
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
    if name == EQUAL {
        return have.len() == sel.len() + 1 && equal_kinds(&have);
    }
    patterns(name).iter().any(|(want, _)| fits(&have, want))
}

/// 選んだ要素のうち、拘束 `name` の組の途中として使えるもの(道具を始める時に残す選び)。
pub(crate) fn usable(name: &str, sketch: &Sketch, sel: &[EntityId]) -> bool {
    let have = kinds(sketch, sel);
    if name == EQUAL {
        return have.len() == sel.len() && (have.is_empty() || equal_kinds(&have));
    }
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
    if have.len() == sel.len() && have.len() > 2 && equal_kinds(&have) {
        return vec![(EQUAL, EQUAL)];
    }
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
    if name == EQUAL && sel.len() > 2 {
        let mut ordered = constraints_ui::selected_in_order(editor, sketch);
        ordered.retain(|id| sel.contains(id));
        for id in sel {
            if !ordered.contains(id) {
                ordered.push(*id);
            }
        }
        return try_multi(name, editor, sketch, sol, values, &ordered, None).flatten();
    }
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
        editor.clear_selection();
        return None;
    };
    let sel = if name == EQUAL {
        constraints_ui::selected_in_order(editor, sketch)
    } else {
        constraints_ui::selected(editor, sketch)
    };
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
        editor.select(id);
    }
    let sel = if name == EQUAL {
        constraints_ui::selected_in_order(editor, sketch)
    } else {
        constraints_ui::selected(editor, sketch)
    };
    if let Some(out) = try_multi(name, editor, sketch, sol, values, &sel, Some(id)) {
        return out;
    }
    let (label, k) = complete(name, editor, sketch, sol, &sel)?;
    editor.clear_selection();
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
    last: Option<EntityId>,
) -> Option<Option<Sketch>> {
    // いくつかの拘束で付ける組は、ひとまとめにして過剰・矛盾を調べる。
    let have = kinds(sketch, sel);
    let as_name = if name == EQUAL && sel.len() > 2 {
        if !usable(name, sketch, sel) {
            return None;
        }
        Some(EQUAL)
    } else {
        patterns(name)
            .iter()
            .find(|(want, _)| want.len() == have.len() && fits(&have, want))
            .map(|(_, as_name)| *as_name)
            .filter(|n| [FIX_SHAPE, SYM_SHAPES, POLYGON].contains(n))
    };
    if let Some(as_name) = as_name {
        editor.clear_selection();
        let mut s = sketch.clone();
        s.adopt_solution(sol);
        if !apply_multi(as_name, &mut s, sol, sel, last) {
            editor.notice = Some(match as_name {
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
            && as_name != FIX_SHAPE
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

    fn apply_equal(s: &Sketch, ids: &[EntityId]) -> (Option<Sketch>, Editor) {
        let v = kmb_sketch::dimension_values(s, &|_| None).unwrap();
        let sol = kmb_sketch::solve(s, &v).unwrap();
        let mut editor = Editor::default();
        let out = try_multi(EQUAL, &mut editor, s, &sol, Some(&v), ids, None).flatten();
        (out, editor)
    }

    fn three_lines() -> (Sketch, Vec<EntityId>) {
        let mut s = Sketch::default();
        let ids = (0..3)
            .map(|i| {
                let a = s.add_point(10.0, i as f64 * 20.0);
                let b = s.add_point(20.0 + i as f64 * 5.0, i as f64 * 20.0);
                s.add_line(a, b)
            })
            .collect();
        (s, ids)
    }

    #[test]
    fn equal_three_lines_uses_the_first_pick_and_solves_all_lengths() {
        use kmb_sketch::ConstraintKind as K;
        let (mut s, ids) = three_lines();
        s.add_constraint(K::Length {
            line: ids[2],
            value: "20 mm".into(),
        });
        let mut editor = Editor::default();
        for id in [ids[2], ids[0], ids[1]] {
            editor.select(id);
        }
        let sel = constraints_ui::selected_in_order(&editor, &s);
        assert_eq!(sel, [ids[2], ids[0], ids[1]]);
        assert!(usable(EQUAL, &s, &sel));
        assert!(accepts(EQUAL, &s, &sel[..2], sel[2]));
        let (out, _) = apply_equal(&s, &sel);
        let out = out.unwrap();
        let pairs: Vec<_> = out
            .constraints
            .iter()
            .filter_map(|c| match c.kind {
                K::Equal { a, b } => Some((a, b)),
                _ => None,
            })
            .collect();
        assert_eq!(pairs, [(ids[2], ids[0]), (ids[2], ids[1])]);
        let v = kmb_sketch::dimension_values(&out, &|_| None).unwrap();
        let sol = kmb_sketch::solve(&out, &v).unwrap();
        assert!(!matches!(
            sol.status,
            kmb_sketch::SolveStatus::OverConstrained { .. }
                | kmb_sketch::SolveStatus::Inconsistent { .. }
        ));
        for id in ids {
            let (p, q) = crate::annot::line_ends(&out, &sol, id).unwrap();
            assert!(((q[0] - p[0]).hypot(q[1] - p[1]) - 20.0).abs() < 1e-6);
        }
    }

    #[test]
    fn equal_three_circles_and_a_mixed_circle_arc_selection_solve_radii() {
        use kmb_sketch::ConstraintKind as K;
        for arc in [false, true] {
            let mut s = Sketch::default();
            let mut ids = Vec::new();
            for i in 0..3 {
                let x = i as f64 * 30.0;
                let c = s.add_point(x, 20.0);
                let id = if arc && i == 2 {
                    let a = s.add_point(x + 8.0, 20.0);
                    let b = s.add_point(x, 28.0);
                    s.add_arc(c, a, b)
                } else {
                    s.add_circle(c, 5.0 + i as f64)
                };
                ids.push(id);
            }
            s.add_constraint(K::Radius {
                circle: ids[0],
                value: "5 mm".into(),
            });
            let (out, _) = apply_equal(&s, &ids);
            let out = out.unwrap();
            assert_eq!(
                out.constraints
                    .iter()
                    .filter(|c| matches!(c.kind, K::Equal { .. }))
                    .count(),
                2
            );
            let v = kmb_sketch::dimension_values(&out, &|_| None).unwrap();
            let sol = kmb_sketch::solve(&out, &v).unwrap();
            for id in ids {
                assert!((kmb_sketch::radius(&out, &sol, id).unwrap() - 5.0).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn mixed_and_overconstrained_equal_selections_are_rejected_atomically() {
        use kmb_sketch::ConstraintKind as K;
        let (mut s, ids) = three_lines();
        let c = s.add_point(50.0, 50.0);
        let circle = s.add_circle(c, 5.0);
        let (out, editor) = apply_equal(&s, &[ids[0], circle, ids[1]]);
        assert!(out.is_none());
        assert!(editor.notice.is_none());
        assert!(!usable(EQUAL, &s, &[ids[0], circle]));
        assert!(!accepts(EQUAL, &s, &[ids[0], ids[1]], circle));
        s.add_constraint(K::Equal {
            a: ids[0],
            b: ids[1],
        });
        let (out, editor) = apply_equal(&s, &ids);
        assert!(out.is_none());
        assert!(editor.notice.unwrap().contains("過剰"));
        assert_eq!(s.constraints.len(), 1);
        // 寸法どうしが矛盾する場合も、付けられる組だけを部分的に残さない。
        s.constraints.clear();
        for (id, value) in [(ids[0], "10 mm"), (ids[1], "15 mm")] {
            s.add_constraint(K::Length {
                line: id,
                value: value.into(),
            });
        }
        assert!(apply_equal(&s, &ids).0.is_none());
    }

    #[test]
    fn clearing_picks_drops_click_order_before_a_bulk_selection() {
        let (s, ids) = three_lines();
        let sol = kmb_sketch::solve(&s, &Default::default()).unwrap();
        let mut editor = Editor::default();
        editor.select(ids[2]);
        editor.select(ids[0]);
        assert!(pick(EQUAL, &mut editor, &s, &sol, None, None).is_none());
        assert!(editor.selection.is_empty());
        assert!(editor.selection_order.is_empty());
        editor.selection.extend(ids.iter().copied());
        assert_eq!(constraints_ui::selected_in_order(&editor, &s), ids);
    }

    #[test]
    fn multi_equal_is_available_from_the_context_menu() {
        let (s, ids) = three_lines();
        assert_eq!(fitting(&s, &ids), [(EQUAL, EQUAL)]);
        let sol = kmb_sketch::solve(&s, &Default::default()).unwrap();
        let mut editor = Editor::default();
        let out = apply_selected(
            EQUAL,
            &mut editor,
            &s,
            &sol,
            Some(&Default::default()),
            &ids,
        )
        .unwrap();
        assert_eq!(out.constraints.len(), 2);
        assert!(hint(EQUAL, &editor).contains("2 つ以上"));
        editor.select(ids[2]);
        editor.select(ids[0]);
        editor.selection.remove(&ids[2]);
        editor.select(ids[2]);
        assert_eq!(
            constraints_ui::selected_in_order(&editor, &s),
            [ids[0], ids[2]]
        );
    }
}
