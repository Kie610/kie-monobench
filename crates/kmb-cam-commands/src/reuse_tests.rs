//! 加工の既定とテンプレートの試験。

use super::*;
use crate::{CamCommands, Setup};
use kmb_core::{
    DocumentData, Extension, FeatureEntry, Kernel, NullKernel, RecomputeReport, Registry,
};

struct Doc {
    data: DocumentData,
    reg: Registry,
    report: RecomputeReport,
    kernel: Arc<dyn Kernel>,
}

fn setup_data() -> Json {
    serde_json::to_value(Setup::new(FeatureId(1))).unwrap()
}

/// 工具ライブラリ(2)、セットアップ 1(3)と 2(4)、セットアップ 1 のフェイス(5)と輪郭(6)。
fn doc() -> Doc {
    let reg = crate::tests::registry();
    let entry = |id: u64, type_id: &str, name: &str, data: Json| FeatureEntry {
        id: FeatureId(id),
        type_id: type_id.into(),
        name: name.into(),
        data,
        suppressed: false,
        pin_bodies: false,
    };
    let face = json!({"setup": 3, "tools": 2, "tool": 1, "leave": "0.2 mm", "rpm": "9000"});
    let contour = json!({
        "setup": 3, "tools": 2, "tool": 3, "face": "f1:end", "side": "outside",
        "extra_depth": "0.5 mm", "leave": "0 mm", "tabs": 2, "tab_width": "4 mm",
        "tab_height": "1.5 mm"
    });
    let data = DocumentData {
        next_id: 7,
        features: vec![
            entry(
                2,
                crate::TOOLS,
                "工具",
                serde_json::to_value(ToolLibrary::starter()).unwrap(),
            ),
            entry(3, SETUP, "セットアップ 1", setup_data()),
            entry(4, SETUP, "セットアップ 2", setup_data()),
            entry(5, kmb_strategy_face::FACE, "フェイス1", face),
            entry(6, kmb_strategy_contour2d::CONTOUR, "輪郭1", contour),
        ],
        ..Default::default()
    };
    Doc {
        data,
        reg,
        report: RecomputeReport::default(),
        kernel: Arc::new(NullKernel),
    }
}

/// 既定を持たない CAM の操作(初めの値と比べるため)。
fn cam_fresh() -> CamCommands {
    cam()
}

fn cam() -> CamCommands {
    crate::tests::cam()
}

fn run(
    cam: &CamCommands,
    d: &Doc,
    id: &str,
    selected: Option<u64>,
    args: Json,
) -> Result<Outcome, kmb_core::Problem> {
    let cx = Context {
        selected: selected.map(FeatureId),
        args: &args,
        ..Context::new(&d.data, &d.report, &d.reg, &d.kernel)
    };
    cam.run(id, &cx).expect("知っている操作")
}

/// 足す変更の (種類、名前、データ)。
fn inserts(o: Outcome) -> Vec<(String, String, Json)> {
    match o {
        Outcome::Change { edits, .. } => edits
            .into_iter()
            .filter_map(|e| match e {
                Edit::Insert {
                    type_id,
                    name,
                    data,
                } => Some((type_id, name, data)),
                _ => None,
            })
            .collect(),
        other => panic!("変更ではない: {other:?}"),
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("kmb-reuse-{tag}-{}-{t}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn saved_defaults_are_the_starting_values_of_the_next_operation() {
    let d = doc();
    let cam = cam();
    let fresh = inserts(run(&cam, &d, "cam.add.face", None, Json::Null).unwrap());
    assert_eq!(fresh[0].2["leave"], "0 mm");
    // フェイス1(残し量 0.2 mm、回転数 9000)を既定にすると、次のフェイスはその値で始まる
    run(&cam, &d, SAVE_DEFAULTS, Some(5), Json::Null).unwrap();
    let next = inserts(run(&cam, &d, "cam.add.face", None, Json::Null).unwrap());
    let data = &next[0].2;
    assert_eq!(data["leave"], "0.2 mm");
    assert_eq!(data["rpm"], "9000");
    // セットアップと工具は、足す時の選び方のまま(最後のセットアップ)
    assert_eq!(data["setup"], 4);
    assert_eq!(data["tools"], 2);
    // 設定に出て、初めの値(空)に戻せる
    let s = cam
        .settings()
        .into_iter()
        .find(|s| s.id == DEFAULTS_SETTINGS)
        .unwrap();
    assert!(s.value.get(kmb_strategy_face::FACE).is_some());
    assert!(s.notes[0].contains("フェイス"), "{:?}", s.notes);
    cam.set_settings(DEFAULTS_SETTINGS, &s.default)
        .unwrap()
        .unwrap();
    let back = inserts(run(&cam, &d, "cam.add.face", None, Json::Null).unwrap());
    assert_eq!(back[0].2["leave"], "0 mm");
}

#[test]
fn an_empty_choice_in_the_defaults_stays_empty() {
    let d = doc();
    let cam = cam();
    // 輪郭は初めは斜めに入る(3 deg)。斜めに入らない輪郭1 を既定にすると、次も入らない
    let fresh = inserts(run(&cam, &d, "cam.add.contour", None, Json::Null).unwrap());
    assert_eq!(fresh[0].2["ramp_angle"], "3 deg");
    run(&cam, &d, SAVE_DEFAULTS, Some(6), Json::Null).unwrap();
    let next = inserts(run(&cam, &d, "cam.add.contour", None, Json::Null).unwrap());
    let data = &next[0].2;
    assert!(data.get("ramp_angle").is_none_or(Json::is_null), "{data}");
    assert_eq!(data["tabs"], 2);
    assert_eq!(data["extra_depth"], "0.5 mm");
    // 面は既定にしない(選んでいない時の外形のまま)
    assert!(data.get("face").is_none_or(Json::is_null), "{data}");
    // この種類の既定だけ消せる
    run(&cam, &d, CLEAR_DEFAULTS, Some(6), Json::Null).unwrap();
    assert!(
        cam.reuse
            .defaults_of(kmb_strategy_contour2d::CONTOUR)
            .is_none()
    );
}

#[test]
fn defaults_that_no_longer_fit_fall_back_to_the_starting_values() {
    let d = doc();
    let cam = cam();
    // 種類の欄が変わって読めない既定(タブの数が文字)は、使わずに初めの値で足す
    cam.set_settings(
        DEFAULTS_SETTINGS,
        &json!({kmb_strategy_contour2d::CONTOUR: {"tabs": "たくさん"}}),
    )
    .unwrap()
    .unwrap();
    let next = inserts(run(&cam, &d, "cam.add.contour", None, Json::Null).unwrap());
    let fresh = inserts(run(&cam_fresh(), &d, "cam.add.contour", None, Json::Null).unwrap());
    assert_eq!(next[0].2, fresh[0].2);
    // 形の違う設定は受け付けない
    assert!(
        cam.set_settings(DEFAULTS_SETTINGS, &json!([1]))
            .unwrap()
            .is_err()
    );
}

#[test]
fn defaults_can_be_exported_and_imported() {
    let d = doc();
    let cam = cam();
    run(&cam, &d, SAVE_DEFAULTS, Some(5), Json::Null).unwrap();
    let Outcome::File { contents, .. } = run(&cam, &d, EXPORT_DEFAULTS, None, Json::Null).unwrap()
    else {
        panic!("ファイルではない");
    };
    let dir = temp_dir("defaults");
    let p = dir.join("既定.json");
    std::fs::write(&p, &contents).unwrap();
    run(&cam, &d, RESET_DEFAULTS, None, Json::Null).unwrap();
    assert!(cam.reuse.defaults_of(kmb_strategy_face::FACE).is_none());
    let cx = Context {
        file: Some(&p),
        ..Context::new(&d.data, &d.report, &d.reg, &d.kernel)
    };
    cam.run(IMPORT_DEFAULTS, &cx).unwrap().unwrap();
    assert_eq!(
        cam.reuse.defaults_of(kmb_strategy_face::FACE).unwrap()["leave"],
        "0.2 mm"
    );
    // 改名の前の名前で書いたファイルも読む
    let old = String::from_utf8(contents.clone())
        .unwrap()
        .replace("kie-monobench/", "jisaku-cadcam/");
    std::fs::write(&p, old).unwrap();
    run(&cam, &d, RESET_DEFAULTS, None, Json::Null).unwrap();
    let cx = Context {
        file: Some(&p),
        ..Context::new(&d.data, &d.report, &d.reg, &d.kernel)
    };
    cam.run(IMPORT_DEFAULTS, &cx).unwrap().unwrap();
    assert!(cam.reuse.defaults_of(kmb_strategy_face::FACE).is_some());
    // 別の JSON は読まない
    std::fs::write(&p, "{\"tools\": []}").unwrap();
    let cx = Context {
        file: Some(&p),
        ..Context::new(&d.data, &d.report, &d.reg, &d.kernel)
    };
    let e = cam.run(IMPORT_DEFAULTS, &cx).unwrap().unwrap_err();
    assert_eq!(e.kind, kmb_core::ProblemKind::Input, "{e}");
    // 読めないファイルはファイルの誤り(CLI の終了の番号 5)
    let missing = dir.join("無い.json");
    let cx = Context {
        file: Some(&missing),
        ..Context::new(&d.data, &d.report, &d.reg, &d.kernel)
    };
    let e = cam.run(IMPORT_DEFAULTS, &cx).unwrap().unwrap_err();
    assert_eq!(e.kind, kmb_core::ProblemKind::File, "{e}");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_template_of_a_setup_is_made_again_in_another_setup() {
    let d = doc();
    let cam = cam();
    let dir = temp_dir("template");
    cam.set_data_dir(Some(&dir));
    // データフォルダがなければ保存できない
    let none = crate::tests::cam();
    assert!(run(&none, &d, SAVE_TEMPLATE, Some(3), json!({"name": "x"})).is_err());
    // セットアップ 1 の加工(フェイスと輪郭)をテンプレートにする。名前の使えない字は _ にする
    let m = run(
        &cam,
        &d,
        SAVE_TEMPLATE,
        Some(3),
        json!({"name": "表/裏 の加工"}),
    )
    .unwrap();
    assert!(
        matches!(&m, Outcome::Message(t) if t.contains("2 個")),
        "{m:?}"
    );
    assert_eq!(cam.reuse.templates(), vec!["表_裏 の加工".to_string()]);
    let file = std::fs::read_to_string(dir.join(TEMPLATE_DIR).join("表_裏 の加工.json")).unwrap();
    // 文書の物(セットアップ・工具ライブラリ・面)はファイルに入れない
    for key in ["\"setup\"", "\"tools\"", "\"face\""] {
        assert!(!file.contains(key), "{key}: {file}");
    }
    // 操作の一覧に、テンプレートから作る操作が出る
    let cx = Context::new(&d.data, &d.report, &d.reg, &d.kernel);
    let use_id = format!("{USE_TEMPLATE}表_裏 の加工");
    assert!(cam.actions(&cx).iter().any(|a| a.id == use_id));
    // セットアップ 2 を選んで使うと、その下に同じ値の加工を足す(名前は重ならないようにする)
    let made = inserts(run(&cam, &d, &use_id, Some(4), Json::Null).unwrap());
    assert_eq!(made.len(), 2);
    assert_eq!(made[0].0, kmb_strategy_face::FACE);
    assert_eq!(made[0].1, "フェイス1 (2)");
    assert_eq!(made[1].1, "輪郭1 (2)");
    for (_, _, data) in &made {
        assert_eq!(data["setup"], 4);
        assert_eq!(data["tools"], 2);
    }
    assert_eq!(made[0].2["leave"], "0.2 mm");
    assert_eq!(made[1].2["tabs"], 2);
    // 面は前の文書の面を指さない(選んでいない時の外形)
    assert!(made[1].2.get("face").is_none_or(Json::is_null));
    // 消すと一覧から消える。フォルダの更新時刻が変わらなくても(Windows では消した直後に
    // 前と同じ時刻のままのことがある)覚えた一覧を返さない
    let tdir = dir.join(TEMPLATE_DIR);
    let before = std::fs::metadata(&tdir).unwrap().modified().unwrap();
    run(
        &cam,
        &d,
        &format!("{DELETE_TEMPLATE}表_裏 の加工"),
        None,
        Json::Null,
    )
    .unwrap();
    // Linux ではフォルダの時刻を消す前に戻して、その様子を作る
    #[cfg(unix)]
    std::fs::File::open(&tdir)
        .unwrap()
        .set_modified(before)
        .unwrap();
    let _ = before;
    assert!(cam.reuse.templates().is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_template_from_selected_operations_keeps_only_those() {
    let d = doc();
    let cam = cam();
    let dir = temp_dir("selection");
    cam.set_data_dir(Some(&dir));
    run(
        &cam,
        &d,
        SAVE_TEMPLATE,
        Some(5),
        json!({"name": "輪郭だけ", "selection": [6]}),
    )
    .unwrap();
    let t = cam.reuse.load_template("輪郭だけ").unwrap();
    assert_eq!(t.operations.len(), 1);
    assert_eq!(t.operations[0].type_id, kmb_strategy_contour2d::CONTOUR);
    assert_eq!(t.operations[0].data["tool"], 3);
    // 工具ライブラリにない工具番号(3)は、足す時の選び方(1 番)にする
    let mut d2 = doc();
    if let Some(f) = d2.data.features.iter_mut().find(|f| f.id == FeatureId(2)) {
        let mut lib = ToolLibrary::starter();
        lib.tools.retain(|t| t.number != 3);
        f.data = serde_json::to_value(lib).unwrap();
    }
    let made = inserts(
        run(
            &cam,
            &d2,
            &format!("{USE_TEMPLATE}輪郭だけ"),
            Some(4),
            Json::Null,
        )
        .unwrap(),
    );
    assert_eq!(made[0].2["tool"], 1);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn names_and_file_names() {
    assert_eq!(template_file_name(" a:b? ").unwrap(), "a_b_");
    assert_eq!(template_file_name("裏.").unwrap(), "裏");
    assert!(template_file_name("  ").is_err());
}
