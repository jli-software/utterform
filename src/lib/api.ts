import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AudioDevice,
  Engine,
  HistoryEntry,
  LocalModel,
  OutputFormat,
  ProcessResult,
  RecordingStatus,
  RemoteIntent,
} from "./types";

export const api = {
  // Delivered once: the intent Utterform was launched with, for a hotkey that
  // had to start the app first.
  takeStartupIntent: () => invoke<RemoteIntent | null>("take_startup_intent"),
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
