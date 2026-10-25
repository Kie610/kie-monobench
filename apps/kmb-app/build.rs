//! Windows の exe にアプリのアイコン(kmb-ui の assets/icon.ico。上の帯の印と同じ絵)を埋め込む(N7b)。
//!
//! リソースのコンパイラ(rc.exe)や外の crate を使わず、.ico から .res を直に組み立てて、MSVC の
//! リンカーに渡す(link.exe は .res をそのまま受け取る)。.res の形は Microsoft の文書
//! 「Resource File Formats」のとおり(各リソースは 32 バイトの頭と 4 バイト境界にそろえた中身)。
//! MSVC 以外の Windows(GNU)では埋め込まず、警告だけ出す。
//!
//! あわせて、画面の台本(tests/ui/*.jcs)ごとに 1 つのテストを書き出す(`ui_scripts.rs`、
//! tests/screens.rs が読み込む)。台本ごとに別のテストにすると nextest が並べて流せる。

use std::path::PathBuf;

const ICO: &str = "../../crates/kmb-ui/assets/icon.ico";
/// リソースの種類(RT_ICON・RT_GROUP_ICON)
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
/// MOVEABLE | DISCARDABLE(llvm-rc がアイコンの絵に付けるのと同じ)
const FLAGS_ICON: u16 = 0x1010;
/// MOVEABLE | PURE | DISCARDABLE(同じくまとめに付けるもの)
const FLAGS_GROUP: u16 = 0x1030;
/// 言語に依らない(LANG_NEUTRAL)
const LANG: u16 = 0;

fn main() {
    println!("cargo:rerun-if-changed={ICO}");
    println!("cargo:rerun-if-changed=build.rs");
    ui_script_tests();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os != "windows" {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default() != "msvc" {
        println!("cargo:warning=MSVC 以外の Windows ではアイコンを exe に埋め込みません");
        return;
    }
    let ico = std::fs::read(ICO).unwrap_or_else(|e| panic!("{ICO} を読めません: {e}"));
    let res = icon_res(&ico).unwrap_or_else(|e| panic!("{ICO} が読めない形です: {e}"));
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR が無い")).join("icon.res");
    std::fs::write(&out, res).unwrap_or_else(|e| panic!("{} を書けません: {e}", out.display()));
    println!("cargo:rustc-link-arg-bin=kie-monobench={}", out.display());
}

/// tests/ui の台本ごとに `#[test] fn <台本の名前>() { ui_script("<名前>") }` を書き出す。
/// テストの名前は台本のファイル名の字のうち、名前に使えない字(空白・括弧・・など)を `_` に、
/// 数字で始まる物は頭に `_` を付けた物。
fn ui_script_tests() {
    const DIR: &str = "tests/ui";
    println!("cargo:rerun-if-changed={DIR}");
    let mut stems: Vec<String> = std::fs::read_dir(DIR)
        .map(|d| {
            d.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "jcs"))
                .filter_map(|p| Some(p.file_stem()?.to_str()?.to_string()))
                .collect()
        })
        .unwrap_or_default();
    stems.sort();
    let mut seen = std::collections::BTreeMap::new();
    let mut code = String::new();
    for stem in &stems {
        let mut id: String = stem
            .chars()
            .map(|c| {
                if c == '_' || c.is_ascii_alphanumeric() || (!c.is_ascii() && c.is_alphabetic()) {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if id.starts_with(|c: char| c.is_ascii_digit()) {
            id.insert(0, '_');
        }
        if let Some(other) = seen.insert(id.clone(), stem) {
            panic!(
                "台本「{stem}」と「{other}」のテストの名前が同じ {id} になる。どちらかの名前を変える"
            );
        }
        code.push_str(&format!(
            "#[test]\nfn {id}() {{\n    super::ui_script({stem:?});\n}}\n"
        ));
    }
    let out =
        PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR が無い")).join("ui_scripts.rs");
    std::fs::write(&out, code).unwrap_or_else(|e| panic!("{} を書けません: {e}", out.display()));
}

/// .ico の中身から .res を作る。絵ごとに RT_ICON(名前 1, 2, …)、まとめて RT_GROUP_ICON(名前 1)。
fn icon_res(ico: &[u8]) -> Result<Vec<u8>, String> {
    let u16_at = |i: usize| -> Result<u16, String> {
        ico.get(i..i + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .ok_or_else(|| "短すぎる".to_string())
    };
    let u32_at = |i: usize| -> Result<u32, String> {
        ico.get(i..i + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "短すぎる".to_string())
    };
    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return Err("アイコンの頭ではない".into());
    }
    let count = u16_at(4)?;
    // 最初は空のリソース(.res の印)
    let mut res = Vec::new();
    push_resource(&mut res, 0, 0, 0, &[]);
    let mut group = Vec::new();
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&count.to_le_bytes());
    for n in 0..count {
        let e = 6 + 16 * n as usize;
        let size = u32_at(e + 8)? as usize;
        let offset = u32_at(e + 12)? as usize;
        let image = ico
            .get(offset..offset + size)
            .ok_or_else(|| format!("{} 枚目の絵がはみ出している", n + 1))?;
        let id = n + 1;
        push_resource(&mut res, RT_ICON, id, FLAGS_ICON, image);
        // 幅・高さ・色数・予約・色の深さ・大きさは .ico の項目と同じで、最後の位置を名前に替える。
        // 面の数は PNG の絵だと 0 のことがあるので、rc と同じく 1 にする
        group.extend_from_slice(&ico[e..e + 4]);
        group.extend_from_slice(&u16_at(e + 4)?.max(1).to_le_bytes());
        group.extend_from_slice(&ico[e + 6..e + 12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    push_resource(&mut res, RT_GROUP_ICON, 1, FLAGS_GROUP, &group);
    Ok(res)
}

/// 種類と名前を番号で持つリソースを 1 つ足す。
fn push_resource(res: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    let len = u32::try_from(data.len()).expect("リソースが大きすぎる");
    res.extend_from_slice(&len.to_le_bytes()); // DataSize
    res.extend_from_slice(&32u32.to_le_bytes()); // HeaderSize
    for v in [0xFFFF, kind, 0xFFFF, name] {
        res.extend_from_slice(&u16::to_le_bytes(v)); // TYPE・NAME(番号)
    }
    res.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    res.extend_from_slice(&flags.to_le_bytes()); // MemoryFlags
    res.extend_from_slice(&LANG.to_le_bytes()); // LanguageId
    res.extend_from_slice(&0u32.to_le_bytes()); // Version
    res.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    res.extend_from_slice(data);
    while !res.len().is_multiple_of(4) {
        res.push(0);
    }
}
