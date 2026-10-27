//! 設定の窓(U39)。上に検索、その下に「一般・設計・加工」のタブ、左に頁の木、右に頁の中身。
//!
//! 画面の設定(`layout.json`)は手で書いた節、モジュールの設定(`kmb_core::Settings`)は 1 つずつ頁にする。
//! id の頭が加工の作業空間の名前(操作の `group` と同じ)なら加工のタブ、ほかは設計のタブ。画面は他の層の
//! 名前を持たない(kmb-bundle の層の試験)ので、モジュールの id を書いて振り分けない。コアの型は変えない。
//! 検索は、頁の名前・節の名前・欄の名前(キーの割り当てではキーの働きと名前)に当て、当たった節を
//! 頁をまたいで並べる。開いた頁は `layout.json` に覚える。
//!
//! キーの割り当て(U36)もここに置く: 行の「変える」を押して次に押したキーを割り当てる。同じキーを
//! 2 つの行に付けると両方の行に印を出し、付け替えられないキー(Esc・Enter・数字など)は理由を出して断る。

use crate::Workspace;
use crate::keys::{self, KEYS, Scope};

/// 親のタブ。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tab {
    General,
    Design,
    Machining,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::General, Tab::Design, Tab::Machining];

    /// 名前の文言の ID(`kmb-i18n` の `settings.ftl`)。
    fn text_id(self) -> &'static str {
        match self {
            Tab::General => "settings-tab-general",
            Tab::Design => "settings-tab-design",
            Tab::Machining => "settings-tab-machining",
        }
    }
}

/// 頁の中の節。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Sec {
    Appearance,
    Mouse,
    Keys,
    Keycast,
    Autosave,
    /// 読み込み(STEP を立てるか聞く)
    Import,
    /// 描画(3D のビューの描き方・細かさ。U26)
    Render,
    Units,
    /// 新しい文書の既定の値(候補 19)
    NewDoc,
    Arrangement,
    View3d,
    /// AI との接続(候補 21)
    Ai,
    /// モジュールの設定(`extensions.settings()` の id)
    Module(String),
}

/// 設定の頁。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Page {
    pub tab: Tab,
    /// `layout.json` に覚える名前
    pub id: String,
    pub title: String,
    pub secs: Vec<Sec>,
}

/// 手で書いた節の名前と、検索に当てる言葉。
fn sec_words(s: &Sec) -> (&'static str, &'static str) {
    match s {
        Sec::Appearance => (
            "画面",
            "言語 表示の言語 English 英語 中文 中国語 한국어 韓国語 language \
             テーマ 明るさ 色 色覚 ハイコントラスト UI の大きさ 字の大きさ アイコンの大きさ \
             部品の詰め具合 角の丸み 字の書体 フォント 動き アニメーション 見本 \
             ツールバー ボタンの名前 アイコンだけ",
        ),
        Sec::Mouse => (
            "マウス",
            "ホイール 押し ドラッグ 回す 動かす 拡大 左 右クリック オービット 3D マウス 速さ 反転 \
             主な軸だけ 遊び ボタン",
        ),
        Sec::Keys => (
            "キーの割り当て",
            "キー ショートカット 割り当て 付け替え 既定に戻す",
        ),
        Sec::Keycast => (
            "操作の表示",
            "操作の表示 押したキー キー マウス 3D マウス 録画 動画 解説 画面に出す",
        ),
        Sec::Autosave => ("自動保存", "保存 間隔 分 写し 落ちた 戻す"),
        Sec::Import => ("読み込み", "STEP 読み込む 立てる Y が上 Z が上 向き"),
        Sec::Render => (
            "描画",
            "描画 グラフィックス 軽さ優先 標準 見た目優先 GPU CPU 細かさ アンチエイリアス \
             網 隠れた辺 描画装置 垂直同期 速さ API DirectX DXC FXC Vulkan",
        ),
        Sec::Units => (
            "単位",
            "長さ 角度 mm cm m in インチ メートル deg rad 式 文書の単位 表示",
        ),
        Sec::NewDoc => (
            "新規デザイン",
            "新規デザイン 新しい文書 既定 初めの値 単位 インチ 作者 所属 著作権 使用条件 ライセンス",
        ),
        Sec::Arrangement => (
            "画面の配置",
            "配置 パネル 幅 開閉 ツールバー ピン タブの順 既定に戻す",
        ),
        Sec::View3d => (
            "3D の表示",
            "表示の形 投影 平行投影 透視投影 画面の速さ fps 陰影 ワイヤーフレーム",
        ),
        Sec::Ai => (
            "AI との接続",
            "AI Claude MCP 接続 待ち受け 合い言葉 操作 画面",
        ),
        Sec::Module(_) => ("", ""),
    }
}

/// 検索の字(空白で分けた言葉が全部入っているか。大文字と小文字は区別しない)。
pub(crate) fn hits(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    query
        .split_whitespace()
        .all(|w| text.contains(&w.to_lowercase()))
}

/// 加工の作業空間のモジュールの設定か(設定が宣言した作業空間 `group` で決める)。
fn is_machining(group: &str) -> bool {
    group == crate::Space::Machining.action_group()
}

/// 作業空間 `group` に置くモジュールの設定 `id` の頁の名前。
pub(crate) fn module_page(id: &str, group: &str) -> String {
    if is_machining(group) {
        format!("machining/{id}")
    } else {
        format!("design/{id}")
    }
}

/// 頁の並び(モジュールの設定 `groups` の id・見出し・作業空間から作る)。
/// 頁の一覧。手で書いた頁の名前は文言の ID で、`text` で今の言語の文にする(モジュールの頁は
/// 設定が宣言した名前のまま)。
pub(crate) fn pages(
    groups: &[(String, String, String)],
    text: &dyn Fn(&str) -> String,
) -> Vec<Page> {
    let page = |tab, id: &str, title: &str, secs| Page {
        tab,
        id: id.to_string(),
        title: text(title),
        secs,
    };
    let mut out = vec![
        page(
            Tab::General,
            "general/appearance",
            "settings-page-appearance",
            vec![Sec::Appearance],
        ),
        page(
            Tab::General,
            "general/input",
            "settings-page-input",
            vec![Sec::Mouse, Sec::Keys, Sec::Keycast],
        ),
        page(
            Tab::General,
            "general/files",
            "settings-page-files",
            vec![Sec::Autosave, Sec::Import],
        ),
        page(
            Tab::General,
            "general/render",
            "settings-page-render",
            vec![Sec::Render],
        ),
        page(
            Tab::General,
            "general/units",
            "settings-page-units",
            vec![Sec::Units],
        ),
        page(
            Tab::General,
            "general/new_doc",
            "settings-page-new-doc",
            vec![Sec::NewDoc],
        ),
        page(
            Tab::General,
            "general/arrangement",
            "settings-page-arrangement",
            vec![Sec::Arrangement],
        ),
        page(
            Tab::General,
            "general/ai",
            "settings-page-ai",
            vec![Sec::Ai],
        ),
        page(
            Tab::Design,
            "design/view",
            "settings-page-view3d",
            vec![Sec::View3d],
        ),
    ];
    // モジュールの設定ごとに 1 頁(設計: スケッチの表示・作る時、加工: 機械・ポスト・シミュレーション・加工の既定)
    for (id, label, group) in groups {
        let tab = if is_machining(group) {
            Tab::Machining
        } else {
            Tab::Design
        };
        out.push(Page {
            tab,
            id: module_page(id, group),
            title: label.clone(),
            secs: vec![Sec::Module(id.clone())],
        });
    }
    out
}

/// 設定の窓の状態(開いているか・検索の字・キーの付け替えの途中など)。
pub(crate) struct SettingsWindow {
    /// 設定の窓を開いているか
    pub(crate) open: bool,
    /// 設定の窓の検索の字
    pub(crate) query: String,
    /// 設定の窓でキーを受けている行(次に押したキーをその行に割り当てる)
    pub(crate) key_capture: Option<usize>,
    /// キーの割り当ての知らせ(付け替えられないキーなど)
    pub(crate) key_note: Option<String>,
    /// 設定の「画面」の知らせ(書き出したテーマの道など)
    pub(crate) appearance_note: Option<String>,
    /// 設定の「字の書体」の候補(設定の窓を開いた時に探す)
    pub(crate) font_list: Option<Vec<(String, String)>>,
}

impl Workspace {
    /// 設定の窓を開き、`page`(頁の名前)を出す。
    pub fn open_settings_at(&mut self, page: &str) {
        self.settings.open = true;
        self.settings.query.clear();
        self.win.layout.settings_page = page.to_string();
    }

    /// ブラウザの「機械」の行を押した: 機械の頁を出していれば閉じ、ほかなら開く(「パラメータ」の行と
    /// 同じく、もう一度押すと閉じる)。
    pub(crate) fn toggle_machine_settings(&mut self) {
        let before = self.win.layout.settings_page.clone();
        let was_open = self.settings.open;
        self.open_machine_settings();
        if was_open && self.win.layout.settings_page == before {
            self.settings.open = false;
        }
    }

    /// 設定の窓を開き、機械の設定(役割の印 `Machine`)の頁を出す。
    pub(crate) fn open_machine_settings(&mut self) {
        let id = self
            .win
            .extensions
            .settings()
            .into_iter()
            .find(|g| g.role == Some(kmb_interchange::role::settings::MACHINE))
            .map(|g| module_page(&g.id, &g.group));
        match id {
            Some(id) => self.open_settings_at(&id),
            None => self.settings.open = true,
        }
    }

    /// 設定の窓の検索の字を入れる(画面の写真と試験に使う。人は窓の先頭の欄に打つ)。
    pub fn set_settings_query(&mut self, q: &str) {
        self.settings.open = true;
        self.settings.query = q.to_string();
    }

    /// 設定の窓。毎フレーム呼ぶ。
    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings.open {
            self.settings.key_capture = None;
            return;
        }
        // 文書ごとに持つ設定(レンダリングのシーン)は今の文書の値
        let groups = self.cur.work.settings();
        let all = pages(
            &groups
                .iter()
                .map(|g| (g.id.clone(), g.label.clone(), g.group.clone()))
                .collect::<Vec<_>>(),
            &|id| self.win.t(id),
        );
        let tab_names = Tab::ALL.map(|t| (t, self.win.t(t.text_id())));
        let tab_name = |t: Tab| {
            tab_names
                .iter()
                .find(|(x, _)| *x == t)
                .map_or(String::new(), |(_, n)| n.clone())
        };
        let search_hint = self.win.t("settings-search-hint");
        let mut open = true;
        let mut change = None;
        egui::Window::new(self.win.t("settings-window-title"))
            .id(egui::Id::new("settings"))
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(720.0, 620.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.query)
                        .hint_text(search_hint.as_str())
                        .desired_width(f32::INFINITY),
                )
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        true,
                        search_hint.as_str(),
                    )
                });
                let query = self.settings.query.trim().to_string();
                // 探している間は、当たった節を頁をまたいで並べる
                let shown: Vec<(&Page, Vec<&Sec>)> = if query.is_empty() {
                    let cur = all
                        .iter()
                        .find(|p| p.id == self.win.layout.settings_page)
                        .unwrap_or(&all[0]);
                    ui.horizontal(|ui| {
                        for t in Tab::ALL {
                            if !all.iter().any(|p| p.tab == t) {
                                continue;
                            }
                            if ui.selectable_label(cur.tab == t, tab_name(t)).clicked()
                                && cur.tab != t
                                && let Some(p) = all.iter().find(|p| p.tab == t)
                            {
                                self.win.layout.settings_page = p.id.clone();
                            }
                        }
                    });
                    ui.separator();
                    vec![(cur, cur.secs.iter().collect())]
                } else {
                    let found: Vec<(&Page, Vec<&Sec>)> = all
                        .iter()
                        .filter_map(|p| {
                            let secs: Vec<&Sec> = p
                                .secs
                                .iter()
                                .filter(|s| self.sec_hits(&query, p, s, &groups))
                                .collect();
                            (!secs.is_empty()).then_some((p, secs))
                        })
                        .collect();
                    ui.weak(self.win.t_with(
                        "settings-search-found",
                        &[
                            ("query", query.as_str().into()),
                            ("count", found.len().into()),
                        ],
                    ));
                    ui.separator();
                    found
                };
                let cur_tab = shown.first().map(|(p, _)| p.tab);
                ui.horizontal_top(|ui| {
                    if query.is_empty() {
                        // 木の幅は一番長い頁の名前が 1 行に入る幅(1 倍なら 130 で足りる)
                        let font = egui::TextStyle::Button.resolve(ui.style());
                        let widest = all
                            .iter()
                            .filter(|p| Some(p.tab) == cur_tab)
                            .map(|p| {
                                ui.painter()
                                    .layout_no_wrap(
                                        p.title.clone(),
                                        font.clone(),
                                        egui::Color32::WHITE,
                                    )
                                    .size()
                                    .x
                            })
                            .fold(0.0, f32::max);
                        ui.vertical(|ui| {
                            ui.set_width(
                                (widest + 2.0 * ui.spacing().button_padding.x + 4.0).max(130.0),
                            );
                            for p in all.iter().filter(|p| Some(p.tab) == cur_tab) {
                                let on = p.id == shown[0].0.id;
                                if ui.selectable_label(on, &p.title).clicked() && !on {
                                    self.win.layout.settings_page = p.id.clone();
                                    self.settings.key_capture = None;
                                }
                            }
                        });
                        ui.separator();
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("settings_page")
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            // 横に並べる枠の中なので、頁の中身は縦に積み直す
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                for (p, secs) in &shown {
                                    if !query.is_empty() {
                                        ui.strong(format!("{} › {}", tab_name(p.tab), p.title));
                                    }
                                    for s in secs {
                                        // 1 節だけの頁は、頁の名前と同じ見出しを重ねて出さない
                                        let heading = !query.is_empty() || p.secs.len() > 1;
                                        if let Some(c) =
                                            self.settings_sec(ui, s, &groups, &query, heading)
                                        {
                                            change = Some(c);
                                        }
                                        ui.separator();
                                    }
                                }
                                if shown.is_empty() {
                                    ui.weak("当たる設定はありません");
                                }
                            });
                        });
                });
            });
        if let Some((id, v)) = change {
            self.change_settings(&id, &v);
        }
        if !open {
            self.settings.open = false;
            self.settings.key_capture = None;
        }
    }

    /// 節が検索に当たるか。
    fn sec_hits(&self, query: &str, p: &Page, s: &Sec, groups: &[kmb_core::Settings]) -> bool {
        if hits(query, &p.title) {
            return true;
        }
        match s {
            Sec::Module(id) => groups.iter().find(|g| &g.id == id).is_some_and(|g| {
                let words: Vec<&str> = std::iter::once(g.label.as_str())
                    .chain(g.notes.iter().map(String::as_str))
                    .chain(g.fields.iter().map(|f| f.label.as_str()))
                    .collect();
                hits(query, &words.join(" "))
            }),
            Sec::Keys => {
                let (t, w) = sec_words(s);
                hits(query, &format!("{t} {w}")) || !self.key_rows(query).is_empty()
            }
            _ => {
                let (t, w) = sec_words(s);
                hits(query, &format!("{t} {w}"))
            }
        }
    }

    /// 節を出す。モジュールの設定が変わったら、その id と値を返す。
    fn settings_sec(
        &mut self,
        ui: &mut egui::Ui,
        s: &Sec,
        groups: &[kmb_core::Settings],
        query: &str,
        heading: bool,
    ) -> Option<(String, serde_json::Value)> {
        if heading {
            ui.heading(match s {
                Sec::Module(id) => groups
                    .iter()
                    .find(|g| &g.id == id)
                    .map_or(id.clone(), |g| g.label.clone()),
                Sec::Keycast => self.win.t("settings-keycast-title"),
                _ => sec_words(s).0.to_string(),
            });
        }
        match s {
            Sec::Appearance => self.appearance_settings(ui),
            Sec::Mouse => self.mouse_settings(ui),
            Sec::Keys => self.key_settings(ui, query),
            Sec::Keycast => self.keycast_settings(ui),
            Sec::Autosave => self.autosave_settings(ui),
            Sec::Units => self.units_settings(ui),
            Sec::NewDoc => self.new_doc_settings(ui),
            Sec::Arrangement => self.arrangement_settings(ui),
            Sec::View3d => self.view3d_settings(ui),
            Sec::Import => self.import_settings(ui),
            Sec::Ai => self.live_settings(ui),
            Sec::Render => self.render_settings(ui),
            Sec::Module(id) => {
                let g = groups.iter().find(|g| &g.id == id)?;
                for n in &g.notes {
                    ui.weak(n);
                }
                let mut v = g.value.clone();
                egui::Grid::new(("settings", &g.id))
                    .num_columns(2)
                    .show(ui, |ui| {
                        // ほかの欄の値で使わない欄(Field::only)は出さない(背景が単色の時だけの色など)
                        for f in g.fields.iter().filter(|f| f.applies(&g.value)) {
                            crate::actions_ui::setting_field(ui, &g.id, f, &mut v);
                            ui.end_row();
                        }
                    });
                if ui
                    .add_enabled(
                        g.value != g.default,
                        egui::Button::new(format!("{}を初めの値に戻す", g.label)),
                    )
                    .clicked()
                {
                    v = g.default.clone();
                }
                if v != g.value {
                    return Some((g.id.clone(), v));
                }
            }
        }
        None
    }

    /// 設定の「単位」の節: 今の文書の長さの単位(文書に入る。取り消せる)と、単位の決まり。
    fn units_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("この文書の長さの単位");
            let mut u = self.length_unit();
            crate::doc_ui::unit_combo(ui, "settings_doc_units", &mut u).on_hover_text(
                "今のタブの文書の単位(設計の属性と同じ)。文書に入って一緒に持ち運ぶ。取り消せる",
            );
            if u != self.length_unit() {
                let mut p = self.cur.work.doc().data().properties.clone();
                p.units = u;
                self.set_properties(p);
            }
        });
        ui.weak(
            "寸法・式の欄・パラメータの値・測る道具はこの単位で出し、数だけ打った長さもこの単位で読む。\
             形と保存は mm、角度は deg のまま(単位を変えても形は変わらない。G-code はいつも mm)。\
             式には mm・cm・m・in・ft・deg・rad を書ける。新規デザインの単位は「新規デザイン」の頁で決める",
        );
    }

    /// 設定の「新しい文書」の節: 新しい文書に初めから入れる単位と設計の属性。
    fn new_doc_settings(&mut self, ui: &mut egui::Ui) {
        ui.weak(
            "新規デザイン・新しいタブ・起動の時の無題の文書に入れる。見本と開いたファイルは変えない",
        );
        let before = self.win.layout.new_doc.clone();
        let nd = &mut self.win.layout.new_doc;
        egui::Grid::new("new_doc_grid")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("長さの単位");
                crate::doc_ui::unit_combo(ui, "settings_new_units", &mut nd.units);
                ui.end_row();
                for (key, label) in crate::layout::NewDoc::TEXTS {
                    ui.label(label);
                    if let Some(v) = nd.text_mut(key) {
                        ui.add(egui::TextEdit::singleline(v).desired_width(240.0));
                    }
                    ui.end_row();
                }
            });
        if ui
            .add_enabled(
                *nd != crate::layout::NewDoc::default(),
                egui::Button::new("新規デザインの値を初めの値に戻す"),
            )
            .clicked()
        {
            *nd = crate::layout::NewDoc::default();
        }
        // まだ何もしていない無題の文書にも、変えた値を入れる(起動してすぐ単位を変えた時に、
        // 開いている空の文書が前の単位のまま残らないように。10-24 u11)
        if self.win.layout.new_doc != before && self.is_blank() {
            self.replace_document(self.blank_document(), None);
        }
    }

    /// 検索に当たるキーの行(検索が空なら全部)。節の名前に当たった時も全部。
    fn key_rows(&self, query: &str) -> Vec<usize> {
        let (t, w) = sec_words(&Sec::Keys);
        let all = query.is_empty() || hits(query, &format!("{t} {w} マウスとキー"));
        (0..KEYS.len())
            .filter(|&i| {
                all || hits(
                    query,
                    &format!(
                        "{} {} {}",
                        KEYS[i].text,
                        KEYS[i].keys,
                        self.win.keymap.label(i)
                    ),
                )
            })
            .collect()
    }

    /// 設定の「キーの割り当て」の節(U36)。
    fn key_settings(&mut self, ui: &mut egui::Ui, query: &str) {
        ui.weak(
            "「変える」を押してから、割り当てたいキーを押す(Ctrl・Shift・Alt と組み合わせてよい)。\
             Esc でやめる。同じキーを 2 つの行に付けると ⚠ が出て、上の行だけが効く\
             (スケッチの中だけの行と 3D のビューだけの行は、同じキーを付けてよい)",
        );
        // 受けている行があれば、このフレームで押したキーを割り当てる
        if let Some(i) = self.settings.key_capture {
            let got = ui.input_mut(|inp| {
                let at = inp.events.iter().position(|e| {
                    matches!(
                        e,
                        egui::Event::Key {
                            pressed: true,
                            repeat: false,
                            ..
                        } | egui::Event::Copy
                            | egui::Event::Cut
                            | egui::Event::Paste(_)
                    )
                })?;
                match inp.events.remove(at) {
                    egui::Event::Key { key, modifiers, .. } => Some(Some((key, modifiers))),
                    _ => Some(None),
                }
            });
            // Ctrl+Shift+C などは写す働きとして届き、キーが分からない。受けたまま黙らず理由を出す
            if got == Some(None) {
                self.settings.key_note = Some(format!(
                    "{}なので付け替えられません。別のキーを押してください(Esc でやめる)",
                    keys::COPY_KEYS
                ));
            }
            if let Some(Some((key, mods))) = got {
                if key == egui::Key::Escape && mods.is_none() {
                    self.settings.key_capture = None;
                    self.settings.key_note = None;
                } else {
                    match keys::capture(key, mods) {
                        Ok(ch) => {
                            self.set_key(i, vec![ch]);
                            self.settings.key_capture = None;
                            let n = keys::chord_name(ch);
                            self.settings.key_note = match self.keymap_for_note(i) {
                                Some(other) => Some(format!(
                                    "{n} を「{}」に付けました。「{other}」と重なっています",
                                    KEYS[i].text
                                )),
                                None => Some(format!("{n} を「{}」に付けました", KEYS[i].text)),
                            };
                        }
                        Err(why) => {
                            self.settings.key_note = Some(format!(
                                "{} は{why}なので付け替えられません。別のキーを押してください(Esc でやめる)",
                                keys::chord_name((mods, key))
                            ));
                        }
                    }
                }
            }
        }
        let warn = crate::theme::look(ui.ctx()).pal.warn;
        if let Some(n) = &self.settings.key_note {
            ui.colored_label(warn, n);
        }
        let rows = self.key_rows(query);
        let scope_name = |s: Scope| match s {
            Scope::Always | Scope::View => "どこでも",
            Scope::Model => "3D のビュー",
            Scope::Sketch => "スケッチ",
        };
        let mut act: Option<(usize, KeyAct)> = None;
        egui::Grid::new("key_map")
            .num_columns(4)
            .striped(true)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.strong("働き");
                ui.strong("キー");
                ui.strong("効く所");
                ui.strong("");
                ui.end_row();
                for i in rows {
                    let r = &KEYS[i];
                    ui.label(keys::row_text(ui.ctx(), r));
                    let label = if self.settings.key_capture == Some(i) {
                        "キーを押す…".to_string()
                    } else {
                        match self.win.keymap.label(i) {
                            s if s.is_empty() => "(なし)".to_string(),
                            s => s,
                        }
                    };
                    let clash = self.win.keymap.conflicts(i);
                    ui.horizontal(|ui| {
                        crate::theme::kbd(ui, &label);
                        if !clash.is_empty() {
                            let names: Vec<&str> = clash.iter().map(|&j| KEYS[j].text).collect();
                            ui.colored_label(warn, "⚠").on_hover_text(format!(
                                "同じキーが「{}」にも付いている(表の上の行だけが効く)",
                                names.join("」「")
                            ));
                        }
                    });
                    ui.weak(scope_name(r.scope));
                    ui.horizontal(|ui| {
                        if r.id.is_empty() {
                            ui.weak("固定");
                            return;
                        }
                        let capturing = self.settings.key_capture == Some(i);
                        if ui
                            .selectable_label(capturing, "変える")
                            .on_hover_text("押してから、割り当てたいキーを押す")
                            .clicked()
                        {
                            act = Some((i, KeyAct::Capture));
                        }
                        if ui
                            .add_enabled(
                                !self.win.keymap.chords(i).is_empty(),
                                egui::Button::new("外す"),
                            )
                            .clicked()
                        {
                            act = Some((i, KeyAct::Clear));
                        }
                        if ui
                            .add_enabled(
                                self.win.layout.keys.contains_key(r.id),
                                egui::Button::new("戻す"),
                            )
                            .on_hover_text(format!("既定の {} に戻す", r.keys))
                            .clicked()
                        {
                            act = Some((i, KeyAct::Reset));
                        }
                    });
                    ui.end_row();
                }
            });
        match act {
            Some((i, KeyAct::Capture)) => {
                self.settings.key_capture = (self.settings.key_capture != Some(i)).then_some(i);
                self.settings.key_note = None;
            }
            Some((i, KeyAct::Clear)) => {
                self.set_key(i, vec![]);
                self.settings.key_note = Some(format!("「{}」のキーを外しました", KEYS[i].text));
            }
            Some((i, KeyAct::Reset)) => {
                self.win.layout.keys.remove(KEYS[i].id);
                self.settings.key_note = Some(format!(
                    "「{}」を既定の {} に戻しました",
                    KEYS[i].text, KEYS[i].keys
                ));
            }
            None => {}
        }
        if ui
            .add_enabled(
                !self.win.layout.keys.is_empty(),
                egui::Button::new("キーを全部既定に戻す"),
            )
            .clicked()
        {
            self.win.layout.keys.clear();
            self.settings.key_capture = None;
            self.settings.key_note = Some("キーを全部既定に戻しました".into());
        }
        // 割り当ての変化をすぐ使う(同じフレームの後の部品と次のフレームのキー)
        self.sync_keymap();
    }

    /// 行 `i` の割り当てを `chords` にする(既定と同じなら覚えない)。
    fn set_key(&mut self, i: usize, chords: Vec<keys::Chord>) {
        let id = KEYS[i].id.to_string();
        if chords.as_slice() == KEYS[i].press {
            self.win.layout.keys.remove(&id);
        } else {
            self.win
                .layout
                .keys
                .insert(id, chords.into_iter().map(keys::chord_name).collect());
        }
        self.sync_keymap();
    }

    /// 行 `i` と重なる行の働き(知らせに出す)。
    fn keymap_for_note(&self, i: usize) -> Option<&'static str> {
        self.win.keymap.conflicts(i).first().map(|&j| KEYS[j].text)
    }
}

/// キーの行のボタン。
enum KeyAct {
    Capture,
    Clear,
    Reset,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_module_gets_a_page_on_its_workspace_tab() {
        let g = |id: &str, label: &str, group: &str| (id.into(), label.into(), group.into());
        // 頁は id の頭ではなく、設定が宣言した作業空間で決まる
        let groups = [
            g("cam.machine", "機械", "cam"),
            g("cam.post", "ポスト", "cam"),
            g("sketch.display", "スケッチの表示", "cad"),
            g("demo.thing", "見本", "cad"),
            g("probe.cycle", "測りの既定", "cam"),
        ];
        let c = kmb_i18n::Catalog::with_os(kmb_i18n::Lang::Ja);
        let ja = |id: &str| c.text(kmb_i18n::Lang::Ja, id);
        let p = pages(&groups, &ja);
        let find = |id: &str| p.iter().find(|x| x.id == id).unwrap();
        assert_eq!(find("machining/cam.machine").tab, Tab::Machining);
        assert_eq!(find("machining/cam.post").title, "ポスト");
        assert_eq!(find("design/sketch.display").tab, Tab::Design);
        assert_eq!(find("machining/probe.cycle").tab, Tab::Machining);
        assert_eq!(
            find("design/demo.thing").secs,
            vec![Sec::Module("demo.thing".into())]
        );
        // 頁の名前は重ならない
        let ids: std::collections::HashSet<&String> = p.iter().map(|x| &x.id).collect();
        assert_eq!(ids.len(), p.len());
        // モジュールが無ければ、空の頁は出さない
        assert_eq!(find("general/appearance").title, "画面");
        let bare = pages(&[], &ja);
        assert!(bare.iter().all(|x| !x.secs.is_empty()));
        assert!(!bare.iter().any(|x| x.tab == Tab::Machining));
    }

    #[test]
    fn search_needs_every_word() {
        assert!(hits("字 大きさ", "字の大きさの倍率"));
        assert!(!hits("字 投影", "字の大きさ"));
        assert!(hits("FPS", "画面の速さ fps"));
        assert!(hits("", "なんでも"));
    }
}
