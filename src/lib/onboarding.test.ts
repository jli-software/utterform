import { describe, expect, it } from "vitest";
import { DEFAULT_SETTINGS } from "./types";
import { applyDraft, draftFrom, languageName, languageProblem, stepReady, systemLanguage } from "./onboarding";

const MODELS = [{ id: "base", name: "Whisper Base", description: "", sizeBytes: 1, downloaded: false }];

describe("first-run assistant choices", () => {
  it("suggests the system language as its two letters", () => {
    expect(systemLanguage(["de-CH", "en"])).toBe("de");
    expect(systemLanguage(["en"])).toBe("en");
    expect(systemLanguage([])).toBe("");
    expect(systemLanguage(["x"])).toBe("");
  });

  it("names a language in its own words, and nothing for a typo", () => {
    expect(languageName("de")).toBe("Deutsch");
    expect(languageName("deutsch!")).toBe("");
    expect(languageProblem("")).toBe("");
    expect(languageProblem("pt-br")).toBe("");
    expect(languageProblem("deutsch!")).not.toBe("");
  });

  it("starts from what is loaded and the recommended model", () => {
    const draft = draftFrom({ ...DEFAULT_SETTINGS, local_model_id: null, language_hints: ["fr", "en"] }, false, true);
    expect(draft).toMatchObject({ mode: "cloud", localModelId: "base", language: "fr", chatGpt: false, autostart: true, apiKey: "" });
  });

  it("writes only what it asked about, and keeps the cloud model through a local choice", () => {
    const loaded = { ...DEFAULT_SETTINGS, cloud_model: "gpt_live_transcribe" as const, language_hints: ["de", "en"], theme: "dark" as const };
    const draft = { ...draftFrom(loaded, true, false), mode: "local" as const, localModelId: "small", vocabulary: ["Careum"], chatGpt: true };
    const next = applyDraft(loaded, draft);
    expect(next).toMatchObject({ engine: "local_whisper", cloud_model: "gpt_live_transcribe", local_model_id: "small", language_hints: ["de", "en"], vocabulary: ["Careum"], text_provider: "codex", theme: "dark" });
    expect(applyDraft(loaded, { ...draft, language: "" }).language_hints).toEqual([]);
    expect(applyDraft(loaded, { ...draft, language: "it" }).language_hints).toEqual(["it"]);
  });

  it("holds Next back only for what the chosen path cannot run without", () => {
    const draft = draftFrom(DEFAULT_SETTINGS, false, false);
    expect(stepReady(1, draft, false, MODELS)).toBe(true);
    expect(stepReady(2, draft, false, MODELS)).toBe(false);
    expect(stepReady(2, draft, true, MODELS)).toBe(true);
    expect(stepReady(2, { ...draft, apiKey: "sk-x" }, false, MODELS)).toBe(true);
    expect(stepReady(2, { ...draft, mode: "local" }, false, MODELS)).toBe(true);
    expect(stepReady(2, { ...draft, mode: "local", localModelId: "gone" }, false, MODELS)).toBe(false);
    expect(stepReady(3, { ...draft, language: "??" }, false, MODELS)).toBe(false);
  });
});
