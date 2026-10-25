//! 再計算の結果を読む: 残ったボディ、質量、status の JSON、人が読む文。

use super::{BodyMass, Headless};
use kmb_core::{FeatureId, FeatureStatus, ParamStatus, ShapeHandle, Value};
use serde_json::{Value as Json, json};

impl Headless {
    /// 最後に残ったボディ。再計算に誤りがあれば断る。
    pub fn bodies(&self) -> Result<Vec<ShapeHandle>, String> {
        let n = self.report.error_count();
        if n > 0 {
            return Err(format!(
                "再計算で誤りが {n} 件あります(status で理由を見られます)"
            ));
        }
        let b: Vec<_> =
            kmb_core::visible_bodies(self.doc.data(), &self.modules.registry, &self.report)
                .into_iter()
                .map(|(_, h)| h)
                .collect();
        if b.is_empty() {
            return Err("ボディがありません".into());
        }
        Ok(b)
    }

    /// 残ったボディごとの体積(mm³)、表面積(mm²)、外形(mm)、重心と質量(g。材料の無いボディは既定の材料の鋼)。
    pub fn mass(&self) -> Result<Vec<BodyMass>, String> {
        self.bodies()?;
        let data = self.doc.data();
        let reg = &self.modules.registry;
        let bodies = kmb_core::visible_bodies(data, reg, &self.report);
        let ids: Vec<FeatureId> = bodies.iter().map(|(id, _)| *id).collect();
        let names = kmb_core::body_labels(data, reg, &ids);
        bodies
            .into_iter()
            .zip(names)
            .map(|((id, h), name)| {
                let props = self.modules.kernel.mass(h).map_err(|e| e.to_string())?;
                let root = kmb_core::body_root(data, reg, id);
                let material = Some(kmb_look::material::material_of(data, root));
                let density = material
                    .as_deref()
                    .and_then(kmb_look::material::phys_material)
                    .map(|m| m.density);
                Ok(BodyMass {
                    name,
                    root,
                    props,
                    mass: density.map(|d| kmb_look::material::mass_of(props.volume, d)),
                    material,
                    density,
                })
            })
            .collect()
    }

    /// 出力の値を言葉で(スケッチの解の状態など、モジュールの知る型はモジュールに聞く)。
    fn describe(&self, v: &Value) -> String {
        match v {
            Value::Scalar(q) => q.to_string(),
            Value::Shape(h) => format!("形状 {}", h.0),
            Value::Custom { type_key, .. } => self
                .modules
                .extensions
                .describe(v)
                .unwrap_or_else(|| format!("({type_key})")),
        }
    }

    /// 文書の要約: パラメータ、タイムライン(状態と理由)、モジュールの足す項目(セットアップなど)。
    pub fn status(&self) -> Json {
        self.status_in(None)
    }

    /// [`Self::status`] の、モジュールが調べに開くファイル(リンクの元など)を `files`(AI に
    /// 許したフォルダ)に限る版(MCP)。
    pub fn status_in(&self, files: Option<&dyn kmb_core::FileGate>) -> Json {
        // 読む値も、書く数だけの値と同じ文書の長さの単位で出す(画面のパラメータの表と同じ)。
        // 値は文書の単位で出す(書く時も文書の単位で読むので、読んだ数を書き戻しても変わらない)
        let units = self.doc.data().properties.units;
        let params: Vec<Json> = self
            .doc
            .data()
            .params
            .iter()
            .map(|p| {
                let r = self.report.params.iter().find(|r| r.id == p.id);
                match r.map(|r| &r.status) {
                    Some(ParamStatus::Ok { value }) => {
                        json!({"name": p.name, "expr": p.expr, "value": units.show(*value)})
                    }
                    Some(ParamStatus::Error { message }) => {
                        json!({"name": p.name, "expr": p.expr, "error": message})
                    }
                    None => json!({"name": p.name, "expr": p.expr}),
                }
            })
            .collect();
        let features: Vec<Json> = self
            .doc
            .data()
            .features
            .iter()
            .map(|f| {
                let mut o = json!({"id": f.id.0, "name": f.name, "type": f.type_id});
                let (state, detail) = match self.report.feature(f.id) {
                    Some(FeatureStatus::Ok { outputs, .. }) => {
                        let scalars: serde_json::Map<String, Json> = outputs
                            .iter()
                            .filter_map(|(k, v)| match v {
                                Value::Scalar(q) => Some((k.clone(), json!(units.show(*q)))),
                                _ => None,
                            })
                            .collect();
                        if !scalars.is_empty() {
                            o["values"] = Json::Object(scalars);
                        }
                        ("ok", None)
                    }
                    Some(FeatureStatus::Error { message }) => ("error", Some(message.clone())),
                    Some(FeatureStatus::Blocked { by, .. }) => {
                        ("blocked", Some(format!("{by} が使えません")))
                    }
                    Some(FeatureStatus::Suppressed) => ("suppressed", None),
                    Some(FeatureStatus::RolledBack) => ("rolled_back", None),
                    None => ("unknown", None),
                };
                o["status"] = json!(state);
                if let Some(d) = detail {
                    o["reason"] = json!(d);
                }
                o
            })
            .collect();
        let mut out = json!({
            "file": self.path.as_ref().map(|p| p.display().to_string()),
            "errors": self.report.error_count(),
            "units": units.name(),
            "params": params,
            "features": features,
            "can_undo": self.doc.can_undo(),
            "can_redo": self.doc.can_redo(),
        });
        // アクティブな部品(作るボディと部品が入る。F7)
        if let Some(c) = self.active.id {
            out["active_component"] = json!(c);
        }
        // 寸法の名前(d1 など。式から参照できる)
        if !self.report.model_params.is_empty() {
            out["dimensions"] = self
                .report
                .model_params
                .iter()
                .map(|p| {
                    let mut o = json!({"name": p.name, "expr": p.expr, "feature": p.feature.0, "label": p.label});
                    match &p.status {
                        ParamStatus::Ok { value } => o["value"] = json!(units.show(*value)),
                        ParamStatus::Error { message } => o["error"] = json!(message),
                    }
                    o
                })
                .collect();
        }
        // 機能の値(押し出しの距離など。set_value で直せる)
        let values =
            crate::params::feature_values(self.doc.data(), &self.modules.registry, &self.report);
        if !values.is_empty() {
            out["values"] = values
                .iter()
                .map(|v| {
                    let mut o = json!({"feature": v.feature.0, "key": v.key, "label": v.label, "expr": v.expr});
                    match &v.status {
                        ParamStatus::Ok { value } => o["value"] = json!(units.show(*value)),
                        ParamStatus::Error { message } => o["error"] = json!(message),
                    }
                    o
                })
                .collect();
        }
        if !self.doc.data().param_views.is_empty() {
            out["param_views"] = self
                .doc
                .data()
                .param_views
                .iter()
                .map(|v| json!({"id": v.id, "name": v.name, "body": v.body.map(|b| b.0)}))
                .collect();
        }
        // 構成表(U55): 列の見出し・行ごとの値・当てた行と、それが今の値と合っているか
        let c = &kmb_interchange::configuration::of(self.doc.data());
        if !kmb_core::SectionType::is_empty(c) {
            let data = self.doc.data();
            out["configurations"] = json!({
                "columns": c.columns.iter()
                    .map(|col| kmb_interchange::configuration::Configurations::column_label(data, col))
                    .collect::<Vec<_>>(),
                "rows": c.rows.iter().map(|r| json!({
                    "name": r.name,
                    "cells": r.cells.iter().map(|cell| match cell {
                        kmb_interchange::configuration::ConfigCell::Expr(e) => json!(e),
                        kmb_interchange::configuration::ConfigCell::Suppressed(s) =>
                            json!(if *s { "抑制" } else { "有効" }),
                    }).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "active": c.active,
                "active_matches": c.active.as_deref().map(|a| c.matches(data, a)),
            });
        }
        if !self.doc.data().groups.is_empty() {
            out["groups"] = self
                .doc
                .data()
                .groups
                .iter()
                .map(|g| {
                    json!({"id": g.id, "name": g.name,
                        "features": crate::edit::group_members(self.doc.data(), g.id)
                            .iter().map(|f| f.0).collect::<Vec<_>>()})
                })
                .collect();
        }
        let cx = kmb_core::Context {
            files,
            ..self.context(None, None, &Json::Null)
        };
        for (k, v) in self.modules.extensions.status(&cx) {
            out[k] = v;
        }
        out
    }

    /// 再計算の結果を人が読む文で(CLI の表示)。
    pub fn report_text(&self) -> String {
        let mut s = String::from("パラメータ:\n");
        for p in &self.report.params {
            match &p.status {
                ParamStatus::Ok { value } => s += &format!("  {} = {value}\n", p.name),
                ParamStatus::Error { message } => {
                    s += &format!("  {} : 誤り({message})\n", p.name);
                }
            }
        }
        if !self.report.model_params.is_empty() {
            s += "寸法の名前:\n";
            for p in &self.report.model_params {
                match &p.status {
                    ParamStatus::Ok { value } => {
                        s += &format!(
                            "  {} = {}({} {})= {value}\n",
                            p.name, p.expr, p.feature, p.label
                        )
                    }
                    ParamStatus::Error { message } => {
                        s += &format!("  {} : 誤り({message})\n", p.name);
                    }
                }
            }
        }
        s += "タイムライン:\n";
        for f in &self.report.features {
            let t = match &f.status {
                FeatureStatus::Ok { outputs, .. } => outputs
                    .iter()
                    .map(|(k, v)| format!("{k} = {}", self.describe(v)))
                    .collect::<Vec<_>>()
                    .join(", "),
                FeatureStatus::Error { message } => format!("誤り({message})"),
                FeatureStatus::Blocked { by, .. } => format!("停止({by} が使えません)"),
                FeatureStatus::Suppressed => "抑制中".into(),
                FeatureStatus::RolledBack => "ロールバック位置より後ろ".into(),
            };
            s += &format!("  {} {}: {t}\n", f.id, f.name);
        }
        s
    }
}
