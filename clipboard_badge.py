"""Compact status badge that never activates or focuses the target window."""
import ctypes
from ctypes import wintypes
import time
import tkinter as tk

PHASES = {
    'closed': ('BAĞLI DEĞİL', 'chatgpt.com sekmesini açın', '#555b63', '#ffffff'),
    'ready': ('HAZIR', 'Ctrl+Alt+Space: konuş', '#18864b', '#ffffff'),
    'listening': ('DİNLİYOR', 'Ctrl+Alt+Space: bitir', '#c62828', '#ffffff'),
    'transcribing': ('YAZIYOR…', 'ChatGPT yazıya döküyor', '#1f5fbf', '#ffffff'),
    'copied': ('YAPIŞTIRILDI', 'Metin panoda', '#e8881b', '#20160b'),
    'error': ('HATA', 'Ayrıntı için simgeye tıkla', '#8e2424', '#ffffff'),
}
FLASH_SECONDS = 4


def phase_for(state, now=None):
    now = time.monotonic() if now is None else now
    phase = state['phase']
    if phase in ('starting', 'listening'):
        return 'listening'
    if phase == 'transcribing':
        return 'transcribing'
    transfer = state['transfer']
    recent = now - state.get('phase_at', 0) < FLASH_SECONDS
    if recent and transfer['stage'] == 'sent' and transfer.get('copied_at', 0) >= state.get('phase_at', 0) - 1:
        return 'copied'
    if recent and state.get('message'):
        return 'error'
    return 'ready' if state['connected'] else 'closed'


class ClipboardBadge:
    def __init__(self, root):
        self.window = tk.Toplevel(root)
        self.window.withdraw()
        self.window.overrideredirect(True)
        self.window.attributes('-topmost', True)
        self.canvas = tk.Canvas(self.window, width=174, height=64, highlightthickness=0)
        self.canvas.pack()
        self.phase = None
        self.hwnd = None
        self.place_without_focus()

    def place_without_focus(self):
        user32 = ctypes.WinDLL('user32', use_last_error=True)
        user32.GetParent.argtypes = [wintypes.HWND]
        user32.GetParent.restype = wintypes.HWND
        user32.GetWindowLongW.argtypes = [wintypes.HWND, ctypes.c_int]
        user32.GetWindowLongW.restype = ctypes.c_long
        user32.SetWindowLongW.argtypes = [wintypes.HWND, ctypes.c_int, ctypes.c_long]
        user32.SetWindowLongW.restype = ctypes.c_long
        user32.SetWindowPos.argtypes = [wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_uint]
        user32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
        user32.SystemParametersInfoW.argtypes = [wintypes.UINT, wintypes.UINT, ctypes.c_void_p, wintypes.UINT]
        self.window.update_idletasks()
        child = self.window.winfo_id()
        self.hwnd = user32.GetParent(child) or child
        style = user32.GetWindowLongW(self.hwnd, -20)
        # WS_EX_NOACTIVATE and WS_EX_TOOLWINDOW: no focus, no taskbar button.
        user32.SetWindowLongW(self.hwnd, -20, style | 0x08000000 | 0x00000080)
        area = wintypes.RECT()
        if not user32.SystemParametersInfoW(0x30, 0, ctypes.byref(area), 0):
            area.right = self.window.winfo_screenwidth()
            area.bottom = self.window.winfo_screenheight() - 48
        x, y = max(area.left, area.right - 194), max(area.top, area.bottom - 84)
        self.window.geometry(f'174x64+{x}+{y}')
        self.user32 = user32

    def show(self):
        self.window.deiconify()
        self.user32.ShowWindow(self.hwnd, 4)  # SW_SHOWNOACTIVATE
        self.user32.SetWindowPos(self.hwnd, ctypes.c_void_p(-1), 0, 0, 0, 0,
                                 0x0001 | 0x0002 | 0x0010 | 0x0040)  # no move, size or activation

    def render(self, state):
        phase = phase_for(state)
        title, subtitle, color, ink = PHASES[phase]
        transfer = state['transfer']
        if phase == 'copied':
            subtitle = str(transfer['characters']) + ' karakter' + (' + Enter' if transfer.get('enter_sent') else '')
        elif phase == 'error':
            subtitle = state['message'][:30]
        signature = (phase, subtitle)
        if signature == self.phase:
            return
        self.phase = signature
        canvas = self.canvas
        canvas.configure(bg=color)
        canvas.delete('all')
        # mikrofon simgesi
        canvas.create_rectangle(16, 12, 28, 34, outline=ink, width=2, fill=ink if phase == 'listening' else '')
        canvas.create_arc(10, 20, 34, 42, start=180, extent=180, style='arc', outline=ink, width=2)
        canvas.create_line(22, 42, 22, 50, fill=ink, width=2)
        canvas.create_line(15, 50, 29, 50, fill=ink, width=2)
        canvas.create_text(104, 23, text=title, fill=ink, font=('Segoe UI', 12, 'bold'))
        canvas.create_text(104, 44, text=subtitle, fill=ink, font=('Segoe UI', 8))
        self.show()
