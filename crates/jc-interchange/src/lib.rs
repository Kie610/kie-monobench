//! 層をまたぐ受け渡しの型。CAD の機能が出力に入れ、加工が読む(CAD と CAM は互いの crate に
//! 依存しない決まりなので、受け渡す形はここだけに置く)。コアには置かない(コアは幾何と文書の
//! 骨組みだけを持つ)。
//!
//! | 型 | 出す機能 | 読む所 |
//! | --- | --- | --- |
//! | [`HoleInfo`](`hole.info@1`、出力 `hole`) | 穴(jc-cad-hole) | 穴あけ・穴から加工を作る |
//! | [`motion::MotionStudies`](区画 `asm.motion`)・[`contact::Contacts`](区画 `asm.contacts`) | 組立(jc-asm) | 画面・jc-session |
//! | [`animation::Storyboard`](区画 `asm.animation`)・ジョイントの今の様子 [`animation::Drive`] | 組立(jc-asm) | 画面・レンダリングの書き出し・jc-session |
//! | [`configuration::Configurations`](区画 `config.table`) | 構成表(jc-config) | 画面・jc-session |
//! | [`ink`] の印(線・箱・平面・形の色・印) | 3D のビューに重ねる物を出すモジュール | 画面のテーマ |
//! | [`role`] の役割の印(操作・機能の種類・設定) | 名乗るモジュール | 画面・CLI・MCP・jc-session |
//!
//! 後で材料(M26)や密度(M47)を足す時もここに置く。
pub mod animation;
pub mod configuration;
pub mod contact;
pub mod hole;
pub mod ink;
pub mod motion;
pub mod role;

pub use hole::{HOLE_OUTPUT, HoleInfo, HoleKind, HolePart, Tap};
