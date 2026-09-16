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

fn entry() -> Result<keyring::Entry, Failure> {
    keyring::Entry::new(SERVICE, OPENAI_ACCOUNT).map_err(|error| {
        Failure::new(
            "keyring",
            format!("Could not access the operating system keyring: {error}"),
        )
        .detail(keyring_class(&error))
    })
}

pub fn set_openai_api_key(api_key: &str) -> Result<(), Failure> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err(Failure::guidance(
            "empty_key",
            "The API key cannot be empty",
        ));
    }
    entry()?.set_secret(trimmed.as_bytes()).map_err(|error| {
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
    let secret = entry()?.get_secret().map_err(|error| match error {
        keyring::Error::NoEntry => Failure::guidance(
            "api_key_missing",
            "No OpenAI API key is stored. Add one in Settings.",
        ),
        other => Failure::new(
            "keyring",
            "The OpenAI API key could not be read from the operating system keyring.",
        )
        .detail(keyring_class(&other)),
    })?;
    String::from_utf8(secret)
        .map_err(|_| Failure::guidance("api_key_invalid", "The stored API key is invalid"))
}

pub fn has_openai_api_key() -> bool {
    match entry().map(|entry| entry.get_secret()) {
        Ok(Ok(_)) => true,
        Ok(Err(keyring::Error::NoEntry)) => false,
        Ok(Err(error)) => {
            diagnostics::warning!("secrets.read_failed", class = keyring_class(&error));
            false
        }
        Err(failure) => {
            diagnostics::warning!("secrets.read_failed", class = failure.class());
            false
        }
    }
}

pub fn delete_openai_api_key() -> Result<(), Failure> {
    match entry()?.delete_credential() {
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
}
