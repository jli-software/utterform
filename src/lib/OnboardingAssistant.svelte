<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api } from "./api";
  import ChipInput from "./ChipInput.svelte";
  import ShortcutRecorder from "./ShortcutRecorder.svelte";
  import { modalFocus } from "./modal-focus";
  import { isApplePlatform } from "./shortcut";
  import {
    ONBOARDING_STEPS,
    RECOMMENDED_MODEL,
    applyDraft,
    draftFrom,
    languageName,
    languageProblem,
    stepReady,
    type OnboardingDraft,
    type OnboardingStep,
  } from "./onboarding";
  import { FORBIDDEN_IN_KEYWORD, type AppSettings, type CodexStatus, type HotkeySupport, type LocalModel } from "./types";

  /// The settings as loaded. Never changed here: Finish hands the new ones to
  /// `ondone`, Skip hands these back marked done.
  export let settings: AppSettings;
  export let hasApiKey: boolean;
  export let models: LocalModel[];
  export let hotkeySupport: HotkeySupport;
  /// Download progress by model id, 0 to 1, from the window's own listener.
  export let downloadProgress: Record<string, number> = {};
  /// Lets go of the registered dictation key while a new one is read.
  export let oncapture: (capturing: boolean) => void = () => {};
  export let ondone: (result: { settings: AppSettings; hasApiKey: boolean; models: LocalModel[]; hotkeyApplied: boolean; skipped: boolean }) => void;

  type Stage = "settings" | "key" | "model" | "hotkey" | "autostart" | "complete";

  let step: OnboardingStep = 1;
  let codex: CodexStatus = { signedIn: false, models: [], defaultModel: null };
  let autostartOs = false;
  let autostartReadable = true;
  let draft: OnboardingDraft = draftFrom(settings, false, false);
  let busy = false;
  let progress = "";
  let errors: Partial<Record<Stage, string>> = {};
  let linkError = "";
  let heading: HTMLHeadingElement | undefined;
  /// What Finish already managed, so a retry does not do it twice.
  let appliedHotkey: string | null | undefined;
  const apple = isApplePlatform();

  $: ready = stepReady(step, draft, hasApiKey, models);
  $: chosenModel = models.find((model) => model.id === draft.localModelId);
  $: spokenName = languageName(draft.language);
  $: languageError = languageProblem(draft.language);
  $: downloaded = downloadProgress[draft.localModelId];

  onMount(() => {
    void refreshCodex();
    void readAutostart();
    window.addEventListener("focus", refreshCodex);
  });

  onDestroy(() => window.removeEventListener("focus", refreshCodex));

  /// `codex login` happens in a terminal while the assistant stays open.
  async function refreshCodex() {
    try {
      codex = await api.codexStatus();
      if (!codex.signedIn) draft = { ...draft, chatGpt: false };
    } catch {
      /* Keep what is showing. */
    }
  }

  /// The switch starts from the operating system's own entry.
  async function readAutostart() {
    try {
      autostartOs = await api.autostartEnabled();
      draft = { ...draft, autostart: autostartOs };
    } catch {
      autostartReadable = false;
    }
  }

  function go(next: OnboardingStep) {
    step = next;
    queueMicrotask(() => heading?.focus());
  }

  async function openKeyPage() {
    linkError = "";
    try {
      await api.openOpenAiApiKeys();
    } catch (error) {
      linkError = String(error);
    }
  }

  async function openCodexPage() {
    try {
      await api.openCodexSetup();
    } catch {
      /* The page is a convenience; Settings has the same link. */
    }
  }

  /// One activation, no question: the assistant closes for good and nothing
  /// of it is applied.
  async function skip() {
    if (busy) return;
    busy = true;
    errors = {};
    try {
      const done = { ...settings, onboarding_completed: true };
      await api.completeOnboarding(done, true);
      ondone({ settings: done, hasApiKey, models, hotkeyApplied: false, skipped: true });
    } catch (error) {
      errors = { complete: String(error) };
    } finally {
      busy = false;
    }
  }

  /// Everything the assistant changes outside itself happens here, in order,
  /// and setup counts as done only after the last step succeeded. A step that
  /// fails sends the user back to where it is chosen; whatever had already
  /// worked stays and is not repeated.
  async function finish() {
    if (busy) return;
    busy = true;
    errors = {};
    const next = applyDraft(settings, draft);
    let stage: Stage = "settings";
    try {
      progress = "Saving…";
      await api.saveSettings({ ...next, onboarding_completed: false });

      stage = "key";
      if (draft.mode === "cloud" && draft.apiKey.trim()) {
        progress = "Storing the key…";
        await api.setOpenAiApiKey(draft.apiKey.trim());
        hasApiKey = true;
        draft = { ...draft, apiKey: "" };
      }

      stage = "model";
      if (draft.mode === "local" && !chosenModel?.downloaded) {
        progress = `Downloading ${chosenModel?.name ?? "the model"}…`;
        await api.downloadLocalModel(draft.localModelId);
        models = await api.listLocalModels();
        if (!models.find((model) => model.id === draft.localModelId)?.downloaded) {
          throw new Error("The model did not arrive. Try again.");
        }
      }

      stage = "hotkey";
      const registered = appliedHotkey === undefined ? settings.global_hotkey : appliedHotkey;
      if (hotkeySupport.supported && draft.hotkey !== registered) {
        progress = "Setting the dictation key…";
        await api.applyGlobalHotkey(draft.hotkey);
        appliedHotkey = draft.hotkey;
      }

      stage = "autostart";
      if (autostartReadable && draft.autostart !== autostartOs) {
        progress = "Setting up start at login…";
        if (draft.autostart) await api.enableAutostart();
        else await api.disableAutostart();
        autostartOs = await api.autostartEnabled();
        if (autostartOs !== draft.autostart) {
          throw new Error(draft.autostart
            ? "Utterform could not be added to your startup items."
            : "Utterform could not be removed from your startup items.");
        }
      }

      stage = "complete";
      progress = "Finishing…";
      const done = { ...next, onboarding_completed: true };
      await api.completeOnboarding(done, false);
      ondone({ settings: done, hasApiKey, models, hotkeyApplied: appliedHotkey !== undefined, skipped: false });
    } catch (error) {
      errors = { [stage]: error instanceof Error ? error.message : String(error) };
      if (stage === "key" || stage === "model") go(2);
      else if (stage === "hotkey" || stage === "autostart") go(4);
    } finally {
      busy = false;
      progress = "";
    }
  }

  function formatBytes(value: number) {
    return `${Math.round(value / 1024 / 1024)} MB`;
  }
</script>

<div class="onboarding" use:modalFocus role="dialog" tabindex="-1" aria-modal="true" aria-labelledby="onboarding-heading">
  <div class="onboarding-body">
    <div class="onboarding-head">
      <span>Setup · {step} of {ONBOARDING_STEPS}</span>
      <span class="onboarding-dots" aria-hidden="true">{#each [1, 2, 3, 4] as item}<i class:on={item <= step}></i>{/each}</span>
    </div>

    {#if step === 1}
      <h1 id="onboarding-heading" tabindex="-1" bind:this={heading}>How should Utterform transcribe?</h1>
      <p class="onboarding-sub">Speak, and the text lands where you need it.</p>
      <div class="onboarding-choices" role="radiogroup" aria-labelledby="onboarding-heading">
        <button type="button" role="radio" class="onboarding-choice" class:selected={draft.mode === "cloud"} aria-checked={draft.mode === "cloud"} data-autofocus
          onclick={() => (draft = { ...draft, mode: "cloud" })}>
          <span class="choice-dot"></span>
          <span><strong>Cloud</strong><span class="badge">Recommended</span><small>GPT Transcribe · best accuracy · needs an OpenAI key</small></span>
        </button>
        <button type="button" role="radio" class="onboarding-choice" class:selected={draft.mode === "local"} aria-checked={draft.mode === "local"}
          onclick={() => (draft = { ...draft, mode: "local" })}>
          <span class="choice-dot"></span>
          <span><strong>On this device</strong><small>Local Whisper · offline · audio stays here</small></span>
        </button>
      </div>
    {:else if step === 2 && draft.mode === "cloud"}
      <h1 id="onboarding-heading" tabindex="-1" bind:this={heading}>Your OpenAI key</h1>
      <p class="onboarding-sub">Stays in the system keyring. OpenAI bills per use.</p>
      <label class="onboarding-field" for="onboarding-key">
        <span>OpenAI key {#if hasApiKey}<em class="key-state">· stored</em>{/if}</span>
      </label>
      <div class="onboarding-inline">
        <input id="onboarding-key" type="password" autocomplete="off" spellcheck="false" data-autofocus
          placeholder={hasApiKey ? "Keep the stored key" : "sk-…"} value={draft.apiKey}
          oninput={(event) => (draft = { ...draft, apiKey: event.currentTarget.value })} />
        <button type="button" class="row-button" onclick={openKeyPage}>Create key ↗</button>
      </div>
      {#if linkError}<p class="setting-error" role="alert">{linkError}</p>{/if}
      {#if errors.key}<p class="setting-error" role="alert">{errors.key}</p>{/if}
    {:else if step === 2}
      <h1 id="onboarding-heading" tabindex="-1" bind:this={heading}>Which model?</h1>
      <p class="onboarding-sub">Runs on this device. Larger is more accurate and slower.</p>
      <div class="onboarding-choices" role="radiogroup" aria-labelledby="onboarding-heading">
        {#each models as model (model.id)}
          <button type="button" role="radio" class="onboarding-choice" class:selected={draft.localModelId === model.id} aria-checked={draft.localModelId === model.id}
            data-autofocus={draft.localModelId === model.id ? "" : undefined}
            onclick={() => (draft = { ...draft, localModelId: model.id })}>
            <span class="choice-dot"></span>
            <span><strong>{model.name}</strong>{#if model.id === RECOMMENDED_MODEL}<span class="badge">Recommended</span>{/if}
              <small>{formatBytes(model.sizeBytes)} · {model.downloaded ? "downloaded" : model.description}</small></span>
          </button>
        {/each}
      </div>
      {#if chosenModel && !chosenModel.downloaded}<p class="row-note">Downloads when you finish.</p>{/if}
      {#if errors.model}<p class="setting-error" role="alert">{errors.model}</p>{/if}
    {:else if step === 3}
      <h1 id="onboarding-heading" tabindex="-1" bind:this={heading}>What do you speak?</h1>
      <p class="onboarding-sub">Type a language code. Empty means detect.</p>
      <label class="onboarding-field" for="onboarding-language"><span>Language</span></label>
      <div class="onboarding-language">
        <input id="onboarding-language" maxlength="12" autocomplete="off" spellcheck="false" placeholder="de" data-autofocus
          aria-invalid={!!languageError} aria-describedby="onboarding-language-name"
          value={draft.language} oninput={(event) => (draft = { ...draft, language: event.currentTarget.value.trim().toLowerCase() })} />
        <span id="onboarding-language-name" class="row-note">{spokenName}</span>
      </div>
      {#if languageError}<p class="setting-error" role="alert">{languageError}</p>{/if}
      <span class="onboarding-field"><span>Words · optional</span></span>
      <ChipInput id="onboarding-vocabulary" label="Words" values={draft.vocabulary} placeholder="Add a name or term"
        validate={(term) => (FORBIDDEN_IN_KEYWORD.test(term) ? "< and > are not allowed" : "")}
        flagged={(term) => FORBIDDEN_IN_KEYWORD.test(term)}
        onchange={(values) => (draft = { ...draft, vocabulary: values })} />
      <p class="row-note">Names and terms Utterform should spell right.</p>
    {:else}
      <h1 id="onboarding-heading" tabindex="-1" bind:this={heading}>Text actions</h1>
      <p class="onboarding-sub">Email, Polish, Context … rewrite what you said.</p>
      <div class="setting-row">
        <span class="row-label">
          <span>Use my ChatGPT subscription <span class="badge experimental">Experimental</span></span>
          <span class="row-note">{codex.signedIn ? "Signed in with the Codex CLI" : "Needs the Codex CLI, signed in"} · otherwise the OpenAI key</span>
        </span>
        <span class="row-control">
          {#if !codex.signedIn}<button type="button" class="row-button quiet" onclick={openCodexPage}>Set up ↗</button>{/if}
          <input type="checkbox" role="switch" class="switch" aria-label="Use my ChatGPT subscription" checked={draft.chatGpt} disabled={!codex.signedIn}
            onchange={(event) => (draft = { ...draft, chatGpt: event.currentTarget.checked })} />
        </span>
      </div>
      <div class="setting-row">
        <span class="row-label"><span>Dictation key</span>
          <span class="row-note">{hotkeySupport.supported ? "Press once to start, again to finish" : hotkeySupport.explanation}</span></span>
        {#if hotkeySupport.supported}
          <span class="row-control">
            <ShortcutRecorder id="onboarding-hotkey" label="Dictation key" value={draft.hotkey || hotkeySupport.default} {apple}
              onchange={(shortcut) => (draft = { ...draft, hotkey: shortcut })} {oncapture} />
            <input type="checkbox" role="switch" class="switch" aria-label="Dictation key" checked={draft.hotkey !== null}
              onchange={(event) => (draft = { ...draft, hotkey: event.currentTarget.checked ? hotkeySupport.default : null })} />
          </span>
        {/if}
        {#if errors.hotkey}<p class="setting-error row-message" role="alert">{errors.hotkey}</p>{/if}
      </div>
      <div class="setting-row">
        <span class="row-label"><span>Start at login</span><span class="row-note">Hidden in the tray</span></span>
        <span class="row-control">
          <input type="checkbox" role="switch" class="switch" aria-label="Start Utterform when I sign in" checked={draft.autostart} disabled={!autostartReadable}
            onchange={(event) => (draft = { ...draft, autostart: event.currentTarget.checked })} />
        </span>
        {#if errors.autostart}<p class="setting-error row-message" role="alert">{errors.autostart}</p>{/if}
      </div>
    {/if}

    {#if errors.settings || errors.complete}<p class="setting-error" role="alert">{errors.settings || errors.complete}</p>{/if}
  </div>

  <div class="onboarding-foot">
    <p class="onboarding-status" role="status" aria-live="polite">{progress}{#if busy && progress.startsWith("Downloading") && downloaded !== undefined}{` ${Math.round(downloaded * 100)} %`}{/if}</p>
    <div class="onboarding-actions">
      <button type="button" class="onboarding-skip" disabled={busy} onclick={skip}>Skip setup</button>
      <span>
        {#if step > 1}<button type="button" class="row-button" disabled={busy} onclick={() => go((step - 1) as OnboardingStep)}>Back</button>{/if}
        {#if step < ONBOARDING_STEPS}
          <button type="button" class="onboarding-next" disabled={busy || !ready} onclick={() => go((step + 1) as OnboardingStep)}>Next</button>
        {:else}
          <button type="button" class="onboarding-next" disabled={busy} onclick={finish}>{busy ? "Finishing…" : "Finish"}</button>
        {/if}
      </span>
    </div>
    <p class="onboarding-later">You can change all of this later in Settings.</p>
  </div>
</div>
