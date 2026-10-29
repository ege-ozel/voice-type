<p align="center">
  <a href="README.md"><img alt="English" src="https://img.shields.io/badge/README-English-5ab3b9?style=for-the-badge"></a>
  <a href="README.tr.md"><img alt="Türkçe" src="https://img.shields.io/badge/README-T%C3%BCrk%C3%A7e-d0608f?style=for-the-badge"></a>
</p>

# voice-type: voice typing into any window (Windows)

Press a hotkey, speak, press it again: what you said is pasted into whichever window has focus
(terminal, editor, browser), or only copied to the clipboard. It handles mixed Turkish/English speech
and software jargon well. Built for talking to Claude Code / Codex in the terminal.

The app is a single Rust executable (`rust/`): ~460 KB, one process, ~2 MB private memory when idle,
no runtime dependencies.

## Methods

Pick one by right-clicking the badge or the tray icon; the choice is remembered.

| | **OpenAI API** (recommended) | **ChatGPT tab** (experimental) |
|---|---|---|
| How | Microphone audio goes straight to the OpenAI speech-to-text API (`gpt-4o-transcribe`) | A Chrome extension uses the dictation button in an open ChatGPT tab |
| Cost | Pay per use (API billing) | Your ChatGPT account's dictation limits |
| Needs | `OPENAI_API_KEY` | Chrome + extension + an open chatgpt.com tab |

> **About the ChatGPT tab method:** it drives the ChatGPT web UI programmatically (clicks the button
> from code, reads the text from the page). OpenAI's Terms of Use prohibit extracting output
> automatically or programmatically, so this method is not the default, is kept as a personal
> experiment, and you use it at your own risk. The terms-compliant path is the OpenAI API method.

## Usage

| Hotkey | Action |
|---|---|
| **Ctrl+Alt+Space** | Start dictation / finish and paste |
| **Ctrl+Alt+Enter** | Finish, paste and press Enter |
| **Ctrl+Alt+Backspace** | Cancel |

The text goes to the window that has focus at the moment you finish: start talking, switch to the
target window, finish. Text is pasted with Ctrl+V (Ctrl+Shift+V only in Alacritty and WezTerm). Line breaks are kept, except in terminals where the text is joined into one line. With **copy to clipboard
only** (right-click menu), no keys are sent at all.

## The badge

A small microphone square stays on screen; its color shows the state. It slides open on its own when
there is something to read (pasted, copied, error, waiting for permission) and on hover, where it shows
both shortcuts. Drag it anywhere: it snaps to the nearest screen edge (corners included, never over the
taskbar) and remembers the spot. Click it, or double-click the tray icon, for the status window;
right-click it for the menu. The UI text is Turkish.

| State | Badge | Open (hover / by itself) |
|---|:---:|---|
| **Ready**<br><sub>idle; hover shows both shortcuts</sub> | <img src="docs/badge/ready-square.png" width="60" alt="Ready"> | <img src="docs/badge/ready-open.png" width="226" alt="Ready"> |
| **Listening**<br><sub>recording, the dot blinks</sub> | <img src="docs/badge/listening-square.png" width="60" alt="Listening"> | <img src="docs/badge/listening-open.png" width="182" alt="Listening"> |
| **Transcribing**<br><sub>speech is being turned into text</sub> | <img src="docs/badge/transcribing-square.png" width="60" alt="Transcribing"> | <img src="docs/badge/transcribing-open.png" width="195" alt="Transcribing"> |
| **Pasted**<br><sub>opens by itself for 4 s</sub> | <img src="docs/badge/pasted-square.png" width="60" alt="Pasted"> | <img src="docs/badge/pasted-open.png" width="203" alt="Pasted"> |
| **Copied**<br><sub>copy-only mode; opens by itself</sub> | <img src="docs/badge/copied-square.png" width="60" alt="Copied"> | <img src="docs/badge/copied-open.png" width="201" alt="Copied"> |
| **Error**<br><sub>opens by itself, full message</sub> | <img src="docs/badge/error-square.png" width="60" alt="Error"> | <img src="docs/badge/error-open.png" width="214" alt="Error"> |
| **Starting**<br><sub>ChatGPT tab: microphone opening</sub> | <img src="docs/badge/starting-square.png" width="60" alt="Starting"> | <img src="docs/badge/starting-open.png" width="187" alt="Starting"> |
| **Permission**<br><sub>ChatGPT tab: allow the mic in Chrome; opens by itself</sub> | <img src="docs/badge/permission-square.png" width="60" alt="Permission"> | <img src="docs/badge/permission-open.png" width="243" alt="Permission"> |
| **Not connected**<br><sub>ChatGPT tab: no chatgpt.com tab</sub> | <img src="docs/badge/closed-square.png" width="60" alt="Not connected"> | <img src="docs/badge/closed-open.png" width="216" alt="Not connected"> |

<sub>Images are rendered by the app's own drawing code: `cargo test badge_shots -- --ignored` (see
`rust/tools/rgba_to_png.py`).</sub>

## Setup

1. Build the EXE ([rustup](https://rustup.rs) + Visual Studio Build Tools), or download the
   `ChatGPT-Dikte-exe` artifact from GitHub Actions:

   ```powershell
   cd rust
   cargo build --release      # rust\target\release\ChatGPT-Dikte.exe
   ```

2. Generate the local key file (`bridge-config.json` next to the EXE, shared by the extension and the
   local bridge): `python make_config.py`, then start the EXE from `dist\`.
3. **OpenAI API:** create a key at https://platform.openai.com/api-keys and run
   `setx OPENAI_API_KEY "sk-..."`. Right-click → **Dikte: OpenAI API**. For the cheaper model, add
   `"openai_model": "gpt-4o-mini-transcribe"` to `dist\dikte-ayarlar.json`.
4. **ChatGPT tab (optional):** `chrome://extensions` → Developer mode → Load unpacked → `extension\`.
   Sign in once in that tab and allow the microphone.

Details, an end-to-end test checklist and a measurement script: [rust/README.md](rust/README.md)
(Turkish).

## Technical notes

- **Rust on plain Win32:** hotkeys via `RegisterHotKey`, GDI UI (neon theme, no drawing unless the state
  changes), clipboard and keystrokes via `SendInput`. Microphone via winmm `waveIn`, HTTPS via Windows'
  built-in WinHTTP, so no TLS or audio library is bundled. Dependencies: `windows-sys`, `serde_json`.
- **Platform-independent state machine** (`rust/src/bridge.rs`): everything that touches Windows sits
  behind a trait, so the 51 unit tests run anywhere; separate opt-in hardware tests cover the microphone
  and HTTPS (`cargo test -- --ignored`). CI tests on Windows and builds the EXE on every push.
- **Local bridge** (ChatGPT tab method): listens on `127.0.0.1` only and checks a random per-install key
  on every request. Dictated text is never written to the log.
- Icons are drawn in code (`rust/src/icon.rs`, SDF anti-aliasing); the extension PNGs are generated
  from the same code and checked by a test.

| | Python version (old) | Rust version |
|---|---|---|
| EXE | ~18 MB | ~0.46 MB |
| Processes | 2 | 1 |
| Idle memory | ~62 MB | ~2 MB private (~12 MB working set) |

## Older Python versions

The Python files in the repository root are the first versions, kept for reference:
`browser_bridge.py` + `tray_app.py` (ChatGPT tab bridge, packaged with PyInstaller) and
`voice_type.py` (the first OpenAI API version). The Rust version speaks the same HTTP protocol, so the
extension works with both.
