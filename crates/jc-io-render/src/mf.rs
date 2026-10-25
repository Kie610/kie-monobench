//! Windows の Media Foundation で、絵を H.264 の MP4 にする(OS が持つ符号器と書き手を使う)。
//!
//! H.264 の符号器は Windows に入っている物(グラフィックスの部品が持つ速い物があればそれ)なので、
//! アプリには何も入れず、何も落とさない。Media Foundation の無い Windows(N 版で Media Feature Pack を
//! 入れていない物)ではアプリが起動できなくならないよう、DLL と関数は実行時に探す。使えない時は誤りを
//! 返し、呼ぶ側が AV1([`crate::av1`])にする。
//!
//! 色は YUV 4:2:0(NV12、限られた幅)にして渡す。式は再生する側が大きさから推す物に合わせる
//! ([`Matrix::guessed`])。書き手は一時ファイルに書くので、書き終えたら読んで消す。
//!
//! OS の関数と COM を呼ぶので、このファイルだけ unsafe を使う。
#![allow(unsafe_code)]

use crate::Rgba;
use crate::yuv::{Matrix, even_size, rgba_to_i420};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{HMODULE, RPC_E_CHANGED_MODE};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{HRESULT, HSTRING, Interface, PCSTR, PCWSTR, s, w};

type Startup = unsafe extern "system" fn(u32, u32) -> HRESULT;
type Shutdown = unsafe extern "system" fn() -> HRESULT;
type CreateAttributes = unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT;
type CreateObject = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type CreateBuffer = unsafe extern "system" fn(u32, *mut *mut c_void) -> HRESULT;
type CreateSinkWriter =
    unsafe extern "system" fn(PCWSTR, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;

/// 実行時に探した Media Foundation の関数。
struct Api {
    startup: Startup,
    shutdown: Shutdown,
    create_attributes: CreateAttributes,
    create_media_type: CreateObject,
    create_memory_buffer: CreateBuffer,
    create_sample: CreateObject,
    create_sink_writer: CreateSinkWriter,
}

/// System32 の DLL を読む(探す場所を System32 に限り、別の場所の同じ名前の物を読まない)。
fn load(dll: PCWSTR) -> Result<HMODULE, String> {
    // SAFETY: 名前は終わりに 0 のある文字列の定数
    unsafe { LoadLibraryExW(dll, None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
        .map_err(|e| format!("Media Foundation が無い Windows です({e})"))
}

/// `module` の関数 `name` を `T` の形で返す。
///
/// # Safety
/// `T` は関数 `name` の本当の形の関数ポインタであること。
unsafe fn find<T: Copy>(module: HMODULE, name: PCSTR) -> Result<T, String> {
    // SAFETY: 名前は終わりに 0 のある文字列の定数。返った物を形 T として読むのは呼ぶ側が保証する
    unsafe {
        let f = GetProcAddress(module, name).ok_or_else(|| {
            format!(
                "Media Foundation に {} がありません",
                name.to_string().unwrap_or_default()
            )
        })?;
        Ok(std::mem::transmute_copy(&f))
    }
}

impl Api {
    fn load() -> Result<Self, String> {
        let plat = load(w!("mfplat.dll"))?;
        let rw = load(w!("mfreadwrite.dll"))?;
        // SAFETY: 形は Windows SDK の mfapi.h・mfreadwrite.h の宣言のとおり
        unsafe {
            Ok(Self {
                startup: find(plat, s!("MFStartup"))?,
                shutdown: find(plat, s!("MFShutdown"))?,
                create_attributes: find(plat, s!("MFCreateAttributes"))?,
                create_media_type: find(plat, s!("MFCreateMediaType"))?,
                create_memory_buffer: find(plat, s!("MFCreateMemoryBuffer"))?,
                create_sample: find(plat, s!("MFCreateSample"))?,
                create_sink_writer: find(rw, s!("MFCreateSinkWriterFromURL"))?,
            })
        }
    }
}

/// 作る関数の返した物を COM の物にする。
///
/// # Safety
/// `make` は成功した時に `I` の参照を 1 つ返すこと。
unsafe fn make<I: Interface>(
    make: impl FnOnce(*mut *mut c_void) -> HRESULT,
) -> windows::core::Result<I> {
    let mut raw = std::ptr::null_mut();
    make(&mut raw).ok()?;
    // SAFETY: 成功したので raw は I の参照(呼ぶ側の保証)
    Ok(unsafe { I::from_raw(raw) })
}

/// COM と Media Foundation を始め、終わる時に閉じる。
struct Session<'a> {
    api: &'a Api,
    com: bool,
}

impl<'a> Session<'a> {
    fn start(api: &'a Api) -> Result<Self, String> {
        // SAFETY: このスレッドで COM を始める。別の形で始まっていれば(RPC_E_CHANGED_MODE)閉じない
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let com = hr.is_ok();
        if !com && hr != RPC_E_CHANGED_MODE {
            return Err(format!("COM を始められません({hr:?})"));
        }
        // SAFETY: 版は SDK の値
        if let Err(e) = unsafe { (api.startup)(MF_VERSION, MFSTARTUP_NOSOCKET) }.ok() {
            if com {
                // SAFETY: 上で始めた COM を閉じる
                unsafe { CoUninitialize() };
            }
            return Err(format!("Media Foundation を始められません({e})"));
        }
        Ok(Self { api, com })
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        // SAFETY: start で始めた物を逆の順に閉じる
        unsafe {
            let _ = (self.api.shutdown)();
            if self.com {
                CoUninitialize();
            }
        }
    }
}

/// 書き終えたら(しくじっても)消す一時ファイル。
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temp_path() -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    std::env::temp_dir().join(format!(
        "jc-video-{}-{}.mp4",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// `frames` を 1 秒に `fps` 枚の H.264 の MP4 にする。`bitrate` は 1 秒あたりのビット数の目安。
pub fn encode_mp4(frames: &[Rgba], fps: f32, bitrate: u32) -> Result<Vec<u8>, String> {
    let (iw, ih) = crate::frame_size(frames)?;
    crate::check_fps(fps)?;
    let size = even_size(iw, ih)?;
    let api = Api::load()?;
    let _session = Session::start(&api)?;
    // グラフィックスの部品の符号器(速い)で試し、だめなら Windows の符号器だけで書く
    let mut why = Vec::new();
    for hardware in [true, false] {
        let file = TempFile(temp_path());
        match write(&api, &file.0, frames, size, fps, bitrate, hardware) {
            Ok(()) => {
                return std::fs::read(&file.0).map_err(|e| format!("書いた MP4 を読めません: {e}"));
            }
            Err(e) => why.push(e.to_string()),
        }
    }
    Err(format!("Windows の H.264 で書けません({})", why.join("。")))
}

/// 幅と高さ、または分子と分母を 1 つの 64 ビットの値にする(Media Foundation の決まり)。
fn pack(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

fn write(
    api: &Api,
    path: &Path,
    frames: &[Rgba],
    (w, h): (u32, u32),
    fps: f32,
    bitrate: u32,
    hardware: bool,
) -> windows::core::Result<()> {
    let rate = (((fps as f64) * 1000.0).round() as u32).max(1);
    let matrix = match Matrix::guessed(h) {
        Matrix::Bt601 => MFVideoTransferMatrix_BT601,
        Matrix::Bt709 => MFVideoTransferMatrix_BT709,
    };
    // SAFETY: Media Foundation の決まりどおりに呼ぶ。作った物は COM の参照の数で片付く
    unsafe {
        let attrs: IMFAttributes = make(|p| (api.create_attributes)(p, 3))?;
        attrs.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)?;
        attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, hardware as u32)?;
        attrs.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
        let url = HSTRING::from(path.as_os_str());
        let writer: IMFSinkWriter = make(|p| {
            (api.create_sink_writer)(
                PCWSTR(url.as_ptr()),
                std::ptr::null_mut(),
                attrs.as_raw(),
                p,
            )
        })?;

        let video_type = |subtype: &windows::core::GUID| -> windows::core::Result<IMFMediaType> {
            let t: IMFMediaType = make(|p| (api.create_media_type)(p))?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
            t.SetUINT64(&MF_MT_FRAME_SIZE, pack(w, h))?;
            t.SetUINT64(&MF_MT_FRAME_RATE, pack(rate, 1000))?;
            t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            t.SetUINT32(&MF_MT_YUV_MATRIX, matrix.0 as u32)?;
            t.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
            Ok(t)
        };
        let out = video_type(&MFVideoFormat_H264)?;
        out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
        if hardware {
            // 速い符号器は High の profile で(Windows の符号器だけの時は既定のまま)
            out.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)?;
        }
        let stream = writer.AddStream(&out)?;
        writer.SetInputMediaType(stream, &video_type(&MFVideoFormat_NV12)?, None)?;
        writer.BeginWriting()?;

        let (wu, hu) = (w as usize, h as usize);
        let len = wu * hu * 3 / 2;
        let mut yuv = Vec::new();
        // 時刻は 100 ナノ秒の目盛り
        let at = |i: usize| (i as f64 * 10_000_000.0 / fps as f64).round() as i64;
        for (i, img) in frames.iter().enumerate() {
            rgba_to_i420(img, (w, h), Matrix::guessed(h), &mut yuv);
            let buffer: IMFMediaBuffer = make(|p| (api.create_memory_buffer)(len as u32, p))?;
            let mut dst = std::ptr::null_mut();
            buffer.Lock(&mut dst, None, None)?;
            let nv12 = std::slice::from_raw_parts_mut(dst, len);
            // Y の面はそのまま、U と V は交互に並べる
            nv12[..wu * hu].copy_from_slice(&yuv[..wu * hu]);
            let quarter = wu * hu / 4;
            let (u, v) = yuv[wu * hu..].split_at(quarter);
            for (k, (a, b)) in u.iter().zip(v).enumerate() {
                nv12[wu * hu + 2 * k] = *a;
                nv12[wu * hu + 2 * k + 1] = *b;
            }
            buffer.Unlock()?;
            buffer.SetCurrentLength(len as u32)?;
            let sample: IMFSample = make(|p| (api.create_sample)(p))?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(at(i))?;
            sample.SetSampleDuration(at(i + 1) - at(i))?;
            writer.WriteSample(stream, &sample)?;
        }
        writer.Finalize()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4::tests::{have, probe, scratch};

    /// Windows の H.264 で書き、MP4 の頭と(ffprobe があれば)中身を確かめる。
    /// Media Foundation の無い Windows では理由を出して飛ばす。
    #[test]
    fn windows_h264_makes_a_readable_mp4() {
        let frames = crate::test_frames(161, 97, 24);
        let bytes = match encode_mp4(&frames, 12.0, 500_000) {
            Ok(b) => b,
            Err(e) if e.contains("Media Foundation が無い") => {
                jc_contract::skip!("{e}。飛ばします");
            }
            Err(e) => panic!("{e}"),
        };
        assert_eq!(&bytes[4..8], b"ftyp");
        if !have("ffprobe") || !have("ffmpeg") {
            jc_contract::skip!("ffprobe が無いので、中身は確かめません");
        }
        let path = scratch("mf").join("out.mp4");
        std::fs::write(&path, &bytes).unwrap();
        let (n, rate, codec, size) = probe(&path);
        assert_eq!((n, codec.as_str(), size), (24, "h264", (160, 96)));
        // 1 秒あたりの枚数は分数(書き手が目盛りを決める)
        let (a, b) = rate.split_once('/').unwrap();
        let rate = a.parse::<f64>().unwrap() / b.parse::<f64>().unwrap();
        assert!((rate - 12.0).abs() < 0.01, "{rate}");
    }
}
