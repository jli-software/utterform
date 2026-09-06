<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { isTauri } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { ACTIONS, DEFAULT_SETTINGS } from "./lib/types";
  import type {
    AppSettings,
    AudioDevice,
    DownloadProgress,
    HistoryEntry,
    LocalModel,
    ProcessResult,
    Theme,
  } from "./lib/types";
  import { api } from "./lib/api";
  import SelectMenu from "./lib/SelectMenu.svelte";
  import { version } from "../package.json";

  type Phase = "idle" | "starting" | "recording" | "processing" | "done" | "error";

  let phase: Phase = "idle";
  let settings: AppSettings = structuredClone(DEFAULT_SETTINGS);
  let devices: AudioDevice[] = [];
  let models: LocalModel[] = [];
  let selectedAction: string = "plain";
  let showSettings = false;
  let settingsSnapshot: AppSettings | null = null;
  let hasApiKey = false;
  let apiKeyInput = "";
  let elapsedSeconds = 0;
  let audioLevel = 0;
  let polling = false;
  let timer: ReturnType<typeof setInterval> | null = null;
  let result: ProcessResult | null = null;
  let history: HistoryEntry[] = [];
  let selectedHistoryId = "";
  let copyState = "Copy";
  let copyReset: ReturnType<typeof setTimeout> | null = null;
  let confirmClear = false;
  let historyMessage = "";
  const copyShortcut = /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘⇧C" : "Ctrl+Shift+C";
  let message = "Ready when you are";
  let downloadProgress: Record<string, number> = {};
  let busyModel: string | null = null;
  let unlistenProgress: UnlistenFn | null = null;
  let unlistenLimit: UnlistenFn | null = null;

  $: selectedHistory = history.find((entry) => entry.id === selectedHistoryId);
  $: displayedText = selectedHistory?.text ?? result?.text ?? "";
  $: actionOptions = [
    ...ACTIONS.map((action) => ({ value: action.id, label: action.label, hint: action.hint, key: action.key })),
    ...settings.custom_actions.map((action) => ({ value: `custom:${action.id}`, label: action.name, hint: "Custom action" })),
  ];
  $: historyOptions = history.map((entry, index) => ({ value: entry.id, label: entry.title, hint: index === 0 ? "Latest text" : undefined }));
  $: canRecord = settings.copy_to_clipboard || settings.save_to_file;
  $: controlsLocked = phase === "starting" || phase === "recording" || phase === "processing";

  onMount(async () => {
    applyTheme(settings.theme);
    if (!isTauri()) {
      message = "UI preview — launch the desktop app to record";
      window.addEventListener("keydown", handleKeyDown);
      return;
    }
    try {
      [settings, devices, models, hasApiKey] = await Promise.all([
        api.getSettings(),
        api.listInputDevices(),
        api.listLocalModels(),
        api.hasOpenAiApiKey(),
      ]);
      applyTheme(settings.theme);
      // Load independently: a damaged history must not disable microphone/settings setup.
      try {
        history = await api.listHistory();
        selectedHistoryId = history[0]?.id ?? "";
      } catch (error) {
        historyMessage = String(error);
      }
    } catch (error) {
      setError(error);
    }
    unlistenProgress = await listen<DownloadProgress>("model-download-progress", (event) => {
      const value = event.payload.totalBytes
        ? event.payload.downloadedBytes / event.payload.totalBytes
        : 0;
      downloadProgress = { ...downloadProgress, [event.payload.modelId]: value };
    });
    unlistenLimit = await listen("recording-limit-reached", () => {
      if (phase === "recording") void finishRecording();
    });
    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("keydown", handleKeyDown);
  });

  onDestroy(() => {
    stopTimer();
    if (copyReset) clearTimeout(copyReset);
    unlistenProgress?.();
    unlistenLimit?.();
    document.removeEventListener("visibilitychange", handleVisibilityChange);
    window.removeEventListener("keydown", handleKeyDown);
  });

  function isTypingTarget(target: EventTarget | null) {
    const element = target as HTMLElement | null;
    return Boolean(
      element?.isContentEditable ||
        element?.tagName === "INPUT" ||
        element?.tagName === "TEXTAREA" ||
        element?.tagName === "SELECT",
    );
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (!showSettings && (event.ctrlKey || event.metaKey) && event.shiftKey && !event.altKey && event.code === "KeyC") {
      event.preventDefault();
      if (!event.repeat) void copyDisplayedText();
      return;
    }
    if (showSettings || isTypingTarget(event.target) || event.metaKey || event.ctrlKey || event.altKey) {
      return;
    }
    if (event.code === "Space") {
      // Focused buttons already activate through their native Space handling.
      if ((event.target as HTMLElement | null)?.tagName === "BUTTON") return;
      event.preventDefault();
      if (!event.repeat && (phase === "recording" || phase === "idle" || phase === "done" || phase === "error")) {
        void toggleRecording();
      }
      return;
    }
    if (phase === "recording" && event.key === "Escape") {
      event.preventDefault();
      void cancelRecording();
      return;
    }
    if (phase === "idle" || phase === "done" || phase === "error") {
      const action = ACTIONS.find((item) => item.key === event.key);
      if (action) selectedAction = action.id;
      if (event.key.toLowerCase() === "c") {
        settings = { ...settings, copy_to_clipboard: !settings.copy_to_clipboard };
      }
      if (event.key.toLowerCase() === "f") {
        settings = { ...settings, save_to_file: !settings.save_to_file };
      }
    }
  }

  async function toggleRecording() {
    if (phase === "starting" || phase === "processing") return;
    if (phase === "recording") {
      await finishRecording();
    } else {
      await startRecording();
    }
  }

  async function startRecording() {
    if (!canRecord) {
      setError("Select Clipboard, File, or both as an output.");
      return;
    }
    if (settings.save_to_file && !settings.output_directory) {
      setError("Choose a default output folder in Settings before saving files.");
      return;
    }
    const requiresApiKey = settings.engine === "open_ai" || selectedAction !== "plain";
    if (requiresApiKey && !hasApiKey) {
      setError("Add an OpenAI API key in Settings before recording with this configuration.");
      return;
    }
    if (
      settings.engine === "local_whisper" &&
      !models.some((model) => model.id === settings.local_model_id && model.downloaded)
    ) {
      setError("Download and select a local Whisper model in Settings before recording.");
      return;
    }
    try {
      phase = "starting";
      message = "Preparing the microphone…";
      await api.saveSettings(settings);
      await api.startRecording(
        settings.input_device,
        settings.engine,
        settings.local_model_id,
        selectedAction.startsWith("custom:") ? "custom" : selectedAction,
      );
      elapsedSeconds = 0;
      phase = "recording";
      message = "Listening…";
      // Only visual/status polling lives in JS. Native capture and its ten-minute
      // cutoff do not depend on focus or WebView timer scheduling.
      timer = setInterval(() => void pollRecording(), 100);
    } catch (error) {
      setError(error);
    }
  }

  function handleVisibilityChange() {
    if (!document.hidden) void pollRecording();
  }

  async function pollRecording() {
    if (phase !== "recording" || polling || document.hidden) return;
    polling = true;
    try {
      const status = await api.getRecordingStatus();
      if (phase !== "recording") return;
      elapsedSeconds = status.elapsedSeconds;
      audioLevel = Math.max(0, Math.min(1, status.level));
      if (status.limitReached) await finishRecording();
    } catch {
      // Metering is non-critical. Keep capture running and Stop available.
      audioLevel = 0;
    } finally {
      polling = false;
    }
  }

  async function finishRecording() {
    if (phase !== "recording") return;
    stopTimer();
    phase = "processing";
    message = settings.engine === "open_ai" ? "Transcribing with GPT Transcribe…" : "Transcribing locally…";
    try {
      result = await api.finishRecording({
        action: selectedAction.startsWith("custom:") ? "custom" : selectedAction,
        customPrompt: selectedAction.startsWith("custom:")
          ? settings.custom_actions.find((action) => `custom:${action.id}` === selectedAction)?.prompt ?? null
          : null,
        copyToClipboard: settings.copy_to_clipboard,
        saveToFile: settings.save_to_file,
        outputFormat: settings.output_format,
      });
      if (result.historyEntry) {
        history = [result.historyEntry, ...history.filter((entry) => entry.id !== result?.historyEntry?.id)].slice(0, 100);
        selectedHistoryId = result.historyEntry.id;
      } else {
        selectedHistoryId = "";
      }
      copyState = "Copy";
      phase = "done";
      message = completionMessage(result);
    } catch (error) {
      setError(error);
    }
  }

  async function cancelRecording() {
    stopTimer();
    try {
      await api.cancelRecording();
      phase = "idle";
      elapsedSeconds = 0;
      message = "Recording discarded";
    } catch (error) {
      setError(error);
    }
  }

  function stopTimer() {
    if (timer) clearInterval(timer);
    timer = null;
    audioLevel = 0;
  }

  function setError(error: unknown) {
    stopTimer();
    phase = "error";
    message = error instanceof Error ? error.message : String(error);
  }

  function completionMessage(value: ProcessResult) {
    if (value.deliveryWarnings.length) return value.deliveryWarnings.join(" · ");
    if (value.savedPath && settings.copy_to_clipboard) return "Copied and saved";
    if (value.savedPath) return "File saved";
    return "Copied to clipboard";
  }

  async function copyDisplayedText() {
    if (!displayedText) return;
    try {
      await api.copyText(displayedText);
      copyState = "Copied";
      if (copyReset) clearTimeout(copyReset);
      copyReset = setTimeout(() => copyState = "Copy", 1800);
    } catch (error) {
      historyMessage = String(error);
    }
  }

  async function clearHistory() {
    if (!confirmClear) { confirmClear = true; return; }
    try {
      await api.clearHistory();
      history = [];
      selectedHistoryId = "";
      result = null;
      historyMessage = "History cleared. Clipboard and exported files are unchanged.";
      confirmClear = false;
    } catch (error) {
      historyMessage = String(error);
    }
  }

  function formatTime(total: number) {
    const minutes = Math.floor(total / 60).toString().padStart(2, "0");
    const seconds = (total % 60).toString().padStart(2, "0");
    return `${minutes}:${seconds}`;
  }

  function formatBytes(value: number) {
    return `${Math.round(value / 1024 / 1024)} MB`;
  }

  async function savePreferences() {
    try {
      await api.saveSettings(settings);
      if (apiKeyInput.trim()) {
        await api.setOpenAiApiKey(apiKeyInput);
        apiKeyInput = "";
        hasApiKey = true;
      }
      applyTheme(settings.theme);
      showSettings = false;
      settingsSnapshot = null;
      phase = phase === "error" ? "idle" : phase;
      message = "Settings saved";
    } catch (error) {
      setError(error);
    }
  }

  async function removeApiKey() {
    try {
      await api.deleteOpenAiApiKey();
      hasApiKey = false;
      apiKeyInput = "";
    } catch (error) {
      setError(error);
    }
  }

  async function chooseOutputFolder() {
    const selected = await open({ directory: true, multiple: false, title: "Choose output folder" });
    if (typeof selected === "string") settings = { ...settings, output_directory: selected };
  }

  async function toggleModel(model: LocalModel) {
    busyModel = model.id;
    try {
      if (model.downloaded) {
        await api.deleteLocalModel(model.id);
      } else {
        downloadProgress = { ...downloadProgress, [model.id]: 0 };
        await api.downloadLocalModel(model.id);
      }
      models = await api.listLocalModels();
    } catch (error) {
      setError(error);
    } finally {
      busyModel = null;
      const next = { ...downloadProgress };
      delete next[model.id];
      downloadProgress = next;
    }
  }

  function applyTheme(theme: Theme) {
    document.documentElement.dataset.theme = theme;
  }

  function openSettings() {
    settingsSnapshot = structuredClone(settings);
    confirmClear = false;
    showSettings = true;
  }

  function cancelSettings() {
    if (settingsSnapshot) settings = settingsSnapshot;
    settingsSnapshot = null;
    applyTheme(settings.theme);
    showSettings = false;
  }

  function addCustomAction() {
    const id = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
    settings = {
      ...settings,
      custom_actions: [...settings.custom_actions, { id, name: "Custom action", prompt: "" }],
    };
  }

  function updateCustomAction(id: string, field: "name" | "prompt", value: string) {
    settings = {
      ...settings,
      custom_actions: settings.custom_actions.map((action) =>
        action.id === id ? { ...action, [field]: value } : action,
      ),
    };
  }

  function removeCustomAction(id: string) {
    settings = {
      ...settings,
      custom_actions: settings.custom_actions.filter((action) => action.id !== id),
    };
    if (selectedAction === `custom:${id}`) selectedAction = "plain";
  }
</script>

<svelte:head><meta name="theme-color" content="#f5f5f8" /></svelte:head>

<main class:has-result={!!displayedText} class:recording={phase === "recording"} class:processing={phase === "processing"} style={`--energy: ${audioLevel}`}>
  <div class="ambience" aria-hidden="true">
    <div class="ambient-field"><div class="aurora aurora-one"></div><div class="aurora aurora-two"></div><div class="aurora aurora-three"></div>
      <div class="orbit orbit-one"></div><div class="orbit orbit-two"></div><div class="orbit orbit-three"></div>
    </div>
  </div>
  <header>
    <div class="brand">
      <div class="brand-mark" aria-hidden="true"><span></span></div>
      <div><strong>Utterform</strong><small>Speak once. Shape the text.</small></div>
    </div>
    <button class="icon-button" aria-label="Open settings" title="Settings" disabled={controlsLocked} onclick={openSettings}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15.5A3.5 3.5 0 1 0 12 8a3.5 3.5 0 0 0 0 7.5Zm8.4-2.2 1.4 1.1-2 3.5-1.8-.7a8 8 0 0 1-2.1 1.2l-.3 1.9h-4l-.3-1.9a8 8 0 0 1-2.1-1.2l-1.8.7-2-3.5 1.4-1.1a8 8 0 0 1 0-2.6L5.4 9.6l2-3.5 1.8.7a8 8 0 0 1 2.1-1.2l.3-1.9h4l.3 1.9A8 8 0 0 1 18 6.8l1.8-.7 2 3.5-1.4 1.1a8 8 0 0 1 0 2.6Z"/></svg>
    </button>
  </header>

  <section class="controls" aria-label="Transcription settings">
    <div class="action-control">
      <span class="control-label">Action</span>
      <SelectMenu id="action" label="Action" bind:value={selectedAction} options={actionOptions} disabled={controlsLocked} />
    </div>
    <div class="engine-chip" title={settings.engine === "open_ai" ? "Audio is sent to OpenAI" : "Audio stays on this device"}>
      <span class:local={settings.engine === "local_whisper"}></span>
      {settings.engine === "open_ai" ? "GPT Transcribe" : "Local Whisper"}
    </div>
  </section>

  <section class="recorder" aria-live="polite">

    <button
      class="mic-button"
      class:active={phase === "recording"}
      class:working={phase === "starting" || phase === "processing"}
      aria-label={phase === "recording" ? "Stop recording" : "Start recording"}
      disabled={phase === "starting" || phase === "processing"}
      onclick={toggleRecording}
    >
      {#if phase === "starting" || phase === "processing"}
        <span class="spinner"></span>
      {:else if phase === "recording"}
        <span class="stop-icon"></span>
      {:else}
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 14.5a3.5 3.5 0 0 0 3.5-3.5V5a3.5 3.5 0 1 0-7 0v6a3.5 3.5 0 0 0 3.5 3.5Zm6-3.5a1 1 0 1 0-2 0 4 4 0 0 1-8 0 1 1 0 1 0-2 0 6 6 0 0 0 5 5.91V19H8a1 1 0 1 0 0 2h8a1 1 0 1 0 0-2h-3v-2.09A6 6 0 0 0 18 11Z"/></svg>
      {/if}
    </button>
    <div class="timer" class:visible={phase === "recording"}><span class="record-dot" aria-hidden="true"></span>{formatTime(elapsedSeconds)}</div>
    <p class:error={phase === "error"}>{message}</p>
    <div class="shortcut"><kbd>Space</kbd><span>{phase === "recording" ? "Stop" : "Start"}</span></div>
  </section>

  <section class="output-bar" aria-label="Output selection">
    <span class="output-label">Send to</span>
    <button class:enabled={settings.copy_to_clipboard} onclick={() => (settings = { ...settings, copy_to_clipboard: !settings.copy_to_clipboard })} disabled={controlsLocked}>
      <svg viewBox="0 0 24 24"><path d="M8 5V3h8v2h2a2 2 0 0 1 2 2v13a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h2Zm2 0h4V4h-4v1Zm-4 2v13h12V7h-2v1H8V7H6Z"/></svg>
      Clipboard <kbd>C</kbd>
    </button>
    <button class:enabled={settings.save_to_file} onclick={() => (settings = { ...settings, save_to_file: !settings.save_to_file })} disabled={controlsLocked}>
      <svg viewBox="0 0 24 24"><path d="M4 2h12l4 4v16H4V2Zm2 2v16h12V7h-3V4H6Zm2 9h8v5H8v-5Zm1-8h4v4H9V5Z"/></svg>
      File <kbd>F</kbd>
    </button>
    <SelectMenu id="format" label="File format" value={settings.output_format}
      onchange={(value) => settings = { ...settings, output_format: value as AppSettings["output_format"] }}
      options={[{ value: "txt", label: "TXT", hint: "Plain text" }, { value: "md", label: "Markdown", hint: "Formatted text" }]}
      disabled={!settings.save_to_file || controlsLocked} compact upwards />
  </section>

  {#if displayedText}
    <section class="result-card" aria-label="Saved text">
      <div class="result-heading">
        <div class="result-title"><span>{selectedHistoryId && selectedHistoryId !== history[0]?.id ? "Previous text" : "Latest text"}</span>
          <small>{Math.max(1, Math.round((selectedHistory?.durationMs ?? result?.durationMs ?? 0) / 1000))}s audio</small></div>
        <button class="copy-button" onclick={copyDisplayedText} title={`Copy displayed text (${copyShortcut})`} aria-keyshortcuts="Control+Shift+C Meta+Shift+C">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 8V3h13v13h-5v5H3V8h5Zm2 0h6v6h3V5h-9v3ZM5 10v9h9v-9H5Z" /></svg>
          <span aria-live="polite">{copyState}</span><kbd>{copyShortcut}</kbd>
        </button>
      </div>
      <!-- Keyboard users need focus here to scroll long transcripts. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="transcript" role="region" tabindex="0" aria-label="Transcript">{displayedText}</div>
      {#if history.length}
        <div class="history-row"><span>Recent · {history.length}</span>
          <SelectMenu id="history" label="Recent texts" bind:value={selectedHistoryId} options={historyOptions} compact upwards onchange={() => copyState = "Copy"} />
        </div>
      {/if}
      {#if result?.savedPath && (!selectedHistoryId || selectedHistoryId === result.historyEntry?.id)}<small class="path">{result.savedPath}</small>{/if}
    </section>
  {/if}
  {#if historyMessage}<p class="history-notice" role="status">{historyMessage}</p>{/if}

  <footer><span>1–5 select an action</span><span>{phase === "recording" ? "Keeps recording in other apps · Esc discards" : "Esc discards a recording"}</span></footer>
</main>

{#if showSettings}
  <div class="modal-backdrop" role="presentation" onclick={(event) => event.target === event.currentTarget && cancelSettings()}>
    <div class="settings-modal" role="dialog" aria-modal="true" aria-labelledby="settings-title">
      <div class="modal-header"><div><small>UTTERFORM · {version} BETA</small><h2 id="settings-title">Settings</h2></div><button class="icon-button" aria-label="Close settings" onclick={cancelSettings}>×</button></div>

      <div class="settings-scroll">
        <div class="setting-group"><h3>Appearance</h3><div class="segmented three">
          {#each ["system", "light", "dark"] as theme}
            <button class:active={settings.theme === theme} onclick={() => { settings = { ...settings, theme: theme as Theme }; applyTheme(settings.theme); }}>{theme[0].toUpperCase() + theme.slice(1)}</button>
          {/each}
        </div></div>

        <div class="setting-group"><h3>Transcription</h3><div class="segmented">
          <button class:active={settings.engine === "open_ai"} onclick={() => (settings = { ...settings, engine: "open_ai" })}>GPT Transcribe</button>
          <button class:active={settings.engine === "local_whisper"} onclick={() => (settings = { ...settings, engine: "local_whisper" })}>Local Whisper</button>
        </div>
        <label class="field"><span>Microphone</span><select bind:value={settings.input_device}><option value={null}>System default</option>{#each devices as device}<option value={device.id}>{device.name}{device.isDefault ? " · default" : ""}</option>{/each}</select></label>
        <label class="field"><span>Language hints <small>comma-separated, optional</small></span><input value={settings.language_hints.join(", ")} oninput={(event) => (settings = { ...settings, language_hints: event.currentTarget.value.split(",").map((v) => v.trim()).filter(Boolean) })} placeholder="en, de, fr" /></label></div>

        <div class="setting-group"><h3>OpenAI</h3><label class="field"><span>API key <small>{hasApiKey ? "stored securely" : "not configured"}</small></span><div class="inline-field"><input type="password" autocomplete="off" bind:value={apiKeyInput} placeholder={hasApiKey ? "Enter a replacement key" : "Enter API key"} />{#if hasApiKey}<button class="danger-text" onclick={removeApiKey}>Remove</button>{/if}</div></label>
        <label class="field"><span>Text transformation model</span><input bind:value={settings.text_model} /></label><p class="privacy-note">GPT Transcribe sends audio to OpenAI. With Local Whisper, only text is sent when using Clean, Polish, Summarize, or Prompt.</p></div>

        <div class="setting-group"><div class="group-heading"><h3>Custom actions</h3><button onclick={addCustomAction}>Add action</button></div>
          {#if settings.custom_actions.length === 0}<p class="empty-note">Create reusable instructions for your own output styles.</p>{/if}
          <div class="custom-actions">{#each settings.custom_actions as action}<div class="custom-action"><div class="inline-field"><input aria-label="Action name" value={action.name} oninput={(event) => updateCustomAction(action.id, "name", event.currentTarget.value)} /><button class="danger-text" onclick={() => removeCustomAction(action.id)}>Remove</button></div><textarea aria-label="Action instructions" placeholder="Describe exactly how the transcript should be transformed…" value={action.prompt} oninput={(event) => updateCustomAction(action.id, "prompt", event.currentTarget.value)}></textarea></div>{/each}</div>
        </div>

        <div class="setting-group"><h3>Local Whisper models</h3><div class="model-list">{#each models as model}<div class="model-row"><div><strong>{model.name}</strong><span>{model.description} · {formatBytes(model.sizeBytes)}</span>{#if busyModel === model.id && !model.downloaded}<progress max="1" value={downloadProgress[model.id] ?? 0}></progress>{/if}</div><button class:downloaded={model.downloaded} disabled={busyModel !== null} onclick={() => toggleModel(model)}>{busyModel === model.id ? "Working…" : model.downloaded ? "Remove" : "Download"}</button></div>{/each}</div>
        <label class="field"><span>Selected local model</span><select bind:value={settings.local_model_id}>{#each models as model}<option value={model.id}>{model.name}{model.downloaded ? " · ready" : ""}</option>{/each}</select></label></div>

        <div class="setting-group"><h3>Recording feedback</h3>
          <label class="toggle-field"><input type="checkbox" bind:checked={settings.sound_enabled} /><span>Play a soft click when starting and stopping</span></label>
        </div>

        <div class="setting-group"><h3>Recent texts</h3>
          <label class="toggle-field"><input type="checkbox" bind:checked={settings.history_enabled} /><span>Remember the last 100 texts on this device</span></label>
          <p class="privacy-note">Stored locally, unencrypted, including clipboard-only results. Titles are made from the text without an AI request. Turning this off keeps existing history until you clear it.</p>
          <button class="clear-history" onclick={clearHistory}>{confirmClear ? "Confirm: delete all saved texts" : "Clear saved history"}</button>
          {#if confirmClear}<button class="clear-history" onclick={() => confirmClear = false}>Keep history</button>{/if}
          {#if historyMessage}<p class="privacy-note" role="status">{historyMessage}</p>{/if}
        </div>

        <div class="setting-group"><h3>File output</h3><label class="field"><span>Default output folder</span><div class="inline-field"><input readonly value={settings.output_directory ?? ""} placeholder="Choose a folder" /><button onclick={chooseOutputFolder}>Browse</button></div></label></div>
      </div>

      <div class="modal-actions"><button class="secondary" onclick={cancelSettings}>Cancel</button><button class="primary" onclick={savePreferences}>Save settings</button></div>
    </div>
  </div>
{/if}
