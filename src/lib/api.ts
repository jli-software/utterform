import { invoke } from "@tauri-apps/api/core";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import type {
  AppSettings,
  AudioDevice,
  BuiltInAction,
  Engine,
  HistoryEntry,
  HotkeySupport,
  LocalModel,
  LiveStatus,
  LiveSupport,
  OutputFormat,
  ProcessResult,
  RecordingStatus,
  RemoteIntent,
  TypingSupport,
} from "./types";

export const api = {
  liveSupport: () => invoke<LiveSupport>("live_support"),
  // Read when Settings opens, so a macOS user learns about the Accessibility
  // grant before the first delivery has to fail over it.
  typingSupport: () => invoke<TypingSupport>("typing_support"),
  getLiveStatus: () => invoke<LiveStatus | null>("get_live_status"),
  // Delivered once: the intent Utterform was launched with, for a hotkey that
  // had to start the app first.
  takeStartupIntent: () => invoke<RemoteIntent | null>("take_startup_intent"),
  globalHotkeySupport: () => invoke<HotkeySupport>("global_hotkey_support"),
  // Whether Utterform starts when the user signs in. The operating system's
  // own entry is the only place this is kept — no setting of ours mirrors it,
  // so nothing can disagree with it — and it is read back after every change.
  autostartEnabled: () => isEnabled(),
  enableAutostart: () => enable(),
  disableAutostart: () => disable(),
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
  startRecording: (
    inputDevice: string | null,
    engine: Engine,
    localModelId: string | null,
    action: string,
  ) => invoke<void>("start_recording", { inputDevice, engine, localModelId, action }),
  setRecordingPaused: (paused: boolean) => invoke<RecordingStatus>("set_recording_paused", { paused }),
  cancelRecording: () => invoke<void>("cancel_recording"),
  getRecordingStatus: () => invoke<RecordingStatus>("get_recording_status"),
  // The three cues, after a pause long enough to put the window in the
  // background — the way a hotkey recording plays them. The outcome arrives
  // on the `test-cues-finished` event.
  playTestCues: (delaySeconds: number) => invoke<void>("play_test_cues", { delaySeconds }),
  diagnosticsLogPath: () => invoke<string | null>("diagnostics_log_path"),
  finishRecording: (request: {
    action: string;
    customPrompt: string | null;
    copyToClipboard: boolean;
    saveToFile: boolean;
    typeAtCursor: boolean;
    outputFormat: OutputFormat;
  }) => invoke<ProcessResult>("finish_recording", { request }),
  hasOpenAiApiKey: () => invoke<boolean>("has_openai_api_key"),
  setOpenAiApiKey: (apiKey: string) => invoke<void>("set_openai_api_key", { apiKey }),
  deleteOpenAiApiKey: () => invoke<void>("delete_openai_api_key"),
  listLocalModels: () => invoke<LocalModel[]>("list_local_models"),
  downloadLocalModel: (modelId: string) =>
    invoke<void>("download_local_model", { modelId }),
  deleteLocalModel: (modelId: string) => invoke<void>("delete_local_model", { modelId }),
};
