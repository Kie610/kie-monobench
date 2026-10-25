//! 検査(`verify`)の組み立ての前・途中・後に、作り直して要らなくなった古い成果物を消して空きを保つ。
//!
//! 依存の宣言(どれかの `Cargo.toml`)が変わると、その crate から下流の crate の指紋が全部変わり、
//! cargo は古い組の隣に新しい組を作る。2026-10-23 のクラウドでは、jc-interchange の依存を変えた
//! main で `verify jc-session --dependents` を流すと、組み立ての 2 分で deps が 12 → 17 GB、
//! incremental が 5.5 → 8.2 GB になり、空きが 12.6 → 4.0 GB に減った(新しく作ったテストの実行
//! ファイル 17 個すべてに、指紋の違う古い同名の物が残っていた)。揃えた依存(jc-workspace-hack)
//! だけを見る組み立て前の片付け(`free_before_build`)は、この時に働かない。
//!
//! - 組み立ての前(`free_before_build`): 揃えた依存が前の組み立てから変わっていれば incremental を
//!   消し、空きが少なければ incremental の前の回を消す。
//! - 組み立ての途中(`Watch`): 空きが `JC_WATCH_BELOW_GB`(既定 6)GB を切ったら、incremental の
//!   前の回と、この組み立てで同じ名前の新しい物ができたテストの実行ファイルを消す。実行ファイルは
//!   ほかのコンパイルに読まれないので、組み立ての最中に消しても壊れない。同じ名前の別の試験
//!   (jc-mcp の lib と bin など)を消し過ぎた時のため、何か消したら組み立てをもう一度流す
//!   (足りない物だけ作る)。
//! - 組み立ての後(`superseded`): 上に足して、自前の crate の lib(`.rlib`・`.rmeta`・`.d`)と
//!   incremental の crate のフォルダのうち、この組み立てで同じ名前の新しい物ができた古い物を消す。
//!   cargo に問い合わせない(問い合わせは選ばなかった crate まで作るので、空きの少ない時に流すと
//!   そこで尽きる)。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use std::process::Command;

use crate::jobs::Done;
use crate::sweep::{self, fingerprint_of, old_sessions, size_of};
use crate::{Workspace, cargo};

fn modified(p: &Path) -> SystemTime {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// 指紋を除いた名前(`jc_ui-0123456789abcdef` → `jc_ui`、`libjc_ui-….rlib` → `libjc_ui.rlib`)
fn stem_of(name: &str) -> Option<String> {
    let h = fingerprint_of(name)?;
    Some(name.replacen(&format!("-{h}"), "", 1))
}

/// incremental の crate のフォルダを最後に使った時刻。中のファイル(回の中身と錠)の時刻で見て、
/// フォルダの時刻は見ない(前の回を消すと親のフォルダの時刻が新しくなり、使ったように見えていた)
fn used_at(dir: &Path) -> SystemTime {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            let q = e.path();
            if q.is_dir() {
                used_at(&q)
            } else {
                modified(&q)
            }
        })
        .fold(SystemTime::UNIX_EPOCH, SystemTime::max)
}

/// 名前ごとに分け、`since` より後に書いた物がある名前の、`since` より前の物を返す
fn older_siblings(items: Vec<(String, PathBuf, SystemTime)>, since: SystemTime) -> Vec<PathBuf> {
    let mut by: BTreeMap<String, Vec<(PathBuf, SystemTime)>> = BTreeMap::new();
    for (stem, p, t) in items {
        by.entry(stem).or_default().push((p, t));
    }
    by.into_values()
        .filter(|v| v.iter().any(|(_, t)| *t >= since))
        .flat_map(|v| v.into_iter().filter(|(_, t)| *t < since).map(|(p, _)| p))
        .collect()
}

/// deps の中の、この組み立てで同じ名前の新しい物ができた古い物。`libs` が空ならテストと実行
/// ファイル(拡張子の無い物)だけ、空でなければその lib の名前(`jc_ui` など)の `.rlib`・`.rmeta`・
/// `.d` も
pub fn superseded_files(deps: &Path, since: SystemTime, libs: &BTreeSet<String>) -> Vec<PathBuf> {
    let items = fs::read_dir(deps)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = stem_of(&name)?;
            let exe = !name.contains('.');
            let lib = name
                .strip_prefix("lib")
                .unwrap_or(&name)
                .split_once('-')
                .is_some_and(|(n, _)| libs.contains(n))
                && [".rlib", ".rmeta", ".d"].iter().any(|x| name.ends_with(x));
            (exe || lib).then(|| (stem, e.path(), modified(&e.path())))
        })
        .collect();
    older_siblings(items, since)
}

/// incremental の crate のフォルダ(`<crate>-<印>`)のうち、同じ crate の別のフォルダがこの組み立てで
/// 使われ、自分は使われなかった物。lib・テスト・clippy の物が同じ crate に並ぶので、clippy を流さ
/// なかった回は clippy の物も消える(次の clippy は 1 回だけ全部を調べ直す)
pub fn superseded_incremental(inc: &Path, since: SystemTime) -> Vec<PathBuf> {
    let items = fs::read_dir(inc)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (krate, _) = name.rsplit_once('-')?;
            Some((krate.to_string(), e.path(), used_at(&e.path())))
        })
        .collect();
    older_siblings(items, since)
}

/// incremental の前の回(rustc が次に読まない物)
fn old_session_dirs(inc: &Path) -> Vec<PathBuf> {
    fs::read_dir(inc)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|e| old_sessions(&e.path()))
        .collect()
}

/// 消した数と大きさ
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Freed {
    pub count: usize,
    pub bytes: u64,
}

impl Freed {
    pub fn gb(&self) -> f64 {
        self.bytes as f64 / 1e9
    }
}

fn remove(paths: Vec<PathBuf>) -> Freed {
    let mut f = Freed::default();
    for p in paths {
        let b = size_of(&p);
        let gone = if p.is_dir() {
            fs::remove_dir_all(&p).is_ok()
        } else {
            fs::remove_file(&p).is_ok()
        };
        if gone {
            f.count += 1;
            f.bytes += b;
        }
    }
    f
}

/// 組み立ての途中に消してよい物(incremental の前の回と、古いテストの実行ファイル)を消す
pub fn free_during_build(profile: &Path, since: SystemTime) -> Freed {
    let mut v = old_session_dirs(&profile.join("incremental"));
    v.extend(superseded_files(
        &profile.join("deps"),
        since,
        &BTreeSet::new(),
    ));
    remove(v)
}

/// 組み立ての後に、この組み立てで作り直して要らなくなった物を消す(cargo に問い合わせない)
pub fn free_after_build(profile: &Path, since: SystemTime, libs: &BTreeSet<String>) -> Freed {
    let inc = profile.join("incremental");
    let mut v = old_session_dirs(&inc);
    v.extend(superseded_incremental(&inc, since));
    v.extend(superseded_files(&profile.join("deps"), since, libs));
    remove(v)
}

/// 組み立ての間、空きを見張る係
pub struct Watch {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<Freed>>,
}

impl Watch {
    /// `avail` が `below` GB を切るたびに `free_during_build` を流す。`avail` が None(測れない所)
    /// なら何もしない
    pub fn start(
        profile: PathBuf,
        since: SystemTime,
        below: f64,
        avail: impl Fn() -> Option<f64> + Send + 'static,
    ) -> Watch {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let handle = std::thread::spawn(move || {
            let mut total = Freed::default();
            while !s.load(Ordering::Relaxed) {
                if let Some(a) = avail()
                    && a < below
                {
                    let f = free_during_build(&profile, since);
                    if f.count > 0 {
                        println!(
                            "\n== 組み立ての途中で空きが {a:.1} GB({below} GB 未満)になったので、作り直して\
                             要らなくなった物を {} 個({:.1} GB)消しました",
                            f.count,
                            f.gb()
                        );
                    }
                    total.count += f.count;
                    total.bytes += f.bytes;
                }
                for _ in 0..20 {
                    if s.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            total
        });
        Watch {
            stop,
            handle: Some(handle),
        }
    }

    /// 見張りを止め、消した物の合計を返す
    pub fn finish(mut self) -> Freed {
        self.stop.store(true, Ordering::Relaxed);
        self.handle
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default()
    }
}

/// 空きを見張り始める GB(`JC_WATCH_BELOW_GB`。0 で見張らない)
pub fn watch_below_gb(v: Option<&str>) -> f64 {
    v.and_then(|v| v.trim().parse().ok()).unwrap_or(6.0)
}

/// `df -Pk` の出力から空きの KB を読む(2 行目の 4 列目)
pub(crate) fn df_avail_kb(out: &str) -> Option<u64> {
    out.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()
}

/// 空きがこの GB を切っていたら、検査の後に sweep を流す(`JC_SWEEP_BELOW_GB`。0 で流さない)
pub(crate) fn sweep_below_gb(v: Option<&str>) -> u64 {
    v.and_then(|v| v.trim().parse().ok()).unwrap_or(12)
}

/// target のあるディスクの空き(GB)。測れない所(`df` の無い Windows など)では None
pub(crate) fn avail_gb(ws: &Workspace) -> Option<f64> {
    avail_gb_at(&ws.target_dir)
}

pub(crate) fn avail_gb_at(dir: &Path) -> Option<f64> {
    Command::new("df")
        .arg("-Pk")
        .arg(dir)
        .output()
        .ok()
        .and_then(|o| df_avail_kb(&String::from_utf8_lossy(&o.stdout)))
        .map(|kb| kb as f64 / 1e6)
}

/// 揃えた依存の crate の Cargo.toml。前の組み立ての時の写しを target/debug に置いて比べる
pub(crate) const HACK_TOML: &str = "crates/jc-workspace-hack/Cargo.toml";

pub(crate) const HACK_SEEN: &str = "debug/.jc-workspace-hack.toml";

/// 揃えた依存(jc-workspace-hack)が前の組み立てから変わっていれば、組み立ての前に incremental を消す。
///
/// 自前の crate は全部この crate に依存するので、ここが変わると全部の指紋が変わって別の名前で
/// 作り直され、前の incremental は 1 つも読まれなくなる(消しても遅くならない)。組み立ての後の
/// sweep を待つと、古い組の隣に新しい組(クラウドで約 10 GB)を作る間に空きが尽きる。あわせて、空きが
/// `JC_SWEEP_BELOW_GB` を切っていれば、incremental の前の回(rustc が次に読まない物)を消す。
pub(crate) fn free_before_build(ws: &Workspace) {
    let seen = ws.target_dir.join(HACK_SEEN);
    let inc = ws.target_dir.join("debug/incremental");
    if let (Ok(old), Ok(now)) = (fs::read(&seen), fs::read(HACK_TOML))
        && old != now
        && inc.is_dir()
    {
        println!(
            "\n== 揃えた依存({HACK_TOML})が前の組み立てから変わったので、使われなくなる \
             target/debug/incremental を先に消します"
        );
        if let Err(e) = fs::remove_dir_all(&inc) {
            println!("消せませんでした: {e}");
        }
    }
    let below = sweep_below_gb(std::env::var("JC_SWEEP_BELOW_GB").ok().as_deref());
    let Some(avail) = avail_gb(ws) else {
        return;
    };
    if avail >= below as f64 {
        return;
    }
    let old: Vec<PathBuf> = fs::read_dir(&inc)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|e| sweep::old_sessions(&e.path()))
        .collect();
    let n = old
        .iter()
        .filter(|p| {
            if p.is_dir() {
                fs::remove_dir_all(p).is_ok()
            } else {
                fs::remove_file(p).is_ok()
            }
        })
        .count();
    println!(
        "\n== ディスクの空きが {avail:.1} GB({below} GB 未満)なので、組み立ての前に incremental の\
         前の回を {n} 個消しました(空き {:.1} GB)",
        avail_gb(ws).unwrap_or(avail)
    );
}

/// 組み立てが通ったら、その時の揃えた依存を覚える(次の free_before_build が比べる)
pub(crate) fn remember_hack(ws: &Workspace, results: &[Done]) {
    if results.iter().any(|d| d.name == "組み立て" && d.ok)
        && let Ok(now) = fs::read(HACK_TOML)
    {
        let _ = fs::write(ws.target_dir.join(HACK_SEEN), now);
    }
}

/// 検査の後、ディスクの空きが少なければ、この検査で作り直して要らなくなった物を消す。
///
/// cargo に問い合わせる sweep は、問い合わせの中でワークスペースの全部のテストと clippy を作るので、
/// 組み立てが通った `--all` の後だけ流す。選んだ crate だけの検査の後に流すと、選ばなかった crate
/// まで作り直して、そこで空きが尽きていた(2026-10-23、組み立ては通り sweep だけ落ちた)。
pub(crate) fn sweep_if_low(ws: &Workspace, results: &[Done], since: SystemTime, all: bool) {
    let below = sweep_below_gb(std::env::var("JC_SWEEP_BELOW_GB").ok().as_deref());
    let Some(avail) = avail_gb(ws) else {
        return;
    };
    if avail >= below as f64 {
        return;
    }
    let libs: BTreeSet<String> = ws
        .packages
        .iter()
        .map(|p| p.name.replace('-', "_"))
        .collect();
    let f = free_after_build(&ws.target_dir.join("debug"), since, &libs);
    let now = avail_gb(ws).unwrap_or(avail);
    println!(
        "\n== ディスクの空きが {avail:.1} GB({below} GB 未満)なので、この検査で作り直して要らなくなった\
         物を {} 個({:.1} GB)消しました(空き {now:.1} GB)",
        f.count,
        f.gb()
    );
    if now >= below as f64 {
        return;
    }
    if !all || !results.iter().any(|d| d.name == "組み立て" && d.ok) {
        println!(
            "cargo に問い合わせる片付けは --all が通った後だけ流します(足りなければ、--all の後に \
             cargo xtask sweep --yes)"
        );
        return;
    }
    println!("今の版で使わない成果物を消します");
    let so = sweep::SweepOptions {
        yes: true,
        keep_days: 3,
    };
    let release = crate::release::cargo_config(&ws.root, &ws.target_dir);
    let env = crate::release::c_env(&ws.root, &ws.target_dir);
    if let Err(e) = sweep::run(&so, &ws.target_dir, &release, &env, &cargo) {
        println!("sweep に失敗: {e}(手で cargo xtask sweep --yes を流してください)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_free_space_from_df() {
        let out = "Filesystem     1024-blocks      Used Available Capacity Mounted on\n\
                   /dev/vda        263174212  25000000   7300000      78% /\n";
        assert_eq!(df_avail_kb(out), Some(7_300_000));
        assert_eq!(df_avail_kb("Filesystem\n"), None);
        assert_eq!(df_avail_kb(""), None);
    }

    #[test]
    fn sweep_threshold_defaults_to_12_gb() {
        assert_eq!(sweep_below_gb(None), 12);
        assert_eq!(sweep_below_gb(Some("20")), 20);
        assert_eq!(sweep_below_gb(Some("0")), 0);
        assert_eq!(sweep_below_gb(Some("たくさん")), 12);
    }

    fn touch(p: &Path, t: SystemTime) {
        if let Some(d) = p.parent() {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(p, b"x").unwrap();
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jc-space-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn only_older_files_with_a_newer_sibling_are_superseded() {
        let d = tmp("files");
        let since = SystemTime::now() + Duration::from_secs(60);
        let old = since - Duration::from_secs(3600);
        let new = since + Duration::from_secs(3600);
        // 作り直した試験(古い同名あり)、作り直していない試験、依存の lib、自前の lib
        touch(&d.join("jc_ui-0000000000000001"), old);
        touch(&d.join("jc_ui-0000000000000002"), new);
        touch(&d.join("cli-0000000000000003"), old);
        touch(&d.join("libserde-0000000000000004.rlib"), old);
        touch(&d.join("libserde-0000000000000005.rlib"), new);
        touch(&d.join("libjc_ui-0000000000000001.rlib"), old);
        touch(&d.join("libjc_ui-0000000000000002.rlib"), new);
        touch(&d.join("jc_ui-0000000000000001.d"), old);
        touch(&d.join("jc_ui-0000000000000002.d"), new);
        let names = |v: Vec<PathBuf>| -> Vec<String> {
            let mut n: Vec<String> = v
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            n.sort();
            n
        };
        assert_eq!(
            names(superseded_files(&d, since, &BTreeSet::new())),
            ["jc_ui-0000000000000001"]
        );
        let libs = BTreeSet::from(["jc_ui".to_string()]);
        assert_eq!(
            names(superseded_files(&d, since, &libs)),
            [
                "jc_ui-0000000000000001",
                "jc_ui-0000000000000001.d",
                "libjc_ui-0000000000000001.rlib"
            ]
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn incremental_dirs_unused_in_this_build_are_superseded() {
        let d = tmp("inc");
        // フォルダ自身は作った今の時刻(`since` より後)になる。前の回を消した後と同じで、それだけでは
        // 使ったことにしない
        let since = SystemTime::now() - Duration::from_secs(60);
        let old = since - Duration::from_secs(3600);
        let new = since + Duration::from_secs(3600);
        touch(&d.join("jc_ui-aaa/s-1-x/f.bin"), old);
        touch(&d.join("jc_ui-bbb/s-2-y/f.bin"), new);
        touch(&d.join("jc_core-ccc/s-1-z/f.bin"), old);
        let got = superseded_incremental(&d, since);
        assert_eq!(got, [d.join("jc_ui-aaa")]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn watch_threshold_defaults_to_6_gb() {
        assert_eq!(watch_below_gb(None), 6.0);
        assert_eq!(watch_below_gb(Some(" 0 ")), 0.0);
        assert_eq!(watch_below_gb(Some("x")), 6.0);
    }
}
