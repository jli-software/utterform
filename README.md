# Utterform

**Speak once. Shape the text.**

Utterform is a lightweight desktop voice-to-text utility for Windows, Linux, and macOS. Record a short voice clip, transcribe it with OpenAI GPT Transcribe or local Whisper, optionally transform the text, and send the result to the clipboard, a TXT/Markdown file, or both.

> Utterform is under active development. **0.3.1** is available (Windows unsigned; macOS ad-hoc signed, not notarized). See the [release notes](docs/releases/v0.3.1.md) and [downloads](https://github.com/jli-software/utterform/releases/tag/v0.3.1).

## Install on Omarchy / Arch Linux

An unsigned x86_64 build is available. It installs for the current user and does not require `sudo`:

```bash
curl -fsSL https://github.com/jli-software/utterform/releases/download/v0.3.1/install-linux.sh | sh
```

Then launch **Utterform** from the app menu or run `utterform`. The installer verifies SHA-256, installs the executable below `~/.local/share/utterform`, and creates a launcher in `~/.local/bin`. The application deliberately uses Omarchy's system GTK, WebKitGTK, and graphics libraries instead of mixing them with an Ubuntu AppImage runtime. Quit the old running app, then re-run the command to update or repair the installation; settings, models and history are preserved.

Required Omarchy/Arch runtime packages:

```bash
sudo pacman -S --needed webkit2gtk-4.1 gtk3 alsa-lib libayatana-appindicator
```

## Windows and macOS

Download the [0.3.1 assets](https://github.com/jli-software/utterform/releases/tag/v0.3.1):

- **Windows x86_64:** `utterform-windows-x86_64-setup.exe`, or the standalone `utterform-windows-x86_64.exe` with WebView2 installed.
- **macOS Apple Silicon:** `utterform-macos-aarch64.dmg` (or `.app.zip`).
- **macOS Intel:** no longer built or supported; macOS 11+ Apple Silicon only.

On macOS drag Utterform into Applications. Windows builds are unsigned; macOS bundles are ad-hoc signed (not Developer ID signed or notarized), so Gatekeeper/SmartScreen may require explicit approval. Version 0.3.1 fixes the unsealed macOS app bundle in 0.3.0 and verifies its signature inside both downloads; this does not bypass Gatekeeper. On macOS use **System Settings → Privacy & Security → Open Anyway** after attempting launch. Verify assets against `SHA256SUMS.txt`; do not disable system-wide security protections.

Push/PR CI runs Linux validation without release compilation. For a test binary, manually run [Actions → Desktop builds](https://github.com/jli-software/utterform/actions/workflows/desktop-builds.yml) and choose Linux, Windows, macOS, or all. GitHub enables manual dispatch once this new workflow is integrated into the default branch; a release tag alone does not enable that button. Tagged releases build all three supported targets once and publish only after all checks and packaging succeed.

## Features

- Batch transcription with `gpt-transcribe` — no realtime session required
- Offline transcription through `whisper.cpp`
- One-click, SHA-256-verified downloads for Tiny, Base, and Small multilingual models
- Plain, Clean, Polish, Summarize, Prompt, and user-defined actions
- Clipboard, TXT, Markdown, or combined output
- Selectable microphone with a system-default fallback
- Focused-window shortcuts and a compact system tray presence
- Background recording across app switches and close-to-tray, with a native ten-minute cutoff
- Microphone-responsive violet/blue ambient motion and optional start/stop clicks plus a distinct completion chime after successful processing and delivery
- Borderless window on Omarchy; standard window controls elsewhere
- Pause/resume without finishing or adding silence; automatic processing after 10 minutes of active recording
- Floating-friendly layout down to 360 × 400; grouped Stop/Pause controls
- Latest text collapsed by default; copy stays visible with a brief checkmark confirmation after actual clipboard success
- Light, dark, and system themes
- API keys stored in the operating system credential store
- Last 100 texts kept locally across restarts, with short titles, European dates/24-hour times, elapsed minutes today, and copy-again controls (can be disabled)

## Keyboard shortcuts

Shortcuts work while the Utterform window is focused.

| Key | Action |
| --- | --- |
| `Space` | Start or finish recording (also while paused) |
| `P` | Pause or resume the current recording without processing it |
| `Escape` | Discard the active or paused recording |
| `1`–`5` | Select Plain, Clean, Polish, Summarize, or Prompt |
| `C` | Toggle clipboard output |
| `F` | Toggle file output |
| `Ctrl+Shift+C` / `Cmd+Shift+C` | Copy the displayed text again (latest by default) |

Global shortcuts are intentionally deferred, primarily because support differs across Linux desktop environments and Wayland compositors. **An already-started recording continues when you switch apps, minimize, or close the window to tray** (including Omarchy's `Super+W`). Reopen from the tray to pause or finish it, or let the ten-minute active-recording limit stop capture. A paused recording remains paused across app switches and close-to-tray. Paused audio is discarded, not stored or sent; the microphone device stays open so resuming works consistently across platforms. Processing resumes when the WebView is available. Tray **Quit** discards active audio and exits; simply launching Utterform does not start recording.

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
npx playwright install chromium
npm run test:e2e
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
