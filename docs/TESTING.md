# Validation

## 0.7.5 Developer ID signing and Apple notarization

Pre-release GitHub Actions run `34696227610` built the Apple-Silicon app with the repository's
Developer ID credentials. Apple's notary service returned `Accepted`; Tauri stapled
the ticket. The packaging step then required all of the following before succeeding:

- `codesign --verify --deep --strict` on the built and ZIP-extracted app;
- a `Developer ID Application` authority and non-empty Team ID;
- `xcrun stapler validate` on both app copies;
- Gatekeeper acceptance with `source=Notarized Developer ID`;
- valid DMG, ZIP and checksums.

Frontend checks and tests, the production GUI suite, Rust formatting, Clippy and Rust
tests also passed. Not automated: a real install-over-update retaining Microphone and
Accessibility grants. The build host has no interactive window session.

The first tagged attempt, 0.7.4, exposed a separate reusable-workflow boundary: GitHub
does not pass secrets through `workflow_call` unless the caller explicitly inherits or
maps them. Its tag job therefore logged that notarization was skipped and packaged an
ad-hoc app. The release is marked superseded. The caller now uses `secrets: inherit`,
and the called workflow independently refuses a tagged build unless all six Apple
values are present, so this failure cannot publish another tag silently.

## 0.7.3 Local Whisper on processors without AVX-512

**Reported by Jonas on 2026-09-11 and diagnosed on the affected device.** Local Whisper with the Small model closed Utterform instantly on his Omarchy machine (Intel Core i3-N300: AVX2 yes, AVX-512 and AMX no); GPT Transcribe was unaffected. Claude on that device read the crash: `SIGILL / ILL_ILLOPN` in `ggml_backend_cpu_device_get_extra_buffers_type`, reached from `whisper_model_load`, three times in two minutes.

**Confirmed here against the published 0.7.2 artifact, not taken on trust.** Disassembling the released `utterform-linux-x86_64-system.tar.gz` binary shows the same instruction at the same address — `ccc0cd: vpbroadcastq %rcx,%xmm0`, EVEX-encoded, immediately after `operator new` — and the binary carries 17,656 `%zmm` operands, 833 `vmovdqu64`, 477 `vpdpbusd` and 270 AMX instructions. The cause is ggml's `GGML_NATIVE`, which defaults to ON and compiles for the build machine; `whisper-rs = "0.16.0"` has been in `Cargo.toml` since the first commit, so every release shipped this way and only the two processors involved decided whether it crashed.

Automated: the rebuilt binary carries **0** AVX-512, VNNI or AMX instructions in any ggml or whisper symbol, against 8,624 in the published 0.7.2 binary, while AVX2 (14,600 `%ymm`) and FMA remain — so the baseline is portable without being slow. `CMakeCache.txt` confirms `GGML_NATIVE=OFF` with `GGML_AVX2`, `GGML_FMA` and `GGML_BMI2` ON and every AVX-512 option OFF. `scripts/test-linux-system-package.sh` was run against both binaries and refuses the old one; the new test in `local.rs` reads the compiled feature set. On the macOS build host, `clang -arch arm64 -mmacosx-version-min=11.0` was checked to default to `-target-cpu apple-m1`, which is why Apple Silicon needs no flag of its own.

**A trap found while fixing it, worth remembering:** `cmake-rs` never reports the environment variables it reads, so Cargo does not rebuild `whisper-rs-sys` when the toolchain file changes — the first local rebuild silently kept `GGML_NATIVE=ON`. CI restores `src-tauri/target` from a cache, so the release would have shipped the old object files. The cache key therefore carries a `-portable-cpu-1` suffix, and both new checks examine the built artifact rather than the configuration.

**Not verified by anyone yet:** that a fixed build actually transcribes on a processor without AVX-512. This build host is a QEMU VM with SSE4.2 only and cannot execute the AVX2 baseline, so the evidence here is the disassembly; the Intel N300 is where the fix is proven. Local Whisper on Windows remains untried by anyone — it was exposed to the same fault, and 0.7.3 is the first build that could work there.

## 0.7.2 shortcut recorder and autostart

Automated, on Linux: 128 native tests (122 before), 94 interface tests (68 before) and 20 production-bundle Playwright scenarios (18 before), with `svelte-check` clean, `cargo fmt --check`, and `cargo clippy --all-targets -D warnings`.

New native coverage: `--autostart` read as an intent of its own rather than as the plain launch it would otherwise be; that intent raising no window, never reaching the interface, and never being kept as a startup intent, while every other intent still is and is still delivered exactly once; and the autostart-only Linux environment policy — the launcher's `GDK_BACKEND=x11` applied when the session set none, and a backend the session did choose left alone.

New interface coverage: a combination taken from the keyboard and registered as exactly itself; the physical key winning over the layout (`KeyY` is the same shortcut on a German keyboard), and Meta stored as `Super` while a Mac is shown `Cmd`; space, a digit, a function key and an arrow; a modifier on its own not finishing the reading; a key without a modifier refused where it was pressed, announced through the live region, and leaving the stored shortcut alone; `Escape` ending the reading and not the dialog, with the second `Escape` still closing it; Cancel keeping the stored shortcut; the registered key released for the reading and taken back on every way out of it — new key, Escape, Cancel; a compositor intent arriving mid-reading not starting a recording; a backend conflict still keeping the dialog open with the message in the field; and Wayland still shown `utterform --toggle` and no recorder. For autostart: the switch coming from `isEnabled` with nothing about it in the saved settings; `enable`/`disable` called only after Save and only on an actual change; Cancel writing nothing; a query failure disabling the switch alone while the rest of Settings still saves and closes; a write failure staying visible in the open dialog; and a write that reports success but changes nothing being caught by reading the entry back. In the production bundle: recording `Ctrl+Alt+K` through real Chromium key events including the refused bare key, `Escape` keeping the old one, the startup switch reading and writing the synthetic `plugin:autostart|*` IPC and writing nothing on Cancel or on an unchanged save, and both controls focusable and uncut inside the settings scroll at 920 × 720 and at the compact 720 × 620.

Desktop builds ran green on all three platforms for commit `f296eb3` (run 34629331357): the new dependency compiles and `clippy --all-targets -D warnings` passes on Linux, Windows and macOS, the Rust tests pass on each, and the Linux tarball, the Windows NSIS installer and the macOS disk image were built and verified by their packaging scripts. Push CI on the same commit is green.

**Not exercised by anyone yet:** every platform-specific part of autostart. No login has been performed anywhere. The Windows registry entry, the macOS LaunchAgent and the `~/.config/autostart` entry are what the plugin writes, not something these tests observed; neither is the tray-only start after a real sign-in, the upgrade path on Linux, or the recorder under a real Windows, macOS or X11 session where the operating system actually holds the key being replaced. The manual steps for all three platforms are in the release notes and the handoff; Desktop builds compiles the new dependency and every `cfg` path on Linux, Windows and macOS.

**One deliberate interpretation, worth knowing before testing.** The brief asked that a hotkey must not start a recording while Settings is open. It is turned away while the shortcut field is listening — which is when the dialog is open and the key press is meant for the field — and not for the whole time Settings happens to be open: on Wayland the compositor binding is the only way to dictate, and silencing it because a dialog is up would take away the documented Omarchy workflow.

## 0.7.1 macOS icon name

**Confirmed on macOS by Jonas on 2026-09-10, on 0.7.0 beta 1** (MacBook Air M2, macOS 26): the microphone dialog, GPT Transcribe and Local Whisper, the dictation key and changing it in Settings, and typing at the cursor. His words: "sensationell". Still open from the beta list: the denied-microphone message and the Dock click, not mentioned either way.

**Diagnosed from the report and Apple's forum, not reproduced here.** The Dock and ⌘-Tab showed the previous logo. The published 0.7.0 bundle was inspected on the build host: its `icon.icns` is byte-identical to the repository's and every size carries the current artwork, so the bundle is right and the Mac serves a cached rendering of an earlier version. Apple's developer forum documents the same symptom (Finder right, Dock and switcher stale) and cache deletion as the fix. 0.7.1 renames the icon file so the cached entry no longer applies; whether that alone is enough on Jonas's machine is what the release will show, and the release notes carry the cache-clearing commands and a discriminating test. The build host's SSH session has no icon services, so `NSWorkspace.icon(forFile:)` returns the placeholder there even for TextEdit — no icon rendering can be verified over SSH.

Automated: `npm run icons` regenerates `Utterform.icns` byte-identical to the former `icon.icns`; the 0.7.1 bundle built on the build host names it in `CFBundleIconFile` and passes `scripts/package-macos.sh`. The Developer ID path in the workflow is inert without secrets and was not exercised.

## 0.7.0 beta 1 macOS microphone entitlement and typing at the cursor

**Diagnosed on the build host, not on the reporting machine.** Jonas reported on 2026-09-10 that 0.6.1 on a MacBook Air (M2, macOS 26) never asked for the microphone and had no entry under Privacy & Security. The 0.5.2 bundle kept on `macmini-build` showed the cause: `codesign -dvv` reports `flags=0x10002(adhoc,runtime)` and `codesign -d --entitlements` reports none, while Info.plist does carry `NSMicrophoneUsageDescription`. Under the hardened runtime that is a silent denial by design.

Automated, on macOS 26.6 (`macmini-build`, Apple Silicon): `cargo fmt --check`, `cargo clippy --tests -- -D warnings` and `cargo test` with the macOS backend compiled in; the release bundle built with the CI command and verified by `scripts/package-macos.sh`, which now fails without the audio-input entitlement or the usage description. Linux native checks (122 tests) and the 68 interface tests pass, including a new one that Settings → Output shows the typing explanation when the backend reports one; the 18 production-bundle Playwright tests pass.

**Not exercised by anyone:** the microphone dialog itself, the denied-microphone message, the Accessibility request, ⌘V and keystroke delivery into a real window, the Dock click. The build host's SSH session has no window server. The five steps in the release notes are what the beta is waiting for.

## 0.6.1 Omarchy Live restart in the same field

Automated, Linux native (`cargo test`, 122 tests): Hyprland event classification (workspace, monitor, special-workspace, layer, submap, config-reload, title, layout, urgent and window-open events are unrelated; a different `activewindowv2` address or `closewindow` of the target is a loss; an empty address is "no window"); a focus guard against a fake Hyprland command socket — unchanged address despite desktop events, confirmed window change latched even after focus returns, a change the events missed but the query catches, target closed, launcher open and closed between chunks versus still open at insert time, a dead command socket and a closed event socket reported as technical faults after retries, one malformed answer retried, fragmented events, a stale guard of an earlier session; keycode allocation (frequent characters on quiet keys, no forbidden keycode, DOM-code-backed, recycling keeps recent characters, keymap round-trips through the real libxkbcommon); stop hold and cancellation in `StopLatch`; and the input worker with a fake typer — two consecutive sessions isolated with a late block staying with its session, a focus loss discarding queued chunks without replay, an idle loss blocking the next chunk, cancellation releasing native input, a failed capture still marking the worker finished, stop requests only extending the hold. Clippy with warnings denied, rustfmt, svelte-check, 67 Vitest tests and 18 production-bundle Playwright tests pass.

Not automated: microphone → OpenAI Live → target field on the real Omarchy desktop, including the ten-round restart scenario, Chromium and XWayland targets, and a real window switch during Live. Windows Live is compiled and packaged by CI; its behaviour is unchanged apart from the shared stop hold.

## 0.4.6 A window running as administrator is named, not typed into

**Confirmed on Windows by Jonas on 2026-09-08, on 0.4.6:** typing at the cursor reaches Windows Terminal, the administrator warning appears where it should, and changing the dictation key in Settings — to `Alt+C` — works. His words: it works "sensationell". This closes the Windows story that started with 0.4.3: recording through a re-plugged dock (0.4.4), the paste chord (0.4.5), the elevated window (0.4.6). Not separately put into words: whether the start click was heard with the window in the background, the case that was silent on 0.4.4; his 0.4.5 log shows the cue playing to the end on every recording.

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
