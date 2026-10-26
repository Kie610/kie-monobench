//! モジュールの操作(拡張点)を画面から使う: 一覧、実行、結果の扱い、重ね描き、注意。
//! 画面は操作の中身を知らず、結果(文書の変更、ファイル、知らせ、時間のかかる仕事)だけを扱う。

use crate::Workspace;
use kmb_core::{Action, Context, FeatureId, Input, Outcome, Overlay};
use std::path::PathBuf;

/// [`Workspace::run_through`] の結果(足す前の取り消しの履歴の長さつき)。
pub(crate) struct Ran {
    pub(crate) handled: kmb_session::Handled,
    before: usize,
}

impl Workspace {
    /// 今の選択での文脈。
    pub(crate) fn context(&self) -> Context<'_> {
        self.context_with(self.cur.selected, None)
    }

    pub(crate) fn context_with<'a>(
        &'a self,
        selected: Option<FeatureId>,
        file: Option<&'a std::path::Path>,
    ) -> Context<'a> {
        Context {
            selected,
            picked: self.cur.picked.as_ref(),
            also_picked: &self.cur.also_picked,
            edges: &self.cur.picked_edges,
            vertices: &self.cur.picked_vertices,
            also_selected: &self.cur.also,
            bodies_picked: self.win.pick_filter.body || self.cur.browser_bodies,
            file,
            doc_path: self.cur.work.path(),
            doc: self.tab_uid(),
            ..Context::new(
                self.cur.work.doc().data(),
                self.cur.work.report(),
                &self.win.registry,
                self.cur.work.kernel(),
            )
        }
    }

    /// 画面のボタンから操作を始める。3D のビューで値を決める手順がある操作(押し出し、面に穴)は
    /// クリックで作る流れに入り、なければすぐ実行する。
    pub fn start_action(&mut self, id: &str) -> Option<FeatureId> {
        // 引いて動かした位置を決めていなければ、先に残すか戻すかを聞く
        if self.hold_for_moved(crate::joint_drag_ui::Held::Action(id.into())) {
            return None;
        }
        // 新しいコマンドを始めたら、作っている途中のものは決め、直すのは終える
        self.commit_creation();
        self.cur.editing = None;
        // 自動分解はすぐ実行せず、ビューの上に小さな道具を出す
        if self
            .actions()
            .iter()
            .any(|a| a.id == id && a.role == Some(kmb_interchange::role::action::AUTO_EXPLODE))
        {
            self.open_auto_explode(id);
            return None;
        }
        // 外観はすぐ実行せず、外観の窓を出す(窓で付ける・直すのもこの操作)
        if self
            .actions()
            .iter()
            .any(|a| a.id == id && a.role == Some(kmb_interchange::role::action::APPEARANCE))
        {
            self.open_looks();
            return None;
        }
        // モーション スタディはすぐ実行せず、スタディの窓を出す(窓のキーもこの操作で置く)
        if self
            .actions()
            .iter()
            .any(|a| a.id == id && a.role == Some(kmb_interchange::role::action::MOTION_STUDY))
        {
            self.open_motion_study();
            return None;
        }
        if self.begin_gesture(id) {
            None
        } else if self.win.layout.space == crate::layout::Space::Animation
            && (self.declares(id, "time") || self.declares(id, "view"))
        {
            // 段を足す操作は、段の帯の今の時刻と 3D のビューの今の視点を渡す
            let mut args = serde_json::Map::new();
            self.animation_args(&mut args);
            if self.declares(id, "view") {
                args.insert("view".into(), serde_json::json!(self.view_pose()));
            }
            self.run_action_args(id, serde_json::Value::Object(args))
        } else {
            self.run_action(id)
        }
    }

    /// 操作を、決まっている引数 `args`(オブジェクト)から始める。残りの手順があれば
    /// 3D のビューで決める流れに入り、なければすぐ実行する。
    pub fn start_action_with(&mut self, id: &str, args: serde_json::Value) -> Option<FeatureId> {
        if self.hold_for_moved(crate::joint_drag_ui::Held::ActionWith(
            id.into(),
            args.clone(),
        )) {
            return None;
        }
        self.commit_creation();
        self.cur.editing = None;
        let given = args.as_object().cloned().unwrap_or_default();
        if self.begin_gesture_with(id, given.clone()) {
            None
        } else {
            self.run_action_args(id, serde_json::Value::Object(given))
        }
    }

    /// 選んでいる機能を作っている途中・直している途中のまま、操作 `id` の 3D の手順を始める
    /// ([`kmb_core::Action::keep_edit`] の操作)。手順がなければすぐ実行する。
    pub fn begin_pick(&mut self, id: &str) {
        if !self.begin_gesture(id) {
            self.run_action(id);
        }
    }

    /// 操作 `id` の、3D のビューで値を決める手順を始める(手順がなければ `false`)。
    /// 作っている途中のものはそのまま(続けて足す操作は、それを選んだ文脈で始める)。
    fn begin_gesture(&mut self, id: &str) -> bool {
        self.begin_gesture_with(id, Default::default())
    }

    /// [`Self::begin_gesture`] を、決まっている引数から始める。引数にある手順は飛ばす。
    fn begin_gesture_with(
        &mut self,
        id: &str,
        given: serde_json::Map<String, serde_json::Value>,
    ) -> bool {
        let mut given = given;
        // 段の帯の時刻は、受け取る操作(分解の移動など)にだけ渡す
        if self.declares(id, "time") {
            self.animation_args(&mut given);
        }
        let args = serde_json::Value::Object(given.clone());
        let handles = {
            let cx = Context {
                args: &args,
                ..self.context()
            };
            self.win.extensions.handles(id, &cx)
        };
        let handles = handles.map(|h| self.without_hidden_choices(h));
        match handles {
            Some(h) if !h.is_empty() => {
                let label = self
                    .actions()
                    .into_iter()
                    .find(|a| a.id == id)
                    .map_or(id.to_string(), |a| a.label);
                let step = h.iter().take_while(|h| given.contains_key(&h.key)).count();
                if step >= h.len() {
                    return false;
                }
                // 候補を足す・外す手順は、前に選んだ物を選んだまま始める
                let picks = h[step].picked.clone();
                self.cur.gesture = Some(crate::gesture::Run {
                    action: id.into(),
                    label,
                    args: given,
                    handles: h,
                    step,
                    live: None,
                    typed: String::new(),
                    picks,
                });
                true
            }
            _ => false,
        }
    }

    /// 画面がどの操作にも渡す引数(隠したボディ・視点・時刻)のうち、操作 `id` が宣言する物だけ
    /// ([`kmb_core::Action::args`]。宣言に無い鍵を渡すと操作は誤りにする)。
    fn declared_args(
        &self,
        id: &str,
        mut given: serde_json::Map<String, serde_json::Value>,
    ) -> serde_json::Map<String, serde_json::Value> {
        if let Some(a) = self.actions().into_iter().find(|a| a.id == id) {
            given.retain(|k, _| a.args.iter().any(|d| &d.key == k));
        }
        given
    }

    /// 操作 `id` が引数 `key` を受け取るか。
    fn declares(&self, id: &str, key: &str) -> bool {
        self.actions()
            .iter()
            .any(|a| a.id == id && a.args.iter().any(|d| d.key == key))
    }

    /// 3D のビューで隠しているスケッチの候補(値の `sketch` がそのスケッチ)を手順から外す。
    fn without_hidden_choices(&self, mut hs: Vec<kmb_core::Handle>) -> Vec<kmb_core::Handle> {
        for h in &mut hs {
            h.choices.retain(|c| {
                c.value
                    .get("sketch")
                    .and_then(|v| v.as_u64())
                    .is_none_or(|id| !self.is_hidden(FeatureId(id)))
            });
        }
        hs
    }

    /// クリックで作る流れの今の手順を、値 `v` で決めて次へ進む。手順は決めた値で出し直す。
    pub(crate) fn gesture_commit_step(&mut self, v: serde_json::Value) {
        let Some(g) = &mut self.cur.gesture else {
            return;
        };
        let Some(h) = g.handles.get(g.step) else {
            return;
        };
        g.args.insert(h.key.clone(), v);
        g.step += 1;
        g.live = None;
        g.typed.clear();
        g.picks.clear();
        let args = serde_json::Value::Object(g.args.clone());
        let id = g.action.clone();
        let cx = Context {
            args: &args,
            ..self.context()
        };
        if let Some(h) = self
            .win
            .extensions
            .handles(&id, &cx)
            .map(|h| self.without_hidden_choices(h))
            && let Some(g) = &mut self.cur.gesture
        {
            g.picks = h.get(g.step).map(|h| h.picked.clone()).unwrap_or_default();
            g.handles = h;
        }
        // 最後の手順を決めたら、そのまま足す(決めた後にもう一度クリックや Enter を求めない)。
        // 残りの値(穴の深さや種類、フィレットの半径など)は「作っている途中」の欄で直す
        if self.cur.gesture.as_ref().is_some_and(|g| g.ready()) {
            self.gesture_finish();
        }
    }

    /// クリックで作る流れを終える(決めた値で操作を実行する)。足したものは「作っている途中」の
    /// ままにし、欄で値を直せるようにする(OK・Enter・別のものを選ぶと決まり、Esc でやめる)。
    pub(crate) fn gesture_finish(&mut self) -> Option<FeatureId> {
        let g = self.cur.gesture.take()?;
        let action = self.actions().into_iter().find(|a| a.id == g.action);
        let repeat = action.as_ref().is_some_and(|a| a.repeat);
        // くり返す操作は、省ける手順を値なしで決めたら(Enter)終える。足していたものも決める
        if repeat && g.args.values().any(serde_json::Value::is_null) {
            self.finish_edit();
            return None;
        }
        let added = self.run_action_args(&g.action, serde_json::Value::Object(g.args));
        if let Some(next) = action.as_ref().and_then(|a| a.then.clone())
            && added.is_some()
        {
            // 足した直後に続けて始める操作(作っている途中のまま)。手順は足したものの形から
            // 作るので、裏の再計算を待つ
            self.wait();
            self.begin_gesture(&next);
        } else if repeat {
            // 失敗しても(面の外をクリックした時など)、続けてクリックできるようにする
            self.wait();
            self.begin_gesture(&g.action);
        }
        added
    }

    /// 操作 `id` を、指定 `args`(CLI・MCP と同じ形)で実行する。
    pub fn run_action_args(&mut self, id: &str, args: serde_json::Value) -> Option<FeatureId> {
        match self.try_run_action_args(id, args) {
            Ok(added) => added,
            Err(e) => {
                self.win.fail(e);
                None
            }
        }
    }

    /// [`Self::run_action_args`] の、誤りを知らせに出さずに返す形(コマンド欄の `run`)。
    pub(crate) fn try_run_action_args(
        &mut self,
        id: &str,
        args: serde_json::Value,
    ) -> Result<Option<FeatureId>, String> {
        log::info!("操作 {id} を実行: {args}");
        let mut args = args;
        if let Some(roots) = &self.cmdline.ai_roots
            && let Some(a) = self.actions().iter().find(|a| a.id == id)
        {
            roots.confine_args(a, &mut args)?;
        }
        let before = self.settings_values();
        let r = self.run_through(id, self.cur.selected, None, &args);
        self.save_settings_if_changed(before);
        r.map(|ran| self.take_ran(ran, None))
    }

    /// 今使えるかどうかつきの、モジュールの操作。
    pub fn actions(&self) -> Vec<Action> {
        self.win.extensions.actions(&self.context())
    }

    /// 役割の印 `role` を持つ操作の名前(画面は機能の名前を書かず、役割で探す)。
    pub(crate) fn action_for_role(&self, role: &kmb_core::ActionRole) -> Option<String> {
        self.actions()
            .into_iter()
            .find(|a| a.role.as_ref() == Some(role))
            .map(|a| a.id)
    }

    /// 機能 `id` を選んだとした時の操作(右クリックのメニュー用)。
    pub(crate) fn actions_for(&self, id: FeatureId) -> Vec<Action> {
        self.win
            .extensions
            .actions(&self.context_with(Some(id), None))
    }

    /// 操作 `id` を実行する。ファイルを求める操作はダイアログで選ばせる。
    /// 機能を足したら、最後に足した機能を返す。
    pub fn run_action(&mut self, id: &str) -> Option<FeatureId> {
        let action = self.actions().into_iter().find(|a| a.id == id);
        let file: Option<PathBuf> = match action.as_ref().map(|a| &a.input) {
            Some(Input::OpenFile {
                title,
                filter,
                extensions,
            }) => {
                let exts: Vec<&str> = extensions.iter().map(String::as_str).collect();
                let Some(f) = self.win.dialogs.open_file(title, filter, &exts) else {
                    self.win.message =
                        Some(format!("{title}: ファイルが選ばれなかったので、やめました"));
                    return None;
                };
                Some(f)
            }
            Some(Input::SaveFile {
                title,
                filter,
                extensions,
            }) => {
                let exts: Vec<&str> = extensions.iter().map(String::as_str).collect();
                let ext = exts.first().copied().unwrap_or_default();
                let stem = self
                    .cur
                    .work
                    .path()
                    .and_then(|p| p.file_stem())
                    .map_or("無題".to_string(), |n| n.to_string_lossy().to_string());
                let name = format!("{stem}.{ext}");
                // 選ばずに閉じた時は、そうと知らせる。窓が出せなかった時は、その理由を出す
                let Some(f) = self.win.dialogs.save_file(title, filter, &exts, &name) else {
                    self.win.message = Some(match self.win.dialogs.unavailable() {
                        Some(why) => format!("{title}: 保存の窓を出せません({why})"),
                        None => format!("{title}: 保存先が選ばれなかったので、やめました"),
                    });
                    return None;
                };
                Some(f)
            }
            Some(Input::Name {
                title,
                label,
                default,
            }) => {
                // 名前を打つ窓を出し、OK で実行する
                self.ask_name(id, title, label, default);
                return None;
            }
            _ => None,
        };
        self.run_action_with(id, file)
    }

    /// 操作 `id` を、選んだファイル `file` で実行する(ダイアログを出さない)。
    /// 書き出しの結果は、`file` があればそこへ、なければ文書の隣に書く。
    pub fn run_action_with(&mut self, id: &str, file: Option<PathBuf>) -> Option<FeatureId> {
        // 書き出しは、目を閉じて隠したボディを除く
        let export = self
            .actions()
            .iter()
            .any(|a| a.id == id && a.role == Some(kmb_interchange::role::action::EXPORT_BODIES));
        let args = if export {
            let hidden: Vec<u64> = self
                .visible_bodies()
                .iter()
                .filter(|(f, _)| self.is_hidden(*f))
                .map(|(f, _)| f.0)
                .collect();
            if !hidden.is_empty() {
                log::info!("{id}: 隠したボディ {} 個を除いて書き出す", hidden.len());
            }
            // 目を閉じたスケッチも除く(今の視点の線は、表示中のスケッチも書く)
            let mut hidden = hidden;
            hidden.extend(
                self.cur
                    .work
                    .doc()
                    .data()
                    .features
                    .iter()
                    .filter(|f| {
                        self.is_hidden(f.id)
                            && self
                                .win
                                .registry
                                .descriptor(&f.type_id)
                                .is_some_and(|d| d.category == kmb_core::category::SKETCH)
                    })
                    .map(|f| f.id.0),
            );
            // 今の視点の DXF は、画面の視点の向きを渡す(平行に投影する)
            let (_, up, eye) = self.cur.camera.basis_pub();
            let f = |v: [f32; 3]| v.map(f64::from).to_vec();
            let mut args = serde_json::json!({
                "exclude": hidden,
                "view": {"eye": f(eye), "up": f(up)},
            });
            // 絵と動画は、画面のビューの視点そのもの(位置・画角・ずれ)も渡す(ビューを描いた後だけ)
            if self.win.view_rect.width() > 1.0 && self.win.view_rect.height() > 1.0 {
                // レンダリングの作業空間では静止画の縦横比の枠を写す(枠の比も渡す)
                let c = self.framed_render_camera(self.win.view_rect);
                let frame = self.render_frame(self.win.view_rect);
                args["camera"] = serde_json::json!({
                    "eye": f(c.eye), "right": f(c.right), "up": f(c.up),
                    "toward": f(c.toward_eye()), "fov": c.fov_y_deg,
                    "ortho_height": c.ortho_height, "shift": c.shift,
                    "focus": c.focus, "aspect": frame.width() / frame.height(),
                });
            }
            Some(serde_json::Value::Object(self.declared_args(
                id,
                args.as_object().cloned().unwrap_or_default(),
            )))
        } else if self.declares(id, "view") {
            // 今の視点を使う操作(下絵を見ている向きの平面に置くなど)には、視点の向きを渡す
            let (_, up, eye) = self.cur.camera.basis_pub();
            let f = |v: [f32; 3]| v.map(f64::from).to_vec();
            Some(serde_json::json!({"view": {"eye": f(eye), "up": f(up)}}))
        } else {
            None
        };
        // STEP の読み込みなら、読み込んだ後に立てるかを聞く(U12)
        let import = self
            .actions()
            .iter()
            .any(|a| a.id == id && a.role == Some(kmb_interchange::role::action::IMPORT_STEP));
        let before = self.settings_values();
        let args = args.unwrap_or(serde_json::Value::Null);
        let r = self.run_through(id, self.cur.selected, file.as_deref(), &args);
        self.save_settings_if_changed(before);
        match r {
            Ok(ran) => {
                let added = self.take_ran(ran, file.clone());
                if import && added.is_some() {
                    if self.win.layout.ask_step_up && added.is_some_and(|a| self.needs_step_up(a)) {
                        self.cur.step_up_ask = added;
                    }
                    // 大きな STEP は計算に時間がかかるので、進み具合と「キャンセル」を出す(U8)
                    if let Some(f) = &file {
                        self.watch_opening(f, crate::files_ui::Back::Undo);
                    }
                }
                added
            }
            Err(e) => {
                self.win.fail(e);
                None
            }
        }
    }

    /// 機能 `id` が、ほかの種類に続けて足される種類([`kmb_core::Descriptor::companion`])か。
    fn is_companion(&self, id: FeatureId) -> bool {
        let Some(t) = self
            .cur
            .work
            .doc()
            .data()
            .feature(id)
            .map(|f| f.type_id.clone())
        else {
            return false;
        };
        self.win
            .registry
            .descriptors()
            .iter()
            .any(|d| d.companion.as_ref().is_some_and(|(c, _)| *c == t))
    }

    /// 操作 `id` を、画面で選んでいる物で kmb-session の道([`kmb_session::Headless::run_with`])を
    /// 通して実行する。使えない操作は断り、文書の変更はそこで適用する(CLI・MCP と同じ道)。
    /// 結果は [`Self::take_ran`] で受け取る。
    pub(crate) fn run_through(
        &mut self,
        id: &str,
        selected: Option<FeatureId>,
        file: Option<&std::path::Path>,
        args: &serde_json::Value,
    ) -> Result<Ran, String> {
        // 作る前の履歴の長さ(作っている途中をキャンセルした時に戻る所)
        let before = self.cur.work.doc().undo_labels().len();
        let doc = self.tab_uid();
        // AI から来た頼みなら、操作が文書のデータから開くファイルも許したフォルダに限る
        let roots = self.ai_roots_now();
        let picks = kmb_session::Picks {
            selected,
            picked: self.cur.picked.as_ref(),
            also_picked: &self.cur.also_picked,
            edges: &self.cur.picked_edges,
            vertices: &self.cur.picked_vertices,
            also_selected: &self.cur.also,
            bodies_picked: self.win.pick_filter.body || self.cur.browser_bodies,
            doc,
            files: roots.as_ref().map(|r| r as &dyn kmb_core::FileGate),
        };
        let handled = self
            .cur
            .work
            .run_with(id, &picks, file, args)
            .map_err(|p| p.to_string())?;
        Ok(Ran { handled, before })
    }

    /// [`Self::run_through`] の結果を画面に受け取る。機能を足したら、最後に足した機能を返す。
    pub(crate) fn take_ran(&mut self, ran: Ran, file: Option<PathBuf>) -> Option<FeatureId> {
        match ran.handled {
            kmb_session::Handled::Changed(added) => {
                self.after_change(Ok(()));
                self.took_change(added, ran.before, file)
            }
            kmb_session::Handled::Other(o) => self.take_outcome(*o, file),
        }
    }

    /// 足した機能 `added` を受け取る: 最後に足した機能を選び、「作っている途中」にする(編集パネルで
    /// 値を直し、OK で決める)。`before` は足す前の取り消しの履歴の長さ。
    fn took_change(
        &mut self,
        added: Vec<FeatureId>,
        before: usize,
        file: Option<PathBuf>,
    ) -> Option<FeatureId> {
        self.cur.picked = None;
        self.cur.picked_edges.clear();
        self.cur.picked_vertices.clear();
        // 続けて足した種類(ボディの分割の残りの側など)でなく、それを足させた方を直す
        let last = added
            .iter()
            .rev()
            .copied()
            .find(|id| !self.is_companion(*id));
        if let Some(id) = last {
            self.select(id);
            // 足したものは「作っている途中」: 編集パネルで値を直し、OK で決める。
            // キャンセル・Esc なら作る前へ戻す(押しただけで形が決まらないように)
            self.cur.creating = Some((id, before));
            // 作る途中に輪郭などを選び替えたら、足した時に隠したスケッチを出し直す
            self.cur.work.begin_creation(id);
            self.cur.focus_new.set(Some(id));
            self.cur.editing = Some(id);
            self.cur.edit_start = Some((id, self.cur.work.doc().undo_labels().len()));
            if let Some(p) = file {
                self.report_imported(id, &p);
            }
        }
        last
    }

    pub(crate) fn take_outcome(&mut self, o: Outcome, file: Option<PathBuf>) -> Option<FeatureId> {
        match o {
            Outcome::Change { label, edits } => {
                let before = self.cur.work.doc().undo_labels().len();
                match self.cur.work.apply_edits(&label, edits) {
                    Ok(added) => {
                        self.after_change(Ok(()));
                        self.took_change(added, before, file)
                    }
                    Err(e) => {
                        self.after_change(Err(e));
                        None
                    }
                }
            }
            Outcome::File {
                suffix, contents, ..
            } => {
                match file {
                    Some(p) => self.write_to(&p, &contents),
                    None => self.write_beside(&suffix, &contents),
                }
                None
            }
            Outcome::Files { files, .. } => {
                self.write_files(file.as_deref(), &files);
                None
            }
            // 空の知らせは「言うことがない」(結果の表示の切り替えなど)。知らせの欄に空の行を残さない
            Outcome::Message(m) => {
                if !m.is_empty() {
                    self.win.message = Some(m);
                }
                None
            }
            Outcome::Text { title, body } => {
                self.cur.viewer = Some((title, body.lines().map(str::to_string).collect()));
                None
            }
            Outcome::Table { title, table } => {
                self.open_table(title, table);
                None
            }
            Outcome::Job(job) => {
                self.start_task(job);
                None
            }
        }
    }

    /// 読み込んだ形の外形の大きさを知らせる(単位の取り違えに気づけるように)。
    fn report_imported(&mut self, id: FeatureId, path: &std::path::Path) {
        self.wait();
        let size = self
            .visible_bodies()
            .into_iter()
            .find(|b| b.0 == id)
            .and_then(|(_, h)| self.cur.work.kernel().bounds(h).ok());
        self.win.message = Some(match size {
            Some((a, b)) => format!(
                "{} を読み込みました。外形 {:.2} × {:.2} × {:.2} mm",
                path.display(),
                b[0] - a[0],
                b[1] - a[1],
                b[2] - a[2]
            ),
            None => format!("{} を読み込みました", path.display()),
        });
    }

    fn write_to(&mut self, out: &std::path::Path, contents: &[u8]) {
        match kmb_session::write_output(out, contents) {
            Ok(()) => self.win.message = Some(format!("{} に書き出しました", out.display())),
            Err(e) => self.win.fail(format!("書き出せません: {e}")),
        }
    }

    /// 文書の隣に `<文書名>-<suffix>` で書き出す。文書がまだ保存先を持たなければ、
    /// 保存の窓で書き出す先を選ぶ(Z23: 「文書を保存先つきで開くと、書き出せます」と出て何もしなかった)。
    fn write_beside(&mut self, suffix: &str, contents: &[u8]) {
        let Some(path) = self.cur.work.path().map(std::path::Path::to_path_buf) else {
            let ext = suffix.rsplit_once('.').map_or("", |(_, e)| e);
            let exts: Vec<&str> = if ext.is_empty() { vec![] } else { vec![ext] };
            let name = format!("無題-{suffix}");
            match self
                .win
                .dialogs
                .save_file("書き出す", "書き出すファイル", &exts, &name)
            {
                Some(out) => self.write_to(&out, contents),
                None => {
                    self.win.message = Some(match self.win.dialogs.unavailable() {
                        Some(why) => format!("保存の窓を出せません({why})"),
                        None => "書き出す先が選ばれなかったので、やめました".into(),
                    })
                }
            }
            return;
        };
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let out = path.with_file_name(format!("{stem}-{suffix}"));
        self.write_to(&out, contents);
    }

    /// いくつものファイルを、選んだファイル `chosen`(なければ文書)の名前に `-<名前の後ろ>` を付けて書く。
    fn write_files(&mut self, chosen: Option<&std::path::Path>, files: &[(String, Vec<u8>)]) {
        let Some(base) = chosen
            .map(|p| p.to_path_buf())
            .or_else(|| self.cur.work.path().map(std::path::Path::to_path_buf))
        else {
            self.win.message = Some("文書を保存してから書き出してください".into());
            return;
        };
        let mut written = Vec::new();
        for (tail, contents) in files {
            let out = kmb_core::sibling_path(&base, tail);
            if let Err(e) = kmb_session::write_output(&out, contents) {
                self.win.fail(e.to_string());
                return;
            }
            written.push(out);
        }
        let names: Vec<String> = written
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        let dir = base
            .parent()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        self.win.message = Some(format!(
            "{dir} に {} 個書き出しました({})",
            names.len(),
            names.join("、")
        ));
    }

    /// 読むための文の窓(行番号つき、長い文も見える所だけ描く)。毎フレーム呼ぶ。
    pub(crate) fn text_viewer(&mut self, ctx: &egui::Context) {
        let Some((title, lines)) = &self.cur.viewer else {
            return;
        };
        let mut open = true;
        let digits = lines.len().max(1).to_string().len();
        egui::Window::new(title.as_str())
            .id(egui::Id::new("text_viewer"))
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(520.0, 480.0))
            .open(&mut open)
            .show(ctx, |ui| {
                let font = egui::TextStyle::Monospace;
                let row = ui.text_style_height(&font);
                egui::ScrollArea::both().auto_shrink(false).show_rows(
                    ui,
                    row,
                    lines.len(),
                    |ui, range| {
                        for k in range {
                            ui.label(
                                egui::RichText::new(format!("{:>digits$}  {}", k + 1, lines[k]))
                                    .monospace(),
                            );
                        }
                    },
                );
            });
        if !open {
            self.cur.viewer = None;
        }
    }

    /// 取り消しの履歴の窓(古い順。今の位置に印、やり直せるものは薄く)。毎フレーム呼ぶ。
    pub(crate) fn undo_history_window(&mut self, ctx: &egui::Context) {
        if !self.win.undo_history_open {
            return;
        }
        let done: Vec<String> = self
            .cur
            .work
            .doc()
            .undo_labels()
            .into_iter()
            .map(String::from)
            .collect();
        let undone: Vec<String> = self
            .cur
            .work
            .doc()
            .redo_labels()
            .into_iter()
            .map(String::from)
            .collect();
        let mut open = true;
        let mut go = None;
        egui::Window::new("取り消しの履歴")
            .id(egui::Id::new("undo_history"))
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(280.0, 320.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("行を押すと、その変更の後の状態に戻る(やり直しもできる)");
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let now = done.len();
                        let rows = std::iter::once("最初の状態")
                            .chain(done.iter().map(String::as_str))
                            .chain(undone.iter().map(String::as_str));
                        for (k, label) in rows.enumerate() {
                            let text = egui::RichText::new(format!("{k}. {label}"));
                            let text = if k > now { text.weak() } else { text };
                            if ui.selectable_label(k == now, text).clicked() && k != now {
                                go = Some(k);
                            }
                        }
                    });
            });
        if let Some(k) = go {
            self.go_to_history(k);
        }
        if !open {
            self.win.undo_history_open = false;
        }
    }

    /// モジュールの設定の今の値(操作が設定を変えたかを見るため)。
    fn settings_values(&self) -> Vec<(String, serde_json::Value)> {
        self.win
            .extensions
            .settings()
            .into_iter()
            .map(|s| (s.id, s.value))
            .collect()
    }

    /// 操作が設定を変えていれば(「パラメータを既定として保存」など)、データフォルダに書く。
    /// 安全モードでは書かない(設定の窓で変えた時と同じ)。
    fn save_settings_if_changed(&mut self, before: Vec<(String, serde_json::Value)>) {
        if self.settings_values() == before || self.win.safe_mode {
            return;
        }
        if let Some(d) = &self.win.data_dir
            && let Err(e) = kmb_session::save_settings(&self.win.extensions, d)
        {
            log::warn!("設定を書けない: {e}");
            self.win.fail(e);
        }
    }

    /// 設定を変え、データフォルダがあれば書く。文書ごとに持つ設定(レンダリングのシーン)は今の
    /// 文書にも書く(取り消せる)。正しくない値なら何も変えずに知らせる。
    pub fn change_settings(&mut self, id: &str, value: &serde_json::Value) -> bool {
        // AI が画面の欄に打った値なら、ファイルの欄(ポストのプラグインなど)を AI に許したフォルダに限る
        let r = match self.ai_roots_now() {
            Some(roots) => self.cur.work.set_settings_in(id, value, &roots),
            None => self.cur.work.set_settings(id, value),
        };
        match r {
            Err(e) => {
                self.win.fail(e);
                return false;
            }
            Ok(true) => {
                self.after_change(Ok(()));
            }
            Ok(false) => {}
        }
        self.win.message = match &self.win.data_dir {
            _ if self.win.safe_mode => {
                Some("安全モードなので、設定は今回だけ使い、ファイルには書きません".into())
            }
            Some(d) => kmb_session::save_settings(&self.win.extensions, d).err(),
            None => None,
        };
        true
    }

    /// モジュールの設定を全部初めの値に戻す(設定の窓の「…を初めの値に戻す」を全部押したのと同じ)。
    /// 戻した設定の id を返す(画面の台本の試験が、台本ごとに初めの値から始めるのにも使う)。
    pub fn reset_module_settings(&mut self) -> Vec<String> {
        let mut reset = Vec::new();
        for g in self.cur.work.settings() {
            if g.value != g.default && self.change_settings(&g.id, &g.default) {
                reset.push(g.id);
            }
        }
        reset
    }

    /// 設定の窓を開く・閉じる。
    pub fn toggle_settings(&mut self) {
        self.settings.open = !self.settings.open;
    }

    /// 取り消しの履歴の窓を開く・閉じる。
    pub fn toggle_undo_history(&mut self) {
        self.win.undo_history_open = !self.win.undo_history_open;
    }

    /// 取り消しの履歴の窓が開いているか。
    pub fn undo_history_open(&self) -> bool {
        self.win.undo_history_open
    }

    /// 読むための文の窓の題と行(開いていれば)。
    pub fn viewer(&self) -> Option<(&str, &[String])> {
        self.cur
            .viewer
            .as_ref()
            .map(|(t, l)| (t.as_str(), l.as_slice()))
    }

    /// 時間のかかる仕事を、画面では別のスレッドで行う。終わると結果を知らせに出す。
    fn start_task(&mut self, job: kmb_core::Job) {
        if self.cur.task.is_some() {
            self.win.message = Some("前の計算の途中です".into());
            return;
        }
        let ctl = std::sync::Arc::new(kmb_core::JobControl::new());
        self.cur.task_doc = self
            .cur
            .work
            .path()
            .map(std::path::Path::to_path_buf)
            .map(|p| (p, self.is_modified()));
        if !self.win.background {
            self.task_done(job(&ctl));
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = self.win.repaint.clone();
        let c = ctl.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job(&c));
            if let Some(ctx) = repaint {
                ctx.request_repaint();
            }
        });
        self.cur.task = Some(rx);
        self.cur.task_ctl = Some(ctl);
        self.cur.task_since = Some(std::time::Instant::now());
        self.win.message = Some("計算中…".into());
    }

    /// 時間のかかる仕事(シミュレーションなど)が走っているか。
    pub fn is_running(&self) -> bool {
        self.cur.task.is_some()
    }

    /// 走っている仕事が知らせた進み(知らせない仕事や、走っていなければ `None`)。
    pub fn task_progress(&self) -> Option<kmb_core::JobProgress> {
        self.cur.task.as_ref()?;
        self.cur.task_ctl.as_ref()?.snapshot()
    }

    /// 走っている仕事に中止を頼む(仕事は次の区切りで止まり、結果を知らせる)。
    pub fn cancel_task(&mut self) {
        if let Some(c) = self.cur.task.as_ref().and(self.cur.task_ctl.as_ref()) {
            log::info!("時間のかかる仕事の中止を頼む");
            c.cancel();
            self.win.message = Some("中止しています…".into());
        }
    }

    /// 走っている仕事に、今の所までで仕上げるよう頼む。
    pub fn finish_task(&mut self) {
        if let Some(c) = self.cur.task.as_ref().and(self.cur.task_ctl.as_ref()) {
            log::info!("時間のかかる仕事を今の所で仕上げるよう頼む");
            c.finish();
            self.win.message = Some("仕上げています…".into());
        }
    }

    /// 中止か仕上げを頼んだ後か。
    pub fn task_stopping(&self) -> bool {
        self.cur
            .task_ctl
            .as_ref()
            .is_some_and(|c| c.cancelled() || c.finishing())
    }

    /// 時間のかかる仕事が走り始めてからの時間(走っていなければ `None`)。
    pub fn task_for(&self) -> Option<std::time::Duration> {
        self.cur
            .task
            .as_ref()
            .and(self.cur.task_since.map(|t| t.elapsed()))
    }

    /// 別のスレッドの仕事が終わっていれば結果を出す。毎フレーム呼ぶ。
    pub(crate) fn poll_task(&mut self) {
        let Some(rx) = &self.cur.task else { return };
        match rx.try_recv() {
            Ok(r) => {
                self.cur.task = None;
                self.task_done(r);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.win.fail("計算が途中で止まりました");
                self.cur.task = None;
            }
        }
    }

    /// 走っている仕事が終わるまで待つ(テスト用)。
    pub fn wait_task(&mut self) {
        if let Some(rx) = self.cur.task.take() {
            match rx.recv() {
                Ok(r) => self.task_done(r),
                Err(_) => self.win.fail("計算が途中で止まりました"),
            }
        }
    }

    /// 仕事の結果を知らせる。知らせの履歴の「計算中…」は結果に置き換え、
    /// 詳しい結果(`info` の `report` の行)があれば、読むための窓に出す(下の帯の 1 行では切れるので)。
    fn task_done(&mut self, r: kmb_core::JobReport) {
        // 「中止しています…」などの途中の知らせも、結果に置き換える
        while self.win.history.last().is_some_and(|m| {
            ["計算中…", "中止しています…", "仕上げています…"].contains(&m.as_str())
        }) {
            self.win.history.pop();
            self.win.history_at.pop();
            self.win.history_tone.pop();
        }
        if let Some(lines) = r.info.get("report").and_then(|v| v.as_array()) {
            let title = r
                .info
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("計算の結果");
            let lines = lines
                .iter()
                .filter_map(|l| l.as_str().map(String::from))
                .collect();
            self.cur.viewer = Some((title.to_string(), lines));
        }
        let mut text = r.text;
        // 描き終えた静止画は、描き始めた文書のギャラリーにも残す(#17)
        if r.ok
            && let Some(image) = r.info.get("image").and_then(|v| v.as_str())
        {
            // 残す枚数はシーンの設定(省いた仕事は前と同じく残す)
            let keep = r.info.get("gallery_keep").and_then(|v| v.as_u64());
            match self.cur.task_doc.clone() {
                Some(_) if keep == Some(0) => {
                    text.push_str("(ギャラリーに残さない設定なので、残していません)")
                }
                Some((doc, changed)) => {
                    let note = r.info.get("note").and_then(|v| v.as_str()).unwrap_or("");
                    let keep = keep.map_or(usize::MAX, |k| k as usize);
                    if let Some(n) =
                        self.add_to_gallery(&doc, std::path::Path::new(image), changed, note, keep)
                    {
                        text.push_str(&format!("。レンダリング ギャラリーの絵 {n}"));
                    }
                }
                None => text.push_str("(保存していない文書なので、ギャラリーには残していません)"),
            }
        }
        self.cur.task_doc = None;
        self.win.message = Some(text);
    }

    /// 3D のビューに重ねるもの(選んでいる機能の経路、ストック、原点、作業範囲、文字の線)。
    pub fn overlay(&self) -> Overlay {
        self.win.extensions.overlay(&self.context())
    }

    /// 機能の注意: 評価の警告と、モジュールの検査(セットアップならストックの大きさや機械の検査)。
    pub fn notes(&self, id: FeatureId) -> Vec<String> {
        let mut out = match self.cur.work.report().feature(id) {
            Some(kmb_core::FeatureStatus::Ok { outputs, .. }) => kmb_core::warnings_of(outputs),
            _ => Vec::new(),
        };
        out.extend(self.win.extensions.notes(&self.context(), id));
        out
    }

    /// 機能の設定欄に出す要約(加工なら経路の長さと時間)。
    pub fn summary(&self, id: FeatureId) -> Option<String> {
        self.win.extensions.summary(&self.context(), id)
    }

    /// 実行ボタンで解く機能(解析の研究)の様子。
    pub fn run_state(&self, id: FeatureId) -> Option<kmb_core::RunState> {
        self.win.extensions.run_state(&self.context(), id)
    }

    /// 選んでいる結果の時刻のコマ(時刻の帯に出す)。
    pub fn frame_bar(&self) -> Option<kmb_core::FrameBar> {
        self.win.extensions.frames(&self.context())
    }
}

/// 実行の様子の色(研究の行の札・設定欄の頭)。
pub(crate) fn tone_color(tone: kmb_core::RunTone, pal: &crate::theme::Palette) -> egui::Color32 {
    match tone {
        kmb_core::RunTone::Idle => pal.ink3,
        kmb_core::RunTone::Busy => pal.sim,
        kmb_core::RunTone::Stale => pal.warn,
        kmb_core::RunTone::Done => pal.ok,
        kmb_core::RunTone::Failed => pal.err,
    }
}

/// 設定の欄を 1 つ描く(数・真偽・文字)。値を変えたら `v` を書き換える。
pub(crate) fn setting_field(
    ui: &mut egui::Ui,
    group: &str,
    f: &kmb_core::Field,
    v: &mut serde_json::Value,
) {
    use kmb_core::FieldKind;
    let label = ui.label(&f.label);
    if !f.help.is_empty() {
        label.on_hover_text(&f.help);
    }
    let key = egui::Id::new(("setting", group, &f.key));
    match &f.kind {
        FieldKind::Number { .. } | FieldKind::Text | FieldKind::File => {
            let cur = match f.get(v) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Null) | None => String::new(),
                Some(x) => x.to_string(),
            };
            if let Some(new) = crate::feature_ui::expr_field(ui, key, &cur) {
                let new = new.trim();
                let value = if matches!(f.kind, FieldKind::Text | FieldKind::File) {
                    Some(serde_json::Value::String(new.to_string()))
                } else {
                    // 読めない値は捨てる(欄は元の値に戻る)
                    new.parse::<f64>().ok().map(serde_json::Value::from)
                };
                if let Some(x) = value {
                    f.set(v, Some(x));
                }
            }
        }
        FieldKind::Bool => {
            let mut b = f
                .get(v)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if ui.checkbox(&mut b, "").changed() {
                f.set(v, Some(serde_json::Value::Bool(b)));
            }
        }
        FieldKind::Choice { choices } => {
            let cur = match f.get(v) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Null) | None => String::new(),
                Some(x) => x.to_string(),
            };
            let shown = choices
                .iter()
                .find(|c| c.value == cur)
                .map_or(cur.clone(), |c| c.label.clone());
            egui::ComboBox::from_id_salt(key)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for c in choices
                        .iter()
                        .filter(|c| !f.hidden.contains(&c.value) || c.value == cur)
                    {
                        if ui.selectable_label(c.value == cur, &c.label).clicked() {
                            f.set(v, Some(serde_json::Value::String(c.value.clone())));
                        }
                    }
                });
        }
        _ => {
            ui.weak("(設定の窓では変えられない欄)");
        }
    }
}
