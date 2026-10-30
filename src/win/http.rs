//! Windows'un yerleşik WinHTTP'siyle tek bir HTTPS POST (TLS kütüphanesi gömmeden). Sistem vekil
//! sunucu ayarlarını ve sertifika deposunu kullanır.
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

use super::ui::wide;

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn check(h: *mut c_void) -> Result<Handle, String> {
    if h.is_null() { Err(net_error()) } else { Ok(Handle(h)) }
}

fn net_error() -> String {
    match unsafe { GetLastError() } {
        12002 => "OpenAI zaman aşımına uğradı; tekrar deneyin.".into(),
        12007 => "İnternet bağlantısı yok (api.openai.com bulunamadı).".into(),
        12029 | 12030 => "OpenAI'ye bağlanılamadı; internet bağlantısını kontrol edin.".into(),
        12175 => "OpenAI ile güvenli bağlantı kurulamadı (sertifika).".into(),
        code => format!("Ağ hatası (WinHTTP {code})."),
    }
}

/// `https://{host}{path}` adresine POST; (durum kodu, gövde).
pub fn post(host: &str, path: &str, headers: &str, body: &[u8]) -> Result<(u32, Vec<u8>), String> {
    unsafe {
        let agent = wide("ChatGPT-Dikte/3");
        let session = check(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0))?;
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 60_000, 90_000);
        let host = wide(host);
        let connection = check(WinHttpConnect(session.0, host.as_ptr(), 443, 0))?;
        let (verb, path) = (wide("POST"), wide(path));
        let request = check(WinHttpOpenRequest(connection.0, verb.as_ptr(), path.as_ptr(), null(), null(), null(),
                                               WINHTTP_FLAG_SECURE))?;
        let headers: Vec<u16> = headers.encode_utf16().collect();
        let len = body.len() as u32;
        if WinHttpSendRequest(request.0, headers.as_ptr(), headers.len() as u32, body.as_ptr() as *const c_void, len,
                              len, 0) == 0
            || WinHttpReceiveResponse(request.0, null_mut()) == 0
        {
            return Err(net_error());
        }
        let mut status = 0u32;
        let mut size = size_of_u32();
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, null(),
                            &mut status as *mut u32 as *mut c_void, &mut size, null_mut());
        let mut out = Vec::new();
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let mut read = 0u32;
            if WinHttpReadData(request.0, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut read) == 0 {
                return Err(net_error());
            }
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
        }
        Ok((status, out))
    }
}

fn size_of_u32() -> u32 {
    std::mem::size_of::<u32>() as u32
}
