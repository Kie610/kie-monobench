//! 3D のビューの通しの試験(見え方・投影・断面・選び方・視点の操作・書き出し)。

use crate::tests::{on_truck, reg};
use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::{reference_document, sample_document};

#[test]
fn reference_document_opens_in_3d() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(reference_document(), None, reg()),
        );
    h.run();
    assert_eq!(h.state().report().error_count(), 0);
    h.get_by_label("全体表示");
    // 選んだだけの編集パネルの「体積を計測」を押す
    h.state_mut().win.layout.panel_always = true;
    h.run();
    h.get_all_by_label("体積を計測").next().unwrap().click();
    h.run();
    assert!(
        h.state().message().unwrap().contains(": 体積 "),
        "{:?}",
        h.state().message()
    );
}

#[test]
fn edges_hidden_behind_the_body_are_not_picked() {
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let all: Vec<(FeatureId, kmb_core::EdgeInfo)> = ws
        .scene_edges()
        .iter()
        .flat_map(|(f, l)| l.iter().map(move |e| (*f, e.clone())))
        .collect();
    let (mut hit, mut hidden) = (0, 0);
    for (f, e) in &all {
        let mid = [0, 1, 2].map(|i| ((e.a[i] + e.b[i]) / 2.0) as f32);
        let p = ws.cur.camera.project(mid, rect).0;
        let got = view3d::edge_at(
            &ws.cur.scene,
            &ws.cur.camera,
            rect,
            p,
            &ws.cur.edge_cache.1,
            false,
        );
        let me = kmb_core::EdgePick {
            feature: *f,
            faces: e.faces.clone(),
        };
        if got.as_ref() == Some(&me) {
            hit += 1;
        } else if got.is_none() {
            hidden += 1;
        }
    }
    assert!(hit > 4, "見えている辺は拾える: {hit}/{}", all.len());
    assert!(hidden > 0, "裏の辺は拾わない");
}

#[test]
fn round_edges_of_the_boss_and_holes_can_be_picked_and_drawn() {
    // 円の辺も辺の候補にする(平面どうしのまっすぐな辺だけだと、円の辺のクリックで選びが消える)
    if on_truck() {
        kmb_contract::skip!("truck は円の辺を返さない");
    }
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let arcs: Vec<(FeatureId, kmb_core::EdgeInfo)> = ws
        .scene_edges()
        .iter()
        .flat_map(|(f, l)| l.iter().map(move |e| (*f, e.clone())))
        .filter(|(_, e)| e.arc.is_some())
        .collect();
    assert!(arcs.len() >= 4, "ボスと穴の縁: {}", arcs.len());
    let mut picked = None;
    for (f, e) in &arcs {
        let me = kmb_core::EdgePick {
            feature: *f,
            faces: e.faces.clone(),
        };
        for q in view3d::edge_polyline(e) {
            let p = ws.cur.camera.project(q.map(|v| v as f32), rect).0;
            let got = view3d::edge_at(
                &ws.cur.scene,
                &ws.cur.camera,
                rect,
                p,
                &ws.cur.edge_cache.1,
                false,
            );
            if got.as_ref() == Some(&me) {
                picked = Some(me.clone());
                break;
            }
        }
    }
    let pick = picked.expect("見えている円の辺は、円の上をクリックすると拾える");
    // 選んだ円の辺は、両端を結ぶ 1 本でなく円に沿った折れ線で描く
    assert!(view3d::edge_ends(&ws.cur.edge_cache.1, &pick).len() > 8);
}

#[test]
fn sketches_show_in_the_view_until_a_feature_uses_them() {
    // スケッチを終了すると 3D のビューに出て、目のボタンも効く
    let part = kmb_samples::reference_part();
    let sketch = part.plate_sketch;
    let mut ws = Workspace::new(part.doc, None, reg());
    // 押し出しに使ったスケッチは隠れている
    assert!(ws.is_hidden(sketch));
    assert!(ws.sketch_lines().is_empty());
    // 目で出すと、板の輪郭(長方形の 4 本と中央の穴)が出る
    ws.set_hidden(sketch, false);
    assert_eq!(ws.sketch_lines().len(), 5);
    // 目の開け閉めは文書の変更(変更ありになり、取り消せる)
    assert!(ws.is_modified());
    ws.undo();
    assert!(ws.is_hidden(sketch), "取り消すと目も戻る");
    // 使っている機能を消しても、道具は目を出し直さない
    ws.set_hidden(sketch, true);
    let top = part.top_sketch;
    assert!(ws.is_hidden(top));
    let users: Vec<FeatureId> = ws
        .document()
        .data()
        .features
        .iter()
        .skip_while(|f| f.id != top)
        .skip(1)
        .map(|f| f.id)
        .collect();
    let tx = kmb_core::Transaction::new(
        "削除",
        users
            .iter()
            .rev()
            .map(|id| kmb_core::Op::RemoveFeature { id: *id })
            .collect(),
    );
    ws.apply(tx);
    ws.wait();
    assert!(ws.is_hidden(top), "使う機能が無くなっても出し直さない");
    assert!(ws.is_hidden(sketch), "利用者が隠したものはそのまま");
    assert!(ws.sketch_lines().is_empty());
}

#[test]
fn faces_are_named_by_their_side_and_the_feature_that_made_them() {
    // 「選んだ面: f8:end」のような内部の名前を出さない
    let part = kmb_samples::reference_part();
    let plate = part.plate;
    let ws = Workspace::new(part.doc, None, reg());
    let name = ws.document().data().feature(plate).unwrap().name.clone();
    assert_eq!(
        ws.face_label(&format!("f{}:end", plate.0)),
        format!("上の面({name})")
    );
    assert_eq!(
        ws.face_label(&format!("f{}:start", plate.0)),
        format!("下の面({name})")
    );
    assert!(
        ws.face_label(&format!("f{}:side:e5", plate.0))
            .starts_with("横の面")
    );
}

#[test]
fn the_eye_in_the_browser_hides_and_shows_a_body() {
    let part = kmb_samples::reference_part();
    let body = part.hole_pattern;
    let mut ws = Workspace::new(part.doc, None, reg());
    // ボディの行の名前は「ボディ1」(最後の機能の名前ではない)
    let name = "ボディ1".to_string();
    let sketch = ws
        .document()
        .data()
        .feature(part.plate_sketch)
        .unwrap()
        .name
        .clone();
    ws.select(part.plate_sketch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    // スケッチの行にも目がある(押し出しに使ったスケッチは、はじめから隠れている)
    h.get_by_label(&format!("{sketch} を出す"));
    h.get_by_label(&format!("{name} を隠す")).click();
    h.run();
    assert!(h.state().is_hidden(body));
    assert!(h.state().cur.scene.is_empty());
    assert_eq!(
        h.state().selected(),
        Some(part.plate_sketch),
        "目を押しても行は選ばない"
    );
    h.get_by_label(&format!("{name} を出す")).click();
    h.run();
    assert!(!h.state().is_hidden(body));
    assert!(!h.state().cur.scene.is_empty());
}

#[test]
fn perspective_rays_pass_the_pivot_and_points_round_trip() {
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    for perspective in [false, true] {
        let mut cam = view3d::Camera::default();
        cam.perspective = perspective;
        cam.target = [5.0, -3.0, 2.0];
        cam.zoom = 3.0;
        cam.set_view(-50.0, 25.0);
        // 画面の中心の視線は回す中心を通る
        let (o, d) = cam.ray(rect.center(), rect);
        let to = [0, 1, 2].map(|i| cam.target[i] - o[i]);
        let along = to[0] * d[0] + to[1] * d[1] + to[2] * d[2];
        let off: f32 = (0..3)
            .map(|i| (to[i] - d[i] * along).powi(2))
            .sum::<f32>()
            .sqrt();
        assert!(off < 1e-3, "{perspective}: {off}");
        // 写して戻すと同じ点(手前も奥も)
        for p in [[0.0, 0.0, 0.0], [12.0, 7.0, -4.0], [-20.0, 15.0, 9.0]] {
            let (q, z) = cam.project(p, rect);
            let back = cam.unproject(q, z, rect);
            let err: f32 = (0..3).map(|i| (back[i] - p[i]).abs()).fold(0.0, f32::max);
            assert!(err < 1e-3, "{perspective}: {p:?} → {back:?}");
            // その点は、その位置の視線の上にある
            let (o, d) = cam.ray(q, rect);
            let w = [0, 1, 2].map(|i| p[i] - o[i]);
            let t = w[0] * d[0] + w[1] * d[1] + w[2] * d[2];
            let off: f32 = (0..3)
                .map(|i| (w[i] - d[i] * t).powi(2))
                .sum::<f32>()
                .sqrt();
            assert!(off < 1e-3, "{perspective}: {p:?} は視線から {off}");
        }
        // 回す中心の平面の上の点は、拡大してもカーソルの下に残る
        let at = egui::pos2(600.0, 150.0);
        let before = cam.unproject(at, 0.0, rect);
        cam.zoom_at(1.7, at, rect);
        let after = cam.project(before, rect).0;
        assert!(after.distance(at) < 1e-2, "{perspective}: {after:?}");
    }
    // 透視投影では、遠くの物ほど小さく写る(平行投影では同じ)
    let mut cam = view3d::Camera::default();
    cam.set_view(-90.0, 0.0);
    let width = |cam: &view3d::Camera, y: f32| {
        let a = cam.project([0.0, y, 0.0], rect).0;
        let b = cam.project([10.0, y, 0.0], rect).0;
        a.distance(b)
    };
    assert!((width(&cam, -20.0) - width(&cam, 20.0)).abs() < 1e-3);
    cam.perspective = true;
    assert!(
        width(&cam, -20.0) > width(&cam, 20.0) * 1.05,
        "手前が大きい"
    );
}

#[test]
fn picking_and_fitting_work_in_perspective() {
    let part = kmb_samples::reference_part();
    let (plate, body) = (part.plate, part.hole_pattern);
    let mut ws = Workspace::new(part.doc, None, reg());
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.perspective = true;
    ws.cur.camera.fit(&ws.cur.scene, rect);
    // 全体が画面に収まる
    for b in &ws.cur.scene.bodies {
        for p in &b.mesh.positions {
            assert!(rect.contains(ws.cur.camera.project(*p, rect).0), "{p:?}");
        }
    }
    // 斜めから見て、上の面を選ぶ。当たった点は上の面の高さ
    let p = ws.cur.camera.project([20.0, 30.0, 10.0], rect).0;
    let (pick, at) = view3d::picks_at(&ws.cur.scene, &ws.cur.camera, rect, p)
        .into_iter()
        .next()
        .expect("当たる");
    assert_eq!(pick.feature, body);
    assert_eq!(pick.face, format!("f{}:end", plate.0));
    assert!(
        (at[2] - 10.0).abs() < 0.05 && (at[0] - 20.0).abs() < 0.1 && (at[1] - 30.0).abs() < 0.1,
        "{at:?}"
    );
    // 範囲で選ぶと、見えている面だけ入る(下の面は入らない)
    let faces = view3d::picks_in(&ws.cur.scene, &ws.cur.camera, rect, rect, false);
    assert!(faces.iter().any(|f| f.face == format!("f{}:end", plate.0)));
    assert!(
        faces
            .iter()
            .all(|f| f.face != format!("f{}:start", plate.0))
    );
    // 上の面の縁の真ん中で、その辺を拾う
    let edges = ws.scene_edges().to_vec();
    let mut hit = 0;
    for (f, list) in &edges {
        for e in list {
            let mid = [0, 1, 2].map(|i| ((e.a[i] + e.b[i]) / 2.0) as f32);
            if (mid[2] - 10.0).abs() > 1e-3 || (mid[0] - 30.0).abs() > 1e-3 {
                continue;
            }
            let q = ws.cur.camera.project(mid, rect).0;
            let got = view3d::edge_at(&ws.cur.scene, &ws.cur.camera, rect, q, &edges, false);
            let me = kmb_core::EdgePick {
                feature: *f,
                faces: e.faces.clone(),
            };
            hit += usize::from(got == Some(me));
        }
    }
    assert!(hit >= 1, "上の面の縁(x=30)の辺を拾える");
    // 寄りすぎて視点が部品の中に入っても止まらない(手前は切り落とす)
    for _ in 0..40 {
        ws.cur.camera.zoom_at(1.25, rect.center(), rect);
    }
    let img = view3d::render(
        &ws.cur.scene,
        &ws.cur.camera,
        [200, 150],
        0.25,
        &[],
        None,
        &view3d::Colors::from_look(&crate::theme::Look::default()),
        Shading::ShadedEdges,
    );
    assert_eq!(img.size, [200, 150]);
    let _ = view3d::picks_at(&ws.cur.scene, &ws.cur.camera, rect, rect.center());
}

#[test]
fn shading_modes_draw_faces_and_edges_as_asked() {
    let part = kmb_samples::reference_part();
    let ws = Workspace::new(part.doc, None, reg());
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(320.0, 240.0));
    let mut cam = ws.cur.camera;
    cam.fit(&ws.cur.scene, rect);
    let colors = view3d::Colors::from_look(&crate::theme::Look::default());
    let count = |shading: Shading, perspective: bool| {
        let mut cam = cam;
        cam.perspective = perspective;
        cam.fit(&ws.cur.scene, rect);
        let img = view3d::render(
            &ws.cur.scene,
            &cam,
            [320, 240],
            1.0,
            &[],
            None,
            &colors,
            shading,
        );
        let back = view3d::backdrop(&colors, 320, 240);
        let filled = img.pixels.iter().zip(&back).filter(|(c, b)| c != b).count();
        let edges = img.pixels.iter().filter(|c| **c == colors.line).count();
        (filled, edges)
    };
    for perspective in [false, true] {
        let (all, e1) = count(Shading::ShadedEdges, perspective);
        let (shaded, e2) = count(Shading::Shaded, perspective);
        let (wire, e3) = count(Shading::Wireframe, perspective);
        assert!(e1 > 100, "陰影と辺: 辺がある {e1}");
        assert_eq!(e2, 0, "陰影だけ: 辺を描かない");
        assert!(
            (all as i64 - shaded as i64).abs() < all as i64 / 20,
            "面の広さは同じ"
        );
        assert_eq!(wire, e3, "線だけ: 面を塗らない");
        assert!(e3 > e1, "線だけ: 隠れた辺も描く {e3} > {e1}");
        assert!(wire < shaded / 2);
    }
}

#[test]
fn hidden_lines_are_dashed_and_follow_their_strength() {
    // U19: 陰影と隠れ線は、陰影と辺に隠れた辺の破線を足す。濃さ 0 % なら陰影と辺と同じ
    let part = kmb_samples::reference_part();
    let ws = Workspace::new(part.doc, None, reg());
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(320.0, 240.0));
    let mut cam = ws.cur.camera;
    cam.fit(&ws.cur.scene, rect);
    let colors = view3d::Colors::from_look(&crate::theme::Look::default());
    let draw = |shading: Shading| {
        view3d::render(
            &ws.cur.scene,
            &cam,
            [320, 240],
            1.0,
            &[],
            None,
            &colors,
            shading,
        )
        .pixels
    };
    let edges = draw(Shading::ShadedEdges);
    let hidden = draw(Shading::ShadedHidden);
    let wire = draw(Shading::Wireframe);
    let diff =
        |a: &[egui::Color32], b: &[egui::Color32]| a.iter().zip(b).filter(|(x, y)| x != y).count();
    let dashed = diff(&edges, &hidden);
    assert!(dashed > 50, "隠れた辺が出る {dashed}");
    // 破線なので、線だけ(隠れた辺も実線)が足す画素より少ない
    let solid = diff(
        &edges,
        &wire
            .iter()
            .zip(&edges)
            .map(|(w, e)| if *w == colors.line { *w } else { *e })
            .collect::<Vec<_>>(),
    );
    assert!(dashed < solid, "破線は実線より少ない {dashed} < {solid}");
    crate::view3d_render::set_current(&crate::RenderSettings {
        hidden_dim: 0,
        ..Default::default()
    });
    let faint = draw(Shading::ShadedHidden);
    crate::view3d_render::set_current(&crate::RenderSettings::default());
    assert_eq!(diff(&edges, &faint), 0, "濃さ 0 % は隠れた辺を描かない");
}

#[test]
fn sketch_lines_behind_a_body_are_faint_dashes() {
    // Q6 v1: 本体の奥のスケッチの線は薄い点線、手前の線は今の色のまま
    let part = kmb_samples::reference_part();
    let ws = Workspace::new(part.doc, None, reg());
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(320.0, 240.0));
    let mut cam = ws.cur.camera;
    cam.fit(&ws.cur.scene, rect);
    let colors = view3d::Colors::from_look(&crate::theme::Look::default());
    let (lo, hi) = ws.cur.scene.bounds();
    let mid: [f32; 3] = std::array::from_fn(|i| (lo[i] + hi[i]) / 2.0);
    let (right, _, eye) = cam.basis_pub();
    let reach = (0..3).map(|i| hi[i] - lo[i]).fold(0.0, f32::max);
    // 本体の真ん中を通り、視線の向きに本体の大きさだけ手前・奥へずらした、画面の左右の線
    let line = |toward: f32| {
        let at = |s: f32| -> [f32; 3] {
            std::array::from_fn(|i| mid[i] + eye[i] * toward * reach + right[i] * s * reach * 0.3)
        };
        view3d::Behind {
            lines: vec![([at(-1.0), at(1.0)], colors.sketch, view3d::SKETCH_WIDTH)],
            tris: vec![],
        }
    };
    let draw = |b: &view3d::Behind| {
        view3d::render_guided(
            &ws.cur.scene,
            &cam,
            [320, 240],
            1.0,
            &[],
            None,
            &colors,
            Shading::ShadedEdges,
            view3d::Guides::default(),
            &[],
            b,
        )
        .pixels
    };
    let plain = draw(&view3d::Behind::default());
    let front = draw(&line(1.0));
    let back = draw(&line(-1.0));
    let full = |p: &[egui::Color32]| p.iter().filter(|c| **c == colors.sketch).count();
    let changed =
        |a: &[egui::Color32], b: &[egui::Color32]| a.iter().zip(b).filter(|(x, y)| x != y).count();
    assert!(full(&front) > 100, "手前の線は今の色 {}", full(&front));
    assert_eq!(full(&back), 0, "奥の線は今の色で描かない");
    let dashed = changed(&plain, &back);
    assert!(dashed > 20, "奥の線は点線で出る {dashed}");
    assert!(dashed < full(&front), "点線は実線より少ない {dashed}");
}

#[test]
fn a_section_hides_one_side_and_paints_the_cut_without_changing_the_shape() {
    // M48: 断面は平面の片側を描かず、選ぶこともできない。形(網)は変えない
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(320.0, 240.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let colors = view3d::Colors::from_look(&crate::theme::Look::default());
    let tris: usize = ws
        .cur
        .scene
        .bodies
        .iter()
        .map(|b| b.mesh.triangles.len())
        .sum();
    let count = |scene: &view3d::Scene, cam: &view3d::Camera| {
        let img = view3d::render(
            scene,
            cam,
            [320, 240],
            1.0,
            &[],
            None,
            &colors,
            Shading::Shaded,
        );
        let back = view3d::backdrop(&colors, 320, 240);
        let filled = img.pixels.iter().zip(&back).filter(|(c, b)| c != b).count();
        // 切り口の地の色と縞の色(断面の無い時は全部のボディで同じ色)
        let caps: Vec<_> = (0..scene.bodies.len())
            .flat_map(|i| {
                let c = scene.cap_color(i, &colors);
                [c, view3d::hatch_color(c)]
            })
            .collect();
        let cap = img.pixels.iter().filter(|c| caps.contains(c)).count();
        (filled, cap)
    };
    let (whole, cap0) = count(&ws.cur.scene, &ws.cur.camera);
    assert_eq!(cap0, 0);
    // 何も選ばずに開くと、ZX 平面で形の真ん中を切る
    ws.toggle_section();
    assert!(ws.is_sectioning());
    let s = ws.cur.section.clone().unwrap();
    assert_eq!(s.plane, crate::section_ui::SectionPlane::Origin(2));
    ws.cur.scene.section = Some(s.section());
    let (cut, cap) = count(&ws.cur.scene, &ws.cur.camera);
    assert!(cut < whole, "片側を描かない {cut} < {whole}");
    assert!(cap > 200, "切り口を蓋の色で塗る {cap}");
    // 切った側は選べない: 手前(Y が小さい側)の面の点を当てても、奥の物しか当たらない
    let (lo, hi) = ws.cur.scene.bounds();
    let front = [(lo[0] + hi[0]) / 2.0, lo[1], (lo[2] + hi[2]) / 2.0];
    let q = ws.cur.camera.project(front, rect).0;
    for (_, at) in view3d::picks_at(&ws.cur.scene, &ws.cur.camera, rect, q) {
        assert!(!ws.cur.scene.section.unwrap().removes(at), "{at:?}");
    }
    let tris2: usize = ws
        .cur
        .scene
        .bodies
        .iter()
        .map(|b| b.mesh.triangles.len())
        .sum();
    assert_eq!(tris, tris2, "網は変えない");
    // 閉じると元の見え方
    ws.toggle_section();
    assert!(!ws.is_sectioning());
    ws.cur.scene.section = None;
    assert_eq!(count(&ws.cur.scene, &ws.cur.camera).0, whole);
}

#[test]
fn the_section_button_opens_the_panel_and_esc_closes_it() {
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.clear_selection();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_all_by_label("断面解析").next().unwrap().click();
    h.run();
    h.run();
    assert!(h.state().is_sectioning());
    assert!(h.state().cur.scene.section.is_some(), "3D のビューに渡る");
    h.get_by_label("YZ 平面").click();
    h.run();
    h.run();
    let s = h.state().cur.section.clone().unwrap();
    assert_eq!(s.plane, crate::section_ui::SectionPlane::Origin(1));
    h.get_by_label("反対側を残す").click();
    h.run();
    h.run();
    let flipped = h.state().cur.section.clone().unwrap();
    assert!(flipped.flip);
    assert_eq!(flipped.section().normal, [-1.0, 0.0, 0.0]);
    h.key_press(egui::Key::Escape);
    h.run();
    h.run();
    assert!(!h.state().is_sectioning());
    assert!(h.state().cur.scene.section.is_none(), "閉じると元の見え方");
}

#[test]
fn window_and_crossing_boxes_pick_differently() {
    // U30: 左から右は全部入った物、右から左はかかった物
    let part = kmb_samples::reference_part();
    let ws = Workspace::new(part.doc, None, reg());
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let mut cam = ws.cur.camera;
    cam.fit(&ws.cur.scene, rect);
    // 左半分だけの範囲: 上の面は半分しか入らない
    let area = egui::Rect::from_min_max(rect.min, egui::pos2(400.0, 600.0));
    let window = view3d::picks_in(&ws.cur.scene, &cam, rect, area, false);
    let crossing = view3d::picks_in(&ws.cur.scene, &cam, rect, area, true);
    let top = format!("f{}:end", part.plate.0);
    assert!(
        window.iter().all(|f| f.face != top),
        "全部入らない面は入らない"
    );
    assert!(crossing.iter().any(|f| f.face == top), "かかった面は入る");
    assert!(
        window.iter().all(|f| crossing.contains(f)),
        "全部入った物はかかった物でもある"
    );
    assert!(crossing.len() > window.len());
    // ボディも同じ。全体を囲めばどちらでも入る
    assert!(view3d::bodies_in(&ws.cur.scene, &cam, rect, area, false).is_empty());
    assert_eq!(
        view3d::bodies_in(&ws.cur.scene, &cam, rect, area, true).len(),
        1
    );
    assert_eq!(
        view3d::bodies_in(&ws.cur.scene, &cam, rect, rect, false).len(),
        1
    );
    // 線と三角形の当たり(範囲をまたぐだけでもかかる)
    let r = egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(20.0, 20.0));
    assert!(view3d::segment_touches(
        r,
        egui::pos2(0.0, 15.0),
        egui::pos2(30.0, 15.0)
    ));
    assert!(!view3d::segment_touches(
        r,
        egui::pos2(0.0, 0.0),
        egui::pos2(30.0, 5.0)
    ));
    assert!(view3d::triangle_touches(
        r,
        [
            egui::pos2(0.0, 0.0),
            egui::pos2(40.0, 0.0),
            egui::pos2(0.0, 40.0)
        ]
    ));
    assert!(!view3d::triangle_touches(
        r,
        [
            egui::pos2(30.0, 30.0),
            egui::pos2(40.0, 30.0),
            egui::pos2(30.0, 40.0)
        ]
    ));
}

#[test]
fn the_mesh_detail_setting_rebuilds_the_view_mesh() {
    // U26: 網の細かさを替えると 3D のビューの網を作り直す(細かいほど三角形が多い)
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    let tris = |ws: &Workspace| -> usize {
        ws.cur
            .scene
            .bodies
            .iter()
            .map(|b| b.mesh.triangles.len())
            .sum()
    };
    let standard = tris(&ws);
    assert!(
        ws.cur
            .scene
            .set_tolerance(ws.cur.work.kernel().as_ref(), 0.01)
    );
    let fine = tris(&ws);
    assert!(
        !ws.cur
            .scene
            .set_tolerance(ws.cur.work.kernel().as_ref(), 0.01),
        "同じ値なら作り直さない"
    );
    assert!(
        ws.cur
            .scene
            .set_tolerance(ws.cur.work.kernel().as_ref(), 0.1)
    );
    let coarse = tris(&ws);
    assert!(
        coarse <= standard && standard < fine,
        "{coarse} {standard} {fine}"
    );
}

#[test]
fn the_view_style_button_switches_shading_and_projection() {
    // 表示の形と投影は、ビューの下の帯の「表示 ▾」の 1 か所で選ぶ(右上の「表示設定」はやめた)
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.clear_selection();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    assert_eq!(h.state().win.layout.shading, Shading::ShadedEdges);
    assert!(!h.state().cur.camera.perspective);
    h.get_by_label("表示").click();
    h.run();
    h.get_by_label("ワイヤフレーム").click();
    h.run();
    assert_eq!(h.state().win.layout.shading, Shading::Wireframe);
    h.get_by_label("表示").click();
    h.run();
    h.get_by_label("透視投影").click();
    h.run();
    h.run();
    assert_eq!(
        h.state().win.layout.projection,
        crate::Projection::Perspective
    );
    assert!(
        h.state().cur.camera.perspective,
        "ビューの視点も透視投影になる"
    );
    // 自動(N1a): 斜めから見る間は透視投影、軸の向き(上から)では平行投影
    h.get_by_label("表示").click();
    h.run();
    h.get_by_label("自動").click();
    h.run();
    h.run();
    assert_eq!(h.state().win.layout.projection, crate::Projection::Auto);
    assert!(h.state().cur.camera.perspective, "斜めの向きは透視投影");
    h.state_mut().cur.camera.set_view(-90.0, 89.9);
    h.run();
    assert!(!h.state().cur.camera.perspective, "上から見る時は平行投影");
    h.state_mut().cur.camera.set_view(0.0, 0.0);
    h.run();
    assert!(!h.state().cur.camera.perspective, "右から見る時も平行投影");
    h.state_mut().cur.camera.set_view(10.0, 0.0);
    h.run();
    assert!(
        h.state().cur.camera.perspective,
        "軸から外れると透視投影に戻る"
    );
}

#[test]
fn origin_planes_float_off_the_origin_and_are_picked_where_drawn() {
    // まとめてレビュー R19: 原点の平面が原点と軸にくっついていた。少し浮かせ、
    // 描く四角とクリックで選ぶ範囲(スケッチを追加の平面の候補)をそろえる
    let mut ws = Workspace::new(sample_document(), None, reg());
    ws.wait();
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    ws.start_action(kmb_sketch::ADD_SKETCH);
    let h = ws.cur.gesture.as_ref().unwrap().current().unwrap().clone();
    let s = view3d::origin_size(&ws.cur.camera, rect);
    let gap = kmb_core::Plane::ORIGIN_GAP as f32 * s;
    assert!(gap > 0.0);
    for (name, quad) in view3d::base_planes(s) {
        // 原点に触れる角は無い(どの角も 2 つの座標が隙間より遠い)
        for q in quad {
            assert_eq!(
                q.iter().filter(|c| **c >= gap * 0.999).count(),
                2,
                "{name}: {q:?}"
            );
        }
        let c = h.choices.iter().find(|c| c.label == name).expect(name);
        let picked = crate::gesture::outline(c, &ws.cur.camera, rect);
        for (p, q) in picked[0].iter().zip(quad) {
            for k in 0..3 {
                assert!(
                    (p[k] as f32 - q[k]).abs() < 1e-3 * s,
                    "{name}: {p:?} と {q:?}"
                );
            }
        }
    }
}

/// CPU の描画の時間を、利用者の画面と同じ大きさ(4K・倍率 2)で測る。
/// `cargo test -p kmb-ui --release -- --ignored cpu_render_time --nocapture`
#[test]
#[ignore = "速さを測る道具(--release で手で流す)"]
fn cpu_render_time() {
    let ws = Workspace::new(reference_document(), None, reg());
    let size = [3000usize, 1800];
    let ppp = 2.0;
    let rect = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(size[0] as f32 / ppp, size[1] as f32 / ppp),
    );
    let colors = view3d::Colors::from_look(&crate::theme::Look::default());
    let tris: usize = ws
        .cur
        .scene
        .bodies
        .iter()
        .map(|b| b.mesh.triangles.len())
        .sum();
    println!("三角形 {tris}");
    for (name, yaw, pitch) in [
        ("等角", -60.0, 30.0),
        ("上", -90.0, 90.0),
        ("前", -90.0, 0.0),
    ] {
        for grid in [false, true] {
            let mut cam = ws.cur.camera;
            cam.set_view(yaw, pitch);
            cam.fit(&ws.cur.scene, rect);
            let guides = view3d::Guides { grid, axes: grid };
            let t = std::time::Instant::now();
            let n = 5;
            for _ in 0..n {
                let _ = view3d::render_guided(
                    &ws.cur.scene,
                    &cam,
                    size,
                    ppp,
                    &[],
                    None,
                    &colors,
                    crate::layout::Shading::ShadedEdges,
                    guides,
                    &[],
                    &view3d::Behind::default(),
                );
            }
            println!(
                "{name} 格子{}: {:.1} ms",
                if grid { "あり" } else { "なし" },
                t.elapsed().as_secs_f64() * 1000.0 / n as f64
            );
        }
    }
}

#[test]
fn the_current_view_is_written_as_a_dxf() {
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, reg());
    ws.wait();
    let dir = std::env::temp_dir().join(format!("kmb-view-dxf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (id, name) in [
        ("io.export_view_dxf", "見える線.dxf"),
        ("io.export_view_dxf_hidden", "隠れ線も.dxf"),
    ] {
        let path = dir.join(name);
        ws.run_action_with(id, Some(path.clone()));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{id}: {e} {:?}", ws.message()));
        assert!(text.contains("VISIBLE"), "{id}");
        // 等角に近い既定の視点からは、板の裏の稜線が隠れる
        let hidden = text.contains("\n  8\nHIDDEN\n");
        assert_eq!(hidden, id == "io.export_view_dxf_hidden", "{id}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pressing_the_iso_button_again_looks_from_the_other_side() {
    // 同じ視点のボタンをもう一度押すと反対側から見る。名前も変わる
    // (上・前・右のボタンは軸の玉に替えた。等角だけボタンで残す)
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(reference_document(), None, reg()),
        );
    h.run();
    for name in ["上", "前", "右"] {
        assert!(
            h.query_by_label(name).is_none(),
            "{name} のボタンは無くした"
        );
    }
    h.state_mut().cur.camera.set_view(0.0, 0.0);
    h.run();
    h.get_by_label("等角").click();
    h.run();
    assert!(h.state().cur.camera.is_view(-60.0, 30.0), "等角から見る");
    h.get_by_label("等角").click();
    h.run();
    assert!(
        h.state().cur.camera.is_view(120.0, 30.0),
        "もう一度押すと後ろの等角"
    );
    assert!(h.query_by_label("等角").is_none(), "名前が変わる");
    h.get_by_label("等角(後)").click();
    h.run();
    assert!(h.state().cur.camera.is_view(-60.0, 30.0), "押すと表へ戻る");
}

#[test]
fn clicking_an_axis_ball_looks_from_that_axis_and_again_from_the_other_side() {
    // 軸の表示: 玉を押すとその軸の先から見る。同じ玉をもう一度押すと反対側
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(reference_document(), None, reg()),
        );
    h.run();
    for (ball, yaw, pitch, back_yaw, back_pitch) in [
        ("上から(+Z)", -90.0, 89.9, -90.0, -89.9),
        ("前から(-Y)", -90.0, 0.0, 90.0, 0.0),
        ("右から(+X)", 0.0, 0.0, 180.0, 0.0),
        ("下から(-Z)", -90.0, -89.9, -90.0, 89.9),
        ("後ろから(+Y)", 90.0, 0.0, -90.0, 0.0),
        ("左から(-X)", 180.0, 0.0, 0.0, 0.0),
    ] {
        // 斜めから見ている時は 6 つの玉が全部見える(同じ所に重ならない)
        h.state_mut().cur.camera.set_view(-60.0, 30.0);
        h.run();
        h.get_by_label(ball).click();
        h.run();
        assert!(h.state().cur.camera.is_view(yaw, pitch), "{ball}");
        // 真正面から見ている軸の玉は、手前の玉がいちばん上に重なっているのでそのまま押せる
        h.get_by_label(ball).click();
        h.run();
        assert!(
            h.state().cur.camera.is_view(back_yaw, back_pitch),
            "{ball} をもう一度押すと反対側から"
        );
    }
}

#[test]
fn dragging_the_axis_display_turns_the_view() {
    // 玉のない所を引くと回す
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(reference_document(), None, reg()),
        );
    h.state_mut().cur.camera.set_view(-60.0, 30.0);
    h.run();
    let disc = h.get_by_label("視点の軸").rect();
    // 玉のない所(斜めから見ている時、右下の玉と玉の間)から引く
    let from = disc.center() + egui::vec2(14.0, 24.0);
    h.drag_at(from);
    h.run();
    h.hover_at(from + egui::vec2(40.0, 0.0));
    h.run();
    h.hover_at(from + egui::vec2(80.0, 10.0));
    h.run();
    h.drop_at(from + egui::vec2(80.0, 10.0));
    h.run();
    assert!(
        !h.state().cur.camera.is_view(-60.0, 30.0),
        "引くと向きが変わる"
    );
}

#[test]
fn the_extrude_being_edited_shows_its_tool_in_the_colour_of_its_operation() {
    // 押し出しのプレビューを新しいボディ・結合・切り取りで色分けする
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    // 新しいボディは灰色、結合は青、切り取りは赤(失敗の知らせの色とは別)、交差は黄
    let inks = ws.win.look.pal.inks;
    for (id, op, color) in [
        (part.plate, kmb_core::ToolOp::New, ws.win.look.pal.ink2),
        (part.boss, kmb_core::ToolOp::Join, inks.ghost_join_edge),
        (part.pocket, kmb_core::ToolOp::Cut, inks.ghost_cut_edge),
    ] {
        let t = ws
            .output(id, kmb_core::TOOL_PREVIEW)
            .and_then(<kmb_core::ToolPreview as kmb_core::Payload>::from_value)
            .expect("押し出しは道具の線を出す");
        assert_eq!(t.op, op);
        assert!(t.lines.len() >= 2, "上と下の輪郭と縦の線");
        assert!(!t.faces.is_empty(), "道具の立体の面も出す");
        // 直している間も押し出す輪郭を青く塗れるよう、閉じた輪郭を出す
        assert!(!t.profile.is_empty());
        assert!(
            t.profile
                .iter()
                .all(|l| l.len() >= 4 && l.first() == l.last())
        );
        assert_eq!(ws.win.look.tool_op(op), color);
    }
    assert_ne!(inks.ghost_cut_edge, ws.win.look.pal.err);
    assert_eq!(
        ws.win.look.tool_op(kmb_core::ToolOp::Intersect),
        ws.win.look.pal.warn
    );
    // 向きの矢印: 輪郭の重心から端まで 1 本。重心は輪郭の外形の箱の中で、輪郭の面の上
    let t = ws
        .output(part.pocket, kmb_core::TOOL_PREVIEW)
        .and_then(<kmb_core::ToolPreview as kmb_core::Payload>::from_value)
        .unwrap();
    assert_eq!(t.arrows.len(), 1);
    let pts: Vec<[f64; 3]> = t.profile.iter().flatten().copied().collect();
    let c = t.arrows[0][0];
    for i in 0..3 {
        let lo = pts.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min);
        let hi = pts.iter().map(|p| p[i]).fold(f64::NEG_INFINITY, f64::max);
        assert!(c[i] >= lo - 1e-9 && c[i] <= hi + 1e-9, "{c:?}");
    }
    // 両側(別の距離)なら 2 本で、それぞれの向きと長さが分かる
    let mut doc = kmb_samples::reference_part().doc;
    let mut data = doc.data().feature(part.pocket).unwrap().data.clone();
    data["direction"] = serde_json::json!("two_sides");
    data["distance"] = serde_json::json!("7 mm");
    data["distance2"] = serde_json::json!("3 mm");
    doc.apply(kmb_core::Transaction::new(
        "両側にする",
        vec![kmb_core::Op::SetFeatureData {
            id: part.pocket,
            data,
        }],
    ))
    .unwrap();
    let two = Workspace::new(doc, None, reg());
    let t = two
        .output(part.pocket, kmb_core::TOOL_PREVIEW)
        .and_then(<kmb_core::ToolPreview as kmb_core::Payload>::from_value)
        .expect("両側の押し出しも道具を出す");
    assert_eq!(t.arrows.len(), 2);
    let v = |[a, b]: [[f64; 3]; 2]| [0, 1, 2].map(|i| b[i] - a[i]);
    let len = |d: [f64; 3]| d.iter().map(|x| x * x).sum::<f64>().sqrt();
    let (a, b) = (v(t.arrows[0]), v(t.arrows[1]));
    assert!((len(a) - 7.0).abs() < 1e-9 && (len(b) - 3.0).abs() < 1e-9);
    assert!(
        a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>() < 0.0,
        "逆向き"
    );
    // 直すのはダブルクリック・編集の時だけ(選んだだけでは描かない)
    ws.select(part.pocket);
    assert_eq!(ws.cur.editing, None);
    ws.open_edit(part.pocket);
    assert_eq!(ws.cur.editing, Some(part.pocket));
}

#[test]
fn the_tool_of_an_extrude_is_drawn_as_a_solid_in_the_colour_of_its_operation() {
    // 順に確かめる:
    // - 道具の色は見本の画面から測った色
    // - ボディがあれば、前へ引くと結合、逆へ引くと切り取りの色。無ければどちらも新しいボディ
    //   (元 dragging_an_extrude_back_into_the_body_is_drawn_as_a_cut)
    // - 道具は立体として塗り、結合は普段の色、切り取り・交差は透かして重ねる
    //   (元 the_tool_of_an_extrude_is_drawn_as_a_coloured_solid)

    // 見本の画面(暗い地)から測った色: 交差は黄土色でなく純な黄、切り取りは赤、輪郭は青
    let y = view3d::GHOST_INTERSECT;
    assert_eq!(y.r(), y.g(), "黄は赤と緑が同じ(黄土色にしない)");
    assert!(y.b() < 30);
    let r = view3d::GHOST_CUT;
    assert!(r.r() > 200 && r.g() < 80 && r.g() == r.b());
    let b = view3d::PROFILE_BLUE;
    assert!(b.b() > 180 && b.r() < 30 && b.g() < b.b() / 2);
    // 表と裏の 2 枚が重なるので、重ねた後の濃さを切り取り 5 割強・交差 4 割強に合わせる
    let twice = |a: u8| 1.0 - (1.0 - f32::from(a) / 255.0).powi(2);
    assert!((0.5..0.62).contains(&twice(view3d::GHOST_CUT_ALPHA)));
    assert!((0.38..0.5).contains(&twice(view3d::GHOST_INTERSECT_ALPHA)));
    // 暗い地 (42,42,42) の上の交差の黄は (127,128,26) から数段以内(陰は 0.9〜1.0)
    let a = twice(view3d::GHOST_INTERSECT_ALPHA);
    let over = |c: u8, k: f32| 42.0 + a * (f32::from(c) * k - 42.0);
    assert!((over(255, 0.9)..=over(255, 1.0)).contains(&127.0));
    assert!((over(0, 1.0) - 26.0).abs() < 4.0);

    // ボディが無ければ、どちらへ引いても新しいボディ
    let doc = sample_document();
    let sketch = doc.data().features[0].id;
    let mut ws = Workspace::new(doc, None, reg());
    ws.select(sketch);
    ws.start_action_with("cad.extrude", serde_json::json!({"regions": null}));
    let h = ws
        .cur
        .gesture
        .as_ref()
        .and_then(|g| g.current())
        .expect("高さの手順に入る");
    assert!(matches!(h.gesture, kmb_core::Gesture::Distance { .. }));
    assert_eq!(
        (h.op_for(5.0), h.op_for(-5.0)),
        (Some(kmb_core::ToolOp::New), Some(kmb_core::ToolOp::New))
    );

    // 枠だけでなく立体として塗る。結合は普段の色、切り取りは赤く透かして形に重ね、
    // 交差は黄色く透かして形に重ねる(残る所は形の普段の色が透けて見える)
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(kmb_samples::reference_part().doc, None, reg());
    let colors = view3d::Colors::from_look(&ws.win.look);
    ws.select(part.top_sketch);
    ws.start_action_with("cad.extrude", serde_json::json!({"regions": null}));
    // 押し出しの高さの手順: ボディがあれば、前へ引くと結合、逆へ引くと切り取りの色で描く
    let h = ws
        .cur
        .gesture
        .as_ref()
        .and_then(|g| g.current())
        .expect("高さの手順に入る");
    assert!(matches!(h.gesture, kmb_core::Gesture::Distance { .. }));
    assert_eq!(
        (h.op_for(5.0), h.op_for(-5.0)),
        (Some(kmb_core::ToolOp::Join), Some(kmb_core::ToolOp::Cut))
    );
    let volume = |g: &view3d::Ghost| {
        g.tris
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (a.map(f64::from), b.map(f64::from), c.map(f64::from));
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum::<f64>()
    };
    let at = |ws: &mut Workspace, v: f64| {
        ws.cur.gesture.as_mut().expect("高さの手順").live = Some(serde_json::json!(v));
        let g = ws.tool_ghosts(&colors);
        assert_eq!(g.len(), 1, "{v}");
        g.into_iter().next().unwrap()
    };
    // 結合は青く透かす(新しいボディの灰色の立体と見分ける)
    let join = at(&mut ws, 5.0);
    assert_eq!(join.layer, view3d::GhostLayer::Over);
    assert_eq!(join.edge_color, ws.win.look.pal.inks.ghost_join_edge);
    assert!(
        join.color.a() < 255 && join.color.b() > join.color.r(),
        "青く透かす"
    );
    let up = volume(&join);
    assert!(up > 0.0, "外向きの面で閉じた立体 {up}");
    let cut = at(&mut ws, -5.0);
    assert_eq!(cut.layer, view3d::GhostLayer::Over);
    assert_eq!(cut.edge_color, ws.win.look.pal.inks.ghost_cut_edge);
    assert!(cut.color.a() < 255, "透かす");
    assert!(
        (volume(&cut) - up).abs() < 1e-3 * up,
        "逆へ引いても同じ大きさ"
    );
    // 値が無い間は描かない
    ws.cur.gesture.as_mut().unwrap().live = None;
    assert!(ws.tool_ghosts(&colors).is_empty());
    ws.cur.gesture = None;

    // 直している押し出し: 結合は形そのものが結果なので塗らない。交差は黄色く透かして重ねる
    let mut doc = part.doc;
    let mut data = doc.data().feature(part.pocket).unwrap().data.clone();
    data["operation"] = serde_json::json!("intersect");
    doc.apply(kmb_core::Transaction::new(
        "交差にする",
        vec![kmb_core::Op::SetFeatureData {
            id: part.pocket,
            data,
        }],
    ))
    .unwrap();
    let mut ws = Workspace::new(doc, None, reg());
    ws.open_edit(part.boss);
    assert!(ws.tool_ghosts(&colors).is_empty(), "結合は塗らない");
    ws.open_edit(part.pocket);
    let g = ws.tool_ghosts(&colors);
    assert_eq!(g.len(), 1, "{:?}", ws.message());
    assert_eq!(g[0].layer, view3d::GhostLayer::Over);
    assert_eq!(g[0].edge_color, view3d::GHOST_INTERSECT_EDGE);
    assert!(g[0].color.a() < 255, "透かす");
    assert!(volume(&g[0]).abs() > 0.0);
}

/// 材料も外観も無いボディを画面内のレンダリングで描く材質は、書き出すレンダリング(kmb-io-render)と
/// 同じ既定の材料(鋼)。既定の灰色の塗装で描くと、同じ文書が画面と書き出しで違って見える。
#[test]
fn an_unmaterialed_body_renders_as_the_default_material_on_screen_too() {
    let part = kmb_samples::reference_part();
    let ws = Workspace::new(part.doc, None, reg());
    let colors = view3d::Colors::from_look(&crate::theme::Look::default());
    let scene = ws.cur.scene.render_scene(&colors);
    let steel = kmb_render::Look::named(None, Some(kmb_look::material::DEFAULT_MATERIAL));
    assert!(!scene.objects.is_empty());
    for (b, o) in ws.cur.scene.bodies.iter().zip(&scene.objects) {
        if b.look.is_none() && b.tint.is_none() {
            assert_eq!(o.look.finish, steel.finish);
            assert_eq!(o.look.color, steel.color);
        }
    }
}

/// 物理材料か外観に鋼を選んでも、何も付けていないボディと同じ見た目のまま(既定は鋼の見た目)。
/// 鋼を選んでも、色と質感を明示した物としては描かない(そうすると金属の質感に替わり、ボディごとの
/// 明るさの見分けも消える)。鋼でない材料には色と質感が付く。
#[test]
fn choosing_steel_keeps_the_default_look() {
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    let (_, root) = ws.body_roots()[0];
    let plain = |ws: &Workspace| {
        (
            ws.cur.scene.bodies[0].tint,
            ws.cur.scene.bodies[0].look.clone(),
        )
    };
    let before = plain(&ws);
    assert_eq!(before, (None, None), "何も付けていないボディは既定の見た目");
    assert!(ws.set_body_material(root, "鋼"));
    assert_eq!(plain(&ws), before, "物理材料を鋼にした");
    assert!(ws.set_body_appearance(root, "鋼"));
    assert_eq!(plain(&ws), before, "外観も鋼にした");
    assert!(ws.set_body_appearance(root, ""));
    assert!(ws.set_body_material(root, "アルミ"));
    let (tint, look) = plain(&ws);
    assert!(
        tint.is_some() && look.is_some(),
        "鋼でない材料は色と質感が付く"
    );
}

/// スケッチを閉じた後も、線や円に使われていない単独の点(穴の位置など)を
/// 3D のビューに出す(1 点の線として渡し、ビューは点として描く)。
#[test]
fn lone_sketch_points_show_in_the_3d_view() {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let a = s.fixed_point(0.0, 0.0);
    let c = s.fixed_point(10.0, 0.0);
    s.line(a, c);
    // 固定した点は原点の印として数えないので、寸法で決めた点にする
    s.point_from(a, "4 mm", "7 mm", [4.0, 7.0]);
    b.feature(kmb_sketch::SKETCH, "点", s.build());
    let mut ws = Workspace::new(b.finish("点"), None, reg());
    ws.wait();
    let lines = ws.sketch_lines();
    let dots: Vec<&Vec<[f64; 3]>> = lines
        .iter()
        .map(|(p, _)| p)
        .filter(|p| p.len() == 1)
        .collect();
    assert_eq!(dots.len(), 1, "単独の点が 1 つ出る: {lines:?}");
    let p = dots[0][0];
    assert!((p[0] - 4.0).abs() < 1e-6 && (p[1] - 7.0).abs() < 1e-6 && p[2].abs() < 1e-6);
    assert_eq!(lines.len(), 2, "線 1 本と点 1 つ");
}

#[test]
fn the_shape_stays_put_when_the_panel_comes_and_goes() {
    let part = kmb_samples::reference_part();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(part.doc, None, reg()),
        );
    h.run();
    h.run();
    let at = |h: &Harness<'static, Workspace>| {
        let ws = h.state();
        (
            ws.cur.camera.project([0.0, 0.0, 0.0], ws.win.view_rect).0,
            ws.win.view_rect,
        )
    };
    let (p0, r0) = at(&h);
    // 作り始めると右に編集パネルが出て、3D のビューの右の端が動く。形は同じ所に残る
    h.state_mut()
        .add_described(kmb_cad_hole::HOLE)
        .expect("足せる");
    h.run();
    h.run();
    let (p1, r1) = at(&h);
    assert!(
        r0.right() - r1.right() > 200.0,
        "右の端が動く: {r0:?} {r1:?}"
    );
    assert!((p1 - p0).length() < 1.0, "原点が跳ぶ: {p0:?} → {p1:?}");
    // OK で閉じても戻らない
    h.state_mut().finish_edit();
    h.run();
    h.run();
    let (p2, r2) = at(&h);
    assert!((r2.right() - r0.right()).abs() < 1.0);
    assert!((p2 - p0).length() < 1.0, "原点が跳ぶ: {p0:?} → {p2:?}");
}
