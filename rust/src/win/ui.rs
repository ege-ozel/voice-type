//! Arayüz (tek iş parçacığı): gizli ana pencere (kısayollar, tepsi simgesi, zamanlayıcı),
//! sağ alttaki odak çalmayan rozet ve durum penceresi. Çizim düz GDI ile yapılır.
use std::cell::{Cell, RefCell};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_BACK, VK_ESCAPE, VK_RETURN, VK_SPACE,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::gfx;
use super::platform::WinPlatform;
use crate::bridge::Bridge;
use crate::view::{self, palette, BadgePhase};

pub const APP_NAME: &str = "ChatGPT Dikte";

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Toggle,
    ToggleEnter,
    Cancel,
}

const WM_TRAY: u32 = WM_APP + 1;
const TIMER_REFRESH: usize = 1;
const MENU_STATUS: usize = 100;
const MENU_QUIT: usize = 101;
const BUTTON_HIDE: usize = 201;
const BUTTON_QUIT: usize = 202;
const BADGE_W: f32 = 212.0;
const BADGE_H: f32 = 60.0;

static BRIDGE: OnceLock<Arc<Bridge<WinPlatform>>> = OnceLock::new();
static ACTIONS: OnceLock<Sender<Action>> = OnceLock::new();

#[derive(Clone, Copy)]
struct Handles {
    host: HWND,
    badge: HWND,
    status: HWND,
    scale: f32,
    icon_small: HICON,
    font_badge_title: HFONT,
    font_badge_sub: HFONT,
    font_heading: HFONT,
    font_label: HFONT,
    font_bold: HFONT,
    font_normal: HFONT,
    font_mono: HFONT,
    taskbar_created: u32,
}

#[derive(Default)]
struct Memory {
    /// (aşama, alt yazı, kayıt noktası yanık mı)
    badge: Option<(BadgePhase, String, bool)>,
    tip: String,
    status: Option<crate::bridge::Snapshot>,
}

thread_local! {
    static H: Cell<Option<Handles>> = const { Cell::new(None) };
    static MEM: RefCell<Memory> = RefCell::new(Memory::default());
}

fn started() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn h() -> Handles {
    H.with(|h| h.get()).expect("arayüz hazır değil")
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn px(v: f32) -> i32 {
    (v * h().scale).round() as i32
}

pub fn message_box(text: &str, error: bool) {
    let flags = if error { MB_ICONERROR } else { MB_ICONINFORMATION };
    unsafe { MessageBoxW(null_mut(), wide(text).as_ptr(), wide(APP_NAME).as_ptr(), flags | MB_SETFOREGROUND) };
}

/// Yazı tipi; yüz yoksa (ör. eski Windows'ta Bahnschrift) GDI en yakınını seçer.
fn font(face: &str, points: i32, weight: i32, dpi: i32) -> HFONT {
    unsafe {
        CreateFontW(-(points * dpi + 36) / 72, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32, CLIP_DEFAULT_PRECIS as u32, CLEARTYPE_QUALITY as u32,
                    (VARIABLE_PITCH | FF_SWISS) as u32, wide(face).as_ptr())
    }
}

/// Simge kaynağı (build.rs ile gömülen, kimlik 1) istenen boyutta.
fn load_icon(instance: HINSTANCE, size: i32) -> HICON {
    unsafe { LoadImageW(instance, 1 as _, IMAGE_ICON, size, size, LR_DEFAULTCOLOR) as HICON }
}

fn register_class(instance: HINSTANCE, name: &[u16], proc_: WNDPROC, icon: HICON, icon_small: HICON) {
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: proc_,
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: icon,
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: name.as_ptr(),
        hIconSm: icon_small,
    };
    unsafe { RegisterClassExW(&class) };
}

/// Çalışma alanının (görev çubuğu hariç) sağ alt köşesi.
fn badge_position() -> (i32, i32) {
    let mut area: RECT = unsafe { zeroed() };
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut area as *mut RECT as *mut _, 0) } != 0;
    if !ok {
        area.right = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        area.bottom = unsafe { GetSystemMetrics(SM_CYSCREEN) } - px(48.0);
    }
    ((area.right - px(BADGE_W + 20.0)).max(area.left), (area.bottom - px(BADGE_H + 20.0)).max(area.top))
}

fn place_badge(show: bool) {
    let hs = h();
    let (x, y) = badge_position();
    let mut flags = SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    if show {
        flags |= SWP_SHOWWINDOW;
    }
    let (w, height) = (px(BADGE_W), px(BADGE_H));
    unsafe {
        SetWindowPos(hs.badge, HWND_TOPMOST, x, y, w, height, flags);
        // Yuvarlak köşeler (pencere bölgesi; boyut değişmedikçe bir kez).
        SetWindowRgn(hs.badge, CreateRoundRectRgn(0, 0, w + 1, height + 1, px(14.0), px(14.0)), 1);
    }
}

fn tray(message: u32, tip: &str) {
    let hs = h();
    let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hs.host;
    data.uID = 1;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = hs.icon_small;
    for (slot, unit) in data.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
        *slot = unit;
    }
    unsafe { Shell_NotifyIconW(message, &data) };
}

fn show_status() {
    let hs = h();
    MEM.with(|m| m.borrow_mut().status = None);
    unsafe {
        ShowWindow(hs.status, SW_SHOW);
        ShowWindow(hs.status, SW_RESTORE);
        SetForegroundWindow(hs.status);
        InvalidateRect(hs.status, null(), 0);
    }
}

fn tray_menu() {
    let hs = h();
    unsafe {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, MENU_STATUS, wide("Durum penceresi").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, MENU_QUIT, wide("Çıkış").as_ptr());
        SetMenuDefaultItem(menu, MENU_STATUS as u32, 0);
        let mut at: POINT = zeroed();
        GetCursorPos(&mut at);
        // Menü dışına tıklayınca kapanması için pencere önde olmalı.
        SetForegroundWindow(hs.host);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, at.x, at.y, 0, hs.host,
                                    null()) as usize;
        PostMessageW(hs.host, WM_NULL, 0, 0);
        DestroyMenu(menu);
        match chosen {
            MENU_STATUS => show_status(),
            MENU_QUIT => PostQuitMessage(0),
            _ => {}
        }
    }
}

/// 200 ms'de bir: değişen ne varsa (rozet, ipucu, durum penceresi) yeniden çiz.
fn refresh() {
    let Some(bridge) = BRIDGE.get() else { return };
    let hs = h();
    let snap = bridge.snapshot();
    let (phase, subtitle) = view::badge(&snap, Instant::now());
    // Dinlerken kayıt noktası ~0,65 sn'de bir yanıp söner; yalnızca o zaman yeniden çizilir.
    let pulse = phase == BadgePhase::Listening && (started().elapsed().as_millis() / 650).is_multiple_of(2);
    let badge = (phase, subtitle, pulse);
    let tip = view::tray_tip(&snap);
    let status_visible = unsafe { IsWindowVisible(hs.status) } != 0;
    let (badge_changed, tip_changed, status_changed) = MEM.with(|m| {
        let mut m = m.borrow_mut();
        let badge_changed = m.badge.as_ref() != Some(&badge);
        let tip_changed = m.tip != tip;
        let status_changed = status_visible && m.status.as_ref() != Some(&snap);
        m.badge = Some(badge);
        if tip_changed {
            m.tip = tip.clone();
        }
        if status_changed {
            m.status = Some(snap);
        }
        (badge_changed, tip_changed, status_changed)
    });
    if badge_changed {
        unsafe { InvalidateRect(hs.badge, null(), 0) };
        place_badge(true); // en üstte kalsın
    }
    if tip_changed {
        tray(NIM_MODIFY, &tip);
    }
    if status_changed {
        unsafe { InvalidateRect(hs.status, null(), 0) };
    }
}

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let action = |a: Action| {
        if let Some(tx) = ACTIONS.get() {
            let _ = tx.send(a);
        }
    };
    match msg {
        WM_HOTKEY => {
            match wparam {
                1 => action(Action::Toggle),
                2 => action(Action::ToggleEnter),
                3 => action(Action::Cancel),
                _ => {}
            }
            0
        }
        WM_TIMER if wparam == TIMER_REFRESH => {
            refresh();
            0
        }
        WM_TRAY => {
            match lparam as u32 {
                WM_RBUTTONUP | WM_CONTEXTMENU => tray_menu(),
                WM_LBUTTONDBLCLK => show_status(),
                _ => {}
            }
            0
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            place_badge(false);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ if H.with(|h| h.get()).is_some_and(|hs| msg == hs.taskbar_created && msg != 0) => {
            // Gezgin yeniden başladı: tepsi simgesini geri ekle.
            let tip = MEM.with(|m| m.borrow().tip.clone());
            tray(NIM_ADD, &tip);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Mikrofon simgesi: önce kalın ve sönük (parlama), sonra ince ve parlak.
unsafe fn draw_mic(dc: HDC, accent: u32, filled: bool) {
    let arc: Vec<(i32, i32)> = (0..=20)
        .map(|i| {
            let a = std::f32::consts::PI * i as f32 / 20.0;
            (px(26.0 - 10.0 * a.cos()), px(28.0 + 9.0 * a.sin()))
        })
        .collect();
    let capsule = RECT { left: px(21.0), top: px(12.0), right: px(31.0), bottom: px(33.0) };
    for (color, width) in [(gfx::mix(accent, palette::BG, 0.28), px(4.0)), (accent, px(2.0))] {
        gfx::round_rect(dc, &capsule, px(10.0), (filled && color == accent).then_some(accent), color, width);
        gfx::line(dc, &arc, color, width);
        gfx::line(dc, &[(px(26.0), px(37.0)), (px(26.0), px(44.0))], color, width);
        gfx::line(dc, &[(px(20.0), px(44.0)), (px(32.0), px(44.0))], color, width);
    }
}

fn paint_badge(hwnd: HWND, dc: HDC) {
    let hs = h();
    let (phase, subtitle, pulse) =
        MEM.with(|m| m.borrow().badge.clone()).unwrap_or((BadgePhase::Closed, String::new(), false));
    let style = phase.style();
    let accent = style.accent;
    unsafe {
        let mut rc: RECT = zeroed();
        GetClientRect(hwnd, &mut rc);
        gfx::buffered(dc, rc.right, rc.bottom, |dc| {
            gfx::gradient(dc, &rc, gfx::mix(accent, palette::BG, 0.13), palette::BG);
            gfx::neon_frame(dc, &rc, px(14.0), accent, palette::BG, px(1.0));
            draw_mic(dc, accent, phase == BadgePhase::Listening);
            gfx::line(dc, &[(px(48.0), px(14.0)), (px(48.0), rc.bottom - px(14.0))],
                      gfx::mix(accent, palette::BG, 0.3), px(1.0));
            let left = px(58.0);
            let title = RECT { left, top: px(8.0), right: rc.right - px(18.0), bottom: px(33.0) };
            let flags = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS;
            SetTextCharacterExtra(dc, px(1.0));
            gfx::glow_text(dc, hs.font_badge_title, style.title, &title, flags, accent, palette::BG, px(1.0));
            SetTextCharacterExtra(dc, 0);
            let sub = RECT { left, top: px(33.0), right: rc.right - px(10.0), bottom: px(50.0) };
            gfx::text(dc, hs.font_badge_sub, gfx::mix(accent, palette::MUTED, 0.35), &subtitle, &sub, flags);
            // Dinlerken yanıp sönen kayıt noktası.
            if phase == BadgePhase::Listening {
                let (x, y) = (rc.right - px(15.0), px(15.0));
                if pulse {
                    gfx::glow_dot(dc, x, y, px(3.0), accent, palette::BG);
                } else {
                    gfx::glow_dot(dc, x, y, px(3.0), gfx::mix(accent, palette::BG, 0.35), palette::BG);
                }
            }
        });
    }
}

unsafe extern "system" fn badge_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            paint_badge(hwnd, dc);
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Kart: koyu dolgu, ince çerçeve, sol üstte kısa neon şerit ve küçük başlık. İçerik yüksekliğini alır,
/// kartın toplam yüksekliğini döner.
unsafe fn card(dc: HDC, x: i32, y: i32, width: i32, content: i32, caption: &str, accent: u32) -> i32 {
    let hs = h();
    let pad = px(16.0);
    let total = pad + px(18.0) + content + pad;
    let r = RECT { left: x, top: y, right: x + width, bottom: y + total };
    gfx::round_rect(dc, &r, px(12.0), Some(palette::CARD), palette::LINE, px(1.0));
    let strip = [(x + px(16.0), y), (x + px(64.0), y)];
    gfx::line(dc, &strip, gfx::mix(accent, palette::CARD, 0.3), px(5.0));
    gfx::line(dc, &strip, accent, px(2.0));
    SetTextCharacterExtra(dc, px(2.0));
    let cap = RECT { left: x + pad, top: y + pad - px(4.0), right: x + width - pad, bottom: y + pad + px(16.0) };
    gfx::text(dc, hs.font_label, gfx::mix(accent, palette::MUTED, 0.55), caption, &cap, DT_LEFT | DT_SINGLELINE);
    SetTextCharacterExtra(dc, 0);
    total
}

fn paint_status(hwnd: HWND, dc: HDC) {
    let hs = h();
    let Some(bridge) = BRIDGE.get() else { return };
    let t = view::status_text(&bridge.snapshot());
    let accent = t.accent;
    unsafe {
        let mut rc: RECT = zeroed();
        GetClientRect(hwnd, &mut rc);
        gfx::buffered(dc, rc.right, rc.bottom, |dc| {
            gfx::gradient(dc, &rc, gfx::mix(accent, palette::BG_TOP, 0.08), palette::BG);
            let (x, width) = (px(24.0), rc.right - px(48.0));
            let wrap = DT_LEFT | DT_WORDBREAK;
            let mut y = px(20.0);

            SetTextCharacterExtra(dc, px(2.0));
            gfx::text(dc, hs.font_label, palette::MUTED, "CHATGPT DİKTE  //  DURUM",
                      &RECT { left: x, top: y, right: x + width, bottom: y + px(18.0) }, DT_LEFT | DT_SINGLELINE);
            SetTextCharacterExtra(dc, 0);
            y += px(26.0);

            gfx::glow_dot(dc, x + px(7.0), y + px(18.0), px(5.0), accent, palette::BG_TOP);
            let heading = RECT { left: x + px(26.0), top: y, right: x + width, bottom: y + px(36.0) };
            SetTextCharacterExtra(dc, px(1.0));
            gfx::glow_text(dc, hs.font_heading, t.heading, &heading, DT_LEFT | DT_VCENTER | DT_SINGLELINE, accent,
                           palette::BG_TOP, px(1.0));
            SetTextCharacterExtra(dc, 0);
            y += px(42.0);

            let hc = gfx::height(dc, hs.font_normal, t.connection, width);
            gfx::text(dc, hs.font_normal, palette::MUTED, t.connection,
                      &RECT { left: x, top: y, right: x + width, bottom: y + hc }, wrap);
            y += hc + px(18.0);

            // Son aktarım.
            let inner = width - px(32.0);
            let ix = x + px(16.0);
            let line_h = px(21.0);
            let h_transfer = gfx::height(dc, hs.font_bold, t.transfer, inner);
            let h_details = gfx::height(dc, hs.font_normal, &t.details, inner);
            let content = h_transfer + px(8.0) + line_h * t.checks.len() as i32 + px(8.0) + h_details;
            let used = card(dc, x, y, width, content, "SON AKTARIM", accent);
            let mut cy = y + px(16.0) + px(18.0);
            let transfer_color = if t.transfer_failed { palette::RED } else { palette::TEXT };
            gfx::text(dc, hs.font_bold, transfer_color, t.transfer,
                      &RECT { left: ix, top: cy, right: ix + inner, bottom: cy + h_transfer }, wrap);
            cy += h_transfer + px(8.0);
            for (ok, label) in &t.checks {
                let row = |left: i32| RECT { left, top: cy, right: ix + inner, bottom: cy + line_h };
                let mid = cy + line_h / 2;
                if *ok {
                    // Onay işareti çizgiyle: yazı tipinde ✓ olmasa da görünür.
                    let tick = [(ix + px(2.0), mid), (ix + px(6.0), mid + px(4.0)), (ix + px(13.0), mid - px(4.0))];
                    gfx::line(dc, &tick, gfx::mix(palette::TEAL, palette::CARD, 0.3), px(4.0));
                    gfx::line(dc, &tick, palette::TEAL, px(2.0));
                } else {
                    gfx::glow_dot(dc, ix + px(7.0), mid, px(2.0), palette::GRAY, palette::CARD);
                }
                gfx::text(dc, hs.font_normal, if *ok { palette::TEXT } else { palette::MUTED }, label,
                          &row(ix + px(20.0)), DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
                cy += line_h;
            }
            cy += px(8.0);
            gfx::text(dc, hs.font_normal, palette::MUTED, &t.details,
                      &RECT { left: ix, top: cy, right: ix + inner, bottom: cy + h_details }, wrap);
            y += used + px(16.0);

            // Kısayollar: tuş kapakları.
            let key_w = view::SHORTCUTS.iter().map(|(k, _)| gfx::width(dc, hs.font_mono, k)).max().unwrap_or(0)
                + px(18.0);
            let row_h = px(30.0);
            let h_note = gfx::height(dc, hs.font_normal, view::SHORTCUT_NOTE, inner);
            let content = row_h * view::SHORTCUTS.len() as i32 + px(6.0) + h_note;
            card(dc, x, y, width, content, "KISAYOLLAR", palette::TEAL);
            let mut cy = y + px(16.0) + px(18.0);
            for (key, what) in view::SHORTCUTS {
                let cap = RECT { left: ix, top: cy + px(2.0), right: ix + key_w, bottom: cy + row_h - px(4.0) };
                gfx::round_rect(dc, &cap, px(8.0), Some(gfx::mix(palette::TEAL, palette::CARD, 0.07)),
                                gfx::mix(palette::TEAL, palette::CARD, 0.45), px(1.0));
                gfx::text(dc, hs.font_mono, palette::TEAL, key, &cap, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                gfx::text(dc, hs.font_normal, palette::TEXT, what,
                          &RECT { left: ix + key_w + px(12.0), top: cy, right: ix + inner, bottom: cy + row_h },
                          DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                cy += row_h;
            }
            cy += px(6.0);
            gfx::text(dc, hs.font_normal, palette::MUTED, view::SHORTCUT_NOTE,
                      &RECT { left: ix, top: cy, right: ix + inner, bottom: cy + h_note }, wrap);
        });
    }
}

/// Koyu, neon çerçeveli düğme (sahibi çizer).
unsafe fn draw_button(item: &DRAWITEMSTRUCT) {
    let hs = h();
    let quit = item.CtlID as usize == BUTTON_QUIT;
    let accent = if quit { palette::RED } else { palette::TEAL };
    let pressed = item.itemState & ODS_SELECTED != 0;
    let focused = item.itemState & ODS_FOCUS != 0;
    let r = item.rcItem;
    gfx::buffered(item.hDC, r.right - r.left, r.bottom - r.top, |dc| {
        let local = RECT { left: 0, top: 0, right: r.right - r.left, bottom: r.bottom - r.top };
        gfx::fill(dc, &local, palette::BG);
        let fill = gfx::mix(accent, palette::BG, if pressed { 0.28 } else { 0.07 });
        let border = gfx::mix(accent, palette::BG, if focused { 0.95 } else { 0.55 });
        gfx::round_rect(dc, &local, px(10.0), Some(fill), border, px(1.0));
        let label = if quit { "Uygulamayı kapat" } else { "Gizle" };
        gfx::text(dc, hs.font_bold, gfx::mix(0xffffff, accent, 0.2), label, &local,
                  DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    });
}

unsafe extern "system" fn status_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Kapatmak yalnızca gizler; köprü çalışmaya devam eder.
        WM_CLOSE => {
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        WM_COMMAND => {
            match wparam & 0xffff {
                BUTTON_HIDE => {
                    ShowWindow(hwnd, SW_HIDE);
                }
                BUTTON_QUIT => PostQuitMessage(0),
                _ => {}
            }
            0
        }
        WM_DRAWITEM => {
            draw_button(&*(lparam as *const DRAWITEMSTRUCT));
            1
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            paint_status(hwnd, dc);
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Windows 10/11: koyu başlık çubuğu; Windows 11'de başlık rengi ve neon kenarlık da.
unsafe fn dark_title_bar(hwnd: HWND) {
    let set = |attr: DWMWINDOWATTRIBUTE, value: u32| {
        let attr = attr as u32;
        DwmSetWindowAttribute(hwnd, attr, &value as *const u32 as *const _, size_of::<u32>() as u32);
    };
    set(DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
    set(DWMWA_CAPTION_COLOR, gfx::rgb(palette::BG_TOP));
    set(DWMWA_BORDER_COLOR, gfx::rgb(gfx::mix(palette::TEAL, palette::BG, 0.6)));
}

fn register_hotkeys(host: HWND) -> Vec<&'static str> {
    let mods: HOT_KEY_MODIFIERS = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
    let keys = [(1, VK_SPACE, "Ctrl+Alt+Space"), (2, VK_RETURN, "Ctrl+Alt+Enter"), (3, VK_BACK, "Ctrl+Alt+Backspace")];
    keys.iter()
        .filter(|(id, vk, _)| unsafe { RegisterHotKey(host, *id, mods, *vk as u32) } == 0)
        .map(|(_, _, name)| *name)
        .collect()
}

/// Pencereleri kurar ve ileti döngüsünü çalıştırır; Çıkış'a kadar döner.
pub fn run(bridge: Arc<Bridge<WinPlatform>>, actions: Sender<Action>) -> Result<(), String> {
    let _ = BRIDGE.set(bridge);
    let _ = ACTIONS.set(actions);
    unsafe {
        let instance = GetModuleHandleW(null()) as HINSTANCE;
        let screen = GetDC(null_mut());
        let dpi = GetDeviceCaps(screen, LOGPIXELSY as _).max(96);
        ReleaseDC(null_mut(), screen);
        let icon_big = load_icon(instance, GetSystemMetrics(SM_CXICON));
        let icon_small = load_icon(instance, GetSystemMetrics(SM_CXSMICON));
        let host_class = wide("ChatGPTDikteHost");
        let badge_class = wide("ChatGPTDikteBadge");
        let status_class = wide("ChatGPTDikteStatus");
        register_class(instance, &host_class, Some(host_proc), icon_big, icon_small);
        register_class(instance, &badge_class, Some(badge_proc), null_mut(), null_mut());
        register_class(instance, &status_class, Some(status_proc), icon_big, icon_small);

        // Gizli üst düzey pencere (yalnızca iletiler için değil: "TaskbarCreated" yayınını da alır).
        let host = CreateWindowExW(0, host_class.as_ptr(), wide(APP_NAME).as_ptr(), WS_OVERLAPPED, 0, 0, 0, 0,
                                   null_mut(), null_mut(), instance, null());
        let badge = CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST, badge_class.as_ptr(),
                                    wide("Dikte").as_ptr(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), instance,
                                    null());
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let scale = dpi as f32 / 96.0;
        let mut outer = RECT { left: 0, top: 0, right: (460.0 * scale) as i32, bottom: (584.0 * scale) as i32 };
        AdjustWindowRectEx(&mut outer, style, 0, 0);
        let status = CreateWindowExW(0, status_class.as_ptr(), wide("ChatGPT Dikte — Durum").as_ptr(), style,
                                     CW_USEDEFAULT, CW_USEDEFAULT, outer.right - outer.left, outer.bottom - outer.top,
                                     null_mut(), null_mut(), instance, null());
        if host.is_null() || badge.is_null() || status.is_null() {
            return Err("Pencereler oluşturulamadı.".into());
        }
        H.with(|h| {
            h.set(Some(Handles {
                host,
                badge,
                status,
                scale,
                icon_small,
                font_badge_title: font("Bahnschrift SemiBold", 13, 400, dpi),
                font_badge_sub: font("Segoe UI", 8, 400, dpi),
                font_heading: font("Bahnschrift SemiBold", 22, 400, dpi),
                font_label: font("Bahnschrift", 8, 400, dpi),
                font_bold: font("Segoe UI Semibold", 10, 600, dpi),
                font_normal: font("Segoe UI", 10, 400, dpi),
                font_mono: font("Consolas", 10, 400, dpi),
                taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            }))
        });
        // Durum penceresi düğmeleri.
        let mut client: RECT = zeroed();
        GetClientRect(status, &mut client);
        let button_y = client.bottom - px(24.0) - px(36.0);
        for (id, label, x, width) in [
            (BUTTON_HIDE, "Gizle", px(24.0), px(110.0)),
            (BUTTON_QUIT, "Uygulamayı kapat", client.right - px(24.0) - px(170.0), px(170.0)),
        ] {
            CreateWindowExW(0, wide("BUTTON").as_ptr(), wide(label).as_ptr(),
                            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, x, button_y, width, px(36.0),
                            status, id as _, instance, null());
        }
        dark_title_bar(status);

        let failed = register_hotkeys(host);
        if !failed.is_empty() {
            let line = format!("Kısayol kaydedilemedi (başka bir uygulama kullanıyor): {}", failed.join(", "));
            BRIDGE.get().unwrap().platform_log(&line);
            message_box(&line, true);
        }

        refresh();
        place_badge(true);
        let tip = MEM.with(|m| m.borrow().tip.clone());
        tray(NIM_ADD, &tip);
        SetTimer(host, TIMER_REFRESH, 200, None);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            let in_status = msg.hwnd == status || IsChild(status, msg.hwnd) != 0;
            if msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE as usize && in_status {
                ShowWindow(status, SW_HIDE);
                continue;
            }
            if in_status && IsDialogMessageW(status, &msg) != 0 {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        tray(NIM_DELETE, "");
    }
    Ok(())
}

trait LogExt {
    fn platform_log(&self, line: &str);
}

impl LogExt for Bridge<WinPlatform> {
    fn platform_log(&self, line: &str) {
        use crate::bridge::Platform;
        self.platform.log(line);
    }
}
