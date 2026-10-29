//! 画面を持たない作業(CLI・MCP が共通で使う)。文書、モジュールの一式、再計算の結果を持ち、
//! モジュールの操作を実行する。画面と同じ拡張点([`kmb_core::Extension`])を通るので、
//! ここは操作の中身(加工、書き出しなど)を知らない。

use crate::edit::GroupMode;
use kmb_core::{
    Document, FeatureId, JobReport, MassProps, Modules, Op, RecomputeReport, ShapeHandle,
    Transaction,
};
use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// ボディごとの測定値。ソリッドには体積と質量、面のボディには面積と外形を返す。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct BodyMass {
    /// ブラウザと同じボディの名前
    pub name: String,
    /// ボディを最初に作った機能(材料はこの ID に付く)
    pub root: FeatureId,
    #[serde(flatten)]
    pub props: Option<MassProps>,
    #[serde(flatten)]
    pub sheet: Option<SheetFacts>,
    /// 付けた物理材料
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// 材料の密度(g/cm³)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density: Option<f64>,
    /// 質量(g)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass: Option<f64>,
}

/// 面のボディの測定値。体積と質量の項目を持たない。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SheetFacts {
    pub body_kind: kmb_core::kernel::BodyKind,
    pub description: String,
    pub area: f64,
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// 画面で選んでいる物(操作の文脈に渡す)。CLI・MCP は機能を 1 つ選ぶだけ([`Headless::run`])。
#[derive(Debug, Clone, Copy, Default)]
pub struct Picks<'a> {
    /// タイムラインで選んでいる機能
    pub selected: Option<FeatureId>,
    /// 3D のビューで選んでいる面と、一緒に選んでいる面
    pub picked: Option<&'a kmb_core::FacePick>,
    pub also_picked: &'a [kmb_core::FacePick],
    /// 3D のビューで選んでいる辺と頂点
    pub edges: &'a [kmb_core::EdgePick],
    pub vertices: &'a [kmb_core::VertexPick],
    /// 一緒に選んでいる機能
    pub also_selected: &'a [FeatureId],
    /// 選んでいる機能をボディとして扱うか
    pub bodies_picked: bool,
    /// 開いている文書の番号(画面のタブごとに違う)
    pub doc: u64,
    /// AI から動く時の、操作が開いてよいファイル(許したフォルダ。[`crate::roots::Roots`])
    pub files: Option<&'a dyn kmb_core::FileGate>,
}

/// [`Headless::run_with`] の結果。
pub enum Handled {
    /// 文書を変えた(足した機能の ID)。適用と再計算は済み
    Changed(Vec<FeatureId>),
    /// 文書の変更でない結果(呼ぶ側が扱う)。大きいので箱に入れる
    Other(Box<kmb_core::Outcome>),
}

/// 操作を実行した結果。
#[derive(Debug)]
pub enum Ran {
    /// 文書を変えた(足した機能の ID)
    Changed(Vec<FeatureId>),
    /// 書き出すファイル(書く場所は呼ぶ側が決める)
    File {
        suffix: String,
        contents: Vec<u8>,
        info: Json,
    },
    /// いくつもの書き出すファイル(名前の後ろと中身。書く場所は呼ぶ側が [`kmb_core::sibling_path`] で決める)
    Files {
        files: Vec<(String, Vec<u8>)>,
        info: Json,
    },
    /// 知らせ
    Message(String),
    /// 時間のかかる仕事の結果(ここでは終わるまで待つ)
    Report(JobReport),
}

mod actions;
mod active;
mod background;
mod params;
mod status;
mod view;
pub use active::Settled;
pub use background::{After, Polled, Recomputed};

/// 開いている文書。CLI・MCP は変更のたびにその場で再計算する。画面は裏で再計算する
/// ([`Headless::in_background`])。
pub struct Headless {
    doc: Document,
    path: Option<PathBuf>,
    view: crate::doc_view::DocView,
    /// None は CLI・MCP の自動選択、Some(None) は画面の保存なし。
    view_directory: Option<Option<PathBuf>>,
    modules: Modules,
    report: RecomputeReport,
    /// 次の計算でも捨てずに残させる形(画面に出している形。計算が失敗しても出し続けるため、M27)
    keep: Vec<ShapeHandle>,
    /// 裏で再計算する時の様子(CLI・MCP は `None`)
    background: Option<background::Background>,
    /// アクティブな部品(開いている間だけの状態)
    active: active::Active,
    /// 作り直した回数(この場で計算した分。裏の計算は数えない)
    recomputes: u64,
    /// 作り直しを後へ回しているか([`Headless::hold_recompute`])
    hold: Hold,
    /// 作る途中(画面の OK の前)の機能と、その途中で使った時に隠した物([`Headless::begin_creation`])
    creating: Option<(FeatureId, BTreeSet<FeatureId>)>,
    /// 最後の変更で使った時に隠した物(作る途中を始めた時に引き継ぐ)
    just_hidden: BTreeSet<FeatureId>,
    /// 開いた文書の参照に前の版の伸びた面の名前が残っているかもしれない(次の計算の結果で今の名前に
    /// 直す。[`Headless::rename_old_faces`])
    old_names: bool,
}

/// 作り直しを後へ回す様子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hold {
    Off,
    /// 回している(まだ変えていない)
    On,
    /// 回している間に文書を変えた
    Owed,
}

impl Headless {
    /// 空の文書で始める。
    pub fn new(modules: Modules) -> Self {
        Self::with_document(modules, Document::new(), None)
    }

    pub fn with_document(modules: Modules, mut doc: Document, path: Option<PathBuf>) -> Self {
        // 古い版の文書の、読むボディを固定する印のうち要らない物を外す(読む形は変わらない)
        doc.release_pins(&modules.registry);
        let report = doc.recompute(&modules.registry, modules.kernel.as_ref());
        let mut h = Headless {
            doc,
            path,
            view: Default::default(),
            view_directory: None,
            modules,
            report,
            keep: vec![],
            background: None,
            active: Default::default(),
            recomputes: 1,
            hold: Hold::Off,
            creating: None,
            just_hidden: BTreeSet::new(),
            old_names: true,
        };
        if h.rename_old_faces() {
            h.recompute();
        }
        h
    }

    /// 開いた後の最初の計算の結果で、前の版の伸びた面の名前の参照を今の名前に直す
    /// ([`Document::rename_old_faces`])。直したら `true`(呼ぶ側が計算し直す)。
    fn rename_old_faces(&mut self) -> bool {
        std::mem::take(&mut self.old_names)
            && self
                .doc
                .rename_old_faces(&self.modules.registry, &self.report)
                > 0
    }

    /// ファイルを開く。
    pub fn open(modules: Modules, path: &Path) -> Result<Self, kmb_core::Problem> {
        let doc = crate::load(path)?;
        Ok(Self::with_document(modules, doc, Some(path.to_path_buf())))
    }

    /// 文書を入れ替える(開く、見本)。裏で前の文書を計算している時は、その結果を使わない。
    /// 前の文書を返す(画面は開くのをやめた時に戻せるよう持っておく)。
    pub fn replace(&mut self, doc: Document, path: Option<PathBuf>) -> Document {
        if let Some(bg) = &mut self.background {
            bg.forget_running();
        }
        let old = std::mem::replace(&mut self.doc, doc);
        self.doc.release_pins(&self.modules.registry);
        self.path = path;
        self.active = Default::default();
        self.restore_view();
        self.old_names = true;
        self.recompute();
        old
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn report(&self) -> &RecomputeReport {
        &self.report
    }

    pub fn modules(&self) -> &Modules {
        &self.modules
    }

    /// モジュールの設定の一覧の、この文書での値(文書ごとに持つ設定は文書の値)。
    pub fn settings(&self) -> Vec<kmb_core::Settings> {
        self.modules.extensions.settings_in(self.doc.data())
    }

    /// 設定 `id` を `value` に変える。PC 全体の値(新しい文書の既定になる、前回使った値)を変え、
    /// 文書ごとに持つ設定(レンダリングのシーンなど)なら、この文書にも 1 回の取り消し単位で書く。
    /// 文書を変えたら真。値が正しくなければ何も変えずに理由を返す。
    pub fn set_settings(&mut self, id: &str, value: &serde_json::Value) -> Result<bool, String> {
        let ext = &self.modules.extensions;
        // 文書の変更を先に作る(値の確かめを兼ねる。誤りなら PC 全体の値も変えない)
        let edit = ext
            .document_settings_edit(id, value, self.doc.data())
            .transpose()?;
        ext.set_settings(id, value)?;
        let Some(edit) = edit else {
            return Ok(false);
        };
        let label = ext
            .settings()
            .into_iter()
            .find(|g| g.id == id)
            .map_or_else(|| id.to_string(), |g| g.label);
        self.apply_edits(&format!("{label}の設定を変える"), vec![edit])?;
        Ok(true)
    }

    /// [`Self::set_settings`] の、AI(MCP)から来た値の版。ファイルの欄(ポストのプラグインなど)を
    /// `roots` の中の絶対パスにしてから変え、外を指せば何も変えずに断る。
    pub fn set_settings_in(
        &mut self,
        id: &str,
        value: &serde_json::Value,
        roots: &crate::roots::Roots,
    ) -> Result<bool, String> {
        let mut value = value.clone();
        if let Some(g) = self.settings().into_iter().find(|g| g.id == id) {
            roots.confine_settings(&g, &mut value)?;
        }
        self.set_settings(id, &value)
    }

    /// 形のカーネル。
    pub fn kernel(&self) -> &std::sync::Arc<dyn kmb_core::Kernel> {
        &self.modules.kernel
    }

    /// 再計算する。裏で再計算する時は、計算が要る印を付ける(走っている計算は終わった後で
    /// やり直す)。始めるのは [`Self::start_recompute`]。
    pub fn recompute(&mut self) {
        if self.hold != Hold::Off {
            self.hold = Hold::Owed;
            return;
        }
        if let Some(bg) = &mut self.background {
            bg.mark();
            return;
        }
        self.doc.hold_shapes(self.keep.iter().copied());
        self.recomputes += 1;
        self.report = self
            .doc
            .recompute(&self.modules.registry, self.modules.kernel.as_ref());
        self.settle_active();
        if self.rename_old_faces() {
            self.recompute();
        }
    }

    /// 次の計算でも捨てずに残させる形(画面に出している形)を決める。
    pub fn keep_shapes(&mut self, shapes: Vec<ShapeHandle>) {
        self.keep = shapes;
    }

    /// 保存する(壊れない書き込み、直前の版を `.bak` に残す)。`path` を省くと開いた場所へ。
    pub fn save(&mut self, path: Option<&Path>) -> Result<PathBuf, String> {
        let p = match path {
            Some(p) => p.to_path_buf(),
            None => self
                .path
                .clone()
                .ok_or("保存先がありません(path を指定してください)")?,
        };
        // 版の履歴に縮小図も残す(形が無ければ絵なし)
        let snap = self
            .meshes()
            .ok()
            .map(|m| {
                let b: Vec<_> = m.iter().map(|(f, m)| (*f, m)).collect();
                crate::history::snapshot_of(&b)
            })
            .unwrap_or_default();
        crate::save_with(&p, &self.doc, &snap, "")?;
        self.set_path(Some(p.clone()));
        Ok(p)
    }

    /// 版の履歴(新しい順。候補 35)。保存していない文書は空。
    pub fn versions(&self) -> Vec<crate::history::Version> {
        self.path
            .as_deref()
            .map(crate::history::list)
            .unwrap_or_default()
    }

    /// 版 `id`(指紋か版の番号)を最新として戻し、保存する(戻したことも新しい版として残る)。
    /// 戻した版を返す。
    pub fn restore_version(&mut self, id: &str) -> Result<crate::history::Version, String> {
        let path = self
            .path
            .clone()
            .ok_or("保存していない文書には版がありません")?;
        let list = crate::history::list(&path);
        let v = list
            .iter()
            .find(|v| v.id == id || v.number.to_string() == id)
            .ok_or_else(|| format!("版 {id} がありません"))?
            .clone();
        let doc = crate::history::read(&path, &v.id)?;
        self.replace(doc, Some(path.clone()));
        let snap = self
            .meshes()
            .ok()
            .map(|m| {
                let b: Vec<_> = m.iter().map(|(f, m)| (*f, m)).collect();
                crate::history::snapshot_of(&b)
            })
            .unwrap_or_default();
        crate::save_with(
            &path,
            &self.doc,
            &snap,
            &format!("版 {} に戻した", v.number),
        )?;
        Ok(v)
    }

    /// 変更を 1 回の取り消し単位で適用して再計算する。`f` で ID を割り当てられる。
    pub fn edit(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Document) -> Result<Vec<Op>, String>,
    ) -> Result<(), String> {
        let ops = f(&mut self.doc)?;
        self.doc
            .apply(Transaction::new(label, ops))
            .map_err(|e| e.to_string())?;
        self.recompute();
        Ok(())
    }

    /// 機能 `ids` をまとめて機能 `target` の前(`after` なら後ろ)へ動かす(画面のタイムラインの
    /// 並べ替えと同じ確かめ)。動かす先が今と同じなら何もしない。
    pub fn move_features(
        &mut self,
        ids: &[FeatureId],
        target: FeatureId,
        after: bool,
    ) -> Result<(), String> {
        match crate::edit::move_features(
            self.doc.data(),
            &self.modules.registry,
            ids,
            target,
            after,
        )? {
            Some(tx) => self.apply(tx),
            None => Ok(()),
        }
    }

    /// 設計の機能 `ids` を写してタイムラインの今の位置に貼る(U42。画面のコピー・貼り付けと同じ:
    /// 値と式は写し、面・辺の選ぶ欄は空にする。選び直しは `set_feature` で)。貼った機能の番号と、
    /// 空にした選ぶ欄の数。写せるのはボディを作る・変える機能だけ。
    pub fn paste_features(&mut self, ids: &[FeatureId]) -> Result<(Vec<FeatureId>, usize), String> {
        let data = self.doc.data();
        let registry = &self.modules.registry;
        for id in ids {
            if data.feature(*id).is_none() {
                return Err(format!("機能 {id} がありません"));
            }
        }
        let entries: Vec<kmb_core::FeatureEntry> = data
            .features
            .iter()
            .filter(|f| ids.contains(&f.id))
            .filter(|f| {
                registry
                    .descriptor(&f.type_id)
                    .is_some_and(|d| d.category == kmb_core::category::SOLID)
            })
            .cloned()
            .collect();
        if entries.len() != ids.len() {
            return Err(
                "写せるのはボディを作る・変える機能です(スケッチと加工は写せません)".into(),
            );
        }
        // 一緒に写していないボディは、今のボディの最後の有効な機能へ付け替える(画面と同じく、
        // 前の形から枝分かれした別のボディを作らない)
        let end = data.rollback.unwrap_or(data.features.len());
        let tip = |old: FeatureId| {
            let root = kmb_core::body_root(data, registry, old);
            data.features[..end.min(data.features.len())]
                .iter()
                .rev()
                .find(|f| !f.suppressed && kmb_core::body_root(data, registry, f.id) == root)
                .map(|f| f.id)
        };
        let tips: Vec<(FeatureId, Option<FeatureId>)> = entries
            .iter()
            .flat_map(|e| self.body_fields(e))
            .map(|old| (old, tip(old)))
            .collect();
        let registry = self.modules.registry.clone();
        let pasted = crate::edit::paste_features(&mut self.doc, &registry, &entries, &|old| {
            tips.iter().find(|(o, _)| *o == old).and_then(|(_, t)| *t)
        });
        self.apply(pasted.tx)?;
        Ok((pasted.ids, pasted.cleared))
    }

    /// 機能 `e` のボディの欄が指す機能。
    fn body_fields(&self, e: &kmb_core::FeatureEntry) -> Vec<FeatureId> {
        let Some(desc) = self.modules.registry.descriptor(&e.type_id) else {
            return Vec::new();
        };
        let d = desc.upgraded(&e.data);
        desc.fields
            .iter()
            .filter(
                |f| matches!(&f.kind, kmb_core::FieldKind::Output { slot, .. } if slot == "body"),
            )
            .filter_map(|f| d.get(&f.key).and_then(|v| v.as_u64()).map(FeatureId))
            .collect()
    }

    /// 名前を付けた視点を保存する(U20。画面のブラウザの「保存」と同じ。名前が空なら「視点N」、
    /// 重なれば番号を足す)。割り当てた番号。
    pub fn save_named_view(&mut self, view: kmb_core::NamedView) -> Result<u64, String> {
        let (tx, id) = crate::edit::save_named_view(&mut self.doc, view);
        self.apply(tx)?;
        Ok(id)
    }

    /// 名前を付けた視点の名前を変える。
    pub fn rename_named_view(&mut self, id: u64, name: &str) -> Result<(), String> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("視点の名前が空です".into());
        }
        let label = format!("視点の名前を「{name}」にする");
        let tx = crate::edit::change_named_view(self.doc.data(), id, &label, |v| v.name = name)?;
        self.apply(tx)
    }

    /// 名前を付けた視点を消す。
    pub fn remove_named_view(&mut self, id: u64) -> Result<(), String> {
        let tx = crate::edit::remove_named_view(self.doc.data(), id)?;
        self.apply(tx)
    }

    /// 機能 `ids` を消すと中身が全部なくなる部品の名前。`with_users` なら、`ids` を使う機能
    /// (たどった先も)も一緒に消す時(採用分 10-24 a1)。
    pub fn parts_emptied_by(&self, ids: &[FeatureId], with_users: bool) -> Vec<String> {
        let data = self.doc.data();
        let mut gone = ids.to_vec();
        if with_users {
            gone.extend(crate::edit::dependents(data, &self.modules.registry, ids));
        }
        crate::edit::components_emptied(data, &gone)
            .into_iter()
            .map(|c| c.name.clone())
            .collect()
    }

    /// 機能 `ids` を、使う機能と中身がなくなる部品と一緒に消す変更(まだ当てない。画面が自分の道で
    /// 当てる)と、消える機能の全部。
    pub fn remove_with_users_tx(&self, ids: &[FeatureId]) -> (Transaction, Vec<FeatureId>) {
        let data = self.doc.data();
        let mut all = ids.to_vec();
        all.extend(crate::edit::dependents(data, &self.modules.registry, ids));
        (
            crate::edit::remove_features_with_users(data, &self.modules.registry, ids),
            all,
        )
    }

    /// 部品 `id` を中の部品ごと複製する(F7。画面のブラウザの「複製」と同じ)。作った部品の番号。
    pub fn duplicate_component(&mut self, id: u64) -> Result<u64, String> {
        let (tx, made) = self.duplicate_component_tx(id)?;
        self.apply(tx)?;
        Ok(made)
    }

    /// 部品を複製する変更を組む(番号は割り当てるが、まだ適用しない。画面が自分の道で適用する)。
    pub fn duplicate_component_tx(&mut self, id: u64) -> Result<(Transaction, u64), String> {
        let made = crate::components::duplicate_component(
            &mut self.doc,
            &self.modules.registry,
            self.modules.kernel.as_ref(),
            &self.report,
            id,
        )?;
        // 複製は作った部品に入れる(アクティブな部品へは移さない)
        self.skip_born();
        Ok(made)
    }

    /// 部品 `id` を中の部品ごと `at` へ移す(F7。画面の部品の木のドラッグと同じ)。
    pub fn move_component(&mut self, id: u64, at: crate::components::Place) -> Result<(), String> {
        match crate::components::move_component(self.doc.data(), id, at)? {
            Some(tx) => self.apply(tx),
            None => Ok(()),
        }
    }

    /// ボディ(どれかの機能の番号)を部品 `to` へ移す(`None` は部品から外す。画面のドラッグと同じ)。
    pub fn move_bodies(&mut self, bodies: &[FeatureId], to: Option<u64>) -> Result<(), String> {
        match crate::components::move_bodies(self.doc.data(), &self.modules.registry, bodies, to)? {
            Some(tx) => self.apply(tx),
            None => Ok(()),
        }
    }

    /// 組み立てた変更([`crate::edit`])を適用して再計算する。
    pub fn apply(&mut self, tx: Transaction) -> Result<(), String> {
        let touched: Vec<FeatureId> = tx
            .ops
            .iter()
            .filter_map(|op| match op {
                kmb_core::Op::SetFeatureData { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        self.commit(touched, |doc| doc.apply(tx))?;
        self.recompute();
        Ok(())
    }

    /// 足した機能 `id` を作る途中にする(画面は足した後、OK かキャンセルまで直す)。足した変更で隠した
    /// スケッチと作業平面などを覚え、途中で輪郭などを選び替えて使わなくなったら出し直す。
    pub fn begin_creation(&mut self, id: FeatureId) {
        self.creating = Some((id, std::mem::take(&mut self.just_hidden)));
    }

    /// 作る途中を終える(OK・キャンセル)。この後は使わなくなっても出し直さない。
    pub fn end_creation(&mut self) {
        self.creating = None;
    }

    /// 文書を変える `change` を走らせ、足した機能と直した機能 `touched` が初めて使うスケッチと
    /// 作業平面などを同じ取り消し単位で隠す(使った時に 1 回隠す。変更を取り消せば目も戻る)。
    /// 作る途中なら、その途中で隠して使わなくなった物を出し直す。再計算はしない。
    pub(crate) fn commit<T>(
        &mut self,
        mut touched: Vec<FeatureId>,
        change: impl FnOnce(&mut Document) -> Result<T, kmb_core::TxError>,
    ) -> Result<T, String> {
        use kmb_core::SectionType;
        use kmb_interchange::visibility as vis;
        self.just_hidden.clear();
        if self
            .creating
            .as_ref()
            .is_some_and(|(id, _)| self.doc.data().feature(*id).is_none())
        {
            self.creating = None;
        }
        let depth = self.doc.undo_depth();
        let ids: std::collections::HashSet<FeatureId> =
            self.doc.data().features.iter().map(|f| f.id).collect();
        let eyes_before = self.doc.data().sections.get(vis::Visibility::KEY).cloned();
        let (before, old) = {
            let cx = self.context(None, None, &Json::Null);
            let refs: BTreeSet<FeatureId> = touched
                .iter()
                .flat_map(|id| vis::references(&cx, *id))
                .collect();
            (refs, vis::of(&cx).hidden)
        };
        let out = change(&mut self.doc).map_err(|e| e.to_string())?;
        touched.extend(
            self.doc
                .data()
                .features
                .iter()
                .map(|f| f.id)
                .filter(|id| !ids.contains(id)),
        );
        let cx = self.context(None, None, &Json::Null);
        // 作る途中に目を自分で替えた物は、もう出し直さない
        let mut mine = self
            .creating
            .as_ref()
            .map(|c| c.1.clone())
            .unwrap_or_default();
        if !mine.is_empty()
            && self.doc.data().sections.get(vis::Visibility::KEY) != eyes_before.as_ref()
        {
            let new = vis::of(&cx).hidden;
            mine.retain(|r| old.contains(r) == new.contains(r));
        }
        let v = vis::hide_first_used(&cx, &touched, &before, &mine);
        if let Some((_, m)) = &mut self.creating {
            m.clone_from(&mine);
        }
        let Some(v) = v else {
            return Ok(out);
        };
        let op = match self.doc.data().section_op(&v) {
            Ok(op) => op,
            Err(e) => {
                log::warn!("使ったスケッチを隠せない: {e}");
                return Ok(out);
            }
        };
        let Some(label) = self.doc.undo_labels().last().map(|l| l.to_string()) else {
            return Ok(out);
        };
        match self.doc.apply(Transaction::new(label.clone(), vec![op])) {
            Ok(()) => {
                self.doc.squash_since(depth, &label);
                self.just_hidden = v.hidden.difference(&old).copied().collect();
                if let Some((_, mine)) = &mut self.creating {
                    mine.retain(|r| v.hidden.contains(r));
                    mine.extend(self.just_hidden.iter().copied());
                }
            }
            Err(e) => log::warn!("使ったスケッチを隠せない: {e}"),
        }
        Ok(out)
    }

    /// 変更 `edits` で直す機能。
    fn edited(edits: &[kmb_core::Edit]) -> Vec<FeatureId> {
        edits
            .iter()
            .filter_map(|e| match e {
                kmb_core::Edit::SetData { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    }

    /// 読めない機能のデータ(数の欄に文字の "NaN" など)を、文書に入れる前に断る。文書に入れると
    /// 赤い機能が残り、後の機能まで誤りになるので、CLI・MCP の入口で作る時も直す時も確かめる
    /// (画面は欄から作るので読めないデータにならない)。
    pub fn check_data(&self, type_id: &str, data: &Json) -> Result<(), String> {
        match self.modules.registry.instantiate(type_id, data) {
            Some(Err(e)) => Err(format!("{type_id} のデータを読めません: {e}")),
            _ => Ok(()),
        }
    }

    /// 機能を最後に足す。名前を省くと種類と ID から付ける。
    pub fn add_feature(
        &mut self,
        type_id: &str,
        name: Option<&str>,
        mut data: Json,
    ) -> Result<FeatureId, String> {
        if !self.modules.registry.contains(type_id) {
            return Err(format!("未登録の機能の種類です: {type_id}(types で一覧)"));
        }
        // 省いた欄は、画面から作った時と同じ既定にする(フィレットの角の形など)
        if let Some(d) = self.modules.registry.descriptor(type_id) {
            d.fill_create_defaults(&mut data);
        }
        self.check_data(type_id, &data)?;
        let name = name.map_or_else(|| self.new_feature_name(type_id), String::from);
        // 続けて足す種類(ボディの分割の残りの側など)があれば、同じ取り消し単位で足す
        let companion = self
            .modules
            .registry
            .descriptor(type_id)
            .and_then(|d| d.companion);
        if let Some((ctype, cdata)) = companion {
            let cname = self
                .modules
                .registry
                .descriptor(&ctype)
                .map_or_else(|| ctype.clone(), |_| self.new_feature_name(&ctype));
            let added = self.commit(Vec::new(), |doc| {
                doc.apply_edits(
                    "機能を追加",
                    vec![
                        kmb_core::Edit::Insert {
                            type_id: type_id.into(),
                            name,
                            data,
                        },
                        kmb_core::Edit::Insert {
                            type_id: ctype,
                            name: cname,
                            data: cdata,
                        },
                    ],
                )
            })?;
            self.recompute();
            return Ok(added[0]);
        }
        let (tx, id) =
            crate::edit::insert_feature(&mut self.doc, "機能を追加", type_id, &name, data);
        self.apply(tx)?;
        Ok(id)
    }

    /// 種類 `type_id` の機能を足す時の名前(画面と同じく「押し出し3」のように、種類の名前に
    /// その種類の数 + 1 を付ける。種類の名前が無ければ種類の ID)。
    pub fn new_feature_name(&self, type_id: &str) -> String {
        let label = self
            .modules
            .registry
            .descriptor(type_id)
            .map_or_else(|| type_id.to_string(), |d| d.label);
        let n = self
            .doc
            .data()
            .features
            .iter()
            .filter(|f| f.type_id == type_id)
            .count();
        format!("{label}{}", n + 1)
    }

    /// 機能 `ids` を名前 `name` のグループにする(続いていなければ理由を返す)。
    pub fn group(&mut self, ids: &[FeatureId], name: &str, mode: GroupMode) -> Result<u64, String> {
        let tx = crate::edit::make_group(&mut self.doc, &self.modules.registry, ids, name, mode)
            .map_err(|e| e.describe(self.doc.data()))?;
        self.apply(tx)?;
        Ok(self.doc.data().groups.last().map_or(0, |g| g.id))
    }

    /// グループを解く(中身は残す)。
    pub fn ungroup(&mut self, gid: u64) -> Result<(), String> {
        let tx = crate::edit::ungroup(self.doc.data(), gid)?;
        self.apply(tx)
    }

    /// 設計の属性(U13。決まった項目と自分で足した項目)。
    pub fn properties(&self) -> Json {
        let p = &self.doc.data().properties;
        let mut out = serde_json::Map::new();
        for (key, _) in kmb_core::Properties::FIELDS {
            out.insert(key.into(), Json::from(p.get(key).unwrap_or("")));
        }
        out.insert("tags".into(), Json::from(p.tags.clone()));
        out.insert("units".into(), Json::from(p.units.name()));
        for (k, v) in &p.custom {
            out.insert(k.clone(), Json::from(v.as_str()));
        }
        Json::Object(out)
    }

    /// 設計の属性を変える(書いた鍵だけ。空の値は消す。`tags` は「,」で区切ったタグ。1 回で取り消せる)。
    pub fn set_properties(&mut self, values: &[(String, String)]) -> Result<(), String> {
        let mut p = self.doc.data().properties.clone();
        for (k, v) in values {
            if k.trim().is_empty() {
                return Err("属性の名前が空です".into());
            }
            if k == "units" && !v.trim().is_empty() && kmb_core::LengthUnit::parse(v).is_none() {
                return Err(format!(
                    "長さの単位 {v} は使えません(mm・cm・m・in・ft のどれか)"
                ));
            }
            p.set(k, v);
        }
        self.apply(crate::edit::set_properties(p))
    }

    /// 読み込んだ STEP の機能を、Y が上の形として立てる(U12。X 軸のまわりに 90 度)。
    pub fn stand_up(&mut self, id: FeatureId, on: bool) -> Result<(), String> {
        let f = self
            .doc
            .data()
            .feature(id)
            .ok_or_else(|| format!("機能 {id} がありません"))?;
        let mut data = f.data.clone();
        data["stand_up"] = Json::from(on);
        let tx = crate::edit::set_feature_data(self.doc.data(), id, data);
        self.apply(tx)
    }

    pub fn undo(&mut self) -> Result<(), String> {
        if !self.doc.undo() {
            return Err("取り消せる操作がありません".into());
        }
        self.recompute();
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), String> {
        if !self.doc.redo() {
            return Err("やり直せる操作がありません".into());
        }
        self.recompute();
        Ok(())
    }

    /// 番号を割り当てながら変更を組む([`crate::edit`]・[`crate::params`] の `&mut Document` を
    /// 受け取る組み立て)。`f` は文書の中身を直に変えず、組んだ変更は [`Self::apply`] で適用する。
    pub fn prepare<T>(&mut self, f: impl FnOnce(&mut Document) -> T) -> T {
        f(&mut self.doc)
    }

    /// 機能の番号を割り当てる(足す変更を組む前。画面の貼り付け・複製)。
    pub fn alloc_feature_id(&mut self) -> FeatureId {
        self.doc.alloc_feature_id()
    }

    /// グループ(部品)の番号を割り当てる。
    pub fn alloc_group_id(&mut self) -> u64 {
        self.doc.alloc_group_id()
    }

    /// 操作の結果の変更([`kmb_core::Edit`])を 1 回の取り消し単位で適用して再計算する。足した
    /// 機能の ID を順に返す。
    pub fn apply_edits(
        &mut self,
        label: &str,
        edits: Vec<kmb_core::Edit>,
    ) -> Result<Vec<FeatureId>, String> {
        let added = self.commit(Self::edited(&edits), |doc| doc.apply_edits(label, edits))?;
        self.recompute();
        Ok(added)
    }

    /// やり直しの履歴を捨てる(戻した後に別の変更を始めた時)。
    pub fn clear_redo(&mut self) {
        self.doc.clear_redo();
    }

    /// 寸法の名前(`d1` など)をそろえる(名前を付けただけでは変更にしない時に、覚える前に呼ぶ)。
    pub fn settle_names(&mut self) {
        self.doc.settle_names(&self.modules.registry);
    }

    /// 計算の控えを捨てて、次は初めから計算する(カーネルを分け合う別の文書の計算で、この文書の
    /// 形が捨てられた時)。
    pub fn drop_cache(&mut self) {
        drop(self.doc.take_cache());
        self.recompute();
    }

    /// 保存先を変える(名前を付けて保存した時など)。
    pub fn set_path(&mut self, path: Option<PathBuf>) {
        self.path = path;
        if let Err(why) = self.save_doc_view(self.view.clone()) {
            log::warn!("文書の見え方を書けません: {why}");
        }
    }

    /// 前の文書と、その時の計算の結果に戻す(開くのをやめた時)。走っている計算は入れ替える前の
    /// 文書のものなので結果を使わず、戻した文書をもう一度計算する。
    pub fn put_back(&mut self, doc: Document, path: Option<PathBuf>, report: RecomputeReport) {
        if let Some(bg) = &mut self.background {
            bg.forget_running();
        }
        self.doc = doc;
        self.path = path;
        self.restore_view();
        self.report = report;
        self.recompute();
    }

    /// 作り直しを後へ回す(`true`)か、やめる(`false`)。回している間に文書を変えたら、やめる時に
    /// 1 回だけ作り直す。まとめて実行で値をいくつも直す時に、1 手ごとに作り直さないため(10-20 s18)。
    /// 回している間の計算の結果(`report` など)は前の形のままなので、結果を読む手の前ではやめる。
    pub fn hold_recompute(&mut self, hold: bool) {
        let owed = self.hold == Hold::Owed;
        self.hold = if hold && owed {
            Hold::Owed
        } else if hold {
            Hold::On
        } else {
            Hold::Off
        };
        if !hold && owed {
            self.recompute();
        }
    }

    /// 作り直した回数(まとめて実行で 1 回にまとまったかを確かめる)。
    pub fn recomputes(&self) -> u64 {
        self.recomputes
    }

    /// まとめて実行の始めの位置(取り消せる変更の数)。
    pub fn mark(&self) -> usize {
        self.doc.undo_depth()
    }

    /// 始めの位置 `mark` まで戻して再計算する(まとめて実行の途中で失敗した時)。戻した数を返す。
    pub fn rollback_to(&mut self, mark: usize) -> usize {
        let n = self.doc.rollback_to(mark);
        if n > 0 {
            log::info!("まとめて実行を戻した: {n} 手");
            self.recompute();
        }
        n
    }

    /// `mark` より後の変更を 1 回の取り消し単位 `label` にまとめる(まとめて実行が通った時)。
    pub fn squash_since(&mut self, mark: usize, label: &str) -> usize {
        self.doc.squash_since(mark, label)
    }

    /// ボディに物理材料を付ける(M26)。`body` はボディを最初に作った機能か、その後の機能の ID
    /// (`None` なら見えているボディすべて)。空の名前なら外す。表に無い名前は断る。
    pub fn set_body_material(
        &mut self,
        body: Option<FeatureId>,
        material: &str,
    ) -> Result<(), String> {
        let name = material.trim();
        if !name.is_empty() && kmb_look::material::phys_material(name).is_none() {
            let known: Vec<&str> = kmb_look::material::PHYS_MATERIALS
                .iter()
                .map(|m| m.name)
                .collect();
            return Err(format!(
                "材料 {name} は知りません(使える材料: {})",
                known.join("・")
            ));
        }
        let reg = &self.modules.registry;
        let data = self.doc.data();
        let roots: Vec<FeatureId> = match body {
            Some(id) => {
                if data.feature(id).is_none() {
                    return Err(format!("機能 {id} がありません"));
                }
                vec![kmb_core::body_root(data, reg, id)]
            }
            None => {
                self.bodies()?;
                kmb_core::visible_bodies(data, reg, &self.report)
                    .into_iter()
                    .map(|(id, _)| kmb_core::body_root(data, reg, id))
                    .collect()
            }
        };
        let tx = crate::edit::set_body_materials(data, &roots, name)?;
        self.apply(tx)
    }
}
