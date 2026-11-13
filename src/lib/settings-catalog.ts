/// Every row Settings has, where it lives, and the words that find it. The
/// search field filters this list; the categories show their part of it. A row
/// missing here can be neither shown nor found, so a new setting starts here.

export type SettingsCategory = "recording" | "transcription" | "ai" | "context" | "output" | "app";

export const SETTINGS_CATEGORIES: Array<{ id: SettingsCategory; label: string }> = [
  { id: "recording", label: "Recording" },
  { id: "transcription", label: "Transcription" },
  { id: "ai", label: "AI" },
  { id: "context", label: "Context" },
  { id: "output", label: "Output" },
  { id: "app", label: "App" },
];

export type SettingsRowId =
  | "microphone" | "mic-level" | "shortcut" | "sounds"
  | "engine" | "api-key" | "language" | "vocabulary" | "context" | "local-model"
  | "actions" | "text-provider" | "text-model"
  | "context-selection" | "context-clipboard" | "app-actions"
  | "typing" | "typing-delay" | "folder" | "history"
  | "theme" | "autostart" | "diagnostics";

export interface SettingsRow {
  id: SettingsRowId;
  category: SettingsCategory;
  label: string;
  /// Words someone might type for this row that its label does not contain.
  keywords: string[];
}

export const SETTINGS_ROWS: SettingsRow[] = [
  { id: "microphone", category: "recording", label: "Microphone", keywords: ["mic", "input", "device", "audio"] },
  { id: "mic-level", category: "recording", label: "Level", keywords: ["volume", "gain", "loud", "quiet", "input level", "hold", "lock", "teams", "pegel"] },
  { id: "shortcut", category: "recording", label: "Shortcut", keywords: ["hotkey", "key", "keyboard", "dictation", "global"] },
  { id: "sounds", category: "recording", label: "Sounds", keywords: ["sound", "cue", "beep", "chime", "click", "test"] },
  { id: "engine", category: "transcription", label: "Engine", keywords: ["cloud", "live", "local", "whisper", "gpt", "transcribe", "mode", "offline"] },
  { id: "api-key", category: "transcription", label: "OpenAI key", keywords: ["api", "token", "secret", "keyring"] },
  { id: "language", category: "transcription", label: "Language", keywords: ["languages", "hints", "german", "english", "deutsch"] },
  { id: "vocabulary", category: "transcription", label: "Vocabulary", keywords: ["words", "terms", "names", "dictionary", "spelling"] },
  { id: "context", category: "transcription", label: "Topic", keywords: ["context", "about", "recording context", "hint", "prompt"] },
  { id: "local-model", category: "transcription", label: "Local model", keywords: ["whisper", "download", "offline", "model", "on device"] },
  { id: "actions", category: "ai", label: "Actions", keywords: ["prompts", "prompt", "clean", "polish", "summarize", "email", "custom", "rewrite"] },
  { id: "text-provider", category: "ai", label: "Provider", keywords: ["chatgpt", "codex", "subscription", "plus", "pro", "abo", "login", "openai", "experimental"] },
  { id: "text-model", category: "ai", label: "Text model", keywords: ["model", "gpt", "effort", "thinking", "reasoning", "llm"] },
  { id: "context-selection", category: "context", label: "Selection", keywords: ["selected", "highlighted", "marked", "primary", "reply", "material"] },
  { id: "context-clipboard", category: "context", label: "Clipboard", keywords: ["copied", "copy", "paste", "material"] },
  { id: "app-actions", category: "context", label: "Per app", keywords: ["application", "apps", "program", "window", "mail", "terminal", "automatic", "rule"] },
  { id: "typing", category: "output", label: "Type at cursor", keywords: ["paste", "keystrokes", "typing", "insert"] },
  { id: "typing-delay", category: "output", label: "Keystroke delay", keywords: ["delay", "milliseconds", "slow", "typing"] },
  { id: "folder", category: "output", label: "Folder", keywords: ["file", "directory", "save", "export", "output"] },
  { id: "history", category: "output", label: "History", keywords: ["recent", "texts", "clear", "delete", "privacy"] },
  { id: "theme", category: "app", label: "Theme", keywords: ["appearance", "dark", "light", "design", "color"] },
  { id: "autostart", category: "app", label: "Start at login", keywords: ["autostart", "startup", "boot", "sign in", "login"] },
  { id: "diagnostics", category: "app", label: "Diagnostics", keywords: ["log", "logs", "support", "debug", "problem", "report"] },
];

/// What a search looks at: every word of the query has to appear somewhere in
/// the row's label, keywords or category, so "local mod" still finds the model.
export function rowMatches(row: SettingsRow, query: string) {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return false;
  const category = SETTINGS_CATEGORIES.find((item) => item.id === row.category)?.label ?? "";
  const haystack = [row.label, category, ...row.keywords].join(" ").toLowerCase();
  return words.every((word) => haystack.includes(word));
}

/// The rows a view shows: a category's own, or every row a search finds.
export function visibleRows(category: SettingsCategory, query: string): Set<SettingsRowId> {
  const searching = query.trim() !== "";
  return new Set(
    SETTINGS_ROWS
      .filter((row) => (searching ? rowMatches(row, query) : row.category === category))
      .map((row) => row.id),
  );
}

export function categoryLabel(id: SettingsRowId) {
  const row = SETTINGS_ROWS.find((item) => item.id === id);
  return SETTINGS_CATEGORIES.find((item) => item.id === row?.category)?.label ?? "";
}
