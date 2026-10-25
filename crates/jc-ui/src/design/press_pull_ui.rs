//! プレス/プル: 選んだ物の種類で、使うコマンドを振り分ける。
//! どの振り分けもタイムラインに機能を足すか、今ある機能を直す(履歴の外で形を変えない)。
//! 決め方は選んだ物の種類と、面を作った機能の種類だけで、隣の面との関係は見ない。

use crate::Workspace;
use jc_core::FeatureId;

/// プレス/プルのツールバーの項目の鍵。
pub(crate) const PRESS_PULL: &str = "u:press_pull";

/// 選んだ物から決めた、プレス/プルの行き先。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PressPull {
    /// スケッチの形か平らな面: 押し出し(面は押すと結合、引くと切り取り)
    Extrude,
    /// 辺: フィレット(選んだ辺が行に入る)
    Fillet,
    /// フィレット・面取り・穴が作った面: その機能を直す
    Edit(FeatureId, String),
    /// 曲がった面: 面のオフセット(面のオフセットの役割の種類)
    Offset(String),
    /// 押し出せない面: 理由
    Refuse(String),
}

impl Workspace {
    /// 今の選択でのプレス/プルの行き先。
    pub(crate) fn press_pull_target(&self) -> PressPull {
        if !self.cur.picked_edges.is_empty()
            && self.cur.picked.is_none()
            && self.cur.also_picked.is_empty()
        {
            return PressPull::Fillet;
        }
        let Some(face) = self.cur.picked.as_ref() else {
            // 何も選んでいない・スケッチを選んでいる: 押し出しがスケッチの形を選ばせる
            return PressPull::Extrude;
        };
        let data = self.cur.work.doc().data();
        // 面を作った機能が、プレス/プルで直す役割の種類なら、その機能を直す
        if let Some(f) = jc_core::face_feature(&face.face).and_then(|id| data.feature(id))
            && let Some(d) = self.win.registry.descriptor(&f.type_id)
            && d.role == Some(jc_interchange::role::kind::EDIT_FROM_FACE)
        {
            return PressPull::Edit(f.id, d.label);
        }
        let picks = self.picked_faces();
        if !picks.iter().all(|p| self.face_planar(p)) {
            // 曲がった面は面のオフセット
            return match self
                .win
                .registry
                .descriptors()
                .into_iter()
                .find(|d| d.role == Some(jc_interchange::role::kind::OFFSET_FACES))
            {
                Some(d) => PressPull::Offset(d.type_id),
                None => PressPull::Refuse(
                    "曲がった面は押し出せません。平らな面か、辺・スケッチの形を選んでください"
                        .into(),
                ),
            };
        }
        if self.faces_extrude(&picks) {
            PressPull::Extrude
        } else {
            PressPull::Refuse(
                "いくつかの面を一緒に押し出すのは、1 つのボディの同じ平面にある面だけです".into(),
            )
        }
    }

    /// 平らな面 `picks` を一緒に押し出せるか。押し出しの役割の操作に、面を引数で渡して聞く
    /// (押し出しの「面を選んで押し出し」と同じ条件)。
    fn faces_extrude(&self, picks: &[crate::view3d::Pick]) -> bool {
        if picks.is_empty() {
            return false;
        }
        let faces: Vec<serde_json::Value> = picks
            .iter()
            .map(|p| serde_json::json!({"kind": "face", "feature": p.feature, "face": p.face}))
            .collect();
        let args = serde_json::json!({ "faces": faces });
        let cx = jc_core::Context {
            args: &args,
            ..self.context()
        };
        self.win
            .extensions
            .actions(&cx)
            .iter()
            .find(|a| a.role == Some(jc_interchange::role::action::EXTRUDE))
            .is_some_and(|a| a.disabled.is_none())
    }

    fn face_planar(&self, pick: &crate::view3d::Pick) -> bool {
        jc_core::Context {
            picked: Some(pick),
            ..self.context()
        }
        .picked_plane()
        .is_some()
    }

    /// プレス/プルを始める。何になったかを知らせに出す。
    pub(crate) fn press_pull(&mut self) {
        match self.press_pull_target() {
            PressPull::Extrude => {
                let face = self.cur.picked.is_some();
                let Some(id) = self.action_for_role(&jc_interchange::role::action::EXTRUDE) else {
                    return;
                };
                self.win.last_item = Some(format!("a:{id}"));
                self.start_action(&id);
                if face && self.win.message.is_none() {
                    self.win.message = Some(
                        "プレス/プル: 平らな面なので押し出しにしました(押すと結合、引くと切り取り)"
                            .into(),
                    );
                }
            }
            PressPull::Fillet => {
                // ツールバーの「フィレット」と同じ(選んだ辺で 1 行目を作る)
                let Some(id) = self.action_for_role(&jc_interchange::role::action::FILLET_EDGES)
                else {
                    return;
                };
                self.win.last_item = Some(format!("a:{id}"));
                if self.start_action(&id).is_some() && self.win.message.is_none() {
                    self.win.message =
                        Some("プレス/プル: 辺なのでフィレットにしました(半径を決めます)".into());
                }
            }
            PressPull::Edit(id, kind) => {
                let name = self
                    .cur
                    .work
                    .doc()
                    .data()
                    .feature(id)
                    .map(|f| f.name.clone())
                    .unwrap_or_default();
                self.commit_creation();
                self.open_edit(id);
                self.win.message =
                    Some(format!("プレス/プル: {kind}の面なので「{name}」を直します"));
            }
            PressPull::Offset(type_id) => {
                // 選んでいた面は、面のオフセットの面の欄に入る
                if self.add_described(&type_id).is_some() && self.win.message.is_none() {
                    self.win.message = Some(
                        "プレス/プル: 曲がった面なので面のオフセットにしました(正で肉を足し、負で削る)"
                            .into(),
                    );
                }
            }
            PressPull::Refuse(why) => self.win.fail(format!("プレス/プル: {why}")),
        }
    }
}
