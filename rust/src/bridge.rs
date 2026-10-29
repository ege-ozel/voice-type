//! Dikte durum makinesi: kısayollar, ChatGPT sekmesiyle komut kuyruğu ve yapıştırma.
//!
//! Windows'a özgü her şey `Platform` arkasındadır; böylece bu modül her yerde test edilir.
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::hotkey::{Hotkeys, Slot};
use crate::text::{is_terminal, normalize_text, paste_chord, prefix, Chord, Window};

pub const POLL_TIMEOUT: Duration = Duration::from_secs(20);
pub const CLIENT_TTL: Duration = Duration::from_secs(20 + 15);
pub const START_TIMEOUT: Duration = Duration::from_secs(8);
/// ChatGPT sekmesi mikrofon izni beklerken (eklenti 60 sn bekler; bu, sekme kaybolursa diye yedek).
pub const PERMISSION_TIMEOUT: Duration = Duration::from_secs(70);
pub const RESULT_TIMEOUT: Duration = Duration::from_secs(120);
/// API modunda kayıt bu süreden sonra kendiliğinden bitirilir (OpenAI dosya sınırı 25 MB).
pub const MAX_RECORDING: Duration = Duration::from_secs(10 * 60);
const STUCK_AFTER: Duration = Duration::from_secs(5);

/// Bu modülün dış dünyaya dokunduğu her yer.
pub trait Platform: Send + Sync + 'static {
    fn foreground_window(&self) -> Window;
    fn foreground_hwnd(&self) -> isize;
    fn is_window(&self, hwnd: isize) -> bool;
    /// Pencereyi öne getirmeyi dener (simge durumundaysa geri yükler).
    fn activate(&self, hwnd: isize);
    fn modifiers_down(&self) -> bool;
    fn set_clipboard(&self, text: &str) -> Result<(), String>;
    fn send_chord(&self, chord: Chord) -> Result<(), String>;
    /// Engellemeden bip.
    fn beep(&self, freq: u32, ms: u32);
    /// Günlüğe bir satır; dikte edilen metin buraya asla verilmez.
    fn log(&self, line: &str);
    /// Yerel saat, "SS:DD:ss".
    fn clock(&self) -> String;
    /// `job`'u `delay` sonra başka bir iş parçacığında çalıştırır.
    fn after(&self, delay: Duration, job: Box<dyn FnOnce() + Send>);
    fn sleep(&self, d: Duration);
    /// API modu: mikrofon kaydını başlatır (anahtar yoksa ya da mikrofon açılamazsa hata).
    fn record_start(&self) -> Result<(), String>;
    /// Kaydı durdurur; WAV baytları. Kayıt yoksa None.
    fn record_stop(&self) -> Option<Vec<u8>>;
    /// WAV'ı OpenAI ile yazıya döker; engeller, `spawn` içinden çağrılır.
    fn transcribe(&self, wav: &[u8]) -> Result<String, String>;
    /// `job`'u hemen başka bir iş parçacığında çalıştırır.
    fn spawn(&self, job: Box<dyn FnOnce() + Send>);
}

/// Sesi kim yazıya döker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Engine {
    /// ChatGPT sekmesindeki dikte (eklenti üzerinden, ücretsiz).
    #[default]
    Browser,
    /// Mikrofon kaydı doğrudan OpenAI ses-metin API'sine (ücretli).
    Api,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Starting,
    /// ChatGPT sekmesi tarayıcının mikrofon izni sorusunun yanıtını bekliyor; henüz dinlemiyor.
    Permission,
    Listening,
    Transcribing,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Starting => "starting",
            Phase::Permission => "permission",
            Phase::Listening => "listening",
            Phase::Transcribing => "transcribing",
        }
    }
}

/// chatgpt.com'un tarayıcıdaki mikrofon izni (eklentinin bildirdiği; Permissions API durumu).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mic {
    #[default]
    Unknown,
    Granted,
    /// İlk dikte de tarayıcı soracak.
    Prompt,
    Denied,
}

impl Mic {
    pub fn parse(s: &str) -> Option<Mic> {
        match s {
            "granted" => Some(Mic::Granted),
            "prompt" => Some(Mic::Prompt),
            "denied" => Some(Mic::Denied),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Start,
    Stop,
    Cancel,
}

impl Command {
    pub fn name(self) -> &'static str {
        match self {
            Command::Start => "start",
            Command::Stop => "stop",
            Command::Cancel => "cancel",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Queued {
    command: Command,
    session: u64,
    /// None: yoklayan herhangi bir sekme alabilir.
    client: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Idle,
    Working,
    Sent,
    /// Yalnızca kopyalama modunda: metin panoda, tuş gönderilmedi.
    Copied,
    Error,
}

/// Son aktarımın adımları (durum penceresi ve rozet için).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transfer {
    pub stage: Stage,
    pub copied: bool,
    pub paste_sent: bool,
    pub enter_sent: bool,
    pub characters: usize,
    pub time: String,
    pub error: String,
    pub copied_at: Option<Instant>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub connected: bool,
    pub phase: Phase,
    pub phase_at: Option<Instant>,
    pub message: String,
    pub target: String,
    pub transfer: Transfer,
    /// Bitirince yapıştırmak yerine yalnızca panoya kopyala.
    pub copy_only: bool,
    pub engine: Engine,
    pub hotkeys: Hotkeys,
    pub mic: Mic,
}

struct State {
    phase: Phase,
    session: u64,
    /// Dikteyi yürüten ChatGPT sekmesinin istemci kimliği.
    owner: Option<String>,
    /// İstemci kimliği -> son görülme zamanı.
    clients: HashMap<String, Instant>,
    commands: Vec<Queued>,
    /// Metnin yapıştırılacağı pencere (durdurma anında odaktaki).
    target: Option<Window>,
    /// Elle durdurulursa: dikte başlarken odaktaki pencere.
    start_target: Option<Window>,
    send_enter: bool,
    message: String,
    phase_at: Option<Instant>,
    transfer: Transfer,
    /// Bitirince yalnızca panoya kopyala.
    copy_only: bool,
    engine: Engine,
    hotkeys: Hotkeys,
    mic: Mic,
}

impl State {
    fn connected(&self) -> bool {
        let now = Instant::now();
        self.clients.values().any(|seen| now.duration_since(*seen) < CLIENT_TTL)
    }

    fn set_phase(&mut self, phase: Phase, message: &str) {
        self.phase = phase;
        self.phase_at = Some(Instant::now());
        self.message = message.to_string();
        if phase == Phase::Idle {
            self.owner = None;
        }
    }

    fn enqueue(&mut self, command: Command, session: u64, client: Option<String>) {
        self.drop_stale(session);
        self.commands.push(Queued { command, session, client });
    }

    /// Eski oturuma ait, teslim edilmemiş komutlar artık geçersiz; iptaller hariç. Python sürümü
    /// iptali de siliyordu: iptalden hemen sonra yeniden başlatılınca sekme eski dikteyi sürdürüp
    /// yeni oturumun "start"ını "zaten sürüyor, bitir" diye yorumluyor ve iptal edilen metni yapıştırıyordu.
    fn drop_stale(&mut self, session: u64) {
        self.commands.retain(|c| c.session == session || c.command == Command::Cancel);
    }

    /// İptal: başlatma komutu henüz hiçbir sekmeye ulaşmadıysa onu (ve arkasındakileri) geri çeker.
    /// Python sürümü burada "start"ı kuyrukta bırakıp ardına "cancel" ekliyordu; sekme geri gelince
    /// mikrofon boşuna açılıyor ve sekme, durum boştayken "owner" oluyordu. O sekme sonra yenilenirse
    /// sonraki tüm başlatmalar başka sekmeye gidemiyor ve zaman aşımına düşüyordu.
    fn enqueue_cancel(&mut self, session: u64) {
        self.drop_stale(session);
        if self.commands.iter().any(|c| c.session == session && c.command == Command::Start) {
            self.commands.retain(|c| c.session != session);
        } else {
            let owner = self.owner.clone();
            self.commands.push(Queued { command: Command::Cancel, session, client: owner });
        }
    }
}

pub struct Bridge<P: Platform> {
    pub platform: P,
    state: Mutex<State>,
    cond: Condvar,
    transfer_lock: Mutex<()>,
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<P: Platform> Bridge<P> {
    pub fn new(platform: P) -> Arc<Self> {
        Arc::new(Bridge {
            platform,
            state: Mutex::new(State {
                phase: Phase::Idle,
                session: 0,
                owner: None,
                clients: HashMap::new(),
                commands: Vec::new(),
                target: None,
                start_target: None,
                send_enter: false,
                copy_only: false,
                engine: Engine::Browser,
                hotkeys: Hotkeys::default(),
                mic: Mic::Unknown,
                message: String::new(),
                phase_at: None,
                transfer: Transfer::default(),
            }),
            cond: Condvar::new(),
            transfer_lock: Mutex::new(()),
        })
    }

    fn st(&self) -> MutexGuard<'_, State> {
        locked(&self.state)
    }

    // --- durum -------------------------------------------------------------
    #[cfg(test)]
    pub fn connected(&self) -> bool {
        self.st().connected()
    }

    pub fn status(&self) -> Value {
        let st = self.st();
        json!({"connected": st.connected(), "phase": st.phase.name()})
    }

    pub fn snapshot(&self) -> Snapshot {
        let st = self.st();
        Snapshot {
            connected: st.connected(),
            phase: st.phase,
            phase_at: st.phase_at,
            message: st.message.clone(),
            target: st.target.as_ref().map(|t| t.title.clone()).unwrap_or_default(),
            transfer: st.transfer.clone(),
            copy_only: st.copy_only,
            engine: st.engine,
            hotkeys: st.hotkeys,
            mic: st.mic,
        }
    }

    pub fn set_hotkeys(&self, hotkeys: Hotkeys) {
        self.st().hotkeys = hotkeys;
        self.log(&format!("Kısayollar: {} başlat/bitir, {} bitir+Enter, {} iptal.", hotkeys.get(Slot::Toggle),
                          hotkeys.get(Slot::ToggleEnter), hotkeys.get(Slot::Cancel)));
    }

    fn toggle_name(&self) -> String {
        self.st().hotkeys.get(Slot::Toggle).to_string()
    }

    /// Eklentinin bildirdiği mikrofon izni; değişince günlüğe yazılır.
    pub fn set_mic(&self, state: &str) {
        let Some(mic) = Mic::parse(state) else { return };
        let changed = std::mem::replace(&mut self.st().mic, mic) != mic;
        if changed {
            self.log(&format!("ChatGPT mikrofon izni: {state}"));
        }
    }

    /// Yöntemi değiştirir; süren dikte varsa önce iptal edilir.
    pub fn set_engine(&self, engine: Engine) {
        if self.st().phase != Phase::Idle {
            self.cancel();
        }
        self.st().engine = engine;
        self.log(match engine {
            Engine::Browser => "Yöntem: ChatGPT sekmesi.",
            Engine::Api => "Yöntem: OpenAI API.",
        });
    }

    pub fn set_copy_only(&self, copy_only: bool) {
        self.st().copy_only = copy_only;
        self.log(if copy_only { "Mod: yalnızca panoya kopyala." } else { "Mod: yapıştır." });
    }

    #[cfg(test)]
    pub fn session(&self) -> u64 {
        self.st().session
    }

    fn set_phase(&self, phase: Phase, message: &str) {
        self.st().set_phase(phase, message);
        self.cond.notify_all();
    }

    fn progress(&self, f: impl FnOnce(&mut Transfer)) {
        f(&mut self.st().transfer);
    }

    fn fail(&self, message: &str) {
        self.set_phase(Phase::Idle, message);
        self.platform.beep(220, 180);
        self.log(message);
    }

    fn log(&self, line: &str) {
        self.platform.log(line);
    }

    // --- kısayollar --------------------------------------------------------
    pub fn toggle(self: &Arc<Self>, send_enter: bool) {
        let (phase, since) = {
            let st = self.st();
            (st.phase, st.phase_at)
        };
        self.log(&format!("kısayol ({})", phase.name()));
        match phase {
            Phase::Idle => self.start(),
            Phase::Starting | Phase::Listening => self.stop(send_enter),
            // Henüz kayıt yok; ikinci basış beklemeyi bırakır.
            Phase::Permission => self.cancel(),
            Phase::Transcribing => {
                // Sonuç gelmediyse ikinci basış takılmayı çözer.
                if since.is_some_and(|t| t.elapsed() > STUCK_AFTER) {
                    self.log("yazıya dökme sonucu gelmedi; sıfırlandı");
                    self.fail("ChatGPT'den metin gelmedi. Metin ChatGPT kutusunda kalmış olabilir.");
                } else {
                    self.platform.beep(330, 60); // yazıya dökülüyor; bekleyin
                }
            }
        }
    }

    pub fn start(self: &Arc<Self>) {
        if self.st().engine == Engine::Api {
            return self.api_start();
        }
        let start_target = self.platform.foreground_window();
        let session = {
            let mut st = self.st();
            if st.phase != Phase::Idle {
                return;
            }
            if !st.connected() {
                drop(st);
                self.fail("ChatGPT sekmesi bağlı değil. Chrome açık mı? Eklenti güncel değilse \
                           chrome://extensions sayfasında ↻ basın veya Chrome'u kapatıp açın.");
                return;
            }
            st.session += 1;
            let session = st.session;
            st.target = None;
            st.start_target = Some(start_target);
            st.send_enter = false;
            st.set_phase(Phase::Starting, "");
            st.enqueue(Command::Start, session, None);
            session
        };
        self.cond.notify_all();
        self.expire_after(START_TIMEOUT, session, Phase::Starting,
                          "Dikte başlamadı. ChatGPT sekmesinde mikrofon iznini kontrol edin.");
        // Sekme izin beklemeye geçerse START_TIMEOUT işlemez; bu yedek yalnızca o aşamada işler.
        self.expire_after(PERMISSION_TIMEOUT, session, Phase::Permission,
                          "Mikrofon izni verilmedi. ChatGPT sekmesinde mikrofona izin verip yeniden deneyin.");
        self.log("Dikte başlatılıyor…");
    }

    pub fn stop(self: &Arc<Self>, send_enter: bool) {
        if self.st().engine == Engine::Api {
            return self.api_stop(send_enter);
        }
        let target = self.platform.foreground_window();
        let title = target.title.clone();
        let session = {
            let mut st = self.st();
            if !matches!(st.phase, Phase::Starting | Phase::Listening) {
                return;
            }
            let session = st.session;
            st.target = Some(target);
            st.send_enter = send_enter;
            st.set_phase(Phase::Transcribing, "");
            let owner = st.owner.clone();
            st.enqueue(Command::Stop, session, owner);
            session
        };
        self.cond.notify_all();
        self.platform.beep(660, 70);
        self.expire_after(RESULT_TIMEOUT, session, Phase::Transcribing,
                          "ChatGPT yanıt vermedi. Dikteyi ChatGPT sekmesinde kontrol edin.");
        self.log(&format!("Dikte durduruldu, metin bekleniyor → {title}"));
    }

    pub fn cancel(&self) {
        let recording = {
            let mut st = self.st();
            if st.phase == Phase::Idle {
                return;
            }
            let api = st.engine == Engine::Api;
            let recording = api && st.phase == Phase::Listening;
            if !api {
                let session = st.session;
                st.enqueue_cancel(session);
            }
            st.set_phase(Phase::Idle, "Dikte iptal edildi.");
            recording
        };
        self.cond.notify_all();
        if recording {
            let _ = self.platform.record_stop();
        }
        self.platform.beep(300, 120);
        self.log("Dikte iptal edildi.");
    }

    fn expire_after(self: &Arc<Self>, delay: Duration, session: u64, phase: Phase, message: &'static str) {
        let me = Arc::clone(self);
        self.platform.after(delay, Box::new(move || me.expire(session, phase, message)));
    }

    pub fn expire(&self, session: u64, phase: Phase, message: &str) {
        {
            let mut st = self.st();
            if st.session != session || st.phase != phase {
                return;
            }
            st.enqueue_cancel(session);
        }
        self.fail(message);
    }

    // --- API modu ---------------------------------------------------------
    fn api_start(self: &Arc<Self>) {
        let start_target = self.platform.foreground_window();
        let session = {
            let mut st = self.st();
            if st.phase != Phase::Idle {
                return;
            }
            st.session += 1;
            st.target = None;
            st.start_target = Some(start_target);
            st.send_enter = false;
            st.session
        };
        if let Err(error) = self.platform.record_start() {
            self.fail(&error);
            return;
        }
        self.set_phase(Phase::Listening, "");
        self.platform.beep(880, 90);
        self.log(&format!("Dinleniyor (OpenAI API)… Bitince {}.", self.toggle_name()));
        let me = Arc::clone(self);
        self.platform.after(MAX_RECORDING, Box::new(move || {
            let still = {
                let st = me.st();
                st.session == session && st.phase == Phase::Listening
            };
            if still {
                me.log("Kayıt süresi doldu; bitiriliyor.");
                me.api_stop(false);
            }
        }));
    }

    fn api_stop(self: &Arc<Self>, send_enter: bool) {
        let target = self.platform.foreground_window();
        let title = target.title.clone();
        let session = {
            let mut st = self.st();
            if st.phase != Phase::Listening {
                return;
            }
            st.target = Some(target);
            st.send_enter = send_enter;
            st.set_phase(Phase::Transcribing, "");
            st.session
        };
        self.cond.notify_all();
        self.platform.beep(660, 70);
        let Some(wav) = self.platform.record_stop().filter(|w| w.len() >= crate::openai::MIN_WAV) else {
            self.fail("Kayıt çok kısa ya da boş; mikrofonu kontrol edin.");
            return;
        };
        let seconds = (wav.len() - 44) as f32 / (crate::openai::SAMPLE_RATE * 2) as f32;
        self.log(&format!("Kayıt bitti ({seconds:.1} sn), OpenAI'ye gönderiliyor → {title}"));
        let me = Arc::clone(self);
        self.platform.spawn(Box::new(move || {
            let result = me.platform.transcribe(&wav);
            me.api_result(session, result);
        }));
    }

    fn api_result(&self, session: u64, result: Result<String, String>) {
        let (target, send_enter) = {
            let mut st = self.st();
            if st.session != session || st.phase != Phase::Transcribing {
                drop(st);
                self.log("OpenAI sonucu geç geldi (iptal edilmiş); atlandı.");
                return;
            }
            let pending = (st.target.clone(), st.send_enter);
            st.set_phase(Phase::Idle, "");
            pending
        };
        self.cond.notify_all();
        match result {
            Ok(text) => {
                let _ = self.finish(Some(&text), target, send_enter);
            }
            Err(error) => self.fail(&error),
        }
    }

    /// Yazıya dökülen metni teslim eder; hata durumunu gösterir ve günlüğe yazar.
    fn finish(&self, text: Option<&str>, target: Option<Window>, send_enter: bool) -> Result<usize, String> {
        self.deliver(text, target, send_enter).inspect_err(|error| {
            self.st().message = error.clone();
            self.platform.beep(220, 180);
            self.log(error);
        })
    }

    // --- eklenti iletişimi -------------------------------------------------
    /// Uzun yoklama: komut gelene ya da `timeout` dolana kadar bekler.
    pub fn poll(&self, client: &str, timeout: Duration, version: &str) -> Result<Option<(Command, u64)>, String> {
        let len = client.chars().count();
        if !(8..=64).contains(&len) {
            return Err("İstemci kimliği geçersiz.".into());
        }
        let deadline = Instant::now() + timeout;
        let mut st = self.st();
        if !st.clients.contains_key(client) {
            let version = if version.is_empty() { "eski sürüm" } else { prefix(version, 20) };
            self.log(&format!("sekme bağlandı: {} (eklenti {version})", prefix(client, 8)));
        }
        let now = Instant::now();
        st.clients.insert(client.to_string(), now);
        // Uzun süredir görünmeyen sekmeleri unut.
        st.clients.retain(|_, seen| now.duration_since(*seen) < CLIENT_TTL * 2);
        loop {
            let owner_ok = |st: &State, c: &Queued| {
                st.owner.as_deref().is_none_or(|o| o == client) || c.command == Command::Cancel
            };
            let found = st.commands.iter().position(|c| {
                c.client.as_deref().is_none_or(|t| t == client) && owner_ok(&st, c)
            });
            if let Some(i) = found {
                let item = st.commands.remove(i);
                if item.command == Command::Start {
                    st.owner = Some(client.to_string());
                }
                st.clients.insert(client.to_string(), Instant::now());
                return Ok(Some((item.command, item.session)));
            }
            let now = Instant::now();
            if now >= deadline {
                st.clients.insert(client.to_string(), now);
                return Ok(None);
            }
            st = self.cond.wait_timeout(st, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    pub fn report(&self, client: Option<&str>, data: &Value) -> Value {
        let event = data.get("event").and_then(Value::as_str);
        let session = data.get("session");
        let shown = |v: Option<&Value>| match v {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => "None".into(),
        };
        let error = data.get("error").and_then(Value::as_str).filter(|e| !e.is_empty());
        // İzin durumu oturumdan bağımsızdır (sekme izni değişince kendiliğinden bildirir).
        if event == Some("mic") {
            self.set_mic(data.get("state").and_then(Value::as_str).unwrap_or(""));
            return json!({"ok": true});
        }
        let mut line = format!("eklenti: {} (oturum {})", shown(data.get("event")), shown(session));
        if event == Some("error") {
            line += &format!(" — {}", error.unwrap_or("None"));
        }
        self.log(&line);

        let mut st = self.st();
        let current = session.and_then(Value::as_u64) == Some(st.session);
        if !current || client.is_none() || client != st.owner.as_deref() {
            return json!({"ok": false, "stale": true});
        }
        match event {
            Some("permission") if st.phase == Phase::Starting => {
                st.set_phase(Phase::Permission, "");
                st.mic = Mic::Prompt;
                drop(st);
                self.cond.notify_all();
                self.log("ChatGPT sekmesi mikrofon iznini bekliyor; Chrome'da izin verin.");
            }
            Some("started") if matches!(st.phase, Phase::Starting | Phase::Permission) => {
                st.set_phase(Phase::Listening, "");
                st.mic = Mic::Granted;
                let toggle = st.hotkeys.get(Slot::Toggle);
                drop(st);
                self.cond.notify_all();
                self.platform.beep(880, 90);
                self.log(&format!("Dinleniyor… Bitince {toggle}."));
            }
            Some("error") if st.phase != Phase::Idle => {
                drop(st);
                self.fail(error.unwrap_or("ChatGPT dikte hatası."));
            }
            Some("result") if st.phase != Phase::Idle => {
                // 'transcribing' dışında gelirse kullanıcı ChatGPT'de kareye elle basmıştır.
                let target = st.target.clone().or_else(|| st.start_target.clone());
                let send_enter = st.send_enter;
                st.set_phase(Phase::Idle, "");
                drop(st);
                self.cond.notify_all();
                let text = data.get("text").and_then(Value::as_str);
                return match self.finish(text, target, send_enter) {
                    Ok(characters) => json!({"ok": true, "characters": characters}),
                    Err(error) => json!({"ok": false, "error": error}),
                };
            }
            _ => {}
        }
        json!({"ok": true})
    }

    // --- yapıştırma --------------------------------------------------------
    pub fn deliver(&self, raw: Option<&str>, target: Option<Window>, send_enter: bool) -> Result<usize, String> {
        let _one_at_a_time = locked(&self.transfer_lock);
        let time = self.platform.clock();
        self.progress(|t| *t = Transfer { stage: Stage::Working, time, ..Transfer::default() });
        let result = self.deliver_inner(raw, target, send_enter);
        if let Err(error) = &result {
            self.progress(|t| {
                t.stage = Stage::Error;
                t.error = error.clone();
            });
        }
        result
    }

    fn deliver_inner(&self, raw: Option<&str>, target: Option<Window>, send_enter: bool) -> Result<usize, String> {
        let p = &self.platform;
        // Satır sonları yalnızca terminale yapıştırırken silinir; panoya kopyalarken ve başka her yerde kalır.
        let keep_lines = self.st().copy_only || !target.as_ref().is_some_and(is_terminal);
        let text = normalize_text(raw, keep_lines)?;
        let characters = text.chars().count();
        self.progress(|t| t.characters = characters);
        // Kısayolun Ctrl/Alt'ı hâlâ basılıysa yapıştırma başka bir kısayola dönüşür.
        let deadline = Instant::now() + Duration::from_secs(3);
        while p.modifiers_down() && Instant::now() < deadline {
            p.sleep(Duration::from_millis(20));
        }
        p.set_clipboard(&text)?;
        self.progress(|t| {
            t.copied = true;
            t.copied_at = Some(Instant::now());
        });
        if self.st().copy_only {
            self.progress(|t| t.stage = Stage::Copied);
            p.beep(1200, 50);
            self.log(&format!("Panoya kopyalandı ({characters} karakter)."));
            return Ok(characters);
        }
        let target = match target {
            Some(t) if p.is_window(t.hwnd) => t,
            _ => return Err("Hedef pencere kapandı. Metin panoda; Ctrl+V ile yapıştırın.".into()),
        };
        if p.foreground_hwnd() != target.hwnd {
            p.activate(target.hwnd);
            for _ in 0..20 {
                if p.foreground_hwnd() == target.hwnd {
                    break;
                }
                p.sleep(Duration::from_millis(25));
            }
            if p.foreground_hwnd() != target.hwnd {
                return Err("Windows pencere geçişine izin vermedi. Metin panoda; Ctrl+V ile yapıştırın.".into());
            }
            p.sleep(Duration::from_millis(80));
        }
        p.send_chord(paste_chord(&target))?;
        self.progress(|t| {
            t.stage = Stage::Sent;
            t.paste_sent = true;
        });
        let mut entered = false;
        if send_enter {
            // Terminal ve web editörleri pano metnini asenkron işleyebilir.
            p.sleep(Duration::from_millis(600));
            if p.foreground_hwnd() == target.hwnd {
                if p.modifiers_down() {
                    return Err("Metin yapıştırıldı; Ctrl/Alt/Shift/Win basılı olduğu için Enter gönderilmedi.".into());
                }
                p.send_chord(Chord::Enter)?;
                self.progress(|t| t.enter_sent = true);
                entered = true;
            } else {
                return Err("Metin yapıştırıldı; hedef pencere değiştiği için Enter gönderilmedi.".into());
            }
        }
        p.beep(1200, 50);
        self.log(&format!("Yapıştırıldı ({characters} karakter){}", if entered { " + Enter." } else { "." }));
        Ok(characters)
    }
}

/// HTTP sunucusunun gördüğü yüz; testte sahte bir uygulama takılabilir.
pub trait Api: Send + Sync + 'static {
    fn status(&self) -> Value;
    /// `mic`: sekmenin bildirdiği mikrofon izni (boş olabilir).
    fn poll(&self, client: &str, version: &str, mic: &str) -> Result<Value, String>;
    fn report(&self, client: Option<&str>, data: &Value) -> Value;
    fn trace(&self, client: &str, msg: &str);
}

impl<P: Platform> Api for Bridge<P> {
    fn status(&self) -> Value {
        Bridge::status(self)
    }

    fn poll(&self, client: &str, version: &str, mic: &str) -> Result<Value, String> {
        if !mic.is_empty() {
            self.set_mic(mic);
        }
        Ok(match Bridge::poll(self, client, POLL_TIMEOUT, version)? {
            Some((command, session)) => json!({"command": command.name(), "session": session}),
            None => json!({"command": null}),
        })
    }

    fn report(&self, client: Option<&str>, data: &Value) -> Value {
        Bridge::report(self, client, data)
    }

    fn trace(&self, client: &str, msg: &str) {
        self.log(&format!("iz {}: {}", prefix(client, 8), prefix(msg, 300)));
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::thread;

    pub fn terminal() -> Window {
        Window { hwnd: 111, title: "Claude Code".into(), class: "CASCADIA_HOSTING_WINDOW_CLASS".into(),
                 exe: "windowsterminal.exe".into() }
    }

    /// Bip, zamanlayıcı ve gerçek pencere çağrılarını kaydeden sahte platform.
    #[derive(Default)]
    pub struct Fake {
        pub foreground: Mutex<isize>,
        pub alive: Mutex<bool>,
        pub clipboard: Mutex<Vec<String>>,
        pub chords: Mutex<Vec<Chord>>,
        pub fail_chord: Mutex<Option<Chord>>,
        pub beeps: Mutex<Vec<u32>>,
        pub logs: Mutex<Vec<String>>,
        pub timers: Mutex<Vec<Duration>>,
        /// API modu: record_start hatası, kayıt sonucu, yazıya dökme sonucu ve çağrı sayıları.
        pub record_error: Mutex<Option<String>>,
        pub recording: Mutex<bool>,
        pub wav: Mutex<Option<Vec<u8>>>,
        pub transcript: Mutex<Option<Result<String, String>>>,
        pub transcribed: Mutex<Vec<usize>>,
        /// spawn edilen işler; testte elle çalıştırılır.
        pub jobs: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    }

    impl Fake {
        pub fn new() -> Self {
            let f = Fake::default();
            *f.foreground.lock().unwrap() = 111;
            *f.alive.lock().unwrap() = true;
            f
        }
    }

    impl Platform for Fake {
        fn foreground_window(&self) -> Window {
            terminal()
        }
        fn foreground_hwnd(&self) -> isize {
            *self.foreground.lock().unwrap()
        }
        fn is_window(&self, _: isize) -> bool {
            *self.alive.lock().unwrap()
        }
        fn activate(&self, _: isize) {}
        fn modifiers_down(&self) -> bool {
            false
        }
        fn set_clipboard(&self, text: &str) -> Result<(), String> {
            self.clipboard.lock().unwrap().push(text.into());
            Ok(())
        }
        fn send_chord(&self, chord: Chord) -> Result<(), String> {
            if *self.fail_chord.lock().unwrap() == Some(chord) {
                return Err("Tuş gönderilemedi".into());
            }
            self.chords.lock().unwrap().push(chord);
            Ok(())
        }
        fn beep(&self, freq: u32, _: u32) {
            self.beeps.lock().unwrap().push(freq);
        }
        fn log(&self, line: &str) {
            self.logs.lock().unwrap().push(line.into());
        }
        fn clock(&self) -> String {
            "12:00:00".into()
        }
        fn after(&self, delay: Duration, _: Box<dyn FnOnce() + Send>) {
            self.timers.lock().unwrap().push(delay);
        }
        fn sleep(&self, _: Duration) {}
        fn record_start(&self) -> Result<(), String> {
            if let Some(error) = self.record_error.lock().unwrap().clone() {
                return Err(error);
            }
            *self.recording.lock().unwrap() = true;
            Ok(())
        }
        fn record_stop(&self) -> Option<Vec<u8>> {
            let was = std::mem::replace(&mut *self.recording.lock().unwrap(), false);
            if was { self.wav.lock().unwrap().clone() } else { None }
        }
        fn transcribe(&self, wav: &[u8]) -> Result<String, String> {
            self.transcribed.lock().unwrap().push(wav.len());
            self.transcript.lock().unwrap().clone().unwrap_or(Err("yok".into()))
        }
        fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
            self.jobs.lock().unwrap().push(job);
        }
    }

    impl Fake {
        /// Bekleyen arka plan işlerini (OpenAI isteği) çalıştırır.
        pub fn run_jobs(&self) {
            let jobs: Vec<_> = self.jobs.lock().unwrap().drain(..).collect();
            for job in jobs {
                job();
            }
        }
    }

    const A: &str = "tab-aaaaaaaa";
    const B: &str = "tab-bbbbbbbb";
    const NOW: Duration = Duration::ZERO;

    fn bridge() -> Arc<Bridge<Fake>> {
        Bridge::new(Fake::new())
    }

    fn poll(b: &Bridge<Fake>, client: &str) -> Option<(Command, u64)> {
        b.poll(client, NOW, "").unwrap()
    }

    fn event(session: u64, event: &str) -> Value {
        json!({"session": session, "event": event})
    }

    /// Sekme bağlanır, dikte başlar ve sekme "started" bildirir.
    fn listening(b: &Arc<Bridge<Fake>>) -> u64 {
        poll(b, A);
        b.toggle(false);
        let (cmd, session) = poll(b, A).unwrap();
        assert_eq!(cmd, Command::Start);
        b.report(Some(A), &event(session, "started"));
        assert_eq!(b.snapshot().phase, Phase::Listening);
        session
    }

    #[test]
    fn start_without_chatgpt_tab_fails_visibly() {
        let b = bridge();
        b.toggle(false);
        let s = b.snapshot();
        assert_eq!(s.phase, Phase::Idle);
        assert!(s.message.contains("ChatGPT sekmesi bağlı değil"));
        assert_eq!(*b.platform.beeps.lock().unwrap(), vec![220]);
    }

    #[test]
    fn full_cycle_pastes_into_window_focused_at_stop() {
        let b = bridge();
        assert_eq!(poll(&b, A), None);
        assert!(b.connected());
        b.toggle(false);
        assert_eq!(b.snapshot().phase, Phase::Starting);
        assert_eq!(*b.platform.timers.lock().unwrap(), vec![START_TIMEOUT, PERMISSION_TIMEOUT]);
        let (cmd, session) = poll(&b, A).unwrap();
        assert_eq!(cmd, Command::Start);
        b.report(Some(A), &event(session, "started"));
        assert_eq!(b.snapshot().phase, Phase::Listening);
        b.toggle(true);
        assert_eq!(b.snapshot().phase, Phase::Transcribing);
        assert_eq!(poll(&b, A), Some((Command::Stop, session)));
        let reply = b.report(Some(A), &json!({"session": session, "event": "result", "text": "merhaba\nworld"}));
        assert_eq!(reply, json!({"ok": true, "characters": 13}));
        assert_eq!(*b.platform.clipboard.lock().unwrap(), vec!["merhaba world".to_string()]);
        assert_eq!(*b.platform.chords.lock().unwrap(), vec![Chord::CtrlV, Chord::Enter]);
        assert_eq!(*b.platform.beeps.lock().unwrap(), vec![880, 660, 1200]);
        let s = b.snapshot();
        assert_eq!(s.phase, Phase::Idle);
        assert_eq!(s.target, "Claude Code");
        assert!(s.transfer.enter_sent);
        // Dikte edilen metin günlüğe asla yazılmaz.
        assert!(!b.platform.logs.lock().unwrap().iter().any(|l| l.contains("merhaba")));
    }

    #[test]
    fn manual_stop_in_chatgpt_pastes_into_start_window() {
        let b = bridge();
        let session = listening(&b);
        let reply = b.report(Some(A), &json!({"session": session, "event": "result", "text": "elle"}));
        assert_eq!(reply["ok"], true);
        assert_eq!(*b.platform.chords.lock().unwrap(), vec![Chord::CtrlV]); // Enter yok
        assert_eq!(b.snapshot().phase, Phase::Idle);
    }

    #[test]
    fn stop_goes_only_to_the_dictating_tab() {
        let b = bridge();
        listening(&b);
        b.toggle(false);
        assert_eq!(poll(&b, B), None);
        assert_eq!(poll(&b, A).unwrap().0, Command::Stop);
    }

    #[test]
    fn reports_from_other_tabs_or_old_sessions_are_ignored() {
        let b = bridge();
        poll(&b, A);
        b.toggle(false);
        let (_, session) = poll(&b, A).unwrap();
        let stale = json!({"ok": false, "stale": true});
        assert_eq!(b.report(Some(B), &event(session, "started")), stale);
        assert_eq!(b.report(Some(A), &event(session - 1, "started")), stale);
        assert_eq!(b.report(None, &event(session, "started")), stale);
        assert_eq!(b.report(Some(A), &json!({"session": "1", "event": "started"})), stale);
        assert_eq!(b.snapshot().phase, Phase::Starting);
    }

    #[test]
    fn chatgpt_error_returns_to_idle() {
        let b = bridge();
        poll(&b, A);
        b.toggle(false);
        let (_, session) = poll(&b, A).unwrap();
        b.report(Some(A), &json!({"session": session, "event": "error", "error": "Mikrofon izni yok"}));
        let s = b.snapshot();
        assert_eq!((s.phase, s.message.as_str()), (Phase::Idle, "Mikrofon izni yok"));
    }

    /// Tarayıcı izin sorarken rozet "dinliyor" demez; izin verilince dinleme başlar.
    #[test]
    fn permission_prompt_is_not_listening_until_granted() {
        let b = bridge();
        poll(&b, A);
        b.toggle(false);
        let (_, session) = poll(&b, A).unwrap();
        assert!(b.platform.timers.lock().unwrap().contains(&PERMISSION_TIMEOUT));
        b.report(Some(A), &event(session, "permission"));
        let s = b.snapshot();
        assert_eq!((s.phase, s.mic), (Phase::Permission, Mic::Prompt));
        assert!(b.platform.beeps.lock().unwrap().is_empty(), "dinleme bipi henüz yok");
        // Başlatma zaman aşımı bu aşamaya dokunmaz; izin yedeği dokunur.
        b.expire(session, Phase::Starting, "Dikte başlamadı.");
        assert_eq!(b.snapshot().phase, Phase::Permission);
        b.report(Some(A), &event(session, "started"));
        let s = b.snapshot();
        assert_eq!((s.phase, s.mic), (Phase::Listening, Mic::Granted));
        assert_eq!(*b.platform.beeps.lock().unwrap(), vec![880]);
    }

    #[test]
    fn shortcut_while_waiting_for_permission_cancels() {
        let b = bridge();
        poll(&b, A);
        b.toggle(false);
        let (_, session) = poll(&b, A).unwrap();
        b.report(Some(A), &event(session, "permission"));
        b.toggle(true);
        assert_eq!(b.snapshot().phase, Phase::Idle);
        assert_eq!(poll(&b, A), Some((Command::Cancel, session)));
        assert!(b.platform.clipboard.lock().unwrap().is_empty());
    }

    #[test]
    fn permission_timeout_and_denial_return_to_idle() {
        let b = bridge();
        poll(&b, A);
        b.toggle(false);
        let (_, session) = poll(&b, A).unwrap();
        b.report(Some(A), &event(session, "permission"));
        b.expire(session, Phase::Permission, "Mikrofon izni verilmedi.");
        assert_eq!(b.snapshot().message, "Mikrofon izni verilmedi.");
        assert_eq!(poll(&b, A), Some((Command::Cancel, session)));

        b.toggle(false);
        let (_, session) = poll(&b, A).unwrap();
        b.report(Some(A), &event(session, "permission"));
        b.report(Some(A), &json!({"event": "mic", "state": "denied"}));
        b.report(Some(A), &json!({"session": session, "event": "error", "error": "Mikrofon izni reddedildi."}));
        let s = b.snapshot();
        assert_eq!((s.phase, s.mic, s.message.as_str()), (Phase::Idle, Mic::Denied, "Mikrofon izni reddedildi."));
    }

    #[test]
    fn mic_state_comes_from_poll_and_change_events() {
        let b = bridge();
        assert_eq!(b.snapshot().mic, Mic::Unknown);
        b.set_mic("prompt");
        assert_eq!(b.snapshot().mic, Mic::Prompt);
        // Oturum ya da sahip gerekmez; tanınmayan değer yok sayılır.
        assert_eq!(b.report(Some(B), &json!({"event": "mic", "state": "granted"})), json!({"ok": true}));
        b.report(Some(B), &json!({"event": "mic", "state": "bilinmez"}));
        assert_eq!(b.snapshot().mic, Mic::Granted);
    }

    #[test]
    fn logs_use_current_hotkeys() {
        let b = api_bridge();
        let keys = Hotkeys::default()
            .with(Slot::Toggle, crate::hotkey::Hotkey::parse("Ctrl+Shift+D").unwrap())
            .unwrap();
        b.set_hotkeys(keys);
        b.toggle(false);
        assert_eq!(b.snapshot().hotkeys, keys);
        assert!(b.platform.logs.lock().unwrap().iter().any(|l| l.contains("Bitince Ctrl+Shift+D")));
    }

    #[test]
    fn cancel_sends_cancel_and_resets() {
        let b = bridge();
        poll(&b, A);
        b.toggle(false);
        let (_, session) = poll(&b, A).unwrap();
        b.cancel();
        assert_eq!(b.snapshot().phase, Phase::Idle);
        assert_eq!(poll(&b, A), Some((Command::Cancel, session)));
    }

    #[test]
    fn start_timeout_cancels_delivered_start() {
        let b = bridge();
        poll(&b, A);
        b.toggle(false);
        let (_, session) = poll(&b, A).unwrap();
        b.expire(session, Phase::Starting, "Dikte başlamadı.");
        assert_eq!(b.snapshot().phase, Phase::Idle);
        assert_eq!(poll(&b, A), Some((Command::Cancel, session)));
    }

    /// Hata düzeltmesi: hiçbir sekmeye ulaşmamış "start", zaman aşımı/iptalden sonra teslim edilmez
    /// ve sonradan gelen sekmeyi sahipsiz durumda "owner" yapmaz.
    #[test]
    fn undelivered_start_is_withdrawn_not_delivered_late() {
        let b = bridge();
        poll(&b, A);
        b.toggle(false);
        let session = b.session();
        b.expire(session, Phase::Starting, "Dikte başlamadı.");
        assert_eq!(poll(&b, A), None); // ne start ne de cancel
        // Sekme A kaybolur, yerine B gelir: yeni başlatma B'ye ulaşabilmeli.
        poll(&b, B);
        b.toggle(false);
        assert_eq!(poll(&b, B), Some((Command::Start, session + 1)));
    }

    /// Hata düzeltmesi: iptalden hemen sonra yeniden başlatmak, teslim edilmemiş iptali silmez.
    #[test]
    fn cancel_survives_quick_restart() {
        let b = bridge();
        let session = listening(&b);
        b.cancel();
        b.toggle(false);
        assert_eq!(poll(&b, A), Some((Command::Cancel, session)));
        assert_eq!(poll(&b, A), Some((Command::Start, session + 1)));
    }

    #[test]
    fn stale_start_timer_does_not_touch_new_session() {
        let b = bridge();
        poll(&b, A);
        b.toggle(false);
        let old = b.session();
        b.cancel();
        b.toggle(false);
        b.expire(old, Phase::Starting, "Dikte başlamadı.");
        assert_eq!(b.snapshot().phase, Phase::Starting);
        assert_eq!(poll(&b, A), Some((Command::Start, old + 1)));
    }

    #[test]
    fn transcribing_press_beeps_until_stuck() {
        let b = bridge();
        listening(&b);
        b.toggle(false);
        b.toggle(false); // hemen ikinci basış: sadece bip
        assert_eq!(b.snapshot().phase, Phase::Transcribing);
        assert_eq!(b.platform.beeps.lock().unwrap().last(), Some(&330));
    }

    #[test]
    fn invalid_client_ids_are_rejected() {
        let b = bridge();
        assert!(b.poll("short", NOW, "").is_err());
        assert!(b.poll(&"x".repeat(65), NOW, "").is_err());
        assert!(!b.connected());
    }

    #[test]
    fn long_poll_wakes_up_on_command() {
        let b = bridge();
        poll(&b, A);
        let waiter = {
            let b = Arc::clone(&b);
            thread::spawn(move || b.poll(A, Duration::from_secs(5), "").unwrap())
        };
        thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        b.toggle(false);
        let got = waiter.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(got.unwrap().0, Command::Start);
    }

    // --- yapıştırma ----------------------------------------------------------
    fn deliver(text: &str, foreground: isize, send_enter: bool) -> (Arc<Bridge<Fake>>, Result<usize, String>) {
        let b = bridge();
        *b.platform.foreground.lock().unwrap() = foreground;
        let result = b.deliver(Some(text), Some(terminal()), send_enter);
        (b, result)
    }

    #[test]
    fn success_pastes_without_enter() {
        let (b, result) = deliver("Deneme\nTürkçe", 111, false);
        assert_eq!(result, Ok(13));
        assert_eq!(*b.platform.clipboard.lock().unwrap(), vec!["Deneme Türkçe".to_string()]);
        assert_eq!(*b.platform.chords.lock().unwrap(), vec![Chord::CtrlV]);
        assert_eq!(b.snapshot().transfer.stage, Stage::Sent);
    }

    /// Terminal dışına (ör. VS Code'daki bir eklenti paneli) satırlar korunarak Ctrl+V ile yapıştırılır;
    /// yalnızca kopyalarken de satırlar kalır.
    #[test]
    fn line_breaks_survive_outside_terminals() {
        let b = bridge();
        let vscode = Window { hwnd: 111, title: "Agent Session Manager".into(), class: "Chrome_WidgetWin_1".into(),
                              exe: "code.exe".into() };
        assert_eq!(b.deliver(Some("Birinci\n\nİkinci"), Some(vscode), false), Ok(15));
        b.set_copy_only(true);
        assert_eq!(b.deliver(Some("a\nb"), Some(terminal()), false), Ok(3));
        assert_eq!(*b.platform.clipboard.lock().unwrap(), vec!["Birinci\n\nİkinci".to_string(), "a\nb".to_string()]);
        assert_eq!(*b.platform.chords.lock().unwrap(), vec![Chord::CtrlV]);
    }

    #[test]
    fn enter_variant_sends_enter_after_paste() {
        let (b, _) = deliver("ls", 111, true);
        assert_eq!(*b.platform.chords.lock().unwrap(), vec![Chord::CtrlV, Chord::Enter]);
        assert!(b.snapshot().transfer.enter_sent);
    }

    #[test]
    fn failed_enter_is_reported_without_claiming_success() {
        let b = bridge();
        *b.platform.fail_chord.lock().unwrap() = Some(Chord::Enter);
        assert!(b.deliver(Some("deneme"), Some(terminal()), true).is_err());
        let transfer = b.snapshot().transfer;
        assert!(transfer.paste_sent);
        assert!(!transfer.enter_sent);
        assert_eq!(transfer.stage, Stage::Error);
        assert_eq!(*b.platform.chords.lock().unwrap(), vec![Chord::CtrlV]);
    }

    #[test]
    fn failed_paste_never_sends_enter() {
        let b = bridge();
        *b.platform.fail_chord.lock().unwrap() = Some(Chord::CtrlV);
        assert!(b.deliver(Some("deneme"), Some(terminal()), true).is_err());
        assert!(b.platform.chords.lock().unwrap().is_empty());
        assert!(!b.snapshot().transfer.enter_sent);
    }

    // --- API modu --------------------------------------------------------------
    fn api_bridge() -> Arc<Bridge<Fake>> {
        let b = bridge();
        b.set_engine(Engine::Api);
        *b.platform.wav.lock().unwrap() = Some(crate::openai::wav(&[0; 32_000], 16_000));
        *b.platform.transcript.lock().unwrap() = Some(Ok("merhaba dünya".into()));
        b
    }

    #[test]
    fn api_cycle_records_transcribes_and_pastes_without_tab() {
        let b = api_bridge();
        assert!(!b.connected(), "sekme gerekmez");
        b.toggle(false);
        assert_eq!(b.snapshot().phase, Phase::Listening);
        assert!(*b.platform.recording.lock().unwrap());
        assert!(b.platform.timers.lock().unwrap().contains(&MAX_RECORDING));
        b.toggle(true);
        assert_eq!(b.snapshot().phase, Phase::Transcribing);
        b.platform.run_jobs();
        assert_eq!(*b.platform.transcribed.lock().unwrap(), vec![44 + 32_000]);
        assert_eq!(b.snapshot().phase, Phase::Idle);
        assert_eq!(*b.platform.clipboard.lock().unwrap(), vec!["merhaba dünya".to_string()]);
        assert_eq!(*b.platform.chords.lock().unwrap(), vec![Chord::CtrlV, Chord::Enter]);
        assert_eq!(*b.platform.beeps.lock().unwrap(), vec![880, 660, 1200]);
        // Eklentiye hiçbir komut gitmez.
        assert!(b.st().commands.is_empty());
    }

    #[test]
    fn api_start_failure_shows_error() {
        let b = api_bridge();
        *b.platform.record_error.lock().unwrap() = Some("OpenAI API anahtarı yok.".into());
        b.toggle(false);
        let s = b.snapshot();
        assert_eq!((s.phase, s.message.as_str()), (Phase::Idle, "OpenAI API anahtarı yok."));
        assert_eq!(*b.platform.beeps.lock().unwrap(), vec![220]);
    }

    #[test]
    fn api_too_short_recording_is_not_sent() {
        let b = api_bridge();
        *b.platform.wav.lock().unwrap() = Some(crate::openai::wav(&[0; 100], 16_000));
        b.toggle(false);
        b.toggle(false);
        b.platform.run_jobs();
        assert!(b.platform.transcribed.lock().unwrap().is_empty());
        assert!(b.snapshot().message.contains("çok kısa"));
    }

    #[test]
    fn api_error_is_shown_and_nothing_pasted() {
        let b = api_bridge();
        *b.platform.transcript.lock().unwrap() = Some(Err("OpenAI API anahtarı geçersiz.".into()));
        b.toggle(false);
        b.toggle(false);
        b.platform.run_jobs();
        let s = b.snapshot();
        assert_eq!((s.phase, s.message.as_str()), (Phase::Idle, "OpenAI API anahtarı geçersiz."));
        assert!(b.platform.clipboard.lock().unwrap().is_empty());
    }

    #[test]
    fn api_cancel_discards_recording_and_late_result() {
        let b = api_bridge();
        b.toggle(false);
        b.cancel();
        assert!(!*b.platform.recording.lock().unwrap(), "kayıt durduruldu");
        assert_eq!(b.snapshot().phase, Phase::Idle);

        // Gönderildikten sonra iptal: geç gelen sonuç yapıştırılmaz.
        b.toggle(false);
        b.toggle(false);
        b.cancel();
        b.platform.run_jobs();
        assert!(b.platform.clipboard.lock().unwrap().is_empty());
        assert!(b.platform.logs.lock().unwrap().iter().any(|l| l.contains("geç geldi")));
    }

    #[test]
    fn switching_engine_cancels_running_dictation() {
        let b = api_bridge();
        b.toggle(false);
        b.set_engine(Engine::Browser);
        let s = b.snapshot();
        assert_eq!((s.phase, s.engine), (Phase::Idle, Engine::Browser));
        assert!(!*b.platform.recording.lock().unwrap());
    }

    #[test]
    fn copy_only_mode_copies_without_keys_or_focus_change() {
        let b = bridge();
        b.set_copy_only(true);
        // Hedef kapanmış ve başka pencere önde olsa da kopyalama başarılı sayılır.
        *b.platform.alive.lock().unwrap() = false;
        *b.platform.foreground.lock().unwrap() = 222;
        assert_eq!(b.deliver(Some("Merhaba"), Some(terminal()), true), Ok(7));
        assert_eq!(*b.platform.clipboard.lock().unwrap(), vec!["Merhaba".to_string()]);
        assert!(b.platform.chords.lock().unwrap().is_empty());
        let s = b.snapshot();
        assert!(s.copy_only);
        assert_eq!((s.transfer.stage, s.transfer.copied, s.transfer.paste_sent, s.transfer.enter_sent),
                   (Stage::Copied, true, false, false));
        b.set_copy_only(false);
        assert!(!b.snapshot().copy_only);
    }

    #[test]
    fn focus_failure_leaves_text_on_clipboard_without_keys() {
        let (b, result) = deliver("Deneme", 222, false);
        assert!(result.unwrap_err().contains("pencere geçişine izin vermedi"));
        assert_eq!(*b.platform.clipboard.lock().unwrap(), vec!["Deneme".to_string()]);
        assert!(b.platform.chords.lock().unwrap().is_empty());
        let t = b.snapshot().transfer;
        assert_eq!((t.stage, t.copied, t.paste_sent), (Stage::Error, true, false));
    }

    #[test]
    fn closed_target_keeps_text_on_clipboard() {
        let b = bridge();
        *b.platform.alive.lock().unwrap() = false;
        let result = b.deliver(Some("x"), Some(terminal()), false);
        assert!(result.unwrap_err().contains("Hedef pencere kapandı"));
        assert_eq!(b.platform.clipboard.lock().unwrap().len(), 1);
    }

    #[test]
    fn empty_transcript_does_not_touch_clipboard() {
        let (b, result) = deliver("   ", 111, false);
        assert!(result.unwrap_err().contains("Ses algılanmadı"));
        assert!(b.platform.clipboard.lock().unwrap().is_empty());
        assert!(b.platform.chords.lock().unwrap().is_empty());
    }

    #[test]
    fn delivery_error_is_reported_and_shown() {
        let b = bridge();
        let session = listening(&b);
        b.toggle(false);
        let reply = b.report(Some(A), &json!({"session": session, "event": "result", "text": " "}));
        assert_eq!(reply["ok"], false);
        assert!(b.snapshot().message.contains("Ses algılanmadı"));
    }
}
