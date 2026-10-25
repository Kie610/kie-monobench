//! アニメーションの作業空間(24b)の試験: 人の操作の順で、作業空間を替え、ジョイントの動きと
//! 分解の段を足し、ストーリーボードで時刻を動かして再生し、動画に書き出す。設計に戻ると
//! ボディは元の位置に戻る。自動分解は小さな道具で仕方・跡の線・目盛を選んで足す。

use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;

/// 箱(30×20×10)と、x = 40 の所の蓋(30×20×2)。蓋は奥の上の辺(y = 20、z = 2)の蝶番で箱につなぐ。
fn hinged() -> (Workspace, FeatureId, FeatureId) {
    let mut b = DocBuilder::new();
    let mut ids = Vec::new();
    for (x, t, name) in [(0.0, "10 mm", "箱"), (40.0, "2 mm", "蓋")] {
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(x, 0.0);
        s.rectangle(o, "30 mm", "20 mm", [x, 0.0, x + 30.0, 20.0]);
        let sk = b.feature(kmb_sketch::SKETCH, &format!("{name}のスケッチ"), s.build());
        ids.push(b.feature(
            EXTRUDE,
            name,
            Extrude {
                profile: Some(sk),
                distance: t.into(),
                ..Default::default()
            },
        ));
    }
    let (boxed, lid) = (ids[0], ids[1]);
    b.feature(
        "asm.joint@1",
        "ジョイント1",
        serde_json::json!({
            "kind": "revolute",
            "moving": {"kind": "body", "root": lid.0},
            "base": {"kind": "body", "root": boxed.0},
            "bodies": [lid.0],
            "origin": [40.0, 20.0, 2.0],
            "axis": [1.0, 0.0, 0.0],
            "angle_min": "-120 deg",
            "angle_max": "0 deg",
        }),
    );
    let mut ws = Workspace::new(b.finish("蓋つきの箱"), None, kmb_bundle::modules());
    ws.wait();
    let joint = ws.cur.work.doc().data().features.last().unwrap().id;
    (ws, lid, joint)
}

fn frame(ws: &mut Workspace) {
    let ctx = egui::Context::default();
    ws.sync_animation(&ctx);
}

#[test]
fn the_storyboard_opens_the_lid_explodes_it_and_plays_back() {
    let (mut ws, lid, joint) = hinged();
    let rest = ws.cur.scene.bounds();
    // 人の手順 1: 作業空間を「アニメーション」へ替える → タブ「アニメーション」に変換と公開
    ws.switch_space(Space::Animation);
    frame(&mut ws);
    let tabs = ws.ribbon_tabs();
    assert_eq!(tabs.len(), 1, "{tabs:?}");
    assert_eq!(tabs[0].name, "アニメーション");
    let groups: Vec<&str> = tabs[0].groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(groups, ["変換", "ビュー", "公開"]);
    let video = ws
        .item_of("a:render.animation_video")
        .expect("動画の書き出し");
    assert!(video.disabled.is_some(), "段が無い間は押せない");
    assert!(ws.item_of("u:measure").is_none(), "測る道具は出さない");

    // 2: 「ジョイントの動き」→ 蝶番の軸をクリック(手順の値を渡す)。下限 -120 度から上限 0 度
    ws.start_action_with("anim.drive", serde_json::json!({"joint": joint.0}));
    assert_eq!(
        kmb_interchange::animation::steps(ws.cur.work.doc().data()).len(),
        1,
        "{:?}",
        ws.message()
    );
    frame(&mut ws);
    // 足した段の終わり(開き切る前の上限 0 度)へ進むので、動いていない
    assert_eq!(ws.animation_time(), 2.0);
    assert!(!ws.cur.scene.is_posed());
    // 段の帯の初め(-120 度)では蓋が立ち上がって奥へ倒れている
    ws.set_animation_time(0.0);
    frame(&mut ws);
    assert!(ws.cur.scene.is_posed());
    let open = ws.cur.scene.bounds();
    assert!(open.1[2] > rest.1[2] + 10.0, "蓋が立つ: {open:?} {rest:?}");

    // 3: 「分解の移動」→ 蓋を Z へ 15 mm(動かす物と向きと距離を渡す)
    ws.start_action_with(
        "anim.explode",
        serde_json::json!({
            "targets": [{"kind": "body", "root": lid.0}],
            "part": "f", "dir": [0.0, 0.0, 1.0], "distance": 15.0,
        }),
    );
    assert_eq!(
        kmb_interchange::animation::steps(ws.cur.work.doc().data()).len(),
        2,
        "{:?}",
        ws.message()
    );
    frame(&mut ws);
    assert_eq!(ws.animation_time(), 3.0, "足した段の終わり");
    let up = ws.cur.scene.bounds();
    // 蓋は閉じた(上限 0 度)まま、上へ 15 mm(厚さ 2 の上の面が z = 17)
    assert!((up.1[2] - 17.0).abs() < 0.05, "{up:?}");
    let label =
        ws.animation_step_label(&kmb_interchange::animation::steps(ws.cur.work.doc().data())[1]);
    assert_eq!(label, "分解: ボディ2 を Z へ 15 mm");

    // 4: 再生: 終わりにいるので初めから。0.5 秒ずつ進めると終わりで止まる
    ws.toggle_animation_play();
    assert!(ws.is_animation_playing());
    assert_eq!(ws.animation_time(), 0.0);
    for _ in 0..10 {
        ws.advance_animation(0.5);
    }
    assert!(!ws.is_animation_playing());
    assert_eq!(ws.animation_time(), 3.0);

    // 5: 動画に書き出す(小さな絵、1 秒 8 枚 → 3 秒で 25 枚の連番 PNG)
    assert!(ws.change_settings(
        "render.scene",
        &serde_json::json!({"video_width": 64, "video_height": 48, "fps": 8}),
    ));
    let d = std::env::temp_dir().join(format!("kmb-ui-anim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let seq = d.join("開く.png");
    ws.run_action_with("render.animation_video", Some(seq.clone()));
    ws.wait_task();
    assert!(d.join("開く-0025.png").exists(), "{:?}", ws.message());
    assert!(!d.join("開く-0026.png").exists());
    std::fs::remove_dir_all(&d).ok();

    // 6: 設計に戻ると元の位置。設計の形は変えていない
    ws.switch_space(Space::Design);
    frame(&mut ws);
    assert!(!ws.cur.scene.is_posed());
    assert_eq!(ws.cur.scene.bounds(), rest);
    // 7: 取り消すと分解の段が消える(シーンの設定は文書ごとなので、その変更を先に取り消す)
    ws.undo();
    ws.undo();
    assert_eq!(
        kmb_interchange::animation::steps(ws.cur.work.doc().data()).len(),
        1
    );
}

#[test]
fn the_storyboard_pane_shows_the_steps_and_edits_the_one_at_the_time() {
    let (mut ws, lid, _) = hinged();
    ws.switch_space(Space::Animation);
    assert!(ws.set_animation(
        "段",
        vec![kmb_interchange::animation::Step {
            kind: kmb_interchange::animation::StepKind::Explode {
                targets: vec![kmb_interchange::animation::Target::Body { root: lid }],
                offset: [0.0, -12.0, 0.0],
            },
            seconds: 1.5,
            trail: false,
        }],
    ));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("段 1/1: 分解: ボディ2 を -Y へ 12 mm");
    h.get_by_label("再生");
    h.get_by_label("段を消す").click();
    h.run();
    assert!(kmb_interchange::animation::steps(h.state().cur.work.doc().data()).is_empty());
}

/// 自動分解: リボンの「自動分解: 1 レベル」で小さな道具が出て、先に
/// 見せるだけ(文書は変えない)。ワンステップは同時の段 1 つ、順次は部品ごとに 1 秒の段。目盛で
/// 距離が変わる。OK で 1 回の取り消し単位で足し、Esc でやめると何も足さず時刻が戻る。
#[test]
fn auto_explode_previews_the_steps_and_adds_them_on_ok() {
    use kmb_interchange::animation::StepKind;
    let (mut ws, _, _) = hinged();
    ws.switch_space(Space::Animation);
    let before = kmb_interchange::animation::steps(ws.cur.work.doc().data());
    let t0 = ws.animation_time();
    assert!(ws.start_action("anim.auto_explode_one").is_none());
    assert!(ws.is_auto_exploding(), "{:?}", ws.message());
    assert_eq!(
        kmb_interchange::animation::steps(ws.cur.work.doc().data()),
        before,
        "先に見せるだけ"
    );
    let shown = ws.animation_steps_shown();
    assert_eq!(shown.len(), before.len() + 1);
    let last = shown.last().unwrap();
    assert!(last.trail, "既定は跡の線あり");
    // 箱(x 0〜30)は組立の中心(x = 35)の左なので -X へ、蓋(x 40〜70)は +X へ、それぞれ長さ 30
    let StepKind::ExplodeMany { moves } = &last.kind else {
        panic!("{last:?}")
    };
    let offs: Vec<[f64; 3]> = moves.iter().map(|m| m.offset).collect();
    assert_eq!(offs, vec![[-30.0, 0.0, 0.0], [30.0, 0.0, 0.0]]);
    assert_eq!(
        ws.animation_time(),
        kmb_interchange::animation::duration(&shown)
    );
    frame(&mut ws);
    let lines = ws.trail_lines();
    assert_eq!(lines.len(), 2, "跡の線は動いた 2 つのボディ");
    // 順次・跡の線なし・目盛 2: 部品ごとの段、距離は 2 倍
    ws.set_auto_explode(true, false, 2.0);
    let shown = ws.animation_steps_shown();
    assert_eq!(shown.len(), before.len() + 2);
    let offs: Vec<[f64; 3]> = shown[before.len()..]
        .iter()
        .map(|s| match &s.kind {
            StepKind::Explode { offset, .. } => *offset,
            k => panic!("{k:?}"),
        })
        .collect();
    assert_eq!(offs, vec![[-60.0, 0.0, 0.0], [60.0, 0.0, 0.0]]);
    assert!(
        shown[before.len()..]
            .iter()
            .all(|s| !s.trail && s.seconds == 1.0)
    );
    frame(&mut ws);
    assert!(ws.trail_lines().is_empty());
    // OK で足す。取り消しは 1 回で全部
    ws.finish_auto_explode();
    assert!(!ws.is_auto_exploding());
    assert_eq!(
        kmb_interchange::animation::steps(ws.cur.work.doc().data()).len(),
        before.len() + 2
    );
    ws.undo();
    assert_eq!(
        kmb_interchange::animation::steps(ws.cur.work.doc().data()),
        before
    );
    // やめると何も足さず、時刻は道具を出す前に戻る
    ws.set_animation_time(t0);
    ws.start_action("anim.auto_explode_all");
    assert!(ws.is_auto_exploding());
    ws.cancel_auto_explode();
    assert_eq!(
        kmb_interchange::animation::steps(ws.cur.work.doc().data()),
        before
    );
    assert_eq!(ws.animation_time(), t0);
}

/// 画面: リボンの「自動分解: 1 レベル」を押すとビューの上に小さな道具が出て、OK で段が足される。
#[test]
fn the_auto_explode_tool_appears_over_the_view_and_ok_adds_the_step() {
    let (mut ws, _, _) = hinged();
    ws.switch_space(Space::Animation);
    let n = kmb_interchange::animation::steps(ws.cur.work.doc().data()).len();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("自動分解: 1 レベル").click();
    h.run();
    assert!(h.state().is_auto_exploding());
    h.get_by_label("トレイルライン").click();
    h.run();
    assert!(!h.state().animation_steps_shown().last().unwrap().trail);
    h.get_by_label("OK").click();
    h.run();
    assert!(!h.state().is_auto_exploding());
    assert_eq!(
        kmb_interchange::animation::steps(h.state().cur.work.doc().data()).len(),
        n + 1
    );
    h.get_by_label(&format!(
        "段 {}/{}: 分解(同時に 2 個): ボディ1・ボディ2",
        n + 1,
        n + 1
    ));
}

/// ストーリーボードの状態(自動分解の道具・引いている途中の段・時刻)は文書ごとに持つ。タブを
/// 替えると道具は文書と一緒にしまわれ、戻ると続きから使える(前の文書の段が今の文書に入らない)。
#[test]
fn the_auto_explode_tool_stays_with_its_document_when_the_tab_changes() {
    let (mut ws, _, _) = hinged();
    ws.switch_space(Space::Animation);
    let n = kmb_interchange::animation::steps(ws.cur.work.doc().data()).len();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("自動分解: 1 レベル").click();
    h.run();
    assert!(h.state().is_auto_exploding());
    // 別の文書のタブへ
    h.state_mut().new_tab();
    h.state_mut().switch_space(Space::Animation);
    h.run();
    assert!(!h.state().is_auto_exploding(), "道具は前の文書のもの");
    assert!(
        h.query_by_label("OK").is_none(),
        "今の文書には道具を出さない"
    );
    assert!(kmb_interchange::animation::steps(h.state().cur.work.doc().data()).is_empty());
    // 戻ると道具が残っていて、OK で前の文書に段が入る
    h.state_mut().switch_tab(0);
    h.run();
    assert!(h.state().is_auto_exploding());
    h.get_by_label("OK").click();
    h.run();
    assert_eq!(
        kmb_interchange::animation::steps(h.state().cur.work.doc().data()).len(),
        n + 1
    );
    h.state_mut().switch_tab(1);
    h.run();
    assert!(
        kmb_interchange::animation::steps(h.state().cur.work.doc().data()).is_empty(),
        "新しいタブの文書は空のまま"
    );
}

/// ストーリーボードのビュー・表示/非表示・外観: 段の帯の「記録」を
/// 入れると今の視点が始めのビューの段になり、視点を回して止めるとビューの段が入る。時刻を動かすと
/// 3D のビューの視点がその時刻の物になる。蓋を「表示/非表示」で隠す段と「外観」で赤にする段を
/// 足すと、時刻ごとに隠れ方と色が替わる(文書の外観は変えない)。設計に戻ると元の色と表示。
#[test]
fn the_storyboard_records_views_and_hides_and_recolors_parts() {
    use kmb_interchange::animation::StepKind;
    let (mut ws, lid, _) = hinged();
    ws.switch_space(Space::Animation);
    frame(&mut ws);
    // 1: 記録を入れる → 今の視点が始めのビューの段
    ws.set_view_recording(true);
    assert_eq!(
        kmb_interchange::animation::steps(ws.cur.work.doc().data()).len(),
        1,
        "{:?}",
        ws.message()
    );
    frame(&mut ws);
    let start = ws.view_pose();
    // 2: 視点を回して止める → しばらく止まるとビューの段が入る(回している間は入れない)
    ws.cur.camera.orbit(egui::vec2(-157.0, 0.0));
    frame(&mut ws);
    assert_eq!(
        kmb_interchange::animation::steps(ws.cur.work.doc().data()).len(),
        1,
        "止まるまで待つ"
    );
    std::thread::sleep(std::time::Duration::from_secs_f64(
        crate::animation_ui::RECORD_SETTLE + 0.05,
    ));
    frame(&mut ws);
    let steps = kmb_interchange::animation::steps(ws.cur.work.doc().data());
    assert_eq!(steps.len(), 2, "{:?}", ws.message());
    let StepKind::View { view: turned } = steps[1].kind else {
        panic!("{:?}", steps[1]);
    };
    assert!(
        (turned.yaw - start.yaw - 90.0).abs() < 0.1,
        "{} {}",
        turned.yaw,
        start.yaw
    );
    ws.set_view_recording(false);
    frame(&mut ws);
    assert_eq!(ws.animation_time(), 2.0, "足した段の終わりへ進む");
    // 3: 時刻を段 2 の真ん中へ → 視点は始めと回した後の真ん中
    ws.set_animation_time(1.5);
    frame(&mut ws);
    let mid = ws.view_pose();
    assert!(
        (mid.yaw - start.yaw - 45.0).abs() < 0.5,
        "{} {}",
        mid.yaw,
        start.yaw
    );
    // 初めへ戻すと始めの視点
    ws.set_animation_time(0.0);
    frame(&mut ws);
    assert!((ws.view_pose().yaw - start.yaw).abs() < 0.1);
    assert_eq!(
        ws.animation_step_label(&steps[1]).split(':').next(),
        Some("ビュー")
    );

    // 4: 蓋を選んで「表示/非表示」→ 隠す段。段の終わりでは蓋が見えない
    let shown = ws.cur.scene.bounds();
    ws.start_action_with(
        "anim.visibility",
        serde_json::json!({"targets": [{"kind": "body", "root": lid.0}]}),
    );
    let steps = kmb_interchange::animation::steps(ws.cur.work.doc().data());
    assert_eq!(steps.len(), 3, "{:?}", ws.message());
    assert!(matches!(
        steps[2].kind,
        StepKind::Visibility { visible: false, .. }
    ));
    assert_eq!(ws.animation_step_label(&steps[2]), "非表示: ボディ2");
    frame(&mut ws);
    let hidden = ws.cur.scene.bounds();
    assert!(
        hidden.1[0] < shown.1[0] - 20.0,
        "蓋(x = 40〜70)が消える: {hidden:?}"
    );
    // 段の前の時刻では見える
    ws.set_animation_time(1.0);
    frame(&mut ws);
    assert_eq!(ws.cur.scene.bounds().1[0], shown.1[0]);

    // 5: もう一度「表示/非表示」(今の時刻の逆で見せる段)、「外観」→ 蓋を赤に
    ws.set_animation_time(kmb_interchange::animation::duration(
        &kmb_interchange::animation::steps(ws.cur.work.doc().data()),
    ));
    ws.start_action_with(
        "anim.visibility",
        serde_json::json!({"targets": [{"kind": "body", "root": lid.0}], "time": ws.animation_time()}),
    );
    ws.start_action_with(
        "anim.appearance",
        serde_json::json!({"targets": [{"kind": "body", "root": lid.0}], "appearance": "赤"}),
    );
    let steps = kmb_interchange::animation::steps(ws.cur.work.doc().data());
    assert_eq!(steps.len(), 5, "{:?}", ws.message());
    assert!(matches!(
        steps[3].kind,
        StepKind::Visibility { visible: true, .. }
    ));
    frame(&mut ws);
    assert_eq!(ws.cur.scene.bounds().1[0], shown.1[0], "見せる段で戻る");
    let lid_last = ws
        .body_roots()
        .into_iter()
        .find(|(_, r)| *r == lid)
        .map(|(l, _)| l)
        .unwrap();
    let red = egui::Color32::from_rgb(200, 70, 64);
    assert_eq!(
        ws.cur.scene.body_colors().get(&lid_last),
        Some(&red),
        "段の終わりは赤"
    );
    // 外観の段の真ん中は、鋼の色と赤の間
    let t = kmb_interchange::animation::start_of(&steps, 4) + 0.5;
    ws.set_animation_time(t);
    frame(&mut ws);
    let c = ws.cur.scene.body_colors()[&lid_last];
    assert!(c != red && c.r() > c.g(), "{c:?}");
    assert!(
        kmb_look::appearance::appearance_of(ws.cur.work.doc().data(), lid).is_none(),
        "文書の外観は変えない"
    );

    // 6: 設計に戻ると元の色と表示
    ws.switch_space(Space::Design);
    frame(&mut ws);
    assert!(ws.cur.scene.body_colors().get(&lid_last).is_none());
    assert_eq!(ws.cur.scene.bounds().1[0], shown.1[0]);
}

/// 外観の段の欄: 段の帯の「替える外観」の一覧で選び直すと、段の外観が替わる(1 回の取り消し単位)。
#[test]
fn the_appearance_step_is_changed_from_the_storyboard_list() {
    use kmb_interchange::animation::{Step, StepKind, Target};
    let (mut ws, lid, _) = hinged();
    ws.switch_space(Space::Animation);
    assert!(ws.set_animation(
        "段",
        vec![Step::new(
            StepKind::Appearance {
                targets: vec![Target::Body { root: lid }],
                appearance: "赤".into(),
            },
            1.0,
        )],
    ));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("段 1/1: 外観: ボディ2 を 赤");
    h.get_by_label("赤 ▾").click();
    h.run();
    h.get_by_label("青").click();
    h.run();
    let steps = &kmb_interchange::animation::steps(h.state().cur.work.doc().data());
    assert!(
        matches!(&steps[0].kind, StepKind::Appearance { appearance, .. } if appearance == "青"),
        "{steps:?}"
    );
    // このデザイン内で作った外観も一覧で選べ、その色で描く
    assert!(
        h.state_mut()
            .looks_run(serde_json::json!({"duplicate": "青"}))
    );
    let own = kmb_look::appearance::of(h.state().cur.work.doc().data())
        .defs
        .last()
        .cloned()
        .expect("写した外観");
    h.run();
    h.get_by_label("青 ▾").click();
    h.run();
    h.get_by_label(&own.name).click();
    h.run();
    let steps = &kmb_interchange::animation::steps(h.state().cur.work.doc().data());
    assert!(
        matches!(&steps[0].kind, StepKind::Appearance { appearance, .. } if *appearance == own.name),
        "{steps:?}"
    );
    let ws = h.state_mut();
    ws.set_animation_time(1.0);
    frame(ws);
    let last = ws
        .body_roots()
        .into_iter()
        .find(|(_, r)| *r == lid)
        .unwrap()
        .0;
    let c = ws.cur.scene.body_colors()[&last];
    assert_eq!([c.r(), c.g(), c.b()], own.color, "作った外観の色");
}

/// モーション スタディ: 組立の「モーション スタディ」で窓が出て、
/// スタディが 1 つできる。蓋のジョイントにキーを置くと、再生の頭の位置へ蓋が動く(形は変えない)。
/// 再生で頭が進み、窓を閉じると元の位置に戻る。
#[test]
fn the_motion_study_window_moves_the_lid_from_its_keys_and_plays() {
    let (mut ws, _, joint) = hinged();
    let id = ws
        .action_for_role(&kmb_interchange::role::action::MOTION_STUDY)
        .unwrap();
    assert!(ws.start_action(&id).is_none());
    assert!(ws.cur.motion.open);
    assert_eq!(ws.motion_studies().len(), 1);
    assert!(ws.motion_run(serde_json::json!({"joint": joint.0, "step": 0, "value": 0.0})));
    assert!(ws.motion_run(serde_json::json!({"joint": joint.0, "step": 50, "value": -90.0})));
    ws.cur.motion.step = 50.0;
    let shape = ws.cur.work.doc().data().clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    assert!(
        h.state().cur.scene.is_posed(),
        "ステップ 50 で蓋が開いている"
    );
    h.get_by_label("ジョイント1 の帯");
    h.get_by_label("ジョイント1 のキーを消す");
    h.state_mut().cur.motion.step = 0.0;
    h.get_by_label("▶ 再生").click();
    h.run_steps(5);
    assert!(h.state().cur.motion.playing.is_some(), "再生している");
    assert!(h.state().cur.motion.step > 0.0);
    h.get_by_label("■ 止める").click();
    h.run();
    assert!(h.state().cur.motion.playing.is_none());
    // 窓を閉じると元の位置に戻る。文書は変えない
    h.state_mut().cur.motion.open = false;
    h.run();
    assert!(!h.state().cur.scene.is_posed());
    assert_eq!(h.state().cur.work.doc().data(), &shape);
}
