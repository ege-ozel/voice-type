"""Windowless Windows tray launcher for the local dictation bridge."""
import ctypes
import json
import os
from pathlib import Path
import sys
import threading
from urllib.request import Request, urlopen

# PyInstaller --windowed has no stdout; the bridge's harmless status prints
# should never turn a successful transfer into an HTTP error.
if sys.stdout is None:
    sys.stdout = open(os.devnull, 'w', encoding='utf-8')
if sys.stderr is None:
    sys.stderr = open(os.devnull, 'w', encoding='utf-8')

import keyboard
import pystray
from PIL import Image, ImageDraw
import browser_bridge
from browser_bridge import Bridge, Server, handler_for
from status_window import StatusWindow

APP_NAME = 'ChatGPT Dikte'
APP_ROOT = Path(sys.executable).resolve().parent if getattr(sys, 'frozen', False) else Path(__file__).resolve().parent


def make_icon():
    image = Image.new('RGBA', (64, 64), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle((2, 2, 62, 62), radius=16, fill='#16734a')
    draw.rounded_rectangle((25, 11, 39, 36), radius=7, fill='white')
    draw.arc((18, 23, 46, 46), 0, 180, fill='white', width=4)
    draw.line((32, 46, 32, 53), fill='white', width=4)
    draw.line((24, 53, 40, 53), fill='white', width=4)
    return image


def message(text, error=False):
    ctypes.windll.user32.MessageBoxW(None, text, APP_NAME, 0x10 if error else 0x40)


def run(smoke_output=None):
    config = json.loads((APP_ROOT / 'bridge-config.json').read_text(encoding='utf-8-sig'))
    token = config['token']
    if not isinstance(token, str) or not token:
        raise ValueError('invalid token')
    if not smoke_output:
        browser_bridge.LOG_PATH = APP_ROOT / 'dikte.log'
    bridge = Bridge()
    try:
        server = Server(('127.0.0.1', 0 if smoke_output else 47831), handler_for(bridge, token))
    except OSError:
        message('Köprü zaten açık veya 47831 portu kullanımda.\n\nstart.bat yardımcı penceresi açıksa orada Ctrl+C ile kapatıp EXE dosyasını yeniden açın.', error=True)
        return 1

    stop = threading.Event()
    tray_ready = threading.Event()
    hooks = []
    started = False
    window = None

    def quit_app(icon=None, item=None):
        window.request_quit()

    window = StatusWindow(bridge, quit_app)
    from PIL import ImageTk
    window.app_icon = ImageTk.PhotoImage(make_icon(), master=window.root)
    window.root.iconphoto(True, window.app_icon)
    icon = pystray.Icon('ChatGPTDikte', make_icon(), APP_NAME,
        menu=pystray.Menu(
            pystray.MenuItem('Durum penceresi', lambda icon, item: window.request_show(), default=True),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem('Çıkış', quit_app)))

    def update_title():
        while not stop.wait(1):
            state = bridge.snapshot()
            transfer = state['transfer']
            if state['phase'] in ('starting', 'listening'):
                title = 'Dikte: dinliyor — bitirmek için Ctrl+Alt+Space'
            elif state['phase'] == 'transcribing':
                title = 'Dikte: ChatGPT yazıya döküyor…'
            elif not state['connected']:
                title = 'Dikte: ChatGPT sekmesi bağlı değil'
            elif transfer['stage'] == 'sent':
                title = 'Dikte: hazır — son aktarım ' + str(transfer['characters']) + ' karakter'
            else:
                title = 'Dikte: hazır — Ctrl+Alt+Space ile konuş'
            icon.title = title[:120]

    def tray_setup(tray):
        tray.visible = True
        tray_ready.set()

    def smoke_test():
        result = {'ok': False}
        try:
            base = 'http://127.0.0.1:' + str(server.server_port)
            auth = {'Authorization': 'Bearer ' + token}
            with urlopen(Request(base + '/status', headers=auth), timeout=3) as response:
                state = json.load(response)
            badge = window.badge
            from browser_bridge import USER32
            foreground = USER32.GetForegroundWindow()
            closed = badge.phase[0] == 'closed'
            # Bir ChatGPT sekmesi bağlanır gibi yoklama yap.
            polled = bridge.poll('smoke-client-1', timeout=0)
            window.refresh()
            ready = badge.phase[0] == 'ready'
            with bridge.lock:
                bridge.phase = 'listening'
            window.refresh()
            listening = badge.phase[0] == 'listening' and badge.canvas.cget('bg') == '#c62828'
            with bridge.lock:
                bridge.phase = 'idle'
            no_focus = USER32.GetForegroundWindow() == foreground
            children = [child for child in window.root.winfo_children() if child.winfo_class() == 'Toplevel']
            single = children == [badge.window]
            style = badge.user32.GetWindowLongW(badge.hwnd, -20)
            no_focus = no_focus and bool(style & 0x08000000)
            result = {'ok': state == {'connected': False, 'phase': 'idle'} and polled == {'command': None}
                            and tray_ready.is_set() and closed and ready and listening and single and no_focus,
                      'tray_visible': icon.visible, 'single_status_window': single,
                      'badge_phases': closed and ready and listening, 'badge_does_not_take_focus': no_focus,
                      'http_status': 200, 'frozen': bool(getattr(sys, 'frozen', False))}
        except Exception as exc:
            result = {'ok': False, 'error_type': type(exc).__name__, 'error': str(exc)}
        finally:
            Path(smoke_output).write_text(json.dumps(result), encoding='utf-8')
            window.root.quit()

    try:
        if not smoke_output:
            hooks = bridge.register_hotkeys()
        threading.Thread(target=server.serve_forever, daemon=True).start()
        started = True
        threading.Thread(target=icon.run, kwargs={'setup': tray_setup}, daemon=True).start()
        threading.Thread(target=update_title, daemon=True).start()
        if smoke_output:
            window.root.after(800, smoke_test)
        window.root.mainloop()
        return 0
    finally:
        stop.set()
        icon.stop()
        for hook in hooks:
            keyboard.remove_hotkey(hook)
        if started:
            server.shutdown()
        server.server_close()
        window.root.destroy()


if __name__ == '__main__':
    try:
        if len(sys.argv) == 3 and sys.argv[1] == '--smoke-test':
            sys.exit(run(Path(sys.argv[2]).resolve()))
        sys.exit(run())
    except Exception:
        message('Köprü başlatılamadı. EXE dosyasının yanında bridge-config.json bulunduğunu kontrol edin.', error=True)
        sys.exit(1)
