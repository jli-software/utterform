# macOS

What a Mac asks of Utterform, what 0.7 does about it, and what is still open.
Verified against the code, Apple's documentation and a real bundle inspected on
the Apple-Silicon build host (`macmini-build`, macOS 26.6); interactive behaviour
is confirmed only where this file says so.

## The two grants

macOS keeps a ledger per application under **System Settings → Privacy & Security**.
Two entries matter to Utterform.

**Microphone.** The system asks on its own the first time an input stream starts —
provided two things are true of the bundle. Its `Info.plist` must carry
`NSMicrophoneUsageDescription` (it did since 0.2), and, because every build is signed
with the hardened runtime, its code signature must carry the entitlement
`com.apple.security.device.audio-input`. Without the entitlement macOS denies the
microphone **without any dialog** and without listing the app; CoreAudio then delivers
silence, not an error. That is exactly what 0.6.1 did, verified on the build host:
`codesign -dvv` showed `flags=0x10002(adhoc,runtime)` and no entitlements at all.
0.7 adds `src-tauri/Entitlements.plist`, referenced from `tauri.conf.json`, and
`scripts/package-macos.sh` fails the release if either piece is missing.

Before a recording, `macos::microphone_access` reads `AVCaptureDevice`'s
authorization status: *denied* and *restricted* are reported with the path to the
switch; *not determined* proceeds, because opening the stream is what makes macOS ask.

**Accessibility.** Posting keyboard events to other applications (`CGEventPost`) is
allowed only for a process the user has turned on under Accessibility; the window
server drops everything else and reports success. `macos::accessibility_trusted`
wraps `AXIsProcessTrustedWithOptions`. Settings asks without the prompt and shows the
explanation; the first delivery asks with it, which opens the system's request dialog
and lists Utterform in the pane. A refused delivery is a warning — clipboard and file
have already run.

The reserved dictation key (Carbon `RegisterEventHotKey`, through
`tauri-plugin-global-shortcut`) needs no grant. Input Monitoring is not needed either:
Utterform posts events, it never listens to them.

## Typing at the cursor

`typing/macos.rs`. **Paste** writes the pasteboard (unless the clipboard output already
did), waits 60 ms for the write to land, and posts ⌘V. A Mac terminal takes ⌘V like
every other window — Command never reaches the shell — so the terminal-chord rule of
Linux and Windows does not apply. **Keystrokes** post one keyboard event per character
with the character as the event's Unicode string, so the active layout does not
matter; a surrogate pair travels in one event; every line ending is one Return
(key code 36). The delay setting paces them, clamped like on Linux. The frontmost
application's name and bundle identifier go to the log; nothing is decided from them.

Not covered: a window with *Secure Keyboard Entry* (a password field, a terminal with
that option) may refuse synthesized input; that surfaces as text that does not arrive,
with the text still on the clipboard.

## Signing, and why grants are lost on update

Release bundles are ad-hoc signed (`signingIdentity: "-"`) with the hardened runtime.
Gatekeeper therefore asks for *Open Anyway* once, and — more annoying in daily use —
**every grant is tied to that exact build**: an ad-hoc signature's designated
requirement is the hash of the code, so each update appears to TCC as a new
application. After an update the user removes the stale Accessibility entry and adds
the new one, and the microphone is asked for again.

A Developer ID certificate fixes both. With `APPLE_SIGNING_IDENTITY`,
`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD` and
`APPLE_TEAM_ID` as repository secrets, Tauri signs with the certificate and notarizes
the app; the designated requirement then names the team and bundle identifier, so
grants survive updates and Gatekeeper opens the app without ceremony. The workflow
change is one `env:` block on the macOS build step. This needs an Apple Developer
Program membership and is a decision for the project owner; nothing else in the
macOS work depends on it.

## Status

| | 0.6.1 | 0.7.0 beta 1 |
| --- | --- | --- |
| Microphone dialog | never appeared (missing entitlement) | entitlement in the signature, verified with `codesign -d --entitlements`; the dialog itself not yet seen by a person |
| GPT Transcribe, Local Whisper | untested on a Mac | expected to work once the microphone is granted; not yet confirmed |
| Typing at the cursor | not implemented | implemented, compiled and Clippy-clean on macOS, **never run by a person** |
| Reserved dictation key | compiled, never run | unchanged; needs no grant |
| Dock click after closing to tray | did nothing | reveals the window; not yet seen by a person |
| Live Dictation | unsupported | unchanged, deferred |
| Signing | ad-hoc, hardened runtime | unchanged; Developer ID recommended |

## Building on the Mac mini

The Linux development machine cannot compile macOS code. `macmini-build`
(`~/.ssh/config`) has Xcode, Rust and Node; CMake is not installed system-wide, so
`whisper-rs-sys` needs `~/.tools/cmake-4.4.3-macos-universal/CMake.app/Contents/bin`
on `PATH`. A checkout under `~/utterform-dev` builds with the same command as CI:

```bash
npm run tauri build -- --bundles app \
  --config '{"bundle":{"macOS":{"signingIdentity":"-","minimumSystemVersion":"11.0"}}}'
scripts/package-macos.sh
```

The SSH user has no window session, so the permission dialogs, typing and the tray
cannot be exercised there; that is done on the MacBook Air. `cargo test`, Clippy and
the bundle checks run there without one.

## Open for 0.7 final

- Confirmation on a real Mac of the five steps in the beta release notes.
- Developer ID signing and notarization, if the project takes the membership.
- Live Dictation on macOS: needs a focus observer (the frontmost application and its
  focused element through the Accessibility API) and the same fail-closed session as
  Windows and Hyprland; a separate piece of work.
