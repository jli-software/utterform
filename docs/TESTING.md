# Validation

## 0.4.6 A window running as administrator is named, not typed into

81 native and 53 interface tests, unchanged: the integrity-level check is Windows API code with no seam a Linux test can reach. **Diagnosed from Jonas's 0.4.5 log, not reproduced here.** The log showed Utterform recognising Windows Terminal and sending Shift+Insert on four recordings with nothing arriving, and Jonas confirmed the terminal runs as administrator; that is UIPI, and `SendInput` counting dropped events as delivered is documented behaviour. The Windows build is verified through Actions → Desktop builds. Not yet seen by anyone: the warning text on a real elevated window, and whether an elevated Utterform pastes into an elevated terminal.

## 0.4.5 Paste into a Windows terminal, cues on their own threads, and a log

81 native and 53 interface tests. New native coverage: a Windows terminal being recognised by its window class or by the program behind it — Windows Terminal, the console host under any shell, Electron terminals by program — while VS Code, browsers, Word and Notepad keep the ordinary paste; and the tray dot being small, soft-edged and confined to the lower right corner while still solid at 16 pixels. The badge tests replace the 0.4.4 ones.

**Diagnosed from Jonas's report, not reproduced here.** The Windows paste failure — text arriving everywhere except in a terminal — matches Windows Terminal ignoring a synthesized Ctrl+V that carries no scan code and names the generic rather than the left Control key. The fix cannot be compiled on this Linux machine: `cargo check --target x86_64-pc-windows-msvc` stops at `ring`'s build script for want of a C compiler, so the Windows build is verified through Actions → Desktop builds on the branch. What no test covers: whether Windows Terminal takes the corrected Ctrl+V as well — it is sent Shift+Insert regardless — and whether Shift+Insert reaches every terminal on the list.

**The start click on Windows is changed, not confirmed.** Jonas reported on 2026-09-08 that on 0.4.4 the click sounds with Utterform's window in front and not otherwise. The two structural differences between the start cue and the stop cue that works are removed (own thread per cue, opened after the microphone) and every outcome is logged. Whether that was the cause is what the log and the test button will say; see the handoff in [DEVELOPMENT.md](DEVELOPMENT.md).

## 0.4.4 A gap is not a failure, and a dot on the tray

79 native and 53 interface tests. New native coverage: a buffer under- or overrun report being counted while the stream error stays clear, and a lost device still ending the recording; the start cue peaking well above Stop and staying audible for longer, while remaining a click rather than an alarm; the recording badge being a solid red disc in the lower right that leaves the rest of the icon untouched, opaque even over a transparent icon, still several pixels across at the 16 pixels a Windows tray gives it, and never panicking on a buffer of the wrong size. The ksni tray test now spawns with both icons.

**Diagnosed from Jonas's report, not reproduced here.** The Windows failure — every recording ending with "Microphone stream failed: A buffer underrun or overrun occurred" after re-plugging a dock and microphone — was traced to cpal's WASAPI backend emitting `Xrun` on `AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY`, which Utterform treated as fatal. The fix is in the one function that classifies stream errors; the unit test exercises it with constructed cpal errors. The Windows driver behaviour that sets the flag is not reproducible on this Linux machine.

**Confirmed on Windows by Jonas on 2026-09-08:** recording through the re-plugged dock and microphone works on 0.4.4, and the tray dot shows while recording. **The start click still does not sound on Windows, at the louder level,** while the stop click does through the same speaker. Audibility is therefore ruled out; the WASAPI output side of the start cue is the open question, with the bisection plan in [DEVELOPMENT.md](DEVELOPMENT.md). The Linux/AirPods start click is fixed and unaffected.

## 0.4.3 The start sound on speakers that suspend

74 native and 53 interface tests. New native coverage: every cue opening with silence a waking device can swallow and the tone still beginning the moment that lead-in ends rather than being pushed later by it; every cue ending in at least 250 ms of silence, which is what stands between a buffered output and a cue nobody hears; and only sample formats that can actually be written being accepted, since silently reporting success for the others is what made a cue disappear with no way to find out why. On the capture side, nothing being kept until the start cue has been played, so the cue cannot land in its own recording.

**Diagnosed on real hardware, not in a test.** Jonas reported on 2026-09-08 that Super+D on his MacBook Air under Linux produced no start click through AirPods, while the stop click always came. The microphone was the internal one, so no Bluetooth profile switch was involved. Playing a YouTube video first — waking the sink — made the start click appear every time, which is what identified the cold start as the cause. No automated test can reach this: it needs a device that suspends.

**Not exercised by anyone yet:** the notification that stands in for a cue no output device will play, and the fix on macOS or Windows speakers. The code is shared and platform-independent, but only the Linux/AirPods case has been seen.

## 0.4.2 Prompts anyone can rewrite, and words the model gets right

70 native and 53 interface tests, plus 11 production Chromium scenarios. New coverage: a shipped prompt running until it is replaced, a replacement winning, a blank replacement falling back rather than failing the recording that used it, and a renamed action still following our instructions; every built-in but Plain carrying a prompt and the ids staying unique; a 0.4.1 settings file gaining the new fields while its custom actions survive, and an edited prompt surviving a save and a reload; a keyword carrying `<`, `>`, a carriage return or a line feed being dropped rather than sent, because the API refuses the whole request over one of them; and the vocabulary reaching Whisper as prior text.

In the interface: the editor showing the shipped instructions and Plain shown as the one with none; a rewritten prompt saved with the original still readable beside it; an emptied box staying empty to type in without storing an edit; Reset restoring the shipped text and the name with it; a rename reaching the action menu and Cancel putting it back; an added prompt selected straight away and dropped again on delete; a chosen reasoning level sent and Auto sending nothing; and a vocabulary term the API would refuse named where it was typed. End to end in Chromium, the same path runs against the production bundle, including the renamed action appearing in the main window's action menu.

One pre-existing test defect was fixed rather than worked around: a test set `takeStartupIntent` to `"toggle"`, and `vi.clearAllMocks()` does not undo a `mockResolvedValue`, so every test after it silently started a recording on mount. It went unnoticed while no later test looked at the recorder.

**Published and independently verified:** [v0.4.2](https://github.com/jli-software/utterform/releases/tag/v0.4.2) is the normal Latest release from `d1461a5`; [release run 34138984113](https://github.com/jli-software/utterform/actions/runs/34138984113) passed all three platforms. All six freshly downloaded assets match `SHA256SUMS.txt`, the downloaded installer is byte-identical to `scripts/install-linux.sh` and defaults to `v0.4.2`, the Windows executable is PE32+ **GUI** subsystem, and the Linux binary is ELF x86-64 with no unresolved shared libraries. The 0.4.2 paths are present in it: `keywords[]`, `transcription_context`, `reasoning`, and the shipped prompt text for both Email and Clean. No installed application was replaced for these checks.

**Not yet exercised by anyone: all of it.** The questions the tests cannot answer are the ones that matter here — whether a rewritten prompt reads the way it should, whether the Email default is worth keeping as shipped, and whether putting a name in the vocabulary actually makes GPT Transcribe spell it that way. Those need a real recording. The reasoning-effort levels have not been sent to a live model either; a model that does not offer the chosen level is handled by falling back to the plain transcript with the reason shown, and that path has not been seen in practice. Windows and macOS remain untouched by a person, as in 0.4.1.

## 0.4.1 A dictation key off Wayland, and text that arrives whole

58 native and 38 interface tests, plus 9 production Chromium scenarios. New coverage: a shortcut read the way the Settings field writes it, and a typo, an empty field or a bare key refused before it reaches the plugin — a bare key would stop producing its own character everywhere in the session; a settings file written by 0.4.0 gaining the new defaults while its existing choices survive, and a dictation key turned off staying off; the paste chord chosen for fifteen terminal window classes and refused for ten ordinary ones, including the browsers and editors where Ctrl+Shift+V means something else; every line ending becoming exactly one Return and non-BMP characters surviving as their surrogate pairs; that keystrokes are paced and preceded by the Shift tap, and that every modifier a paste presses is released again; and that paste delivery is told whether the clipboard already holds the transcript, across all eight combinations of requested outputs and all eight of succeeding and failing ones.

In the interface: a changed shortcut registered on save and an unchanged one left alone; a key another application holds keeping the dialog open with the reason where it was entered, while the rest of the settings are stored; turning the key off unregistering it; a Wayland session offered `utterform --toggle` instead of a dead field; and the keystroke delay staying out of the way until keystrokes are chosen.

The first v0.4.1 release run failed on Windows and macOS after the tag was pushed: the paste-chord and delay rules in `typing/mod.rs` are used only by the Linux backend, so Linux Clippy saw them alive while both other platforms rejected them as dead code under `-D warnings`. They are now compiled for `cfg(target_os = "linux")` and for tests, so they are still checked everywhere. Nothing had been published — the release job never ran — and the corrected tag was verified on all three platforms through Desktop builds first.

Windows then failed a second time, on the test binary rather than the build: `tauri = { features = ["test"] }` as a dev-dependency left it unable to start (`STATUS_ENTRYPOINT_NOT_FOUND`). The mock runtime is gone again and the dictation-key reporting is tested through a pure `support_with(failure)` instead, which needs no Tauri runtime. Both corrections were verified on all three platforms through Desktop builds before the tag was moved.

**Confirmed on Omarchy by Jonas on 2026-09-07:** the finished text now arrives in the focused window intact. He reported the defect from his own dictation — "Session" reaching a terminal as "ession" — and reports it gone after installing 0.4.1. Paste delivery is what no automated test could establish, because the failure lived between the compositor, the input method and the receiving application rather than in Utterform.

**Published and independently verified:** [v0.4.1](https://github.com/jli-software/utterform/releases/tag/v0.4.1) is the normal Latest release from `39ff72d`; [release run 34133318734](https://github.com/jli-software/utterform/actions/runs/34133318734) passed all three platforms. All six freshly downloaded assets match `SHA256SUMS.txt`, the downloaded installer is byte-identical to `scripts/install-linux.sh` and defaults to `v0.4.1`, the Windows executable is PE32+ **GUI** subsystem, and the Linux binary is ELF x86-64 with no unresolved shared libraries. The Rust-side 0.4.1 paths are present in it (`hyprctl`, `initialClass`, the unavailable-shortcut message). Two apparent absences were measurement artifacts, not defects: short literals such as `ctrl+shift+v` and `Shift_L` are materialized as instruction immediates in a release build rather than kept in `.rodata` (`ctrl+shi` and `ft+v` are each findable), and the embedded frontend is compressed, so no interface string is findable — a 0.3.1-era string like "Latest text" is equally absent. No installed application was replaced for these checks.

Still not exercised by anyone: **Windows and macOS**. The reserved dictation key and Windows typing at the cursor are type-checked against the real `windows-sys` API for `x86_64-pc-windows-msvc` and have **never been run**; Jonas plans to test Windows later. Keystroke delivery on Linux was not re-tested after the Shift tap and the delay were added — paste is the default and is what he confirmed. Compilation is not a claim about any of this.

## 0.4.0 Global dictation

41 native and 31 interface tests. New coverage: every combination of clipboard, file and typing requested against every combination of succeeding and failing, including that typing runs last so the text is already safe; the tool choice for Wayland, X11, XWayland and nothing installed; that both typing tools take the text on stdin, so a transcript starting with `-` or containing newlines is never parsed as options; the hotkey starting and finishing a recording through the emitted event; a hotkey that had to start the app first; and stop/cancel ignored when nothing is recording.

A test that only failed in the full suite exposed a real defect rather than a flake: a component torn down before `onMount` finished left its keydown handler bound to the window, so a later keypress ran two handlers. Listeners are now bound before the first await.

**Confirmed on Omarchy by Jonas on 2026-09-07:** the compositor binding starts and finishes a recording, and the finished text is typed straight into the focused window. He calls the direct insertion the feature that makes the tool work for him. This is what no automated test could establish.

Not covered anywhere: macOS and Windows behaviour. Typing at the cursor is not implemented there at all (see the platform reality in the handoff), and the hotkey, single instance and tray click have never been exercised interactively on either. Compilation on those platforms is not a claim about them.

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
