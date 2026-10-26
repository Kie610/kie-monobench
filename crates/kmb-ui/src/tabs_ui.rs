//! 文書のタブ。複数の文書を 1 つの窓で開き、上の帯のタブで
//! 切り替える。タブは上の帯(ファイル・保存の右)に並べる。
//!
//! 作り: 今のタブの文書ごとの状態(文書・取り消し・計算・選んでいる物・カメラ・編集の途中の状態・
//! カーネル。[`DocState`])は作業場の `cur` にある。ほかのタブはそれをしまっておき、切り替える時に
//! 丸ごと入れ替える。PC 全体の物(設定・画面の配置・最近使ったファイル・知らせ)は入れ替えない。しまったタブは描かず、新しい計算も始めない。走っている計算はそのまま走り、
//! 戻った時に結果を取り込む。
//!
//! 形のカーネルはタブごとに作る([`Workspace::with_kernel_factory`])。カーネルは再計算のたびに
//! 「今の文書に無い形」を捨てるので、1 つを分け合うと、片方の計算でもう片方の形が消える。
//! 作り方を渡されていない時(テスト)は分け合い、戻ったタブを一から計算し直す。

use crate::Workspace;
use crate::doc_state::DocState;
use crate::theme::RightMenu as _;
use kmb_core::Document;
use std::path::Path;
use std::sync::Arc;

/// タブごとに形のカーネルを作る関数。
pub type KernelFactory = Arc<dyn Fn() -> Arc<dyn kmb_core::Kernel> + Send + Sync>;

/// 文書のタブ 1 つ。
pub(crate) struct DocTab {
    /// タブの通し番号(画面の部品の記憶を文書ごとに分ける)
    pub uid: u64,
    /// しまってある文書(今のタブは空。中身は作業場の `cur`)
    pub parked: Option<Box<DocState>>,
}

/// タブに出す名前(拡張子なし。無題は今の言語の「(無題)」の `untitled`)。
fn tab_name(path: Option<&Path>, label: Option<&str>, untitled: &str) -> String {
    match (path.and_then(|p| p.file_stem()), label) {
        (Some(n), _) => n.to_string_lossy().to_string(),
        (None, Some(l)) => l.to_string(),
        (None, None) => untitled.to_string(),
    }
}

impl Workspace {
    /// タブごとに形のカーネルを作る関数を渡す(アプリは kmb-bundle の `kernel`)。
    pub fn with_kernel_factory(mut self, f: KernelFactory) -> Self {
        self.win.kernel_factory = Some(f);
        self
    }

    /// 開いている文書の数。
    pub fn tab_count(&self) -> usize {
        self.win.doc_tabs.len()
    }

    /// 今のタブの位置(0 から)。
    pub fn active_tab(&self) -> usize {
        self.win.doc_tab
    }

    /// タブの名前と、未保存か(並べた順)。
    pub fn tab_titles(&self) -> Vec<(String, bool)> {
        let untitled = self.win.t("frame-tab-untitled");
        self.win
            .doc_tabs
            .iter()
            .map(|t| match &t.parked {
                Some(w) => (
                    tab_name(w.work.path(), w.doc_label.as_deref(), &untitled),
                    w.modified,
                ),
                None => (
                    tab_name(
                        self.cur.work.path(),
                        self.cur.doc_label.as_deref(),
                        &untitled,
                    ),
                    self.cur.modified,
                ),
            })
            .collect()
    }

    /// 今のタブの通し番号(画面の部品の記憶の鍵に混ぜる)。
    pub(crate) fn tab_uid(&self) -> u64 {
        self.win.doc_tabs.get(self.win.doc_tab).map_or(0, |t| t.uid)
    }

    /// 何も書いていない無題の文書か(開く時は新しいタブを作らず、ここへ開く)。
    pub(crate) fn is_blank(&self) -> bool {
        let d = self.cur.work.doc().data();
        self.cur.work.path().is_none()
            && !self.cur.modified
            && d.features.is_empty()
            && d.params.is_empty()
            && !self.cur.work.doc().can_undo()
    }

    /// `path` を開いているタブ。
    pub(crate) fn tab_of(&self, path: &Path) -> Option<usize> {
        let real = std::fs::canonicalize(path).ok();
        let same = |p: Option<&Path>| {
            p.is_some_and(|p| p == path || real.is_some() && std::fs::canonicalize(p).ok() == real)
        };
        self.win.doc_tabs.iter().position(|t| match &t.parked {
            Some(w) => same(w.work.path()),
            None => same(self.cur.work.path()),
        })
    }

    /// タブ `i` へ切り替える。
    pub fn switch_tab(&mut self, i: usize) {
        if i >= self.win.doc_tabs.len() {
            return;
        }
        // タブを押したら開始画面を閉じてその文書へ
        self.home.start = false;
        if i == self.win.doc_tab {
            return;
        }
        let Some(mut other) = self.win.doc_tabs[i].parked.take() else {
            return;
        };
        // 3D のビューの欄が 1 つの道具で止まっているなど、画面全体の途中の状態は閉じる
        self.search.text = None;
        self.view_menu.candidates = None;
        self.view_menu.context_others.1.clear();
        std::mem::swap(&mut self.cur, &mut *other);
        self.win.doc_tabs[self.win.doc_tab].parked = Some(other);
        self.win.doc_tab = i;
        if self.win.kernel_factory.is_none() {
            // カーネルを分け合っている: 前のタブの計算で、このタブの形は捨てられている
            self.cur.work.drop_cache();
            self.cur.edge_cache = Default::default();
            self.cur.vertex_cache.clear();
            self.recompute();
        }
        log::info!("タブを替えた: {}", self.document_name());
    }

    /// タブ `from` を `to` の位置へ動かす(タブを引いて並べ替える)。
    /// 今のタブは今のまま(引いたタブへは切り替えない)。
    pub fn move_tab(&mut self, from: usize, to: usize) {
        let n = self.win.doc_tabs.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let t = self.win.doc_tabs.remove(from);
        self.win.doc_tabs.insert(to, t);
        // 今のタブは中身を作業場に持つ(しまっていない)ただ 1 つのタブ
        self.win.doc_tab = self
            .win
            .doc_tabs
            .iter()
            .position(|t| t.parked.is_none())
            .expect("今のタブが 1 つある");
        log::info!("タブを {} 番目から {} 番目へ動かした", from + 1, to + 1);
    }

    /// 次(`step` が 1)・前(-1)のタブへ。端では反対の端へ回る。
    pub fn cycle_tab(&mut self, step: isize) {
        let n = self.win.doc_tabs.len() as isize;
        if n > 1 {
            let i = (self.win.doc_tab as isize + step).rem_euclid(n);
            self.switch_tab(i as usize);
        }
    }

    /// 新しい文書の状態(無題の空の文書。カーネルはタブごとに作り、編集器は今の物の新しい写し)。
    fn fresh_doc_state(&self) -> DocState {
        let kernel = match &self.win.kernel_factory {
            Some(f) => f(),
            None => self.cur.work.kernel().clone(),
        };
        let doc = Document::new();
        let saved = doc.data().clone();
        let modules = kmb_core::Modules {
            registry: self.win.registry.clone(),
            kernel,
            extensions: self.win.extensions.clone(),
        };
        let mut d = DocState::new(doc, None, saved, modules);
        d.editors = self
            .cur
            .editors
            .iter()
            .map(|e| {
                let mut n = e.fresh();
                n.set_data_dir(self.win.data_dir.as_deref());
                n
            })
            .collect();
        d.autosave = self.win.data_dir.as_deref().map(kmb_session::Autosave::new);
        d
    }

    /// 無題の文書を新しいタブで開き、そこへ切り替える(今のタブの右に置く)。
    pub fn new_tab(&mut self) {
        let shell = self.fresh_doc_state();
        let shell = std::mem::replace(&mut self.cur, shell);
        // 前の文書を今のタブへしまい、新しいタブを右に足す
        self.win.doc_tabs[self.win.doc_tab].parked = Some(Box::new(shell));
        self.win.tab_seq += 1;
        let at = self.win.doc_tab + 1;
        self.win.doc_tabs.insert(
            at,
            crate::tabs_ui::DocTab {
                uid: self.win.tab_seq,
                parked: None,
            },
        );
        self.win.doc_tab = at;
        // この作業場の設定(作業空間・表示)で初めの状態にそろえる(新しい文書の既定の値も入れる)
        self.replace_document(self.blank_document(), None);
        log::info!("新しいタブ(全部で {})", self.win.doc_tabs.len());
    }

    /// 今のタブが空でなければ、新しいタブを作る。作ったら真。
    pub(crate) fn make_room(&mut self) -> bool {
        if self.is_blank() {
            return false;
        }
        self.new_tab();
        true
    }

    /// ファイルを開く。開いていればそのタブへ切り替え、今のタブが空でなければ新しいタブに開く
    /// (前の文書はそのタブに残る)。開けなければ作ったタブを閉じる。
    pub fn open_in_tab(&mut self, path: &Path) -> bool {
        if let Some(i) = self.tab_of(path) {
            self.switch_tab(i);
            self.win.message = Some(format!("{} は開いています", path.display()));
            return true;
        }
        let made = self.make_room();
        let ok = self.load_path(path);
        if !ok && made {
            let m = self.win.message.take();
            self.drop_tab();
            self.win.message = m;
        }
        ok
    }

    /// 今のタブを確かめずに閉じる(最後の 1 つなら無題の文書にする)。
    pub(crate) fn drop_tab(&mut self) {
        if self.win.doc_tabs.len() <= 1 {
            // 文書の状態を丸ごと作り直す(前はカメラ・部品の番号などが次の文書に残った)
            if let Some(a) = &mut self.cur.autosave {
                a.clear();
            }
            let fresh = self.fresh_doc_state();
            let mut old = std::mem::replace(&mut self.cur, fresh);
            old.stop();
            self.replace_document(self.blank_document(), None);
            return;
        }
        let i = self.win.doc_tab;
        let to = if i + 1 < self.win.doc_tabs.len() {
            i + 1
        } else {
            i - 1
        };
        if let Some(a) = &mut self.cur.autosave {
            a.clear();
        }
        self.switch_tab(to);
        if let Some(mut gone) = self.win.doc_tabs.remove(i).parked {
            gone.stop();
        }
        if self.win.doc_tab > i {
            self.win.doc_tab -= 1;
        }
    }

    /// タブ `i` を閉じる。未保存なら保存するかを聞き、やめたら偽。最後のタブを閉じると、
    /// 無題の文書と開始画面になる。
    pub fn close_tab(&mut self, i: usize) -> bool {
        if i >= self.win.doc_tabs.len() {
            return false;
        }
        self.switch_tab(i);
        if !self.confirm_discard() {
            return false;
        }
        let name = self.document_name();
        let last = self.win.doc_tabs.len() == 1;
        self.drop_tab();
        if last {
            self.home.start = true;
            self.check_recent();
        }
        self.win.message = Some(format!("{name} を閉じました"));
        true
    }

    /// 窓を閉じてよいか: 未保存のタブを順に開いて、1 つずつ聞く。
    pub fn confirm_close(&mut self) -> bool {
        for i in 0..self.win.doc_tabs.len() {
            let modified = match &self.win.doc_tabs[i].parked {
                Some(w) => w.modified,
                None => self.cur.modified,
            };
            if modified {
                self.switch_tab(i);
                if !self.confirm_discard() {
                    return false;
                }
            }
        }
        true
    }

    /// 上の帯のタブ(名前・未保存の「•」・閉じる)と「+」。残りの幅は窓を引く所にする。
    pub(crate) fn doc_tabs_bar(&mut self, ui: &mut egui::Ui) {
        let l = crate::theme::look(ui.ctx());
        let titles = self.tab_titles();
        let paths: Vec<Option<String>> = self
            .win
            .doc_tabs
            .iter()
            .map(|t| {
                match &t.parked {
                    Some(w) => w.work.path(),
                    None => self.cur.work.path(),
                }
                .map(|p| p.display().to_string())
            })
            .collect();
        let mut switch = None;
        let mut close = None;
        let mut close_others = None;
        let mut moved = None;
        let t = |id: &str| self.win.t(id);
        let (close_text, close_others_text) = (t("frame-tab-close"), t("frame-tab-close-others"));
        let (saved_mod, unsaved, unsaved_mod) = (
            "frame-tab-saved-modified",
            t("frame-tab-unsaved"),
            t("frame-tab-unsaved-modified"),
        );
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let h = ui.available_height() - 6.0;
            // 左端は閉じられないホームのタブ(モデル管理の画面)
            self.home_tab(ui, h);
            // タブが多い時は 1 つの幅を縮める(最小でも名前の頭は見える)。それでも入らない分は
            // 右の「▾」の一覧にまとめ、帯からはみ出さない(前は 70 より細くならず、タブを 10 枚
            // ほど開くとコマンド検索の欄に重なった。字を大きくした時は 1 枚でも重なった)。
            // 幅・閉じる印は字の倍率に合わせる(1 倍で 70〜190、閉じる 16)
            let (min_w, max_w) = (crate::theme::fs(70.0), crate::theme::fs(190.0));
            let button_w = 30.0 * crate::theme::band() + 2.0;
            let room = (ui.available_width() - button_w - 60.0).max(0.0);
            let n = titles.len().max(1);
            // 入る枚数(最小の幅で入らなくても、四角 1 つ分あれば 1 枚は細くして出す)。四角 1 つも
            // 入らない時は 0 枚にして、全部を「▾」の一覧に入れる(字 250 % の狭い帯。左端に
            // データパネルのボタンを足した時、前は 1 枚を出してコマンド検索の欄に重ねた)
            let fits = |room: f32| {
                ((room / (min_w + 2.0)).floor() as usize).max(usize::from(room >= h + 2.0))
            };
            let k = if fits(room) >= n {
                n
            } else {
                fits(room - button_w).min(n)
            };
            let tabs_room = if k < n { room - button_w } else { room };
            let w = if k == 0 {
                0.0
            } else {
                (tabs_room / k as f32 - 2.0)
                    .min(max_w)
                    .max(if k > 1 { min_w } else { h })
            };
            // 見せるタブ: 今のタブを含む k 枚(今のタブがなるべく真ん中)
            let start = self.win.doc_tab.saturating_sub(k / 2).min(n - k);
            let shown = start..start + k;
            let x_w = crate::theme::fs(16.0);
            // 引いて並べ替える: 引いている間は落とす先に縦の線を引き、
            // 離したら動かす。落とす先は見えているタブの中で、カーソルの x がどのタブの上かで決める
            let left = ui.cursor().left();
            let slot = |x: f32| {
                let j = ((x - left) / (w + 2.0)).floor().max(0.0) as usize;
                (start + j).min(start + k.max(1) - 1)
            };
            for (i, (name, modified)) in titles.iter().enumerate() {
                if !shown.contains(&i) {
                    continue;
                }
                let on = i == self.win.doc_tab && !self.home.start;
                let (rect, r) =
                    ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click_and_drag());
                let x_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.right() - 4.0 - x_w / 2.0, rect.center().y),
                    egui::vec2(x_w, x_w),
                );
                // 画面の台本・MCP がタブを名前で押せるように読み上げの名前を付ける
                r.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, name)
                });
                let over_x = r.hover_pos().is_some_and(|p| x_rect.contains(p));
                let fill = if on {
                    l.pal.chrome
                } else if r.hovered() {
                    l.pal.edge.gamma_multiply(0.35)
                } else {
                    l.pal.edge.gamma_multiply(0.15)
                };
                let p = ui.painter();
                p.rect_filled(rect, crate::theme::corner(5.0), fill);
                if on {
                    p.hline(
                        rect.x_range().shrink(4.0),
                        rect.bottom() - 1.0,
                        egui::Stroke::new(2.0, l.ws()),
                    );
                }
                let text = if *modified {
                    format!("• {name}")
                } else {
                    name.clone()
                };
                let galley = p.layout(
                    text,
                    crate::theme::prop(12.5),
                    if on { l.pal.ink } else { l.pal.ink2 },
                    (w - 18.0 - x_w).max(1.0),
                );
                // 1 行に収まらない名前は切る(全部はホバーで出す)
                let row = galley.rows.first().map_or(0.0, |r| r.rect().height());
                let clip = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 8.0, rect.top()),
                    egui::pos2(rect.right() - 6.0 - x_w, rect.bottom()),
                );
                p.with_clip_rect(clip).galley(
                    egui::pos2(rect.left() + 8.0, rect.center().y - row / 2.0),
                    galley,
                    l.pal.ink,
                );
                if on || r.hovered() {
                    if over_x {
                        p.rect_filled(x_rect, crate::theme::corner(3.0), l.pal.edge);
                    }
                    p.text(
                        x_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "×",
                        crate::theme::prop(13.0),
                        l.pal.ink2,
                    );
                }
                if r.clicked() {
                    if over_x {
                        close = Some(i);
                    } else {
                        switch = Some(i);
                    }
                }
                if r.middle_clicked() {
                    close = Some(i);
                }
                if r.dragged()
                    && let Some(x) = r.interact_pointer_pos().map(|p| p.x)
                {
                    let to = slot(x);
                    if to != i {
                        // 右へ動かす時は落とす先のタブの右の端、左へは左の端に線を引く
                        let at = left
                            + (to - start) as f32 * (w + 2.0)
                            + if to > i { w + 1.0 } else { -1.0 };
                        ui.painter()
                            .vline(at, rect.y_range(), egui::Stroke::new(2.0, l.ws()));
                    }
                }
                if r.drag_stopped()
                    && let Some(x) = r.interact_pointer_pos().map(|p| p.x)
                {
                    moved = Some((i, slot(x)));
                }
                let tip = match (&paths[i], *modified) {
                    (Some(p), false) => p.clone(),
                    (Some(p), true) => self.win.t_with(saved_mod, &[("path", p.as_str().into())]),
                    (None, false) => unsaved.clone(),
                    (None, true) => unsaved_mod.clone(),
                };
                r.on_hover_text(tip).right_menu(|ui| {
                    if crate::theme::menu_row(
                        ui,
                        "x",
                        &close_text,
                        &crate::keys::label("close_tab"),
                        true,
                    )
                    .clicked()
                    {
                        close = Some(i);
                        ui.close();
                    }
                    if crate::theme::menu_row(ui, "", &close_others_text, "", titles.len() > 1)
                        .clicked()
                    {
                        close_others = Some(i);
                        ui.close();
                    }
                });
            }
            if k < n {
                let r = crate::theme::icon_button(
                    ui,
                    "chev",
                    &self.win.t("frame-tab-more"),
                    true,
                    false,
                )
                .on_hover_text(
                    self.win
                        .t_with("frame-tab-more-tip", &[("count", (n - k).into())]),
                );
                egui::Popup::menu(&r).show(|ui| {
                    for (i, (name, modified)) in titles.iter().enumerate() {
                        let text = if *modified {
                            format!("• {name}")
                        } else {
                            name.clone()
                        };
                        if ui.selectable_label(i == self.win.doc_tab, text).clicked() {
                            switch = Some(i);
                            ui.close();
                        }
                    }
                });
            }
            if crate::theme::icon_button(ui, "plus", &self.win.t("frame-tab-new"), true, false)
                .on_hover_text(self.win.t("frame-tab-new-tip"))
                .clicked()
            {
                self.new_document();
            }
            // 残りは帯の空いた所: 引くと窓が動き、二度押しで最大化と戻す
            let rest = ui.available_size();
            let (_, r) = ui.allocate_exact_size(rest, egui::Sense::click_and_drag());
            self.title_drag(ui, &r);
        });
        if let Some((from, to)) = moved {
            self.move_tab(from, to);
        }
        if let Some(i) = switch {
            self.switch_tab(i);
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
        if let Some(keep) = close_others {
            self.close_other_tabs(keep);
        }
    }

    /// `keep` のほかのタブを閉じる(未保存なら 1 つずつ聞き、やめたらそこで止める)。
    pub fn close_other_tabs(&mut self, keep: usize) {
        let uid = self.win.doc_tabs.get(keep).map(|t| t.uid);
        while let Some(i) = self.win.doc_tabs.iter().position(|t| Some(t.uid) != uid) {
            if !self.close_tab(i) {
                break;
            }
        }
    }
}
