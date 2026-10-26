//! 加工の使い回し(複製・コピー・貼り付け・ドラッグ・テンプレート・既定)の試験。

use crate::reuse_ui::DropAt;
use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::reference_cam_document;

fn reg() -> kmb_core::Modules {
    kmb_bundle::modules()
}

/// 名前の加工かセットアップ(設計の機能にも同じ名前「ポケット」があるので、加工の物から探す)。
fn id_of(ws: &Workspace, name: &str) -> FeatureId {
    ws.document()
        .data()
        .features
        .iter()
        .find(|f| f.name == name && f.type_id.starts_with("cam."))
        .unwrap_or_else(|| panic!("{name} がない"))
        .id
}

/// タイムラインの順の名前。
fn names(ws: &Workspace) -> Vec<String> {
    ws.document()
        .data()
        .features
        .iter()
        .map(|f| f.name.clone())
        .collect()
}

fn setup_of(ws: &Workspace, id: FeatureId) -> Option<u64> {
    ws.document()
        .data()
        .feature(id)?
        .data
        .get("setup")?
        .as_u64()
}

/// 基準の加工の文書に、2 つ目のセットアップ(加工なし)を足したもの。
fn two_setups() -> (Workspace, FeatureId, FeatureId) {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    ws.layout_mut().space = Space::Machining;
    let s1 = id_of(&ws, "セットアップ 1");
    let s2 = ws
        .run_action(kmb_cam_commands::ADD_SETUP)
        .expect("セットアップを足す");
    ws.commit_creation();
    (ws, s1, s2)
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("kmb-ui-reuse-{tag}-{}-{t}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn an_operation_is_duplicated_right_after_itself() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    let pocket = id_of(&ws, "ポケット");
    let before = names(&ws);
    let made = ws.duplicate_features(&[pocket]);
    assert_eq!(made.len(), 1);
    let after = names(&ws);
    let i = ws.document().data().index_of(pocket).unwrap();
    // 写しは元のすぐ後ろ(加工の順も元の次)で、名前が重ならない
    assert_eq!(after[i + 1], "ポケット (2)");
    assert_eq!(ws.selection(), made);
    let (a, b) = (
        ws.document().data().feature(pocket).unwrap().data.clone(),
        ws.document().data().feature(made[0]).unwrap().data.clone(),
    );
    assert_eq!(a, b, "同じ値");
    assert_eq!(ws.report().error_count(), 0);
    // 写しの写しは「(3)」
    ws.duplicate_features(&made);
    assert!(names(&ws).contains(&"ポケット (3)".to_string()));
    // 1 回ずつ取り消せる
    ws.undo();
    ws.undo();
    assert_eq!(names(&ws), before);
}

#[test]
fn a_setup_is_duplicated_with_its_operations() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    let s1 = id_of(&ws, "セットアップ 1");
    let ops = ws.operations_in(&[s1]);
    let made = ws.duplicate_features(&[s1]);
    assert_eq!(made.len(), ops.len() + 1);
    let s2 = made[0];
    assert_eq!(
        ws.document().data().feature(s2).unwrap().name,
        "セットアップ 1 (2)"
    );
    for id in &made[1..] {
        assert_eq!(setup_of(&ws, *id), Some(s2.0));
    }
    // 元のセットアップの加工は変わらない
    for id in &ops {
        assert_eq!(setup_of(&ws, *id), Some(s1.0));
    }
    assert_eq!(ws.report().error_count(), 0);
}

#[test]
fn copied_operations_are_pasted_into_another_setup() {
    let (mut ws, s1, s2) = two_setups();
    let face = id_of(&ws, "上面のフェイス");
    let outline = id_of(&ws, "外形");
    assert!(!ws.has_op_clip());
    assert!(ws.paste_operations(Some(s2)).is_empty());
    assert_eq!(ws.copy_operations(None, &[face, outline]), 2);
    // セットアップに貼ると、その加工の最後に、そのセットアップの加工として足す
    let made = ws.paste_operations(Some(s2));
    assert_eq!(made.len(), 2);
    for id in &made {
        assert_eq!(setup_of(&ws, *id), Some(s2.0));
    }
    let n = names(&ws);
    assert_eq!(&n[n.len() - 2..], ["上面のフェイス (2)", "外形 (2)"]);
    // 加工に貼ると、その後ろに同じセットアップの加工として足す
    let again = ws.paste_operations(Some(face));
    let n = names(&ws);
    let i = n.iter().position(|x| x == "上面のフェイス").unwrap();
    assert_eq!(n[i + 1], "上面のフェイス (3)");
    assert_eq!(setup_of(&ws, again[0]), Some(s1.0));
    // セットアップを写すと、中の加工すべてを写す
    let all = ws.operations_in(&[s1]).len();
    assert_eq!(ws.copy_operations(None, &[s1]), all);
    // 切り取りは写してから消す
    let pocket = id_of(&ws, "ポケット");
    ws.cut_operations(None, &[pocket]);
    assert!(ws.document().data().feature(pocket).is_none());
    let moved = ws.paste_operations(Some(s2));
    assert_eq!(setup_of(&ws, moved[0]), Some(s2.0));
    // 名前は設計の機能(「ポケット」)とも重ならないようにする
    assert_eq!(
        ws.document().data().feature(moved[0]).unwrap().name,
        "ポケット (2)"
    );
}

#[test]
fn operations_are_dragged_to_another_place_or_setup() {
    let (mut ws, s1, s2) = two_setups();
    let face = id_of(&ws, "上面のフェイス");
    let pocket = id_of(&ws, "ポケット");
    let outline = id_of(&ws, "外形");
    let order = |ws: &Workspace| ws.operations_in(&[s1]);
    // フェイスをポケットの後ろへ
    assert!(ws.drop_operation(face, DropAt::After(pocket)));
    let o = order(&ws);
    let p = o.iter().position(|x| *x == pocket).unwrap();
    assert_eq!(o[p + 1], face);
    // 外形を先頭(フェイスより前…の、いちばん前の加工の前)へ
    let first = order(&ws)[0];
    assert!(ws.drop_operation(outline, DropAt::Before(first)));
    assert_eq!(order(&ws)[0], outline);
    // 同じ所へ落としても変えない
    assert!(!ws.drop_operation(outline, DropAt::Before(first)));
    // 別のセットアップへ落とすと、そのセットアップの加工になる(1 回で取り消せる)
    assert!(ws.drop_operation(face, DropAt::Into(s2)));
    assert_eq!(setup_of(&ws, face), Some(s2.0));
    assert_eq!(ws.operations_in(&[s2]), vec![face]);
    ws.undo();
    assert_eq!(setup_of(&ws, face), Some(s1.0));
}

#[test]
fn keys_duplicate_copy_paste_and_rename_in_machining() {
    let (mut ws, _, s2) = two_setups();
    let face = id_of(&ws, "上面のフェイス");
    ws.select(face);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    // Ctrl+D で複製
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    h.run();
    assert!(names(h.state()).contains(&"上面のフェイス (2)".to_string()));
    // Ctrl+C で写し、2 つ目のセットアップを選んで Ctrl+V
    h.state_mut().select(face);
    h.event(egui::Event::Copy);
    h.run();
    assert!(h.state().has_op_clip());
    let text = h.state().cur.op_clip.as_ref().unwrap().text.clone();
    h.state_mut().select(s2);
    // ほかの文がクリップボードにある時の Ctrl+V では貼らない
    h.event(egui::Event::Paste("別の文".into()));
    h.run();
    assert!(h.state().operations_in(&[s2]).is_empty());
    h.event(egui::Event::Paste(text));
    h.run();
    let pasted = h.state().operations_in(&[s2]);
    assert_eq!(pasted.len(), 1);
    // F2 で名前を変える窓
    h.key_press(egui::Key::F2);
    h.run();
    assert!(h.state().cur.renaming.is_some());
    h.get_by_label("名前変更");
}

#[test]
fn the_right_click_menu_of_an_operation_has_the_reuse_items() {
    let data = temp_dir("menu");
    let mut ws = Workspace::new(reference_cam_document(), None, reg()).with_data_dir(data.clone());
    ws.layout_mut().space = Space::Machining;
    let boss = id_of(&ws, "ボスの周り");
    ws.select(boss);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("ボスの周り").click_secondary();
    h.run();
    for item in [
        "名前変更",
        "複製",
        "切り取り",
        "コピー",
        "貼り付け",
        "テンプレートとして保存…",
        "パラメータを既定として保存",
    ] {
        h.get_by_label(item);
    }
    // 一覧を開く項目(右に矢印が付く)
    h.get_by_label_contains("テンプレートから作成");
    // テンプレートとして保存は名前を聞き、OK でデータフォルダに書く
    h.get_by_label("テンプレートとして保存…").click();
    h.run();
    assert!(h.state().cur.name_ask.is_some());
    h.state_mut().cur.name_ask.as_mut().unwrap().text = "荒取り".into();
    h.get_by_label("OK").click();
    h.run();
    assert!(data.join("templates").join("荒取り.json").exists());
    assert!(h.state().message().is_some_and(|m| m.contains("荒取り")));
    std::fs::remove_dir_all(&data).ok();
}

#[test]
fn a_template_is_made_in_the_chosen_setup_and_defaults_are_kept_in_settings() {
    let data = temp_dir("template");
    let (ws, s1, s2) = two_setups();
    let mut ws = ws.with_data_dir(data.clone());
    // セットアップ 1 の加工を、名前を決めてテンプレートにする
    ws.select(s1);
    ws.run_action(kmb_cam_commands::SAVE_TEMPLATE);
    let ask = ws.cur.name_ask.clone().expect("名前を聞く");
    assert_eq!(ask.text, "セットアップ 1の加工");
    ws.cur.name_ask = None;
    ws.run_named(&reuse_ui::NameAsk {
        text: "表の加工".into(),
        ..ask
    });
    let n = ws.operations_in(&[s1]).len();
    // セットアップ 2 で使う。ポケットの底の面は、先に 3D のビューで選ぶ
    ws.select(s2);
    let use_id = format!("{}表の加工", kmb_cam_commands::USE_TEMPLATE);
    ws.run_action(&use_id);
    assert!(
        ws.message().is_some_and(|m| m.contains("面")),
        "{:?}",
        ws.message()
    );
    // 面の要らない加工だけのテンプレートなら、そのまま足す
    let face = id_of(&ws, "上面のフェイス");
    let outline = id_of(&ws, "外形");
    ws.select(face);
    ws.run_named(&reuse_ui::NameAsk {
        action: kmb_cam_commands::SAVE_TEMPLATE.into(),
        title: String::new(),
        label: String::new(),
        text: "外回り".into(),
        selection: vec![face, outline],
    });
    ws.select(s2);
    ws.run_action(&format!("{}外回り", kmb_cam_commands::USE_TEMPLATE));
    ws.commit_creation();
    assert_eq!(ws.operations_in(&[s2]).len(), 2, "{:?}", ws.message());
    assert!(n > 2);
    // パラメータを既定として保存すると、設定のファイルに書き、次に足すフェイスがその値になる
    let new_face = ws.operations_in(&[s2])[0];
    let mut d = ws.document().data().feature(new_face).unwrap().data.clone();
    d["leave"] = "0.3 mm".into();
    ws.set_feature_data(new_face, d);
    ws.select(new_face);
    ws.run_action(kmb_cam_commands::SAVE_DEFAULTS);
    let settings = std::fs::read_to_string(data.join("settings.json")).unwrap();
    assert!(settings.contains("cam.op_defaults"), "{settings}");
    let added = ws.run_action("cam.add.face").unwrap();
    assert_eq!(
        ws.document().data().feature(added).unwrap().data["leave"],
        "0.3 mm"
    );
    std::fs::remove_dir_all(&data).ok();
}
