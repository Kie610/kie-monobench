//! コマンド検索(S でカーソルの所、Ctrl+K で画面の上)。名前の一部や読み(例: 「おしだし」
//! 「extrude」)で探して実行する。画面を止めない小さな窓で、使えないコマンドは理由と一緒に灰色で出す。
//! 行の右には、ツールバーのどこにあるか(「ソリッド › 作成」)とキーを出す(U1。次から帯で押せるように)。

use crate::Workspace;

/// コマンドの種類。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandId {
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    UndoHistory,
    Settings,
    StartScreen,
    /// 測る道具
    Measure,
    /// 断面で切って中を見る(M48)
    Section,
    /// 選んだボディ・機能を隠す・出す
    ToggleVisible,
    /// 選んだ面を正面から見る
    LookAt,
    /// モジュールの操作(操作の ID)
    Action(String),
    /// 記述子の初めのデータで機能を足す(種類の ID)
    Add(String),
    /// 中央のビューを持つ編集器(スケッチ)の帯の道具(道具の ID)
    EditorTool(String),
}

/// 一覧に出すコマンド。
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub id: CommandId,
    pub name: String,
    /// 検索に使う別名(読み、英語)
    pub aliases: String,
    /// 使えない理由(使えるなら `None`)
    pub disabled: Option<String>,
}

/// 探す文字列 `q` がコマンドに合うか。合えば点数(小さいほど良い)を返す。
/// 名前・別名に含まれる、または文字を順に含む(飛ばし読み)なら合う。
fn score(c: &Command, q: &str) -> Option<usize> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let name = c.name.to_lowercase();
    if name.starts_with(&q) {
        return Some(0);
    }
    if name.contains(&q) {
        return Some(1);
    }
    if c.aliases.split(' ').any(|a| a.starts_with(&q)) {
        return Some(2);
    }
    let hay = format!("{name} {}", c.aliases);
    let mut it = hay.chars();
    q.chars()
        .all(|ch| ch == ' ' || it.any(|h| h == ch))
        .then_some(3)
}

/// コマンド検索の窓の状態(S で開く)。
pub(crate) struct CommandSearch {
    /// コマンド検索の窓の文字列(開いている時だけ)
    pub(crate) text: Option<String>,
    /// コマンド検索の窓を出す所(S で開いた時のカーソルの位置。`None` なら画面の上の真ん中)
    pub(crate) at: Option<egui::Pos2>,
    /// 中央のビューを持つ編集器の帯の道具(ID、名前、説明、押せるか、まとまりの名前)。
    /// コマンド検索に出す
    pub(crate) view_tools: Vec<(String, String, String, bool, String)>,
}

impl Workspace {
    /// 今使えるかどうかつきの、全部のコマンド(画面のコマンド、モジュールの操作、
    /// 記述子から足せる種類)。
    pub fn commands(&self) -> Vec<Command> {
        let v = [
            (
                CommandId::New,
                "新規デザイン",
                "しんき でざいん あたらしい 新しい文書 new",
                None,
            ),
            (CommandId::Open, "開く", "ひらく open", None),
            (CommandId::Save, "保存", "ほぞん save", None),
            (
                CommandId::SaveAs,
                "名前を付けて保存",
                "なまえ save as べつめい",
                None,
            ),
            (
                CommandId::Undo,
                "元に戻す",
                "とりけし undo もどす",
                (!self.cur.work.doc().can_undo()).then_some("取り消せる操作がない"),
            ),
            (
                CommandId::Redo,
                "やり直し",
                "やりなおし redo",
                (!self.cur.work.doc().can_redo()).then_some("やり直せる操作がない"),
            ),
            (
                CommandId::UndoHistory,
                "取り消しの履歴",
                "とりけし りれき undo history もどす",
                None,
            ),
            (
                CommandId::Settings,
                "設定",
                "せってい settings きかい machine たんい ぽすと",
                None,
            ),
            (
                CommandId::StartScreen,
                "ホーム",
                "ほーむ かいしがめん 開始画面 start home",
                None,
            ),
            (
                CommandId::Measure,
                "計測",
                "はかる きょり かくど ちょっけい めんせき measure distance angle",
                None,
            ),
            (
                CommandId::Section,
                "断面解析",
                "だんめん きる なかをみる section cut clip",
                None,
            ),
            (
                CommandId::ToggleVisible,
                "表示/非表示",
                "ひょうじ ひひょうじ かくす だす visibility hide show",
                self.selection_is_empty()
                    .then_some("隠す・出すボディか機能を選んでください"),
            ),
            (
                CommandId::LookAt,
                "正面から見る",
                "しょうめん せいたい みる look at めん",
                self.look_at_target().err(),
            ),
        ];
        let mut out: Vec<Command> = v
            .into_iter()
            .map(|(id, name, aliases, disabled)| Command {
                id,
                name: name.to_string(),
                aliases: aliases.to_string(),
                disabled: disabled.map(String::from),
            })
            .collect();
        for a in self.actions().into_iter().filter(|a| self.belongs_here(a)) {
            out.push(Command {
                id: CommandId::Action(a.id),
                name: a.label,
                aliases: a.aliases,
                disabled: a.disabled,
            });
        }
        // スケッチを直している間は、帯の道具も探せる
        if self.view_editor_type().is_some() {
            for (id, label, hint, enabled, _) in &self.search.view_tools {
                out.push(Command {
                    id: CommandId::EditorTool(id.clone()),
                    name: label.clone(),
                    aliases: hint.clone(),
                    disabled: (!enabled).then(|| "先に図の要素を選んでください".to_string()),
                });
            }
        }
        for (type_id, label, why) in self.addable(kmb_core::category::SOLID) {
            let aliases = self
                .win
                .registry
                .descriptor(&type_id)
                .map(|d| d.aliases)
                .filter(|a| !a.is_empty())
                .unwrap_or_else(|| type_id.clone());
            out.push(Command {
                aliases,
                id: CommandId::Add(type_id),
                name: label,
                disabled: why,
            });
        }
        out
    }

    /// `q` に合うコマンドを、良く合う順(同じなら使えるものを先)に並べる。
    pub fn find_commands(&self, q: &str) -> Vec<Command> {
        let mut v: Vec<(usize, usize, Command)> = self
            .commands()
            .into_iter()
            .enumerate()
            .filter_map(|(i, c)| {
                score(&c, q).map(|s| (s * 2 + c.disabled.is_some() as usize, i, c))
            })
            .collect();
        v.sort_by_key(|(s, i, _)| (*s, *i));
        v.into_iter().map(|(_, _, c)| c).collect()
    }

    pub fn run_command(&mut self, id: CommandId) {
        match id {
            CommandId::New => self.new_document(),
            CommandId::Open => self.open_document(),
            CommandId::Save => self.save(),
            CommandId::SaveAs => self.save_as(),
            CommandId::Undo => self.undo(),
            CommandId::Redo => self.redo(),
            CommandId::UndoHistory => self.toggle_undo_history(),
            CommandId::Settings => self.settings.open = !self.settings.open,
            CommandId::StartScreen => self.show_home(),
            CommandId::Measure => self.toggle_measure(),
            CommandId::Section => self.toggle_section(),
            CommandId::ToggleVisible => self.toggle_selected_visible(),
            CommandId::LookAt => self.look_at_selected(),
            CommandId::Action(a) => {
                self.start_action(&a);
            }
            CommandId::Add(t) => {
                self.add_described(&t);
            }
            CommandId::EditorTool(id) => self.use_editor_tool(&id),
        }
    }

    /// コマンド検索の窓を開く・閉じる(画面の上の真ん中に出す)。
    pub fn toggle_command_palette(&mut self) {
        self.search.at = None;
        self.search.text = match self.search.text {
            Some(_) => None,
            None => Some(String::new()),
        };
    }

    /// コマンド検索の窓をカーソルの所 `at` に開く(`None` なら画面の上)。
    /// 開いていれば閉じる。
    pub fn open_command_palette_at(&mut self, at: Option<egui::Pos2>) {
        if self.search.text.take().is_some() {
            return;
        }
        self.search.text = Some(String::new());
        self.search.at = at;
        log::info!("コマンド検索を開く: {at:?}");
    }

    /// コマンドが置いてあるツールバーの場所(「ソリッド › 作成」)。無ければ空。
    pub(crate) fn place_of(&self, id: &CommandId, places: &[(String, String)]) -> String {
        let key = match id {
            CommandId::Action(a) => format!("a:{a}"),
            CommandId::Add(t) => format!("f:{t}"),
            CommandId::Measure => "u:measure".into(),
            CommandId::Section => "u:section".into(),
            CommandId::EditorTool(t) => {
                return self
                    .search
                    .view_tools
                    .iter()
                    .find(|v| v.0 == *t)
                    .map(|v| format!("スケッチ › {}", v.4))
                    .unwrap_or_default();
            }
            CommandId::New | CommandId::Open | CommandId::Save | CommandId::SaveAs => {
                return "ファイル".into();
            }
            CommandId::LookAt | CommandId::ToggleVisible => return "ビュー".into(),
            _ => return String::new(),
        };
        places
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, p)| p.clone())
            .unwrap_or_default()
    }

    /// ツールバーの項目の鍵と、その場所(「タブ › まとまり」)。
    pub(crate) fn ribbon_places(&self) -> Vec<(String, String)> {
        self.ribbon_tabs()
            .into_iter()
            .flat_map(|t| {
                t.groups.into_iter().flat_map(move |g| {
                    let place = if g.name == t.name {
                        t.name.clone()
                    } else {
                        format!("{} › {}", t.name, g.name)
                    };
                    g.items.into_iter().map(move |i| (i.key, place.clone()))
                })
            })
            .collect()
    }

    pub fn is_command_palette_open(&self) -> bool {
        self.search.text.is_some()
    }

    /// コマンド検索の窓。毎フレーム呼ぶ。
    pub(crate) fn command_palette(&mut self, ctx: &egui::Context) {
        let Some(mut q) = self.search.text.take() else {
            return;
        };
        let found = self.find_commands(&q);
        let places = self.ribbon_places();
        let right: Vec<String> = found
            .iter()
            .map(|c| {
                let (p, k) = (self.place_of(&c.id, &places), self.key_of(&c.id));
                match (p.is_empty(), k.is_empty()) {
                    (false, false) => format!("{p}   {k}"),
                    (false, true) => p,
                    _ => k.to_string(),
                }
            })
            .collect();
        let mut run = None;
        let mut open = true;
        let win = egui::Window::new("コマンド検索")
            .collapsible(false)
            .resizable(true)
            .default_width(400.0);
        // S で開いた時はカーソルの所(欄がカーソルの少し下に来るように)、Ctrl+K は画面の上の真ん中
        let win = match self.search.at {
            // 動かせる窓では、egui 0.36 が覚えた前の位置で current_pos を上書きするので動かさない
            Some(p) => win.movable(false).current_pos(p - egui::vec2(24.0, 18.0)),
            None => win.anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0)),
        };
        win.open(&mut open).show(ctx, |ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut q)
                    .hint_text("名前や読みで探す(例: おしだし、pocket)")
                    .desired_width(f32::INFINITY),
            );
            // Enter で最初に押せる候補を実行する(欄の焦点を毎フレーム戻すので、焦点が外れたかでは見ない)
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            r.request_focus();
            if enter {
                run = found
                    .iter()
                    .find(|c| c.disabled.is_none())
                    .map(|c| c.id.clone());
            }
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (c, right) in found.iter().zip(&right) {
                        let b = ui.add_enabled(
                            c.disabled.is_none(),
                            // 行を窓の幅に広げ、場所とキーを右に寄せる
                            egui::Button::new(c.name.as_str())
                                .shortcut_text(right.as_str())
                                .min_size(egui::vec2(ui.available_width(), 0.0)),
                        );
                        if let Some(why) = &c.disabled {
                            b.on_disabled_hover_text(why);
                        } else if b.clicked() {
                            run = Some(c.id.clone());
                        }
                    }
                    if found.is_empty() {
                        ui.weak("合うコマンドがありません");
                    }
                });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        if let Some(id) = run {
            self.run_command(id);
        } else if open {
            self.search.text = Some(q);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(name: &str, aliases: &str) -> Command {
        Command {
            id: CommandId::New,
            name: name.into(),
            aliases: aliases.into(),
            disabled: None,
        }
    }

    #[test]
    fn commands_match_by_name_reading_and_skipped_letters() {
        let c = cmd("押し出し", "おしだし extrude");
        assert_eq!(score(&c, "押し"), Some(0));
        assert_eq!(score(&c, "出し"), Some(1));
        assert_eq!(score(&c, "おし"), Some(2));
        assert_eq!(score(&c, "EXT"), Some(2));
        assert_eq!(score(&c, "extd"), Some(3));
        assert_eq!(score(&c, "pocket"), None);
    }
}
