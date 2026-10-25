//! 3D スケッチの操作(画面・CLI・MCP に出す): 3D スケッチの追加。

use crate::{SKETCH3D, Sketch3d};
use jc_core::{Action, Context, Extension, Outcome, Place};

/// 3D スケッチを足す(点と線を 3 次元に置く)
pub const ADD_SKETCH3D: &str = "cad.add.sketch3d";

/// 3D スケッチの操作。
pub struct Sketch3dCommands;

impl Extension for Sketch3dCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        vec![Action::new(ADD_SKETCH3D, "3D スケッチを追加", "cad")
            .menu("作成")
            .icon("sketch3d")
            .aliases("すけっち 3d sketch さんじげん りったい 3dすけっち")
            .hint("点と線を 3 次元に置くスケッチを足す。高さの違う図や直交する図を 1 つのスケッチに描き、軸に沿う・平行・直角と 3 次元の角度で形を決める。平らな面を選んでいればその面が基準")
            .at(&[Place::Toolbar])
            // 2D の「スケッチを作成」のすぐ後(「面にスケッチ」の前)に並べる
            .before(jc_sketch::SKETCH_ON_FACE)]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        (id == ADD_SKETCH3D).then(|| {
            Ok(jc_sketch::insert_feature(
                "3D スケッチを追加",
                SKETCH3D,
                format!("3D スケッチ{}", cx.count_of(SKETCH3D) + 1),
                &Sketch3d {
                    plane: jc_sketch::picked_sketch_plane(cx).unwrap_or_default(),
                    ..Default::default()
                },
            ))
        })
    }
}
