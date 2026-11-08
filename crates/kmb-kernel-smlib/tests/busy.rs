//! ほかの文書の計算が SMLib を使っている間に、錠を待たない糸(画面)が固まらずに戻るか。
//! SMLib の錠はすべての文書で 1 つなので(src/ffi.rs)、待たない糸は「使用中」で戻る。
use kmb_contract::kernel::plate_profile;
use kmb_core::kernel::{Kernel, KernelError, kernel_busy_count, kernel_waits, set_kernel_wait};
use kmb_kernel_smlib::{SmlibKernel, occupy};
use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::Duration;

/// 戻らなかった時に試験を止めるまでの時間(速さの確かめではなく、固まった時の打ち切り)。
const HANG: Duration = Duration::from_secs(60);

/// 錠を待たない糸で `f` を呼び、答えを返す。戻らなければ固まったとして落とす。
fn without_waiting<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> (R, u64) {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        assert!(kernel_waits(), "糸の既定は待つ");
        set_kernel_wait(false);
        let before = kernel_busy_count();
        let r = f();
        let _ = tx.send((r, kernel_busy_count() - before));
    });
    rx.recv_timeout(HANG)
        .expect("錠を待たない糸が、ほかの文書の計算の終わりを待って固まった")
}

#[test]
fn a_screen_thread_is_not_held_by_another_documents_computation() {
    // 文書 B のカーネルと、その中の板
    let b = Arc::new(SmlibKernel::new());
    let plate = b.extrude(&plate_profile(), 0.0, 5.0, "f1").unwrap();
    // 文書 A の重い計算が SMLib を使っている間
    let busy = occupy(HANG);
    let k = b.clone();
    let ((bounds, edges, made), count) = without_waiting(move || {
        (
            k.bounds(plate),
            k.edges(plate),
            k.extrude(&plate_profile(), 0.0, 3.0, "f2"),
        )
    });
    assert_eq!(bounds, Err(KernelError::Busy));
    assert_eq!(edges, Err(KernelError::Busy));
    assert_eq!(made, Err(KernelError::Busy));
    assert_eq!(count, 3, "待たずに戻った数を数える");
    assert!(busy.still_held());
    // 計算が終われば、同じ問い合わせに答える
    drop(busy);
    let k = b.clone();
    let ((bounds, edges), count) = without_waiting(move || (k.bounds(plate), k.edges(plate)));
    assert!(bounds.is_ok() && edges.is_ok_and(|e| !e.is_empty()));
    assert_eq!(count, 0);
}

#[test]
fn a_computing_thread_still_waits_and_gets_its_answer() {
    let b = Arc::new(SmlibKernel::new());
    let plate = b.extrude(&plate_profile(), 0.0, 5.0, "f1").unwrap();
    let busy = occupy(HANG);
    let k = b.clone();
    let (tx, rx) = channel();
    // 待つ糸(裏の再計算)は錠が空くまで待ち、「使用中」で失敗しない
    std::thread::spawn(move || {
        let _ = tx.send(k.bounds(plate));
    });
    drop(busy);
    assert!(rx.recv_timeout(HANG).expect("錠が空いても戻らない").is_ok());
}
