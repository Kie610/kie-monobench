//! 何枚もの絵を、ループする GIF に書く。
//!
//! - 色: GIF は 1 枚に 256 色まで。全部の絵で共通の色の表を 1 つ作る(絵ごとに表を作ると、
//!   同じ所の色が絵ごとに変わってちらつく)。使う色が 256 以下ならその色をそのまま使い、
//!   多ければ NeuQuant(color_quant)で選ぶ。学ぶ画素は全部の絵から間引いて集める。
//! - 透明: アルファが半分(128)より小さい画素は透明にする(GIF は透けるか透けないかだけ)。
//!   その時は表の最後(255 番)を透明の色に取っておき、絵ごとに前の絵を消してから描く。
//! - 時間: GIF の待ち時間は 1/100 秒の整数。積み上げの時刻から丸めるので、長く流しても
//!   ずれない。ブラウザは 0.01 秒以下の待ちを 0.1 秒に直すので、待ちは 0.02 秒以上にする
//!   (1 秒に 50 枚より速くはならない)。

use crate::Rgba;
use std::collections::HashMap;
use std::io::{self, Write};

/// 色を学ぶのに使う画素の数の上限(多い時は間引く)。
const SAMPLE_LIMIT: usize = 1 << 20;

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

fn opaque(p: &[u8; 4]) -> bool {
    p[3] >= 128
}

/// 全部の絵で共通の色の表。
enum Palette {
    /// 使う色が少ない時。色 → 番号。
    Exact(HashMap<[u8; 3], u8>),
    /// 多い時。NeuQuant で選んだ表。
    Learned(color_quant::NeuQuant),
}

impl Palette {
    fn build(frames: &[Rgba], colors: usize) -> (Self, Vec<u8>) {
        // まず、使う色が表に収まるか数える
        let mut exact: HashMap<[u8; 3], u8> = HashMap::new();
        let mut fits = true;
        'outer: for f in frames {
            for p in f.pixels.as_chunks::<4>().0.iter().filter(|p| opaque(p)) {
                let c = [p[0], p[1], p[2]];
                if !exact.contains_key(&c) {
                    if exact.len() == colors {
                        fits = false;
                        break 'outer;
                    }
                    let n = exact.len() as u8;
                    exact.insert(c, n);
                }
            }
        }
        if fits {
            let mut table = vec![0u8; 256 * 3];
            for (c, &i) in &exact {
                table[i as usize * 3..i as usize * 3 + 3].copy_from_slice(c);
            }
            return (Palette::Exact(exact), table);
        }

        // 多い: 全部の絵から間引いて学ぶ。間隔は幅と互いに素にして、同じ列ばかり拾わない
        let total: usize = frames.iter().map(|f| f.pixels.len() / 4).sum();
        let width = frames[0].width.max(1) as usize;
        let mut step = total.div_ceil(SAMPLE_LIMIT).max(1);
        while step > 1 && gcd(step, width) != 1 {
            step += 1;
        }
        let mut sample = Vec::with_capacity((total / step + 1) * 4);
        let mut k = 0usize;
        for f in frames {
            for p in f.pixels.as_chunks::<4>().0.iter() {
                if k.is_multiple_of(step) && opaque(p) {
                    sample.extend_from_slice(&[p[0], p[1], p[2], 255]);
                }
                k += 1;
            }
        }
        let nq = color_quant::NeuQuant::new(10, colors, &sample);
        let mut table = vec![0u8; 256 * 3];
        for (i, c) in nq.color_map_rgb().as_chunks::<3>().0.iter().enumerate() {
            table[i * 3..i * 3 + 3].copy_from_slice(c);
        }
        (Palette::Learned(nq), table)
    }

    fn index(&self, p: &[u8]) -> u8 {
        match self {
            Palette::Exact(m) => m[&[p[0], p[1], p[2]]],
            Palette::Learned(nq) => nq.index_of(&[p[0], p[1], p[2], 255]) as u8,
        }
    }
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// `frames` を、1 秒に `fps` 枚で、ループする GIF にして書く。
pub fn write_gif(frames: &[Rgba], fps: f32, w: impl Write) -> io::Result<()> {
    let (width, height) = crate::frame_size(frames).map_err(invalid)?;
    crate::check_fps(fps).map_err(invalid)?;
    let (Ok(gw), Ok(gh)) = (u16::try_from(width), u16::try_from(height)) else {
        return Err(invalid(format!(
            "GIF にできる大きさは 65535×65535 までです({width}×{height})"
        )));
    };
    let transparent = frames
        .iter()
        .any(|f| f.pixels.as_chunks::<4>().0.iter().any(|p| !opaque(p)));
    let colors = if transparent { 255 } else { 256 };
    let (palette, table) = Palette::build(frames, colors);

    let mut enc = ::gif::Encoder::new(w, gw, gh, &table).map_err(gif_error)?;
    enc.set_repeat(::gif::Repeat::Infinite).map_err(gif_error)?;
    let mut indexes = vec![0u8; width as usize * height as usize];
    let mut shown_cs = 0i64; // ここまでに書いた待ち時間の合計(1/100 秒)
    for (i, f) in frames.iter().enumerate() {
        for (dst, p) in indexes.iter_mut().zip(f.pixels.as_chunks::<4>().0.iter()) {
            *dst = if opaque(p) { palette.index(p) } else { 255 };
        }
        // 積み上げの時刻から待ちを決める(丸めの誤差をためない)
        let end_cs = ((i + 1) as f64 * 100.0 / fps as f64).round() as i64;
        let delay = (end_cs - shown_cs).clamp(2, u16::MAX as i64);
        shown_cs += delay;
        let frame = ::gif::Frame {
            width: gw,
            height: gh,
            delay: delay as u16,
            dispose: if transparent {
                ::gif::DisposalMethod::Background
            } else {
                ::gif::DisposalMethod::Keep
            },
            transparent: transparent.then_some(255),
            buffer: std::borrow::Cow::Borrowed(&indexes),
            ..Default::default()
        };
        enc.write_frame(&frame).map_err(gif_error)?;
    }
    enc.into_inner().map_err(gif_error)?;
    Ok(())
}

fn gif_error(e: ::gif::EncodingError) -> io::Error {
    match e {
        ::gif::EncodingError::Io(e) => e,
        e => io::Error::other(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> (Vec<(u16, Vec<u8>)>, ::gif::Repeat) {
        let mut opts = ::gif::DecodeOptions::new();
        opts.set_color_output(::gif::ColorOutput::RGBA);
        let mut d = opts.read_info(bytes).unwrap();
        let mut out = Vec::new();
        while let Some(f) = d.read_next_frame().unwrap() {
            out.push((f.delay, f.buffer.to_vec()));
        }
        let repeat = d.repeat();
        (out, repeat)
    }

    #[test]
    fn few_colours_are_kept_exactly_and_it_loops() {
        let mut frames = Vec::new();
        for k in 0..4u8 {
            let mut f = Rgba::new(8, 4);
            for (i, p) in f.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                p.copy_from_slice(&[(i as u8 * 7) ^ k, 100, 200 - k, 255]);
            }
            frames.push(f);
        }
        let mut out = Vec::new();
        write_gif(&frames, 10.0, &mut out).unwrap();
        let (got, repeat) = decode(&out);
        assert_eq!(repeat, ::gif::Repeat::Infinite);
        assert_eq!(got.len(), 4);
        for (g, f) in got.iter().zip(&frames) {
            assert_eq!(g.0, 10);
            assert_eq!(g.1, f.pixels);
        }
    }

    #[test]
    fn many_colours_share_one_table_and_stay_close() {
        let frames = crate::test_frames(64, 48, 5);
        let mut out = Vec::new();
        write_gif(&frames, 30.0, &mut out).unwrap();
        let (got, _) = decode(&out);
        assert_eq!(got.len(), 5);
        // 30 枚/秒: 3, 4, 3, 3, 4 … の 1/100 秒で、合計は 5/30 秒 ≒ 17
        let total: u32 = got.iter().map(|g| g.0 as u32).sum();
        assert_eq!(total, 17);
        // 1 つの共通の表: 各絵にローカルの表は無い
        let mut d = ::gif::DecodeOptions::new().read_info(&out[..]).unwrap();
        assert!(d.global_palette().is_some());
        while let Some(f) = d.read_next_frame().unwrap() {
            assert!(f.palette.is_none());
        }
        // 色の差の平均は小さい
        let mut err = 0u64;
        for (g, f) in got.iter().zip(&frames) {
            for (a, b) in g.1.iter().zip(&f.pixels) {
                err += (*a as i32 - *b as i32).unsigned_abs() as u64;
            }
        }
        let mean = err as f64 / (frames.len() * frames[0].pixels.len()) as f64;
        assert!(mean < 6.0, "{mean}");
    }

    #[test]
    fn transparent_pixels_stay_transparent() {
        let mut frames = crate::test_frames(32, 16, 2);
        for f in &mut frames {
            for p in f.pixels.as_chunks_mut::<4>().0.iter_mut().take(40) {
                p[3] = 0;
            }
        }
        let mut out = Vec::new();
        write_gif(&frames, 25.0, &mut out).unwrap();
        let (got, _) = decode(&out);
        for g in &got {
            assert_eq!(g.0, 4);
            assert!(g.1.as_chunks::<4>().0.iter().take(40).all(|p| p[3] == 0));
            assert!(g.1.as_chunks::<4>().0.iter().skip(40).all(|p| p[3] == 255));
        }
    }

    #[test]
    fn mismatched_sizes_are_an_error() {
        let frames = [Rgba::new(4, 4), Rgba::new(5, 4)];
        let e = write_gif(&frames, 10.0, Vec::new()).unwrap_err();
        assert!(e.to_string().contains("2 枚目"), "{e}");
        assert!(write_gif(&[], 10.0, Vec::new()).is_err());
        assert!(write_gif(&[Rgba::new(2, 2)], 0.0, Vec::new()).is_err());
    }
}
