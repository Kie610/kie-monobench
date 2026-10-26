//! 拘束を足す前の確かめ・自動拘束・オフセットの余分な関係を、疎な分解で求めた結果が、
//! 密な行列で階数を取り直す前のやり方と同じになることを確かめる。

use super::speed_tests::{chain, circular, lines_of, pattern};
use super::*;

/// 前のやり方の [`determined_by`]: 行を選び直すたびに密な SVD で階数を取り直す。
fn dense_determined_by(
    sketch: &Sketch,
    values: &HashMap<ConstraintId, f64>,
    candidate: &ConstraintKind,
) -> Option<Vec<ConstraintId>> {
    let mut s = sketch.clone();
    let id = ConstraintId(s.constraints.iter().map(|c| c.id.0).max().unwrap_or(0) + 1);
    s.constraints.push(Constraint::new(id, candidate.clone()));
    let sys = build(&s, values).expect("式を組む");
    let eqs = &sys.equations;
    let j = jacobian(eqs, &sys.x0);
    let rank_of = |rows: &[usize]| rank(&j.select_rows(rows.iter()));
    let grows = |keep: &dyn Fn(Option<ConstraintId>) -> bool| -> bool {
        let rows: Vec<usize> = (0..eqs.len())
            .filter(|&i| eqs[i].0 != Some(id) && keep(eqs[i].0))
            .collect();
        let mut with = rows.clone();
        with.extend((0..eqs.len()).filter(|&i| eqs[i].0 == Some(id)));
        rank_of(&with) > rank_of(&rows)
    };
    if grows(&|_| true) {
        return None;
    }
    let mut tags: Vec<ConstraintId> = eqs.iter().filter_map(|(t, _)| *t).collect();
    tags.dedup();
    tags.retain(|t| *t != id);
    Some(
        tags.into_iter()
            .filter(|&t| grows(&|x| x != Some(t)))
            .collect(),
    )
}

/// 前のやり方の [`independent`]: 長さ 1 にした行を密な基底から外す(グラム・シュミット)。
fn dense_independent(
    sketch: &Sketch,
    values: &HashMap<ConstraintId, f64>,
    candidates: &[(ConstraintKind, Option<f64>)],
) -> (Vec<usize>, usize, usize) {
    let mut s = sketch.clone();
    let mut values = values.clone();
    let base = s.constraints.iter().map(|c| c.id.0).max().unwrap_or(0) + 1;
    let ids: Vec<ConstraintId> = (0..candidates.len())
        .map(|i| ConstraintId(base + i as u32))
        .collect();
    for ((kind, value), id) in candidates.iter().zip(&ids) {
        s.constraints.push(Constraint::new(*id, kind.clone()));
        if let Some(v) = value {
            values.insert(*id, *v);
        }
    }
    let sys = build(&s, &values).expect("式を組む");
    let j = jacobian(&sys.equations, &sys.x0);
    let n = j.ncols();
    let row = |i: usize| -> Vec<f64> { (0..n).map(|c| j[(i, c)]).collect() };
    let reduce = |basis: &[Vec<f64>], mut v: Vec<f64>| -> Option<Vec<f64>> {
        let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-12 {
            return None;
        }
        v.iter_mut().for_each(|x| *x /= norm);
        for _ in 0..2 {
            for b in basis {
                let d: f64 = v.iter().zip(b).map(|(x, y)| x * y).sum();
                v.iter_mut().zip(b).for_each(|(x, y)| *x -= d * y);
            }
        }
        let rest = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        (rest > INDEPENDENT_TOL).then(|| v.into_iter().map(|x| x / rest).collect())
    };
    let mut basis: Vec<Vec<f64>> = Vec::new();
    for (i, (tag, _)) in sys.equations.iter().enumerate() {
        if tag.is_none_or(|t| t.0 < base)
            && let Some(v) = reduce(&basis, row(i))
        {
            basis.push(v);
        }
    }
    let before = n - basis.len();
    let mut kept = vec![];
    for (k, id) in ids.iter().enumerate() {
        let rows: Vec<usize> = (0..sys.equations.len())
            .filter(|&i| sys.equations[i].0 == Some(*id))
            .collect();
        if rows.is_empty() {
            continue;
        }
        let mut trial = basis.clone();
        if rows.iter().all(|&i| match reduce(&trial, row(i)) {
            Some(v) => {
                trial.push(v);
                true
            }
            None => false,
        }) {
            basis = trial;
            kept.push(k);
        }
    }
    (kept, before, n - basis.len())
}

/// 確かめに使うスケッチ(寸法つき・寸法の無い・パターン・円形状パターン)を、解いた位置で。
fn sketches() -> Vec<(&'static str, Sketch, HashMap<ConstraintId, f64>)> {
    let (a, va, _) = chain(4, true, true);
    let (b, vb, _) = chain(4, false, false);
    let (c, vc, _) = chain(3, true, false);
    let (d, vd) = pattern(3, true);
    let (e, ve) = circular(6);
    [
        ("寸法つきの長方形 4 個", a, va),
        ("寸法の無い長方形 4 個", b, vb),
        ("高さの自由な長方形 3 個", c, vc),
        ("パターン 3×3", d, vd),
        ("円形状パターン 6 個", e, ve),
    ]
    .into_iter()
    .map(|(name, s, v)| {
        let mut solved = s.clone();
        solved.adopt_solution(&solve(&s, &v).expect("解く"));
        (name, solved, v)
    })
    .collect()
}

/// 線ごとの水平・垂直と、隣り合う線の平行・等しい長さの候補。
fn candidates(s: &Sketch) -> Vec<ConstraintKind> {
    let lines = lines_of(s);
    let mut out = vec![];
    for (i, &line) in lines.iter().enumerate() {
        out.push(ConstraintKind::Horizontal { line });
        out.push(ConstraintKind::Vertical { line });
        if let Some(&b) = lines.get(i + 1) {
            out.push(ConstraintKind::Parallel { a: line, b });
            out.push(ConstraintKind::Equal { a: line, b });
        }
    }
    out
}

#[test]
fn checks_before_adding_agree_with_taking_the_rank_again() {
    for (name, s, v) in sketches() {
        let mut determined = 0;
        for kind in candidates(&s) {
            let got = determined_by(&s, &v, &kind, None).expect("式を組む");
            assert_eq!(got, dense_determined_by(&s, &v, &kind), "{name}: {kind:?}");
            determined += usize::from(got.is_some());
        }
        // 余分になる候補と、ならない候補の両方を試している
        assert!(determined > 0, "{name}");
    }
}

#[test]
fn auto_constrain_picks_agree_with_the_dense_basis() {
    for (name, s, v) in sketches() {
        let pairs: Vec<(ConstraintKind, Option<f64>)> =
            candidates(&s).into_iter().map(|k| (k, None)).collect();
        let got = independent(&s, &v, &pairs).expect("式を組む");
        let (kept, before, after) = dense_independent(&s, &v, &pairs);
        assert_eq!(got.kept, kept, "{name}");
        assert_eq!((got.dof_before, got.dof_after), (before, after), "{name}");
    }
}

/// オフセットの関係を、ほかの式の張る空間に順に足して広がらないものを余分とする(前のやり方は
/// 足すたびに密な SVD で階数を取り直した)。関係の 1 つと同じ平行の拘束を足して余分を作る。
#[test]
fn offset_redundant_parts_agree_with_taking_the_rank_again() {
    let (mut s, mut values, _) = chain(2, true, true);
    let lines = lines_of(&s);
    assert!(!s.offset(&lines, 2.0, "2 mm", true).is_empty());
    for c in &s.constraints {
        if c.kind.value().is_some() {
            values.entry(c.id).or_insert(2.0);
        }
    }
    let (id, from, to) = s
        .constraints
        .iter()
        .find_map(|c| match &c.kind {
            ConstraintKind::Offset { parts } => parts.iter().find_map(|p| match p {
                OffsetPart::Parallel { from, to } => Some((c.id, *from, *to)),
                _ => None,
            }),
            _ => None,
        })
        .expect("平行の関係");
    s.add_constraint(ConstraintKind::Parallel { a: from, b: to });
    let got = redundant_parts(&s, &values, id).expect("式を組む");
    let sys = build(&s, &values).expect("式を組む");
    let j = jacobian(&sys.equations, &sys.x0);
    let rank_of = |rows: &[usize]| rank(&j.select_rows(rows.iter()));
    let own: BTreeSet<usize> = sys
        .part_rows
        .iter()
        .filter(|r| r.1 == id)
        .map(|r| r.0)
        .collect();
    let mut rows: Vec<usize> = (0..sys.equations.len())
        .filter(|i| !own.contains(i))
        .collect();
    let mut want = vec![];
    for r in sys.part_rows.iter().filter(|r| r.1 == id) {
        let before = rank_of(&rows);
        rows.push(r.0);
        if rank_of(&rows) == before {
            rows.pop();
            want.push(r.2);
        }
    }
    assert!(!want.is_empty());
    assert_eq!(got, want);
}
