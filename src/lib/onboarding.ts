/// The first-run assistant's choices, kept apart from the settings until
/// Finish. Nothing in here touches the backend: a skip throws the draft away
/// and leaves exactly what was loaded.

import { LANGUAGE_CODE, type AppSettings, type LocalModel } from "./types";

export type OnboardingMode = "cloud" | "local";
export type OnboardingStep = 1 | 2 | 3 | 4;
export const ONBOARDING_STEPS = 4;

export interface OnboardingDraft {
  mode: OnboardingMode;
  /// Typed into the assistant; stored in the keyring on Finish, never in a file.
  apiKey: string;
  localModelId: string;
  /// One code, as typed. Empty lets the engine detect the language.
  language: string;
  vocabulary: string[];
  /// Text actions through the ChatGPT subscription (Codex login).
  chatGpt: boolean;
  hotkey: string | null;
  autostart: boolean;
}

/// The model the assistant recommends, and picks unless another is set.
export const RECOMMENDED_MODEL = "base";

/// The two letters of the system language, if it has any worth suggesting.
export function systemLanguage(locales: readonly string[] = typeof navigator === "undefined" ? [] : navigator.languages ?? [navigator.language]) {
  const primary = (locales[0] ?? "").split("-")[0].toLowerCase();
  return /^[a-z]{2,3}$/.test(primary) ? primary : "";
}

/// The language's own name for itself — "Deutsch" for `de` — or nothing when
/// the code is not one the platform knows.
export function languageName(code: string) {
  const value = code.trim().toLowerCase();
  if (!LANGUAGE_CODE.test(value)) return "";
  try {
    const name = new Intl.DisplayNames([value], { type: "language" }).of(value) ?? "";
    if (!name || name.toLowerCase() === value) return "";
    return name.charAt(0).toLocaleUpperCase(value) + name.slice(1);
  } catch {
    return "";
  }
}

export function languageProblem(code: string) {
  const value = code.trim();
  return !value || LANGUAGE_CODE.test(value) ? "" : "Use a code like de or en";
}

/// Where the assistant starts: what is loaded, the recommended model and the
/// system language for a new installation.
export function draftFrom(settings: AppSettings, hasCodexLogin: boolean, autostart: boolean): OnboardingDraft {
  return {
    mode: settings.engine === "local_whisper" ? "local" : "cloud",
    apiKey: "",
    localModelId: settings.local_model_id ?? RECOMMENDED_MODEL,
    language: settings.language_hints[0] ?? systemLanguage(),
    vocabulary: [...settings.vocabulary],
    chatGpt: hasCodexLogin && settings.text_provider === "codex",
    hotkey: settings.global_hotkey,
    autostart,
  };
}

/// The settings Finish writes. Fields the assistant does not ask about stay
/// as they are; a cloud model chosen before survives a local choice, as it
/// does in the main window.
export function applyDraft(settings: AppSettings, draft: OnboardingDraft): AppSettings {
  const language = draft.language.trim().toLowerCase();
  const language_hints = !language ? []
    : settings.language_hints[0] === language ? settings.language_hints
    : [language];
  return {
    ...settings,
    engine: draft.mode === "local" ? "local_whisper" : "open_ai",
    local_model_id: draft.mode === "local" ? draft.localModelId : settings.local_model_id,
    language_hints,
    vocabulary: [...draft.vocabulary],
    text_provider: draft.chatGpt ? "codex" : "openai_key",
    global_hotkey: draft.hotkey,
  };
}

/// Whether Next may leave `step`. Only what the chosen path cannot run
/// without holds it back; everything optional can stay empty.
export function stepReady(step: OnboardingStep, draft: OnboardingDraft, hasApiKey: boolean, models: LocalModel[]) {
  if (step === 2) {
    return draft.mode === "cloud"
      ? hasApiKey || draft.apiKey.trim() !== ""
      : models.some((model) => model.id === draft.localModelId);
  }
  if (step === 3) return !languageProblem(draft.language);
  return true;
}
