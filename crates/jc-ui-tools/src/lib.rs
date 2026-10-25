//! 工具ライブラリの画面。
//!
//! 工具ライブラリは 2 層にする。PC 全体のライブラリ(データフォルダの `tools.json`、平文)と、
//! 文書の中のコピー(機能 `cam.tools@2`)。文書のコピーがライブラリと違う工具には印を出し、
//! 「ライブラリに揃える」「ライブラリへ書き戻す」を選べる。勝手には同期しない
//! (文書を他の PC で開いても同じ結果になるように)。
//!
//! ほかに、Carvera Air に付く工具をまとめて足す、ほかの CAM の工具ライブラリの読み込みと書き出し、
//! 番号の振り直し(加工の工具番号も一緒に直す)、工具の形の絵、加工の工具の欄で PC のライブラリの
//! 工具を選ぶ欄(選ぶと文書のライブラリへ写す)を持つ。

use jc_cam_tool::ToolLibrary;
use jc_cam_tool::{TOOLS, TOOLS_V1, Tool, ToolKind};
use jc_core::FeatureEntry;
use jc_ui::{EditorHost, FeatureEditor};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};

mod files;
mod form;
mod picture;
mod refs;
pub use files::{export_library, import_library};
pub use refs::renumber_edits;

/// PC 全体のライブラリの状態。
#[derive(Debug, Clone, PartialEq, Default)]
pub enum GlobalTools {
    /// データフォルダがない(テストなど)
    #[default]
    Unavailable,
    /// まだファイルがない
    Missing,
    Loaded(ToolLibrary),
    /// 読めなかった。利用者のデータを消さないよう、書き戻さない
    Broken(String),
}

/// 工具ライブラリの編集器(画面の拡張)。PC 全体のライブラリの状態を持つ。
#[derive(Default)]
pub struct ToolsEditor {
    data_dir: Option<PathBuf>,
    global: GlobalTools,
}

/// 文書の工具ライブラリの中身。
pub fn library_of(entry: &FeatureEntry) -> Option<ToolLibrary> {
    serde_json::from_value(entry.data.clone()).ok()
}

impl ToolsEditor {
    pub fn new() -> Self {
        Self::default()
    }

    fn path(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().map(|d| d.join("tools.json"))
    }

    /// PC 全体のライブラリを読み直す。
    pub fn load(&mut self) {
        self.global = match self.path() {
            None => GlobalTools::Unavailable,
            Some(p) if !p.exists() => GlobalTools::Missing,
            Some(p) => match std::fs::read_to_string(&p)
                .map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str::<ToolLibrary>(&s).map_err(|e| e.to_string()))
            {
                Ok(lib) => GlobalTools::Loaded(lib),
                Err(e) => GlobalTools::Broken(format!("{} を読めません: {e}", p.display())),
            },
        };
    }

    pub fn global(&self) -> &GlobalTools {
        &self.global
    }

    /// PC 全体のライブラリを書く。前のファイルは `.bak` で残す。
    pub fn save_global(&mut self, lib: &ToolLibrary) -> Result<(), String> {
        if let GlobalTools::Broken(e) = &self.global {
            return Err(format!("{e}。ファイルを直すまで書き戻しません"));
        }
        let path = self
            .path()
            .ok_or("データフォルダがないので、PC のライブラリを使えません")?;
        lib.check()?;
        let text = serde_json::to_string_pretty(lib).map_err(|e| e.to_string())?;
        jc_session::write_atomic(&path, &text, true)?;
        self.global = GlobalTools::Loaded(lib.clone());
        Ok(())
    }

    /// 文書のライブラリ `lib` の工具 `number` を、PC のライブラリの中身にしたもの
    /// (ライブラリになければ消す)。PC のライブラリがなければ `None`。
    pub fn from_global(&self, lib: &ToolLibrary, number: u32) -> Option<ToolLibrary> {
        let GlobalTools::Loaded(g) = &self.global else {
            return None;
        };
        let mut lib = lib.clone();
        lib.take_from(g, number);
        Some(lib)
    }

    /// 文書の工具 `number` を、PC のライブラリへ書き戻す(文書になければライブラリから消す)。
    /// 知らせの文を返す。
    pub fn to_global(&mut self, lib: &ToolLibrary, number: u32) -> String {
        let mut g = match &self.global {
            GlobalTools::Loaded(g) => g.clone(),
            GlobalTools::Missing => ToolLibrary::default(),
            GlobalTools::Unavailable | GlobalTools::Broken(_) => {
                return "PC のライブラリに書けません".into();
            }
        };
        g.take_from(lib, number);
        match self.save_global(&g) {
            Ok(()) => format!("工具 T{number} を PC のライブラリへ書き戻しました"),
            Err(e) => e,
        }
    }

    /// 文書の工具を全部 PC のライブラリにする(ライブラリがまだない時に使う)。
    pub fn all_to_global(&mut self, lib: &ToolLibrary) -> String {
        match self.save_global(lib) {
            Ok(()) => "この工具の一覧を PC のライブラリにしました".into(),
            Err(e) => e,
        }
    }

    /// 文書の工具で、PC のライブラリと違う番号。
    pub fn differing(&self, lib: &ToolLibrary) -> Vec<u32> {
        match &self.global {
            GlobalTools::Loaded(g) => lib.differs_from(g),
            _ => Vec::new(),
        }
    }
}

impl FeatureEditor for ToolsEditor {
    fn fresh(&self) -> Box<dyn FeatureEditor> {
        Box::new(ToolsEditor::new())
    }

    fn handles(&self, type_id: &str) -> bool {
        type_id == TOOLS || type_id == TOOLS_V1
    }

    fn panel(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<Json> {
        tool_library_panel(ui, self, host, entry)
    }

    fn assist(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut EditorHost<'_>,
        entry: &FeatureEntry,
    ) -> Option<Json> {
        refs::pc_tool_picker(self, ui, host, entry);
        None
    }

    fn set_data_dir(&mut self, dir: Option<&Path>) {
        self.data_dir = dir.map(Path::to_path_buf);
        self.load();
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn kind_label(k: ToolKind) -> &'static str {
    k.label()
}

/// 描き終えてから行う操作(書き戻しなど)。変えた文書のライブラリを返すことがある。
type Action =
    Box<dyn FnOnce(&mut ToolsEditor, &ToolLibrary, &mut Option<String>) -> Option<ToolLibrary>>;

/// 一覧と編集の状態(画面の記憶に置く)。
#[derive(Clone)]
struct PanelState {
    selected: Option<u32>,
    search: String,
    kind: Option<ToolKind>,
    /// 番号の振り直しの開始と増分
    renumber: (u32, u32),
}

impl Default for PanelState {
    fn default() -> Self {
        PanelState {
            selected: None,
            search: String::new(),
            kind: None,
            renumber: (1, 1),
        }
    }
}

/// 工具ライブラリの編集画面。データを変えたら新しいデータを返す。
fn tool_library_panel(
    ui: &mut egui::Ui,
    ed: &mut ToolsEditor,
    host: &mut EditorHost<'_>,
    entry: &FeatureEntry,
) -> Option<Json> {
    let mut lib: ToolLibrary = serde_json::from_value(entry.data.clone()).ok()?;
    let before = lib.clone();
    let sid = jc_ui::doc_id(ui.ctx(), ("tools_panel", entry.id));
    let mut st: PanelState = ui.data(|d| d.get_temp(sid)).unwrap_or_default();
    let differ = ed.differing(&lib);
    let global = ed.global.clone();
    // 長さは文書の単位で見せる(保存は mm)
    let unit = host.data.properties.units;
    let mut action: Option<Action> = None;
    // 書き換えた文書のライブラリと、その後に出す知らせ
    let mut commit: Option<(ToolLibrary, String)> = None;

    egui::CollapsingHeader::new(format!("工具({} 本)", lib.tools.len()))
        .id_salt(("tools", entry.id))
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("検索");
                ui.add(egui::TextEdit::singleline(&mut st.search).desired_width(100.0));
                egui::ComboBox::from_id_salt(("tool_kind", entry.id))
                    .selected_text(st.kind.map_or("すべての種類", kind_label))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.kind, None, "すべての種類");
                        for k in ToolKind::ALL {
                            ui.selectable_value(&mut st.kind, Some(k), kind_label(k));
                        }
                    });
                if ui.button("工具を足す").clicked() {
                    let number = lib.next_number();
                    let mut t = lib
                        .tools
                        .iter()
                        .find(|t| Some(t.number) == st.selected)
                        .or(lib.tools.first())
                        .cloned()
                        .unwrap_or_else(|| ToolLibrary::starter().tools[1].clone());
                    t.number = number;
                    t.name = format!("{} の写し", t.name);
                    lib.tools.push(t);
                    st.selected = Some(number);
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Carvera Air の工具を足す")
                    .on_hover_text(
                        "Carvera Air に付く工具(Ø3.175 の平 刃長 25・12、彫刻の刃、面取り、コーン)を\
                         まとめて足す。同じ名前の工具は足さない",
                    )
                    .clicked()
                {
                    let mut l = before.clone();
                    let added = l.add_tools(jc_cam_tool::carvera_air_tools(), true);
                    match (added.first(), added.last()) {
                        (Some(a), Some(b)) => {
                            st.selected = Some(*a);
                            commit = Some((
                                l,
                                format!("Carvera Air の工具 {} 本を足しました(T{a}〜T{b})", added.len()),
                            ));
                        }
                        _ => *host.message = Some("Carvera Air の工具はもう全部あります".into()),
                    }
                }
                if ui
                    .button("読み込む…")
                    .on_hover_text(
                        "ほかの CAM の工具ライブラリ(.tools・.json)や、この CAM の工具の一覧を\
                         読み、ここに足す",
                    )
                    .clicked()
                {
                    match host.dialogs.open_file(
                        "工具ライブラリを読み込む",
                        "工具ライブラリ(.tools・.json)",
                        &["tools", "json"],
                    ) {
                        Some(p) => {
                            let mut l = before.clone();
                            match import_library(&mut l, &p) {
                                Ok(m) if l != before => commit = Some((l, m)),
                                Ok(m) | Err(m) => *host.message = Some(m),
                            }
                        }
                        None => {
                            *host.message = Some(
                                "工具ライブラリを読み込む: ファイルが選ばれなかったので、やめました"
                                    .into(),
                            )
                        }
                    }
                }
                if ui
                    .button("書き出す…")
                    .on_hover_text("この一覧をほかの CAM の工具ライブラリの形(.json・.tools)か CSV(.csv)に書く")
                    .clicked()
                {
                    *host.message = Some(
                        match host.dialogs.save_file(
                            "工具ライブラリを書き出す",
                            "工具ライブラリ(.json・.tools・.csv)",
                            &["json", "tools", "csv"],
                            "工具ライブラリ.json",
                        ) {
                            Some(p) => export_library(&before, &p).unwrap_or_else(|e| e),
                            None => {
                                "工具ライブラリを書き出す: 保存先が選ばれなかったので、やめました"
                                    .into()
                            }
                        },
                    );
                }
            });
            // 幅が足りなければ(右の編集パネルの既定の幅)、一覧の下に工具の形を出す
            let wide = ui.available_width() >= 480.0;
            let layout = if wide {
                egui::Layout::left_to_right(egui::Align::Min)
            } else {
                egui::Layout::top_down(egui::Align::Min)
            };
            ui.with_layout(layout, |ui| {
                // 左(狭ければ上): 一覧
                ui.vertical(|ui| {
                    if wide {
                        ui.set_width(200.0);
                    }
                    egui::ScrollArea::vertical()
                        .id_salt(("tool_list", entry.id))
                        .max_height(if wide { 260.0 } else { 160.0 })
                        .show(ui, |ui| {
                            let q = st.search.trim().to_lowercase();
                            for t in &lib.tools {
                                if st.kind.is_some_and(|k| k != t.kind)
                                    || !q.is_empty() && !t.name.to_lowercase().contains(&q)
                                {
                                    continue;
                                }
                                let mark = if differ.contains(&t.number) {
                                    " ●"
                                } else {
                                    ""
                                };
                                let dia = if unit.is_mm() {
                                    t.diameter.to_string()
                                } else {
                                    unit.length(t.diameter, unit.digits())
                                };
                                let label = format!("T{} {}(径 {dia}){mark}", t.number, t.name);
                                let r = ui.selectable_label(st.selected == Some(t.number), label);
                                let r = if mark.is_empty() {
                                    r
                                } else {
                                    r.on_hover_text("PC のライブラリと中身が違う")
                                };
                                if r.clicked() {
                                    st.selected = Some(t.number);
                                }
                            }
                        });
                });
                ui.separator();
                // 右: 選んだ工具の形とプリセット
                ui.vertical(|ui| {
                    let Some(i) = lib.tools.iter().position(|t| Some(t.number) == st.selected)
                    else {
                        ui.weak("左の一覧から工具を選ぶ");
                        return;
                    };
                    let numbers: Vec<u32> = lib.tools.iter().map(|t| t.number).collect();
                    if form::tool_form(ui, entry.id, &mut lib.tools[i], &numbers, unit) {
                        st.selected = Some(lib.tools[i].number);
                    }
                    let number = lib.tools[i].number;
                    ui.horizontal(|ui| {
                        if ui.button("この工具を消す").clicked() {
                            lib.tools.remove(i);
                            st.selected = None;
                        }
                        if matches!(global, GlobalTools::Loaded(_) | GlobalTools::Missing)
                            && ui
                                .button("ライブラリへ書き戻す")
                                .on_hover_text(
                                    "PC 全体の工具ライブラリ(tools.json)に、この工具を書く",
                                )
                                .clicked()
                        {
                            action = Some(Box::new(move |ed, lib, msg| {
                                *msg = Some(ed.to_global(lib, number));
                                None
                            }));
                        }
                        if differ.contains(&number)
                            && matches!(global, GlobalTools::Loaded(_))
                            && ui
                                .button("ライブラリに揃える")
                                .on_hover_text("この工具を PC のライブラリの中身にする")
                                .clicked()
                        {
                            action = Some(Box::new(move |ed, lib, _| ed.from_global(lib, number)));
                        }
                    });
                });
            });
            ui.separator();
            match &global {
                GlobalTools::Unavailable => {}
                GlobalTools::Missing => {
                    ui.horizontal(|ui| {
                        ui.weak("PC 全体の工具ライブラリはまだありません。");
                        if ui.button("この一覧を PC のライブラリにする").clicked() {
                            action = Some(Box::new(move |ed, lib, msg| {
                                *msg = Some(ed.all_to_global(lib));
                                None
                            }));
                        }
                    });
                }
                GlobalTools::Broken(e) => {
                    ui.colored_label(ui.visuals().warn_fg_color, e);
                }
                GlobalTools::Loaded(g) => {
                    let only: Vec<&Tool> = g
                        .tools
                        .iter()
                        .filter(|t| lib.get(t.number).is_none())
                        .collect();
                    if !only.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.weak("PC のライブラリから足す:");
                            for t in only {
                                if ui
                                    .small_button(format!("T{} {}", t.number, t.name))
                                    .clicked()
                                {
                                    lib.tools.push(t.clone());
                                    lib.tools.sort_by_key(|t| t.number);
                                    st.selected = Some(t.number);
                                }
                            }
                        });
                    }
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("番号の振り直し").on_hover_text(
                    "一覧の順に、開始の番号から増分ずつ振り直す。この一覧の工具を使う加工の工具番号も一緒に直す",
                );
                ui.label("開始");
                ui.add(egui::DragValue::new(&mut st.renumber.0).range(1..=99));
                ui.label("増分");
                ui.add(egui::DragValue::new(&mut st.renumber.1).range(1..=50));
                if ui.button("振り直す").clicked() {
                    let mut l = before.clone();
                    match l.renumber(st.renumber.0, st.renumber.1) {
                        Ok(map) => {
                            st.selected = st
                                .selected
                                .and_then(|n| map.iter().find(|(o, _)| *o == n).map(|(_, m)| *m));
                            let mut args = renumber_edits(host.data, entry.id, &l, &map);
                            args["message"] = format!(
                                "工具の番号を {} から {} ずつ振り直しました(加工の工具番号も直しました)",
                                st.renumber.0, st.renumber.1
                            )
                            .into();
                            *host.then = Some((jc_ui::APP_EDITS.into(), args));
                        }
                        Err(e) => *host.message = Some(e),
                    }
                }
            });
        });
    ui.data_mut(|d| d.insert_temp(sid, st));
    if let Some((l, message)) = commit {
        // 知らせを残すため、データは画面に頼んで書き換える(書き換えると知らせが消えるので)
        *host.then = Some((
            jc_ui::APP_EDITS.into(),
            serde_json::json!({
                "label": "工具を足す",
                "edits": [{"id": entry.id.0, "data": serde_json::to_value(&l).expect("JSON にできる")}],
                "message": message,
            }),
        ));
        return None;
    }
    if let Some(a) = action {
        // 書き戻しなどは、文書に入っている(編集前の)ライブラリに対して行う
        return a(ed, &before, host.message)
            .map(|lib| serde_json::to_value(&lib).expect("JSON にできる"));
    }
    (lib != before).then(|| serde_json::to_value(&lib).expect("JSON にできる"))
}

#[cfg(test)]
mod tests;
