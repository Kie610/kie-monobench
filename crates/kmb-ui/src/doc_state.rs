//! 文書ごとの画面の状態。開いている文書のタブ 1 つ分の状態を 1 つの struct にまとめ、タブの切り替えは
//! 丸ごと入れ替え、新しい文書は丸ごと作り直す。欄を 1 つずつ入れ替えると、入れ替え忘れた欄(解析の
//! 仕事の止め口など)がタブを替えても前の文書のまま残り、カメラや部品の番号が次の文書へ漏れる。
//!
//! ここに置くのは、文書を替えたら一緒に替わるべき物だけ。PC 全体・窓全体の物(設定・画面の配置・
//! 最近使ったファイル・知らせ)は作業場([`crate::Workspace`])に置く。

use crate::*;
use kmb_core::{Document, FeatureId, ParamId};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Instant;

/// パラメータと寸法の名前ごとの、使っている物の名前(パラメータの表の控え)。
pub(crate) type ParamUsers = std::sync::Arc<HashMap<String, Vec<String>>>;

/// 開いている文書 1 つ分の画面の状態。
pub(crate) struct DocState {
    /// 開いている文書・保存先・計算の結果・裏の再計算。CLI・MCP と同じ kmb-session の部品で、画面の
    /// 文書の変更もここを通す
    pub(crate) work: kmb_session::Headless,
    pub(crate) selected: Option<FeatureId>,
    /// 編集中のパラメータの式・名前(確定前)
    pub(crate) expr_edits: HashMap<ParamId, FieldEdit>,
    pub(crate) name_edits: HashMap<ParamId, FieldEdit>,
    pub(crate) new_param: (String, String),
    /// パラメータの表の追加の行の単位
    pub(crate) new_param_unit: kmb_core::ParamUnit,
    pub(crate) modified: bool,
    /// 最後に開いた・保存した時の中身(取り消しで戻した時に「変更あり」を外すのに使う)
    pub(crate) saved: kmb_core::DocumentData,
    /// 専用の編集器(スケッチ、工具ライブラリなど。実行ファイルが組み込む)
    pub(crate) editors: Vec<Box<dyn FeatureEditor>>,
    /// 前のコマに中央のビューを描いた編集器の扱う種類(終えた次のコマで、その視点を 3D のビューへ
    /// 移す。10-23 u1)
    pub(crate) view_editor_was: Option<String>,
    /// 裏の再計算が(やり直しも続けて)走り始めた時刻。下の帯に経った秒数を出す
    pub(crate) busy_since: Option<Instant>,
    /// 別のスレッドで走っている時間のかかる操作(シミュレーションなど。終わると結果の文を送る)
    pub(crate) task: Option<Receiver<kmb_core::JobReport>>,
    /// 走っている時間のかかる操作の進み具合と中止の受け渡し
    pub(crate) task_ctl: Option<std::sync::Arc<kmb_core::JobControl>>,
    /// 時間のかかる操作が走り始めた時刻(下の帯に経った秒数を出す)
    pub(crate) task_since: Option<Instant>,
    /// 時間のかかる操作を始めた時の文書の保存先と、保存の後に変えていたか(描き終えた静止画を
    /// その文書のギャラリーに残す。途中でタブを替えても始めた文書に残す)
    pub(crate) task_doc: Option<(PathBuf, bool)>,
    pub(crate) scene: view3d::Scene,
    pub(crate) camera: view3d::Camera,
    pub(crate) picked: Option<view3d::Pick>,
    /// 機能のデータを JSON で編集している時の文字列
    pub(crate) data_edit: Option<(FeatureId, String)>,
    /// 読み込んだ STEP の機能を立てるかを聞いている(U12)
    pub(crate) step_up_ask: Option<FeatureId>,
    /// 開いている設計の属性の窓の、直している途中の値(U13)
    pub(crate) props_draft: Option<kmb_core::Properties>,
    /// 名前を変えている名前付きの視点(番号、打っている名前。U20)
    pub(crate) renaming_view: Option<(u64, String)>,
    /// 計算に失敗した間、前に計算できた形を出し続けている機能(M27)
    pub(crate) held: Vec<FeatureId>,
    /// 次の計算の結果は、失敗していても形を出し直す(文書を入れ替えた時)
    pub(crate) scene_fresh: bool,
    pub(crate) autosave: Option<kmb_session::Autosave>,
    /// シミュレーションを再生している間の、前のコマの時刻(秒)。止めていれば `None`
    pub(crate) playing: Option<f64>,
    /// 解析の結果のコマを再生している間の、始めた時刻(秒)とその時のコマ。止めていれば `None`
    pub(crate) frames_playing: Option<(f64, usize)>,
    /// 3D のビューで部品を引いてジョイントを動かしている途中(`joint_drag_ui`)
    pub(crate) joint_drag: Option<joint_drag_ui::JointDrag>,
    /// 回転・スライダを引いている間と、離して形を作り直し終わるまで、3D のビューのボディを動かして
    /// 見せる動き(引いている間は形を作り直さない。10-23 a1。`joint_drag_ui`)
    pub(crate) drag_pose:
        Option<std::collections::BTreeMap<kmb_core::FeatureId, kmb_core::kernel::Matrix4>>,
    /// 引いて動かした後、残すか戻すかを決めていない変更(`joint_drag_ui`)
    pub(crate) moved: Option<joint_drag_ui::Moved>,
    /// 残すか戻すかを聞いている間、止めているコマンド
    pub(crate) moved_ask: Option<joint_drag_ui::Held>,
    /// 再生の間に隠す線の描き方の役割
    pub(crate) hidden_lines: Vec<kmb_core::LineInk>,
    /// 読むための文の窓(題と行。G-code の確認など)
    pub(crate) viewer: Option<(String, Vec<String>)>,
    /// 表の窓(部品表など。列を選べる)
    pub(crate) table_view: Option<table_ui::TableView>,
    /// 専用の編集器が頼んだ、直すのを終えてから始める操作と引数(スケッチで領域を選んで押し出す)
    pub(crate) then_action: Option<(String, serde_json::Value)>,
    /// 一緒に選んでいる機能(Ctrl・Shift で足したもの。`selected` は含めない)
    pub(crate) also: Vec<FeatureId>,
    /// Shift で範囲を選ぶ時の起点(最後に普通に押した物か Ctrl で足した物。範囲を選び直しても動かない)
    pub(crate) range_anchor: Option<FeatureId>,
    /// ブラウザの原点の下で選んだ平面(`browser_ui::ORIGIN_PLANES` の番号。正面から見る向き)
    pub(crate) origin_pick: Option<usize>,
    /// 一緒に選んでいる面(Shift で足したもの。`picked` は含めない)
    pub(crate) also_picked: Vec<view3d::Pick>,
    /// 3D のビューで選んでいる辺
    pub(crate) picked_edges: Vec<kmb_core::EdgePick>,
    /// 3D のビューで選んでいる頂点
    pub(crate) picked_vertices: Vec<kmb_core::VertexPick>,
    /// 表示しているボディの角(辺から作る。`edge_cache` と一緒に作り直す)
    pub(crate) vertex_cache: Vec<(FeatureId, Vec<kmb_core::VertexInfo>)>,
    /// 前のフレームでマウスの下にあった物の種類(頂点・辺なら面を明るくしない)
    pub(crate) hover_kind: Option<kmb_core::RefKind>,
    /// 前のフレームでマウスの下にあった面・辺・頂点の機能(ブラウザでそのボディの行に印を付ける)
    pub(crate) hover_feature: Option<FeatureId>,
    /// 編集パネルの選ぶ欄の状態(どの欄に 3D のビューのクリックを入れるか)
    pub(crate) pick_field: select_ui::PickFieldState,
    /// 3D のビューに、どの機能の手前の形を出しているか(選ぶ欄に入れている間は、その機能の手前)
    pub(crate) scene_before: Option<FeatureId>,
    /// 表示しているボディの辺(辺を選ぶのに使う。表示のボディが変わったら作り直す)
    pub(crate) edge_cache: (Vec<(FeatureId, ShapeHandle)>, BodyEdges),
    /// 辺の機能の値の箱の位置の控え([`value_box_ui`])
    pub(crate) value_boxes: value_box_ui::BoxAnchors,
    /// 今の選択がブラウザのボディの行から(機能ではなくボディを選んだ。パターンなどの欄へボディとして渡す)
    pub(crate) browser_bodies: bool,
    /// パラメータの窓で開いている表(0 は「全部」。U50)
    pub(crate) param_tab: u64,
    /// 編集パネルで直し始めた時の取り消しの履歴の長さ(キャンセルで戻る所)
    pub(crate) edit_start: Option<(FeatureId, usize)>,
    /// 畳んでいるタイムラインのグループ
    pub(crate) collapsed: HashSet<u64>,
    /// グループにできなかった時の問い(選んだ機能、理由)
    pub(crate) group_ask: Option<(Vec<FeatureId>, edit::GroupError)>,
    /// 消す前の確かめ(消す機能と、それを使っている機能)
    pub(crate) remove_ask: Option<(Vec<FeatureId>, Vec<FeatureId>)>,
    /// 名前を打っているグループ
    pub(crate) renaming_group: Option<(u64, String)>,
    /// 名前を打っている機能かボディ(右クリックの「名前変更」)
    pub(crate) renaming: Option<(menus_ui::Renaming, String)>,
    /// 写した加工(Ctrl+C・右クリックの「コピー」)
    pub(crate) op_clip: Option<reuse_ui::OpClip>,
    /// 開いている途中の文書(終わるまで「キャンセル」で前の文書に戻れる。U8)
    pub(crate) opening: Option<files_ui::Opening>,
    /// 写した設計の機能(U42)
    pub(crate) feature_clip: Option<feature_clip_ui::FeatureClip>,
    /// 名前を打たせてから実行する操作(テンプレートとして保存など)
    pub(crate) name_ask: Option<reuse_ui::NameAsk>,
    /// 進めている「クリックで作る」操作
    pub(crate) gesture: Option<gesture::Run>,
    /// 編集パネルで直している機能(選んだだけでは直さない。ダブルクリックか「編集」で開く)
    pub(crate) editing: Option<FeatureId>,
    /// 作っている途中の機能と、作る前の取り消しの履歴の長さ。OK で決め、キャンセルで作る前へ戻す
    pub(crate) creating: Option<(FeatureId, usize)>,
    /// 作り始めた機能(編集パネルの最初の式の欄にキーボードを移す。移したら空にする)
    pub(crate) focus_new: std::cell::Cell<Option<FeatureId>>,
    /// 複数選んだ機能をまとめて直しているか
    pub(crate) multi_edit: bool,
    /// 右クリックした面の点(「ここを回す中心にする」)
    pub(crate) context_point: Option<[f32; 3]>,
    /// 右クリックした所の物(頂点・辺・面。「タイムライン内を検索」など)
    pub(crate) context_hit: Option<select_ui::Hit>,
    /// マウスの下の面が平らか(面を選べる手順でのホバー。面が変わった時だけ問い合わせる)
    pub(crate) planar_hover: Option<(view3d::Pick, bool)>,
    /// 文書の目(区画 `cad.visibility` を読んだ写し。文書が変わるたびに読み直す。替えるのは操作
    /// 「表示/非表示」)
    pub(crate) visibility: kmb_interchange::visibility::Visibility,
    /// `hidden_now` を決め直した時に直していた機能(直す機能が替われば、直す間に一時的に見せる物を
    /// 決め直す)
    pub(crate) shown_for_edit: Option<FeatureId>,
    /// 3D のビューで実際に隠している機能(文書の目・一時の見せ・選択表示を合わせた物)
    pub(crate) hidden_now: HashSet<FeatureId>,
    /// 測る道具(開いている時だけ)
    pub(crate) measure: Option<measure_ui::Measure>,
    /// 断面で切って見る平面(開いている時だけ。M48)
    pub(crate) section: Option<section_ui::SectionView>,
    /// キャンバス内レンダリング(レンダリングの作業空間で押している時だけ)
    pub(crate) canvas_render: Option<render_ui::CanvasRender>,
    /// アニメーションのストーリーボードの時刻と再生(作業空間「アニメーション」)
    pub(crate) anim: animation_ui::AnimState,
    /// 注意・エラーを確かめる窓で見ている機能
    pub(crate) review: Option<review_ui::Review>,
    /// 選択表示で、これだけを 3D のビューに出しているボディ(最初に作った機能。空なら選択表示して
    /// いない)。目の状態は変えずに上から隠すので、やめると前の見え方に戻る
    pub(crate) isolated: Vec<FeatureId>,
    /// 「タイムライン内を検索」「ブラウザ内を検索」で探した物と探した時刻(しばらく光らせる)
    pub(crate) found: Option<(Found, std::time::Instant)>,
    /// 探した物を、次に描く時に見える所まで送る
    pub(crate) found_scroll: Option<Found>,
    /// 保存していない文書のタブの名前(見本の名前・開いた版。保存したらファイルの名前)
    pub(crate) doc_label: Option<String>,
    /// 設計の属性の窓で書いているタグ(「,」で区切る)
    pub(crate) props_tags: String,
    /// 版の履歴の窓(候補 35)
    pub(crate) versions: versions_ui::VersionsPage,
    /// レンダリング ギャラリー
    pub(crate) gallery: gallery_ui::GalleryPage,
    /// モーション スタディの窓(#18)
    pub(crate) motion: motion_ui::MotionPage,
    /// 外観の窓(#11)
    pub(crate) looks: looks_ui::LooksPage,
    /// リンクで組み込んだ部品の様子(元のファイルが変わったか)と、最後に調べた時。数秒ごとに調べ直す
    pub(crate) links: Vec<kmb_core::LinkInfo>,
    pub(crate) links_at: Option<std::time::Instant>,
    /// パラメータの表の「使っている所」(名前ごと)と、作った時の計算の番号・取り消しの深さ。
    /// 毎こま行ごとに探すと大きな設計で表が重くなるので、文書が変わった時だけ作り直す
    pub(crate) param_users: Option<((u64, usize), ParamUsers)>,
    /// 足せる機能の一覧([`Workspace::addable`])の控え。鍵は(種類の分け方、計算の番号、取り消しの
    /// 深さ)。ツールバーを描くたびに全部の記述子で足せるかを試すと 1 コマの半分を使っていたので
    /// (10-23 v8)、文書が変わった時だけ作り直す。ツールバーは `&self` で組むので鍵をかけて持つ
    pub(crate) addable: std::sync::Mutex<Option<(AddableKey, Addable)>>,
}

/// [`DocState::addable`] の鍵。
pub(crate) type AddableKey = (String, u64, usize);
/// 足せる機能(種類の ID、名前、足せない理由)。
pub(crate) type Addable = std::sync::Arc<Vec<(String, String, Option<String>)>>;

impl DocState {
    /// 開いたばかりの文書の状態(選んでいる物・途中の操作・隠した物は無い)。`saved` は最後に開いた・
    /// 保存した時の中身。
    pub(crate) fn new(
        doc: Document,
        path: Option<PathBuf>,
        saved: kmb_core::DocumentData,
        modules: kmb_core::Modules,
    ) -> Self {
        DocState {
            work: kmb_session::Headless::in_background(modules, doc, path),
            selected: None,
            expr_edits: HashMap::new(),
            name_edits: HashMap::new(),
            new_param: (String::new(), String::new()),
            new_param_unit: kmb_core::ParamUnit::Length,
            modified: false,
            saved,
            editors: Vec::new(),
            view_editor_was: None,
            busy_since: None,
            task: None,
            task_ctl: None,
            task_since: None,
            task_doc: None,
            scene: view3d::Scene::default(),
            camera: view3d::Camera::default(),
            param_users: None,
            addable: Default::default(),
            picked: None,
            data_edit: None,
            step_up_ask: None,
            props_draft: None,
            renaming_view: None,
            held: Vec::new(),
            scene_fresh: true,
            autosave: None,
            playing: None,
            frames_playing: None,
            joint_drag: None,
            drag_pose: None,
            moved: None,
            moved_ask: None,
            hidden_lines: Vec::new(),
            viewer: None,
            table_view: None,
            then_action: None,
            also: Vec::new(),
            range_anchor: None,
            origin_pick: None,
            also_picked: Vec::new(),
            picked_edges: Vec::new(),
            picked_vertices: Vec::new(),
            vertex_cache: Vec::new(),
            hover_kind: None,
            hover_feature: None,
            pick_field: Default::default(),
            scene_before: None,
            edge_cache: (Vec::new(), Vec::new()),
            value_boxes: None,
            browser_bodies: false,
            param_tab: 0,
            edit_start: None,
            collapsed: HashSet::new(),
            group_ask: None,
            remove_ask: None,
            renaming_group: None,
            renaming: None,
            op_clip: None,
            feature_clip: None,
            opening: None,
            name_ask: None,
            gesture: None,
            editing: None,
            creating: None,
            focus_new: Default::default(),
            multi_edit: false,
            context_point: None,
            context_hit: None,
            planar_hover: None,
            visibility: Default::default(),
            shown_for_edit: None,
            hidden_now: HashSet::new(),
            measure: None,
            section: None,
            canvas_render: None,
            anim: Default::default(),
            review: None,
            isolated: Vec::new(),
            found: None,
            found_scroll: None,
            doc_label: None,
            props_tags: String::new(),
            versions: Default::default(),
            gallery: Default::default(),
            motion: Default::default(),
            looks: Default::default(),
            links: Vec::new(),
            links_at: None,
        }
    }

    /// 走っている計算と時間のかかる操作を止める(タブを閉じた・文書を作り直した時。結果を受け取る
    /// 所がもう無い)。
    pub(crate) fn stop(&mut self) {
        self.work.supersede_recompute();
        if let Some(c) = self.task_ctl.take() {
            c.cancel();
        }
    }

    /// この文書(手前のタブか、しまってあるタブ)の写しを、時なら書く。
    pub(crate) fn autosave_own(
        &mut self,
        now: std::time::SystemTime,
        interval: std::time::Duration,
    ) -> Result<(), String> {
        let modified = self.modified;
        let Some(a) = &mut self.autosave else {
            return Ok(());
        };
        a.interval = interval;
        if a.due(now, modified) {
            a.write(self.work.doc(), self.work.path(), now)?;
        }
        Ok(())
    }
}
