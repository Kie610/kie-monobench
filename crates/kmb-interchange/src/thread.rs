//! ねじの見せ方の印。ねじを付ける機能(ねじの属性・ねじ穴)が出力 [`THREAD_MARKS_OUTPUT`] に入れ、
//! 3D のビューが読んで、その円筒の面にねじ山の陰影を付ける(形は変えない)。
//!
//! 面は名前でなく円筒の幾何(軸・半径・軸の向きの範囲)で示す。後の機能で面の名前が分かれても、
//! 同じ円筒が残っていれば付いていく。

use kmb_core::Payload;
use serde::{Deserialize, Serialize};

/// ねじの見せ方の印を入れる出力の名前。
pub const THREAD_MARKS_OUTPUT: &str = "thread_marks";

/// ねじを付けた円筒 1 つ(mm)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThreadMark {
    /// 軸の上の、ねじの始まりの点
    pub start: [f64; 3],
    /// 軸の向き(長さ 1。始まりからねじの続く向き)
    pub axis: [f64; 3],
    /// ねじの長さ(軸の向き)
    pub length: f64,
    /// 面の半径(おねじは外径、めねじは下穴の半径)
    pub radius: f64,
    pub pitch: f64,
    /// めねじ(穴)か
    pub internal: bool,
}

/// 機能 1 つが出す印の一覧。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ThreadMarks {
    pub marks: Vec<ThreadMark>,
}

impl Payload for ThreadMarks {
    const TYPE: &'static str = "cad.thread_marks@1";
}
