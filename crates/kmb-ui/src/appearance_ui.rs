//! 設定の「画面」(U38): 表示の言語・テーマ・明るさ・UI の大きさ・字とアイコンの大きさ・部品の詰め具合・
//! 角の丸み・字の書体・動き。選んだ物は毎フレーム [`theme::apply`] で画面に当てる。

use crate::layout::{Appearance, Corners, Density, Motion, SCALE_RANGE};
use crate::{Workspace, fonts, theme, themes};

/// UI 全体の大きさの範囲(egui の表示倍率)。
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.5..=2.5;

impl Workspace {
    /// 見た目の設定を画面に当てる(毎フレーム。変わった時だけ egui の見た目と字を入れ替える)。
    pub(crate) fn apply_appearance(&mut self, ctx: &egui::Context, sketching: bool) {
        let t = self.win.themes.theme(&self.win.layout.appearance);
        theme::apply(ctx, &t, self.win.layout.space, sketching);
        let font = self.win.layout.appearance.font.clone();
        match &self.win.font_applied {
            // 起動した時の自動の字はアプリが入れている
            None if font.is_none() => self.win.font_applied = Some(None),
            Some(f) if *f == font => {}
            _ => {
                match fonts::install_fonts(ctx, font.as_deref()) {
                    Some(f) => log::info!("字の書体: {f}"),
                    None => log::warn!("日本語のフォントが見つかりません"),
                }
                self.win.font_applied = Some(font);
            }
        }
    }

    /// 設定の窓の「画面」の節。
    pub(crate) fn appearance_settings(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // 表示の言語の一覧(言語の名前はその言語で書き、今の言語に訳さない)
        let os = self.win.texts.os_lang();
        let languages: Vec<(String, String)> = kmb_i18n::Choice::all()
            .map(|c| {
                let name = match c {
                    kmb_i18n::Choice::Auto => self.win.t_with(
                        "settings-language-auto",
                        &[("lang", os.native_name().into())],
                    ),
                    kmb_i18n::Choice::Fixed(l) => l.native_name().to_string(),
                };
                (c.code().to_string(), name)
            })
            .collect();
        let language_label = self.win.t("settings-language");
        let language_hint = self.win.t("settings-language-hint");
        let a = &mut self.win.layout.appearance;
        let pct = |v: f64| format!("{:.0} %", v * 100.0);
        // 欄に打った「50」「50 %」「50%」は 0.5 倍(書き方と同じく % で読む)
        let unpct = |t: &str| {
            t.trim()
                .trim_end_matches('%')
                .trim()
                .parse::<f64>()
                .ok()
                .map(|v| v / 100.0)
        };
        egui::Grid::new("appearance")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label(language_label.as_str());
                let now = kmb_i18n::Choice::parse(&a.language).code();
                egui::ComboBox::from_id_salt("appearance_language")
                    .selected_text(
                        languages
                            .iter()
                            .find(|(c, _)| c == now)
                            .map_or("", |(_, n)| n.as_str()),
                    )
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for (code, name) in &languages {
                            ui.selectable_value(&mut a.language, code.clone(), name);
                        }
                    })
                    .response
                    .on_hover_text(language_hint.as_str());
                ui.end_row();
                ui.label("テーマ");
                egui::ComboBox::from_id_salt("appearance_theme")
                    .selected_text(self.win.themes.label(&a.theme))
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for (id, name) in self.win.themes.choices() {
                            ui.selectable_value(&mut a.theme, id, name);
                        }
                    })
                    .response
                    .on_hover_text(
                        "同梱のテーマは、明るい・暗いの両方を持つ(下の「明るさ」で替わる)。\n\
                         色覚の型に合わせたテーマは、赤と緑(1・2 型)、青と黄(3 型)の区別に頼らない色にする。\n\
                         自分のテーマは、データフォルダの themes に JSON で置く(変えたい色だけ書けばよい)",
                    );
                ui.end_row();

                ui.label("見本");
                swatches(ui);
                ui.end_row();

                ui.label("明るさ");
                ui.horizontal(|ui| {
                    use crate::layout::Brightness;
                    let b = &mut self.win.layout.brightness;
                    ui.radio_value(b, Brightness::System, "PC に合わせる");
                    ui.radio_value(b, Brightness::Light, "明るい");
                    ui.radio_value(b, Brightness::Dark, "暗い");
                });
                ui.end_row();

                ui.label("UI の大きさ");
                let mut zoom = ctx.zoom_factor();
                let r = ui
                    .add(
                        egui::Slider::new(&mut zoom, ZOOM_RANGE)
                            .step_by(0.1)
                            .custom_formatter(|v, _| pct(v))
                        .custom_parser(unpct),
                    )
                    .on_hover_text(
                        "画面全体(字・アイコン・部品・3D のビューの線)をまとめて大きくする。\n\
                         Ctrl を押しながら + と - でも変わり、窓の大きさと一緒に覚える",
                    );
                // 引いている間は替えない(大きさが変わると、つまみがマウスの下から逃げる)
                if (r.drag_stopped() || (r.changed() && !r.dragged()))
                    && (zoom - ctx.zoom_factor()).abs() > 1e-3
                {
                    ctx.set_zoom_factor(zoom);
                }
                ui.end_row();

                ui.label("字の大きさ");
                ui.add(
                    egui::Slider::new(&mut a.text_scale, SCALE_RANGE)
                        .step_by(0.05)
                        .custom_formatter(|v, _| pct(v))
                        .custom_parser(unpct),
                )
                .on_hover_text(
                    "字だけを大きくする(50〜250 %)。ツールバーのボタンと帯も字に合わせて広がる",
                );
                ui.end_row();

                ui.label("アイコンの大きさ");
                ui.add(
                    egui::Slider::new(&mut a.icon_scale, SCALE_RANGE)
                        .step_by(0.05)
                        .custom_formatter(|v, _| pct(v))
                        .custom_parser(unpct),
                );
                ui.end_row();

                ui.label("部品の詰め具合");
                ui.horizontal(|ui| {
                    for d in Density::ALL {
                        ui.radio_value(&mut a.density, d, d.label());
                    }
                });
                ui.end_row();

                ui.label("角の丸み");
                ui.horizontal(|ui| {
                    for c in Corners::ALL {
                        ui.radio_value(&mut a.corners, c, c.label());
                    }
                });
                ui.end_row();

                ui.label("字の書体");
                let list = self
                    .settings.font_list
                    .get_or_insert_with(|| fonts::font_choices(self.win.data_dir.as_deref()));
                let name_of = |p: &Option<String>| match p {
                    None => "自動(OS の日本語の字)".to_string(),
                    Some(p) => list
                        .iter()
                        .find(|(q, _)| q == p)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_else(|| p.clone()),
                };
                egui::ComboBox::from_id_salt("appearance_font")
                    .selected_text(name_of(&a.font))
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut a.font, None, name_of(&None));
                        for (p, n) in list.iter() {
                            ui.selectable_value(&mut a.font, Some(p.clone()), n)
                                .on_hover_text(p);
                        }
                    })
                    .response
                    .on_hover_text(
                        "日本語の字の入ったフォントを選ぶ。候補にないフォントは、\n\
                         データフォルダの fonts に .ttf・.otf・.ttc を置くと出る",
                    );
                ui.end_row();

                ui.label("動き");
                ui.horizontal(|ui| {
                    for m in Motion::ALL {
                        ui.radio_value(&mut a.motion, m, m.label());
                    }
                })
                .response
                .on_hover_text(
                    "パネルの開閉や、軸の玉で向きを替える時の滑らかな動き。\
                     「しない」はすぐに切り替える",
                );
                ui.end_row();

                ui.label("ツールバー");
                ui.checkbox(&mut a.tool_labels, "ボタンに名前を出す")
                    .on_hover_text(
                        "切るとアイコンだけになり、ツールバーが狭く低くなる。\n\
                         名前は、ボタンに指を置くと出る",
                    );
                ui.end_row();

                ui.label("アイコン");
                ui.checkbox(&mut a.role_colors, "役の色を付ける")
                    .on_hover_text(
                        "道具が作る・変える・増やす・決める部分を、それぞれの色で描く。\n\
                         切ると、アイコンを字と同じ 1 色で描く",
                    );
                ui.end_row();
            });
        ui.horizontal_wrapped(|ui| {
            let reset = Appearance {
                font: None,
                ..Appearance::default()
            };
            if ui
                .add_enabled(
                    self.win.layout.appearance != reset || (ctx.zoom_factor() - 1.0).abs() > 1e-3,
                    egui::Button::new("見た目を既定に戻す"),
                )
                .on_hover_text(
                    "テーマ・大きさ・角・詰め具合・字の書体・動き・ボタンの名前を既定に戻す\
                     (明るさは残す)",
                )
                .clicked()
            {
                self.win.layout.appearance = reset;
                ctx.set_zoom_factor(1.0);
            }
            if let Some(dir) = self.win.themes.dir.clone() {
                if ui
                    .button("今のテーマを書き出す")
                    .on_hover_text(
                        "今のテーマの全部の色を、色の名前つきで themes のフォルダに書く。\n\
                         書いたファイルの色を直すと、自分のテーマとして選べる",
                    )
                    .clicked()
                {
                    self.settings.appearance_note = Some(
                        match themes::export(&dir, &self.win.themes, &self.win.layout.appearance) {
                            Ok(p) => {
                                self.win.themes =
                                    themes::Themes::load(self.win.data_dir.as_deref());
                                format!("書き出しました: {}", p.display())
                            }
                            Err(e) => format!("書き出せませんでした: {e}"),
                        },
                    );
                }
                if ui
                    .button("テーマを読み直す")
                    .on_hover_text("themes のフォルダのファイルを直した後に押す")
                    .clicked()
                {
                    self.win.themes = themes::Themes::load(self.win.data_dir.as_deref());
                    self.settings.font_list = None;
                    self.settings.appearance_note = Some(format!(
                        "読み直しました(自分のテーマ {} 個)",
                        self.win.themes.user.len()
                    ));
                }
                if ui
                    .button("テーマのフォルダの道を写す")
                    .on_hover_text(dir.display().to_string())
                    .clicked()
                {
                    ctx.copy_text(dir.display().to_string());
                    self.settings.appearance_note = Some(format!("写しました: {}", dir.display()));
                }
            }
        });
        let warn = theme::look(&ctx).pal.warn;
        if let Some(n) = &self.settings.appearance_note {
            ui.weak(n);
        }
        for (file, why) in &self.win.themes.broken {
            ui.colored_label(warn, format!("テーマ {file} を読めませんでした: {why}"));
        }
        if let Some(t) = self
            .win
            .themes
            .user_theme(&self.win.layout.appearance.theme)
        {
            for n in &t.notes {
                ui.colored_label(warn, format!("テーマ {}: {n}", t.name));
            }
        }
    }
}

/// 今のテーマの、見分けたい色の見本(状態・軸・加工の比較・道具の立体)。
fn swatches(ui: &mut egui::Ui) {
    let look = theme::look(ui.ctx());
    let (p, k) = (look.pal, look.pal.inks);
    let groups: [(&str, &[(egui::Color32, &str)]); 4] = [
        ("状態", &[(p.ok, "良い"), (p.warn, "注意"), (p.err, "誤り")]),
        (
            "軸",
            &[(k.axis[0], "X"), (k.axis[1], "Y"), (k.axis[2], "Z")],
        ),
        (
            "比較",
            &[
                (k.deviation[0], "許容内"),
                (k.deviation[1], "不足"),
                (k.deviation[2], "過ぎ"),
            ],
        ),
        (
            "道具",
            &[
                (k.ghost_join, "結合"),
                (k.ghost_cut, "切り取り"),
                (k.ghost_intersect, "交差"),
            ],
        ),
    ];
    ui.horizontal(|ui| {
        for (name, cs) in groups {
            ui.weak(name);
            for (c, label) in cs {
                let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, *c);
                ui.label(*label);
            }
            ui.add_space(6.0);
        }
    });
}
