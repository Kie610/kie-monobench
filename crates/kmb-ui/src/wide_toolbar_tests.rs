//! 部品の木を読んで実際の入力を送り、全作業空間のツールバーを確かめる。直接の実行口は使わない。
use crate::{
    Space, Workspace,
    drive::{Query, UiDriver, UiNode},
    ribbon,
};
use egui_kittest::Harness;
use kmb_core::{Document, Input};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[path = "../../kmb-samples/tests/support/wide.rs"]
mod support;

type Screen = Harness<'static, Workspace>;

#[derive(Clone)]
struct Fixture {
    name: &'static str,
    make: fn() -> Document,
    space: Space,
}
#[derive(Clone)]
struct Case {
    fixture: Fixture,
    tab: String,
    group: String,
    item: ribbon::Item,
    menu: bool,
}
#[derive(Clone)]
struct PickCase {
    fixture: Fixture,
    tab: String,
    name: String,
    role: String,
    menu: bool,
}

#[derive(Clone)]
struct MenuControl {
    fixture: Fixture,
    tab: String,
    group: String,
    name: String,
    role: String,
    index: usize,
}

#[derive(Default)]
struct Dialogs(Arc<Mutex<Vec<String>>>);
impl crate::Dialogs for Dialogs {
    fn open_document(&self) -> Option<std::path::PathBuf> {
        self.0.lock().unwrap().push("open_document".into());
        None
    }
    fn save_document(&self, _: &str) -> Option<std::path::PathBuf> {
        self.0.lock().unwrap().push("save_document".into());
        None
    }
    fn open_file(&self, title: &str, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
        self.0.lock().unwrap().push(format!("open:{title}"));
        None
    }
    fn save_file(&self, title: &str, _: &str, _: &[&str], _: &str) -> Option<std::path::PathBuf> {
        self.0.lock().unwrap().push(format!("save:{title}"));
        None
    }
    fn unsaved(&self, _: &str) -> crate::Unsaved {
        crate::Unsaved::Cancel
    }
    fn pick_folder(&self, title: &str) -> Option<std::path::PathBuf> {
        self.0.lock().unwrap().push(format!("folder:{title}"));
        None
    }
}

fn study_fixture() -> Document {
    support::study_document(kmb_samples::reference_document, "fea.static@1")
}

fn positive_cam() -> Document {
    let cam = kmb_samples::reference_cam();
    support::positive_cam(cam.part.doc, cam.tools, cam.setup)
}

fn script(h: &mut Screen, text: &str) -> Result<(), String> {
    let batch = h.state_mut().run_script(text)?;
    let start = Instant::now();
    loop {
        h.step();
        if let Some(result) = h.state_mut().script_result(batch) {
            return if result.ok {
                Ok(())
            } else {
                Err(result.report())
            };
        }
        if start.elapsed() > Duration::from_secs(120) {
            return Err(format!("台本が時間切れ: {text}"));
        }
    }
}
fn nodes(h: &Screen) -> Vec<UiNode> {
    h.ctx
        .plugin::<UiDriver>()
        .lock()
        .nodes(h.ctx.pixels_per_point())
}

/// 同じ名前の裏のボタンを押さず、実際に出た最上層の部品を番号で指定する。
fn query_for(h: &Screen, node: &UiNode) -> Result<String, String> {
    let all = nodes(h);
    let query = format!("{}:{}", node.role, node.name);
    let parsed = Query::parse(&query);
    let found = parsed.find(&all, h.ctx.content_rect());
    let index = found
        .iter()
        .position(|n| n.id == node.id)
        .ok_or("対象が部品の木から消えました")?;
    Ok(format!("{query}#{}", index + 1))
}
fn find_named(h: &Screen, name: &str, toolbar: bool) -> Option<UiNode> {
    nodes(h)
        .into_iter()
        .filter(|n| {
            n.role != "Label"
                && n.name == name
                && (!toolbar || n.rect.center().y < h.state().win.view_rect.top())
        })
        .max_by(|a, b| {
            a.layer
                .cmp(&b.layer)
                .then_with(|| a.rect.center().y.total_cmp(&b.rect.center().y))
        })
}
fn input(h: &mut Screen, verb: &str, node: &UiNode) -> Result<(), String> {
    let query = query_for(h, node)?;
    script(
        h,
        &format!("{verb} {}", serde_json::to_string(&query).unwrap()),
    )
}
fn click_name(h: &mut Screen, name: &str, toolbar: bool) -> Result<(), String> {
    let node = find_named(h, name, toolbar).ok_or_else(|| format!("部品がありません: {name}"))?;
    input(h, "click", &node)
}
fn screen(fixture: &Fixture) -> Result<Screen, String> {
    let ws = Workspace::new((fixture.make)(), None, kmb_bundle::modules());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1920.0, 1000.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    // 木を作り、再計算と部品の配置が整うまで既存の台本の道で待つ。
    script(&mut h, "tree")?;
    if fixture.space == Space::Machining {
        let cx = h.state().context();
        let profile = cx
            .data
            .features
            .iter()
            .find(|f| {
                cx.output(f.id, "profile")
                    .and_then(<kmb_core::Profile as kmb_core::Payload>::from_value)
                    .is_some_and(|p| !p.regions.is_empty())
            })
            .map(|f| (f.id, f.name.clone()))
            .ok_or("加工に使う閉じた輪郭がありません")?;
        click_name(&mut h, &profile.1, false)?;
        if h.state().cur.selected != Some(profile.0) {
            return Err("閉じた輪郭の選択が届きません".into());
        }
    }
    let current = h.state().win.space_name(h.state().win.layout.space);
    let box_name = h
        .state()
        .win
        .t_with("toolbar-space-now", &[("name", current.into())]);
    click_name(&mut h, &box_name, true)?;
    let wanted = h.state().win.space_name(fixture.space);
    click_name(&mut h, &wanted, false)?;
    if h.state().win.layout.space != fixture.space {
        return Err("作業空間のクリックが届きません".into());
    }
    Ok(h)
}
fn tab(h: &mut Screen, name: &str) -> Result<(), String> {
    let shown = h.state().win.t_or(ribbon::tab_id(name), name);
    click_name(h, &shown, true)?;
    if h.state().win.tab != name {
        return Err(format!("タブへ切り替わりません: {name}"));
    }
    Ok(())
}
fn open_group(h: &mut Screen, name: &str) -> Result<(), String> {
    let shown = h.state().win.t_or(ribbon::group_id(name), name);
    if let Some(n) = find_named(h, &format!("{shown} ▾"), true) {
        return input(h, "click", &n);
    }
    let more = h.state().win.t("toolbar-group-more");
    click_name(h, &format!("{more} ▾"), true)?;
    let n = find_named(h, &shown, false)
        .ok_or_else(|| format!("その他にもまとまりがありません: {shown}"))?;
    input(h, "hover", &n)
}
fn pending(ws: &Workspace) -> bool {
    ws.cur.creating.is_some()
        || ws.cur.editing.is_some()
        || ws.cur.gesture.is_some()
        || ws.cur.name_ask.is_some()
}
fn cleanup(
    h: &mut Screen,
    data: &kmb_core::DocumentData,
    geometry: &support::Geometry,
    undo: usize,
) -> Result<(), String> {
    for _ in 0..8 {
        script(h, "key Escape")?;
        if !pending(h.state()) && !egui::Popup::is_any_open(&h.ctx) {
            break;
        }
    }
    if pending(h.state()) {
        return Err("Esc で作る途中の状態から抜けられません".into());
    }
    let start = Instant::now();
    while h.state().document().undo_labels().len() > undo {
        script(h, "key ctrl+z")?;
        if start.elapsed() > Duration::from_secs(120) {
            return Err("取り消しが時間切れです".into());
        }
    }
    let after_geometry = support::geometry(&h.state().context())?;
    support::restored(
        data,
        h.state().document().data(),
        geometry,
        &after_geometry,
        undo,
        h.state().document().undo_labels().len(),
    )
}
fn ui_handler_reached(ws: &Workspace, key: &str) -> bool {
    match key {
        "u:params" => ws.win.show_params,
        "u:machines" | "u:scene" => ws.settings.open,
        "u:measure" => ws.is_measuring(),
        "u:section" => ws.is_sectioning(),
        "u:tools" => ws.cur.editing.is_some(),
        "u:appearance" => ws.cur.looks.open,
        "u:press_pull" => ws.win.last_item.is_some() || pending(ws) || ws.message().is_some(),
        _ => ws.win.last_item.as_deref() == Some(key),
    }
}
fn one_case(case: &Case) -> Result<serde_json::Value, String> {
    let mut h = screen(&case.fixture)?;
    tab(&mut h, &case.tab)?;
    let before = h.state().document().data().clone();
    let geometry = support::geometry(&h.state().context())?;
    let undo = h.state().document().undo_labels().len();
    let dialogs = Arc::new(Mutex::new(Vec::new()));
    h.state_mut().win.dialogs = Box::new(Dialogs(dialogs.clone()));
    let action = case
        .item
        .key
        .strip_prefix("a:")
        .and_then(|id| h.state().actions().into_iter().find(|a| a.id == id));
    let asks_file = action
        .as_ref()
        .is_some_and(|a| matches!(a.input, Input::OpenFile { .. } | Input::SaveFile { .. }));
    let operation = case.item.key.strip_prefix("a:").unwrap_or(&case.item.key);
    if let Some(reason) = support::long_reason(operation).filter(|_| !asks_file) {
        return Ok(json!({"result":"not_run","reason":reason}));
    }
    if case.menu {
        open_group(&mut h, &case.group)?;
    }
    let node = find_named(&h, &case.item.label, !case.menu)
        .ok_or_else(|| format!("項目の部品がありません: {}", case.item.label))?;
    if let Some(reason) = &case.item.disabled {
        if !node.disabled {
            return Err("宣言では無効なのにボタンが押せます".into());
        }
        return Ok(json!({"result":"not_run","reason":reason,"disabled_node":true}));
    }
    input(&mut h, "click", &node)?;
    let calls = dialogs.lock().unwrap().clone();
    if asks_file && calls.is_empty() {
        return Err("ファイルの窓の口へ届きません".into());
    }
    if !ui_handler_reached(h.state(), &case.item.key) {
        return Err("ボタンの実行口へ届きません".into());
    }
    cleanup(&mut h, &before, &geometry, undo)?;
    Ok(
        json!({"result":if calls.is_empty(){"clicked_and_restored"}else{"dialog_cancelled_and_restored"},"dialogs":calls,"script":h.state().command_log().into_iter().map(|(kind,line)|format!("{kind:?}: {line}")).collect::<Vec<_>>()}),
    )
}

/// 右端の区画は、実際に描いた「選択」の名前の幅とタブの下端から見つける。
fn pick_controls(h: &Screen, tab_name: &str) -> Result<Vec<UiNode>, String> {
    let select = h.state().win.t("toolbar-group-select");
    let label = find_named(h, &format!("{select} ▾"), true).ok_or("選択の区画がありません")?;
    let shown_tab = h.state().win.t_or(ribbon::tab_id(tab_name), tab_name);
    let tab = find_named(h, &shown_tab, true).ok_or("タブの部品がありません")?;
    let controls: Vec<_> = nodes(h)
        .into_iter()
        .filter(|n| {
            n.role != "Label"
                && n.name != label.name
                && n.rect.center().x >= label.rect.left()
                && n.rect.center().x <= label.rect.right()
                && n.rect.center().y > tab.rect.bottom()
                && n.rect.center().y < label.rect.top()
        })
        .collect();
    if controls.len() != 4 {
        return Err(format!(
            "選択の切り替えが4個ではありません: {}",
            controls.len()
        ));
    }
    Ok(controls)
}
fn open_pick_menu(h: &mut Screen) -> Result<(), String> {
    let name = h.state().win.t("toolbar-group-select");
    click_name(h, &format!("{name} ▾"), true)
}
fn one_pick(case: &PickCase) -> Result<serde_json::Value, String> {
    let mut h = screen(&case.fixture)?;
    tab(&mut h, &case.tab)?;
    let before = h.state().document().data().clone();
    let geometry = support::geometry(&h.state().context())?;
    let undo = h.state().document().undo_labels().len();
    // 「全部選ぶ」は一つ切った状態から戻すことで、押した結果も確認する。
    if case.menu && case.name == h.state().win.t("toolbar-pick-all") {
        let first = pick_controls(&h, &case.tab)?[0].clone();
        input(&mut h, "click", &first)?;
    }
    if case.menu {
        open_pick_menu(&mut h)?;
    }
    let candidates = if case.menu {
        nodes(&h)
    } else {
        pick_controls(&h, &case.tab)?
    };
    let node = candidates
        .into_iter()
        .filter(|n| n.name == case.name && n.role == case.role)
        .max_by_key(|n| n.layer)
        .ok_or("選び方の部品がありません")?;
    if node.disabled {
        return Ok(
            json!({"result":"not_run","reason":"この選び方は今の文脈では使えない","disabled_node":true}),
        );
    }
    let old = format!("{:?}", h.state().win.pick_filter);
    input(&mut h, "click", &node)?;
    if old == format!("{:?}", h.state().win.pick_filter) {
        return Err("選び方のクリックが値に届きません".into());
    }
    cleanup(&mut h, &before, &geometry, undo)?;
    Ok(json!({"result":"clicked_and_restored","filter_changed":true}))
}

/// 一覧内のピン・戻すボタンも、実部品の名前と役割から拾う。
fn menu_controls(h: &Screen, group: &ribbon::Group) -> Vec<UiNode> {
    let all: Vec<_> = nodes(h)
        .into_iter()
        .filter(|n| {
            matches!(
                n.role.as_str(),
                "Button" | "CheckBox" | "RadioButton" | "MenuItem"
            )
        })
        .collect();
    let layer = all.iter().map(|n| n.layer).max().unwrap_or(0);
    all.into_iter()
        .filter(|n| n.layer == layer && !group.items.iter().any(|i| i.label == n.name))
        .collect()
}
fn one_menu_control(case: &MenuControl) -> Result<serde_json::Value, String> {
    let mut h = screen(&case.fixture)?;
    tab(&mut h, &case.tab)?;
    let before = h.state().document().data().clone();
    let geometry = support::geometry(&h.state().context())?;
    let undo = h.state().document().undo_labels().len();
    open_group(&mut h, &case.group)?;
    if case.name == h.state().win.t("toolbar-pins-reset") {
        let pin = h.state().win.t("toolbar-pin");
        let unpin = h.state().win.t("toolbar-unpin");
        let first = nodes(&h)
            .into_iter()
            .filter(|n| n.name == pin || n.name == unpin)
            .max_by_key(|n| n.layer)
            .ok_or("並びを変える部品がありません")?;
        input(&mut h, "click", &first)?;
        if !egui::Popup::is_any_open(&h.ctx) {
            open_group(&mut h, &case.group)?;
        }
    }
    let old = h.state().win.layout.clone();
    let all: Vec<_> = nodes(&h)
        .into_iter()
        .filter(|n| n.name == case.name && n.role == case.role)
        .collect();
    let layer = all.iter().map(|n| n.layer).max().unwrap_or(0);
    let n = all
        .into_iter()
        .filter(|n| n.layer == layer)
        .nth(case.index)
        .ok_or("一覧の操作具がありません")?;
    if n.disabled {
        return Ok(json!({"result":"not_run","reason":"一覧の操作具が無効","disabled_node":true}));
    }
    input(&mut h, "click", &n)?;
    if h.state().win.layout == old {
        return Err("並びの操作が設定へ届きません".into());
    }
    cleanup(&mut h, &before, &geometry, undo)?;
    Ok(json!({"result":"clicked_and_restored","layout_changed":true}))
}

/// 確かめること: 全空間の実物の前ボタンと一覧項目を台本で押し、Esc と取り消しで戻る。
#[test]
#[ignore = "全部の操作は時間がかかるので、週に 1 回の回と手で流す"]
fn wide_toolbar_buttons_escape_and_undo() {
    let started = Instant::now();
    let mut records = support::Report::new("toolbar");
    let mut fixtures = Vec::new();
    for space in Space::ALL {
        fixtures.push(Fixture {
            name: "基準",
            make: if space == Space::Machining {
                positive_cam
            } else {
                kmb_samples::reference_document
            },
            space,
        });
    }
    fixtures.push(Fixture {
        name: "研究",
        make: study_fixture,
        space: Space::Simulation,
    });
    fixtures.push(Fixture {
        name: "組立",
        make: kmb_samples::drawing_assembly_document,
        space: Space::Animation,
    });
    let mut cases = BTreeMap::<String, Case>::new();
    let mut picks = BTreeMap::<String, PickCase>::new();
    let mut menus = BTreeMap::<String, MenuControl>::new();
    let mut failures = Vec::new();
    for fixture in fixtures {
        let inventory = match screen(&fixture) {
            Ok(h) => h,
            Err(e) => {
                failures.push(format!("{:?}: {e}", fixture.space));
                continue;
            }
        };
        let tabs = inventory.state().ribbon_tabs();
        for t in tabs {
            let mut h = screen(&fixture).expect("空間をクリックで開ける");
            tab(&mut h, &t.name).expect("タブをクリックで開ける");
            for group in t.groups {
                let mut h = screen(&fixture).expect("まとまりごとに新しい画面を開ける");
                tab(&mut h, &t.name).expect("タブをクリックで開ける");
                let front = ribbon::front(fixture.space, &t.name, &group, &h.state().win.layout);
                for item in &group.items {
                    for menu in [false, true] {
                        if !menu
                            && (!front.iter().any(|i| i.key == item.key)
                                || find_named(&h, &item.label, true).is_none())
                        {
                            continue;
                        }
                        let key = format!(
                            "{:?}/{}/{}/{}:{}",
                            fixture.space,
                            t.name,
                            group.name,
                            item.key,
                            if menu { "menu" } else { "front" }
                        );
                        let replace = cases.get(&key).is_none_or(|old| {
                            old.item.disabled.is_some() && item.disabled.is_none()
                        });
                        if replace {
                            cases.insert(
                                key,
                                Case {
                                    fixture: fixture.clone(),
                                    tab: t.name.clone(),
                                    group: group.name.clone(),
                                    item: item.clone(),
                                    menu,
                                },
                            );
                        }
                    }
                }
                open_group(&mut h, &group.name).expect("まとまりの一覧を実クリックで開ける");
                records.event(json!({"wide":"group_inventory", "space":format!("{:?}",fixture.space),"tab":t.name,"group":group.name,"pending":pending(h.state()),"popup_open":egui::Popup::is_any_open(&h.ctx),"nodes":nodes(&h).iter().map(UiNode::to_json).collect::<Vec<_>>()}));
                assert!(
                    !pending(h.state()),
                    "一覧の列挙中に作成・編集中の状態へ入りました"
                );
                assert!(egui::Popup::is_any_open(&h.ctx), "実際の一覧が開きません");
                let mut seen = BTreeMap::<(String, String), usize>::new();
                for n in menu_controls(&h, &group) {
                    let index = seen.entry((n.name.clone(), n.role.clone())).or_default();
                    let key = format!(
                        "{:?}/{}/{}/menu-control/{}/{}",
                        fixture.space, t.name, group.name, n.name, *index
                    );
                    menus.insert(
                        key,
                        MenuControl {
                            fixture: fixture.clone(),
                            tab: t.name.clone(),
                            group: group.name.clone(),
                            name: n.name,
                            role: n.role,
                            index: *index,
                        },
                    );
                    *index += 1;
                }
                script(&mut h, "key Escape\nkey Escape").expect("一覧を閉じる");
            }
            // 右端の選択の操作具は ribbon_tabs の項目に入らないので、実部品と突き合わせる。
            let controls = match pick_controls(&h, &t.name) {
                Ok(v) => v,
                Err(e) => {
                    failures.push(format!("{:?}/{}: {e}", fixture.space, t.name));
                    continue;
                }
            };
            for n in &controls {
                let key = format!("{:?}/{}/select/front/{}", fixture.space, t.name, n.name);
                picks.insert(
                    key,
                    PickCase {
                        fixture: fixture.clone(),
                        tab: t.name.clone(),
                        name: n.name.clone(),
                        role: n.role.clone(),
                        menu: false,
                    },
                );
            }
            open_pick_menu(&mut h).expect("選び方の一覧をクリックで開ける");
            let menu_nodes: Vec<_> = nodes(&h)
                .into_iter()
                .filter(|n| matches!(n.role.as_str(), "CheckBox" | "Button"))
                .collect();
            let layer = menu_nodes.iter().map(|n| n.layer).max().unwrap_or(0);
            let menu_controls: Vec<_> = menu_nodes
                .into_iter()
                .filter(|n| n.layer == layer)
                .collect();
            if menu_controls.len() != 5 {
                failures.push(format!(
                    "選択の一覧の4切り替えと全選択が揃いません: {}",
                    menu_controls.len()
                ));
            }
            for n in &menu_controls {
                let key = format!("{:?}/{}/select/menu/{}", fixture.space, t.name, n.name);
                picks.insert(
                    key,
                    PickCase {
                        fixture: fixture.clone(),
                        tab: t.name.clone(),
                        name: n.name.clone(),
                        role: n.role.clone(),
                        menu: true,
                    },
                );
            }
            records.event(json!({"wide":"toolbar_coverage","space":format!("{:?}",fixture.space),"tab":t.name,"items":cases.len(),"selection_nodes":controls.iter().chain(&menu_controls).map(UiNode::to_json).collect::<Vec<_>>(),"gap":"有限の文書と通常の作業空間。スケッチ専用帯と未計算の研究結果は含まない"}));
        }
    }
    records.event(json!({"wide":"inventory_counts", "buttons":cases.len(),"selection_buttons":picks.len(),"menu_controls":menus.len(),"failures":failures}));
    assert!(
        failures.is_empty(),
        "部品の列挙に失敗:\n{}",
        failures.join("\n")
    );
    let mut counts = BTreeMap::<String, usize>::new();
    for (key, case) in cases {
        records.event(json!({"wide":"starting_button","key":key,"fixture":case.fixture.name}));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| one_case(&case)));
        let result = match outcome {
            Ok(Ok(result)) => result,
            Ok(Err(reason)) => json!({"result":"failure","reason":reason}),
            Err(_) => json!({"result":"failure","reason":"panic"}),
        };
        if result["result"] == "failure" {
            failures.push(format!("{key}: {}", result["reason"]));
        }
        *counts
            .entry(result["result"].as_str().unwrap_or("unknown").into())
            .or_default() += 1;
        records.event(json!({"wide":"button","key":key,"fixture":case.fixture.name,"label":case.item.label,"outcome":result}));
    }
    for (key, case) in picks {
        records.event(json!({"wide":"starting_pick_button","key":key}));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| one_pick(&case)));
        let result = match outcome {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => json!({"result":"failure","reason":e}),
            Err(_) => json!({"result":"failure","reason":"panic"}),
        };
        if result["result"] == "failure" {
            failures.push(format!("{key}: {}", result["reason"]));
        }
        *counts
            .entry(result["result"].as_str().unwrap_or("unknown").into())
            .or_default() += 1;
        records.event(json!({"wide":"pick_button","key":key,"outcome":result}));
    }
    for (key, case) in menus {
        records.event(json!({"wide":"starting_menu_control","key":key}));
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| one_menu_control(&case)));
        let result = match outcome {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => json!({"result":"failure","reason":e}),
            Err(_) => json!({"result":"failure","reason":"panic"}),
        };
        if result["result"] == "failure" {
            failures.push(format!("{key}: {}", result["reason"]));
        }
        *counts
            .entry(result["result"].as_str().unwrap_or("unknown").into())
            .or_default() += 1;
        records.event(json!({"wide":"menu_control","key":key,"outcome":result}));
    }
    records.event(json!({"wide":"button_counts","counts":counts,"failures":failures,"seconds":started.elapsed().as_secs_f64()}));
    assert!(
        failures.is_empty(),
        "広い画面の失敗:\n{}",
        failures.join("\n")
    );
}
