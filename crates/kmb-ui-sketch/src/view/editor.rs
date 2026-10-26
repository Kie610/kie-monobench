//! 編集器の状態(選び、描きかけの点、道具の途中の様子)。

use super::*;

/// クリックで決めた位置。既存の点に吸着した時は、その点を使う。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pick {
    pub point: Option<EntityId>,
    pub pos: [f64; 2],
}

/// スケッチ編集の状態(文書には保存しない)。
pub(crate) struct Editor {
    /// 寸法の見せ方(スケッチの設定。画面の更新ごとに組み立ての共有の値から読む)
    pub(crate) fmt: kmb_sketch::DimFormat,
    pub tool: Tool,
    pub selection: BTreeSet<EntityId>,
    pub(crate) pending: Option<Pick>,
    /// 円弧の始点(中心の次に決める)
    pub(crate) pending2: Option<Pick>,
    /// 3 つ目の位置(3 点の円、円弧に沿う長穴、回す時の基準)
    pub(crate) pending3: Option<Pick>,
    /// 道具で選んだ線(3 本の線に接する円)
    pub(crate) picked: Vec<EntityId>,
    /// 選ぶ道具で掴んでいるもの(drag.rs)
    pub(crate) drag: Option<crate::drag::Grab>,
    /// 選ぶ道具で、何もない所から囲み始めた位置(画面)
    pub(crate) window: Option<Pos2>,
    /// 選んだ閉じた領域の名前(クリックか、右から左へ囲んで選ぶ。押し出しに渡す)
    pub(crate) regions: Vec<String>,
    /// 線の道具で、つなげて描いている線の最初の点(ここへ戻ると輪が閉じて道具を終える)
    pub(crate) chain_start: Option<EntityId>,
    /// 最後に使い終えた道具(右クリックの「繰り返し」で戻る)
    pub last_tool: Option<Tool>,
    /// 寸法を追加する時の式
    pub dim_text: String,
    /// パネルの「寸法の値」に打ったまま図を左クリックした(その位置に、打った値で寸法を付ける)
    pub(crate) dim_typed_click: bool,
    /// パネルの「寸法の値」に人が打った(測った値で埋めただけではない)。外のクリックで決めるのはこの時だけ
    pub(crate) dim_dirty: bool,
    /// 描いている途中に打ち込んだ寸法(線の長さ、円の直径)。Enter で決める
    pub typed: String,
    /// 寸法の道具で、いま置こうとしている寸法(選びが変わったら値の欄を測った値にする)
    pub dim_kind: Option<kmb_sketch::ConstraintKind>,
    /// `dim_kind` を出した時の選び(同じ選びで種類だけ変わった時は、人が打った値を残す)
    pub(crate) dim_sel: Vec<EntityId>,
    /// 寸法の操作の誤り(参照寸法を駆動に戻せない時など)
    pub dim_error: Option<String>,
    /// 値の札をダブルクリックして直している寸法と、打っている値
    pub edit_dim: Option<(kmb_sketch::ConstraintId, String)>,
    /// 値を直す欄を出したばかり(次のフレームで入力を移す)
    pub(crate) edit_dim_fresh: bool,
    /// 値を直す欄を出し直す時のカーソルの位置(字の数)。`None` なら全部を選ぶ
    pub(crate) edit_dim_caret: Option<usize>,
    /// E を押した: スケッチを終えて、選んだ領域を押し出す(lib.rs が画面へ頼む)
    pub(crate) wants_extrude: bool,
    /// 文字の道具で点を置いた(置いた点を基準に文字を作るよう画面に頼む。K32)
    pub(crate) wants_text: bool,
    /// 長押しで出した、重なったものの一覧(出す位置と候補。手前の点から)
    pub(crate) others: Option<(Pos2, Vec<Other>)>,
    /// 一覧で指を置いている候補(図で光らせる)
    pub(crate) others_hover: Option<Other>,
    /// 寸法の道具で、カーソルについてくる寸法と、今の形で測った値(作り置き)
    pub(crate) ghost: Option<(ConstraintKind, String)>,
    /// `ghost` を出した時の選び(カーソルが図の外へ出ても、選びが同じなら寸法の種類を覚えておき、
    /// パネルの値の欄と Enter で付ける寸法をそれに合わせる)
    pub(crate) ghost_sel: Vec<EntityId>,
    /// パターンの欄を引いている間の見本(引いている途中の定義、それで作り直したスケッチと
    /// 解いた形)。ビューは文書の代わりにこれを描く。引き終えると消して文書へ入れる
    pub(crate) pattern_ghost: Option<(kmb_sketch::PatternDef, Sketch, Solution)>,
    /// 寸法の道具で、置く位置をクリックした寸法(その場に値を打つ欄を出している)
    pub(crate) placing: Option<Placing>,
    /// 道具の知らせ(拘束の道具で、既に決まっているので付けなかった時など)。案内の下に出す
    pub(crate) notice: Option<String>,
    /// パレットの「正面から見る」を押した(ビューが 3D の視点を正面へ回す。N1c)
    pub(crate) look_front: bool,
    /// ビューが 3D の視点で見ている(パレットの「正面から見る」を押せる)
    pub(crate) in_space: bool,
    /// 描く道具の吸い付きと推測で覚えておくもの(infer.rs)
    pub(crate) infer: crate::infer::State,
    /// 描いている途中の寸法の欄(長さ・直径など。Tab で移って固定する)
    pub(crate) hud: crate::hud::Hud,
    /// 円弧の終点を決める間、始点からカーソルが回った角度(前の角度、回った量。正なら反時計回り)
    pub(crate) arc_turn: Option<(f64, f64)>,
    /// 図の印をクリックして選んだ拘束(Delete で外す)
    pub(crate) picked_constraint: Option<kmb_sketch::ConstraintId>,
    /// 図に拘束の印を出すか(パネルで切り替える)
    pub(crate) show_marks: bool,
    /// 直しているスケッチ(貼り付けで、写した元と同じスケッチか見るのに使う)
    pub(crate) feature: Option<kmb_core::FeatureId>,
    /// 選ぶ道具で右クリックした位置(メニューの「貼り付け」をそこへ置く)
    pub(crate) menu_at: Option<[f64; 2]>,
    /// 右クリックのメニューを開いた時の様子(開いている間は同じ項目を出す)
    pub(crate) menu: Option<crate::menu::Context>,
    /// 最近使った道具(新しい順に 3 つまで。右クリックのメニューに出す)
    pub(crate) recent: Vec<Tool>,
    /// 次に描く線を補助線にする(描く前に線種を選ぶ)
    pub(crate) draw_construction: bool,
    /// 画面へ頼む操作(取り消しなど。lib.rs が `EditorHost::then` へ移す)
    pub(crate) app_ask: Option<&'static str>,
    /// 戻す物が無い時に Esc を押した時刻(続けてもう 1 回でスケッチを終える)
    pub(crate) empty_esc: Option<f64>,
    /// 右クリックで頼んだモジュールの操作の役割(DXF として保存。画面が役割で操作を探し、
    /// ダイアログを出して実行する)
    pub(crate) run_action: Option<kmb_core::ActionRole>,
    /// オフセットの道具の状態
    pub(crate) offset: crate::offset_tool::State,
    /// 右クリックの「OK」: 次のコマで打った値の形を作ってから道具を終える
    pub(crate) ok: bool,
    /// 測る道具の結果(案内の下に出す)
    pub(crate) measured: Option<String>,
    /// 欄の脇の ✓ を押した: 次のコマで打った値の形を作る(道具は続ける)
    pub(crate) confirm: bool,
    /// 前のコマのカーソルの位置(✓ や右クリックの「OK」で形を作る時に使う)
    pub(crate) last_cursor: Option<[f64; 2]>,
    /// 図に出す物と刻みへの吸い付き(display.rs)
    pub(crate) display: crate::display::Display,
    /// 選べる物の絞り込み
    pub(crate) filter: crate::display::Filter,
    /// 何もない所から引いた時の選び方(キー 1・2・3)
    pub(crate) select_mode: crate::display::SelectMode,
    /// 自由形状・なぞるで引いた道(画面の座標)
    pub(crate) lasso: Vec<Pos2>,
    /// 寸法の道具で、右クリックで選び直した寸法の種類と、その時の選び(選びが変わると戻す)
    pub(crate) dim_choice: crate::dim_choice::DimChoice,
    pub(crate) dim_choice_sel: Vec<EntityId>,
    /// 選ぶ道具で、値の札を掴んで動かしている寸法(離すと置く位置を変える。網羅版 F5)
    pub(crate) dim_drag: Option<kmb_sketch::ConstraintId>,
    /// 掴んだ所から札の真ん中までのずれ(画面。引き始めで札が跳ねないように)
    pub(crate) dim_grab: Vec2,
    /// 先に選ぶ道具(並べる・動かす・ミラー)を何も選ばずに始めた: クリックで要素を選び、
    /// Enter で選び終える
    pub(crate) choosing: bool,
    /// 多角形の辺の数(前に描いた数を覚える。中心を決める前に数字キーでも変えられる。網羅版 C8)
    pub(crate) polygon_n: usize,
    /// 中心を決める前に辺の数を打っている途中
    pub(crate) polygon_typed: bool,
    /// 線分の道具でつなげて描く時の、前の線分の向き(始点から前の線分の他端へ。rad)。
    /// 2 本目からの角度はこの向きから測る
    pub(crate) line_base: Option<f64>,
    /// 線分の道具で、前の線分の端から押したまま引いている(離すと接線円弧。網羅版 C2)
    pub(crate) arc_drag: bool,
    /// トリムの道具で、押したまま引いている道(スケッチの座標)。離すと、なぞった区間をまとめて消す
    pub(crate) trim_path: Vec<[f64; 2]>,
    /// 投影の道具で写す物(面・辺・点か、ボディのシルエットか、ボディの辺と角すべてか)
    pub(crate) project_pick: ProjectPick,
    /// 投影の道具で、元の形とのつながり(投影リンク)を保つ。外すと写した線は固定した普通の線
    pub(crate) project_link: bool,
    /// 交差の道具で、押したボディの断面をまとめて写す(外すと押した線だけ)
    pub(crate) intersect_body: bool,
    /// 交差の道具で、元の形とのつながりを保つ(ボディの形が変わると断面の線が付いて行く)
    pub(crate) intersect_link: bool,
    /// スプラインの道具で置いた点
    pub(crate) spline_pts: Vec<Pick>,
    /// ブレンド曲線を接するだけ(G1)にする(既定は曲率もそろう G2)
    pub(crate) blend_g1: bool,
    /// 制御点のスプラインの次数(3 か 5。既定は 5: 曲率まで滑らか。Kie の決定 2026-10-19)
    pub(crate) spline_degree: u8,
    /// 次に描く線分を中心線にする(パレットの線種。補助線で描くとは同時に立てない)
    pub(crate) draw_centerline: bool,
    /// 「端をつなぐ」で、開いた端どうしをつなぐ距離(mm。K25)
    pub(crate) tie_gap: f64,
    /// 自動拘束の予告(K18。候補を図に別の色で出し、パネルで種類ごとに外して OK で付ける)
    pub(crate) auto: Option<crate::auto_ui::State>,
    /// 動かす・回す・拡大縮小で、元を残して写しを作る(網羅版 D6)
    pub(crate) copy_mode: bool,
    /// 動かす時に、選んでいない要素とのつながり(共有する点・拘束)を切る
    pub(crate) detach: bool,
}

/// その場に値を打つ欄を出している寸法。
#[derive(Clone, Debug)]
pub(crate) struct Placing {
    pub kind: ConstraintKind,
    /// 置いた位置(スケッチの平面の座標)
    pub at: [f64; 2],
    /// 打っている値(初めは今の形で測った値)
    pub text: String,
    /// 測った値(単位を補うのに使う)
    pub measured: String,
    /// 欄を出したばかり(次のフレームで入力を移して全部選ぶ)
    pub fresh: bool,
    /// 被駆動(参照寸法)として置く(右クリックで選んだ)
    pub driven: bool,
    /// 欄を出さずに、打った値ですぐ付ける(パネルの「寸法の値」に打ってから図を押した)
    pub ready: bool,
}

/// 重なったものの一覧の候補。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Other {
    Entity(EntityId),
    /// 閉じた領域(名前)
    Region(String),
}

impl Default for Editor {
    fn default() -> Self {
        Editor {
            fmt: kmb_sketch::DimFormat::default(),
            tool: Tool::Select,
            selection: BTreeSet::new(),
            pending: None,
            pending2: None,
            pending3: None,
            picked: Vec::new(),
            drag: None,
            window: None,
            regions: Vec::new(),
            chain_start: None,
            last_tool: None,
            dim_text: String::new(),
            dim_typed_click: false,
            dim_dirty: false,
            typed: String::new(),
            dim_kind: None,
            dim_sel: Vec::new(),
            dim_error: None,
            edit_dim: None,
            edit_dim_fresh: false,
            edit_dim_caret: None,
            wants_extrude: false,
            wants_text: false,
            others: None,
            others_hover: None,
            ghost: None,
            ghost_sel: Vec::new(),
            pattern_ghost: None,
            placing: None,
            notice: None,
            look_front: false,
            in_space: false,
            infer: crate::infer::State::default(),
            hud: crate::hud::Hud::default(),
            arc_turn: None,
            picked_constraint: None,
            show_marks: true,
            feature: None,
            menu_at: None,
            menu: None,
            recent: Vec::new(),
            draw_construction: false,
            app_ask: None,
            empty_esc: None,
            run_action: None,
            offset: crate::offset_tool::State::default(),
            ok: false,
            measured: None,
            confirm: false,
            last_cursor: None,
            display: crate::display::Display::default(),
            filter: crate::display::Filter::default(),
            select_mode: crate::display::SelectMode::default(),
            lasso: Vec::new(),
            dim_choice: Default::default(),
            dim_choice_sel: Vec::new(),
            dim_drag: None,
            dim_grab: Vec2::ZERO,
            choosing: false,
            polygon_n: 6,
            polygon_typed: false,
            line_base: None,
            arc_drag: false,
            trim_path: Vec::new(),
            spline_pts: Vec::new(),
            blend_g1: false,
            spline_degree: 5,
            draw_centerline: false,
            tie_gap: 0.1,
            auto: None,
            project_pick: ProjectPick::Parts,
            project_link: true,
            intersect_body: false,
            intersect_link: true,
            copy_mode: false,
            detach: false,
        }
    }
}

impl Editor {
    pub fn set_tool(&mut self, tool: Tool) {
        // 使っていた道具を最近の道具に覚える(右クリックのメニューに出す)
        let old = self.tool;
        if old != tool && !matches!(old, Tool::Select | Tool::Constraint(_)) {
            self.recent.retain(|t| *t != old);
            self.recent.insert(0, old);
            self.recent.truncate(3);
        }
        self.tool = tool;
        self.cancel();
        self.choosing = (crate::more_tools::needs_selection(tool) || tool == Tool::MirrorAxis)
            && self.selection.is_empty();
    }

    /// 道具を終えて選ぶ道具に戻る(選びはそのまま)。終えた道具は「繰り返し」で戻れるよう覚える。
    pub(crate) fn finish(&mut self, tool: Tool) {
        if tool != Tool::Select {
            self.last_tool = Some(tool);
        }
        self.set_tool(Tool::Select);
    }

    /// 描いている途中の位置と打ち込みを捨てる。
    pub(crate) fn cancel(&mut self) {
        // 寸法の道具が測った値で埋めた「寸法の値」の欄は空に戻す(残ると次の角の丸めや
        // オフセットの値に使われる)。人が打った値は残す
        if self.dim_kind.take().is_some() {
            self.dim_text.clear();
        }
        self.notice = None;
        self.infer.clear();
        self.placing = None;
        self.dim_typed_click = false;
        self.dim_dirty = false;
        self.trim_path.clear();
        self.spline_pts.clear();
        self.arc_drag = false;
        self.ghost = None;
        self.chain_start = None;
        self.pending = None;
        self.pending2 = None;
        self.pending3 = None;
        self.picked.clear();
        self.typed.clear();
        self.hud.clear();
        self.arc_turn = None;
        self.offset.clear();
        self.measured = None;
    }

    /// 文書へ移す時に詰めた要素の番号へ、選びと描いている途中の位置を付け替える。
    pub(crate) fn renumber(&mut self, moved: &std::collections::BTreeMap<EntityId, EntityId>) {
        if moved.is_empty() {
            return;
        }
        let at = |id: EntityId| moved.get(&id).copied().unwrap_or(id);
        self.selection = self.selection.iter().map(|id| at(*id)).collect();
        for p in [&mut self.pending, &mut self.pending2, &mut self.pending3]
            .into_iter()
            .flatten()
        {
            p.point = p.point.map(at);
        }
        for id in &mut self.picked {
            *id = at(*id);
        }
        self.chain_start = self.chain_start.map(at);
    }

    /// 道具の何段目か(案内に使う)。
    pub fn step(&self) -> usize {
        self.pending.is_some() as usize
            + self.pending2.is_some() as usize
            + self.pending3.is_some() as usize
            + self.picked.len()
            + self.spline_pts.len()
    }
}
