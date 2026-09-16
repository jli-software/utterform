# Architecture

Utterform uses Tauri 2 as its desktop shell, Rust for all privileged or compute-heavy work, and Svelte with TypeScript for the interface.

## Boundaries

- `audio.rs` — device discovery, CPAL capture, bounded handoff, RMS envelope, native cutoff, temporary WAV lifecycle, Whisper normalization
- `feedback.rs` — synthesized start/stop clicks and distinct post-delivery Done chime through CPAL output, played to completion on outputs that suspend when idle; the productive Done carries a generation the next recording makes stale
- `platform.rs` — Omarchy-only native window-decoration policy
- `activation.rs` — the single way to reveal the one main window, and which tray gestures ask for it; on macOS the app is an accessory (menu-bar) application for good, so revealing also activates it
- `tray.rs` — Tauri's native tray on Windows/macOS, an own StatusNotifierItem on Linux so a left click arrives; both show a red dot while recording
- `actions.rs` — the prompts Utterform ships with, and how a user's replacement resolves against them
- `transcription/openai.rs` — GPT Transcribe and Responses API calls
- `transcription/local.rs` — blocking local Whisper inference
- `packaging/cmake/portable-cpu.cmake` — the instruction set whisper.cpp is compiled for, and why it is not the build machine's
- `models.rs` — curated model catalog, downloads, progress events, and SHA-256 verification
- `cli.rs` — what a command line or a second launch asks the running app to do
- `hotkey.rs` — a reserved key combination where the OS grants one, routed into the same intent as the command line
- `src/lib/shortcut.ts` — reading a key combination off the keyboard in the one spelling `hotkey.rs` parses
- `typing/` — putting the finished text into the focused window: paste or keystrokes, through wtype/xdotool on Linux and `SendInput` on Windows
- `output.rs` — independent clipboard, file and cursor delivery
- `settings.rs` — non-secret JSON settings
- `history.rs` — bounded, atomic, device-local text history; local titles, no AI calls
- `secrets.rs` — OS credential-store access
- `commands.rs` — thin Tauri command boundary; owns the Record → Transcribe → Transform → Deliver events of a batch recording
- `diagnostics.rs` — the single diagnostic boundary: event format, levels, ids and references, `Failure`, redaction (`diagnostics/redact.rs`), the bounded support report (`diagnostics/report.rs`), run marker, panic hook and the support actions
- `src/lib/diagnostics.ts` — the interface's bridge for failures no command saw
- `src/` — presentation and focused-window keyboard interaction

The frontend never receives an API key or temporary audio path. Network calls originate in Rust.

## Recording lifecycle

1. Resolve the selected device by CPAL's stable device ID, falling back to the system default.
2. Capture native `f32`, `i16`, or `u16` samples.
3. Send small sample chunks over a bounded channel to a WAV writer thread.
4. Stop capture and finalize the temporary WAV.
5. Send the WAV to GPT Transcribe, or normalize it to 16 kHz mono `f32` for local Whisper.
6. Optionally transform the transcript with a separate text-model request.
7. Persist the completed text to local history (if enabled), before external delivery.
8. Deliver to each selected output independently.
9. Delete the temporary recording when the artifact leaves scope.

## Background capture & feedback

Capture is owned by Rust/CPAL, not the WebView. Focus changes, minimization, and closing the window to tray do not stop a recording. Explicit Stop processes it, Escape discards it, and tray Quit discards active audio before exiting. No microphone capture starts merely by launching the app. The in-window keys stay focused-window shortcuts; global dictation is a separate path described below.

A backend's buffer under- or overrun report (`ErrorKind::Xrun`) is a gap in a stream that keeps running — Windows raises it through `AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY`, macOS through a processor-overload listener — and is counted, not fatal. Only an error meaning the stream is gone ends a recording. `note_stream_error` is the one place that decides.

Pause/resume is an explicit, idempotent `set_recording_paused` command returning native recording status. CPAL stays open for cross-platform reliability; an atomic gate discards paused callbacks before conversion/allocation/writing. No silent gap is inserted. The OS may continue to show its microphone-use indicator. A native clock excludes paused intervals from elapsed time and the cutoff while keeping session identity unchanged. Stop can consume paused audio; Escape and tray Quit still discard it. The UI serializes pause commands, ignores stale polling responses, and handles a watchdog-completion race without processing twice.

A native watchdog checks the active session every 250 ms and finalizes capture at ten minutes of unpaused recording, even if the WebView is suspended. The completed artifact stays in native state until processing consumes it once. An event starts processing immediately when the WebView is running; status polling catches up after a hidden/suspended window resumes. Session identity prevents an old watchdog from stopping a later recording. The UI reads native elapsed time rather than incrementing a JS timer.

Only a bounded RMS-derived envelope crosses IPC, at most 10 times per second while visible and recording; no raw audio reaches the frontend. The full-window violet/blue ambient field responds to this envelope behind stationary controls. Reduced-motion mode disables field movement. Cues are generated locally, quiet and short, and can be disabled in Settings.

A cue is played to the end rather than for a fixed time. An output device that suspends when idle — Bluetooth, HDMI, most docks — needs a few hundred milliseconds to carry any sound and queues 150-250 ms more, and closing a CPAL stream discards what the device still holds. So every cue carries a 60 ms silent lead-in and a 300 ms silent tail, and the stream stays open until the device has taken all of it; the three-second deadline is a safety net, not the expected wait. Every cue is opened, played, finished and closed on a thread of its own, never on the thread that answered the interface. The start cue's output stream is opened once the microphone is running, because opening a capture stream can reconfigure the device that plays the cue; capture starts immediately and discards until that cue has been played, which is what keeps it out of its own recording and what the recording clock counts from. Every cue outcome is written to the log. A speaker failure still never fails a recording, but it is reported rather than dropped: a start cue that could not be played raises a desktop notification in its place, because a hidden window offers nothing else.

The Done cue is the one cue nothing waits for. `finish_recording` — batch and live alike — returns as soon as history has been attempted and every requested output has run; the Done cue is started then, on its own thread, through `feedback::DoneCues`, and the interface is ready for the next dictation the moment the command answers, while the cue and the ribbon's release run on. Only one productive Done exists at a time: each carries the generation it was issued in, issuing the next makes the previous stale, and every real recording start — `commands::begin_recording` for batch and local, `live::start` for GPT Live — makes them all stale before its own start cue is scheduled. A stale cue hands the device silence from its next buffer, its thread closes the stream, and the log says it was silenced and when. Making a cue stale is one atomic increment that never waits for the audio thread, the device or the deadline; dropping a `Playback` handle does not do it. Start, Stop and the cues Settings plays carry no generation and are never cut short. Together with the start cue's own arming rule — capture discards until the start cue has been played — this keeps the tail of the previous Done out of the next recording. `processing` in the interface still covers transcription, transformation, the history attempt and every requested output; an intent that arrives during it is dropped, never queued, and the first one after the answer starts exactly one recording.

Omarchy is detected only in a Hyprland desktop session with an Omarchy installation/path. Native decorations are disabled before first showing the window. Other desktops, macOS, and Windows retain their standard decorations. No compositor config or automatic Omarchy theme integration is added.

## Shared visual identity

`src-tauri/icons/app-icon.svg` is the single source for the in-app brand (imported by Vite), Settings header, and generated desktop icons. Run `npm run icons` after changing it. The generator uses Tauri's renderer, copies only desktop assets, and canonicalizes ICNS chunk order for byte-stable regeneration. The app identifier, executable name and storage paths remain unchanged. The tray continues to use Tauri's default window icon.

Settings uses the same theme tokens and custom `SelectMenu` as the main controls, including microphone/model selection. Opening motion is disabled under reduced motion; keyboard focus is contained and restored, and Escape closes an open selector before closing the dialog.

The dialog is a tablist — General, Recording, AI & Models, Actions, Output — with arrow-key navigation and roving focus; the rail lies down above the panel below 760 px. General is the initial tab. Cancel restores the preference snapshot across every tab; explicit resource operations (model downloads/removal, API key removal, confirmed history deletion and the three support actions at the end of General) remain immediate. Closed selectors show labels, with opt-in hints for missing local models; option descriptions and history timestamps remain available in the open menu. Recording status is announced by its visible status label, without duplicate helper text.

## Prompts

`actions.rs` holds the shipped prompts and is the only place their text exists. The interface fetches them through `list_built_in_actions` rather than keeping a copy, so what Settings shows, what *Reset* restores, and what a recording runs cannot drift apart.

`AppSettings::action_overrides` stores only what the user replaced, per action id, with `name` and `prompt` independently optional: an action that was renamed still follows a later, better default prompt. A blank replacement resolves back to the default in `actions::instructions`, so an emptied box cannot fail the recording that used it. Resolution happens in Rust with the settings a recording already loads, which is why the tray, the dictation key and `utterform --toggle` run the edited prompts without the frontend sending any prompt text.

`plain` is in the same list with no prompt at all: it is shown in the editor as the action that never reaches a text model, and has nothing to edit.

## What the local engine is compiled for

whisper.cpp is built from source by `whisper-rs-sys`, and ggml's CMake targets the machine doing the compiling unless told otherwise. For a release built on someone else's server that is a trap rather than an optimization: the 0.7.2 Linux binary carried the runner's AVX-512 and AMX instructions and was stopped with SIGILL on an Intel N300 while loading a model — inside `ggml_backend_cpu_device_get_extra_buffers_type`, plain C++ frame code, which is before ggml's own runtime dispatch can choose a kernel and therefore beyond its help. The bug had been there since the first release; only the pairing of build machine and user machine decided whether it fired.

`packaging/cmake/portable-cpu.cmake` sets `GGML_NATIVE=OFF`, which makes ggml use its explicit defaults instead: SSE4.2, AVX, AVX2, FMA, F16C and BMI2 on x86-64, with AVX-512, VNNI and AMX left out; on Apple Silicon clang already targets `apple-m1`, the oldest Mac supported. The file is deliberately free of `CMAKE_SYSTEM_NAME`, which would mark the build as cross-compiling and drop ggml to plain x86-64 with no AVX2 at all. It arrives through `CMAKE_TOOLCHAIN_FILE` in `.cargo/config.toml`, because `whisper-rs-sys` accepts no defines from its dependents.

`cmake-rs` never tells Cargo which environment variables it read, so a cached `whisper-rs-sys` survives a change to that file — which is why the CI cache key carries a suffix to retire one, and why two checks look at the result rather than at the intent: `scripts/test-linux-system-package.sh` disassembles the packaged binary and refuses AVX-512 or AMX inside a ggml or whisper symbol (other crates carry such code too, but reach it through a CPUID check first), and a test in `local.rs` reads the compiled feature set through `whisper_print_system_info` on every platform. The same line is written to the log the first time a local transcription runs.

## Transcription context

`vocabulary` is a list of literal terms. GPT Transcribe takes them as `keywords[]`, the parameter the API offers for expected terms; local Whisper takes them joined as `initial_prompt`, its own way of biasing a spelling. The API refuses a keyword containing `<`, `>`, CR or LF — and refuses the whole request with it — so terms are entered one per line, the interface names one it would have to drop, and `usable_keywords` filters them again before sending. `transcription_context` is the API's free-form `prompt` and reaches GPT Transcribe only.

`text_effort` becomes `reasoning.effort` on the Responses call, and is sent only when the user chose a level. Left on Auto nothing is sent, so a text model that does not offer the level we would otherwise have guessed cannot reject the request.

## Text history

`history.json` lives in Tauri's app-local-data directory, separately from settings and exported files. The newest 100 texts are retained, newest first, including clipboard-only results. Titles are the first words of the text (Unicode-safe, at most 64 characters plus ellipsis), without timestamps or additional model calls. The UI restores the latest entry and keeps the displayed text while another recording runs.

New entries include `createdAtMs` (Unix milliseconds). Missing timestamps from older histories are recovered from their Unix-nanosecond IDs at read time, without rewriting files on load; the next normal append persists the compatible additional field. The UI uses local calendar days and `de-CH` formatting: `Today · HH:mm · N min ago` today, `DD.MM.YYYY` on earlier days, with full date/time in the result's tooltip. A lightweight timer refreshes relative ages every 15 seconds. Unknown/invalid dates are labelled unavailable instead of guessed.

Writes are serialized with a mutex, use an owner-only temporary file in the same directory, flush data, and atomically replace the old file. Unreadable/corrupt history is never silently overwritten; the UI still receives the new text with a persistence warning. History is unencrypted and local to each device, not stored in Git or synchronized by the app. Users can disable future history and separately confirm clearing existing history in Settings. Clearing does not touch the clipboard or exported files.

## Global dictation

One intent, two ways in. A compositor binding runs `utterform --toggle` and the single-instance plugin hands the command line to the running app; a reserved key combination fires in the backend. Both emit the same `remote-intent` event, so the interface owns exactly one recording state machine and every guard against double starts applies to both.

Key combinations are reserved only where the platform grants them: Windows, macOS, and Linux under X11. A Wayland session registers nothing — the compositor owns the keyboard — and Settings shows what to bind there instead. The plugin is installed from `setup` rather than at build time, so a host that refuses to create a hotkey manager costs the dictation key and not the application; a key another application already holds is reported in Settings, where it was entered, and the previously working key is released only after the new one parses.

Neither path raises the window. The audio cues are therefore the confirmation, and they come from Rust rather than the WebView, so they sound whether or not the window is visible. Because the start cue is the only sign the microphone is live, it is played to completion before capture keeps anything, and a notification stands in when no output device will play it. The tray icon carries a red dot for as long as a recording runs, set from the same commands that start and end capture, so there is a confirmation that does not depend on a speaker at all.

The combination itself is recorded rather than typed. `shortcut.ts` turns a `keydown` into the string `hotkey.rs` parses and nothing else: the physical key comes from `KeyboardEvent.code`, so a layout cannot change which shortcut a key produces, and every token it emits — `A`–`Z`, `0`–`9`, `Space`, `F9`, `Up`, `Comma`, `Super` — is one `global-hotkey` already accepts, which is why what is shown, what `settings.json` holds and what is registered are one string. Rust still validates on save; there is no second syntax. A bare key is refused in the field for the same reason it is refused in `parse`. `ShortcutRecorder.svelte` reads keys in the capture phase on the window, so the dialog's Escape, the focus trap and the main window's shortcuts cannot take a key meant for the recorder, and `Escape` ends the reading alone.

A shortcut the operating system is holding for Utterform would never reach the field it is being replaced in, so the reading releases it — `apply_global_hotkey(null)` — for exactly that moment and registers it again when the reading ends, in a serialized queue Save waits on. A new key is still only made permanent by Save. On Wayland nothing is registered and nothing is suspended; a compositor binding that fires mid-reading is turned away in the interface instead.

## Starting with the session

`tauri-plugin-autostart` owns the entry: HKCU Run on Windows, a per-user LaunchAgent on macOS, `~/.config/autostart/Utterform.desktop` on Linux. The entry is the only state — no field in `AppSettings` mirrors it — so Settings asks `isEnabled` when it opens, writes only a change and only on Save, and asks again afterwards rather than trusting the call. A system that will not answer costs the switch and nothing else.

Every entry carries `--autostart`, and that argument is the reason it can exist: `cli.rs` reads an unknown or bare command line as `Show`, so without an intent of its own a login would open the window in front of whatever the user signed in to do. `Intent::Autostart` raises no window, is never stored as a startup intent, and is never emitted as `remote-intent` — so it does nothing on this instance and nothing at all when it meets a running one. On Linux it also settles the graphics backend before GTK exists: every other Linux start goes through the launcher the installer writes, which pins `GDK_BACKEND=x11`, and an autostart entry runs the executable directly, so the same policy is applied in `run()` for that one kind of start and a backend the session chose is left alone.

## Delivery to the focused window

Clipboard, file and cursor are independent, and typing runs last so a delivery failure there costs a warning rather than the text.

Paste is the default because synthesized keystrokes cross the compositor, the input method and the receiving application one character at a time, and each can drop or reorder one when input arrives faster than a person could type — the terminal that turns "Session" into "ession". A paste moves the whole text at once, so there is nothing to reorder. The chord depends on the window: terminals reserve plain Ctrl+V for the shell and take the text on Ctrl+Shift+V on Linux and Shift+Insert on Windows, while those chords mean something else entirely in a browser or editor, so the focused window decides. Hyprland answers over its own socket, X11 through xdotool, Windows through the foreground window's class and the program behind it; an unrecognized window gets the chord every graphical toolkit agrees on. On Windows every key of a chord carries its scan code and names the left-hand modifier, because Windows Terminal reads both and ignores a bare virtual key. Guessing wrong costs a paste that does not arrive, never the text.

Paste delivery is told whether the clipboard output already ran, so it neither writes the transcript twice nor takes the clipboard when it did not have to. When it does write, the text stays there — restoring the previous contents would race the receiving window's read and could paste the wrong text.

Keystrokes stay selectable for the windows that refuse a paste, with a leading Shift press and release for the Wayland clients that swallow the first character a fresh virtual keyboard sends, and a delay between keys, clamped so a hand-edited settings file cannot stall delivery for minutes. Text always reaches wtype and xdotool on stdin, so a transcript starting with a dash is never read as options.

Windows needs no helper program: one `SendInput` call appends its whole batch to the input queue atomically, so both methods deliver the text intact. Characters go in as Unicode rather than scan codes, making the transcript independent of the active keyboard layout, and every line ending becomes exactly one Return. Windows blocks input from a normal process to an elevated window; that is reported rather than counted as success.

macOS posts CoreGraphics keyboard events (`typing/macos.rs`): ⌘V for a paste, since a Mac terminal takes the same chord as every other window, and one Unicode keyboard event per character for keystrokes, paced by the delay. The window server drops events from a process without the Accessibility grant and says nothing, so `macos.rs` checks the grant first, opens the system request on the first delivery, and reports the refusal as a warning. The same module refuses to record through a microphone macOS has denied, and `Entitlements.plist` carries the audio-input entitlement the hardened runtime requires before macOS asks for the microphone at all. See `MACOS.md`.

## Diagnostics

Diagnostics are local, bounded and free of user content, and `diagnostics.rs` is the only code that knows how they are written.

**Storage.** `tauri-plugin-log` 2.9 supplies the filtered logger and writes to stderr; Utterform's bounded writer stores `utterform.log` in Tauri's log directory (`app_log_dir`: `$XDG_DATA_HOME/software.jli.utterform/logs` on Linux, `~/Library/Logs/software.jli.utterform` on macOS, `%LOCALAPPDATA%\software.jli.utterform\logs` on Windows). It checks the size after every record and rotates at 2 MiB to `utterform.1.log` and `utterform.2.log`, so the disk budget is about 6 MiB. It buffers at most one already-bounded record and drops that record after a write or rotation failure, preventing a full disk or a removed/locked log directory from growing process memory without limit. The level is `INFO` in release builds and `DEBUG` in debug builds, and a filter admits only the `utterform`/`utterform_lib` targets and the frontend bridge's `utterform::frontend` — dependency output is never written. The logger is built with the plugin's `split` and attached in `setup` before anything else runs, rather than registered as a plugin: its only other content is a JavaScript `log` command the WebView must not reach, and a log directory that cannot be created then costs the file instead of the launch (events still reach stderr and `diagnostics.file_unavailable` says why). The 0.7.8 `utterform.log` in app-local-data is not moved, deleted or written.

**Events.** One line per event: `2026-09-16T08:00:00.123Z INFO  utterform run=1a2b3c4d recording.opened recording=3 device="Studio microphone" open_ms=42`. Names are stable and dotted (`app.*`, `recording.*`, `transcription.*`, `transform.*`, `history.*`, `delivery.*`, `live.*`, `cue.*`, `settings.*`, `hotkey.*`, `models.*`, `secrets.*`, `support.*`, `window.*`, `tray.*`, `typing.*`, `frontend.error`); fields are `key=value`, quoted and escaped when they contain whitespace, quotes, `=` or backslashes. Every line carries `run`, eight hex digits for this process, so a file shared by several runs stays attributable. Every batch and Live recording gets a process-local `recording` id from one counter, carried through `StartedRecording`, `ActiveRecording`, `RecordingArtifact` and the Live `Session` — separate from the `Instant` that identifies a capture session. A run of a recording therefore reads as `recording.requested` → `recording.opened` → `recording.armed` → `recording.stopped` → `transcription.started/completed` → `transform.started/completed` or `transform.fallback` → `history.saved` → `delivery.completed` → `recording.completed`, and a Live one adds `live.input_ready`, `live.connected`, `live.streaming`, `live.stream_completed` and the input worker's events.

**Failures and references.** Low-level code returns a `diagnostics::Failure`: the message the user reads, kept apart from a closed-vocabulary `class`, an optional HTTP `status` and an optional `detail` (an `io::ErrorKind`, a keyring variant, an audio backend's error). `Failure` has no `Display`, so its message — which can quote an OpenAI error body — cannot become a log field by accident. The code that knows the context owns the one event: `failure!` logs `ERROR`, `fallback!` logs `WARN` for work that continued without that part, and both append `(ref <run>-<n>)` to the message; the same reference is the event's `ref` field. Guidance the user acts on (add a key, choose an output, a model not yet downloaded, install `wtype`, grant Accessibility, start Live from another application's text field) is logged at `INFO` with `outcome=guidance` and no reference; `typing::is_guidance` and `typing::is_capture_guidance` recognise the platform modules' own refusal sentences for that. Commands that only pass a failure on do not log it again.

**Privacy.** Allowed: version, OS/architecture, desktop and session kind, run/recording/reference ids, durations, counts, enabled modes, model and action ids (a custom action is only `custom`), HTTP statuses, microphone and output device names, program/application names, window classes and bundle ids, error classes, and the platform typing modules' own refusal sentences, which name tools, programs and counts. The text model is named only when it looks like a model id; anything else typed into that field is `custom`. Never: API keys or authorization material, transcripts, history, clipboard or output-file contents, audio, prompts and prompt overrides, vocabulary, language hints, recording context, window titles, request/response bodies, WebSocket deltas, raw third-party errors, full settings or history JSON, output folders (a saved file is named by its basename). Linux Live focus events keep their existing rendering: event name, window address, workspace id. Every record is still normalized by `redact::line` — one physical line, no control or direction-override characters, home directories reduced to `$HOME`, `sk-…` keys, bearer tokens and `Authorization:` values replaced — and bounded to 16 KiB; a copied report applies the same again, legacy lines included. The sanitizer cannot recognise dictated prose; the protection is that it never reaches diagnostics.

**Bounds.** Nothing is logged from the audio callback, the writer loop, per delta, per keystroke, per animation frame or per status poll: glitches, delivered chunks, WebSocket events and audio bytes are counted and reported once. `Limiter` keeps Linux Live focus events at 200 per session and reports `live.focus_events_suppressed`; the frontend bridge admits 50 reports per run (20 per mounted interface on its own side); a failing `get_recording_status` is reported once per run; undelivered model-download progress events are counted and reported once.

**Unclean exits and panics.** `setup` reads `utterform.running` in app-local-data: present means the previous run never reached `RunEvent::Exit` (`app.previous_run_unclean`, and the report header says so), then it is atomically replaced with this run's id. `RunEvent::Exit` writes `app.exiting` and removes the marker if it is this run's. Tray Quit still discards capture first. A force-kill or power loss is therefore reported as unclean, by design; a marker that cannot be read or written is logged and never blocks startup. The panic hook, installed once the logger is attached and chained to the previous hook, records thread, source location (build-machine paths reduced), the payload without the text std quotes when a string is sliced at the wrong place (`is not a char boundary`, `out of bounds of`, `when slicing`), and a captured backtrace without the panic machinery's own frames, bounded to 8 KiB; it guards against recursion. Aborts, segmentation faults and native driver crashes never reach a hook: they appear only as an unclean previous run, and no crash dumps are collected.

**The interface.** `App.svelte` registers `error` and `unhandledrejection` listeners on mount and removes the same handlers on destroy. `createFailureReporter` sends a source category, the error's name and message (500 characters), a stack without the serving origin (2,000 characters) and the current phase — never the rejection value itself, which becomes `Non-error value of type …` — and swallows every failure of its own, so logging cannot cause the next rejection. A command rejects with a string that already carries its reference, so only an `Error` thrown in the interface is reported from `setError`; autostart plugin failures, which no Utterform command sees, are reported explicitly. The backend accepts only known source and phase values, reduces origins again and returns the reference shown beside the message.

**Support actions.** `diagnostics_info` returns the backend's own resolved path; `copy_diagnostics`, `open_log_file` and `open_log_folder` take no arguments at all. The report is built on a blocking thread from the tails of the current, rotated and legacy logs — never settings, history, recordings or exports — newest first, reading only as many bytes as fit, dropping a partial first line so no UTF-8 sequence is split, and stopping at the first file that no longer fits whole so the kept lines are one unbroken stretch. The header names format version, generation time, version, OS, architecture, desktop, run id, previous-run state, log path and suppressed frontend reports; the whole text never exceeds 256 KiB and says when earlier lines were omitted. Opening uses `tauri-plugin-opener`'s Rust API — registered without its link-click script and without any JavaScript permission — `open_path` for the file and `reveal_item_in_dir` for the folder, falling back to opening the folder where the desktop offers no way to select a file. Settings disables only the action in flight, shows its result or failure in the group, and leaves Save and Cancel out of it.

**Known limits.** Diagnostics are best-effort. If the current log cannot be written or rotated — for example because the disk is full, the directory was removed or Windows has locked a file — that record is still written to stderr but may be absent from the files and copied report. The next record retries; no failed record is retained in memory.

## Deliberate MVP constraints

- Batch transcription only
- One dictation key, not a set of separately bindable global shortcuts
- Autostart on or off, with no separate "start minimized" choice: a login start is always silent
- No global dictation key under Wayland; the compositor binding covers it
- No Live Dictation on macOS
- CPU local inference by default
- No updater
- No files-as-clipboard-objects
- No local LLM for transformations
- No telemetry, upload or crash dumps: diagnostics stay on the device, and a native crash shows only as an unclean previous run

GPU backends and release signing require separate platform work and testing.

## Live dictation

`live.rs` owns one numbered session at a time: an OpenAI Realtime transcription socket, the microphone's live PCM tap and a dedicated native input thread. Deltas are appended in order, split into short chunks and handed to the input worker over a bounded queue; nothing is retried or replayed, and a final transcript that differs from the live text stays in Utterform. The worker owns the platform typer (`typing/live.rs`) for the whole session and drops it before the next session may start; between deltas it only reads queued focus events, and each chunk confirms the target once before typing. A stop request holds typing for 700 ms so a still-held shortcut modifier cannot combine with typed keys.

On Omarchy/Hyprland the typer is a persistent Wayland virtual keyboard plus Hyprland's event socket. Only a confirmed change of the active window address or the target closing is a loss; workspace, monitor, layer, submap and config-reload events are logged but ignored, and IPC failures are reported as technical faults. Because Hyprland matches bindings against a virtual keyboard by keycode through its own layout, characters are placed on keycodes without default bindings, editing, browser or media meaning, and with DOM codes so Chromium on Wayland accepts them; the map is seeded with the most frequent characters and extended on demand. Windows uses Unicode `SendInput` with WinEvent foreground/focus/desktop observation on the worker thread and waits for physically held modifiers directly.
