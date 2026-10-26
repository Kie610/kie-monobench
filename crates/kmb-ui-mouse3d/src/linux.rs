//! Linux で 3D マウスを探して読む(`/dev/hidraw*`。std だけで読む)。
//!
//! `/sys/class/hidraw/hidrawN/device/` の `uevent`(会社と製品の番号・名前)と `report_descriptor`
//! (用途)だけを見て、多軸の機器だけを `/dev/hidrawN` で読み取りのために開く。hidraw は evdev とは
//! 別なので、ほかの読み手が動いていても読める。既定では root しか読めないことが多く、開けなければ
//! 権限が無いと返す。

use crate::{Device, Reader, wanted};
use std::io::Read;
use std::path::Path;

const CLASS: &str = "/sys/class/hidraw";
const DEV: &str = "/dev";

/// 繋がっている 3D マウス。
pub(crate) fn scan() -> Vec<Device> {
    scan_in(Path::new(CLASS), Path::new(DEV))
}

/// `class` の下の hidraw を並べ、多軸の機器だけを返す(`dev` は機器のファイルの置き場所)。
pub(crate) fn scan_in(class: &Path, dev: &Path) -> Vec<Device> {
    let Ok(entries) = std::fs::read_dir(class) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("hidraw"))
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|n| {
            let d = class.join(&n).join("device");
            let (vendor, product, name) = uevent(&std::fs::read_to_string(d.join("uevent")).ok()?)?;
            let desc = std::fs::read(d.join("report_descriptor")).ok()?;
            let (page, usage) = top_usage(&desc)?;
            wanted(vendor, page, usage).then(|| Device {
                path: dev.join(&n),
                name,
                vendor,
                product,
                shape: format!("報告の記述 {} バイト", desc.len()),
            })
        })
        .collect()
}

/// `uevent` の `HID_ID=バス:会社:製品`(16 進)と `HID_NAME` を読む。
fn uevent(text: &str) -> Option<(u16, u16, String)> {
    let mut ids = None;
    let mut name = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("HID_ID=") {
            let mut parts = v.split(':').skip(1);
            let vendor = u32::from_str_radix(parts.next()?, 16).ok()?;
            let product = u32::from_str_radix(parts.next()?, 16).ok()?;
            ids = Some((u16::try_from(vendor).ok()?, u16::try_from(product).ok()?));
        } else if let Some(v) = line.strip_prefix("HID_NAME=") {
            name = Some(v.trim().to_string());
        }
    }
    let (vendor, product) = ids?;
    Some((vendor, product, name.unwrap_or_else(|| "3D マウス".into())))
}

/// 報告の記述の最初の集まり(Collection)の用途の頁と用途。
pub(crate) fn top_usage(desc: &[u8]) -> Option<(u16, u16)> {
    let mut page = 0u16;
    let mut usage = None;
    let mut i = 0;
    while i < desc.len() {
        let b = desc[i];
        if b == 0xFE {
            // 長い項目: 中身の長さ・印のあとに中身
            i += 3 + usize::from(*desc.get(i + 1)?);
            continue;
        }
        let size = [0, 1, 2, 4][usize::from(b & 3)];
        let data = desc.get(i + 1..i + 1 + size)?;
        let mut v = 0u32;
        for (k, x) in data.iter().enumerate() {
            v |= u32::from(*x) << (8 * k);
        }
        match b & 0xFC {
            0x04 => page = v as u16,
            0x08 if size == 4 => usage = Some(((v >> 16) as u16, v as u16)),
            0x08 => usage = Some((page, v as u16)),
            0xA0 => return usage,
            _ => {}
        }
        i += 1 + size;
    }
    None
}

/// 機器を開いて、外れるか止まる合図が来るまで読む。
pub(crate) fn read(dev: &Device, reader: &mut Reader<'_>) -> std::io::Result<()> {
    let mut file = std::fs::File::open(&dev.path)?;
    reader.connected();
    let mut buf = [0u8; 64];
    while !reader.stopped() {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        reader.feed(&buf[..n]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 多軸の機器の記述の頭(Generic Desktop / Multi-axis Controller、Application の集まり)
    const MULTI_AXIS: [u8; 6] = [0x05, 0x01, 0x09, 0x08, 0xA1, 0x01];
    /// キーボードの記述の頭(Generic Desktop / Keyboard)
    const KEYBOARD: [u8; 6] = [0x05, 0x01, 0x09, 0x06, 0xA1, 0x01];

    fn node(class: &Path, n: &str, id: &str, name: &str, desc: &[u8]) {
        let d = class.join(n).join("device");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("uevent"),
            format!("DRIVER=hid-generic\nHID_ID={id}\nHID_NAME={name}\n"),
        )
        .unwrap();
        std::fs::write(d.join("report_descriptor"), desc).unwrap();
    }

    /// 確かめること: 作り物の sysfs の木で、多軸の機器で会社の番号の合う物だけを拾い、同じ会社の
    /// キーボードと、別の会社の多軸の機器は拾わない。用途は 4 バイトの用途の書き方でも読める。
    #[test]
    fn scan_fake_tree() {
        let root = std::env::temp_dir().join(format!("kmb-mouse3d-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let class = root.join("class");
        node(&class, "hidraw0", "0003:0000046D:0000C31C", "Keyboard", &KEYBOARD);
        node(&class, "hidraw1", "0003:0000256F:0000C635", "Mouse A", &MULTI_AXIS);
        node(&class, "hidraw2", "0003:00001234:00000001", "Other", &MULTI_AXIS);
        // 用途を頁つきの 4 バイトで書いた記述
        node(
            &class,
            "hidraw3",
            "0003:0000046D:0000C626",
            "Mouse B",
            &[0x0B, 0x08, 0x00, 0x01, 0x00, 0xA1, 0x01],
        );
        let found = scan_in(&class, Path::new("/dev"));
        let got: Vec<_> = found
            .iter()
            .map(|d| (d.path.to_string_lossy().into_owned(), d.name.as_str(), d.vendor, d.product))
            .collect();
        assert_eq!(
            got,
            vec![
                ("/dev/hidraw1".to_string(), "Mouse A", 0x256F, 0xC635),
                ("/dev/hidraw3".to_string(), "Mouse B", 0x046D, 0xC626),
            ]
        );
        assert!(scan_in(&root.join("無い"), Path::new("/dev")).is_empty());
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(top_usage(&MULTI_AXIS), Some((1, 8)));
        assert_eq!(top_usage(&[0x05, 0x01]), None);
        assert_eq!(top_usage(&[0x05]), None);
    }
}
