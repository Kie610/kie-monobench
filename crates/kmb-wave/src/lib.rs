//! 音の波の解き方。今は周波数の解き方(外の空間の境界要素法)の核を持つ。
//!
//! 剛い・振動する・音を吸う物体のまわりの音圧を、周波数ごとにヘルムホルツの式で解く。
//! 使い方(物の陰の音・スピーカーの指向性・HRTF)はこの上の薄い層で作る。HRTF は耳の穴の
//! 小さな面を振動させて外へ出る音を遠方で見る(相反の定理で、方向ごとの耳の位置の音と同じ値になる)。
//!
//! - 入口は純関数 [`solve_bem`]。仕事([`BemJob`])と結果([`BemOut`])は JSON で往復できる。
//!   カーネル・文書・画面を知らない(研究の機能が網と条件を集めて仕事を作る)。
//! - 時間の向きは e^{jωt}。外へ出る波は e^{−jkr}/r。長さは m、周波数は Hz、音圧は Pa。
//! - 網は閉じた三角形の網で、法線(頂点の並びの右ねじ)は物体の外向き。開いた網・裏返った網は
//!   入力の誤りにする(黙って向きを直さない)。
//! - 確かめ用の解析解は [`exact`]、受け取り方向の格子は [`grid`]。
//! - 操作 `wave.solve`([`action`])は仕事の JSON を解いて結果の JSON を書き出す(CLI・MCP・画面)。
//! - 研究「音の放射・散乱」([`study`]、`wave.radiate@1`)はボディと振動する面から仕事を作り、
//!   「実行」で解く(進み具合と中止は kmb-study の実行が受け持つ)。

pub mod action;
pub mod bem;
pub mod exact;
mod gmres;
pub mod grid;
pub mod job;
pub mod mesh;
mod quad;
pub mod study;

pub use bem::{Watch, solve_bem};
pub use job::{
    BemJob, BemOut, Boundary, FrequencyOut, Medium, Receivers, Solver, Source, WaveError,
    WaveErrorKind,
};
pub use mesh::{BemMesh, icosphere};
pub use num_complex::Complex64;

/// この crate の名乗り(研究の種類 `wave.radiate@1` と操作 `wave.solve`)。実行・中止・結果の
/// 見せ方は kmb-study の操作が受け持つ。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-wave", study::register).with_extension(action::WaveCommands)
}
