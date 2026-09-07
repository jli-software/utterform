use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionEngine {
    #[default]
    OpenAi,
    LocalWhisper,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub engine: TranscriptionEngine,
    pub input_device: Option<String>,
    pub output_directory: Option<String>,
    pub copy_to_clipboard: bool,
    pub save_to_file: bool,
    pub output_format: OutputFormat,
    pub local_model_id: Option<String>,
    pub language_hints: Vec<String>,
    pub type_at_cursor: bool,
    pub text_model: String,
    pub theme: Theme,
    pub custom_actions: Vec<CustomAction>,
    pub history_enabled: bool,
    pub sound_enabled: bool,
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
            input_device: None,
            output_directory: None,
            copy_to_clipboard: true,
            save_to_file: false,
            output_format: OutputFormat::Txt,
            local_model_id: Some("base".into()),
            language_hints: Vec::new(),
            type_at_cursor: false,
            text_model: "gpt-5-mini".into(),
            theme: Theme::System,
            custom_actions: Vec::new(),
            history_enabled: true,
            sound_enabled: true,
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
