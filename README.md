# Utterform

**Speak once. Shape the text.**

Utterform is a lightweight desktop voice-to-text utility for Windows, Linux, and macOS. Record a short voice clip, transcribe it with OpenAI GPT Transcribe or local Whisper, optionally transform the text, and send the result to the clipboard, a TXT/Markdown file, or both.

> Utterform is under active development. The current `0.1.0` alpha is an initial, unsigned MVP.

## Install on Omarchy / Arch Linux

An unsigned x86_64 alpha build is available for early testing. It installs for the current user and does not require `sudo`:

```bash
curl -fsSL https://github.com/jli-software/utterform/releases/download/v0.1.0-alpha.2/install-linux.sh | sh
```

Then launch **Utterform** from the app menu or run `utterform`. The installer verifies SHA-256, installs the executable below `~/.local/share/utterform`, and creates a launcher in `~/.local/bin`. The application deliberately uses Omarchy's system GTK, WebKitGTK, and graphics libraries instead of mixing them with an Ubuntu AppImage runtime. Re-run the command to repair or reinstall this alpha.

Required Omarchy/Arch runtime packages:

```bash
sudo pacman -S --needed webkit2gtk-4.1 gtk3 alsa-lib libayatana-appindicator
```

The build is currently limited to Linux x86_64. Windows and macOS test builds will follow later; signed packages are not currently planned.

## Features

- Batch transcription with `gpt-transcribe` — no realtime session required
- Offline transcription through `whisper.cpp`
- One-click, SHA-256-verified downloads for Tiny, Base, and Small multilingual models
- Plain, Clean, Polish, Summarize, Prompt, and user-defined actions
- Clipboard, TXT, Markdown, or combined output
- Selectable microphone with a system-default fallback
- Focused-window shortcuts and a compact system tray presence
- Background recording across app switches and close-to-tray, with a native ten-minute cutoff
- Microphone-responsive violet/blue ambient motion and optional start/stop clicks
- Borderless window on Omarchy; standard window controls elsewhere
- Automatic processing when a recording reaches 10 minutes
- Light, dark, and system themes
- API keys stored in the operating system credential store
- Last 100 texts kept locally across restarts, with short titles and copy-again controls (can be disabled)

## Keyboard shortcuts

Shortcuts work while the Utterform window is focused.

| Key | Action |
| --- | --- |
| `Space` | Start or stop recording |
| `Escape` | Discard the active recording |
| `1`–`5` | Select Plain, Clean, Polish, Summarize, or Prompt |
| `C` | Toggle clipboard output |
| `F` | Toggle file output |
| `Ctrl+Shift+C` / `Cmd+Shift+C` | Copy the displayed text again (latest by default) |

Global shortcuts are intentionally deferred, primarily because support differs across Linux desktop environments and Wayland compositors. **An already-started recording continues when you switch apps, minimize, or close the window to tray** (including Omarchy's `Super+W`). Reopen from the tray to stop it, or let the ten-minute limit stop capture. Processing resumes when the WebView is available. Tray **Quit** discards active audio and exits; simply launching Utterform does not start recording.

## Privacy model

- **GPT Transcribe:** the temporary audio recording is sent to OpenAI.
- **Local Whisper + Plain:** audio and text stay on the device.
- **Local Whisper + transformed action:** audio stays local; transcribed text is sent to OpenAI.
- Temporary recordings are deleted after processing or cancellation.
- The OpenAI API key is never written to `settings.json`; it is stored through the native OS keyring.
- The last 100 completed texts are stored unencrypted on this device in `history.json`, including clipboard-only output. Titles are generated locally, without an AI call. Disable future storage or clear existing history in Settings; exported files and clipboard contents are not cleared.
- Text history paths: Linux `~/.local/share/software.jli.utterform/history.json` (or `$XDG_DATA_HOME`), macOS `~/Library/Application Support/software.jli.utterform/history.json`, Windows `%LOCALAPPDATA%\\software.jli.utterform\\history.json`.

## Development

Requirements:

- Node.js 22+
- Rust 1.88+
- CMake and a C/C++ toolchain for `whisper.cpp`
- Platform dependencies required by [Tauri 2](https://v2.tauri.app/start/prerequisites/)

On Debian/Ubuntu, the native dependencies include:

```text
build-essential cmake clang libasound2-dev libayatana-appindicator3-dev
librsvg2-dev libssl-dev libwebkit2gtk-4.1-dev
```

Run the app:

```bash
npm install
npm run tauri dev
```

Run checks:

```bash
npm run check
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

## Architecture

The Rust core owns audio, secrets, network access, model files, transcription, and delivery. The Svelte frontend only drives the UI and receives non-sensitive results.

```text
Microphone → temporary WAV → TranscriptionProvider
                              ├─ GPT Transcribe
                              └─ local whisper.cpp
                           → optional TextTransformer
                           → Clipboard / TXT / Markdown
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for boundaries and design decisions and [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the shared development workflow and handoff.

## License

[MIT](LICENSE)
