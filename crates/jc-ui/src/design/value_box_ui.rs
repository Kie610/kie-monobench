//! 辺の機能(フィレット・面取り)を作っている・直している間、行ごとの値の箱を 3D のビューの
//! 辺の近くに出す(目を欄と形の間で往復させない)。箱は機能を選んで直して
//! いる時だけ出し、カーソルを乗せただけでは何も出さず、何も変えない(docs/避ける形.md)。
//! 引いて値を決める取っ手は作らず、打つだけにする。

use crate::Workspace;
use egui::Rect;
use jc_core::{FeatureId, FieldKind, Ref, RefKind, Value};

/// 箱の位置の控え(機能、行の参照と形の組の文字列、行ごとの位置)。
pub(crate) type BoxAnchors = Option<(FeatureId, String, Vec<Option<[f64; 3]>>)>;

impl Workspace {
    /// 辺の値の箱を描く。打って Enter(か欄を出る)で、その行の値にする。
    pub(crate) fn edge_value_boxes(&mut self, ui: &egui::Ui, rect: Rect) {
        let Some(id) = self.cur.editing.or(self.cur.creating.map(|c| c.0)) else {
            return;
        };
        let Some(entry) = self.cur.work.doc().data().feature(id).cloned() else {
            return;
        };
        let Some(desc) = self.win.registry.descriptor(&entry.type_id) else {
            return;
        };
        // 行の並びで、辺を受ける選ぶ欄と式の欄を持つ物(フィレットの半径、面取りの距離)
        let Some((rows, pick, value)) = desc.fields.iter().find_map(|f| {
            let FieldKind::Rows { fields } = &f.kind else {
                return None;
            };
            let pick = fields.iter().find(|s| {
                matches!(&s.kind, FieldKind::Pick { accept, .. } if accept.contains(&RefKind::Edge))
            })?;
            // 値は行ごとに必ずある欄だけ(条件で隠れる欄は箱にしない。組立の拘束のオフセットは
            // 角度の行で隠れ、辺も受けるので、条件を見ないと隠れた欄の箱が出ていた)
            let value = fields
                .iter()
                .find(|s| matches!(s.kind, FieldKind::Expr { .. }) && s.when.is_none())?;
            Some((f.clone(), pick.clone(), value.clone()))
        }) else {
            return;
        };
        let data = desc.upgraded(&entry.data);
        let n = rows.row_count(&data);
        let refs: Vec<Vec<Ref>> = (0..n)
            .map(|i| rows.row_field(i, &pick).refs(&data))
            .collect();
        let anchors = self.box_anchors(id, &refs);
        let mut changed = None;
        for (i, at) in anchors.iter().enumerate() {
            let Some(at) = at else { continue };
            let (p, _) = self.cur.camera.project(at.map(|v| v as f32), rect);
            if !rect.contains(p) {
                continue;
            }
            let rf = rows.row_field(i, &value);
            let cur = rf
                .get(&data)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            egui::Area::new(egui::Id::new(("edge_value_box", id, i)))
                .fixed_pos(p + egui::vec2(10.0, -28.0))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let name = if n > 1 {
                                    format!("{} {}", i + 1, value.label)
                                } else {
                                    value.label.clone()
                                };
                                ui.label(egui::RichText::new(name).size(crate::theme::fs(11.0)));
                                ui.set_max_width(150.0);
                                let key = crate::doc_id(ui.ctx(), ("edge_value", id, i));
                                if let Some(v) = crate::feature_ui::expr_field(ui, key, &cur) {
                                    let v = crate::feature_ui::typed_expr(self, &rf, &cur, &v);
                                    if !v.is_empty() && v != cur {
                                        changed = Some((rf.clone(), v));
                                    }
                                }
                            });
                        });
                });
        }
        if let Some((rf, v)) = changed {
            let mut d = data;
            rf.set(&mut d, Some(serde_json::Value::String(v)));
            self.set_feature_data(id, d);
        }
    }

    /// 行ごとの箱の位置: 行の最初の辺の中ほど(面なら面の上の点)。形の計算は重いので、行の参照と
    /// 形が変わった時だけ作り直す。
    fn box_anchors(&mut self, id: FeatureId, refs: &[Vec<Ref>]) -> Vec<Option<[f64; 3]>> {
        let shape = |r: &Ref| match self.output(r.feature(), "body") {
            Some(Value::Shape(h)) => Some(*h),
            _ => None,
        };
        let key = format!(
            "{:?}",
            refs.iter()
                .map(|r| r.first().map(|r| (r.clone(), shape(r))))
                .collect::<Vec<_>>()
        );
        if let Some((f, k, v)) = &self.cur.value_boxes
            && *f == id
            && *k == key
        {
            return v.clone();
        }
        let anchors: Vec<Option<[f64; 3]>> = refs
            .iter()
            .map(|r| {
                let r = r.first()?;
                let h = shape(r)?;
                match r {
                    Ref::Edge { faces, .. } => {
                        let mut all = self.cur.work.kernel().edges(h).unwrap_or_default();
                        all.extend(self.cur.work.kernel().arc_edges(h).unwrap_or_default());
                        all.extend(self.cur.work.kernel().curve_edges(h).unwrap_or_default());
                        let e = all.iter().find(|e| {
                            e.faces == *faces || (e.faces[0] == faces[1] && e.faces[1] == faces[0])
                        })?;
                        let line = e.polyline(16);
                        Some(if line.len() == 2 {
                            [0, 1, 2].map(|i| (line[0][i] + line[1][i]) / 2.0)
                        } else {
                            line[line.len() / 2]
                        })
                    }
                    Ref::Face { face, .. } => self
                        .cur
                        .work
                        .kernel()
                        .faces(h)
                        .ok()?
                        .into_iter()
                        .find(|f| &f.name == face)?
                        .planar
                        .map(|p| p.point),
                    _ => None,
                }
            })
            .collect();
        self.cur.value_boxes = Some((id, key, anchors.clone()));
        anchors
    }
}
