//! 動いている間の描画の誤りで落ちず、次の瞬間に描き直すための扱い。
//!
//! 窓の大きさを変えた時やスリープの後には、描く先の面が古くなったり失われたりする。また
//! wgpu は誰も受け取らない誤りを既定ではパニックにするので、egui の描画の 1 回の失敗で
//! アプリごと落ちて作業が消えていた。ここでは誤りをログに出してその回だけ描かずに続け、
//! 次の回に描き直す(3D のビューが自分で受ける誤りは gpu3d の誤りの範囲が先に受ける)。

use eframe::egui;
use eframe::egui_wgpu::SurfaceErrorAction;
use eframe::wgpu;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// 描く先の面が使えない時の扱い。面を作り直すか設定し直し、どちらも次の回の描画を頼むので、
/// 待ちの切れや確かめの誤りでも画面が止まったままにならない。隠れている間だけは描かずに待つ
/// (窓が見えれば描き直しが来る)。
pub fn surface_action(status: &wgpu::CurrentSurfaceTexture) -> SurfaceErrorAction {
    match status {
        wgpu::CurrentSurfaceTexture::Occluded => SurfaceErrorAction::SkipFrame,
        wgpu::CurrentSurfaceTexture::Lost => {
            log::info!("描く先の面を失ったので作り直す");
            SurfaceErrorAction::RecreateSurface
        }
        wgpu::CurrentSurfaceTexture::Outdated => SurfaceErrorAction::Reconfigure,
        other => {
            log::warn!("描く先の面を取れなかったので、設定し直して次の回に描く: {other:?}");
            SurfaceErrorAction::Reconfigure
        }
    }
}

/// 誤りを n 回目にログへ出すか。毎回の描画で同じ誤りが続いてもログを埋めないよう、
/// 1・2・4・8… 回目だけ出す。
fn should_log(n: u64) -> bool {
    n.is_power_of_two()
}

/// 描画装置に、受け手の無い誤りと装置を失った時の扱いを付ける。誤りはパニックにせずログに出し、
/// 次の回の描画を頼む。
pub fn install_device_handlers(ctx: &egui::Context, device: &wgpu::Device) {
    let count = Arc::new(AtomicU64::new(0));
    let repaint = ctx.clone();
    device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
        let n = count.fetch_add(1, Ordering::Relaxed) + 1;
        if should_log(n) {
            log::error!("描画の誤り({n} 回目。この回は描かずに続ける): {e}");
        }
        repaint.request_repaint();
    }));
    device.set_device_lost_callback(|reason, message| {
        // 終わる時の装置の片付けでも呼ばれる
        if reason == wgpu::DeviceLostReason::Destroyed {
            log::debug!("描画装置を片付けた: {message}");
        } else {
            log::error!("描画装置を失った({reason:?}): {message}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_errors_retry_next_frame() {
        use wgpu::CurrentSurfaceTexture as S;
        assert!(matches!(surface_action(&S::Occluded), SurfaceErrorAction::SkipFrame));
        assert!(matches!(surface_action(&S::Lost), SurfaceErrorAction::RecreateSurface));
        // 待ちの切れ・確かめの誤りも描かずに捨てたままにせず、設定し直して次の回を頼む
        for s in [S::Outdated, S::Timeout, S::Validation] {
            assert!(matches!(surface_action(&s), SurfaceErrorAction::Reconfigure));
        }
    }

    #[test]
    fn repeated_errors_log_sparsely() {
        let logged: Vec<u64> = (1..=20).filter(|&n| should_log(n)).collect();
        assert_eq!(logged, vec![1, 2, 4, 8, 16]);
    }
}
