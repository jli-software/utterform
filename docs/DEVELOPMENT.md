# Development & handoff

## Shared workflow

- Canonical repository: https://github.com/jli-software/utterform
- Fetch `main` before starting; work on a dedicated branch and preserve other contributors' changes. Commit and push validated work; integrate through a pull request only when authorized. Never force-push.
- **Always release completed work:** Jonas explicitly wants every completed, successfully validated set of Utterform changes published as a new GitHub version with all platform binaries, not left as source-only changes. Unless a version is specified, increment the patch by default (0.3.0 → 0.3.1); increment the minor only when Jonas asks, respecting any explicit version; use normal `vX.Y.Z` releases unless a prerelease is requested. Synchronize versions/installer/docs, run checks, tag, wait for every platform and publishing job, and verify the downloaded assets before calling the work released. No additional release confirmation is needed. Never move a published tag or publish failing/partial work; fix blockers first.
- Development is shared across machines and coding assistants. Keep architecture, decisions, release notes, and the current handoff in this repository, not only in chat history.
- Keep credentials, recordings, local transcript history, dependencies, and machine-specific configuration out of Git.
- GitHub Actions builds the downloadable binaries. Releases must include platform assets, not just source archives.

## Current handoff — 0.4.1

0.4.0 made dictation work without the window, on Omarchy. 0.4.1 gives Windows and macOS the same key and fixes the text arriving damaged.

**One intent, two ways in.** `hotkey.rs` reserves a key combination where the platform grants one — Windows, macOS, Linux under X11 — and emits the same `remote-intent` event that a compositor binding produces through `cli.rs` and the single-instance plugin. The interface therefore keeps exactly one recording state machine. Wayland registers nothing and Settings says what to bind instead. The plugin is installed from `setup`, not at build time: a host that will not create a hotkey manager must cost the dictation key, not the application.

**Typing at the cursor pastes by default.** Jonas reported terminals losing letters — "Session" arriving as "ession" — while a manual paste of the same text was intact. That is delivery, not transcription: keystrokes cross the compositor, the input method and the target application one character at a time, and any of them can drop or reorder one. A paste moves the whole text at once. `typing/` chooses the chord from the focused window's class, because Ctrl+Shift+V pastes in a terminal and does something else entirely in a browser or an editor.

Keystrokes stay selectable, with the two documented fixes: a leading `Shift_L` press/release for the Wayland clients that swallow the first character a fresh virtual keyboard sends, and a delay between keys.

**Windows types through `SendInput`.** No helper program, Unicode rather than scan codes, one atomic call per batch. `typing/mod.rs` holds the text→key rule so it is tested on every platform; `typing/windows.rs` holds only the unsafe glue.

### Platform reality — do not assume parity

Only Linux has been exercised by a person. The rest is compilation, not evidence. The Windows paths in this release are type-checked against the real `windows-sys` API for `x86_64-pc-windows-msvc` and were never run.

| | Linux / Omarchy | macOS | Windows |
| --- | --- | --- | --- |
| Typing at the cursor | 0.4.0 confirmed; paste delivery **new, unconfirmed** | **not implemented** | implemented, **never run** |
| Reserved dictation key | n/a under Wayland, by design | implemented, **never run** | implemented, **never run** |
| `utterform --toggle` reaching the running app | works, confirmed | plugin supports it, never tried | plugin supports it, never tried |
| Binding it to a key | `bind =` in hyprland.conf | Settings → Dictation key | Settings → Dictation key |
| Tray click opens the window | works, confirmed | never tried | never tried |
| Recording, transcription, clipboard, file | works, confirmed | never tried interactively | never tried interactively |

What to ask Jonas after he tests: whether paste delivery fixed the dropped letters in his terminal, and whether `Ctrl+Alt+D` is free on his Windows machine.

The misleading "needs a graphical session" message on Windows is gone — the platform is implemented. macOS still reports that typing at the cursor is not available there, which is now true rather than misleading.

### Waiting, not released

Pull request #8 (`feat/robustness-and-models`, CI green, mergeable) carries two things Jonas asked for but has not released, because he wanted to test 0.4.0 first:

- Robustness: a command answers even when its work panics (`resilience.rs`), bounded retries with backoff for rate limits and server faults (`openai::retry_delay`), and a 30-minute ceiling in the interface (`lib/ceiling.ts`).
- The larger offline models: Medium, Large v3 Turbo, and the quantized Large v3 Turbo.

**It bumps the version to 0.4.1, which this release now uses.** Jonas asked for the Windows dictation key as 0.4.1 and asked for it first. Re-target #8 to 0.4.2 before merging it; do not release it unasked.

### Then

Ordered as Jonas chose: robustness (in #8) before streaming. After that, file streaming and the vocabulary, then longer recordings and GPU acceleration. File streaming and realtime transcription are different projects and only the first is planned for 0.4.

### Running the tests

`dbus-run-session -- cargo test --manifest-path src-tauri/Cargo.toml`. The tray activation test needs a session bus of its own; plain `cargo test` fails without one.

The Windows-only code cannot be checked with `cargo check --target x86_64-pc-windows-msvc`: `ring`'s build script needs an MSVC toolchain. Type-check `typing/windows.rs` against the real API by compiling it in a throwaway crate that depends only on `windows-sys`, with the Tauri and domain layers stubbed out.

The hotkey tests deliberately avoid `tauri::test::mock_app`: adding `tauri = { features = ["test"] }` as a dev-dependency made the Windows test binary fail to start with `STATUS_ENTRYPOINT_NOT_FOUND`, the same class of dev-feature unification problem as the 0.3.4 `ksni`/`zbus` regression. Keep the decisions out of the `AppHandle` — a pure `support_with(failure)` and a `Failure` that records and reports on its own — so the tests need no runtime at all.

**A green local Clippy says nothing about the other platforms.** `cargo clippy -- -D warnings` on Linux compiles only the `cfg(target_os = "linux")` items, so anything the Linux backend alone uses looks alive here and is dead code — a hard error — on Windows and macOS. That failed the first v0.4.1 release run after the tag was already pushed. **Whenever a change adds or moves `cfg(target_os)` code, run Actions → Desktop builds on the branch for `all` before tagging.** It runs the same fmt/Clippy/test steps on all three platforms and publishes nothing.

## Current handoff — 0.3.4

0.3.3 broke GPT Transcribe and 0.3.4 fixes it. The lesson is about Cargo feature unification: `ksni`'s tokio feature switched `zbus` to `zbus/tokio` build-wide, and the keyring's blocking zbus calls then panicked inside Tauri's async runtime. Keep `ksni` on `async-io`, and check `cargo tree -e features -i zbus` before adding any dependency that speaks D-Bus.

## Current handoff — 0.3.3

Utterform is single-instance (0.3.2) and owns its Linux tray icon (0.3.3). Every path that opens the window — tray click, tray menu, a second launch from the app drawer — goes through `activation::reveal_main_window`.

The Linux tray is a `ksni` StatusNotifierItem, not AppIndicator, because AppIndicator exposes no `Activate` and therefore cannot report a left click. Tauri's native tray remains for Windows/macOS and as the Linux fallback when no StatusNotifierItem host answers. Run the Rust suite as `dbus-run-session -- cargo test`: the tray activation test needs a session bus of its own.

**Confirmed on Omarchy by Jonas on 2026-09-07:** a left click on the tray icon opens Utterform. The `ksni` path works on the real desktop, not only against the test's D-Bus watcher.

Still to confirm by hand: that launching Utterform a second time from the app drawer focuses the running instance instead of starting another.

## Current handoff — 0.3.1

**Published and verified:** [Utterform 0.3.1](https://github.com/jli-software/utterform/releases/tag/v0.3.1) is Latest, source `368bdc7`, branch `feat/compact-recorder-0.3.1` (not merged into `main`). The [release run](https://github.com/jli-software/utterform/actions/runs/34040148263) passed all three platforms and published all six assets plus checksums. Independently downloaded assets, isolated Linux installation, Windows GUI subsystem and real-Mac DMG/ZIP strict signatures passed. See [release notes](releases/v0.3.1.md) and [testing](TESTING.md) for validation/publication status. The tray popup remains a [discussion proposal](TRAY-POPUP.md), not part of this release.

Latest text is a collapsed disclosure by default, leaving the copy button visible. The native result includes `copiedToClipboard` from actual delivery, never inferred from requested settings. Automatic and manual copying show a short checkmark confirmation; stale asynchronous copy replies cannot label a different history entry as copied. Manual Copy stays visible but is disabled while recording/processing; an outstanding manual write drains before a new recording starts. A native two-note Done cue follows successful transform and all requested outputs; history-only failure does not suppress delivery success. All cues follow the existing sound preference.

The floating-window minimum is now 360 × 400. Stop and Pause share a control group; short/narrow layouts reduce secondary hints while preserving recording controls, output and copy. Expanded history/settings can scroll. Ambient recording motion and reduced-motion preferences remain intact.

The 0.3.0 ARM macOS artifact was tested on a real Mac and failed strict signature verification: its executable had only a linker signature and the app resources were unsealed. New macOS builds explicitly ad-hoc sign the assembled bundle before packaging, then verify signatures in the app, mounted DMG and extracted ZIP. This fixes a verified packaging defect, **not** Apple trust/notarization. Intel builds are removed; Apple Silicon requires macOS 11+.

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

Versions are synchronized across npm, Cargo (including lockfiles), Tauri, the installer and docs. Settings reads the package version automatically.

Push/PR `CI` runs frontend checks, production UI tests and native Linux fmt/Clippy/tests, without release-profile compilation or packaging. `Desktop builds` is manually runnable for Linux, Windows, macOS or all; it produces artifacts but never publishes. Manual dispatch becomes available only after the workflow reaches the default branch; tag releases already call it directly from their tagged source. The tag-triggered `Release` workflow validates metadata and calls `Desktop builds` once for all three supported targets. It requires all six binary/installer assets plus a combined checksum manifest before publication. Normal `vX.Y.Z` is Latest; explicit prerelease tags do not replace Latest. Stable download filenames are retained.

For each completed change set, update package.json/package-lock.json, Cargo.toml/the Utterform Cargo.lock entry, tauri.conf.json, the Linux installer's default tag, README, changelog and `docs/releases/<tag>.md`. Run `npm run release:check -- <tag>` and the normal test suite, commit/push, then create and push the new tag. Wait for the Release workflow and verify its downloadable checksums/asset set. Never move a published tag; use a new version for later corrections. Validation-only follow-up documentation for an already verified release does not need an otherwise identical new application release.

Before starting work on another device/assistant, pull `main`, read this file, `CHANGELOG.md`, and `docs/ARCHITECTURE.md`, and inspect the latest Actions result. Project-wide decisions stay here; machine-specific setup and user data stay outside Git.

The [four-target CI run](https://github.com/jli-software/utterform/actions/runs/34027520241) passed, including all installers/archives. The downloaded Linux CI binary also passed an isolated native startup/tray-Quit smoke test. See [TESTING.md](TESTING.md) for exact coverage and an open, intermittent WebKit subprocess shutdown observation on the local Omarchy runtime. No fix for that non-reproducible observation is claimed.

Beta 1 was published with all assets and verified checksums. Final asset inspection found its Windows executable used the console PE subsystem. Beta 2 corrects that desktop-only issue and adds a binary-level CI assertion; Beta 1's tag is left intact. Previous published release: [`v0.2.0-beta.2`](https://github.com/jli-software/utterform/releases/tag/v0.2.0-beta.2). All four release jobs passed; all downloaded assets, the Linux installer, the Windows GUI subsystem, and both macOS architectures were verified. The next work is beta-user feedback and the open WebKit shutdown observation in `TESTING.md`, not unfinished release packaging.

Native interactive microphone/speaker and macOS/Windows desktop tests must be distinguished from mocked browser tests and cross-platform compilation.
