//! What macOS asks before Utterform may listen or type.
//!
//! macOS keeps two separate ledgers for this application, both under System
//! Settings → Privacy & Security. **Microphone** is asked for by the system
//! itself the first time an input stream starts, provided the bundle's code
//! signature carries the audio-input entitlement (`Entitlements.plist`) and
//! its Info.plist explains why; a bundle without the entitlement is denied
//! without a dialog, which is what 0.6.1 did. **Accessibility** is what lets
//! a process post keyboard events to other applications; without it
//! `CGEventPost` drops every event and says nothing. Asking for it opens the
//! system's own request dialog, which also lists Utterform in the settings
//! pane so the user can turn it on.
//!
//! Both grants are tied to the code signature. An ad-hoc signed bundle is
//! identified by the hash of that exact build, so every update starts over —
//! a Developer ID signature keeps the grants across versions.

use std::ffi::c_void;

use core_foundation::{
    base::TCFType,
    boolean::CFBoolean,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
use objc2::{class, msg_send};
use objc2_foundation::NSString;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: &'static NSString;
}

/// `AVAuthorizationStatus`, as `AVCaptureDevice` reports it for the microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MicrophoneStatus {
    /// Never asked. The first input stream makes the system ask.
    NotDetermined,
    /// A device profile or parental control forbids it; no dialog can help.
    Restricted,
    /// The user said no, or the switch was turned off later.
    Denied,
    Authorized,
}

fn microphone_status() -> MicrophoneStatus {
    let status: isize = unsafe {
        msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: AVMediaTypeAudio]
    };
    match status {
        1 => MicrophoneStatus::Restricted,
        2 => MicrophoneStatus::Denied,
        3 => MicrophoneStatus::Authorized,
        _ => MicrophoneStatus::NotDetermined,
    }
}

/// Refuse to start a recording that would only capture silence. A microphone
/// the system has not asked about yet is fine: opening the stream is what
/// makes it ask, and the recording carries on once the user has answered.
pub fn microphone_access() -> Result<(), String> {
    match microphone_status() {
        MicrophoneStatus::Authorized | MicrophoneStatus::NotDetermined => Ok(()),
        MicrophoneStatus::Denied => Err(
            "macOS has not given Utterform the microphone. Turn it on under System Settings → Privacy & Security → Microphone, then record again."
                .into(),
        ),
        MicrophoneStatus::Restricted => {
            Err("Microphone access is restricted on this Mac by a profile or parental control".into())
        }
    }
}

/// Whether Utterform may post keyboard events to other applications. With
/// `prompt`, a refusal also opens the system's request dialog — once per
/// process, macOS does not repeat it.
pub fn accessibility_trusted(prompt: bool) -> bool {
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options =
        CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::from(prompt).as_CFType())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef().cast()) }
}

/// What to tell the user when typing is not possible for want of the grant.
pub const ACCESSIBILITY_HELP: &str = "macOS lets Utterform type into other windows only with Accessibility. Turn Utterform on under System Settings → Privacy & Security → Accessibility, then try again. The text is on the clipboard meanwhile.";
