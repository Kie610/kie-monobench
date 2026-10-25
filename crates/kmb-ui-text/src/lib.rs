//! 文字の編集器(画面の拡張)。文字の設定欄の下に「フォント」の選び欄を出す(1 本線の 2 つと、PC の
//! 輪郭のフォント)。輪郭のフォントの文字は、打った字の形が文書に入っていなければ、PC の同じフォント
//! から足す(計算はフォントを読まないので、字の形を文書に入れるのは画面と操作の役目)。

use kmb_core::{FeatureEntry, FeatureId};
use kmb_text::fonts::{Face, FontIndex};
use kmb_text::{Font, TEXT, Text};
use kmb_ui::{EditorHost, FeatureEditor};
use std::path::Path;

/// 文字の編集器。PC のフォントの一覧を持つ(初めて「フォント」の一覧を開いた時に作る)。
#[derive(Default)]
pub struct TextEditor {
    fonts: FontIndex,
    /// 一覧を名前で絞る文字
    search: String,
    /// 字を足せなかった文字(同じ文字で毎回読み直さない)
    failed: Option<(FeatureId, String)>,
}

impl TextEditor {
    pub fn new() -> Self {
        Self::default()
    }

    /// フォントを探すフォルダを足す(試験のフォント)。
    pub fn with_font_dir(mut self, dir: &Path) -> Self {
        self.fonts.add_dir(dir);
        self
    }
}

/// 選び欄の 1 行: 1 本線のフォントか、PC のフォント。
enum Pick {
    Single(Font),
    Outline(Face),
}

impl TextEditor {
    /// 形の入っていない字を PC の同じフォントから足す。足したら新しいデータ。
    fn fill(&mut self, host: &mut EditorHost<'_>, id: FeatureId, t: &mut Text) -> bool {
        let key = (id, t.text.clone());
        if self.failed.as_ref() == Some(&key) {
            return false;
        }
        match kmb_text::refresh_glyphs(t, &mut self.fonts) {
            Ok(Some(absent)) => {
                if !absent.is_empty() {
                    let s: String = absent.iter().take(12).collect();
                    *host.message = Some(format!("このフォントに無い字: {s}"));
                    self.failed = Some(key);
                }
                true
            }
            Ok(None) => false,
            Err(e) => {
                *host.message = Some(e);
                self.failed = Some(key);
                false
            }
        }
    }
}

fn label_of(t: &Text) -> String {
    match &t.outline {
        Some(f) => f.label_or_family(),
        None => single_label(t.font).into(),
    }
}

fn single_label(f: Font) -> &'static str {
    match f {
        Font::Sans => "1 本線(飾りなし)",
        Font::Roman => "1 本線(飾りあり)",
    }
}

impl FeatureEditor for TextEditor {
    fn handles(&self, type_id: &str) -> bool {
        type_id == TEXT
    }

    fn panel(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<serde_json::Value> {
        let mut t: Text = serde_json::from_value(entry.data.clone()).ok()?;
        if self.fill(host, entry.id, &mut t) {
            return serde_json::to_value(&t).ok();
        }
        let mut pick: Option<Pick> = None;
        ui.horizontal(|ui| {
            let label = ui.label("フォント");
            egui::ComboBox::from_id_salt(("text_font", entry.id))
                .selected_text(label_of(&t))
                .height(360.0)
                // 一覧の中の「名前で絞る」欄を押しても閉じない(選んだら閉じる)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show_ui(ui, |ui| {
                    for f in [Font::Sans, Font::Roman] {
                        if ui
                            .selectable_label(t.outline.is_none() && t.font == f, single_label(f))
                            .on_hover_text("彫刻でなぞる 1 本線の字(英数字と記号)")
                            .clicked()
                        {
                            pick = Some(Pick::Single(f));
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        let l = ui.label("名前で絞る");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search)
                                .hint_text("例: Noto")
                                .desired_width(160.0),
                        )
                        .labelled_by(l.id);
                    });
                    let low = self.search.to_lowercase();
                    let now = t
                        .outline
                        .as_ref()
                        .map(|f| (f.family.clone(), f.style.clone()));
                    for face in self.fonts.faces() {
                        if !low.is_empty()
                            && !face.label.to_lowercase().contains(&low)
                            && !face.family.to_lowercase().contains(&low)
                        {
                            continue;
                        }
                        let on = now.as_ref() == Some(&(face.family.clone(), face.style.clone()));
                        let r = ui.add_enabled(
                            face.embeddable,
                            egui::Button::selectable(on, &face.label),
                        );
                        let r = if face.embeddable {
                            r
                        } else {
                            r.on_disabled_hover_text(
                                "作った人が字の形を文書に入れることを許していないフォント",
                            )
                        };
                        if r.clicked() {
                            pick = Some(Pick::Outline(face.clone()));
                        }
                    }
                    if pick.is_some() {
                        ui.close();
                    }
                })
                .response
                .labelled_by(label.id);
        });
        match pick? {
            Pick::Single(f) => {
                t.outline = None;
                t.font = f;
            }
            Pick::Outline(face) => {
                self.failed = None;
                match kmb_text::use_face(&mut t, &face) {
                    Ok(absent) if !absent.is_empty() => {
                        let s: String = absent.iter().take(12).collect();
                        *host.message = Some(format!("このフォントに無い字: {s}"));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        *host.message = Some(e);
                        return None;
                    }
                }
            }
        }
        serde_json::to_value(&t).ok()
    }

    fn edit_label(&self) -> &'static str {
        "文字を変更"
    }

    fn reset(&mut self) {
        self.search.clear();
    }

    fn set_data_dir(&mut self, dir: Option<&Path>) {
        if let Some(d) = dir {
            self.fonts.add_dir(&d.join("fonts"));
        }
    }

    fn fresh(&self) -> Box<dyn FeatureEditor> {
        // 足したフォルダ(試験のフォント)は引き継ぐ。一覧は作り直す
        let mut e = TextEditor::new();
        for d in self.fonts.added_dirs() {
            e.fonts.add_dir(d);
        }
        Box::new(e)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests;
