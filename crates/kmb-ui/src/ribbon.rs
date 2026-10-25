//! タブ付きツールバーの中身(作業空間 → タブ → まとまり → 項目)。
//! 項目はモジュールの操作と機能の記述子から作り、ここでは並べ方だけを決める(egui を使わない)。
//! 前に出すボタンは利用者がピンで決められる(決めていなければ、モジュールが既定にしたもの)。
//! 帯に入りきらない時の畳み方([`fit`])もここで決める。

use crate::layout::{Layout, Space};

/// ツールバーの項目。`key` は「a:操作の ID」「f:機能の種類」「u:画面の窓」。
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub key: String,
    pub label: String,
    /// アイコンの名前(モジュールが操作や種類に付けたもの)
    pub icon: String,
    pub hint: String,
    /// 使えない理由
    pub disabled: Option<String>,
    /// 既定で前に出すか
    pub front: bool,
    /// 前の鍵(操作が置き換えた機能の種類 `f:…`、[`kmb_core::Action::replaces`])。利用者が前の鍵で
    /// 前に出していた項目も前に出す
    pub formerly: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub name: String,
    pub groups: Vec<Group>,
}

/// 作業空間ごとのタブと、タブに並べるまとまり(既定の順)。まとまりは作業の流れの順に並べる
/// (Kie 2026-10-21 判断 3: 設計は部品を作る → 写す → 組む → 調べる → 取り込む → 書き出す、加工は
/// 作る → 確かめる → 出す)。加工は 2D・3D・穴の順(Kie 2026-10-21)。
fn tab_plan(space: Space) -> &'static [(&'static str, &'static [&'static str])] {
    match space {
        // 検査はソリッドのタブの中のまとまり(別の「検査」タブは作らない)
        Space::Design => &[(
            "ソリッド",
            &[
                "作成",
                "修正",
                "構築",
                "パターン",
                "組立",
                "検査",
                "挿入",
                "書き出し",
            ],
        )],
        Space::Render => &[(
            "レンダリング",
            &[
                "設定",
                "キャンバス内レンダリング",
                "レンダリング",
                "書き出し",
            ],
        )],
        Space::Animation => &[("アニメーション", &["変換", "ビュー", "公開"])],
        Space::Machining => &[(
            "フライス",
            &[
                "セットアップ",
                "2D",
                "3D",
                "穴",
                "確かめる",
                "NC プログラム",
                "出力",
                "管理",
            ],
        )],
        Space::Simulation => &[("解析", &["研究", "流れ", "実行", "結果", "検査"])],
    }
}

/// 項目をタブに分ける。`items` は(まとまりの名前、項目)。どのタブにもないまとまりは最初のタブの後ろに足す。
/// 同じまとまりの中では、渡した順に並べる。
pub fn tabs(space: Space, items: Vec<(String, Item)>, layout: &Layout) -> Vec<Tab> {
    let plan = tab_plan(space);
    let mut tabs: Vec<Tab> = plan
        .iter()
        .map(|(name, groups)| Tab {
            name: name.to_string(),
            groups: groups
                .iter()
                .map(|g| Group {
                    name: g.to_string(),
                    items: Vec::new(),
                })
                .collect(),
        })
        .collect();
    for (menu, item) in items {
        let menu = if menu.is_empty() {
            "その他".to_string()
        } else {
            menu
        };
        let mut placed = false;
        for t in &mut tabs {
            if let Some(g) = t.groups.iter_mut().find(|g| g.name == menu) {
                if !g.items.iter().any(|i| i.key == item.key) {
                    g.items.push(item.clone());
                }
                placed = true;
            }
        }
        if !placed {
            tabs[0].groups.push(Group {
                name: menu,
                items: vec![item],
            });
        }
    }
    for t in &mut tabs {
        t.groups.retain(|g| !g.items.is_empty());
    }
    tabs.retain(|t| !t.groups.is_empty());
    // 利用者が決めたタブの順
    if let Some(order) = layout.tab_order.get(space_key(space)) {
        tabs.sort_by_key(|t| {
            order
                .iter()
                .position(|n| *n == t.name)
                .unwrap_or(usize::MAX)
        });
    }
    tabs
}

fn space_key(space: Space) -> &'static str {
    match space {
        Space::Design => "design",
        Space::Machining => "machining",
        Space::Render => "render",
        Space::Animation => "animation",
        Space::Simulation => "simulation",
    }
}

/// スケッチの帯のまとまりの順(描く → 決める → 直す → 取り込む → 立体 → 選ぶ。Kie 2026-10-21 判断 3)。
/// まとまりは編集器が出すので、帯に並べる時にこの順へ並べ直す。
pub const SKETCH_ORDER: &[&str] = &["作成", "拘束", "寸法", "修正", "挿入", "立体", "選択"];

/// `order` にある名前のまとまりだけを、その順に並べ直す。無い名前のまとまり(3D スケッチの
/// 別のまとまりなど)は元の場所のまま。
pub fn sort_known<T: Clone>(items: &mut [T], name: impl Fn(&T) -> &str, order: &[&str]) {
    let rank = |t: &T| order.iter().position(|o| *o == name(t));
    let slots: Vec<usize> = (0..items.len())
        .filter(|&i| rank(&items[i]).is_some())
        .collect();
    let mut known: Vec<T> = slots.iter().map(|&i| items[i].clone()).collect();
    known.sort_by_key(|t| rank(t));
    for (i, t) in slots.into_iter().zip(known) {
        items[i] = t;
    }
}

/// ピンの鍵(作業空間/タブ/まとまり)。
pub fn pin_key(space: Space, tab: &str, group: &str) -> String {
    format!("{}/{tab}/{group}", space_key(space))
}

/// まとまりの前に出す項目(並び順)。
pub fn front<'a>(space: Space, tab: &str, g: &'a Group, layout: &Layout) -> Vec<&'a Item> {
    match layout.pins.get(&pin_key(space, tab, &g.name)) {
        Some(keys) => keys
            .iter()
            .filter_map(|k| {
                g.items
                    .iter()
                    .find(|i| i.key == *k || i.formerly.contains(k))
            })
            .collect(),
        None => g.items.iter().filter(|i| i.front).collect(),
    }
}

/// ピンの変え方。
#[derive(Debug, Clone, PartialEq)]
pub enum PinEdit {
    /// 前に出す(後ろに足す)
    Pin(String),
    /// 前から外す
    Unpin(String),
    /// 前の並びで 1 つ左(`-1`)か右(`1`)へ
    Move(String, i32),
    /// このまとまりを既定に戻す
    Reset,
}

/// ピンを変える。決めていなかったまとまりは、今の既定の並びから始める。
pub fn edit_pins(space: Space, tab: &str, g: &Group, layout: &mut Layout, e: PinEdit) {
    let key = pin_key(space, tab, &g.name);
    if e == PinEdit::Reset {
        layout.pins.remove(&key);
        return;
    }
    let mut keys: Vec<String> = front(space, tab, g, layout)
        .iter()
        .map(|i| i.key.clone())
        .collect();
    match e {
        PinEdit::Pin(k) => {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        PinEdit::Unpin(k) => keys.retain(|x| *x != k),
        PinEdit::Move(k, d) => {
            if let Some(i) = keys.iter().position(|x| *x == k) {
                let j = (i as i64 + d as i64).clamp(0, keys.len() as i64 - 1) as usize;
                let it = keys.remove(i);
                keys.insert(j, it);
            }
        }
        PinEdit::Reset => unreachable!(),
    }
    layout.pins.insert(key, keys);
}

/// 項目が別のまとまりへ移った時(スケッチのミラーを「修正」から「作成」へなど)、利用者が前に
/// 出していた項目を、移った先のまとまりの前にも出す。移る前のまとまりのピンに残った鍵は、
/// そのまとまりに無いので前に出ず、黙って消えてしまうため。どのまとまりにも無い鍵(別の
/// 編集器の道具)はそのまま残す。変えたら真。
pub fn migrate_pins(space: Space, tab: &str, groups: &[Group], layout: &mut Layout) -> bool {
    let mut changed = false;
    for g in groups {
        let key = pin_key(space, tab, &g.name);
        let Some(keys) = layout.pins.get(&key).cloned() else {
            continue;
        };
        for k in keys {
            if g.items.iter().any(|i| i.key == k) {
                continue;
            }
            let Some(to) = groups
                .iter()
                .find(|h| h.name != g.name && h.items.iter().any(|i| i.key == k))
            else {
                continue;
            };
            if let Some(v) = layout.pins.get_mut(&key) {
                v.retain(|x| *x != k);
            }
            edit_pins(space, tab, to, layout, PinEdit::Pin(k));
            changed = true;
        }
    }
    changed
}

/// 帯の 1 つのまとまりの出し方。帯に入りきらない時は右のまとまりから順に畳む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fold {
    /// 前の並びをそのまま(名前を出す設定なら名前つき)
    Full,
    /// 前の並びの先頭から、この数だけをアイコンだけで出す(残りはまとまりの名前の ▾ の一覧から)
    Compact(usize),
    /// 帯に出さず、右端の「その他」の一覧に入れる
    Hidden,
}

/// まとまりの幅(出し方ごと)。
#[derive(Debug, Clone, PartialEq)]
pub struct Widths {
    /// 前の並びをそのまま出した幅
    pub full: f32,
    /// 前の並びの先頭から k 個(`compact[k - 1]`)をアイコンだけで出した幅。前の並びが空でも 1 つは持つ
    pub compact: Vec<f32>,
}

impl Widths {
    fn of(&self, f: Fold) -> f32 {
        match f {
            Fold::Full => self.full,
            Fold::Compact(k) => self
                .compact
                .get(k.max(1) - 1)
                .copied()
                .unwrap_or(self.full)
                .min(self.full),
            Fold::Hidden => 0.0,
        }
    }
}

/// 帯の幅 `width` に収まるように、まとまりごとの出し方を決める。`fixed` は必ず出す物
/// (「選択」のまとまり・「スケッチを終了」)の幅、`more` は右端の「その他」の幅。
/// 右のまとまりから順にアイコンだけにし、それでも入らなければ右から 1 つずつボタンを減らし
/// (先頭の 1 つまで)、それでも入らなければ右から「その他」へ移す。並びの順は変えない
/// (Microsoft のリボンの縮め方と同じ順)。
pub fn fit(groups: &[Widths], fixed: f32, more: f32, width: f32) -> Vec<Fold> {
    let mut fold = vec![Fold::Full; groups.len()];
    let fits = |fold: &[Fold]| -> bool {
        let hidden = fold.contains(&Fold::Hidden);
        groups.iter().zip(fold).map(|(w, f)| w.of(*f)).sum::<f32>()
            + fixed
            + if hidden { more } else { 0.0 }
            <= width
    };
    for i in (0..groups.len()).rev() {
        if fits(&fold) {
            return fold;
        }
        fold[i] = Fold::Compact(groups[i].compact.len());
    }
    for i in (0..groups.len()).rev() {
        for k in (1..groups[i].compact.len()).rev() {
            if fits(&fold) {
                return fold;
            }
            fold[i] = Fold::Compact(k);
        }
    }
    for i in (0..groups.len()).rev() {
        if fits(&fold) {
            return fold;
        }
        fold[i] = Fold::Hidden;
    }
    fold
}

/// タブを 1 つ左(`-1`)か右(`1`)へ動かす。
pub fn move_tab(space: Space, tabs: &[Tab], name: &str, delta: i32, layout: &mut Layout) {
    let mut names: Vec<String> = tabs.iter().map(|t| t.name.clone()).collect();
    if let Some(i) = names.iter().position(|n| n == name) {
        let j = (i as i64 + delta as i64).clamp(0, names.len() as i64 - 1) as usize;
        let n = names.remove(i);
        names.insert(j, n);
    }
    layout.tab_order.insert(space_key(space).into(), names);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, front: bool) -> Item {
        Item {
            key: key.into(),
            label: key.into(),
            icon: String::new(),
            hint: String::new(),
            disabled: None,
            front,
            formerly: Vec::new(),
        }
    }

    /// 操作が置き換えた機能の種類を前に出していた利用者は、その操作が前に出る(回転は欄で足す種類から
    /// クリックで作る操作に替わった。対応は画面に名前の表で持たず、操作の宣言から引く)。
    #[test]
    fn pins_of_a_replaced_feature_type_bring_the_new_action_to_the_front() {
        let mut layout = Layout::default();
        let mut revolve = item("a:new.revolve", false);
        revolve.formerly = vec!["f:old.revolve@1".into()];
        let items = vec![
            ("作成".to_string(), item("a:sketch", true)),
            ("作成".into(), revolve),
        ];
        let t = tabs(Space::Design, items, &layout);
        let g = &t[0].groups[0];
        layout.pins.insert(
            pin_key(Space::Design, "ソリッド", "作成"),
            vec!["f:old.revolve@1".into(), "a:sketch".into()],
        );
        let keys: Vec<&str> = front(Space::Design, "ソリッド", g, &layout)
            .iter()
            .map(|i| i.key.as_str())
            .collect();
        assert_eq!(keys, ["a:new.revolve", "a:sketch"]);
    }

    #[test]
    fn groups_go_to_their_tabs_and_pins_override_the_defaults() {
        let mut layout = Layout::default();
        let items = vec![
            ("作成".to_string(), item("a:sketch", true)),
            ("作成".into(), item("a:text", false)),
            ("修正".into(), item("f:boolean", true)),
            ("新しい".into(), item("a:new", false)),
            ("".into(), item("a:other", false)),
        ];
        let t = tabs(Space::Design, items, &layout);
        assert_eq!(t[0].name, "ソリッド");
        let names: Vec<&str> = t[0].groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["作成", "修正", "新しい", "その他"]);
        // 空のタブ(調べる)は残らない
        assert_eq!(t.len(), 1);
        let g = &t[0].groups[0];
        let keys = |l: &Layout| -> Vec<String> {
            front(Space::Design, "ソリッド", g, l)
                .iter()
                .map(|i| i.key.clone())
                .collect()
        };
        assert_eq!(keys(&layout), ["a:sketch"]);
        edit_pins(
            Space::Design,
            "ソリッド",
            g,
            &mut layout,
            PinEdit::Pin("a:text".into()),
        );
        assert_eq!(keys(&layout), ["a:sketch", "a:text"]);
        edit_pins(
            Space::Design,
            "ソリッド",
            g,
            &mut layout,
            PinEdit::Move("a:text".into(), -1),
        );
        assert_eq!(keys(&layout), ["a:text", "a:sketch"]);
        edit_pins(
            Space::Design,
            "ソリッド",
            g,
            &mut layout,
            PinEdit::Unpin("a:sketch".into()),
        );
        assert_eq!(keys(&layout), ["a:text"]);
        edit_pins(Space::Design, "ソリッド", g, &mut layout, PinEdit::Reset);
        assert_eq!(keys(&layout), ["a:sketch"]);
    }

    #[test]
    fn pins_follow_items_that_moved_to_another_group() {
        let mut layout = Layout::default();
        // 前の版: ミラーは「修正」にあり、利用者が前に出していた
        layout.pins.insert(
            pin_key(Space::Design, "スケッチ", "修正"),
            vec!["trim".into(), "mirror".into(), "tool3:x".into()],
        );
        let draw = Group {
            name: "作成".into(),
            items: vec![item("line", true), item("mirror", false)],
        };
        let modify = Group {
            name: "修正".into(),
            items: vec![item("trim", true), item("fillet", true)],
        };
        let groups = [draw, modify];
        assert!(migrate_pins(
            Space::Design,
            "スケッチ",
            &groups,
            &mut layout
        ));
        let keys = |g: &Group, l: &Layout| -> Vec<String> {
            front(Space::Design, "スケッチ", g, l)
                .iter()
                .map(|i| i.key.clone())
                .collect()
        };
        // 描くは既定の前の並びに、移ってきたミラーを足す
        assert_eq!(keys(&groups[0], &layout), ["line", "mirror"]);
        // 直すは利用者の並びのまま(ミラーは抜ける)
        assert_eq!(keys(&groups[1], &layout), ["trim"]);
        // どのまとまりにも無い鍵(3D スケッチの道具)は残す
        assert_eq!(
            layout.pins[&pin_key(Space::Design, "スケッチ", "修正")],
            ["trim", "tool3:x"]
        );
        // 2 回目は何もしない
        assert!(!migrate_pins(
            Space::Design,
            "スケッチ",
            &groups,
            &mut layout
        ));
    }

    #[test]
    fn tabs_can_be_reordered() {
        let mut layout = Layout::default();
        // 設計のタブは今 1 つ(検査はソリッドのまとまり)なので、並べ替えはタブの並びで確かめる
        let tab = |name: &str| Tab {
            name: name.into(),
            groups: Vec::new(),
        };
        let t = [tab("ソリッド"), tab("検査")];
        move_tab(Space::Design, &t, "検査", -1, &mut layout);
        assert_eq!(layout.tab_order["design"], ["検査", "ソリッド"]);
    }

    #[test]
    fn groups_follow_the_order_of_the_work() {
        let names = |space: Space, menus: &[&str]| -> Vec<String> {
            let items = menus
                .iter()
                .map(|m| (m.to_string(), item(&format!("a:{m}"), true)))
                .collect();
            tabs(space, items, &Layout::default())[0]
                .groups
                .iter()
                .map(|g| g.name.clone())
                .collect()
        };
        assert_eq!(
            names(
                Space::Design,
                &["挿入", "検査", "組立", "パターン", "修正", "作成"]
            ),
            ["作成", "修正", "パターン", "組立", "検査", "挿入"]
        );
        assert_eq!(
            names(
                Space::Machining,
                &[
                    "管理",
                    "出力",
                    "確かめる",
                    "NC プログラム",
                    "穴",
                    "3D",
                    "2D",
                    "セットアップ"
                ]
            ),
            [
                "セットアップ",
                "2D",
                "3D",
                "穴",
                "確かめる",
                "NC プログラム",
                "出力",
                "管理"
            ]
        );
        // スケッチ: 知っているまとまりだけを並べ直し、知らないまとまりは元の場所のまま
        let mut g = [
            "作成",
            "修正",
            "拘束",
            "寸法",
            "線の種類",
            "挿入",
            "立体",
            "選択",
        ];
        sort_known(&mut g, |n| n, SKETCH_ORDER);
        assert_eq!(
            g,
            [
                "作成",
                "拘束",
                "寸法",
                "修正",
                "線の種類",
                "挿入",
                "立体",
                "選択"
            ]
        );
    }

    #[test]
    fn inspect_is_a_group_of_the_solid_tab_not_a_second_tab() {
        let items = vec![
            ("作成".to_string(), item("a:x", true)),
            ("検査".into(), item("a:m", true)),
        ];
        let t = tabs(Space::Design, items, &Layout::default());
        assert_eq!(t.len(), 1);
        let names: Vec<&str> = t[0].groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["作成", "検査"]);
    }

    #[test]
    fn groups_fold_from_the_right_until_the_band_fits() {
        // 名前つきのボタン 100、アイコンだけ 40(まとまりの幅 = 数 × 幅)
        let w = |n: usize| Widths {
            full: n as f32 * 100.0,
            compact: (1..=n).map(|k| k as f32 * 40.0).collect(),
        };
        let g = [w(4), w(3), w(2)];
        use Fold::*;
        // 入る時は畳まない
        assert_eq!(fit(&g, 100.0, 50.0, 1000.0), [Full, Full, Full]);
        // 右から順にアイコンだけにする
        assert_eq!(fit(&g, 100.0, 50.0, 880.0), [Full, Full, Compact(2)]);
        assert_eq!(fit(&g, 100.0, 50.0, 700.0), [Full, Compact(3), Compact(2)]);
        // 全部アイコンだけでも入らなければ、右から 1 つずつ減らす(先頭の 1 つまで)
        assert_eq!(
            fit(&g, 100.0, 50.0, 440.0),
            [Compact(4), Compact(3), Compact(1)]
        );
        assert_eq!(
            fit(&g, 100.0, 50.0, 380.0),
            [Compact(4), Compact(2), Compact(1)]
        );
        assert_eq!(
            fit(&g, 100.0, 50.0, 220.0),
            [Compact(1), Compact(1), Compact(1)]
        );
        // それでも入らなければ、右から「その他」へ(その他の幅も足す)
        assert_eq!(fit(&g, 100.0, 50.0, 200.0), [Compact(1), Hidden, Hidden]);
        assert_eq!(fit(&g, 100.0, 50.0, 10.0), [Hidden, Hidden, Hidden]);
    }
}
