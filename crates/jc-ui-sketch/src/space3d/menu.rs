//! 3D スケッチの図の右クリックのメニュー(2D のスケッチの menu.rs と同じ分け方。まとめてレビュー R3)。
//! 道具の中・何も選んでいない時・選んでいる時で、出す項目を分ける。メニューは押された項目
//! ([`Ask3`])を返すだけで、形を変えるのは [`Sketch3dEditor::apply_menu`]。

use super::ops::{self, Sel};
use super::{Sketch3dEditor, Tool3, sketch_of};
use jc_core::FeatureEntry;
use jc_sketch::EntityId;
use jc_sketch_3d::{Axis, Entity3d, Kind3d, Sketch3d, Solution3d};
use jc_ui::EditorHost;

/// メニューで押した項目。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Ask3 {
    /// 道具を終える(描いた所までは残す。打った長さがあればその線分を引いてから)
    Ok,
    /// 描きかけを捨てて道具を終える
    Cancel,
    /// 次に描く線を補助線にする(もう一度で戻す)
    DrawConstruction,
    /// 道具を選ぶ(繰り返し・最近の道具)
    Tool(Tool3),
    /// 選んだ物に寸法を付ける(D)
    Dimension,
    /// 選んだ線と円の補助線の切り替え(X)
    Construction,
    /// 固定する(真)か、固定を外す(偽)
    Fix(bool),
    /// 選んだ線分を軸に沿わせる
    Along(Axis),
    /// 選んだ物で作図面を作る
    WorkPlane,
    Delete,
    Deselect,
    /// 図の印で選んだ拘束を外す
    Unconstrain,
    /// 選んだ半径の寸法を直径に、直径を半径にする
    RoundDim,
    Undo,
    Redo,
    /// 全体が収まるように表示する
    Fit,
    /// パラメータの一覧を開く
    Params,
}

fn item(ui: &mut egui::Ui, text: &str, key: &str, out: &mut Option<Ask3>, ask: Ask3) {
    let label = if key.is_empty() {
        text.to_string()
    } else {
        format!("{text}({key})")
    };
    if ui.button(label).clicked() {
        *out = Some(ask);
        ui.close();
    }
}

/// 選んだ線分と円(補助線の切り替えの相手)。
fn curves(sk: &Sketch3d, sel: &[Sel]) -> Vec<EntityId> {
    sel.iter()
        .filter_map(|s| match s {
            Sel::Ent(id) => Some(*id),
            _ => None,
        })
        .filter(|id| {
            matches!(
                sk.entity(*id),
                Some(Entity3d::Line { .. } | Entity3d::Circle { .. })
            )
        })
        .collect()
}

/// 固定の拘束の相手: 点、線分の両端、円そのもの(拘束の道具の「固定」と同じ)。
fn fix_targets(sk: &Sketch3d, sel: &[Sel]) -> Vec<EntityId> {
    let mut out = Vec::new();
    for s in sel {
        let Sel::Ent(id) = s else { continue };
        let ids = match sk.entity(*id) {
            Some(Entity3d::Line { a, b, .. }) => vec![*a, *b],
            Some(_) => vec![*id],
            None => vec![],
        };
        for i in ids {
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    out
}

fn has_fix(sk: &Sketch3d, sel: &[Sel]) -> bool {
    let t = fix_targets(sk, sel);
    sk.constraints
        .iter()
        .any(|c| matches!(c.kind, Kind3d::Fix { point } if t.contains(&point)))
}

impl Sketch3dEditor {
    /// 右クリックのメニューの中身。押した項目を返す。
    pub(crate) fn menu_ui(
        &self,
        ui: &mut egui::Ui,
        sk: &Sketch3d,
        sol: &Solution3d,
    ) -> Option<Ask3> {
        let mut out = None;
        let sel = &self.selection;
        if self.tool != Tool3::Select {
            // 道具の中
            ui.weak(self.tool.label());
            item(ui, "OK", "Enter", &mut out, Ask3::Ok);
            item(ui, "キャンセル", "Esc", &mut out, Ask3::Cancel);
            if !matches!(
                self.tool,
                Tool3::Include | Tool3::Project | Tool3::Intersect
            ) {
                let text = if self.construction {
                    "補助線で描くのをやめる"
                } else {
                    "補助線で描く"
                };
                item(ui, text, "X", &mut out, Ask3::DrawConstruction);
            }
            if !sel.is_empty() && self.start.is_none() {
                ui.separator();
                item(ui, "削除", "Delete", &mut out, Ask3::Delete);
            }
            return out;
        }
        match self.last_tool {
            Some(t) => item(
                ui,
                &format!("繰り返し: {}", t.label()),
                "",
                &mut out,
                Ask3::Tool(t),
            ),
            None => {
                ui.label("繰り返す道具はまだありません");
            }
        }
        if let Some(c) = self.picked_constraint.and_then(|c| sk.constraint(c)) {
            match c.kind {
                Kind3d::Radius { .. } => {
                    item(ui, "直径の寸法にする", "", &mut out, Ask3::RoundDim);
                }
                Kind3d::Diameter { .. } => {
                    item(ui, "半径の寸法にする", "", &mut out, Ask3::RoundDim);
                }
                _ => {}
            }
            item(
                ui,
                &format!("{}の拘束を外す", c.kind.label()),
                "Delete",
                &mut out,
                Ask3::Unconstrain,
            );
        }
        if sel.is_empty() {
            item(ui, "元に戻す", "Ctrl+Z", &mut out, Ask3::Undo);
            item(ui, "やり直し", "Ctrl+Y", &mut out, Ask3::Redo);
            ui.separator();
            item(ui, "すべて表示", "", &mut out, Ask3::Fit);
            item(ui, "パラメータを変更", "", &mut out, Ask3::Params);
            item(ui, "作図面を作る", "", &mut out, Ask3::WorkPlane);
            let recent: Vec<Tool3> = if self.recent.is_empty() {
                vec![Tool3::Line, Tool3::Rectangle, Tool3::Circle]
            } else {
                self.recent.clone()
            };
            ui.separator();
            ui.weak("最近の道具");
            for t in recent {
                let key = super::DRAW3
                    .iter()
                    .find(|d| d.0 == t)
                    .map_or(String::new(), |d| jc_ui::keys::label(d.3));
                item(ui, t.label(), &key, &mut out, Ask3::Tool(t));
            }
            return out;
        }
        // 選んだ物
        let target = self.target_plane();
        if ops::dimension_kind(&self.fmt, "寸法", sk, sol, sel, &target).is_ok() {
            item(ui, "スケッチ寸法", "D", &mut out, Ask3::Dimension);
        }
        if !curves(sk, sel).is_empty() {
            item(ui, "標準/補助線", "X", &mut out, Ask3::Construction);
        }
        if !fix_targets(sk, sel).is_empty() {
            if has_fix(sk, sel) {
                item(ui, "固定解除", "", &mut out, Ask3::Fix(false));
            } else {
                item(ui, "固定する", "", &mut out, Ask3::Fix(true));
            }
        }
        // 線分だけを選んでいれば、軸に沿わせる(2D の水平・垂直にする)
        if ops::constraint_kinds("X に沿う", sk, sel, &target).is_ok() {
            for (axis, name) in [(Axis::X, "X"), (Axis::Y, "Y"), (Axis::Z, "Z")] {
                item(
                    ui,
                    &format!("{name} に沿わせる"),
                    "",
                    &mut out,
                    Ask3::Along(axis),
                );
            }
        }
        if ops::work_plane_def(sk, sol, sel, self.plane, self.anchor, &target).is_ok() {
            item(ui, "選んだ物で作図面を作る", "", &mut out, Ask3::WorkPlane);
        }
        ui.separator();
        item(ui, "削除", "Delete", &mut out, Ask3::Delete);
        item(ui, "選択を外す", "Esc", &mut out, Ask3::Deselect);
        out
    }

    /// メニューで押した項目を行う。文書を変えれば新しいスケッチ。
    pub(crate) fn apply_menu(
        &mut self,
        ask: Ask3,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
        at: Option<egui::Pos2>,
    ) -> Option<Sketch3d> {
        let (sk, sol) = sketch_of(host, entry)?;
        match ask {
            Ask3::Ok => {
                // 打った長さがあれば、その線分を右クリックした向きへ引いてから終える(2D と同じ)
                let s = if self.tool == Tool3::Line && !self.typed.is_empty() {
                    self.typed_line(host, at, &sk, &sol)
                } else {
                    None
                };
                self.finish_tool();
                s
            }
            Ask3::Cancel => {
                self.finish_tool();
                None
            }
            Ask3::DrawConstruction => {
                self.construction = !self.construction;
                None
            }
            Ask3::Tool(t) => {
                self.set_tool(t);
                None
            }
            Ask3::Dimension => self.apply_dimension("寸法", host, entry),
            Ask3::Construction => self.toggle_construction(&sk),
            Ask3::Fix(true) => self.apply_constraint("固定", host, entry),
            Ask3::Fix(false) => {
                let t = fix_targets(&sk, &self.selection);
                let mut s = sk.clone();
                s.constraints
                    .retain(|c| !matches!(c.kind, Kind3d::Fix { point } if t.contains(&point)));
                Some(s)
            }
            Ask3::Along(axis) => {
                let name = match axis {
                    Axis::X => "X に沿う",
                    Axis::Y => "Y に沿う",
                    Axis::Z => "Z に沿う",
                };
                self.apply_constraint(name, host, entry)
            }
            Ask3::WorkPlane => self.make_work_plane(host, entry),
            Ask3::Delete => self.delete_selected(&sk),
            Ask3::Deselect => {
                self.selection.clear();
                self.picked_constraint = None;
                None
            }
            Ask3::Unconstrain => {
                let c = self.picked_constraint.take()?;
                let mut s = sk.clone();
                s.constraints.retain(|x| x.id != c);
                Some(s)
            }
            Ask3::RoundDim => {
                let c = self.picked_constraint?;
                let mut s = sk.clone();
                s.toggle_radius(c).then_some(s)
            }
            Ask3::Undo | Ask3::Redo | Ask3::Params => {
                let what = match ask {
                    Ask3::Undo => jc_ui::APP_UNDO,
                    Ask3::Redo => jc_ui::APP_REDO,
                    _ => jc_ui::APP_PARAMS,
                };
                *host.then = Some((what.into(), serde_json::json!({})));
                None
            }
            Ask3::Fit => {
                self.fitted = false;
                None
            }
        }
    }
}
