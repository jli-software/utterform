export type CloudModel = "gpt_transcribe" | "gpt_live_transcribe";
export type Engine = "open_ai" | "local_whisper";
export type OutputFormat = "txt" | "md";
export type Theme = "light" | "dark" | "system";
/// How hard the text model thinks before rewriting a transcript. `null` sends
/// nothing and leaves the model its own default.
export type TextEffort = "minimal" | "low" | "medium" | "high";
/// How the finished text reaches the focused window. A paste moves it whole;
/// keystrokes travel one character at a time and can be dropped on the way.
export type TypingMethod = "paste" | "keystrokes";
/// What a compositor hotkey or a second launch asks the running app to do.
export type RemoteIntent = "show" | "toggle" | "start" | "stop" | "cancel";

export interface AppSettings {
  engine: Engine;
  cloud_model: CloudModel;
  input_device: string | null;
  output_directory: string | null;
  copy_to_clipboard: boolean;
  save_to_file: boolean;
  type_at_cursor: boolean;
  output_format: OutputFormat;
  local_model_id: string | null;
  language_hints: string[];
  text_model: string;
  text_effort: TextEffort | null;
  vocabulary: string[];
  transcription_context: string;
  theme: Theme;
  action_overrides: Record<string, ActionOverride>;
  custom_actions: CustomAction[];
  history_enabled: boolean;
  sound_enabled: boolean;
  typing_method: TypingMethod;
  typing_delay_ms: number;
  global_hotkey: string | null;
}

/// What this session allows for a system-wide dictation key.
export interface HotkeySupport {
  supported: boolean;
  default: string;
  explanation: string;
  /// Why the stored key is not in effect — startup registration has no one to
  /// report to, so the reason waits here until Settings is opened.
  failure: string | null;
}

export interface CustomAction {
  id: string;
  name: string;
  prompt: string;
}

/// A prompt Utterform ships with. The text is a starting point: anything here
/// can be rewritten in Settings, and the replacement is kept in
/// `AppSettings.action_overrides` under the same id.
export interface BuiltInAction {
  id: string;
  name: string;
  hint: string;
  /// Empty for `plain`, which is delivered as transcribed and asks for nothing.
  prompt: string;
}

/// What the user put in place of a shipped name or prompt. A field left unset
/// still follows the default, so a later release that improves a default prompt
/// still reaches someone who only renamed the action.
export interface ActionOverride {
  name?: string | null;
  prompt?: string | null;
}

export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface LocalModel {
  id: string;
  name: string;
  description: string;
  sizeBytes: number;
  downloaded: boolean;
}

export interface HistoryEntry {
  id: string;
  createdAtMs?: number | null;
  title: string;
  text: string;
  durationMs: number;
  engine: Engine;
}

export interface ProcessResult {
  historyEntry: HistoryEntry | null;
  text: string;
  savedPath: string | null;
  copiedToClipboard: boolean;
  typedAtCursor: boolean;
  deliveryWarnings: string[];
  durationMs: number;
  engine: Engine;
}

export interface RecordingStatus {
  recording: boolean;
  paused: boolean;
  limitReached: boolean;
  elapsedSeconds: number;
  level: number;
}

export interface DownloadProgress {
  modelId: string;
  downloadedBytes: number;
  totalBytes: number;
}

export const DEFAULT_SETTINGS: AppSettings = {
  engine: "open_ai",
  cloud_model: "gpt_transcribe",
  input_device: null,
  output_directory: null,
  copy_to_clipboard: true,
  save_to_file: false,
  type_at_cursor: false,
  output_format: "txt",
  local_model_id: "base",
  language_hints: [],
  text_model: "gpt-5-mini",
  text_effort: null,
  vocabulary: [],
  transcription_context: "",
  theme: "system",
  action_overrides: {},
  custom_actions: [],
  history_enabled: true,
  sound_enabled: true,
  typing_method: "paste",
  typing_delay_ms: 15,
  global_hotkey: "Ctrl+Alt+D",
};

/// The actions to show until the backend answers with the shipped prompts, and
/// the whole list in a browser preview where there is no backend. The prompts
/// themselves live in Rust (`src-tauri/src/actions.rs`) and are never copied
/// here, so there is nothing to drift.
export const BUILT_IN_ACTIONS: BuiltInAction[] = [
  { id: "plain", name: "Plain", hint: "Transcription only", prompt: "" },
  { id: "clean", name: "Clean", hint: "Fix punctuation and obvious errors", prompt: "" },
  { id: "polish", name: "Polish", hint: "Rewrite for clarity", prompt: "" },
  { id: "summarize", name: "Summarize", hint: "Keep the essentials", prompt: "" },
  { id: "prompt", name: "Prompt", hint: "Shape it into an AI prompt", prompt: "" },
  { id: "email", name: "Email", hint: "Turn it into a ready-to-send email", prompt: "" },
];

/// The name an action goes by: the user's, where they gave it one.
export function actionName(action: BuiltInAction, overrides: Record<string, ActionOverride>) {
  const chosen = overrides[action.id]?.name?.trim();
  return chosen || action.name;
}

/// The instructions an action runs with. An emptied replacement falls back to
/// the shipped text, exactly as the backend resolves it before a recording.
export function actionPrompt(action: BuiltInAction, overrides: Record<string, ActionOverride>) {
  const chosen = overrides[action.id]?.prompt?.trim();
  return chosen || action.prompt;
}

/// Whether anything about this action is the user's rather than ours.
export function isActionEdited(id: string, overrides: Record<string, ActionOverride>) {
  const stored = overrides[id];
  return Boolean(stored?.name?.trim() || stored?.prompt?.trim());
}

/// A keyword carrying one of these is refused by the transcription API, and the
/// whole request goes with it, so Settings never lets one through.
export const FORBIDDEN_IN_KEYWORD = /[<>]/;

export interface LiveSupport {
  supported: boolean;
  explanation: string;
}

export interface LiveStatus {
  text: string;
  insertedText: string;
  deliveryPaused: boolean;
  warning: string | null;
  phase: "connecting" | "streaming" | "finishing" | "completed" | "failed";
}
