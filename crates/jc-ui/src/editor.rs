//! 専用の編集器(画面の拡張)。汎用の設定欄では表せない種類(スケッチ、工具ライブラリ)は、
//! 別の crate が [`FeatureEditor`] を実装し、実行ファイルが [`crate::Workspace::with_editor`] で
//! 組み込む。画面の外枠は、編集器が扱う種類の中身を知らない。

use jc_core::{
    DocumentData, FeatureEntry, FeatureId, FeatureStatus, Kernel, Quantity, RecomputeReport,
    Registry, Value,
};
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// 編集器に渡す、文書の状態と画面の知らせ。
pub struct EditorHost<'a> {
    pub data: &'a DocumentData,
    pub report: &'a RecomputeReport,
    pub registry: &'a Registry,
    pub kernel: &'a Arc<dyn Kernel>,
    /// ユーザーのデータフォルダ(なければ `None`)
    pub data_dir: Option<&'a Path>,
    /// ツールバーに出す知らせ
    pub message: &'a mut Option<String>,
    /// 直すのを終えてから始める操作と、決まっている引数。次の画面の更新で始まる。ほかの機能の操作は
    /// 名前を書かず、[`APP_RUN`] に役割の印で頼む(例: スケッチで選んだ領域を押し出す
    /// `(APP_RUN, {"role": jc_interchange::role::action::EXTRUDE, "finish": true, "args": {"regions": [...]}})`)。
    /// `app:` で始まるものは直すのを終えずに画面へ頼む操作([`APP_UNDO`] など)
    pub then: &'a mut Option<(String, serde_json::Value)>,
    /// ファイルの選択のダイアログ(G-code の書き出しと同じもの。工具ライブラリの読み込みなど)
    pub dialogs: &'a dyn crate::Dialogs,
    /// 3D のビューで隠している機能(ブラウザの目。ボディは受け継いだ最後の機能の ID で持つ)
    pub hidden: &'a HashSet<FeatureId>,
    /// 3D のビューの投影(表示のメニュー「表示設定」。平行・透視・自動)。
    /// 3D の図を描く編集器(3D スケッチ)も同じ描き方にする
    pub projection: crate::Projection,
}

impl EditorHost<'_> {
    /// 計算できた機能の出力。
    pub fn output(&self, id: FeatureId, slot: &str) -> Option<&Value> {
        match self.report.feature(id) {
            Some(FeatureStatus::Ok { outputs, .. }) => outputs.get(slot),
            _ => None,
        }
    }

    /// 機能が計算できなかった理由(計算できていれば `None`)。
    pub fn failure(&self, id: FeatureId) -> Option<String> {
        match self.report.feature(id) {
            Some(FeatureStatus::Error { message }) => Some(message.clone()),
            Some(FeatureStatus::Blocked { by, .. }) => Some(format!("{by} が使えません")),
            Some(FeatureStatus::Ok { .. }) => None,
            _ => Some("計算できていません".into()),
        }
    }

    /// 機能 `until` より前にできて残っているボディのうち、ブラウザの目で隠していないもの。
    /// 隠したボディは、スケッチでも描かず吸い付かない(まとめてレビュー R1)。
    pub fn bodies_before(&self, until: FeatureId) -> Vec<(FeatureId, jc_core::ShapeHandle)> {
        jc_core::visible_bodies_before(self.data, self.registry, self.report, Some(until))
            .into_iter()
            .filter(|(f, _)| !self.body_hidden(*f))
            .collect()
    }

    /// 機能 `f` が出したボディを隠しているか(後の機能が受け継いだボディは、受け継いだ最後の
    /// 機能の目で決まる)。
    pub fn body_hidden(&self, f: FeatureId) -> bool {
        !self.hidden.is_empty()
            && self.hidden.contains(&jc_core::body_heir(
                self.data,
                self.registry,
                self.report,
                f,
            ))
    }

    /// 計算できたパラメータの値(名前 → 値)。寸法の名前(`d1` など、モデルのパラメータ)も入る。
    pub fn param_values(&self) -> HashMap<&str, Quantity> {
        self.report.param_values()
    }

    /// 名前がパラメータか寸法の名前で使われているか。
    pub fn name_taken(&self, name: &str) -> bool {
        self.data.param_by_name(name).is_some()
            || self.report.model_params.iter().any(|p| p.name == name)
    }

    /// 式を今のパラメータの値で計算する。
    pub fn eval(&self, src: &str) -> Result<Quantity, String> {
        let params = self.param_values();
        jc_core::eval_str(src, &|n| params.get(n).copied()).map_err(|e| e.to_string())
    }
}

/// 編集器から画面へ頼む操作(`EditorHost::then` に入れる。直すのは終えない)。
/// 取り消し
pub const APP_UNDO: &str = "app:undo";
/// やり直し
pub const APP_REDO: &str = "app:redo";
/// パラメータの一覧を開く
pub const APP_PARAMS: &str = "app:params";
/// 全部を計算し直す
pub const APP_RECOMPUTE: &str = "app:recompute";
/// パラメータを足す(引数 `{"name": 名前, "expr": 式}`)
pub const APP_ADD_PARAM: &str = "app:add_param";
/// コマンド検索の窓を開く(スケッチの S)
pub const APP_SEARCH: &str = "app:search";
/// 直すのを終える(「スケッチを終了」と同じ。スケッチで戻す物が無い時に Esc を素早く 2 回)
pub const APP_FINISH: &str = "app:finish";
/// モジュールの操作を、直すのを終えずに実行する(引数 `{"id": 操作}` か、役割で探す
/// `{"role": 役割の印}`。ファイルを求める操作はダイアログを出す。スケッチの中からの DXF・SVG の
/// 挿入と DXF として保存)。`"finish": true` なら直すのを終えてから `"args"` の値で始める
/// (スケッチの中からの文字)
pub const APP_RUN: &str = "app:run";
/// いくつかの機能のデータを 1 回の取り消し単位で書き換える
/// (引数 `{"label": 取り消しの名前, "edits": [{"id": 機能の ID, "data": 新しいデータ}, ...],
/// "message": 書き換えた後の知らせ(省ける)}`)。
/// 工具の番号の振り直しで、工具ライブラリと加工の工具番号を一緒に変えるのに使う
pub const APP_EDITS: &str = "app:edits";

/// リボンの帯に出す道具のまとまり(中央のビューを持つ編集器が出す)。
#[derive(Clone, Debug, Default)]
pub struct ToolGroup {
    pub name: String,
    /// 帯に並べる道具
    pub tools: Vec<EditorTool>,
    /// 帯で `tools` の右に、2 段の小さなボタン(アイコンだけ)で並べる道具。全部を大きくすると
    /// 1280 幅に入らない拘束の残りに使う(Kie 2026-10-21 の画板)。▾ の一覧にも出す
    pub small: Vec<EditorTool>,
    /// まとまりの名前を押すと出す、残りの道具
    pub more: Vec<EditorTool>,
    /// ▾ の一覧の並び(道具の id)。空なら `tools`、`more` の順。前に出す道具と一覧の並びを
    /// 別に決める時に使う(前には使う物だけを出す)
    pub order: Vec<String>,
}

impl ToolGroup {
    /// まとまりの道具の全部(帯の前・小さなボタン・残りの順)。
    pub fn all(&self) -> Vec<&EditorTool> {
        self.tools
            .iter()
            .chain(&self.small)
            .chain(&self.more)
            .collect()
    }

    /// ▾ の一覧に出す順の道具(`order` に無い道具は後ろに足す)。
    pub fn menu(&self) -> Vec<&EditorTool> {
        let all = self.all();
        let mut out: Vec<&EditorTool> = self
            .order
            .iter()
            .filter_map(|id| all.iter().find(|t| t.id == *id).copied())
            .collect();
        for t in all {
            if !out.iter().any(|o| o.id == t.id) {
                out.push(t);
            }
        }
        out
    }
}

/// リボンの道具。絵は名前([`EditorTool::icon`])で指し、描くのは画面の外枠。
#[derive(Clone, Debug, Default)]
pub struct EditorTool {
    pub id: String,
    pub label: String,
    pub icon: String,
    /// 指を置いた時の説明
    pub hint: String,
    /// いま使っている道具か
    pub on: bool,
    pub enabled: bool,
    /// ▾ の一覧で入れる親の名前。`None` は親なし
    pub sub: Option<String>,
    /// ▾ の一覧で、この道具の前に区切りの線を引く
    pub sep: bool,
}

impl EditorTool {
    pub fn new(id: &str, label: &str, icon: &str, hint: &str) -> Self {
        EditorTool {
            id: id.into(),
            label: label.into(),
            icon: icon.into(),
            hint: hint.into(),
            on: false,
            enabled: true,
            sub: None,
            sep: false,
        }
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// ▾ の一覧で `parent` の子にする。
    pub fn under(mut self, parent: Option<&str>) -> Self {
        self.sub = parent.map(str::to_string);
        self
    }

    /// ▾ の一覧で、前に区切りの線を引く。
    pub fn sep(mut self, sep: bool) -> Self {
        self.sep = sep;
        self
    }
}

/// 専用の編集器。返す値は機能の新しいデータで、画面が 1 回の取り消し単位で適用する。
/// `Send` は、アプリの外の窓が本体と別の時に `Workspace` を使って描くため(`Workspace::into_shared`)。
pub trait FeatureEditor: Any + Send {
    /// この編集器が扱う種類か。
    fn handles(&self, type_id: &str) -> bool;

    /// 中央のビュー(3D のビューの代わり)を描くなら真。
    fn takes_view(&self) -> bool {
        false
    }

    /// 中央のビューを描く([`FeatureEditor::takes_view`] が真の時)。
    fn view(
        &mut self,
        _ui: &mut egui::Ui,
        _host: &mut EditorHost<'_>,
        _entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        None
    }

    /// 設定欄の下に、専用の欄を描く。
    fn panel(
        &mut self,
        _ui: &mut egui::Ui,
        _host: &mut EditorHost<'_>,
        _entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        None
    }

    /// 扱う種類でない機能の設定欄の下に、補いの欄を描く(工具ライブラリの編集器が、加工の
    /// 工具の欄の下に PC のライブラリの工具を出すなど)。変えたデータを返すのは [`FeatureEditor::panel`]
    /// と同じ。何も描かなければ何も出ない。
    fn assist(
        &mut self,
        _ui: &mut egui::Ui,
        _host: &mut EditorHost<'_>,
        _entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        None
    }

    /// 中央のビューを持つ編集器が最後に描いた視点と、その時のビューの範囲。編集を終えた時、3D の
    /// ビューをこの向き・真ん中・大きさにする(スケッチで見ていた向きのまま戻る。10-23 u1)。
    /// `None` なら 3D のビューは入る前の視点のまま。
    fn view_camera(&self) -> Option<(crate::view3d::Camera, egui::Rect)> {
        None
    }

    /// リボンの帯に出す道具(中央のビューを持つ編集器が使う。空なら帯に説明だけを出す)。
    fn tools(&self, _host: &EditorHost<'_>, _entry: &FeatureEntry) -> Vec<ToolGroup> {
        Vec::new()
    }

    /// リボンの道具が押された時。データを変えれば新しいデータを返す(拘束を付けるなど)。
    fn use_tool(
        &mut self,
        _id: &str,
        _host: &mut EditorHost<'_>,
        _entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        None
    }

    /// 編集パネルの見出しとアイコンの名前(`None` なら「名前 を編集」)。
    fn panel_title(&self) -> Option<(&'static str, &'static str)> {
        None
    }

    /// 変更の取り消しの名前。
    fn edit_label(&self) -> &'static str {
        "機能を変更"
    }

    /// 下の帯に出す、今できるマウスとキーの操作(キー, 説明)。空なら 3D のビューの操作を出す。
    /// 中央のビューを持つ編集器(スケッチ)が、道具に合わせて出す。
    fn status_hints(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// 選ぶ機能が変わった時(編集の途中の状態を捨てる)。
    fn reset(&mut self) {}

    /// データフォルダが決まった時(PC 全体の設定を読む)。
    fn set_data_dir(&mut self, _dir: Option<&Path>) {}

    /// 同じ種類の新しい編集器(文書のタブごとに 1 つずつ持ち、編集の途中の状態を混ぜない。候補 20)。
    /// データフォルダは呼ぶ側が [`FeatureEditor::set_data_dir`] で渡し直す。
    fn fresh(&self) -> Box<dyn FeatureEditor>;

    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
