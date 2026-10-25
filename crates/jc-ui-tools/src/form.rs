//! 1 本の工具の欄: 形と寸法、ねじ切りの欄、材料ごとの切削条件、形の絵。

use super::{kind_label, picture};
use jc_cam_tool::{Cutting, MATERIALS, Preset, Tool, ToolKind, chip_load_range};
use jc_core::{FeatureId, LengthUnit};

/// 1 本の工具の欄と形の絵。長さは文書の単位 `unit` で見せて打たせる(保存は mm)。番号を変えたら真。
pub(super) fn tool_form(
    ui: &mut egui::Ui,
    feature: FeatureId,
    t: &mut Tool,
    numbers: &[u32],
    unit: LengthUnit,
) -> bool {
    // 欄の横に絵を置けるだけの幅があれば横に、なければ欄の下に出す
    let renumbered = if ui.available_width() >= 500.0 {
        ui.horizontal_top(|ui| {
            let r = ui
                .vertical(|ui| shape_fields(ui, feature, t, numbers, unit))
                .inner;
            picture_box(ui, t, unit, egui::vec2(240.0, 300.0));
            r
        })
        .inner
    } else {
        let r = shape_fields(ui, feature, t, numbers, unit);
        let w = ui.available_width().clamp(200.0, 320.0);
        picture_box(ui, t, unit, egui::vec2(w, 240.0));
        r
    };
    presets_form(ui, feature, t, unit);
    renumbered
}

/// 工具の形の絵と、寸法の誤り(打ち間違いが見て分かるように)。
fn picture_box(ui: &mut egui::Ui, t: &Tool, unit: LengthUnit, size: egui::Vec2) {
    ui.vertical(|ui| {
        egui::Frame::group(ui.style()).show(ui, |ui| picture::tool_picture(ui, t, unit, size));
        if let Err(e) = t.check() {
            ui.colored_label(ui.visuals().warn_fg_color, e);
        }
    });
}

/// 形の欄。番号を変えたら真。
fn shape_fields(
    ui: &mut egui::Ui,
    feature: FeatureId,
    t: &mut Tool,
    numbers: &[u32],
    unit: LengthUnit,
) -> bool {
    let mut renumbered = false;
    egui::Grid::new(("tool_form", feature))
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("工具番号")
                .on_hover_text("手動の工具交換で機械が求める番号");
            let mut n = t.number;
            if ui.add(egui::DragValue::new(&mut n).range(1..=99)).changed() && !numbers.contains(&n)
            {
                t.number = n;
                renumbered = true;
            }
            ui.end_row();
            ui.label("名前");
            ui.text_edit_singleline(&mut t.name);
            ui.end_row();
            ui.label("種類");
            egui::ComboBox::from_id_salt(("tool_form_kind", feature))
                .selected_text(kind_label(t.kind))
                .show_ui(ui, |ui| {
                    let was = t.kind;
                    for k in ToolKind::ALL {
                        ui.selectable_value(&mut t.kind, k, kind_label(k));
                    }
                    if t.kind != was {
                        new_kind_defaults(t, was);
                    }
                });
            ui.end_row();
            mm(ui, unit, "刃の径", &mut t.diameter);
            mm(ui, unit, "刃長", &mut t.flute_length);
            ui.label("刃数");
            ui.add(egui::DragValue::new(&mut t.flutes).range(1..=8));
            ui.end_row();
            ui.label("シャンク径")
                .on_hover_text("つかむ所の径。コレットに合うかを調べる");
            let mut shank = t.shank();
            if length(ui, unit, &mut shank, 0.1..=50.0, 0.05, true).changed() {
                t.shank_diameter = Some(shank);
            }
            ui.end_row();
            if t.kind == ToolKind::BullNose {
                let mut rc = t.corner_radius.unwrap_or(default_corner(t));
                ui.label("角 R")
                    .on_hover_text("刃の先の角の丸みの半径。刃の半径以下");
                if length(ui, unit, &mut rc, 0.01..=50.0, 0.01, true).changed()
                    || t.corner_radius.is_none()
                {
                    t.corner_radius = Some(rc);
                }
                ui.end_row();
            }
            if matches!(
                t.kind,
                ToolKind::Drill
                    | ToolKind::VBit
                    | ToolKind::TaperEnd
                    | ToolKind::TaperBall
                    | ToolKind::ThreadMill
            ) {
                let (label, hint) = match t.kind {
                    ToolKind::ThreadMill => {
                        ("山の角度", "歯の山の角度(全角)。メートルねじは 60 度")
                    }
                    ToolKind::TaperEnd => ("側面の開き(全角)", "片側の角度の 2 倍"),
                    ToolKind::TaperBall => ("テーパー角(全角)", "側面の開き。片側の角度の 2 倍"),
                    _ => ("先端角", "刃先の角度(全角)"),
                };
                ui.label(label).on_hover_text(hint);
                ui.add(
                    egui::DragValue::new(&mut t.tip_angle)
                        .range(1.0..=179.0)
                        .suffix(" 度"),
                );
                ui.end_row();
            }
            if t.kind == ToolKind::TaperEnd {
                let mut tip = t.tip_diameter.unwrap_or((t.diameter / 4.0).max(0.1));
                ui.label("先の径").on_hover_text("先の平らな所の径");
                length(ui, unit, &mut tip, 0.01..=50.0, 0.01, true);
                t.tip_diameter = Some(tip);
                ui.end_row();
            }
            if t.kind == ToolKind::TaperBall {
                let mut tip = t.tip_diameter.unwrap_or(default_ball(t));
                ui.label("先の球の径")
                    .on_hover_text("先の丸い所の径(球の半径の 2 倍)。側面はこの球に接する");
                if length(ui, unit, &mut tip, 0.01..=50.0, 0.01, true).changed()
                    || t.tip_diameter.is_none()
                {
                    t.tip_diameter = Some(tip);
                }
                ui.end_row();
            }
            if t.kind == ToolKind::ThreadMill {
                thread_form(ui, t, unit);
            }
            opt_mm(
                ui,
                unit,
                "突き出し",
                "コレットの端から刃先まで。入れるとシミュレーションでホルダーの当たりを調べる",
                &mut t.stickout,
                t.flute_length,
            );
            opt_mm(
                ui,
                unit,
                "ホルダーの径",
                "コレットナットの径。突き出しと組で使う",
                &mut t.holder_diameter,
                16.0,
            );
            let long = t.neck_top().max(t.stickout.unwrap_or(0.0));
            opt_mm(
                ui,
                unit,
                "全長",
                "刃先からシャンクの端まで。絵の長さに使う",
                &mut t.overall_length,
                (long + 10.0).max(38.0),
            );
            let neck = t.flute_length + 5.0;
            opt_mm(
                ui,
                unit,
                "首下長",
                "刃先からシャンクの始まりまで。刃より上は刃の径の首とみなし、\
                 シミュレーションで首とシャンクの当たりを調べる",
                &mut t.neck_length,
                neck,
            );
        });
    renumbered
}

/// 種類を変えた時、新しい種類に要る値を入れる(前の種類から移る値は残す)。
fn new_kind_defaults(t: &mut Tool, was: ToolKind) {
    match t.kind {
        ToolKind::BullNose if t.corner_radius.is_none() => {
            t.corner_radius = Some(default_corner(t));
        }
        ToolKind::TaperBall => {
            if t.tip_diameter.is_none() || was != ToolKind::TaperEnd {
                t.tip_diameter = Some(default_ball(t));
            }
            // 角度のない種類から移った時は、よくある 10°(片側 5°)にする
            if !matches!(was, ToolKind::TaperEnd | ToolKind::VBit) {
                t.tip_angle = 10.0;
            }
        }
        // ねじ切りミルは 1 山、山の角度 60 度(メートルねじ)から
        ToolKind::ThreadMill => {
            t.tip_angle = 60.0;
            if t.thread.is_none() {
                t.thread = Some(Default::default());
            }
        }
        _ => {}
    }
}

/// ねじ切りミルの歯の段の数・歯の間隔・首の径。
fn thread_form(ui: &mut egui::Ui, t: &mut Tool, unit: LengthUnit) {
    let mut f = t.thread.unwrap_or_default();
    ui.label("歯の段の数")
        .on_hover_text("1 なら 1 山(どのピッチも切れる)。2 以上は、決まった間隔で歯が並んだ工具");
    ui.add(egui::DragValue::new(&mut f.teeth).range(1..=40));
    ui.end_row();
    if f.teeth > 1 {
        let mut p = f.pitch.unwrap_or(1.0);
        ui.label("歯の間隔(ピッチ)")
            .on_hover_text("この工具で切れるねじのピッチ");
        length(ui, unit, &mut p, 0.1..=10.0, 0.01, true);
        f.pitch = Some(p);
        ui.end_row();
    } else {
        f.pitch = None;
    }
    let mut neck = f
        .neck_diameter
        .unwrap_or(((t.diameter * 0.65 * 100.0).round() / 100.0).max(0.1));
    ui.label("首の径")
        .on_hover_text("歯より上の細い所の径。穴の壁やボスの山に当たらないかを調べる");
    length(ui, unit, &mut neck, 0.1..=50.0, 0.01, true);
    f.neck_diameter = Some(neck);
    ui.end_row();
    t.thread = Some(f);
}

fn default_corner(t: &Tool) -> f64 {
    ((t.radius() * 0.2 * 100.0).round() / 100.0).clamp(0.05, t.radius().max(0.05))
}

fn default_ball(t: &Tool) -> f64 {
    ((t.diameter / 4.0 * 100.0).round() / 100.0).clamp(0.1, t.diameter.max(0.1))
}

/// 材料ごとの切削条件の表。
/// 送り・下降は 1 分あたりの長さ、切込みとチップロードは長さで、文書の単位で見せる(単位は見出しに書く)。
fn presets_form(ui: &mut egui::Ui, feature: FeatureId, t: &mut Tool, unit: LengthUnit) {
    ui.label("材料ごとの切削条件(一番上が既定)");
    let mut remove = None;
    let warn = ui.visuals().warn_fg_color;
    egui::Grid::new(("presets", feature))
        .num_columns(9)
        .striped(true)
        .show(ui, |ui| {
            let u = unit.name();
            for h in [
                "材料".to_string(),
                "主軸回転数".to_string(),
                format!("切削送り({u}/min)"),
                format!("下降({u}/min)"),
                format!("切込み({u})"),
                "切削ピッチ".to_string(),
                "チップロード".to_string(),
                String::new(),
                String::new(),
            ] {
                ui.weak(h);
            }
            ui.end_row();
            let (diameter, flutes) = (t.diameter, t.flutes);
            let n = t.presets.len();
            for (i, p) in t.presets.iter_mut().enumerate() {
                egui::ComboBox::from_id_salt(("preset_mat", feature, i))
                    .selected_text(if p.material.is_empty() {
                        "(問わない)".to_string()
                    } else {
                        p.material.clone()
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut p.material, String::new(), "(問わない)");
                        for m in MATERIALS {
                            ui.selectable_value(&mut p.material, m.name.to_string(), m.name);
                        }
                    });
                let c = &mut p.cutting;
                ui.add(
                    egui::DragValue::new(&mut c.rpm)
                        .range(100.0..=60000.0)
                        .speed(50.0),
                );
                length_to(ui, unit, &mut c.feed, 1.0..=20000.0, 5.0, false, 0.1);
                length_to(ui, unit, &mut c.plunge, 1.0..=20000.0, 5.0, false, 0.1);
                length(ui, unit, &mut c.stepdown, 0.01..=50.0, 0.01, false);
                ui.add(
                    egui::DragValue::new(&mut c.stepover)
                        .range(0.01..=1.0)
                        .speed(0.01),
                );
                let cl = c.chip_load(flutes);
                // チップロードは小さいので、インチなどは 1 桁多く出す
                let d = if unit.is_mm() { 4 } else { 5 };
                let shown = format!("{:.d$} {u}", unit.from_mm(cl));
                let range = |lo: f64, hi: f64| {
                    format!(
                        "{:.e$}〜{:.e$} {u}",
                        unit.from_mm(lo),
                        unit.from_mm(hi),
                        e = d - 1
                    )
                };
                match chip_load_range(&p.material, diameter) {
                    Some((lo, hi)) if cl < lo || cl > hi => {
                        ui.colored_label(warn, shown).on_hover_text(format!(
                            "{}の目安 {} を外れています(目安は確定ではない)",
                            p.material,
                            range(lo, hi)
                        ));
                    }
                    Some((lo, hi)) => {
                        ui.label(shown)
                            .on_hover_text(format!("目安 {}", range(lo, hi)));
                    }
                    None => {
                        ui.label(shown);
                    }
                }
                if i > 0
                    && ui
                        .small_button("上へ")
                        .on_hover_text("既定にするには一番上へ")
                        .clicked()
                {
                    remove = Some((i, true));
                } else if i == 0 {
                    ui.label("");
                }
                if n > 1 && ui.small_button("削除").clicked() {
                    remove = Some((i, false));
                } else if n <= 1 {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    match remove {
        Some((i, true)) => t.presets.swap(i, i - 1),
        Some((i, false)) => {
            t.presets.remove(i);
        }
        None => {}
    }
    if ui.button("条件を足す").clicked() {
        let used: Vec<&str> = t.presets.iter().map(|p| p.material.as_str()).collect();
        let material = MATERIALS
            .iter()
            .map(|m| m.name)
            .find(|m| !used.contains(m))
            .unwrap_or("")
            .to_string();
        let cutting: Cutting = t.cutting();
        t.presets.push(Preset { material, cutting });
    }
}

/// 長さの値の欄。`v`・`range`・`speed` は mm で、文書の単位 `unit` に直して見せ、打った値を mm に
/// 戻す(動かさなければ書き戻さないので、mm の値が丸まらない)。`suffix` なら単位の名前を添える。
fn length(
    ui: &mut egui::Ui,
    unit: LengthUnit,
    v: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    suffix: bool,
) -> egui::Response {
    length_to(ui, unit, v, range, speed, suffix, 0.001)
}

/// [`length`] の、見せる細かさ `fine`(mm)を決める形(送りは 0.1 mm/min ほどで足りる)。
fn length_to(
    ui: &mut egui::Ui,
    unit: LengthUnit,
    v: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    suffix: bool,
    fine: f64,
) -> egui::Response {
    let k = unit.mm();
    let mut shown = *v / k;
    let mut drag = egui::DragValue::new(&mut shown)
        .range(range.start() / k..=range.end() / k)
        .speed(speed / k);
    if suffix {
        drag = drag.suffix(format!(" {}", unit.name()));
    }
    // mm 以外は mm の `fine` ほどの桁までにする(17 mm が 0.66929 in と長く出ないように)
    if !unit.is_mm() {
        drag = drag.max_decimals((-(fine / k).log10()).round().max(0.0) as usize);
    }
    let r = ui.add(drag);
    if r.changed() {
        *v = shown * k;
    }
    r
}

fn mm(ui: &mut egui::Ui, unit: LengthUnit, label: &str, v: &mut f64) {
    ui.label(label);
    length(ui, unit, v, 0.01..=200.0, 0.05, true);
    ui.end_row();
}

/// 空にできる長さ。印を外すと空(調べない)、入れると `default` から始める。
fn opt_mm(
    ui: &mut egui::Ui,
    unit: LengthUnit,
    label: &str,
    hint: &str,
    v: &mut Option<f64>,
    default: f64,
) {
    ui.label(label).on_hover_text(hint);
    ui.horizontal(|ui| {
        let mut on = v.is_some();
        if ui.checkbox(&mut on, "").changed() {
            *v = on.then_some(default.max(0.1));
        }
        if let Some(x) = v {
            length(ui, unit, x, 0.1..=200.0, 0.05, true);
        } else {
            ui.weak("なし");
        }
    });
    ui.end_row();
}
