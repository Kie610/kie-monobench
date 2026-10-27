//! ファイルの形式の書き手と読み手の小さな部品。形の型(コアの網・スケッチ)を知らず、
//! バイト列と数だけを扱うので、どの層の crate からも使える。
//!
//! | モジュール | 中身 |
//! | --- | --- |
//! | [`zip`] | ファイルを並べた zip を書く(deflate)。書いた zip から名前で取り出す |
//! | [`png`] | 8 ビットの RGB・RGBA の PNG を書く。書いた RGB の PNG を読み戻す |
//! | [`dxf`] | DXF R12(AC1009)の最小の形(HEADER・TABLES・ENTITIES)を書く |
//! | [`svg`] | SVG の数の書き方と、文字の置き換え |
//! | [`base64`] | バイト列と base64 の文字の行き来(絵を JSON に入れる) |
//! | [`step`] | STEP の長さの単位を読む(形の読み手が mm にそろえる倍率) |

pub mod base64;
pub mod dxf;
pub mod png;
pub mod step;
pub mod svg;
pub mod zip;
