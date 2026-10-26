//! 画面の部品を自由に並べる(Unity 式のドッキング。まとめてレビュー N4)。
//!
//! 3D のビュー・ブラウザ・編集パネル・タイムラインを、タブを引いて上下左右へ付け替えたり、
//! 同じ所にタブで重ねたり、窓の中に浮かせたりできる。上の帯・ツールバー・下の状態の欄は動かさない
//! (Unity のツールバーと同じ)。部品の並びは `egui_dock`(MIT)が持ち、`layout.json` に書く。
//!
//! 部品を出すか隠すかは、`Layout::browser_open` などの印で決める。隠すと並びから外し、
//! 外す前の場所(隣の部品と分け方)を覚えて、また出す時にそこへ戻す。

use crate::layout::Space;
use crate::theme::RightMenu as _;
use crate::{Workspace, theme};
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, NodeIndex, Split, SurfaceIndex, TabViewer};
use serde::{Deserialize, Serialize};

/// 並べられる部品。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pane {
    /// 3D のビュー(閉じられない。必ず並びにある)
    View,
    Browser,
    Panel,
    /// 設計はタイムライン、加工は段の帯、アニメーションはストーリーボード。
    /// レンダリングと解析には出さない([`Pane::in_space`])
    Timeline,
    /// コマンド欄(初めは隠す)
    Commands,
    /// データパネル(初めは隠す。左いっぱいに置く)。
    /// 中身はホームと同じ拡張([`crate::HomePage::panel`])
    Data,
}

impl Pane {
    /// 隠せる部品(3D のビューのほか)。
    pub const HIDABLE: [Pane; 5] = [
        Pane::Browser,
        Pane::Panel,
        Pane::Timeline,
        Pane::Commands,
        Pane::Data,
    ];

    /// 既定の並びで出している部品か(コマンド欄とデータパネルは初めは隠す)。
    pub fn open_by_default(self) -> bool {
        !matches!(self, Pane::Commands | Pane::Data)
    }

    /// その作業空間で並べる部品か。レンダリングと解析では下の帯を出さない(レンダリングの
    /// 下はギャラリー、解析の研究の木はブラウザにあり、設計のタイムラインは用が無い)。
    /// 隠す印(`timeline_open`)は変えないので、設計へ戻すとまた出る。
    pub fn in_space(self, space: Space) -> bool {
        !matches!((self, space), (Pane::Timeline, Space::Simulation))
    }

    /// タブの名前。
    pub fn label(self, space: Space) -> &'static str {
        match (self, space) {
            (Pane::View, _) => "3D ビュー",
            (Pane::Browser, _) => "ブラウザ",
            (Pane::Panel, _) => "編集パネル",
            (Pane::Timeline, Space::Design) => "タイムライン",
            (Pane::Timeline, Space::Render) => "レンダリング ギャラリー",
            (Pane::Timeline, Space::Machining) => "段の帯",
            (Pane::Timeline, Space::Animation) => "ストーリーボード",
            (Pane::Timeline, Space::Simulation) => "タイムライン",
            (Pane::Commands, _) => "コマンド",
            (Pane::Data, _) => "データパネル",
        }
    }
}

/// 隠した部品を、また出す時に戻す場所。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Spot {
    /// この部品と同じ所にタブで重ねる
    Tab(Pane),
    /// この部品たち(隣の枝にあった部品)の隣に分けて置く(`fraction` は分けた所の左・上の側の割合)
    Beside {
        anchor: [Option<Pane>; 4],
        split: Split,
        fraction: f32,
    },
    /// 窓に浮かせる
    Window,
}

/// タブの帯の高さ(字の大きさに合わせる)。
fn tab_bar_height() -> f32 {
    theme::fs(24.0).max(18.0)
}

/// 下の帯(タイムラインの部品)の既定の高さ。設計のタイムラインはアイコンの札が入る最小
/// (札の上下に空きを残さない。Kie 2026-10-21)、加工の段の帯とストーリーボードは 72
/// (字の倍率を掛ける。段の帯は名前と工具の行が要る)。
pub(crate) fn band_default_height(space: Space) -> f32 {
    match space {
        Space::Machining | Space::Animation => 72.0 * theme::band(),
        // 縮小図と版の行が入る高さ
        Space::Render => 150.0 * theme::band(),
        _ => crate::timeline_ui::band_min_height(),
    }
}

/// 下の帯の部品の今の高さ(上下に分けた所にある時だけ)。
fn band_height_of(d: &DockState<Pane>) -> Option<f32> {
    let path = d.find_tab(&Pane::Timeline)?;
    if !path.surface.is_main() {
        return None;
    }
    let parent = path.node.parent()?;
    match &d[path.surface][parent] {
        egui_dock::Node::Vertical(_) => {}
        _ => return None,
    }
    let h = d[path.surface][path.node].rect()?.height();
    (h.is_finite() && h > 0.0).then_some(h)
}

/// 既定の並び(前の版の固定の配置と同じ: 下いっぱいにタイムライン、その上の左にブラウザ、右に
/// 編集パネル、真ん中に 3D のビュー)。幅は覚えている幅(1 倍の時の幅に字の大きさを掛ける)、
/// 高さは下の帯の高さ `band_h` から、`area`(並べる所の大きさ)に対する割合にする。
pub(crate) fn default_dock(
    area: egui::Vec2,
    browser_w: f32,
    panel_w: f32,
    band_h: f32,
) -> DockState<Pane> {
    let mut d = DockState::new(vec![Pane::View]);
    let s = d.main_surface_mut();
    let (w, h) = (area.x.max(1.0), area.y.max(1.0));
    let k = theme::band().clamp(1.0, 1.6);
    let timeline_h = band_h;
    let [top, _] = s.split_below(
        NodeIndex::root(),
        (1.0 - timeline_h / h).clamp(0.5, 0.95),
        vec![Pane::Timeline],
    );
    // 分けた所の `fraction` は、左(上)の子の割合(`split_left` では新しい方)
    let bw = browser_w * k;
    let [rest, _] = s.split_left(top, (bw / w).clamp(0.1, 0.5), vec![Pane::Browser]);
    let pw = panel_w * k;
    s.split_right(
        rest,
        (1.0 - pw / (w - bw).max(1.0)).clamp(0.4, 0.9),
        vec![Pane::Panel],
    );
    d.translations = translations();
    d
}

/// 並びの右クリックのメニューと、ボタンの説明の日本語。
fn translations() -> egui_dock::Translations {
    let mut t = egui_dock::Translations::english();
    t.tab_context_menu.close_button = "隠す".into();
    t.tab_context_menu.eject_button = "窓に浮かせる".into();
    t.tab_context_menu.hide_tab_bar_button = "タブの帯を隠す".into();
    t.tab_context_menu.show_tab_bar_button = "タブの帯を出す".into();
    t.leaf.close_button_disabled_tooltip = "3D ビューは隠せません".into();
    t.leaf.close_all_button = "窓を閉じる".into();
    t.leaf.close_all_button_menu_hint = "右クリックで、閉じるタブを選ぶ".into();
    t.leaf.close_all_button_modifier_hint = "Shift を押しながらで、閉じるタブを選ぶ".into();
    t.leaf.close_all_button_modifier_menu_hint = "右クリックで、閉じるタブを選ぶ".into();
    t.leaf.close_all_button_disabled_tooltip = "3D ビューのある窓は閉じられません".into();
    t.leaf.minimize_button = "たたむ".into();
    t.leaf.minimize_button_menu_hint = "右クリックで、たたむタブを選ぶ".into();
    t.leaf.minimize_button_modifier_hint = "Shift を押しながらで、たたむタブを選ぶ".into();
    t.leaf.minimize_button_modifier_menu_hint = "右クリックで、たたむタブを選ぶ".into();
    t
}

/// 書いてあった並びを読む(読めない・3D のビューが無い並びは使わない)。
pub(crate) fn dock_from_json(v: &serde_json::Value) -> Option<DockState<Pane>> {
    let mut d: DockState<Pane> = serde_json::from_value(v.clone()).ok()?;
    d.find_tab(&Pane::View)?;
    // 同じ部品が 2 つあれば、後の方を外す(手で書き換えた layout.json など)
    let mut seen = Vec::new();
    d.retain_tabs(|p| {
        let first = !seen.contains(p);
        seen.push(*p);
        first
    });
    d.translations = translations();
    Some(d)
}

/// 覚える並びの形(JSON)。描くたびに変わる値(部品の場所の四角・タブの帯の巻き取り・どこを
/// 押したか)は外す。読む時は既定の値になり、次に描く時に決まる。
pub(crate) fn dock_shape(d: &DockState<Pane>) -> Option<serde_json::Value> {
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                for k in [
                    "rect",
                    "viewport",
                    "scroll",
                    "focused_node",
                    "focused_surface",
                ] {
                    m.remove(k);
                }
                m.values_mut().for_each(strip);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut v = serde_json::to_value(d).ok()?;
    strip(&mut v);
    Some(v)
}

/// 部品を外す前に、戻す場所を調べる。
pub(crate) fn spot_of(d: &DockState<Pane>, pane: Pane) -> Option<Spot> {
    let path = d.find_tab(&pane)?;
    if path.surface != SurfaceIndex::main() {
        return Some(Spot::Window);
    }
    let tree = &d[SurfaceIndex::main()];
    let leaf = tree[path.node].get_leaf()?;
    if let Some(other) = leaf.tabs.iter().find(|p| **p != pane) {
        return Some(Spot::Tab(*other));
    }
    let node = path.node;
    let parent = node.parent()?;
    let is_left = node.is_left();
    let sibling = if is_left {
        parent.right()
    } else {
        parent.left()
    };
    // 隣(兄弟の枝)にある部品の全部。戻す時は、それらを含む一番小さい枝の隣に置く
    let mut anchor = [None; 4];
    let found = d
        .iter_all_tabs()
        .filter(|(p, _)| p.surface == SurfaceIndex::main() && under(p.node, sibling))
        .map(|(_, t)| *t);
    for (slot, t) in anchor.iter_mut().zip(found) {
        *slot = Some(t);
    }
    anchor[0]?;
    let (horizontal, fraction) = match &tree[parent] {
        egui_dock::Node::Horizontal(s) => (true, s.fraction),
        egui_dock::Node::Vertical(s) => (false, s.fraction),
        _ => return None,
    };
    // 親の `fraction` は左(上)の子の割合。戻す時も同じ割合で分ける
    let split = match (horizontal, is_left) {
        (true, true) => Split::Left,
        (true, false) => Split::Right,
        (false, true) => Split::Above,
        (false, false) => Split::Below,
    };
    Some(Spot::Beside {
        anchor,
        split,
        fraction: fraction.clamp(0.05, 0.95),
    })
}

/// 2 つの場所を含む一番小さい枝。
fn common_parent(a: NodeIndex, b: NodeIndex) -> NodeIndex {
    let mut a = a;
    while !under(b, a) {
        match a.parent() {
            Some(p) => a = p,
            None => return NodeIndex::root(),
        }
    }
    a
}

/// `node` が `root` の枝の中にあるか。
fn under(mut node: NodeIndex, root: NodeIndex) -> bool {
    loop {
        if node == root {
            return true;
        }
        match node.parent() {
            Some(p) => node = p,
            None => return false,
        }
    }
}

/// 部品を戻す。覚えた場所の隣の部品が無ければ、既定の場所(既定の並びと同じ辺)へ置く。
pub(crate) fn put_back(
    d: &mut DockState<Pane>,
    pane: Pane,
    spot: Option<Spot>,
    area: egui::Vec2,
    browser_w: f32,
    panel_w: f32,
    band_h: f32,
) {
    let main = SurfaceIndex::main();
    let leaf_of = |d: &DockState<Pane>, p: Pane| {
        d.find_tab(&p)
            .filter(|path| path.surface == main)
            .map(|path| path.node)
    };
    match spot {
        Some(Spot::Tab(other)) if let Some(node) = leaf_of(d, other) => {
            if let Ok(leaf) = d[main].leaf_mut(node) {
                leaf.append_tab(pane);
            }
            return;
        }
        Some(Spot::Beside {
            anchor,
            split,
            fraction,
        }) if let Some(node) = anchor
            .iter()
            .flatten()
            .filter_map(|p| leaf_of(d, *p))
            .reduce(common_parent) =>
        {
            d[main].split(node, split, fraction, egui_dock::Node::leaf(pane));
            return;
        }
        Some(Spot::Window) => {
            d.add_window(vec![pane]);
            return;
        }
        _ => {}
    }
    let (w, h) = (area.x.max(1.0), area.y.max(1.0));
    let k = theme::band().clamp(1.0, 1.6);
    let view = leaf_of(d, Pane::View).unwrap_or(NodeIndex::root());
    match pane {
        Pane::Browser => {
            let f = (browser_w * k / w).clamp(0.1, 0.5);
            d[main].split_left(view, f, vec![pane]);
        }
        Pane::Panel => {
            let f = (1.0 - panel_w * k / w).clamp(0.4, 0.9);
            d[main].split_right(view, f, vec![pane]);
        }
        Pane::Timeline | Pane::View => {
            let f = (1.0 - band_h / h).clamp(0.5, 0.95);
            d[main].split_below(NodeIndex::root(), f, vec![pane]);
        }
        Pane::Commands => {
            let f = (1.0 - COMMANDS_H * theme::band() / h).clamp(0.4, 0.95);
            d[main].split_below(NodeIndex::root(), f, vec![pane]);
        }
        Pane::Data => {
            // 左いっぱい(下の帯の横まで)
            let f = (DATA_W * k / w).clamp(0.1, 0.5);
            d[main].split_left(NodeIndex::root(), f, vec![pane]);
        }
    }
}

/// データパネルの幅(1 倍の時)。
const DATA_W: f32 = 264.0;

/// コマンド欄の高さ(1 倍の時。記録の数行と入力の 1 行)。
const COMMANDS_H: f32 = 150.0;

/// 並びの見た目(画面のテーマの色に合わせる)。
/// タブの帯は、部品を重ねた所と浮かせた窓にだけ出す。部品が 1 つだけの所は前の版と同じく帯を
/// 出さず(見出しは部品のもの)、その見出しを引いて動かす。
fn fit_tab_bars(d: &mut DockState<Pane>) {
    for (_, leaf) in d.iter_leaves_mut() {
        leaf.tab_bar_hidden = leaf.tabs.len() == 1;
    }
}

/// 部品 1 つだけを浮かせた窓か(その時は、外の窓と同じ帯を部品の上に描く)。
fn lone_float(d: &DockState<Pane>, pane: Pane) -> Option<SurfaceIndex> {
    let path = d.find_tab(&pane)?;
    (!path.surface.is_main() && d[path.surface].num_tabs() == 1).then_some(path.surface)
}

/// 部品の幅(`horizontal`)か高さを `size` にする(左右・上下に分けた所の割合を変える)。分け方が
/// 違う(幅を合わせたいのに上下に分けた)時や、まだ大きさが決まっていない時は何もしない。
fn fit_size(d: &mut DockState<Pane>, pane: Pane, horizontal: bool, size: f32) -> bool {
    let Some(path) = d.find_tab(&pane) else {
        return false;
    };
    let tree = &mut d[path.surface];
    let Some(parent) = path.node.parent() else {
        return false;
    };
    let is_left = path.node.is_left();
    let split = match (&mut tree[parent], horizontal) {
        (egui_dock::Node::Horizontal(s), true) | (egui_dock::Node::Vertical(s), false) => s,
        _ => return false,
    };
    let total = if horizontal {
        split.rect.width()
    } else {
        split.rect.height()
    };
    if !total.is_finite() || total <= size {
        return false;
    }
    let share = size / total;
    // 既定の並びと同じく 5 % まで寄せる(10 % では、低いタイムラインが窓の高さの 1 割より低くなれない)
    split.fraction = if is_left { share } else { 1.0 - share }.clamp(0.05, 0.95);
    true
}

/// 部品の右クリックのメニュー(タブ、またはタブの帯を出さない所の見出し)。
pub(crate) fn pane_menu(ui: &mut egui::Ui, pane: Pane, act: &mut Option<DockAction>) {
    if ui.button("並びを既定に戻す").clicked() {
        *act = Some(DockAction::Reset);
        ui.close();
    }
    if pane == Pane::View {
        return;
    }
    ui.separator();
    if ui.button("窓に浮かせる").clicked() {
        *act = Some(DockAction::Float(pane));
        ui.close();
    }
    if ui
        .button("アプリの外の窓に出す")
        .on_hover_text("別の窓にして、別のモニターへも動かせる。その窓を閉じると元の場所に戻る")
        .clicked()
    {
        *act = Some(DockAction::Outer(pane));
        ui.close();
    }
    if ui.button("隠す").clicked() {
        *act = Some(DockAction::Hide(pane));
        ui.close();
    }
}

/// 部品のタブ・見出しを引いている途中。本体の窓の外で離したら、その部品を外の窓に出し、
/// 閉じた時は引く前の場所へ戻す。
pub(crate) struct DockDrag {
    pane: Pane,
    /// 引く前の場所
    spot: Option<Spot>,
    /// 引く前の大きさ
    size: egui::Vec2,
    /// 最後に分かったカーソルの位置(本体の窓の中の座標。窓の外では外の値になる)
    last: Option<egui::Pos2>,
}

/// egui_dock がタブ・タブの帯を出さない所の見出しを引く時の id(egui_dock 0.21 の作りと同じ)と、その部品。
fn drag_ids(d: &DockState<Pane>, dock_id: egui::Id) -> Vec<(egui::Id, Pane)> {
    let mut v = Vec::new();
    for (path, leaf) in d.iter_leaves() {
        let base = dock_id
            .with((path.surface, "surface"))
            .with((path.node, "node"));
        if let Some(active) = leaf.tabs.get(leaf.active.0) {
            v.push((base.with("hidden_tab_bar_drag"), *active));
        }
        for (i, t) in leaf.tabs.iter().enumerate() {
            v.push((base.with((i, "tab")), *t));
        }
    }
    v
}

/// 部品の右クリックのメニューで選んだこと(並びを描いた後に行う)。
#[derive(Clone, Copy)]
pub(crate) enum DockAction {
    Reset,
    Float(Pane),
    Outer(Pane),
    Hide(Pane),
    /// 浮かせた窓を閉じて、浮かせる前の場所へ戻す
    Return(Pane),
}

fn dock_style(ui: &egui::Ui, l: &theme::Look) -> egui_dock::Style {
    let mut s = egui_dock::Style::from_egui(ui.style());
    s.dock_area_padding = None;
    s.main_surface_border_stroke = egui::Stroke::NONE;
    s.main_surface_border_rounding = egui::CornerRadius::ZERO;
    s.separator.width = 1.0;
    s.separator.extra_interact_width = 4.0;
    // 境を引いて部品を狭められる下限(タブの帯と少し。既定の 175 px ではタイムラインが広がってしまう)
    s.separator.extra = tab_bar_height() + 20.0;
    s.separator.color_idle = l.pal.edge;
    s.separator.color_hovered = l.ws();
    s.separator.color_dragged = l.ws();
    s.tab_bar.bg_fill = l.pal.chrome2;
    s.tab_bar.height = tab_bar_height();
    s.tab_bar.hline_color = l.pal.edge;
    s.tab_bar.corner_radius = egui::CornerRadius::ZERO;
    let r = theme::corner(4.0).round().clamp(0.0, 255.0) as u8;
    let tab_corner = egui::CornerRadius {
        nw: r,
        ne: r,
        sw: 0,
        se: 0,
    };
    for (t, fill, ink) in [
        (&mut s.tab.active, l.pal.panel, l.pal.ink),
        (&mut s.tab.focused, l.pal.panel, l.pal.ink),
        (&mut s.tab.active_with_kb_focus, l.pal.panel, l.pal.ink),
        (&mut s.tab.focused_with_kb_focus, l.pal.panel, l.pal.ink),
        (&mut s.tab.inactive, l.pal.chrome2, l.pal.ink2),
        (&mut s.tab.inactive_with_kb_focus, l.pal.chrome2, l.pal.ink2),
        (&mut s.tab.hovered, l.pal.chrome, l.pal.ink),
    ] {
        t.bg_fill = fill;
        t.text_color = ink;
        t.outline_color = l.pal.edge;
        t.corner_radius = tab_corner;
    }
    s.tab.tab_body.inner_margin = egui::Margin::ZERO;
    // タブの帯を出さない所は、部品の上の端(見出し)を引いて動かす。帯を出す三角の印は出さない
    s.tab.tab_body.hidden_tab_bar_drag_height = Some(tab_bar_height());
    s.buttons.show_tab_bar_size = 0.0;
    s.tab.tab_body.stroke = egui::Stroke::NONE;
    s.tab.tab_body.bg_fill = l.pal.panel;
    s.tab.tab_body.corner_radius = egui::CornerRadius::ZERO;
    s.tab.hline_below_active_tab_name = false;
    s.buttons.close_tab_color = l.pal.ink3;
    s.buttons.close_tab_active_color = l.pal.ink;
    s.buttons.close_tab_bg_fill = l.pal.chrome;
    // 落とす所は、引いている所の部品の中の位置で決める(端なら分けて隣へ、真ん中ならタブで重ねる。
    // Unity と同じ)。落とした後の場所を色の付いた四角で見せる
    s.overlay.overlay_type = egui_dock::OverlayType::HighlightedAreas;
    s.overlay.selection_color = l.ws().gamma_multiply(0.35);
    s.overlay.button_color = l.ws();
    s.overlay.button_border_stroke = egui::Stroke::new(1.0, l.pal.edge);
    s
}

/// 並びの中に部品を描く役。
struct Viewer<'a> {
    ws: &'a mut Workspace,
    /// 右クリックのメニューで選んだこと
    act: Option<DockAction>,
    /// この描画で見えている部品と、中身を描く所(層・四角・切り取り)。中身は並びを描いた後に描く
    bodies: Vec<(Pane, egui::LayerId, egui::Rect, egui::Rect)>,
}

/// 部品の中身を描く順(前の版の固定の配置と同じ: タイムライン・ブラウザ・編集パネル・3D ビュー)。
/// 機能の部品は、パネルで変えたことが同じ描画のビューに効く順を前提にしているので、並びが
/// どう変わってもこの順で描く(例: ビューで数字を打ち始めるとパネルの値の欄へ移す)。
fn draw_order(p: Pane) -> u8 {
    match p {
        Pane::Timeline => 0,
        Pane::Browser => 1,
        Pane::Panel => 2,
        Pane::View => 3,
        // データパネルは 3D のビューより前(引いて来たデザインをビューが同じ描画で受ける)
        Pane::Data => 1,
        // コマンド欄は最後(同じ描画で変わった部品の一覧を見ないよう、ほかを描いた後)
        Pane::Commands => 4,
    }
}

impl TabViewer for Viewer<'_> {
    type Tab = Pane;

    fn id(&mut self, tab: &mut Pane) -> egui::Id {
        egui::Id::new(("dock", *tab))
    }

    fn title(&mut self, tab: &mut Pane) -> egui::WidgetText {
        egui::RichText::new(tab.label(self.ws.win.layout.space))
            .size(theme::fs(11.5))
            .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Pane) {
        // egui_dock は中身を ScrollArea で包み、その中の大きさは限りが無い。部品は前の版と同じく、
        // 見えている所(タブの中の四角)いっぱいに描く(スケッチの格子などが無限に広がらないように)
        let body = ui.clip_rect().intersect(ui.max_rect());
        self.bodies
            .push((*tab, ui.layer_id(), body, ui.clip_rect()));
        ui.advance_cursor_after_rect(body);
    }

    fn on_tab_button(&mut self, tab: &mut Pane, response: &egui::Response) {
        // egui_dock のメニューは使わない(「タブの帯を隠す」が出てしまうため)。項目は見出しと同じ
        response.right_menu(|ui| pane_menu(ui, *tab, &mut self.act));
    }

    fn is_closeable(&self, tab: &Pane) -> bool {
        *tab != Pane::View
    }

    fn on_close(&mut self, tab: &mut Pane) -> OnCloseResponse {
        // 並びから外すのは次の描画の前(戻す場所を覚えてから外す)
        self.ws.set_pane_open(*tab, false);
        OnCloseResponse::Ignore
    }

    fn allowed_in_windows(&self, tab: &mut Pane) -> bool {
        // 3D のビューは窓に浮かせない(真ん中に必ず残す)
        *tab != Pane::View
    }

    fn clear_background(&self, _tab: &Pane) -> bool {
        false
    }

    fn scroll_bars(&self, _tab: &Pane) -> [bool; 2] {
        // 部品はそれぞれ自分で巻き取る
        [false, false]
    }
}

/// 画面の部品の並び(ドック)と、外の窓・全画面の状態(窓に 1 つ)。
pub(crate) struct Docking {
    /// 画面の部品の並び(N4。初めて描く時に `layout.dock` か既定から作る)
    pub(crate) tree: Option<egui_dock::DockState<Pane>>,
    /// 隠した部品を、また出す時に戻す場所
    pub(crate) spots: std::collections::BTreeMap<Pane, Spot>,
    /// 最後に覚えた並びの形(変わったら `layout.dock` に書く)
    pub(crate) shape: Option<serde_json::Value>,
    /// 並びを既定から動かしたか
    pub(crate) custom: bool,
    /// 前に並べた所の大きさ
    pub(crate) area: egui::Vec2,
    /// 下の帯の高さを合わせた作業空間(替わったら、その空間の高さにする)
    pub(crate) band_space: Option<Space>,
    /// 作業空間ごとの下の帯の高さ(替える前に覚えた高さ)
    pub(crate) band_heights: std::collections::HashMap<Space, f32>,
    /// 押した所にあった部品と、その時の場所(引いて落とした部品を見分ける)
    pub(crate) press: Vec<(Pane, egui_dock::NodePath)>,
    /// 引いて落とした部品と、データパネルを出し入れした時の左右の部品(次の描画で、前の版の幅・高さに
    /// 合わせる)
    pub(crate) fit: Vec<Pane>,
    /// 部品のタブ・見出しを引いている途中(本体の窓の外で離すと外の窓に出す)
    pub(crate) drag: Option<DockDrag>,
    /// アプリの外の窓を、作った時の場所と大きさ(作った後は同じ値を渡し続ける)
    pub(crate) outer_start: std::collections::BTreeMap<Pane, [f32; 4]>,
    /// 全画面にする前の並び
    pub(crate) focus_tree: Option<egui_dock::DockState<Pane>>,
}

impl Workspace {
    /// 今の作業空間の下の帯の高さ(覚えた高さ、無ければ既定)。
    pub(crate) fn bottom_band_height(&self) -> f32 {
        let space = self.win.layout.space;
        self.docking
            .band_heights
            .get(&space)
            .copied()
            .unwrap_or_else(|| band_default_height(space))
    }

    /// 部品を出しているか。
    pub(crate) fn pane_open(&self, pane: Pane) -> bool {
        match pane {
            Pane::View => true,
            Pane::Browser => self.win.layout.browser_open,
            Pane::Panel => self.win.layout.panel_open,
            Pane::Timeline => self.win.layout.timeline_open,
            Pane::Commands => self.win.layout.commands_open,
            Pane::Data => self.win.layout.data_open,
        }
    }

    /// 部品を並びに置くか(出していて、今の作業空間で使い、アプリの外の窓に出していない)。
    pub(crate) fn pane_docked(&self, pane: Pane) -> bool {
        self.pane_open(pane)
            && pane.in_space(self.win.layout.space)
            && !self.win.layout.outer.contains_key(&pane)
    }

    pub(crate) fn set_pane_open(&mut self, pane: Pane, open: bool) {
        match pane {
            Pane::View => {}
            Pane::Browser => self.win.layout.browser_open = open,
            Pane::Panel => self.win.layout.panel_open = open,
            Pane::Timeline => self.win.layout.timeline_open = open,
            Pane::Commands => self.win.layout.commands_open = open,
            Pane::Data => self.win.layout.data_open = open,
        }
    }

    /// 並びを既定に戻す(部品をすべて出し、前の版の固定の配置と同じにする)。
    pub(crate) fn reset_dock(&mut self) {
        for p in Pane::HIDABLE {
            self.set_pane_open(p, p.open_by_default());
        }
        self.win.layout.dock = None;
        self.win.layout.outer.clear();
        self.docking.tree = None;
        self.docking.spots.clear();
        self.docking.shape = None;
        self.docking.custom = false;
        self.docking.focus_tree = None;
        self.win.focus_mode = None;
    }

    /// 並びが既定のままか(既定に戻すボタンを押せるか)。
    pub(crate) fn dock_is_default(&self) -> bool {
        !self.docking.custom
            && self.win.layout.outer.is_empty()
            && Pane::HIDABLE
                .iter()
                .all(|p| self.pane_open(*p) == p.open_by_default())
    }

    /// 全画面(部品を隠す)に入る前の並びを覚える・出る時に戻す。
    pub(crate) fn remember_dock_for_focus(&mut self, entering: bool) {
        if entering {
            self.docking.focus_tree = self.docking.tree.clone();
        } else if let Some(d) = self.docking.focus_tree.take() {
            self.docking.tree = Some(d);
        }
    }

    /// 出す・隠す印に合わせて並びを直す。
    fn sync_dock(&mut self, area: egui::Vec2) {
        let (bw, pw) = (self.win.layout.browser_width, self.win.layout.panel_width);
        // 下の帯の高さは作業空間ごとに覚える(設計のタイムラインはアイコンだけの低い帯、加工の
        // 段の帯は名前と工具の行が入る高さ)。作業空間が替わったら、前の空間の高さを覚えて替える
        let space = self.win.layout.space;
        let mut refit = false;
        if self.docking.band_space != Some(space) {
            if let (Some(old), Some(d)) = (self.docking.band_space, self.docking.tree.as_ref())
                && let Some(h) = band_height_of(d)
            {
                self.docking.band_heights.insert(old, h);
            }
            refit = self.docking.band_space.is_some();
            self.docking.band_space = Some(space);
        }
        let band_h = self.bottom_band_height();
        // 既定のまま(まだ動かしていない)なら、並べる所の大きさが変わるたびに既定から作り直す。
        // 前の版と同じく、ブラウザ・編集パネルの幅とタイムラインの高さを px で保つ(割合で伸びない)
        let rebuild = !self.docking.custom
            && self.docking.area != area
            && self.docking.tree.is_some()
            && !self.win.layout.dock.is_some();
        self.docking.area = area;
        if rebuild {
            self.docking.tree = None;
        }
        let created = self.docking.tree.is_none();
        let mut d = match self.docking.tree.take() {
            Some(d) => d,
            None => {
                // 書いてある並び(無ければ既定)。隠している部品は下で外す
                match self.win.layout.dock.as_ref().and_then(dock_from_json) {
                    Some(d) => {
                        self.docking.custom = true;
                        d
                    }
                    None => {
                        self.docking.custom = false;
                        default_dock(area, bw, pw, band_h)
                    }
                }
            }
        };
        let mut changed = false;
        for p in Pane::HIDABLE {
            let has = d.find_tab(&p).is_some();
            changed |= self.pane_docked(p) != has;
            if p == Pane::Data && self.pane_docked(p) != has {
                // 左いっぱいに足す・外すと、ほかの部品は割合で縮む・伸びるので、前の幅に戻す
                self.docking.fit = vec![Pane::Data, Pane::Browser, Pane::Panel];
            }
            match (self.pane_docked(p), has) {
                (true, false) => {
                    let spot = self.docking.spots.remove(&p);
                    put_back(&mut d, p, spot, area, bw, pw, band_h);
                }
                (false, true) => {
                    if let Some(s) = spot_of(&d, p) {
                        self.docking.spots.insert(p, s);
                    }
                    if let Some(path) = d.find_tab(&p) {
                        d.remove_tab(path);
                    }
                }
                _ => {}
            }
        }
        if d.find_tab(&Pane::View).is_none() {
            d = default_dock(area, bw, pw, band_h);
            self.docking.custom = false;
        }
        fit_tab_bars(&mut d);
        if refit && !created && fit_size(&mut d, Pane::Timeline, false, band_h) {
            // 作業空間に合わせた高さは、利用者が動かしたことにしない(動かした並びなら、その高さで覚える)
            if self.docking.custom {
                self.win.layout.dock = dock_shape(&d);
            }
            self.docking.shape = dock_shape(&d);
        }
        // 出す・隠すは並びを動かしたことにしない(印の方で覚えている)
        if created || (changed && !self.docking.custom) {
            self.docking.shape = dock_shape(&d);
        }
        self.docking.tree = Some(d);
    }

    /// 部品の並び(上の帯・ツールバー・下の状態の欄の内側)を描く。
    /// 部品 1 つの中身(タブの中の四角いっぱい)。
    pub(crate) fn pane_ui(&mut self, ui: &mut egui::Ui, pane: Pane) {
        let l = theme::look(ui.ctx());
        let (fill, margin) = match pane {
            Pane::View => (l.pal.canvas, 0),
            Pane::Browser | Pane::Panel | Pane::Data => (l.pal.panel, 0),
            Pane::Timeline => (l.pal.chrome2, 12),
            Pane::Commands => (l.pal.chrome2, 8),
        };
        // 文書の部品は、ui から作る記憶の鍵(開いた枝・巻き取りの位置・欄の字)をタブごとにする
        // (タブを替えた同じコマでも今のタブの鍵で描く)。コマンド欄とデータパネルは窓全体の物
        crate::set_doc_scope(ui.ctx(), self.tab_uid());
        let tab = (!matches!(pane, Pane::Commands | Pane::Data)).then(|| self.tab_uid());
        egui::Frame::new()
            .fill(fill)
            .inner_margin(egui::Margin::symmetric(margin, 0))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.push_id(("jc_doc", tab), |ui| match (pane, self.win.layout.space) {
                    (Pane::View, _) => self.view_ui(ui),
                    (Pane::Browser, _) => self.browser_ui(ui),
                    (Pane::Panel, _) => self.panel_frame(ui),
                    (Pane::Timeline, Space::Design | Space::Simulation) => {
                        // 解析では並べない(in_space)が、外の窓に出した時は設計のまま
                        self.timeline_ui(ui)
                    }
                    // レンダリングでは描いた絵の帯
                    (Pane::Timeline, Space::Render) => self.gallery_ui(ui),
                    (Pane::Timeline, Space::Machining) => self.steps_ui(ui),
                    (Pane::Timeline, Space::Animation) => self.storyboard_ui(ui),
                    (Pane::Commands, _) => self.cmdline_ui(ui),
                    (Pane::Data, _) => self.data_panel_ui(ui),
                });
            });
    }

    pub(crate) fn dock_ui(&mut self, ui: &mut egui::Ui) {
        let area = ui.available_size();
        self.sync_dock(area);
        let Some(mut dock) = self.docking.tree.take() else {
            return;
        };
        fit_tab_bars(&mut dock);
        // 押した所の部品を覚える(タブ、または見出しを引くと、離した時に場所が変わる)
        let (pressed, released) = ui.input(|i| {
            (
                i.pointer
                    .primary_pressed()
                    .then(|| i.pointer.interact_pos())
                    .flatten(),
                i.pointer.primary_released(),
            )
        });
        if let Some(at) = pressed {
            self.docking.press = dock
                .iter_leaves()
                .filter(|(_, l)| l.rect.contains(at))
                .flat_map(|(path, l)| l.tabs.iter().map(move |t| (*t, path)))
                .collect();
        }
        let style = dock_style(ui, &self.win.look);
        let dock_id = egui::Id::new("jc_dock");
        let ids = drag_ids(&dock, dock_id);
        // 引き始めた部品の、引く前の場所と大きさを覚える(egui_dock は離した時に並びを変える)
        if let Some(dragged) = ui.ctx().dragged_id()
            && ui.input(|i| i.pointer.is_decidedly_dragging())
            && let Some((_, pane)) = ids.iter().find(|(id, _)| *id == dragged)
            && self.docking.drag.as_ref().is_none_or(|g| g.pane != *pane)
        {
            let size = dock
                .find_tab(pane)
                .and_then(|p| dock[p.surface][p.node].rect())
                .map_or(egui::vec2(320.0, 480.0), |r| r.size());
            self.docking.drag = Some(DockDrag {
                pane: *pane,
                spot: spot_of(&dock, *pane),
                size,
                last: None,
            });
        }
        if let Some(g) = self.docking.drag.as_mut()
            && let Some(p) = ui.input(|i| i.pointer.latest_pos())
        {
            g.last = Some(p);
        }
        let mut viewer = Viewer {
            ws: self,
            act: None,
            bodies: Vec::new(),
        };
        // 浮かせた窓の縁は付けない(外の窓と同じく、帯が窓の上の端まで来る)
        let margin = ui.spacing().window_margin;
        ui.spacing_mut().window_margin = egui::Margin::ZERO;
        DockArea::new(&mut dock)
            .id(dock_id)
            .style(style)
            .show_add_buttons(false)
            .show_leaf_collapse_buttons(false)
            .show_leaf_close_all_buttons(false)
            .tab_context_menus(false)
            .hidable_tab_bars(true)
            .draggable_tabs(true)
            .window_bounds(ui.max_rect())
            .show_inside(ui, &mut viewer);
        ui.spacing_mut().window_margin = margin;
        let mut act = viewer.act;
        let mut bodies = std::mem::take(&mut viewer.bodies);
        self.docking.tree = Some(dock);
        bodies.sort_by_key(|b| draw_order(b.0));
        for (pane, layer, body, clip) in bodies {
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("dock_body", pane))
                    .layer_id(layer)
                    .max_rect(body),
            );
            child.set_clip_rect(clip.intersect(body));
            // タブの帯を出さない所は、見出しの右クリックで部品のメニュー(引くのは egui_dock が受ける)
            let bare = self.docking.tree.as_ref().is_some_and(|d| {
                d.find_tab(&pane).is_some_and(|p| {
                    d[p.surface][p.node]
                        .get_leaf()
                        .is_some_and(|l| l.tab_bar_hidden)
                })
            });
            let float = self.docking.tree.as_ref().and_then(|d| lone_float(d, pane));
            if let Some(surface) = float {
                // 浮かせた窓は、外の窓と同じ帯(真ん中に名前、右に閉じる)。引くと窓が動く
                let h = 34.0 * theme::band();
                let bar = egui::Rect::from_min_size(body.min, egui::vec2(body.width(), h));
                let mut bar_ui = child.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("float_bar", pane))
                        .max_rect(bar)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let out = self.pane_bar(&mut bar_ui, pane, false, &mut act);
                // egui_dock は浮かせた窓を egui の窓(id は "window {surface:?}")で出すので、その場所から動かす
                let area = egui::Id::new(format!("window {surface:?}"));
                if let Some(delta) = out.drag
                    && let Some(now) = ui.ctx().memory(|m| m.area_rect(area))
                    && let Some(d) = self.docking.tree.as_mut()
                    && let Some(w) = d.get_window_state_mut(surface)
                {
                    w.set_position(now.min + delta);
                }
                // 浮かせた窓を本体の窓の外まで引いて離したら、外の窓に出す(戻す場所は浮かせた時のまま)
                if let Some(p) = out.dropped
                    && !ui.ctx().content_rect().contains(p)
                {
                    if let Some(d) = self.docking.tree.as_mut()
                        && let Some(path) = d.find_tab(&pane)
                    {
                        d.remove_tab(path);
                    }
                    self.pop_out_at(ui.ctx(), pane, p, body.size());
                    continue;
                }
                let rest = egui::Rect::from_min_max(egui::pos2(body.min.x, bar.max.y), body.max);
                let mut inner = child.new_child(egui::UiBuilder::new().max_rect(rest));
                inner.set_clip_rect(clip.intersect(rest));
                self.pane_ui(&mut inner, pane);
                continue;
            }
            if bare && pane != Pane::View {
                let strip =
                    egui::Rect::from_min_size(body.min, egui::vec2(body.width(), tab_bar_height()));
                let r = child.interact(
                    strip,
                    egui::Id::new(("dock_strip", pane)),
                    egui::Sense::click(),
                );
                r.right_menu(|ui| pane_menu(ui, pane, &mut act));
            }
            self.pane_ui(&mut child, pane);
            if pane == Pane::View {
                // データパネルから引いて来たデザインを離すと、リンクで挿入する
                self.design_drop(&child, body);
            }
        }
        // 本体の窓の外で離したら、外の窓に出す(Unity と同じ)
        if released
            && let Some(g) = self.docking.drag.take()
            && let Some(last) = g.last
            && !ui.ctx().content_rect().contains(last)
            && g.pane != Pane::View
        {
            if let Some(d) = self.docking.tree.as_mut()
                && let Some(path) = d.find_tab(&g.pane)
            {
                d.remove_tab(path);
            }
            if let Some(spot) = g.spot {
                self.docking.spots.insert(g.pane, spot);
            }
            self.pop_out_at(ui.ctx(), g.pane, last, g.size);
        } else if !ui.input(|i| i.pointer.any_down()) {
            self.docking.drag = None;
        }
        match act {
            Some(DockAction::Reset) => {
                self.reset_dock();
                return;
            }
            Some(DockAction::Hide(p)) => self.set_pane_open(p, false),
            Some(DockAction::Outer(p)) => self.pop_out(ui.ctx(), p),
            Some(DockAction::Float(p)) => {
                if let Some(d) = self.docking.tree.as_mut()
                    && let Some(path) = d.find_tab(&p)
                    && let Some(rect) = d[path.surface][path.node].rect()
                {
                    // 閉じた時に戻す場所を覚えてから浮かせる
                    if let Some(spot) = spot_of(d, p) {
                        self.docking.spots.insert(p, spot);
                    }
                    log::info!("部品を窓に浮かせる: {p:?}");
                    d.detach_tab(path, rect);
                }
            }
            Some(DockAction::Return(p)) => {
                // 並びから外すと、次の描画で覚えた場所へ戻る(隠した部品を出す時と同じ)
                if let Some(d) = self.docking.tree.as_mut()
                    && let Some(path) = d.find_tab(&p)
                {
                    log::info!("浮かせた窓を閉じたので並びへ戻す: {p:?}");
                    d.remove_tab(path);
                    ui.ctx().request_repaint();
                }
            }
            None => {}
        }
        // 引いて落とした部品は、egui_dock では半分ずつに分かれる。前の版の幅・高さ(ブラウザ・編集
        // パネルは幅、タイムラインは高さ)に合わせる。分けた所の大きさは次の描画で決まるので、その後で
        for p in std::mem::take(&mut self.docking.fit) {
            let k = theme::band().clamp(1.0, 1.6);
            // タイムラインは、上下に置けば前の高さ、左右に置けば縦の一覧の幅
            let natural: &[(bool, f32)] = match p {
                Pane::Browser => &[(true, self.win.layout.browser_width * k)],
                Pane::Panel => &[(true, self.win.layout.panel_width * k)],
                Pane::Timeline => &[(false, self.bottom_band_height()), (true, 240.0 * k)],
                Pane::Commands => &[(false, COMMANDS_H * theme::band()), (true, 360.0 * k)],
                Pane::Data => &[(true, DATA_W * k)],
                Pane::View => &[],
            };
            if let Some(d) = self.docking.tree.as_mut()
                && natural
                    .iter()
                    .any(|(horizontal, size)| fit_size(d, p, *horizontal, *size))
            {
                ui.ctx().request_repaint();
                if self.docking.custom
                    && let Some(shape) = dock_shape(d)
                {
                    self.docking.shape = Some(shape.clone());
                    self.win.layout.dock = Some(shape);
                }
            }
        }
        if released {
            let press = std::mem::take(&mut self.docking.press);
            if let Some(d) = self.docking.tree.as_ref() {
                let moved: Vec<Pane> = press
                    .iter()
                    .filter(|(p, before)| {
                        d.find_tab(p).is_some_and(|now| {
                            now.node_path() != *before
                                && now.surface.is_main()
                                && d[now.surface][now.node]
                                    .get_leaf()
                                    .is_some_and(|l| l.tabs.len() == 1)
                        })
                    })
                    .map(|(p, _)| *p)
                    .collect();
                if let [p] = moved[..] {
                    self.docking.fit = vec![p];
                    ui.ctx().request_repaint();
                }
            }
        }
        // 並びを覚える(ボタンを離した時だけ見る。タブを落とす・境を引く・タブを選ぶ・窓を動かすは、
        // どれも離した時に決まる)。既定のまま何も動かしていなければ書かない
        if ui.ctx().input(|i| i.pointer.any_released())
            && let Some(shape) = self.docking.tree.as_ref().and_then(dock_shape)
            && self.docking.shape.as_ref() != Some(&shape)
        {
            self.docking.custom = true;
            self.docking.shape = Some(shape.clone());
            self.win.layout.dock = Some(shape);
        }
        self.outer_windows(ui.ctx());
    }
}
