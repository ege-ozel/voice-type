[English](README.md) · [Türkçe](README.tr.md)

# voice-type — ChatGPT Dikte

Windows voice typing into the focused window. Press a hotkey, speak, then finish to paste the
transcription or keep it on the clipboard. One native Rust executable; no runtime installation.

## Install

1. Download the Windows x64 ZIP from [Releases](https://github.com/ege-ozel/voice-type/releases/latest).
2. Extract it to a writable folder and run `Start.bat`. First launch creates private local credentials.
3. Right-click the microphone badge to choose a method:

| Method | Setup |
|---|---|
| OpenAI API | Set your Windows user environment variable `OPENAI_API_KEY`, then choose **Dikte: OpenAI API**. Audio is sent to the OpenAI transcription API; API billing applies. |
| ChatGPT tab | Run `Setup.bat`, then load the package's `extension` folder through Chrome's `chrome://extensions` → Developer mode → Load unpacked. Keep a signed-in chatgpt.com tab open and allow the microphone. This method depends on ChatGPT's web interface. |

For the API key, run `setx OPENAI_API_KEY "your-api-key"` in PowerShell. The app reads the Windows
user environment, so an existing process can pick it up. The default model is `gpt-4o-transcribe`;
an `openai_model` entry beside the EXE in `dikte-ayarlar.json` overrides it.

## Use

| Default shortcut | Action |
|---|---|
| Ctrl+Alt+Space | Start / finish and paste |
| Ctrl+Alt+Enter | Finish, paste and press Enter |
| Ctrl+Alt+Backspace | Cancel |

Finish while the target window has focus. Line breaks are preserved outside terminals. Click the
badge for the status window, which opens beside the badge's docked edge. From there you can change
shortcuts, choose **Bitirince: Yapıştır / Yalnızca panoya kopyala**, select **Orijinal / Ağız** sound
packs, and adjust notification volume (0–100%, 0 mutes). These settings persist beside the EXE.
The same output and sound options are available in the right-click menu. Windows master volume
is unchanged. Close the application through the tray menu's **Çıkış** command.

The closed badge is 64×64 px at 100% Windows scaling. Drag it to dock on any screen edge;
the position is remembered. Hover to reveal shortcuts, with **+ Enter** emphasized.

## Badge states

| State | Badge | Open (hover / by itself) |
|---|:---:|---|
| **Ready**<br><sub>idle; hover shows both shortcuts</sub> | <img src="docs/badge/ready-square.png" width="64" alt="Ready"> | <img src="docs/badge/ready-open.png" width="224" alt="Ready"> |
| **Listening**<br><sub>recording, the dot blinks</sub> | <img src="docs/badge/listening-square.png" width="64" alt="Listening"> | <img src="docs/badge/listening-open.png" width="214" alt="Listening"> |
| **Transcribing**<br><sub>speech is being turned into text</sub> | <img src="docs/badge/transcribing-square.png" width="64" alt="Transcribing"> | <img src="docs/badge/transcribing-open.png" width="200" alt="Transcribing"> |
| **Pasted**<br><sub>opens by itself for 4 s</sub> | <img src="docs/badge/pasted-square.png" width="64" alt="Pasted"> | <img src="docs/badge/pasted-open.png" width="208" alt="Pasted"> |
| **Copied**<br><sub>copy-only mode; opens by itself</sub> | <img src="docs/badge/copied-square.png" width="64" alt="Copied"> | <img src="docs/badge/copied-open.png" width="205" alt="Copied"> |
| **Error**<br><sub>opens by itself, full message</sub> | <img src="docs/badge/error-square.png" width="64" alt="Error"> | <img src="docs/badge/error-open.png" width="218" alt="Error"> |
| **Starting**<br><sub>ChatGPT tab: microphone opening</sub> | <img src="docs/badge/starting-square.png" width="64" alt="Starting"> | <img src="docs/badge/starting-open.png" width="192" alt="Starting"> |
| **Permission**<br><sub>ChatGPT tab: allow the mic in Chrome; opens by itself</sub> | <img src="docs/badge/permission-square.png" width="64" alt="Permission"> | <img src="docs/badge/permission-open.png" width="247" alt="Permission"> |
| **Not connected**<br><sub>ChatGPT tab: no chatgpt.com tab</sub> | <img src="docs/badge/closed-square.png" width="64" alt="Not connected"> | <img src="docs/badge/closed-open.png" width="220" alt="Not connected"> |

## Update

Exit the old app, replace the executable and extension folder, and keep `bridge-config.json` and
`dikte-ayarlar.json`. Run `Setup.bat`, then reload the extension and ChatGPT tab if you use them.
Generated config files may contain credentials and should stay local.

## Build

With Rust and Visual Studio C++ Build Tools installed, run from the repository root:

```powershell
cargo test --locked
./scripts/package.ps1
```

The ZIP and SHA-256 checksum are written to `dist/`. For a local checkout, run
`./scripts/setup.ps1` and copy `target/release/ChatGPT-Dikte.exe` into `dist/`.
Python is only used by optional asset-generation tools, not by the application or installer.

[Development guide](docs/DEVELOPMENT.md) · [Sound assets](assets/sounds/README.md) · [Changelog](CHANGELOG.md)
