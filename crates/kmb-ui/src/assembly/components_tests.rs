//! 部品の木の画面の試験: ボディから部品を作るとブラウザの部品の行の下に出て、ボディのまとまりから
//! 消える。部品の外観は中のボディの色になり、解くと元に戻る。どれも 1 回の取り消しで戻る。
//! 部品のアクティブ化(作ったボディが入る・ほかを淡く)、木の並べ替え、部品の複製も。

use crate::assembly::components_ui::{TreeDrag, TreeDrop};
use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_core::{FeatureEntry, FeatureId, Op, Transaction};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use std::collections::HashMap;

/// ボディ(最初に作った機能)`roots` から部品 `name` を作る(画面と同じ部品の操作の道。`parent` を
/// 省くと、ボディがみな同じ部品に入っていればその中)。作った部品の番号。
pub(crate) fn make_component(
    ws: &mut Workspace,
    roots: &[FeatureId],
    name: &str,
    parent: Option<u64>,
) -> Option<u64> {
    let ids: Vec<u64> = roots.iter().map(|r| r.0).collect();
    let mut args = serde_json::json!({ "selection": ids, "name": name });
    if let Some(p) = parent {
        args["parent"] = serde_json::json!(p);
    }
    ws.new_component(args)
}

/// 30×20 の板を 2 枚(高さ 10 と、x = 40 の所に高さ 2)。
fn two_plates() -> Workspace {
    let mut b = DocBuilder::new();
    for (x, t, name) in [(0.0, "10 mm", "箱"), (40.0, "2 mm", "蓋")] {
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(x, 0.0);
        s.rectangle(o, "30 mm", "20 mm", [x, 0.0, x + 30.0, 20.0]);
        let sk = b.feature(kmb_sketch::SKETCH, &format!("{name}のスケッチ"), s.build());
        b.feature(
            EXTRUDE,
            name,
            Extrude {
                profile: Some(sk),
                distance: t.into(),
                ..Default::default()
            },
        );
    }
    let mut ws = Workspace::new(b.finish("板"), None, kmb_bundle::modules());
    ws.wait();
    ws
}

#[test]
fn a_component_groups_bodies_in_the_browser_and_colors_them() {
    let ws = two_plates();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let lid = {
        let ws = h.state_mut();
        let roots = ws.body_roots();
        assert_eq!(roots.len(), 2);
        let lid = roots[1].1;
        let c = make_component(ws, &[lid], "蓋の部品", None).expect("作れる");
        assert_eq!(
            ws.cur.work.doc().data().component_of(lid).map(|c| c.id),
            Some(c)
        );
        lid
    };
    h.run();
    // 部品の行と、その下のボディ。ボディのまとまりには箱だけ
    h.get_by_label("蓋の部品");
    assert_eq!(h.query_all_by_label("ボディ2").count(), 1);
    {
        let ws = h.state_mut();
        // 外観: 部品に付けると、外観の無いボディはその色
        let first = ws.cur.work.doc().data().components[0].id;
        let name = kmb_look::material::appearances()[0].name;
        assert!(ws.set_component_appearance("外観", first, Some(name)));
        let a = kmb_look::material::appearances()[0].color;
        assert_eq!(
            ws.body_appearance_color(lid),
            Some(egui::Color32::from_rgb(a[0], a[1], a[2]))
        );
        // 解くとボディは直下へ戻り、色も無い。取り消すと部品に戻る
        let id = ws.cur.work.doc().data().components[0].id;
        let gone = ws.cur.work.doc().data().components_without(id);
        assert!(ws.set_components("解く", gone));
        assert!(ws.cur.work.doc().data().components.is_empty());
        assert_eq!(ws.body_appearance_color(lid), None);
        ws.undo();
        assert_eq!(
            ws.cur.work.doc().data().component_of(lid).map(|c| c.id),
            Some(id)
        );
    }
}

/// ブラウザの右クリックの「新しい部品にする」「中に新しい部品」は、AI(MCP の `run_action`)と同じ
/// 部品の操作を通る。名前の既定は「部品N」。親は、選んだボディがみな同じ部品に入っていればその中、
/// 別々の部品に入っていれば文書の直下(押した行のボディの部品には入れない)。
#[test]
fn new_components_from_the_browser_menus_follow_the_same_rule_as_the_ai() {
    let ws = two_plates();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let roots = h.state().body_roots();
    let ((box_tip, boxed), (lid_tip, lid)) = (roots[0], roots[1]);
    let comp = |ws: &Workspace, name: &str| {
        ws.cur
            .work
            .doc()
            .data()
            .components
            .iter()
            .find(|c| c.name == name)
            .cloned()
            .unwrap_or_else(|| panic!("部品「{name}」が無い: {:?}", ws.message()))
    };
    let menu = |h: &mut Harness<'static, Workspace>, row: &str, item: &str| {
        h.query_all_by_label(row)
            .next()
            .unwrap_or_else(|| panic!("行「{row}」"))
            .click_secondary();
        h.run();
        h.get_by_label(item).click();
        h.run();
    };
    // 1: 直下の蓋のボディから作ると、直下に「部品1」
    menu(&mut h, "ボディ2", "新しい部品にする");
    let p1 = comp(h.state(), "部品1");
    assert_eq!((p1.parent, p1.bodies.clone()), (None, vec![lid]));
    // 2: 部品の行の「中に新しい部品」は、その中に空の「部品2」
    menu(&mut h, "部品1", "中に新しい部品");
    let p2 = comp(h.state(), "部品2");
    assert_eq!((p2.parent, p2.bodies.len()), (Some(p1.id), 0));
    // 3: 箱を部品2へ移し、蓋(部品1)と箱(部品2)を一緒に選んで蓋の行から作ると、別々の部品なので直下
    {
        let ws = h.state_mut();
        let list = ws
            .cur
            .work
            .doc()
            .data()
            .components_moving(&[boxed], Some(p2.id));
        assert!(ws.set_components("箱を部品2へ", list));
        ws.select_body_row(lid_tip, false);
        ws.select_body_row(box_tip, true);
    }
    h.run();
    menu(&mut h, "ボディ2", "新しい部品にする");
    let p3 = comp(h.state(), "部品3");
    assert_eq!(p3.parent, None, "押した行の部品(部品1)には入れない");
    assert_eq!(p3.bodies.len(), 2);
    // AI の道(役割の印で操作を探して流す)でも同じ結果になる(部品の番号は取り消しても戻らないので比べない)
    let shape = |ws: &Workspace| -> Vec<(String, Option<u64>, Vec<FeatureId>)> {
        ws.cur
            .work
            .doc()
            .data()
            .components
            .iter()
            .map(|c| {
                let mut b = c.bodies.clone();
                b.sort();
                (c.name.clone(), c.parent, b)
            })
            .collect()
    };
    let made = shape(h.state());
    h.state_mut().undo();
    let id = h
        .state()
        .action_for_role(&kmb_interchange::role::action::NEW_COMPONENT)
        .expect("部品の操作");
    h.state_mut()
        .run_action_args(&id, serde_json::json!({ "selection": [lid.0, boxed.0] }));
    assert_eq!(shape(h.state()), made);
    // 4: 取り消して箱だけを選び、箱の行から作ると、箱の入っている部品2の中
    h.state_mut().undo();
    h.state_mut().select_body_row(box_tip, false);
    h.run();
    menu(&mut h, "ボディ1", "新しい部品にする");
    let p3 = comp(h.state(), "部品3");
    assert_eq!((p3.parent, p3.bodies.clone()), (Some(p2.id), vec![boxed]));
}

/// 部品の名前変更は窓で打つ(右クリックのメニューの中の欄は、クリックするとメニューごと閉じたため)。
#[test]
fn a_component_is_renamed_in_the_rename_window() {
    let ws = two_plates();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let id = {
        let ws = h.state_mut();
        let lid = ws.body_roots()[1].1;
        let id = make_component(ws, &[lid], "部品1", None).unwrap();
        ws.cur.renaming = Some((menus_ui::Renaming::Component(id), "蓋".into()));
        id
    };
    h.run();
    h.get_by_label("OK").click();
    h.run();
    let ws = h.state_mut();
    assert_eq!(ws.cur.work.doc().data().component(id).unwrap().name, "蓋");
    assert!(ws.cur.renaming.is_none());
    ws.undo();
    assert_eq!(
        ws.cur.work.doc().data().component(id).unwrap().name,
        "部品1"
    );
}

/// 板 1 枚(高さ `t`)の文書を `path` に書く。
fn write_plate(path: &std::path::Path, t: &str) {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 30.0);
    s.rectangle(o, "10 mm", "10 mm", [0.0, 30.0, 10.0, 40.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "つまみのスケッチ", s.build());
    b.feature(
        EXTRUDE,
        "つまみ",
        Extrude {
            profile: Some(sk),
            distance: t.into(),
            ..Default::default()
        },
    );
    std::fs::write(path, b.finish("つまみ").to_json()).unwrap();
}

/// 人の手順: 別のファイルの「つまみ」をリンクで挿入する → ブラウザに「つまみ」の部品がリンクの印で
/// 出る → 元を直して保存する → 下の帯の知らせと「元が変わった」の様子 → 部品の右クリックの
/// 「最新にする」→ 印が消え、形が新しい高さになる。取り消すと前の高さ。
#[test]
fn a_linked_part_shows_when_its_source_changed_and_updates_from_the_menu() {
    let dir = std::env::temp_dir().join(format!("kmb-ui-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("つまみ.json");
    write_plate(&src, "4 mm");
    let mut ws = two_plates();
    ws.cur.work.set_path(Some(dir.join("組立.json")));
    let id = ws
        .actions()
        .into_iter()
        .find(|a| a.label == "リンクで挿入…")
        .expect("挿入のメニューにある")
        .id;
    ws.run_action_with(&id, Some(src.clone()));
    ws.commit_creation();
    ws.wait();
    assert_eq!(ws.body_roots().len(), 3);
    let height = |ws: &Workspace| {
        let (_, h) = ws.visible_bodies()[2];
        let (lo, hi) = ws.cur.work.kernel().bounds(h).unwrap();
        hi[2] - lo[2]
    };
    assert!((height(&ws) - 4.0).abs() < 1e-3);

    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("つまみ");
    assert_eq!(h.state().cur.links.len(), 1);
    assert_eq!(h.state().cur.links[0].status, kmb_core::LinkStatus::Latest);
    // 元と同じ間は「最新にする」を出さない
    h.get_by_label("つまみ").click_secondary();
    h.run();
    h.get_by_label("この版に固定");
    assert!(h.query_by_label("最新にする").is_none());
    h.key_press(egui::Key::Escape);
    h.run();

    // 元を直して保存する(すぐ調べ直させる)
    write_plate(&src, "7 mm");
    h.state_mut().cur.links_at = None;
    h.run();
    assert_eq!(h.state().cur.links[0].status, kmb_core::LinkStatus::Stale);
    let msg = h.state().win.message.clone().unwrap_or_default();
    assert!(msg.contains("元のファイルが変わりました"), "{msg}");

    // 部品の右クリック → 最新にする
    h.get_by_label("つまみ").click_secondary();
    h.run();
    h.get_by_label("この版に固定");
    h.get_by_label("リンクを切る");
    h.get_by_label("元を開く");
    h.get_by_label("最新にする").click();
    h.run();
    h.state_mut().wait();
    h.run();
    assert_eq!(h.state().cur.links[0].status, kmb_core::LinkStatus::Latest);
    assert!((height(h.state()) - 7.0).abs() < 1e-3);

    // 取り消すと前の写し(また「元が変わった」)
    h.state_mut().undo();
    h.state_mut().wait();
    h.run();
    assert!((height(h.state()) - 4.0).abs() < 1e-3);
    assert_eq!(h.state().cur.links[0].status, kmb_core::LinkStatus::Stale);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 部品表は表の窓に出る。「列…」で列を出し・隠し・並べ替え、選び方は画面の配置に覚え、
/// 「既定に戻す」で戻る。窓を開いている間に部品を作ると、表も作り直す。
#[test]
fn the_parts_list_is_a_table_with_chosen_columns() {
    let mut ws = two_plates();
    ws.run_action("asm.bom");
    assert!(ws.viewer().is_none(), "文の窓ではなく表の窓");
    let (title, t) = ws.table_view().expect("表の窓");
    assert_eq!(title, "部品表");
    assert_eq!(t.rows.len(), 2);
    let ids = |ws: &Workspace| -> Vec<String> {
        let (_, t) = ws.table_view().unwrap();
        ws.table_shown()
            .iter()
            .map(|&i| t.columns[i].id.clone())
            .collect()
    };
    assert_eq!(
        ids(&ws),
        [
            "item",
            "name",
            "count",
            "material",
            "length",
            "width",
            "thickness",
            "volume",
            "mass"
        ]
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("長さ mm");
    assert_eq!(h.query_all_by_label("種類").count(), 0, "既定では隠す列");
    // 列の設定を開き、「種類」を出す(既定の並びの位置に入る)
    h.get_by_label("列…").click();
    h.run();
    h.get_by_label("種類").click();
    h.run();
    assert_eq!(
        ids(h.state()),
        [
            "item",
            "name",
            "kind",
            "count",
            "material",
            "length",
            "width",
            "thickness",
            "volume",
            "mass"
        ]
    );
    assert_eq!(
        h.query_all_by_label("種類").count(),
        2,
        "列の設定と表の見出し"
    );
    {
        // 並べ替えと隠す。最後の 1 列は隠さない
        let ws = h.state_mut();
        ws.table_column("kind", -1);
        ws.table_column("mass", 0);
        assert_eq!(
            ids(ws),
            [
                "item",
                "kind",
                "name",
                "count",
                "material",
                "length",
                "width",
                "thickness",
                "volume"
            ]
        );
        assert_eq!(
            ws.win.layout.table_columns["asm.bom"].len(),
            9,
            "配置に覚える"
        );
        for id in [
            "item",
            "kind",
            "name",
            "count",
            "material",
            "length",
            "width",
            "thickness",
        ] {
            ws.table_column(id, 0);
        }
        assert_eq!(ids(ws), ["volume"]);
        ws.table_column("volume", 0);
        assert_eq!(ids(ws), ["volume"], "最後の列は隠さない");
    }
    h.run();
    h.get_by_label("既定に戻す").click();
    h.run();
    assert!(h.state().win.layout.table_columns.is_empty());
    assert_eq!(ids(h.state()).len(), 9);
    // 開いたまま部品を作ると、表も新しい名前になる
    {
        let ws = h.state_mut();
        let lid = ws.body_roots()[1].1;
        make_component(ws, &[lid], "蓋の部品", None).expect("作れる");
    }
    h.run();
    let (_, t) = h.state().table_view().unwrap();
    assert!(
        t.rows.iter().any(|r| r[1] == "蓋の部品" && r[2] == "部品"),
        "{:?}",
        t.rows
    );
    assert_eq!(h.query_all_by_label("蓋の部品").count(), 2, "部品の木と表");
    // 閉じると消える
    h.state_mut().cur.table_view = None;
    h.run();
    assert_eq!(h.query_all_by_label("長さ mm").count(), 0);
}

#[test]
fn table_columns_toggle_in_the_default_order_and_move_within_ends() {
    use crate::table_ui::{move_column, toggle_column};
    let mut shown = vec![0, 1, 3, 4];
    toggle_column(&mut shown, 2);
    assert_eq!(shown, [0, 1, 2, 3, 4]);
    toggle_column(&mut shown, 1);
    assert_eq!(shown, [0, 2, 3, 4]);
    toggle_column(&mut shown, 9);
    assert_eq!(shown, [0, 2, 3, 4, 9]);
    move_column(&mut shown, 0, -1);
    assert_eq!(shown, [0, 2, 3, 4, 9]);
    move_column(&mut shown, 9, 1);
    assert_eq!(shown, [0, 2, 3, 4, 9]);
    move_column(&mut shown, 3, -1);
    assert_eq!(shown, [0, 3, 2, 4, 9]);
    move_column(&mut shown, 7, 1);
    assert_eq!(shown, [0, 3, 2, 4, 9]);
}

/// 箱のボディを写した新しいボディを足す(新しいボディを作る機能の代わり)。
fn add_copy_of_box(ws: &mut Workspace) -> FeatureId {
    let (last, _) = ws.body_roots()[0];
    let id = ws.cur.work.alloc_feature_id();
    assert!(ws.apply(Transaction::new(
        "写しを作成",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: "solid.move@1".into(),
                name: "写し".into(),
                data: serde_json::json!({"body": last.0, "copy": true, "z": "50 mm"}),
                suppressed: false,
            },
        }],
    )));
    ws.wait();
    id
}

/// アクティブな部品へ、作ったボディと直下に作った部品が入る(作った変更と 1 回の取り消しで戻る)。
/// ほかの部品のボディは淡く描く。外したボディはまた入れない。部品が無くなると直下に戻る。
#[test]
fn bodies_made_while_a_component_is_active_go_into_it() {
    let mut ws = two_plates();
    let (box_last, box_root) = ws.body_roots()[0];
    let lid = ws.body_roots()[1].1;
    let c = make_component(&mut ws, &[lid], "蓋の部品", None).expect("作れる");
    ws.wait();
    assert!(ws.cur.scene.faded().is_empty());
    ws.activate_component(Some(c));
    assert_eq!(ws.active_component(), Some(c));
    // 部品の外の箱は淡く、中の蓋は淡くしない
    assert!(ws.cur.scene.faded().contains(&box_last));
    assert_eq!(ws.cur.scene.faded().len(), 1);
    let depth = ws.cur.work.doc().undo_depth();
    let copy = add_copy_of_box(&mut ws);
    assert_eq!(
        ws.cur.work.doc().data().component_of(copy).map(|x| x.id),
        Some(c)
    );
    assert_eq!(
        ws.cur.work.doc().undo_depth(),
        depth + 1,
        "作った変更と同じ取り消し単位"
    );
    // 取り消すとボディも部品の中身も戻り、やり直すと部品に入ったまま戻る
    ws.undo();
    ws.wait();
    assert!(ws.cur.work.doc().data().feature(copy).is_none());
    assert_eq!(
        ws.cur.work.doc().data().component(c).unwrap().bodies,
        vec![lid]
    );
    ws.redo();
    ws.wait();
    assert_eq!(
        ws.cur.work.doc().data().component_of(copy).map(|x| x.id),
        Some(c)
    );
    // 外したボディは、次の計算で戻さない
    let out = ws.cur.work.doc().data().components_moving(&[copy], None);
    assert!(ws.set_components("外す", out));
    ws.wait();
    assert!(ws.cur.work.doc().data().component_of(copy).is_none());
    // 直下に作った部品は、アクティブな部品の中へ
    let k = make_component(&mut ws, &[], "中の部品", None).expect("作れる");
    ws.wait();
    assert_eq!(
        ws.cur.work.doc().data().component(k).unwrap().parent,
        Some(c)
    );
    // 箱は部品の外のまま
    assert!(ws.cur.work.doc().data().component_of(box_root).is_none());
    // 文書の直下に戻すと、淡くしない。作った物は入らない
    ws.activate_component(None);
    assert!(ws.cur.scene.faded().is_empty());
    let other = add_copy_of_box(&mut ws);
    assert!(ws.cur.work.doc().data().component_of(other).is_none());
    // アクティブな部品を解くと直下に戻る
    ws.activate_component(Some(c));
    let gone = ws.cur.work.doc().data().components_without(c);
    assert!(ws.set_components("解く", gone));
    ws.wait();
    assert_eq!(ws.active_component(), None);
    assert!(ws.cur.scene.faded().is_empty());
}

/// 移動/コピーの写しは、元のボディの部品へ入る(アクティブな部品があっても元の部品が先)。
/// 作った変更と 1 回の取り消しで戻る。
#[test]
fn copies_go_into_the_component_of_their_original() {
    let mut ws = two_plates();
    let box_root = ws.body_roots()[0].1;
    let lid = ws.body_roots()[1].1;
    let a = make_component(&mut ws, &[box_root], "箱の部品", None).expect("作れる");
    let c = make_component(&mut ws, &[lid], "蓋の部品", None).expect("作れる");
    ws.wait();
    let copy = add_copy_of_box(&mut ws);
    let home = |ws: &Workspace, id| ws.cur.work.doc().data().component_of(id).map(|k| k.id);
    assert_eq!(home(&ws, copy), Some(a));
    // 取り消すと写しごと消え、部品の木も元に戻る(取り消し 1 回)
    ws.undo();
    ws.wait();
    assert!(ws.cur.work.doc().data().feature(copy).is_none());
    assert_eq!(
        ws.cur.work.doc().data().component(a).unwrap().bodies,
        vec![box_root]
    );
    // 蓋の部品がアクティブでも、箱の写しは箱の部品へ
    ws.activate_component(Some(c));
    let copy = add_copy_of_box(&mut ws);
    assert_eq!(home(&ws, copy), Some(a));
}

/// 部品の行の丸を押すとアクティブになり、文書の行の丸で直下へ戻る。
#[test]
fn the_round_button_on_a_component_row_activates_it() {
    let mut ws = two_plates();
    let lid = ws.body_roots()[1].1;
    let c = make_component(&mut ws, &[lid], "蓋の部品", None).expect("作れる");
    ws.wait();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("蓋の部品 をアクティブにする").click();
    h.run();
    assert_eq!(h.state().active_component(), Some(c));
    h.get_by_label("蓋の部品 はアクティブ");
    h.get_by_label("(無題) をアクティブにする").click();
    h.run();
    assert_eq!(h.state().active_component(), None);
}

/// 部品の木の並べ替え: 部品を別の部品の中・前後へ、ボディを部品へ。自分の中へは落とせない。
#[test]
fn components_and_bodies_move_in_the_tree() {
    let mut ws = two_plates();
    let lid = ws.body_roots()[1].1;
    let a = make_component(&mut ws, &[], "A", None).expect("作れる");
    let b = make_component(&mut ws, &[], "B", None).expect("作れる");
    let c = make_component(&mut ws, &[], "C", None).expect("作れる");
    let order = |ws: &Workspace, p: Option<u64>| -> Vec<u64> {
        ws.cur
            .work
            .doc()
            .data()
            .child_components(p)
            .iter()
            .map(|c| c.id)
            .collect()
    };
    assert_eq!(order(&ws, None), vec![a, b, c]);
    // C を A の前へ
    assert!(ws.tree_drop_ok(TreeDrag::Component(c), TreeDrop::Before(a)));
    assert!(ws.drop_in_tree(TreeDrag::Component(c), TreeDrop::Before(a)));
    assert_eq!(order(&ws, None), vec![c, a, b]);
    // B を A の中へ。A は B の中へ落とせない(輪になる)
    assert!(ws.drop_in_tree(TreeDrag::Component(b), TreeDrop::Into(Some(a))));
    assert_eq!(order(&ws, Some(a)), vec![b]);
    assert!(!ws.tree_drop_ok(TreeDrag::Component(a), TreeDrop::Into(Some(b))));
    assert!(!ws.tree_drop_ok(TreeDrag::Component(a), TreeDrop::Before(b)));
    // C を A の中の B の後ろへ(親も替わる)
    assert!(ws.drop_in_tree(TreeDrag::Component(c), TreeDrop::After(b)));
    assert_eq!(order(&ws, Some(a)), vec![b, c]);
    // ボディを C へ、文書の直下へ
    assert!(ws.tree_drop_ok(TreeDrag::Body(lid), TreeDrop::Into(Some(c))));
    assert!(ws.drop_in_tree(TreeDrag::Body(lid), TreeDrop::Into(Some(c))));
    assert_eq!(
        ws.cur.work.doc().data().component_of(lid).map(|x| x.id),
        Some(c)
    );
    assert!(!ws.tree_drop_ok(TreeDrag::Body(lid), TreeDrop::Into(Some(c))));
    assert!(ws.drop_in_tree(TreeDrag::Body(lid), TreeDrop::Into(None)));
    assert!(ws.cur.work.doc().data().component_of(lid).is_none());
    // どれも 1 回で戻る
    ws.undo();
    assert_eq!(
        ws.cur.work.doc().data().component_of(lid).map(|x| x.id),
        Some(c)
    );
}

/// 部品の複製: 中の部品ごと写し、元の横(幅 30 + 隙間 10)に置く。元を直すと写しも変わる。1 回で戻る。
#[test]
fn a_component_is_duplicated_beside_the_original() {
    let mut ws = two_plates();
    let lid = ws.body_roots()[1].1;
    let p = make_component(&mut ws, &[lid], "蓋の部品", None).expect("作れる");
    let inner = make_component(&mut ws, &[], "中", Some(p)).expect("作れる");
    ws.wait();
    let depth = ws.cur.work.doc().undo_depth();
    let top = ws.duplicate_component(p).expect("複製できる");
    ws.wait();
    assert_eq!(ws.cur.work.doc().undo_depth(), depth + 1);
    let data = ws.cur.work.doc().data();
    let made = data.component(top).unwrap();
    assert_eq!(made.name, "蓋の部品 (2)");
    assert_eq!(made.parent, None);
    assert_eq!(made.bodies.len(), 1);
    // 元のすぐ後ろに並び、中の部品も写す
    let tops: Vec<u64> = data.child_components(None).iter().map(|c| c.id).collect();
    assert_eq!(tops, vec![p, top]);
    let kids = data.child_components(Some(top));
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0].name, "中");
    assert_ne!(kids[0].id, inner);
    // ボディが 3 つになり、写しは x = 80〜110(蓋は 40〜70)
    let copy = made.bodies[0];
    let roots = ws.body_roots();
    assert_eq!(roots.len(), 3);
    let shapes: HashMap<FeatureId, kmb_core::ShapeHandle> =
        ws.visible_bodies().into_iter().collect();
    let last = roots.iter().find(|(_, r)| *r == copy).unwrap().0;
    let (lo, hi) = ws.cur.work.kernel().bounds(shapes[&last]).unwrap();
    assert!(
        (lo[0] - 80.0).abs() < 1e-3 && (hi[0] - 110.0).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
    ws.undo();
    ws.wait();
    assert_eq!(ws.body_roots().len(), 2);
    assert!(ws.cur.work.doc().data().component(top).is_none());
}

/// 30×20×10 の塊を 3 つ: x = 0 と、そこへ 10 めり込む x = 20 と、その x = 50 の面で接する x = 50。
fn three_blocks() -> Workspace {
    let mut b = DocBuilder::new();
    for (x, name) in [(0.0, "A"), (20.0, "B"), (50.0, "C")] {
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(x, 0.0);
        s.rectangle(o, "30 mm", "20 mm", [x, 0.0, x + 30.0, 20.0]);
        let sk = b.feature(kmb_sketch::SKETCH, &format!("{name}のスケッチ"), s.build());
        b.feature(
            EXTRUDE,
            name,
            Extrude {
                profile: Some(sk),
                distance: "10 mm".into(),
                ..Default::default()
            },
        );
    }
    let mut ws = Workspace::new(b.finish("塊"), None, kmb_bundle::modules());
    ws.wait();
    ws
}

/// 干渉は表の窓に組ごとの体積で出し、重なった所を 3D に赤く重ねる。「一致した面を含める」で
/// 面で接する組も出し、「すべての干渉を表示」を切ると押した行の所だけを重ねる。
#[test]
fn interference_is_a_table_with_red_overlaps() {
    let mut ws = three_blocks();
    ws.run_action("asm.interference");
    assert!(ws.viewer().is_none(), "文の窓ではなく表の窓");
    let (title, t) = ws.table_view().expect("表の窓");
    assert_eq!(title, "干渉");
    assert_eq!(t.rows.len(), 1, "{:?}", t.rows);
    assert_eq!(t.rows[0][1], "2000.000");
    assert_eq!(ws.table_ghosts().len(), 1, "重なりを赤く重ねる");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("グループ");
    h.get_by_label("部品 2");
    h.get_by_label("一致した面を含める").click();
    h.run();
    {
        let (_, t) = h.state().table_view().unwrap();
        assert_eq!(t.rows.len(), 2, "{:?}", t.rows);
        assert_eq!(t.rows[1][1], "0.000");
        assert_eq!(t.rows[1][4], "面の一致");
        assert_eq!(t.rows[1][5], "200.00");
    }
    assert_eq!(h.state().table_ghosts().len(), 2, "接する面も塗る");
    // 1 組ずつ見る: 切ると何も重ねず、行を押すとその組だけ
    h.get_by_label("すべての干渉を表示").click();
    h.run();
    assert!(h.state().table_ghosts().is_empty());
    h.get_by_label("2000.000").click();
    h.run();
    assert_eq!(h.state().table_row(), Some(0));
    assert_eq!(h.state().table_ghosts().len(), 1);
    // 文書が変わって作り直しても、切り替えと選んだ行は残る
    let b = h.state().body_roots()[1].1;
    let mut data = h.state().document().data().feature(b).unwrap().data.clone();
    data["distance"] = "5 mm".into();
    h.state_mut().set_feature_data(b, data);
    h.state_mut().wait();
    h.run();
    let (_, t) = h.state().table_view().unwrap();
    assert_eq!(t.rows[0][1], "1000.000", "B を低くした形で調べ直す");
    assert!(t.options[0].on, "一致した面を含めるのまま");
    assert_eq!(t.rows.len(), 2);
    assert_eq!(h.state().table_row(), Some(0));
    // もう一度押すと外す
    h.get_by_label("1000.000").click();
    h.run();
    assert_eq!(h.state().table_row(), None);
}

/// ファイルを選ぶ窓で、いつも `0` を選ぶ。
struct PickFile(std::path::PathBuf);

impl crate::Dialogs for PickFile {
    fn open_document(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn save_document(&self, _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn open_file(&self, _: &str, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
        Some(self.0.clone())
    }
    fn save_file(&self, _: &str, _: &str, _: &[&str], _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn unsaved(&self, _: &str) -> crate::Unsaved {
        crate::Unsaved::Cancel
    }
}

/// 人の手順(10-20 a7): 「つまみ」をリンクで挿入する → 元のファイルを消す → 下の帯に「見つかりません」と
/// 選び直し方が出る → 部品の右クリックに「最新にする」は無く「元のファイルを選び直す…」がある →
/// 押して別のファイル(つまみが無く、台だけ)を選ぶ → 下の帯に見つからないボディ「つまみ」が並ぶ。
#[test]
fn a_missing_link_source_can_be_picked_again_from_the_menu() {
    let dir = std::env::temp_dir().join(format!("kmb-ui-relink-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("つまみ.json");
    write_plate(&src, "4 mm");
    let other = dir.join("台.json");
    {
        let mut b = DocBuilder::new();
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(0.0, 0.0);
        s.rectangle(o, "20 mm", "20 mm", [0.0, 0.0, 20.0, 20.0]);
        let sk = b.feature(kmb_sketch::SKETCH, "台のスケッチ", s.build());
        let base = b.feature(
            EXTRUDE,
            "台",
            Extrude {
                profile: Some(sk),
                distance: "3 mm".into(),
                ..Default::default()
            },
        );
        // 台のボディに名前を付ける(つまみの「ボディ1」と名前で対応しない)
        let mut v: serde_json::Value = serde_json::from_str(&b.finish("台").to_json()).unwrap();
        v["body_names"] = serde_json::json!({ base.0.to_string(): "台" });
        std::fs::write(&other, v.to_string()).unwrap();
    }
    let mut ws = two_plates();
    ws.cur.work.set_path(Some(dir.join("組立.json")));
    let id = ws
        .actions()
        .into_iter()
        .find(|a| a.label == "リンクで挿入…")
        .expect("挿入のメニューにある")
        .id;
    ws.run_action_with(&id, Some(src.clone()));
    ws.commit_creation();
    ws.wait();
    ws.win.dialogs = Box::new(PickFile(other.clone()));

    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    std::fs::remove_file(&src).unwrap();
    h.state_mut().cur.links_at = None;
    h.run();
    assert_eq!(h.state().cur.links[0].status, kmb_core::LinkStatus::Missing);
    let msg = h.state().win.message.clone().unwrap_or_default();
    assert!(msg.contains("元のファイルを選び直す"), "{msg}");

    h.get_by_label("つまみ").click_secondary();
    h.run();
    assert!(h.query_by_label("最新にする").is_none());
    h.get_by_label("元のファイルを選び直す…").click();
    h.run();
    h.state_mut().wait();
    h.run();
    let msg = h.state().win.message.clone().unwrap_or_default();
    assert!(msg.contains("見つからないボディ: ボディ1"), "{msg}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 部品のボディを選んで M を押すと、中の形でなく部品ごと値で動かす位置の機能を作り始める
/// (Esc で足す前に戻る)。部品をアクティブにした中では、ボディの移動/コピーのまま(10-22 a2)。
#[test]
fn m_on_a_part_moves_the_whole_part_by_value() {
    let mut ws = two_plates();
    let (lid_body, lid) = ws.body_roots()[1];
    let c = make_component(&mut ws, &[lid], "蓋の部品", None).expect("作れる");
    ws.wait();
    ws.select_body_row(lid_body, false);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let count = h.state().document().data().features.len();
    let started = |h: &mut Harness<Workspace>| {
        h.key_press(egui::Key::M);
        h.run();
        let id = h.state().creating().expect("M で始める");
        let ws = h.state();
        let t = ws.document().data().feature(id).unwrap().type_id.clone();
        ws.win.registry.descriptor(&t).and_then(|d| d.role)
    };
    assert_eq!(
        started(&mut h),
        Some(kmb_interchange::role::kind::COMPONENT_POSITION),
        "部品ごと動かす位置の機能"
    );
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(
        h.state().document().data().features.len(),
        count,
        "Esc で戻る"
    );
    h.state_mut().activate_component(Some(c));
    h.state_mut().select_body_row(lid_body, false);
    h.run();
    assert_eq!(
        started(&mut h),
        Some(kmb_interchange::role::kind::COPY_BODY),
        "アクティブな部品の中ではボディの移動/コピー"
    );
}

/// ブラウザの木の開け閉め・原点とまとまりの目・ボディと行の目・畳んだグループは、文書を開き直しても
/// 戻る(10-25 u1)。「ブラウザ内を検索」「タイムライン内を検索」は、畳んだ部品(親も)とグループを
/// 開いて探した行を見せる(10-26 b1)。
#[test]
fn the_browser_tree_and_eyes_come_back_on_reopen_and_find_opens_folded_rows() {
    use crate::browser_ui::{Kind, tree_key};
    let d = crate::tests::temp_dir("doc-view");
    let path = d.join(format!("板.{}", kmb_core::DOCUMENT_EXTENSION));
    let mut ws = two_plates().with_data_dir(d.join("data"));
    let roots = ws.body_roots();
    let ((box_tip, boxed), (_, lid)) = (roots[0], roots[1]);
    let outer = make_component(&mut ws, &[boxed], "外", None).expect("外");
    let inner = make_component(&mut ws, &[boxed], "中", Some(outer)).expect("中");
    assert_eq!(
        ws.document().data().component_of(boxed).map(|c| c.id),
        Some(inner)
    );
    // 畳んだ部品の中のボディを探すと、文書・外・中を開く
    for k in [
        tree_key("doc"),
        tree_key(("component", outer)),
        tree_key(("component", inner)),
    ] {
        ws.cur.tree_open.insert(k, false);
    }
    ws.find_in_browser(box_tip);
    for k in [
        tree_key("doc"),
        tree_key(("component", outer)),
        tree_key(("component", inner)),
    ] {
        assert_eq!(ws.cur.tree_open.get(&k), Some(&true), "{k} を開く");
    }
    // 畳んだグループの中の機能を探すと、グループを開く
    let named = |ws: &Workspace, n: &str| {
        ws.document()
            .data()
            .features
            .iter()
            .find(|f| f.name == n)
            .map(|f| f.id)
            .expect(n)
    };
    let (sketch, plate) = (named(&ws, "蓋のスケッチ"), named(&ws, "蓋"));
    let g = ws
        .group_features(&[sketch, plate], kmb_session::edit::GroupMode::Exact)
        .expect("グループ");
    ws.cur.collapsed.insert(g);
    ws.find_in_timeline(plate);
    assert!(!ws.cur.collapsed.contains(&g), "グループを開く");

    // 保存した文書を開き、見え方を変える
    kmb_session::save(&path, ws.document()).unwrap();
    assert!(ws.load_path(&path));
    ws.wait();
    assert_eq!(ws.doc_view(), Default::default(), "覚えていなければ既定");
    let box_sketch = named(&ws, "箱のスケッチ");
    assert!(ws.is_own_hidden(box_sketch), "使ったスケッチは隠れている");
    // 目は文書の区画に入る(文書を保存すれば開き直しても戻る)
    ws.set_hidden(box_sketch, false);
    ws.set_origin_shown(true);
    ws.set_folder_hidden(Kind::Sketch, true);
    ws.set_body_hidden(lid, true);
    kmb_session::save(&path, ws.document()).unwrap();
    ws.cur.collapsed.insert(g);
    ws.cur
        .tree_open
        .insert(tree_key(("component", outer)), false);
    ws.save_doc_view();
    let view = ws.doc_view();

    // 次に起動して開き直すと戻る
    let mut again = Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_data_dir(d.join("data"));
    assert!(again.load_path(&path));
    again.wait();
    assert_eq!(again.doc_view(), view);
    assert!(again.is_origin_shown() && again.is_folder_hidden(Kind::Sketch));
    assert!(again.cur.collapsed.contains(&g));
    assert!(
        !again.is_own_hidden(box_sketch),
        "目を開けたスケッチは出したまま"
    );
    assert!(again.is_own_hidden(named(&again, "蓋のスケッチ")));
    assert!(
        again.cur.hidden_now.contains(&again.body_roots()[1].0),
        "隠したボディ"
    );
    // 新しい文書は既定の見え方
    again.new_document();
    assert_eq!(again.doc_view(), Default::default());
    std::fs::remove_dir_all(&d).ok();
}
