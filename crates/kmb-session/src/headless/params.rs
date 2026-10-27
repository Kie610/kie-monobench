//! パラメータ・寸法の名前・機能の値の式と、パラメータの表(ビュー)の変更と書き出し・読み込み。

use super::Headless;
use kmb_core::{FeatureId, ParamId, ParamStatus};
use serde_json::{Value as Json, json};

impl Headless {
    pub fn param_id(&self, name: &str) -> Result<ParamId, String> {
        self.doc
            .data()
            .param_by_name(name)
            .map(|p| p.id)
            .ok_or_else(|| format!("パラメータ {name} がありません"))
    }

    /// パラメータを足す。
    pub fn add_param(&mut self, name: &str, expr: &str) -> Result<(), String> {
        self.add_param_info(name, expr, &Default::default())
    }

    /// コメントと単位つきでパラメータを足す(表の列と同じ物を、CLI・MCP・コマンド欄からも入れる)。
    pub fn add_param_info(
        &mut self,
        name: &str,
        expr: &str,
        info: &crate::edit::ParamInfo,
    ) -> Result<(), String> {
        if self.is_model_param(name) {
            return Err(format!("寸法の名前 {name} と同じ名前は使えません"));
        }
        let tx = crate::edit::add_param_info(&mut self.doc, name, expr, info);
        self.apply(tx)
    }

    /// パラメータの式とコメント・単位を 1 回の取り消し単位で変える。式を省くと式は今のまま。
    /// 寸法の名前にはコメントと単位を付けられない。
    pub fn set_param_info(
        &mut self,
        name: &str,
        expr: Option<&str>,
        info: &crate::edit::ParamInfo,
    ) -> Result<(), String> {
        if info.is_empty() {
            return match expr {
                Some(e) => self.set_param(name, e),
                None => Ok(()),
            };
        }
        if self.doc.data().param_by_name(name).is_none() && self.is_model_param(name) {
            return Err(format!("寸法 {name} にはコメントと単位を付けられません"));
        }
        let id = self.param_id(name)?;
        let data = self.doc.data();
        let mut tx = match expr {
            Some(e) => crate::edit::set_param_expr(data, id, e),
            None => kmb_core::Transaction::new(format!("{name} のコメントと単位を変更"), vec![]),
        };
        tx.ops.extend(info.op(data, id));
        self.apply(tx)
    }

    /// 寸法の名前(`d1` など、機能が持つモデルのパラメータ)か。
    pub fn is_model_param(&self, name: &str) -> bool {
        self.report.model_params.iter().any(|p| p.name == name)
    }

    /// パラメータの式を変える。寸法の名前(`d1` など)なら、その寸法の式を変える。
    pub fn set_param(&mut self, name: &str, expr: &str) -> Result<(), String> {
        if self.doc.data().param_by_name(name).is_none() && self.is_model_param(name) {
            self.doc
                .set_model_param(&self.modules.registry, name, expr)
                .map_err(|e| e.to_string())?;
            self.recompute();
            return Ok(());
        }
        let id = self.param_id(name)?;
        self.apply(crate::edit::set_param_expr(self.doc.data(), id, expr))
    }

    /// パラメータ名を変え、参照している式も書き換える。寸法の名前(`d1` など)も変えられる。
    pub fn rename_param(&mut self, name: &str, new_name: &str) -> Result<(), String> {
        if self.doc.data().param_by_name(name).is_none() && self.is_model_param(name) {
            self.doc
                .rename_model_param(&self.modules.registry, name, new_name)
                .map_err(|e| e.to_string())?;
            self.recompute();
            return Ok(());
        }
        let id = self.param_id(name)?;
        self.doc
            .rename_param(&self.modules.registry, id, new_name)
            .map_err(|e| e.to_string())?;
        self.recompute();
        Ok(())
    }

    /// 機能の値(押し出しの距離など)の式を変える。数だけなら今の単位を補う。
    pub fn set_value(&mut self, feature: FeatureId, key: &str, expr: &str) -> Result<(), String> {
        // 作り直しを後へ回している間は、今の値が前の形のまま。単位を補うのに使う値が誤りなら、
        // 先に作り直して読む(無い値は作り直しても無いので、そのまま断る)
        if self.hold == super::Hold::Owed {
            let unread = crate::params::feature_values(
                self.doc.data(),
                &self.modules.registry,
                &self.report,
            )
            .into_iter()
            .any(|v| {
                v.feature == feature && v.key == key && !matches!(v.status, ParamStatus::Ok { .. })
            });
            if unread {
                self.hold_recompute(false);
                self.hold_recompute(true);
            }
        }
        let tx = crate::params::set_feature_value(
            self.doc.data(),
            &self.modules.registry,
            &self.report,
            feature,
            key,
            expr,
        )?;
        self.apply(tx)
    }

    /// 行の書き方(`width`、`d1`、`5:distance`)を表の行にする。
    pub fn param_key(&self, s: &str) -> Result<kmb_core::ParamKey, String> {
        crate::params::parse_key(self.doc.data(), &self.modules.registry, &self.report, s)
    }

    /// パラメータの表の一覧(`id` を指すとその表だけ)。行ごとに書き方・名前・式・値。
    /// `id` が 0 なら「全部」の表(保存しない決まりの表)。
    pub fn param_views(&self, id: Option<u64>) -> Result<Json, String> {
        let (data, reg, rep) = (self.doc.data(), &self.modules.registry, &self.report);
        let rows = |keys: Vec<kmb_core::ParamKey>| -> Vec<Json> {
            keys.iter()
                .map(|k| {
                    let mut o = json!({
                        "row": crate::params::key_text(rep, data, k),
                        "label": crate::params::key_label(data, reg, rep, k),
                    });
                    let (expr, status) = self.row_value(k);
                    o["expr"] = json!(expr);
                    match status {
                        Some(ParamStatus::Ok { value }) => o["value"] = json!(value.to_string()),
                        Some(ParamStatus::Text { value }) => o["value"] = json!(value),
                        Some(ParamStatus::Error { message }) => o["error"] = json!(message),
                        None => {}
                    }
                    o
                })
                .collect()
        };
        let one = |v: &kmb_core::ParamView| {
            json!({"id": v.id, "name": v.name, "body": v.body.map(|b| b.0),
                "rows": rows(crate::params::view_keys(data, reg, rep, v))})
        };
        match id {
            Some(0) => Ok(
                json!({"id": 0, "name": "全部", "rows": rows(crate::params::all_keys(data, reg, rep))}),
            ),
            Some(i) => data
                .param_view(i)
                .map(one)
                .ok_or_else(|| format!("パラメータの表 {i} がありません")),
            None => Ok(json!(data.param_views.iter().map(one).collect::<Vec<_>>())),
        }
    }

    /// 行の式と値。
    fn row_value(&self, k: &kmb_core::ParamKey) -> (String, Option<ParamStatus>) {
        use kmb_core::ParamKey;
        match k {
            ParamKey::User { id } => (
                self.doc
                    .data()
                    .params
                    .iter()
                    .find(|p| p.id == *id)
                    .map(|p| p.expr.clone())
                    .unwrap_or_default(),
                self.report
                    .params
                    .iter()
                    .find(|r| r.id == *id)
                    .map(|r| r.status.clone()),
            ),
            ParamKey::Model { feature, key } => self
                .report
                .model_params
                .iter()
                .find(|p| {
                    p.feature == *feature && (p.key == *key || (p.key.is_empty() && p.name == *key))
                })
                .map_or((String::new(), None), |p| {
                    (p.expr.clone(), Some(p.status.clone()))
                }),
            ParamKey::Field { feature, key } => {
                crate::params::feature_values(self.doc.data(), &self.modules.registry, &self.report)
                    .into_iter()
                    .find(|v| v.feature == *feature && v.key == *key)
                    .map_or((String::new(), None), |v| (v.expr, Some(v.status)))
            }
        }
    }

    /// パラメータの表を作る。`body` があればそのボディに関わる行を決まりで入れる。名前を省くと
    /// ボディの名前から付ける。番号を返す。
    pub fn add_param_view(
        &mut self,
        name: Option<&str>,
        body: Option<FeatureId>,
        rows: &[String],
    ) -> Result<u64, String> {
        if let Some(b) = body {
            let root = kmb_core::body_root(self.doc.data(), &self.modules.registry, b);
            if self.doc.data().feature(root).is_none() {
                return Err(format!("ボディ {} がありません", b.0));
            }
        }
        let body = body.map(|b| kmb_core::body_root(self.doc.data(), &self.modules.registry, b));
        let members = rows
            .iter()
            .map(|r| self.param_key(r))
            .collect::<Result<Vec<_>, _>>()?;
        let name = match (name, body) {
            (Some(n), _) => n.to_string(),
            (None, Some(b)) => crate::params::body_view_name(
                self.doc.data(),
                &self.modules.registry,
                &self.report,
                b,
            ),
            (None, None) => "パラメータの表".to_string(),
        };
        let (tx, id) = crate::params::add_view(&mut self.doc, &name, body, members)?;
        self.apply(tx)?;
        Ok(id)
    }

    /// 表に行を入れる(`add`)、外す(`!add`)。
    pub fn param_view_rows(&mut self, id: u64, rows: &[String], add: bool) -> Result<(), String> {
        let keys = rows
            .iter()
            .map(|r| self.param_key(r))
            .collect::<Result<Vec<_>, _>>()?;
        let tx = if add {
            crate::params::add_to_view(self.doc.data(), id, &keys)?
        } else {
            crate::params::remove_from_view(
                self.doc.data(),
                &self.modules.registry,
                &self.report,
                id,
                &keys,
            )?
        };
        self.apply(tx)
    }

    pub fn rename_param_view(&mut self, id: u64, name: &str) -> Result<(), String> {
        let tx = crate::params::rename_view(self.doc.data(), id, name)?;
        self.apply(tx)
    }

    pub fn remove_param_view(&mut self, id: u64) -> Result<(), String> {
        let tx = crate::params::remove_view(self.doc.data(), id)?;
        self.apply(tx)
    }

    /// パラメータを消す(式で使われていれば断る)。消した物は [`Self::restore_param`] で戻せる。
    pub fn remove_param(&mut self, name: &str) -> Result<(), String> {
        let id = self.param_id(name)?;
        let users = crate::edit::param_users(self.doc.data(), &self.modules.registry, id);
        if !users.is_empty() {
            return Err(format!("{}が使っているので消せません", users.join("、")));
        }
        let tx = crate::edit::remove_param(self.doc.data(), id);
        self.apply(tx)
    }

    /// 消したパラメータ `name` を戻す(同じ名前を何度か消していれば、最後に消した物)。
    pub fn restore_param(&mut self, name: &str) -> Result<(), String> {
        let id = self
            .doc
            .data()
            .removed_params
            .iter()
            .rev()
            .find(|p| p.name == name)
            .map(|p| p.id)
            .ok_or_else(|| format!("消したパラメータ {name} がありません"))?;
        let model: Vec<String> = self
            .report
            .model_params
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let tx = crate::edit::restore_param(self.doc.data(), &model, id)?;
        self.apply(tx)
    }

    /// パラメータの表 `id`(0 は「全部」)を `format` の文字にする(U54)。
    pub fn export_params(
        &self,
        id: u64,
        format: crate::param_io::Format,
    ) -> Result<String, String> {
        let (data, reg, rep) = (self.doc.data(), &self.modules.registry, &self.report);
        let (name, keys) = if id == 0 {
            ("全部".to_string(), crate::params::all_keys(data, reg, rep))
        } else {
            let v = data
                .param_view(id)
                .ok_or_else(|| format!("パラメータの表 {id} がありません"))?;
            (v.name.clone(), crate::params::view_keys(data, reg, rep, v))
        };
        let rows = crate::param_io::rows(data, reg, rep, &keys);
        Ok(crate::param_io::export(&name, rows, format))
    }

    /// 書き出した表を読み込む(U54。1 回の取り消し)。何をしたかの要約を返す。
    pub fn import_params(
        &mut self,
        text: &str,
        format: crate::param_io::Format,
    ) -> Result<crate::param_io::Imported, String> {
        let rows = crate::param_io::parse(text, format)?;
        let (tx, done) =
            crate::param_io::import(&mut self.doc, &self.modules.registry, &self.report, &rows);
        if let Some(tx) = tx {
            self.apply(tx)?;
        }
        Ok(done)
    }

    /// パラメータ `id` の名前を変え、参照する式も同じ取り消し単位で書き換えて再計算する。
    pub fn rename_param_id(&mut self, id: ParamId, new_name: &str) -> Result<(), String> {
        self.doc
            .rename_param(&self.modules.registry, id, new_name)
            .map_err(|e| e.to_string())?;
        self.recompute();
        Ok(())
    }

    /// 寸法の名前(モデルのパラメータ `old`)を変え、参照する式も書き換えて再計算する。
    pub fn rename_model_param(&mut self, old: &str, new_name: &str) -> Result<(), String> {
        self.doc
            .rename_model_param(&self.modules.registry, old, new_name)
            .map_err(|e| e.to_string())?;
        self.recompute();
        Ok(())
    }

    /// 寸法の値(モデルのパラメータ `name`)の式を変えて再計算する。
    pub fn set_model_param(&mut self, name: &str, expr: &str) -> Result<(), String> {
        self.doc
            .set_model_param(&self.modules.registry, name, expr)
            .map_err(|e| e.to_string())?;
        self.recompute();
        Ok(())
    }
}
