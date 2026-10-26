//! 3D マウス(6 軸の多軸の機器)を探して読む。画面はこの値で 3D のビューを回す・画面移動・ズームする。
//!
//! 機器は OS の HID の口から直に読む(Windows は HID の関数、Linux は `/dev/hidraw*`)。メーカーの
//! 開発キットとドライバの口は使わない。探す時は HID の用途が「多軸の機器」(Generic Desktop /
//! Multi-axis Controller)で、3D マウスを作る会社の番号の物だけを読むために開く。キーボードや普通の
//! マウスは用途を見るだけで中身を読まない(同じ会社の番号でも用途で外す)。
//!
//! 読み取りは裏のスレッドで行い、画面は毎コマ [`Mouse3d::snapshot`] で今の値を受け取る。状態は
//! この値の中に持ち、プロセス全体の static に置かない。egui は知らない(描き直しの頼みは
//! [`Mouse3d::set_wake`] で受けた関数を呼ぶ)。

pub mod report;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

use report::{Axes, Report};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// 3D マウスを作る会社の USB の番号。用途が多軸の機器で、この番号の物だけを使う。
pub const VENDORS: [u16; 2] = [0x256F, 0x046D];

/// HID の用途の頁 Generic Desktop と、その中の多軸の機器。
pub const USAGE_PAGE: u16 = 0x01;
pub const USAGE_MULTI_AXIS: u16 = 0x08;

/// 機器が 1 つも無い時に探し直す間隔(抜き差しに追従する)。
const RESCAN: Duration = Duration::from_secs(3);

/// 用途と会社の番号から、読む機器かを決める。
pub fn wanted(vendor: u16, usage_page: u16, usage: u16) -> bool {
    usage_page == USAGE_PAGE && usage == USAGE_MULTI_AXIS && VENDORS.contains(&vendor)
}

/// 読み取りの状態(設定の頁に 1 行で出す)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// 機器を探している(見つからない)
    Searching,
    /// つながって読んでいる
    Connected { name: String },
    /// 機器はあるが、読む権限が無い(Linux の既定)
    NoPermission,
    /// この OS では読めない
    Unsupported,
}

impl Status {
    /// 設定の頁に出す文。
    pub fn label(&self) -> String {
        match self {
            Status::Searching => "見つからない(つなぐと 3 秒ほどで使える)".into(),
            Status::Connected { name } => format!("つながった: {name}"),
            Status::NoPermission if cfg!(target_os = "linux") => {
                "機器はあるが読む権限が無い。udev の規則で hidraw を読めるようにする(例: \
                 KERNEL==\"hidraw*\", ATTRS{idVendor}==\"256f\", TAG+=\"uaccess\" を \
                 /etc/udev/rules.d/ に置き、差し直す)"
                    .into()
            }
            Status::NoPermission => "機器はあるが読む権限が無い".into(),
            Status::Unsupported => "この OS ではまだ使えない".into(),
        }
    }
}

/// ある時の 3D マウスの値。
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// 移動 X・Y・Z と回転 RX・RY・RZ(-1〜1。手を放していれば 0)
    pub axes: [f32; 6],
    /// 前の [`Mouse3d::snapshot`] から押されたボタン(1 から、押した順)
    pub buttons_pressed: Vec<u8>,
    pub status: Status,
}

type Wake = Box<dyn Fn() + Send + Sync>;

struct State {
    axes: Axes,
    status: Status,
    wake: Option<Wake>,
}

struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 3D マウスの読み手。落とすと裏のスレッドに止まる合図を出す。
pub struct Mouse3d {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Mouse3d {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mouse3d")
            .field("status", &self.shared.lock().status)
            .finish()
    }
}

/// 3D マウスを探し始める。読める OS では裏のスレッドで探して読み、それ以外では
/// [`Status::Unsupported`] のまま何もしない。
pub fn open() -> Mouse3d {
    let m = Mouse3d::with_status(Status::Searching);
    #[cfg(any(target_os = "linux", windows))]
    {
        let link = Link {
            shared: m.shared.clone(),
        };
        let spawned = std::thread::Builder::new()
            .name("3D マウス".into())
            .spawn(move || link.run());
        if let Err(e) = spawned {
            log::warn!("3D マウスを読むスレッドを作れない: {e}");
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        m.shared.lock().status = Status::Unsupported;
        log::info!("3D マウス: この OS ではまだ読めない");
    }
    m
}

impl Mouse3d {
    fn with_status(status: Status) -> Self {
        Mouse3d {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    axes: Axes::default(),
                    status,
                    wake: None,
                }),
                stop: AtomicBool::new(false),
            }),
        }
    }

    /// 試験用の作り物。機器は探さず、[`Mouse3d::feed`] で入れた報告だけを読む。
    pub fn fake() -> Self {
        Mouse3d::with_status(Status::Connected {
            name: "作り物の 3D マウス".into(),
        })
    }

    /// 作り物に報告を 1 つ入れる(頭の 1 バイトが報告の番号)。
    pub fn feed(&self, bytes: &[u8]) {
        let mut s = self.shared.lock();
        let r = report::parse(bytes, s.axes.rotate_seen);
        s.axes.apply(&r, Instant::now());
    }

    /// 手を放した時や機器が動き出した時に、描き直しを頼む関数を渡す(画面の Context が出来てから)。
    pub fn set_wake(&self, wake: impl Fn() + Send + Sync + 'static) {
        self.shared.lock().wake = Some(Box::new(wake));
    }

    /// 今の状態。
    pub fn status(&self) -> Status {
        self.shared.lock().status.clone()
    }

    /// 今の値と、前に呼んだ時から押されたボタン。毎コマ 1 回呼ぶ。
    pub fn snapshot(&self) -> Snapshot {
        let mut s = self.shared.lock();
        Snapshot {
            axes: s.axes.at(Instant::now()),
            buttons_pressed: std::mem::take(&mut s.axes.pressed),
            status: s.status.clone(),
        }
    }
}

impl Drop for Mouse3d {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

/// 見つけた機器。
#[derive(Debug, Clone)]
pub(crate) struct Device {
    /// 開く所(Linux は `/dev/hidrawN`、Windows は機器のパス)
    pub path: std::path::PathBuf,
    pub name: String,
    pub vendor: u16,
    pub product: u16,
    /// 報告の形の手がかり(ログに出す)
    pub shape: String,
}

/// 読み取りのスレッドが持つ、読み手との繋がり。
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
pub(crate) struct Link {
    shared: Arc<Shared>,
}

#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
impl Link {
    pub(crate) fn stopped(&self) -> bool {
        self.shared.stop.load(Ordering::Relaxed)
    }

    fn set_status(&self, status: Status) {
        let mut s = self.shared.lock();
        if s.status != status {
            s.status = status;
            s.axes = Axes::default();
            if let Some(w) = &s.wake {
                w();
            }
        }
    }

    /// 止まる合図が来るまで、探す・読むを繰り返す。
    fn run(self) {
        #[cfg(target_os = "linux")]
        use linux as os;
        #[cfg(windows)]
        use windows as os;
        let mut told_permission = false;
        while !self.stopped() {
            let mut denied = false;
            for dev in &os::scan() {
                let mut reader = Reader::new(&self, dev);
                match os::read(dev, &mut reader) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        denied = true;
                        if !told_permission {
                            log::warn!(
                                "3D マウス {}({:04x}:{:04x})を読む権限が無い({}): {e}",
                                dev.name,
                                dev.vendor,
                                dev.product,
                                dev.path.display()
                            );
                            told_permission = true;
                        }
                        continue;
                    }
                    Err(e) => log::warn!("3D マウス {} を読めなくなった: {e}", dev.name),
                }
                if reader.connected {
                    log::info!("3D マウス {} が外れた", dev.name);
                }
                // 使う機器は 1 つ。読んでいた機器が外れたら、間を置いて探し直す
                denied = false;
                break;
            }
            if self.stopped() {
                return;
            }
            self.set_status(if denied {
                Status::NoPermission
            } else {
                Status::Searching
            });
            let until = Instant::now() + RESCAN;
            while Instant::now() < until && !self.stopped() {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

/// 1 つの機器を読んでいる間の控え。報告を受けて読み手の値を更新し、形をログに出す。
pub(crate) struct Reader<'a> {
    link: &'a Link,
    dev: &'a Device,
    connected: bool,
    /// 受けた報告の数(初めの数回だけ番号と長さを debug に出す)
    count: u32,
    /// ログに形を出した報告の番号
    told: Vec<u8>,
}

/// 番号と長さを debug に出す、つながってからの報告の数。
const DEBUG_REPORTS: u32 = 8;

#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
impl<'a> Reader<'a> {
    fn new(link: &'a Link, dev: &'a Device) -> Self {
        Reader {
            link,
            dev,
            connected: false,
            count: 0,
            told: Vec::new(),
        }
    }

    pub(crate) fn stopped(&self) -> bool {
        self.link.stopped()
    }

    /// 開けた時に 1 回呼ぶ。
    pub(crate) fn connected(&mut self) {
        let d = self.dev;
        log::info!(
            "3D マウスがつながった: {}({:04x}:{:04x}、{}、{})",
            d.name,
            d.vendor,
            d.product,
            d.shape,
            d.path.display()
        );
        self.connected = true;
        self.link.set_status(Status::Connected {
            name: d.name.clone(),
        });
    }

    /// 報告を 1 つ受ける(頭の 1 バイトが番号)。
    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        let now = Instant::now();
        let mut s = self.link.shared.lock();
        let r = report::parse(bytes, s.axes.rotate_seen);
        let id = bytes.first().copied().unwrap_or(0);
        if self.count < DEBUG_REPORTS {
            self.count += 1;
            log::debug!("3D マウスの報告: 番号 {id}、{} バイト", bytes.len());
        }
        if !self.told.contains(&id) {
            self.told.push(id);
            let len = bytes.len().saturating_sub(1);
            match &r {
                Report::Unknown { .. } => {
                    log::warn!("3D マウスの読めない報告: 番号 {id}、中身 {len} バイト")
                }
                Report::Translate(_) => log::info!("3D マウスの報告 {id}(中身 {len} バイト)を移動と読む"),
                Report::Rotate(_) => log::info!("3D マウスの報告 {id}(中身 {len} バイト)を回転と読む"),
                Report::Both(_) => {
                    log::info!("3D マウスの報告 {id}(中身 {len} バイト)を移動と回転と読む")
                }
                Report::Buttons(_) => log::info!("3D マウスの報告 {id}(中身 {len} バイト)をボタンと読む"),
            }
        }
        let was_still = s.axes.at(now) == [0.0; 6];
        s.axes.apply(&r, now);
        if let Report::Buttons(bits) = r
            && bits > 0b11
        {
            log::debug!("3D マウスのボタン 3 番目から先は使わない(ビット {bits:#b})");
        }
        let moving = s.axes.at(now) != [0.0; 6];
        let woke = (was_still && moving) || !s.axes.pressed.is_empty();
        if woke && let Some(w) = &s.wake {
            w();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: 作り物に入れた報告が値とボタンになり、ボタンは 1 回だけ渡り、
    /// 多軸の機器で会社の番号の合う物だけを読む。
    #[test]
    fn fake_and_wanted() {
        let m = Mouse3d::fake();
        m.feed(&[1, 0x5e, 0x01, 0, 0, 0, 0]);
        m.feed(&[3, 1]);
        let s = m.snapshot();
        assert_eq!(s.axes[0], 1.0);
        assert_eq!(s.buttons_pressed, vec![1]);
        assert!(matches!(s.status, Status::Connected { .. }));
        assert!(m.snapshot().buttons_pressed.is_empty());

        assert!(wanted(0x256F, 1, 8));
        assert!(wanted(0x046D, 1, 8));
        // 同じ会社のキーボード(1/6)・マウス(1/2)は読まない
        assert!(!wanted(0x046D, 1, 6));
        assert!(!wanted(0x046D, 1, 2));
        assert!(!wanted(0x1234, 1, 8));
    }
}
