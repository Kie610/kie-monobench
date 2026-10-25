//! Windows で、本体の窓を最初の描画まで画面に出さない(起動時に一瞬出る窓の対策)。
//!
//! eframe は窓を隠したまま作り、最初の描画の後で出す。ところが winit(0.30)は、隠した窓が最大化だと、
//! 窓の設定を 1 つ変えるたびに `ShowWindow(SW_MAXIMIZE)`(表示を伴う)を呼んでから `SW_HIDE` で隠す
//! (platform_impl/windows/window_state.rs の apply_diff)。タスクバーのピンから起動すると窓は作られた時点で
//! 最大化になっているので(誰が最大化するかは [`log_minmax`] の記録で見る)、描く前の窓が 3 回ほど
//! 出ては消える。
//!
//! そこで、作られた本体の窓を DWM で覆い(DWMWA_CLOAK。覆った窓は見えている扱いのまま画面に出ない)、
//! 最初の描画を出した後で外す。最大化などの状態の変わり方は変えず、見え方だけを止める。
//! 窓が出来る瞬間を捕まえるため、窓を作る前に、このスレッドだけのフック(CBT と CALLWNDPROC)を掛ける。
//!
//! OS の関数を呼ぶので、このファイルだけ unsafe を使う。
#![allow(unsafe_code)]

use std::sync::Mutex;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CLOAK, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentThreadId, GetStartupInfoW, STARTF_USESHOWWINDOW, STARTUPINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CBT_CREATEWNDW, CWPSTRUCT, CallNextHookEx, GWL_STYLE, GetClassNameW, GetWindowLongW,
    GetWindowPlacement, GetWindowRect, HCBT_CREATEWND, HCBT_MINMAX, HHOOK, IsIconic,
    IsWindowVisible, IsZoomed, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow, SetWindowPos,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_CALLWNDPROC, WH_CBT, WINDOWPLACEMENT, WM_CREATE,
    WM_NCCREATE, WM_SHOWWINDOW, WM_WINDOWPOSCHANGING, WS_CHILD, WS_MAXIMIZE, WS_VISIBLE,
};

/// winit が作る窓の種類の名前(winit の既定)。
const WINIT_CLASS: &str = "Window Class";

/// 掛けたフック(CBT と、送られた窓のメッセージを見る CALLWNDPROC)と、覆った窓(HWND を数で持つ)。
struct State {
    hooks: Vec<isize>,
    cloaked: Vec<isize>,
    /// 本体の窓(最初に作られた winit の最上位の窓。無ければ 0)
    main: isize,
}

static STATE: Mutex<State> = Mutex::new(State {
    hooks: Vec::new(),
    cloaked: Vec::new(),
    main: 0,
});

/// 窓を作る前に呼ぶ。起動元が渡した窓の出し方をログに書き、フックを掛ける。
pub fn before_window() {
    let mut si = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: si は呼ぶ側が持つ正しい大きさの構造体
    unsafe { GetStartupInfoW(&mut si) };
    log::info!(
        "起動元からの窓の出し方: dwFlags=0x{:X}、wShowWindow={}",
        si.dwFlags,
        if si.dwFlags & STARTF_USESHOWWINDOW != 0 {
            si.wShowWindow.to_string()
        } else {
            "指定なし".into()
        }
    );
    let mut hooks = Vec::new();
    for (id, proc) in [
        (WH_CBT, cbt as HookProc),
        (WH_CALLWNDPROC, call_wnd_proc as HookProc),
    ] {
        // SAFETY: このスレッドだけのフック(hmod は無し)。after_first_frame で外す
        let hook = unsafe {
            SetWindowsHookExW(id, Some(proc), std::ptr::null_mut(), GetCurrentThreadId())
        };
        if hook.is_null() {
            log::warn!(
                "起動の窓を覆うフック({id})を掛けられません: {}",
                std::io::Error::last_os_error()
            );
        } else {
            hooks.push(hook as isize);
        }
    }
    if let Ok(mut s) = STATE.lock() {
        s.hooks = hooks;
    }
}

type HookProc = unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT;

/// 最初の描画を出した後に呼ぶ。覆いを外し、フックを外す。本体の窓を今のモニターの作業域に収め、
/// 1 回だけ前面に出す。2 回目からは何もしない。
pub fn after_first_frame() {
    let (hooks, cloaked, main) = match STATE.lock() {
        Ok(mut s) => (
            std::mem::take(&mut s.hooks),
            std::mem::take(&mut s.cloaked),
            std::mem::take(&mut s.main),
        ),
        Err(_) => return,
    };
    if main != 0 {
        fit_to_work_area(main as HWND);
        // 最初の画面が出た時に一度だけ前面に出す(起動を待つ間にほかの窓を触ると、後ろに
        // 開いたままになる)。OS が断ればタスクバーが点滅するだけ
        // SAFETY: main はこのスレッドが作った本体の窓
        let raised = unsafe { SetForegroundWindow(main as HWND) } != 0;
        log::info!(
            "起動の窓を前面に出す: {}",
            if raised {
                "出した"
            } else {
                "OS が断った"
            }
        );
    }
    for hook in hooks {
        // SAFETY: before_window で掛けたフック
        unsafe { UnhookWindowsHookEx(hook as HHOOK) };
    }
    for hwnd in cloaked {
        let hwnd = hwnd as HWND;
        if set_cloak(hwnd, false) {
            log::info!("起動の窓の覆いを外した({})", describe(hwnd));
        }
    }
}

/// CBT のフック。最大化・最小化・戻すの要求をログに書き(止めない)、その前に窓を覆う。
unsafe extern "system" fn cbt(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hwnd = wparam as HWND;
    match code as u32 {
        HCBT_CREATEWND if is_winit_top_level(hwnd) => {
            // SAFETY: HCBT_CREATEWND の lparam は CBT_CREATEWNDW を指す
            let style = unsafe { (*(*(lparam as *const CBT_CREATEWNDW)).lpcs).style } as u32;
            log::info!(
                "起動の窓を作る(作る時の style=0x{style:08X}、最大化 {}、見える {})",
                style & WS_MAXIMIZE != 0,
                style & WS_VISIBLE != 0
            );
            if let Ok(mut s) = STATE.lock()
                && s.main == 0
            {
                s.main = hwnd as isize;
            }
        }
        HCBT_MINMAX if is_winit_top_level(hwnd) => {
            ensure_cloaked(hwnd, "最大化などの前");
            log_minmax(hwnd, lparam);
        }
        _ => {}
    }
    // SAFETY: 次のフックへ渡す(hhk は無視される)
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// 窓へ送られたメッセージを、窓が受け取る前に見るフック。窓が出来たら(WM_NCCREATE。winit はここで
/// 窓の状態を当て、最大化なら表示も伴う)、遅くとも出る直前(WM_SHOWWINDOW・WM_WINDOWPOSCHANGING)までに覆う。
/// HCBT_CREATEWND の時点では DWM がまだ窓を知らず、覆えない(E_HANDLE。2026-10-21 PC で確認)。
unsafe extern "system" fn call_wnd_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: WH_CALLWNDPROC の lparam は CWPSTRUCT を指す
        let m = unsafe { &*(lparam as *const CWPSTRUCT) };
        let stage = match m.message {
            WM_NCCREATE => Some("WM_NCCREATE"),
            WM_CREATE => Some("WM_CREATE"),
            WM_SHOWWINDOW => Some("WM_SHOWWINDOW"),
            WM_WINDOWPOSCHANGING => Some("WM_WINDOWPOSCHANGING"),
            _ => None,
        };
        if let Some(stage) = stage
            && is_winit_top_level(m.hwnd)
        {
            ensure_cloaked(m.hwnd, stage);
        }
    }
    // SAFETY: 次のフックへ渡す(hhk は無視される)
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// まだ覆っていなければ覆う(覆えなければ次の機会に試し直す)。
fn ensure_cloaked(hwnd: HWND, stage: &str) {
    let Ok(mut s) = STATE.lock() else {
        return;
    };
    // フックを外した後(最初の画面の後)や、もう覆った窓は何もしない
    if s.hooks.is_empty() || s.cloaked.contains(&(hwnd as isize)) {
        return;
    }
    if set_cloak(hwnd, true) {
        s.cloaked.push(hwnd as isize);
        drop(s);
        log::info!("起動の窓を覆った({stage}、{})", describe(hwnd));
    }
}

/// 最大化などの要求を、誰が出したか(呼び出しの積み)と一緒に書く。
fn log_minmax(hwnd: HWND, lparam: LPARAM) {
    let cmd = (lparam & 0xFFFF) as u32;
    let name = match cmd {
        0 => "SW_HIDE",
        1 => "SW_SHOWNORMAL",
        2 => "SW_SHOWMINIMIZED",
        3 => "SW_MAXIMIZE",
        6 => "SW_MINIMIZE",
        9 => "SW_RESTORE",
        _ => "その他",
    };
    let trace = std::backtrace::Backtrace::force_capture().to_string();
    // 関数名の行だけを、このフックの外から(OS と winit の側)数行
    let frames: Vec<&str> = trace
        .lines()
        .map(str::trim)
        .filter(|l| {
            l.split_once(": ")
                .is_some_and(|(n, _)| n.parse::<u32>().is_ok())
        })
        .map(|l| l.split_once(": ").map_or(l, |(_, f)| f))
        .skip_while(|f| f.contains("backtrace") || f.contains("win_start"))
        .take(10)
        .collect();
    log::info!(
        "起動の窓に {name}({cmd})の要求({})。呼び出し: {}",
        describe(hwnd),
        frames.join(" ← ")
    );
}

/// 窓の今の状態(見える・最大化・元の大きさ)。
fn describe(hwnd: HWND) -> String {
    let mut wp = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: hwnd はこのスレッドの窓、wp は正しい大きさの構造体
    let (visible, ok) = unsafe {
        (
            IsWindowVisible(hwnd) != 0,
            GetWindowPlacement(hwnd, &mut wp) != 0,
        )
    };
    let r = wp.rcNormalPosition;
    if ok {
        format!(
            "見える {visible}、showCmd {}、元の位置と大きさ ({},{}) {}x{}",
            wp.showCmd,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top
        )
    } else {
        format!("見える {visible}")
    }
}

/// 窓を、今いるモニター(重なりのいちばん大きいモニター)の作業域(タスクバーを除いた所)に収める。
/// 外したモニターの位置に窓が残ったり、窓の上の帯がタスクバーや画面の外に隠れたりして、掴めなく
/// なるのを防ぐ(eframe は覚えた位置をモニター全体には収めるが、作業域は見ない)。
/// 最大化・最小化の窓は OS が置くので触らない。見える枠(DWM の枠。周りの透明な縁を除く)で比べる。
fn fit_to_work_area(hwnd: HWND) {
    // SAFETY: hwnd はこのスレッドの窓
    if unsafe { IsZoomed(hwnd) != 0 || IsIconic(hwnd) != 0 } {
        return;
    }
    let mut outer = RECT::default();
    let mut seen = RECT::default();
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: どれも正しい大きさの構造体を渡す
    let ok = unsafe {
        GetWindowRect(hwnd, &mut outer) != 0
            && DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                (&mut seen as *mut RECT).cast(),
                size_of::<RECT>() as u32,
            ) >= 0
            && {
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0
            }
    };
    if !ok {
        return;
    }
    let r = |r: RECT| [r.left, r.top, r.right, r.bottom];
    let (seen, work) = (r(seen), r(info.rcWork));
    let fit = crate::fit_rect(seen, work);
    if fit == seen {
        return;
    }
    // 見える枠を動かした分と縮めた分を、透明な縁を含む窓の枠に当てる
    let outer = r(outer);
    let (dx, dy) = (fit[0] - seen[0], fit[1] - seen[1]);
    let (dw, dh) = (
        (fit[2] - fit[0]) - (seen[2] - seen[0]),
        (fit[3] - fit[1]) - (seen[3] - seen[1]),
    );
    let (x, y) = (outer[0] + dx, outer[1] + dy);
    let (w, h) = (outer[2] - outer[0] + dw, outer[3] - outer[1] + dh);
    // SAFETY: hwnd はこのスレッドの窓
    let moved = unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        ) != 0
    };
    log::info!(
        "起動の窓を作業域 {work:?} に収める: 見える枠 {seen:?} → {fit:?}({})",
        if moved {
            "動かした"
        } else {
            "動かせない"
        }
    );
}

/// winit が作った最上位の窓か。
fn is_winit_top_level(hwnd: HWND) -> bool {
    let mut buf = [0u16; 64];
    // SAFETY: buf の長さを渡す
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    // SAFETY: hwnd はフックに渡された窓
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
    n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == WINIT_CLASS && style & WS_CHILD == 0
}

/// DWM で窓を覆う・外す。できたか。
fn set_cloak(hwnd: HWND, on: bool) -> bool {
    let v: i32 = on.into();
    // SAFETY: v は 4 バイトの BOOL
    let hr = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK as u32,
            (&v as *const i32).cast(),
            size_of::<i32>() as u32,
        )
    };
    if hr < 0 {
        log::info!(
            "起動の窓を{}: HRESULT 0x{hr:08X}",
            if on {
                "まだ覆えない"
            } else {
                "覆いを外せない"
            }
        );
    }
    hr >= 0
}
