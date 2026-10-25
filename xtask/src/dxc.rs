//! `cargo xtask dxc`: DirectX 12 のシェーダーを作る DXC(Microsoft の dxcompiler.dll)を取ってきて、
//! アプリの実行ファイルの横(`target/debug` と `target/release`)に置く。
//!
//! アプリは設定の描画の方式が「DirectX 12(DXC)」(既定)の時、実行ファイルの横の dxcompiler.dll を
//! 読む。無ければ FXC(Windows に入っている古い作り方)で描く。DXC は Microsoft の公式の配布
//! (GitHub の microsoft/DirectXShaderCompiler のリリース)を使い、SHA-256 を確かめてから置く。
//! 許諾は LLVM のもの(University of Illinois/NCSA)で、配る時は許諾の文を一緒に置く。
//!
//! ダウンロードと展開は OS の `curl` と `tar` を使う(Windows 10 以降は両方入っている)。
//! 取ってきたものは `target/dxc/<版>/` に残し、2 回目からは取りに行かない。

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 使う DXC の版(2026-09-29 のリリース。シェーダーモデル 6.9 まで)。
pub const VERSION: &str = "v1.9.2609";
const ZIP: &str = "dxc_2026_09_29.zip";
/// GitHub のリリースの頁に出ている SHA-256(2026-10-19 に確かめた)。
const SHA256: &str = "ad31b1fc8443175d204f77a611fdb3ef2ec42759bdc2f1167368de24a4a7e7f1";
/// zip の中の、64 ビットの Windows 向けの本体。
const DLL_IN_ZIP: &str = "bin/x64/dxcompiler.dll";
pub const DLL: &str = "dxcompiler.dll";
/// 本体と一緒に置く許諾の文(zip の中の名前、置く名前)。
const LICENSES: [(&str, &str); 1] = [("LICENSE-LLVM.txt", "dxcompiler-LICENSE-LLVM.txt")];

fn url() -> String {
    format!("https://github.com/microsoft/DirectXShaderCompiler/releases/download/{VERSION}/{ZIP}")
}

/// ファイルの SHA-256(16 進の小文字)。
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `target/dxc/<版>/` に zip を取ってきて確かめ、展開する。展開したフォルダを返す。
fn fetch(target: &Path) -> Result<PathBuf, String> {
    let dir = target.join("dxc").join(VERSION);
    if dir.join(DLL_IN_ZIP).is_file() {
        println!("取ってある DXC {VERSION} を使います({})", dir.display());
        return Ok(dir);
    }
    fs::create_dir_all(&dir).map_err(|e| format!("{} を作れません: {e}", dir.display()))?;
    let zip = dir.join(ZIP);
    println!("DXC {VERSION} を取ってきます: {}", url());
    let st = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&zip)
        .arg(url())
        .status()
        .map_err(|e| format!("curl を動かせません: {e}"))?;
    if !st.success() {
        return Err(format!("DXC を取ってこられません(curl {st})"));
    }
    let bytes = fs::read(&zip).map_err(|e| format!("{} を読めません: {e}", zip.display()))?;
    let got = sha256_hex(&bytes);
    if got != SHA256 {
        let _ = fs::remove_file(&zip);
        return Err(format!(
            "取ってきた DXC の SHA-256 が合いません(期待 {SHA256}、実際 {got})。置きません"
        ));
    }
    let st = Command::new("tar")
        .arg("-xf")
        .arg(&zip)
        .arg("-C")
        .arg(&dir)
        .status()
        .map_err(|e| format!("tar を動かせません: {e}"))?;
    if !st.success() || !dir.join(DLL_IN_ZIP).is_file() {
        return Err(format!("DXC の zip を展開できません(tar {st})"));
    }
    let _ = fs::remove_file(&zip);
    Ok(dir)
}

/// 展開したフォルダ `from` から、`profiles`(`target/debug` など)に本体と許諾の文を置く。
/// 置いたファイルを返す。プロファイルのフォルダが無ければ作る。
pub fn place(from: &Path, profiles: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut placed = Vec::new();
    for p in profiles {
        fs::create_dir_all(p).map_err(|e| format!("{} を作れません: {e}", p.display()))?;
        let to = p.join(DLL);
        fs::copy(from.join(DLL_IN_ZIP), &to)
            .map_err(|e| format!("{} に置けません: {e}", to.display()))?;
        placed.push(to);
        for (name, as_name) in LICENSES {
            if from.join(name).is_file() {
                let to = p.join(as_name);
                fs::copy(from.join(name), &to)
                    .map_err(|e| format!("{} に置けません: {e}", to.display()))?;
                placed.push(to);
            }
        }
    }
    Ok(placed)
}

/// `cargo xtask dxc` の本体。
pub fn run(target: &Path) -> Result<(), String> {
    let dir = fetch(target)?;
    let placed = place(&dir, &[target.join("debug"), target.join("release")])?;
    for p in &placed {
        println!("置きました: {}", p.display());
    }
    println!(
        "DXC {VERSION} を置きました。アプリの設定の描画の方式が「DirectX 12(DXC)」なら、次の起動から使います"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_the_dll_and_its_license_next_to_each_profile() {
        let root = std::env::temp_dir().join(format!("kmb-dxc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let from = root.join("x");
        fs::create_dir_all(from.join("bin/x64")).unwrap();
        fs::write(from.join(DLL_IN_ZIP), b"dll").unwrap();
        fs::write(from.join("LICENSE-LLVM.txt"), b"license").unwrap();
        let profiles = [root.join("debug"), root.join("release")];
        let placed = place(&from, &profiles).unwrap();
        assert_eq!(placed.len(), 4);
        for p in &profiles {
            assert_eq!(fs::read(p.join(DLL)).unwrap(), b"dll");
            assert_eq!(
                fs::read(p.join("dxcompiler-LICENSE-LLVM.txt")).unwrap(),
                b"license"
            );
        }
        fs::remove_dir_all(&root).unwrap();
    }
}
