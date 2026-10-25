//! 設計の機能のコピー・貼り付け(U42。タイムラインとブラウザの右クリック、作業空間「設計」の
//! Ctrl+C / Ctrl+V)。写すのは機能の値と式で、面・辺などの選ぶ欄は空にして貼り、最初の選ぶ欄を
//! クリックを受ける状態にする(貼る先の面や辺は利用者が選び直す)。
//!
//! 避ける形(docs/避ける形.md): 貼る先の形を探して似た面・辺を勧めない、消えた参照を
//! 似た物で自動では直さない。選ぶ欄は必ず空にし、選び直しは利用者のクリックだけで行う。
//! 前の機能を指す欄(対象のボディ・スケッチ)はそのまま残す(貼る所は今の最後なので必ず前にある)。

use crate::Workspace;
use kmb_core::{FeatureEntry, FeatureId, category};

/// 写した設計の機能。
#[derive(Clone)]
pub(crate) struct FeatureClip {
    pub entries: Vec<FeatureEntry>,
    /// OS のクリップボードに置いた文(Ctrl+V で届いた文がこれなら貼る)
    pub text: String,
}

impl Workspace {
    /// 写せる設計の機能か(ボディを作る・変える機能。スケッチと加工は別の写し方がある)。
    pub(crate) fn is_copyable_feature(&self, id: FeatureId) -> bool {
        self.cur
            .work
            .doc()
            .data()
            .feature(id)
            .and_then(|f| self.win.registry.descriptor(&f.type_id))
            .is_some_and(|d| d.category == category::SOLID)
    }

    /// 選んだ設計の機能を写す(値と式)。写した数を返す。`ctx` があれば OS のクリップボードにも
    /// 短い文を置く(Ctrl+V を届かせるため)。
    pub fn copy_features(&mut self, ctx: Option<&egui::Context>, ids: &[FeatureId]) -> usize {
        let data = self.cur.work.doc().data();
        let entries: Vec<FeatureEntry> = data
            .features
            .iter()
            .filter(|f| ids.contains(&f.id) && self.is_copyable_feature(f.id))
            .cloned()
            .collect();
        if entries.is_empty() {
            self.win.message = Some(
                "写せるのはボディを作る・変える機能です(スケッチはスケッチの中で、加工は加工の作業空間で写す)"
                    .into(),
            );
            return 0;
        }
        let text = format!(
            "kieMonoBench の機能 {} 個({})",
            entries.len(),
            entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>()
                .join("、")
        );
        if let Some(ctx) = ctx {
            ctx.copy_text(text.clone());
        }
        let n = entries.len();
        log::info!("機能を写した: {n} 個");
        self.cur.feature_clip = Some(FeatureClip { entries, text });
        self.win.message = Some(format!(
            "機能 {n} 個を写しました。Ctrl+V(右クリックの「貼り付け」)で最後に貼り、面や辺を選び直す"
        ));
        n
    }

    /// 写した設計の機能があるか。
    pub fn has_feature_clip(&self) -> bool {
        self.cur.feature_clip.is_some()
    }

    /// 写した機能を、タイムラインの今の位置(ロールバックしていればそこ)に貼る。選ぶ欄は空にし、
    /// 最初の貼ったものの最初の選ぶ欄をクリックを受ける状態にする。貼ったものの番号を返す。
    pub fn paste_features(&mut self) -> Vec<FeatureId> {
        let Some(clip) = self.cur.feature_clip.clone() else {
            self.win.message = Some("先に機能をコピー(Ctrl+C)してください".into());
            return Vec::new();
        };
        self.commit_creation();
        // 対象のボディが写した時より後で変わっていれば、今のボディ(最後の機能)を対象にする。
        // そのままだと前の形から枝分かれした別のボディができる
        let tips = self.body_roots();
        let roots: Vec<(FeatureId, FeatureId)> = clip
            .entries
            .iter()
            .flat_map(|e| self.output_bodies(e))
            .map(|old| (old, self.body_root(old)))
            .collect();
        let tip = |old: FeatureId| {
            let root = roots.iter().find(|(o, _)| *o == old)?.1;
            tips.iter().find(|(_, r)| *r == root).map(|(t, _)| *t)
        };
        let registry = self.win.registry.clone();
        let pasted = self
            .cur
            .work
            .prepare(|d| kmb_session::edit::paste_features(d, &registry, &clip.entries, &tip));
        let (made, cleared) = (pasted.ids, pasted.cleared);
        if !self.apply(pasted.tx) {
            return Vec::new();
        }
        log::info!(
            "機能を貼り付けた: {} 個(選ぶ欄 {cleared} 個を空にした)",
            made.len()
        );
        if let Some(first) = made.first().copied() {
            self.open_edit(first);
            self.cur.also = made[1..].to_vec();
            // 最初の選ぶ欄(行の並びの行の欄も含む)をクリックを受ける状態にする
            let key = self.cur.work.doc().data().feature(first).and_then(|f| {
                let desc = self.win.registry.descriptor(&f.type_id)?;
                let d = desc.upgraded(&f.data);
                desc.pick_fields(&d)
                    .into_iter()
                    .find(|p| p.applies(&d))
                    .map(|p| p.key)
            });
            *self.cur.pick_field.active.borrow_mut() = key.map(|k| (first, k));
        }
        self.win.message = Some(if cleared > 0 {
            format!(
                "{} 個を貼りました。編集パネルの選ぶ欄で、貼る先の面や辺をクリックして選び直してください",
                made.len()
            )
        } else {
            format!("{} 個を貼りました(値と式はそのまま)", made.len())
        });
        made
    }

    /// 写した機能 `e` のボディの欄が指す機能(今の文書にある物だけ)。
    fn output_bodies(&self, e: &FeatureEntry) -> Vec<FeatureId> {
        let Some(desc) = self.win.registry.descriptor(&e.type_id) else {
            return Vec::new();
        };
        let d = desc.upgraded(&e.data);
        desc.fields
            .iter()
            .filter(
                |f| matches!(&f.kind, kmb_core::FieldKind::Output { slot, .. } if slot == "body"),
            )
            .filter_map(|f| d.get(&f.key).and_then(|v| v.as_u64()).map(FeatureId))
            .filter(|id| self.cur.work.doc().data().feature(*id).is_some())
            .collect()
    }

    /// タイムラインとブラウザの右クリックの、設計の機能のコピー・貼り付け。押したら真。
    pub(crate) fn feature_clip_menu(&mut self, ui: &mut egui::Ui, id: FeatureId) -> bool {
        if !self.is_copyable_feature(id) {
            return false;
        }
        let mut done = false;
        let sel = self.selection();
        let ids = if sel.contains(&id) { sel } else { vec![id] };
        if ui
            .button("コピー(Ctrl+C)")
            .on_hover_text("値と式を写す。貼る時に面や辺を選び直す")
            .clicked()
        {
            self.copy_features(Some(ui.ctx()), &ids);
            done = true;
        }
        let r = ui.add_enabled(
            self.has_feature_clip(),
            egui::Button::new("貼り付け(Ctrl+V)"),
        );
        let r = if self.has_feature_clip() {
            r.on_hover_text("タイムラインの最後に貼り、面や辺を選び直す")
        } else {
            r.on_disabled_hover_text("先に機能をコピー(Ctrl+C)する")
        };
        if r.clicked() {
            self.paste_features();
            done = true;
        }
        ui.separator();
        done
    }

    /// 作業空間「設計」の Ctrl+C / Ctrl+V(文字の欄を打っていない時だけ)。
    pub(crate) fn feature_clip_keys(&mut self, ctx: &egui::Context) {
        if self.win.layout.space != crate::Space::Design {
            return;
        }
        let events: Vec<egui::Event> = ctx.input(|i| {
            i.events
                .iter()
                .filter(|e| matches!(e, egui::Event::Copy | egui::Event::Paste(_)))
                .cloned()
                .collect()
        });
        for e in events {
            match e {
                egui::Event::Copy => {
                    let sel: Vec<FeatureId> = self
                        .selection()
                        .into_iter()
                        .filter(|id| self.is_copyable_feature(*id))
                        .collect();
                    if !sel.is_empty() {
                        self.copy_features(Some(ctx), &sel);
                    }
                }
                egui::Event::Paste(text)
                    if self
                        .cur
                        .feature_clip
                        .as_ref()
                        .is_some_and(|c| c.text == text) =>
                {
                    self.paste_features();
                }
                _ => {}
            }
        }
    }
}
