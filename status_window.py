"""One live status window; all Tk operations stay on the main thread."""
import queue
import tkinter as tk
from clipboard_badge import ClipboardBadge

PHASE_TEXT = {'idle': 'Hazır', 'starting': 'Dikte başlatılıyor…', 'listening': 'Dinliyor',
              'transcribing': 'ChatGPT yazıya döküyor…'}


class StatusWindow:
    def __init__(self, bridge, on_quit):
        self.bridge = bridge
        self.on_quit = on_quit
        self.commands = queue.Queue()
        self.root = tk.Tk()
        self.root.title('ChatGPT Dikte — Durum')
        self.root.geometry('440x450')
        self.root.minsize(440, 450)
        self.root.configure(bg='#f4f7f5')
        self.root.protocol('WM_DELETE_WINDOW', self.root.withdraw)
        self.root.bind('<Escape>', lambda event: self.root.withdraw())
        self.values = {}
        panel = tk.Frame(self.root, bg='#f4f7f5', padx=20, pady=16)
        panel.pack(fill='both', expand=True)
        for key, size, weight in [('heading', 16, 'bold'), ('connection', 10, 'normal'),
                                  ('transfer', 12, 'bold'), ('checks', 10, 'normal'),
                                  ('details', 10, 'normal')]:
            self.values[key] = tk.Label(panel, text='', bg='#f4f7f5', fg='#233a2d',
                                       font=('Segoe UI', size, weight), anchor='w',
                                       justify='left', wraplength=395)
            self.values[key].pack(fill='x', pady=(0, 10))
        tk.Label(panel, text='Ctrl+Alt+Space — dikteyi başlat / bitir ve yapıştır\n'
                             'Ctrl+Alt+Enter — bitir, yapıştır ve Enter gönder\n'
                             'Ctrl+Alt+Backspace — iptal\n'
                             'Metin, bitirdiğin anda odakta olan pencereye gider.',
                 bg='#f4f7f5', fg='#58645d', font=('Segoe UI', 10), justify='left', anchor='w').pack(fill='x', pady=(4, 12))
        buttons = tk.Frame(panel, bg='#f4f7f5')
        buttons.pack(side='bottom', fill='x')
        tk.Button(buttons, text='Gizle', command=self.root.withdraw, padx=20).pack(side='left')
        tk.Button(buttons, text='Uygulamayı kapat', command=on_quit, padx=12).pack(side='right')
        self.badge = ClipboardBadge(self.root)
        self.last = None
        self.refresh()
        self.root.withdraw()

    def request_show(self):
        self.commands.put('show')

    def request_quit(self):
        self.commands.put('quit')

    def render(self, state):
        self.badge.render(state)
        transfer = state['transfer']
        busy = state['phase'] != 'idle'
        self.values['heading'].configure(text='● ' + PHASE_TEXT[state['phase']] if state['connected'] or busy else '● ChatGPT bağlı değil',
                                         fg='#c62828' if busy else '#16734a' if state['connected'] else '#9c5f0b')
        self.values['connection'].configure(
            text='ChatGPT sekmesi bağlı.' if state['connected'] else
            'Chrome\'da chatgpt.com sekmesini açın (arka planda kalabilir). Eklenti yüklü olmalı.')
        labels = {'idle': 'Aktarım bekleniyor', 'working': 'Aktarılıyor…',
                  'sent': 'Yapıştırıldı', 'error': 'Aktarım tamamlanamadı'}
        self.values['transfer'].configure(text=labels[transfer['stage']],
                                          fg='#a52a2a' if transfer['stage'] == 'error' else '#233a2d')
        copied = '✓ Metin panoya kopyalandı' if transfer['copied'] else '— Panoya kopyalama bekleniyor'
        sent = '✓ Yapıştırma kısayolu gönderildi' if transfer['paste_sent'] else '— Yapıştırma kısayolu gönderilmedi'
        if transfer.get('enter_sent'):
            sent += '\n✓ Enter gönderildi'
        self.values['checks'].configure(text=copied + '\n' + sent)
        details = (str(transfer['characters']) + ' karakter • ' + transfer['time']) if transfer['time'] else 'Henüz metin aktarılmadı.'
        if state['target']:
            details += '\nHedef: ' + state['target'][:120]
        if state['message']:
            details += '\n' + state['message']
        elif transfer['error']:
            details += '\n' + transfer['error']
        self.values['details'].configure(text=details)

    def refresh(self):
        while not self.commands.empty():
            command = self.commands.get_nowait()
            if command == 'quit':
                self.root.quit()
                return
            self.root.deiconify()
            self.root.lift()
        state = self.bridge.snapshot()
        if state != self.last:
            self.render(state)
            self.last = state
        else:
            self.badge.render(state)  # süreli "YAPIŞTIRILDI"/"HATA" gösterimi kendiliğinden sönsün
        self.root.after(200, self.refresh)
