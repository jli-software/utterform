# Changelog

## 0.4.1 — v0.4.1

- Dictate from anywhere on Windows, macOS and X11 with a reserved key combination, `Ctrl+Alt+D` by default, configurable in Settings. The window is never raised; the start/stop clicks and the completion chime are the confirmation. Wayland keeps using `utterform --toggle`, because the compositor owns the keyboard there.
- Stop typing at the cursor dropping letters in terminals: deliver the text as one paste by default, choosing the paste a terminal listens for over the one every other window takes.
- Keep synthesized keystrokes as an option, and make them survivable — a leading Shift tap for the Wayland clients that swallow the first character, and an adjustable delay between keys.
- Type at the cursor on Windows through `SendInput`, with no helper program: Unicode rather than scan codes, so the layout does not matter, and real Return presses for line breaks.

See [0.4.1 release notes](docs/releases/v0.4.1.md).

## 0.4.0 — v0.4.0

- Dictate from anywhere: `utterform --toggle` from a compositor binding starts and finishes a recording without raising the window. `--start`, `--stop` and `--cancel` are available too, and the command starts Utterform if it is not running.
- Type the finished text into the focused window as a third output next to clipboard and file, through `wtype` on Wayland or `xdotool` on X11.
- Bind interface listeners before the first await, so a component torn down while starting up cannot leave a keyboard handler on the window.

See [0.4.0 release notes](docs/releases/v0.4.0.md).

## 0.3.4 — v0.3.4

- Fix GPT Transcribe hanging forever, a 0.3.3 regression: `ksni`'s tokio feature turned on `zbus/tokio` for the whole build, so reading the API key from the OS keyring panicked inside Tauri's async runtime and the command never answered. Build `ksni` on async-io and test the keyring from an async runtime.
- Stop cutting off long recordings after two minutes: bound inactivity rather than the whole OpenAI exchange.

See [0.3.4 release notes](docs/releases/v0.3.4.md).

## 0.3.3 — v0.3.3

- Open Utterform with a left or middle click on the Linux tray icon: Utterform serves its own StatusNotifierItem instead of using AppIndicator, which exposes no `Activate`. This corrects the claim in 0.3.2 that Linux tray clicks are undeliverable.
- Re-register the tray icon when the panel restarts, and fall back to the AppIndicator tray when no StatusNotifierItem host answers.
- Verify tray activation against a real D-Bus watcher in CI.

See [0.3.3 release notes](docs/releases/v0.3.3.md).

## 0.3.2 — v0.3.2

- Run as a single instance: launching Utterform again reveals the running window instead of starting a second app.
- Open Utterform with a left click or double click on the tray icon (Windows and macOS; Linux tray clicks are not deliverable and keep using the menu).
- Declare the window class in the Linux desktop entry so desktops match the running window to the launcher entry.

See [0.3.2 release notes](docs/releases/v0.3.2.md).

## 0.3.1 — v0.3.1

- Collapse Latest text by default while keeping Copy visible, with truthful clipboard-success feedback.
- Adapt the recorder to 360 × 400 floating windows and group Stop/Pause controls.
- Add a distinct native completion chime after successful transformation and delivery.
- Correct macOS bundle signing and verify packaged DMG/ZIP contents; support Apple Silicon only.
- Separate Linux validation from manual/platform release builds to shorten debugging cycles.
- Document the tray-popup proposal without changing existing tray behavior.

See [0.3.1 release notes](docs/releases/v0.3.1.md).

## 0.3.0 — v0.3.0

- Keep the Utterform name; unify the in-app mark and Linux/Windows/macOS icons around one refined violet/blue microphone SVG.
- Add reproducible desktop icon generation with `npm run icons`.
- Refine Settings with a clean sliders icon, subtle opening motion, themed section/model cards, and custom microphone/model selectors. Respect reduced motion and trap/restore keyboard focus.
- Show European history dates: today with 24-hour time and elapsed minutes; older entries with DD.MM.YYYY and full date/time on hover. Recover timestamps from existing history IDs without destructive migration.
- Pause/resume recording with a button or P, without transcribing, delivering, or inserting silence. Space still finishes, Escape still discards, including while paused. Paused time does not count toward the native ten-minute limit.
- Preserve microphone-responsive ambient motion, close-to-tray/background behavior, app identifiers, settings and existing history.
- Support normal versioned GitHub releases, fail closed on mismatched release metadata, and publish all platform assets after successful validation.

See [0.3.0 release notes](docs/releases/v0.3.0.md).

## 0.2.0 Beta 2 — v0.2.0-beta.2

- Fix the Windows release executable to use the GUI subsystem instead of opening an extra console window.
- Verify Windows PE architecture/subsystem in CI before publishing.
- Supersede Beta 1 without moving or replacing its published tag. App version remains 0.2.0.

See [Beta 2 release notes](docs/releases/v0.2.0-beta.2.md).

## 0.2.0 Beta 1 — v0.2.0-beta.1

- Add persistent device-local history for the latest 100 texts, short local titles, copy-again button and Ctrl/Cmd+Shift+C.
- Keep the previous text visible while recording or processing; add history privacy controls and confirmed clearing.
- Replace action/file/history native popup menus with themed, keyboard-accessible selectors.
- Add large-area, audio-responsive violet/blue recording motion with reduced-motion support.
- Add optional, synthesized native start/stop cues outside the recording interval.
- Remove native decorations in Omarchy/Hyprland only.
- Preserve native microphone capture across focus changes, minimization and close-to-tray; add a native ten-minute cutoff and native elapsed-time reporting.
- Build unsigned Linux, Windows and macOS (Apple Silicon/Intel) downloads via a shared CI/release pipeline; publish only after all targets pass.
- Add history durability/privacy tests, audio-envelope/cue/session tests, component tests and production-browser GUI tests.

See [release notes](docs/releases/v0.2.0-beta.1.md) for downloads, privacy changes and platform caveats.

## 0.1.0 Alpha 2 — v0.1.0-alpha.2

- Use Linux system libraries rather than a bundled AppImage runtime on Omarchy.
- Add a checksum-verifying per-user Linux installer and package validation.

## 0.1.0 Alpha 1 — v0.1.0-alpha.1

- Initial Tauri/Svelte desktop MVP with OpenAI and local Whisper transcription, text actions, clipboard/file delivery, model downloads, settings and tray.
