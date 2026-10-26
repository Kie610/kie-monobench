use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::FeatureId;
use kmb_samples::reference_cam_document;
use kmb_ui::Workspace;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kmb-ui-tools-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn first_of(ws: &Workspace, type_id: &str) -> FeatureId {
    ws.document()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == type_id)
        .unwrap()
        .id
}

fn ws_with(dir: Option<PathBuf>) -> Workspace {
    let ws = Workspace::new(reference_cam_document(), None, kmb_bundle::modules())
        .with_editor(ToolsEditor::new());
    match dir {
        Some(d) => ws.with_data_dir(d),
        None => ws,
    }
}

fn lib_of(ws: &Workspace, id: FeatureId) -> ToolLibrary {
    library_of(ws.document().data().feature(id).unwrap()).unwrap()
}

#[test]
fn tools_are_synced_with_the_pc_library_only_when_asked() {
    let d = temp_dir("tools");
    let mut ws = ws_with(Some(d.clone()));
    assert_eq!(
        ws.editor::<ToolsEditor>().unwrap().global(),
        &GlobalTools::Missing
    );
    let tools = first_of(&ws, TOOLS);
    let lib = lib_of(&ws, tools);
    ws.editor_mut::<ToolsEditor>().unwrap().all_to_global(&lib);
    let e = ws.editor::<ToolsEditor>().unwrap();
    assert!(matches!(e.global(), GlobalTools::Loaded(_)));
    assert!(e.differing(&lib).is_empty());

    // 文書の工具を変えると印が付く。PC のライブラリは変わらない
    let mut lib = lib_of(&ws, tools);
    lib.tools[0].presets[0].cutting.feed = 777.0;
    ws.set_feature_data(tools, serde_json::to_value(&lib).unwrap());
    let lib = lib_of(&ws, tools);
    assert_eq!(ws.editor::<ToolsEditor>().unwrap().differing(&lib), vec![1]);
    // 書き戻すと揃い、前のファイルは .bak に残る
    ws.editor_mut::<ToolsEditor>().unwrap().to_global(&lib, 1);
    assert!(
        ws.editor::<ToolsEditor>()
            .unwrap()
            .differing(&lib)
            .is_empty()
    );
    assert!(d.join("tools.json.bak").exists());
    let text = std::fs::read_to_string(d.join("tools.json")).unwrap();
    assert!(text.contains("777"));

    // 文書を戻して、ライブラリに揃える
    ws.undo();
    let lib = lib_of(&ws, tools);
    assert_eq!(ws.editor::<ToolsEditor>().unwrap().differing(&lib), vec![1]);
    let synced = ws
        .editor::<ToolsEditor>()
        .unwrap()
        .from_global(&lib, 1)
        .unwrap();
    assert_eq!(synced.tools[0].cutting().feed, 777.0);

    // 読めないライブラリは書き戻さない(利用者のデータを消さない)
    std::fs::write(d.join("tools.json"), "{ 壊れた").unwrap();
    let mut ws = ws_with(Some(d.clone()));
    assert!(matches!(
        ws.editor::<ToolsEditor>().unwrap().global(),
        GlobalTools::Broken(_)
    ));
    let lib = lib_of(&ws, tools);
    ws.editor_mut::<ToolsEditor>().unwrap().to_global(&lib, 1);
    assert_eq!(
        std::fs::read_to_string(d.join("tools.json")).unwrap(),
        "{ 壊れた"
    );
    std::fs::remove_dir_all(&d).ok();
}

/// 決まったファイルを選ぶダイアログ。
struct PickFile(PathBuf);

impl kmb_ui::Dialogs for PickFile {
    fn open_document(&self) -> Option<PathBuf> {
        None
    }
    fn save_document(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn open_file(&self, _: &str, _: &str, _: &[&str]) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn save_file(&self, _: &str, _: &str, _: &[&str], _: &str) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn unsaved(&self, _: &str) -> kmb_ui::Unsaved {
        kmb_ui::Unsaved::Cancel
    }
}

fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 1000.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws)
}

/// 工具ライブラリ `tools` を指す加工の (ID, 工具番号)。
fn ops_using(ws: &Workspace, tools: FeatureId) -> Vec<(FeatureId, u64)> {
    ws.document()
        .data()
        .features
        .iter()
        .filter(|f| f.data.get("tools").and_then(|v| v.as_u64()) == Some(tools.0))
        .filter_map(|f| Some((f.id, f.data.get("tool")?.as_u64()?)))
        .collect()
}

#[test]
fn the_carvera_tools_are_added_with_one_click_and_kept_once() {
    let mut h = harness(ws_with(None));
    let tools = first_of(h.state(), TOOLS);
    h.state_mut().open_edit(tools);
    h.run();
    h.get_by_label("Carvera Air の工具を足す").click();
    h.run();
    let lib = lib_of(h.state(), tools);
    assert_eq!(lib.tools.len(), 15);
    assert_eq!(lib.get(1).unwrap().name, "平 6 mm");
    assert!(lib.tools.iter().any(|t| t.name == "平 3.175 mm 刃長 25"));
    assert!(h.state().message().unwrap().contains("10 本を足しました"));
    // 1 回の取り消しで戻る
    h.state_mut().undo();
    assert_eq!(lib_of(h.state(), tools).tools.len(), 5);
    h.state_mut().redo();
    h.run();
    h.get_by_label("Carvera Air の工具を足す").click();
    h.run();
    assert_eq!(lib_of(h.state(), tools).tools.len(), 15);
    assert!(h.state().message().unwrap().contains("もう全部あります"));
}

#[test]
fn the_tool_library_form_shows_its_fields_and_a_bull_nose_gets_a_corner_radius() {
    // 工具ライブラリは一覧と欄で見せ、工具を足せる
    // T1 の欄には全長と首下長があり、種類をブルノーズにすると角 R の欄が出る
    let mut h = harness(ws_with(None));
    let tools = first_of(h.state(), TOOLS);
    h.state_mut().open_edit(tools);
    h.run();
    h.get_by_label("工具(5 本)");
    h.get_by_label("T2 平 3.175 mm(径 3.175)").click();
    h.run();
    h.get_by_label("材料ごとの切削条件(一番上が既定)");
    // Z13: 全長の欄(データにはあったが欄が無かった)
    h.get_by_label("全長");
    // 木材の 600 mm/min、12000 rpm、2 枚刃のチップロード
    h.get_by_label("0.0250 mm");
    h.get_by_label("T1 平 6 mm(径 6)").click();
    h.run();
    h.get_by_label("全長");
    h.get_by_label("首下長");
    // 種類の一覧からブルノーズを選ぶと、角 R の欄が出て、既定の値が入る
    h.get_by_value("フラットエンドミル").click();
    h.run();
    h.get_by_label("ブルノーズ").click();
    h.run();
    h.get_by_label("角 R");
    let lib = lib_of(h.state(), tools);
    let t = lib.get(1).unwrap();
    assert_eq!(t.kind, ToolKind::BullNose);
    assert_eq!(t.corner_radius, Some(0.6));
    t.check().unwrap();
    h.get_by_label("工具を足す").click();
    h.run();
    let lib = lib_of(h.state(), tools);
    assert_eq!(lib.tools.len(), 6);
    assert_eq!(lib.tools[5].number, 6);
}

#[test]
fn libraries_are_exported_and_imported_through_the_file_dialogs() {
    let d = temp_dir("exchange");
    let file = d.join("工具.tools");
    // 書き出す
    let ws = ws_with(None).with_dialogs(Box::new(PickFile(file.clone())));
    let mut h = harness(ws);
    let tools = first_of(h.state(), TOOLS);
    h.state_mut().open_edit(tools);
    h.run();
    h.get_by_label("書き出す…").click();
    h.run();
    assert!(file.exists());
    assert!(
        h.state().message().unwrap().contains("5 本"),
        "{:?}",
        h.state().message()
    );
    // 同じ文書に読み込むと、同じ工具なので増えない
    h.get_by_label("読み込む…").click();
    h.run();
    assert_eq!(lib_of(h.state(), tools).tools.len(), 5);
    assert!(h.state().message().unwrap().contains("同じ工具 5 本"));
    // 番号の重なる違う工具は、空いている番号で足す
    let mut other = ToolLibrary::starter();
    other.tools[0].name = "よその平".into();
    other.tools.truncate(1);
    std::fs::write(
        &file,
        kmb_cam_tool::exchange::write_library(&other, "tools"),
    )
    .unwrap();
    h.get_by_label("読み込む…").click();
    h.run();
    let lib = lib_of(h.state(), tools);
    assert_eq!(lib.tools.len(), 6);
    assert_eq!(lib.get(6).unwrap().name, "よその平");
    assert!(h.state().message().unwrap().contains("T6"));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn renumbering_also_renumbers_the_operations() {
    let mut h = harness(ws_with(None));
    let tools = first_of(h.state(), TOOLS);
    let before = ops_using(h.state(), tools);
    assert!(!before.is_empty());
    h.state_mut().open_edit(tools);
    h.run();
    // 開始 10・増分 10(欄はドラッグの数の欄なので、状態を直に入れて押す)
    let sid = kmb_ui::doc_id(&h.ctx, ("tools_panel", tools));
    h.ctx.data_mut(|d| {
        let mut st: PanelState = d.get_temp(sid).unwrap_or_default();
        st.renumber = (10, 10);
        d.insert_temp(sid, st);
    });
    h.get_by_label("振り直す").click();
    h.run();
    let lib = lib_of(h.state(), tools);
    let numbers: Vec<u32> = lib.tools.iter().map(|t| t.number).collect();
    assert_eq!(numbers, [10, 20, 30, 40, 50]);
    let after = ops_using(h.state(), tools);
    for ((id, old), (id2, new)) in before.iter().zip(&after) {
        assert_eq!(id, id2);
        assert_eq!(*new, *old * 10);
    }
    // 加工は直した番号で計算できる(1 回の取り消し単位)
    assert!(h.state().message().unwrap().contains("振り直しました"));
    h.state_mut().undo();
    assert_eq!(ops_using(h.state(), tools), before);
}

#[test]
fn an_operation_can_use_a_tool_from_the_pc_library() {
    let d = temp_dir("pick");
    let mut pc = ToolLibrary::starter();
    pc.tools.truncate(1);
    pc.tools[0].name = "PC の平 4 mm".into();
    pc.tools[0].diameter = 4.0;
    std::fs::write(d.join("tools.json"), serde_json::to_string(&pc).unwrap()).unwrap();
    let mut h = harness(ws_with(Some(d.clone())));
    let tools = first_of(h.state(), TOOLS);
    let (op, _) = ops_using(h.state(), tools)[0];
    h.state_mut().open_edit(op);
    h.run();
    // 加工の欄が長いと、工具の欄の下の行は見える所の外にある
    h.get_by_label("PC のライブラリの工具").scroll_to_me();
    h.run();
    h.get_by_value("選ぶ…").click();
    h.run();
    h.get_by_label("T1 PC の平 4 mm").click();
    h.run();
    let lib = lib_of(h.state(), tools);
    assert_eq!(lib.tools.len(), 6);
    assert_eq!(lib.get(6).unwrap().name, "PC の平 4 mm");
    assert_eq!(ops_using(h.state(), tools)[0], (op, 6));
    assert!(h.state().message().unwrap().contains("T6"));
    // 写した後は、もう候補に出ない
    h.run();
    assert!(h.query_by_label("PC のライブラリの工具").is_none());
    std::fs::remove_dir_all(&d).ok();
}

/// 文書の長さの単位がインチなら、工具の欄の長さもインチで見せて打たせる(保存は mm のまま)。
/// 一覧の径・寸法の欄・切削条件の見出し・チップロードがインチになり、開いただけでは値が変わらない。
/// 刃の径の欄に 0.25 と打つと 6.35 mm が入る。
#[test]
fn the_tool_form_shows_and_takes_lengths_in_the_document_unit() {
    use egui::accesskit::Role;
    let mut ws = ws_with(None);
    let mut p = ws.document().data().properties.clone();
    p.units = kmb_core::LengthUnit::In;
    assert!(ws.set_properties(p));
    let mut h = harness(ws);
    let tools = first_of(h.state(), TOOLS);
    h.state_mut().open_edit(tools);
    h.run();
    let before = lib_of(h.state(), tools);
    h.get_by_label("T2 平 3.175 mm(径 0.125 in)").click();
    h.run();
    h.get_by_label("切削送り(in/min)");
    h.get_by_label("切込み(in)");
    // 木材の 600 mm/min、12000 rpm、2 枚刃のチップロード 0.025 mm
    h.get_by_label("0.00098 in");
    assert_eq!(
        lib_of(h.state(), tools),
        before,
        "見せただけで値が変わらない"
    );
    // 刃の径の欄(ラベルの右の数の欄)をクリックして 0.25 と打ち直す
    let rect = {
        let label = h.get_by_label("刃の径").rect();
        h.query_all_by_role(Role::SpinButton)
            .map(|n| n.rect())
            .filter(|r| (r.center().y - label.center().y).abs() < 4.0 && r.left() > label.right())
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .expect("刃の径の欄")
    };
    let c = rect.center();
    h.hover_at(c);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: c,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("0.25".into()));
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
    let d = lib_of(h.state(), tools).get(2).unwrap().diameter;
    assert!((d - 6.35).abs() < 1e-9, "{d}");
}
