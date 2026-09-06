import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AudioDevice,
  Engine,
  LocalModel,
  OutputFormat,
  ProcessResult,
} from "./types";

export const api = {
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (value: AppSettings) => invoke<void>("save_settings", { value }),
  listInputDevices: () => invoke<AudioDevice[]>("list_input_devices"),
  startRecording: (
    inputDevice: string | null,
    engine: Engine,
    localModelId: string | null,
    action: string,
  ) => invoke<void>("start_recording", { inputDevice, engine, localModelId, action }),
  cancelRecording: () => invoke<void>("cancel_recording"),
  finishRecording: (request: {
    action: string;
    customPrompt: string | null;
    copyToClipboard: boolean;
    saveToFile: boolean;
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
