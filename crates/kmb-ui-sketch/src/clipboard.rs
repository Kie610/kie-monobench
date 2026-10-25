//! スケッチの要素のコピー・切り取り・貼り付け(Ctrl+C / Ctrl+X / Ctrl+V と、選ぶ道具の
//! 右クリックメニュー)。写したもの([`kmb_sketch::Clip`])は画面の中に持ち、別のスケッチへも
//! 貼れる。OS のクリップボードには短い文を置く: Ctrl+V はクリップボードに文がある時だけ
//! 貼り付けの合図として届くため。ほかの文をコピーした後の Ctrl+V では貼らない。

use crate::view::Editor;
use kmb_sketch::{Clip, Entity, EntityId, Sketch, Solution};

/// 画面の中に持つ、写したもの。
#[derive(Clone)]
pub(crate) struct Held {
    pub clip: Clip,
    /// OS のクリップボードに置いた文(Ctrl+V で届いた文がこれなら貼る)
    pub text: String,
}

fn key() -> egui::Id {
    egui::Id::new("jc_sketch_clipboard")
}

/// 写したもの(なければ `None`)。
pub(crate) fn held(ctx: &egui::Context) -> Option<Held> {
    ctx.data(|d| d.get_temp::<Held>(key()))
}

/// このフレームに届いた、コピー・切り取り・貼り付けの合図。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Ask {
    Copy,
    Cut,
    Paste(String),
}

pub(crate) fn asks(ctx: &egui::Context) -> Vec<Ask> {
    ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Copy => Some(Ask::Copy),
                egui::Event::Cut => Some(Ask::Cut),
                egui::Event::Paste(t) => Some(Ask::Paste(t.clone())),
                _ => None,
            })
            .collect()
    })
}

/// OS のクリップボードに置く文(写した物の数と種類)。
fn summary(clip: &Clip) -> String {
    let used: std::collections::BTreeSet<EntityId> =
        clip.entities.iter().flat_map(|e| e.points()).collect();
    let (mut lines, mut circles, mut arcs, mut points) = (0, 0, 0, 0);
    let (mut ellipses, mut splines, mut conics) = (0, 0, 0);
    for e in &clip.entities {
        match e {
            Entity::Line { .. } => lines += 1,
            Entity::Circle { .. } => circles += 1,
            Entity::Arc { .. } => arcs += 1,
            Entity::Ellipse { .. } => ellipses += 1,
            Entity::Spline { .. } => splines += 1,
            Entity::Conic { .. } => conics += 1,
            Entity::Point { id, .. } if !used.contains(id) => points += 1,
            Entity::Point { .. } => {}
        }
    }
    let parts: Vec<String> = [
        ("線分", lines),
        ("円", circles),
        ("円弧", arcs),
        ("楕円", ellipses),
        ("スプライン", splines),
        ("円錐曲線", conics),
        ("点", points),
    ]
    .into_iter()
    .filter(|(_, n)| *n > 0)
    .map(|(name, n)| format!("{name} {n}"))
    .collect();
    format!(
        "kieMonoBench のスケッチの要素({}、拘束と寸法 {})",
        parts.join("・"),
        clip.constraints.len()
    )
}

/// 選んだ要素を写す。写せたら写した物の数を返す。
pub(crate) fn copy(
    ctx: &egui::Context,
    editor: &Editor,
    sketch: &Sketch,
    sol: &Solution,
) -> Option<usize> {
    let sel = crate::constraints_ui::selected(editor, sketch);
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    let clip = s.copy_entities(&sel)?;
    let text = summary(&clip);
    let n = clip.len();
    ctx.copy_text(text.clone());
    ctx.data_mut(|d| d.insert_temp(key(), Held { clip, text }));
    Some(n)
}

/// 写したものを貼る。写した時と同じ位置に重ねて置き、貼った要素を選んで
/// 移動の道具を始める(基準は写した形を囲む箱の中心。クリックした所へ動くか、「x, y」の量を
/// 打って Enter。Esc ならその位置のまま)。
pub(crate) fn paste(
    ctx: &egui::Context,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
) -> Option<Sketch> {
    let h = held(ctx)?;
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    let new = s.paste(&h.clip, [0.0, 0.0]);
    if new.is_empty() {
        return None;
    }
    if editor.tool != crate::Tool::Select {
        editor.finish(editor.tool);
    }
    editor.selection = new.into_iter().collect();
    editor.regions.clear();
    editor.picked_constraint = None;
    editor.set_tool(crate::Tool::Move);
    editor.pending = Some(crate::view::Pick {
        point: None,
        pos: h.clip.center(),
    });
    Some(s)
}

/// 切り取る: 写してから、選んだ要素を消す。
pub(crate) fn cut(
    ctx: &egui::Context,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
) -> Option<Sketch> {
    copy(ctx, editor, sketch, sol)?;
    crate::constraints_ui::selection_action(editor, sketch, sol, "delete")
}

#[cfg(test)]
#[path = "clipboard_tests.rs"]
mod tests;
