//! スケッチの表示の切り替え、選べる物の絞り込み(選択フィルタ)、
//! 選び方(窓・自由形状・なぞる。キー 1・2・3)。パネルの上の方に出す(網羅版 I1・A2・H2・G5・G8)。

use crate::view::Editor;
use jc_sketch::{Entity, EntityId, Sketch, Solution};
use std::collections::BTreeSet;

/// 図に出す物。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Display {
    /// 閉じた領域を薄く塗り、閉じていない線の端を赤い輪で示す
    pub profiles: bool,
    /// 点(線の端・中心・単独の点)
    pub points: bool,
    /// 寸法
    pub dims: bool,
    /// 写した形と、既存の形の辺(吸い付く先)
    pub projected: bool,
    /// 補助線
    pub construction: bool,
    /// 格子
    pub grid: bool,
    /// 格子に吸い付く(描く道具)
    pub snap_grid: bool,
    /// 吸い付く刻み(mm)。拡大率では変えない(docs/避ける形.md)
    pub snap_step: f64,
    /// スライス: 前にできたボディをスケッチの平面で切った断面を見せる(2D の図ではボディの手前を隠す代わりに断面を描く)
    pub slice: bool,
}

impl Default for Display {
    fn default() -> Self {
        Display {
            profiles: true,
            points: true,
            dims: true,
            projected: true,
            construction: true,
            grid: true,
            snap_grid: false,
            snap_step: 1.0,
            slice: false,
        }
    }
}

impl Display {
    /// 図に出さない要素か(出さない物は選べない)。
    pub(crate) fn hides(&self, sketch: &jc_sketch::Sketch, id: jc_sketch::EntityId) -> bool {
        let Some(e) = sketch.entity(id) else {
            return false;
        };
        let projected = sketch.is_projected(id) || sketch.projected_parts().0.contains(&id);
        (!self.projected && projected)
            || (!self.points && matches!(e, jc_sketch::Entity::Point { .. }))
            || (!self.construction && e.is_construction())
    }
}

/// 選ぶ道具で選べる物。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Filter {
    pub points: bool,
    /// 線分・円弧・円
    pub curves: bool,
    /// 寸法の値の札(ダブルクリックで直す)
    pub dims: bool,
    /// 拘束の印
    pub constraints: bool,
    /// 閉じた領域
    pub profiles: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            points: true,
            curves: true,
            dims: true,
            constraints: true,
            profiles: true,
        }
    }
}

impl Filter {
    /// 要素を選べるか。
    pub(crate) fn takes(&self, sketch: &jc_sketch::Sketch, id: jc_sketch::EntityId) -> bool {
        match sketch.entity(id) {
            Some(jc_sketch::Entity::Point { .. }) => self.points,
            Some(_) => self.curves,
            None => false,
        }
    }
}

/// 選べる要素か(絞り込みと表示の両方を見る)。
pub(crate) fn selectable(
    editor: &Editor,
    sketch: &jc_sketch::Sketch,
    id: jc_sketch::EntityId,
) -> bool {
    editor.filter.takes(sketch, id) && !editor.display.hides(sketch, id)
}

/// 自由な形(スケッチの平面の座標の多角形)の中に全部入った要素。
pub(crate) fn lasso_pick(
    sketch: &jc_sketch::Sketch,
    sol: &jc_sketch::Solution,
    poly: &[[f64; 2]],
) -> BTreeSet<jc_sketch::EntityId> {
    let mut out = BTreeSet::new();
    for e in &sketch.entities {
        let id = e.id();
        let pts = match e {
            jc_sketch::Entity::Point { .. } => sol.points.get(&id).map(|p| vec![*p]),
            _ => sketch.curve_polyline(sol, id, jc_sketch::ARC_TOLERANCE * 10.0),
        };
        if pts.is_some_and(|v| !v.is_empty() && v.iter().all(|p| inside(poly, *p))) {
            out.insert(id);
        }
    }
    out
}

/// なぞった道(スケッチの平面の座標)が `tol` 以内を通った要素。点の間は `tol` の刻みで補う。
pub(crate) fn paint_pick(
    sketch: &jc_sketch::Sketch,
    sol: &jc_sketch::Solution,
    path: &[[f64; 2]],
    tol: f64,
) -> BTreeSet<jc_sketch::EntityId> {
    let mut out = BTreeSet::new();
    let mut probe = |p: [f64; 2]| {
        for h in sketch.hits(sol, p[0], p[1], tol) {
            out.insert(h.entity);
        }
    };
    for (i, p) in path.iter().enumerate() {
        if i > 0 {
            let q = path[i - 1];
            let d = (p[0] - q[0]).hypot(p[1] - q[1]);
            let n = (d / tol.max(1e-9)).ceil().min(200.0) as usize;
            for k in 1..n {
                let f = k as f64 / n as f64;
                probe([q[0] + (p[0] - q[0]) * f, q[1] + (p[1] - q[1]) * f]);
            }
        }
        probe(*p);
    }
    out
}

/// 何もない所から引いた時の選び方。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum SelectMode {
    /// 四角で囲む(右へは中に入った物、左へは触れた物)
    #[default]
    Window,
    /// 自由な形で囲む(中に入った物)
    Lasso,
    /// なぞって触れた物を選ぶ
    Paint,
}

impl SelectMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            SelectMode::Window => "窓",
            SelectMode::Lasso => "自由形状",
            SelectMode::Paint => "なぞる",
        }
    }
}

/// 格子の交点へ寄せた位置。
pub(crate) fn to_grid(w: [f64; 2], step: f64) -> [f64; 2] {
    [(w[0] / step).round() * step, (w[1] / step).round() * step]
}

/// パネルの「スケッチの表示」と「選び方」。
/// 線の種類。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LineType {
    Normal,
    Construction,
    Centerline,
}

impl LineType {
    const ALL: [(LineType, &'static str, &'static str); 3] = [
        (LineType::Normal, "標準", "普通の線(輪郭になる)"),
        (
            LineType::Construction,
            "補助線",
            "形を決める補助の線(輪郭に入らない。X)",
        ),
        (
            LineType::Centerline,
            "中心線",
            "回転の軸・直径の寸法の基準になる一点鎖線(線分だけ)",
        ),
    ];
}

/// 選んだ線を線種 `t` にしたスケッチ(線種を変えられる線が無ければ `None`)。中心線は線分だけ。
pub(crate) fn set_linetype(sketch: &Sketch, sel: &[EntityId], t: LineType) -> Option<Sketch> {
    let mut s = sketch.clone();
    let mut changed = false;
    for id in sel {
        let Some(e) = s.entity(*id) else { continue };
        if matches!(e, Entity::Point { .. }) {
            continue;
        }
        let line = matches!(e, Entity::Line { .. });
        let (cons, center) = (e.is_construction(), s.is_centerline(*id));
        match t {
            LineType::Normal if center => s.toggle_centerline(*id),
            LineType::Normal if cons => s.toggle_construction(*id),
            LineType::Construction if center => {
                // 中心線を外すと普通の線になるので、補助線へ戻す
                s.toggle_centerline(*id);
                s.toggle_construction(*id);
            }
            LineType::Construction if !cons => s.toggle_construction(*id),
            LineType::Centerline if line && !center => s.toggle_centerline(*id),
            _ => continue,
        }
        changed = true;
    }
    changed.then_some(s)
}

/// パレットの「オプション」: 線種のボタン(選んでいれば選んだ線を変え、選んでいなければ次に描く線の
/// 種類にする)、「正面から見る」(N1c)と、3D スケッチの入口の案内(K4・K14)。変えたスケッチを返す。
pub(crate) fn options(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
) -> Option<Sketch> {
    let sel: Vec<EntityId> = crate::constraints_ui::selected(editor, sketch)
        .into_iter()
        .filter(|id| !matches!(sketch.entity(*id), Some(Entity::Point { .. }) | None))
        .collect();
    // 今の線種: 選んでいれば選んだ線(全部同じ時だけ)、いなければ次に描く線
    let now = if sel.is_empty() {
        Some(if editor.draw_centerline {
            LineType::Centerline
        } else if editor.draw_construction {
            LineType::Construction
        } else {
            LineType::Normal
        })
    } else {
        let of = |id: &EntityId| {
            if sketch.is_centerline(*id) {
                LineType::Centerline
            } else if sketch.entity(*id).is_some_and(|e| e.is_construction()) {
                LineType::Construction
            } else {
                LineType::Normal
            }
        };
        let first = of(&sel[0]);
        sel.iter().all(|id| of(id) == first).then_some(first)
    };
    let mut out = None;
    ui.horizontal_wrapped(|ui| {
        ui.label("線種");
        for (t, label, hint) in LineType::ALL {
            let hint = if sel.is_empty() {
                format!("{hint}。押すと次に描く線をこの種類にする")
            } else {
                format!("{hint}。押すと選んだ線をこの種類にする")
            };
            if ui
                .selectable_label(now == Some(t), label)
                .on_hover_text(hint)
                .clicked()
            {
                if sel.is_empty() {
                    editor.draw_construction = t == LineType::Construction;
                    editor.draw_centerline = t == LineType::Centerline;
                } else {
                    let mut base = sketch.clone();
                    base.adopt_solution(sol);
                    out = set_linetype(&base, &sel, t);
                    if out.is_none() && t == LineType::Centerline {
                        editor.notice = Some("中心線にできるのは線分だけです".into());
                    }
                }
            }
        }
    });
    // 正面から見る(N1c)。右ドラッグで 3D の視点へ回した後は
    // スケッチの平面を真正面から見る平らな図へ戻し、どちらでも形全体が収まるよう合わせる(t2)
    if ui
        .button("正面から見る")
        .on_hover_text(if editor.in_space {
            "スケッチの平面を真正面から見る平らな図に戻し、形全体が収まるよう合わせる"
        } else {
            "形全体が収まるよう合わせる(右ドラッグで 3D の視点に回した後は、真正面へ戻す)"
        })
        .clicked()
    {
        editor.look_front = true;
    }
    // 作図面をはっきりさせるため 3D スケッチは別の種類のスケッチにしているので、
    // 平面のスケッチの表示の欄で入口を案内する
    ui.label(
        egui::RichText::new("3D スケッチは「作成 → 3D スケッチ」で別に作ります(空間に線を描く)")
            .weak()
            .small(),
    )
    .on_hover_text("平面のスケッチの中では描かず、作図面を選べる 3D スケッチを作る");
    out
}

pub(crate) fn panel(ui: &mut egui::Ui, editor: &mut Editor) {
    egui::CollapsingHeader::new("スケッチの表示")
        .id_salt("sketch_display")
        .default_open(false)
        .show(ui, |ui| {
            let d = &mut editor.display;
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut d.profiles, "領域の塗り")
                    .on_hover_text("閉じた領域を薄く塗り、閉じていない線の端を赤い輪で示す");
                ui.checkbox(&mut d.points, "点");
                ui.checkbox(&mut d.dims, "寸法");
                ui.checkbox(&mut d.projected, "投影");
                ui.checkbox(&mut d.construction, "補助線");
                ui.checkbox(&mut d.grid, "グリッド");
                ui.checkbox(&mut d.slice, "スライス")
                    .on_hover_text("前にできたボディをスケッチの平面で切った断面を描く(交差の道具で線にできる)");
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.snap_grid, "スナップ")
                    .on_hover_text("描く道具で、点を刻みの交点へ寄せる(既存の点や線に吸い付く時はそちらを先にする)。刻みは拡大しても変わらない");
                ui.add(
                    egui::DragValue::new(&mut d.snap_step)
                        .range(0.01..=1000.0)
                        .speed(0.1)
                        .suffix(" mm"),
                );
            });
        });
    egui::CollapsingHeader::new("選べる物と選び方")
        .id_salt("sketch_filter")
        .default_open(false)
        .show(ui, |ui| {
            let f = &mut editor.filter;
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut f.points, "点");
                ui.checkbox(&mut f.curves, "線");
                ui.checkbox(&mut f.dims, "寸法");
                ui.checkbox(&mut f.constraints, "拘束");
                ui.checkbox(&mut f.profiles, "領域");
            });
            ui.horizontal(|ui| {
                ui.label("空いた所から引くと");
                for (m, key) in [
                    (SelectMode::Window, "1"),
                    (SelectMode::Lasso, "2"),
                    (SelectMode::Paint, "3"),
                ] {
                    ui.radio_value(&mut editor.select_mode, m, m.label())
                        .on_hover_text(format!("キー {key}"));
                }
            });
        });
}

/// 点が多角形(画面の座標)の中か(偶奇の数え方)。
pub(crate) fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut c = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
        j = i;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_snapping_rounds_to_the_drawn_step() {
        assert_eq!(to_grid([3.4, -2.6], 2.0), [4.0, -2.0]);
        let sq = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        assert!(inside(&sq, [1.0, 1.0]));
        assert!(!inside(&sq, [5.0, 1.0]));
    }

    #[test]
    fn lasso_takes_what_is_inside_and_paint_takes_what_it_crosses() {
        let mut s = jc_sketch::Sketch::default();
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(10.0, 0.0);
        let near = s.add_line(a, b);
        let c = s.add_point(0.0, 50.0);
        let d = s.add_point(10.0, 50.0);
        let far = s.add_line(c, d);
        let sol = jc_sketch::solve(&s, &Default::default()).unwrap();
        let poly = [[-1.0, -1.0], [11.0, -1.0], [11.0, 1.0], [-1.0, 1.0]];
        let got = lasso_pick(&s, &sol, &poly);
        assert!(got.contains(&near) && got.contains(&a) && !got.contains(&far));
        let got = paint_pick(&s, &sol, &[[5.0, -10.0], [5.0, 60.0]], 0.5);
        assert!(got.contains(&near) && got.contains(&far) && !got.contains(&a));
    }
}
