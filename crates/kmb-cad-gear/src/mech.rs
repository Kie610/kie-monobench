//! カム・自在継手・クラッチの機能の登録。

pub mod cam;

pub fn register(reg: &mut kmb_core::Registry) {
    cam::register(reg);
}
