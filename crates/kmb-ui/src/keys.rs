//! 画面のキーの一覧(U2)。キーで始める物、コマンド検索の行の右とツールバーの説明に出すキー、
//! 操作の手引き(F1)の表は、どれもこの表から作る(別々に持つと食い違う)。
//! 1 文字のキーは I=測る、V=表示の切り替え、S=検索。F は Kie の決定で
//! 回す中心のまま。スケッチの中のキーもこの表に置き、スケッチの
//! 編集器が行の名前([`pressed`]・[`label`])で読む。
//!
//! 割り当ては設定の「マウスとキー」で付け替えられる(U36)。付け替えた物だけを `layout.json` の
//! `keys` に覚え([`KeyMap`])、書いていない行は既定の割り当てのまま。

use crate::Workspace;
use crate::commands_ui::CommandId;
use egui::{Key, Modifiers};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// キーで起こすこと。
pub(crate) enum Does {
    /// 役割の印で探す操作(ツールバーの項目)を始める
    Role(kmb_core::ActionRole),
    /// 役割の印で探す機能の種類(ツールバーの項目)を始める
    Kind(kmb_core::KindRole),
    /// ツールバーの項目(鍵)を始める
    Item(&'static str),
    /// 画面のコマンド
    Run(Cmd),
    /// スケッチの編集器が `id` で読む
    Sketch,
    /// ほかの所で扱う(手引きに出すだけ)
    Note,
}

/// 画面のコマンドのうち、キーを割り当てたもの。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    /// 今の文書のタブを閉じる(候補 20)
    CloseTab,
    /// 次・前の文書のタブへ
    NextTab,
    PrevTab,
    Undo,
    Redo,
    Recompute,
    /// コマンド検索を画面の上に出す
    Search,
    /// コマンド検索をカーソルの所に出す
    SearchHere,
    Measure,
    /// 選んだボディ・機能を隠す・出す
    ToggleVisible,
    Group,
    Params,
    /// 設定の窓を開く・閉じる
    Settings,
}

/// どこで効くか。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// 文字の欄を打っている時も効く(Ctrl のキー)
    Always,
    /// 文字の欄を打っていない時だけ
    View,
    /// 3D のビューで、作る途中でも編集器の中でもない時だけ(1 文字のキー)
    Model,
    /// スケッチの編集器の中(編集器が自分で読む。S・I・E は 3D のビューと同じ行を使う)
    Sketch,
}

/// キーの 1 行。`press` が空なら手引きに出すだけ。
pub(crate) struct Row {
    /// 割り当てを覚える名前(`layout.json` の `keys` の鍵)。空なら付け替えられない(手引きに出すだけ)
    pub id: &'static str,
    /// 手引きとコマンド検索に出すキーの名前(既定の割り当て)
    pub keys: &'static str,
    /// 手引きに出す働き
    pub text: &'static str,
    pub press: &'static [(Modifiers, Key)],
    pub scope: Scope,
    pub does: Does,
}

const NONE: Modifiers = Modifiers::NONE;
const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers {
    alt: false,
    ctrl: false,
    shift: true,
    mac_cmd: false,
    command: true,
};

/// 画面のキーの表(手引きにはこの順に出す)。同じキーの長い組み合わせを先に置く
/// (Ctrl+Shift+S を Ctrl+S と取り違えない)。
pub(crate) static KEYS: &[Row] = &[
    Row {
        id: "search_here",
        keys: "S",
        text: "カーソルの所にコマンド検索を出す(名前や読みで探す。例「おしだし」)",
        press: &[(NONE, Key::S)],
        scope: Scope::Model,
        does: Does::Run(Cmd::SearchHere),
    },
    Row {
        id: "search",
        keys: "Ctrl+K",
        text: "コマンド検索を画面の上に出す",
        press: &[(CTRL, Key::K)],
        scope: Scope::Always,
        does: Does::Run(Cmd::Search),
    },
    Row {
        id: "",
        keys: "Enter",
        text: "選んだ機能を直し始める。作る途中・直す途中なら決める(OK)",
        press: &[],
        scope: Scope::View,
        does: Does::Note,
    },
    Row {
        id: "",
        keys: "Esc",
        text: "1 段ずつ戻す: 作る途中をやめる → 選んだ面・辺 → 選んだ機能",
        press: &[],
        scope: Scope::View,
        does: Does::Note,
    },
    Row {
        id: "",
        keys: "Delete",
        text: "選んだ機能を消す",
        press: &[],
        scope: Scope::View,
        does: Does::Note,
    },
    Row {
        id: "",
        keys: "F",
        text: "選んだ面か機能の中心を回す中心にする(拡大はしない)",
        press: &[],
        scope: Scope::View,
        does: Does::Note,
    },
    Row {
        id: "",
        keys: "Home",
        text: "全体表示",
        press: &[],
        scope: Scope::View,
        does: Does::Note,
    },
    Row {
        id: "extrude",
        keys: "E",
        text: "押し出しを始める(スケッチの中では選んだ領域を押し出す)",
        press: &[(NONE, Key::E)],
        scope: Scope::Model,
        does: Does::Role(kmb_interchange::role::action::EXTRUDE),
    },
    Row {
        id: "hole",
        keys: "H",
        text: "穴を始める",
        press: &[(NONE, Key::H)],
        scope: Scope::Model,
        does: Does::Role(kmb_interchange::role::action::HOLE_ON_FACE),
    },
    Row {
        id: "press_pull",
        keys: "Q",
        text: "プレス/プル(選んだ面を押し出す、辺を丸める、面から作った機能を直す)",
        press: &[(NONE, Key::Q)],
        scope: Scope::Model,
        does: Does::Item(crate::press_pull_ui::PRESS_PULL),
    },
    Row {
        id: "measure",
        keys: "I",
        text: "計測",
        press: &[(NONE, Key::I)],
        scope: Scope::Model,
        does: Does::Run(Cmd::Measure),
    },
    // A は外観
    Row {
        id: "appearance",
        keys: "A",
        text: "外観の窓を開く(ボディ・部品・面に色と質感を付ける)",
        press: &[(NONE, Key::A)],
        scope: Scope::Model,
        does: Does::Role(kmb_interchange::role::action::APPEARANCE),
    },
    // M は移動/コピー(計測は I。Kie 2026-10-22)
    Row {
        id: "move",
        keys: "M",
        text: "移動/コピーを始める",
        press: &[(NONE, Key::M)],
        scope: Scope::Model,
        does: Does::Kind(kmb_interchange::role::kind::COPY_BODY),
    },
    Row {
        id: "toggle_visible",
        keys: "V",
        text: "選んだボディ・機能を隠す・出す",
        press: &[(NONE, Key::V)],
        scope: Scope::Model,
        does: Does::Run(Cmd::ToggleVisible),
    },
    Row {
        id: "",
        keys: "右クリック",
        text: "「繰り返す」で最後に使ったコマンドをもう一度始める",
        press: &[],
        scope: Scope::View,
        does: Does::Note,
    },
    Row {
        id: "redo",
        keys: "Ctrl+Y",
        text: "やり直す",
        press: &[(CTRL, Key::Y)],
        scope: Scope::Always,
        does: Does::Run(Cmd::Redo),
    },
    Row {
        id: "redo_alt",
        keys: "Ctrl+Shift+Z",
        text: "やり直す",
        press: &[(CTRL_SHIFT, Key::Z)],
        scope: Scope::Always,
        does: Does::Run(Cmd::Redo),
    },
    Row {
        id: "undo",
        keys: "Ctrl+Z",
        text: "元に戻す",
        press: &[(CTRL, Key::Z)],
        scope: Scope::Always,
        does: Does::Run(Cmd::Undo),
    },
    Row {
        id: "",
        keys: "Ctrl+C / Ctrl+X / Ctrl+V",
        text: "スケッチの中で、選んだ線のコピー / 切り取り / 貼り付け(カーソルの所へ)",
        press: &[],
        scope: Scope::View,
        does: Does::Note,
    },
    Row {
        id: "save_as",
        keys: "Ctrl+Shift+S",
        text: "名前を付けて保存",
        press: &[(CTRL_SHIFT, Key::S)],
        scope: Scope::Always,
        does: Does::Run(Cmd::SaveAs),
    },
    Row {
        id: "save",
        keys: "Ctrl+S",
        text: "保存",
        press: &[(CTRL, Key::S)],
        scope: Scope::Always,
        does: Does::Run(Cmd::Save),
    },
    Row {
        id: "new",
        keys: "Ctrl+N",
        text: "新規デザイン",
        press: &[(CTRL, Key::N)],
        scope: Scope::Always,
        does: Does::Run(Cmd::New),
    },
    Row {
        id: "open",
        keys: "Ctrl+O",
        text: "開く",
        press: &[(CTRL, Key::O)],
        scope: Scope::Always,
        does: Does::Run(Cmd::Open),
    },
    Row {
        id: "close_tab",
        keys: "Ctrl+W",
        text: "今の文書のタブを閉じる",
        press: &[(CTRL, Key::W)],
        scope: Scope::Always,
        does: Does::Run(Cmd::CloseTab),
    },
    Row {
        id: "next_tab",
        keys: "Ctrl+PageDown",
        text: "次の文書のタブへ",
        press: &[(CTRL, Key::PageDown)],
        scope: Scope::Always,
        does: Does::Run(Cmd::NextTab),
    },
    Row {
        id: "prev_tab",
        keys: "Ctrl+PageUp",
        text: "前の文書のタブへ",
        press: &[(CTRL, Key::PageUp)],
        scope: Scope::Always,
        does: Does::Run(Cmd::PrevTab),
    },
    Row {
        id: "recompute",
        keys: "Ctrl+B",
        text: "全部を計算し直す",
        press: &[(CTRL, Key::B)],
        scope: Scope::Always,
        does: Does::Run(Cmd::Recompute),
    },
    Row {
        id: "group",
        keys: "Ctrl+G",
        text: "選んだ機能をタイムラインでまとめる",
        press: &[(CTRL, Key::G)],
        scope: Scope::View,
        does: Does::Run(Cmd::Group),
    },
    Row {
        id: "params",
        keys: "Ctrl+P",
        text: "パラメータの一覧を出す・隠す",
        press: &[(CTRL, Key::P)],
        scope: Scope::View,
        does: Does::Run(Cmd::Params),
    },
    Row {
        id: "settings",
        keys: "Ctrl+,",
        text: "設定の窓を開く・閉じる",
        press: &[(CTRL, Key::Comma)],
        scope: Scope::Always,
        does: Does::Run(Cmd::Settings),
    },
    Row {
        id: "",
        keys: "数字",
        text: "クリックで作る途中なら、そのまま値を打てる(欄を押さなくてよい)",
        press: &[],
        scope: Scope::View,
        does: Does::Note,
    },
    Row {
        id: "sketch_line",
        keys: "L",
        text: "スケッチ: 線分",
        press: &[(NONE, Key::L)],
        scope: Scope::Sketch,
        does: Does::Sketch,
    },
    Row {
        id: "sketch_rect",
        keys: "R",
        text: "スケッチ: 長方形",
        press: &[(NONE, Key::R)],
        scope: Scope::Sketch,
        does: Does::Sketch,
    },
    Row {
        id: "sketch_circle",
        keys: "C",
        text: "スケッチ: 円",
        press: &[(NONE, Key::C)],
        scope: Scope::Sketch,
        does: Does::Sketch,
    },
    Row {
        id: "sketch_point",
        keys: "P",
        text: "スケッチ: 点",
        press: &[(NONE, Key::P)],
        scope: Scope::Sketch,
        does: Does::Sketch,
    },
    Row {
        id: "sketch_trim",
        keys: "T",
        text: "スケッチ: トリム",
        press: &[(NONE, Key::T)],
        scope: Scope::Sketch,
        does: Does::Sketch,
    },
    Row {
        id: "sketch_offset",
        keys: "O",
        text: "スケッチ: オフセット",
        press: &[(NONE, Key::O)],
        scope: Scope::Sketch,
        does: Does::Sketch,
    },
    Row {
        id: "sketch_dimension",
        keys: "D",
        text: "スケッチ: 寸法(3D スケッチでは選んだ線に寸法を付ける)",
        press: &[(NONE, Key::D)],
        scope: Scope::Sketch,
        does: Does::Sketch,
    },
    Row {
        id: "sketch_construction",
        keys: "X",
        text: "スケッチ: 選んだ線を補助線と切り替える。描く道具の中では次に描く線を補助線にする",
        press: &[(NONE, Key::X)],
        scope: Scope::Sketch,
        does: Does::Sketch,
    },
    Row {
        id: "",
        keys: "1 / 2 / 3",
        text: "スケッチの選ぶ道具で、空いた所から引いた時の選び方(窓・自由形状・なぞる)",
        press: &[],
        scope: Scope::Sketch,
        does: Does::Note,
    },
    Row {
        id: "",
        keys: "F1",
        text: "この手引き",
        press: &[],
        scope: Scope::Always,
        does: Does::Note,
    },
];

/// キーの組み合わせ(修飾キーとキー)。
pub type Chord = (Modifiers, Key);

/// 付け替えられないキー。値を打つ・欄を動く・写す・拡大するのに使い、どこでも同じ働きにしておく。
/// 写す・切り取る・貼り付けの組み合わせを断る理由。
pub(crate) const COPY_KEYS: &str =
    "写す・切り取る・貼り付けるキー(Shift・Alt を足しても PC が写す働きとして渡す)";

fn reserved(ch: Chord) -> Option<&'static str> {
    let (m, k) = ch;
    let plain = !m.command && !m.ctrl && !m.alt;
    use Key::*;
    match k {
        Escape | Enter | Tab | Backspace | Delete | F1 | Home => Some("決まった働きのキー"),
        ArrowUp | ArrowDown | ArrowLeft | ArrowRight => Some("欄やメニューを動くキー"),
        Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9 | Minus | Period
        | Plus | Equals | Slash | Space
            if plain =>
        {
            Some("値を打つのに使うキー")
        }
        F if plain && !m.shift => Some("回す中心を決めるキー"),
        C | X | V | A if (m.command || m.ctrl) && !m.shift && !m.alt => {
            Some("写す・切り取る・貼り付ける・全部選ぶキー")
        }
        // Shift・Alt を足しても、PC は写す・切り取る・貼り付けるとして渡し、キーとしては来ない
        C | X | V | Insert if m.command || m.ctrl => Some(COPY_KEYS),
        Plus | Equals | Minus | Num0 if m.command || m.ctrl => Some("画面の拡大と縮小のキー"),
        _ => None,
    }
}

/// 押したキーの修飾が、割り当てと同じか(Ctrl は Windows の Ctrl と Mac の Cmd のどちらでもよい)。
fn same_mods(pressed: Modifiers, want: Modifiers) -> bool {
    (pressed.command || pressed.ctrl || pressed.mac_cmd)
        == (want.command || want.ctrl || want.mac_cmd)
        && pressed.shift == want.shift
        && pressed.alt == want.alt
}

/// キーの組み合わせの名前(例「Ctrl+Shift+S」「L」)。
pub fn chord_name(ch: Chord) -> String {
    let (m, k) = ch;
    let mut s = String::new();
    if m.command || m.ctrl || m.mac_cmd {
        s.push_str("Ctrl+");
    }
    if m.shift {
        s.push_str("Shift+");
    }
    if m.alt {
        s.push_str("Alt+");
    }
    s.push_str(k.symbol_or_name());
    s
}

/// 名前からキーの組み合わせを読む([`chord_name`] の逆)。
pub fn parse_chord(s: &str) -> Option<Chord> {
    let mut m = NONE;
    let mut rest = s.trim();
    loop {
        if let Some(r) = rest.strip_prefix("Ctrl+") {
            m.command = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("Shift+") {
            m.shift = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("Alt+") {
            m.alt = true;
            rest = r;
        } else {
            break;
        }
    }
    Some((m, Key::from_name(rest)?))
}

/// 付け替えた後のキーの割り当て(行ごと。並びは [`KEYS`] と同じ)。
#[derive(Clone, Debug, PartialEq)]
pub struct KeyMap {
    binds: Vec<Vec<Chord>>,
}

impl Default for KeyMap {
    fn default() -> Self {
        KeyMap::new(&BTreeMap::new())
    }
}

impl KeyMap {
    /// 既定の割り当てに、設定で付け替えた物(行の `id` → キーの名前の並び。空の並びは外す)を当てる。
    /// 読めないキーと付け替えられないキーは飛ばす。
    pub fn new(over: &BTreeMap<String, Vec<String>>) -> KeyMap {
        let binds = KEYS
            .iter()
            .map(|r| match over.get(r.id).filter(|_| !r.id.is_empty()) {
                None => r.press.to_vec(),
                Some(names) => names
                    .iter()
                    .filter_map(|n| match parse_chord(n) {
                        Some(ch) if reserved(ch).is_none() => Some(ch),
                        _ => {
                            log::warn!("キーの割り当て {}: {n} は使えないので飛ばしました", r.id);
                            None
                        }
                    })
                    .collect(),
            })
            .collect();
        KeyMap { binds }
    }

    /// 行の割り当て。
    pub fn chords(&self, i: usize) -> &[Chord] {
        &self.binds[i]
    }

    /// `id` の行の割り当て(無い行なら空)。
    pub fn chords_of(&self, id: &str) -> &[Chord] {
        row_index(id).map_or(&[], |i| &self.binds[i])
    }

    /// 行のキーの名前(手引きと一覧に出す)。付け替えられない行は表の名前、外した行は空。
    pub fn label(&self, i: usize) -> String {
        if KEYS[i].id.is_empty() {
            return KEYS[i].keys.to_string();
        }
        self.binds[i]
            .iter()
            .map(|c| chord_name(*c))
            .collect::<Vec<_>>()
            .join(" / ")
    }

    /// `id` の行のキーの名前(無い行・外した行なら空)。
    pub fn label_of(&self, id: &str) -> String {
        row_index(id).map_or(String::new(), |i| self.label(i))
    }

    /// 行 `i` と同じキーを持つほかの行。同時に効かない行どうし(スケッチの中だけの行と、3D の
    /// ビューだけの行)は同じキーでも重なりにしない(10-24 u4)。
    pub fn conflicts(&self, i: usize) -> Vec<usize> {
        (0..self.binds.len())
            .filter(|&j| {
                j != i
                    && live_together(&KEYS[i], &KEYS[j])
                    && self.binds[j].iter().any(|c| self.binds[i].contains(c))
            })
            .collect()
    }

    /// 押したキー(修飾つき)が `id` の行の割り当てか。
    pub fn matches(&self, id: &str, key: Key, mods: Modifiers) -> bool {
        self.chords_of(id)
            .iter()
            .any(|(m, k)| *k == key && same_mods(mods, *m))
    }
}

/// 3D のビューの行のうち、スケッチの編集器の中でも効く行(編集器が `pressed`・`is` で読む。
/// 押し出し・その場で探す・測る)。
pub(crate) const SKETCH_ALSO: [&str; 3] = ["extrude", "search_here", "measure"];

/// 2 つの行が同時に効くことがあるか。スケッチの中の行は編集器の中だけ、3D のビューの行は
/// 編集器の外だけで読むので、編集器の中でも読む行(`SKETCH_ALSO`)のほかは同時に効かない。
fn live_together(a: &Row, b: &Row) -> bool {
    match (a.scope, b.scope) {
        (Scope::Sketch, Scope::Model) => SKETCH_ALSO.contains(&b.id),
        (Scope::Model, Scope::Sketch) => SKETCH_ALSO.contains(&a.id),
        _ => true,
    }
}

/// `id` の行の番号。
fn row_index(id: &str) -> Option<usize> {
    (!id.is_empty())
        .then(|| KEYS.iter().position(|r| r.id == id))
        .flatten()
}

/// 押したキーを、割り当てに使える組み合わせとして読む(付け替えられないキーなら理由)。
pub fn capture(key: Key, mods: Modifiers) -> Result<Chord, &'static str> {
    let ch = (
        Modifiers {
            command: mods.command || mods.ctrl || mods.mac_cmd,
            shift: mods.shift,
            alt: mods.alt,
            ..NONE
        },
        key,
    );
    match reserved(ch) {
        Some(why) => Err(why),
        None => Ok(ch),
    }
}

thread_local! {
    /// 今の割り当て(スケッチの編集器など、画面の外の部品が読む。画面が設定から作って置く)。
    /// 画面を描くスレッドごとに持つ(試験は並べて走るので、プロセスで 1 つだと割り当てが混ざる)。
    static CURRENT: RefCell<Option<Arc<KeyMap>>> = const { RefCell::new(None) };
}
/// 設定の窓でキーを受けている間は、どのキーも働かせない。
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// 今の割り当て。
pub fn current() -> Arc<KeyMap> {
    CURRENT.with(|c| c.borrow().clone()).unwrap_or_default()
}

fn publish(map: Arc<KeyMap>) {
    CURRENT.with(|c| *c.borrow_mut() = Some(map));
}

/// 設定の窓でキーを受けているか。
pub fn capturing() -> bool {
    CAPTURING.load(Ordering::Relaxed)
}

/// このフレームで `id` の行のキーが押されたか(スケッチの編集器が使う)。
pub fn pressed(ctx: &egui::Context, id: &str) -> bool {
    if capturing() {
        return false;
    }
    let map = current();
    let pressed = ctx.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, egui::Event::Key { key, pressed: true, modifiers, .. }
                if map.matches(id, *key, *modifiers))
        })
    });
    if pressed && let Some(i) = row_index(id) {
        crate::keycast::command_name(ctx, row_text(ctx, &KEYS[i]), true);
    }
    pressed
}

/// キーの表と操作の表示で使う、固定の働きの名前。
pub(crate) fn row_text(ctx: &egui::Context, row: &Row) -> String {
    if row.id.is_empty() {
        return row.text.to_string();
    }
    crate::window_state::Words::of(ctx)
        .t(&format!("settings-key-name-{}", row.id.replace('_', "-")))
}

/// 押したキーが `id` の行の割り当てか(イベントを自分で回す所が使う)。
pub fn is(id: &str, key: Key, mods: Modifiers) -> bool {
    !capturing() && current().matches(id, key, mods)
}

/// `id` の行のキーの名前(道具の説明やメニューに出す。外していれば空)。
pub fn label(id: &str) -> String {
    current().label_of(id)
}

/// 画面のコマンドのうち、キーの表の `Cmd` に当たるもの。
fn cmd_of(id: &CommandId) -> Option<Cmd> {
    Some(match id {
        CommandId::New => Cmd::New,
        CommandId::Open => Cmd::Open,
        CommandId::Save => Cmd::Save,
        CommandId::SaveAs => Cmd::SaveAs,
        CommandId::Undo => Cmd::Undo,
        CommandId::Redo => Cmd::Redo,
        CommandId::Measure => Cmd::Measure,
        CommandId::ToggleVisible => Cmd::ToggleVisible,
        CommandId::Settings => Cmd::Settings,
        _ => return None,
    })
}

impl Workspace {
    /// 設定の付け替えから割り当てを作り直す(変わった時だけ。毎フレームの初めに呼ぶ)。
    pub(crate) fn sync_keymap(&mut self) {
        if self.win.keymap_src.as_ref() != Some(&self.win.layout.keys) {
            self.win.keymap = Arc::new(KeyMap::new(&self.win.layout.keys));
            self.win.keymap_src = Some(self.win.layout.keys.clone());
        }
        publish(self.win.keymap.clone());
        CAPTURING.store(self.settings.key_capture.is_some(), Ordering::Relaxed);
    }

    /// キーの行が始めるツールバーの項目の鍵(「a:操作」など)。
    fn row_item(&self, does: &Does) -> Option<String> {
        match does {
            // 操作か、機能の種類(記述子の役割の印)
            Does::Role(r) => self.action_for_role(r).map(|a| format!("a:{a}")),
            Does::Kind(r) => self
                .win
                .registry
                .descriptors()
                .into_iter()
                .find(|d| d.role.as_ref() == Some(r))
                .map(|d| format!("f:{}", d.type_id)),
            Does::Item(k) => Some(k.to_string()),
            _ => None,
        }
    }

    /// ツールバーの項目 `key` に割り当てたキーの名前(外していれば `None`)。
    pub(crate) fn key_of_item(&self, key: &str) -> Option<String> {
        KEYS.iter()
            .position(|r| self.row_item(&r.does).as_deref() == Some(key))
            .map(|i| self.win.keymap.label(i))
            .filter(|s| !s.is_empty())
    }

    /// コマンド検索の行の右に出すキー(なければ空)。
    pub fn key_of(&self, id: &CommandId) -> String {
        if let Some(c) = cmd_of(id) {
            return KEYS
                .iter()
                .position(|r| matches!(r.does, Does::Run(x) if x == c))
                .map_or(String::new(), |i| self.win.keymap.label(i));
        }
        match id {
            CommandId::Action(a) => self.key_of_item(&format!("a:{a}")).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// 今のフレームで押された、`scope` のキーの行(押したキーを使い切る)。修飾キーは同じ物だけに合う
    /// (Ctrl+Shift+S を Ctrl+S と取り違えない)。
    pub(crate) fn pressed_key(&self, ctx: &egui::Context, scope: Scope) -> Option<&'static Row> {
        if self.settings.key_capture.is_some() {
            return None;
        }
        KEYS.iter().enumerate().find_map(|(n, r)| {
            if r.scope != scope || r.id.is_empty() {
                return None;
            }
            let hit = self.win.keymap.chords(n).iter().any(|(m, k)| {
                ctx.input_mut(|i| {
                    let at = i.events.iter().position(|e| {
                        matches!(e, egui::Event::Key { key, pressed: true, modifiers, .. }
                            if key == k && same_mods(*modifiers, *m))
                    });
                    if let Some(at) = at {
                        i.events.remove(at);
                    }
                    at.is_some()
                })
            });
            hit.then_some(r)
        })
    }

    /// キーの行の働きをする。
    pub(crate) fn run_key(&mut self, ctx: &egui::Context, row: &Row) {
        crate::keycast::command_name(ctx, row_text(ctx, row), true);
        log::info!("キー: {}({})", row.id, row.text);
        // 移動のキーは、部品を選んでいれば部品ごと動かす(10-22 a2)
        if matches!(row.does, Does::Kind(k) if k == kmb_interchange::role::kind::COPY_BODY)
            && self.move_part_by_key()
        {
            return;
        }
        match &row.does {
            Does::Role(_) | Does::Kind(_) | Does::Item(_) => match self.row_item(&row.does) {
                Some(key) => self.run_shortcut(&key),
                None => self.win.message = Some("この作業空間では使えないキーです".into()),
            },
            Does::Run(c) => match c {
                Cmd::New => self.new_document(),
                Cmd::Open => self.open_document(),
                Cmd::Save => self.save(),
                Cmd::SaveAs => self.save_as(),
                Cmd::CloseTab => {
                    self.close_tab(self.active_tab());
                }
                Cmd::NextTab => self.cycle_tab(1),
                Cmd::PrevTab => self.cycle_tab(-1),
                Cmd::Undo => self.undo(),
                Cmd::Redo => self.redo(),
                Cmd::Recompute => self.recompute(),
                Cmd::Search => self.toggle_command_palette(),
                Cmd::SearchHere => self.open_command_palette_at(ctx.pointer_latest_pos()),
                Cmd::Measure => self.toggle_measure(),
                Cmd::ToggleVisible => self.toggle_selected_visible(),
                Cmd::Group => {
                    let sel = self.selection();
                    if !sel.is_empty() {
                        self.group_features(&sel, kmb_session::edit::GroupMode::Exact);
                    }
                }
                Cmd::Params => {
                    self.win.show_params = !self.win.show_params;
                }
                Cmd::Settings => self.toggle_settings(),
            },
            Does::Sketch | Does::Note => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_written_once_and_the_one_letter_keys_are_there() {
        // 同じキーを 2 つの行に割り当てない(先の行だけが効き、後ろの行の説明が嘘になる)
        let map = KeyMap::default();
        for (i, r) in KEYS.iter().enumerate() {
            assert!(map.conflicts(i).is_empty(), "{} が重なる", r.keys);
            // 既定の名前は、割り当てから作る名前と同じ(手引きとコマンド検索が食い違わない)
            if !r.id.is_empty() {
                assert_eq!(map.label(i), r.keys, "{}", r.id);
                assert!(!r.press.is_empty(), "{}", r.id);
            }
            for c in r.press {
                assert_eq!(reserved(*c), None, "{}", r.keys);
            }
        }
        // 割り当てを覚える名前は重ならない
        let ids: Vec<&str> = KEYS
            .iter()
            .map(|r| r.id)
            .filter(|i| !i.is_empty())
            .collect();
        let set: std::collections::HashSet<&&str> = ids.iter().collect();
        assert_eq!(set.len(), ids.len());
        // スケッチの中でも読む行は 3D のビューの行(ほかの組は重なりの確かめで同時に効くと見る)
        for id in SKETCH_ALSO {
            assert!(
                row_index(id).is_some_and(|i| KEYS[i].scope == Scope::Model),
                "{id}"
            );
        }
        // 決まった 1 文字のキー(F は回す中心のまま)
        for k in ["S", "E", "H", "Q", "V"] {
            assert!(KEYS.iter().any(|r| r.keys == k), "{k}");
        }
        assert!(
            KEYS.iter()
                .any(|r| r.keys == "F" && matches!(r.does, Does::Note))
        );
    }

    #[test]
    fn chords_read_back_from_their_names() {
        for r in KEYS {
            for c in r.press {
                assert_eq!(parse_chord(&chord_name(*c)), Some(*c), "{}", r.keys);
            }
        }
        assert_eq!(chord_name((CTRL, Key::Comma)), "Ctrl+,");
        assert_eq!(parse_chord("Ctrl+Alt+K").map(|c| c.1), Some(Key::K));
        assert_eq!(parse_chord("Ctrl+"), None);
        assert_eq!(parse_chord("なに"), None);
    }

    #[test]
    fn remapping_moves_the_key_and_flags_the_same_key_on_two_rows() {
        let mut over = BTreeMap::new();
        // 押し出しを W に、測るを外す、線分を E に(押し出しの既定と重なる…が押し出しは W にした)
        over.insert("extrude".to_string(), vec!["W".to_string()]);
        over.insert("measure".to_string(), vec![]);
        over.insert("sketch_line".to_string(), vec!["Shift+L".to_string()]);
        let map = KeyMap::new(&over);
        assert!(map.matches("extrude", Key::W, NONE));
        assert!(!map.matches("extrude", Key::E, NONE));
        assert_eq!(map.label_of("measure"), "");
        assert!(map.matches("sketch_line", Key::L, Modifiers::SHIFT));
        assert!(
            !map.matches("sketch_line", Key::L, NONE),
            "修飾キーまで同じ時だけ"
        );
        // 同じキーを 2 つの行に付けると、両方の行に重なりとして出る
        over.insert("hole".to_string(), vec!["W".to_string()]);
        let map = KeyMap::new(&over);
        let ex = row_index("extrude").unwrap();
        let hole = row_index("hole").unwrap();
        assert_eq!(map.conflicts(ex), vec![hole]);
        assert_eq!(map.conflicts(hole), vec![ex]);
        // スケッチの中だけの行と 3D のビューだけの行は、同じキーでも重ならない(10-24 u4)
        over.insert("hole".to_string(), vec!["Shift+L".to_string()]);
        let map = KeyMap::new(&over);
        assert!(map.conflicts(hole).is_empty());
        assert!(map.conflicts(row_index("sketch_line").unwrap()).is_empty());
        // スケッチの中でも効く 3D のビューの行(押し出し)とは重なる
        over.insert("extrude".to_string(), vec!["Shift+L".to_string()]);
        let map = KeyMap::new(&over);
        assert_eq!(map.conflicts(row_index("sketch_line").unwrap()), vec![ex]);
        // 付け替えられないキーは飛ばす
        over.insert(
            "hole".to_string(),
            vec!["Escape".to_string(), "Ctrl+C".to_string()],
        );
        assert!(KeyMap::new(&over).chords_of("hole").is_empty());
        assert_eq!(capture(Key::Num5, NONE), Err("値を打つのに使うキー"));
        assert!(capture(Key::Num5, Modifiers::CTRL).is_ok());
        assert_eq!(capture(Key::Z, Modifiers::CTRL), Ok((CTRL, Key::Z)));
    }
}
