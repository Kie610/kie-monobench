//! 流れと空力の解析(52)。翼は渦格子法(VLM)で揚力・誘導抗力・中立点を、ダクトや物のまわりの
//! 流れは格子ボルツマン法(2D は D2Q9、3D は D3Q19)で速さ・圧力・渦と抗力を出す。
//!
//! 結果はどれも目安。翼は薄い翼の理論(失速・摩擦の抗力を含まない)、流れは格子で表せる
//! レイノルズ数に下げて解くことがあり、抗力の絶対値は ±数十% と考える。解き方は自前
//! (GPL 系の部品を使わない)。計算は CPU のスレッドで並列に行う。結果の形と、実行・見せ方の操作は
//! 強度と熱(jc-fea)と共通(jc-study)。

pub mod flow;
pub mod geom;
pub mod lbm;
pub mod study;
pub mod vlm;
pub mod wing;

#[cfg(test)]
mod validation_tests;

pub use study::{SECTION, VOLUME, WING, register};

/// この crate の名乗り(登録する種類)。実行と結果の見せ方の操作は jc-study が受け持つ。
pub fn module() -> jc_core::Module {
    jc_core::Module::new("jc-flow", register)
}
