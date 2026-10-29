// chatgpt.com içinde çalışır: köprüden komut bekler, ChatGPT'nin kendi dikte düğmelerine basar.
// Mesaj asla gönderilmez: "Transcribe and send" düğmesine dokunulmaz.
(() => {
  // Eklenti yeniden yüklenince eski betiğin bağlamı geçersizleşir; o durumda yenisi devralır.
  if (window.__dikteAlive?.()) return;
  window.__dikteAlive = () => !!chrome.runtime?.id;

  const CLIENT = crypto.randomUUID();
  const LABELS = {
    dictate: ['dictate', 'dikte et', 'dikte'],
    stop: ['stop dictation', 'dikteyi durdur', 'dikteyi bitir'],
    cancel: ['cancel dictation', 'cancel transcription', 'dikteyi iptal et', 'yazıya dökmeyi iptal et'],
    busy: ['cancel transcription', 'yazıya dökmeyi iptal et']
  };

  let active = null; // {session, baseline, started: Promise, cancelled: Promise, cancelNow}
  const MIC_DENIED = 'ChatGPT mikrofon izni reddedilmiş. Chrome adres çubuğundaki site ayarlarından chatgpt.com için Mikrofon: İzin ver.';
  const PERMISSION_WAIT = 60000;

  const sleep = ms => new Promise(r => setTimeout(r, ms));

  function findButton(kind) {
    const wanted = LABELS[kind];
    for (const b of document.querySelectorAll('form button[aria-label]')) {
      const label = b.getAttribute('aria-label').trim().toLowerCase();
      if (wanted.includes(label) && b.getClientRects().length) return b;
    }
    return null;
  }

  function buttonLabels() {
    return [...document.querySelectorAll('form button[aria-label]')]
      .map(b => b.getAttribute('aria-label') + (b.getClientRects().length ? '' : '(gizli)')).join('|');
  }

  function editor() {
    const form = findButton('dictate')?.closest('form') || findButton('stop')?.closest('form') || document;
    return form.querySelector('.ProseMirror[contenteditable="true"]') ||
      form.querySelector('[contenteditable="true"]') || form.querySelector('textarea');
  }

  function editorText() {
    const e = editor();
    if (!e) return '';
    return (e instanceof HTMLTextAreaElement ? e.value : e.innerText).trim();
  }

  // Sekme arka plandayken zamanlayıcılar kısılır; DOM değişimini MutationObserver ile bekle.
  function waitFor(check, timeout) {
    return new Promise(resolve => {
      const first = check();
      if (first) return resolve(first);
      const observer = new MutationObserver(() => {
        const value = check();
        if (value) { cleanup(); resolve(value); }
      });
      const timer = setTimeout(() => { cleanup(); resolve(check() || null); }, timeout);
      function cleanup() { observer.disconnect(); clearTimeout(timer); }
      observer.observe(document.body, {subtree: true, childList: true, attributes: true, characterData: true});
    });
  }

  // Metin 400 ms boyunca değişmeyene kadar bekle (en fazla 3 sn).
  function waitStable() {
    return new Promise(resolve => {
      let last = editorText();
      let quiet;
      const done = () => { observer.disconnect(); clearTimeout(quiet); clearTimeout(cap); resolve(editorText()); };
      const observer = new MutationObserver(() => {
        const now = editorText();
        if (now !== last) { last = now; clearTimeout(quiet); quiet = setTimeout(done, 400); }
      });
      quiet = setTimeout(done, 400);
      const cap = setTimeout(done, 3000);
      observer.observe(document.body, {subtree: true, childList: true, characterData: true});
    });
  }

  function clearEditor(baseline) {
    const e = editor();
    if (!e) return;
    try {
      if (e instanceof HTMLTextAreaElement) {
        e.value = baseline;
        e.dispatchEvent(new Event('input', {bubbles: true}));
        return;
      }
      e.focus({preventScroll: true});
      const range = document.createRange();
      range.selectNodeContents(e);
      const sel = getSelection();
      sel.removeAllRanges();
      sel.addRange(range);
      if (baseline) document.execCommand('insertText', false, baseline);
      else document.execCommand('delete');
    } catch (_) { /* temizlenemezse bir sonraki oturum farkı alır */ }
  }

  // chatgpt.com'un mikrofon izni: 'granted' | 'prompt' | 'denied' | 'unknown'.
  let micStatus = null;
  async function micPermission() {
    try {
      micStatus ??= await navigator.permissions.query({name: 'microphone'});
      return micStatus.state;
    } catch (_) {
      return 'unknown';
    }
  }

  // İzin sorusu yanıtlanana (ya da süre dolana) kadar bekler; son durumu döner.
  async function waitPermission(timeout) {
    if ((await micPermission()) !== 'prompt') return micStatus?.state || 'unknown';
    return new Promise(resolve => {
      const done = () => { micStatus.removeEventListener('change', changed); clearTimeout(timer); resolve(micStatus.state); };
      const changed = () => { if (micStatus.state !== 'prompt') done(); };
      const timer = setTimeout(done, timeout);
      micStatus.addEventListener('change', changed);
    });
  }

  async function send(payload) {
    try { await chrome.runtime.sendMessage({type: 'report', client: CLIENT, ...payload}); } catch (_) {}
  }

  const log = (...args) => {
    console.info('[Dikte]', ...args);
    // Eklenti yeniden yüklenince bu eski betikte sendMessage eşzamanlı hata fırlatır
    // ("Extension context invalidated"); .catch yalnızca reddedilen sözü yakalar.
    if (!chrome.runtime?.id) return;
    try {
      chrome.runtime.sendMessage({type: 'trace', client: CLIENT, msg: args.join(' ')}).catch(() => {});
    } catch (_) {}
  };

  // Dikteyi bitirip metni köprüye bildirir. Kısayolla (clickStop=true) ya da kullanıcı ChatGPT'de
  // kare düğmesine kendisi bastığında (clickStop=false) çağrılır; ikisi de aynı sonucu verir.
  async function finish(session, baseline, clickStop) {
    if (active) active.finishing = true;
    if (clickStop) {
      const stopButton = findButton('stop');
      if (stopButton) stopButton.click();
      else if (!findButton('busy')) log('durdurma düğmesi yok; dikte zaten bitmiş');
    }
    log('yazıya dökme bekleniyor');
    const finished = await waitFor(() => findButton('dictate') && !findButton('stop') && !findButton('busy'), 110000);
    active = null;
    log('yazıya dökme ' + (finished ? 'bitti' : 'zaman aşımı') + '; düğmeler: ' + buttonLabels());
    if (!finished) return send({session, event: 'error', error: 'ChatGPT metni yazıya dökemedi (zaman aşımı).'});
    const full = await waitStable();
    const text = (full.startsWith(baseline) ? full.slice(baseline.length) : full).trim();
    clearEditor(baseline);
    log('sonuç', text.length, 'karakter');
    send({session, event: 'result', text});
  }

  // Kullanıcı kare (durdur) düğmesine elle basarsa da metni aktar.
  function watchManualStop(session) {
    const observer = new MutationObserver(() => {
      if (!active || active.session !== session) return observer.disconnect();
      if (active.finishing || findButton('stop')) return;
      observer.disconnect();
      log('dikte ChatGPT içinde durduruldu');
      finish(session, active.baseline, false);
    });
    observer.observe(document.body, {subtree: true, childList: true, attributes: true});
  }

  async function start(session) {
    log('başlat', session);
    if (findButton('stop') || findButton('busy')) {
      // Bu sekmede zaten dikte sürüyor (ör. önceki başlatma bildirimi kayboldu): ikinci basış onu bitirir.
      log('dikte zaten sürüyor; bitiriliyor');
      return finish(session, active ? active.baseline : '', true);
    }
    const button = findButton('dictate');
    if (!button) {
      return send({session, event: 'error', error: 'ChatGPT dikte düğmesi bulunamadı. Sekmeyi yenileyin ve giriş yapın.'});
    }
    const mic = await micPermission();
    if (mic === 'denied') return send({session, event: 'error', error: MIC_DENIED});
    const baseline = editorText();
    let resolveStarted, cancelNow;
    active = {
      session, baseline, finishing: false,
      started: new Promise(r => { resolveStarted = r; }),
      cancelled: new Promise(r => { cancelNow = r; })
    };
    active.cancelNow = cancelNow;
    button.click();
    if (mic === 'prompt') {
      // Tarayıcı izin soruyor: ChatGPT kare düğmesini gösterse bile henüz dinlemiyor. Köprüye
      // "izin bekleniyor" denir; sekme öne getirilir (arka plandaki sekmede soru görünmez).
      log('mikrofon izni bekleniyor');
      send({session, event: 'permission'});
      chrome.runtime.sendMessage({type: 'focus'}).catch(() => {});
      const answer = await Promise.race([waitPermission(PERMISSION_WAIT), active.cancelled.then(() => 'cancelled')]);
      log('mikrofon izni: ' + answer);
      if (answer !== 'granted') {
        resolveStarted(false);
        active = null;
        if (answer === 'cancelled') return;
        (findButton('cancel') || findButton('stop'))?.click();
        return send({session, event: 'error', error: answer === 'denied' ? MIC_DENIED : 'Mikrofon izni verilmedi (süre doldu). ChatGPT sekmesinde izin verip yeniden deneyin.'});
      }
    }
    const ok = await waitFor(() => findButton('stop'), 7000);
    resolveStarted(!!ok);
    log('başlatma sonucu: ' + !!ok + '; düğmeler: ' + buttonLabels());
    if (!ok) {
      active = null;
      return send({session, event: 'error', error: 'Dikte başlamadı. ChatGPT sekmesinde mikrofon iznini kontrol edin.'});
    }
    watchManualStop(session);
    log('dinliyor');
    send({session, event: 'started'});
  }

  async function stop(session) {
    log('durdur', session);
    if (active && active.session === session) {
      if (!(await active.started)) return;
      if (active?.finishing) return; // elle durduruldu; sonuç zaten yolda
      return finish(session, active.baseline, true);
    }
    if (findButton('stop') || findButton('busy')) return finish(session, active ? active.baseline : '', true);
    send({session, event: 'error', error: 'Bu sekmede aktif dikte yok.'});
  }

  async function cancel(session) {
    log('iptal', session);
    if (active && active.session !== session) return; // eski oturumun iptali yenisine dokunmasın
    active?.cancelNow?.(); // izin bekleniyorsa beklemeyi bırak
    if (active) await active.started;
    active = null;
    (findButton('cancel') || findButton('busy'))?.click();
  }

  async function loop() {
    while (true) {
      let reply;
      try {
        reply = await chrome.runtime.sendMessage({type: 'poll', client: CLIENT, mic: await micPermission()});
      } catch (_) {
        if (!chrome.runtime?.id) return; // eklenti yeniden yüklendi; bu betik emekliye ayrıldı
        await sleep(2000);
        continue;
      }
      if (!reply?.ok || !reply.command) continue; // arka plan hata durumunda zaten bekletir
      const {command, session} = reply;
      if (command === 'start') start(session);
      else if (command === 'stop') stop(session);
      else if (command === 'cancel') cancel(session);
    }
  }

  // İzin değişince (verildi / reddedildi / sıfırlandı) köprü hemen bilsin; durum penceresinde görünür.
  micPermission().then(() => micStatus?.addEventListener('change', () => {
    log('mikrofon izni değişti: ' + micStatus.state);
    send({event: 'mic', state: micStatus.state});
  }));

  log('betik yüklendi ' + chrome.runtime.getManifest().version + '; düğmeler: ' + buttonLabels());
  loop();
})();
