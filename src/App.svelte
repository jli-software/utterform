<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { isTauri } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    BUILT_IN_ACTIONS,
    DEFAULT_SETTINGS,
    FORBIDDEN_IN_KEYWORD,
    actionName,
    actionPrompt,
    isActionEdited,
  } from "./lib/types";
  import type {
    AppSettings,
    AudioDevice,
    BuiltInAction,
    DownloadProgress,
    HistoryEntry,
    HotkeySupport,
    LocalModel,
    ProcessResult,
    RemoteIntent,
    TextEffort,
    Theme,
  } from "./lib/types";
  import { api } from "./lib/api";
  import SelectMenu from "./lib/SelectMenu.svelte";
  import { version } from "../package.json";
  import brandIcon from "../src-tauri/icons/app-icon.svg";
  import { formatHistoryTime, fullHistoryDate, historyTimestamp } from "./lib/history-time";
  import { modalFocus } from "./lib/modal-focus";

  type Phase = "idle" | "starting" | "recording" | "paused" | "processing" | "done" | "error";
  type SettingsTab = "voice" | "prompts" | "output" | "general";

  const SETTINGS_TABS: Array<{ id: SettingsTab; label: string; hint: string }> = [
    { id: "voice", label: "Voice", hint: "Engine, microphone, vocabulary" },
    { id: "prompts", label: "Prompts", hint: "What each action asks for" },
    { id: "output", label: "Output", hint: "Where the finished text goes" },
    { id: "general", label: "General", hint: "Appearance, key, history" },
  ];
  const EFFORTS: Array<{ value: TextEffort | null; label: string }> = [
    { value: null, label: "Auto" },
    { value: "minimal", label: "Minimal" },
    { value: "low", label: "Low" },
    { value: "medium", label: "Medium" },
    { value: "high", label: "High" },
  ];

  let phase: Phase = "idle";
  let settings: AppSettings = structuredClone(DEFAULT_SETTINGS);
  let devices: AudioDevice[] = [];
  let models: LocalModel[] = [];
  let selectedAction: string = "plain";
  let showSettings = false;
  let settingsTab: SettingsTab = "voice";
  // The shipped prompts come from the backend so there is only ever one copy of
  // them; this list stands in until it answers, and in a browser preview.
  let builtInActions: BuiltInAction[] = BUILT_IN_ACTIONS;
  let selectedPrompt = "clean";
  // The editor writes through drafts rather than the resolved value: clearing
  // the box would otherwise drop the override and snap the shipped text back
  // under the cursor.
  let nameDraft = "";
  let promptDraft = "";
  /// Which prompt is showing the text Utterform ships, so it can be read before
  /// it is written over.
  let showsDefault = "";
  // Typed as written rather than reassembled from the parsed terms, so a blank
  // line being typed is not swallowed under the cursor.
  let vocabularyDraft = "";
  let settingsSnapshot: AppSettings | null = null;
  let hasApiKey = false;
  let apiKeyInput = "";
  // Until the backend answers, assume the session grants nothing: a field that
  // appears and then turns out to be dead is worse than one that arrives late.
  let hotkeySupport: HotkeySupport = { supported: false, default: "Ctrl+Alt+D", explanation: "", failure: null };
  let hotkeyError = "";
  let elapsedSeconds = 0;
  let audioLevel = 0;
  let polling = false;
  let changingPause = false;
  let recordingRevision = 0;
  let now = Date.now();
  let resultTimestamp: number | null = null;
  let historyTimer: ReturnType<typeof setInterval> | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let result: ProcessResult | null = null;
  let history: HistoryEntry[] = [];
  let selectedHistoryId = "";
  let resultExpanded = false;
  let copyState = "Copy";
  let copyRevision = 0;
  let copyPending: Promise<void> | null = null;
  let copyReset: ReturnType<typeof setTimeout> | null = null;
  let confirmClear = false;
  let historyMessage = "";
  let cueTestMessage = "";
  let cueTestPending = false;
  let logPath: string | null = null;
  let unlistenCueTest: UnlistenFn | undefined;
  const copyShortcut = /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘⇧C" : "Ctrl+Shift+C";
  let message = "Ready when you are";
  let downloadProgress: Record<string, number> = {};
  let busyModel: string | null = null;
  let unlistenProgress: UnlistenFn | null = null;
  let unlistenLimit: UnlistenFn | null = null;
  let unlistenIntent: UnlistenFn | null = null;
  let destroyed = false;

  $: selectedHistory = history.find((entry) => entry.id === selectedHistoryId);
  $: displayedText = selectedHistory?.text ?? result?.text ?? "";
  // Number keys follow the list, so a renamed or added action still has one.
  $: resolvedActions = builtInActions.map((action, index) => ({
    id: action.id,
    label: actionName(action, settings.action_overrides),
    hint: action.hint,
    key: index < 9 ? String(index + 1) : undefined,
  }));
  $: actionOptions = [
    ...resolvedActions.map((action) => ({ value: action.id, label: action.label, hint: action.hint, key: action.key })),
    ...settings.custom_actions.map((action) => ({ value: `custom:${action.id}`, label: action.name, hint: "Custom action" })),
  ];
  $: editedPrompt = builtInActions.find((action) => action.id === selectedPrompt) ?? null;
  $: editedCustom = settings.custom_actions.find((action) => `custom:${action.id}` === selectedPrompt) ?? null;
  // Split on lines, so a term can never carry the newline the API refuses; only
  // the angle brackets have to be caught here.
  $: rejectedTerms = vocabularyDraft
    .split("\n")
    .map((term) => term.trim())
    .filter((term) => term && FORBIDDEN_IN_KEYWORD.test(term));
  $: historyOptions = history.map((entry) => ({ value: entry.id, label: entry.title, hint: formatHistoryTime(historyTimestamp(entry), now) }));
  $: displayedTimestamp = selectedHistory ? historyTimestamp(selectedHistory) : resultTimestamp;
  $: recordingActive = phase === "recording" || phase === "paused";
  $: deviceOptions = [{ value: "", label: "System default" }, ...devices.map((device) => ({ value: device.id, label: device.name, hint: device.isDefault ? "Default microphone" : undefined }))];
  $: modelOptions = models.map((model) => ({ value: model.id, label: model.name, hint: model.downloaded ? "Ready on this device" : "Download required" }));
  $: canRecord = settings.copy_to_clipboard || settings.save_to_file || settings.type_at_cursor;
  // A key that could not be reserved at startup had nowhere to report; the
  // field it belongs to is where the user finds out. Derived rather than read
  // once, so a Settings dialog opened before the backend answered still shows it.
  $: hotkeyMessage = hotkeyError || hotkeySupport.failure || "";
  $: typingNote = settings.typing_method === "paste"
    ? "The whole text moves at once, so nothing can be dropped or reordered on the way — terminals included. It is left on the clipboard."
    : "The text is typed one character at a time. Windows that receive it faster than a person could type may drop letters; raise the delay if characters go missing.";
  $: controlsLocked = phase === "starting" || recordingActive || phase === "processing";

  onMount(async () => {
    applyTheme(settings.theme);
    historyTimer = setInterval(() => now = Date.now(), 15_000);
    // Registered before the first await: a component torn down mid-setup must
    // not leave a keyboard handler bound to the window after it is gone.
    window.addEventListener("keydown", handleKeyDown);
    document.addEventListener("visibilitychange", handleVisibilityChange);
    if (!isTauri()) {
      message = "UI preview — launch the desktop app to record";
      return;
    }
    try {
      [settings, devices, models, hasApiKey, hotkeySupport, builtInActions] = await Promise.all([
        api.getSettings(),
        api.listInputDevices(),
        api.listLocalModels(),
        api.hasOpenAiApiKey(),
        api.globalHotkeySupport(),
        api.listBuiltInActions(),
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
    if (destroyed) return;
    unlistenProgress = await listen<DownloadProgress>("model-download-progress", (event) => {
      const value = event.payload.totalBytes
        ? event.payload.downloadedBytes / event.payload.totalBytes
        : 0;
      downloadProgress = { ...downloadProgress, [event.payload.modelId]: value };
    });
    unlistenLimit = await listen("recording-limit-reached", () => {
      if (recordingActive && !changingPause) void finishRecording();
    });
    unlistenIntent = await listen<RemoteIntent>("remote-intent", (event) => {
      void applyIntent(event.payload);
    });
    unlistenCueTest = await listen<string>("test-cues-finished", (event) => {
      cueTestPending = false;
      cueTestMessage = event.payload;
    });
    if (destroyed) {
      unlistenProgress?.();
      unlistenLimit?.();
      unlistenIntent?.();
      unlistenCueTest?.();
      return;
    }
    try {
      logPath = await api.diagnosticsLogPath();
    } catch {
      logPath = null;
    }
    // A hotkey that had to start Utterform still means "record now".
    try {
      await applyIntent(await api.takeStartupIntent());
    } catch (error) {
      setError(error);
    }
  });

  onDestroy(() => {
    destroyed = true;
    stopTimer();
    if (historyTimer) clearInterval(historyTimer);
    if (copyReset) clearTimeout(copyReset);
    unlistenProgress?.();
    unlistenLimit?.();
    unlistenIntent?.();
    unlistenCueTest?.();
    document.removeEventListener("visibilitychange", handleVisibilityChange);
    window.removeEventListener("keydown", handleKeyDown);
  });

  /// A compositor hotkey routes through here so it takes exactly the same path
  /// as the buttons, including every guard against double starts.
  async function applyIntent(intent: RemoteIntent | null) {
    switch (intent) {
      case "toggle":
        if (recordingActive) await finishRecording();
        else if (phase !== "starting" && phase !== "processing") await startRecording();
        break;
      case "start":
        if (!recordingActive && phase !== "starting" && phase !== "processing") await startRecording();
        break;
      case "stop":
        if (recordingActive) await finishRecording();
        break;
      case "cancel":
        if (recordingActive) await cancelRecording();
        break;
      default:
        break;
    }
  }

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
      // While capturing, Space always finishes (even if Pause owns focus).
      // Otherwise preserve native button activation and menu keyboard handling.
      if (!recordingActive && (event.target as HTMLElement | null)?.tagName === "BUTTON") return;
      event.preventDefault();
      if (!event.repeat && (recordingActive || phase === "idle" || phase === "done" || phase === "error")) {
        void toggleRecording();
      }
      return;
    }
    if (recordingActive && event.code === "KeyP") {
      event.preventDefault();
      if (!event.repeat) void togglePause();
      return;
    }
    if (recordingActive && event.key === "Escape") {
      event.preventDefault();
      void cancelRecording();
      return;
    }
    if (phase === "idle" || phase === "done" || phase === "error") {
      const action = resolvedActions.find((item) => item.key === event.key);
      if (action) selectedAction = action.id;
      if (event.key.toLowerCase() === "c") {
        settings = { ...settings, copy_to_clipboard: !settings.copy_to_clipboard };
      }
      if (event.key.toLowerCase() === "f") {
        settings = { ...settings, save_to_file: !settings.save_to_file };
      }
      if (event.key.toLowerCase() === "t") {
        settings = { ...settings, type_at_cursor: !settings.type_at_cursor };
      }
    }
  }

  /// Five seconds is enough to click and switch to another window, which is
  /// the situation a hotkey recording plays its cues in.
  async function testCues() {
    cueTestMessage = "";
    cueTestPending = true;
    try {
      await api.playTestCues(5);
    } catch (error) {
      cueTestPending = false;
      cueTestMessage = String(error);
    }
  }

  async function toggleRecording() {
    if (phase === "starting" || phase === "processing" || changingPause) return;
    if (recordingActive) {
      await finishRecording();
    } else {
      await startRecording();
    }
  }

  async function togglePause() {
    if (!recordingActive || changingPause) return;
    changingPause = true;
    recordingRevision++;
    try {
      const status = await api.setRecordingPaused(phase !== "paused");
      elapsedSeconds = status.elapsedSeconds;
      audioLevel = 0;
      phase = status.paused ? "paused" : "recording";
      message = status.paused ? "Paused — continue when you’re ready" : "Listening…";
      changingPause = false;
      if (status.limitReached) await finishRecording();
    } catch (error) {
      // A failed pause must not abandon a still-active native recording.
      message = `Could not change pause: ${String(error)}`;
    } finally {
      changingPause = false;
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
      resetCopyFeedback();
      phase = "starting";
      message = "Preparing the microphone…";
      await api.saveSettings(settings);
      // Drain a preceding manual copy before a new session can deliver its result.
      await copyPending?.catch(() => {});
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
    if (!recordingActive || changingPause || polling || document.hidden) return;
    polling = true;
    const revision = recordingRevision;
    try {
      const status = await api.getRecordingStatus();
      if (!recordingActive || changingPause || revision !== recordingRevision) return;
      elapsedSeconds = status.elapsedSeconds;
      phase = status.paused ? "paused" : "recording";
      audioLevel = status.paused ? 0 : Math.max(0, Math.min(1, status.level));
      if (status.limitReached) await finishRecording();
    } catch {
      // Metering is non-critical. Keep capture running and Stop available.
      audioLevel = 0;
    } finally {
      polling = false;
    }
  }

  async function finishRecording() {
    if ((phase !== "recording" && phase !== "paused") || changingPause) return;
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
        typeAtCursor: settings.type_at_cursor,
        outputFormat: settings.output_format,
      });
      now = Date.now();
      resultTimestamp = now;
      if (result.historyEntry) {
        history = [result.historyEntry, ...history.filter((entry) => entry.id !== result?.historyEntry?.id)].slice(0, 100);
        selectedHistoryId = result.historyEntry.id;
      } else {
        selectedHistoryId = "";
      }
      resetCopyFeedback();
      if (result.copiedToClipboard) showCopyFeedback();
      phase = "done";
      message = completionMessage(result);
    } catch (error) {
      setError(error);
    }
  }

  async function cancelRecording() {
    if ((phase !== "recording" && phase !== "paused") || changingPause) return;
    stopTimer();
    phase = "starting";
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
    recordingRevision++;
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
    const done = [
      value.typedAtCursor ? "Typed at the cursor" : "",
      value.copiedToClipboard ? "Copied to clipboard" : "",
      value.savedPath ? "File saved" : "",
    ].filter(Boolean);
    return done.length ? done.join(" · ") : "Text ready";
  }

  function resetCopyFeedback() {
    copyRevision++;
    if (copyReset) clearTimeout(copyReset);
    copyReset = null;
    copyState = "Copy";
  }

  function showCopyFeedback() {
    resetCopyFeedback();
    copyState = "Copied";
    copyReset = setTimeout(() => { copyState = "Copy"; copyReset = null; }, 2400);
  }

  async function copyDisplayedText() {
    if (!displayedText || controlsLocked || copyPending) return;
    resetCopyFeedback();
    const revision = copyRevision;
    try {
      copyPending = api.copyText(displayedText);
      await copyPending;
      if (revision === copyRevision) showCopyFeedback();
    } catch (error) {
      if (revision === copyRevision) historyMessage = `Could not copy: ${String(error)}`;
    } finally {
      copyPending = null;
    }
  }

  async function clearHistory() {
    if (!confirmClear) { confirmClear = true; return; }
    try {
      await api.clearHistory();
      resetCopyFeedback();
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
    const previousHotkey = settingsSnapshot?.global_hotkey ?? null;
    try {
      await api.saveSettings(settings);
      if (apiKeyInput.trim()) {
        await api.setOpenAiApiKey(apiKeyInput);
        apiKeyInput = "";
        hasApiKey = true;
      }
      applyTheme(settings.theme);
      hotkeyError = "";
      // Also retried when the key is unchanged but never took effect, so
      // saving is the way to try again once the other application is gone.
      if (hotkeySupport.supported && (settings.global_hotkey !== previousHotkey || hotkeySupport.failure)) {
        try {
          await api.applyGlobalHotkey(settings.global_hotkey);
          hotkeySupport = { ...hotkeySupport, failure: null };
        } catch (error) {
          // Everything else is saved; only the key needs another attempt, so
          // the dialog stays open where the shortcut was entered.
          hotkeyError = String(error);
          hotkeySupport = { ...hotkeySupport, failure: hotkeyError };
          return;
        }
      }
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
    settingsTab = "voice";
    vocabularyDraft = settings.vocabulary.join("\n");
    selectPrompt(promptExists(selectedPrompt) ? selectedPrompt : firstEditablePrompt());
    showSettings = true;
  }

  function promptExists(id: string) {
    return builtInActions.some((action) => action.id === id)
      || settings.custom_actions.some((action) => `custom:${action.id}` === id);
  }

  function firstEditablePrompt() {
    return builtInActions.find((action) => action.prompt !== "" || action.id !== "plain")?.id
      ?? builtInActions[0]?.id
      ?? "";
  }

  function moveSettingsTab(event: KeyboardEvent, index: number) {
    const step = event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : event.key === "ArrowUp" || event.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const next = SETTINGS_TABS[(index + step + SETTINGS_TABS.length) % SETTINGS_TABS.length];
    settingsTab = next.id;
    // Follow the selection with focus, as a tablist is expected to.
    (document.getElementById(`settings-tab-${next.id}`) as HTMLElement | null)?.focus();
  }

  function updateVocabulary(value: string) {
    vocabularyDraft = value;
    settings = {
      ...settings,
      vocabulary: value.split("\n").map((term) => term.trim()).filter(Boolean),
    };
  }

  /// Store only what differs from what we ship. A field returned to the default
  /// stops being an override, so a later release can still improve it, and an
  /// action with nothing left of the user's follows the default again.
  function overrideAction(action: BuiltInAction, field: "name" | "prompt", value: string) {
    const shipped = field === "name" ? action.name : action.prompt;
    const stored = { ...settings.action_overrides[action.id] };
    if (value.trim() === shipped.trim() || !value.trim()) delete stored[field];
    else stored[field] = value;
    const overrides = { ...settings.action_overrides };
    if (stored.name?.trim() || stored.prompt?.trim()) overrides[action.id] = stored;
    else delete overrides[action.id];
    settings = { ...settings, action_overrides: overrides };
  }

  function resetAction(id: string) {
    const overrides = { ...settings.action_overrides };
    delete overrides[id];
    settings = { ...settings, action_overrides: overrides };
    selectPrompt(id);
  }

  function selectPrompt(id: string) {
    selectedPrompt = id;
    showsDefault = "";
    const builtIn = builtInActions.find((action) => action.id === id);
    const custom = settings.custom_actions.find((action) => `custom:${action.id}` === id);
    nameDraft = builtIn ? actionName(builtIn, settings.action_overrides) : custom?.name ?? "";
    promptDraft = builtIn ? actionPrompt(builtIn, settings.action_overrides) : custom?.prompt ?? "";
  }

  function cancelSettings() {
    hotkeyError = "";
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
    selectPrompt(`custom:${id}`);
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
    if (selectedPrompt === `custom:${id}`) selectPrompt(firstEditablePrompt());
  }
</script>

<svelte:head><meta name="theme-color" content="#f5f5f8" /></svelte:head>

<main inert={showSettings} class:has-result={!!displayedText} class:paused={phase === "paused"} class:recording={phase === "recording"} class:processing={phase === "processing"} style={`--energy: ${audioLevel}`}>
  <div class="ambience" aria-hidden="true">
    <div class="ambient-field"><div class="aurora aurora-one"></div><div class="aurora aurora-two"></div><div class="aurora aurora-three"></div>
      <div class="orbit orbit-one"></div><div class="orbit orbit-two"></div><div class="orbit orbit-three"></div>
    </div>
  </div>
  <header>
    <div class="brand">
      <img class="brand-mark" src={brandIcon} alt="" aria-hidden="true" />
      <div><strong>Utterform</strong><small>Speak once. Shape the text.</small></div>
    </div>
    <button class="icon-button settings-trigger" aria-label="Open settings" title="Settings" disabled={controlsLocked} onclick={openSettings}>
      <svg class="settings-glyph" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h5m6 0h5M4 17h9m6 0h1"/><circle cx="12" cy="7" r="3"/><circle cx="16" cy="17" r="3"/></svg>
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
    <div class="recording-controls" class:capturing={recordingActive}>
    <button
      class="mic-button"
      class:active={recordingActive}
      class:working={phase === "starting" || phase === "processing"}
      aria-label={recordingActive ? "Stop recording" : "Start recording"}
      disabled={phase === "starting" || phase === "processing" || changingPause}
      onclick={toggleRecording}
    >
      {#if phase === "starting" || phase === "processing"}
        <span class="spinner"></span>
      {:else if recordingActive}
        <span class="stop-icon"></span>
      {:else}
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 14.5a3.5 3.5 0 0 0 3.5-3.5V5a3.5 3.5 0 1 0-7 0v6a3.5 3.5 0 0 0 3.5 3.5Zm6-3.5a1 1 0 1 0-2 0 4 4 0 0 1-8 0 1 1 0 1 0-2 0 6 6 0 0 0 5 5.91V19H8a1 1 0 1 0 0 2h8a1 1 0 1 0 0-2h-3v-2.09A6 6 0 0 0 18 11Z"/></svg>
      {/if}
    </button>
    {#if recordingActive}
      <button class="pause-button" aria-label={phase === "paused" ? "Resume recording" : "Pause recording"} aria-keyshortcuts="P" disabled={changingPause} onclick={togglePause}>
        <svg viewBox="0 0 20 20" aria-hidden="true">{#if phase === "paused"}<path d="m7 4 9 6-9 6Z"/>{:else}<path d="M5 4h3v12H5zM12 4h3v12h-3z"/>{/if}</svg>
        {phase === "paused" ? "Resume" : "Pause"}<kbd>P</kbd>
      </button>
    {/if}
    </div>
    <div class="timer" class:visible={recordingActive}><span class="record-dot" aria-hidden="true"></span>{formatTime(elapsedSeconds)}</div>
    <p class:error={phase === "error"}>{message}</p>
    <div class="shortcut"><kbd>Space</kbd><span>{recordingActive ? "Finish" : "Start"}</span></div>
  </section>

  <section class="output-bar" aria-label="Output selection">
    <span class="output-label">Send to</span>
    <button class:enabled={settings.copy_to_clipboard} onclick={() => (settings = { ...settings, copy_to_clipboard: !settings.copy_to_clipboard })} disabled={controlsLocked} title="Copy to clipboard">
      <svg viewBox="0 0 24 24"><path d="M8 5V3h8v2h2a2 2 0 0 1 2 2v13a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h2Zm2 0h4V4h-4v1Zm-4 2v13h12V7h-2v1H8V7H6Z"/></svg>
      <span class="output-name">Clipboard</span> <kbd>C</kbd>
    </button>
    <button class:enabled={settings.save_to_file} onclick={() => (settings = { ...settings, save_to_file: !settings.save_to_file })} disabled={controlsLocked} title="Save to file">
      <svg viewBox="0 0 24 24"><path d="M4 2h12l4 4v16H4V2Zm2 2v16h12V7h-3V4H6Zm2 9h8v5H8v-5Zm1-8h4v4H9V5Z"/></svg>
      <span class="output-name">File</span> <kbd>F</kbd>
    </button>
    <button class:enabled={settings.type_at_cursor} onclick={() => (settings = { ...settings, type_at_cursor: !settings.type_at_cursor })} disabled={controlsLocked} title="Type the finished text into whatever window has focus">
      <svg viewBox="0 0 24 24"><path d="M3 5h18v14H3V5Zm2 2v10h14V7H5Zm2 2h2v2H7V9Zm3 0h2v2h-2V9Zm3 0h2v2h-2V9Zm3 0h2v2h-2V9ZM7 12h2v2H7v-2Zm3 0h2v2h-2v-2Zm3 0h2v2h-2v-2Zm3 0h2v2h-2v-2Zm-7 3h6v2H9v-2Z"/></svg>
      <span class="output-name">Type</span> <kbd>T</kbd>
    </button>
    <SelectMenu id="format" label="File format" value={settings.output_format}
      onchange={(value) => settings = { ...settings, output_format: value as AppSettings["output_format"] }}
      options={[{ value: "txt", label: "TXT", hint: "Plain text" }, { value: "md", label: "Markdown", hint: "Formatted text" }]}
      disabled={!settings.save_to_file || controlsLocked} compact upwards />
  </section>

  {#if displayedText}
    <section class="result-card" aria-label="Saved text">
      <div class="result-heading">
        <button class="result-toggle" aria-expanded={resultExpanded} aria-controls="result-details" onclick={() => resultExpanded = !resultExpanded}>
          <svg viewBox="0 0 20 20" aria-hidden="true" class:expanded={resultExpanded}><path d="m7 4 6 6-6 6" /></svg>
          <span class="result-title"><span>{selectedHistoryId && selectedHistoryId !== history[0]?.id ? "Previous text" : "Latest text"}</span>
          <small>{Math.max(1, Math.round((selectedHistory?.durationMs ?? result?.durationMs ?? 0) / 1000))}s audio</small></span>
        </button>
        <button class="copy-button" class:copied={copyState === "Copied"} disabled={controlsLocked || !!copyPending} onclick={copyDisplayedText} title={`Copy displayed text (${copyShortcut})`} aria-keyshortcuts="Control+Shift+C Meta+Shift+C">
          <svg viewBox="0 0 24 24" aria-hidden="true">{#if copyState === "Copied"}<path d="m9 16.2-4.2-4.2L3.4 13.4 9 19 21 7l-1.4-1.4Z" />{:else}<path d="M8 8V3h13v13h-5v5H3V8h5Zm2 0h6v6h3V5h-9v3ZM5 10v9h9v-9H5Z" />{/if}</svg>
          <span aria-live="polite">{copyState}</span><kbd>{copyShortcut}</kbd>
        </button>
      </div>
      <div id="result-details" hidden={!resultExpanded}>
      <div class="result-date" title={fullHistoryDate(displayedTimestamp)}>
        {#if displayedTimestamp !== null}<time datetime={new Date(displayedTimestamp).toISOString()}>{formatHistoryTime(displayedTimestamp, now)}</time>{:else}<span>Date unavailable</span>{/if}
      </div>
      <!-- Keyboard users need focus here to scroll long transcripts. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="transcript" role="region" tabindex="0" aria-label="Transcript">{displayedText}</div>
      {#if history.length}
        <div class="history-row"><span>Recent · {history.length}</span>
          <SelectMenu id="history" label="Recent texts" bind:value={selectedHistoryId} options={historyOptions} compact upwards onchange={resetCopyFeedback} />
        </div>
      {/if}
      {#if result?.savedPath && (!selectedHistoryId || selectedHistoryId === result.historyEntry?.id)}<small class="path">{result.savedPath}</small>{/if}
      </div>
    </section>
  {/if}
  {#if historyMessage}<p class="history-notice" role="status">{historyMessage}</p>{/if}

  <footer><span>1–5 select an action</span><span>{phase === "paused" ? "Paused · P resumes · Esc discards" : phase === "recording" ? "Keeps recording in other apps · Esc discards" : "Esc discards a recording"}</span></footer>
</main>

{#if showSettings}
  <div class="modal-backdrop" role="presentation" onclick={(event) => event.target === event.currentTarget && cancelSettings()}>
    <div class="settings-modal" use:modalFocus role="dialog" tabindex="-1" aria-modal="true" aria-labelledby="settings-title" onkeydown={(event) => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); cancelSettings(); } }}>
      <div class="modal-header"><div class="settings-brand"><img src={brandIcon} alt="" /><div><small>UTTERFORM · {version}</small><h2 id="settings-title">Settings</h2><p>Make room for your way of working.</p></div></div><button class="icon-button" aria-label="Close settings" onclick={cancelSettings}><svg class="line-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18"/></svg></button></div>

      <div class="settings-body">
        <div class="settings-rail" role="tablist" aria-label="Settings sections" aria-orientation="vertical">
          {#each SETTINGS_TABS as tab, index}
            <button id={`settings-tab-${tab.id}`} role="tab" class:active={settingsTab === tab.id}
              aria-selected={settingsTab === tab.id} aria-controls="settings-panel"
              tabindex={settingsTab === tab.id ? 0 : -1}
              onclick={() => (settingsTab = tab.id)} onkeydown={(event) => moveSettingsTab(event, index)}>
              <strong>{tab.label}</strong><small>{tab.hint}</small>
            </button>
          {/each}
        </div>

        <div class="settings-scroll" id="settings-panel" role="tabpanel" aria-labelledby={`settings-tab-${settingsTab}`}>
        {#if settingsTab === "voice"}
        <div class="setting-group"><h3>Transcription</h3><div class="segmented">
          <button class:active={settings.engine === "open_ai"} onclick={() => (settings = { ...settings, engine: "open_ai" })}>GPT Transcribe</button>
          <button class:active={settings.engine === "local_whisper"} onclick={() => (settings = { ...settings, engine: "local_whisper" })}>Local Whisper</button>
        </div>
        <div class="field"><span>Microphone</span><SelectMenu id="microphone" label="Microphone" value={settings.input_device ?? ""} options={deviceOptions} onchange={(value) => settings = { ...settings, input_device: value || null }} /></div>
        <label class="field"><span>Language hints <small>comma-separated, optional</small></span><input value={settings.language_hints.join(", ")} oninput={(event) => (settings = { ...settings, language_hints: event.currentTarget.value.split(",").map((v) => v.trim()).filter(Boolean) })} placeholder="en, de, fr" /></label>
        <p class="privacy-note">GPT Transcribe sends audio to OpenAI. With Local Whisper, only text is sent when an action other than Plain is used.</p></div>

        <div class="setting-group"><div class="group-heading"><h3>Vocabulary</h3><span class="section-badge">Both engines</span></div>
          <p class="section-description">Names, products and spellings the model would otherwise guess at. One per line.</p>
          <label class="field"><span class="visually-hidden">Vocabulary</span><textarea class="vocabulary" rows="4" aria-label="Vocabulary" placeholder={"Careum\nUtterform\nOmarchy"} value={vocabularyDraft} oninput={(event) => updateVocabulary(event.currentTarget.value)}></textarea></label>
          {#if rejectedTerms.length}
            <p class="setting-error" role="alert">Not sent, because the transcription API refuses a term containing &lt; or &gt;: {rejectedTerms.join(", ")}</p>
          {/if}
          <label class="field"><span>Recording context <small>optional, GPT Transcribe only</small></span><textarea rows="2" placeholder="A standup about the billing rewrite." value={settings.transcription_context} oninput={(event) => (settings = { ...settings, transcription_context: event.currentTarget.value })}></textarea></label>
          <p class="privacy-note">GPT Transcribe takes the words as keywords; local Whisper is given them as the text it starts from. They are hints either way — the model still transcribes what it hears.</p>
        </div>

        <div class="setting-group local-models"><div class="group-heading"><h3>Local Whisper models</h3><span class="section-badge">On-device audio</span></div>
          <p class="section-description">Your voice stays here. Choose the balance of speed and accuracy that suits you.</p>
          <div class="model-list">{#each models as model}<div class="model-row" class:selected={model.downloaded && settings.local_model_id === model.id}>
            <div class="model-symbol" aria-hidden="true"><svg viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" rx="3"/><path d="M9 2v4m6-4v4M9 18v4m6-4v4M2 9h4m-4 6h4m12-6h4m-4 6h4"/></svg></div>
            <div class="model-info"><strong>{model.name}<span class="model-state">{model.downloaded ? settings.local_model_id === model.id ? "Selected" : "Ready" : formatBytes(model.sizeBytes)}</span></strong><span class="model-description">{model.description}</span>
              {#if busyModel === model.id && !model.downloaded}<progress aria-label={`Downloading ${model.name}`} max="1" value={downloadProgress[model.id] ?? 0}></progress>{/if}
            </div>
            <button class:downloaded={model.downloaded} disabled={busyModel !== null} aria-label={`${model.downloaded ? "Remove" : "Download"} ${model.name}`} onclick={() => toggleModel(model)}>{busyModel === model.id ? "Working…" : model.downloaded ? "Remove" : "Download"}</button>
          </div>{/each}</div>
          <div class="field"><span>Selected local model</span><SelectMenu id="local-model" label="Selected local model" value={settings.local_model_id ?? ""} options={modelOptions} onchange={(value) => settings = { ...settings, local_model_id: value }} upwards /></div>
        </div>

        <div class="setting-group"><h3>Recording feedback</h3>
          <label class="toggle-field"><input type="checkbox" bind:checked={settings.sound_enabled} /><span>Play start/stop clicks and a chime when the text is ready</span></label>
          <button class="cue-test" disabled={cueTestPending} onclick={testCues}>{cueTestPending ? "Playing in 5 seconds…" : "Play the sounds in 5 seconds"}</button>
          <p class="privacy-note">Click, then switch to another window — that is how a hotkey recording plays them. What each sound did is shown here afterwards{#if logPath} and written to the log at <code>{logPath}</code>{/if}.</p>
          {#if cueTestMessage}<pre class="cue-report" role="status">{cueTestMessage}</pre>{/if}
        </div>
        {/if}

        {#if settingsTab === "prompts"}
        <div class="setting-group prompts-group"><div class="group-heading"><h3>Prompts</h3><button onclick={addCustomAction}>Add prompt</button></div>
          <p class="section-description">Every prompt Utterform ships with is a starting point. Rewrite any of them — Reset brings the original back.</p>
          <div class="prompt-workbench">
            <div class="prompt-list" role="group" aria-label="Prompts">
              {#each builtInActions as action}
                <button class="prompt-entry" class:active={selectedPrompt === action.id} aria-pressed={selectedPrompt === action.id} onclick={() => selectPrompt(action.id)}>
                  <span class="prompt-entry-name">{actionName(action, settings.action_overrides)}</span>
                  {#if !action.prompt && action.id === "plain"}<span class="prompt-tag">no prompt</span>
                  {:else if isActionEdited(action.id, settings.action_overrides)}<span class="prompt-dot" aria-label="Edited"></span>{/if}
                </button>
              {/each}
              {#if settings.custom_actions.length}<span class="prompt-divider">Your own</span>{/if}
              {#each settings.custom_actions as action}
                <button class="prompt-entry" class:active={selectedPrompt === `custom:${action.id}`} aria-pressed={selectedPrompt === `custom:${action.id}`} onclick={() => selectPrompt(`custom:${action.id}`)}>
                  <span class="prompt-entry-name">{action.name || "Untitled prompt"}</span>
                </button>
              {/each}
            </div>

            <div class="prompt-editor">
              {#if editedCustom}
                <label class="field"><span>Name</span><input aria-label="Prompt name" value={nameDraft} oninput={(event) => { nameDraft = event.currentTarget.value; updateCustomAction(editedCustom.id, "name", nameDraft); }} /></label>
                <label class="field"><span>Instructions <small>sent with every recording that uses it</small></span><textarea class="prompt-text" rows="7" aria-label="Prompt instructions" placeholder="Describe exactly how the transcript should be transformed…" value={promptDraft} oninput={(event) => { promptDraft = event.currentTarget.value; updateCustomAction(editedCustom.id, "prompt", promptDraft); }}></textarea></label>
                <div class="prompt-footer"><span>{promptDraft.trim().length} characters</span>
                  <button class="danger-text" onclick={() => removeCustomAction(editedCustom.id)}>Delete prompt</button>
                </div>
              {:else if editedPrompt && !editedPrompt.prompt}
                <div class="prompt-empty"><strong>{actionName(editedPrompt, settings.action_overrides)}</strong>
                  <p>Delivers what you said, word for word. It never reaches a text model, so there is no prompt to write — and no API cost when Local Whisper does the transcribing.</p>
                </div>
              {:else if editedPrompt}
                <label class="field"><span>Name</span><input aria-label="Prompt name" value={nameDraft} oninput={(event) => { nameDraft = event.currentTarget.value; overrideAction(editedPrompt, "name", nameDraft); }} /></label>
                <label class="field"><span>Instructions <small>sent with every recording that uses it</small></span><textarea class="prompt-text" rows="7" aria-label="Prompt instructions" value={promptDraft} oninput={(event) => { promptDraft = event.currentTarget.value; overrideAction(editedPrompt, "prompt", promptDraft); }}></textarea></label>
                <div class="prompt-footer">
                  <span>{promptDraft.trim().length} characters{isActionEdited(editedPrompt.id, settings.action_overrides) ? " · edited" : ""}</span>
                  <span class="prompt-footer-actions">
                    <button class="link-button" onclick={() => (showsDefault = showsDefault === editedPrompt.id ? "" : editedPrompt.id)}>{showsDefault === editedPrompt.id ? "Hide the original" : "View the original"}</button>
                    <button class="danger-text" disabled={!isActionEdited(editedPrompt.id, settings.action_overrides)} onclick={() => resetAction(editedPrompt.id)}>Reset</button>
                  </span>
                </div>
                {#if showsDefault === editedPrompt.id}<p class="prompt-default">{editedPrompt.prompt}</p>{/if}
              {/if}
            </div>
          </div>
        </div>

        <div class="setting-group"><h3>Text model</h3>
          <p class="section-description">The model that runs these prompts. Plain never reaches it.</p>
          <label class="field"><span>Model</span><input bind:value={settings.text_model} /></label>
          <div class="field"><span>Thinking effort <small>lower is faster and cheaper</small></span>
            <div class="segmented five">
              {#each EFFORTS as effort}
                <button class:active={settings.text_effort === effort.value} onclick={() => (settings = { ...settings, text_effort: effort.value })}>{effort.label}</button>
              {/each}
            </div>
          </div>
          <p class="privacy-note">Auto leaves the model its own default. Not every model offers every level; one that does not know the level you chose refuses the request, and the plain transcript is delivered instead.</p>
        </div>
        {/if}

        {#if settingsTab === "output"}
        <div class="setting-group"><h3>Dictation key</h3>
          {#if hotkeySupport.supported}
            <label class="toggle-field"><input type="checkbox" checked={settings.global_hotkey !== null} onchange={(event) => (settings = { ...settings, global_hotkey: event.currentTarget.checked ? settings.global_hotkey ?? hotkeySupport.default : null })} /><span>Start and finish a recording from anywhere, without raising the window</span></label>
            {#if settings.global_hotkey !== null}
              <label class="field"><span>Shortcut <small>modifiers first, for example {hotkeySupport.default}</small></span><input value={settings.global_hotkey} oninput={(event) => (settings = { ...settings, global_hotkey: event.currentTarget.value })} placeholder={hotkeySupport.default} /></label>
            {/if}
            {#if hotkeyMessage}<p class="setting-error" role="alert">{hotkeyMessage}</p>{/if}
            <p class="privacy-note">The key is reserved for Utterform while it runs. Press it once to start and again to finish; the sounds are the confirmation, since the window never comes forward.</p>
          {:else}
            <p class="privacy-note">{hotkeySupport.explanation}</p>
          {/if}
        </div>

        <div class="setting-group"><h3>Typing at the cursor</h3><div class="segmented">
          <button class:active={settings.typing_method === "paste"} onclick={() => (settings = { ...settings, typing_method: "paste" })}>Paste</button>
          <button class:active={settings.typing_method === "keystrokes"} onclick={() => (settings = { ...settings, typing_method: "keystrokes" })}>Keystrokes</button>
        </div>
          {#if settings.typing_method === "keystrokes"}
            <label class="field"><span>Delay between keystrokes <small>milliseconds</small></span><input type="number" min="0" max="500" step="1" value={settings.typing_delay_ms} oninput={(event) => (settings = { ...settings, typing_delay_ms: Math.max(0, Math.min(500, Math.round(Number(event.currentTarget.value) || 0))) })} /></label>
          {/if}
          <p class="privacy-note">{typingNote}</p>
        </div>

        <div class="setting-group"><h3>File output</h3><label class="field"><span>Default output folder</span><div class="inline-field"><input readonly value={settings.output_directory ?? ""} placeholder="Choose a folder" /><button onclick={chooseOutputFolder}>Browse</button></div></label></div>
        {/if}

        {#if settingsTab === "general"}
        <div class="setting-group"><h3>Appearance</h3><div class="segmented three">
          {#each ["system", "light", "dark"] as theme}
            <button class:active={settings.theme === theme} onclick={() => { settings = { ...settings, theme: theme as Theme }; applyTheme(settings.theme); }}>{theme[0].toUpperCase() + theme.slice(1)}</button>
          {/each}
        </div></div>

        <div class="setting-group"><h3>OpenAI</h3><label class="field"><span>API key <small>{hasApiKey ? "stored securely" : "not configured"}</small></span><div class="inline-field"><input type="password" autocomplete="off" bind:value={apiKeyInput} placeholder={hasApiKey ? "Enter a replacement key" : "Enter API key"} />{#if hasApiKey}<button class="danger-text" onclick={removeApiKey}>Remove</button>{/if}</div></label>
        <p class="privacy-note">The key is kept in the operating system keyring, never in the settings file.</p></div>

        <div class="setting-group"><h3>Recent texts</h3>
          <label class="toggle-field"><input type="checkbox" bind:checked={settings.history_enabled} /><span>Remember the last 100 texts on this device</span></label>
          <p class="privacy-note">Stored locally, unencrypted, including clipboard-only results. Titles are made from the text without an AI request. Turning this off keeps existing history until you clear it.</p>
          <button class="clear-history" onclick={clearHistory}>{confirmClear ? "Confirm: delete all saved texts" : "Clear saved history"}</button>
          {#if confirmClear}<button class="clear-history" onclick={() => confirmClear = false}>Keep history</button>{/if}
          {#if historyMessage}<p class="privacy-note" role="status">{historyMessage}</p>{/if}
        </div>
        {/if}
        </div>
      </div>

      <div class="modal-actions"><button class="secondary" onclick={cancelSettings}>Cancel</button><button class="primary" onclick={savePreferences}>Save settings</button></div>
    </div>
  </div>
{/if}
