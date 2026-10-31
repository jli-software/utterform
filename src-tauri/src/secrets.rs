use crate::diagnostics::{self, Failure};

const SERVICE: &str = "software.jli.utterform";
const OPENAI_ACCOUNT: &str = "openai-api-key";

/// The kind of keyring error, never its contents.
fn keyring_class(error: &keyring::Error) -> &'static str {
    match error {
        keyring::Error::NoEntry => "no_entry",
        keyring::Error::NoStorageAccess(_) => "no_storage_access",
        keyring::Error::PlatformFailure(_) => "platform_failure",
        keyring::Error::BadEncoding(_) => "bad_encoding",
        _ => "other",
    }
}

/// Runs `operation` on the entry, and once more on a new session if the store
/// failed. A store that cannot answer is not always a store that is gone.
fn with_entry<T>(
    operation: impl Fn(&keyring::Entry) -> Result<T, keyring::Error>,
) -> Result<T, keyring::Error> {
    let attempt =
        || keyring::Entry::new(SERVICE, OPENAI_ACCOUNT).and_then(|entry| operation(&entry));
    match attempt() {
        Err(keyring::Error::PlatformFailure(_) | keyring::Error::NoStorageAccess(_))
            if reconnect() =>
        {
            attempt()
        }
        outcome => outcome,
    }
}

/// The store holds one Secret Service session for as long as the process
/// lives, and that session ends with the daemon. Without this, one restart of
/// the keyring daemon failed every later read until Utterform itself was
/// restarted, however healthy the new daemon was.
#[cfg(target_os = "linux")]
fn reconnect() -> bool {
    match zbus_secret_service_keyring_store::Store::new() {
        Ok(store) => {
            keyring_core::set_default_store(store);
            diagnostics::info!("secrets.reconnected");
            true
        }
        Err(error) => {
            diagnostics::warning!("secrets.reconnect_failed", class = keyring_class(&error));
            false
        }
    }
}

/// macOS and Windows open their store per call; there is no session to renew.
#[cfg(not(target_os = "linux"))]
fn reconnect() -> bool {
    false
}

pub fn set_openai_api_key(api_key: &str) -> Result<(), Failure> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err(Failure::guidance(
            "empty_key",
            "The API key cannot be empty",
        ));
    }
    with_entry(|entry| entry.set_secret(trimmed.as_bytes())).map_err(|error| {
        Failure::new(
            "keyring",
            format!("Could not save the API key in the operating system keyring: {error}"),
        )
        .detail(keyring_class(&error))
    })
}

/// The stored key. A missing key is guidance; a keyring that cannot be read
/// is a failure, and says so rather than claiming no key was stored.
pub fn openai_api_key() -> Result<String, Failure> {
    let secret = with_entry(keyring::Entry::get_secret).map_err(|error| match error {
        keyring::Error::NoEntry => Failure::guidance(
            "api_key_missing",
            "Add an OpenAI API key in Settings → AI & Models before recording.",
        ),
        other => Failure::new(
            "keyring",
            "The OpenAI API key could not be read from the operating system keyring. \
             Check that the keyring is unlocked, then try again.",
        )
        .detail(keyring_class(&other)),
    })?;
    String::from_utf8(secret)
        .map_err(|_| Failure::guidance("api_key_invalid", "The stored API key is invalid"))
}

pub fn has_openai_api_key() -> bool {
    match with_entry(keyring::Entry::get_secret) {
        Ok(_) => true,
        Err(keyring::Error::NoEntry) => false,
        Err(error) => {
            diagnostics::warning!("secrets.read_failed", class = keyring_class(&error));
            false
        }
    }
}

pub fn delete_openai_api_key() -> Result<(), Failure> {
    match with_entry(keyring::Entry::delete_credential) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(Failure::new(
            "keyring",
            format!("Could not remove the API key: {error}"),
        )
        .detail(keyring_class(&error))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading the key happens inside `finish_recording`, which Tauri runs on
    /// its async runtime. The OS keyring goes through zbus' blocking API on
    /// Linux, and that must not try to start a runtime inside a runtime.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_keyring_can_be_reached_from_the_async_runtime() {
        // Absence of a stored key is fine here; a panic or a hang is not.
        let _ = has_openai_api_key();
    }

    /// Against a real daemon, which only `scripts/test-keyring-restart.sh` may
    /// provide: it runs this on a private bus with a throwaway keyring. The
    /// variable keeps a stray `--ignored` run away from the user's own keyring.
    #[test]
    #[ignore = "needs scripts/test-keyring-restart.sh"]
    fn a_restarted_keyring_daemon_is_found_again() {
        let Ok(restart) = std::env::var("UTTERFORM_KEYRING_RESTART") else {
            panic!("run through scripts/test-keyring-restart.sh");
        };
        set_openai_api_key("sk-throwaway").expect("the fresh daemon stores a key");
        assert_eq!(openai_api_key().unwrap(), "sk-throwaway");

        let restarted = std::process::Command::new(restart).status().unwrap();
        assert!(restarted.success());

        assert_eq!(
            openai_api_key().expect("the key is readable after the daemon restarted"),
            "sk-throwaway"
        );
    }
}
