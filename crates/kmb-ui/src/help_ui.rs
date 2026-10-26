//! メニューの「ヘルプ」(操作の手引き・F1)と、クラッシュ報告の窓。報告を探す・読むのは
//! kmb-session(画面は出すだけ)。報告は PC の中に残すだけで、どこへも送らない。

use crate::Workspace;
use crate::layout::MiddleDrag;
use kmb_session::crash;

/// 操作の手引きの流れ(題、手順)。
pub(crate) const FLOWS: &[(&str, &[&str])] = &[
    (
        "形を作る(設計)",
        &[
            "「スケッチを作成」で置く面を選び、線・円・長方形をクリックで描き、寸法を付けて「✓ スケッチを終了」",
            "「押し出し」を押し、押し出す輪郭(領域)をクリックで選び、高さを打って Enter",
            "辺や面をクリックで選び、「フィレット」「面取り」「穴」などを足す",
            "形を直す時は、タイムラインかブラウザで機能をダブルクリック(または選んで Enter)",
        ],
    ),
    (
        "削る(加工)",
        &[
            "左上の作業空間を「加工」にし、「セットアップ」を足して材料とストックを決める",
            "必要なら「固定具を置く」でベッドをクリック(ストックに重なる所では外へずらす)",
            "「フェイス」「輪郭」「ポケット」「穴あけ」などを足す。工具は欄の一覧から選ぶ",
            "「シミュレーション」で食い込みと削り残しを確かめる(結果は窓に出る)",
            "セットアップを選び、編集パネルの「G-code を名前を付けて書き出す…」で保存する",
        ],
    ),
];

/// クラッシュ報告の窓の状態(開いた時に一覧を読む)。
#[derive(Default)]
pub(crate) struct CrashPage {
    pub open: bool,
    pub list: Vec<crash::Summary>,
}

/// 今の UNIX 時刻の秒。
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// UNIX 時刻の秒を「何分前」の形に。
pub(crate) fn ago(t: u64) -> String {
    let m = now().saturating_sub(t) / 60;
    match m {
        0 => "たった今".into(),
        1..=59 => format!("{m} 分前"),
        60..=1439 => format!("{} 時間前", m / 60),
        _ => format!("{} 日前", m / 1440),
    }
}

impl Workspace {
    pub(crate) fn help_menu(&mut self, ui: &mut egui::Ui) {
        let help = self.win.t("frame-help");
        let r = crate::theme::icon_button(ui, "help", &help, true, false).on_hover_text(&help);
        egui::Popup::menu(&r).show(|ui| {
            if ui
                .add(egui::Button::new(self.win.t("menu-help-guide")).shortcut_text("F1"))
                .on_hover_text(self.win.t("menu-help-guide-tip"))
                .clicked()
            {
                self.win.guide_open = true;
                ui.close();
            }
            if ui
                .button(self.win.t("menu-help-search"))
                .on_hover_text(self.win.t("menu-help-search-tip"))
                .clicked()
            {
                if !self.is_command_palette_open() {
                    self.toggle_command_palette();
                }
                ui.close();
            }
            ui.separator();
            if ui
                .button(self.win.t("menu-help-crash"))
                .on_hover_text(self.win.t("menu-help-crash-tip"))
                .clicked()
            {
                self.open_crash_reports();
                ui.close();
            }
        });
    }

    /// 操作の手引きの窓(F1 で開く・閉じる)。毎フレーム呼ぶ。
    pub(crate) fn guide_window(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F1)) {
            self.win.guide_open = !self.win.guide_open;
        }
        if !self.win.guide_open {
            return;
        }
        let mut open = true;
        let mid = match self.win.layout.middle_drag {
            MiddleDrag::Pan => "動かす",
            MiddleDrag::Orbit => "回す",
        };
        egui::Window::new("操作の手引き")
            .open(&mut open)
            .default_size([520.0, 520.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("マウス");
                    egui::Grid::new("guide_mouse").num_columns(2).show(ui, |ui| {
                        for (k, v) in [
                            ("左クリック", "選ぶ(Ctrl・Shift で足す)。ドラッグで範囲、長押しで重なった候補"),
                            ("右クリック", "その場のメニュー(表示の形や投影もここ)"),
                            ("右ドラッグ", "回す"),
                            ("ホイール押しドラッグ", mid),
                            ("ホイール押しの二度押し", "そこを回す中心にする"),
                            ("ホイール", "拡大・縮小(カーソルの位置へ)"),
                        ] {
                            ui.strong(k);
                            ui.label(v);
                            ui.end_row();
                        }
                    });
                    ui.weak("ホイール押しドラッグの働きは「設定」の「一般 › マウスとキー」で替えられる");
                    ui.separator();
                    ui.heading("キー");
                    ui.weak("キーは「設定」の「一般 › マウスとキー」で付け替えられる(Ctrl+,)");
                    egui::Grid::new("guide_keys").num_columns(2).show(ui, |ui| {
                        // 設定で付け替えたキーは、付け替えた後の名前で出す
                        for (i, r) in crate::keys::KEYS.iter().enumerate() {
                            match self.win.keymap.label(i) {
                                k if k.is_empty() => ui.weak("(なし)"),
                                k => ui.strong(k),
                            };
                            ui.label(r.text);
                            ui.end_row();
                        }
                    });
                    for (title, steps) in FLOWS {
                        ui.separator();
                        ui.heading(*title);
                        for (n, s) in steps.iter().enumerate() {
                            ui.label(format!("{}. {s}", n + 1));
                        }
                    }
                });
            });
        self.win.guide_open &= open;
    }

    /// クラッシュ報告の窓を開く(一覧を読み直す)。
    pub fn open_crash_reports(&mut self) {
        self.win.crash_page.list = self
            .win
            .data_dir
            .as_deref()
            .map(crash::list)
            .unwrap_or_default();
        self.win.crash_page.open = true;
    }

    pub(crate) fn crash_window(&mut self, ctx: &egui::Context) {
        if !self.win.crash_page.open {
            return;
        }
        let mut open = true;
        let mut read = None;
        let mut copy = None;
        let mut open_dir = false;
        egui::Window::new("クラッシュ報告")
            .open(&mut open)
            .default_size([460.0, 320.0])
            .show(ctx, |ui| {
                let Some(dir) = self.win.data_dir.clone() else {
                    ui.weak("データフォルダがないので、報告は残していません");
                    return;
                };
                ui.weak(format!(
                    "置き場所: {}(最大 {} 件・{} 日。どこへも送りません)",
                    crash::crash_dir(&dir).display(),
                    crash::MAX_REPORTS,
                    crash::MAX_AGE.as_secs() / 86_400
                ));
                ui.horizontal(|ui| {
                    if ui.small_button("読み直す").clicked() {
                        self.win.crash_page.list = crash::list(&dir);
                    }
                    // 報告は開発者へファイルで渡す(どこへも送らない)
                    if ui
                        .small_button("報告の入ったフォルダを開く")
                        .on_hover_text(
                            "報告のファイルを OS のファイルの窓で開く(メールなどに付けて渡せる)",
                        )
                        .clicked()
                    {
                        open_dir = true;
                    }
                });
                ui.separator();
                if self.win.crash_page.list.is_empty() {
                    ui.label("報告はありません");
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for s in &self.win.crash_page.list {
                        let kind = match s.kind.as_str() {
                            "panic" => "内部の誤り",
                            "abnormal_exit" => "異常終了",
                            k => k,
                        };
                        ui.horizontal(|ui| {
                            if ui.button("見る").clicked() {
                                read = Some(s.name.clone());
                            }
                            if ui
                                .button("写す")
                                .on_hover_text(
                                    "この報告の中身をクリップボードへ写す(貼り付けて渡せる)",
                                )
                                .clicked()
                            {
                                copy = Some(s.name.clone());
                            }
                            // 次の起動で見つけた異常終了は、時刻が最後に操作を記録した頃(落ちたのはその後)
                            let when = if s.detected_later {
                                format!("最後の記録 {}", ago(s.time))
                            } else {
                                ago(s.time)
                            };
                            ui.label(format!("{when}・{kind}")).on_hover_text(
                                if s.detected_later {
                                    "次の起動で見つけた異常終了。時刻は最後に操作を記録した頃"
                                } else {
                                    "起きた時に書いた報告"
                                },
                            );
                            ui.weak(&s.message);
                        });
                    }
                });
            });
        if let (true, Some(dir)) = (open_dir, self.win.data_dir.as_ref())
            && let Err(e) = kmb_session::open_folder(&crash::crash_dir(dir))
        {
            self.win.fail(e);
        }
        if let (Some(name), Some(dir)) = (copy, self.win.data_dir.clone()) {
            match crash::read(&dir, &name) {
                Ok(v) => {
                    ctx.copy_text(serde_json::to_string_pretty(&v).unwrap_or_default());
                    self.win.message = Some(format!("報告 {name} をクリップボードへ写しました"));
                    self.win.message_tone = crate::window_state::Tone::Info;
                }
                Err(e) => self.win.fail(e),
            }
        }
        if let (Some(name), Some(dir)) = (read, self.win.data_dir.clone()) {
            match crash::read(&dir, &name) {
                Ok(v) => {
                    let text = serde_json::to_string_pretty(&v).unwrap_or_default();
                    self.cur.viewer = Some((name, text.lines().map(str::to_string).collect()));
                }
                Err(e) => self.win.fail(e),
            }
        }
        self.win.crash_page.open &= open;
    }
}
