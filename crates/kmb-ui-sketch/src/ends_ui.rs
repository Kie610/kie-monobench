//! パネルの「開いた端」の行と「端をつなぐ」ボタン。読み込んだ DXF や描いた輪郭のわずかな隙間で領域ができない時に、
//! 1 回で直す。

use crate::view::{DimValues, Editor};
use kmb_sketch::{Sketch, Solution};

/// 端の行。つないだらスケッチを返す。開いた端も位置だけで接する端も無ければ何も出さない。
pub(crate) fn row(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
) -> Option<Sketch> {
    let loose = kmb_sketch::loose_ends(sketch, sol);
    if loose.open.is_empty() && loose.untied.is_empty() {
        return None;
    }
    // 組の数は拘束の候補の数(余分な物を除くのは押した時だけ。毎コマ解き直さない)
    let count = kmb_sketch::tie_ends(sketch, sol, editor.tie_gap).len();
    let mut out = None;
    ui.separator();
    ui.label(format!(
        "開いた端 {} ・ 位置だけで接する端 {}",
        loose.open.len(),
        loose.untied.len()
    ))
    .on_hover_text(
        "開いた端: どの線ともつながらない端(赤い輪。輪郭が閉じず押し出せない)。位置だけで接する端: 重なっているが一致・線上の拘束が無い端(動かすと離れる)",
    );
    ui.horizontal(|ui| {
        ui.label("つなぐ距離");
        ui.add(
            egui::DragValue::new(&mut editor.tie_gap)
                .range(0.0..=10.0)
                .speed(0.01)
                .max_decimals(3)
                .suffix(" mm"),
        )
        .on_hover_text("この距離より近い開いた端どうしを、近い順に 1 対 1 で一致にする(0 なら重なった端だけ)");
        let label = format!("端をつなぐ({count} 組)");
        if ui
            .add_enabled(count > 0, egui::Button::new(label))
            .on_hover_text("位置だけで接する端に一致・線上を付け、近い開いた端どうしを一致にする")
            .clicked()
        {
            match tie(sketch, sol, editor.tie_gap, values) {
                Some((s, n)) if n > 0 => out = Some(s),
                _ => {
                    editor.notice =
                        Some("つなげる端がありません(ほかの拘束で決まっているか、つなぐと解けなくなります)".into());
                }
            }
        }
    });
    out
}

/// 端をつないだスケッチと、付けた拘束の数。ほかの拘束で決まっている物(余分)と、付けると解けなく
/// なる物は付けない。
fn tie(
    sketch: &Sketch,
    sol: &Solution,
    gap: f64,
    values: Option<&DimValues>,
) -> Option<(Sketch, usize)> {
    let values = values?;
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    let mut n = 0;
    for k in kmb_sketch::tie_ends(sketch, sol, gap) {
        if kmb_sketch::redundant_with(&s, values, &k).is_some() {
            continue;
        }
        let mut t = s.clone();
        t.add_constraint(k);
        if kmb_sketch::solve(&t, values)
            .is_ok_and(|r| !matches!(r.status, kmb_sketch::SolveStatus::Inconsistent { .. }))
        {
            s = t;
            n += 1;
        }
    }
    Some((s, n))
}
