//! 集めた絵を、選んだ形式のファイルの中身にする。
//!
//! 連番 PNG は何枚ものファイルに、GIF と MP4 は 1 つのファイルになる。
//! MP4 は Windows では OS の H.264 で、使えない時と Windows 以外は AV1 で書く([`encode_mp4`])。

use crate::Rgba;

/// (ファイルの名前, 中身)の並び。
pub type Files = Vec<(String, Vec<u8>)>;

/// 動画の形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFormat {
    /// 1 枚ずつの PNG(`名前-0001.png` …)。
    PngSequence,
    /// ループする GIF。
    Gif,
    /// MP4(H.264 か AV1。[`encode_mp4`])。
    Mp4,
}

impl VideoFormat {
    /// 拡張子(頭の `.` はあってもなくてもよい。大文字も可)から決める。
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "png" => Some(Self::PngSequence),
            "gif" => Some(Self::Gif),
            "mp4" | "m4v" => Some(Self::Mp4),
            _ => None,
        }
    }

    /// 書くファイルの拡張子(`.` なし)。
    pub fn extension(self) -> &'static str {
        match self {
            Self::PngSequence => "png",
            Self::Gif => "gif",
            Self::Mp4 => "mp4",
        }
    }
}

/// MP4 の 1 秒あたりのビット数の目安。CAD の絵は平らな色が多いので控えめ(1 画素 1 枚あたり 0.1 ビット)。
pub fn default_bitrate(width: u32, height: u32, fps: f32) -> u32 {
    let bits = width as f64 * height as f64 * fps as f64 * 0.1;
    bits.clamp(500_000.0, 20_000_000.0) as u32
}

/// MP4 の中の符号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mp4Codec {
    /// Windows の Media Foundation の H.264(どこでも再生できる)。
    H264,
    /// アプリに入っている rav1e の AV1。`fallback` は Windows の H.264 を使えなかった理由。
    Av1 { fallback: Option<String> },
}

impl Mp4Codec {
    /// 知らせに添える短い名前(H.264・AV1)。
    pub fn label(&self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::Av1 { .. } => "AV1",
        }
    }

    /// 利用者に知らせること(AV1 になった時の、理由と再生の仕方)。
    pub fn note(&self) -> Option<String> {
        match self {
            Self::H264 => None,
            Self::Av1 { fallback } => Some(format!(
                "{}AV1 で書いた(Windows では Microsoft Store の無料の「AV1 Video Extension」か VLC などで再生できる)",
                fallback
                    .as_deref()
                    .map(|w| format!("{w}ので、"))
                    .unwrap_or_default()
            )),
        }
    }
}

/// `frames` を 1 秒に `fps` 枚の MP4 にする。Windows では OS の H.264 を先に使い、使えなければ AV1 にする。
pub fn encode_mp4(frames: &[Rgba], fps: f32) -> Result<(Vec<u8>, Mp4Codec), String> {
    let (width, height) = crate::frame_size(frames)?;
    crate::check_fps(fps)?;
    let bitrate = default_bitrate(width, height, fps);
    #[cfg(windows)]
    let fallback = match crate::mf::encode_mp4(frames, fps, bitrate) {
        Ok(bytes) => return Ok((bytes, Mp4Codec::H264)),
        Err(e) => {
            log::warn!("MP4: {e}。AV1 で書く");
            Some(e)
        }
    };
    #[cfg(not(windows))]
    let fallback = None;
    let bytes = crate::av1::encode_mp4(frames, fps, bitrate)?;
    Ok((bytes, Mp4Codec::Av1 { fallback }))
}

/// `frames` を 1 秒に `fps` 枚の動画にし、(ファイルの名前, 中身)の並びと、MP4 なら中の符号を返す。
/// 名前は `stem` に拡張子(連番 PNG は `-0001` などの番号も)を付けた物。
pub fn encode_with_codec(
    format: VideoFormat,
    frames: &[Rgba],
    fps: f32,
    stem: &str,
) -> Result<(Files, Option<Mp4Codec>), String> {
    crate::frame_size(frames)?;
    crate::check_fps(fps)?;
    match format {
        VideoFormat::PngSequence => Ok((crate::png_seq::png_sequence(frames, stem), None)),
        VideoFormat::Gif => {
            let mut out = Vec::new();
            crate::gif::write_gif(frames, fps, &mut out)
                .map_err(|e| format!("GIF を書けません: {e}"))?;
            Ok((vec![(format!("{stem}.gif"), out)], None))
        }
        VideoFormat::Mp4 => {
            let (out, codec) = encode_mp4(frames, fps)?;
            Ok((vec![(format!("{stem}.mp4"), out)], Some(codec)))
        }
    }
}

/// [`encode_with_codec`] のファイルだけ。
pub fn encode(format: VideoFormat, frames: &[Rgba], fps: f32, stem: &str) -> Result<Files, String> {
    encode_with_codec(format, frames, fps, stem).map(|(files, _)| files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_comes_from_the_extension() {
        assert_eq!(VideoFormat::from_extension("GIF"), Some(VideoFormat::Gif));
        assert_eq!(VideoFormat::from_extension(".mp4"), Some(VideoFormat::Mp4));
        assert_eq!(
            VideoFormat::from_extension("png"),
            Some(VideoFormat::PngSequence)
        );
        assert_eq!(VideoFormat::from_extension("avi"), None);
        for f in [VideoFormat::PngSequence, VideoFormat::Gif, VideoFormat::Mp4] {
            assert_eq!(VideoFormat::from_extension(f.extension()), Some(f));
        }
    }

    #[test]
    fn png_and_gif_are_named_from_the_stem() {
        let frames = crate::test_frames(20, 10, 3);
        let png = encode(VideoFormat::PngSequence, &frames, 10.0, "a").unwrap();
        assert_eq!(png.len(), 3);
        assert_eq!(png[2].0, "a-0003.png");
        let gif = encode(VideoFormat::Gif, &frames, 10.0, "a").unwrap();
        assert_eq!(gif.len(), 1);
        assert_eq!(gif[0].0, "a.gif");
        assert!(gif[0].1.starts_with(b"GIF89a"));
    }

    /// MP4 は何も入れずに書ける(Windows は H.264、ほかは AV1)。
    #[test]
    fn mp4_is_written_without_anything_installed() {
        let frames = crate::test_frames(32, 24, 4);
        let (files, codec) = encode_with_codec(VideoFormat::Mp4, &frames, 10.0, "a").unwrap();
        assert_eq!(files[0].0, "a.mp4");
        assert_eq!(&files[0].1[4..8], b"ftyp");
        let codec = codec.unwrap();
        if cfg!(windows) {
            eprintln!("MP4 の符号: {codec:?}");
        } else {
            assert_eq!(codec, Mp4Codec::Av1 { fallback: None });
            assert!(codec.note().unwrap().contains("AV1 Video Extension"));
        }
        assert!(encode(VideoFormat::Gif, &[], 10.0, "a").is_err());
        assert!(encode(VideoFormat::Mp4, &frames[..1], 0.0, "a").is_err());
    }

    #[test]
    fn bitrate_is_clamped() {
        assert_eq!(default_bitrate(64, 48, 10.0), 500_000);
        assert_eq!(default_bitrate(1920, 1080, 30.0), 6_220_800);
        assert_eq!(default_bitrate(3840, 2160, 60.0), 20_000_000);
    }
}
