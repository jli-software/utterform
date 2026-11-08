use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::actions::ActionOverride;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionEngine {
    #[default]
    OpenAi,
    LocalWhisper,
}

impl TranscriptionEngine {
    /// The stored name, for diagnostic events.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAi => "open_ai",
            Self::LocalWhisper => "local_whisper",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloudModel {
    #[default]
    GptTranscribe,
    GptLiveTranscribe,
}

impl CloudModel {
    /// The stored name, for diagnostic events.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GptTranscribe => "gpt_transcribe",
            Self::GptLiveTranscribe => "gpt_live_transcribe",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Txt,
    Md,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Md => "md",
        }
    }
}

/// How the finished text reaches the focused window.
///
/// Synthesized keystrokes travel one character at a time and every hop can
/// drop or reorder one: a terminal that swallows the letter after a word
/// break turns "Session" into "ession". A paste moves the whole text at once,
/// so there is nothing to reorder, which is why it is the default.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TypingMethod {
    #[default]
    Paste,
    Keystrokes,
}

impl TypingMethod {
    /// The stored name, for diagnostic events.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paste => "paste",
            Self::Keystrokes => "keystrokes",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

/// Who runs the text actions. Transcription always uses the OpenAI key or a
/// local model; only the rewriting afterwards can go through ChatGPT.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TextProvider {
    /// The OpenAI API, paid per request with the stored key.
    #[default]
    #[serde(rename = "openai_key")]
    OpenAiKey,
    /// The user's ChatGPT subscription through the Codex CLI login.
    /// Experimental: OpenAI does not support it for third-party apps.
    #[serde(rename = "codex")]
    Codex,
}

impl TextProvider {
    /// The stored name, for diagnostic events.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiKey => "openai_key",
            Self::Codex => "codex",
        }
    }
}

/// How much the text model thinks before rewriting a transcript. Lower is
/// faster and cheaper; `None` sends nothing and leaves the model its own
/// default, so a model that does not know a level cannot reject the request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TextEffort {
    Minimal,
    Low,
    Medium,
    High,
}

impl TextEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub engine: TranscriptionEngine,
    pub cloud_model: CloudModel,
    pub input_device: Option<String>,
    pub output_directory: Option<String>,
    pub copy_to_clipboard: bool,
    pub save_to_file: bool,
    pub output_format: OutputFormat,
    pub local_model_id: Option<String>,
    pub language_hints: Vec<String>,
    pub type_at_cursor: bool,
    pub text_model: String,
    /// Where the text actions run. The OpenAI key stays in use for transcription.
    pub text_provider: TextProvider,
    /// The ChatGPT model for text actions. `None` takes the fastest model the
    /// Codex CLI lists, so a model OpenAI retires is never stuck in Settings.
    pub codex_model: Option<String>,
    /// Reasoning effort for the text transformation, where the model offers it.
    pub text_effort: Option<TextEffort>,
    /// Literal terms to expect in the audio: names, products, spellings the
    /// model would otherwise guess at. Sent to GPT Transcribe as keywords, and
    /// given to local Whisper as its initial prompt.
    pub vocabulary: Vec<String>,
    /// Free-form context about what is being recorded. GPT Transcribe only.
    pub transcription_context: String,
    pub theme: Theme,
    /// What the user typed in place of a shipped prompt, by action id.
    pub action_overrides: BTreeMap<String, ActionOverride>,
    pub custom_actions: Vec<CustomAction>,
    pub history_enabled: bool,
    pub sound_enabled: bool,
    pub typing_method: TypingMethod,
    /// Milliseconds between synthesized keystrokes. Ignored by paste delivery
    /// and on Windows, where one call injects the whole text atomically.
    pub typing_delay_ms: u32,
    /// A system-wide dictation key, where the operating system grants one.
    /// `None` disables it; Wayland ignores it and keeps using `--toggle`.
    pub global_hotkey: Option<String>,
    /// The Context action may read the selected text (Linux).
    pub context_selection: bool,
    /// The Context action may read the clipboard when nothing is selected.
    pub context_clipboard: bool,
    /// The action a dictation key press takes in a given application. Empty
    /// means the action chosen in the window applies everywhere.
    pub app_actions: Vec<AppAction>,
    /// The applications dictated into lately, newest first, to choose from
    /// when adding one of the above. Kept on this device only.
    pub recent_apps: Vec<KnownApp>,
}

/// An application as Settings knows it: the id rules match, and its name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownApp {
    pub id: String,
    pub name: String,
}

/// "In this application, take this action." `action` is a built-in id or
/// `custom:<id>`; a rule whose action no longer exists is ignored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppAction {
    pub app: KnownApp,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomAction {
    pub id: String,
    pub name: String,
    pub prompt: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            engine: TranscriptionEngine::OpenAi,
            cloud_model: CloudModel::GptTranscribe,
            input_device: None,
            output_directory: None,
            copy_to_clipboard: true,
            save_to_file: false,
            output_format: OutputFormat::Txt,
            local_model_id: Some("base".into()),
            language_hints: Vec::new(),
            type_at_cursor: false,
            text_model: "gpt-5-mini".into(),
            text_provider: TextProvider::OpenAiKey,
            codex_model: None,
            text_effort: None,
            vocabulary: Vec::new(),
            transcription_context: String::new(),
            theme: Theme::System,
            action_overrides: BTreeMap::new(),
            custom_actions: Vec::new(),
            history_enabled: true,
            sound_enabled: true,
            typing_method: TypingMethod::Paste,
            typing_delay_ms: crate::typing::DEFAULT_DELAY_MS,
            global_hotkey: Some(crate::hotkey::DEFAULT.to_string()),
            context_selection: true,
            context_clipboard: true,
            app_actions: Vec::new(),
            recent_apps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRequest {
    pub action: String,
    pub custom_prompt: Option<String>,
    pub copy_to_clipboard: bool,
    pub save_to_file: bool,
    pub type_at_cursor: bool,
    pub output_format: OutputFormat,
    /// What the Context action read when its recording started. Never sent by
    /// the interface; carried here so a kept recording retries with it.
    #[serde(skip)]
    pub context: Option<crate::context::Captured>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessResult {
    pub history_entry: Option<crate::history::HistoryEntry>,
    pub text: String,
    pub saved_path: Option<String>,
    pub copied_to_clipboard: bool,
    pub typed_at_cursor: bool,
    pub delivery_warnings: Vec<String>,
    pub duration_ms: u64,
    pub engine: TranscriptionEngine,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub size_bytes: u64,
    pub downloaded: bool,
}
