//! 工具の形の絵。刃・首・シャンク・ホルダーを寸法どおりの縮尺で横から見た 2D に描き、主な寸法の札と
//! 縮尺の目盛りを添える(寸法の打ち間違いが見て分かるように)。

use jc_cam_tool::Tool;
use jc_core::LengthUnit;

/// 絵の部品(刃先からの高さ `h` と、中心からの半径 `r`。mm。寸法の札は文書の単位)。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shape {
    /// 刃の外形の右半分(下から上へ)
    pub flute: Vec<[f64; 2]>,
    /// 首(下の高さ、上の高さ、半径)
    pub neck: Option<(f64, f64, f64)>,
    /// シャンク(下の高さ、上の高さ、半径)。全長が空なら上の高さは仮
    pub shank: (f64, f64, f64),
    /// 全長が決まっているか
    pub known_length: bool,
    /// ホルダー(下の高さ、上の高さ、半径)
    pub holder: Option<(f64, f64, f64)>,
    /// 寸法の札(名前、下の高さ、上の高さ)
    pub dims: Vec<(String, f64, f64)>,
}

/// 長さ `v`(mm)を単位 `unit` の数で(末尾の 0 は落とす。インチなどは 1 桁多く)。
fn num(v: f64, unit: LengthUnit) -> String {
    let d = if unit.is_mm() { 3 } else { 4 };
    let s = format!("{:.d$}", unit.from_mm(v));
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 工具の絵の部品と寸法(寸法の札の数は単位 `unit`)。
pub(crate) fn shape(t: &Tool, unit: LengthUnit) -> Shape {
    let mm = |v| num(v, unit);
    let lcf = t.flute_length.max(0.01);
    // ねじ切りミルは歯の山の折れ目をそのまま(等しい間隔で拾うと尖った山の先を外す)
    let flute: Vec<[f64; 2]> = t.thread_outline().unwrap_or_else(|| {
        (0..=24)
            .map(|i| {
                // 先端の近くを細かく(球や円錐の形が出るように)
                let u = i as f64 / 24.0;
                let h = lcf * u * u;
                [h, t.outline_at(h).unwrap_or(t.radius())]
            })
            .collect()
    });
    let top_r = t.outline_at(lcf).unwrap_or(t.radius());
    let neck_top = t.neck_top();
    let neck = (neck_top > lcf + 1e-9).then_some((lcf, neck_top, top_r));
    let known_length = t.overall_length.is_some();
    // 全長が空なら、シャンクは刃と同じくらいの長さだけ仮に描く
    let end = t
        .overall_length
        .unwrap_or(neck_top + lcf.max(10.0))
        .max(neck_top);
    let shank = (neck_top, end, t.shank() / 2.0);
    let holder = match (t.stickout, t.holder_diameter) {
        (Some(s), Some(d)) => Some((s, s + (d * 0.8).max(5.0), d / 2.0)),
        _ => None,
    };
    let mut dims = vec![(format!("刃長 {}", mm(t.flute_length)), 0.0, lcf)];
    if neck.is_some() {
        dims.push((format!("首下長 {}", mm(neck_top)), 0.0, neck_top));
    }
    if let Some(s) = t.stickout {
        dims.push((format!("突き出し {}", mm(s)), 0.0, s));
    }
    if let Some(l) = t.overall_length {
        dims.push((format!("全長 {}", mm(l)), 0.0, l));
    }
    Shape {
        flute,
        neck,
        shank,
        known_length,
        holder,
        dims,
    }
}

/// 縮尺の目盛りの長さ(単位の数)。`max_mm` 以下の 1・2・5 の倍数のうち、いちばん長いもの。
pub(crate) fn scale_step(max_mm: f64) -> f64 {
    let mut best = 0.1;
    for e in -1..4 {
        for m in [1.0, 2.0, 5.0] {
            let v = m * 10f64.powi(e);
            if v <= max_mm {
                best = v;
            }
        }
    }
    best
}

/// 工具の絵を描く。
pub(crate) fn tool_picture(ui: &mut egui::Ui, t: &Tool, unit: LengthUnit, size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let v = ui.visuals();
    let line = v.text_color();
    let weak = v.weak_text_color();
    let s = shape(t, unit);
    let font = jc_ui::theme::prop(11.0);
    // 右に寸法の札を並べる幅、下に径と目盛りを書く高さ
    let label_w = 118.0;
    let (top_pad, bottom_pad) = (8.0, 34.0);
    let width_mm = [t.diameter, t.shank(), s.holder.map_or(0.0, |h| h.2 * 2.0)]
        .into_iter()
        .fold(0.0, f64::max)
        .max(0.1);
    let height_mm = s.shank.1.max(s.holder.map_or(0.0, |h| h.1)).max(0.1);
    let draw_w = (rect.width() - label_w - 16.0).max(20.0) as f64;
    let draw_h = (rect.height() - top_pad - bottom_pad).max(20.0) as f64;
    let k = (draw_w / width_mm).min(draw_h / height_mm);
    let cx = rect.left() + 8.0 + (draw_w / 2.0) as f32;
    let base = rect.bottom() - bottom_pad;
    let at = |r: f64, h: f64| egui::pos2(cx + (r * k) as f32, base - (h * k) as f32);
    let rect_of = |(h0, h1, r): (f64, f64, f64)| egui::Rect::from_two_pos(at(-r, h0), at(r, h1));
    let stroke = egui::Stroke::new(1.0, line);
    // ホルダー(奥に、薄く)
    if let Some(h) = s.holder {
        painter.rect_filled(
            rect_of(h),
            jc_ui::theme::corner(2.0),
            weak.gamma_multiply(0.35),
        );
        painter.rect_stroke(
            rect_of(h),
            jc_ui::theme::corner(2.0),
            stroke,
            egui::StrokeKind::Inside,
        );
    }
    // シャンク
    let inks = jc_ui::theme::inks_now();
    let shank_fill = inks.tool_shank.gamma_multiply(0.8);
    painter.rect_filled(rect_of(s.shank), jc_ui::theme::corner(0.0), shank_fill);
    if s.known_length {
        painter.rect_stroke(
            rect_of(s.shank),
            jc_ui::theme::corner(0.0),
            stroke,
            egui::StrokeKind::Inside,
        );
    } else {
        // 全長が空: 上の端を点線にする
        let (h0, h1, r) = s.shank;
        for x in [-r, r] {
            painter.line_segment([at(x, h0), at(x, h1)], stroke);
        }
        let dash = egui::Shape::dashed_line(&[at(-r, h1), at(r, h1)], stroke, 3.0, 3.0);
        painter.add(dash);
    }
    // 首
    let flute_fill = inks.tool_flute;
    if let Some(n) = s.neck {
        painter.rect_filled(
            rect_of(n),
            jc_ui::theme::corner(0.0),
            flute_fill.gamma_multiply(0.45),
        );
        painter.rect_stroke(
            rect_of(n),
            jc_ui::theme::corner(0.0),
            stroke,
            egui::StrokeKind::Inside,
        );
    }
    // 刃(右半分を左へ折り返した外形)
    let mut pts: Vec<egui::Pos2> = s.flute.iter().map(|[h, r]| at(*r, *h)).collect();
    pts.extend(s.flute.iter().rev().map(|[h, r]| at(-r, *h)));
    painter.add(egui::Shape::convex_polygon(pts, flute_fill, stroke));
    // 刃先の位置(原点)
    painter.circle_filled(at(0.0, 0.0), 2.5, line);
    // 寸法の札: 高さの線を右に並べる
    let x0 = cx + (width_mm / 2.0 * k) as f32 + 8.0;
    for (i, (name, h0, h1)) in s.dims.iter().enumerate() {
        let x = x0 + 6.0 * i as f32;
        let (p0, p1) = (egui::pos2(x, at(0.0, *h0).y), egui::pos2(x, at(0.0, *h1).y));
        painter.line_segment([p0, p1], egui::Stroke::new(1.0, weak));
        for p in [p0, p1] {
            painter.line_segment(
                [p - egui::vec2(3.0, 0.0), p + egui::vec2(3.0, 0.0)],
                egui::Stroke::new(1.0, weak),
            );
        }
        let tx = x0 + 6.0 * s.dims.len() as f32 + 4.0;
        painter.line_segment(
            [p1, egui::pos2(tx - 2.0, p1.y)],
            egui::Stroke::new(0.5, weak),
        );
        painter.text(
            egui::pos2(tx, p1.y),
            egui::Align2::LEFT_CENTER,
            name,
            font.clone(),
            line,
        );
    }
    // 径(下)
    let r = t.radius();
    let y = base + 10.0;
    painter.line_segment(
        [egui::pos2(at(-r, 0.0).x, y), egui::pos2(at(r, 0.0).x, y)],
        egui::Stroke::new(1.0, weak),
    );
    painter.text(
        egui::pos2(cx, y + 2.0),
        egui::Align2::CENTER_TOP,
        format!("Ø{}", num(t.diameter, unit)),
        font.clone(),
        line,
    );
    // 縮尺の目盛り(左下。文書の単位で切りのよい長さ)
    let step = scale_step(40.0 / k / unit.mm());
    let len = (step * unit.mm() * k) as f32;
    let (l0, yb) = (rect.left() + 4.0, rect.bottom() - 4.0);
    painter.line_segment(
        [egui::pos2(l0, yb), egui::pos2(l0 + len, yb)],
        egui::Stroke::new(1.5, line),
    );
    for x in [l0, l0 + len] {
        painter.line_segment(
            [egui::pos2(x, yb - 4.0), egui::pos2(x, yb)],
            egui::Stroke::new(1.5, line),
        );
    }
    painter.text(
        egui::pos2(l0 + len + 4.0, yb),
        egui::Align2::LEFT_BOTTOM,
        format!("{} {}", num(step * unit.mm(), unit), unit.name()),
        font,
        weak,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_cam_tool::{ToolKind, ToolLibrary};

    #[test]
    fn the_picture_follows_the_dimensions() {
        let mut t = ToolLibrary::starter().tools[1].clone(); // 平 3.175、刃長 17
        let s = shape(&t, LengthUnit::Mm);
        assert_eq!(s.flute.first(), Some(&[0.0, 3.175 / 2.0]));
        assert_eq!(s.flute.last(), Some(&[17.0, 3.175 / 2.0]));
        assert_eq!((s.neck, s.holder, s.known_length), (None, None, false));
        assert_eq!(s.dims.len(), 1);
        t.overall_length = Some(38.0);
        t.neck_length = Some(26.0);
        t.stickout = Some(26.0);
        t.holder_diameter = Some(16.0);
        let s = shape(&t, LengthUnit::Mm);
        assert_eq!(s.neck, Some((17.0, 26.0, 3.175 / 2.0)));
        assert_eq!(s.shank, (26.0, 38.0, 3.175 / 2.0));
        assert_eq!(s.holder.map(|h| (h.0, h.2)), Some((26.0, 8.0)));
        let names: Vec<&str> = s.dims.iter().map(|d| d.0.as_str()).collect();
        assert_eq!(names, ["刃長 17", "首下長 26", "突き出し 26", "全長 38"]);
        // ブルノーズは先端で角 R の分だけ細い
        t.kind = ToolKind::BullNose;
        t.corner_radius = Some(0.5);
        let s = shape(&t, LengthUnit::Mm);
        assert!((s.flute[0][1] - (3.175 / 2.0 - 0.5)).abs() < 1e-9);
        // V は先がとがる
        t.kind = ToolKind::VBit;
        t.tip_angle = 90.0;
        assert_eq!(shape(&t, LengthUnit::Mm).flute[0], [0.0, 0.0]);
        // ねじ切りミルは歯の山の先が刃の径、その上は首の径
        t.kind = ToolKind::ThreadMill;
        t.tip_angle = 60.0;
        t.thread = Some(jc_cam_tool::ThreadForm {
            teeth: 1,
            pitch: None,
            neck_diameter: Some(2.0),
        });
        let s = shape(&t, LengthUnit::Mm);
        let widest = s.flute.iter().map(|p| p[1]).fold(0.0, f64::max);
        assert!((widest - 3.175 / 2.0).abs() < 1e-9, "{widest}");
        assert_eq!(s.flute.last().unwrap()[1], 1.0);
        assert_eq!(scale_step(7.0), 5.0);
        assert_eq!(scale_step(0.35), 0.2);
        assert_eq!(scale_step(40.0), 20.0);
    }
}
