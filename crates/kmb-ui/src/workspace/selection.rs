//! 選ぶことと、機能の編集の始めと終わり(新しく作っている機能の決定・取り消しを含む)。

use crate::*;

impl Workspace {
    /// 機能を 1 つだけ選ぶ(一緒に選んでいたものは外す)。
    pub fn select(&mut self, id: FeatureId) {
        self.cur.selected_component = None;
        self.cur.browser_bodies = false;
        self.cur.also.clear();
        self.cur.range_anchor = Some(id);
        self.cur.origin_pick = None;
        self.win.show_params = false;
        self.cur.multi_edit = false;
        if self.cur.selected == Some(id) {
            return;
        }
        // 別のものを選んだら、作っている途中のものは決め、直すのは終える(直した所は残す)
        self.commit_creation();
        self.cur.editing = None;
        self.cur.selected = Some(id);
        self.cur.edit_start = Some((id, self.cur.work.doc().undo_labels().len()));
        for e in &mut self.cur.editors {
            e.reset();
        }
        self.cur.data_edit = None;
    }

    /// 部品の行を選ぶ。作業先と文書は変えず、自動分解へ渡す対象を覚える。
    pub(crate) fn select_component_row(&mut self, id: u64) {
        if self.cur.work.doc().data().component(id).is_none() {
            return;
        }
        self.clear_selection();
        self.cur.picked = None;
        self.cur.also_picked.clear();
        self.cur.picked_edges.clear();
        self.cur.picked_vertices.clear();
        self.cur.selected_component = Some(id);
    }

    /// ブラウザのボディの行を押した時に選ぶ(`add` なら一緒に選ぶ物へ足す)。
    pub fn select_body_row(&mut self, id: FeatureId, add: bool) {
        let more = add && (self.cur.browser_bodies || self.cur.selected.is_none());
        if add {
            self.toggle_select(id);
        } else {
            self.select(id);
        }
        // 前の選択もボディの行の時だけ、足した物をボディとして扱う(タイムラインの機能と混ぜない)
        self.cur.browser_bodies = !add || more;
    }

    /// 何も選ばない。
    pub fn clear_selection(&mut self) {
        self.cur.selected_component = None;
        self.cur.browser_bodies = false;
        self.cur.origin_pick = None;
        self.commit_creation();
        self.cur.editing = None;
        self.cur.multi_edit = false;
        self.cur.selected = None;
        self.cur.also.clear();
        self.cur.edit_start = None;
        for e in &mut self.cur.editors {
            e.reset();
        }
        self.cur.data_edit = None;
    }

    /// 機能 `id` を編集パネルで直し始める(タイムライン・ブラウザのダブルクリック、右クリックの「編集」)。
    /// 選んだだけでは直さないので、新しく作る操作や選び直しで既存の機能が変わることはない。
    pub fn open_edit(&mut self, id: FeatureId) {
        if self.cur.work.doc().data().feature(id).is_none() {
            return;
        }
        // 引いて動かした位置を決めていなければ、先に残すか戻すかを聞く(開くと値が変わることがある)
        if self.cur.editing != Some(id) && self.hold_for_moved(joint_drag_ui::Held::Edit(id)) {
            return;
        }
        self.select(id);
        if self.cur.editing != Some(id) {
            self.cur.editing = Some(id);
            self.cur.edit_start = Some((id, self.cur.work.doc().undo_labels().len()));
            // 履歴の印より後の機能は計算されないので、直しても形に出ない。印をその機能の後ろまで
            // 進める。直し始めの後に入れるので、キャンセルで印も戻る
            let data = self.cur.work.doc().data();
            if data
                .features
                .iter()
                .position(|f| f.id == id)
                .is_some_and(|k| k >= data.active_len())
                && let Ok(tx) = edit::roll_back_after(data, Some(id))
            {
                log::info!("履歴の印を、直す機能 {id:?} の後ろまで進める");
                self.apply(tx);
            }
        }
    }

    /// 直しているのを終える(直した所は残す。OK と同じ)。
    pub fn finish_edit(&mut self) {
        self.commit_creation();
        self.cur.editing = None;
        self.cur.multi_edit = false;
        if let Some(id) = self.cur.selected {
            self.cur.edit_start = Some((id, self.cur.work.doc().undo_labels().len()));
        }
    }

    /// 直したところを捨てて、直し始める前へ戻す(キャンセル)。作っている途中なら作る前へ戻す。
    pub fn cancel_edit(&mut self) {
        if self.cur.creating.is_some() {
            self.cancel_creation();
            return;
        }
        if let (Some(id), Some((sid, start))) = (self.cur.editing, self.cur.edit_start)
            && id == sid
        {
            let now = self.cur.work.doc().undo_labels().len();
            if now > start {
                self.go_to_history(start);
            }
        }
        self.cur.editing = None;
        self.cur.multi_edit = false;
        if let Some(id) = self.cur.selected {
            self.cur.edit_start = Some((id, self.cur.work.doc().undo_labels().len()));
        }
    }

    /// 機能 `id` を直しているか(作っている途中も含む)。
    pub fn is_editing(&self, id: FeatureId) -> bool {
        self.cur.editing == Some(id)
    }

    /// 作っている途中の機能(OK かキャンセルを待っている)。
    pub fn creating(&self) -> Option<FeatureId> {
        self.cur.creating.map(|c| c.0)
    }

    /// 作っている途中のものを決める(なければ何もしない)。
    pub fn commit_creation(&mut self) {
        self.cur.focus_new.set(None);
        if let Some((id, _)) = self.cur.creating.take() {
            self.cur.work.end_creation();
            // 作った後に続けて足していた手順(同じ穴を足す、など)も終える
            self.cur.gesture = None;
            if self.cur.editing == Some(id) {
                self.cur.editing = None;
            }
            self.cur.edit_start = Some((id, self.cur.work.doc().undo_labels().len()));
        }
    }

    /// 作っている途中のものを捨てる(作る前の履歴へ戻す。やり直しで戻せる)。
    pub fn cancel_creation(&mut self) {
        self.cur.focus_new.set(None);
        if let Some((_, before)) = self.cur.creating.take() {
            self.cur.work.end_creation();
            self.cur.gesture = None;
            self.cur.editing = None;
            self.cur.selected = None;
            self.cur.also.clear();
            self.cur.edit_start = None;
            for e in &mut self.cur.editors {
                e.reset();
            }
            self.cur.data_edit = None;
            if self.cur.work.doc().undo_labels().len() > before {
                self.go_to_history(before);
                // やめたものは、やり直しでも出てこない
                self.cur.work.clear_redo();
            }
        }
    }

    /// 消えた機能を指したままの編集の状態を外す(取り消しなどで消えた時)。
    pub(crate) fn drop_stale_edit(&mut self) {
        let data = self.cur.work.doc().data();
        let stale = self.cur.creating.is_some_and(|(id, before)| {
            data.feature(id).is_none() || self.cur.work.doc().undo_labels().len() < before
        });
        if self
            .cur
            .editing
            .is_some_and(|id| data.feature(id).is_none())
        {
            self.cur.editing = None;
        }
        if stale {
            self.cur.creating = None;
            self.cur.work.end_creation();
        }
    }

    /// Ctrl: 選びに足す・外す。
    pub fn toggle_select(&mut self, id: FeatureId) {
        self.cur.selected_component = None;
        self.cur.browser_bodies = false;
        self.commit_creation();
        self.cur.editing = None;
        self.cur.multi_edit = false;
        if self.cur.selected == Some(id) {
            self.cur.selected = self.cur.also.pop();
        } else if let Some(i) = self.cur.also.iter().position(|x| *x == id) {
            self.cur.also.remove(i);
        } else if let Some(s) = self.cur.selected {
            self.cur.also.push(s);
            self.cur.selected = Some(id);
            self.cur.range_anchor = Some(id);
        } else {
            self.select(id);
        }
    }

    /// Shift: 起点(最後に普通に押した物)から `id` までをタイムラインの順で選ぶ。
    /// 続けて Shift で押し直しても起点は動かず、範囲だけが替わる(ファイルの一覧などと同じ)。
    pub fn select_range(&mut self, id: FeatureId) {
        self.cur.selected_component = None;
        self.cur.browser_bodies = false;
        self.commit_creation();
        self.cur.editing = None;
        self.cur.multi_edit = false;
        // 起点がもう選びに無ければ(ほかの道で選びが替わった)、今選んでいる物を起点にする
        let anchor = self
            .cur
            .range_anchor
            .filter(|a| self.cur.selected == Some(*a) || self.cur.also.contains(a))
            .or(self.cur.selected);
        let data = self.cur.work.doc().data();
        let (Some(a), Some(b)) = (anchor.and_then(|s| data.index_of(s)), data.index_of(id)) else {
            self.select(id);
            return;
        };
        self.cur.range_anchor = anchor;
        let (lo, hi) = (a.min(b), a.max(b));
        self.cur.also = data.features[lo..=hi]
            .iter()
            .map(|f| f.id)
            .filter(|f| *f != id)
            .collect();
        self.cur.selected = Some(id);
    }

    /// 選んでいる機能(タイムラインの順)。
    /// 機能も面も選んでいないか。
    pub(crate) fn selection_is_empty(&self) -> bool {
        self.selection().is_empty() && self.picked_faces().is_empty()
    }

    pub fn selection(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = self
            .cur
            .also
            .iter()
            .copied()
            .chain(self.cur.selected)
            .collect();
        let data = self.cur.work.doc().data();
        v.sort_by_key(|id| data.index_of(*id).unwrap_or(usize::MAX));
        v.dedup();
        v
    }

    /// 選んでいる面(最後に選んだものが後ろ)。
    pub fn picked_faces(&self) -> Vec<view3d::Pick> {
        self.cur
            .also_picked
            .iter()
            .cloned()
            .chain(self.cur.picked.clone())
            .collect()
    }

    pub fn selected(&self) -> Option<FeatureId> {
        self.cur.selected
    }

    pub(crate) fn clear_edits(&mut self) {
        self.cur.expr_edits.clear();
        self.cur.name_edits.clear();
    }
}
