# Development & handoff

## Shared workflow

- Canonical repository: https://github.com/jli-software/utterform
- Fetch `main` before starting; work on a dedicated branch and preserve other contributors' changes. Commit and push validated work; integrate through a pull request only when authorized. Never force-push.
- **Always release completed work:** Jonas explicitly wants every completed, successfully validated set of Utterform changes published as a new GitHub version with all platform binaries, not left as source-only changes. Unless a version is specified, increment the patch by default (0.3.0 → 0.3.1); increment the minor only when Jonas asks, respecting any explicit version; use normal `vX.Y.Z` releases unless a prerelease is requested. Synchronize versions/installer/docs, run checks, tag, wait for every platform and publishing job, and verify the downloaded assets before calling the work released. No additional release confirmation is needed. Never move a published tag or publish failing/partial work; fix blockers first.
- Development is shared across machines and coding assistants. Keep architecture, decisions, release notes, and the current handoff in this repository, not only in chat history.
- Keep credentials, recordings, local transcript history, dependencies, and machine-specific configuration out of Git.
- GitHub Actions builds the downloadable binaries. Releases must include platform assets, not just source archives.

## Current handoff — 0.7.3

**Local Whisper crashed on Linux because the binary was built for the build server.**
ggml's `GGML_NATIVE` defaults to ON — `-march=native` — so GitHub's runner baked its
AVX-512 and AMX into the release. On a processor without them the model load dies with
SIGILL. Fixed by `packaging/cmake/portable-cpu.cmake`, reached through
`.cargo/config.toml`. Full evidence in `docs/TESTING.md`.

**Two things to know before touching the whisper build again:**

1. **Cargo will not rebuild `whisper-rs-sys` when that toolchain file changes.**
   `cmake-rs` emits no `rerun-if-env-changed`, so the flag only reaches a *fresh*
   build. Locally: `cargo clean -p whisper-rs-sys --release` (the `--release` matters;
   without it only the dev profile is cleaned). In CI: raise the `-portable-cpu-N`
   suffix on the `Swatinem/rust-cache` key in both workflows.
2. **Verify the artifact, never the intent.** `scripts/test-linux-system-package.sh`
   disassembles the packaged binary and refuses AVX-512 or AMX inside a ggml or
   whisper symbol; `local.rs` has a test that reads the compiled feature set on every
   platform. Both were checked against the broken 0.7.2 binary and flag it.

**Still open:** nobody has transcribed with Local Whisper on a processor without
AVX-512 with a fixed build — that is Jonas on the N300. Local Whisper on Windows has
never been tried by anyone at all; it had the same fault, so 0.7.3 is the first build
that could work there.

## Current handoff — 0.7.2

**Branch `feat/shortcut-autostart-0.7.2`, pushed, not tagged.** Two features, both built and
tested on Linux only: the dictation key is recorded by pressing it, and Utterform can start
with the user's session. No release was made — the brief for this work asked for none.

**What needs a real desktop, and nobody has given it one.** Every platform-specific part of
autostart is what `tauri-plugin-autostart` writes, not something a test here observed:

- **Omarchy/Linux:** install 0.7.2, turn the switch on, quit, sign out and in again. Expect a
  process and a tray icon, no window and no focus taken; the tray opens it; `utterform --toggle`
  reaches that same instance. Turn it off and check `~/.config/autostart/Utterform.desktop` is
  gone. Then reinstall over it and confirm an entry that was on still works — it names
  `~/.local/share/utterform/bin/utterform`, resolved when the app starts, and that path does not
  move on update.
- **macOS:** from `/Applications`, not the mounted DMG — `auto-launch` refuses a path that does
  not exist, and the LaunchAgent would name a volume that is gone. After a login, no window and
  no focus; the tray and the Dock still open it. Try the recorder with ⌃⌥ and ⌘ combinations.
- **Windows:** the NSIS build. The entry is under HKCU, so no elevation; after signing in the app
  is in the tray with no window. Change the key with the recorder.

**What the recorder does that a Linux session cannot show:** on Windows, macOS and X11 the
operating system really is holding the combination, so the field releases it for the seconds it
is listening and takes it back afterwards. If a shortcut ever stops working after visiting
Settings, that is the place to look — `captureHotkey`/`suspendHotkey` in `App.svelte`, serialized
through one promise chain that Save waits on.

**One judgement call to know about.** The brief asked that a hotkey must not start a recording
while Settings is open. It is turned away while the shortcut field is listening, not for as long
as the dialog happens to be open: on Wayland the compositor binding is the only way to dictate,
and silencing it because a dialog is up would take away the Omarchy workflow the README documents.

## Current handoff — 0.6.1

**Omarchy Live restart regression (Tony, 2026-09-09):** a second GPT Live Transcribe start in the same field could stop at once with *"Live typing paused because the target or desktop focus changed"*, with a workspace reaction around the restart. Diagnosed from the code, Hyprland's sources (`KeybindManager::onKeyEvent`, `FocusState`), Omarchy's default bindings and Chromium's Wayland keyboard path; fixed on `fix/omarchy-live-focus-0.6.1` and released as 0.6.1. Three verified mechanisms:

1. `live_linux.rs` latched every `workspace*`, `focusedmon*`, `activespecial*`, `openlayer`, `submap`, `configreloaded` and monitor event as a permanent target loss (the only source of that message) and queried Hyprland's command socket every 20 ms, including during the OpenAI handshake.
2. Hyprland resolves bindings for a virtual keyboard by **keycode through its own configured layout**, with modifiers merged from all keyboards. Utterform's keymap put "ä" on keycode 172 = `XF86AudioPlay` (Omarchy: play/pause via swayosd, whose OSD layer then fired `openlayer`), "." on Tab (`SUPER+TAB` = next workspace), "!"…")" on `code:10`–`18` (`SUPER+code:N` = workspace N), "-" on Backspace. Text typed while the stop shortcut's Super key is still held therefore switched workspaces.
3. Chromium on Wayland drops keys without a DOM code (keycode 92 carried "s", 172 "ä") and derives editing from the US meaning of the keycode ("-" on Backspace = DeleteBackward).

**Fix:** only `activewindowv2` with a different non-empty address or `closewindow` of the target is a loss; an empty active window blocks only text due while it lasts; one Hyprland query per chunk with retries; technical faults worded as such; characters allocated on quiet, DOM-code-backed keycodes (keypad, F13–F19/F24, Intl, legacy keys) before ordinary keys, never Escape/Tab/Backspace/Return/modifiers/F1–F12/navigation/media/print/power; a 700 ms typing hold after every stop request (`STOP_HOLD` in `live.rs`); numbered sessions whose worker releases the virtual keyboard and event socket before the next start, with privacy-safe per-session log lines (`utterform.log`). The input worker is driven through the `LiveInput` trait so its lifecycle is unit-tested with a fake typer.

**Still to confirm on the real Omarchy desktop (Tony):** ten `Super+D → speak → Super+D → restart in the same field` rounds in a native editor, Chromium and an XWayland app; no workspace change, no old or duplicated text; a real window switch still stops insertion; "ä", "s" and "-" arrive in Chromium; the log shows one `live session N` block per round. If a workspace reaction remains, send the `live session` lines of the log — they name the classified Hyprland events without any text.

**Build note:** the portable CMake described below had been removed from `.tools/`; it was re-downloaded (official 4.4.3 tarball, SHA-256 verified) into `.tools/cmake-4.4.3-linux-x86_64`, still ignored by Git. `cargo test` needs its `bin` on `PATH` because `whisper-rs-sys` builds whisper.cpp with CMake.

## Current handoff — 0.4.6

**Confirmed on Windows by Jonas on 2026-09-08, on 0.4.6.** Paste into Windows Terminal works, the administrator warning shows, changing the dictation key to `Alt+C` works, and his verdict on Windows overall was "sensationell". Nothing is open on Windows from his side. What remains unconfirmed in words is only the start click with the window in the background (see below); the 0.4.5 log shows it playing.

**Jonas's Windows setup, for the next diagnosis:** Windows Terminal started as administrator; a Jabra Link 390 headset as the output; a Logitech C270 webcam as the microphone at 48 kHz stereo F32; the log lives at `%LOCALAPPDATA%\software.jli.utterform\utterform.log` and he sends it when asked. Ask for it before theorising: the 0.4.5 round was settled by one log line.

**How this was found, for whoever debugs Windows next:**

Jonas tested 0.4.5 on Windows within the hour. Paste into Windows Terminal still failed, and his log settled why: `pasting into window class "CASCADIA_HOSTING_WINDOW_CLASS" of program "WindowsTerminal" with Shift+Insert` on every attempt — recognition and chord were right — and his terminal runs as administrator. That is UIPI: a medium-integrity process cannot inject input into a high-integrity window, and `SendInput` still returns the full count, so 0.4.5's "Windows blocked the keystrokes" warning could never fire. `typing/windows.rs` now reads both processes' integrity levels from their tokens (`TokenIntegrityLevel`, last SID sub-authority: 0x2000 medium, 0x3000 high) and refuses with a warning naming the program when the window outranks Utterform. Do not try to work around UIPI: the only sanctioned route is a code-signed binary with `uiAccess="true"` installed under Program Files, and the Windows builds are unsigned. The user's options are an elevated Utterform or an unelevated terminal.

**The start click may already be fixed.** The same log shows `start cue played on "Kopfhörer (Jabra Link 390)" … device took the whole cue after ~500 ms` on all five recordings, with `recording armed ~525 ms after the microphone opened`. Not yet known: whether Utterform's window was in the background for those, which is the case that was silent on 0.4.4. Ask before closing the story.

### Before 0.4.6

## Current handoff — 0.4.5

Jonas's second Windows report, on 0.4.4, the same day: the text pastes everywhere except into a terminal; the start click sounds only when Utterform's own window is in the foreground — behind another window there is nothing, and once the stop click was missing too; and the tray dot should be more discreet.

**Windows terminals get Shift+Insert, and every key carries a scan code.** `typing/windows.rs` pressed `VK_CONTROL` and `VK_V` with `wScan = 0`. Classic Win32, GTK, Qt and Chromium windows take that; Windows Terminal reads `KeyStatus().ScanCode` off the message and asks `CoreWindow.GetKeyState` for `LeftControl` and `RightControl` separately, and a bare `VK_CONTROL` without a scan code satisfies neither. `virtual_key` now fills the scan code from `MapVirtualKeyW` and sets the extended flag on Insert; the chord names `VK_LCONTROL` / `VK_LSHIFT`. `focused_window` reads the foreground window's class and, through `QueryFullProcessImageNameW`, the program behind it, and `paste_for_window` in `typing/mod.rs` recognises a terminal by either — Windows Terminal's class is `CASCADIA_HOSTING_WINDOW_CLASS`, the console host's `ConsoleWindowClass` under any shell, Electron terminals only by program. A terminal gets Shift+Insert rather than Ctrl+Shift+V because conhost, PuTTY and ConEmu do not know the latter and all of them know the former. The decision is logged with the class and program, so a window that gets the wrong chord can be named. VS Code is deliberately not a terminal: the whole window pastes on Ctrl+V.

**Every cue on its own thread, after the microphone.** `feedback::start` now spawns a thread that opens, plays, finishes and drops the stream and logs the outcome; `Playback` is a receiver for the result. `audio::start_recording` opens the microphone first and starts the cue after `stream.play()`. `commands::start_recording` is `async` and does its work in `spawn_blocking`, so none of it runs on the main thread any more — a synchronous Tauri command does, and on Windows that is WebView2's thread. `arm` logs unconditionally with the time since the microphone opened.

**What is not settled: why the foreground mattered.** Nothing in WASAPI cares which window is in front. Candidates, in the order to check: the cue thread and stream living on the main thread (removed now); the microphone opening reconfiguring the output device — a Bluetooth headset dropping to its hands-free profile silences the A2DP stream the cue was on, and this also explains a stop click going missing when the profile switches back late (order changed now, so the cue opens on whatever the device has become); Windows' communications ducking, Sound → Communications → "Mute all other sounds", which starts when a capture stream opens on the default communications device; and Windows 11 timer coalescing for processes whose windows are not visible, which should not touch an event-driven WASAPI stream but has not been ruled out.

**What to ask Jonas, and what the log will say.** Which speaker and which microphone — laptop, dock, Bluetooth headset — and whether they are the same device. Then: Settings → Recording feedback → *Play the sounds in 5 seconds*, switch to another window, and read the result. All three play → the cue path is fine in the background and the microphone is the culprit; then try Sound → Communications → *Do nothing*, and a wired microphone. None play → the cue path itself is blocked in the background; the log line says whether the stream opened and how long the device took. The log is `%LOCALAPPDATA%\software.jli.utterform\utterform.log`; every real recording writes `microphone … open after`, `start cue played on …` or `start cue not played: …`, and `recording armed … ms after the microphone opened`. For the terminal: which terminal program, and whether a physical Shift+Insert pastes there.

**The tray dot is a third of the size, without the ring.** `recording_badge` in `tray.rs`: radius 0.19 of the side, 0.06 margin, one pixel of anti-aliasing, no white ring — the corner it sits in is the icon's dark surface. Still six pixels across at 16. If Jonas wants it quieter still, the next step is tinting the microphone capsule rather than adding anything.

**Windows cannot be compiled here.** `cargo check --target x86_64-pc-windows-msvc` fails in `ring`'s build script for want of a C compiler for that target. Run Actions → Desktop builds on the branch, `windows` for a quick answer and `all` before tagging.

### Before 0.4.5

## Current handoff — 0.4.4

Jonas tested 0.4.3 on Windows the same afternoon. Three findings, all fixed here; the first was a blocker.

**An Xrun is a glitch, not a failure.** After re-plugging a docking station and microphone, every recording ended with "Microphone stream failed: A buffer underrun or overrun occurred." cpal's WASAPI backend emits `ErrorKind::Xrun` whenever a capture packet carries `AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY` — a note that a few milliseconds are missing from a stream that carries on. `build_input_stream`'s error callback stored every error as fatal, and `finalize` discarded the recording over it. `note_stream_error` in `audio.rs` now counts an Xrun and fails only on the rest. macOS raises the same kind from a processor-overload listener, so this was waiting there too. Do not turn the glitch count into a UI warning: some Windows drivers set the flag on every recording and the warning would be noise.

**The start click was too quiet for a laptop speaker.** Calibrated at 0.12 for headphones; now 0.26 and 70 ms, about 6 dB over Stop, with a slower decay. Stop and Done are unchanged on purpose — Jonas hears them fine and the contrast is the point.

**The tray shows recording.** `tray::set_recording` is called from the four places capture starts or ends in `commands.rs`: start, finish, cancel, and the limit watchdog. The badge is `recording_badge`, drawn over the app icon at runtime — a second icon file would drift. Linux goes through the ksni handle's `update`, which emits `NewIcon`; Windows and macOS through `tray_by_id(NATIVE_TRAY)`, which is why the native tray now has an id. Windows 11 hides new tray icons behind the overflow arrow until the user pins them; the README says so.

**Confirmed on Windows by Jonas on 2026-09-08, on 0.4.4:** recording through the re-plugged dock and microphone works again, and the tray dot shows. **The start click is still missing on Windows.** At the new level, so this is not audibility — the stop click, through the same speaker, is fine. That is where the next session starts.

What is known: no notification appeared, so `arm` most likely returned `Ok` — the device took every sample and `finish` returned normally — and nothing was heard. Stop, through `play_detached`, works. The two cues differ in exactly three ways, and the job is to bisect them:

1. **When the stream is opened.** Start's output stream is opened *before* the microphone, Stop's after capture has ended. Try Start through `play_detached` after `stream.play()` (accepting it may land in the recording for the experiment) — if it sounds, the answer is in what opening WASAPI capture does to a render stream opened moments earlier.
2. **Which thread opens it.** Start is opened on the Tauri command thread and finished on a spawned one; Stop is opened and finished on one fresh thread. cpal initialises COM per thread; a WASAPI stream created on one thread and dropped on another is worth ruling out.
3. **The waveform.** Different frequency, gain and length — least likely, since the Linux fix works with the same code.

First step before any of that: log the outcome of `arm` unconditionally with the time `finish` took, so a Windows run says which branch it went down instead of leaving it to inference. The AirPods-on-Linux case is fixed and should not be regressed while doing this.

### Before 0.4.4

## Current handoff — 0.4.3

0.4.3 is one fix: the start click was missing on AirPods. Jonas found it dictating with the window hidden, where that click is the only feedback there is.

**The cause was not Bluetooth.** His microphone was the internal one, so no profile switch was involved. An idle output device suspends, and waking one costs a few hundred milliseconds before it makes any sound; `feedback.rs` gave the cue 500 ms and then closed the stream, which discards whatever the device still holds. The stop click survived because by then the speaker was awake. Confirmed by Jonas on 2026-09-08: playing a YouTube video first, so the sink was already running, made the start click appear every time.

**The rule this leaves behind: a cue is finished when the device says so, not when a timer says so.** Every cue now has a 60 ms silent lead-in for a device that discards its first samples and a 300 ms tail for one that buffers, and `Playback::finish` waits for the callback to take all of it. Do not shorten the tail to make something feel snappier — that is the bug.

**Capture now waits for the cue instead of the other way round.** The microphone opens first and discards through the `armed` gate until the start cue has played. That is what keeps the cue out of its own recording, now that the cue may take longer than the 500 ms it used to be allowed. The recording clock restarts at arming, so the ten-minute limit still counts kept audio.

**Cue failures are reported.** `feedback::play` returns a reason, and the one that used to look like success — an output sample format we cannot write — is an error now. `announce_recording` in `commands.rs` raises a desktop notification only when the start cue could not be played at all; it is not a second confirmation channel, and adding one would defeat the point of a quiet click.

Untested on real hardware other than Jonas's: the fix is platform-independent Rust and the same code runs everywhere, but the AirPods case that motivated it can only be confirmed on his machine.

### Before 0.4.3

## Current handoff — 0.4.2

0.4.2 hands the prompts to whoever installed the app. Jonas asked for it directly: the five shipped actions are fine as defaults, but the people who download Utterform must be able to adapt them.

**The shipped prompts live in `actions.rs` and nowhere else.** The interface fetches them through `list_built_in_actions` instead of keeping a copy, so what Settings shows, what Reset restores and what a recording runs cannot drift apart. `action_overrides` stores only what the user replaced, `name` and `prompt` independently, so an action that was merely renamed still benefits when we improve its instructions. A blank replacement resolves back to the default rather than failing the recording that used it. Resolution is in Rust, with the settings `finish_recording` already loads, which is why the tray, the dictation key and `--toggle` run the edited prompts without the frontend sending prompt text.

**Vocabulary is one field feeding two engines.** GPT Transcribe takes it as `keywords[]`; local Whisper takes it joined as `initial_prompt`. The API refuses a keyword containing `<`, `>`, CR or LF and refuses the whole request with it, so the interface names a term it would drop and `usable_keywords` filters again before sending. `text_effort` becomes `reasoning.effort` and is sent only when chosen — Auto keeps 0.4.1 behaviour and cannot be rejected by a model that does not offer the level.

**Settings is a tablist now** — Voice, Prompts, Output, General. Tests reach a control by clicking its tab first; `showSettingsTab` in `src/App.test.ts` is the way in.

**`gpt-transcribe` has no effort parameter.** Jonas asked for one alongside the vocabulary; the current API gives transcription `keywords`, `prompt` and `languages` and nothing else. Effort is `reasoning.effort` on the Responses call that runs Clean/Polish/Summarize/Prompt/Email, which is where it now sits — under **Prompts → Text model**, not under the microphone. Do not move it back without re-reading the transcription reference.

**[v0.4.2](https://github.com/jli-software/utterform/releases/tag/v0.4.2) is published and its assets independently verified** ([release run 34138984113](https://github.com/jli-software/utterform/actions/runs/34138984113), all three platforms, from `d1461a5`). This is the state to build on and debug from; the next session starts here.

Not yet used by a person: everything in 0.4.2. It is tested (70 Rust, 53 Vitest, 11 Playwright) but the words that matter — does a rewritten prompt read the way Jonas wants, is the Email default worth keeping as shipped, does "Careum" come back spelled correctly once it is in the vocabulary — are for a real recording to answer. The reasoning-effort levels have never been sent to a live model either. **Start the next session by asking Jonas what his own use turned up**, before adding anything.

### Before 0.4.2

0.4.0 made dictation work without the window, on Omarchy. 0.4.1 gives Windows and macOS the same key and fixes the text arriving damaged.

**One intent, two ways in.** `hotkey.rs` reserves a key combination where the platform grants one — Windows, macOS, Linux under X11 — and emits the same `remote-intent` event that a compositor binding produces through `cli.rs` and the single-instance plugin. The interface therefore keeps exactly one recording state machine. Wayland registers nothing and Settings says what to bind instead. The plugin is installed from `setup`, not at build time: a host that will not create a hotkey manager must cost the dictation key, not the application.

**Typing at the cursor pastes by default.** Jonas reported terminals losing letters — "Session" arriving as "ession" — while a manual paste of the same text was intact. That is delivery, not transcription: keystrokes cross the compositor, the input method and the target application one character at a time, and any of them can drop or reorder one. A paste moves the whole text at once. `typing/` chooses the chord from the focused window's class, because Ctrl+Shift+V pastes in a terminal and does something else entirely in a browser or an editor.

Keystrokes stay selectable, with the two documented fixes: a leading `Shift_L` press/release for the Wayland clients that swallow the first character a fresh virtual keyboard sends, and a delay between keys.

**Windows types through `SendInput`.** No helper program, Unicode rather than scan codes, one atomic call per batch. `typing/mod.rs` holds the text→key rule so it is tested on every platform; `typing/windows.rs` holds only the unsafe glue.

**Confirmed on Omarchy by Jonas on 2026-09-07.** The dropped letters are gone: the finished text arrives in the focused window intact. [v0.4.1](https://github.com/jli-software/utterform/releases/tag/v0.4.1) is published and its assets independently verified. This is the state to build on and debug from; the next session starts here.

### Platform reality — do not assume parity

All three platforms have now been exercised by a person; macOS since 0.7.0 beta 1, confirmed by Jonas on a MacBook Air (M2, macOS 26) on 2026-09-10. Windows cannot be compiled on the Linux development machine (`ring`'s build script needs a C compiler for the target), so every change to `cfg(target_os = "windows")` code is verified through Actions → Desktop builds on the branch before it is tagged.

| | Linux / Omarchy | macOS | Windows |
| --- | --- | --- | --- |
| Typing at the cursor, as a paste | **works, confirmed 2026-09-07** | **works, confirmed 2026-09-10 (0.7.0 beta 1)**, ⌘V after the Accessibility grant | **works, confirmed 2026-09-08 (0.4.6)**, Windows Terminal included; elevated windows are refused with a warning |
| Typing at the cursor, as keystrokes | 0.4.0 worked; not re-tested since the Shift tap and delay | implemented in 0.7.0 beta 1; Jonas confirmed typing without saying which method | implemented, never tried by a person |
| Reserved dictation key | n/a under Wayland, by design | **works, confirmed 2026-09-10**, including changing it in Settings | **works, confirmed 2026-09-08**, including changing it in Settings (`Alt+C`) |
| `utterform --toggle` reaching the running app | works, confirmed | plugin supports it, never tried | plugin supports it, never tried — the key is what Jonas uses |
| Binding it to a key | `bind =` in hyprland.conf | Settings → Dictation key | Settings → Dictation key |
| Tray click opens the window | works, confirmed | never tried | the recording dot is confirmed (0.4.4, smaller since 0.4.5); the click was not mentioned |
| Recording, transcription, clipboard, file | works, confirmed | **works, confirmed 2026-09-10** (microphone dialog, GPT Transcribe, Local Whisper); 0.6.1 never asked for the microphone | **works, confirmed 2026-09-08** (dock and webcam microphone, Jabra headset) |

Jonas tested Windows on 2026-09-08 across 0.4.4 to 0.4.6; the answers are in the handoffs above. If a paste does not arrive in some window, read the `pasting into window class …` line in the log first: it names the class, the program and the chord. An elevated window is refused by design; an unrecognised terminal is added to `typing::TERMINALS`; the last fallback is Settings → Typing at the cursor → Keystrokes.

The misleading "needs a graphical session" message on Windows is gone — the platform is implemented. macOS typing arrived in 0.7.0 beta 1; what it needs and how it is built on the Mac mini is in `MACOS.md`.

### Waiting, not released

Pull request #8 (`feat/robustness-and-models`, CI green, mergeable) carries two things Jonas asked for but has not released, because he wanted to test 0.4.0 first:

- Robustness: a command answers even when its work panics (`resilience.rs`), bounded retries with backoff for rate limits and server faults (`openai::retry_delay`), and a 30-minute ceiling in the interface (`lib/ceiling.ts`).
- The larger offline models: Medium, Large v3 Turbo, and the quantized Large v3 Turbo.

**It bumps the version to 0.4.1, which is already released.** Jonas asked for the Windows dictation key as 0.4.1 and asked for it first; 0.4.2 is now taken as well. Re-target #8 to 0.4.3 before merging it; do not release it unasked.

### Windows distribution — signing and winget

Windows builds are unsigned, and staying unsigned is a deliberate decision, not an
oversight. Researched on 2026-09-07 for Jonas as a Swiss sole proprietor
(*Einzelfirma*):

- **Azure Artifact Signing** (the renamed Azure Trusted Signing) is the cheapest
  CI-capable route at roughly CHF 100/year, but Microsoft restricts *Individual
  Developer* accounts to the USA and Canada. A Swiss Einzelfirma only qualifies
  through *Organization* identity validation, which requires an Azure billing
  account of that type and an officially verifiable registration. Not confirmed
  for this business; it has to be tested by actually running the validation.
- **SSL.com EV Sole Proprietor + eSigner** (roughly CHF 485/year) is the
  fallback that explicitly covers sole proprietors, lists Switzerland, and
  documents GitHub Actions.
- Signing does **not** silence SmartScreen immediately in any case. Reputation
  accrues to the signing identity over downloads.

The decision: do not buy a certificate at current download volumes. Revisit if
users report abandoning the install because of the warning.

Instead, distribute through **winget**, which is free and needs no certificate.
Manifests for 0.4.2 are prepared and validated in `packaging/winget/`; see that
directory's README for the submission command and what is still open. The
package is **not submitted yet**, so the README must not advertise
`winget install` until the pull request is merged.

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

**The release title appears once.** `docs/releases/<tag>.md` opens with `# Utterform X.Y.Z — …`, which `release:check` requires and the docs link to. The Release workflow takes that line as the GitHub release title and publishes the file *without* it as the body, because GitHub shows the title above the body already; 0.3.x to 0.4.5 had it twice, and Jonas asked on 2026-09-08 that it stop. Do not repeat the title inside the body, and do not remove it from the file.

Push/PR `CI` runs frontend checks, production UI tests and native Linux fmt/Clippy/tests, without release-profile compilation or packaging. `Desktop builds` is manually runnable for Linux, Windows, macOS or all; it produces artifacts but never publishes. Manual dispatch becomes available only after the workflow reaches the default branch; tag releases already call it directly from their tagged source. The tag-triggered `Release` workflow validates metadata and calls `Desktop builds` once for all three supported targets. It requires all six binary/installer assets plus a combined checksum manifest before publication. Normal `vX.Y.Z` is Latest; explicit prerelease tags do not replace Latest. Stable download filenames are retained.

For each completed change set, update package.json/package-lock.json, Cargo.toml/the Utterform Cargo.lock entry, tauri.conf.json, the Linux installer's default tag, README, changelog and `docs/releases/<tag>.md`. Run `npm run release:check -- <tag>` and the normal test suite, commit/push, then create and push the new tag. Wait for the Release workflow and verify its downloadable checksums/asset set. Never move a published tag; use a new version for later corrections. Validation-only follow-up documentation for an already verified release does not need an otherwise identical new application release.

Before starting work on another device/assistant, pull `main`, read this file, `CHANGELOG.md`, and `docs/ARCHITECTURE.md`, and inspect the latest Actions result. Project-wide decisions stay here; machine-specific setup and user data stay outside Git.

The [four-target CI run](https://github.com/jli-software/utterform/actions/runs/34027520241) passed, including all installers/archives. The downloaded Linux CI binary also passed an isolated native startup/tray-Quit smoke test. See [TESTING.md](TESTING.md) for exact coverage and an open, intermittent WebKit subprocess shutdown observation on the local Omarchy runtime. No fix for that non-reproducible observation is claimed.

Beta 1 was published with all assets and verified checksums. Final asset inspection found its Windows executable used the console PE subsystem. Beta 2 corrects that desktop-only issue and adds a binary-level CI assertion; Beta 1's tag is left intact. Previous published release: [`v0.2.0-beta.2`](https://github.com/jli-software/utterform/releases/tag/v0.2.0-beta.2). All four release jobs passed; all downloaded assets, the Linux installer, the Windows GUI subsystem, and both macOS architectures were verified. The next work is beta-user feedback and the open WebKit shutdown observation in `TESTING.md`, not unfinished release packaging.

Native interactive microphone/speaker and macOS/Windows desktop tests must be distinguished from mocked browser tests and cross-platform compilation.
