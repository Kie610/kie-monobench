//! スケッチの拘束ソルバの数値の部分。要素と拘束の番号([`EntityId`]・[`ConstraintId`])、
//! 自由曲線の計算([`curve`])、平面の向きの小さな計算([`vec2`])、拘束の式とその解き方([`system`])、式の形を持たない残差の行の
//! 解き方([`rows`]、3D スケッチ)を持つ。
//!
//! スケッチの形(要素と拘束の種類)は知らない。スケッチ(kmb-sketch)が拘束を式に変えて
//! [`system::System`] を作り、ここの [`system::solve_values`]・[`system::solve_moving`] で解く。
//! 解き方を作り直す時は、ここに新しい解き方を足し、同じ [`system::System`] で前の解き方と
//! 比べる。

pub mod curve;
mod ids;
pub mod rows;
pub mod system;
pub mod vec2;

pub use ids::{ConstraintId, EntityId};
