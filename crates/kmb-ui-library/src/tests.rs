use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use kmb_ui::Workspace;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kmb-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 属性を付けた文書を、版と縮小図つきで保存する(画面と同じ保存の道)。
fn save_design(path: &Path, doc: kmb_core::Document, props: &[(&str, &str)]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut h = kmb_session::Headless::with_document(kmb_bundle::modules(), doc, None);
    h.save(Some(path)).unwrap();
    if !props.is_empty() {
        let values: Vec<(String, String)> = props
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        h.set_properties(&values).unwrap();
        h.save(None).unwrap();
    }
}

/// 見本のデザインと、最近使ったデザインの一覧を `base` に作る。返すのはデータフォルダ。
/// 天板(ピン留め・作品「棚」)・台座・クランプ・見つからない「古い試作」。
fn make_fixture(base: &Path, data: &Path) -> Vec<PathBuf> {
    let shelf = base.join("棚");
    let jig = base.join("治具");
    let files = vec![
        shelf.join("天板.json"),
        shelf.join("台座.json"),
        jig.join("クランプ.json"),
        base.join("古い試作.json"),
    ];
    save_design(
        &files[0],
        kmb_samples::reference_document(),
        &[
            ("title", "棚の天板"),
            ("project", "棚"),
            ("genre", "家具"),
            ("tags", "ナラ, 塗装"),
            ("material", "ナラ"),
        ],
    );
    save_design(
        &files[1],
        kmb_samples::reference_cam_document(),
        &[("project", "棚"), ("material", "A5052")],
    );
    save_design(&files[2], kmb_samples::reference_cam_3d_document(), &[]);
    save_design(&files[3], kmb_samples::reference_document(), &[]);
    std::fs::remove_file(&files[3]).unwrap();
    let mut recent = kmb_session::Recent::default();
    for f in files.iter().rev() {
        recent.push(f);
    }
    recent.set_pinned(&std::path::absolute(&files[0]).unwrap(), true);
    recent.save(data).unwrap();
    let mut projects = kmb_session::projects::Projects::default();
    projects.add(&shelf).unwrap();
    projects.save(data).unwrap();
    files
}

/// 画面を撮る時の見本の場を作る道具。`JC_LIBRARY_FIXTURE` のフォルダにデザインを、
/// `JC_LIBRARY_DATA` のデータフォルダに最近の一覧を書く(scripts/ui-shot/session.sh で撮る時に使う)。
#[test]
#[ignore = "道具: JC_LIBRARY_FIXTURE と JC_LIBRARY_DATA に見本の場を書く"]
fn write_fixture() {
    let (Ok(base), Ok(data)) = (
        std::env::var("JC_LIBRARY_FIXTURE"),
        std::env::var("JC_LIBRARY_DATA"),
    ) else {
        return;
    };
    make_fixture(Path::new(&base), Path::new(&data));
    // 整理の頁に出る物(中身が同じ写し・名前の似た物・分類の無い物)
    let shelf = Path::new(&base).join("棚");
    std::fs::create_dir_all(shelf.join("古い")).unwrap();
    std::fs::copy(
        shelf.join("天板.json"),
        shelf.join("古い").join("天板 (1).json"),
    )
    .unwrap();
    save_design(
        &shelf.join("天板_v2.json"),
        kmb_samples::reference_document(),
        &[("genre", "家具")],
    );
}

fn home(ws: Workspace, size: [f32; 2]) -> Harness<'static, Workspace> {
    // 概要と木は裏で読み、読み終えるたびに描き直すので、run は描き直しが続いても止めない(run_ok)
    let mut h = Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run_ok();
    h
}

/// 裏で読む物を待つ長さの上限。検査の全体を並べて流すと読むスレッドが CPU を待つので、長めに取る。
const SETTLE_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// 裏で読む概要・縮小図・フォルダの中身が届き、`until` が成り立つまで回す。成り立たないまま
/// 上限を過ぎたら `what` を書いて止める(待ちきれずに先へ進み、別の所で落ちないように)。
///
/// 仕分けの後の読み直しの間は前の中身を出すので、`until` には次に使う物そのもの(中央のカードなら
/// [`find_card`])を書く。名前だけで探すと、右の概要の「最近変えたデザイン」など同じ名前の別の物で
/// 成り立ってしまい、読み直しを待たない。
fn settle(
    h: &mut Harness<'static, Workspace>,
    what: &str,
    until: impl Fn(&Harness<'static, Workspace>) -> bool,
) {
    let start = std::time::Instant::now();
    while !until(h) {
        assert!(
            start.elapsed() < SETTLE_LIMIT,
            "{what}を {} 秒待っても揃わない",
            SETTLE_LIMIT.as_secs()
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.run_ok();
    }
}

/// 中央に「`name`」のカードが出るまで待つ。
fn wait_card(h: &mut Harness<'static, Workspace>, name: &str) {
    settle(h, &format!("中央の「{name}」のカード"), |h| {
        find_card(h, name).is_some()
    });
}

/// プロジェクトの中身(左の木と中央の「部品」フォルダ)が出るまで待つ。
fn wait_project(h: &mut Harness<'static, Workspace>) {
    settle(h, "左の木と中央の「部品」", |h| {
        find_tree_row(h, "部品").is_some() && find_card(h, "部品").is_some()
    });
}

/// データパネルに「`name`」が出るまで待つ。
fn wait_in_panel(h: &mut Harness<'static, Workspace>, name: &str) {
    settle(h, &format!("データパネルの「{name}」"), |h| {
        find_in_panel(h, name).is_some()
    });
}

/// 右の概要の分類の欄の値(欄が無ければ `None`)。
fn field(h: &Harness<'static, Workspace>, label: &str) -> Option<String> {
    h.query_all_by_label(label)
        .find(|n| format!("{:?}", n.accesskit_node().role()) == "TextInput")
        .and_then(|n| n.value())
}

#[test]
fn home_shows_recent_designs_and_a_summary_of_the_selected_one() {
    let base = temp_dir("home");
    let data = base.join("data");
    let files = make_fixture(&base.join("作業"), &data);
    let ws = Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_home(Library::new())
        .with_data_dir(data.clone())
        .with_samples(vec![("基準部品".into(), kmb_samples::reference_document)])
        .show_start();
    let mut h = home(ws, [1400.0, 820.0]);
    // 左: 始め方・場所。初めは最近使ったデザイン
    h.get_by_label("新規デザイン");
    h.get_by_label("開く…");
    h.get_by_label("プロジェクトを追加");
    // 左の場所・中央の見出し・右の概要(何も選んでいない時は今の場所)の 3 つ
    assert_eq!(h.query_all_by_label("最近使ったデザイン").count(), 3);
    h.get_by_label("ピン留めしたデザイン");
    h.get_by_label("棚");
    // 中央: 4 枚のカード(見つからない物も)
    for n in ["天板", "台座", "クランプ", "古い試作"] {
        h.get_by_label(n);
    }
    // 押すと選び、右に概要(分類・版・ファイル)
    h.get_by_label("天板").click();
    h.run_ok();
    settle(&mut h, "右の概要の「部品名」の欄", |h| {
        field(h, "部品名").is_some()
    });
    assert_eq!(field(&h, "部品名").as_deref(), Some("棚の天板"));
    assert_eq!(field(&h, "タグ").as_deref(), Some("ナラ, 塗装"));
    h.get_by_label("版の履歴をすべて見る");
    assert!(
        h.query_all_by_label_contains("v2").next().is_some(),
        "版の番号"
    );
    assert!(h.state().is_start_screen(), "押しただけでは開かない");
    // 右の「開く」で開く
    h.get_by_label("開く").click();
    h.run_ok();
    assert!(!h.state().is_start_screen());
    assert_eq!(
        h.state().path().map(Path::to_path_buf),
        Some(std::path::absolute(&files[0]).unwrap())
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn pinned_page_and_search_narrow_the_cards() {
    let base = temp_dir("filter");
    let data = base.join("data");
    make_fixture(&base.join("作業"), &data);
    let ws = Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_home(Library::new())
        .with_data_dir(data)
        .show_start();
    let mut h = home(ws, [1400.0, 820.0]);
    h.get_by_label("ピン留めしたデザイン").click();
    h.run_ok();
    h.get_by_label("天板");
    assert!(h.query_by_label("台座").is_none());
    h.query_all_by_label("最近使ったデザイン")
        .next()
        .unwrap()
        .click();
    h.run_ok();
    // 作品の名前でも探せる(作品は裏で読む概要にあるので、読み終えて絞られるまで待つ)
    h.get_by_label("名前・作品・タグで探す").click();
    h.get_by_label("名前・作品・タグで探す").type_text("棚");
    h.run_ok();
    settle(&mut h, "作品「棚」で絞ったカード", |h| {
        find_card(h, "天板").is_some()
            && find_card(h, "台座").is_some()
            && h.query_by_label("クランプ").is_none()
    });
    h.get_by_label("天板");
    h.get_by_label("台座");
    assert!(h.query_by_label("クランプ").is_none());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_document_is_opened_from_home_and_samples_and_steps_are_kept() {
    let d = temp_dir("start");
    let file = d.join("部品.json");
    kmb_session::save(&file, &kmb_samples::reference_document()).unwrap();
    let ws = Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_home(Library::new())
        .with_dialogs(Box::new(OpenOnly(file.clone())))
        .with_data_dir(d.join("data"))
        .with_samples(vec![("基準部品".into(), kmb_samples::reference_document)])
        .show_start();
    let mut h = home(ws, [1200.0, 760.0]);
    // 初めて使う(最近が無い)うちは、中央に手順の案内
    h.get_by_label("はじめての手順");
    h.get_by_label_contains("Ctrl+K のコマンド検索");
    h.get_by_label("見本とはじめての手順").click();
    h.run_ok();
    h.get_by_label("基準部品");
    h.get_by_label("開く…").click();
    h.run_ok();
    assert!(!h.state().is_start_screen());
    assert_eq!(h.state().path(), Some(file.as_path()));
    let _ = std::fs::remove_dir_all(&d);
}

/// プロジェクトの場: 棚(天板・台座、下に「部品」フォルダと脚)。データフォルダを返す。
fn project_fixture(d: &Path) -> (PathBuf, PathBuf) {
    let proj = d.join("棚");
    save_design(
        &proj.join("天板.json"),
        kmb_samples::reference_document(),
        &[("genre", "家具"), ("tags", "ナラ")],
    );
    save_design(
        &proj.join("台座.json"),
        kmb_samples::reference_document(),
        &[],
    );
    save_design(
        &proj.join("部品").join("脚.json"),
        kmb_samples::sample_document(),
        &[("genre", "家具")],
    );
    let data = d.join("data");
    let mut p = kmb_session::projects::Projects::default();
    p.add(&proj).unwrap();
    p.save(&data).unwrap();
    let mut recent = kmb_session::Recent::default();
    recent.push(&proj.join("天板.json"));
    recent.save(&data).unwrap();
    (std::path::absolute(&proj).unwrap(), data)
}

/// 左の木の行(中央のフォルダのカードと同じ名前なので、左の区画にある方)。
fn find_tree_row<'a>(
    h: &'a Harness<'static, Workspace>,
    name: &'a str,
) -> Option<egui_kittest::Node<'a>> {
    h.query_all_by_label(name).find(|n| n.rect().left() < 240.0)
}

fn tree_row<'a>(h: &'a Harness<'static, Workspace>, name: &'a str) -> egui_kittest::Node<'a> {
    find_tree_row(h, name).unwrap_or_else(|| panic!("左の木に「{name}」が無い"))
}

/// 中央の帯の「元に戻す」(上の帯の文書の「元に戻す」と分ける)。
fn undo_band<'a>(h: &'a Harness<'static, Workspace>) -> egui_kittest::Node<'a> {
    h.query_all_by_label("元に戻す")
        .find(|n| n.rect().left() > 240.0)
        .expect("中央の帯に「元に戻す」")
}

/// 中央のカード(左の木と右の概要を除く)。
fn find_card<'a>(
    h: &'a Harness<'static, Workspace>,
    name: &'a str,
) -> Option<egui_kittest::Node<'a>> {
    h.query_all_by_label(name)
        .find(|n| n.rect().left() > 240.0 && n.rect().right() < 1100.0)
}

fn card_of<'a>(h: &'a Harness<'static, Workspace>, name: &'a str) -> egui_kittest::Node<'a> {
    find_card(h, name).unwrap_or_else(|| panic!("中央に「{name}」のカードが無い"))
}

fn project_home(data: PathBuf) -> Harness<'static, Workspace> {
    let ws = Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_home(Library::new())
        .with_data_dir(data)
        .show_start();
    home(ws, [1400.0, 820.0])
}

#[test]
fn a_model_is_found_and_opened_from_a_project_folder() {
    let d = temp_dir("projects");
    let (proj, data) = project_fixture(&d);
    let mut h = project_home(data);
    // 左のプロジェクトを押すと、前の頁ではなくホームの中央にフォルダとデザインのカード
    h.get_by_label("棚").click();
    h.run_ok();
    wait_project(&mut h);
    assert!(
        h.query_by_label("プロジェクトから探す").is_none(),
        "前の頁は出さない"
    );
    tree_row(&h, "部品");
    card_of(&h, "部品");
    card_of(&h, "天板");
    card_of(&h, "台座");
    assert!(
        h.query_all_by_label("脚").all(|n| n.rect().left() > 1100.0),
        "下のフォルダの物は中に入ってから(右の「最近変えたデザイン」には出る)"
    );
    // 何も選んでいない時の右は、プロジェクトの概要(下のフォルダも数える)
    h.get_by_label("3 件");
    h.get_by_label("エクスプローラーで開く");
    // フォルダのカードの右クリックの「開く」で中へ(二度押しと同じ)
    card_of(&h, "部品").click_secondary();
    h.run_ok();
    h.get_by_label("開く").click();
    h.run_ok();
    wait_card(&mut h, "脚");
    card_of(&h, "脚").click();
    h.run_ok();
    h.get_by_label("開く").click();
    h.run_ok();
    assert!(!h.state().is_start_screen());
    assert_eq!(
        h.state().path().map(Path::to_path_buf),
        Some(proj.join("部品").join("脚.json"))
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn designs_are_moved_by_menu_and_by_dragging_and_come_back() {
    let d = temp_dir("move");
    let (proj, data) = project_fixture(&d);
    let mut h = project_home(data.clone());
    h.get_by_label("棚").click();
    h.run_ok();
    wait_project(&mut h);
    // 右クリックの「移す」から「部品」へ
    card_of(&h, "天板").click_secondary();
    h.run_ok();
    h.get_by_label("移す ⏵").click();
    h.run_ok();
    h.query_all_by_label_contains("部品")
        .find(|n| {
            n.rect().left() > 240.0
                && n.value().is_none()
                && n.rect().height() < 40.0
                && n.rect().top() > card_of(&h, "天板").rect().top()
        })
        .expect("行き先の一覧に「部品」")
        .click();
    h.run_ok();
    let moved = proj.join("部品").join("天板.json");
    assert!(moved.is_file() && !proj.join("天板.json").exists());
    assert_eq!(
        kmb_session::history::list(&moved).len(),
        2,
        "版の履歴も一緒に"
    );
    assert_eq!(
        kmb_session::organize::moved(&proj)[0].to,
        moved,
        "元のフォルダに移した記録"
    );
    assert!(
        kmb_session::Recent::load(&data).files.contains(&moved),
        "最近の一覧のパスも付け替える"
    );
    // 帯の「元に戻す」で戻る
    assert!(
        h.query_all_by_label_contains("「天板」を「部品」へ移しました")
            .next()
            .is_some()
    );
    undo_band(&h).click();
    h.run_ok();
    assert!(proj.join("天板.json").is_file() && !moved.exists());
    assert!(
        kmb_session::Recent::load(&data)
            .files
            .contains(&proj.join("天板.json"))
    );
    // 台座を左の木の「部品」へ引いて離す
    wait_card(&mut h, "台座");
    let from = card_of(&h, "台座").rect().center();
    let to = tree_row(&h, "部品").rect().center();
    h.drag_at(from);
    h.run_ok();
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run_ok();
    }
    h.drop_at(to);
    h.run_ok();
    assert!(
        proj.join("部品").join("台座.json").is_file(),
        "引いて離すと移る"
    );
    // ホームでの Ctrl+Z でも戻る
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_ok();
    assert!(proj.join("台座.json").is_file());
    // 左のフォルダを押すのは開くだけ(選んでいても移さない。Kie 2026-10-23「a」)
    wait_card(&mut h, "台座");
    card_of(&h, "台座").click();
    h.run_ok();
    tree_row(&h, "部品").click();
    h.run_ok();
    assert!(proj.join("台座.json").is_file());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_design_is_renamed_classified_trashed_and_restored_and_a_folder_is_made() {
    let d = temp_dir("organize");
    let (proj, data) = project_fixture(&d);
    let mut h = project_home(data);
    h.get_by_label("棚").click();
    h.run_ok();
    wait_card(&mut h, "台座");
    // 名前を変える(右クリック → 名前の欄 → Enter)
    card_of(&h, "台座").click_secondary();
    h.run_ok();
    h.get_by_label("名前を変える…").click();
    h.run_ok();
    h.get_by_label("新しい名前").type_text("2");
    h.key_press(egui::Key::Enter);
    h.run_ok();
    let renamed = proj.join("台座2.json");
    assert!(renamed.is_file() && !proj.join("台座.json").exists());
    assert_eq!(kmb_session::history::list(&renamed).len(), 1, "版の履歴も");
    // 分類を開かずに直す(欄を離れると保存し、新しい版「分類を変更」)
    wait_card(&mut h, "台座2");
    card_of(&h, "台座2").click();
    h.run_ok();
    settle(&mut h, "右の概要の「材料」の欄", |h| {
        field(h, "材料").is_some()
    });
    let input = |h: &Harness<'static, Workspace>| {
        h.query_all_by_label("材料")
            .find(|n| format!("{:?}", n.accesskit_node().role()) == "TextInput")
            .unwrap()
            .rect()
            .center()
    };
    let at = input(&h);
    h.query_all_by_label("材料")
        .find(|n| n.rect().center() == at)
        .unwrap()
        .click();
    h.run_ok();
    h.query_all_by_label("材料")
        .find(|n| n.rect().center() == at)
        .unwrap()
        .type_text("A5052");
    h.key_press(egui::Key::Enter);
    h.run_ok();
    assert_eq!(kmb_session::projects::model_info(&renamed).material, "A5052");
    assert_eq!(kmb_session::history::list(&renamed)[0].note, "分類を変更");
    // ごみ箱へ(確かめの帯で一覧を出してから)と、元に戻す
    card_of(&h, "台座2").click_secondary();
    h.run_ok();
    h.get_by_label("削除…").click();
    h.run_ok();
    h.get_by_label_contains("PC のごみ箱へ移します");
    assert!(renamed.is_file(), "確かめるまでは消さない");
    h.get_by_label("ごみ箱へ移す").click();
    h.run_ok();
    assert!(!renamed.exists());
    undo_band(&h).click();
    h.run_ok();
    assert!(renamed.is_file(), "ごみ箱から戻る");
    // 新しいフォルダ(左の欄に名前を書いて Enter)
    h.get_by_label("新しいフォルダの名前").click();
    h.get_by_label("新しいフォルダの名前").type_text("治具");
    h.key_press(egui::Key::Enter);
    h.run_ok();
    assert!(proj.join("治具").is_dir());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn several_designs_are_selected_with_ctrl_and_summarized_together() {
    let d = temp_dir("multi");
    let (_, data) = project_fixture(&d);
    let mut h = project_home(data);
    h.get_by_label("棚").click();
    h.run_ok();
    wait_card(&mut h, "台座");
    card_of(&h, "天板").click();
    h.run_ok();
    card_of(&h, "台座").click_modifiers(egui::Modifiers::COMMAND);
    h.run_ok();
    // 中央の帯と右の概要の 2 か所
    assert_eq!(h.query_all_by_label("2 件を選択中").count(), 2);
    h.get_by_label("全部に共通の分類");
    h.get_by_label("移すと一緒に動く物");
    h.get_by_label("選択を解除").click();
    h.run_ok();
    assert!(h.query_by_label("2 件を選択中").is_none());
    let _ = std::fs::remove_dir_all(&d);
}

/// 開く窓だけを返すダイアログ。
struct OpenOnly(PathBuf);

impl kmb_ui::Dialogs for OpenOnly {
    fn open_document(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn save_document(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn open_file(&self, _: &str, _: &str, _: &[&str]) -> Option<PathBuf> {
        None
    }
    fn save_file(&self, _: &str, _: &str, _: &[&str], _: &str) -> Option<PathBuf> {
        None
    }
    fn unsaved(&self, _: &str) -> kmb_ui::Unsaved {
        kmb_ui::Unsaved::Cancel
    }
}

/// データパネルの中の物(ブラウザ・ビューを除く。パネルは左いっぱいに出る)。
fn find_in_panel<'a>(
    h: &'a Harness<'static, Workspace>,
    name: &'a str,
) -> Option<egui_kittest::Node<'a>> {
    h.query_all_by_label(name)
        .find(|n| n.rect().left() < 300.0 && n.rect().top() > 60.0)
}

fn in_panel<'a>(h: &'a Harness<'static, Workspace>, name: &'a str) -> egui_kittest::Node<'a> {
    find_in_panel(h, name).unwrap_or_else(|| panic!("データパネルに「{name}」が無い"))
}

/// 人の手順: 設計の画面で上の帯の左端の「データパネル」を押す → 左に場所の一覧 → プロジェクト「棚」
/// を押す → フォルダとデザインの行 → 「天板」を押すと広がり、版と「現在のデザインに挿入」→ 押すと
/// リンクで挿入される → 「台座」を 3D のビューへ引いて離すと、それも挿入される → 閉じると消える。
#[test]
fn the_data_panel_lists_designs_and_inserts_them_by_button_and_by_dragging() {
    let d = temp_dir("panel");
    let (proj, data) = project_fixture(&d);
    let ws = Workspace::new(kmb_samples::reference_document(), None, kmb_bundle::modules())
        .with_home(Library::new())
        .with_data_dir(data);
    let mut h = home(ws, [1400.0, 820.0]);
    let bodies = h.state().body_roots().len();
    assert!(h.query_by_label("すべての場所").is_none(), "初めは隠す");
    h.get_by_label("データパネル").click();
    h.run_ok();
    assert!(h.state().layout().data_open);
    in_panel(&h, "すべての場所");
    in_panel(&h, "最近使ったデザイン");
    in_panel(&h, "棚").click();
    h.run_ok();
    wait_in_panel(&mut h, "台座");
    in_panel(&h, "部品");
    in_panel(&h, "天板").click();
    h.run_ok();
    settle(&mut h, "データパネルの「天板」の版", |h| {
        h.query_all_by_label_contains("v2").next().is_some()
    });
    in_panel(&h, "現在のデザインに挿入").click();
    for _ in 0..5 {
        h.run_ok();
    }
    assert!(
        h.state().body_roots().len() > bodies,
        "ボタンでリンクで挿入する"
    );
    let after_button = h.state().body_roots().len();
    // 台座を 3D のビューの真ん中へ引いて離す
    let from = in_panel(&h, "台座").rect().center();
    let to = egui::pos2(800.0, 400.0);
    h.drag_at(from);
    h.run_ok();
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run_ok();
    }
    assert!(
        h.query_all_by_label_contains("離すと「台座」をリンクで挿入します")
            .next()
            .is_none(),
        "案内はビューに描くだけ(部品ではない)"
    );
    h.drop_at(to);
    for _ in 0..5 {
        h.run_ok();
    }
    assert!(
        h.state().body_roots().len() > after_button,
        "ビューへ離してもリンクで挿入する"
    );
    assert!(proj.join("台座.json").is_file(), "ビューへ離しても移さない");
    // 閉じる
    h.get_by_label("データパネルを閉じる").click();
    h.run_ok();
    assert!(!h.state().layout().data_open);
    assert!(h.query_by_label("すべての場所").is_none());
    let _ = std::fs::remove_dir_all(&d);
}

/// 開いているデザインそのものは挿入できない(ボタンを押せない。ビューへ離しても輪の拒否で止まる)。
#[test]
fn the_open_design_itself_cannot_be_inserted_from_the_data_panel() {
    let d = temp_dir("panel-own");
    let (proj, data) = project_fixture(&d);
    let own = proj.join("天板.json");
    let ws = Workspace::new(
        kmb_session::load(&own).unwrap(),
        Some(own),
        kmb_bundle::modules(),
    )
    .with_home(Library::new())
    .with_data_dir(data);
    let mut h = home(ws, [1400.0, 820.0]);
    h.get_by_label("データパネル").click();
    h.run_ok();
    in_panel(&h, "棚").click();
    h.run_ok();
    wait_in_panel(&mut h, "台座");
    in_panel(&h, "天板").click();
    h.run_ok();
    assert!(
        in_panel(&h, "現在のデザインに挿入")
            .accesskit_node()
            .is_disabled()
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// 整理の場: 棚の場に、天板の写し(部品/天板 (1))・名前の似た脚_v2 を足し、台座を部品へ移して
/// 最近の一覧に元の場所のまま残す(見つからない・移した先が分かる)。
fn tidy_fixture(d: &Path) -> (PathBuf, PathBuf) {
    let (proj, data) = project_fixture(d);
    std::fs::copy(
        proj.join("天板.json"),
        proj.join("部品").join("天板 (1).json"),
    )
    .unwrap();
    save_design(
        &proj.join("部品").join("脚_v2.json"),
        kmb_samples::sample_document(),
        &[("genre", "椅子")],
    );
    let mut recent = kmb_session::Recent::load(&data);
    recent.push(&proj.join("台座.json"));
    recent.save(&data).unwrap();
    kmb_session::organize::move_design(&proj.join("台座.json"), &proj.join("部品"), None).unwrap();
    (proj, data)
}

#[test]
fn tidy_pages_show_similar_groups_unclassified_and_missing_designs() {
    let d = temp_dir("tidy");
    let (proj, data) = tidy_fixture(&d);
    let mut h = project_home(data.clone());
    // 同じ・似ているデザイン: 3 種類の組(候補だけ)
    tree_row(&h, "同じ・似ているデザイン").click();
    h.run_ok();
    settle(&mut h, "「形が近い(1 組)」", |h| {
        h.query_by_label("形が近い(1 組)").is_some()
    });
    h.get_by_label("中身が同じ(1 組)");
    h.get_by_label("名前が似ている(1 組)");
    // 組を押すと右に組の違いと「版の多い方を残して、ほかを削除…」(押すと確かめの帯)
    h.get_by_label("中身が同じの組 1").click();
    h.run_ok();
    h.get_by_label("この組の違い");
    h.get_by_label("版の多い方を残して、ほかを削除…").click();
    h.run_ok();
    h.get_by_label_contains("次の 1 件を PC のごみ箱へ移します");
    h.get_by_label("キャンセル").click();
    h.run_ok();
    assert!(
        proj.join("部品").join("天板 (1).json").is_file(),
        "押すまで消さない"
    );
    // 分類のないデザイン: 台座だけ(移した先で)
    tree_row(&h, "分類のないデザイン").click();
    h.run_ok();
    wait_card(&mut h, "台座");
    card_of(&h, "台座");
    assert!(
        h.query_all_by_label("天板")
            .all(|n| n.rect().left() < 240.0 || n.rect().right() > 1100.0),
        "分類のある物は出ない"
    );
    // 一覧(表)に切り替える
    h.get_by_label("一覧で表示").click();
    h.run_ok();
    h.get_by_label("更新日");
    card_of(&h, "台座");
    // 見つからないファイル: 最近の一覧の台座。移した記録から分かる先へ付け替える
    tree_row(&h, "見つからないファイル").click();
    h.run_ok();
    h.get_by_label_contains("1 件は移した記録から今の場所が分かります");
    h.get_by_label("まとめて付け替える").click();
    h.run_ok();
    let moved = std::path::absolute(proj.join("部品").join("台座.json")).unwrap();
    assert!(kmb_session::Recent::load(&data).files.contains(&moved));
    h.get_by_label("見つからないファイルはありません。");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn filters_are_saved_and_folders_get_new_designs_and_are_renamed() {
    let d = temp_dir("filters");
    let (proj, data) = project_fixture(&d);
    let mut h = project_home(data.clone());
    h.get_by_label("棚").click();
    h.run_ok();
    wait_project(&mut h);
    // 探す語で絞り、名前を付けて保存すると左の「保存した絞り込み」に並び、そこを開く
    h.get_by_label("名前・作品・タグで探す").click();
    h.get_by_label("名前・作品・タグで探す").type_text("天");
    h.run_ok();
    h.get_by_label("絞り込みを保存…").click();
    h.run_ok();
    h.get_by_label("絞り込みの名前").type_text("天の物");
    h.key_press(egui::Key::Enter);
    h.run_ok();
    let saved = kmb_session::filters::Saved::load(&data);
    assert_eq!(saved.filters.len(), 1);
    assert_eq!(saved.filters[0].query, "天");
    tree_row(&h, "天の物");
    wait_card(&mut h, "天板");
    card_of(&h, "天板");
    assert!(
        h.query_all_by_label("台座")
            .all(|n| n.rect().left() < 240.0 || n.rect().right() > 1100.0)
    );
    // フォルダを選ぶと右に「ここに新規デザイン」。押すとそのフォルダに作って開く
    tree_row(&h, "棚").click();
    h.run_ok();
    wait_project(&mut h);
    card_of(&h, "部品").click();
    h.run_ok();
    h.get_by_label("ここに新規デザイン").click();
    h.run_ok();
    let new = proj
        .join("部品")
        .join(format!("新規デザイン.{}", kmb_core::DOCUMENT_EXTENSION));
    assert!(new.is_file());
    assert!(!h.state().is_start_screen());
    assert_eq!(h.state().path(), Some(new.as_path()));
    // フォルダの名前を変える(右の「…」→ 名前を変える…)。開いているタブのパスも付け替わる
    h.state_mut().show_home();
    h.run_ok();
    wait_project(&mut h);
    card_of(&h, "部品").click();
    h.run_ok();
    h.get_by_label("フォルダの操作").click();
    h.run_ok();
    h.get_by_label("名前を変える…").click();
    h.run_ok();
    h.get_by_label("新しい名前").type_text("2");
    h.key_press(egui::Key::Enter);
    h.run_ok();
    let renamed = proj.join("部品2");
    assert!(
        renamed
            .join(format!("新規デザイン.{}", kmb_core::DOCUMENT_EXTENSION))
            .is_file()
            && !proj.join("部品").exists()
    );
    assert_eq!(
        h.state().path(),
        Some(
            renamed
                .join(format!("新規デザイン.{}", kmb_core::DOCUMENT_EXTENSION))
                .as_path()
        ),
        "開いているタブのパスも付け替わる"
    );
    // 元に戻す
    undo_band(&h).click();
    h.run_ok();
    assert!(
        proj.join("部品")
            .join(format!("新規デザイン.{}", kmb_core::DOCUMENT_EXTENSION))
            .is_file()
    );
    let _ = std::fs::remove_dir_all(&d);
}
