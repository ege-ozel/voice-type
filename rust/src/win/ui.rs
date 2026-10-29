//! Arayüz (tek iş parçacığı): gizli ana pencere (kısayollar, tepsi simgesi, zamanlayıcı),
//! sağ alttaki odak çalmayan rozet ve durum penceresi. Çizim düz GDI ile yapılır.
use std::cell::{Cell, RefCell};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED, WM_MOUSELEAVE};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
    MOD_WIN, VK_CONTROL, VK_ESCAPE, VK_F10, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE, SetCapture, ReleaseCapture,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, DefSubclassProc, SetWindowSubclass, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::gfx;
use super::platform::WinPlatform;
use crate::bridge::{Bridge, Engine};
use crate::dock::{self, Edge};
use crate::hotkey::{self, Hotkey, Hotkeys, Slot, SLOTS};
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
const MENU_PASTE: usize = 102;
const MENU_COPY: usize = 103;
const MENU_BROWSER: usize = 104;
const MENU_API: usize = 105;
/// + Slot::index()
const MENU_KEY: usize = 110;
const MENU_KEYS_DEFAULT: usize = 113;
const BUTTON_HIDE: usize = 201;
const BUTTON_QUIT: usize = 202;
const TIMER_ANIM: usize = 2;
/// Kare rozetin kenarı (genişlemiş hâlin yüksekliği de bu).
const BADGE_SIDE: f32 = 60.0;
/// Rozetle ekran kenarı arasındaki boşluk (DeskTuk'taki gibi).
const BADGE_GAP: f32 = 12.0;
/// Genişleme, daralma ve kenara oturma süresi.
const ANIM_MS: f32 = 190.0;

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
    font_key: HFONT,
    taskbar_created: u32,
}

#[derive(Default)]
struct Memory {
    /// (aşama, alt yazı, kayıt noktası yanık mı)
    badge: Option<(BadgePhase, String, bool)>,
    tip: String,
    status: Option<crate::bridge::Snapshot>,
    /// Yeni birleşimi beklenen kısayol (o sırada genel kısayollar kayıtlı değildir).
    capture: Option<Slot>,
    capture_pending: Option<Hotkey>,
    capture_preview: String,
    /// Son kısayol değiştirme hatası (durum penceresinde kırmızı).
    capture_error: String,
    /// Durum penceresindeki kısayol satırları (tıklama için).
    rows: Vec<(Slot, RECT)>,
    hover_row: Option<Slot>,
    hover_button: Option<usize>,
}

/// Rozetin yeri, geçişi ve fare etkileşimi.
#[derive(Default)]
struct BadgeUi {
    /// Kullanıcının yapıştırdığı kenar ve karenin merkezi (yoksa sağ alt köşe).
    anchor: Option<(Edge, (i32, i32))>,
    /// Ekrandaki dikdörtgen (x, y, genişlik); yükseklik hep BADGE_SIDE.
    shown: (i32, i32, i32),
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    anim_start: Option<Instant>,
    /// Rozet sola doğru mu açılıyor (kare sağda kalır).
    grow_left: bool,
    /// Son genişlemiş hâlin genişliği; daralırken metin yerinde kalsın diye korunur.
    full_w: i32,
    hover: bool,
    press: Option<Press>,
}

#[derive(Clone, Copy)]
struct Press {
    at: POINT,
    /// İmlecin karenin sol üst köşesine göre yeri.
    offset: (i32, i32),
    dragging: bool,
}

thread_local! {
    static H: Cell<Option<Handles>> = const { Cell::new(None) };
    static MEM: RefCell<Memory> = RefCell::new(Memory::default());
    static BADGE: RefCell<BadgeUi> = RefCell::new(BadgeUi::default());
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

fn bounds(r: RECT) -> dock::Bounds {
    (r.left, r.top, r.right, r.bottom)
}

/// Ana ekranın çalışma alanı (görev çubuğu hariç).
fn primary_work_area() -> dock::Bounds {
    let mut area: RECT = unsafe { zeroed() };
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut area as *mut RECT as *mut _, 0) } != 0;
    if !ok {
        area.right = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        area.bottom = unsafe { GetSystemMetrics(SM_CYSCREEN) } - px(48.0);
    }
    bounds(area)
}

/// Noktaya en yakın ekranın çalışma alanı.
fn work_area_at((x, y): (i32, i32)) -> dock::Bounds {
    unsafe {
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST), &mut info) != 0 {
            return bounds(info.rcWork);
        }
    }
    primary_work_area()
}

/// Kare rozetin sol üst köşesi ve bulunduğu ekranın çalışma alanı.
fn badge_square() -> ((i32, i32), dock::Bounds) {
    let anchor = BADGE.with(|b| b.borrow().anchor);
    let (edge, at) = anchor.unwrap_or_else(|| {
        let work = primary_work_area();
        (Edge::Right, (work.2, work.3))
    });
    let work = work_area_at(at);
    (dock::snap(work, edge, at, px(BADGE_SIDE), px(BADGE_GAP)), work)
}

fn load_badge_anchor() {
    let settings = super::read_settings(&super::app_dir());
    let anchor = settings.get("badge").and_then(|v| {
        let edge = Edge::parse(v.get("edge")?.as_str()?)?;
        let x = i32::try_from(v.get("x")?.as_i64()?).ok()?;
        let y = i32::try_from(v.get("y")?.as_i64()?).ok()?;
        Some((edge, (x, y)))
    });
    BADGE.with(|b| b.borrow_mut().anchor = anchor);
}

fn save_badge_anchor(edge: Edge, at: (i32, i32)) {
    let value = serde_json::json!({ "edge": edge.name(), "x": at.0, "y": at.1 });
    if let Err(error) = super::save_setting(&super::app_dir(), "badge", value) {
        if let Some(bridge) = BRIDGE.get() {
            bridge.platform_log(&format!("Rozet konumu kaydedilemedi: {error}"));
        }
    }
}

/// Genişlemiş rozetin genişliği: kare + başlık ya da alt yazıdan uzun olanı.
fn badge_full_width(title: &str, subtitle: &str) -> i32 {
    let hs = h();
    unsafe {
        let dc = GetDC(hs.badge);
        let title_w = gfx::width(dc, hs.font_badge_title, title) + px(1.0) * title.chars().count() as i32;
        let sub_w = gfx::width(dc, hs.font_badge_sub, subtitle);
        ReleaseDC(hs.badge, dc);
        (px(BADGE_SIDE) + px(12.0) + title_w.max(sub_w) + px(16.0)).clamp(px(150.0), px(400.0))
    }
}

/// Rozet bu durumlarda kendiliğinden açılır; geri kalanında yalnızca fare üstündeyken.
fn opens_by_itself(phase: BadgePhase) -> bool {
    matches!(phase, BadgePhase::Copied | BadgePhase::Clipboard | BadgePhase::Error | BadgePhase::Permission)
}

/// Rozeti olması gereken yere ve boyuta götürür; `animate` ise yumuşak geçişle.
fn sync_badge(animate: bool) {
    let hs = h();
    let (phase, subtitle) = MEM.with(|m| m.borrow().badge.as_ref().map(|b| (b.0, b.1.clone())))
        .unwrap_or((BadgePhase::Closed, String::new()));
    let (hover, dragging) = BADGE.with(|b| {
        let b = b.borrow();
        (b.hover, b.press.is_some_and(|p| p.dragging))
    });
    if dragging {
        return;
    }
    let ((sx, sy), work) = badge_square();
    let side = px(BADGE_SIDE);
    let open = hover || opens_by_itself(phase);
    let full = badge_full_width(phase.style().title, &subtitle);
    let (ex, grow_left) = dock::expand(work, (sx, sy), side, full);
    let target = if open { (ex, sy, full) } else { (sx, sy, side) };
    let start = BADGE.with(|b| {
        let mut b = b.borrow_mut();
        b.grow_left = grow_left;
        if open {
            b.full_w = full;
        }
        let settled = b.anim_start.is_some() || b.shown == target;
        if b.to == target && settled {
            return None;
        }
        b.to = target;
        if animate && b.shown.2 > 0 {
            b.from = b.shown;
            b.anim_start = Some(Instant::now());
            Some(true)
        } else {
            b.shown = target;
            b.anim_start = None;
            Some(false)
        }
    });
    match start {
        Some(true) => unsafe {
            SetTimer(hs.badge, TIMER_ANIM, 10, None);
        },
        Some(false) => apply_badge_rect(),
        None => {}
    }
}

/// `shown` dikdörtgenini pencereye uygular: konum, boyut ve içerik tek adımda değişir.
fn apply_badge_rect() {
    render_badge();
    let badge = h().badge;
    if unsafe { IsWindowVisible(badge) } == 0 {
        unsafe { ShowWindow(badge, SW_SHOWNOACTIVATE) };
    }
}

/// Geçişin bir karesi; bitince zamanlayıcı durur.
fn badge_anim_tick() {
    let done = BADGE.with(|b| {
        let mut b = b.borrow_mut();
        let Some(start) = b.anim_start else { return true };
        let t = start.elapsed().as_secs_f32() * 1000.0 / ANIM_MS;
        let e = dock::ease_out(t);
        let (from, to) = (b.from, b.to);
        b.shown = (dock::lerp(from.0, to.0, e), dock::lerp(from.1, to.1, e), dock::lerp(from.2, to.2, e));
        if t >= 1.0 {
            b.shown = to;
            b.anim_start = None;
        }
        b.anim_start.is_none()
    });
    apply_badge_rect();
    if done {
        unsafe { KillTimer(h().badge, TIMER_ANIM) };
    }
}

/// Rozet her zaman en üstte kalsın (başka bir "hep üstte" pencere öne geçmiş olabilir).
fn raise_badge() {
    unsafe {
        SetWindowPos(h().badge, HWND_TOPMOST, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW);
    }
}

fn cursor() -> POINT {
    let mut at: POINT = unsafe { zeroed() };
    unsafe { GetCursorPos(&mut at) };
    at
}

fn badge_press(hwnd: HWND) {
    let at = cursor();
    let side = px(BADGE_SIDE);
    BADGE.with(|b| {
        let mut b = b.borrow_mut();
        let (x, y, w) = b.shown;
        let square_x = if b.grow_left { x + w - side } else { x };
        let offset = ((at.x - square_x).clamp(0, side), (at.y - y).clamp(0, side));
        b.press = Some(Press { at, offset, dragging: false });
    });
    unsafe { SetCapture(hwnd) };
}

enum Moved {
    Nothing,
    Hover,
    Drag,
}

fn badge_mouse_move(hwnd: HWND) {
    let at = cursor();
    let side = px(BADGE_SIDE);
    let (dx, dy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
    let moved = BADGE.with(|b| {
        let mut b = b.borrow_mut();
        let Some(mut press) = b.press else {
            if b.hover {
                return Moved::Nothing;
            }
            b.hover = true;
            return Moved::Hover;
        };
        if !press.dragging {
            if (at.x - press.at.x).abs() <= dx && (at.y - press.at.y).abs() <= dy {
                return Moved::Nothing;
            }
            // Sürükleme başladı: rozet kareye döner, imleç karenin üstünde kalır.
            press.dragging = true;
            b.hover = false;
            b.anim_start = None;
            b.press = Some(press);
        }
        let work = work_area_at((at.x, at.y));
        let (x, y) = dock::clamp_inside(work, (at.x - press.offset.0, at.y - press.offset.1), side);
        b.shown = (x, y, side);
        b.to = b.shown;
        Moved::Drag
    });
    match moved {
        Moved::Nothing => {}
        Moved::Hover => {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe { TrackMouseEvent(&mut track) };
            sync_badge(true);
        }
        Moved::Drag => {
            unsafe { KillTimer(hwnd, TIMER_ANIM) };
            apply_badge_rect();
        }
    }
}

/// Bırakma: sürükleniyorsa en yakın kenara oturur ve yer kaydedilir; kısa tık durum penceresini açar.
/// `click` yalnızca gerçek düğme bırakmada doğrudur (fare yakalaması başka yolla kaybolunca yanlış).
fn badge_release(click: bool) {
    let Some(press) = BADGE.with(|b| b.borrow_mut().press.take()) else { return };
    if click {
        unsafe { ReleaseCapture() };
    }
    if !press.dragging {
        if click {
            show_status();
        }
        return;
    }
    let side = px(BADGE_SIDE);
    let (x, y, _) = BADGE.with(|b| b.borrow().shown);
    let center = (x + side / 2, y + side / 2);
    let work = work_area_at(center);
    let edge = dock::nearest_edge(work, center);
    let (sx, sy) = dock::snap(work, edge, center, side, px(BADGE_GAP));
    let at = (sx + side / 2, sy + side / 2);
    BADGE.with(|b| b.borrow_mut().anchor = Some((edge, at)));
    save_badge_anchor(edge, at);
    sync_badge(true);
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
        let snap = BRIDGE.get().map(|b| b.snapshot());
        let copy_only = snap.as_ref().is_some_and(|s| s.copy_only);
        let api = snap.as_ref().is_some_and(|s| s.engine == Engine::Api);
        AppendMenuW(menu, MF_STRING, MENU_BROWSER, wide("Dikte: ChatGPT sekmesi (ücretsiz)").as_ptr());
        AppendMenuW(menu, MF_STRING, MENU_API, wide("Dikte: OpenAI API (ücretli)").as_ptr());
        let engine = if api { MENU_API } else { MENU_BROWSER };
        CheckMenuRadioItem(menu, MENU_BROWSER as u32, MENU_API as u32, engine as u32, MF_BYCOMMAND);
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, MENU_PASTE, wide("Bitirince yapıştır").as_ptr());
        AppendMenuW(menu, MF_STRING, MENU_COPY, wide("Bitirince yalnızca panoya kopyala").as_ptr());
        let checked = if copy_only { MENU_COPY } else { MENU_PASTE };
        CheckMenuRadioItem(menu, MENU_PASTE as u32, MENU_COPY as u32, checked as u32, MF_BYCOMMAND);
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        // Kısayollar: bir satır seçilince durum penceresinde yeni birleşim beklenir.
        let keys_menu = CreatePopupMenu();
        let hotkeys = snap.as_ref().map(|s| s.hotkeys).unwrap_or_default();
        for slot in SLOTS {
            let label = format!("{}…\t{}", slot.label(), hotkeys.get(slot));
            AppendMenuW(keys_menu, MF_STRING, MENU_KEY + slot.index(), wide(&label).as_ptr());
        }
        AppendMenuW(keys_menu, MF_SEPARATOR, 0, null());
        let default_flags = if hotkeys == Hotkeys::default() { MF_STRING | MF_GRAYED } else { MF_STRING };
        AppendMenuW(keys_menu, default_flags, MENU_KEYS_DEFAULT, wide("Varsayılanlara dön").as_ptr());
        AppendMenuW(menu, MF_POPUP, keys_menu as usize, wide("Kısayolları değiştir").as_ptr());
        AppendMenuW(menu, MF_STRING, MENU_STATUS, wide("Durum penceresi").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, MENU_QUIT, wide("Çıkış").as_ptr());
        SetMenuDefaultItem(menu, MENU_STATUS as u32, 0);
        let mut at: POINT = zeroed();
        GetCursorPos(&mut at);
        // Menü dışına tıklayınca kapanması için pencere önde olmalı.
        // Rozet topmost kaldığında yerel açılır menünün üstünü örtebiliyor.
        SetWindowPos(hs.badge, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        SetForegroundWindow(hs.host);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, at.x, at.y, 0, hs.host,
                                    null()) as usize;
        PostMessageW(hs.host, WM_NULL, 0, 0);
        DestroyMenu(menu);
        SetWindowPos(hs.badge, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        match chosen {
            MENU_PASTE | MENU_COPY => set_copy_only(chosen == MENU_COPY),
            MENU_BROWSER | MENU_API => set_engine(if chosen == MENU_API { Engine::Api } else { Engine::Browser }),
            MENU_STATUS => show_status(),
            MENU_QUIT => PostQuitMessage(0),
            MENU_KEYS_DEFAULT => {
                let result = apply_hotkeys(Hotkeys::default());
                show_status();
                MEM.with(|m| m.borrow_mut().capture_error = result.err().unwrap_or_default());
            }
            n if (MENU_KEY..MENU_KEY + SLOTS.len()).contains(&n) => begin_capture(SLOTS[n - MENU_KEY]),
            _ => {}
        }
    }
}

// --- kısayollar ------------------------------------------------------------------

fn modifiers(key: Hotkey) -> HOT_KEY_MODIFIERS {
    [(hotkey::CTRL, MOD_CONTROL), (hotkey::ALT, MOD_ALT), (hotkey::SHIFT, MOD_SHIFT), (hotkey::WIN, MOD_WIN)]
        .iter()
        .filter(|(m, _)| key.mods & m != 0)
        .fold(MOD_NOREPEAT, |acc, (_, flag)| acc | flag)
}

fn unregister_hotkeys() {
    let host = h().host;
    for slot in SLOTS {
        unsafe { UnregisterHotKey(host, slot.index() as i32 + 1) };
    }
}

/// Önce hepsini bırakıp yeniden kaydeder (kimlik = Slot::index() + 1); kaydedilemeyenlerin adları.
fn register_hotkeys(keys: &Hotkeys) -> Vec<String> {
    unregister_hotkeys();
    let host = h().host;
    SLOTS
        .iter()
        .map(|&slot| keys.get(slot))
        .enumerate()
        .filter(|(i, key)| unsafe { RegisterHotKey(host, *i as i32 + 1, modifiers(*key), key.vk as u32) } == 0)
        .map(|(_, key)| key.to_string())
        .collect()
}

/// Yeni kısayolları kaydeder ve saklar; biri başka uygulamadaysa eskilerine döner.
fn apply_hotkeys(keys: Hotkeys) -> Result<(), String> {
    let Some(bridge) = BRIDGE.get() else { return Ok(()) };
    let old = bridge.snapshot().hotkeys;
    let failed = register_hotkeys(&keys);
    if !failed.is_empty() {
        register_hotkeys(&old);
        return Err(format!("{} başka bir uygulama tarafından kullanılıyor; başka bir birleşim deneyin.",
                           failed.join(", ")));
    }
    if let Err(error) = super::save_hotkeys(&super::app_dir(), &keys) {
        register_hotkeys(&old);
        bridge.platform_log(&format!("Ayar kaydedilemedi: {error}"));
        return Err("Kısayollar kaydedilemedi; eski kısayollar korundu. Ayar klasörünün yazılabilir olduğunu kontrol edin.".into());
    }
    bridge.set_hotkeys(keys);
    Ok(())
}

/// Durum penceresinde `slot` satırı yeni birleşimi bekler. Genel kısayollar bu sürede bırakılır;
/// yoksa ör. Ctrl+Alt+Space'e basmak birleşimi yakalamak yerine dikteyi başlatırdı.
fn begin_capture(slot: Slot) {
    show_status();
    unregister_hotkeys();
    MEM.with(|m| {
        let mut m = m.borrow_mut();
        m.capture = Some(slot);
        m.capture_pending = None;
        m.capture_preview.clear();
        m.capture_error.clear();
    });
    unsafe { InvalidateRect(h().status, null(), 0) };
}

/// Yakalamayı bırakır ve geçerli kısayolları yeniden kaydeder.
fn end_capture() {
    if MEM.with(|m| m.borrow_mut().capture.take()).is_none() {
        return;
    }
    if let Some(bridge) = BRIDGE.get() {
        let failed = register_hotkeys(&bridge.snapshot().hotkeys);
        if !failed.is_empty() {
            let line = format!("Kısayol kaydedilemedi (başka bir uygulama kullanıyor): {}", failed.join(", "));
            bridge.platform_log(&line);
            MEM.with(|m| m.borrow_mut().capture_error = line);
        }
    }
    unsafe { InvalidateRect(h().status, null(), 0) };
}

/// Yakalama sırasında basılan tuş. Değiştirici tuşlar tek başına beklenir; Esc (yalnız) vazgeçer.
fn capture_key(vk: u16, pressed: bool) {
    let Some(slot) = MEM.with(|m| m.borrow().capture) else { return };
    let down = |key: u16| unsafe { GetKeyState(key as i32) } < 0;
    let mods = [(VK_CONTROL, hotkey::CTRL), (VK_MENU, hotkey::ALT), (VK_SHIFT, hotkey::SHIFT)]
        .iter()
        .filter(|(key, _)| down(*key))
        .fold(if down(VK_LWIN) || down(VK_RWIN) { hotkey::WIN } else { 0 }, |acc, (_, m)| acc | m);
    if pressed && vk == VK_ESCAPE && mods == 0 {
        end_capture();
        return;
    }
    let pending = MEM.with(|m| {
        let mut m = m.borrow_mut();
        if pressed && !hotkey::is_modifier(vk) {
            m.capture_pending = Some(Hotkey::new(mods, vk));
        }
        m.capture_preview = if let Some(key) = m.capture_pending {
            key.to_string()
        } else {
            [(hotkey::CTRL, "Ctrl"), (hotkey::ALT, "Alt"), (hotkey::SHIFT, "Shift"), (hotkey::WIN, "Win")]
                .iter().filter(|(flag, _)| mods & flag != 0).map(|(_, name)| *name).collect::<Vec<_>>().join(" + ")
        };
        m.capture_pending
    });
    unsafe { InvalidateRect(h().status, null(), 0) };
    // Gösterilen birleşimi bütün tuşlar bırakıldığında uygula.
    let Some(key) = pending.filter(|key| !pressed && mods == 0 && !down(key.vk)) else { return };
    let Some(bridge) = BRIDGE.get() else { return };
    let result = bridge.snapshot().hotkeys.with(slot, key).and_then(apply_hotkeys);
    MEM.with(|m| {
        let mut m = m.borrow_mut();
        match result {
            Ok(()) => {
                m.capture = None;
                m.capture_error.clear();
            }
            Err(error) => {
                m.capture_error = error;
                m.capture_pending = None;
                m.capture_preview.clear();
            }
        }
    });
    // Hata: yakalama sürüyor, kısayollar yine bırakılsın (apply_hotkeys eskileri geri kaydetti).
    if MEM.with(|m| m.borrow().capture.is_some()) {
        unregister_hotkeys();
    }
    unsafe { InvalidateRect(h().status, null(), 0) };
}

fn set_engine(engine: Engine) {
    let Some(bridge) = BRIDGE.get() else { return };
    bridge.set_engine(engine);
    if let Err(error) = super::save_engine(&super::app_dir(), engine) {
        crate::bridge::Platform::log(&bridge.platform, &format!("Ayar kaydedilemedi: {error}"));
    }
    MEM.with(|m| m.borrow_mut().status = None);
    refresh();
}

fn set_copy_only(copy_only: bool) {
    let Some(bridge) = BRIDGE.get() else { return };
    bridge.set_copy_only(copy_only);
    if let Err(error) = super::save_copy_only(&super::app_dir(), copy_only) {
        crate::bridge::Platform::log(&bridge.platform, &format!("Ayar kaydedilemedi: {error}"));
    }
    // Durum penceresi kısayol açıklamalarını yeniden çizsin.
    MEM.with(|m| m.borrow_mut().status = None);
    refresh();
}

/// 200 ms'de bir: değişen ne varsa (rozet, ipucu, durum penceresi) yeniden çiz.
fn refresh() {
    let Some(bridge) = BRIDGE.get() else { return };
    let hs = h();
    let snap = bridge.snapshot();
    let now = Instant::now();
    let (phase, subtitle) = view::badge(&snap, now);
    // Dinlerken (ve izin beklerken) nokta ~0,65 sn'de bir yanıp söner; yalnızca o zaman yeniden çizilir.
    let pulse = matches!(phase, BadgePhase::Listening | BadgePhase::Permission)
        && (started().elapsed().as_millis() / 650).is_multiple_of(2);
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
        sync_badge(true);
        render_badge();
        raise_badge();
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
            match SLOTS.get(wparam.wrapping_sub(1)) {
                Some(Slot::Toggle) => action(Action::Toggle),
                Some(Slot::ToggleEnter) => action(Action::ToggleEnter),
                Some(Slot::Cancel) => action(Action::Cancel),
                None => {}
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
            // Ekran ya da görev çubuğu değişti: rozet yeni çalışma alanına oturur.
            sync_badge(false);
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

/// Mikrofon: uygulama simgesindeki çizimin içi boş, zeminsiz hâli; (cx, cy) merkezli, durum renginde.
unsafe fn draw_mic(dc: HDC, cx: i32, cy: i32, accent: u32) {
    // Kutu, parlamanın sönmesine yetecek kadar çizimden geniş.
    let size = px(54.0);
    let pixels = crate::icon::glyph(size as usize, 38.0 * h().scale, 2.0 * h().scale, accent);
    gfx::blend(dc, cx - size / 2, cy - size / 2, size, &pixels);
}

/// Kare: mikrofon ve (dinlerken) yanıp sönen nokta. Genişleyince karenin yanında başlık ve alt yazı;
/// metin genişlemiş hâle göre yerleşir, pencere daha darken kenarından kırpılır (açılırken ortaya çıkar).
unsafe fn draw_badge(dc: HDC, rc: &RECT) {
    let hs = h();
    let (phase, subtitle, pulse) =
        MEM.with(|m| m.borrow().badge.clone()).unwrap_or((BadgePhase::Closed, String::new(), false));
    let (grow_left, full_w) = BADGE.with(|b| {
        let b = b.borrow();
        (b.grow_left, b.full_w)
    });
    let style = phase.style();
    let accent = style.accent;
    let side = px(BADGE_SIDE);
    gfx::gradient(dc, rc, gfx::mix(accent, palette::BG, 0.13), palette::BG);
    gfx::neon_frame(dc, rc, px(14.0), accent, palette::BG, px(1.0));
    let square = if grow_left { rc.right - side } else { 0 };
    draw_mic(dc, square + side / 2, side / 2, accent);
    if rc.right > side + px(4.0) {
        let divider = if grow_left { square } else { side };
        gfx::line(dc, &[(divider, px(14.0)), (divider, rc.bottom - px(14.0))],
                  gfx::mix(accent, palette::BG, 0.3), px(1.0));
        let text_w = (full_w - side - px(28.0)).max(0);
        let left = if grow_left { square - px(12.0) - text_w } else { side + px(12.0) };
        let right = left + text_w;
        let flags = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS;
        SetTextCharacterExtra(dc, px(1.0));
        let title = RECT { left, top: px(8.0), right, bottom: px(33.0) };
        gfx::glow_text(dc, hs.font_badge_title, style.title, &title, flags, accent, palette::BG, px(1.0));
        SetTextCharacterExtra(dc, 0);
        let sub = RECT { left, top: px(33.0), right, bottom: px(50.0) };
        gfx::text(dc, hs.font_badge_sub, gfx::mix(accent, palette::MUTED, 0.35), &subtitle, &sub, flags);
    }
    // Dinlerken (ve izin beklerken) karenin sağ üstünde yanıp sönen nokta.
    if matches!(phase, BadgePhase::Listening | BadgePhase::Permission) {
        let (x, y) = (square + side - px(11.0), px(11.0));
        let color = if pulse { accent } else { gfx::mix(accent, palette::BG, 0.35) };
        gfx::glow_dot(dc, x, y, px(3.0), color, palette::BG);
    }
}

/// Rozetin köşe dışı: saydam sayılan renk (çizimde hiç çıkmaz: tüm vurgu renklerinde yeşil > 0).
const BADGE_KEY: u32 = 0xff00ff;

/// Rozeti bellekte çizip konum ve boyutla birlikte tek çağrıda ekrana koyar (UpdateLayeredWindow).
/// Konum, boyut ve içerik ayrı ayrı değişince geçiş sırasında ara kareler görünüyordu (titreme).
fn render_badge() {
    let hs = h();
    let (x, y, w) = BADGE.with(|b| b.borrow().shown);
    let height = px(BADGE_SIDE);
    if w <= 0 {
        return;
    }
    unsafe {
        let screen = GetDC(null_mut());
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, height);
        let old = SelectObject(mem, bmp);
        SetBkMode(mem, TRANSPARENT as _);
        let rc = RECT { left: 0, top: 0, right: w, bottom: height };
        gfx::fill(mem, &rc, BADGE_KEY);
        // Yuvarlak köşeler: çizim bu bölgeyle kırpılır, dışı saydam renkte kalır.
        let region = CreateRoundRectRgn(0, 0, w + 1, height + 1, px(14.0), px(14.0));
        SelectClipRgn(mem, region);
        draw_badge(mem, &rc);
        SelectClipRgn(mem, null_mut());
        DeleteObject(region);
        let (at, size, origin) = (POINT { x, y }, SIZE { cx: w, cy: height }, POINT { x: 0, y: 0 });
        UpdateLayeredWindow(hs.badge, screen, &at, &size, mem, &origin, gfx::rgb(BADGE_KEY), null(), ULW_COLORKEY);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
        ReleaseDC(null_mut(), screen);
    }
}

unsafe extern "system" fn badge_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SETCURSOR => {
            let dragging = BADGE.with(|b| b.borrow().press.is_some_and(|p| p.dragging));
            SetCursor(LoadCursorW(null_mut(), if dragging { IDC_SIZEALL } else { IDC_HAND }));
            1
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_MOUSEMOVE => {
            badge_mouse_move(hwnd);
            0
        }
        WM_MOUSELEAVE => {
            let pressed = BADGE.with(|b| {
                let mut b = b.borrow_mut();
                if b.press.is_none() {
                    b.hover = false;
                }
                b.press.is_some()
            });
            if !pressed {
                sync_badge(true);
            }
            0
        }
        // Basılı tutup sürükle: taşınır; kısa tık: durum penceresi.
        WM_LBUTTONDOWN => {
            badge_press(hwnd);
            0
        }
        WM_LBUTTONUP => {
            badge_release(true);
            0
        }
        WM_CAPTURECHANGED => {
            badge_release(false);
            0
        }
        // Sağ tık: tepsi simgesiyle aynı menü (yapıştır / yalnızca kopyala, kısayollar).
        WM_RBUTTONUP => {
            tray_menu();
            0
        }
        WM_TIMER if wparam == TIMER_ANIM => {
            badge_anim_tick();
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            // Katmanlı pencere: içerik render_badge ile gelir; burada yalnızca boyama isteği kapatılır.
            BeginPaint(hwnd, &mut ps);
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
    let snap = bridge.snapshot();
    let t = view::status_text(&snap);
    let accent = t.accent;
    let (capture, capture_error, hover_row, capture_preview) = MEM.with(|m| {
        let m = m.borrow();
        (m.capture, m.capture_error.clone(), m.hover_row, m.capture_preview.clone())
    });
    let mut rows = Vec::new();
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

            let hc = gfx::height(dc, hs.font_normal, &t.connection, width);
            gfx::text(dc, hs.font_normal, palette::MUTED, &t.connection,
                      &RECT { left: x, top: y, right: x + width, bottom: y + hc }, wrap);
            y += hc + px(18.0);

            // Son aktarım.
            let inner = width - px(32.0);
            let ix = x + px(16.0);
            let line_h = px(21.0);
            let h_transfer = gfx::height(dc, hs.font_bold, t.transfer, inner);
            let h_details = gfx::height(dc, hs.font_normal, &t.details, inner);
            let content = h_transfer + px(8.0) + line_h * t.checks.len() as i32 + px(8.0) + h_details;
            let used = card(dc, x, y, width, content, "SON DİKTE", accent);
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

            // Kısayollar: her tuş ayrı kapak, aralarında "+"; açıklamalar tek sütunda hizalı. Satıra tıklayınca
            // (ya da menüden seçince) o satır yeni birleşimi bekler.
            let cap_w = |k: &str| (gfx::width(dc, hs.font_key, k) + px(16.0)).max(px(38.0));
            let plus_w = px(16.0);
            let keys_w = |keys: &[String]| keys.iter().map(|k| cap_w(k)).sum::<i32>() + plus_w * (keys.len() as i32 - 1);
            let shortcuts = view::shortcuts(&snap);
            let note = if capture.is_some() {
                "Yeni birleşime basın: Ctrl, Alt, Shift ya da Win ile bir tuş (F tuşları tek başına da olur). \
                 Tuşları bırakınca kaydedilir. Esc veya aynı satıra tıklamak vazgeçer."
            } else {
                view::shortcut_note(snap.copy_only)
            };
            let column = shortcuts.iter().map(|s| keys_w(&s.keys)).max().unwrap_or(0).max(px(150.0)) + px(14.0);
            let row_h = px(34.0);
            let h_note = gfx::height(dc, hs.font_normal, note, inner);
            let h_error = if capture_error.is_empty() { 0 } else {
                px(6.0) + gfx::height(dc, hs.font_normal, &capture_error, inner)
            };
            let content = row_h * shortcuts.len() as i32 + px(6.0) + h_note + h_error;
            let card_accent = if capture.is_some() { palette::PINK } else { palette::TEAL };
            card(dc, x, y, width, content, "KISAYOLLAR", card_accent);
            let mut cy = y + px(16.0) + px(18.0);
            for shortcut in &shortcuts {
                let row = RECT { left: ix - px(8.0), top: cy, right: ix + inner + px(8.0), bottom: cy + row_h };
                rows.push((shortcut.slot, row));
                if hover_row == Some(shortcut.slot) || capture == Some(shortcut.slot) {
                    let color = if capture == Some(shortcut.slot) { palette::PINK } else { palette::TEAL };
                    let highlight = RECT { top: row.top + px(1.0), bottom: row.bottom - px(1.0), ..row };
                    gfx::round_rect(dc, &highlight, px(8.0), Some(gfx::mix(color, palette::CARD, 0.13)),
                                    gfx::mix(color, palette::CARD, 0.55), px(1.0));
                }
                let mut kx = ix;
                let (top, bottom) = (cy + px(4.0), cy + row_h - px(5.0));
                if capture == Some(shortcut.slot) {
                    let wait = RECT { left: ix, top, right: ix + column - px(14.0), bottom };
                    gfx::round_rect(dc, &wait, px(7.0), Some(gfx::mix(palette::PINK, palette::CARD, 0.12)),
                                    palette::PINK, px(1.0));
                    let preview = if capture_preview.is_empty() { "tuşlara basın…" } else { &capture_preview };
                    gfx::text(dc, hs.font_key, palette::PINK, preview, &wait,
                              DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                } else {
                    for (i, key) in shortcut.keys.iter().enumerate() {
                        if i > 0 {
                            gfx::text(dc, hs.font_normal, palette::MUTED, "+",
                                      &RECT { left: kx, top, right: kx + plus_w, bottom },
                                      DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                            kx += plus_w;
                        }
                        let w = cap_w(key);
                        // Kapak: alt kenarı biraz kalın, basılabilir tuş gibi.
                        let shadow = RECT { left: kx, top: top + px(2.0), right: kx + w, bottom: bottom + px(2.0) };
                        gfx::round_rect(dc, &shadow, px(7.0), Some(gfx::mix(palette::TEAL, palette::CARD, 0.22)),
                                        gfx::mix(palette::TEAL, palette::CARD, 0.22), px(1.0));
                        let cap = RECT { left: kx, top, right: kx + w, bottom };
                        gfx::round_rect(dc, &cap, px(7.0), Some(gfx::mix(palette::TEAL, palette::CARD, 0.09)),
                                        gfx::mix(palette::TEAL, palette::CARD, 0.55), px(1.0));
                        gfx::text(dc, hs.font_key, palette::TEAL, key, &cap, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                        kx += w;
                    }
                }
                gfx::text(dc, hs.font_normal, palette::TEXT, shortcut.what,
                          &RECT { left: ix + column, top: cy, right: ix + inner, bottom: cy + row_h },
                          DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                cy += row_h;
            }
            cy += px(6.0);
            gfx::text(dc, hs.font_normal, palette::MUTED, note,
                      &RECT { left: ix, top: cy, right: ix + inner, bottom: cy + h_note }, wrap);
            if !capture_error.is_empty() {
                cy += h_note + px(6.0);
                gfx::text(dc, hs.font_normal, palette::RED, &capture_error,
                          &RECT { left: ix, top: cy, right: ix + inner, bottom: cy + h_error }, wrap);
            }
        });
    }
    MEM.with(|m| m.borrow_mut().rows = rows);
}

/// Durum penceresinde (x, y) noktasındaki kısayol satırı.
fn shortcut_row_at(x: i32, y: i32) -> Option<Slot> {
    MEM.with(|m| {
        m.borrow().rows.iter().find(|(_, r)| x >= r.left && x < r.right && y >= r.top && y < r.bottom).map(|(s, _)| *s)
    })
}

fn hide_status(hwnd: HWND) {
    end_capture();
    set_hover_row(None);
    unsafe { ShowWindow(hwnd, SW_HIDE) };
}

fn set_hover_row(row: Option<Slot>) {
    let changed = MEM.with(|m| {
        let mut m = m.borrow_mut();
        if m.hover_row == row { return false; }
        m.hover_row = row;
        true
    });
    if changed {
        unsafe { InvalidateRect(h().status, null(), 0) };
    }
}

unsafe fn track_mouse_leave(hwnd: HWND) {
    let mut tracking = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    TrackMouseEvent(&mut tracking);
}

/// Yerel düğmenin tıklama ve klavye davranışını koruyup yalnızca hover çizimini ekler.
unsafe extern "system" fn button_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
                                      id: usize, _data: usize) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => end_capture(),
        WM_MOUSEMOVE => {
            set_hover_row(None);
            let changed = MEM.with(|m| {
                let mut m = m.borrow_mut();
                if m.hover_button == Some(id) { return false; }
                m.hover_button = Some(id);
                true
            });
            if changed {
                track_mouse_leave(hwnd);
                InvalidateRect(hwnd, null(), 0);
            }
        }
        WM_MOUSELEAVE => {
            MEM.with(|m| {
                let mut m = m.borrow_mut();
                if m.hover_button == Some(id) { m.hover_button = None; }
            });
            InvalidateRect(hwnd, null(), 0);
        }
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(null_mut(), IDC_HAND));
            return 1;
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// Koyu, neon çerçeveli düğme (sahibi çizer).
unsafe fn draw_button(item: &DRAWITEMSTRUCT) {
    let hs = h();
    let quit = item.CtlID as usize == BUTTON_QUIT;
    let accent = if quit { palette::RED } else { palette::TEAL };
    let pressed = item.itemState & ODS_SELECTED != 0;
    let focused = item.itemState & ODS_FOCUS != 0;
    let hovered = MEM.with(|m| m.borrow().hover_button == Some(item.CtlID as usize));
    let r = item.rcItem;
    gfx::buffered(item.hDC, r.right - r.left, r.bottom - r.top, |dc| {
        let local = RECT { left: 0, top: 0, right: r.right - r.left, bottom: r.bottom - r.top };
        gfx::fill(dc, &local, palette::BG);
        let fill = gfx::mix(accent, palette::BG, if pressed { 0.34 } else if hovered { 0.21 } else { 0.07 });
        let border = gfx::mix(accent, palette::BG, if focused || hovered { 0.95 } else { 0.55 });
        gfx::round_rect(dc, &local, px(10.0), Some(fill), border, px(1.0));
        let label = if quit { "Uygulamayı kapat" } else { "Gizle" };
        gfx::text(dc, hs.font_bold, gfx::mix(0xffffff, accent, 0.2), label, &local,
                  DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    });
}

unsafe extern "system" fn status_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = ((lparam & 0xffff) as i16 as i32, ((lparam >> 16) & 0xffff) as i16 as i32);
            set_hover_row(shortcut_row_at(x, y));
            track_mouse_leave(hwnd);
            0
        }
        WM_MOUSELEAVE => {
            set_hover_row(None);
            0
        }
        // Kapatmak yalnızca gizler; köprü çalışmaya devam eder.
        WM_CLOSE => {
            hide_status(hwnd);
            0
        }
        // Başka pencereye geçilince yakalama biter; kısayollar yeniden kaydedilir.
        WM_ACTIVATE => {
            if wparam & 0xffff == WA_INACTIVE as usize {
                end_capture();
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONUP => {
            let (x, y) = ((lparam & 0xffff) as i16 as i32, ((lparam >> 16) & 0xffff) as i16 as i32);
            let active = MEM.with(|m| m.borrow().capture);
            if let Some(slot) = shortcut_row_at(x, y).filter(|slot| Some(*slot) != active) {
                begin_capture(slot);
            } else {
                end_capture();
            }
            0
        }
        WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_NCLBUTTONDOWN => {
            end_capture();
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_SETCURSOR if wparam as HWND == hwnd && (lparam & 0xffff) as u32 == HTCLIENT => {
            let mut at: POINT = zeroed();
            GetCursorPos(&mut at);
            ScreenToClient(hwnd, &mut at);
            let cursor = if shortcut_row_at(at.x, at.y).is_some() { IDC_HAND } else { IDC_ARROW };
            SetCursor(LoadCursorW(null_mut(), cursor));
            1
        }
        WM_COMMAND => {
            match wparam & 0xffff {
                BUTTON_HIDE => hide_status(hwnd),
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
        let badge = CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED,
                                    badge_class.as_ptr(),
                                    wide("Dikte").as_ptr(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), instance,
                                    null());
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let scale = dpi as f32 / 96.0;
        let mut outer = RECT { left: 0, top: 0, right: (500.0 * scale) as i32, bottom: (640.0 * scale) as i32 };
        AdjustWindowRectEx(&mut outer, style, 0, 0);
        let status = CreateWindowExW(0, status_class.as_ptr(),
                                     wide(concat!("ChatGPT Dikte v", env!("CARGO_PKG_VERSION"), " — Durum")).as_ptr(), style,
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
                font_key: font("Segoe UI Semibold", 9, 600, dpi),
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
            let button = CreateWindowExW(0, wide("BUTTON").as_ptr(), wide(label).as_ptr(),
                            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, x, button_y, width, px(36.0),
                            status, id as _, instance, null());
            SetWindowSubclass(button, Some(button_proc), id, 0);
        }
        dark_title_bar(status);

        let failed = register_hotkeys(&BRIDGE.get().unwrap().snapshot().hotkeys);
        if !failed.is_empty() {
            let line = format!("Kısayol kaydedilemedi (başka bir uygulama kullanıyor): {}", failed.join(", "));
            BRIDGE.get().unwrap().platform_log(&line);
            message_box(&line, true);
        }

        load_badge_anchor();
        refresh();
        sync_badge(false);
        let tip = MEM.with(|m| m.borrow().tip.clone());
        tray(NIM_ADD, &tip);
        SetTimer(host, TIMER_REFRESH, 200, None);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            let in_status = msg.hwnd == status || IsChild(status, msg.hwnd) != 0;
            if in_status && MEM.with(|m| m.borrow().capture.is_some()) {
                // Yakalama: tuşlar düğmelere ya da sistem menüsüne gitmez (Alt dahil).
                match msg.message {
                    WM_KEYDOWN | WM_SYSKEYDOWN => {
                        capture_key(msg.wParam as u16, true);
                        continue;
                    }
                    WM_KEYUP | WM_SYSKEYUP => {
                        capture_key(msg.wParam as u16, false);
                        continue;
                    }
                    WM_CHAR | WM_SYSCHAR => continue,
                    _ => {}
                }
            }
            // Alt'lı birleşim yakalandıktan sonra bırakılan Alt sistem menüsünü açmasın.
            if in_status && msg.message == WM_SYSKEYUP && [VK_MENU, VK_F10].contains(&(msg.wParam as u16)) {
                continue;
            }
            if msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE as usize && in_status {
                hide_status(status);
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
