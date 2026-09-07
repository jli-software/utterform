# Utterform

**Speak once. Shape the text.**

Utterform is a lightweight desktop voice-to-text utility for Windows, Linux, and macOS. Record a short voice clip, transcribe it with OpenAI GPT Transcribe or local Whisper, optionally transform the text, and send the result to the clipboard, a TXT/Markdown file, or both.

> Utterform is under active development. **0.4.1** is available (Windows unsigned; macOS ad-hoc signed, not notarized). See the [release notes](docs/releases/v0.4.1.md) and [downloads](https://github.com/jli-software/utterform/releases/tag/v0.4.1).

## Install on Omarchy / Arch Linux

An unsigned x86_64 build is available. It installs for the current user and does not require `sudo`:

```bash
curl -fsSL https://github.com/jli-software/utterform/releases/latest/download/install-linux.sh | sh
```

The `releases/latest` URL always selects the newest stable GitHub release. Then launch **Utterform** from the app menu or run `utterform`. The installer verifies SHA-256, installs the executable below `~/.local/share/utterform`, and creates a launcher in `~/.local/bin`. The application deliberately uses Omarchy's system GTK, WebKitGTK, and graphics libraries instead of mixing them with an Ubuntu AppImage runtime. Quit the old running app, then re-run the command to update or repair the installation; settings, models and history are preserved.

Required Omarchy/Arch runtime packages:

```bash
sudo pacman -S --needed webkit2gtk-4.1 gtk3 alsa-lib libayatana-appindicator
```

## Dictate from anywhere

Press one key, speak, press it again. The window never comes forward, so you stay in whatever you were typing in. How the key reaches Utterform depends on who owns the keyboard.

**Windows, macOS and X11 — a reserved key combination.** `Ctrl+Alt+D` by default; change it or turn it off under **Settings → Dictation key**. Nothing to configure elsewhere. A combination another application already holds is reported where you entered it.

**Wayland (Hyprland, Omarchy) — a compositor binding.** Wayland gives no application the right to grab a global shortcut, so Utterform takes its orders from the command line instead and the single running instance receives them. Add one line to `~/.config/hypr/hyprland.conf`:

```
bind = SUPER, D, exec, utterform --toggle
```

The command line works on every platform, from any launcher, script or panel button:

| Command | Effect |
| --- | --- |
| `utterform --toggle` | Start recording, or finish the running one |
| `utterform --start` | Start recording |
| `utterform --stop` | Finish and process |
| `utterform --cancel` | Discard without transcribing |
| `utterform` | Show the window |

If Utterform is not running yet, the command starts it and still records.

Because the window stays where it is, the sounds are the confirmation: a click when recording starts, a click when it stops, and a distinct chime once the text has been transformed and delivered. Turn them off under **Settings → Recording feedback**.

## Typing at the cursor

Enable **Type** in the output bar (or press `T`) to have the finished text put into the window you were working in, next to clipboard and file. Clipboard, file and typing are independent, and typing happens last, so a failure there costs a warning and never the text.

**Paste** is the default. The whole text moves in one step, so nothing can be dropped or reordered on the way — the reason a terminal used to turn "Session" into "ession". Utterform sends the paste a terminal listens for (`Ctrl+Shift+V`) and the one every other window takes (`Ctrl+V`). Paste delivery leaves the text on the clipboard.

**Keystrokes** is available under **Settings → Typing at the cursor** for windows that refuse a paste. It types character by character, with a leading Shift tap for the Wayland clients that swallow the first character and an adjustable delay (15 ms by default) for the ones that reorder fast input.

On Linux both methods need the session's own input tool, because a Wayland client cannot synthesize input for another window:

```bash
sudo pacman -S --needed wtype     # Wayland/Hyprland
sudo pacman -S --needed xdotool   # X11
```

On Windows nothing needs installing; Utterform uses `SendInput` directly, sending Unicode rather than scan codes so the active keyboard layout does not matter. Windows refuses input from a normal process to a window running as administrator, which is reported as a delivery warning.

Typing at the cursor is not implemented on **macOS** yet.

## Windows and macOS

Download the [0.4.1 assets](https://github.com/jli-software/utterform/releases/tag/v0.4.1):

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
- Global dictation with a reserved key combination on Windows, macOS and X11, or a compositor binding on Wayland: either way without raising the window
- Optional typing of the finished text into the focused window, next to clipboard and file, as one paste or as keystrokes (Linux and Windows)
- Focused-window shortcuts and a compact system tray presence
- Single instance: launching Utterform again reveals the running window instead of starting a second one
- Tray left click opens the window on every platform, double click on Windows and macOS, middle click on Linux; right click keeps the menu
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

Global dictation is separate from these: see [Dictate from anywhere](#dictate-from-anywhere). **An already-started recording continues when you switch apps, minimize, or close the window to tray** (including Omarchy's `Super+W`). Reopen from the tray to pause or finish it, or let the ten-minute active-recording limit stop capture. A paused recording remains paused across app switches and close-to-tray. Paused audio is discarded, not stored or sent; the microphone device stays open so resuming works consistently across platforms. Processing resumes when the WebView is available. Tray **Quit** discards active audio and exits; simply launching Utterform does not start recording.

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
