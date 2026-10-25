//! AV1 の流れを MP4 の入れ物に詰める(自前の書き手。依存なし。「ISO BMFF への AV1 の入れ方」v1.2.0 に従う)。
//!
//! 受け取るのは 1 枚分(temporal unit)の OBU の並び(どれも大きさの欄がある「低い上乗せ」の形)。
//! MP4 の絵には区切りの OBU(temporal delimiter)と詰め物の OBU を入れない。最初の sequence header は
//! `av1C` にも写す(キーフレームの絵の中にも残す)。
//!
//! 箱の並び: `ftyp`、`moov`(先頭に置く。読み込みながら再生できる)、`mdat`。
//! `moov` の大きさは中身の位置に依らないので、先に 1 度組んで大きさを測り、`mdat` の位置を決めてから組み直す。
//!
//! 表示の順と符号の順は同じ(AV1 は見せない絵を同じ temporal unit に入れる)ので `ctts` は書かない。

use std::io::{self, Write};

/// MP4 の書き手。[`Mp4Writer::push`] で 1 枚ずつ入れ、[`Mp4Writer::finish`] で書く。
#[derive(Debug, Clone)]
pub struct Mp4Writer {
    width: u32,
    height: u32,
    /// 絵の時間の目盛り(1 秒あたり)。1 枚は `delta` 目盛り。
    timescale: u32,
    delta: u32,
    /// `av1C` の頭の 4 バイト(版・profile・level・色の形)。
    config: [u8; 4],
    /// 最初の sequence header の OBU。
    sequence_header: Option<Vec<u8>>,
    /// 絵の中身(OBU)をつなげたもの。
    data: Vec<u8>,
    sizes: Vec<u32>,
    durations: Vec<u32>,
    /// 区切りのよい絵(キーフレーム)の番号(1 から)。
    sync: Vec<u32>,
}

/// OBU の種類(sequence header・区切り・詰め物)。
const OBU_SEQUENCE_HEADER: u8 = 1;
const OBU_TEMPORAL_DELIMITER: u8 = 2;
const OBU_PADDING: u8 = 15;

/// 1 枚分を OBU に分ける。返すのは(種類, 頭から終わりまでの OBU)。大きさの欄が無い OBU や、
/// 途中で切れている時は `None`。
pub fn obus(tu: &[u8]) -> Option<Vec<(u8, &[u8])>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tu.len() {
        let h = tu[i];
        let kind = (h >> 3) & 0x0f;
        let ext = h & 0x04 != 0;
        if h & 0x02 == 0 {
            return None;
        }
        let mut j = i + 1 + ext as usize;
        // 大きさは leb128
        let mut size = 0u64;
        for k in 0..8 {
            let b = *tu.get(j)?;
            j += 1;
            size |= ((b & 0x7f) as u64) << (7 * k);
            if b & 0x80 == 0 {
                break;
            }
        }
        let end = j.checked_add(usize::try_from(size).ok()?)?;
        if end > tu.len() {
            return None;
        }
        out.push((kind, &tu[i..end]));
        i = end;
    }
    Some(out)
}

impl Mp4Writer {
    /// 幅・高さ(画素)と、1 秒あたりの枚数と、`av1C` の頭の 4 バイト(符号器が返す物。後ろは使わない)。
    pub fn new(width: u32, height: u32, fps: f32, av1c: &[u8]) -> Self {
        // 目盛りは 1 枚 = 1000。29.97 のような半端な値も 29970/1000 で表せる
        let fps = if fps.is_finite() && fps > 0.0 {
            fps
        } else {
            30.0
        };
        let timescale = ((fps as f64 * 1000.0).round() as u32).max(1);
        Self {
            width,
            height,
            timescale,
            delta: 1000,
            config: av1c
                .get(..4)
                .and_then(|c| c.try_into().ok())
                .unwrap_or([0x81, 0, 0, 0]),
            sequence_header: None,
            data: Vec::new(),
            sizes: Vec::new(),
            durations: Vec::new(),
            sync: Vec::new(),
        }
    }

    /// 入れた絵の数。
    pub fn frame_count(&self) -> usize {
        self.sizes.len()
    }

    /// 1 枚分(temporal unit)を足す。`keyframe` は区切りのよい絵か。
    /// 絵の OBU が無い時(区切りだけの時)は、前の絵を 1 枚分長く見せる。
    pub fn push(&mut self, temporal_unit: &[u8], keyframe: bool) -> io::Result<()> {
        let parts = obus(temporal_unit)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "AV1 の OBU が読めません"))?;
        let start = self.data.len();
        for (kind, obu) in parts {
            match kind {
                OBU_TEMPORAL_DELIMITER | OBU_PADDING => {}
                k => {
                    if k == OBU_SEQUENCE_HEADER && self.sequence_header.is_none() {
                        self.sequence_header = Some(obu.to_vec());
                    }
                    self.data.extend_from_slice(obu);
                }
            }
        }
        let size = self.data.len() - start;
        if size == 0 {
            if let Some(d) = self.durations.last_mut() {
                *d += self.delta;
            }
            return Ok(());
        }
        self.sizes.push(size as u32);
        self.durations.push(self.delta);
        if keyframe {
            self.sync.push(self.sizes.len() as u32);
        }
        Ok(())
    }

    /// MP4 にして書く。絵が無い時、sequence header が無い時は誤り。
    pub fn finish(&self, mut w: impl Write) -> io::Result<()> {
        let invalid = |m: &str| io::Error::new(io::ErrorKind::InvalidInput, m.to_string());
        if self.sizes.is_empty() {
            return Err(invalid("MP4 に入れる絵がありません"));
        }
        let Some(seq) = &self.sequence_header else {
            return Err(invalid("AV1 の流れに sequence header がありません"));
        };

        let ftyp = ftyp();
        let large = self.data.len() as u64 + 8 > u32::MAX as u64;
        let mdat_header = if large { 16 } else { 8 };
        // 1 度組んで大きさを測り、mdat の中身の位置を決めて組み直す
        let probe = self.moov(seq, u64::MAX)?;
        let offset = (ftyp.len() + probe.len() + mdat_header) as u64;
        let moov = self.moov(seq, offset)?;
        debug_assert_eq!(moov.len(), probe.len());

        w.write_all(&ftyp)?;
        w.write_all(&moov)?;
        if large {
            w.write_all(&1u32.to_be_bytes())?;
            w.write_all(b"mdat")?;
            w.write_all(&(self.data.len() as u64 + 16).to_be_bytes())?;
        } else {
            w.write_all(&(self.data.len() as u32 + 8).to_be_bytes())?;
            w.write_all(b"mdat")?;
        }
        w.write_all(&self.data)
    }

    fn moov(&self, seq: &[u8], chunk_offset: u64) -> io::Result<Vec<u8>> {
        let media_duration: u64 = self.durations.iter().map(|&d| d as u64).sum();
        // 動画全体の目盛りは 1/1000 秒
        let movie_duration = media_duration * 1000 / self.timescale as u64;
        let too_long = || io::Error::new(io::ErrorKind::InvalidInput, "動画が長すぎます");
        let media_duration = u32::try_from(media_duration).map_err(|_| too_long())?;
        let movie_duration = u32::try_from(movie_duration).map_err(|_| too_long())?;

        let mut mvhd = full(0, 0);
        put32(&mut mvhd, 0); // 作った時刻
        put32(&mut mvhd, 0); // 変えた時刻
        put32(&mut mvhd, 1000);
        put32(&mut mvhd, movie_duration);
        put32(&mut mvhd, 0x0001_0000); // 速さ 1.0
        put16(&mut mvhd, 0x0100); // 音量 1.0
        mvhd.extend_from_slice(&[0; 10]);
        matrix(&mut mvhd);
        mvhd.extend_from_slice(&[0; 24]);
        put32(&mut mvhd, 2); // 次の track の番号

        let mut tkhd = full(0, 3); // 有効・動画に入る
        put32(&mut tkhd, 0);
        put32(&mut tkhd, 0);
        put32(&mut tkhd, 1); // track の番号
        put32(&mut tkhd, 0);
        put32(&mut tkhd, movie_duration);
        tkhd.extend_from_slice(&[0; 8]);
        put16(&mut tkhd, 0); // 重なりの順
        put16(&mut tkhd, 0); // 代わりの組
        put16(&mut tkhd, 0); // 音量(動画は 0)
        put16(&mut tkhd, 0);
        matrix(&mut tkhd);
        put32(&mut tkhd, self.width << 16);
        put32(&mut tkhd, self.height << 16);

        let mut mdhd = full(0, 0);
        put32(&mut mdhd, 0);
        put32(&mut mdhd, 0);
        put32(&mut mdhd, self.timescale);
        put32(&mut mdhd, media_duration);
        put16(&mut mdhd, 0x55C4); // 言葉は "und"(決めない)
        put16(&mut mdhd, 0);

        let mut hdlr = full(0, 0);
        put32(&mut hdlr, 0);
        hdlr.extend_from_slice(b"vide");
        hdlr.extend_from_slice(&[0; 12]);
        hdlr.extend_from_slice(b"VideoHandler\0");

        let mut vmhd = full(0, 1);
        vmhd.extend_from_slice(&[0; 8]);

        let mut dref = full(0, 0);
        put32(&mut dref, 1);
        dref.extend_from_slice(&bx(b"url ", &full(0, 1))); // 中身は同じファイル
        let dinf = bx(b"dinf", &bx(b"dref", &dref));

        let stbl = [
            bx(b"stsd", &self.stsd(seq)),
            bx(b"stts", &self.stts()),
            bx(b"stss", &self.stss()),
            bx(b"stsc", &self.stsc()),
            bx(b"stsz", &self.stsz()),
            self.chunk_offset_box(chunk_offset),
        ]
        .concat();
        let minf = [bx(b"vmhd", &vmhd), dinf, bx(b"stbl", &stbl)].concat();
        let mdia = [bx(b"mdhd", &mdhd), bx(b"hdlr", &hdlr), bx(b"minf", &minf)].concat();
        let trak = [bx(b"tkhd", &tkhd), bx(b"mdia", &mdia)].concat();
        Ok(bx(
            b"moov",
            &[bx(b"mvhd", &mvhd), bx(b"trak", &trak)].concat(),
        ))
    }

    fn stsd(&self, seq: &[u8]) -> Vec<u8> {
        let mut av01 = vec![0; 6];
        put16(&mut av01, 1); // 中身の参照の番号
        av01.extend_from_slice(&[0; 16]);
        put16(&mut av01, self.width.min(u16::MAX as u32) as u16);
        put16(&mut av01, self.height.min(u16::MAX as u32) as u16);
        put32(&mut av01, 0x0048_0000); // 72 dpi
        put32(&mut av01, 0x0048_0000);
        put32(&mut av01, 0);
        put16(&mut av01, 1); // 1 つの絵に 1 枚
        av01.extend_from_slice(&[0; 32]); // 符号器の名前(空)
        put16(&mut av01, 0x0018); // 色の深さ
        put16(&mut av01, 0xFFFF);
        av01.extend_from_slice(&bx(b"av1C", &[&self.config[..], seq].concat()));
        let mut stsd = full(0, 0);
        put32(&mut stsd, 1);
        stsd.extend_from_slice(&bx(b"av01", &av01));
        stsd
    }

    /// 絵ごとの長さ(同じ長さが続く所はまとめる)。
    fn stts(&self) -> Vec<u8> {
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for &d in &self.durations {
            match runs.last_mut() {
                Some((n, dd)) if *dd == d => *n += 1,
                _ => runs.push((1, d)),
            }
        }
        let mut b = full(0, 0);
        put32(&mut b, runs.len() as u32);
        for (n, d) in runs {
            put32(&mut b, n);
            put32(&mut b, d);
        }
        b
    }

    fn stss(&self) -> Vec<u8> {
        let mut b = full(0, 0);
        put32(&mut b, self.sync.len() as u32);
        for &s in &self.sync {
            put32(&mut b, s);
        }
        b
    }

    /// 全部の絵を 1 つの塊(chunk)に入れる。
    fn stsc(&self) -> Vec<u8> {
        let mut b = full(0, 0);
        put32(&mut b, 1);
        put32(&mut b, 1);
        put32(&mut b, self.sizes.len() as u32);
        put32(&mut b, 1);
        b
    }

    fn stsz(&self) -> Vec<u8> {
        let mut b = full(0, 0);
        put32(&mut b, 0);
        put32(&mut b, self.sizes.len() as u32);
        for &s in &self.sizes {
            put32(&mut b, s);
        }
        b
    }

    /// 塊の位置。4 GB を超える所なら co64。大きさを測る時(`u64::MAX`)は短い方の stco で組む。
    fn chunk_offset_box(&self, offset: u64) -> Vec<u8> {
        let mut b = full(0, 0);
        put32(&mut b, 1);
        match u32::try_from(offset) {
            Ok(o) => {
                put32(&mut b, o);
                bx(b"stco", &b)
            }
            Err(_) if offset == u64::MAX => {
                put32(&mut b, 0);
                bx(b"stco", &b)
            }
            Err(_) => {
                b.extend_from_slice(&offset.to_be_bytes());
                bx(b"co64", &b)
            }
        }
    }
}

fn ftyp() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"isom");
    put32(&mut b, 0x200);
    for brand in [b"isom", b"iso6", b"av01", b"mp41"] {
        b.extend_from_slice(brand);
    }
    bx(b"ftyp", &b)
}

fn put16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_be_bytes());
}

fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_be_bytes());
}

/// 箱(大きさ・種類・中身)。
fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(body.len() + 8);
    put32(&mut b, body.len() as u32 + 8);
    b.extend_from_slice(kind);
    b.extend_from_slice(body);
    b
}

/// 版と旗のある箱の頭。
fn full(version: u8, flags: u32) -> Vec<u8> {
    let mut b = vec![version];
    b.extend_from_slice(&flags.to_be_bytes()[1..]);
    b
}

/// 回さない変換の行列。
fn matrix(b: &mut Vec<u8>) {
    for v in [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000u32] {
        put32(b, v);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// 試験の作業場所(OS の一時フォルダの下。試験ごと・プロセスごとに分ける)。
    pub(crate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("kmb-io-render")
            .join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(crate) fn have(tool: &str) -> bool {
        Command::new(tool)
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// ffprobe で、絵の数・1 秒あたりの枚数・形式を読む。ffmpeg でも最後まで読んで誤りが無いか見る。
    pub(crate) fn probe(path: &Path) -> (u32, String, String, (u32, u32)) {
        let o = Command::new("ffprobe")
            .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
            .args([
                "-show_entries",
                "stream=codec_name,width,height,avg_frame_rate,nb_read_frames",
            ])
            .args(["-of", "default=noprint_wrappers=1"])
            .arg(path)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(
            o.status.success() && err.trim().is_empty(),
            "ffprobe: {err}"
        );
        let text = String::from_utf8_lossy(&o.stdout).to_string();
        let get = |k: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("{k}=")))
                .unwrap_or_default()
                .to_string()
        };
        let d = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "null", "-"])
            .output()
            .unwrap();
        let derr = String::from_utf8_lossy(&d.stderr);
        assert!(
            d.status.success() && derr.trim().is_empty(),
            "ffmpeg: {derr}"
        );
        (
            get("nb_read_frames").parse().unwrap(),
            get("avg_frame_rate"),
            get("codec_name"),
            (
                get("width").parse().unwrap(),
                get("height").parse().unwrap(),
            ),
        )
    }

    /// 区切り・sequence header・絵の OBU(どれも大きさの欄あり)。
    const TD: [u8; 2] = [0x12, 0x00];
    const SEQ: [u8; 5] = [0x0A, 0x03, 0, 0, 0x0C];
    fn frame(b: u8) -> [u8; 4] {
        [0x32, 0x02, 0x10, b]
    }
    const AV1C: [u8; 4] = [0x81, 0x00, 0x0C, 0x00];

    #[test]
    fn obus_are_split_by_their_sizes() {
        let tu = [&TD[..], &SEQ, &frame(1)].concat();
        let o = obus(&tu).unwrap();
        let kinds: Vec<u8> = o.iter().map(|p| p.0).collect();
        assert_eq!(kinds, [2, 1, 6]);
        assert_eq!(o[1].1, SEQ);
        // 大きさが中身より長い時と、大きさの欄が無い時は読めない
        assert!(obus(&[0x32, 0x05, 1]).is_none());
        assert!(obus(&[0x30, 1, 2]).is_none());
        // 2 バイトの leb128(130 バイト)
        let long = [&[0x7A, 0x82, 0x01][..], &[0; 130]].concat();
        assert_eq!(obus(&long).unwrap()[0].1.len(), 133);
    }

    #[test]
    fn boxes_are_laid_out_with_moov_first() {
        let mut m = Mp4Writer::new(64, 48, 10.0, &AV1C);
        m.push(&[&TD[..], &SEQ, &frame(1)].concat(), true).unwrap();
        m.push(&[&TD[..], &frame(2)].concat(), false).unwrap();
        m.push(&TD, false).unwrap(); // 絵が無い: 前の絵が 2 枚分になる
        m.push(&[&TD[..], &frame(3)].concat(), false).unwrap();
        assert_eq!(m.frame_count(), 3);
        assert!(m.push(&[0x32, 0x09], false).is_err());
        let mut out = Vec::new();
        m.finish(&mut out).unwrap();
        // 頭の箱の並び
        let mut kinds = Vec::new();
        let mut i = 0;
        while i < out.len() {
            let size = u32::from_be_bytes(out[i..i + 4].try_into().unwrap()) as usize;
            kinds.push(String::from_utf8_lossy(&out[i + 4..i + 8]).to_string());
            i += size;
        }
        assert_eq!(i, out.len());
        assert_eq!(kinds, ["ftyp", "moov", "mdat"]);
        // stco の位置に最初の絵(区切りを外した sequence header から)がある
        let at = out.windows(4).position(|w| w == b"stco").unwrap();
        let off = u32::from_be_bytes(out[at + 12..at + 16].try_into().unwrap()) as usize;
        assert_eq!(&out[off..off + 9], &[&SEQ[..], &frame(1)].concat()[..]);
        // stts: 1000 が 1 回、2000 が 1 回、1000 が 1 回
        let at = out.windows(4).position(|w| w == b"stts").unwrap();
        let n = u32::from_be_bytes(out[at + 8..at + 12].try_into().unwrap());
        assert_eq!(n, 3);
        // av1C は頭の 4 バイトと sequence header
        let at = out.windows(4).position(|w| w == b"av1C").unwrap();
        assert_eq!(&out[at + 4..at + 13], &[&AV1C[..], &SEQ].concat()[..]);
        assert_eq!(m.sync, [1]);
    }

    #[test]
    fn empty_writer_or_no_sequence_header_is_an_error() {
        assert!(
            Mp4Writer::new(16, 16, 30.0, &AV1C)
                .finish(Vec::new())
                .is_err()
        );
        let mut m = Mp4Writer::new(16, 16, 30.0, &AV1C);
        m.push(&frame(1), true).unwrap();
        assert!(m.finish(Vec::new()).is_err());
    }

    /// 半端な 1 秒あたりの枚数(29.97)も目盛りで表せる。
    #[test]
    fn fractional_fps_is_kept() {
        let m = Mp4Writer::new(16, 16, 29.97, &AV1C);
        assert_eq!((m.timescale, m.delta), (29970, 1000));
    }
}
