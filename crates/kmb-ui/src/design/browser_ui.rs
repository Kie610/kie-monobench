//! 左のブラウザ(試作の `.browser`): 設計では原点・パラメータ・スケッチ・ボディ、加工では機械と
//! セットアップの木(セットアップの下に加工の操作)。上に検索の欄と種類の札。
//! 並べ方は記述子(区分と、親を指す欄)から決め、アイコンも記述子の名前を使う。

use crate::theme::RightMenu as _;
use crate::timeline_ui::Health;
use crate::{Found, Space, Workspace, icons, theme};
use egui::{Color32, Rect, Sense, Stroke, Vec2};
use kmb_core::{FeatureEntry, FeatureId, FeatureStatus, category};

/// 種類の絞り込み(設計のまとまりの目でも使う)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    All,
    Sketch,
    Body,
    Params,
    Operation,
    Tools,
    Other,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::All => "すべて",
            Kind::Sketch => "スケッチ",
            Kind::Body => "ボディ",
            Kind::Params => "パラメータ",
            Kind::Operation => "操作",
            Kind::Tools => "工具",
            Kind::Other => "そのほか",
        }
    }

    fn chips(space: Space) -> &'static [Kind] {
        match space {
            Space::Design => &[Kind::All, Kind::Sketch, Kind::Body, Kind::Params],
            Space::Machining => &[Kind::All, Kind::Operation, Kind::Tools],
            Space::Render | Space::Animation => &[Kind::All, Kind::Body],
            Space::Simulation => &[Kind::All],
        }
    }
}

/// 木の 1 行の見た目。
struct Row<'a> {
    depth: usize,
    /// 開いているか(枝でなければ `None`)
    open: Option<bool>,
    icon: &'a str,
    name: &'a str,
    meta: &'a str,
    dot: Option<Color32>,
    /// 計算の具合。注意・エラーなら点の代わりに形の違う印を出す
    health: Option<Health>,
    selected: bool,
    weak: bool,
    /// 右端の目(3D のビューに出しているなら真。目がない行は `None`)
    eye: Option<bool>,
    /// 右に出す札(研究の「済み」「古い」「計算中 42%」と、その色)
    badge: Option<(&'a str, Color32)>,
    /// 目の左の丸いボタン(部品のアクティブ化。アクティブなら真。無い行は `None`)
    radio: Option<bool>,
}

/// 開け閉めも目も印もない、名前だけの行(名前付きの視点など)。
pub(crate) fn plain_row(ui: &mut egui::Ui, depth: usize, icon: &str, name: &str) -> egui::Response {
    row(
        ui,
        Row {
            depth,
            open: None,
            icon,
            name,
            meta: "",
            dot: None,
            health: None,
            selected: false,
            weak: false,
            eye: None,
            badge: None,
            radio: None,
        },
    )
    .0
}

/// 1 行を描く。(行の応答、開閉の印を押したか、目を押したか)を返す。
/// ブラウザの原点の下の平面(行の順。XY・YZ・ZX)。
pub(crate) const ORIGIN_PLANES: [kmb_core::Plane; 3] = [
    kmb_core::Plane::XY,
    kmb_core::Plane::YZ,
    kmb_core::Plane::ZX,
];

fn row(ui: &mut egui::Ui, r: Row<'_>) -> (egui::Response, bool, bool) {
    let (resp, twisty, eye, _) = row_radio(ui, r);
    (resp, twisty, eye)
}

/// [`row`] と同じだが、丸いボタンを押したかも返す。
fn row_radio(ui: &mut egui::Ui, r: Row<'_>) -> (egui::Response, bool, bool, bool) {
    let l = theme::look(ui.ctx());
    let w = ui.available_width();
    // 字とアイコンの倍率で行を広げる(1 倍なら 24)
    let b = theme::band();
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(w, 24.0 * b), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, r.selected, r.name)
    });
    // 目は行の上に重ねた別のボタン(押しても行は選ばない)
    let eye = r.eye.map(|shown| {
        let er = Rect::from_center_size(
            egui::pos2(rect.right() - 16.0 * b, rect.center().y),
            Vec2::new(22.0, 20.0) * b,
        );
        let name = if shown {
            format!("{} を隠す", r.name)
        } else {
            format!("{} を出す", r.name)
        };
        let resp = ui
            .interact(er, resp.id.with("eye"), Sense::click())
            .on_hover_text(if shown {
                "3D のビューで隠す(計算と加工には使う)"
            } else {
                "3D のビューに出す"
            });
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        (er, shown, resp)
    });
    // 丸いボタン(目の左。部品のアクティブ化。行に乗った時だけ出し、アクティブなら
    // いつも塗った丸を出す)
    let radio = r.radio.map(|on| {
        let cx = match &eye {
            Some((er, _, _)) => er.left() - 12.0 * b,
            None => rect.right() - 16.0 * b,
        };
        let rr = Rect::from_center_size(egui::pos2(cx, rect.center().y), Vec2::splat(20.0 * b));
        let label = if on {
            format!("{} はアクティブ", r.name)
        } else {
            format!("{} をアクティブにする", r.name)
        };
        let rresp = ui
            .interact(rr, resp.id.with("radio"), Sense::click())
            .on_hover_text(if on {
                "アクティブ: 新しく作るボディと部品はこの中に入る"
            } else {
                "アクティブにする: 新しく作るボディと部品をこの中に入れ、ほかの部品を淡く出す"
            });
        rresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        (rr, on, rresp)
    });
    let mut twisty = false;
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if r.selected {
            p.rect_filled(rect, 0.0, l.pal.sel);
        } else if resp.hovered() {
            p.rect_filled(rect, 0.0, l.pal.chrome2);
        }
        let mut x = rect.left() + 8.0 + 14.0 * b * r.depth as f32;
        let cy = rect.center().y;
        let tw = Rect::from_center_size(egui::pos2(x + 6.0 * b, cy), Vec2::splat(12.0 * b));
        match r.open {
            Some(open) => {
                p.circle_stroke(tw.center(), 5.5 * b, Stroke::new(1.0, l.pal.edge));
                let k = 2.2 * b;
                let c = tw.center();
                let pts = if open {
                    vec![
                        c + Vec2::new(-k, -k * 0.5),
                        c + Vec2::new(k, -k * 0.5),
                        c + Vec2::new(0.0, k * 0.8),
                    ]
                } else {
                    vec![
                        c + Vec2::new(-k * 0.5, -k),
                        c + Vec2::new(-k * 0.5, k),
                        c + Vec2::new(k * 0.8, 0.0),
                    ]
                };
                p.add(egui::Shape::convex_polygon(pts, l.pal.ink3, Stroke::NONE));
                if let Some(pos) = resp.interact_pointer_pos()
                    && resp.clicked()
                    && tw.expand(4.0).contains(pos)
                {
                    twisty = true;
                }
            }
            None => {
                p.hline(
                    egui::Rangef::new(tw.left() + 1.0, tw.right() - 1.0),
                    cy,
                    Stroke::new(1.0, l.pal.ink3),
                );
            }
        }
        x = tw.right() + 5.0;
        let ink = if r.weak { l.pal.ink3 } else { l.pal.ink };
        icons::paint_exact(
            p,
            Rect::from_center_size(
                egui::pos2(x + 7.0 * b, cy),
                Vec2::splat(theme::icon_px(14.0)),
            ),
            r.icon,
            if r.weak { l.pal.ink3 } else { l.pal.ink2 },
            1.6,
        );
        x += 19.0 * b;
        let mut right = rect.right() - 8.0;
        if let Some((er, shown, eresp)) = &eye {
            // 出している時は行に乗った時だけ濃く、隠している時はいつも閉じた目を出す
            if eresp.hovered() {
                p.rect_filled(*er, 3.0, l.pal.edge);
            }
            let c = if eresp.hovered() || !*shown || resp.hovered() {
                l.pal.ink2
            } else {
                l.pal.ink3
            };
            icons::paint_exact(
                p,
                Rect::from_center_size(er.center(), Vec2::splat(theme::icon_px(15.0))),
                if *shown { "eye" } else { "eyeoff" },
                c,
                1.5,
            );
            right = er.left() - 4.0;
        }
        if let Some((rr, on, rresp)) = &radio {
            let c = rr.center();
            if *on {
                p.circle_filled(c, 4.5 * b, l.pal.design);
                p.circle_stroke(c, 7.0 * b, Stroke::new(1.2, l.pal.design));
            } else if resp.hovered() || rresp.hovered() {
                let ink = if rresp.hovered() {
                    l.pal.ink
                } else {
                    l.pal.ink3
                };
                p.circle_stroke(c, 7.0 * b, Stroke::new(1.2, ink));
            }
            right = rr.left() - 4.0;
        }
        if let Some(h) = r.health.as_ref().filter(|h| h.is_problem()) {
            crate::timeline_ui::paint_health(p, egui::pos2(right - 5.0, cy), h, &l, false);
            right -= 16.0;
        } else if let Some(c) = r.dot {
            p.circle_filled(egui::pos2(right - 4.0 * b, cy), 4.0 * b, c);
            right -= 14.0 * b;
        }
        if let Some((text, c)) = r.badge.filter(|(t, _)| !t.is_empty()) {
            let g = p.layout_no_wrap(text.to_string(), theme::prop(10.5), c);
            let pad = egui::vec2(5.0, 1.5);
            let pill = Rect::from_min_size(
                egui::pos2(
                    right - g.size().x - 2.0 * pad.x,
                    cy - g.size().y / 2.0 - pad.y,
                ),
                g.size() + 2.0 * pad,
            );
            p.rect_filled(pill, 7.0, c.gamma_multiply(0.14));
            p.rect_stroke(
                pill,
                7.0,
                Stroke::new(1.0, c.gamma_multiply(0.55)),
                egui::StrokeKind::Inside,
            );
            p.galley(pill.min + pad, g, c);
            right = pill.left() - 6.0;
        }
        if !r.meta.is_empty() {
            let g = p.layout_no_wrap(r.meta.to_string(), theme::mono(11.0), l.pal.ink3);
            let gw = g.size().x;
            p.galley(egui::pos2(right - gw, cy - g.size().y / 2.0), g, l.pal.ink3);
            right -= gw + 8.0;
        }
        let mut job =
            egui::text::LayoutJob::simple_singleline(r.name.to_string(), theme::prop(12.5), ink);
        job.wrap.max_width = (right - x).max(10.0);
        job.wrap.max_rows = 1;
        let g = ui.fonts_mut(|f| f.layout_job(job));
        // 長い名前は … で切る。切った時は名前の全部をツールチップに出す(
        // 行の注意や状態の文があれば、egui はその下に重ねて出す)
        if g.elided {
            resp = resp.on_hover_text(r.name.to_string());
        }
        p.galley(egui::pos2(x, cy - g.size().y / 2.0), g, ink);
    }
    let eye_clicked = eye.is_some_and(|e| e.2.clicked());
    let radio_clicked = radio.is_some_and(|r| r.2.clicked());
    (resp, twisty, eye_clicked, radio_clicked)
}

/// ブラウザの部品の状態(検索・絞り込み・ドラッグで落とす所)。
pub(crate) struct BrowserPart {
    /// ブラウザの検索と種類の絞り込み
    pub(crate) query: String,
    pub(crate) kind: Kind,
    /// 次に描く時に、ブラウザのボディのまとまりを開く(「ブラウザ内を検索」)
    pub(crate) open_folder: bool,
    /// ブラウザの加工とセットアップの行(ドラッグで落とす所。描くたびに集め直す。画面の上の位置なので
    /// 文書ごとの状態に置かない)
    pub(crate) drop_rows: Vec<(FeatureId, egui::Rect)>,
    /// 部品の木のドラッグで落とす所(部品の番号、`None` は文書の直下。描くたびに集め直す)
    pub(crate) component_drop_rows: Vec<(Option<u64>, egui::Rect)>,
    /// 3D のビューで選んでいる・指している面のボディ(最初に作った機能)。行と親の行に印を付ける
    /// (描くたびに集め直す。選びは変えない)
    pub(crate) marked: Vec<FeatureId>,
}

/// 3D のビューで選んでいる・指している物のボディの行(と親の行)の印。行の左の縁の細い棒。
pub(crate) fn mark_row(ui: &egui::Ui, rect: egui::Rect) {
    let l = theme::look(ui.ctx());
    let bar = egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height()));
    ui.painter()
        .rect_filled(bar.shrink2(egui::vec2(0.0, 2.0)), 1.0, l.ws());
}

impl Workspace {
    fn kind_of(&self, f: &FeatureEntry, bodies: &[FeatureId]) -> Kind {
        // 記述子は写さずに区分だけ引く(行ごとに引くので、機能が多いと重い。10-24 v10)
        let cat = self.win.registry.category(&f.type_id).unwrap_or_default();
        match cat {
            category::SKETCH => Kind::Sketch,
            category::OPERATION | category::SETUP => Kind::Operation,
            category::TOOLS => Kind::Tools,
            _ if bodies.contains(&f.id) => Kind::Body,
            _ => Kind::Other,
        }
    }

    /// 機能のアイコン(記述子が名前を持たなければ区分から)。
    fn feature_icon(&self, f: &FeatureEntry) -> String {
        let d = self.win.registry.descriptor(&f.type_id);
        match d {
            Some(d) if !d.icon.is_empty() => d.icon,
            Some(d) => match d.category.as_str() {
                category::SKETCH => "sketch".into(),
                category::SETUP => "setup".into(),
                category::OPERATION => "path".into(),
                category::TOOLS => "tools".into(),
                _ => "box".into(),
            },
            None => "box".into(),
        }
    }

    /// 状態の点の色(問題がなければ、`always` の時だけ緑)。
    fn status_dot(&self, id: FeatureId, always: bool) -> Option<Color32> {
        let l = self.win.look;
        match self.cur.work.report().feature(id) {
            Some(FeatureStatus::Ok { outputs, .. }) => {
                if !kmb_core::warnings_of(outputs).is_empty() {
                    Some(l.pal.warn)
                } else if always {
                    Some(l.pal.ok)
                } else {
                    None
                }
            }
            Some(FeatureStatus::Error { .. }) | Some(FeatureStatus::Blocked { .. }) => {
                Some(l.pal.err)
            }
            Some(FeatureStatus::Suppressed) | Some(FeatureStatus::RolledBack) => Some(l.pal.ink3),
            None => None,
        }
    }

    pub(crate) fn browser_ui(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        // 見出し
        let (head, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 28.0 * theme::band()),
            Sense::hover(),
        );
        ui.painter().rect_filled(head, 0.0, l.pal.chrome);
        ui.painter().hline(
            head.x_range(),
            head.bottom() - 0.5,
            Stroke::new(1.0, l.pal.edge),
        );
        ui.painter().text(
            egui::pos2(head.left() + 10.0, head.center().y),
            egui::Align2::LEFT_CENTER,
            "ブラウザ",
            theme::prop(11.0),
            l.pal.ink2,
        );
        let x = Rect::from_center_size(
            egui::pos2(head.right() - 16.0, head.center().y),
            Vec2::splat(20.0),
        );
        let xr = ui
            .interact(x, ui.id().with("browser_close"), Sense::click())
            .on_hover_text("ブラウザを隠す(左の端の「ブラウザ」で戻す)");
        xr.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "ブラウザを隠す")
        });
        if xr.hovered() {
            ui.painter().rect_filled(x, 3.0, l.pal.chrome2);
        }
        icons::paint(ui.painter(), x.shrink(3.0), "chevl", l.pal.ink3, 1.6);
        if xr.clicked() {
            self.win.layout.browser_open = false;
        }
        // 検索と種類
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 5.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.browser.query)
                        .hint_text("名前で探す")
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 4)),
                );
                ui.horizontal_wrapped(|ui| {
                    let kinds = Kind::chips(self.win.layout.space);
                    if !kinds.contains(&self.browser.kind) {
                        self.browser.kind = Kind::All;
                    }
                    for k in kinds {
                        if theme::chip_toggle(ui, self.browser.kind == *k, k.label()).clicked() {
                            self.browser.kind = *k;
                        }
                    }
                });
            });
        let r = ui.available_rect_before_wrap();
        ui.painter()
            .hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, l.pal.edge));
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                match self.win.layout.space {
                    Space::Design | Space::Render | Space::Animation => {
                        self.design_tree(ui);
                        self.finish_component_drag(ui);
                    }
                    Space::Machining => self.setup_tree(ui),
                    Space::Simulation => self.study_tree(ui),
                }
            });
    }

    /// 検索と絞り込みに合う機能(タイムラインの順)。
    fn browser_items(&self) -> Vec<(FeatureEntry, Kind)> {
        let bodies: Vec<FeatureId> = self.visible_bodies().iter().map(|b| b.0).collect();
        let q = self.browser.query.trim().to_lowercase();
        self.cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .map(|f| (f.clone(), self.kind_of(f, &bodies)))
            .filter(|(f, k)| {
                (self.browser.kind == Kind::All || self.browser.kind == *k)
                    && (q.is_empty() || f.name.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// 開閉を覚える枝の行。開いているかを返す。
    /// `body` はボディの行の、ボディを最初に作った機能(`f` はボディを最後に変えた機能、名前はボディの名前)。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn browser_row(
        &mut self,
        ui: &mut egui::Ui,
        f: &FeatureEntry,
        depth: usize,
        meta: &str,
        open: Option<bool>,
        eye: bool,
        body: Option<FeatureId>,
    ) -> bool {
        let (_, detail) = self.status_line(f.id);
        let health = self.health(f.id);
        // ボディの行はボディのアイコン(最後の機能が穴でも穴のアイコンにしない)
        let icon = match body {
            Some(_) => "body".to_string(),
            None => self.feature_icon(f),
        };
        let cam = self.win.layout.space == Space::Machining;
        let dot = self.status_dot(f.id, cam && meta.starts_with('T'));
        // 目は行の目(ボディは最初に作った機能で覚える)、薄くするのはまとまりの目で隠れた時も
        let own_hidden = match body {
            Some(root) => self.is_body_hidden(root),
            None => self.is_own_hidden(f.id),
        };
        let hidden = self.is_hidden(f.id);
        // 研究は実行の様子を札で出す(未実行・計算中・古い・済み)
        let run = self.run_state(f.id);
        let badge = run.as_ref().map(|s| {
            (
                s.badge.clone(),
                crate::actions_ui::tone_color(s.tone, &self.win.look.pal),
            )
        });
        if run
            .as_ref()
            .is_some_and(|s| s.tone == kmb_core::RunTone::Busy)
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        let (r, tw, eye) = row(
            ui,
            Row {
                depth,
                open,
                icon: &icon,
                name: &f.name,
                meta,
                dot,
                health: Some(health.clone()),
                selected: self.selection().contains(&f.id),
                weak: f.suppressed || hidden,
                eye: eye.then_some(!own_hidden),
                badge: badge.as_ref().map(|(t, c)| (t.as_str(), *c)),
                radio: None,
            },
        );
        if body.is_some_and(|root| self.browser.marked.contains(&root)) {
            mark_row(ui, r.rect);
        }
        if eye {
            match body {
                Some(root) => self.set_body_hidden(root, !own_hidden),
                None => self.set_hidden(f.id, !own_hidden),
            }
        }
        // 「ブラウザ内を検索」で探したボディは、しばらく差し色の枠で光らせ、見える所まで送る。
        // AI が足した・直した機能(とそのボディ)の行も同じ枠で光らせる
        if let Some(glow) = body
            .and_then(|root| self.found_glow(ui.ctx(), Found::Browser(root)))
            .or_else(|| self.ai_glow(ui.ctx(), f.id, body))
        {
            let l = theme::look(ui.ctx());
            ui.painter().rect_stroke(
                r.rect.shrink(1.0),
                3.0,
                Stroke::new(2.0, l.ws().gamma_multiply(glow)),
                egui::StrokeKind::Inside,
            );
            if let Some(root) = body
                && self
                    .cur
                    .found_scroll
                    .take_if(|x| *x == Found::Browser(root))
                    .is_some()
            {
                r.scroll_to_me(Some(egui::Align::Center));
            }
        }
        // 加工の行はドラッグで並べ替える・別のセットアップへ移す(セットアップの行は落とす所)
        let r = if cam && self.is_operation(f.id) {
            r.interact(Sense::drag())
        } else {
            r
        };
        if cam {
            self.browser_drag_row(ui, f.id, &r);
        }
        // ボディの行は部品の行へ引いて移す(部品の木の並べ替え)
        let r = match body {
            Some(root) if !cam => {
                let r = r.interact(Sense::drag());
                self.body_drag_source(ui, root, &r);
                r
            }
            _ => r,
        };
        let r = if health.is_problem() {
            r.on_hover_text(crate::timeline_ui::health_tip(&f.name, &health))
        } else if detail.is_empty() {
            r
        } else {
            r.on_hover_text(detail)
        };
        if r.double_clicked() && !tw {
            // ボディは機能ではないので、ダブルクリックは名前の変更(最後の機能を開かない)
            match body {
                Some(root) => self.start_rename_body(root),
                None => self.open_edit(f.id),
            }
        } else if r.clicked() && !tw {
            let m = ui.input(|i| i.modifiers);
            if let Some(root) = body
                && self.pick_field_body_row(root, m.command)
            {
                // 作っている途中の機能を決めずに、受けている欄へ入れた
            } else if body.is_some() && !m.shift {
                self.select_body_row(f.id, m.command);
            } else if m.command {
                self.toggle_select(f.id);
            } else if m.shift {
                self.select_range(f.id);
            } else {
                self.select(f.id);
            }
        }
        // 右クリックした行を選ぶ(前に選んでいた物にメニューが効かないように)
        if r.secondary_clicked() && !self.selection().contains(&f.id) {
            self.select(f.id);
        }
        r.right_menu(|ui| {
            if !self.selection().contains(&f.id) {
                self.select(f.id);
            }
            // ボディの行はボディの操作だけ(最後の機能の操作は、タイムラインのその機能の右クリックで)
            match body {
                Some(root) => self.body_menu(ui, root, f.id),
                None => self.feature_menu(ui, f.id),
            }
        });
        tw
    }

    /// ボディの名前(付けた名前か「ボディN」。[`kmb_core::body_labels`])と、最初に作った機能・行。
    /// ボディを最初に作った機能のタイムラインの順に並べる(後から機能を足しても、ほかのボディを
    /// 結合で使っても名前は変わらない)。
    pub fn body_numbers(&self, list: &[FeatureEntry]) -> Vec<(String, FeatureId, FeatureEntry)> {
        let data = self.cur.work.doc().data();
        let reg = &self.win.registry;
        let ids: Vec<FeatureId> = list.iter().map(|f| f.id).collect();
        let labels = kmb_core::body_labels(data, reg, &ids);
        let mut v: Vec<(usize, String, FeatureId, FeatureEntry)> = list
            .iter()
            .zip(labels)
            .map(|(f, label)| {
                let r = kmb_core::body_root(data, reg, f.id);
                (data.index_of(r).unwrap_or(usize::MAX), label, r, f.clone())
            })
            .collect();
        v.sort_by_key(|(i, ..)| *i);
        v.into_iter().map(|(_, l, r, f)| (l, r, f)).collect()
    }

    fn design_tree(&mut self, ui: &mut egui::Ui) {
        let name = self
            .cur
            .work
            .path()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().to_string())
            .or_else(|| self.cur.doc_label.clone())
            .unwrap_or_else(|| "(無題)".to_string());
        let items = self.browser_items();
        let filtering = !self.browser.query.trim().is_empty() || self.browser.kind != Kind::All;
        self.browser.marked = self.marked_bodies();
        // 「ブラウザ内を検索」: 文書とボディのまとまりを開いておく
        if std::mem::take(&mut self.browser.open_folder) {
            for key in ["doc", "ボディ"] {
                let id = ui.make_persistent_id(("browser_open", key));
                ui.data_mut(|d| d.insert_temp(id, true));
            }
        }
        // 文書の行の右クリックで設計の属性
        let part = self.cur.work.doc().data().properties.part_number.clone();
        // 部品があれば、文書の行にも丸(押すと文書の直下をアクティブに戻す)
        let has_parts = !self.cur.work.doc().data().components.is_empty();
        let root_active = self.active_component().is_none();
        let (open, r, _, radio) = folder_row_radio(
            ui,
            "doc",
            (0, "box", &name, &part),
            true,
            None,
            None,
            has_parts.then_some(root_active),
        );
        if radio {
            self.activate_component(None);
        }
        // 部品の木のドラッグ: 文書の行へ落とすと直下へ
        self.browser.component_drop_rows.push((None, r.rect));
        r.right_menu(|ui| {
            if has_parts
                && !root_active
                && crate::theme::menu_row(ui, "component", "アクティブにする", "", true)
                    .on_hover_text("文書の直下をアクティブに戻す(新しく作るボディは部品に入らない)")
                    .clicked()
            {
                self.activate_component(None);
                ui.close();
            }
            ui.separator();
            if crate::theme::menu_row(ui, "table", "設計の属性…", "", true).clicked() {
                self.open_properties();
                ui.close();
            }
        });
        if !open {
            return;
        }
        if self.browser.kind == Kind::All && self.browser.query.trim().is_empty() {
            // 原点の目: 原点の平面をいつも出す(既定は隠す。空の文書と軸を選ぶ間は目と関係なく出す)
            let shown = self.is_origin_shown();
            let (open, _, eye) = folder_row(
                ui,
                "origin",
                1,
                "origin",
                "原点",
                "",
                false,
                None,
                Some(shown),
            );
            if eye {
                self.set_origin_shown(!shown);
            }
            if open {
                self.origin_rows(ui);
            }
            // 名前を付けた視点
            let n = self.cur.work.doc().data().named_views.len().to_string();
            let (open, r) = branch_row(
                ui,
                "named_views",
                1,
                "folder",
                "名前の付いたビュー",
                &n,
                false,
                None,
            );
            // 開け閉めの印はブラウザの中の ID(メニューの中で作ると別の ID になる)
            let open_key = ui.make_persistent_id(("browser_open", "named_views"));
            r.right_menu(|ui| {
                if crate::theme::menu_row(ui, "camera", "今のビューを保存", "", true).clicked()
                {
                    if let Some(id) = self.save_named_view("") {
                        // すぐ名前を打てるようにする
                        let name = self
                            .cur
                            .work
                            .doc()
                            .data()
                            .named_view(id)
                            .map(|v| v.name.clone())
                            .unwrap_or_default();
                        self.cur.renaming_view = Some((id, name));
                        ui.data_mut(|d| d.insert_temp(open_key, true));
                    }
                    ui.close();
                }
            });
            if open {
                self.named_view_rows(ui);
            }
            // モーション スタディ。作った時だけ出す
            let n = self.motion_studies().len();
            if n > 0 {
                let (open, _) = branch_row(
                    ui,
                    "motion_studies",
                    1,
                    "folder",
                    "モーション スタディ",
                    &n.to_string(),
                    false,
                    None,
                );
                if open {
                    self.motion_study_rows(ui);
                }
            }
            // 接触セット。目で全体を入れる・切る
            let contacts = kmb_interchange::contact::of(self.cur.work.doc().data());
            if !contacts.sets.is_empty() {
                let (open, _, eye) = folder_row(
                    ui,
                    "contact_sets",
                    1,
                    "folder",
                    "接触セット",
                    &contacts.sets.len().to_string(),
                    false,
                    None,
                    Some(contacts.enabled),
                );
                if eye {
                    let mut c = contacts.clone();
                    c.enabled = !c.enabled;
                    self.set_contacts(
                        if c.enabled {
                            "接触セットを有効にする"
                        } else {
                            "接触セットを無効にする"
                        },
                        c,
                    );
                } else if open {
                    self.contact_set_rows(ui, &contacts);
                }
            }
        }
        if matches!(self.browser.kind, Kind::All | Kind::Params) {
            let n = self.cur.work.doc().data().params.len().to_string();
            let (r, _, _) = row(
                ui,
                Row {
                    depth: 1,
                    open: None,
                    icon: "param",
                    name: "パラメータ",
                    meta: &n,
                    dot: None,
                    health: None,
                    selected: self.win.show_params,
                    weak: false,
                    eye: None,
                    badge: None,
                    radio: None,
                },
            );
            if r.on_hover_text("Ctrl+P。右の編集パネルに一覧を出す")
                .clicked()
            {
                self.win.show_params = !self.win.show_params;
            }
        }
        for (title, kind) in [
            ("スケッチ", Kind::Sketch),
            ("ボディ", Kind::Body),
            ("そのほかの機能", Kind::Other),
        ] {
            let list: Vec<FeatureEntry> = items
                .iter()
                .filter(|i| i.1 == kind)
                .map(|i| i.0.clone())
                .collect();
            if list.is_empty() && (filtering || kind == Kind::Other) {
                continue;
            }
            let n = if list.is_empty() {
                "0".to_string()
            } else {
                String::new()
            };
            let worst = crate::timeline_ui::worst_health(
                list.iter()
                    .map(|f| Health::of(self.cur.work.report().feature(f.id))),
            );
            // スケッチとボディのまとまりには、中をまとめて隠す目(中の行の目はそのまま残す)。
            // そのほかの機能(途中の形)は 3D のビューに出ないので目を付けない
            let folder_eye = matches!(kind, Kind::Sketch | Kind::Body);
            let folder_shown = !self.is_folder_hidden(kind);
            let (open, folder, eye_clicked) = folder_row(
                ui,
                title,
                1,
                "folder",
                title,
                &n,
                kind != Kind::Other || filtering,
                worst,
                folder_eye.then_some(folder_shown),
            );
            if kind == Kind::Body {
                // ボディを「ボディ」へ落とすと部品から外す
                self.browser.component_drop_rows.push((None, folder.rect));
                let data = self.cur.work.doc().data();
                if self
                    .browser
                    .marked
                    .iter()
                    .any(|r| data.component_of(*r).is_none())
                {
                    mark_row(ui, folder.rect);
                }
            }
            if eye_clicked {
                self.set_folder_hidden(kind, folder_shown);
            }
            if open {
                // ボディとスケッチの行には、3D のビューで隠す・出す目を付ける
                let eye = matches!(kind, Kind::Sketch | Kind::Body);
                if kind == Kind::Body {
                    // ボディはボディの名前(付けた名前か「ボディ1」のように作った順の名前)だけで出す。
                    // 最後の機能の名前は添えない。
                    // 部品に入っているボディは部品の行の下に出す(探している間はここにも出す)
                    for (label, root, f) in self.body_numbers(&list) {
                        if !filtering && self.cur.work.doc().data().component_of(root).is_some() {
                            continue;
                        }
                        let shown = FeatureEntry { name: label, ..f };
                        self.browser_row(ui, &shown, 2, "", None, eye, Some(root));
                    }
                } else {
                    for f in &list {
                        self.browser_row(ui, f, 2, "", None, eye, None);
                    }
                }
            }
        }
        // 部品の木(ボディのまとまりの後に文書の直下の部品を並べる)
        if !filtering && !self.cur.work.doc().data().components.is_empty() {
            let bodies: Vec<FeatureEntry> = items
                .iter()
                .filter(|i| i.1 == Kind::Body)
                .map(|i| i.0.clone())
                .collect();
            let numbered = self.body_numbers(&bodies);
            self.component_tree_ui(ui, &numbered);
        }
        let ops = items.iter().filter(|i| i.1 == Kind::Operation).count();
        if ops > 0 && !filtering {
            ui.add_space(6.0);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(10, 0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "加工の操作 {ops} 個は作業空間「加工」で出します"
                        ))
                        .size(theme::fs(11.0))
                        .color(self.win.look.pal.ink3),
                    );
                });
        }
    }

    /// 原点の下の行(平面 3 つと軸 3 つ)。原点の目を閉じている間は薄く出す。
    /// 平面の行は押すと選び(正面から見るの向き)、もう一度押すと外す。
    fn origin_rows(&mut self, ui: &mut egui::Ui) {
        let shown = self.is_origin_shown();
        for (i, p) in ["XY 平面", "YZ 平面", "ZX 平面", "X 軸", "Y 軸", "Z 軸"]
            .into_iter()
            .enumerate()
        {
            let plane = i < ORIGIN_PLANES.len();
            let (resp, _, _) = row(
                ui,
                Row {
                    depth: 2,
                    open: None,
                    icon: if plane { "rect" } else { "line" },
                    name: p,
                    meta: "",
                    dot: None,
                    health: None,
                    selected: plane && self.cur.origin_pick == Some(i),
                    weak: !shown,
                    eye: None,
                    badge: None,
                    radio: None,
                },
            );
            if plane && resp.clicked() {
                let again = self.cur.origin_pick == Some(i);
                self.clear_selection();
                self.cur.picked = None;
                self.cur.also_picked.clear();
                self.cur.origin_pick = (!again).then_some(i);
            }
        }
    }

    /// 加工: 機械、セットアップの下にそれを指す操作(記述子の `parent` の欄で親を決める)、工具。
    /// 解析の作業空間の木: 研究の一覧。
    fn study_tree(&mut self, ui: &mut egui::Ui) {
        let items = self.browser_items();
        let studies: Vec<FeatureEntry> = items
            .into_iter()
            .map(|i| i.0)
            .filter(|f| self.win.registry.is(&f.type_id, category::STUDY))
            .collect();
        let (open, _) = branch_row(ui, "studies", 0, "folder", "研究", "", true, None);
        if !open {
            return;
        }
        if studies.is_empty() {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(30, 4))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "まだありません。上の「静的応力」「熱」「固有振動」「軽い形の参考」「翼の揚力」「2D の流れ」「3D の流れ」から足す",
                        )
                        .size(theme::fs(11.0))
                        .color(self.win.look.pal.ink3),
                    );
                });
        }
        for s in &studies {
            self.browser_row(ui, s, 1, "", None, false, None);
        }
    }

    fn setup_tree(&mut self, ui: &mut egui::Ui) {
        let items = self.browser_items();
        let data = self.cur.work.doc().data().clone();
        let registry = self.win.registry.clone();
        let parent_of = |f: &FeatureEntry| -> Option<FeatureId> {
            let d = registry.descriptor(&f.type_id)?;
            let key = d.parent?;
            f.data.get(&key)?.as_u64().map(FeatureId)
        };
        let is_cat = |f: &FeatureEntry, c: &str| {
            registry
                .descriptor(&f.type_id)
                .is_some_and(|d| d.category == c)
        };
        if self.browser.kind == Kind::All && self.browser.query.trim().is_empty() {
            let machine = self.machine_name();
            let (r, _, _) = row(
                ui,
                Row {
                    depth: 0,
                    open: None,
                    icon: "box",
                    name: "機械",
                    meta: &machine,
                    dot: None,
                    health: None,
                    selected: false,
                    weak: false,
                    eye: None,
                    badge: None,
                    radio: None,
                },
            );
            let r = r.on_hover_text("機械の値は設定の「機械」で直す");
            if r.clicked() {
                self.toggle_machine_settings();
            }
            r.right_menu(|ui| {
                if crate::theme::menu_row(ui, "machine", "機械の設定", "", true).clicked() {
                    self.open_machine_settings();
                    ui.close();
                }
            });
        }
        let setups: Vec<FeatureEntry> = data
            .features
            .iter()
            .filter(|f| is_cat(f, category::SETUP))
            .cloned()
            .collect();
        if matches!(self.browser.kind, Kind::All | Kind::Operation) {
            let (open, r) = branch_row(ui, "setups", 0, "folder", "セットアップ", "", true, None);
            // 右クリックで、リボンの「セットアップ」のまとまりの操作(セットアップを足すなど)を出す
            // (Z4: 右クリックしても何も出なかった)
            r.right_menu(|ui| {
                let acts: Vec<kmb_core::Action> = self
                    .actions()
                    .into_iter()
                    .filter(|a| a.menu == "セットアップ" && a.is_at(kmb_core::Place::Toolbar))
                    .collect();
                if acts.is_empty() {
                    ui.weak("使える操作はありません");
                }
                for a in acts {
                    let label = if a.label == "セットアップ" {
                        "新しいセットアップ".to_string()
                    } else {
                        a.label.clone()
                    };
                    let b = crate::theme::menu_row(ui, &a.icon, &label, "", a.disabled.is_none());
                    let b = match &a.disabled {
                        Some(w) => b.on_disabled_hover_text(w),
                        None => b.on_hover_text(&a.hint),
                    };
                    if b.clicked() {
                        self.start_action(&a.id);
                        ui.close();
                    }
                }
            });
            if open {
                if setups.is_empty() {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(30, 4))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("まだありません。上の「セットアップ」から足す")
                                    .size(theme::fs(11.0))
                                    .color(self.win.look.pal.ink3),
                            );
                        });
                }
                for s in &setups {
                    let children: Vec<FeatureEntry> = items
                        .iter()
                        .map(|i| i.0.clone())
                        .filter(|f| parent_of(f) == Some(s.id))
                        .collect();
                    let id = ui.make_persistent_id(("browser_open", "setup", s.id));
                    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
                    let material = s
                        .data
                        .pointer("/stock/material")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if self.browser_row(ui, s, 1, &material, Some(open), false, None) {
                        ui.data_mut(|d| d.insert_temp(id, !open));
                    }
                    if open {
                        let n = s
                            .data
                            .get("fixtures")
                            .and_then(|v| v.as_array())
                            .map_or(0, |a| a.len());
                        if n > 0 {
                            row(
                                ui,
                                Row {
                                    depth: 2,
                                    open: None,
                                    icon: "lock",
                                    name: "固定具",
                                    meta: &format!("クランプ {n}"),
                                    dot: None,
                                    health: None,
                                    selected: false,
                                    weak: false,
                                    eye: None,
                                    badge: None,
                                    radio: None,
                                },
                            );
                        }
                        for f in &children {
                            let meta = tool_meta(f);
                            self.browser_row(ui, f, 2, &meta, None, false, None);
                        }
                    }
                }
                // 親のない加工の操作
                let orphans: Vec<FeatureEntry> = items
                    .iter()
                    .map(|i| i.0.clone())
                    .filter(|f| {
                        is_cat(f, category::OPERATION)
                            && parent_of(f).is_none_or(|p| data.feature(p).is_none())
                    })
                    .collect();
                if !orphans.is_empty() {
                    let open = branch(
                        ui,
                        "orphans",
                        1,
                        "folder",
                        "セットアップのない操作",
                        "",
                        true,
                    );
                    if open {
                        for f in &orphans {
                            let meta = tool_meta(f);
                            self.browser_row(ui, f, 2, &meta, None, false, None);
                        }
                    }
                }
            }
        }
        self.finish_browser_drag(ui);
        let tools: Vec<FeatureEntry> = items
            .iter()
            .map(|i| i.0.clone())
            .filter(|f| is_cat(f, category::TOOLS))
            .collect();
        if !tools.is_empty() {
            let n = tools.len().to_string();
            let open = branch(ui, "tools", 0, "tools", "この文書の工具", &n, true);
            if open {
                for f in &tools {
                    self.browser_row(ui, f, 1, "", None, false, None);
                }
            }
        }
    }
}

/// 加工の操作の工具番号(「T5」、残り削りがあれば前の工具から「T2→T5」、前の工具を直径で打てば
/// 「Ø3.175 mm→T5」。パネルの工具の順番と同じ向き)。
pub(crate) fn tool_meta(f: &FeatureEntry) -> String {
    let t = f.data.get("tool").and_then(|v| v.as_u64());
    let rest = f.data.get("rest_tool").and_then(|v| v.as_u64());
    let size = f.data.get("rest_diameter").and_then(|v| v.as_str());
    match (t, rest, size) {
        (Some(now), Some(prev), _) => format!("T{prev}→T{now}"),
        (Some(now), None, Some(d)) => format!("Ø{d}→T{now}"),
        (Some(a), None, None) => format!("T{a}"),
        _ => String::new(),
    }
}

/// 開け閉めできる行(畳んだ状態は egui の記憶に置く)。開いていれば真。
fn branch(
    ui: &mut egui::Ui,
    key: impl std::hash::Hash + std::fmt::Debug,
    depth: usize,
    icon: &str,
    name: &str,
    meta: &str,
    default_open: bool,
) -> bool {
    folder_row(ui, key, depth, icon, name, meta, default_open, None, None).0
}

/// 枝の行(開いているかと、右クリックのメニューを付けるための応答)。
#[allow(clippy::too_many_arguments)]
fn branch_row(
    ui: &mut egui::Ui,
    key: impl std::hash::Hash + std::fmt::Debug,
    depth: usize,
    icon: &str,
    name: &str,
    meta: &str,
    default_open: bool,
    worst: Option<Health>,
) -> (bool, egui::Response) {
    let (open, r, _) = folder_row(ui, key, depth, icon, name, meta, default_open, worst, None);
    (open, r)
}

/// 枝の行。閉じている時は、中のいちばん重い問題(`worst`)の印を出す(開けば中の行に出る)。
/// `eye` があれば右端に目を出す(中をまとめて隠す・出す。出しているなら真)。
/// (開いているか、応答、目を押したか)を返す。目を押しても開け閉めしない。
#[allow(clippy::too_many_arguments)]
pub(crate) fn folder_row(
    ui: &mut egui::Ui,
    key: impl std::hash::Hash + std::fmt::Debug,
    depth: usize,
    icon: &str,
    name: &str,
    meta: &str,
    default_open: bool,
    worst: Option<Health>,
    eye: Option<bool>,
) -> (bool, egui::Response, bool) {
    let (open, r, eye, _) = folder_row_radio(
        ui,
        key,
        (depth, icon, name, meta),
        default_open,
        worst,
        eye,
        None,
    );
    (open, r, eye)
}

/// [`folder_row`] に、目の左の丸いボタン `radio`(部品のアクティブ化。アクティブなら真)を足した物。
/// (開いているか、応答、目を押したか、丸を押したか)を返す。丸を押しても開け閉めしない。
pub(crate) fn folder_row_radio(
    ui: &mut egui::Ui,
    key: impl std::hash::Hash + std::fmt::Debug,
    (depth, icon, name, meta): (usize, &str, &str, &str),
    default_open: bool,
    worst: Option<Health>,
    eye: Option<bool>,
    radio: Option<bool>,
) -> (bool, egui::Response, bool, bool) {
    let id = ui.make_persistent_id(("browser_open", key));
    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(default_open);
    let (r, tw, eye_clicked, radio_clicked) = row_radio(
        ui,
        Row {
            depth,
            open: Some(open),
            icon,
            name,
            meta,
            dot: None,
            health: if open { None } else { worst.clone() },
            selected: false,
            weak: eye == Some(false),
            eye,
            badge: None,
            radio,
        },
    );
    let r = match worst.filter(|_| !open) {
        Some(h) => r.on_hover_text(format!("中に{}があります(開くと見える)", h.title())),
        None => r,
    };
    if !eye_clicked && !radio_clicked && (tw || r.double_clicked() || r.clicked()) {
        ui.data_mut(|d| d.insert_temp(id, !open));
        return (!open, r, false, false);
    }
    (open, r, eye_clicked, radio_clicked)
}

impl Workspace {
    /// 接触セットを置き換える(1 回の取り消し単位)。
    fn set_contacts(&mut self, label: &str, contacts: kmb_interchange::contact::Contacts) -> bool {
        log::info!("{label}");
        match kmb_session::edit::set_section(self.cur.work.doc().data(), label, &contacts) {
            Ok(tx) => self.apply(tx),
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    /// ブラウザの「接触セット」の中の行。目でセットを入れる・切る、右クリックで消す。
    fn contact_set_rows(
        &mut self,
        ui: &mut egui::Ui,
        contacts: &kmb_interchange::contact::Contacts,
    ) {
        let names: Vec<String> = contacts
            .sets
            .iter()
            .map(|s| {
                s.bodies
                    .iter()
                    .map(|b| self.body_name_of(*b))
                    .collect::<Vec<_>>()
                    .join("・")
            })
            .collect();
        for (i, s) in contacts.sets.iter().enumerate() {
            let (r, _, eye) = row(
                ui,
                Row {
                    depth: 2,
                    open: None,
                    icon: "interference",
                    name: &s.name,
                    meta: "",
                    dot: None,
                    health: None,
                    selected: false,
                    weak: !s.enabled || !contacts.enabled,
                    eye: Some(s.enabled),
                    badge: None,
                    radio: None,
                },
            );
            let r = r.on_hover_text(format!("ぶつかり合うボディ: {}", names[i]));
            if eye {
                let mut c = contacts.clone();
                c.sets[i].enabled = !s.enabled;
                let label = format!(
                    "接触セット「{}」を{}",
                    s.name,
                    if c.sets[i].enabled {
                        "使う"
                    } else {
                        "使わない"
                    }
                );
                self.set_contacts(&label, c);
            }
            r.right_menu(|ui| {
                if crate::theme::menu_row(ui, "trash", "削除", "", true).clicked() {
                    let mut c = contacts.clone();
                    c.sets.remove(i);
                    self.set_contacts(&format!("接触セット「{}」を消す", s.name), c);
                    ui.close();
                }
            });
        }
    }
}
