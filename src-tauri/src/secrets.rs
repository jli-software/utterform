const SERVICE: &str = "software.jli.utterform";
const OPENAI_ACCOUNT: &str = "openai-api-key";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, OPENAI_ACCOUNT)
        .map_err(|error| format!("Could not access the operating system keyring: {error}"))
}

pub fn set_openai_api_key(api_key: &str) -> Result<(), String> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err("The API key cannot be empty".into());
    }
    entry()?.set_secret(trimmed.as_bytes()).map_err(|error| {
        format!("Could not save the API key in the operating system keyring: {error}")
    })
}

pub fn openai_api_key() -> Result<String, String> {
    let secret = entry()?
        .get_secret()
        .map_err(|_| "No OpenAI API key is stored. Add one in Settings.".to_string())?;
    String::from_utf8(secret).map_err(|_| "The stored API key is invalid".to_string())
}

pub fn has_openai_api_key() -> bool {
    entry()
        .and_then(|value| value.get_secret().map_err(|error| error.to_string()))
        .is_ok()
}

pub fn delete_openai_api_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("Could not remove the API key: {error}")),
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
