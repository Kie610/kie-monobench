//! 頭部伝達関数(HRTF)のデータの型と、形式をそろえる道具。
//!
//! いくつもの HRTF のデータは、方向の格子・座標の決まり・サンプリング周波数・遅れの持ち方がそろって
//! いない。この crate は 1 つの型(`HrirSet`)に読み込んだ物へ、JSON で書いた処理の手順(`recipe`)を
//! かけて形をそろえ、時間差を除いた半スペクトルと遅延の表(`table`)などに書き出す。同じ手順を、計算で
//! 作った HRTF にも測った HRTF にもかけられる。
//!
//! 処理はファイルにも文書にも触らない純関数で、書き出しはファイルの名前とバイト列を返すだけにする
//! (置くのは呼ぶ側の壊れない書き込み)。FFT と float16 の変換は自前で持ち、外の crate に頼らない。

pub mod dsp;
mod error;
pub mod fft;
pub mod grid;
pub mod half;
pub mod recipe;
mod set;
pub mod table;

#[cfg(test)]
mod tests;

pub use error::{ErrorKind, HrtfError};
pub use recipe::{Recipe, Step, run};
pub use set::{Dir, Frame, Grid, HrirSet};
pub use table::{Layout, OutFile, TableSpec};
