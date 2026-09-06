# Development & handoff

## Shared workflow

- Canonical repository: https://github.com/jli-software/utterform
- Work directly on `main`. Pull before starting; commit and push each completed, validated change immediately. Never force-push over another contributor's work.
- **Always release completed work:** Jonas explicitly wants every completed, successfully validated set of Utterform changes published as a new GitHub version with all platform binaries, not left as source-only changes. Unless a version is specified, choose the next appropriate SemVer (patch for fixes/polish, minor for features); use normal `vX.Y.Z` releases unless a prerelease is requested. Synchronize versions/installer/docs, run checks, tag, wait for every platform and publishing job, and verify the downloaded assets before calling the work released. No additional release confirmation is needed. Never move a published tag or publish failing/partial work; fix blockers first.
- Development is shared across machines and coding assistants. Keep architecture, decisions, release notes, and the current handoff in this repository, not only in chat history.
- Keep credentials, recordings, local transcript history, dependencies, and machine-specific configuration out of Git.
- GitHub Actions builds the downloadable binaries. Releases must include platform assets, not just source archives.

## Current handoff — 0.3.0

**Published and verified:** [Utterform 0.3.0](https://github.com/jli-software/utterform/releases/tag/v0.3.0) is the normal Latest release. The [release run](https://github.com/jli-software/utterform/actions/runs/34038549482) passed on all four targets and published all eight assets plus checksums. Downloaded SHA-256 checks, isolated Linux installation, binary architectures/Windows GUI subsystem, macOS version metadata and shared icon payloads all passed. Release source: `2c67667`. The first attempt was blocked by Windows CRLF handling in the new metadata guard; the fix has a regression test, and Jonas explicitly approved replacing the blocked tag before any 0.3.0 release had been published. The now-published tag is immutable.

The name **Utterform** is intentionally retained. Implemented user feedback:

- One refined violet/blue microphone mark for the app, Settings and all desktop icon formats; regenerate from `src-tauri/icons/app-icon.svg` with `npm run icons`.
- Themed Settings/model cards, custom microphone/model selectors, a sliders symbol, subtle opening motion and keyboard-safe modal focus.
- European history dates, local 24-hour times and live elapsed minutes today; existing history IDs provide a backwards-compatible timestamp fallback.
- Native pause/resume via button or P. Space finishes and Escape discards, also while paused. Pauses neither deliver text nor add silence; the ten-minute limit counts active recording only. CPAL remains open while callbacks discard paused samples.

The audio-reactive ambient field, app identifiers and storage locations are unchanged. Version 0.3.0 packages this work as a normal release (`v0.3.0`); older published tags remain untouched. Signing and auto-update remain out of scope. The installed app must be updated separately; do not interrupt a user's active recording.

Local validation: 23 frontend/release-metadata tests (including Windows CRLF checkouts), 20 Rust tests, 5 production Chromium tests, Svelte/TypeScript, Rustfmt and Clippy. Settings/model/pause/history screenshots checked in light/dark and compact/reduced-motion modes. Tests use synthetic IPC/audio, not the user's microphone, API key, clipboard or history. Desktop icons regenerate byte-identically; ICNS PNG payloads match the corresponding standalone assets. Interactive pause/resume still needs a real desktop microphone check on each platform.

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

Version 0.3.0 is synchronized across npm, Cargo (including lockfiles), Tauri, and the Settings header. Tag `v0.3.0` names **Utterform 0.3.0**. The package version remains numeric for desktop installers; alpha/beta/rc suffixes may be used on tags only when explicitly intended.

CI builds and uploads Linux x86_64 system packages, Windows x86_64 standalone/NSIS executables, and macOS Apple Silicon/Intel DMG/app archives. The `Release` workflow (historical filename `linux-release.yml`) first checks tag/version/installer/docs consistency, then reuses the same four-target CI, verifies all eight required assets, generates one combined checksum manifest, and publishes only after every target passes. Normal `vX.Y.Z` releases are marked Latest; `-alpha.N`, `-beta.N` and `-rc.N` tags become prereleases without replacing Latest.

For each completed change set, update package.json/package-lock.json, Cargo.toml/the Utterform Cargo.lock entry, tauri.conf.json, the Linux installer's default tag, README, changelog and `docs/releases/<tag>.md`. Run `npm run release:check -- <tag>` and the normal test suite, commit/push, then create and push the new tag. Wait for the Release workflow and verify its downloadable checksums/asset set. Never move a published tag; use a new version for later corrections. Validation-only follow-up documentation for an already verified release does not need an otherwise identical new application release.

Before starting work on another device/assistant, pull `main`, read this file, `CHANGELOG.md`, and `docs/ARCHITECTURE.md`, and inspect the latest Actions result. Project-wide decisions stay here; machine-specific setup and user data stay outside Git.

The [four-target CI run](https://github.com/jli-software/utterform/actions/runs/34027520241) passed, including all installers/archives. The downloaded Linux CI binary also passed an isolated native startup/tray-Quit smoke test. See [TESTING.md](TESTING.md) for exact coverage and an open, intermittent WebKit subprocess shutdown observation on the local Omarchy runtime. No fix for that non-reproducible observation is claimed.

Beta 1 was published with all assets and verified checksums. Final asset inspection found its Windows executable used the console PE subsystem. Beta 2 corrects that desktop-only issue and adds a binary-level CI assertion; Beta 1's tag is left intact. Previous published release: [`v0.2.0-beta.2`](https://github.com/jli-software/utterform/releases/tag/v0.2.0-beta.2). All four release jobs passed; all downloaded assets, the Linux installer, the Windows GUI subsystem, and both macOS architectures were verified. The next work is beta-user feedback and the open WebKit shutdown observation in `TESTING.md`, not unfinished release packaging.

Native interactive microphone/speaker and macOS/Windows desktop tests must be distinguished from mocked browser tests and cross-platform compilation.
