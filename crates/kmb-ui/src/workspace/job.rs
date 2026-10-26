//! 裏の再計算: 文書が変わったら kmb-session の裏の再計算を始め、毎コマ結果を受け取る。

use crate::*;

impl Workspace {
    /// 文書が変わった後の再計算。計算は kmb-session の裏の再計算で行う(CLI・MCP と同じ部品)。
    /// 画面を出さない時(試験・書き出し)は、その場で終わるまで待つ。
    pub(crate) fn recompute(&mut self) {
        self.start_recompute();
        if !self.win.background {
            self.wait();
        }
    }

    /// 計算が要る印を付け、走っていなければ裏で始める(走っていれば、終わった後でやり直す)。
    /// 寸法の名前(d1 など)は kmb-session が文書を写す前にそろえる。
    pub(crate) fn start_recompute(&mut self) {
        // 文書が変わった(取り消し・やり直しも): リンクの様子を次の描画で調べ直す
        self.cur.links_at = None;
        // 目は文書の区画なので、取り消し・やり直しでも読み直す(形を計算し直さない変更もある)
        self.refresh_hidden();
        // 出している形は、次の計算が失敗しても出し続けられるよう、カーネルに残させる(M27)
        let shown: Vec<ShapeHandle> = self.cur.scene.key().iter().map(|(_, h)| *h).collect();
        self.cur.work.keep_shapes(shown);
        self.cur.work.recompute();
        if !self.cur.work.needs_recompute() {
            return;
        }
        // 網を作り直すかは結果を見ないと分からないので、今の表示の鍵を渡す
        let scene_key = self.cur.scene.key().to_vec();
        let tolerance = self.cur.scene.tolerance();
        let after: kmb_session::After = Box::new(move |kernel, bodies| {
            (scene_key.as_slice() != bodies).then(|| {
                Box::new(view3d::build_meshes_at(kernel, bodies, tolerance))
                    as Box<dyn std::any::Any + Send>
            })
        });
        // 画面を出さない時はその場で待つので、描き直しを頼まない(待った後の描画で出る)
        let wake = self
            .win
            .repaint
            .clone()
            .filter(|_| self.win.background)
            .map(|ctx| Box::new(move || ctx.request_repaint()) as Box<dyn FnOnce() + Send>);
        self.cur.work.start_recompute(after, wake);
        self.cur.busy_since.get_or_insert_with(Instant::now);
    }

    /// 新しい計算の結果(`self.cur.work.report()`)で、形を出し直さずに前の形を出し続けるか(M27)。
    /// `old` は前の計算の結果。直した機能の計算が失敗した時(前の結果では計算できていた、または
    /// 前から出し続けている)は、前に計算できた形を出し続ける(入れた値で形が
    /// 消えない)。機能を足した・消した・並べ替えた時、選ぶ欄が受けている間、文書を入れ替えた時は
    /// 出し直す。編集パネルで直している機能の失敗も出し直す(欄に形が作れない値を打ったら予告の
    /// 形を消す。採用分の点検 B14)。
    pub(crate) fn hold_scene(&mut self, old: &RecomputeReport) -> bool {
        let fresh = std::mem::take(&mut self.cur.scene_fresh);
        let new = self.cur.work.report();
        let same_features = old.features.len() == new.features.len()
            && old
                .features
                .iter()
                .zip(&new.features)
                .all(|(a, b)| a.id == b.id);
        let failed: Vec<FeatureId> = new
            .features
            .iter()
            .filter(|r| matches!(r.status, FeatureStatus::Error { .. }))
            // 形を作る機能(ソリッド)の失敗だけ。加工の操作が失敗しても形は今のものを出す
            .filter(|r| self.is_copyable_feature(r.id))
            .filter(|r| self.cur.editing != Some(r.id))
            .filter(|r| {
                self.cur.held.contains(&r.id)
                    || matches!(old.feature(r.id), Some(FeatureStatus::Ok { .. }))
            })
            .map(|r| r.id)
            .collect();
        if fresh
            || !same_features
            || failed.is_empty()
            || self.cur.scene_before.is_some()
            || self.cur.scene.key().is_empty()
        {
            if !self.cur.held.is_empty() {
                log::info!("前の形を出すのをやめ、今の計算の形を出す");
            }
            self.cur.held.clear();
            return false;
        }
        if self.cur.held != failed {
            log::info!("計算に失敗した機能 {failed:?} があるので、前に計算できた形を出し続ける");
        }
        self.cur.held = failed;
        true
    }

    /// 計算に失敗して、前に計算できた形を出し続けている機能(M27。出し直していれば空)。
    pub fn held_features(&self) -> &[FeatureId] {
        &self.cur.held
    }

    /// 走っている再計算を、もう要らないものとしてやめさせる(次の機能の前で止まる)。
    /// 取り消し・やり直し・ロールバックの棒のように、行き先の形だけが要る時に呼ぶ。
    /// 終わった後で今の文書を計算し直す。値を続けて変える時(ドラッグなど)は呼ばない
    /// (途中の形を出し続けるため)。
    pub(crate) fn supersede_job(&mut self) {
        self.cur.work.supersede_recompute();
    }

    /// 裏の再計算の進み具合(通った機能の数、全部の数)。走っていなければ `None`。
    pub fn progress(&self) -> Option<(usize, usize)> {
        self.cur.work.recompute_progress()
    }

    /// 裏の再計算が続けて走っている秒数(走っていなければ `None`)。
    pub fn busy_for(&self) -> Option<std::time::Duration> {
        self.cur.busy_since.map(|t| t.elapsed())
    }

    /// 別のスレッドの再計算が終わっていれば結果を取り込む。毎フレーム呼ぶ。
    pub(crate) fn poll_job(&mut self) {
        let polled = self.cur.work.poll_recompute();
        self.absorb(polled);
    }

    pub(crate) fn absorb(&mut self, polled: kmb_session::Polled) {
        use kmb_session::Polled;
        let r = match polled {
            Polled::Idle | Polled::Running => return,
            Polled::Lost => {
                // 計算中に止まった。キャッシュは失われたので、次の再計算は最初から行う
                self.cur.busy_since = None;
                self.win.fail("再計算が途中で止まりました");
                return;
            }
            Polled::Dropped => {
                // やめた計算か、入れ替える前の文書の計算: 今の文書で計算し直す
                self.start_recompute();
                return;
            }
            Polled::Done(r) => r,
        };
        let hold = self.hold_scene(&r.previous);
        self.table_stale();
        if hold {
            // 計算に失敗した: 前に計算できた形のまま(M27)
        } else if let Some(until) = self.cur.scene_before {
            // 選ぶ欄が受けている間は、その機能の手前の形のまま(select_ui)
            let bodies = kmb_core::visible_bodies_before(
                self.cur.work.doc().data(),
                &self.win.registry,
                self.cur.work.report(),
                Some(until),
            );
            self.cur
                .scene
                .update(self.cur.work.kernel().as_ref(), &bodies);
        } else if let Some(meshes) = r
            .extra
            .and_then(|e| e.downcast::<Vec<(FeatureId, Mesh)>>().ok())
        {
            self.cur.scene.set(&r.bodies, *meshes);
        } else if self.cur.scene.needs(&r.bodies) {
            self.cur
                .scene
                .update(self.cur.work.kernel().as_ref(), &r.bodies);
        }
        // ボディの最後の機能が替わったら、隠したボディを新しい最後の機能で隠し直す
        self.refresh_hidden();
        self.settle_active();
        if self.cur.work.needs_recompute() {
            // 計算の間に文書が変わった
            self.start_recompute();
        }
        if !self.cur.work.is_recomputing() {
            self.cur.busy_since = None;
        }
    }

    /// 再計算が走っているか。
    pub fn is_busy(&self) -> bool {
        self.cur.work.is_recomputing()
    }

    /// 走っている再計算が(やり直しも含めて)終わるまで待つ。
    pub fn wait(&mut self) {
        while self.cur.work.is_recomputing() {
            let polled = self.cur.work.wait_recompute();
            self.absorb(polled);
        }
    }
}
