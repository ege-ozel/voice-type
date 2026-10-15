import importlib.util
import json
from pathlib import Path
import threading
import unittest
from unittest.mock import MagicMock, patch
from urllib.error import HTTPError
from urllib.request import Request, urlopen

spec = importlib.util.spec_from_file_location('bridge', Path(__file__).with_name('browser_bridge.py'))
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)

TERMINAL = {'hwnd': 111, 'pid': 3, 'title': 'Claude Code', 'class': 'CASCADIA_HOSTING_WINDOW_CLASS',
            'exe': 'windowsterminal.exe'}


def quiet():
    """Bip, zamanlayıcı ve gerçek pencere çağrılarını sustur."""
    native = MagicMock()
    native.IsWindow.return_value = True
    native.GetForegroundWindow.return_value = TERMINAL['hwnd']
    return [patch.object(bridge, 'beep'), patch.object(bridge.threading, 'Timer'),
            patch.object(bridge, 'USER32', native), patch.object(bridge, 'window_info', return_value=TERMINAL)]


class Patched(unittest.TestCase):
    def setUp(self):
        for p in quiet():
            p.start()
            self.addCleanup(p.stop)


class TextTests(unittest.TestCase):
    def test_control_characters_cannot_submit_input(self):
        self.assertEqual(bridge.normalize_text('Türkçe\r\nmetin\tİngilizce\x1b[31m'), 'Türkçe metin İngilizce [31m')
        for value in ('', '\n\x00', None, 'a' * 32001):
            with self.assertRaises(ValueError):
                bridge.normalize_text(value)

    def test_paste_chord_per_window(self):
        self.assertEqual(bridge.paste_chord(TERMINAL), 'ctrl+shift+v')
        self.assertEqual(bridge.paste_chord({**TERMINAL, 'class': 'ConsoleWindowClass', 'exe': 'cmd.exe'}), 'ctrl+v')
        self.assertEqual(bridge.paste_chord({**TERMINAL, 'class': 'Notepad', 'exe': 'notepad.exe'}), 'ctrl+v')


class FlowTests(Patched):
    def test_start_without_chatgpt_tab_fails_visibly(self):
        b = bridge.Bridge()
        b.toggle()
        state = b.snapshot()
        self.assertEqual(state['phase'], 'idle')
        self.assertIn('ChatGPT sekmesi bağlı değil', state['message'])

    def test_full_cycle_pastes_into_window_focused_at_stop(self):
        b = bridge.Bridge()
        self.assertEqual(b.poll('tab-aaaaaaaa', timeout=0), {'command': None})
        self.assertTrue(b.connected())
        b.toggle()
        self.assertEqual(b.snapshot()['phase'], 'starting')
        cmd = b.poll('tab-aaaaaaaa', timeout=0)
        self.assertEqual(cmd['command'], 'start')
        b.report('tab-aaaaaaaa', {'session': cmd['session'], 'event': 'started'})
        self.assertEqual(b.snapshot()['phase'], 'listening')
        b.toggle(send_enter=True)
        self.assertEqual(b.snapshot()['phase'], 'transcribing')
        self.assertEqual(b.poll('tab-aaaaaaaa', timeout=0)['command'], 'stop')
        with patch.object(b, 'deliver', return_value={'ok': True}) as deliver:
            b.report('tab-aaaaaaaa', {'session': cmd['session'], 'event': 'result', 'text': 'merhaba world'})
        deliver.assert_called_once_with('merhaba world', TERMINAL, True)
        self.assertEqual(b.snapshot()['phase'], 'idle')

    def test_manual_stop_in_chatgpt_pastes_into_start_window(self):
        b = bridge.Bridge()
        b.poll('tab-aaaaaaaa', timeout=0)
        b.toggle()
        session = b.poll('tab-aaaaaaaa', timeout=0)['session']
        b.report('tab-aaaaaaaa', {'session': session, 'event': 'started'})
        with patch.object(b, 'deliver', return_value={'ok': True}) as deliver:
            b.report('tab-aaaaaaaa', {'session': session, 'event': 'result', 'text': 'elle'})
        deliver.assert_called_once_with('elle', TERMINAL, False)
        self.assertEqual(b.snapshot()['phase'], 'idle')

    def test_stop_goes_only_to_the_dictating_tab(self):
        b = bridge.Bridge()
        b.poll('tab-aaaaaaaa', timeout=0)
        b.toggle()
        session = b.poll('tab-aaaaaaaa', timeout=0)['session']
        b.report('tab-aaaaaaaa', {'session': session, 'event': 'started'})
        b.toggle()
        self.assertEqual(b.poll('tab-bbbbbbbb', timeout=0), {'command': None})
        self.assertEqual(b.poll('tab-aaaaaaaa', timeout=0)['command'], 'stop')

    def test_reports_from_other_tabs_or_old_sessions_are_ignored(self):
        b = bridge.Bridge()
        b.poll('tab-aaaaaaaa', timeout=0)
        b.toggle()
        session = b.poll('tab-aaaaaaaa', timeout=0)['session']
        self.assertTrue(b.report('tab-bbbbbbbb', {'session': session, 'event': 'started'}).get('stale'))
        self.assertTrue(b.report('tab-aaaaaaaa', {'session': session - 1, 'event': 'started'}).get('stale'))
        self.assertEqual(b.snapshot()['phase'], 'starting')

    def test_chatgpt_error_returns_to_idle(self):
        b = bridge.Bridge()
        b.poll('tab-aaaaaaaa', timeout=0)
        b.toggle()
        session = b.poll('tab-aaaaaaaa', timeout=0)['session']
        b.report('tab-aaaaaaaa', {'session': session, 'event': 'error', 'error': 'Mikrofon izni yok'})
        state = b.snapshot()
        self.assertEqual((state['phase'], state['message']), ('idle', 'Mikrofon izni yok'))

    def test_cancel_sends_cancel_and_resets(self):
        b = bridge.Bridge()
        b.poll('tab-aaaaaaaa', timeout=0)
        b.toggle()
        session = b.poll('tab-aaaaaaaa', timeout=0)['session']
        b.cancel()
        self.assertEqual(b.snapshot()['phase'], 'idle')
        self.assertEqual(b.poll('tab-aaaaaaaa', timeout=0), {'command': 'cancel', 'session': session})

    def test_start_timeout_cancels(self):
        b = bridge.Bridge()
        b.poll('tab-aaaaaaaa', timeout=0)
        b.toggle()
        b._expire(b.session, 'starting', 'Dikte başlamadı.')
        self.assertEqual(b.snapshot()['phase'], 'idle')
        commands = [c['command'] for c in b.commands]
        self.assertEqual(commands, ['start', 'cancel'])

    def test_long_poll_wakes_up_on_command(self):
        b = bridge.Bridge()
        b.poll('tab-aaaaaaaa', timeout=0)
        got = []
        waiter = threading.Thread(target=lambda: got.append(b.poll('tab-aaaaaaaa', timeout=5)))
        waiter.start()
        b.toggle()
        waiter.join(2)
        self.assertFalse(waiter.is_alive())
        self.assertEqual(got[0]['command'], 'start')


class DeliverTests(Patched):
    def deliver(self, text, foreground=111, send_enter=False):
        b = bridge.Bridge()
        bridge.USER32.GetForegroundWindow.return_value = foreground
        with patch.object(bridge.keyboard, 'is_pressed', return_value=False), \
             patch.object(bridge.keyboard, 'send') as send, patch.object(bridge.pyperclip, 'copy') as copy, \
             patch.object(bridge.time, 'sleep'):
            try:
                result = b.deliver(text, TERMINAL, send_enter)
            except ValueError as exc:
                result = exc
        return b, result, send, copy

    def test_success_pastes_without_enter(self):
        b, result, send, copy = self.deliver('Deneme\nTürkçe')
        self.assertTrue(result['ok'])
        copy.assert_called_once_with('Deneme Türkçe')
        send.assert_called_once_with('ctrl+shift+v')
        self.assertEqual(b.snapshot()['transfer']['stage'], 'sent')

    def test_enter_variant_sends_enter_after_paste(self):
        b, result, send, _ = self.deliver('ls', send_enter=True)
        self.assertEqual([c.args[0] for c in send.call_args_list], ['ctrl+shift+v', 'enter'])
        self.assertTrue(b.snapshot()['transfer']['enter_sent'])

    def test_focus_failure_leaves_text_on_clipboard_without_keys(self):
        b, result, send, copy = self.deliver('Deneme', foreground=222)
        self.assertIsInstance(result, ValueError)
        copy.assert_called_once_with('Deneme')
        send.assert_not_called()
        state = b.snapshot()['transfer']
        self.assertEqual((state['stage'], state['copied'], state['paste_sent']), ('error', True, False))

    def test_empty_transcript_does_not_touch_clipboard(self):
        b, result, send, copy = self.deliver('   ')
        self.assertIn('Ses algılanmadı', str(result))
        copy.assert_not_called()
        send.assert_not_called()


class ServerTests(unittest.TestCase):
    def test_server_rejects_unauthorized_web_origins_and_routes(self):
        fake = MagicMock()
        fake.status.return_value = {'connected': False, 'phase': 'idle'}
        fake.poll.return_value = {'command': None}
        fake.report.return_value = {'ok': True}
        server = bridge.Server(('127.0.0.1', 0), bridge.handler_for(fake, 'test-token'))
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = 'http://127.0.0.1:' + str(server.server_port)
        try:
            for headers in ({}, {'Authorization': 'Bearer wrong'},
                            {'Authorization': 'Bearer test-token', 'Origin': 'https://chatgpt.com'}):
                with self.assertRaises(HTTPError) as error:
                    urlopen(Request(base + '/status', headers=headers), timeout=2)
                self.assertEqual(error.exception.code, 403)
            headers = {'Authorization': 'Bearer test-token', 'Origin': 'chrome-extension://test',
                       'Content-Type': 'application/json'}
            with urlopen(Request(base + '/status', headers=headers), timeout=2) as response:
                self.assertFalse(json.load(response)['connected'])
            with urlopen(Request(base + '/poll?client=tab-aaaaaaaa', headers=headers), timeout=2) as response:
                self.assertEqual(json.load(response), {'command': None})
            fake.poll.assert_called_once_with('tab-aaaaaaaa', version='')
            body = json.dumps({'client': 'tab-aaaaaaaa', 'session': 1, 'event': 'result', 'text': 'x'}).encode()
            with urlopen(Request(base + '/report', data=body, headers=headers), timeout=2) as response:
                self.assertTrue(json.load(response)['ok'])
        finally:
            server.shutdown()
            server.server_close()


if __name__ == '__main__':
    unittest.main()
