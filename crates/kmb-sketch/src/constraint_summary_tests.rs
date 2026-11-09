//! 確かめること: 計算済みの拘束の状態を、機能の要約で読み、文書は変えない。

use crate::{ConstraintId, ConstraintKind, EntityId, Sketch, SolveStatus};
use kmb_core::{Context, Document, Edit, FeatureStatus, Modules, NullKernel};
use std::sync::Arc;

fn summary_of(sketch: Sketch, expected: SolveStatus) -> String {
    let modules = Modules::assemble(vec![crate::module()], Arc::new(NullKernel));
    let mut doc = Document::new();
    let id = doc
        .apply_edits(
            "拘束の状態の試験",
            vec![Edit::Insert {
                type_id: crate::SKETCH.into(),
                name: "拘束の見本".into(),
                data: serde_json::to_value(sketch).unwrap(),
            }],
        )
        .unwrap()[0];
    let report = doc.recompute(&modules.registry, modules.kernel.as_ref());
    assert!(matches!(report.feature(id), Some(FeatureStatus::Ok { .. })));
    let before = doc.to_json();
    let cx = Context::new(doc.data(), &report, &modules.registry, &modules.kernel);
    let solution = crate::solution_of(cx.output(id, "sketch").unwrap()).unwrap();
    assert_eq!(solution.status, expected, "試験の図の拘束の状態");
    let summary = modules.extensions.summary(&cx, id);
    assert_eq!(
        summary,
        Some(crate::commands::status_text(&solution)),
        "行と設定欄が計算済みの拘束の状態を同じ要約から読める"
    );
    assert_eq!(doc.to_json(), before, "要約を読んでも文書は変えない");
    assert_eq!(modules.extensions.summary(&cx, id), summary);
    summary.unwrap()
}

#[test]
fn a_fully_constrained_sketch_exposes_its_current_summary() {
    assert_eq!(
        summary_of(
            crate::tests::rectangle("40 mm", "25 mm"),
            SolveStatus::WellConstrained,
        ),
        "完全拘束"
    );
}

#[test]
fn an_underconstrained_sketch_summary_includes_the_remaining_freedom() {
    let mut sketch = crate::tests::rectangle("40 mm", "25 mm");
    sketch.constraints.retain(|c| c.id != ConstraintId(7));
    assert_eq!(
        summary_of(sketch, SolveStatus::UnderConstrained { dof: 1 }),
        "拘束不足、自由度 1"
    );
}

#[test]
fn redundant_constraints_are_distinct_from_a_fully_constrained_summary() {
    let mut sketch = crate::tests::rectangle("40 mm", "25 mm");
    sketch.add_constraint(ConstraintKind::Length {
        line: EntityId(14),
        value: "25 mm".into(),
    });
    let text = summary_of(
        sketch,
        SolveStatus::OverConstrained {
            redundant: vec![ConstraintId(8)],
        },
    );
    assert_eq!(text, "過剰な拘束 C8");
}
