//! C の中継層(shim/jc_smlib.cpp)をビルドする。SMLib 本体は smlib-sys がビルドしてリンクする。
fn main() {
    println!("cargo:rerun-if-changed=shim/jc_smlib.cpp");
    println!("cargo:rerun-if-changed=shim/jc_step.cpp");
    println!("cargo:rerun-if-changed=shim/jc_smlib.h");
    let includes =
        std::env::var("DEP_SMLIB_INCLUDE").expect("smlib-sys の見出しの場所が渡されていません");
    let msvc = std::env::var("TARGET").unwrap().contains("msvc");
    let mut b = cc::Build::new();
    b.cpp(true)
        .std("c++17")
        .file("shim/jc_smlib.cpp")
        .file("shim/jc_step.cpp")
        .include("shim")
        .define("NOMINMAX", None)
        .define("SM_GRAPHICS_CALLBACKS", "1")
        .define("SM_API_EXPORTS", None)
        .define("NDEBUG", None)
        .opt_level(2);
    // 見出しの中の `__FILE__`(SMLib の FILE_NAME)は探した場所のパスのまま実行ファイルに入るので、
    // このフォルダ(ビルドスクリプトが動く場所)からの相対のパスで渡す(smlib-sys の build.rs と同じ)
    let here = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    for dir in includes.split(';').filter(|s| !s.is_empty()) {
        b.include(relative(std::path::Path::new(dir), &here));
    }
    if msvc {
        b.flag("/utf-8").flag("/EHsc");
    } else {
        // SMLib の見出しの警告が多いので消す(中継層だけの警告は g++ -Wall で別に確かめる)
        b.flag("-w");
    }
    b.compile("jc_smlib");
}

/// `path` を `base` からの相対のパスにする。同じ頭(ドライブなど)を持たなければそのまま返す。
fn relative(path: &std::path::Path, base: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let p: Vec<Component> = path.components().collect();
    let b: Vec<Component> = base.components().collect();
    let same = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if same == 0 || !matches!(p[0], Component::RootDir | Component::Prefix(_)) {
        return path.to_path_buf();
    }
    let mut out = std::path::PathBuf::new();
    for _ in same..b.len() {
        out.push("..");
    }
    for c in &p[same..] {
        out.push(c.as_os_str());
    }
    out
}
