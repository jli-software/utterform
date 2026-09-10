# Architecture

Utterform uses Tauri 2 as its desktop shell, Rust for all privileged or compute-heavy work, and Svelte with TypeScript for the interface.

## Boundaries

- `audio.rs` — device discovery, CPAL capture, bounded handoff, RMS envelope, native cutoff, temporary WAV lifecycle, Whisper normalization
- `feedback.rs` — synthesized start/stop clicks and distinct post-delivery Done chime through CPAL output, played to completion on outputs that suspend when idle
- `platform.rs` — Omarchy-only native window-decoration policy
- `activation.rs` — the single way to reveal the one main window, and which tray gestures ask for it
- `tray.rs` — Tauri's native tray on Windows/macOS, an own StatusNotifierItem on Linux so a left click arrives; both show a red dot while recording
- `actions.rs` — the prompts Utterform ships with, and how a user's replacement resolves against them
- `transcription/openai.rs` — GPT Transcribe and Responses API calls
- `transcription/local.rs` — blocking local Whisper inference
- `models.rs` — curated model catalog, downloads, progress events, and SHA-256 verification
- `cli.rs` — what a command line or a second launch asks the running app to do
- `hotkey.rs` — a reserved key combination where the OS grants one, routed into the same intent as the command line
- `typing/` — putting the finished text into the focused window: paste or keystrokes, through wtype/xdotool on Linux and `SendInput` on Windows
- `output.rs` — independent clipboard, file and cursor delivery
- `settings.rs` — non-secret JSON settings
- `history.rs` — bounded, atomic, device-local text history; local titles, no AI calls
- `secrets.rs` — OS credential-store access
- `commands.rs` — thin Tauri command boundary
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

Omarchy is detected only in a Hyprland desktop session with an Omarchy installation/path. Native decorations are disabled before first showing the window. Other desktops, macOS, and Windows retain their standard decorations. No compositor config or automatic Omarchy theme integration is added.

## Shared visual identity

`src-tauri/icons/app-icon.svg` is the single source for the in-app brand (imported by Vite), Settings header, and generated desktop icons. Run `npm run icons` after changing it. The generator uses Tauri's renderer, copies only desktop assets, and canonicalizes ICNS chunk order for byte-stable regeneration. The app identifier, executable name and storage paths remain unchanged. The tray continues to use Tauri's default window icon.

Settings uses the same theme tokens and custom `SelectMenu` as the main controls, including microphone/model selection. Opening motion is disabled under reduced motion; keyboard focus is contained and restored, and Escape closes an open selector before closing the dialog.

The dialog is a tablist — Voice, Prompts, Output, General — with arrow-key navigation and roving focus; the rail lies down above the panel below 760 px. Cancel still restores the snapshot taken when the dialog opened, across every tab.

## Prompts

`actions.rs` holds the shipped prompts and is the only place their text exists. The interface fetches them through `list_built_in_actions` rather than keeping a copy, so what Settings shows, what *Reset* restores, and what a recording runs cannot drift apart.

`AppSettings::action_overrides` stores only what the user replaced, per action id, with `name` and `prompt` independently optional: an action that was renamed still follows a later, better default prompt. A blank replacement resolves back to the default in `actions::instructions`, so an emptied box cannot fail the recording that used it. Resolution happens in Rust with the settings a recording already loads, which is why the tray, the dictation key and `utterform --toggle` run the edited prompts without the frontend sending any prompt text.

`plain` is in the same list with no prompt at all: it is shown in the editor as the action that never reaches a text model, and has nothing to edit.

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

## Delivery to the focused window

Clipboard, file and cursor are independent, and typing runs last so a delivery failure there costs a warning rather than the text.

Paste is the default because synthesized keystrokes cross the compositor, the input method and the receiving application one character at a time, and each can drop or reorder one when input arrives faster than a person could type — the terminal that turns "Session" into "ession". A paste moves the whole text at once, so there is nothing to reorder. The chord depends on the window: terminals reserve plain Ctrl+V for the shell and take the text on Ctrl+Shift+V on Linux and Shift+Insert on Windows, while those chords mean something else entirely in a browser or editor, so the focused window decides. Hyprland answers over its own socket, X11 through xdotool, Windows through the foreground window's class and the program behind it; an unrecognized window gets the chord every graphical toolkit agrees on. On Windows every key of a chord carries its scan code and names the left-hand modifier, because Windows Terminal reads both and ignores a bare virtual key. Guessing wrong costs a paste that does not arrive, never the text.

Paste delivery is told whether the clipboard output already ran, so it neither writes the transcript twice nor takes the clipboard when it did not have to. When it does write, the text stays there — restoring the previous contents would race the receiving window's read and could paste the wrong text.

Keystrokes stay selectable for the windows that refuse a paste, with a leading Shift press and release for the Wayland clients that swallow the first character a fresh virtual keyboard sends, and a delay between keys, clamped so a hand-edited settings file cannot stall delivery for minutes. Text always reaches wtype and xdotool on stdin, so a transcript starting with a dash is never read as options.

Windows needs no helper program: one `SendInput` call appends its whole batch to the input queue atomically, so both methods deliver the text intact. Characters go in as Unicode rather than scan codes, making the transcript independent of the active keyboard layout, and every line ending becomes exactly one Return. Windows blocks input from a normal process to an elevated window; that is reported rather than counted as success.

macOS posts CoreGraphics keyboard events (`typing/macos.rs`): ⌘V for a paste, since a Mac terminal takes the same chord as every other window, and one Unicode keyboard event per character for keystrokes, paced by the delay. The window server drops events from a process without the Accessibility grant and says nothing, so `macos.rs` checks the grant first, opens the system request on the first delivery, and reports the refusal as a warning. The same module refuses to record through a microphone macOS has denied, and `Entitlements.plist` carries the audio-input entitlement the hardened runtime requires before macOS asks for the microphone at all. See `MACOS.md`.

## Deliberate MVP constraints

- Batch transcription only
- One dictation key, not a set of separately bindable global shortcuts
- No global dictation key under Wayland; the compositor binding covers it
- No Live Dictation on macOS
- CPU local inference by default
- No autostart or updater
- No files-as-clipboard-objects
- No local LLM for transformations

GPU backends and release signing require separate platform work and testing.

## Live dictation

`live.rs` owns one numbered session at a time: an OpenAI Realtime transcription socket, the microphone's live PCM tap and a dedicated native input thread. Deltas are appended in order, split into short chunks and handed to the input worker over a bounded queue; nothing is retried or replayed, and a final transcript that differs from the live text stays in Utterform. The worker owns the platform typer (`typing/live.rs`) for the whole session and drops it before the next session may start; between deltas it only reads queued focus events, and each chunk confirms the target once before typing. A stop request holds typing for 700 ms so a still-held shortcut modifier cannot combine with typed keys.

On Omarchy/Hyprland the typer is a persistent Wayland virtual keyboard plus Hyprland's event socket. Only a confirmed change of the active window address or the target closing is a loss; workspace, monitor, layer, submap and config-reload events are logged but ignored, and IPC failures are reported as technical faults. Because Hyprland matches bindings against a virtual keyboard by keycode through its own layout, characters are placed on keycodes without default bindings, editing, browser or media meaning, and with DOM codes so Chromium on Wayland accepts them; the map is seeded with the most frequent characters and extended on demand. Windows uses Unicode `SendInput` with WinEvent foreground/focus/desktop observation on the worker thread and waits for physically held modifiers directly.

