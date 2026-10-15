"""ChatGPT diktesini klavye kısayoluyla başlatıp metni odaktaki pencereye yapıştırır; API kullanmaz.

Akış:
  Ctrl+Alt+Space      -> ChatGPT sekmesinde dikte başlar (pencere değişmez)
  Ctrl+Alt+Space      -> dikte biter, metin odaktaki pencereye yapıştırılır
  Ctrl+Alt+Enter      -> dikte biter, metin yapıştırılır ve Enter gönderilir
  Ctrl+Alt+Backspace  -> dikte iptal edilir

Chrome eklentisinin içerik betiği /poll ile komut bekler, sonucu /report ile bildirir.
"""
import ctypes
from ctypes import wintypes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import secrets
import sys
import threading
import time
from urllib.parse import parse_qs, urlparse
import winsound

import keyboard
import pyperclip

if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')

ROOT = Path(__file__).resolve().parent
PORT = 47831
POLL_SECONDS = 20
CLIENT_TTL = POLL_SECONDS + 15
START_TIMEOUT = 8
RESULT_TIMEOUT = 120
HOTKEYS = {'toggle': 'ctrl+alt+space', 'enter': 'ctrl+alt+enter', 'cancel': 'ctrl+alt+backspace'}

USER32 = ctypes.WinDLL('user32', use_last_error=True)
KERNEL32 = ctypes.WinDLL('kernel32', use_last_error=True)
USER32.GetForegroundWindow.restype = wintypes.HWND
USER32.IsWindow.argtypes = [wintypes.HWND]
USER32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
USER32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
USER32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
USER32.SetForegroundWindow.argtypes = [wintypes.HWND]
USER32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
KERNEL32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
KERNEL32.OpenProcess.restype = wintypes.HANDLE
KERNEL32.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
KERNEL32.CloseHandle.argtypes = [wintypes.HANDLE]

# Bu uygulamalarda Ctrl+V terminal yerine editöre veya uygulamaya gidebilir; Ctrl+Shift+V terminal yapıştırmasıdır.
SHIFT_PASTE_EXES = {'windowsterminal.exe', 'code.exe', 'cursor.exe', 'windsurf.exe', 'wezterm-gui.exe',
                    'alacritty.exe', 'conemu64.exe', 'conemu.exe'}


def normalize_text(text):
    if not isinstance(text, str):
        raise ValueError('Metin geçerli değil.')
    # Yeni satır ve kontrol karakterleri terminalde komut gönderimine dönüşmesin.
    text = ' '.join(''.join(c if c.isprintable() else ' ' for c in text).split())
    if not text:
        raise ValueError('Ses algılanmadı; metin boş.')
    if len(text) > 32000:
        raise ValueError('Metin çok uzun (en fazla 32000 karakter).')
    return text


def window_info(hwnd):
    pid = wintypes.DWORD()
    USER32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    title, cls, path = (ctypes.create_unicode_buffer(32768) for _ in range(3))
    USER32.GetWindowTextW(hwnd, title, len(title))
    USER32.GetClassNameW(hwnd, cls, len(cls))
    proc = KERNEL32.OpenProcess(0x1000, False, pid.value)
    if proc:
        try:
            size = wintypes.DWORD(len(path))
            KERNEL32.QueryFullProcessImageNameW(proc, 0, path, ctypes.byref(size))
        finally:
            KERNEL32.CloseHandle(proc)
    return {'hwnd': hwnd, 'pid': pid.value, 'title': title.value,
            'class': cls.value, 'exe': Path(path.value).name.lower()}


def paste_chord(target):
    if target['class'] == 'ConsoleWindowClass':
        return 'ctrl+v'
    return 'ctrl+shift+v' if target['exe'] in SHIFT_PASTE_EXES else 'ctrl+v'


LOG_PATH = None  # tray_app EXE yanındaki dikte.log dosyasını ayarlar


def log(message):
    """Olayları kaydeder; dikte edilen metin asla günlüğe yazılmaz."""
    print(message, flush=True)
    if LOG_PATH:
        try:
            with open(LOG_PATH, 'a', encoding='utf-8') as f:
                f.write(time.strftime('%Y-%m-%d %H:%M:%S ') + message + '\n')
        except OSError:
            pass


def beep(freq, ms):
    threading.Thread(target=winsound.Beep, args=(freq, ms), daemon=True).start()


class Bridge:
    def __init__(self):
        self.cond = threading.Condition(threading.RLock())
        self.lock = self.cond
        self.transfer_lock = threading.Lock()
        self.phase = 'idle'          # idle | starting | listening | transcribing
        self.session = 0
        self.owner = None            # dikteyi yürüten ChatGPT sekmesinin istemci kimliği
        self.clients = {}            # istemci kimliği -> son görülme zamanı
        self.commands = []
        self.target = None           # metnin yapıştırılacağı pencere (durdurma anında odaktaki)
        self.start_target = None     # elle durdurulursa: dikte başlarken odaktaki pencere
        self.send_enter = False
        self.message = ''
        self.phase_at = 0
        self.transfer = {'stage': 'idle', 'copied': False, 'paste_sent': False, 'enter_sent': False,
                         'characters': 0, 'time': '', 'error': '', 'copied_at': 0}

    # --- durum -------------------------------------------------------------
    def connected(self):
        with self.lock:
            now = time.monotonic()
            return any(now - seen < CLIENT_TTL for seen in self.clients.values())

    def status(self):
        with self.lock:
            return {'connected': self.connected(), 'phase': self.phase}

    def snapshot(self):
        with self.lock:
            target = self.target
            return {'connected': self.connected(), 'phase': self.phase, 'phase_at': self.phase_at,
                    'message': self.message, 'target': target['title'] if target else '',
                    'transfer': dict(self.transfer)}

    def _set_phase(self, phase, message=''):
        with self.lock:
            self.phase = phase
            self.phase_at = time.monotonic()
            self.message = message
            if phase == 'idle':
                self.owner = None
            self.cond.notify_all()

    def _progress(self, **changes):
        with self.lock:
            self.transfer.update(changes)

    def _fail(self, message):
        self._set_phase('idle', message)
        beep(220, 180)
        log(message)

    # --- kısayollar --------------------------------------------------------
    def toggle(self, send_enter=False):
        with self.lock:
            phase = self.phase
        log('kısayol (' + phase + ')')
        if phase == 'idle':
            self.start()
        elif phase in ('starting', 'listening'):
            self.stop(send_enter)
        else:
            with self.lock:
                stuck = time.monotonic() - self.phase_at > 5
            if stuck:  # sonuç gelmediyse ikinci basış takılmayı çözer
                log('yazıya dökme sonucu gelmedi; sıfırlandı')
                self._fail('ChatGPT\'den metin gelmedi. Metin ChatGPT kutusunda kalmış olabilir.')
            else:
                beep(330, 60)  # yazıya dökülüyor; bekleyin

    def start(self):
        start_target = window_info(USER32.GetForegroundWindow())
        with self.lock:
            if self.phase != 'idle':
                return
            if not self.connected():
                self._fail('ChatGPT sekmesi bağlı değil. Chrome açık mı? Eklenti güncel değilse '
                           'chrome://extensions sayfasında ↻ basın veya Chrome\'u kapatıp açın.')
                return
            self.session += 1
            session = self.session
            self.target = None
            self.start_target = start_target
            self.send_enter = False
            self._set_phase('starting')
            self._enqueue('start', session, None)
        threading.Timer(START_TIMEOUT, self._expire, args=(session, 'starting',
                        'Dikte başlamadı. ChatGPT sekmesinde mikrofon iznini kontrol edin.')).start()
        log('Dikte başlatılıyor…')

    def stop(self, send_enter=False):
        target = window_info(USER32.GetForegroundWindow())
        with self.lock:
            if self.phase not in ('starting', 'listening'):
                return
            session = self.session
            self.target = target
            self.send_enter = send_enter
            self._set_phase('transcribing')
            self._enqueue('stop', session, self.owner)
        beep(660, 70)
        threading.Timer(RESULT_TIMEOUT, self._expire, args=(session, 'transcribing',
                        'ChatGPT yanıt vermedi. Dikteyi ChatGPT sekmesinde kontrol edin.')).start()
        log('Dikte durduruldu, metin bekleniyor → ' + target['title'])

    def cancel(self):
        with self.lock:
            if self.phase == 'idle':
                return
            self._enqueue('cancel', self.session, self.owner)
            self._set_phase('idle', 'Dikte iptal edildi.')
        beep(300, 120)
        log('Dikte iptal edildi.')

    def _expire(self, session, phase, message):
        with self.lock:
            if self.session != session or self.phase != phase:
                return
            self._enqueue('cancel', session, self.owner)
        self._fail(message)

    # --- eklenti iletişimi -------------------------------------------------
    def _enqueue(self, command, session, client):
        with self.lock:
            # Eski oturuma ait, teslim edilmemiş komutlar artık geçersiz.
            self.commands = [c for c in self.commands if c['session'] == session]
            self.commands.append({'command': command, 'session': session, 'client': client})
            self.cond.notify_all()

    def poll(self, client, timeout=POLL_SECONDS, version=''):
        if not isinstance(client, str) or not 8 <= len(client) <= 64:
            raise ValueError('İstemci kimliği geçersiz.')
        deadline = time.monotonic() + timeout
        with self.cond:
            if client not in self.clients:
                log('sekme bağlandı: ' + client[:8] + ' (eklenti ' + (version[:20] or 'eski sürüm') + ')')
            self.clients[client] = time.monotonic()
            # Uzun süredir görünmeyen sekmeleri unut.
            now = time.monotonic()
            self.clients = {k: v for k, v in self.clients.items() if now - v < CLIENT_TTL * 2}
            while True:
                for item in self.commands:
                    if item['client'] in (None, client) and (self.owner in (None, client) or item['command'] == 'cancel'):
                        self.commands.remove(item)
                        if item['command'] == 'start':
                            self.owner = client
                        self.clients[client] = time.monotonic()
                        return {'command': item['command'], 'session': item['session']}
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    self.clients[client] = time.monotonic()
                    return {'command': None}
                self.cond.wait(remaining)

    def report(self, client, data):
        session, event = data.get('session'), data.get('event')
        log('eklenti: ' + str(event) + ' (oturum ' + str(session) + ')' +
            (' — ' + str(data.get('error')) if event == 'error' else ''))
        with self.lock:
            if session != self.session or client != self.owner:
                return {'ok': False, 'stale': True}
            phase = self.phase
        if event == 'started' and phase == 'starting':
            self._set_phase('listening')
            beep(880, 90)
            log('Dinleniyor… Bitince Ctrl+Alt+Space.')
        elif event == 'error' and phase != 'idle':
            error = data.get('error')
            self._fail(error if isinstance(error, str) and error else 'ChatGPT dikte hatası.')
        elif event == 'result' and phase != 'idle':
            # 'transcribing' dışında gelirse kullanıcı ChatGPT'de kareye elle basmıştır.
            with self.lock:
                target, send_enter = self.target or self.start_target, self.send_enter
            self._set_phase('idle')
            try:
                return self.deliver(data.get('text'), target, send_enter)
            except ValueError as exc:
                with self.lock:
                    self.message = str(exc)
                beep(220, 180)
                log(str(exc))
                return {'ok': False, 'error': str(exc)}
        return {'ok': True}

    # --- yapıştırma --------------------------------------------------------
    def deliver(self, raw_text, target, send_enter=False):
        with self.transfer_lock:
            self._progress(stage='working', copied=False, paste_sent=False, enter_sent=False,
                           characters=0, time=time.strftime('%H:%M:%S'), error='', copied_at=0)
            try:
                return self._deliver(raw_text, target, send_enter)
            except Exception as exc:
                error = str(exc) if isinstance(exc, ValueError) else 'Aktarım tamamlanamadı. Yeniden deneyin.'
                self._progress(stage='error', error=error)
                if isinstance(exc, ValueError):
                    raise
                raise ValueError(error) from exc

    def _deliver(self, raw_text, target, send_enter):
        text = normalize_text(raw_text)
        self._progress(characters=len(text))
        deadline = time.monotonic() + 3
        while any(keyboard.is_pressed(k) for k in ('ctrl', 'alt', 'shift', 'windows')):
            if time.monotonic() > deadline:
                break  # tuşlar hâlâ basılıysa yine de dene; kısayol gönderimi bunları geçici bırakır
            time.sleep(0.02)
        pyperclip.copy(text)
        self._progress(copied=True, copied_at=time.monotonic())
        if not target or not USER32.IsWindow(target['hwnd']):
            raise ValueError('Hedef pencere kapandı. Metin panoda; Ctrl+V ile yapıştırın.')
        if USER32.GetForegroundWindow() != target['hwnd']:
            USER32.ShowWindow(target['hwnd'], 9)
            USER32.SetForegroundWindow(target['hwnd'])
            for _ in range(20):
                if USER32.GetForegroundWindow() == target['hwnd']:
                    break
                time.sleep(0.025)
            if USER32.GetForegroundWindow() != target['hwnd']:
                raise ValueError('Windows pencere geçişine izin vermedi. Metin panoda; Ctrl+V ile yapıştırın.')
            time.sleep(0.08)
        keyboard.send(paste_chord(target))
        self._progress(stage='sent', paste_sent=True)
        if send_enter:
            time.sleep(0.25)  # yapıştırma işlenmeden Enter gitmesin
            if USER32.GetForegroundWindow() == target['hwnd']:
                keyboard.send('enter')
                self._progress(enter_sent=True)
        beep(1200, 50)
        log('Yapıştırıldı (' + str(len(text)) + ' karakter)' + (' + Enter.' if send_enter else '.'))
        return {'ok': True, 'characters': len(text)}

    # --- kısayol kaydı -----------------------------------------------------
    def register_hotkeys(self):
        def spawn(fn, *args):
            # Klavye kancası iş parçacığını asla bekletme.
            return lambda: threading.Thread(target=fn, args=args, daemon=True).start()
        return [keyboard.add_hotkey(HOTKEYS['toggle'], spawn(self.toggle), suppress=True, trigger_on_release=True),
                keyboard.add_hotkey(HOTKEYS['enter'], spawn(self.toggle, True), suppress=True, trigger_on_release=True),
                keyboard.add_hotkey(HOTKEYS['cancel'], spawn(self.cancel), suppress=True, trigger_on_release=True)]


class Server(ThreadingHTTPServer):
    daemon_threads = True


def handler_for(bridge, token):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass  # Metin veya erişim anahtarı günlüklenmez.

        def reply(self, status, payload):
            body = json.dumps(payload, ensure_ascii=False).encode('utf-8')
            self.send_response(status)
            self.send_header('Content-Type', 'application/json; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Cache-Control', 'no-store')
            origin = self.headers.get('Origin', '')
            if origin.startswith('chrome-extension://'):
                self.send_header('Access-Control-Allow-Origin', origin)
                self.send_header('Vary', 'Origin')
            self.end_headers()
            self.wfile.write(body)

        def authorized(self):
            origin = self.headers.get('Origin', '')
            supplied = self.headers.get('Authorization', '')
            return (not origin or origin.startswith('chrome-extension://')) and secrets.compare_digest(supplied, 'Bearer ' + token)

        def do_OPTIONS(self):
            origin = self.headers.get('Origin', '')
            if not origin.startswith('chrome-extension://'):
                self.reply(403, {'error': 'Erişim reddedildi.'})
                return
            self.send_response(204)
            self.send_header('Access-Control-Allow-Origin', origin)
            self.send_header('Access-Control-Allow-Methods', 'GET, POST, OPTIONS')
            self.send_header('Access-Control-Allow-Headers', 'Authorization, Content-Type')
            self.end_headers()

        def do_GET(self):
            if not self.authorized():
                self.reply(403, {'error': 'Erişim reddedildi.'})
                return
            url = urlparse(self.path)
            try:
                if url.path == '/status':
                    self.reply(200, bridge.status())
                elif url.path == '/poll':
                    query = parse_qs(url.query)
                    self.reply(200, bridge.poll(query.get('client', [''])[0], version=query.get('v', [''])[0]))
                elif url.path == '/trace':
                    query = parse_qs(url.query)
                    log('iz ' + query.get('client', ['?'])[0][:8] + ': ' + query.get('msg', [''])[0][:300])
                    self.reply(200, {'ok': True})
                else:
                    self.reply(404, {'error': 'Bulunamadı.'})
            except ValueError as exc:
                self.reply(400, {'error': str(exc)})

        def do_POST(self):
            if not self.authorized():
                self.reply(403, {'error': 'Erişim reddedildi.'})
                return
            if self.path != '/report':
                self.reply(404, {'error': 'Bulunamadı.'})
                return
            try:
                size = int(self.headers.get('Content-Length', '0'))
                if size < 1 or size > 262144:
                    raise ValueError('İstek boyutu geçersiz.')
                self.connection.settimeout(5)
                data = json.loads(self.rfile.read(size))
                if not isinstance(data, dict):
                    raise ValueError('İstek geçersiz.')
                self.reply(200, bridge.report(data.get('client'), data))
            except (ValueError, UnicodeError, TimeoutError) as exc:
                self.reply(400, {'error': str(exc)})
            except Exception:
                self.reply(500, {'error': 'İşlem tamamlanamadı.'})
    return Handler


def main():
    config = json.loads((ROOT / 'bridge-config.json').read_text(encoding='utf-8-sig'))
    bridge = Bridge()
    try:
        server = Server(('127.0.0.1', PORT), handler_for(bridge, config['token']))
    except OSError:
        log('Köprü zaten açık veya 47831 portu kullanımda.')
        return
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        bridge.register_hotkeys()
        log('Hazır. Chrome\'da chatgpt.com açık kalsın (arka planda olabilir).\n'
              '  Ctrl+Alt+Space      dikteyi başlat / bitir ve yapıştır\n'
              '  Ctrl+Alt+Enter      bitir, yapıştır ve Enter gönder\n'
              '  Ctrl+Alt+Backspace  iptal\n'
              'API kullanılmaz. Bu pencere açık kalmalı. Çıkış: Ctrl+C.')
        keyboard.wait()
    except KeyboardInterrupt:
        pass
    finally:
        keyboard.unhook_all_hotkeys()
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    main()
