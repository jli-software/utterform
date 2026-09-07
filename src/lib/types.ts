export type Engine = "open_ai" | "local_whisper";
export type OutputFormat = "txt" | "md";
export type Theme = "light" | "dark" | "system";
export type ActionId = "plain" | "clean" | "polish" | "summarize" | "prompt";
/// How the finished text reaches the focused window. A paste moves it whole;
/// keystrokes travel one character at a time and can be dropped on the way.
export type TypingMethod = "paste" | "keystrokes";
/// What a compositor hotkey or a second launch asks the running app to do.
export type RemoteIntent = "show" | "toggle" | "start" | "stop" | "cancel";

export interface AppSettings {
  engine: Engine;
  input_device: string | null;
  output_directory: string | null;
  copy_to_clipboard: boolean;
  save_to_file: boolean;
  type_at_cursor: boolean;
  output_format: OutputFormat;
  local_model_id: string | null;
  language_hints: string[];
  text_model: string;
  theme: Theme;
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
}

export interface CustomAction {
  id: string;
  name: string;
  prompt: string;
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
  input_device: null,
  output_directory: null,
  copy_to_clipboard: true,
  save_to_file: false,
  type_at_cursor: false,
  output_format: "txt",
  local_model_id: "base",
  language_hints: [],
  text_model: "gpt-5-mini",
  theme: "system",
  custom_actions: [],
  history_enabled: true,
  sound_enabled: true,
  typing_method: "paste",
  typing_delay_ms: 15,
  global_hotkey: "Ctrl+Alt+D",
};

export const ACTIONS: Array<{ id: ActionId; label: string; hint: string; key: string }> = [
  { id: "plain", label: "Plain", hint: "Transcription only", key: "1" },
  { id: "clean", label: "Clean", hint: "Fix punctuation and obvious errors", key: "2" },
  { id: "polish", label: "Polish", hint: "Rewrite for clarity", key: "3" },
  { id: "summarize", label: "Summarize", hint: "Keep the essentials", key: "4" },
  { id: "prompt", label: "Prompt", hint: "Shape it into an AI prompt", key: "5" },
];
