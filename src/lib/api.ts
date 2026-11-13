import { invoke } from "@tauri-apps/api/core";
import type {
  RetainedRecording,
  AppSettings,
  AudioDevice,
  BuiltInAction,
  CodexStatus,
  CopiedDiagnostics,
  DiagnosticsInfo,
  Engine,
  FrontendErrorReport,
  HistoryEntry,
  HotkeySupport,
  LocalModel,
  LiveStatus,
  LiveSupport,
  OutputFormat,
  FileSource,
  ProcessResult,
  RecordingStatus,
  RemoteIntent,
  TypingSupport,
  ContextSupport,
  ForegroundSupport,
  KnownApp,
} from "./types";

export const api = {
  liveSupport: () => invoke<LiveSupport>("live_support"),
  // Read when Settings opens, so a macOS user learns about the Accessibility
  // grant before the first delivery has to fail over it.
  typingSupport: () => invoke<TypingSupport>("typing_support"),
  contextSupport: () => invoke<ContextSupport>("context_support"),
  foregroundSupport: () => invoke<ForegroundSupport>("foreground_support"),
  // The application in front as the dictation key is pressed; `null` for
  // Utterform itself or a desktop that does not say.
  foregroundApp: () => invoke<KnownApp | null>("foreground_app"),
  announceAppAction: (action: string, actionName: string, appName: string) =>
    invoke<void>("announce_app_action", { action, actionName, appName }),
  getLiveStatus: () => invoke<LiveStatus | null>("get_live_status"),
  // Delivered once: the intent Utterform was launched with, for a hotkey that
  // had to start the app first.
  takeStartupIntent: () => invoke<RemoteIntent | null>("take_startup_intent"),
  globalHotkeySupport: () => invoke<HotkeySupport>("global_hotkey_support"),
  // Whether Utterform starts when the user signs in. The operating system's
  // own entry is the only place this is kept — no setting of ours mirrors it,
  // so nothing can disagree with it — and it is read back after every change.
  autostartEnabled: () => invoke<boolean>("autostart_enabled"),
  enableAutostart: () => invoke<void>("enable_autostart"),
  disableAutostart: () => invoke<void>("disable_autostart"),
  // Separate from saving settings: only a changed dictation key re-registers,
  // and a key another application holds is reported where it was entered.
  applyGlobalHotkey: (shortcut: string | null) =>
    invoke<void>("apply_global_hotkey", { shortcut }),
  // The prompts Utterform ships with. Fetched rather than copied into the
  // interface, so Settings shows and resets exactly the text a recording runs.
  listBuiltInActions: () => invoke<BuiltInAction[]>("list_built_in_actions"),
  getSettings: () => invoke<AppSettings>("get_settings"),
  listHistory: () => invoke<HistoryEntry[]>("list_history"),
  clearHistory: () => invoke<void>("clear_history"),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  saveSettings: (value: AppSettings) => invoke<void>("save_settings", { value }),
  listInputDevices: () => invoke<AudioDevice[]>("list_input_devices"),
  // The microphone's level now, in percent; rejects where it cannot be held.
  readMicrophoneLevel: (device: string | null) => invoke<number>("read_microphone_level", { device }),
  startRecording: (
    inputDevice: string | null,
    engine: Engine,
    localModelId: string | null,
    action: string,
  ) => invoke<void>("start_recording", { inputDevice, engine, localModelId, action }),
  setRecordingPaused: (paused: boolean) => invoke<RecordingStatus>("set_recording_paused", { paused }),
  cancelRecording: () => invoke<void>("cancel_recording"),
  retainedRecording: () => invoke<RetainedRecording | null>("retained_recording"),
  retryRecording: () => invoke<ProcessResult>("retry_recording"),
  discardRecording: () => invoke<void>("discard_recording"),
  notifyIfUnattended: (message: string, failed: boolean) => invoke<void>("notify_if_unattended", { message, failed }),
  getRecordingStatus: () => invoke<RecordingStatus>("get_recording_status"),
  // The three cues, after a pause long enough to put the window in the
  // background — the way a hotkey recording plays them. The outcome arrives
  // on the `test-cues-finished` event.
  playTestCues: (delaySeconds: number) => invoke<void>("play_test_cues", { delaySeconds }),
  // Support & diagnostics. None of these takes a path: the backend alone
  // decides which files are read, copied or opened.
  diagnosticsInfo: () => invoke<DiagnosticsInfo>("diagnostics_info"),
  copyDiagnostics: () => invoke<CopiedDiagnostics>("copy_diagnostics"),
  openLogFile: () => invoke<void>("open_log_file"),
  openLogFolder: () => invoke<void>("open_log_folder"),
  // A failure only the interface saw; answers with the reference to show.
  reportFrontendError: (report: FrontendErrorReport) =>
    invoke<string | null>("report_frontend_error", { report }),
  finishRecording: (request: {
    action: string;
    customPrompt: string | null;
    copyToClipboard: boolean;
    saveToFile: boolean;
    typeAtCursor: boolean;
    outputFormat: OutputFormat;
  }) => invoke<ProcessResult>("finish_recording", { request }),
  // An audio file: decoded, transcribed, then the same path as a recording.
  transcribeFile: (path: string, source: FileSource, request: {
    action: string;
    customPrompt: string | null;
    copyToClipboard: boolean;
    saveToFile: boolean;
    typeAtCursor: boolean;
    outputFormat: OutputFormat;
  }) => invoke<ProcessResult>("transcribe_file", { path, source, request }),
  // Files handed in from the command line or "Open with", one at a time.
  takePendingFile: () => invoke<string | null>("take_pending_file"),
  // Whether `codex login` signed the Codex CLI in to ChatGPT, and its models.
  codexStatus: () => invoke<CodexStatus>("codex_status"),
  openCodexSetup: () => invoke<void>("open_codex_setup"),
  // The page where an OpenAI key is made; the backend knows the address.
  openOpenAiApiKeys: () => invoke<void>("open_openai_api_keys"),
  // Marks setup done for good; a skip passes the settings as they were loaded.
  completeOnboarding: (value: AppSettings, skipped: boolean) =>
    invoke<void>("complete_onboarding", { value, skipped }),
  hasOpenAiApiKey: () => invoke<boolean>("has_openai_api_key"),
  setOpenAiApiKey: (apiKey: string) => invoke<void>("set_openai_api_key", { apiKey }),
  deleteOpenAiApiKey: () => invoke<void>("delete_openai_api_key"),
  listLocalModels: () => invoke<LocalModel[]>("list_local_models"),
  downloadLocalModel: (modelId: string) =>
    invoke<void>("download_local_model", { modelId }),
  deleteLocalModel: (modelId: string) => invoke<void>("delete_local_model", { modelId }),
};
