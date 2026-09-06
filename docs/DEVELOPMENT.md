# Development & handoff

## Shared workflow

- Canonical repository: https://github.com/jli-software/utterform
- Work directly on `main`. Pull before starting; commit and push each completed, validated change immediately. Never force-push over another contributor's work.
- Development is shared across machines and coding assistants. Keep architecture, decisions, release notes, and the current handoff in this repository, not only in chat history.
- Keep credentials, recordings, local transcript history, dependencies, and machine-specific configuration out of Git.
- GitHub Actions builds the downloadable binaries. Releases must include platform assets, not just source archives.

## Current handoff — unreleased UI polish and pause

The name **Utterform** is intentionally retained. Implemented user feedback:

- One refined violet/blue microphone mark for the app, Settings and all desktop icon formats; regenerate from `src-tauri/icons/app-icon.svg` with `npm run icons`.
- Themed Settings/model cards, custom microphone/model selectors, a sliders symbol, subtle opening motion and keyboard-safe modal focus.
- European history dates, local 24-hour times and live elapsed minutes today; existing history IDs provide a backwards-compatible timestamp fallback.
- Native pause/resume via button or P. Space finishes and Escape discards, also while paused. Pauses neither deliver text nor add silence; the ten-minute limit counts active recording only. CPAL remains open while callbacks discard paused samples.

The audio-reactive ambient field, app identifiers, storage locations and release tags are unchanged. This work is on `main`, not a new published release or an update of the installed app. Beta 2 remains the latest tagged download until a new release is explicitly prepared.

Local validation: 19 frontend tests, 20 Rust tests, 5 production Chromium tests, Svelte/TypeScript, Rustfmt and Clippy. Settings/model/pause/history screenshots checked in light/dark and compact/reduced-motion modes. Tests use synthetic IPC/audio, not the user's microphone, API key, clipboard or history. Desktop icons regenerate byte-identically; ICNS PNG payloads match the corresponding standalone assets. Interactive pause/resume still needs a real desktop microphone check on each platform.

## 0.2.0 Beta scope

- Remove native window decorations on Omarchy only; preserve other platforms' window controls.
- Subtle violet/blue, large-area, microphone-responsive recording motion, respecting reduced-motion preferences.
- Short start/stop sounds; styled action and file-format menus.
- Persistent, locally titled recent texts, a compact history selector, and copy-again button/shortcut.
- Verify microphone capture continues across focus changes and when the window is hidden.
- Publish unsigned Linux, macOS, and Windows binaries through Actions.
- No automatic Omarchy theme switching in this iteration. Global recording shortcuts remain separate from background microphone capture.

## Initial audit

The app uses Svelte 5/TypeScript and Tauri 2/Rust, with CPAL microphone capture. Focus changes do not stop capture, but the current close-to-tray handler explicitly cancels it. The frontend currently discards its last text when starting another recording and keeps no persistent history. Release automation currently publishes Linux packages only.

## Implemented for 0.2.0

History, themed menus, copy-again controls, responsive ambient motion, optional native cues, Omarchy decorations, and background/close-to-tray capture are implemented. Local checks pass: Svelte/TypeScript, 9 component/unit tests, 16 Rust tests, Clippy, and production-bundle browser tests. Browser screenshots have been inspected in light/dark themes, at 920×720 and 720×620, including reduced motion and menu overlays. Browser tests use synthetic IPC, never the user's microphone, credentials, or clipboard.

Browser validation: `npx playwright install chromium`, then `npm run test:e2e`. For an existing local Chromium installation use `PLAYWRIGHT_CHROMIUM_EXECUTABLE=/usr/bin/chromium npm run test:e2e`. Visual artifacts are written below ignored `test-results/`.

On the initial Linux development machine CMake was missing. An official, SHA-256-verified portable CMake was extracted into ignored `.tools/cmake-4.4.3-linux-x86_64`; prepend its `bin` directory to PATH for native builds. No system configuration was changed.

## Release workflow

Version 0.2.0 is synchronized across npm, Cargo (including lockfiles), Tauri, and the Settings header. Tag `v0.2.0-beta.2` names **Utterform 0.2.0 Beta**; the package version intentionally remains numeric for desktop installers.

CI builds and uploads Linux x86_64 system packages, Windows x86_64 standalone/NSIS executables, and macOS Apple Silicon/Intel DMG/app archives. The `Release` workflow (historical filename `linux-release.yml`) reuses the exact same checks and builds for alpha/beta tags, verifies all eight required assets, generates one combined checksum manifest, and only then publishes the prerelease. Update version files, `scripts/install-linux.sh`, release notes and changelog before tagging. Never move a published tag; use a new beta tag for later corrections.

Before starting work on another device/assistant, pull `main`, read this file, `CHANGELOG.md`, and `docs/ARCHITECTURE.md`, and inspect the latest Actions result. Project-wide decisions stay here; machine-specific setup and user data stay outside Git.

The [four-target CI run](https://github.com/jli-software/utterform/actions/runs/34027520241) passed, including all installers/archives. The downloaded Linux CI binary also passed an isolated native startup/tray-Quit smoke test. See [TESTING.md](TESTING.md) for exact coverage and an open, intermittent WebKit subprocess shutdown observation on the local Omarchy runtime. No fix for that non-reproducible observation is claimed.

Beta 1 was published with all assets and verified checksums. Final asset inspection found its Windows executable used the console PE subsystem. Beta 2 corrects that desktop-only issue and adds a binary-level CI assertion; Beta 1's tag is left intact. Current published release: [`v0.2.0-beta.2`](https://github.com/jli-software/utterform/releases/tag/v0.2.0-beta.2). All four release jobs passed; all downloaded assets, the Linux installer, the Windows GUI subsystem, and both macOS architectures were verified. The next work is beta-user feedback and the open WebKit shutdown observation in `TESTING.md`, not unfinished release packaging.

Native interactive microphone/speaker and macOS/Windows desktop tests must be distinguished from mocked browser tests and cross-platform compilation.
