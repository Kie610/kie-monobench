//! 強度と熱の解析(51)。ボディを形に沿った 2 次の四面体の網(10 節点。辺の中点を曲面に
//! 乗せる)に区切り、線形静解析(応力とたわみ・安全率)、定常の熱伝導(温度)、固有振動(共振)、
//! 軽い形の参考(素の SIMP によるトポロジー最適化)を解く。網は形に沿って作る(表面の張り直し → Delaunay で中を埋める → 形を整える、[`tetmesh`])。
//! 連立方程式は代数的マルチグリッドを前処理にした共役勾配法([`amg`]・[`solve`])。
//!
//! 結果はどれも線形・小変形・接触なしの目安。網の確かさ(体積の差・表面のずれ・要素の形)は
//! 結果の数値に出す。解き方は自前(GPL 系の部品を使わない)。
//!
//! 研究の評価は解く準備(網にするボディの形と、読み切った条件)だけをして出力 `plan` を出し、
//! 解くのは「実行」の時(kmb-study)。結果は kmb-study の共通の形([`kmb_study::SimResult`])。

pub(crate) mod amg;
pub mod boundary;
pub(crate) mod delaunay;
pub mod geom;
pub(crate) mod material;
pub mod modal;
pub(crate) mod result;
pub(crate) mod singular;
pub(crate) mod solve;
pub(crate) mod sparse;
pub mod statics;
pub mod study;
pub mod surfmesh;
pub mod tet10;
pub mod tetmesh;
pub(crate) mod tetopt;
pub mod thermal;
pub mod topology;
pub(crate) mod volmesh;

#[cfg(test)]
mod recovery_lab;
#[cfg(test)]
mod validation_tests;

pub use study::{MODAL, STATIC, THERMAL, TOPOLOGY, register};

/// この crate の名乗り(登録する種類)。実行・中止・結果の見せ方は kmb-study の操作が受け持つ。
/// 組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-fea", register)
}
