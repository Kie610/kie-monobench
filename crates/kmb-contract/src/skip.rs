//! テストを実行中に飛ばす印。環境(GPU・ffmpeg など)が無くて確かめられない時に、黙って `return`
//! して「通過」に数えないようにする。`cargo xtask verify` は印を数えて「飛ばし(実行中)」に分け、
//! `xtask/skips.toml` の `[runtime]` に無い飛ばしを落とす(docs/verify.md の 1 節)。

/// テストを飛ばす。理由を印として書いてから `return` する(戻り値の無いテストの中で使う)。
///
/// ```ignore
/// let Some(gpu) = gpu() else { kmb_contract::skip!("GPU が無い") };
/// ```
#[macro_export]
macro_rules! skip {
    ($($why:tt)+) => {{
        $crate::skip::mark(&format!($($why)+));
        return;
    }};
}

/// 飛ばしの印を書く。標準エラーと、`JC_SKIP_LOG` があればそのファイル(1 行 `テストの名前\t理由`)へ。
/// テストの名前は、テストを動かす仕組みが付けるスレッドの名前。
pub fn mark(why: &str) {
    let name = std::thread::current()
        .name()
        .unwrap_or("(名前の無いスレッド)")
        .to_string();
    write(std::env::var_os("JC_SKIP_LOG").as_deref(), &name, why);
}

fn write(log: Option<&std::ffi::OsStr>, name: &str, why: &str) {
    let why = why.replace(['\t', '\n'], " ");
    eprintln!("kmb-skip: {name}: {why}");
    if let Some(path) = log {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{name}\t{why}");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_skip_returns_and_writes_the_test_name_and_the_reason() {
        fn body(ran: &mut bool) {
            if std::hint::black_box(true) {
                crate::skip!("GPU が{}", "無い");
            }
            *ran = true;
        }
        let mut ran = false;
        body(&mut ran);
        assert!(!ran);

        let dir = std::env::temp_dir().join(format!("kmb-skip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("skips.log");
        let _ = std::fs::remove_file(&log);
        super::write(Some(log.as_os_str()), "tests::gpu_pick", "GPU が\t無い");
        super::write(None, "tests::other", "書かない");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "tests::gpu_pick\tGPU が 無い\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
