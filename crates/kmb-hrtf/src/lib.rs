//! 頭部伝達関数(HRTF)のデータの型と、形式をそろえる道具。
//!
//! いくつもの HRTF のデータは、方向の格子・座標の決まり・サンプリング周波数・遅れの持ち方がそろって
//! いない。この crate は 1 つの型(`HrirSet`)に読み込んだ物へ、JSON で書いた処理の手順(`recipe`)を
//! かけて形をそろえ、時間差を除いた半スペクトルと遅延の表(`table`)などに書き出す。同じ手順を、計算で
//! 作った HRTF にも測った HRTF にもかけられる。
//!
//! 処理はファイルにも文書にも触らない純関数で、書き出しはファイルの名前とバイト列を返すだけにする
//! (置くのは呼ぶ側の壊れない書き込み)。FFT・float16 の変換・SOFA に要る HDF5 の書き込みは自前で持ち、
//! よそで作られた SOFA の読み込みだけを純 Rust の HDF5 の読み手(`hdf5-pure`)に任せる。ファイルを読む
//! のは操作(`commands`、`hrtf.process`)だけ。
//!
//! 頭の網から HRTF を計算する入口(`compute`)も持つ。耳の入口を振動させて境界要素法(`kmb-wave`)で
//! 解き、HRIR の集まりにする。結果には同じ処理の手順をかけられる。頭の網は確かめて小さな穴を埋め、
//! 耳からの距離で要素の大きさを変えて作り直せる(`head`)。

mod commands;
pub mod compute;
pub mod dsp;
mod error;
pub mod fft;
pub mod grid;
pub mod half;
mod hdf5w;
pub mod head;
pub mod recipe;
pub mod regrid;
mod set;
pub mod sofa;
pub mod table;

#[cfg(test)]
mod tests;

pub use commands::{HrtfCommands, PROCESS};
pub use compute::{Directions, HeadJob, HeadOut, Transfer, compute, head_meshes};
pub use error::{ErrorKind, HrtfError};
pub use head::{Grading, Hole, Tidy, regrade, tidy};
pub use recipe::{Recipe, RunOptions, Step, run, run_with};
pub use set::{Dir, Frame, Grid, HrirSet};
pub use table::{Layout, OutFile, TableSpec};

/// この crate の名乗り(HRTF の形式をそろえる操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::commands("kmb-hrtf").with_extension(HrtfCommands)
}
