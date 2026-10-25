//! MCP の道具の中身。1 つの文書を開いておき、変更のたびに再計算する。
//! 道具はどれも JSON を返す。誤りは文字列で返す。
//!
//! 文書の扱いとモジュールの操作は、CLI と同じ [`kmb_session::Headless`] に任せる。
//! ここは許したフォルダ(作業フォルダと `--allow` のフォルダ)の外に触らないことと、
//! 結果の JSON の形だけを受け持つ。`..` やリンクでフォルダの外へ出るパスも断る。

use crate::server::{EditViewArg, MeasureTarget, SaveViewArg, ScreenshotArg};
use kmb_bundle::Sample;
use kmb_core::{FeatureId, Registry};
use kmb_session::roots::Roots;
use kmb_session::{Headless, MeasurePick, Ran, edit};
use serde_json::{Value as Json, json};
use std::path::{Path, PathBuf};

pub type Reply = Result<Json, kmb_core::Problem>;

pub fn registry() -> Registry {
    kmb_bundle::registry()
}

pub struct Session {
    /// ファイルを読み書きしてよいフォルダ(作業フォルダと `--allow`)。相対パスは作業フォルダから数える
    roots: Roots,
    /// ユーザーのデータフォルダ(クラッシュ報告を読む)
    data_dir: Option<PathBuf>,
    pub(crate) work: Headless,
}

impl Session {
    /// 空の文書で始める。`root` の外のファイルには触らない。
    pub fn new(root: PathBuf) -> Self {
        Session {
            roots: Roots::new(root),
            data_dir: None,
            work: Headless::new(kmb_bundle::modules()),
        }
    }

    /// データフォルダ(動いている画面の `live.json` を探す所)。
    pub fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }

    pub fn root(&self) -> &Path {
        self.roots.root()
    }

    /// 読み書きしてよいフォルダ(動いている画面へ送る頼みにも添える)。
    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    /// 作業フォルダのほかに、読み書きしてよいフォルダを足す。
    pub fn allow(&mut self, dir: &Path) -> Result<(), String> {
        self.roots.allow(dir)
    }

    /// ユーザーのデータフォルダ(クラッシュ報告を読む場所)。
    pub fn with_data_dir(mut self, dir: PathBuf) -> Self {
        self.data_dir = Some(dir);
        self
    }

    /// 開いている文書。
    pub fn document(&self) -> &kmb_core::Document {
        self.work.doc()
    }

    /// 使っているモジュール(起動時に PC 全体の設定を当てる)。
    pub fn modules(&self) -> &kmb_core::Modules {
        self.work.modules()
    }

    /// 許したフォルダの中のパスにする。外を指すもの(`..` やリンクで出るものも)は断る。
    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        self.roots.resolve(path)
    }

    /// クラッシュ報告。`name` を省くと一覧(新しい順)、指すとその報告の中身。
    pub fn crash_reports(&self, name: Option<&str>) -> Reply {
        let dir = self
            .data_dir
            .as_deref()
            .ok_or("データフォルダがないので、クラッシュ報告はありません")?;
        match name {
            Some(n) => kmb_session::crash::read(dir, n).map_err(Into::into),
            None => Ok(json!(kmb_session::crash::list(dir))),
        }
    }

    fn then_status(&self, r: Result<(), String>) -> Reply {
        r.map(|()| self.status()).map_err(Into::into)
    }

    /// 文書の要約: パラメータ、タイムライン(状態と理由)、セットアップ。
    pub fn status(&self) -> Json {
        self.work.status()
    }

    pub fn open(&mut self, path: &str) -> Reply {
        let p = self.resolve(path)?;
        let doc = kmb_session::load(&p)?;
        self.work.replace(doc, Some(p));
        Ok(self.status())
    }

    /// 基準部品の文書で始める(`cam` なら加工も、`three_d` なら面取り、フィレット、
    /// 走査線と等高線の仕上げも入れる)。保存先はまだない。
    pub fn sample(&mut self, cam: bool, three_d: bool) -> Reply {
        let kind = if three_d {
            Sample::Cam3d
        } else if cam {
            Sample::Cam
        } else {
            Sample::Part
        };
        self.work.replace(kind.make()(), None);
        Ok(self.status())
    }

    /// 保存する。`path` を省くと開いた場所へ。既にあるファイルへの別名保存は `overwrite` が要る。
    pub fn save(&mut self, path: Option<&str>, overwrite: bool) -> Reply {
        let p = match path {
            Some(s) => {
                let p = self.resolve(s)?;
                if p.exists() && Some(p.as_path()) != self.work.path() && !overwrite {
                    return Err(format!(
                        "{} は既にあります(上書きするなら overwrite)",
                        p.display()
                    )
                    .into());
                }
                Some(p)
            }
            None => None,
        };
        let p = self.work.save(p.as_deref())?;
        Ok(json!({"saved": p.display().to_string()}))
    }

    /// パラメータの式を変える。なければ足す。
    pub fn set_param(&mut self, name: &str, expr: &str) -> Reply {
        self.set_param_info(name, expr, None, None)
    }

    /// `comment`・`unit` があれば表のコメントと単位も同じ取り消し単位で入れる。
    pub fn set_param_info(
        &mut self,
        name: &str,
        expr: &str,
        comment: Option<&str>,
        unit: Option<&str>,
    ) -> Reply {
        let r = edit::ParamInfo::parse(comment, unit).and_then(|info| {
            if self.work.param_id(name).is_ok() || self.work.is_model_param(name) {
                self.work.set_param_info(name, Some(expr), &info)
            } else {
                self.work.add_param_info(name, expr, &info)
            }
        });
        self.then_status(r)
    }

    /// パラメータ名を変える。参照している式も書き換える。
    pub fn rename_param(&mut self, name: &str, new_name: &str) -> Reply {
        let r = self.work.rename_param(name, new_name);
        self.then_status(r)
    }

    /// 機能の値(押し出しの距離など)の式を変える。
    pub fn set_value(&mut self, feature: u64, key: &str, expr: &str) -> Reply {
        let r = self.work.set_value(FeatureId(feature), key, expr);
        self.then_status(r)
    }

    /// パラメータの表(`id` が 0 なら全部の表、省くと作った表すべて)。
    pub fn param_views(&self, id: Option<u64>) -> Reply {
        self.work.param_views(id).map_err(Into::into)
    }

    /// パラメータの表を作る・変える。結果は変えた後の表(消した時は表の一覧)。
    pub fn edit_param_view(&mut self, a: &EditViewArg) -> Reply {
        let id = || {
            a.id.ok_or_else(|| format!("{} には id が要ります", a.action))
        };
        let id = match a.action.as_str() {
            "create" => {
                self.work
                    .add_param_view(a.name.as_deref(), a.body.map(FeatureId), &a.rows)?
            }
            "rename" => {
                let id = id()?;
                let name = a.name.as_deref().ok_or("rename には name が要ります")?;
                self.work.rename_param_view(id, name)?;
                id
            }
            "remove" => {
                self.work.remove_param_view(id()?)?;
                return self.work.param_views(None).map_err(Into::into);
            }
            "add" | "hide" => {
                let id = id()?;
                if a.rows.is_empty() {
                    return Err("rows に行を 1 つ以上書いてください".into());
                }
                self.work.param_view_rows(id, &a.rows, a.action == "add")?;
                id
            }
            x => {
                return Err(format!(
                    "action は create・rename・remove・add・hide のどれかです: {x}"
                )
                .into());
            }
        };
        self.work.param_views(Some(id)).map_err(Into::into)
    }

    pub fn feature_data(&self, id: u64) -> Reply {
        let f = self
            .work
            .doc()
            .data()
            .feature(FeatureId(id))
            .ok_or_else(|| format!("機能 {id} がありません"))?;
        Ok(
            json!({"id": id, "name": f.name, "type": f.type_id, "suppressed": f.suppressed, "data": f.data}),
        )
    }

    pub fn add_feature(&mut self, type_id: &str, name: Option<&str>, data: Json) -> Reply {
        let id = self.work.add_feature(type_id, name, data)?;
        let mut out = self.status();
        out["added"] = json!(id.0);
        Ok(out)
    }

    /// 設計の属性を変え(`set` が空なら変えない)、今の属性を返す。
    pub fn properties(&mut self, set: &std::collections::BTreeMap<String, String>) -> Reply {
        if !set.is_empty() {
            let values: Vec<(String, String)> =
                set.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            self.work.set_properties(&values)?;
        }
        Ok(json!({"properties": self.work.properties()}))
    }

    /// 版の履歴(候補 35)。`milestone_of` があれば、その版に節目の名前を付けてから返す。
    pub fn versions(&mut self, milestone_of: Option<&str>, milestone: &str) -> Reply {
        if let Some(id) = milestone_of {
            let path = self
                .work
                .path()
                .ok_or("保存していない文書には版がありません")?
                .to_path_buf();
            let v = self
                .work
                .versions()
                .into_iter()
                .find(|v| v.id == id || v.number.to_string() == id)
                .ok_or_else(|| format!("版 {id} がありません"))?;
            kmb_session::history::set_milestone(&path, &v.id, milestone)?;
        }
        Ok(json!({"versions": self.work.versions()}))
    }

    /// 前の版を最新として戻して保存する。
    pub fn restore_version(&mut self, id: &str) -> Reply {
        let v = self.work.restore_version(id)?;
        let mut out = self.status();
        out["restored"] = json!({"number": v.number, "id": v.id});
        Ok(out)
    }

    /// ロールバックの位置を動かす(画面・CLI と同じ操作を通す。`None` なら最後まで進める)。
    pub fn roll_back(&mut self, position: Option<i64>) -> Reply {
        let id = self
            .work
            .action_for_role(&kmb_interchange::role::action::ROLL_BACK)?;
        self.run_action(&id, None, None, &json!({ "position": position }), None)
    }

    pub fn import_step(&mut self, path: &str, stand_up: bool) -> Reply {
        let p = self.resolve(path)?;
        let mark = self.work.mark();
        let mut out = self.run_action(
            &self
                .work
                .action_for_role(&kmb_interchange::role::action::IMPORT_STEP)?,
            None,
            Some(&p),
            &Json::Null,
            None,
        )?;
        if let Some(id) = out.get("added").and_then(|a| a.as_array()?.last().cloned()) {
            if stand_up && let Some(f) = id.as_u64() {
                self.work.stand_up(FeatureId(f), true)?;
                // 読み込んで立てるのを、1 回で取り消せるようにまとめる
                self.work.squash_since(mark, "STEP を読み込んで立てる");
                out = self.status();
            }
            out["added"] = id;
        }
        Ok(out)
    }

    pub fn set_feature(&mut self, id: u64, data: Json) -> Reply {
        // 作る時と同じく、読めないデータは文書に入れずに断る
        let type_id = self
            .work
            .doc()
            .data()
            .feature(FeatureId(id))
            .map(|f| f.type_id.clone())
            .ok_or_else(|| format!("機能 {id} がありません"))?;
        self.work.check_data(&type_id, &data)?;
        let tx = edit::set_feature_data(self.work.doc().data(), FeatureId(id), data);
        let r = self.work.apply(tx);
        self.then_status(r)
    }

    pub fn suppress(&mut self, id: u64, suppressed: bool) -> Reply {
        let r = self
            .work
            .apply(edit::set_suppressed(&[FeatureId(id)], suppressed));
        self.then_status(r)
    }

    pub fn move_features(&mut self, ids: &[u64], target: u64, after: bool) -> Reply {
        let ids: Vec<FeatureId> = ids.iter().copied().map(FeatureId).collect();
        let r = self.work.move_features(&ids, FeatureId(target), after);
        self.then_status(r)
    }

    pub fn remove_feature(&mut self, id: u64) -> Reply {
        let tx = edit::remove_features(self.work.doc().data(), &[FeatureId(id)]);
        let r = self.work.apply(tx);
        self.then_status(r)
    }

    /// 機能をタイムラインのグループにまとめる。続いていなければ、`gaps` で間も含めるか
    /// `gather` で寄せるかを選ぶ(どちらもなければ理由を返す)。
    pub fn group(&mut self, ids: &[u64], name: &str, mode: &str) -> Reply {
        let mode = match mode {
            "" | "exact" => edit::GroupMode::Exact,
            "gaps" => edit::GroupMode::IncludeGaps,
            "gather" => edit::GroupMode::Gather,
            m => return Err(format!("mode は exact・gaps・gather のどれかです: {m}").into()),
        };
        let ids: Vec<FeatureId> = ids.iter().map(|&i| FeatureId(i)).collect();
        let g = self.work.group(&ids, name, mode)?;
        let mut out = self.status();
        out["group"] = json!(g);
        Ok(out)
    }

    pub fn paste_features(&mut self, ids: &[u64]) -> Reply {
        let ids: Vec<FeatureId> = ids.iter().copied().map(FeatureId).collect();
        let (made, cleared) = self.work.paste_features(&ids)?;
        let mut r = self.status();
        r["pasted"] = json!(made.iter().map(|f| f.0).collect::<Vec<_>>());
        r["cleared"] = json!(cleared);
        Ok(r)
    }

    pub fn move_component(&mut self, a: &crate::server::MoveComponentArg) -> Reply {
        use kmb_session::components::Place;
        let at = match (a.before, a.after) {
            (Some(_), Some(_)) => {
                return Err("before と after はどちらか 1 つです".to_string().into());
            }
            (Some(c), None) => Place::Before(c),
            (None, Some(c)) => Place::After(c),
            (None, None) => Place::Into(a.into),
        };
        if a.into.is_some() && !matches!(at, Place::Into(_)) {
            return Err("into は before・after と一緒に使えません"
                .to_string()
                .into());
        }
        let r = self.work.move_component(a.id, at);
        self.then_status(r)
    }

    pub fn activate_component(&mut self, component: Option<u64>) -> Reply {
        self.work.activate_component(component)?;
        Ok(self.status())
    }

    pub fn move_bodies(&mut self, ids: &[u64], component: Option<u64>) -> Reply {
        let ids: Vec<FeatureId> = ids.iter().copied().map(FeatureId).collect();
        let r = self.work.move_bodies(&ids, component);
        self.then_status(r)
    }

    pub fn duplicate_component(&mut self, id: u64) -> Reply {
        let made = self.work.duplicate_component(id)?;
        let mut r = self.status();
        r["component"] = json!(made);
        Ok(r)
    }

    /// 名前を付けた視点の一覧。
    pub fn named_views(&self) -> Json {
        json!({"named_views": self.work.doc().data().named_views})
    }

    pub fn save_named_view(&mut self, a: &SaveViewArg) -> Reply {
        let view = kmb_core::NamedView {
            id: 0,
            name: a.name.clone(),
            target: a.target,
            yaw: a.yaw,
            pitch: a.pitch,
            roll: a.roll,
            height: a.height,
            perspective: a.perspective,
        };
        let id = self.work.save_named_view(view)?;
        Ok(json!(self.work.doc().data().named_view(id)))
    }

    pub fn rename_named_view(&mut self, id: u64, name: &str) -> Reply {
        self.work.rename_named_view(id, name)?;
        Ok(self.named_views())
    }

    pub fn remove_named_view(&mut self, id: u64) -> Reply {
        self.work.remove_named_view(id)?;
        Ok(self.named_views())
    }

    pub fn ungroup(&mut self, gid: u64) -> Reply {
        let r = self.work.ungroup(gid);
        self.then_status(r)
    }

    pub fn undo(&mut self) -> Reply {
        let r = self.work.undo();
        self.then_status(r)
    }

    pub fn redo(&mut self) -> Reply {
        let r = self.work.redo();
        self.then_status(r)
    }

    /// 作り直しを後へ回す(値をいくつも直すまとめて実行の間)。
    pub(crate) fn hold_recompute(&mut self, hold: bool) {
        self.work.hold_recompute(hold);
    }

    /// 作り直した回数。
    pub(crate) fn recomputes(&self) -> u64 {
        self.work.recomputes()
    }

    /// まとめて実行の始めの位置。
    pub(crate) fn mark(&self) -> usize {
        self.work.mark()
    }

    /// まとめて実行の始めの位置まで戻す。戻した数を返す。
    pub(crate) fn rollback_to(&mut self, mark: usize) -> usize {
        self.work.rollback_to(mark)
    }

    /// まとめて実行で変えた手を 1 回の取り消し単位にまとめる。まとめた数を返す。
    pub(crate) fn squash_since(&mut self, mark: usize, label: &str) -> usize {
        self.work.squash_since(mark, label)
    }

    /// モジュールの設定(機械など。文書ごとに持つレンダリングのシーンは今の文書の値)。
    pub fn settings(&self) -> Json {
        json!(self.work.settings())
    }

    /// 設定を変える。この接続の間だけ効き、PC の設定のファイルには書かない。文書ごとに持つ設定
    /// (レンダリングのシーン)は今の文書にも書く(取り消せる)。
    pub fn set_settings(&mut self, id: &str, value: &Json) -> Reply {
        // ファイルの欄(ポストのプラグインなど)も許したフォルダの中に限る
        self.work
            .set_settings_in(id, value, &self.roots)
            .map_err(|e| kmb_core::Problem::new(kmb_core::ProblemKind::Input, e))?;
        Ok(self.settings())
    }

    /// 取り消しの履歴(済んだ変更は古い順、やり直せる変更は次にやり直すものが先頭)。
    pub fn history(&self) -> Json {
        let d = self.work.doc();
        json!({"done": d.undo_labels(), "undone": d.redo_labels()})
    }

    pub fn types(&self) -> Json {
        json!(self.work.modules().registry.descriptors())
    }

    /// 今使える操作の一覧(`selected` の機能を選んでいるとした時)。
    pub fn actions(&self, selected: Option<u64>) -> Json {
        json!(self.work.actions(selected.map(FeatureId)))
    }

    /// 操作を実行する。ファイルを書き出す操作は、`out` があればそこへ書き、
    /// なければ中身(文字)を結果の `contents` に入れる(文字でない中身は `out` が要る)。
    pub fn run_action(
        &mut self,
        id: &str,
        selected: Option<u64>,
        file: Option<&Path>,
        args: &Json,
        out: Option<&Path>,
    ) -> Reply {
        match self.work.run(id, selected.map(FeatureId), file, args)? {
            Ran::Changed(added) => {
                let mut r = self.status();
                r["added"] = json!(added.iter().map(|a| a.0).collect::<Vec<_>>());
                Ok(r)
            }
            Ran::File { contents, info, .. } => {
                let mut r = if info.is_object() { info } else { json!({}) };
                match out {
                    Some(p) => {
                        kmb_session::write_output(p, &contents)?;
                        r["written"] = json!(p.display().to_string());
                    }
                    // 文字でない中身(3MF・STL・PNG など)を文字にすると壊れるので、書き出し先を求める
                    None => match std::str::from_utf8(&contents) {
                        Ok(text) => r["contents"] = json!(text),
                        Err(_) => {
                            return Err(format!(
                                "この操作は文字でないファイル({} バイト)を書くので、書き出し先 out が要ります",
                                contents.len()
                            ).into());
                        }
                    },
                }
                Ok(r)
            }
            Ran::Files { files, info } => {
                let mut r = if info.is_object() { info } else { json!({}) };
                let Some(p) = out else {
                    return Err("いくつものファイルを書く操作は、書き出し先が要ります".into());
                };
                let written = Headless::write_files(p, &files)?;
                r["written"] = json!(
                    written
                        .iter()
                        .map(|w| w.display().to_string())
                        .collect::<Vec<_>>()
                );
                Ok(r)
            }
            Ran::Message(m) => Ok(json!({"message": m})),
            Ran::Report(rep) => {
                let mut r = if rep.info.is_object() {
                    rep.info
                } else {
                    json!({})
                };
                r["ok"] = json!(rep.ok);
                r["summary"] = r.get("summary").cloned().unwrap_or(json!(rep.text));
                Ok(r)
            }
        }
    }

    /// [`Session::run_action`] の、パスを作業フォルダから数える版(MCP の道具)。
    pub fn run_action_at(
        &mut self,
        id: &str,
        selected: Option<u64>,
        file: Option<&str>,
        args: &Json,
        out: Option<&str>,
    ) -> Reply {
        let file = file.map(|f| self.resolve(f)).transpose()?;
        let out = out.map(|o| self.resolve(o)).transpose()?;
        // ファイルを読む引数も許したフォルダの中に限る(絵を貼る・フォントを読むなど)
        let mut args = args.clone();
        if let Some(a) = self
            .work
            .actions(selected.map(FeatureId))
            .iter()
            .find(|a| a.id == id)
        {
            self.roots.confine_args(a, &mut args)?;
        }
        self.run_action(id, selected, file.as_deref(), &args, out.as_deref())
    }

    /// 残ったボディごとの体積(mm³)、表面積(mm²)、外形(mm)、重心、材料を付けていれば質量(g)。
    pub fn mass(&self) -> Reply {
        Ok(json!(self.work.mass()?))
    }

    /// ボディに物理材料を付け(`body` が無ければ見えているボディすべて)、質量を返す。
    pub fn set_material(&mut self, body: Option<u64>, material: &str) -> Reply {
        self.work
            .set_body_material(body.map(kmb_core::FeatureId), material)?;
        self.mass()
    }

    /// 残ったボディの面と辺と、それぞれの形。
    pub fn topology(&self, body: Option<u64>) -> Reply {
        self.work.topology(body.map(FeatureId)).map_err(Into::into)
    }

    /// 面・辺・円の中心・点を 1 つか 2 つ測る(画面の測る道具と同じ計算)。
    pub fn measure(&self, targets: &[MeasureTarget], basis: Option<&str>) -> Reply {
        let picks: Vec<MeasurePick> = targets.iter().map(MeasureTarget::pick).collect();
        self.work.measure(&picks, basis).map_err(Into::into)
    }

    /// 残ったボディの絵(PNG)と説明。`out` があれば PNG をそこへも書く。
    pub fn screenshot(&self, a: &ScreenshotArg) -> Result<(Vec<u8>, Json), String> {
        let angles = match (a.named_view, a.yaw, a.pitch) {
            (Some(id), _, _) => {
                let v = self
                    .work
                    .doc()
                    .data()
                    .named_view(id)
                    .ok_or_else(|| format!("視点 {id} がありません"))?;
                Some((v.yaw as f32, v.pitch as f32))
            }
            (None, Some(y), Some(p)) => Some((y, p)),
            (None, None, None) => None,
            _ => return Err("向きを度で決める時は yaw と pitch の両方が要ります".into()),
        };
        let out = a.out.as_deref().map(|o| self.resolve(o)).transpose()?;
        let spec = kmb_session::PictureSpec {
            view: a.view.clone(),
            angles,
            width: a.width.unwrap_or(800),
            height: a.height.unwrap_or(600),
            body: a.body.map(FeatureId),
            faces: a
                .faces
                .iter()
                .map(|f| kmb_core::FacePick {
                    feature: FeatureId(f.body),
                    face: f.face.clone(),
                })
                .collect(),
            edges: a
                .edges
                .iter()
                .map(|e| (FeatureId(e.body), e.edge))
                .collect(),
        };
        let (png, mut info) = self.work.picture(&spec)?;
        if let Some(p) = out {
            kmb_session::write_output(&p, &png)?;
            info["written"] = json!(p.display().to_string());
        }
        Ok((png, info))
    }

    /// 見えているボディを書き出す。拡張子で形式を決める([`Headless::save_action_for`]。`action` で
    /// 操作を指定もできる)。`each` が真ならボディごとに別のファイルにする(`path` の名前の後ろに
    /// `-<ボディの名前>` を付ける)。
    pub fn export(
        &mut self,
        path: &str,
        action: Option<&str>,
        view: Option<&Json>,
        tolerance: Option<f64>,
        each: bool,
    ) -> Reply {
        let out = self.resolve(path)?;
        let id = self.work.save_action_for(&out, action)?;
        let mut args = self.work.export_args(&id, &out);
        if let Some(t) = tolerance {
            args["tolerance"] = json!(t);
        }
        if let Some(v) = view {
            args["view"] = v.clone();
        }
        if each {
            args["each"] = json!(true);
        }
        let r = self.run_action(&id, None, None, &args, Some(&out))?;
        Ok(json!({"written": r["written"]}))
    }

    /// G-code を作る。ポストは Carvera Air か、`post` で指定した WASM のプラグイン。
    /// `out` を指定するとファイルへ書き、要約だけ返す。
    pub fn gcode(&mut self, setup: Option<u64>, out: Option<&str>, post: Option<&str>) -> Reply {
        let mut args = json!({});
        if let Some(p) = post {
            args["post"] = json!(self.resolve(p)?.display().to_string());
        }
        let out = out.map(|o| self.resolve(o)).transpose()?;
        let mut r = self.run_action(
            &self
                .work
                .action_for_role(&kmb_interchange::role::action::EXPORT_GCODE)?,
            setup.or_else(|| self.work.first_setup().map(|f| f.0)),
            None,
            &args,
            out.as_deref(),
        )?;
        if let Some(g) = r.as_object_mut().and_then(|o| o.remove("contents")) {
            r["gcode"] = g;
        }
        Ok(r)
    }

    /// ストックを経路で削ってみる。食い込み、早送りでの切削、削り残し。
    pub fn simulate(&mut self, setup: Option<u64>, cell: Option<f64>) -> Reply {
        // 省いた時は渡さない(操作の既定と設定に任せ、画面・CLI と同じ結果にする)
        let args = cell.map_or(Json::Null, |c| json!({"cell": c}));
        self.run_action(
            &self
                .work
                .action_for_role(&kmb_interchange::role::action::SIMULATE)?,
            setup.or_else(|| self.work.first_setup().map(|f| f.0)),
            None,
            &args,
            None,
        )
    }
}

/// バイト列を base64 の文字にする(MCP の絵の中身)。
pub fn base64(b: &[u8]) -> String {
    kmb_session::live::base64(b)
}
