# Changelog

## Unreleased

- Keep the Utterform name; unify the in-app mark and Linux/Windows/macOS icons around one refined violet/blue microphone SVG.
- Add reproducible desktop icon generation with `npm run icons`.
- Refine Settings with a clean sliders icon, subtle opening motion, themed section/model cards, and custom microphone/model selectors. Respect reduced motion and trap/restore keyboard focus.
- Show European history dates: today with 24-hour time and elapsed minutes; older entries with DD.MM.YYYY and full date/time on hover. Recover timestamps from existing history IDs without destructive migration.
- Pause/resume recording with a button or P, without transcribing, delivering, or inserting silence. Space still finishes, Escape still discards, including while paused. Paused time does not count toward the native ten-minute limit.
- Preserve microphone-responsive ambient motion, close-to-tray/background behavior, app identifiers, settings and existing history.

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
