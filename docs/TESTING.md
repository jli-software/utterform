# Validation

## 0.3.0 UI polish and pause

- Svelte/TypeScript: no errors or warnings; production Vite bundle builds.
- 22 frontend/release-metadata tests: release consistency and invalid/mismatched-tag guards; existing coverage plus pause/resume/finish/discard, repeat/busy guards, pause failure recovery and watchdog race; legacy timestamps, European dates, midnight/year boundaries, future/unknown dates and live relative-time updates.
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
