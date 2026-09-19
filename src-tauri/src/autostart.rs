//! The operating system's "start when I sign in" registration.
//!
//! Linux and macOS keep using `tauri-plugin-autostart`. Windows owns the
//! registry boundary here because `auto-launch` 0.5.0 writes an unquoted Run
//! command and then reports any existing value as enabled. A per-user install
//! below a path containing spaces therefore looks enabled but cannot launch.

use tauri::AppHandle;

use crate::diagnostics::Failure;

#[cfg(target_os = "windows")]
const APP_NAME: &str = "Utterform";

#[cfg(not(target_os = "windows"))]
use tauri_plugin_autostart::ManagerExt;

#[cfg(not(target_os = "windows"))]
pub fn enabled(app: &AppHandle) -> Result<bool, Failure> {
    app.autolaunch().is_enabled().map_err(|_| {
        Failure::new(
            "read",
            "Utterform could not read the operating system's startup entry",
        )
    })
}

#[cfg(not(target_os = "windows"))]
pub fn enable(app: &AppHandle) -> Result<(), Failure> {
    app.autolaunch().enable().map_err(|_| {
        Failure::new(
            "enable",
            "Utterform could not add itself to the operating system's startup items",
        )
    })
}

#[cfg(not(target_os = "windows"))]
pub fn disable(app: &AppHandle) -> Result<(), Failure> {
    app.autolaunch().disable().map_err(|_| {
        Failure::new(
            "disable",
            "Utterform could not remove itself from the operating system's startup items",
        )
    })
}

#[cfg(target_os = "windows")]
pub fn enabled(_app: &AppHandle) -> Result<bool, Failure> {
    windows::enabled()
}

#[cfg(target_os = "windows")]
pub fn enable(_app: &AppHandle) -> Result<(), Failure> {
    windows::enable()
}

#[cfg(target_os = "windows")]
pub fn disable(_app: &AppHandle) -> Result<(), Failure> {
    windows::disable()
}

/// Windows receives one command-line string from the Run registry value. The
/// executable is always quoted, even when today's path has no spaces, so an
/// account or install-directory rename cannot change how it is parsed.
#[cfg(any(target_os = "windows", test))]
fn windows_command_line(executable: &str) -> Result<String, Failure> {
    if executable.is_empty() || executable.contains(['\0', '"']) {
        return Err(Failure::new(
            "command",
            "Utterform could not build a safe Windows startup command",
        ));
    }
    Ok(format!("\"{executable}\" {}", crate::cli::AUTOSTART_FLAG))
}

/// A missing StartupApproved value means Windows has not disabled the Run
/// entry. A present value is enabled only when its timestamp bytes are zero;
/// malformed data is not treated as permission to claim that startup works.
#[cfg(any(target_os = "windows", test))]
fn startup_approved(value: Option<&[u8]>) -> bool {
    value.is_none_or(|bytes| bytes.len() >= 8 && bytes.iter().rev().take(8).all(|byte| *byte == 0))
}

#[cfg(any(target_os = "windows", test))]
fn registration_enabled(registered: Option<&str>, expected: &str, approved: Option<&[u8]>) -> bool {
    registered.is_some_and(|value| value == expected) && startup_approved(approved)
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{env, io};

    use winreg::{
        RegKey, RegValue,
        enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, RegType::REG_BINARY},
    };

    use super::{APP_NAME, registration_enabled, windows_command_line};
    use crate::diagnostics::Failure;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const STARTUP_APPROVED_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const ENABLED: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    fn failure(class: &'static str, error: &io::Error, message: &'static str) -> Failure {
        Failure::io(class, error, message)
    }

    fn expected_command_line() -> Result<String, Failure> {
        let executable = env::current_exe().map_err(|error| {
            failure(
                "executable",
                &error,
                "Utterform could not resolve its Windows executable",
            )
        })?;
        let executable = executable.to_str().ok_or_else(|| {
            Failure::new(
                "executable",
                "Utterform's Windows executable path could not be represented safely",
            )
        })?;
        windows_command_line(executable)
    }

    fn read_string(key: &str, name: &str) -> Result<Option<String>, Failure> {
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let key = match root.open_subkey_with_flags(key, KEY_READ) {
            Ok(key) => key,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(failure(
                    "read",
                    &error,
                    "Utterform could not read the Windows startup entry",
                ));
            }
        };
        match key.get_value(name) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(failure(
                "read",
                &error,
                "Utterform could not read the Windows startup entry",
            )),
        }
    }

    fn read_approved(name: &str) -> Result<Option<Vec<u8>>, Failure> {
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let key = match root.open_subkey_with_flags(STARTUP_APPROVED_KEY, KEY_READ) {
            Ok(key) => key,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(failure(
                    "read",
                    &error,
                    "Utterform could not read the Windows startup approval",
                ));
            }
        };
        match key.get_raw_value(name) {
            Ok(value) if value.vtype == REG_BINARY => Ok(Some(value.bytes)),
            Ok(_) => Ok(Some(Vec::new())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(failure(
                "read",
                &error,
                "Utterform could not read the Windows startup approval",
            )),
        }
    }

    fn enabled_for(name: &str, expected: &str) -> Result<bool, Failure> {
        let Some(registered) = read_string(RUN_KEY, name)? else {
            return Ok(false);
        };
        if registered != expected {
            return Ok(false);
        }
        let approved = read_approved(name)?;
        Ok(registration_enabled(
            Some(&registered),
            expected,
            approved.as_deref(),
        ))
    }

    fn write_for(name: &str, command: &str) -> Result<(), Failure> {
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let (run, _) = root.create_subkey(RUN_KEY).map_err(|error| {
            failure(
                "enable",
                &error,
                "Utterform could not create the Windows startup entry",
            )
        })?;
        run.set_value(name, &command).map_err(|error| {
            failure(
                "enable",
                &error,
                "Utterform could not write the Windows startup entry",
            )
        })?;

        // Explorer creates this key when it needs to track an override. Do not
        // create a private Explorer key, but clear a pre-existing disabled
        // state when the user explicitly enables startup again.
        if let Ok(approved) = root.open_subkey_with_flags(STARTUP_APPROVED_KEY, KEY_SET_VALUE) {
            approved
                .set_raw_value(
                    name,
                    &RegValue {
                        vtype: REG_BINARY,
                        bytes: ENABLED.to_vec(),
                    },
                )
                .map_err(|error| {
                    failure(
                        "enable",
                        &error,
                        "Utterform could not enable the Windows startup entry",
                    )
                })?;
        }
        Ok(())
    }

    fn delete_value(key: &str, name: &str) -> Result<(), Failure> {
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let key = match root.open_subkey_with_flags(key, KEY_SET_VALUE) {
            Ok(key) => key,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(failure(
                    "disable",
                    &error,
                    "Utterform could not open the Windows startup entry",
                ));
            }
        };
        match key.delete_value(name) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(failure(
                "disable",
                &error,
                "Utterform could not remove the Windows startup entry",
            )),
        }
    }

    fn disable_for(name: &str) -> Result<(), Failure> {
        delete_value(RUN_KEY, name)?;
        delete_value(STARTUP_APPROVED_KEY, name)
    }

    pub(super) fn enabled() -> Result<bool, Failure> {
        enabled_for(APP_NAME, &expected_command_line()?)
    }

    pub(super) fn enable() -> Result<(), Failure> {
        let command = expected_command_line()?;
        write_for(APP_NAME, &command)?;
        if enabled_for(APP_NAME, &command)? {
            Ok(())
        } else {
            Err(Failure::new(
                "verify",
                "Windows did not retain a working Utterform startup entry",
            ))
        }
    }

    pub(super) fn disable() -> Result<(), Failure> {
        disable_for(APP_NAME)?;
        if read_string(RUN_KEY, APP_NAME)?.is_none() && read_approved(APP_NAME)?.is_none() {
            Ok(())
        } else {
            Err(Failure::new(
                "verify",
                "Windows did not remove the Utterform startup entry",
            ))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct Registration(String);

        impl Registration {
            fn new() -> Self {
                Self(format!("UtterformTest-{}", std::process::id()))
            }
        }

        impl Drop for Registration {
            fn drop(&mut self) {
                let _ = disable_for(&self.0);
            }
        }

        #[test]
        fn registry_registration_is_verified_and_removed() {
            let registration = Registration::new();
            let command =
                windows_command_line(r"C:\Users\Jonas Example\Utterform\utterform.exe").unwrap();

            write_for(&registration.0, &command).unwrap();
            assert!(enabled_for(&registration.0, &command).unwrap());
            assert!(!enabled_for(&registration.0, "wrong command").unwrap());

            disable_for(&registration.0).unwrap();
            assert!(read_string(RUN_KEY, &registration.0).unwrap().is_none());
            assert!(read_approved(&registration.0).unwrap().is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_executable_is_always_quoted() {
        assert_eq!(
            windows_command_line(r"C:\Users\Jonas Example\Utterform\utterform.exe").unwrap(),
            r#""C:\Users\Jonas Example\Utterform\utterform.exe" --autostart"#
        );
        assert_eq!(
            windows_command_line(r"C:\Utterform\utterform.exe").unwrap(),
            r#""C:\Utterform\utterform.exe" --autostart"#
        );
    }

    #[test]
    fn unsafe_windows_executable_is_rejected() {
        assert!(windows_command_line("").is_err());
        assert!(windows_command_line("bad\0path.exe").is_err());
        assert!(windows_command_line("bad\"path.exe").is_err());
    }

    #[test]
    fn read_back_requires_the_exact_command_and_an_enabled_override() {
        let expected = r#""C:\Program Files\Utterform\utterform.exe" --autostart"#;
        let enabled = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let disabled = [3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];

        assert!(registration_enabled(Some(expected), expected, None));
        assert!(registration_enabled(
            Some(expected),
            expected,
            Some(&enabled)
        ));
        assert!(!registration_enabled(None, expected, Some(&enabled)));
        assert!(!registration_enabled(
            Some("unquoted.exe --autostart"),
            expected,
            None
        ));
        assert!(!registration_enabled(
            Some(expected),
            expected,
            Some(&disabled)
        ));
        assert!(!registration_enabled(
            Some(expected),
            expected,
            Some(&[2, 0, 0])
        ));
    }
}
