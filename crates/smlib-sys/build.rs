//! usd-brep(SMLib + SM_API)を cc でビルドする。
//!
//! ソースはリポジトリの third_party にある(third_party/README.md)。環境変数で別の場所を指せる。
//! - `USD_BREP_DIR`: usd-brep のチェックアウト(既定は third_party/usd-brep、1.0.0 の commit c9c5979)
//! - `SMLIB_PATCH_DIR`: 不具合を直した写し(既定は third_party/smlibpy-patches)。上流 1.0.0 の
//!   ブーリアンには解放済みメモリを使う不具合があるので、空にするとビルドを止める
//!   (`SMLIB_ALLOW_UNPATCHED=1` で通す)
//!
//! OCCT・OpenUSD に依存する部分(OCCT_BREP_*、SM_API_USD など)は使わない。
//!
//! C++ の結果は target の下の `smlib-cache/` に取っておき、ソース・コンパイラ・オプションが
//! 同じなら作り直さずに写す。cargo はこのビルドスクリプトを、テストと clippy、開発とリリース、
//! Rust の版の切り替えなどで別々に走らせ直す。C++ は数百ファイルあり、1 回に数分から十数分
//! かかるので、その度にコンパイルし直さない(docs/verify.md の 6 節)。
//! - `SMLIB_CACHE_DIR`: 取っておく場所を変える。`off` にすると使わない(毎回コンパイルする)
//! - `SMLIB_SHARED_CACHE`: コンテナをまたいで残る置き場所(クラウドの作業場。`.cargo/config.toml` で指す)
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn cpp_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{} を読めません: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "cpp"))
        .collect();
    v.sort();
    v
}

fn main() {
    println!("cargo:rerun-if-env-changed=USD_BREP_DIR");
    println!("cargo:rerun-if-env-changed=SMLIB_PATCH_DIR");
    println!("cargo:rerun-if-env-changed=SMLIB_ALLOW_UNPATCHED");
    // ソースと見出しは、このフォルダ(ビルドスクリプトが動く場所)からの相対のパスで渡す。C++ の
    // `__FILE__`(SMLib のエラーの文の FILE_NAME)は渡したパスのまま実行ファイルに入るので、絶対の
    // パスで渡すと、ビルドした PC のリポジトリの置き場所が配る exe に残る(xtask/src/release.rs)。
    // 指紋も PC の置き場所で変わらなくなる
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let third_party = PathBuf::from("../../third_party");
    let root = env::var_os("USD_BREP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| third_party.join("usd-brep"));
    let smlib = root.join("source/SMLib");
    let smapi = root.join("source/SM_API");
    assert!(
        smlib.join("src/SmBrep.cpp").exists(),
        "{} に SMLib がありません(third_party/README.md)",
        smlib.display()
    );
    for d in ["src", "inc"] {
        println!("cargo:rerun-if-changed={}", smlib.join(d).display());
        println!("cargo:rerun-if-changed={}", smapi.join(d).display());
    }

    let mut sources = cpp_files(&smlib.join("src"));
    let patch_dir = env::var_os("SMLIB_PATCH_DIR")
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_else(|| third_party.join("smlibpy-patches").display().to_string());
    match patch_dir {
        dir if !dir.is_empty() => {
            for p in cpp_files(Path::new(&dir)) {
                let name = p.file_name().unwrap().to_owned();
                let before = sources.len();
                sources.retain(|s| s.file_name().unwrap() != name);
                assert_eq!(
                    before,
                    sources.len() + 1,
                    "修正 {} の元が見つかりません",
                    p.display()
                );
                println!("cargo:rerun-if-changed={}", p.display());
                sources.push(p);
            }
        }
        _ if env::var("SMLIB_ALLOW_UNPATCHED").as_deref() == Ok("1") => {
            println!(
                "cargo:warning=SMLIB_PATCH_DIR が空です。上流 1.0.0 のブーリアンには解放済みメモリを使う不具合があります"
            );
        }
        _ => panic!(
            "SMLIB_PATCH_DIR に smlibpy の third_party/patches/SMLib/src を指定してください \
             (上流 1.0.0 のブーリアンの不具合の修正。試すだけなら SMLIB_ALLOW_UNPATCHED=1)"
        ),
    }
    let api_sources = cpp_files(&smapi.join("src"));

    let msvc = env::var("TARGET").unwrap().contains("msvc");
    // SMLib と SM_API はどちらも StdAfx.h を持つので、見出しを探す順を分けてビルドする
    let build = |files: &[PathBuf], includes: &[PathBuf]| {
        let mut b = cc::Build::new();
        b.cpp(true)
            .std("c++17")
            .files(files)
            .define("NOMINMAX", None)
            .define("SM_GRAPHICS_CALLBACKS", "1")
            .define("SM_API_EXPORTS", None)
            .define("NDEBUG", None)
            .warnings(false)
            // カーネルは最適化なしだと数倍遅いので、開発ビルドでも最適化する
            .opt_level(2)
            .debug(false);
        for i in includes {
            b.include(i);
        }
        if msvc {
            b.flag("/utf-8").flag("/bigobj").flag("/EHsc");
        } else {
            b.flag("-w");
        }
        b
    };
    // リンクの順: SM_API が SMLib を使うので、SM_API を先に並べる
    let specs = [
        (
            "smapi",
            &api_sources,
            vec![smapi.join("inc"), smlib.join("inc")],
        ),
        ("smlib", &sources, vec![smlib.join("inc")]),
    ];
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let static_inc = out.join("static-inc");
    write_static_headers(&static_inc, &[smapi.join("inc"), smlib.join("inc")]);
    // 指紋は書き直した見出しの場所(OUT_DIR で変わる)を除いた形で取る。書き直しの中身は
    // 元の見出しと build.rs で決まり、どちらも指紋に入る
    let libs = specs.map(|(name, files, includes)| {
        let with_static: Vec<PathBuf> = std::iter::once(static_inc.clone())
            .chain(includes.iter().cloned())
            .collect();
        (name, build(files, &includes), build(files, &with_static))
    });
    let file_name = |name: &str| {
        if msvc {
            format!("{name}.lib")
        } else {
            format!("lib{name}.a")
        }
    };
    // 指紋は毎回計算する(コンパイラを調べる時に cc が rerun-if-env-changed を出すので)
    let key = {
        let mut h = Fnv::new();
        h.field(env::var("TARGET").unwrap().as_bytes());
        h.field(&fs::read(manifest.join("build.rs")).unwrap_or_default());
        for (name, b, _) in &libs {
            h.field(name.as_bytes());
            h.tool(b);
        }
        // .cpp はビルドに使うもの(直した写しを含む)、見出しはフォルダの中の全部
        h.files(sources.iter().chain(&api_sources));
        for dir in [
            smlib.join("src"),
            smlib.join("inc"),
            smapi.join("src"),
            smapi.join("inc"),
        ] {
            h.headers(&dir);
        }
        format!("{:016x}", h.0)
    };
    let cache = cache_dir(&out).map(|root| root.join(&key));
    let shared = shared_cache_dir().map(|root| root.join(&key));
    let names: Vec<String> = libs.iter().map(|(n, _, _)| file_name(n)).collect();
    let copy_from = |c: &PathBuf| {
        names
            .iter()
            .all(|n| fs::copy(c.join(n), out.join(n)).is_ok())
    };
    let hit = cache.as_ref().is_some_and(copy_from);
    let shared_hit = !hit && shared.as_ref().is_some_and(copy_from);
    if shared_hit {
        if let Some(c) = &cache {
            store(c, &out, &names);
        }
    } else if !hit {
        for (name, _, b) in &libs {
            b.clone().cargo_metadata(false).out_dir(&out).compile(name);
        }
        for c in cache.iter().chain(&shared) {
            store(c, &out, &names);
        }
    }
    // リンクの指示は cc に任せず自分で出す(取っておいた結果を写した時も同じにする)
    println!("cargo:rustc-link-search=native={}", out.display());
    for (name, _, _) in &libs {
        println!("cargo:rustc-link-lib=static={name}");
    }
    if !msvc {
        let stdlib = if env::var("TARGET").unwrap().contains("apple") {
            "c++"
        } else {
            "stdc++"
        };
        println!("cargo:rustc-link-lib={stdlib}");
    }

    // 中継層(kmb-kernel-smlib)も書き直した見出しを先に探す
    // 中継層は別のフォルダで動くので、絶対のパスで渡す
    println!(
        "cargo:include={};{};{}",
        static_inc.display(),
        manifest.join(smapi.join("inc")).display(),
        manifest.join(smlib.join("inc")).display()
    );
}

/// Windows 向けの見出しを、静的ライブラリ向けに書き直して `dir` に置く(Linux 向けと同じ形にする)。
///
/// 上流の見出しは DLL を作る前提で、Windows では (1) クラスに `__declspec(dllexport)` を付け、
/// (2) `SM_TARRAY_TEMPLATE_PREDECLARATION` などで多くのテンプレートを明示的に実体化する。
/// どちらも見出しを読む全ての .cpp(SmSmlibAll.h で全部を読む)に全クラス・全テンプレートの
/// 関数を書き出させ、MSVC では 1 ファイル約 9 MB、smlib.lib が約 2 GB になった(Linux は 39 MB)。
/// ここでは静的にリンクするだけなので、(1) は空にし、(2) は Linux と同じく何もしない形にする。
/// 書き直すのは該当する見出しだけで、`dir` を見出しを探す順の先頭に置く。
fn write_static_headers(dir: &Path, inc_dirs: &[PathBuf]) {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{} を作れません: {e}", dir.display()));
    let mut changed = 0;
    for inc in inc_dirs {
        let mut files: Vec<PathBuf> = fs::read_dir(inc)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "h"))
            .collect();
        files.sort();
        for f in files {
            let Ok(text) = fs::read(&f) else { continue };
            // 見出しには Shift_JIS などの非 UTF-8 が混じり得るので、行は lossy に見て元の bytes を写す
            if let Some(new) = static_header(&text) {
                fs::write(dir.join(f.file_name().unwrap()), new)
                    .unwrap_or_else(|e| panic!("{} を書けません: {e}", dir.display()));
                changed += 1;
            }
        }
    }
    // 上流が変わって書き直しが効かなくなったら気づけるようにする
    assert!(
        changed >= 3,
        "書き直す見出しが見つかりません(SmTypes.h・nurbs.h・SmApiTypes.h など)"
    );
}

/// 1 つの見出しを書き直す。変える所が無ければ None
fn static_header(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len());
    let mut changed = false;
    let mut skipping = false;
    for line in text.split_inclusive(|&b| b == b'\n') {
        let s = String::from_utf8_lossy(line);
        let body = s.trim_end_matches(['\r', '\n']);
        if skipping {
            // `#define` の続きの行(行末が \\)を捨てる
            skipping = body.ends_with('\\');
            continue;
        }
        let t = body.trim_start();
        if let Some(rest) = t.strip_prefix('#') {
            let rest = rest.trim_start();
            if let Some(def) = rest.strip_prefix("define") {
                let def = def.trim_start();
                let name: String = def
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                let after = &def[name.len()..];
                if name.starts_with("SM_")
                    && name.ends_with("_TEMPLATE_PREDECLARATION")
                    && after.starts_with('(')
                {
                    let params = after.find(')').map_or(after, |i| &after[..=i]);
                    out.extend_from_slice(format!("#define {name}{params}\n").as_bytes());
                    changed = true;
                    skipping = body.ends_with('\\');
                    continue;
                }
            }
        }
        let mut l = line.to_vec();
        for pat in [
            "__declspec( dllexport )",
            "__declspec(dllexport)",
            "__declspec(dllimport)",
            "__declspec( dllimport )",
        ] {
            while let Some(i) = find(&l, pat.as_bytes()) {
                l.splice(i..i + pat.len(), std::iter::empty());
                changed = true;
            }
        }
        out.extend_from_slice(&l);
    }
    changed.then_some(out)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// 結果を取っておく場所(`<target>/smlib-cache`)。target は cargo が根に置く `.rustc_info.json`
/// か `CACHEDIR.TAG` で見分ける(`CACHEDIR.TAG` は新しく作った target にしかない)。
fn cache_dir(out: &Path) -> Option<PathBuf> {
    println!("cargo:rerun-if-env-changed=SMLIB_CACHE_DIR");
    match env::var_os("SMLIB_CACHE_DIR") {
        Some(d) if d == "off" => None,
        Some(d) if !d.is_empty() => Some(PathBuf::from(d)),
        _ => out
            .ancestors()
            .find(|d| d.join(".rustc_info.json").is_file() || d.join("CACHEDIR.TAG").is_file())
            .map(|d| d.join("smlib-cache")),
    }
}

/// コンテナをまたいで残る置き場所(`SMLIB_SHARED_CACHE`)。クラウドの作業場はコンテナごとに
/// target が空から始まり、そのたびに C++ を 7 分ほどコンパイルしていた(docs/verify.md の 6 節)。
/// 置き場所の 2 つ上(`/mnt/project-files/.cache/smlib-sys` なら `/mnt/project-files`)が無い所
/// (利用者の PC など)では使わない。置き場所はできるものに効かないので、`rerun-if-env-changed`
/// は出さない(変えても、このビルドスクリプトを走らせ直さない)。
fn shared_cache_dir() -> Option<PathBuf> {
    if env::var_os("SMLIB_CACHE_DIR").is_some_and(|d| d == "off") {
        return None;
    }
    let d = PathBuf::from(env::var_os("SMLIB_SHARED_CACHE").filter(|d| !d.is_empty())?);
    d.parent()?.parent()?.is_dir().then_some(d)
}

/// できたライブラリを取っておく。別の cargo が同時に書いても壊れないよう、仮の名前で
/// 書いてから名前を変える。失敗しても(場所が書けない等)ビルドは止めない。
/// 新しい方から 4 つまで、合わせて 512 MB までを残す(1 つは Linux で約 40 MB。
/// 開発とリリース、clippy とテストは同じ指紋になる)。大きさでも切るのは、以前の 2 GB の
/// 結果(write_static_headers の前)のようなものを残し続けないため。
fn store(dir: &Path, out: &Path, names: &[String]) {
    // コンテナが違うと番号(pid)が重なるので、時刻も名前に入れる
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let tmp = dir.with_extension(format!("tmp{}-{nanos}", std::process::id()));
    let ok = fs::create_dir_all(&tmp).is_ok()
        && names
            .iter()
            .all(|n| fs::copy(out.join(n), tmp.join(n)).is_ok())
        && fs::rename(&tmp, dir).is_ok();
    if !ok {
        let _ = fs::remove_dir_all(&tmp);
        return;
    }
    let Some(root) = dir.parent() else { return };
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.0));
    let size = |d: &Path| -> u64 {
        fs::read_dir(d)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok()?.metadata().ok())
            .map(|m| m.len())
            .sum()
    };
    let mut total = size(dir);
    let mut kept = 1;
    for (_, old) in entries.iter().filter(|(_, p)| p != dir) {
        total += size(old);
        if kept >= 4 || total > 512 << 20 {
            let _ = fs::remove_dir_all(old);
        } else {
            kept += 1;
        }
    }
}

/// 64 ビットの FNV-1a。Rust の版が変わっても同じ値になる(std の Hasher は保証がない)。
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    /// 長さを先に入れ、区切りの違う並びを同じ値にしない
    fn field(&mut self, bytes: &[u8]) {
        self.write(&(bytes.len() as u64).to_le_bytes());
        self.write(bytes);
    }

    /// ファイルの名前と中身(git の取り出しで時刻だけ変わっても同じ値にする)
    fn files<'a>(&mut self, files: impl Iterator<Item = &'a PathBuf>) {
        for f in files {
            self.field(f.as_os_str().as_encoded_bytes());
            self.field(&fs::read(f).unwrap_or_default());
        }
    }

    /// フォルダの中の .cpp 以外のファイル(見出し)
    fn headers(&mut self, dir: &Path) {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.extension().is_none_or(|x| x != "cpp"))
            .collect();
        files.sort();
        self.files(files.iter());
    }

    /// コンパイラの場所・引数・環境(MSVC の版や SDK の場所を含む)と、実行ファイルの大きさと時刻
    fn tool(&mut self, b: &cc::Build) {
        let c = b.get_compiler();
        self.field(c.path().as_os_str().as_encoded_bytes());
        for a in c.args() {
            self.field(a.as_encoded_bytes());
        }
        // PATH は呼び方で変わる(cargo run が足す等)ので入れない。コンパイラは場所と時刻で見る
        for (k, v) in c
            .get_envs()
            .filter(|(k, _)| !k.eq_ignore_ascii_case("PATH"))
        {
            self.field(k.as_encoded_bytes());
            self.field(v.as_encoded_bytes());
        }
        let exe = which(c.path());
        if let Ok(m) = exe.and_then(|p| fs::metadata(p).ok()).ok_or(()) {
            self.field(&m.len().to_le_bytes());
            let t = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
            self.field(&t.map_or(0, |d| d.as_secs()).to_le_bytes());
        }
        self.field(b.get_archiver().get_program().as_encoded_bytes());
    }
}

/// コンパイラの実行ファイル(名前だけなら PATH から探す)
fn which(p: &Path) -> Option<PathBuf> {
    if p.components().count() > 1 {
        return Some(p.to_path_buf());
    }
    let exts: &[&str] = if cfg!(windows) { &["", ".exe"] } else { &[""] };
    env::split_paths(&env::var_os("PATH")?).find_map(|d| {
        exts.iter()
            .map(|e| d.join(format!("{}{e}", p.display())))
            .find(|f| f.is_file())
    })
}
