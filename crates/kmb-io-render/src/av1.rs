//! 絵を AV1 にして MP4 に詰める(rav1e。純 Rust で、アプリに入っている)。
//!
//! AV1 は使用料なしで使える符号(AOMedia の特許の許諾)。Windows では OS の H.264([`crate::mf`])を
//! 先に使い、使えない時と Windows 以外でこれを使う。
//!
//! 色は BT.709 の限られた幅の YUV 4:2:0 にし、その旨を流れ(sequence header)に書く。
//! 2 秒ごとにキーフレームを入れ、途中から再生しやすくする。符号の速さを優先する。

use crate::Rgba;
use crate::mp4::Mp4Writer;
use crate::yuv::{Matrix, even_size, rgba_to_i420};
use rav1e::prelude::*;

/// `frames` を 1 秒に `fps` 枚の AV1 の MP4 にする。`bitrate` は 1 秒あたりのビット数の目安。
pub fn encode_mp4(frames: &[Rgba], fps: f32, bitrate: u32) -> Result<Vec<u8>, String> {
    let (iw, ih) = crate::frame_size(frames)?;
    crate::check_fps(fps)?;
    let (w, h) = even_size(iw, ih)?;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let period = ((fps * 2.0).round() as u64).max(1);
    let enc = EncoderConfig {
        width: w as usize,
        height: h as usize,
        // 目盛りは 1 枚 = 1000(29.97 のような半端な値も表せる)
        time_base: Rational::new(1000, ((fps as f64 * 1000.0).round() as u64).max(1)),
        bit_depth: 8,
        chroma_sampling: ChromaSampling::Cs420,
        pixel_range: PixelRange::Limited,
        color_description: Some(ColorDescription {
            color_primaries: ColorPrimaries::BT709,
            transfer_characteristics: TransferCharacteristics::BT709,
            matrix_coefficients: MatrixCoefficients::BT709,
        }),
        bitrate: bitrate.min(i32::MAX as u32) as i32,
        min_key_frame_interval: period.min(12),
        max_key_frame_interval: period,
        speed_settings: SpeedSettings::from_preset(10),
        // 絵を区切った枡(タイル)ごとに並べて符号にする
        tiles: threads.min(8),
        ..Default::default()
    };
    let cfg = Config::new().with_encoder_config(enc).with_threads(threads);
    let mut ctx: Context<u8> = cfg
        .new_context()
        .map_err(|e| format!("AV1 の符号器を作れません: {e}"))?;
    let mut mp4 = Mp4Writer::new(w, h, fps, &ctx.container_sequence_header());
    let mut yuv = Vec::new();
    let (cw, ch) = (w as usize / 2, h as usize / 2);
    let luma = w as usize * h as usize;
    for img in frames {
        rgba_to_i420(img, (w, h), Matrix::Bt709, &mut yuv);
        let mut f = ctx.new_frame();
        f.planes[0].copy_from_raw_u8(&yuv[..luma], w as usize, 1);
        f.planes[1].copy_from_raw_u8(&yuv[luma..luma + cw * ch], cw, 1);
        f.planes[2].copy_from_raw_u8(&yuv[luma + cw * ch..], cw, 1);
        ctx.send_frame(f)
            .map_err(|e| format!("AV1 にできません: {e}"))?;
        drain(&mut ctx, &mut mp4)?;
    }
    ctx.flush();
    drain(&mut ctx, &mut mp4)?;
    if mp4.frame_count() == 0 {
        return Err("AV1 の符号器が絵を返しませんでした".into());
    }
    let mut out = Vec::new();
    mp4.finish(&mut out)
        .map_err(|e| format!("MP4 を書けません: {e}"))?;
    Ok(out)
}

/// できた絵を MP4 に入れる(符号器が次の絵を待つか、終わるまで)。
fn drain(ctx: &mut Context<u8>, mp4: &mut Mp4Writer) -> Result<(), String> {
    loop {
        match ctx.receive_packet() {
            Ok(p) => mp4
                .push(&p.data, p.frame_type == FrameType::KEY)
                .map_err(|e| format!("MP4 に入れられません: {e}"))?,
            Err(EncoderStatus::Encoded) => {}
            Err(EncoderStatus::NeedMoreData | EncoderStatus::LimitReached) => return Ok(()),
            Err(e) => return Err(format!("AV1 にできません: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4::tests::{have, probe, scratch};

    /// 奇数の大きさの絵を AV1 の MP4 にし、ffprobe で読む。ffmpeg で絵に戻して元の絵と比べる。
    #[test]
    fn frames_become_a_readable_av1_mp4() {
        let frames = crate::test_frames(97, 65, 24);
        let t = std::time::Instant::now();
        let bytes = encode_mp4(&frames, 12.0, 400_000).unwrap();
        eprintln!(
            "AV1: 96×64 を 24 枚、{:.2} 秒、{} バイト",
            t.elapsed().as_secs_f64(),
            bytes.len()
        );
        assert_eq!(&bytes[4..12], b"ftypisom");
        if !have("ffprobe") || !have("ffmpeg") {
            kmb_contract::skip!("ffmpeg か ffprobe が無いので、中身は確かめません");
        }
        let dir = scratch("av1");
        let path = dir.join("out.mp4");
        std::fs::write(&path, &bytes).unwrap();
        let (n, rate, codec, size) = probe(&path);
        assert_eq!(
            (n, rate.as_str(), codec.as_str(), size),
            (24, "12/1", "av1", (96, 64))
        );
        // 絵に戻し、最後の絵の明るさ(Y)を元の絵と比べる
        let o = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&path)
            .args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
            .output()
            .unwrap();
        if !o.status.success() {
            kmb_contract::skip!(
                "ffmpeg で AV1 を絵に戻せないので、色は確かめません: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        let one = 96 * 64 * 3 / 2;
        assert_eq!(o.stdout.len(), one * 24);
        let last = &o.stdout[one * 23..one * 23 + 96 * 64];
        let mut want = Vec::new();
        rgba_to_i420(&frames[23], (96, 64), Matrix::Bt709, &mut want);
        let mse = last
            .iter()
            .zip(&want[..96 * 64])
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / last.len() as f64;
        let psnr = 10.0 * (255.0f64 * 255.0 / mse.max(1e-9)).log10();
        eprintln!("AV1: 最後の絵の明るさの PSNR {psnr:.1} dB");
        assert!(psnr > 28.0, "{psnr}");
    }
}
