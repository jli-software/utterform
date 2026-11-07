//! Text actions through the user's ChatGPT subscription (experimental).
//!
//! Utterform has no login of its own: the Codex CLI's `codex login` leaves its
//! tokens in `~/.codex/auth.json` (or `$CODEX_HOME`), and the subscription
//! answers on the Responses API at `chatgpt.com/backend-api/codex`. When the
//! access token is about to expire, Utterform renews it and writes the result
//! back in the same format, because OpenAI's refresh tokens are single-use: a
//! renewal the CLI never sees would sign the CLI out.
//!
//! OpenAI does not support third-party apps on this path and may close it.
//! There is deliberately no fallback to the API key (Jonas, 2026-11-07): a
//! failure here is reported as such, and the transcript is delivered plain.
//!
//! Nothing from `auth.json` — token, account, e-mail — ever reaches the log.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    diagnostics::Failure,
    domain::{AppSettings, TextEffort},
};

const RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// The Codex CLI's public OAuth client. Refreshing under it keeps the tokens
/// usable by the CLI that issued them.
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
/// Renew a token that has less than this left, so it cannot expire mid-request.
const EXPIRY_MARGIN_SECONDS: i64 = 300;
pub const SETUP_URL: &str = "https://developers.openai.com/codex/cli";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

const NOT_SIGNED_IN: &str =
    "ChatGPT is not signed in. Install the Codex CLI and run `codex login` once.";
const SIGN_IN_AGAIN: &str = "The ChatGPT sign-in has expired. Run `codex login` again.";

/// What Settings may know about the Codex login: whether there is one, and
/// which models it offers. Never a token or an account.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexStatus {
    pub signed_in: bool,
    pub models: Vec<CodexModel>,
    /// The model used while the user has not chosen one.
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexModel {
    pub id: String,
    pub name: String,
}

pub fn status() -> CodexStatus {
    let home = codex_home();
    let models = home
        .as_deref()
        .and_then(|home| fs::read_to_string(home.join("models_cache.json")).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .map(|cache| listed_models(&cache))
        .unwrap_or_default();
    CodexStatus {
        signed_in: home
            .and_then(|home| Auth::load(&home.join("auth.json")).ok())
            .is_some(),
        default_model: default_model(&models),
        models,
    }
}

/// The model a text action runs on: the user's choice, else the default.
pub fn effective_model(settings: &AppSettings) -> String {
    settings
        .codex_model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .or_else(|| status().default_model)
        .unwrap_or_else(|| FALLBACK_MODEL.to_string())
}

/// Used only when the CLI has not cached its model list yet.
const FALLBACK_MODEL: &str = "gpt-6-luna";

/// Checked before a recording starts, like a missing API key.
pub fn ensure_signed_in() -> Result<(), Failure> {
    let path = auth_path()?;
    Auth::load(&path).map(|_| ())
}

pub async fn transform(
    instructions: &str,
    transcript: &str,
    settings: &AppSettings,
) -> Result<String, Failure> {
    let path = auth_path()?;
    let mut auth = Auth::load(&path)?;
    let body = request_body(
        &effective_model(settings),
        instructions,
        transcript,
        settings.text_effort,
    );
    let client = client()?;
    if auth.needs_refresh(now()) {
        auth.refresh(&client, &path).await?;
    }
    let mut renewed = false;
    loop {
        let mut request = client
            .post(RESPONSES_URL)
            .bearer_auth(auth.access_token())
            .header("originator", "utterform")
            .header("accept", "text/event-stream")
            .json(&body);
        if let Some(account) = auth.account_id() {
            request = request.header("chatgpt-account-id", account);
        }
        let response = request.send().await.map_err(|error| {
            Failure::new(
                super::openai::transport_class(&error),
                format!("ChatGPT could not be reached: {error}"),
            )
        })?;
        let status = response.status();
        // A token the server no longer accepts gets one renewal, then counts
        // as a lost sign-in.
        if status == StatusCode::UNAUTHORIZED && !renewed {
            renewed = true;
            auth.refresh(&client, &path).await?;
            continue;
        }
        let text = response.text().await.map_err(|error| {
            Failure::new(
                super::openai::transport_class(&error),
                format!("ChatGPT's answer was interrupted: {error}"),
            )
        })?;
        if !status.is_success() {
            return Err(status_failure(status, &text));
        }
        return answer_text(&text);
    }
}

fn client() -> Result<Client, Failure> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| {
            Failure::new(
                "client_init",
                format!("Could not initialize the ChatGPT client: {error}"),
            )
        })
}

fn request_body(
    model: &str,
    instructions: &str,
    transcript: &str,
    effort: Option<TextEffort>,
) -> Value {
    let mut body = json!({
        "model": model,
        "instructions": instructions,
        // The subscription endpoint takes a list of messages only.
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": transcript }],
        }],
        "store": false,
        // …and answers only as a stream.
        "stream": true,
    });
    if let Some(effort) = effort {
        // The Codex models start at "low"; "minimal" is the API's level.
        let level = match effort {
            TextEffort::Minimal | TextEffort::Low => "low",
            TextEffort::Medium => "medium",
            TextEffort::High => "high",
        };
        body["reasoning"] = json!({ "effort": level });
    }
    body
}

/// The text of a finished server-sent-event stream.
fn answer_text(stream: &str) -> Result<String, Failure> {
    let mut deltas = String::new();
    let mut done = None;
    for line in stream.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(event) = serde_json::from_str::<Value>(data.trim()) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "response.output_text.delta" => {
                deltas.push_str(event.get("delta").and_then(Value::as_str).unwrap_or(""));
            }
            "response.output_text.done" => {
                done = event
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            "response.failed" | "error" => {
                let message = event
                    .get("response")
                    .and_then(error_message)
                    .or_else(|| error_message(&event))
                    .unwrap_or_else(|| "ChatGPT could not finish the answer".into());
                return Err(Failure::new("codex_failed", format!("ChatGPT: {message}")));
            }
            _ => {}
        }
    }
    let text = done
        .filter(|text| !text.trim().is_empty())
        .unwrap_or(deltas);
    let text = text.trim();
    if text.is_empty() {
        return Err(Failure::new(
            "empty_result",
            "ChatGPT returned an empty transformation",
        ));
    }
    Ok(text.to_string())
}

fn error_message(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .or_else(|| value.get("detail"))
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_string)
}

/// The user reads what ChatGPT said; the log only ever gets the status.
fn status_failure(status: StatusCode, body: &str) -> Failure {
    let parsed = serde_json::from_str::<Value>(body).unwrap_or(Value::Null);
    let failure = match status {
        StatusCode::UNAUTHORIZED => Failure::guidance("codex_signed_out", SIGN_IN_AGAIN),
        StatusCode::TOO_MANY_REQUESTS => {
            let minutes = parsed
                .get("error")
                .and_then(|error| error.get("resets_in_seconds"))
                .and_then(Value::as_i64)
                .map(|seconds| (seconds + 59) / 60);
            Failure::guidance(
                "codex_limit",
                match minutes {
                    Some(minutes) => format!(
                        "The ChatGPT usage limit is reached. It resets in about {minutes} min."
                    ),
                    None => "The ChatGPT usage limit is reached.".to_string(),
                },
            )
        }
        _ => Failure::new(
            "http",
            error_message(&parsed)
                .map(|message| format!("ChatGPT: {message}"))
                .unwrap_or_else(|| format!("ChatGPT request failed with HTTP {status}")),
        ),
    };
    failure.status(status.as_u16())
}

fn codex_home() -> Option<PathBuf> {
    codex_home_from(
        std::env::var_os("CODEX_HOME"),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
    )
}

fn codex_home_from(
    codex_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    match codex_home {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => home
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".codex")),
    }
}

fn auth_path() -> Result<PathBuf, Failure> {
    codex_home()
        .map(|home| home.join("auth.json"))
        .ok_or_else(|| Failure::guidance("codex_signed_out", NOT_SIGNED_IN))
}

/// The models the Codex CLI shows in its own picker, in its order.
fn listed_models(cache: &Value) -> Vec<CodexModel> {
    let mut models: Vec<(i64, CodexModel)> = cache
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|model| model.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|model| {
            let id = model.get("slug")?.as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            let name = model
                .get("display_name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(id);
            let priority = model
                .get("priority")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            Some((
                priority,
                CodexModel {
                    id: id.to_string(),
                    name: name.to_string(),
                },
            ))
        })
        .collect();
    models.sort_by_key(|(priority, _)| *priority);
    models.into_iter().map(|(_, model)| model).collect()
}

/// Dictation has to stay quick: the newest of the small, fast models
/// ("…-luna", "…-mini"), else whatever the CLI lists first.
fn default_model(models: &[CodexModel]) -> Option<String> {
    models
        .iter()
        .find(|model| {
            let id = model.id.to_ascii_lowercase();
            id.ends_with("-luna") || id.ends_with("-mini")
        })
        .or_else(|| models.first())
        .map(|model| model.id.clone())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// The expiry of a JWT, read without verifying it: the server does that.
fn jwt_expiry(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .get("exp")?
        .as_i64()
}

/// `auth.json` as the Codex CLI wrote it, kept whole so a write-back changes
/// only the tokens.
struct Auth {
    data: Value,
}

impl Auth {
    fn load(path: &Path) -> Result<Self, Failure> {
        let text = fs::read_to_string(path)
            .map_err(|_| Failure::guidance("codex_signed_out", NOT_SIGNED_IN))?;
        let data = serde_json::from_str::<Value>(&text)
            .map_err(|_| Failure::guidance("codex_signed_out", NOT_SIGNED_IN))?;
        let auth = Self { data };
        if auth.token("access_token").is_none() || auth.token("refresh_token").is_none() {
            return Err(Failure::guidance("codex_signed_out", NOT_SIGNED_IN));
        }
        Ok(auth)
    }

    fn token(&self, key: &str) -> Option<&str> {
        self.data
            .get("tokens")?
            .get(key)?
            .as_str()
            .filter(|value| !value.is_empty())
    }

    fn access_token(&self) -> &str {
        self.token("access_token").unwrap_or_default()
    }

    fn account_id(&self) -> Option<&str> {
        self.token("account_id")
    }

    fn needs_refresh(&self, now: i64) -> bool {
        jwt_expiry(self.access_token()).is_some_and(|exp| exp - now < EXPIRY_MARGIN_SECONDS)
    }

    async fn refresh(&mut self, client: &Client, path: &Path) -> Result<(), Failure> {
        // The CLI may have renewed in the meantime, and its refresh token is
        // then the only one still valid: take its token rather than renew again.
        let stale = self.access_token().to_string();
        *self = Self::load(path)?;
        if self.access_token() != stale && !self.needs_refresh(now()) {
            return Ok(());
        }
        let response = client
            .post(TOKEN_URL)
            .json(&json!({
                "client_id": CLIENT_ID,
                "grant_type": "refresh_token",
                "refresh_token": self.token("refresh_token").unwrap_or_default(),
                "scope": "openid profile email",
            }))
            .send()
            .await
            .map_err(|error| {
                Failure::new(
                    super::openai::transport_class(&error),
                    format!("The ChatGPT sign-in could not be renewed: {error}"),
                )
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(
                Failure::guidance("codex_signed_out", SIGN_IN_AGAIN).status(status.as_u16())
            );
        }
        let renewed = response.json::<Value>().await.map_err(|_| {
            Failure::new(
                "invalid_response",
                "The ChatGPT sign-in renewal returned an unreadable answer",
            )
        })?;
        self.merge(&renewed, &chrono::Utc::now().to_rfc3339());
        write_back(path, &self.data)
    }

    /// Takes the tokens a renewal returned; everything else stays as written.
    fn merge(&mut self, renewed: &Value, stamp: &str) {
        if let Some(tokens) = self.data.get_mut("tokens").and_then(Value::as_object_mut) {
            for key in ["access_token", "refresh_token", "id_token"] {
                if let Some(value) = renewed
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    tokens.insert(key.into(), Value::String(value.into()));
                }
            }
        }
        self.data["last_refresh"] = Value::String(stamp.into());
    }
}

/// Replace `auth.json` in one step, readable by the user alone, so neither a
/// crash nor the CLI reading at the same moment ever sees half a file.
fn write_back(path: &Path, data: &Value) -> Result<(), Failure> {
    let failed = |error: std::io::Error| {
        Failure::io(
            "codex_auth_write",
            &error,
            "The renewed ChatGPT sign-in could not be saved",
        )
    };
    let text = serde_json::to_string_pretty(data).map_err(|_| {
        Failure::new(
            "codex_auth_write",
            "The renewed ChatGPT sign-in could not be saved",
        )
    })?;
    let temporary = path.with_extension("json.utterform-tmp");
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let mut file = options.open(&temporary).map_err(failed)?;
        file.write_all(text.as_bytes()).map_err(failed)?;
        file.sync_all().map_err(failed)?;
    }
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        failed(error)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(exp: i64) -> String {
        format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#))
        )
    }

    fn auth(access: &str) -> Auth {
        Auth {
            data: json!({
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": null,
                "tokens": { "access_token": access, "refresh_token": "r1", "id_token": "i1", "account_id": "acc" },
                "last_refresh": "2026-10-29T16:33:30Z",
            }),
        }
    }

    #[test]
    fn a_token_is_renewed_only_shortly_before_it_expires() {
        let now = 1_000_000;
        assert!(!auth(&jwt(now + 3600)).needs_refresh(now));
        assert!(auth(&jwt(now + 60)).needs_refresh(now));
        assert!(auth(&jwt(now - 1)).needs_refresh(now));
        // Not a JWT: let the server decide, then renew on its 401.
        assert!(!auth("opaque").needs_refresh(now));
    }

    #[test]
    fn a_renewal_replaces_the_tokens_and_keeps_everything_else() {
        let mut auth = auth("old");
        auth.merge(
            &json!({ "access_token": "new", "refresh_token": "r2", "id_token": "" }),
            "2026-11-07T10:00:00Z",
        );
        assert_eq!(auth.access_token(), "new");
        assert_eq!(auth.token("refresh_token"), Some("r2"));
        // An empty value in the answer never erases a token.
        assert_eq!(auth.token("id_token"), Some("i1"));
        assert_eq!(auth.account_id(), Some("acc"));
        assert_eq!(auth.data["auth_mode"], "chatgpt");
        assert_eq!(auth.data["last_refresh"], "2026-11-07T10:00:00Z");
    }

    #[test]
    fn the_write_back_leaves_a_whole_file_only_the_user_can_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        fs::write(&path, "{}").unwrap();
        write_back(&path, &auth("written").data).unwrap();
        assert_eq!(Auth::load(&path).unwrap().access_token(), "written");
        assert!(!dir.path().join("auth.json.utterform-tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn without_a_chatgpt_login_the_user_is_told_how_to_sign_in() {
        let dir = tempfile::tempdir().unwrap();
        let missing = Auth::load(&dir.path().join("auth.json")).err().unwrap();
        assert_eq!(missing.class(), "codex_signed_out");
        assert!(missing.message().contains("codex login"));

        // Signed in with an API key only: there are no ChatGPT tokens.
        let path = dir.path().join("auth.json");
        fs::write(&path, r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-x"}"#).unwrap();
        assert!(Auth::load(&path).is_err());
    }

    #[test]
    fn the_codex_folder_follows_codex_home_then_the_home_folder() {
        assert_eq!(
            codex_home_from(Some("/custom".into()), Some("/home/u".into())),
            Some(PathBuf::from("/custom"))
        );
        assert_eq!(
            codex_home_from(Some("".into()), Some("/home/u".into())),
            Some(PathBuf::from("/home/u/.codex"))
        );
        assert_eq!(codex_home_from(None, None), None);
    }

    #[test]
    fn only_the_models_the_cli_lists_are_offered_in_its_order() {
        let cache = json!({ "models": [
            { "slug": "gpt-6-luna", "display_name": "GPT-6-Luna", "visibility": "list", "priority": 7 },
            { "slug": "hidden", "display_name": "Hidden", "visibility": "hide", "priority": 1 },
            { "slug": "gpt-6.1-sol", "display_name": "GPT-6.1-Sol", "visibility": "list", "priority": 1 },
            { "slug": "gpt-5.6-luna", "visibility": "list", "priority": 10 },
        ]});
        let models = listed_models(&cache);
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["gpt-6.1-sol", "gpt-6-luna", "gpt-5.6-luna"]
        );
        assert_eq!(models[2].name, "gpt-5.6-luna");
        assert_eq!(default_model(&models).as_deref(), Some("gpt-6-luna"));
        assert_eq!(default_model(&models[..1]).as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(default_model(&[]), None);
    }

    #[test]
    fn the_request_is_a_stream_of_one_message_with_codex_effort_levels() {
        let body = request_body("gpt-6-luna", "Fix it.", "hallo", None);
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["input"][0]["content"][0]["text"], "hallo");
        assert!(body.get("reasoning").is_none());
        let minimal = request_body("m", "i", "t", Some(TextEffort::Minimal));
        assert_eq!(minimal["reasoning"]["effort"], "low");
        let high = request_body("m", "i", "t", Some(TextEffort::High));
        assert_eq!(high["reasoning"]["effort"], "high");
    }

    #[test]
    fn the_answer_is_read_from_the_event_stream() {
        let stream = "event: response.created\ndata: {\"type\":\"response.created\"}\n\n\
            data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hallo \"}\n\n\
            data: {\"type\":\"response.output_text.delta\",\"delta\":\"Welt.\"}\n\n\
            data: {\"type\":\"response.completed\"}\n\n";
        assert_eq!(answer_text(stream).unwrap(), "Hallo Welt.");

        let done = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hal\"}\n\
            data: {\"type\":\"response.output_text.done\",\"text\":\"Hallo Welt.\"}\n";
        assert_eq!(answer_text(done).unwrap(), "Hallo Welt.");

        let failed = "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"model busy\"}}}\n";
        assert_eq!(
            answer_text(failed).unwrap_err().message(),
            "ChatGPT: model busy"
        );
        assert_eq!(answer_text("").unwrap_err().class(), "empty_result");
    }

    #[test]
    fn refusals_say_what_the_user_can_do() {
        let limit = status_failure(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"resets_in_seconds":600}}"#,
        );
        assert_eq!(limit.class(), "codex_limit");
        assert!(limit.message().contains("10 min"));
        let expired = status_failure(StatusCode::UNAUTHORIZED, "");
        assert!(expired.message().contains("codex login"));
        let other = status_failure(
            StatusCode::BAD_REQUEST,
            r#"{"detail":"Stream must be set to true"}"#,
        );
        assert_eq!(other.message(), "ChatGPT: Stream must be set to true");
        assert_eq!(
            status_failure(StatusCode::BAD_GATEWAY, "<html>").message(),
            "ChatGPT request failed with HTTP 502 Bad Gateway"
        );
    }
}

/// Against the real subscription, with this machine's `codex login`:
/// `cargo test codex_live -- --ignored --nocapture`.
#[cfg(test)]
mod live {
    #[tokio::test]
    #[ignore = "uses the real ChatGPT login and subscription"]
    async fn codex_live_transform() {
        let settings = crate::domain::AppSettings {
            text_provider: crate::domain::TextProvider::Codex,
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let text = super::transform(
            crate::actions::find("clean").unwrap().prompt,
            "also ich habe am montag zeit bitte sag mir ob zehn uhr passt",
            &settings,
        )
        .await
        .unwrap();
        println!(
            "{} in {:?}: {text}",
            super::effective_model(&settings),
            started.elapsed()
        );
        assert!(!text.is_empty());
    }
}
