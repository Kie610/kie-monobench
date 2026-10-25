//! 解析の研究の共通の部品: 解き方と見せ方の間で受け渡す結果の形([`SimResult`]: 網・量の値・
//! 時刻のコマ)、研究を「実行」で解く・中止するしくみ([`run`])、結果を色分け・断面・流線で
//! 見せる部品([`view`])、解析の作業空間の操作([`StudyCommands`])。
//!
//! 解き方(jc-fea の強度・熱、jc-flow の流れ)は評価で解く準備だけをして出力 `plan` を出し、
//! 重い計算は「実行」でここが別のスレッドで回す。どの解き方の結果も同じ形なので、どの
//! 見せ方とも組み合わせられる。

pub mod commands;
pub mod fields;
pub mod result;
pub mod run;
pub mod view;

pub use commands::{CANCEL, RUN, StudyCommands, solve_now};
pub use result::{
    Derive, Frame, Geometry, Grid, Mark, Part, Quantity, Role, SimResult, Start, Tets, Values,
};
pub use run::{Control, PLAN, Plan, Prepared, Task, prepare, prepared};

/// この crate の名乗り(操作だけ。種類は解き方の crate が登録する)。
pub fn module() -> jc_core::Module {
    // 解く仕事の預かり所は組み立てごとに 1 つ。解析の機能が評価の中で預ける
    jc_core::Module::new("jc-study", |reg| {
        reg.provide(std::sync::Arc::new(Prepared::default()))
    })
    .with_extension(StudyCommands::default())
}
