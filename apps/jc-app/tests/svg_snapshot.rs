//! 画面の描画内容を SVG に書き出す。GPU のない環境で見た目を確かめるための補助。
//!
//! `JC_SVG_OUT=out.svg cargo test -p jc-app --test svg_snapshot -- --ignored`

use egui::epaint::{ColorMode, Shape};
use egui::{Color32, Pos2};
use egui_kittest::Harness;
use jc_samples::{
    reference_cam_3d_document, reference_cam_document, reference_document, sample_document,
};
use jc_ui::Workspace;
use std::fmt::Write;

fn color(c: Color32) -> String {
    format!(
        "rgba({},{},{},{:.3})",
        c.r(),
        c.g(),
        c.b(),
        c.a() as f32 / 255.0
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn points(p: &[Pos2]) -> String {
    p.iter()
        .map(|p| format!("{:.1},{:.1}", p.x, p.y))
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_shape(out: &mut String, shape: &Shape) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| write_shape(out, s)),
        Shape::LineSegment { points: p, stroke } => {
            let _ = writeln!(
                out,
                r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="{}"/>"#,
                p[0].x,
                p[0].y,
                p[1].x,
                p[1].y,
                color(stroke.color),
                stroke.width
            );
        }
        Shape::Path(p) => {
            let stroke = match &p.stroke.color {
                ColorMode::Solid(c) => color(*c),
                _ => "none".into(),
            };
            let tag = if p.closed { "polygon" } else { "polyline" };
            let fill = if p.closed {
                color(p.fill)
            } else {
                "none".into()
            };
            let _ = writeln!(
                out,
                r#"<{tag} points="{}" fill="{fill}" stroke="{stroke}" stroke-width="{}"/>"#,
                points(&p.points),
                p.stroke.width
            );
        }
        Shape::Circle(c) => {
            let _ = writeln!(
                out,
                r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="{}" stroke="{}" stroke-width="{}"/>"#,
                c.center.x,
                c.center.y,
                c.radius,
                color(c.fill),
                color(c.stroke.color),
                c.stroke.width
            );
        }
        Shape::Rect(r) => {
            let _ = writeln!(
                out,
                r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{}" stroke="{}" stroke-width="{}"/>"#,
                r.rect.min.x,
                r.rect.min.y,
                r.rect.width(),
                r.rect.height(),
                color(r.fill),
                color(r.stroke.color),
                r.stroke.width
            );
        }
        Shape::Text(t) => {
            let c = t.override_text_color.unwrap_or(t.fallback_color);
            let c = t.galley.job.sections.first().map_or(c, |s| {
                if s.format.color == Color32::PLACEHOLDER {
                    c
                } else {
                    s.format.color
                }
            });
            let size = t
                .galley
                .job
                .sections
                .first()
                .map_or(13.0, |s| s.format.font_id.size);
            for (row, line) in t.galley.rows.iter().zip(t.galley.text().split('\n')) {
                let p = t.pos + row.pos.to_vec2();
                let _ = writeln!(
                    out,
                    r#"<text x="{:.1}" y="{:.1}" font-size="{size}" fill="{}" dominant-baseline="hanging">{}</text>"#,
                    p.x,
                    p.y,
                    color(c),
                    escape(line)
                );
            }
        }
        Shape::Mesh(m) => {
            for t in m.indices.chunks(3) {
                let v = |i: u32| &m.vertices[i as usize];
                let c = v(t[0]).color;
                let _ = writeln!(
                    out,
                    r#"<polygon points="{}" fill="{}" stroke="{}" stroke-width="0.3"/>"#,
                    points(&[v(t[0]).pos, v(t[1]).pos, v(t[2]).pos]),
                    color(c),
                    color(c)
                );
            }
        }
        _ => {}
    }
}

fn snapshot(path: &str, doc: jc_core::Document) {
    let size = egui::vec2(1100.0, 650.0);
    let mut h = Harness::builder()
        .with_size(size)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws_of(doc, None, reg()));
    h.run();
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" font-family="WenQuanYi Zen Hei, sans-serif">"#,
        size.x, size.y
    );
    svg.push('\n');
    for clipped in &h.output().shapes {
        write_shape(&mut svg, &clipped.shape);
    }
    svg.push_str("</svg>\n");
    std::fs::write(path, svg).unwrap();
}

/// `JC_SVG_OUT` にスケッチの画面、`JC_SVG_OUT3D` に基準部品の 3D の画面、
/// `JC_BMP_OUT3D` に 3D の絵、`JC_BMP_OUTCAM` に加工の経路を重ねた絵、
/// `JC_BMP_OUTCAM3D` に面取りとフィレットを掛けた部品と走査線の経路の絵を書く。
#[test]
#[ignore = "道具: JC_SVG_OUT などに画面の絵を書く"]
fn write_svg() {
    if let Ok(path) = std::env::var("JC_SVG_OUT") {
        snapshot(&path, sample_document());
    }
    if let Ok(path) = std::env::var("JC_SVG_OUT3D") {
        snapshot(&path, reference_document());
    }
    if let Ok(path) = std::env::var("JC_BMP_OUT3D") {
        let ws = ws_of(reference_document(), None, reg());
        write_bmp(&path, &ws.render_3d([800, 600]));
    }
    if let Ok(path) = std::env::var("JC_BMP_OUTCAM") {
        // 基準部品の加工のセットアップを選び、経路を重ねる
        let mut ws = ws_of(reference_cam_document(), None, reg());
        let setup = ws
            .document()
            .data()
            .features
            .iter()
            .find(|f| f.type_id == "cam.setup@1")
            .map(|f| f.id)
            .unwrap();
        ws.select(setup);
        write_bmp(&path, &ws.render_3d([800, 600]));
    }
    if let Ok(path) = std::env::var("JC_BMP_OUTCAM3D") {
        let mut ws = ws_of(reference_cam_3d_document(), None, reg());
        let raster = ws
            .document()
            .data()
            .features
            .iter()
            .find(|f| f.type_id == "cam.raster3d@1")
            .map(|f| f.id)
            .unwrap();
        ws.select(raster);
        write_bmp(&path, &ws.render_3d([800, 600]));
    }
}

/// 24 ビットの BMP を書く(依存を増やさずに絵を確かめるため)。
fn write_bmp(path: &str, img: &egui::ColorImage) {
    let [w, h] = img.size;
    let row = (w * 3).div_ceil(4) * 4;
    let mut out = Vec::with_capacity(54 + row * h);
    let size = (54 + row * h) as u32;
    out.extend(b"BM");
    out.extend(size.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(54u32.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend((w as i32).to_le_bytes());
    out.extend((h as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend([0u8; 24]);
    for y in (0..h).rev() {
        let start = out.len();
        for x in 0..w {
            let c = img.pixels[y * w + x];
            out.extend([c.b(), c.g(), c.r()]);
        }
        out.resize(start + row, 0);
    }
    std::fs::write(path, out).unwrap();
}

/// 画面のアプリと同じ編集器を組み込んだ作業場。
fn ws_of(
    doc: jc_core::Document,
    path: Option<std::path::PathBuf>,
    m: jc_core::Modules,
) -> Workspace {
    Workspace::new(doc, path, m)
        .with_editor(jc_ui_sketch::SketchEditor::new())
        .with_editor(jc_ui_tools::ToolsEditor::new())
}

fn reg() -> jc_core::Modules {
    jc_bundle::modules()
}
