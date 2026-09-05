# Utterform

**Speak once. Shape the text.**

Utterform is a lightweight desktop voice-to-text utility for Windows, Linux, and macOS. Record a short voice clip, transcribe it with OpenAI GPT Transcribe or local Whisper, optionally transform the text, and send the result to the clipboard, a TXT/Markdown file, or both.

> Utterform is under active development. The current `0.1.0` branch is an initial MVP and is not a signed release.

## Features

- Batch transcription with `gpt-transcribe` — no realtime session required
- Offline transcription through `whisper.cpp`
- One-click, SHA-256-verified downloads for Tiny, Base, and Small multilingual models
- Plain, Clean, Polish, Summarize, Prompt, and user-defined actions
- Clipboard, TXT, Markdown, or combined output
- Selectable microphone with a system-default fallback
- Focused-window shortcuts and a compact system tray presence
- Automatic processing when a recording reaches 10 minutes
- Light, dark, and system themes
- API keys stored in the operating system credential store
- No transcription history by default

## Keyboard shortcuts

Shortcuts work while the Utterform window is focused.

| Key | Action |
| --- | --- |
| `Space` | Start or stop recording |
| `Escape` | Discard the active recording |
| `1`–`5` | Select Plain, Clean, Polish, Summarize, or Prompt |
| `C` | Toggle clipboard output |
| `F` | Toggle file output |

Global shortcuts are intentionally deferred, primarily because support differs across Linux desktop environments and Wayland compositors.

## Privacy model

- **GPT Transcribe:** the temporary audio recording is sent to OpenAI.
- **Local Whisper + Plain:** audio and text stay on the device.
- **Local Whisper + transformed action:** audio stays local; transcribed text is sent to OpenAI.
- Temporary recordings are deleted after processing or cancellation.
- The OpenAI API key is never written to `settings.json`; it is stored through the native OS keyring.

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

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for boundaries and design decisions.

## License

[MIT](LICENSE)
