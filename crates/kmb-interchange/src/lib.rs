//! 層をまたぐ受け渡しの型。CAD の機能が出力に入れ、加工が読む(CAD と CAM は互いの crate に
//! 依存しない決まりなので、受け渡す形はここだけに置く)。コアには置かない(コアは幾何と文書の
//! 骨組みだけを持つ)。
//!
//! | 型 | 出す機能 | 読む所 |
//! | --- | --- | --- |
//! | [`DatumPlane`](`datum.plane@1`、出力 `datum`) | 作業平面(kmb-cad-datum) | スケッチの平面 |
//! | [`DatumAxis`](`datum.axis@1`)・[`DatumPoint`](`datum.point@1`、出力 `datum`) | 作業軸・作業点(kmb-cad-datum) | 回転・パターン・移動の軸、作業平面・作業軸の点 |
//! | [`Segments`](`sketch.segments@1`、出力 `segments`) | 3D スケッチ(kmb-sketch-3d) | 回転・パターン・移動の軸 |
//! | [`HoleInfo`](`hole.info@1`、出力 `hole`) | 穴(kmb-cad-hole) | 穴あけ・穴から加工を作る |
//! | [`ThreadMarks`](`cad.thread_marks@1`、出力 `thread_marks`) | ねじ・ねじ穴(kmb-cad-hole) | 3D のビューのねじ山の陰影 |
//! | [`motion::MotionStudies`](区画 `asm.motion`)・[`contact::Contacts`](区画 `asm.contacts`) | 組立(kmb-asm) | 画面・kmb-session |
//! | [`animation::Storyboard`](区画 `asm.animation`)・ジョイントの今の様子 [`animation::Drive`] | 組立(kmb-asm) | 画面・レンダリングの書き出し・kmb-session |
//! | [`configuration::Configurations`](区画 `config.table`) | 構成表(kmb-config) | 画面・kmb-session |
//! | [`visibility::Visibility`](区画 `cad.visibility`) | CAD の共通の操作(kmb-cad) | 画面・kmb-session |
//! | [`ink`] の印(線・箱・平面・形の色・印) | 3D のビューに重ねる物を出すモジュール | 画面のテーマ |
//! | [`role`] の役割の印(操作・機能の種類・設定) | 名乗るモジュール | 画面・CLI・MCP・kmb-session |
//!
//! 後で材料(M26)や密度(M47)を足す時もここに置く。
pub mod animation;
pub mod configuration;
pub mod contact;
pub mod datum;
pub mod hole;
pub mod ink;
pub mod motion;
pub mod role;
pub mod segment;
pub mod thread;
pub mod visibility;

pub use datum::{
    DATUM_OUTPUT, DatumAxis, DatumPlane, DatumPoint, datum_axes, datum_axis, datum_axis_choices,
    datum_choices, datum_plane, datum_planes, datum_point, datum_points, selected_datum,
};
pub use hole::{HOLE_OUTPUT, HoleInfo, HoleKind, HolePart, Tap};
pub use segment::{SEGMENTS_OUTPUT, Segment, Segments, segment_axis, segments_now};
pub use thread::{THREAD_MARKS_OUTPUT, ThreadMark, ThreadMarks};
