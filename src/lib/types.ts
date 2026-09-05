export type Engine = "open_ai" | "local_whisper";
export type OutputFormat = "txt" | "md";
export type Theme = "light" | "dark" | "system";
export type ActionId = "plain" | "clean" | "polish" | "summarize" | "prompt";

export interface AppSettings {
  engine: Engine;
  input_device: string | null;
  output_directory: string | null;
  copy_to_clipboard: boolean;
  save_to_file: boolean;
  output_format: OutputFormat;
  local_model_id: string | null;
  language_hints: string[];
  text_model: string;
  theme: Theme;
  custom_actions: CustomAction[];
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

export interface ProcessResult {
  text: string;
  savedPath: string | null;
  deliveryWarnings: string[];
  durationMs: number;
  engine: Engine;
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
  output_format: "txt",
  local_model_id: "base",
  language_hints: [],
  text_model: "gpt-5-mini",
  theme: "system",
  custom_actions: [],
};

export const ACTIONS: Array<{ id: ActionId; label: string; hint: string; key: string }> = [
  { id: "plain", label: "Plain", hint: "Transcription only", key: "1" },
  { id: "clean", label: "Clean", hint: "Fix punctuation and obvious errors", key: "2" },
  { id: "polish", label: "Polish", hint: "Rewrite for clarity", key: "3" },
  { id: "summarize", label: "Summarize", hint: "Keep the essentials", key: "4" },
  { id: "prompt", label: "Prompt", hint: "Shape it into an AI prompt", key: "5" },
];
