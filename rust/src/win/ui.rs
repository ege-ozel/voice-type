//! Arayüz (tek iş parçacığı): gizli ana pencere (kısayollar, tepsi simgesi, zamanlayıcı),
//! sağ alttaki odak çalmayan rozet ve durum penceresi. Çizim düz GDI ile yapılır.
use std::cell::{Cell, RefCell};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use windows_sys::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_BACK, VK_ESCAPE, VK_RETURN, VK_SPACE,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::platform::WinPlatform;
use crate::bridge::Bridge;
use crate::view::{self, BadgePhase};

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
const STATUS_BG: u32 = 0xf4f7f5;

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
    font_bold: HFONT,
    font_normal: HFONT,
    taskbar_created: u32,
}

#[derive(Default)]
struct Memory {
    badge: Option<(BadgePhase, String)>,
    tip: String,
    status: Option<crate::bridge::Snapshot>,
}

thread_local! {
    static H: Cell<Option<Handles>> = const { Cell::new(None) };
    static MEM: RefCell<Memory> = RefCell::new(Memory::default());
}

fn h() -> Handles {
    H.with(|h| h.get()).expect("arayüz hazır değil")
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn rgb(c: u32) -> COLORREF {
    ((c & 0xff) << 16) | (c & 0xff00) | ((c >> 16) & 0xff)
}

fn px(v: f32) -> i32 {
    (v * h().scale).round() as i32
}

pub fn message_box(text: &str, error: bool) {
    let flags = if error { MB_ICONERROR } else { MB_ICONINFORMATION };
    unsafe { MessageBoxW(null_mut(), wide(text).as_ptr(), wide(APP_NAME).as_ptr(), flags | MB_SETFOREGROUND) };
}

fn font(points: i32, bold: bool, dpi: i32) -> HFONT {
    unsafe {
        CreateFontW(-(points * dpi + 36) / 72, 0, 0, 0, if bold { FW_BOLD as i32 } else { FW_NORMAL as i32 }, 0, 0, 0,
                    DEFAULT_CHARSET as u32, OUT_DEFAULT_PRECIS as u32, CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32, 0,
                    wide("Segoe UI").as_ptr())
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
    ((area.right - px(194.0)).max(area.left), (area.bottom - px(84.0)).max(area.top))
}

fn place_badge(show: bool) {
    let hs = h();
    let (x, y) = badge_position();
    let mut flags = SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    if show {
        flags |= SWP_SHOWWINDOW;
    }
    unsafe { SetWindowPos(hs.badge, HWND_TOPMOST, x, y, px(174.0), px(64.0), flags) };
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
    let badge = view::badge(&snap, Instant::now());
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

fn paint_badge(hwnd: HWND, dc: HDC) {
    let hs = h();
    let (phase, subtitle) = MEM.with(|m| m.borrow().badge.clone()).unwrap_or((BadgePhase::Closed, String::new()));
    let style = phase.style();
    let (bg, ink) = (rgb(style.color), rgb(style.ink));
    unsafe {
        let mut rc: RECT = zeroed();
        GetClientRect(hwnd, &mut rc);
        let brush = CreateSolidBrush(bg);
        FillRect(dc, &rc, brush);
        DeleteObject(brush);

        // Mikrofon simgesi (clipboard_badge.py ile aynı koordinatlar).
        let pen = CreatePen(PS_SOLID, px(2.0).max(1), ink);
        let old_pen = SelectObject(dc, pen);
        let fill = if phase == BadgePhase::Listening { CreateSolidBrush(ink) } else { GetStockObject(NULL_BRUSH) };
        let old_brush = SelectObject(dc, fill);
        Rectangle(dc, px(16.0), px(12.0), px(28.0), px(34.0));
        SelectObject(dc, old_brush);
        if phase == BadgePhase::Listening {
            DeleteObject(fill);
        }
        // Alt yarım elips (10,20)-(34,42).
        let points: Vec<POINT> = (0..=24)
            .map(|i| {
                let a = std::f32::consts::PI * i as f32 / 24.0;
                POINT { x: px(22.0 - 12.0 * a.cos()), y: px(31.0 + 11.0 * a.sin()) }
            })
            .collect();
        Polyline(dc, points.as_ptr(), points.len() as i32);
        MoveToEx(dc, px(22.0), px(42.0), null_mut());
        LineTo(dc, px(22.0), px(50.0));
        MoveToEx(dc, px(15.0), px(50.0), null_mut());
        LineTo(dc, px(29.0), px(50.0));
        SelectObject(dc, old_pen);
        DeleteObject(pen);

        SetBkMode(dc, TRANSPARENT as _);
        SetTextColor(dc, ink);
        let line = |text: &str, f: HFONT, top: f32, bottom: f32| {
            let old = SelectObject(dc, f);
            let mut r = RECT { left: px(38.0), top: px(top), right: px(170.0), bottom: px(bottom) };
            let w: Vec<u16> = text.encode_utf16().collect();
            DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut r,
                      DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
            SelectObject(dc, old);
        };
        line(style.title, hs.font_badge_title, 10.0, 36.0);
        line(&subtitle, hs.font_badge_sub, 35.0, 53.0);
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

/// Sarılı metin çizer, kullandığı yüksekliği döner.
unsafe fn draw_block(dc: HDC, f: HFONT, color: u32, text: &str, x: i32, y: i32, width: i32) -> i32 {
    let old = SelectObject(dc, f);
    SetTextColor(dc, rgb(color));
    let w: Vec<u16> = text.encode_utf16().collect();
    let flags = DT_LEFT | DT_WORDBREAK | DT_NOPREFIX;
    let mut r = RECT { left: x, top: y, right: x + width, bottom: y };
    DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut r, flags | DT_CALCRECT);
    let height = r.bottom - r.top;
    r.right = x + width;
    DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut r, flags);
    SelectObject(dc, old);
    height
}

fn paint_status(hwnd: HWND, dc: HDC) {
    let hs = h();
    let Some(bridge) = BRIDGE.get() else { return };
    let t = view::status_text(&bridge.snapshot());
    unsafe {
        let mut rc: RECT = zeroed();
        GetClientRect(hwnd, &mut rc);
        let (w, height) = (rc.right, rc.bottom);
        // Titremesin diye önce bellekte çiz.
        let mem = CreateCompatibleDC(dc);
        let bmp = CreateCompatibleBitmap(dc, w, height);
        let old_bmp = SelectObject(mem, bmp);
        let brush = CreateSolidBrush(rgb(STATUS_BG));
        FillRect(mem, &rc, brush);
        DeleteObject(brush);
        SetBkMode(mem, TRANSPARENT as _);
        let (x, width, gap) = (px(20.0), w - px(40.0), px(10.0));
        let mut y = px(16.0);
        let ink = 0x233a2d;
        y += draw_block(mem, hs.font_heading, t.heading_color, &t.heading, x, y, width) + gap;
        y += draw_block(mem, hs.font_normal, ink, t.connection, x, y, width) + gap;
        y += draw_block(mem, hs.font_bold, t.transfer_color, t.transfer, x, y, width) + gap;
        y += draw_block(mem, hs.font_normal, ink, &t.checks, x, y, width) + gap;
        y += draw_block(mem, hs.font_normal, ink, &t.details, x, y, width) + gap + px(4.0);
        draw_block(mem, hs.font_normal, 0x58645d, view::HELP, x, y, width);
        BitBlt(dc, 0, 0, w, height, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old_bmp);
        DeleteObject(bmp);
        DeleteDC(mem);
    }
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
        WM_ERASEBKGND => 1,
        WM_CTLCOLORBTN => {
            static BG: OnceLock<usize> = OnceLock::new();
            *BG.get_or_init(|| CreateSolidBrush(rgb(STATUS_BG)) as usize) as LRESULT
        }
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
        let mut outer = RECT { left: 0, top: 0, right: (440.0 * scale) as i32, bottom: (450.0 * scale) as i32 };
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
                font_badge_title: font(12, true, dpi),
                font_badge_sub: font(8, false, dpi),
                font_heading: font(16, true, dpi),
                font_bold: font(12, true, dpi),
                font_normal: font(10, false, dpi),
                taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            }))
        });
        let hs = h();

        // Durum penceresi düğmeleri.
        let mut client: RECT = zeroed();
        GetClientRect(status, &mut client);
        let button_y = client.bottom - px(16.0) - px(30.0);
        for (id, label, x, width) in [
            (BUTTON_HIDE, "Gizle", px(20.0), px(90.0)),
            (BUTTON_QUIT, "Uygulamayı kapat", client.right - px(20.0) - px(150.0), px(150.0)),
        ] {
            let button = CreateWindowExW(0, wide("BUTTON").as_ptr(), wide(label).as_ptr(),
                                         WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32, x, button_y,
                                         width, px(30.0), status, id as _, instance, null());
            SendMessageW(button, WM_SETFONT, hs.font_normal as WPARAM, 1);
        }

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
