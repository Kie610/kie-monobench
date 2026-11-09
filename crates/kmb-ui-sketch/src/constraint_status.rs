//! 計算済みの拘束の状態を、行と状態の帯に出す文と印へ直す。

use kmb_sketch::{Solution, SolveStatus};
use kmb_ui::{FeatureMark, FeaturePresentation};

pub(crate) fn presentation(
    solution: &Solution,
    text: &dyn Fn(&str, usize) -> String,
) -> FeaturePresentation {
    let (key, mark) = if solution.is_blank() {
        ("sketch-state-empty", FeatureMark::Open)
    } else {
        match &solution.status {
            SolveStatus::WellConstrained => ("sketch-state-full", FeatureMark::Complete),
            SolveStatus::UnderConstrained { .. } => ("sketch-state-under", FeatureMark::Open),
            SolveStatus::OverConstrained { .. } => ("sketch-state-over", FeatureMark::Warning),
            SolveStatus::Inconsistent { .. } => ("sketch-state-inconsistent", FeatureMark::Error),
        }
    };
    FeaturePresentation {
        text: text(key, solution.dof),
        mark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_sketch::{ConstraintKind, Sketch};

    #[test]
    fn solved_states_choose_distinct_marks_and_pass_the_actual_freedom_to_translation() {
        let mut sketch = Sketch::default();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(10.0, 0.0);
        let line = sketch.add_line(a, b);
        sketch.add_constraint(ConstraintKind::Fix { point: a });
        sketch.add_constraint(ConstraintKind::Horizontal { line });
        let translate = |key: &str, dof: usize| format!("{key}:{dof}");
        let under = kmb_sketch::solve(&sketch, &Default::default()).unwrap();
        assert_eq!(under.status, SolveStatus::UnderConstrained { dof: 1 });
        assert_eq!(
            presentation(&under, &translate),
            FeaturePresentation {
                text: "sketch-state-under:1".into(),
                mark: FeatureMark::Open,
            }
        );
        let dimension = sketch.add_constraint(ConstraintKind::Length {
            line,
            value: "10 mm".into(),
        });
        let mut values = std::collections::HashMap::from([(dimension, 10.0)]);
        let full = kmb_sketch::solve(&sketch, &values).unwrap();
        assert_eq!(full.status, SolveStatus::WellConstrained);
        assert_eq!(
            presentation(&full, &translate),
            FeaturePresentation {
                text: "sketch-state-full:0".into(),
                mark: FeatureMark::Complete,
            }
        );
        let duplicate = sketch.add_constraint(ConstraintKind::Length {
            line,
            value: "10 mm".into(),
        });
        values.insert(duplicate, 10.0);
        let over = kmb_sketch::solve(&sketch, &values).unwrap();
        assert!(matches!(over.status, SolveStatus::OverConstrained { .. }));
        assert_eq!(
            presentation(&over, &translate),
            FeaturePresentation {
                text: "sketch-state-over:0".into(),
                mark: FeatureMark::Warning,
            }
        );
        // 表示は与えた計算済みの結果を読むだけで、古い自由度を残さない。
        assert_eq!(
            presentation(&under, &translate).text,
            "sketch-state-under:1"
        );
    }
}
