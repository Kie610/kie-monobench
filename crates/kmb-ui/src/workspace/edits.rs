//! 文書を変える操作(取り消せる 1 単位): 機能のデータ・抑制・パラメータ・参照の付け替え、取り消しとやり直し。

use crate::*;

impl Workspace {
    /// 参照が切れて止まった機能を、選んでいる平らな面に付け替えられるか
    /// (記述子に付け替えの方法がある種類: 面の上のスケッチ、ポケット、輪郭、フィレットなど)。
    pub fn can_repair_with_picked(&self, id: FeatureId) -> bool {
        let Some(entry) = self.cur.work.doc().data().feature(id) else {
            return false;
        };
        let repairable = self
            .win
            .registry
            .descriptor(&entry.type_id)
            .and_then(|d| d.retarget)
            .is_some_and(|r| r.applies(&entry.data));
        repairable
            && matches!(
                self.cur.work.report().feature(id),
                Some(FeatureStatus::Error { .. })
            )
            && self.picked_face_is_planar()
    }

    /// 計算できている機能(面取り加工、輪郭など)の面を、選んでいる平らな面に替えられるか。
    /// 選んだ面が今の面と同じなら替えない(Z11: 健全な時は付け替えのボタンが出なかった)。
    pub fn can_retarget_to_picked(&self, id: FeatureId) -> bool {
        let Some(entry) = self.cur.work.doc().data().feature(id) else {
            return false;
        };
        let Some(d) = self.win.registry.descriptor(&entry.type_id) else {
            return false;
        };
        let now = d.upgraded(&entry.data);
        let Some(r) = d.retarget.filter(|r| r.applies(&now)) else {
            return false;
        };
        let Some(p) = &self.cur.picked else {
            return false;
        };
        let mut data = now.clone();
        r.apply(&mut data, p.feature.0, &p.face);
        matches!(
            self.cur.work.report().feature(id),
            Some(FeatureStatus::Ok { .. })
        ) && self.picked_face_is_planar()
            && data != now
    }

    /// 計算できている機能の面を、選んでいる面に替える。
    pub fn retarget_to_picked(&mut self, id: FeatureId) -> bool {
        self.can_retarget_to_picked(id) && self.retarget_with_picked(id)
    }

    /// 参照が切れた機能の面を、選んでいる面に付け替える。
    pub fn repair_with_picked(&mut self, id: FeatureId) -> bool {
        self.can_repair_with_picked(id) && self.retarget_with_picked(id)
    }

    pub(crate) fn retarget_with_picked(&mut self, id: FeatureId) -> bool {
        let (Some(p), Some(entry)) = (
            self.cur.picked.clone(),
            self.cur.work.doc().data().feature(id).cloned(),
        ) else {
            return false;
        };
        let Some(d) = self.win.registry.descriptor(&entry.type_id) else {
            return false;
        };
        let Some(r) = d.retarget.clone() else {
            return false;
        };
        let mut data = d.upgraded(&entry.data);
        r.apply(&mut data, p.feature.0, &p.face);
        self.cur.picked = None;
        let mut tx = edit::set_feature_data(self.cur.work.doc().data(), id, data);
        tx.label = "参照を付け替え".into();
        self.apply(tx)
    }

    /// 文書を変える(1 回の取り消し単位)。変更は kmb-session の部品で作る(CLI・MCP と同じ)。
    pub(crate) fn apply(&mut self, tx: Transaction) -> bool {
        let r = self.cur.work.apply(tx);
        self.after_change(r)
    }

    /// [`Workspace::apply`] と同じだが、走っている再計算はやめさせる(ロールバックの棒など、
    /// 行き先の形だけが要る変更)。この変更で始まる計算はやめないよう、先に印を立てる。
    pub(crate) fn apply_superseding(&mut self, tx: Transaction) -> bool {
        self.supersede_job();
        self.apply(tx)
    }

    /// 機能のデータを置き換える(1 回の取り消し単位)。
    pub fn set_feature_data(&mut self, id: FeatureId, data: serde_json::Value) -> bool {
        self.apply(edit::set_feature_data(self.cur.work.doc().data(), id, data))
    }

    pub(crate) fn after_change(&mut self, result: Result<(), String>) -> bool {
        match result {
            Ok(()) => {
                self.cur.modified = self.differs_from_saved();
                self.win.message = None;
                self.recompute();
                true
            }
            Err(e) => {
                self.win.fail(e.to_string());
                false
            }
        }
    }

    /// 機能を抑制する(または戻す)。後ろの機能は、参照が切れれば理由つきで止まる。
    pub fn set_suppressed(&mut self, id: FeatureId, suppressed: bool) -> bool {
        self.apply(edit::set_suppressed(&[id], suppressed))
    }

    pub fn set_param_expr(&mut self, id: ParamId, expr: &str) -> bool {
        self.apply(edit::set_param_expr(self.cur.work.doc().data(), id, expr))
    }

    pub fn rename_param(&mut self, id: ParamId, name: &str) -> bool {
        let r = self.cur.work.rename_param_id(id, name);
        self.after_change(r)
    }

    /// パラメータを消す(取り消せる)。式で使われていれば消さず、理由を出す。
    pub fn remove_param(&mut self, id: ParamId) -> bool {
        let users = edit::param_users(self.cur.work.doc().data(), &self.win.registry, id);
        if !users.is_empty() {
            self.win
                .fail(format!("{}が使っているので消せません", users.join("、")));
            return false;
        }
        log::info!("パラメータを消す: {id:?}");
        let tx = edit::remove_param(self.cur.work.doc().data(), id);
        let ok = self.apply(tx);
        if ok {
            self.cur.name_edits.remove(&id);
            self.cur.expr_edits.remove(&id);
        }
        ok
    }

    pub fn add_param(&mut self, name: &str, expr: &str) -> bool {
        self.add_param_with(name, expr, kmb_core::ParamUnit::Any)
    }

    /// 単位つきでパラメータを足す(パラメータの表の追加の行)。
    pub fn add_param_with(&mut self, name: &str, expr: &str, unit: kmb_core::ParamUnit) -> bool {
        // 寸法の名前(d1 など)と重ねない
        if self
            .cur
            .work
            .report()
            .model_params
            .iter()
            .any(|p| p.name == name)
        {
            self.win
                .fail(format!("寸法の名前 {name} と同じ名前は使えません"));
            return false;
        }
        log::info!("パラメータを足す: {name} = {expr}({unit:?})");
        let tx = self
            .cur
            .work
            .prepare(|d| edit::add_param_with(d, name, expr, unit));
        self.apply(tx)
    }

    /// 寸法の名前(モデルのパラメータ)を変え、参照する式も書き換える。
    pub fn rename_model_param(&mut self, old: &str, new: &str) -> bool {
        let r = self.cur.work.rename_model_param(old, new);
        self.after_change(r)
    }

    /// 寸法の名前 `name` の式を変える(パラメータの一覧から寸法の値を直す)。
    pub fn set_model_param(&mut self, name: &str, expr: &str) -> bool {
        let r = self.cur.work.set_model_param(name, expr);
        self.after_change(r)
    }

    pub fn undo(&mut self) {
        // ホームでは文書でなく、直前の仕分け(移す・名前・ごみ箱・分類)を戻す
        if self.home.start {
            self.undo_files();
            return;
        }
        if self.editor_undo_step() {
            return;
        }
        if self.cur.work.undo().is_ok() {
            self.supersede_job();
            self.cur.modified = self.differs_from_saved();
            self.clear_edits();
            self.recompute();
        }
        self.drop_stale_edit();
    }

    /// 中央のビューを持つ編集器が、描いている途中の 1 段を戻したか(スプラインの最後の点など)。
    fn editor_undo_step(&mut self) -> bool {
        let Some(type_id) = self.view_editor_type() else {
            return false;
        };
        self.cur
            .editors
            .iter_mut()
            .find(|ed| ed.handles(&type_id) && ed.takes_view())
            .is_some_and(|ed| ed.undo_step())
    }

    pub fn redo(&mut self) {
        if self.cur.work.redo().is_ok() {
            self.supersede_job();
            self.cur.modified = self.differs_from_saved();
            self.clear_edits();
            self.recompute();
        }
        self.drop_stale_edit();
    }

    /// 取り消しの履歴の位置へ移る。`n` は済んだ変更の数(0 なら最初の状態)。
    /// 何段戻っても進んでも、再計算は 1 回だけ。
    pub fn go_to_history(&mut self, n: usize) {
        let now = self.cur.work.doc().undo_labels().len();
        let mut moved = false;
        for _ in n..now {
            moved |= self.cur.work.undo().is_ok();
        }
        for _ in now..n {
            moved |= self.cur.work.redo().is_ok();
        }
        if moved {
            self.supersede_job();
            self.cur.modified = self.differs_from_saved();
            self.clear_edits();
            self.recompute();
        }
        self.drop_stale_edit();
    }
}
