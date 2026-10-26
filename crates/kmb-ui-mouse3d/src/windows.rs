//! Windows で 3D マウスを探して読む(HID の関数と SetupDi)。
//!
//! 探す時は HID の機器を 1 つずつ、読み書きの権限なし(アクセス 0)・共有で開き、会社と製品の番号と
//! 用途だけを見る。アクセス 0 の開き方は報告を読まないので、キーボードやマウスの入力に触らない。
//! 多軸の機器だけを読み取りの権限で開き直し、重ねた I/O で読む(200 ms ごとに止まる合図を見る)。
//! メーカーのドライバが入っていても、HID は共有で開くので一緒に読める。
//!
//! OS の関数を呼ぶので、このファイルだけ unsafe を使う。
#![allow(unsafe_code)]

use crate::{Device, Reader, wanted};
use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FreePreparsedData, HidD_GetAttributes,
    HidD_GetHidGuid, HidD_GetPreparsedData, HidD_GetProductString, HidP_GetCaps,
    PHIDP_PREPARSED_DATA,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile,
};
use windows_sys::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};
use windows_sys::core::GUID;

/// 閉じ忘れない HANDLE。
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// 終わりに 0 の付いた幅の広い文字のパス。
fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn open(path: &[u16], access: u32, flags: u32) -> io::Result<Handle> {
    let h = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            flags,
            null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(Handle(h))
    }
}

/// 繋がっている 3D マウス。
pub(crate) fn scan() -> Vec<Device> {
    let mut guid = GUID::from_u128(0);
    unsafe { HidD_GetHidGuid(&mut guid) };
    let set: HDEVINFO = unsafe {
        SetupDiGetClassDevsW(
            &guid,
            null(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    if set == INVALID_HANDLE_VALUE as HDEVINFO {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0.. {
        let mut data = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInterfaces(set, null(), &guid, i, &mut data) } == 0 {
            break;
        }
        if let Some(d) = interface_path(set, &data).and_then(|p| probe(&p)) {
            out.push(d);
        }
    }
    unsafe { SetupDiDestroyDeviceInfoList(set) };
    out
}

/// 機器の口のパス(終わりに 0 付き)。
fn interface_path(set: HDEVINFO, data: &SP_DEVICE_INTERFACE_DATA) -> Option<Vec<u16>> {
    let mut need = 0u32;
    unsafe { SetupDiGetDeviceInterfaceDetailW(set, data, null_mut(), 0, &mut need, null_mut()) };
    let at = std::mem::offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);
    let need_len = need as usize;
    if need_len <= at {
        return None;
    }
    // 中身の頭の欄(cbSize)が 4 バイトの境に並ぶよう、u32 で取る
    let mut buf = vec![0u32; need_len.div_ceil(4)];
    let detail = buf.as_mut_ptr().cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
    unsafe { (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 };
    let ok = unsafe {
        SetupDiGetDeviceInterfaceDetailW(set, data, detail, need, null_mut(), null_mut())
    };
    if ok == 0 {
        return None;
    }
    let chars = unsafe {
        std::slice::from_raw_parts(
            buf.as_ptr().cast::<u8>().add(at).cast::<u16>(),
            (need_len - at) / 2,
        )
    };
    let len = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
    let mut path = chars[..len].to_vec();
    path.push(0);
    Some(path)
}

/// 中身を読まずに開いて番号と用途を見て、多軸の機器なら返す。
fn probe(path: &[u16]) -> Option<Device> {
    let h = open(path, 0, 0).ok()?;
    let mut attrs = HIDD_ATTRIBUTES {
        Size: size_of::<HIDD_ATTRIBUTES>() as u32,
        ..Default::default()
    };
    if !unsafe { HidD_GetAttributes(h.0, &mut attrs) } {
        return None;
    }
    let mut pp: PHIDP_PREPARSED_DATA = 0;
    if !unsafe { HidD_GetPreparsedData(h.0, &mut pp) } {
        return None;
    }
    let mut caps = HIDP_CAPS::default();
    let status = unsafe { HidP_GetCaps(pp, &mut caps) };
    unsafe { HidD_FreePreparsedData(pp) };
    if status != HIDP_STATUS_SUCCESS || !wanted(attrs.VendorID, caps.UsagePage, caps.Usage) {
        return None;
    }
    let mut name = [0u16; 128];
    let got = unsafe {
        HidD_GetProductString(h.0, name.as_mut_ptr().cast(), size_of_val(&name) as u32)
    };
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let name = if got && len > 0 {
        String::from_utf16_lossy(&name[..len])
    } else {
        "3D マウス".into()
    };
    Some(Device {
        path: PathBuf::from(OsString::from_wide(&path[..path.len() - 1])),
        name,
        vendor: attrs.VendorID,
        product: attrs.ProductID,
        shape: format!("入力の報告 {} バイト", caps.InputReportByteLength),
    })
}

/// 機器を開いて、外れるか止まる合図が来るまで読む。
pub(crate) fn read(dev: &Device, reader: &mut Reader<'_>) -> io::Result<()> {
    let h = open(&wide(&dev.path), GENERIC_READ, FILE_FLAG_OVERLAPPED)?;
    let ev = unsafe { CreateEventW(null(), 1, 0, null()) };
    if ev.is_null() {
        return Err(io::Error::last_os_error());
    }
    let ev = Handle(ev);
    reader.connected();
    // 入力の報告の長さ以上なら 1 回で 1 つの報告を受ける
    let mut buf = [0u8; 256];
    while !reader.stopped() {
        let mut ov = OVERLAPPED {
            hEvent: ev.0,
            ..Default::default()
        };
        unsafe { ResetEvent(ev.0) };
        let mut n = 0u32;
        let ok = unsafe { ReadFile(h.0, buf.as_mut_ptr(), buf.len() as u32, &mut n, &mut ov) };
        if ok == 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                return Err(e);
            }
            while unsafe { WaitForSingleObject(ev.0, 200) } == WAIT_TIMEOUT {
                if reader.stopped() {
                    // 読みかけを取り消し、終わるのを待ってから buf と ov を放す
                    unsafe {
                        CancelIo(h.0);
                        GetOverlappedResult(h.0, &ov, &mut n, 1);
                    }
                    return Ok(());
                }
            }
            if unsafe { GetOverlappedResult(h.0, &ov, &mut n, 0) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        reader.feed(&buf[..n as usize]);
    }
    Ok(())
}
