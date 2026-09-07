# Validation

## 0.4.0 Global dictation

41 native and 31 interface tests. New coverage: every combination of clipboard, file and typing requested against every combination of succeeding and failing, including that typing runs last so the text is already safe; the tool choice for Wayland, X11, XWayland and nothing installed; that both typing tools take the text on stdin, so a transcript starting with `-` or containing newlines is never parsed as options; the hotkey starting and finishing a recording through the emitted event; a hotkey that had to start the app first; and stop/cancel ignored when nothing is recording.

A test that only failed in the full suite exposed a real defect rather than a flake: a component torn down before `onMount` finished left its keydown handler bound to the window, so a later keypress ran two handlers. Listeners are now bound before the first await.

Not covered: an actual keypress on Omarchy and actual typing into another window. Both need a real desktop session and the compositor's own binding.

## 0.3.4 GPT Transcribe regression

0.3.3 broke OpenAI transcription. `cargo tree -e features -i zbus` showed `zbus feature "tokio"` enabled by exactly one edge: `ksni feature "tokio"`, added in 0.3.3. With that feature `zbus::block_on` drives a static tokio runtime instead of calling `async_io::block_on`; the keyring reaches it through `zbus::blocking` from inside Tauri's async runtime, and tokio panics with "Cannot start a runtime from within a runtime". The panicking command never answers, so the interface waits forever. Local Whisper with the plain action never reads the keyring on that path, which matches the report that only OpenAI was affected.

`secrets::tests::the_keyring_can_be_reached_from_the_async_runtime` reproduced the panic before the fix and passes after it. It is a real guard: any dependency that reintroduces `zbus/tokio` fails it.

An actual OpenAI transcription was not run here — that needs a key and a microphone.

## 0.3.3 Linux tray activation

The claim in 0.3.2 that Linux cannot deliver tray clicks was checked and is wrong. `libayatana-appindicator3.so.1`, which `tray-icon` uses for Tauri's Linux tray, exports `SecondaryActivate` and `XAyatanaSecondaryActivate` but no `Activate`, so its items can only offer a menu. That is a property of that library, not of the platform: hosts do call `Activate`, and applications that serve the StatusNotifierItem themselves receive it.

`tray::status_notifier_item::tests::a_left_click_reaches_the_application` proves the new path end to end. It owns `org.kde.StatusNotifierWatcher` on a private session bus, lets the real tray register with it, and calls `org.kde.StatusNotifierItem.Activate` — the D-Bus call a left click produces. The reveal action must run exactly once. CI runs the Rust suite through `dbus-run-session` on Linux; run it the same way locally.

**Confirmed on the real desktop:** Jonas reports on 2026-09-07 that a left click on the tray icon opens Utterform on Omarchy. The panel there does map a left click to `Activate`, which no automated test could establish.

Still uncovered: single-instance focusing needs a compositor and was not observed in an automated test or reported back yet.

## 0.3.2 single instance

29 Rust tests including tray gesture mapping and ARGB32 icon conversion, 28 frontend/metadata tests, Rustfmt, Clippy with warnings denied, and the Linux package test asserting the desktop entry's window class. No display was available to the implementing session, so second-launch focusing was not observed directly.

## 0.3.1 compact recording and completion

Local validation: Svelte/TypeScript, production Vite build, release metadata guard, 28 frontend/metadata tests, Rustfmt, Clippy with warnings denied, and 22 native Rust tests. Eight production Chromium scenarios cover recording/pause, settings focus, history, reduced motion, and floating layouts at 360 × 400, 480 × 480 and 920 × 400. Screenshots are inspected in light/dark themes. A review-found clipboard race is covered with deferred IPC: an in-flight copy drains before recording starts, and manual copy is unavailable throughout recording/processing so it cannot overwrite final delivery.

Native output tests exercise all requested/successful output combinations; waveform tests check the two-note Done signal at 44.1/48/96 kHz. Clipboard confirmation uses the native `copiedToClipboard` result even when history/file warnings exist. Tests do not access personal audio, history, clipboard or paid APIs.

On the real `macmini-build` Apple-Silicon host, the downloaded 0.3.0 app failed strict codesign verification (unsealed resources/linker-only signature). Re-signing the isolated test copy and running the new packaging script passed app, mounted DMG and extracted ZIP checks; original and deliberately altered test bundles were rejected. This verifies the packaging correction, not Gatekeeper approval or notarization.

**Published and independently verified:** [v0.3.1](https://github.com/jli-software/utterform/releases/tag/v0.3.1), source `368bdc7`, is the normal Latest release. [Release run 34040148263](https://github.com/jli-software/utterform/actions/runs/34040148263) passed Linux, Windows and macOS ARM. The first Mac attempt passed app signing but hit a transient `Resource temporarily unavailable` immediately after DMG creation; re-running only failed jobs passed without source/tag changes or rebuilding Linux/Windows. All six freshly downloaded assets match the combined SHA-256 manifest. The Linux installer matches source and passes isolated upgrade/system-library verification; ELF x86_64 and Windows x86_64 GUI subsystem are correct. On the real Apple-Silicon Mac, the published DMG checksum/integrity, mounted app and extracted ZIP strict codesign checks passed, with version 0.3.1 and ARM64 architecture. No installed application was replaced. Windows/macOS interactive speech capture and actual speaker playback still need user testing. The tray-popup concept is not implemented.


## 0.3.0 UI polish and pause

[Release run 34038549482](https://github.com/jli-software/utterform/actions/runs/34038549482) passed all four platform builds and publication. [v0.3.0](https://github.com/jli-software/utterform/releases/tag/v0.3.0) is published as a normal Latest release from `2c67667`. All eight downloaded assets match `SHA256SUMS.txt`. The downloaded Linux installer matches the source and passed isolated installation/replacement and system-library checks. Direct artifact inspection verified Linux x86_64 ELF, Windows x86_64 GUI subsystem and embedded new icon, both macOS Mach-O architectures, macOS version 0.3.0 and unchanged bundle identifier, and exact shared PNG/ICNS payloads in the Linux/macOS packages. No installed app was replaced or launched for these checks.

- Svelte/TypeScript: no errors or warnings; production Vite bundle builds.
- 23 frontend/release-metadata tests: release consistency, Windows CRLF checkouts and invalid/mismatched-tag guards; existing coverage plus pause/resume/finish/discard, repeat/busy guards, pause failure recovery and watchdog race; legacy timestamps, European dates, midnight/year boundaries, future/unknown dates and live relative-time updates.
- 20 Rust tests: existing coverage plus paused sample exclusion (no conversion/no inserted silence), repeated pause intervals and active-time limit, pause-vs-watchdog completion and nondestructive legacy history timestamp recovery.
- 5 production Chromium tests with synthetic IPC: themed Settings/model selection and download-state updates, modal focus trapping/restoration and layered Escape, pause timer freeze/resume/Space completion, European history dates, plus the existing responsive audio field and history/copy tests.
- Screenshots inspected in light/dark, 920×720 and compact 720×620/reduced-motion layouts. Model menus remain inside the scroll viewport; Settings animations are disabled with reduced motion.
- Rustfmt, Clippy with warnings denied, and `git diff --check`.
- Two `npm run icons` passes produce identical hashes for every desktop icon. ICNS container/image payloads verified against standalone PNGs; ICO image sizes decoded with ImageMagick. This is not an interactive macOS/Windows shell-icon test.

No real microphone capture, paid API request, clipboard replacement or personal history modification was used. A manual follow-up should record speech A, pause and speak B, resume with C, then finish: only A/C should be transcribed, elapsed time should exclude B, and hiding/reopening the app must preserve the paused state. Repeat with Escape/tray Quit and the active ten-minute limit. The installed app is not replaced by validation; the 0.3.0 Release workflow publishes new assets while leaving Beta 2's tag/assets intact.

## Published 0.2.0 Beta validation

## Automated checks

The four-target [CI run for the release implementation](https://github.com/jli-software/utterform/actions/runs/34027520241) passed on Linux x86_64, Windows x86_64, macOS Apple Silicon, and macOS Intel, including native packaging. The [Beta 2 release run](https://github.com/jli-software/utterform/actions/runs/34028732467) repeated all checks successfully and published [v0.2.0-beta.2](https://github.com/jli-software/utterform/releases/tag/v0.2.0-beta.2). All eight downloaded release assets passed their SHA-256 checks; the published Linux installer passed another isolated install test. Direct binary inspection confirmed Windows x86_64 **GUI** subsystem and the correct ARM64/x86_64 Mach-O architecture of each macOS app. Beta 1 had a console-subsystem Windows executable and is superseded, not retagged.

- Svelte/TypeScript: no errors or warnings.
- Frontend: 9 unit/component tests (history restore/copy/clear, focus loss/cancel, persistence failure, keyboard/pointer menus).
- Rust: 16 tests (history retention/atomic replacement/privacy/corruption handling, audio status/one-time completed-artifact consumption/envelope, cue generation, Omarchy detection, existing settings/model/transformation tests).
- Rustfmt and Clippy with warnings denied.
- Production Chromium: 2 end-to-end tests with **synthetic IPC**, covering recording UI, theme rendering, history/copy/menu interactions, compact layout and reduced motion. Screenshots inspected in light/dark themes. These are not hardware microphone tests.
- Linux system-package installer: checksums, isolated installation/replacement, shared-library resolution, and absence of bundled runtime libraries/RPATH.
- Actionlint and shell syntax validation.

## Native Omarchy checks

The Linux release binary starts with isolated app settings/data, using the existing installer policy (`GDK_BACKEND=x11`, no inherited `LD_LIBRARY_PATH`). The real window's `_MOTIF_WM_HINTS` confirmed decorations disabled. A native close request hides the window while keeping the process alive. Tray Quit was exercised through its native D-Bus menu. Both the locally compiled binary and the downloaded, checksum-verified CI binary were tested. The existing installed app and its settings were not replaced.

No user's microphone recording, API request, clipboard replacement, or transcript-history change was needed for these smoke tests. Interactive microphone/speaker behavior, device permissions and tray behavior still need beta-user testing on each desktop. Compilation on macOS/Windows is not a claim of interactive testing there.

## Open runtime observation: Linux WebKit shutdown

Two isolated local smoke tests produced a `WebKitWebProcess` SIGABRT during process exit, with `free(): corrupted unsorted chunks`. One followed forced test termination; the other followed normal tray Quit. The Utterform parent did not dump core; normal Quit returned exit code 0. No recording was active and no user text was involved. The core shows libc exit/free handling, with WebKit/Mesa renderer teardown on another thread; that is evidence of a shutdown-time renderer issue, **not proof of its original cause**. There was no OOM.

Observed runtime: WebKitGTK 2.52.6, Mesa 26.2.1, Omarchy/XWayland. Subsequent short and 65-second beta startup/quit tests, an alpha comparison, and the downloaded CI Linux binary completed without the warning. The issue is not reliably reproducible and is **not claimed fixed**. No speculative renderer or system-wide workaround was installed. If it recurs, retain the timestamp, runtime versions and whether the window was active/hidden, then isolate the renderer teardown. Do not upload raw core dumps: they can contain credentials or user text.
