//! 小さな日本語の試験用フォントを、既定の書体の名前で一時フォルダに置く。
use std::path::{Path, PathBuf};

pub struct GothicFonts(PathBuf);

impl GothicFonts {
    pub fn new(source: &Path) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kmb-gothic-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut bytes = std::fs::read(source).unwrap();
        // 名前の表の大きさと位置を保ったまま、書体の選択を試せる名前にする。
        for (from, to) in [
            (b"JC Test JP".to_vec(), b"IPA Gothic".to_vec()),
            (
                "JC Test JP"
                    .encode_utf16()
                    .flat_map(u16::to_be_bytes)
                    .collect(),
                "IPA Gothic"
                    .encode_utf16()
                    .flat_map(u16::to_be_bytes)
                    .collect(),
            ),
        ] {
            assert_eq!(from.len(), to.len());
            let positions: Vec<_> = bytes
                .windows(from.len())
                .enumerate()
                .filter_map(|(i, b)| (b == from).then_some(i))
                .collect();
            for i in positions {
                bytes[i..i + from.len()].copy_from_slice(&to);
            }
        }
        // 既定の文字列 TEXT も小さい字体で試す。単独の cmap 区間の送り先だけを変える。
        let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
        let rec = (0..count)
            .map(|i| 12 + 16 * i)
            .find(|&r| &bytes[r..r + 4] == b"cmap")
            .unwrap();
        let cmap = u32::from_be_bytes(bytes[rec + 8..rec + 12].try_into().unwrap()) as usize;
        let table =
            cmap + u32::from_be_bytes(bytes[cmap + 8..cmap + 12].try_into().unwrap()) as usize;
        assert_eq!(
            u16::from_be_bytes(bytes[table..table + 2].try_into().unwrap()),
            4
        );
        let n = u16::from_be_bytes(bytes[table + 6..table + 8].try_into().unwrap()) as usize / 2;
        for (old, new) in [(b'8', b'E'), (b'A', b'T'), (b'O', b'X')] {
            let end = (0..n)
                .map(|i| table + 14 + i * 2)
                .find(|&r| u16::from_be_bytes(bytes[r..r + 2].try_into().unwrap()) == old as u16)
                .unwrap();
            let start = end + 2 * n + 2;
            let delta = end + 4 * n + 2;
            assert_eq!(
                u16::from_be_bytes(bytes[start..start + 2].try_into().unwrap()),
                old as u16
            );
            let d = u16::from_be_bytes(bytes[delta..delta + 2].try_into().unwrap());
            bytes[end..end + 2].copy_from_slice(&(new as u16).to_be_bytes());
            bytes[start..start + 2].copy_from_slice(&(new as u16).to_be_bytes());
            bytes[delta..delta + 2].copy_from_slice(
                &d.wrapping_add(old as u16)
                    .wrapping_sub(new as u16)
                    .to_be_bytes(),
            );
        }
        std::fs::write(dir.join("gothic.ttf"), bytes).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for GothicFonts {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
