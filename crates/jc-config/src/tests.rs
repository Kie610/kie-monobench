use super::*;
use jc_core::{Document, Modules, RecomputeReport};
use serde_json::{Value as Json, json};

/// 基準部品(穴あきの板)と組み立てのモジュール。操作は CLI・MCP と同じく引数の宣言を通して実行する。
struct T {
    doc: Document,
    modules: Modules,
    report: RecomputeReport,
    holes: FeatureId,
}

impl T {
    fn new() -> Self {
        let part = jc_samples::reference_part();
        let modules = jc_bundle::modules();
        let mut doc = part.doc;
        let report = doc.recompute(&modules.registry, modules.kernel.as_ref());
        assert_eq!(report.error_count(), 0);
        Self {
            doc,
            modules,
            report,
            holes: part.hole_pattern,
        }
    }

    fn run(&mut self, id: &str, args: Json) -> Result<(), String> {
        let cx = Context {
            args: &args,
            ..Context::new(
                self.doc.data(),
                &self.report,
                &self.modules.registry,
                &self.modules.kernel,
            )
        };
        if let Some(why) = self
            .modules
            .extensions
            .action(id, &cx)
            .and_then(|a| a.disabled)
        {
            return Err(why);
        }
        match self.modules.extensions.run(id, &cx)? {
            Outcome::Change { label, edits } => {
                self.doc
                    .apply_edits(&label, edits)
                    .map_err(|e| e.to_string())?;
            }
            _ => panic!("変更でない"),
        }
        self.report = self
            .doc
            .recompute(&self.modules.registry, self.modules.kernel.as_ref());
        Ok(())
    }

    fn expr(&self, name: &str) -> String {
        self.doc.data().param_by_name(name).unwrap().expr.clone()
    }

    fn suppressed(&self, id: FeatureId) -> bool {
        self.doc.data().feature(id).unwrap().suppressed
    }

    /// 幅と四隅の穴のパターンの抑制を列にし、「小」(幅 40・穴なし)と「大」(幅 80・穴あり)の 2 行を作る。
    fn two_rows(&mut self) {
        self.run(ADD_COLUMN, json!({"param": "width"})).unwrap();
        self.run(ADD_COLUMN, json!({"feature": self.holes.0}))
            .unwrap();
        self.run(ADD_ROW, json!({"name": "小"})).unwrap();
        self.run(ADD_ROW, json!({"name": "大"})).unwrap();
        self.run(
            SET_CELL,
            json!({"row": "小", "column": 1, "value": "40 mm"}),
        )
        .unwrap();
        self.run(SET_CELL, json!({"row": "小", "column": 2, "value": "抑制"}))
            .unwrap();
        self.run(
            SET_CELL,
            json!({"row": "大", "column": 1, "value": "80 mm"}),
        )
        .unwrap();
    }
}

#[test]
fn activating_a_row_writes_its_values_and_one_undo_returns() {
    let mut t = T::new();
    t.two_rows();
    let c = &jc_interchange::configuration::of(t.doc.data());
    assert_eq!(c.columns.len(), 2);
    assert_eq!(c.rows.len(), 2);
    // 行を足した時は今の値が入る(幅 60・穴あり)。直すまで形は変わらない
    assert_eq!(t.expr("width"), "60 mm");
    t.run(ACTIVATE, json!({"row": "小"})).unwrap();
    assert_eq!(t.expr("width"), "40 mm");
    assert!(t.suppressed(t.holes));
    assert_eq!(t.report.error_count(), 0, "{:?}", t.report.features);
    // 最後の機能を抑制しても、その前のボディが残る
    let bodies = jc_core::visible_bodies(t.doc.data(), &t.modules.registry, &t.report);
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        jc_interchange::configuration::of(t.doc.data())
            .active
            .as_deref(),
        Some("小")
    );
    assert!(jc_interchange::configuration::of(t.doc.data()).matches(t.doc.data(), "小"));
    t.run(ACTIVATE, json!({"row": "大"})).unwrap();
    assert_eq!(t.expr("width"), "80 mm");
    assert!(!t.suppressed(t.holes));
    // 1 回の取り消しで、前の構成の値と当てた行に戻る
    assert!(t.doc.undo());
    assert_eq!(t.expr("width"), "40 mm");
    assert!(t.suppressed(t.holes));
    assert_eq!(
        jc_interchange::configuration::of(t.doc.data())
            .active
            .as_deref(),
        Some("小")
    );
}

#[test]
fn a_changed_value_is_written_back_to_the_active_row() {
    let mut t = T::new();
    t.two_rows();
    t.run(ACTIVATE, json!({"row": "小"})).unwrap();
    let id = t.doc.data().param_by_name("width").unwrap().id;
    t.doc
        .apply(jc_core::Transaction::new(
            "幅を直す",
            vec![jc_core::Op::SetParamExpr {
                id,
                expr: "45 mm".into(),
            }],
        ))
        .unwrap();
    assert!(!jc_interchange::configuration::of(t.doc.data()).matches(t.doc.data(), "小"));
    // 行を省くと当てた行に書く
    t.run(UPDATE_ROW, json!({})).unwrap();
    let c = &jc_interchange::configuration::of(t.doc.data());
    assert_eq!(
        c.row("小").unwrap().cells[0],
        jc_interchange::configuration::ConfigCell::Expr("45 mm".into())
    );
    assert!(c.matches(t.doc.data(), "小"));
}

#[test]
fn bad_input_is_refused_with_a_reason() {
    let mut t = T::new();
    // 行が無い時は当てられない
    assert!(t.run(ACTIVATE, json!({"row": "小"})).is_err());
    t.two_rows();
    let e = t.run(ADD_ROW, json!({"name": "小"})).unwrap_err();
    assert!(e.contains("既に"), "{e}");
    let e = t.run(ADD_COLUMN, json!({"param": "width"})).unwrap_err();
    assert!(e.contains("既に") || e.contains("ある"), "{e}");
    let e = t.run(ADD_COLUMN, json!({"param": "nothing"})).unwrap_err();
    assert!(e.contains("nothing"), "{e}");
    let e = t
        .run(
            SET_CELL,
            json!({"row": "小", "column": 2, "value": "たぶん"}),
        )
        .unwrap_err();
    assert!(e.contains("抑制"), "{e}");
    let e = t
        .run(SET_CELL, json!({"row": "小", "column": 9, "value": "1 mm"}))
        .unwrap_err();
    assert!(e.contains("列 9"), "{e}");
    let e = t.run(ACTIVATE, json!({"row": "中"})).unwrap_err();
    assert!(e.contains("中"), "{e}");
}

#[test]
fn removing_rows_and_columns_keeps_the_design() {
    let mut t = T::new();
    t.two_rows();
    t.run(ACTIVATE, json!({"row": "小"})).unwrap();
    t.run(REMOVE_COLUMN, json!({"column": 2})).unwrap();
    let c = &jc_interchange::configuration::of(t.doc.data());
    assert_eq!(c.columns.len(), 1);
    assert!(c.rows.iter().all(|r| r.cells.len() == 1));
    // 抑制は外した列のまま残る(列を外しても設計は変えない)
    assert!(t.suppressed(t.holes));
    t.run(REMOVE_ROW, json!({"row": "小"})).unwrap();
    let c = &jc_interchange::configuration::of(t.doc.data());
    assert_eq!(c.rows.len(), 1);
    assert_eq!(c.active, None);
    assert_eq!(t.expr("width"), "40 mm");
}

#[test]
fn the_actions_stay_out_of_feature_panels() {
    let t = T::new();
    let base = Context::new(
        t.doc.data(),
        &t.report,
        &t.modules.registry,
        &t.modules.kernel,
    );
    let cx = Context {
        selected: Some(t.holes),
        ..Context::new(
            t.doc.data(),
            &t.report,
            &t.modules.registry,
            &t.modules.kernel,
        )
    };
    assert_eq!(ConfigCommands.actions(&base).len(), 7);
    assert!(ConfigCommands.actions(&cx).is_empty());
}
