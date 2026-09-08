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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloudModel {
    #[default]
    GptTranscribe,
    GptLiveTranscribe,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
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
