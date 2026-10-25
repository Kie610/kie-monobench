//! 測る道具の画面。M・「検査」タブ・右クリックで開き、3D のビューで点(頂点・円の中心)・辺・面を
//! クリックで選ぶ。1 つなら座標・長さ・径・面積、2 つなら最短の距離・ΔX/ΔY/ΔZ・角度を
//! ビューの右上の小さな窓に出し、ビューには最短の 2 点を結ぶ寸法の線を描く。
//! 何を測るかは選んだ物の組み合わせで決める(先に測る種類を選ばせない)。
//! 円・穴(円柱の面)が入っていれば中心か軸から測り、窓で最短・最長に切り替えられる。
//! 長穴の端(半円)を 1 つ選ぶと、向かい合う端を探して幅・中心間・全長を出す。

use crate::Workspace;
use crate::measure::{self, Basis, Report, Target};
use crate::view3d::{self, Pick};
use egui::{Color32, Pos2, Rect};

/// 測る道具の状態。
#[derive(Default)]
pub struct Measure {
    /// 選んだ物(2 つまで。3 つ目をクリックすると、それを 1 つ目にして測り直す)
    pub picks: Vec<Target>,
    /// マウスの下の物
    pub hover: Option<Target>,
    /// 選んだ物の値
    pub(crate) report: Report,
    /// 選んだ時の網の番号(網が変わったら選び直す)
    generation: u64,
    /// 「写した」と出す時刻(秒)
    copied: Option<f64>,
    /// 丸い物が入っている時の測り方(中心・最短・最長)。選び直しても覚えておく
    basis: Basis,
    /// 値を書いた時の文書の長さの単位(変わったら書き直す。候補 19)
    unit: kmb_core::LengthUnit,
}

impl Workspace {
    /// 測る道具を開いているか。
    pub fn is_measuring(&self) -> bool {
        self.cur.measure.is_some()
    }

    /// 測る道具を開く・閉じる。開く時に選んでいた面は、そのまま測る物にする。
    pub fn toggle_measure(&mut self) {
        if self.cur.measure.take().is_some() {
            return;
        }
        self.cur.gesture = None;
        self.cur.review = None;
        let picks: Vec<Target> = self
            .picked_faces()
            .into_iter()
            .take(2)
            .map(Target::Face)
            .collect();
        self.cur.measure = Some(Measure {
            picks,
            generation: self.cur.scene.generation(),
            ..Default::default()
        });
        self.measure_refresh();
    }

    /// 選んだ物の値を測り直す。
    pub(crate) fn measure_refresh(&mut self) {
        let generation = self.cur.scene.generation();
        // 面を 1 つ選んだ時は、そのボディの体積・表面積・外形・重心と、材料があれば質量も出す
        // (状態の帯より見やすい所に)
        let face = match self.cur.measure.as_ref().map(|m| m.picks.as_slice()) {
            Some([Target::Face(p)])
                if self
                    .cur
                    .measure
                    .as_ref()
                    .is_some_and(|m| m.generation == generation) =>
            {
                Some(p.feature)
            }
            _ => None,
        };
        let body = face.and_then(|feature| {
            let props = self
                .cur
                .scene
                .key()
                .iter()
                .find(|(f, _)| *f == feature)
                .and_then(|(_, h)| self.cur.work.kernel().mass(*h).ok())?;
            Some(measure::BodyFacts {
                props,
                material: kmb_session::body_material(
                    self.cur.work.doc().data(),
                    self.body_root(feature),
                ),
            })
        });
        let Some(m) = &mut self.cur.measure else {
            return;
        };
        if m.generation != generation {
            m.picks.clear();
            m.hover = None;
            m.generation = generation;
        }
        m.unit = kmb_core::display_unit();
        m.report = measure::measure(
            &self.cur.scene,
            &m.picks,
            m.basis,
            self.win.layout.measure_xyz,
            body,
        );
    }

    /// 3D のビューの出来事を、測る物の選びに直す。
    pub(crate) fn measure_events(&mut self, ev: &view3d::ViewEvents, rect: Rect) {
        if self.cur.measure.as_ref().is_some_and(|m| {
            m.generation != self.cur.scene.generation() || m.unit != kmb_core::display_unit()
        }) {
            self.measure_refresh();
        }
        let see_through = !self.win.layout.shading.faces();
        let hit = |ws: &Self, p: Pos2| {
            view3d::measure_hit(&ws.cur.scene, &ws.cur.camera, rect, p, see_through)
        };
        let hover = ev.hover.and_then(|p| hit(self, p));
        let click = ev.click.map(|p| hit(self, p));
        let Some(m) = &mut self.cur.measure else {
            return;
        };
        m.hover = hover;
        let Some(click) = click else { return };
        match click {
            // 選んだ物をもう一度押すと外す
            Some(t) if m.picks.contains(&t) => m.picks.retain(|x| *x != t),
            Some(t) if m.picks.len() >= 2 => m.picks = vec![t],
            Some(t) => m.picks.push(t),
            None => m.picks.clear(),
        }
        self.measure_refresh();
    }

    /// 測る物のうち、色を変える面。
    pub(crate) fn measure_faces(&self) -> Vec<Pick> {
        let Some(m) = &self.cur.measure else {
            return vec![];
        };
        m.picks
            .iter()
            .filter_map(|t| match t {
                Target::Face(p) => Some(p.clone()),
                _ => None,
            })
            .collect()
    }

    /// マウスの下が面でない時は、ビューに面の色付けをさせない(点や辺を拾う時に面が光らないよう)。
    pub(crate) fn measure_hovers_face(&self) -> bool {
        self.cur
            .measure
            .as_ref()
            .is_some_and(|m| matches!(m.hover, Some(Target::Face(_)) | None))
    }

    /// 選んだ物の名前(窓とマウスの横に出す)。
    fn target_label(&self, t: &Target) -> String {
        let name = |f: kmb_core::FeatureId| {
            self.cur
                .work
                .doc()
                .data()
                .feature(f)
                .map_or(String::new(), |e| format!("({})", e.name))
        };
        match t {
            Target::Vertex { feature, .. } => format!("点{}", name(*feature)),
            Target::Center { feature, .. } => format!("円の中心{}", name(*feature)),
            Target::Edge { feature, .. } => {
                let kind = match measure::resolve(&self.cur.scene, t).map(|g| g.kind) {
                    Some(measure::Kind::Line { .. }) => "辺(直線)".to_string(),
                    Some(measure::Kind::Circle(c)) if c.closed => {
                        format!("辺(円 Ø{:.3})", 2.0 * c.radius)
                    }
                    Some(measure::Kind::Circle(c)) => format!("辺(円弧 R{:.3})", c.radius),
                    _ => "辺".to_string(),
                };
                format!("{kind}{}", name(*feature))
            }
            Target::Face(p) => {
                let label = self.face_label(&p.face);
                match measure::resolve(&self.cur.scene, t).map(|g| g.kind) {
                    Some(measure::Kind::Cylinder(c)) if c.closed => {
                        format!("{label} 円柱 Ø{:.3}", 2.0 * c.radius)
                    }
                    Some(measure::Kind::Cylinder(c)) => {
                        format!("{label} 円柱の一部 R{:.3}", c.radius)
                    }
                    _ => label,
                }
            }
        }
    }

    /// 選んだ物・マウスの下の物・寸法の線を、ビューに重ねて描く。
    pub(crate) fn draw_measure(&self, ui: &egui::Ui, rect: Rect) {
        let Some(m) = &self.cur.measure else { return };
        let painter = ui.painter_at(rect);
        let color = self.win.look.ws();
        let soft = crate::theme::mix(self.win.look.pal.panel, color, 0.6);
        let to = |p: [f64; 3]| self.cur.camera.project(p.map(|v| v as f32), rect).0;
        let mark = |t: &Target, c: Color32, width: f32| match t {
            Target::Vertex { .. } | Target::Center { .. } => {
                if let Some(measure::Kind::Point(p)) =
                    measure::resolve(&self.cur.scene, t).map(|g| g.kind)
                {
                    painter.circle_stroke(to(p), 5.0, egui::Stroke::new(width, c));
                    painter.circle_filled(to(p), 2.0, c);
                }
            }
            Target::Edge { .. } => {
                if let Some(g) = measure::resolve(&self.cur.scene, t) {
                    for s in &g.segs {
                        let f32s = |p: [f64; 3]| p.map(|v| v as f32);
                        if let Some((a, b)) = self.cur.camera.segment(f32s(s[0]), f32s(s[1]), rect)
                        {
                            painter.line_segment([a, b], egui::Stroke::new(width, c));
                        }
                    }
                }
            }
            // 面はビューが塗る。円柱の面は、測る元の軸を破線で
            Target::Face(_) => {
                if let Some(measure::Kind::Cylinder(cy)) =
                    measure::resolve(&self.cur.scene, t).map(|g| g.kind)
                {
                    let h = cy.length / 2.0 + cy.radius;
                    let end = |s: f64| to(std::array::from_fn(|i| cy.center[i] + cy.axis[i] * s));
                    painter.add(egui::Shape::dashed_line(
                        &[end(-h), end(h)],
                        egui::Stroke::new(width * 0.6, c),
                        8.0,
                        3.0,
                    ));
                }
            }
        };
        for t in &m.picks {
            mark(t, color, 3.0);
        }
        if let Some(h) = m.hover.as_ref().filter(|h| !m.picks.contains(h)) {
            mark(h, soft, 2.0);
        }
        // X・Y・Z ごとの成分: 2 点の間を軸に沿って 3 本の線で結び、それぞれの長さを、
        // 線で囲まれた所の外側に添える(真ん中の値の札と重ならないよう、札は反対側に出す)
        let mut away: Option<egui::Vec2> = None;
        if let (Some((a, b)), true) = (
            m.report.line,
            self.win.layout.measure_xyz && m.picks.len() == 2,
        ) {
            let mut corners = vec![a];
            for k in 0..3 {
                let mut next = corners[corners.len() - 1];
                next[k] = b[k];
                corners.push(next);
            }
            let screen: Vec<Pos2> = corners.iter().map(|p| to(*p)).collect();
            let centroid =
                screen.iter().fold(egui::Vec2::ZERO, |s, p| s + p.to_vec2()) / screen.len() as f32;
            let centroid = centroid.to_pos2();
            for k in 0..3 {
                let d = b[k] - a[k];
                if d.abs() < 5e-4 {
                    continue;
                }
                let (p0, p1) = (screen[k], screen[k + 1]);
                let ink = self.win.look.pal.inks.axis[k];
                painter.line_segment([p0, p1], egui::Stroke::new(1.4, ink));
                if p0.distance(p1) > 24.0 {
                    let mid = p0 + (p1 - p0) * 0.5;
                    let mut out = (mid - centroid).normalized();
                    if !out.is_finite() || out.length() < 0.5 {
                        out = egui::vec2(0.7, 0.7);
                    }
                    let align = egui::Align2([
                        if out.x >= 0.0 {
                            egui::Align::Min
                        } else {
                            egui::Align::Max
                        },
                        if out.y >= 0.0 {
                            egui::Align::Min
                        } else {
                            egui::Align::Max
                        },
                    ]);
                    painter.text(
                        mid + out * 6.0,
                        align,
                        format!("{} {:.3}", ["X", "Y", "Z"][k], d.abs()),
                        crate::theme::prop(11.5),
                        ink,
                    );
                }
            }
            let mid = screen[0] + (screen[3] - screen[0]) * 0.5;
            let out = (mid - centroid).normalized();
            if out.length() > 0.5 {
                away = Some(out);
            }
        }
        // ボディの重心(面を 1 つ選んだ時): 十字の入った丸と「重心」の札。奥にあっても見える
        if let Some(c) = m.report.point {
            let p = to(c);
            let ink = self.win.look.pal.ink;
            painter.circle_filled(p, 7.0, self.win.look.pal.panel);
            painter.circle_stroke(p, 7.0, egui::Stroke::new(1.6, ink));
            painter.line_segment(
                [p - egui::vec2(7.0, 0.0), p + egui::vec2(7.0, 0.0)],
                egui::Stroke::new(1.2, ink),
            );
            painter.line_segment(
                [p - egui::vec2(0.0, 7.0), p + egui::vec2(0.0, 7.0)],
                egui::Stroke::new(1.2, ink),
            );
            painter.text(
                p + egui::vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                "重心",
                crate::theme::prop(11.5),
                ink,
            );
        }
        // 寸法の線: 選んだ測り方の 2 点(長穴なら両端の中心)を結ぶ破線と、真ん中に値
        if let Some((a, b)) = m.report.line {
            let (pa, pb) = (to(a), to(b));
            let ink = self.win.look.pal.ink;
            painter.add(egui::Shape::dashed_line(
                &[pa, pb],
                egui::Stroke::new(1.6, ink),
                6.0,
                4.0,
            ));
            painter.circle_filled(pa, 3.0, ink);
            painter.circle_filled(pb, 3.0, ink);
            if let Some(v) = &m.report.label {
                let g =
                    ui.fonts_mut(|f| f.layout_no_wrap(v.clone(), crate::theme::prop(13.0), ink));
                let size = g.size() + egui::vec2(10.0, 4.0);
                let center = pa + (pb - pa) * 0.5;
                let r = match away {
                    // 成分の線の反対側へ、札の大きさの半分だけずらす
                    Some(out) => Rect::from_center_size(
                        center + out * (8.0 + 0.5 * (size.x * out.x.abs() + size.y * out.y.abs())),
                        size,
                    ),
                    None => {
                        Rect::from_min_size(center + egui::vec2(8.0, -8.0 - g.size().y - 4.0), size)
                    }
                };
                painter.rect(
                    r,
                    3.0,
                    self.win.look.pal.panel,
                    egui::Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
                painter.galley(r.min + egui::vec2(5.0, 2.0), g, ink);
            }
        }
        // マウスの横に、拾う物の名前
        if let (Some(h), Some(p)) = (&m.hover, ui.input(|i| i.pointer.hover_pos())) {
            painter.text(
                p + egui::vec2(14.0, -14.0),
                egui::Align2::LEFT_BOTTOM,
                self.target_label(h),
                crate::theme::prop(12.5),
                color,
            );
        }
    }

    /// ビューの右上の小さな窓(選んだ物と値、写す・選び直す・閉じる)。
    pub(crate) fn measure_panel(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(m) = &self.cur.measure else { return };
        let labels: Vec<String> = m.picks.iter().map(|t| self.target_label(t)).collect();
        let rows = m.report.rows.clone();
        let primary = m.report.primary;
        let text = m.report.text();
        let bases = m.report.bases.clone();
        let mut basis = m.report.basis;
        let two = m.picks.len() == 2;
        let mut xyz = self.win.layout.measure_xyz;
        let copied = m.copied.is_some_and(|t| ui.input(|i| i.time) - t < 2.0);
        let l = self.win.look;
        let mut close = false;
        let mut clear = false;
        let mut copy: Option<String> = None;
        // ビューの右下(下の帯の上)。部品はふつう真ん中にあるので、隅の方が形を隠しにくい
        let w = 300.0;
        let screen = ui.ctx().content_rect();
        let offset = egui::vec2(
            rect.right() - screen.right() - 10.0,
            rect.bottom() - screen.bottom() - 56.0,
        );
        egui::Area::new(egui::Id::new("measure_panel"))
            .anchor(egui::Align2::RIGHT_BOTTOM, offset)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(l.pal.panel)
                    .stroke(egui::Stroke::new(1.0, l.ws()))
                    .corner_radius(crate::theme::corner(6.0))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(w - 20.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("計測").strong().color(l.ws()));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button("閉じる")
                                        .on_hover_text("Esc か M でも閉じる")
                                        .clicked()
                                    {
                                        close = true;
                                    }
                                },
                            );
                        });
                        ui.label(
                            egui::RichText::new(
                                "点・辺・面をクリック。2 つ選ぶと距離と角度。\
                                 選んだ物をもう一度押すと外し、3 つ目は測り直し",
                            )
                            .size(crate::theme::fs(11.0))
                            .color(l.pal.ink2),
                        );
                        ui.add_space(4.0);
                        egui::Grid::new("measure_picks")
                            .num_columns(2)
                            .spacing([8.0, 3.0])
                            .show(ui, |ui| {
                                for (i, name) in ["選択 1", "選択 2"].iter().enumerate() {
                                    ui.label(
                                        egui::RichText::new(*name)
                                            .size(crate::theme::fs(11.0))
                                            .weak(),
                                    );
                                    match labels.get(i) {
                                        Some(s) => ui.label(s),
                                        None => ui.label(
                                            egui::RichText::new("(まだ選んでいません)").weak(),
                                        ),
                                    };
                                    ui.end_row();
                                }
                            });
                        if bases.len() > 1 {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("測り方")
                                        .size(crate::theme::fs(11.0))
                                        .weak(),
                                );
                                for b in &bases {
                                    ui.selectable_value(&mut basis, *b, b.label())
                                        .on_hover_text(match b {
                                            Basis::Center => "穴・円は中心(円柱は軸)から測る",
                                            Basis::Min => "いちばん近い所どうし",
                                            Basis::Max => "いちばん遠い所どうし",
                                        });
                                }
                            });
                        }
                        if two {
                            ui.checkbox(&mut xyz, "X・Y・Z ごとの距離")
                                .on_hover_text("表に ΔX・ΔY・ΔZ を出し、ビューに軸ごとの線を描く");
                        }
                        if !rows.is_empty() {
                            ui.separator();
                            egui::Grid::new("measure_rows")
                                .num_columns(2)
                                .spacing([10.0, 3.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    for (i, (k, v)) in rows.iter().enumerate() {
                                        ui.label(egui::RichText::new(k).color(l.pal.ink2));
                                        let mut t = egui::RichText::new(v).monospace();
                                        if i == primary {
                                            t = t.strong();
                                        }
                                        let r = ui
                                            .add(egui::Label::new(t).sense(egui::Sense::click()))
                                            .on_hover_text("クリックでこの値を写す");
                                        if r.clicked() {
                                            copy = Some(v.clone());
                                        }
                                        ui.end_row();
                                    }
                                });
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(!rows.is_empty(), egui::Button::new("結果を写す"))
                                .on_hover_text("項目と値を、表に貼れる形でクリップボードへ")
                                .clicked()
                            {
                                copy = Some(text.clone());
                            }
                            if ui
                                .add_enabled(!labels.is_empty(), egui::Button::new("選択をクリア"))
                                .clicked()
                            {
                                clear = true;
                            }
                            if copied {
                                ui.label(egui::RichText::new("写しました").color(l.ws()));
                            }
                        });
                    });
            });
        if let Some(t) = copy {
            ui.ctx().copy_text(t);
            if let Some(m) = &mut self.cur.measure {
                m.copied = Some(ui.input(|i| i.time));
            }
        }
        let changed = xyz != self.win.layout.measure_xyz
            || self
                .cur
                .measure
                .as_ref()
                .is_some_and(|m| m.report.basis != basis);
        if changed {
            self.win.layout.measure_xyz = xyz;
            if let Some(m) = &mut self.cur.measure {
                m.basis = basis;
            }
            self.measure_refresh();
        }
        if clear && let Some(m) = &mut self.cur.measure {
            m.picks.clear();
            self.measure_refresh();
        }
        if close {
            self.cur.measure = None;
        }
    }
}
