//! 式の欄の名前の候補(U51・K19)。打っている名前の頭の文字で、パラメータ・寸法・関数の名前を
//! 欄の下に出す。大かっこの中(`5 [c`)では単位の名前を出す。↑↓ で選び、Tab か Enter で入れ、
//! Esc で閉じる。候補をクリックしても入り、欄からキーボードは外れない。
//!
//! 欄の作り方はそのままにして前後に挟む: [`begin`] で開いている時のキーを先に取り、欄に
//! [`Completion::filter`] を掛けて置き、すぐ後に [`Completion::end`] で候補を出す。文書の名前は
//! 画面([`crate::Workspace`])が毎フレーム [`set_names`] で渡す。

use egui::{Id, Key, Modifiers};
use std::sync::Arc;

/// 候補 1 つ。
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub name: String,
    /// 右に薄く出す説明(今の値、「寸法」「関数」など)
    pub note: String,
    /// 関数(入れる時に `(` を続ける)
    pub call: bool,
}

impl Candidate {
    pub fn new(name: impl Into<String>, note: impl Into<String>) -> Self {
        Candidate {
            name: name.into(),
            note: note.into(),
            call: false,
        }
    }
}

/// 一度に出す候補の数。
const MAX_ROWS: usize = 8;

fn names_id() -> Id {
    Id::new("jc_expr_candidates")
}

/// 文書の名前(パラメータ・寸法)を、このフレームの候補にする。
pub fn set_names(ctx: &egui::Context, names: Vec<Candidate>) {
    ctx.data_mut(|d| d.insert_temp(names_id(), Arc::new(names)));
}

/// 関数と定数の候補(式で使える組み込みの名前)。
fn builtins() -> Vec<Candidate> {
    const CONSTANTS: [&str; 4] = ["pi", "PI", "π", "E"];
    kmb_core::BUILTIN_NAMES
        .iter()
        .map(|n| {
            if CONSTANTS.contains(n) {
                Candidate::new(*n, "定数")
            } else {
                Candidate {
                    call: true,
                    ..Candidate::new(*n, "関数")
                }
            }
        })
        .collect()
}

/// 名前に使える字か(字・数字・下線。日本語の名前も使える)。
fn name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// カーソルの前で打っている名前(先頭の字の位置と字)と、大かっこの中か。
fn token(text: &str, cursor: usize) -> (usize, String, bool) {
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    let mut start = cursor;
    while start > 0 && name_char(chars[start - 1]) {
        start -= 1;
    }
    // 数字で始まる語(`2`、`10mm` など)は名前ではないので候補を出さない
    if chars.get(start).is_some_and(char::is_ascii_digit) {
        start = cursor;
    }
    let word: String = chars[start..cursor].iter().collect();
    let before: String = chars[..start].iter().collect();
    let in_unit = before
        .rfind('[')
        .is_some_and(|o| before[o..].find(']').is_none());
    (start, word, in_unit)
}

/// `word` で始まる候補(前に合う物が先、次に途中に含む物)。名前の重なりは先の物を残す。
fn matches(word: &str, all: &[Candidate]) -> Vec<Candidate> {
    let w = word.to_lowercase();
    let mut seen = std::collections::HashSet::new();
    let all: Vec<&Candidate> = all.iter().filter(|c| seen.insert(c.name.clone())).collect();
    let mut out: Vec<Candidate> = all
        .iter()
        .filter(|c| c.name.to_lowercase().starts_with(&w))
        .map(|c| (*c).clone())
        .collect();
    out.extend(
        all.iter()
            .filter(|c| {
                let n = c.name.to_lowercase();
                !n.starts_with(&w) && n.contains(&w)
            })
            .map(|c| (*c).clone()),
    );
    out.truncate(MAX_ROWS);
    out
}

/// 欄ごとの覚え(画面の一時の記憶)。
#[derive(Clone, Debug, Default)]
struct State {
    /// 前のフレームで候補を出していたか
    open: bool,
    sel: usize,
    /// Esc で閉じた時の名前の位置と字(打ち足すとまた出す)
    closed: Option<(usize, String)>,
}

fn state_id(id: Id) -> Id {
    id.with("jc_complete")
}

/// 開いている候補への操作。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Up,
    Down,
    Accept,
    Close,
}

/// 開いている間、欄が Tab・Esc・↑↓ でキーボードを手放さないようにする。
const LOCK: egui::EventFilter = egui::EventFilter {
    tab: true,
    horizontal_arrows: true,
    vertical_arrows: true,
    escape: true,
};

/// 1 つの欄の候補(欄を置く前に [`begin`] で作る)。
pub struct Completion {
    id: Id,
    open: bool,
    act: Option<Act>,
    except: Option<String>,
}

/// 欄 `id` を置く前に呼ぶ。候補が開いていれば、↑↓・Tab・Enter・Esc を欄より先に取る。
pub fn begin(ui: &egui::Ui, id: Id) -> Completion {
    let st: State = ui.data(|d| d.get_temp(state_id(id))).unwrap_or_default();
    let open = st.open && ui.memory(|m| m.has_focus(id));
    let mut act = None;
    if open {
        ui.input_mut(|i| {
            act = if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                Some(Act::Down)
            } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                Some(Act::Up)
            } else if i.consume_key(Modifiers::NONE, Key::Tab)
                || i.consume_key(Modifiers::NONE, Key::Enter)
            {
                Some(Act::Accept)
            } else if i.consume_key(Modifiers::NONE, Key::Escape) {
                Some(Act::Close)
            } else {
                None
            };
        });
    }
    Completion {
        id,
        open,
        act,
        except: None,
    }
}

impl Completion {
    /// 名前 `name` を候補に出さない(直している寸法の自分の名前など。自分を指す式は作れない)。
    pub fn except(mut self, name: Option<&str>) -> Self {
        self.except = name.map(str::to_string);
        self
    }

    /// 欄の作りに掛ける(欄の名前を `id` にし、開いている間はキーボードを手放さない)。
    pub fn filter<'t>(&self, edit: egui::TextEdit<'t>) -> egui::TextEdit<'t> {
        let edit = edit.id(self.id);
        if self.open {
            edit.event_filter(LOCK)
        } else {
            edit
        }
    }

    /// 欄を置いたすぐ後に呼ぶ(欄の入力 `text` を直すことがある)。`extra` は欄ごとに足す候補
    /// (スケッチの中の寸法など。文書の名前より先に出す)。候補を入れたら真。
    pub fn end(
        self,
        ui: &egui::Ui,
        response: &egui::Response,
        text: &mut String,
        extra: &[Candidate],
    ) -> bool {
        let id = self.id;
        let sid = state_id(id);
        let mut st: State = ui.data(|d| d.get_temp(sid)).unwrap_or_default();
        let focused = response.has_focus()
            || (self.open && response.lost_focus() && ui.input(|i| i.pointer.any_click()));
        let cursor = egui::TextEdit::load_state(ui.ctx(), id)
            .and_then(|s| s.cursor.char_range())
            .filter(|r| r.primary == r.secondary)
            .map(|r| r.primary.index.0);
        let mut found = Vec::new();
        let mut at = (0, String::new());
        if focused && let Some(c) = cursor {
            let (start, word, in_unit) = token(text, c);
            if !word.is_empty() && st.closed.as_ref() != Some(&(start, word.clone())) {
                let all: Vec<Candidate> = if in_unit {
                    kmb_core::UNIT_NAMES
                        .iter()
                        .map(|u| Candidate::new(*u, "単位"))
                        .collect()
                } else {
                    let doc: Arc<Vec<Candidate>> =
                        ui.data(|d| d.get_temp(names_id())).unwrap_or_default();
                    extra
                        .iter()
                        .cloned()
                        .chain(doc.iter().cloned())
                        .chain(builtins())
                        .collect()
                };
                let all: Vec<Candidate> = all
                    .into_iter()
                    .filter(|c| self.except.as_deref() != Some(c.name.as_str()))
                    .collect();
                found = matches(&word, &all);
                // 打ち終えた名前だけが残ったら出さない(Enter で欄を決められるように)
                if found.len() == 1 && found[0].name == word && !found[0].call {
                    found.clear();
                }
            }
            if !word.is_empty() || st.closed.is_some() {
                at = (start, word);
            }
            if st.closed.as_ref().is_some_and(|c| *c != at) {
                st.closed = None;
            }
        }
        if found.is_empty() {
            ui.data_mut(|d| {
                d.insert_temp(
                    sid,
                    State {
                        open: false,
                        sel: 0,
                        ..st
                    },
                )
            });
            return false;
        }
        st.sel = st.sel.min(found.len() - 1);
        let mut pick = None;
        match self.act {
            Some(Act::Down) => st.sel = (st.sel + 1) % found.len(),
            Some(Act::Up) => st.sel = (st.sel + found.len() - 1) % found.len(),
            Some(Act::Accept) => pick = Some(st.sel),
            Some(Act::Close) => {
                st.closed = Some(at.clone());
                st.open = false;
                ui.data_mut(|d| d.insert_temp(sid, st));
                return false;
            }
            None => {}
        }
        if pick.is_none() {
            let area = egui::Area::new(id.with("jc_complete_list"))
                .order(egui::Order::Tooltip)
                .fixed_pos(response.rect.left_bottom() + egui::vec2(0.0, 2.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .show(ui, |ui| {
                            ui.set_min_width(response.rect.width().max(120.0));
                            let mut clicked = None;
                            for (i, c) in found.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    let r = ui.selectable_label(i == st.sel, &c.name);
                                    if !c.note.is_empty() {
                                        ui.label(egui::RichText::new(&c.note).small().weak());
                                    }
                                    if r.clicked() {
                                        clicked = Some(i);
                                    }
                                    if r.hovered() {
                                        st.sel = i;
                                    }
                                });
                            }
                            ui.label(
                                egui::RichText::new("↑↓ で選び Tab・Enter で入れる。Esc で閉じる")
                                    .small()
                                    .weak(),
                            );
                            clicked
                        })
                        .inner
                });
            pick = area.inner;
        }
        let Some(i) = pick else {
            st.open = true;
            ui.data_mut(|d| d.insert_temp(sid, st));
            // 開いた最初のフレームから Esc・Tab で欄を離れないように
            ui.memory_mut(|m| m.set_focus_lock_filter(id, LOCK));
            return false;
        };
        let c = &found[i];
        let (start, word) = &at;
        let chars: Vec<char> = text.chars().collect();
        let end = (start + word.chars().count()).min(chars.len());
        let mut new: String = chars[..*start].iter().collect();
        new.push_str(&c.name);
        if c.call && chars.get(end) != Some(&'(') {
            new.push('(');
        }
        let caret = new.chars().count();
        new.extend(&chars[end..]);
        log::info!("式の欄に名前の候補を入れる: {}", c.name);
        *text = new;
        if let Some(mut s) = egui::TextEdit::load_state(ui.ctx(), id) {
            s.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(caret),
            )));
            s.store(ui.ctx(), id);
        }
        // クリックで欄から外れた時も、打ち続けられるようにキーボードを戻す
        response.request_focus();
        ui.data_mut(|d| d.insert_temp(sid, State::default()));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_the_name_before_the_cursor() {
        assert_eq!(token("wid", 3), (0, "wid".into(), false));
        assert_eq!(token("2 * hei + 1", 7), (4, "hei".into(), false));
        assert_eq!(token("5 [c", 4), (3, "c".into(), true));
        assert_eq!(token("5 [cm] + w", 10), (9, "w".into(), false));
        assert_eq!(token("幅a", 2), (0, "幅a".into(), false));
        assert_eq!(token("1 + ", 4).1, "");
        assert_eq!(token("width / 2", 9).1, "", "数字は名前ではない");
        assert_eq!(token("10mm", 4).1, "");
    }

    #[test]
    fn prefix_matches_come_first() {
        let all = vec![
            Candidate::new("height", ""),
            Candidate::new("width", ""),
            Candidate::new("wall", ""),
            Candidate::new("width", "重なり"),
        ];
        let m = matches("w", &all);
        let names: Vec<&str> = m.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["width", "wall"]);
        let m = matches("ei", &all);
        assert_eq!(m[0].name, "height");
        assert_eq!(matches("W", &all).len(), 2, "大文字小文字は問わない");
    }
}
