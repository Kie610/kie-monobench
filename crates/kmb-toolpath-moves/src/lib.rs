//! 工具の動きの部品。加工の戦略が経路を作る時に共通で使う。
//!
//! - 入り方・リード・仕上げの周回([`EntryOpts`]・[`LeadOpts`]・[`FinishOpts`] と、その動き)
//! - 輪郭に沿う動き(1 周、斜めに下りる、タブの上で持ち上げる、入りと抜けの円弧、始点を回す)と、
//!   安全高さへ上げる・下げる動き
//! - 経路のつなぎ([`link`]。削り終えた所から次に削り始める所への、削らない動きを短くする)
//!
//! 加工の機能の形(セットアップ・工具ライブラリ・欄の並び)は kmb-cam-setup が持ち、ここは
//! 動きだけを作る。kmb-cam-setup はここの名前を同じ名前で再公開する。

mod entry;
pub mod link;
mod moves;

pub use entry::{
    DEFAULT_ENTRY_DEG, ENTRY_AREA, EntryArea, EntryKind, EntryOpts, EntryPlan, FinishOpts,
    FinishPlan, Lap, LeadOpts, LeadShape, Leads, add_entry_area, entry_area_of, entry_fields,
    feed_value, finish_fields, finish_offsets, helix_moves, helix_start, lap_start,
    lead_shape_fields, leads, start_after, start_at, walk_moves, zigzag_moves,
};
pub(crate) use moves::seg_length;
pub use moves::{
    LeadArcs, approach, free_on_left, lead_arcs, lift_over_tabs, lift_over_tabs_shaped,
    loop_length, loop_moves, ramp_moves, retract, start_mid_line, start_near, tab_centers,
};

use kmb_core::{Dim, EvalCtx, EvalError};

/// 入り方・リード・仕上げの欄を読む時に要る、工具と切削条件(加工の準備が答える)。
pub trait Cutter {
    /// 工具の半径(mm)
    fn radius(&self) -> f64;
    /// 切削の送り(mm/min)
    fn feed(&self) -> f64;
    /// 真下に下ろす送り(mm/min)
    fn plunge(&self) -> f64;
}

/// 長さの式を読む(長さの単位が要る)。
/// 式 `src` を長さ(mm)として解く。長さの単位でなければ誤り(`what` は欄の名前)。
pub fn length(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if q.dim != Dim::LENGTH {
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は長さの単位が必要です(値: {q})"
        )));
    }
    Ok(q.value)
}

fn invalid(m: impl Into<String>) -> EvalError {
    EvalError::InvalidData(m.into())
}
