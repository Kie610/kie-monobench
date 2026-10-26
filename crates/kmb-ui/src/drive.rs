//! 画面の部品を名前で探し、人と同じ入力(マウスとキー)で操作する(コマンド欄・MCP・バッチの e2e の芯)。
//!
//! egui の差し込み口(`Plugin`)で、毎フレームの AccessKit の木(部品ごとの役割・名前・値・位置)を
//! 受け取り、頼まれた擬似のマウスとキーの出来事を次のフレームの入力の先頭へ入れる。アプリ(eframe)
//! でも試験の Harness でも同じに動く。押す所は部品の中央(egui_kittest の `click` と同じ考え)。
//! 内部の値を直接書き換えないので、欄の単位の補い方や、重なって押せないボタンも人と同じに通る。
//!
//! AccessKit の木は、使い始めた時に作り始める(使わない間は作らない)。木が届くのは次のフレームから。

use egui::accesskit::{Role, Toggled, TreeUpdate};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect};

/// 画面の部品 1 つ(木から写した物。位置は egui の論理の座標)。
#[derive(Debug, Clone, PartialEq)]
pub struct UiNode {
    /// 役割(Button・TextInput・Label・CheckBox・ComboBox など。AccessKit の名前)
    pub role: String,
    /// 名前(ラベルの部品は表示している字)
    pub name: String,
    /// 値(入力欄の中身など)
    pub value: Option<String>,
    pub rect: Rect,
    pub disabled: bool,
    pub focused: bool,
    /// 字を打てる欄か
    pub text_input: bool,
    /// 入切の部品の状態
    pub checked: Option<bool>,
    /// 木の中の番号(見える所まで巻くのに使う)
    pub id: (egui::accesskit::NodeId, egui::accesskit::TreeId),
    /// 一番上の層(窓・浮かせた一覧など)の番号。欄の名前と欄は同じ層で探す(後ろの窓の欄を拾わない)
    pub layer: usize,
}

impl UiNode {
    /// 一覧に出す 1 行(例: `Button "熱" (120,40 32×24)`)。
    pub fn line(&self) -> String {
        let mut s = format!("{} \"{}\"", self.role, self.name);
        if let Some(v) = self.value.as_ref().filter(|v| **v != self.name) {
            s += &format!(" = \"{v}\"");
        }
        if let Some(c) = self.checked {
            s += if c { " [入]" } else { " [切]" };
        }
        if self.disabled {
            s += " (押せない)";
        }
        if self.focused {
            s += " (入力中)";
        }
        s + &format!(
            " @({:.0},{:.0} {:.0}×{:.0})",
            self.rect.min.x,
            self.rect.min.y,
            self.rect.width(),
            self.rect.height()
        )
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "role": self.role,
            "name": self.name,
            "value": self.value,
            "rect": [self.rect.min.x, self.rect.min.y, self.rect.width(), self.rect.height()],
            "disabled": self.disabled,
            "focused": self.focused,
            "text_input": self.text_input,
            "checked": self.checked,
        })
    }
}

/// 差し込み口。`ctx.plugin::<UiDriver>()` で使う。
#[derive(Default)]
pub struct UiDriver {
    tree: Option<accesskit_consumer::Tree>,
    /// 次のフレームの入力に入れる出来事
    queue: Vec<Event>,
    /// 届いた木の数(木が新しくなったかを見る)
    pub frames: u64,
}

struct NoChange;

impl accesskit_consumer::TreeChangeHandler for NoChange {
    fn node_added(&mut self, _: &accesskit_consumer::Node<'_>) {}
    fn node_updated(&mut self, _: &accesskit_consumer::Node<'_>, _: &accesskit_consumer::Node<'_>) {
    }
    fn focus_moved(
        &mut self,
        _: Option<&accesskit_consumer::Node<'_>>,
        _: Option<&accesskit_consumer::Node<'_>>,
    ) {
    }
    fn node_removed(&mut self, _: &accesskit_consumer::Node<'_>) {}
}

impl egui::Plugin for UiDriver {
    fn debug_name(&self) -> &'static str {
        "jc_ui_driver"
    }

    fn input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        if !self.queue.is_empty() {
            // 人の入力より先に入れる(同じフレームに人が触っても、頼んだ順を崩さない)
            let mut ev = std::mem::take(&mut self.queue);
            ev.append(&mut input.events);
            input.events = ev;
        }
    }

    fn output_hook(&mut self, _ctx: &egui::Context, output: &mut egui::FullOutput) {
        if let Some(u) = output.platform_output.accesskit_update.clone() {
            self.take(u);
        }
    }
}

impl UiDriver {
    fn take(&mut self, u: TreeUpdate) {
        match &mut self.tree {
            Some(t) => t.update_and_process_changes(u, &mut NoChange),
            None => self.tree = Some(accesskit_consumer::Tree::new(u, true)),
        }
        self.frames += 1;
    }

    /// 木が届いているか。
    pub fn ready(&self) -> bool {
        self.tree.is_some()
    }

    /// 次のフレームの入力に出来事を足す。
    pub fn push(&mut self, ev: impl IntoIterator<Item = Event>) {
        self.queue.extend(ev);
    }

    /// まだ入力に入れていない出来事があるか。
    pub fn pending(&self) -> bool {
        !self.queue.is_empty()
    }

    /// 今の画面の部品を、木の順(上の帯 → パネル → …)に全部。`ppp` は 1 点の画素数。
    pub fn nodes(&self, ppp: f32) -> Vec<UiNode> {
        let Some(t) = &self.tree else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (i, c) in t.state().root().children().enumerate() {
            walk(c, ppp, i, &mut out);
        }
        out
    }
}

fn walk(n: accesskit_consumer::Node<'_>, ppp: f32, layer: usize, out: &mut Vec<UiNode>) {
    if n.is_hidden() {
        return;
    }
    let role = n.role();
    // ラベルの部品は字を値に持つ(AccessKit の決まり)
    let name = if role == Role::Label {
        n.value()
    } else {
        n.label()
    }
    .unwrap_or_default();
    if let Some(b) = n.bounding_box()
        && role != Role::Window
        && role != Role::GenericContainer
        // ラベルの中の字の並び(ラベルと同じ字)
        && role != Role::TextRun
    {
        let rect = Rect::from_min_max(
            Pos2::new(b.x0 as f32 / ppp, b.y0 as f32 / ppp),
            Pos2::new(b.x1 as f32 / ppp, b.y1 as f32 / ppp),
        );
        out.push(UiNode {
            role: format!("{role:?}"),
            value: if role == Role::Label { None } else { n.value() },
            name,
            rect,
            disabled: n.is_disabled(),
            focused: n.is_focused(),
            text_input: n.is_text_input(),
            checked: n.toggled().map(|t| t == Toggled::True),
            id: n.locate(),
            layer,
        });
    }
    for c in n.children() {
        walk(c, ppp, layer, out);
    }
}

/// 部品の探し方。`"熱"` は名前がちょうど同じ物、`~熱` は名前に含む物。`button:熱` のように役割を
/// 前に付けると絞る(大小は区別しない)。後ろの `#2` は見つかった中の 2 番目。
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub text: String,
    pub contains: bool,
    pub role: Option<String>,
    pub nth: Option<usize>,
}

impl Query {
    pub fn parse(s: &str) -> Query {
        let mut s = s.trim().to_string();
        let mut nth = None;
        if let Some((head, n)) = s.rsplit_once('#')
            && let Ok(k) = n.parse::<usize>()
            && k >= 1
        {
            nth = Some(k);
            s = head.to_string();
        }
        let mut role = None;
        if let Some((r, rest)) = s.split_once(':')
            && !r.is_empty()
            && r.chars().all(|c| c.is_ascii_alphabetic())
        {
            role = Some(r.to_ascii_lowercase());
            s = rest.to_string();
        }
        let contains = s.starts_with('~');
        let text = s.trim_start_matches('~').to_string();
        Query {
            text,
            contains,
            role,
            nth,
        }
    }

    fn role_ok(&self, n: &UiNode) -> bool {
        self.role.as_ref().is_none_or(|r| {
            let have = n.role.to_ascii_lowercase();
            match r.as_str() {
                // 打てる欄の言い方
                "field" | "input" | "textinput" | "text" => n.text_input,
                _ => have == *r,
            }
        })
    }

    /// 合う部品(押せない物も含む)。`~` が無ければ名前がちょうど同じ物だけ(「解析」で
    /// 「断面解析」を押さないように。Playwright の exact と同じ考え)。
    pub fn find<'a>(&self, nodes: &'a [UiNode], screen: Rect) -> Vec<&'a UiNode> {
        let seen = |n: &&UiNode| n.rect.intersects(screen) && n.rect.area() > 0.0;
        nodes
            .iter()
            .filter(seen)
            .filter(|n| {
                self.role_ok(n)
                    && if self.contains {
                        !self.text.is_empty() && n.name.contains(&self.text)
                    } else {
                        n.name == self.text
                    }
            })
            .collect()
    }

    /// 1 つに決める。`#n` が無いのに幾つも合う時は、黙って最初の物を押さず誤りにして候補を挙げる
    /// (違う物を押したまま台本が通らないように)。見つからない時は近い名前を挙げる。
    pub fn one<'a>(&self, nodes: &'a [UiNode], screen: Rect) -> Result<&'a UiNode, String> {
        let found = self.find(nodes, screen);
        // 欄の名前のラベルと、その名前を持つ横の欄(入切・一覧)は 1 つの物として数える
        // (一覧の口の名前は右に、スライダの名前は左に書くので、同じ行ですぐ隣の物もそのラベルの物)
        let next_to = |l: &UiNode, c: &UiNode| {
            (c.rect.center().y - l.rect.center().y).abs() < l.rect.height()
                && ((0.0..=24.0).contains(&(l.rect.left() - c.rect.right()))
                    || (0.0..=24.0).contains(&(c.rect.left() - l.rect.right())))
        };
        let names_its_own = |l: &UiNode| {
            l.role == "Label"
                && ([
                    field_for(l, nodes),
                    check_for(l, nodes),
                    combo_for(l, nodes),
                ]
                .into_iter()
                .flatten()
                .any(|t| t.id != l.id && found.iter().any(|f| f.id == t.id))
                    || found.iter().any(|c| c.role != "Label" && next_to(l, c)))
        };
        let distinct = found.iter().filter(|n| !names_its_own(n)).count();
        if self.nth.is_none() && distinct > 1 {
            let list: Vec<String> = found
                .iter()
                .take(8)
                .enumerate()
                .map(|(i, n)| format!("#{}: {}", i + 1, n.line()))
                .collect();
            return Err(format!(
                "「{}」に合う部品が {} 個あります。役割(button: など)か番号(#2 など)で 1 つに絞ってください\n{}",
                self.text,
                found.len(),
                list.join("\n")
            ));
        }
        let k = self.nth.unwrap_or(1);
        found.get(k - 1).copied().ok_or_else(|| {
            if found.is_empty() {
                // 名前を含む物を先に、次に字を分け合う物を挙げる
                let mut near: Vec<&UiNode> = nodes
                    .iter()
                    .filter(|n| !self.text.is_empty() && n.name.contains(&self.text))
                    .collect();
                near.extend(nodes.iter().filter(|n| {
                    !n.name.is_empty()
                        && !n.name.contains(&self.text)
                        && self
                            .text
                            .chars()
                            .any(|c| !c.is_ascii() && n.name.contains(c))
                }));
                let near: Vec<String> = near
                    .into_iter()
                    .take(8)
                    .map(|n| format!("\"{}\"", n.name))
                    .collect();
                if near.is_empty() {
                    format!("画面に「{}」が見つかりません", self.text)
                } else {
                    format!(
                        "画面に「{}」が見つかりません(近い名前: {})",
                        self.text,
                        near.join("・")
                    )
                }
            } else {
                format!("「{}」は {} 個しかありません", self.text, found.len())
            }
        })
    }
}

/// 名前の部品(欄の名前のラベル)に対応する、打てる欄。部品が打てる欄ならそれ自身。
/// 同じ行の右にある欄のうち最も近い物、無ければすぐ下の欄(表の列の見出しなら、その列の最初の欄。
/// 人が欄の名前から欄を見つけるのと同じ)。
pub fn field_for<'a>(label: &'a UiNode, nodes: &'a [UiNode]) -> Option<&'a UiNode> {
    beside(label, nodes, |n| n.text_input)
}

/// 名前の部品(欄の名前のラベル)に対応する一覧の口。部品が一覧の口ならそれ自身。
pub fn combo_for<'a>(label: &'a UiNode, nodes: &'a [UiNode]) -> Option<&'a UiNode> {
    beside(label, nodes, |n| n.role == "ComboBox")
}

/// 名前の部品(欄の名前のラベル)に対応する入切の部品。部品が入切の部品ならそれ自身。
/// 加工の欄などの入切は名前を横のラベルに書き、入切の部品そのものは名前を持たない。
/// 下の行は見ない(下の行の入切は別の欄)。
pub fn check_for<'a>(label: &'a UiNode, nodes: &'a [UiNode]) -> Option<&'a UiNode> {
    if label.role == "CheckBox" {
        return Some(label);
    }
    let c = label.rect.center();
    nodes
        .iter()
        .filter(|n| n.layer == label.layer && n.role == "CheckBox")
        .filter(|n| n.rect.min.x >= label.rect.max.x - 2.0)
        .filter(|n| {
            (n.rect.center().y - c.y).abs() <= label.rect.height().max(n.rect.height()) * 0.6
        })
        .min_by(|a, b| a.rect.min.x.total_cmp(&b.rect.min.x))
}

/// 名前の部品の横(同じ行の右の最も近い物)か、すぐ下にある、`is` に合う部品。
fn beside<'a>(
    label: &'a UiNode,
    nodes: &'a [UiNode],
    is: impl Fn(&UiNode) -> bool,
) -> Option<&'a UiNode> {
    if is(label) {
        return Some(label);
    }
    let c = label.rect.center();
    let is = |n: &UiNode| n.layer == label.layer && is(n);
    let right = nodes
        .iter()
        .filter(|n| is(n) && n.rect.min.x >= label.rect.max.x - 2.0)
        .filter(|n| {
            (n.rect.center().y - c.y).abs() <= label.rect.height().max(n.rect.height()) * 0.6
        })
        .min_by(|a, b| a.rect.min.x.total_cmp(&b.rect.min.x));
    right.or_else(|| {
        nodes
            .iter()
            .filter(|n| {
                // 表の列の見出しの下の欄も拾う(行の並びの欄)
                is(n)
                    && n.rect.min.y >= label.rect.max.y - 2.0
                    && n.rect.min.y - label.rect.max.y < 80.0
                    && n.rect.max.x > label.rect.min.x
                    && n.rect.min.x < label.rect.max.x + 20.0
            })
            .min_by(|a, b| a.rect.min.y.total_cmp(&b.rect.min.y))
    })
}

/// 点 `at` を押す出来事(動かす → 押す → 離す)。
pub fn click_at(at: Pos2, button: PointerButton, modifiers: Modifiers) -> Vec<Event> {
    vec![
        Event::PointerMoved(at),
        Event::PointerButton {
            pos: at,
            button,
            pressed: true,
            modifiers,
        },
        Event::PointerButton {
            pos: at,
            button,
            pressed: false,
            modifiers,
        },
    ]
}

/// キーを押して離す出来事。
pub fn key_events(key: Key, modifiers: Modifiers) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}

/// `ctrl+z`・`Enter`・`shift+Tab` のような書き方を読む。
pub fn parse_key(s: &str) -> Result<(Key, Modifiers), String> {
    let mut m = Modifiers::NONE;
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let (name, mods) = parts.split_last().ok_or("キーが空です")?;
    for p in mods {
        match p.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "cmd" | "command" => m |= Modifiers::COMMAND,
            "shift" => m |= Modifiers::SHIFT,
            "alt" => m |= Modifiers::ALT,
            other => return Err(format!("{other} は知らない修飾キーです(ctrl・shift・alt)")),
        }
    }
    let key = Key::from_name(name)
        .or_else(|| {
            let mut c = name.chars();
            let first = c.next()?.to_ascii_uppercase();
            Key::from_name(&format!("{first}{}", c.as_str()))
        })
        .ok_or_else(|| format!("{name} は知らないキーです(Enter・Escape・Tab・A・F2 など)"))?;
    Ok((key, m))
}

/// 部品が見える所まで巻く出来事(パネルの下の方の欄など。人が巻いてから押すのと同じ)。
pub fn scroll_into_view(n: &UiNode) -> Event {
    Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::ScrollIntoView,
        target_tree: n.id.1,
        target_node: n.id.0,
        data: None,
    })
}
