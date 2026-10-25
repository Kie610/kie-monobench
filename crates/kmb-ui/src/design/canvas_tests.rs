//! 下絵(キャンバス)の画面の試験: 絵を置くと 3D のビューに重なり、
//! キャリブレーションで 1 点目を動かさずに大きさが合い、目を閉じると消える。

use crate::Workspace;
use kmb_sketch_canvas::{CANVAS_CALIBRATE, CANVAS_INSERT};
use serde_json::json;

/// 横 40・縦 20 画素の絵(左半分は赤、右半分は青)をファイルに書く。
fn picture_file(name: &str) -> std::path::PathBuf {
    let mut px = Vec::new();
    for _ in 0..20 {
        for x in 0..40 {
            px.extend(if x < 20 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            });
        }
    }
    let dir = std::env::temp_dir().join(format!("kmb-canvas-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, kmb_format::png::write_rgba(40, 20, &px)).unwrap();
    path
}

#[test]
fn a_canvas_overlays_the_view_and_calibrates_about_the_first_point() {
    let mut ws = Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules());
    ws.wait();
    let path = picture_file("見本.png");
    let id = ws
        .run_action_args(CANVAS_INSERT, json!({"path": path.display().to_string()}))
        .unwrap_or_else(|| panic!("置けない: {:?}", ws.win.message));
    ws.wait();
    let ctx = egui::Context::default();
    let pics = ws.pictures(&ctx);
    assert_eq!(pics.len(), 1);
    // 既定の幅 100 mm、縦は絵の比で 50 mm。XY 平面の原点が中心
    assert_eq!(pics[0].corners[0], [-50.0, -25.0, 0.0]);
    assert_eq!(pics[0].corners[2], [50.0, 25.0, 0.0]);
    assert_eq!((pics[0].rgba.0, pics[0].rgba.1), (40, 20));
    assert_eq!(&pics[0].rgba.2[..4], &[255, 0, 0, 255]);
    assert!((pics[0].opacity - 0.5).abs() < 1e-6);

    // 左の端から中心までの 50 mm を、本当は 10 mm と合わせる: 絵は 0.2 倍、左の端の点は動かない
    ws.select(id);
    ws.run_action_args(
        CANVAS_CALIBRATE,
        json!({"from": [-50, 0, 0], "to": [0, 0, 0], "distance": 10}),
    )
    .or_else(|| ws.win.message.clone().map(|m| panic!("{m}")));
    ws.wait();
    let data = &ws.cur.work.doc().data().feature(id).unwrap().data;
    assert_eq!(data["width"], "20 mm", "{data}");
    assert_eq!(data["x"], "-40 mm", "{data}");
    let pics = ws.pictures(&ctx);
    assert!(
        (pics[0].corners[0][0] + 50.0).abs() < 1e-9,
        "{:?}",
        pics[0].corners
    );

    // 目を閉じると 3D のビューに出さない
    ws.set_all_hidden([id].into());
    assert!(ws.pictures(&ctx).is_empty());
    std::fs::remove_file(path).ok();
}
