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

## Signing and stable grants across updates

Since 0.7.4 release bundles are signed with a Developer ID Application certificate and
the hardened runtime, submitted to Apple's notary service and shipped with the ticket
stapled to the app. The designated requirement names the team and bundle identifier,
so macOS can recognise a later version as the same application instead of tying its
privacy grants to an ad-hoc code hash. Gatekeeper accepts the bundle as
`Notarized Developer ID`.

The macOS build reads six repository secrets as one atomic set. Tauri imports the
certificate into a temporary keychain, signs the app, submits it and staples the
ticket. A partial configuration fails the job; it cannot silently publish an ad-hoc
release. `scripts/package-macos.sh` verifies the Developer ID authority, Team ID,
ticket and Gatekeeper result before packaging succeeds.

### Credential setup and renewal

The initial setup below was completed for 0.7.4. Keep the procedure for certificate
renewal or credential rotation: the certificate can only be created by the account
holder; everything else can be done from the Linux machine and the Mac mini.

1. **Certificate signing request** (done by the agent on the Mac mini): a private key
   and CSR, `openssl req -new -newkey rsa:2048 -nodes -keyout developerid.key -out developerid.csr`,
   kept under `~/utterform-signing` on `macmini-build`, never committed.
2. **Certificate** (Jonas, about five minutes): [developer.apple.com/account/resources/certificates](https://developer.apple.com/account/resources/certificates/add)
   → *Developer ID Application* → upload the CSR → download `developerID_application.cer`
   and hand it back. Developer ID certificates are valid for five years.
3. **Notarization credentials** (Jonas): an app-specific password at
   [account.apple.com](https://account.apple.com) → *Sign-In and Security* →
   *App-Specific Passwords*, and the Team ID from the membership page. The Apple ID
   is the account's e-mail address.
4. **Secrets** (agent): the `.cer` and the private key become a `.p12`
   (`openssl pkcs12 -export`), base64-encoded into `APPLE_CERTIFICATE`, its password
   into `APPLE_CERTIFICATE_PASSWORD`; `APPLE_SIGNING_IDENTITY` is the certificate's
   common name, `Developer ID Application: <name> (<TEAMID>)`; `APPLE_ID`,
   `APPLE_PASSWORD` and `APPLE_TEAM_ID` are the credentials from step 3. Set with
   `gh secret set`, then a Desktop-builds run on a branch shows the stapled ticket
   before the next tag.

TestFlight and the App Store are a different path (App Store distribution
certificate, sandbox, review) and are not needed for a downloaded desktop app.

## Icon cache

macOS renders an application's icon once and caches it — the Dock and the ⌘-Tab
switcher read the cache, the Finder does not always. A bundle installed over an
earlier version at the same path, with the icon under the same file name, keeps the
earlier rendering; Jonas saw the pre-0.5 logo on a 0.7.0 bundle whose `icon.icns`
was verified to hold the current artwork. Since 0.7.1 the file is `Utterform.icns`
(`generate-icons.mjs` writes it, `tauri.conf.json` lists it, the bundler names it in
`CFBundleIconFile`), which gives it a cache entry of its own. The commands that clear
the cache by hand are in the 0.7.1 release notes.

## Status

Confirmed by Jonas on a MacBook Air (M2, macOS 26) on 2026-09-10, on 0.7.0 beta 1.

| | 0.6.1 | Current (0.7.4) |
| --- | --- | --- |
| Microphone dialog | never appeared (missing entitlement) | **confirmed since 0.7.1** |
| GPT Transcribe, Local Whisper | untested on a Mac | **confirmed since 0.7.1** |
| Typing at the cursor | not implemented | **confirmed since 0.7.1** (method not stated) |
| Reserved dictation key | compiled, never run | **confirmed since 0.7.1**, including changing it |
| Dock click after closing to tray | did nothing | implemented; not mentioned either way |
| Dock and ⌘-Tab icon | stale rendering of an earlier version | icon file renamed in 0.7.1; to be seen |
| Live Dictation | unsupported | unchanged, planned for a later release |
| Signing | ad-hoc, hardened runtime | **Developer ID signed and Apple notarized since 0.7.4** |

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

## Open

- Whether the icon rename alone refreshes the Dock on Jonas's machine.
- Install-over-update test for Microphone and Accessibility grant persistence.
- Live Dictation on macOS: needs a focus observer (the frontmost application and its
  focused element through the Accessibility API) and the same fail-closed session as
  Windows and Hyprland; planned for a later release.
