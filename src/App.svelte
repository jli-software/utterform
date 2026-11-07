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
    RetainedRecording,
    AppSettings,
    AudioDevice,
    BuiltInAction,
    CloudModel,
    DownloadProgress,
    HistoryEntry,
    HotkeySupport,
    LocalModel,
    LiveStatus,
    LiveSupport,
    TypingSupport,
    ProcessResult,
    RemoteIntent,
    TextEffort,
    Theme,
  } from "./lib/types";
  import { api } from "./lib/api";
  import SelectMenu from "./lib/SelectMenu.svelte";
  import ShortcutRecorder from "./lib/ShortcutRecorder.svelte";
  import { isApplePlatform } from "./lib/shortcut";
  import { version } from "../package.json";
  import Brand from "./lib/Brand.svelte";
  import SignalField from "./lib/SignalField.svelte";
  import { formatHistoryTime, fullHistoryDate, historyTimestamp } from "./lib/history-time";
  import { modalFocus } from "./lib/modal-focus";
  import ChipInput from "./lib/ChipInput.svelte";
  import { SETTINGS_CATEGORIES, categoryLabel, visibleRows } from "./lib/settings-catalog";
  import type { SettingsCategory, SettingsRowId } from "./lib/settings-catalog";
  import { copiedSummary, createFailureReporter } from "./lib/diagnostics";

  type Phase = "idle" | "starting" | "recording" | "paused" | "processing" | "done" | "error";
  /// The one choice the main window offers for transcription. Not stored
  /// anywhere: it is read from `engine` and `cloud_model` and written back as
  /// exactly those two, so Settings and the main window cannot disagree.
  type TranscriptionMode = "gpt_transcribe" | "gpt_live_transcribe" | "local_whisper";
  type SupportAction = "copy" | "file" | "folder";
  /// A page one level below a row, for what does not belong in front: the
  /// local model list, the text model's id and effort, one action's prompt.
  type SettingsDetail = "" | "local-model" | "text-model" | "action";

  /// How long a text field waits after the last keystroke before it is written.
  const TYPING_SAVE_DELAY_MS = 500;
  const ENGINES: Array<{ mode: TranscriptionMode; label: string; title: string }> = [
    { mode: "gpt_transcribe", label: "Cloud", title: "GPT Transcribe: finished text, audio is sent to OpenAI" },
    { mode: "gpt_live_transcribe", label: "Live", title: "GPT Live Transcribe: text appears at the cursor while you speak" },
    { mode: "local_whisper", label: "Local", title: "Local Whisper: audio stays on this device" },
  ];
  /// Short codes such as de, en or pt-BR, the way the transcription API takes them.
  const LANGUAGE_CODE = /^[a-z]{2,3}(-[a-z0-9]{2,8})?$/i;
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
  let settingsCategory: SettingsCategory = "recording";
  let settingsQuery = "";
  let settingsDetail: SettingsDetail = "";
  // Every change is written at once; text fields wait for a pause in typing.
  // Writes are chained, so an older one can never land after a newer one.
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let saveWork: Promise<void> = Promise.resolve();
  let saveState: "" | "saved" | "failed" = "";
  let saveError = "";
  let saveStateReset: ReturnType<typeof setTimeout> | null = null;
  let editingApiKey = false;
  let apiKeyError = "";
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
  let hasApiKey = false;
  let apiKeyInput = "";
  // Until the backend answers, assume the session grants nothing: a field that
  // appears and then turns out to be dead is worse than one that arrives late.
  let hotkeySupport: HotkeySupport = { supported: false, default: "Ctrl+Alt+D", explanation: "", failure: null };
  let hotkeyError = "";
  /// While the recorder is listening, the key it is listening for must not be
  /// held by the operating system on Utterform's behalf, or pressing it would
  /// start a recording instead of being read. Released for that moment only,
  /// and put back from here — the recorder itself knows nothing about it.
  let hotkeyCapturing = false;
  let suspendedHotkey: string | null = null;
  let hotkeyCaptureWork: Promise<void> = Promise.resolve();
  // Autostart is the operating system's entry, not a setting of ours: what it
  // says is what the switch shows, and Settings asks it again every time.
  let autostartEnabled = false;
  let autostartBusy = false;
  let autostartChecking = false;
  let autostartAvailable = true;
  let autostartError = "";
  let elapsedSeconds = 0;
  let audioLevel = 0;
  let liveStatus: LiveStatus | null = null;
  let liveSupport: LiveSupport = { supported: false, explanation: "Live dictation requires the Windows or Omarchy desktop app." };
  let typingSupport: TypingSupport = { supported: true, explanation: "" };
  let liveStatusError = "";
  let liveFailurePending = false;
  let polling = false;
  let changingPause = false;
  let recordingRevision = 0;
  let now = Date.now();
  let resultTimestamp: number | null = null;
  let historyTimer: ReturnType<typeof setInterval> | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let result: ProcessResult | null = null;
  let retained: RetainedRecording | null = null;
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
  // Support actions are immediate, like model downloads: Save and Cancel
  // neither apply nor undo them, and each disables only itself while it runs.
  let supportBusy: Record<SupportAction, boolean> = { copy: false, file: false, folder: false };
  let supportMessage = "";
  let supportError = "";
  // Failures the backend never saw go to the local log from here; a command
  // that fails has already recorded itself natively.
  const failures = createFailureReporter(() => phase);
  let unlistenCueTest: UnlistenFn | undefined;
  const apple = isApplePlatform();
  const copyShortcut = apple ? "⌘⇧C" : "Ctrl+Shift+C";
  let message = "";
  let downloadProgress: Record<string, number> = {};
  let busyModel: string | null = null;
  let unlistenProgress: UnlistenFn | null = null;
  let unlistenLimit: UnlistenFn | null = null;
  let unlistenLiveFailed: UnlistenFn | null = null;
  let unlistenIntent: UnlistenFn | null = null;
  let destroyed = false;
  // The main-window transcription selector is locked while its write is out,
  // so a second choice cannot overtake the first; a recording that starts
  // meanwhile waits for the write, so it runs with what was confirmed.
  let transcriptionSaving = false;
  let transcriptionWork: Promise<void> = Promise.resolve();

  $: liveMode = settings.engine === "open_ai" && settings.cloud_model === "gpt_live_transcribe";
  $: transcriptionMode = transcriptionModeOf(settings);
  $: selectedModel = models.find((model) => model.id === settings.local_model_id) ?? null;
  // The model itself is chosen and downloaded in Settings; here it is only named,
  // or its absence is, so nobody records into a model that is not there.
  $: localModelHint = !settings.local_model_id
    ? "On device · Choose a model in Settings"
    : selectedModel && !selectedModel.downloaded
      ? `On device · Download ${selectedModel.name} in Settings`
      : `On device · ${selectedModel?.name ?? settings.local_model_id}`;
  $: transcriptionOptions = [
    { value: "gpt_transcribe", label: "GPT Transcribe", hint: "Finished text · Cloud" },
    { value: "gpt_live_transcribe", label: "GPT Live Transcribe", hint: "Live at cursor · Windows and Omarchy" },
    { value: "local_whisper", label: "Local Whisper", hint: localModelHint },
  ];
  $: effectiveAction = liveMode ? "plain" : selectedAction;
  $: liveStartHint = !liveSupport.supported ? liveSupport.explanation
    : hotkeySupport.supported && settings.global_hotkey && !hotkeyMessage
      ? `Place the cursor in your text field, then press ${settings.global_hotkey} to start and finish live dictation.`
      : "Place the cursor in your text field and use your system dictation shortcut to start and finish. Configure it in Settings → Recording.";
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
  $: settingsSearching = settingsQuery.trim() !== "";
  $: shownRows = visibleRows(settingsCategory, settingsQuery);
  $: downloadedModels = models.filter((model) => model.downloaded);
  $: localModelSummary = selectedModel
    ? `${selectedModel.name}${selectedModel.downloaded ? "" : " · not downloaded"} · ${downloadedModels.length} on device`
    : `None · ${downloadedModels.length} on device`;
  $: effortLabel = EFFORTS.find((effort) => effort.value === settings.text_effort)?.label ?? "Auto";
  $: historyOptions = history.map((entry) => ({ value: entry.id, label: entry.title, hint: formatHistoryTime(historyTimestamp(entry), now) }));
  $: displayedTimestamp = selectedHistory ? historyTimestamp(selectedHistory) : resultTimestamp;
  $: recordingActive = phase === "recording" || phase === "paused";
  $: deviceOptions = [{ value: "", label: "System default" }, ...devices.map((device) => ({ value: device.id, label: device.name, hint: device.isDefault ? "Default microphone" : undefined }))];
  $: modelOptions = models.map((model) => ({ value: model.id, label: model.name, hint: model.downloaded ? "Ready on this device" : "Download required" }));
  $: canRecord = liveMode || settings.copy_to_clipboard || settings.save_to_file || settings.type_at_cursor;
  // A key that could not be reserved at startup had nowhere to report; the
  // field it belongs to is where the user finds out. Derived rather than read
  // once, so a Settings dialog opened before the backend answered still shows it.
  $: hotkeyMessage = hotkeyError || hotkeySupport.failure || "";
  $: typingNote = settings.typing_method === "paste"
    ? "Pastes the text at once and leaves it on the clipboard."
    : "Types character by character. Increase the delay if letters go missing.";
  $: settingsEmpty = settingsSearching && shownRows.size === 0;
  $: todaySeconds = Math.round(history.filter((entry) => new Date(historyTimestamp(entry) ?? NaN).toDateString() === new Date(now).toDateString()).reduce((total, entry) => total + entry.durationMs, 0) / 1000);
  $: controlsLocked = phase === "starting" || recordingActive || phase === "processing";

  onMount(async () => {
    applyTheme(settings.theme);
    historyTimer = setInterval(() => now = Date.now(), 15_000);
    // Registered before the first await: a component torn down mid-setup must
    // not leave a keyboard handler bound to the window after it is gone.
    window.addEventListener("keydown", handleKeyDown);
    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("error", failures.onError);
    window.addEventListener("unhandledrejection", failures.onUnhandledRejection);
    if (!isTauri()) {
      message = "UI preview — launch the desktop app to record";
      return;
    }
    try {
      [settings, devices, models, hasApiKey, hotkeySupport, builtInActions, liveSupport, typingSupport] = await Promise.all([
        api.getSettings(),
        api.listInputDevices(),
        api.listLocalModels(),
        api.hasOpenAiApiKey(),
        api.globalHotkeySupport(),
        api.listBuiltInActions(),
        api.liveSupport().catch(() => ({ supported: false, explanation: "Live dictation support could not be checked. Restart Utterform to try again." })),
        api.typingSupport().catch(() => ({ supported: true, explanation: "" })),
      ]);
      settings = { ...DEFAULT_SETTINGS, ...settings, cloud_model: settings.cloud_model ?? "gpt_transcribe" };
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
    unlistenLiveFailed = await listen("live-failed", () => {
      if (liveMode && (phase === "starting" || recordingActive)) {
        liveFailurePending = true;
        if (recordingActive && !changingPause) void finishRecording();
      }
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
      unlistenLiveFailed?.();
      unlistenIntent?.();
      unlistenCueTest?.();
      return;
    }
    await refreshDiagnosticsInfo();
    await refreshRetained();
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
    unlistenLiveFailed?.();
    unlistenIntent?.();
    unlistenCueTest?.();
    document.removeEventListener("visibilitychange", handleVisibilityChange);
    window.removeEventListener("keydown", handleKeyDown);
    window.removeEventListener("error", failures.onError);
    window.removeEventListener("unhandledrejection", failures.onUnhandledRejection);
  });

  /// The path the backend writes to, shown for manual recovery. A query:
  /// failing it only leaves the path unknown.
  async function refreshDiagnosticsInfo() {
    try {
      logPath = (await api.diagnosticsInfo()).logPath;
    } catch {
      logPath = null;
    }
  }

  async function runSupportAction(action: SupportAction) {
    if (supportBusy[action]) return;
    supportBusy = { ...supportBusy, [action]: true };
    supportMessage = "";
    supportError = "";
    try {
      if (action === "copy") {
        const copied = await api.copyDiagnostics();
        supportMessage = copiedSummary(copied.bytes, copied.lines, copied.truncated);
      } else if (action === "file") {
        await api.openLogFile();
        supportMessage = "Log file opened";
      } else {
        await api.openLogFolder();
        supportMessage = "Log folder opened";
      }
    } catch (error) {
      // The backend's message already carries the reference of its event.
      supportError = error instanceof Error ? error.message : String(error);
    } finally {
      supportBusy = { ...supportBusy, [action]: false };
    }
  }

  /// Shows `text` at once, then adds the reference of the logged failure if
  /// the text is still the one showing when the backend answers.
  async function withReference(source: "autostart" | "state", reason: unknown, shown: () => string, show: (text: string) => void) {
    const text = shown();
    const reference = await failures.report(source, reason);
    if (reference && shown() === text) show(`${text} (ref ${reference})`);
  }

  function transcriptionModeOf(value: AppSettings): TranscriptionMode {
    if (value.engine === "local_whisper") return "local_whisper";
    return value.cloud_model === "gpt_live_transcribe" ? "gpt_live_transcribe" : "gpt_transcribe";
  }

  function transcriptionSettings(mode: string): AppSettings {
    return mode === "local_whisper"
      ? { ...settings, engine: "local_whisper" }
      : { ...settings, engine: "open_ai", cloud_model: mode as CloudModel };
  }

  /// Put a choice made in the main window into effect at once, through the
  /// same save Settings uses. Local Whisper keeps whatever cloud model was
  /// chosen before, so switching back returns to it. A write that fails puts
  /// the two fields this control owns back to the last value the backend
  /// confirmed — only those two, so an output toggled meanwhile stays.
  function chooseTranscription(mode: string) {
    if (transcriptionSaving || controlsLocked) return;
    const confirmed = { engine: settings.engine, cloud_model: settings.cloud_model };
    const next = transcriptionSettings(mode);
    if (transcriptionModeOf(next) === transcriptionMode) return;
    transcriptionSaving = true;
    settings = next;
    transcriptionWork = (async () => {
      try {
        await api.saveSettings(next);
        if (phase === "error") {
          phase = "idle";
          message = "";
        }
      } catch (error) {
        settings = { ...settings, ...confirmed };
        setError(`The transcription choice could not be saved: ${String(error)}`);
      } finally {
        transcriptionSaving = false;
      }
    })();
  }

  /// A compositor hotkey routes through here so it takes exactly the same path
  /// as the buttons, including every guard against double starts.
  async function applyIntent(intent: RemoteIntent | null) {
    // A key press that is being read into the dictation-key field is being
    // chosen, not used. Platforms that grant a global shortcut have already
    // released it for the moment; a compositor binding still arrives, and is
    // what this turns away.
    if (hotkeyCapturing) return;
    switch (intent) {
      case "toggle":
        if (recordingActive) await finishRecording();
        else if (phase !== "starting" && phase !== "processing") await startRecording(true);
        break;
      case "start":
        if (!recordingActive && phase !== "starting" && phase !== "processing") await startRecording(true);
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
      if (action && !liveMode) selectedAction = action.id;
      if (event.key.toLowerCase() === "c") {
        settings = { ...settings, copy_to_clipboard: !settings.copy_to_clipboard };
      }
      if (event.key.toLowerCase() === "f") {
        settings = { ...settings, save_to_file: !settings.save_to_file };
      }
      if (!liveMode && event.key.toLowerCase() === "t") {
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
      message = "";
      changingPause = false;
      if (status.limitReached) await finishRecording();
    } catch (error) {
      // A failed pause must not abandon a still-active native recording.
      message = `Could not change pause: ${String(error)}`;
    } finally {
      changingPause = false;
      if (liveFailurePending && recordingActive) void finishRecording();
    }
  }

  async function startRecording(fromTarget = false) {
    if (showSettings) return;
    if (liveMode && (!fromTarget || !liveSupport.supported)) {
      message = liveStartHint;
      return;
    }
    if (!canRecord) {
      setError("Select Clipboard, File, or both as an output.");
      return;
    }
    if (settings.save_to_file && !settings.output_directory) {
      setError("Choose a folder in Settings → Output before saving files.");
      return;
    }
    if (
      settings.engine === "local_whisper" &&
      !models.some((model) => model.id === settings.local_model_id && model.downloaded)
    ) {
      setError("Download and select a model in Settings → Transcription before recording.");
      return;
    }
    try {
      resetCopyFeedback();
      liveStatus = null;
      liveStatusError = "";
      liveFailurePending = false;
      phase = "starting";
      message = liveMode ? "Connecting GPT Live Transcribe…" : "Preparing the microphone…";
      // After `phase` is set, so a second press meanwhile is turned away; a
      // transcription choice still being written settles before this
      // recording is configured from it.
      await transcriptionWork;
      await api.saveSettings(settings);
      // Drain a preceding manual copy before a new session can deliver its result.
      await copyPending?.catch(() => {});
      await api.startRecording(
        settings.input_device,
        settings.engine,
        settings.local_model_id,
        effectiveAction.startsWith("custom:") ? "custom" : effectiveAction,
      );
      elapsedSeconds = 0;
      phase = "recording";
      message = "";
      // Only visual/status polling lives in JS. Native capture and its ten-minute
      // cutoff do not depend on focus or WebView timer scheduling.
      timer = setInterval(() => void pollRecording(), 100);
      // The worker can fail before start IPC resolves. Hidden WebViews skip
      // polling, so retain the native event through the starting phase.
      if (liveMode && liveFailurePending) await finishRecording();
      else if (liveMode) void pollRecording();
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
      const [status, snapshot] = await Promise.all([
        api.getRecordingStatus(),
        liveMode ? api.getLiveStatus().catch(() => {
          liveStatusError = "Live preview is temporarily unavailable. You can still stop the recording.";
          return undefined;
        }) : Promise.resolve(undefined),
      ]);
      if (!recordingActive || changingPause || revision !== recordingRevision) return;
      if (snapshot !== undefined) {
        liveStatus = snapshot;
        liveStatusError = "";
      }
      elapsedSeconds = status.elapsedSeconds;
      phase = status.paused ? "paused" : "recording";
      audioLevel = status.paused ? 0 : Math.max(0, Math.min(1, status.level));
      if (status.limitReached || liveStatus?.phase === "failed") await finishRecording();
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
    message = liveMode ? "Finishing live transcript…" : settings.engine === "open_ai" ? "Transcribing with GPT Transcribe…" : "Transcribing locally…";
    try {
      const finished = await api.finishRecording({
        action: effectiveAction.startsWith("custom:") ? "custom" : effectiveAction,
        customPrompt: effectiveAction.startsWith("custom:")
          ? settings.custom_actions.find((action) => `custom:${action.id}` === selectedAction)?.prompt ?? null
          : null,
        copyToClipboard: settings.copy_to_clipboard,
        saveToFile: settings.save_to_file,
        typeAtCursor: liveMode ? false : settings.type_at_cursor,
        outputFormat: settings.output_format,
      });
      if (liveMode) {
        try { liveStatus = await api.getLiveStatus() ?? liveStatus; } catch { /* Keep the previous snapshot. */ }
      }
      showResult(finished);
    } catch (error) {
      if (liveMode) await recoverLiveText();
      setError(error);
      await refreshRetained();
    }
  }

  function showResult(value: ProcessResult) {
    result = value;
    now = Date.now();
    resultTimestamp = now;
    if (value.historyEntry) {
      history = [value.historyEntry, ...history.filter((entry) => entry.id !== value.historyEntry?.id)].slice(0, 100);
      selectedHistoryId = value.historyEntry.id;
    } else {
      selectedHistoryId = "";
    }
    resetCopyFeedback();
    if (value.copiedToClipboard) showCopyFeedback();
    phase = "done";
    message = completionMessage(value);
    // A partly delivered result is as invisible from a hidden window as a failure.
    if (value.deliveryWarnings.length) notifyUnattended(message, false);
  }

  /// A query: failing it only leaves the offer unshown, never the audio gone.
  async function refreshRetained() {
    try {
      retained = await api.retainedRecording();
    } catch {
      retained = null;
    }
  }

  async function retryRetained() {
    if (controlsLocked) return;
    phase = "processing";
    message = "Trying again…";
    try {
      showResult(await api.retryRecording());
    } catch (error) {
      setError(error);
    }
    await refreshRetained();
  }

  async function discardRetained() {
    if (controlsLocked) return;
    try {
      await api.discardRecording();
      if (phase === "error") {
        phase = "idle";
        message = "";
      }
    } catch (error) {
      setError(error);
    }
    await refreshRetained();
  }

  async function cancelRecording() {
    if ((phase !== "recording" && phase !== "paused") || changingPause) return;
    stopTimer();
    phase = "starting";
    try {
      await api.cancelRecording();
      if (liveMode) await recoverLiveText();
      phase = "idle";
      elapsedSeconds = 0;
      message = liveMode ? "Live dictation stopped. Text already inserted is unchanged; the transcript remains available to copy." : "Recording discarded";
    } catch (error) {
      if (liveMode) await recoverLiveText();
      setError(error);
    }
  }

  async function recoverLiveText() {
    try { liveStatus = await api.getLiveStatus() ?? liveStatus; } catch { /* Keep the last visible snapshot. */ }
    if (!liveStatus?.text) return;
    result = {
      text: liveStatus.text, historyEntry: null, savedPath: null,
      durationMs: elapsedSeconds * 1000, engine: "open_ai",
      copiedToClipboard: false, typedAtCursor: false,
      deliveryWarnings: liveStatus.warning ? [liveStatus.warning] : [],
    };
    resultTimestamp = Date.now();
    selectedHistoryId = "";
    resultExpanded = true;
    resetCopyFeedback();
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
    // A command rejects with its message, a string that already carries the
    // reference of its native event. An `Error` was thrown in the interface.
    if (error instanceof Error) void withReference("state", error, () => message, (text) => (message = text));
    notifyUnattended(message, true);
  }

  /// A recording started by hotkey has nowhere else to report: the window that
  /// shows `message` may be hidden, and silence reads as success. The backend
  /// decides, because only it can tell whether anyone is looking. `failed`
  /// turns the tray icon amber; text that arrived with a warning stays green.
  function notifyUnattended(text: string, failed: boolean) {
    if (isTauri()) void api.notifyIfUnattended(text, failed).catch(() => {});
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

  /// Put a change into effect and write it. `delay` is for text fields: the
  /// write waits for a pause in typing, and the next change restarts the wait.
  function change(patch: Partial<AppSettings>, delay = 0) {
    settings = { ...settings, ...patch };
    scheduleSave(delay);
  }

  function scheduleSave(delay = 0) {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = null;
    if (delay > 0) saveTimer = setTimeout(() => void flushSave(), delay);
    else void flushSave();
  }

  /// Write what is showing now. Chained behind the previous write, so the
  /// backend always ends with the newest settings.
  function flushSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = null;
    const value = settings;
    saveWork = saveWork.then(async () => {
      try {
        await api.saveSettings(value);
        markSaved();
      } catch (error) {
        saveState = "failed";
        saveError = error instanceof Error ? error.message : String(error);
        if (saveStateReset) clearTimeout(saveStateReset);
        saveStateReset = null;
      }
    });
    return saveWork;
  }

  /// A short confirmation instead of a Save button: it shows that the change
  /// arrived, then gets out of the way.
  function markSaved() {
    saveState = "saved";
    saveError = "";
    if (saveStateReset) clearTimeout(saveStateReset);
    saveStateReset = setTimeout(() => { saveState = ""; saveStateReset = null; }, 1600);
  }

  /// Let go of the registered dictation key while a new one is being read,
  /// and take it back again afterwards. Serialized, because the end of a
  /// reading and the registration that follows it must not reach the backend
  /// out of order and leave the session without a key.
  function captureHotkey(capturing: boolean) {
    hotkeyCapturing = capturing;
    hotkeyCaptureWork = hotkeyCaptureWork.then(() => suspendHotkey(capturing));
  }

  async function suspendHotkey(capturing: boolean) {
    if (!hotkeySupport.supported) return;
    if (capturing) {
      // What the operating system is actually holding: nothing, when the
      // stored key could not be registered.
      const registered = hotkeySupport.failure ? null : settings.global_hotkey;
      if (suspendedHotkey !== null || !registered) return;
      suspendedHotkey = registered;
      try {
        await api.applyGlobalHotkey(null);
      } catch (error) {
        suspendedHotkey = null;
        hotkeyError = String(error);
      }
      return;
    }
    if (suspendedHotkey === null) return;
    const restore = suspendedHotkey;
    suspendedHotkey = null;
    try {
      await api.applyGlobalHotkey(restore);
    } catch (error) {
      // Whatever went wrong, the session must not be left without the key it
      // had; the failure stays beside the field until a key is registered.
      hotkeyError = String(error);
      hotkeySupport = { ...hotkeySupport, failure: hotkeyError };
    }
  }

  /// A new key, or none: stored at once, then registered behind any reading
  /// that is still giving the old key back.
  function chooseHotkey(shortcut: string | null) {
    change({ global_hotkey: shortcut });
    hotkeyCaptureWork = hotkeyCaptureWork.then(() => registerHotkey(shortcut));
  }

  async function registerHotkey(shortcut: string | null) {
    if (!hotkeySupport.supported) return;
    try {
      await api.applyGlobalHotkey(shortcut);
      hotkeyError = "";
      hotkeySupport = { ...hotkeySupport, failure: null };
    } catch (error) {
      // The key is stored; it only needs another attempt, and the field it
      // was entered in is where the reason waits.
      hotkeyError = String(error);
      hotkeySupport = { ...hotkeySupport, failure: hotkeyError };
    }
  }

  /// The operating system's answer, every time Settings opens: an entry
  /// removed or added elsewhere since must not be shown as anything else.
  async function refreshAutostart() {
    autostartChecking = true;
    autostartError = "";
    try {
      autostartEnabled = await api.autostartEnabled();
      autostartAvailable = true;
    } catch (error) {
      // A platform that will not answer costs the switch and nothing else:
      // recording, the other settings and the app itself go on working.
      autostartAvailable = false;
      autostartError = `Utterform could not read your startup settings: ${error}`;
      void withReference("autostart", error, () => autostartError, (text) => (autostartError = text));
    } finally {
      autostartChecking = false;
    }
  }

  /// Write the switch at once and believe the entry rather than the request:
  /// `isEnabled` is asked again afterwards.
  async function setAutostart(wanted: boolean) {
    if (!autostartAvailable || autostartBusy || wanted === autostartEnabled) return;
    autostartBusy = true;
    try {
      if (wanted) await api.enableAutostart();
      else await api.disableAutostart();
      autostartEnabled = await api.autostartEnabled();
      if (autostartEnabled !== wanted) {
        autostartError = wanted
          ? "Utterform could not be added to your startup items."
          : "Utterform could not be removed from your startup items.";
        void withReference("autostart", `the startup entry did not change after ${wanted ? "enable" : "disable"}`, () => autostartError, (text) => (autostartError = text));
        return;
      }
      autostartError = "";
      markSaved();
    } catch (error) {
      autostartError = String(error);
      void withReference("autostart", error, () => autostartError, (text) => (autostartError = text));
      try { autostartEnabled = await api.autostartEnabled(); } catch { /* Keep what is showing. */ }
    } finally {
      autostartBusy = false;
    }
  }

  /// The key goes to the system keyring, never into the settings file. Stored
  /// on Enter or when the field is left; an empty field changes nothing.
  async function storeApiKey() {
    const key = apiKeyInput.trim();
    if (!key) {
      editingApiKey = false;
      apiKeyError = "";
      return;
    }
    try {
      await api.setOpenAiApiKey(key);
      apiKeyInput = "";
      hasApiKey = true;
      editingApiKey = false;
      apiKeyError = "";
      markSaved();
    } catch (error) {
      apiKeyError = String(error);
    }
  }

  async function removeApiKey() {
    try {
      await api.deleteOpenAiApiKey();
      hasApiKey = false;
      apiKeyInput = "";
      editingApiKey = false;
      apiKeyError = "";
      markSaved();
    } catch (error) {
      apiKeyError = String(error);
    }
  }

  async function chooseOutputFolder() {
    let selected: string | string[] | null;
    try {
      selected = await open({ directory: true, multiple: false, title: "Choose output folder" });
    } catch (error) {
      // The dialog plugin answers the interface alone; no command saw this.
      void failures.report("dialog", error);
      return;
    }
    if (typeof selected === "string") change({ output_directory: selected });
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
      // A first model on this device is the one to use.
      const chosen = models.find((item) => item.id === settings.local_model_id);
      if (!model.downloaded && !chosen?.downloaded) change({ local_model_id: model.id });
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

  function chooseTheme(theme: Theme) {
    change({ theme });
    applyTheme(theme);
  }

  function chooseEngine(mode: TranscriptionMode) {
    if (mode === transcriptionMode) return;
    settings = transcriptionSettings(mode);
    scheduleSave();
  }

  function openSettings() {
    confirmClear = false;
    settingsQuery = "";
    settingsDetail = "";
    saveState = "";
    saveError = "";
    selectPrompt(promptExists(selectedPrompt) ? selectedPrompt : firstEditablePrompt());
    supportMessage = "";
    supportError = "";
    apiKeyInput = "";
    apiKeyError = "";
    editingApiKey = false;
    showSettings = true;
    if (isTauri()) {
      void refreshAutostart();
      void refreshDiagnosticsInfo();
      void refreshInputState();
    }
  }

  /// Nothing is left to confirm: a text field still waiting for its pause in
  /// typing is written now, and the window goes back to recording.
  async function closeSettings() {
    if (hotkeyCapturing) return;
    showSettings = false;
    settingsQuery = "";
    settingsDetail = "";
    if (apiKeyInput.trim()) await storeApiKey();
    if (saveTimer) await flushSave();
  }

  /// Microphones come and go while Utterform stays open, and a key that could
  /// not be read at startup may be readable now. Neither answer is worth
  /// interrupting anything for; the previous one simply stands.
  async function refreshInputState() {
    try {
      [devices, hasApiKey] = await Promise.all([api.listInputDevices(), api.hasOpenAiApiKey()]);
    } catch {
      /* Keep what is showing. */
    }
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

  function selectSettingsCategory(category: SettingsCategory) {
    settingsCategory = category;
    settingsQuery = "";
    settingsDetail = "";
    const panel = document.getElementById("settings-panel");
    if (panel) panel.scrollTop = 0;
  }

  function moveSettingsCategory(event: KeyboardEvent, index: number) {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const next = SETTINGS_CATEGORIES[(index + step + SETTINGS_CATEGORIES.length) % SETTINGS_CATEGORIES.length];
    selectSettingsCategory(next.id);
    // Follow the selection with focus, as a tablist is expected to.
    (document.getElementById(`settings-tab-${next.id}`) as HTMLElement | null)?.focus();
  }

  function openDetail(detail: SettingsDetail, prompt?: string) {
    if (prompt) selectPrompt(prompt);
    // The detail belongs to its category; leaving a search for it keeps the
    // way back where the row lives.
    const owner: SettingsRowId = detail === "local-model" ? "local-model" : detail === "text-model" ? "text-model" : "actions";
    settingsCategory = owner === "local-model" ? "transcription" : "ai";
    settingsQuery = "";
    settingsDetail = detail;
  }

  function vocabularyProblem(term: string) {
    return FORBIDDEN_IN_KEYWORD.test(term) ? "< and > are not allowed" : "";
  }

  function languageProblem(code: string) {
    return LANGUAGE_CODE.test(code) ? "" : "Use a code like de or en";
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
    change({ action_overrides: overrides }, TYPING_SAVE_DELAY_MS);
  }

  function resetAction(id: string) {
    const overrides = { ...settings.action_overrides };
    delete overrides[id];
    change({ action_overrides: overrides });
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

  function addCustomAction() {
    const id = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
    change({ custom_actions: [...settings.custom_actions, { id, name: "Custom action", prompt: "" }] });
    openDetail("action", `custom:${id}`);
  }

  function updateCustomAction(id: string, field: "name" | "prompt", value: string) {
    change({
      custom_actions: settings.custom_actions.map((action) =>
        action.id === id ? { ...action, [field]: value } : action,
      ),
    }, TYPING_SAVE_DELAY_MS);
  }

  function removeCustomAction(id: string) {
    change({ custom_actions: settings.custom_actions.filter((action) => action.id !== id) });
    if (selectedAction === `custom:${id}`) selectedAction = "plain";
    if (selectedPrompt === `custom:${id}`) selectPrompt(firstEditablePrompt());
    settingsDetail = "";
  }
</script>

<svelte:head><meta name="theme-color" content={settings.theme === "dark" ? "#121516" : "#fcfcfa"} /></svelte:head>

<main inert={showSettings} class:result-expanded={resultExpanded} class:has-result={!!displayedText} class:paused={phase === "paused"} class:recording={phase === "recording"} class:processing={phase === "processing"}>
  <header>
    <div class="brand">
      <Brand />
      <strong>utterform</strong>
    </div>
    <div class="header-tools"><span class="app-version">{version}</span>
    <button class="icon-button settings-trigger" aria-label="Open settings" title="Settings" disabled={controlsLocked || transcriptionSaving} onclick={openSettings}>
      <svg class="settings-glyph" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h5m6 0h5M4 17h9m6 0h1"/><circle cx="12" cy="7" r="3"/><circle cx="16" cy="17" r="3"/></svg>
    </button></div>
  </header>

  <section class="controls" aria-label="Transcription settings">
    <div class="action-control">
      <span class="control-label">Action</span>
      {#if liveMode}<div class="engine-chip">Plain</div>
      {:else}<SelectMenu id="action" label="Action" bind:value={selectedAction} options={actionOptions} disabled={controlsLocked} />{/if}
    </div>
    <div class="engine-control" title={settings.engine === "open_ai" ? "Audio is sent to OpenAI" : "Audio stays on this device"}><span class="control-label">Transcription</span>
      <SelectMenu id="transcription" label="Transcription" value={transcriptionMode} options={transcriptionOptions} showSelectedHint={settings.engine === "local_whisper" && (!settings.local_model_id || !!selectedModel && !selectedModel.downloaded)} disabled={controlsLocked || transcriptionSaving} onchange={chooseTranscription} />
    </div>
  </section>

  <section class="recorder" aria-label="Recording">
    <div class="record-status" role="status"><span class="record-dot" class:visible={recordingActive}></span>{phase === "recording" ? "Recording" : phase === "paused" ? "Paused" : phase === "processing" ? "Processing" : phase === "starting" ? "Preparing" : phase === "error" ? "Attention" : "Ready"}</div>
    <div class="timer">{formatTime(elapsedSeconds)}</div>
    <SignalField phase={phase} level={audioLevel} />
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
      <span>{phase === "starting" ? "Preparing" : phase === "processing" ? "Processing" : recordingActive ? "Finish" : "Record"}</span>
      {#if !liveMode || recordingActive}<kbd>Space</kbd>{/if}
    </button>
    {#if recordingActive}
      <button class="pause-button" aria-label={phase === "paused" ? "Resume recording" : "Pause recording"} aria-keyshortcuts="P" disabled={changingPause} onclick={togglePause}>
        <svg viewBox="0 0 20 20" aria-hidden="true">{#if phase === "paused"}<path d="m7 4 9 6-9 6Z"/>{:else}<path d="M5 4h3v12H5zM12 4h3v12h-3z"/>{/if}</svg>
        {phase === "paused" ? "Resume" : "Pause"}<kbd>P</kbd>
      </button>
    {/if}
    </div>
    {#if liveMode && !controlsLocked}<p class="live-help">{liveStartHint}</p>{/if}
    <p class="recorder-message" role="status" class:error={phase === "error"}>{message}</p>
    {#if retained && !controlsLocked}
      <div class="retained-recording" role="group" aria-label="Kept recording">
        <span>{retained.count > 1 ? `${retained.count} recordings kept` : `Recording kept · ${formatTime(Math.round(retained.durationMs / 1000))}`}</span>
        <button type="button" onclick={retryRetained}>Try again</button>
        <button type="button" class="quiet" onclick={discardRetained}>Discard</button>
      </div>
    {/if}
  </section>

  {#if liveMode && controlsLocked}
    <section class="live-preview" aria-label="Live dictation">
      <strong>Live transcript</strong>
      <p role="status" class:error={liveStatus?.deliveryPaused || liveStatus?.phase === "failed" || !!liveStatusError}>
        {liveStatusError || liveStatus?.warning || (liveStatus?.deliveryPaused ? "Insertion paused. Finish, then copy the transcript; insertion will not resume automatically." : phase === "processing" ? "Finishing transcript…" : "")}
      </p>
      <div class="transcript" role="region" aria-label="Live transcript">{liveStatus?.text || "Waiting for speech…"}</div>
      {#if liveStatus?.deliveryPaused}<small>Some text may already be inserted. Copying the full transcript can duplicate it.</small>{/if}
    </section>
  {/if}

  <section class="output-bar" aria-label="Output selection">
    <span class="output-label">Send to</span>
    <button class:enabled={settings.copy_to_clipboard} aria-pressed={settings.copy_to_clipboard} onclick={() => (settings = { ...settings, copy_to_clipboard: !settings.copy_to_clipboard })} disabled={controlsLocked} title="Copy to clipboard">
      <span class="output-check" aria-hidden="true">✓</span>
      <span class="output-name">Clipboard</span> <kbd>C</kbd>
    </button>
    <button class:enabled={settings.save_to_file} aria-pressed={settings.save_to_file} onclick={() => (settings = { ...settings, save_to_file: !settings.save_to_file })} disabled={controlsLocked} title="Save to file">
      <span class="output-check" aria-hidden="true">✓</span>
      <span class="output-name">File</span> <kbd>F</kbd>
    </button>
    <button class:enabled={liveMode || settings.type_at_cursor} aria-pressed={liveMode || settings.type_at_cursor} onclick={() => (settings = { ...settings, type_at_cursor: !settings.type_at_cursor })} disabled={controlsLocked || liveMode} title={liveMode ? "Live text is inserted as you speak; it is never typed again on finish" : "Type the finished text into whatever window has focus"}>
      <span class="output-check" aria-hidden="true">✓</span>
      <span class="output-name">{liveMode ? "Live" : "Type"}</span> {#if !liveMode}<kbd>T</kbd>{/if}
    </button>
    <SelectMenu id="format" label="File format" value={settings.output_format}
      onchange={(value) => settings = { ...settings, output_format: value as AppSettings["output_format"] }}
      options={[{ value: "txt", label: "TXT", hint: "Plain text" }, { value: "md", label: "Markdown", hint: "Formatted text" }]}
      disabled={!settings.save_to_file || controlsLocked} compact upwards />
  </section>

  {#if displayedText}
    <section class="result-card" aria-label="Saved text">
      {#if liveStatus?.text && displayedText === liveStatus.text && liveStatus.insertedText}<p class="live-recovery-note">Copy includes the full transcript, including text already inserted. Check your target field before pasting.</p>{/if}
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

  <footer><span>{recordingActive ? "P " + (phase === "paused" ? "resumes" : "pauses") + (liveMode ? " · Esc stops" : " · Esc discards") : liveMode ? "Live dictation · Plain only" : "1–6 select an action"}</span><span title="Audio duration in today’s retained history entries (up to 100 texts)">{formatTime(todaySeconds)} saved audio today</span></footer>
</main>

{#snippet rowHead(id: SettingsRowId, label: string, forId: string | null = null)}
  <div class="row-label">
    {#if settingsSearching}<small class="row-where">{categoryLabel(id)}</small>{/if}
    {#if forId}<label for={forId}>{label}</label>{:else}<span>{label}</span>{/if}
  </div>
{/snippet}

{#if showSettings}
  <div class="settings-page" use:modalFocus role="dialog" tabindex="-1" aria-modal="true" aria-label="Settings"
    onkeydown={(event) => {
      if (event.key !== "Escape" || hotkeyCapturing) return;
      event.preventDefault();
      event.stopPropagation();
      if (settingsDetail) settingsDetail = "";
      else if (settingsSearching) settingsQuery = "";
      else void closeSettings();
    }}>
    <div class="settings-top">
      <button class="icon-button" aria-label="Close settings" title="Back" onclick={closeSettings}><svg class="line-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M19 12H5m6-6-6 6 6 6"/></svg></button>
      <label class="settings-search">
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></svg>
        <input type="search" data-autofocus aria-label="Search settings" placeholder="Search settings" bind:value={settingsQuery} oninput={() => (settingsDetail = "")} />
      </label>
      <span class="save-state" class:failed={saveState === "failed"} role="status" title={saveError}>{saveState === "saved" ? "✓ Saved" : saveState === "failed" ? "Not saved" : ""}</span>
    </div>
    {#if saveState === "failed"}<p class="setting-error save-error" role="alert">Not saved: {saveError}</p>{/if}

    <div class="settings-tabs" role="tablist" aria-label="Settings categories">
      {#each SETTINGS_CATEGORIES as category, index}
        <button id={`settings-tab-${category.id}`} role="tab" class:active={!settingsSearching && settingsCategory === category.id}
          aria-selected={!settingsSearching && settingsCategory === category.id} aria-controls="settings-panel"
          tabindex={settingsCategory === category.id ? 0 : -1}
          onclick={() => selectSettingsCategory(category.id)} onkeydown={(event) => moveSettingsCategory(event, index)}>{category.label}</button>
      {/each}
    </div>

    <div class="settings-panel" id="settings-panel" role="tabpanel" aria-labelledby={`settings-tab-${settingsCategory}`}>
    {#if settingsDetail === "local-model"}
      <button class="detail-back" onclick={() => (settingsDetail = "")}>‹ Transcription</button>
      <h2 class="detail-title">Local model</h2>
      <div class="model-list" role="radiogroup" aria-label="Local model">
        {#each models as model}
          <div class="model-row" class:selected={settings.local_model_id === model.id} title={model.description}>
            <label class="model-choice">
              <input type="radio" name="local-model" value={model.id} checked={settings.local_model_id === model.id} disabled={!model.downloaded} onchange={() => change({ local_model_id: model.id })} />
              <span><strong>{model.name}</strong><small>{model.downloaded ? "On device" : formatBytes(model.sizeBytes)}</small></span>
            </label>
            {#if busyModel === model.id && !model.downloaded}<progress aria-label={`Downloading ${model.name}`} max="1" value={downloadProgress[model.id] ?? 0}></progress>{/if}
            <button class="row-button" class:quiet={model.downloaded} disabled={busyModel !== null} aria-label={`${model.downloaded ? "Remove" : "Download"} ${model.name}`} onclick={() => toggleModel(model)}>{busyModel === model.id ? "…" : model.downloaded ? "Remove" : "Download"}</button>
          </div>
        {:else}
          <p class="row-note">No models available.</p>
        {/each}
      </div>
    {:else if settingsDetail === "text-model"}
      <button class="detail-back" onclick={() => (settingsDetail = "")}>‹ AI</button>
      <h2 class="detail-title">Text model</h2>
      <div class="setting-row"><div class="row-label"><label for="text-model">Model</label></div>
        <input id="text-model" class="row-input" value={settings.text_model} oninput={(event) => change({ text_model: event.currentTarget.value }, TYPING_SAVE_DELAY_MS)} /></div>
      <div class="setting-row" title="Lower is faster and cheaper. Auto uses the model's default."><div class="row-label"><span>Effort</span></div>
        <div class="segmented" role="group" aria-label="Thinking effort">
          {#each EFFORTS as effort}
            <button class:active={settings.text_effort === effort.value} aria-pressed={settings.text_effort === effort.value} onclick={() => change({ text_effort: effort.value })}>{effort.label}</button>
          {/each}
        </div></div>
    {:else if settingsDetail === "action"}
      <button class="detail-back" onclick={() => (settingsDetail = "")}>‹ AI</button>
      {#if editedCustom}
        <label class="field"><span>Name</span><input aria-label="Prompt name" value={nameDraft} oninput={(event) => { nameDraft = event.currentTarget.value; updateCustomAction(editedCustom.id, "name", nameDraft); }} /></label>
        <label class="field"><span>Instructions</span><textarea class="prompt-text" rows="9" aria-label="Prompt instructions" placeholder="How the transcript should be rewritten" value={promptDraft} oninput={(event) => { promptDraft = event.currentTarget.value; updateCustomAction(editedCustom.id, "prompt", promptDraft); }}></textarea></label>
        <div class="prompt-footer"><span></span><button class="danger-text" onclick={() => removeCustomAction(editedCustom.id)}>Delete</button></div>
      {:else if editedPrompt && !editedPrompt.prompt}
        <h2 class="detail-title">{actionName(editedPrompt, settings.action_overrides)}</h2>
        <p class="row-note">Transcription only, no prompt.</p>
      {:else if editedPrompt}
        <label class="field"><span>Name</span><input aria-label="Prompt name" value={nameDraft} oninput={(event) => { nameDraft = event.currentTarget.value; overrideAction(editedPrompt, "name", nameDraft); }} /></label>
        <label class="field"><span>Instructions</span><textarea class="prompt-text" rows="9" aria-label="Prompt instructions" value={promptDraft} oninput={(event) => { promptDraft = event.currentTarget.value; overrideAction(editedPrompt, "prompt", promptDraft); }}></textarea></label>
        <div class="prompt-footer">
          <span>{isActionEdited(editedPrompt.id, settings.action_overrides) ? "Edited" : ""}</span>
          <span class="prompt-footer-actions">
            <button class="link-button" onclick={() => (showsDefault = showsDefault === editedPrompt.id ? "" : editedPrompt.id)}>{showsDefault === editedPrompt.id ? "Hide the original" : "View the original"}</button>
            <button class="danger-text" disabled={!isActionEdited(editedPrompt.id, settings.action_overrides)} onclick={() => resetAction(editedPrompt.id)}>Reset</button>
          </span>
        </div>
        {#if showsDefault === editedPrompt.id}<p class="prompt-default">{editedPrompt.prompt}</p>{/if}
      {/if}
    {:else}
      <!-- Recording -->
      {#if shownRows.has("microphone")}
        <div class="setting-row">{@render rowHead("microphone", "Microphone")}
          <div class="row-control wide"><SelectMenu id="microphone" label="Microphone" value={settings.input_device ?? ""} options={deviceOptions} onchange={(value) => change({ input_device: value || null })} /></div></div>
      {/if}
      {#if shownRows.has("shortcut")}
        <div class="setting-row" title="Press once to record, again to finish. Works in other apps.">{@render rowHead("shortcut", "Shortcut")}
          <div class="row-control">
            {#if hotkeySupport.supported}
              {#if settings.global_hotkey !== null}
                <ShortcutRecorder id="dictation-shortcut" label="Shortcut" {apple}
                  value={settings.global_hotkey || hotkeySupport.default}
                  onchange={(shortcut) => chooseHotkey(shortcut)} oncapture={captureHotkey} />
              {/if}
              <input type="checkbox" role="switch" class="switch" aria-label="Global dictation shortcut" checked={settings.global_hotkey !== null}
                onchange={(event) => chooseHotkey(event.currentTarget.checked ? settings.global_hotkey ?? hotkeySupport.default : null)} />
            {:else}
              <span class="row-note">{hotkeySupport.explanation}</span>
            {/if}
          </div>
          {#if hotkeyMessage}<p class="setting-error row-message" role="alert">{hotkeyMessage}</p>{/if}
        </div>
      {/if}
      {#if shownRows.has("sounds")}
        <div class="setting-row" title="Start and stop clicks, a chime when text is ready">{@render rowHead("sounds", "Sounds")}
          <div class="row-control">
            <button class="row-button" disabled={cueTestPending} title="Plays the sounds after 5 seconds, so you can switch to another window" onclick={testCues}>{cueTestPending ? "In 5 s…" : "Test"}</button>
            <input type="checkbox" role="switch" class="switch" aria-label="Recording sounds" checked={settings.sound_enabled} onchange={(event) => change({ sound_enabled: event.currentTarget.checked })} />
          </div>
          {#if cueTestMessage}<pre class="cue-report row-message" role="status">{cueTestMessage}</pre>{/if}
        </div>
      {/if}

      <!-- Transcription -->
      {#if shownRows.has("engine")}
        <div class="setting-row">{@render rowHead("engine", "Engine")}
          <div class="segmented" role="group" aria-label="Transcription engine">
            {#each ENGINES as engine}
              <button class:active={transcriptionMode === engine.mode} aria-pressed={transcriptionMode === engine.mode} title={engine.title} onclick={() => chooseEngine(engine.mode)}>{engine.label}</button>
            {/each}
          </div>
          {#if liveMode && !liveSupport.supported}<p class="setting-error row-message" role="status">{liveSupport.explanation}</p>{/if}
        </div>
      {/if}
      {#if shownRows.has("api-key")}
        <div class="setting-row" title="Stored in your system keyring. Used for cloud transcription and text actions.">{@render rowHead("api-key", "OpenAI key", hasApiKey && !editingApiKey ? null : "api-key")}
          <div class="row-control">
            {#if hasApiKey && !editingApiKey}
              <span class="key-state">● Stored</span>
              <button class="row-button" onclick={() => { editingApiKey = true; queueMicrotask(() => document.getElementById("api-key")?.focus()); }}>Change</button>
              <button class="row-button danger-text" onclick={removeApiKey}>Remove</button>
            {:else}
              <input id="api-key" class="row-input" type="password" autocomplete="off" placeholder="sk-…" bind:value={apiKeyInput}
                onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); void storeApiKey(); } }} onblur={() => void storeApiKey()} />
            {/if}
          </div>
          {#if apiKeyError}<p class="setting-error row-message" role="alert">{apiKeyError}</p>{/if}
        </div>
      {/if}
      {#if shownRows.has("language")}
        <div class="setting-row" title="Optional. Codes such as de or en help the transcription.">{@render rowHead("language", "Language", "language")}
          <div class="row-control wide"><ChipInput id="language" label="Language" values={settings.language_hints} placeholder="Add"
            normalize={(value) => value.trim().toLowerCase()} validate={languageProblem} flagged={(value) => !!languageProblem(value)}
            onchange={(values) => change({ language_hints: values })} /></div></div>
      {/if}
      {#if shownRows.has("vocabulary")}
        <div class="setting-row" title="Names and terms to recognise. Works with both engines.">{@render rowHead("vocabulary", "Vocabulary", "vocabulary")}
          <div class="row-control wide"><ChipInput id="vocabulary" label="Vocabulary" values={settings.vocabulary} placeholder="Add word"
            validate={vocabularyProblem} flagged={(term) => !!vocabularyProblem(term)}
            onchange={(values) => change({ vocabulary: values })} /></div></div>
      {/if}
      {#if shownRows.has("context")}
        <div class="setting-row" title="Optional, cloud only. What the recordings are about.">{@render rowHead("context", "Context", "transcription-context")}
          <input id="transcription-context" class="row-input" placeholder="Optional" value={settings.transcription_context}
            oninput={(event) => change({ transcription_context: event.currentTarget.value }, TYPING_SAVE_DELAY_MS)} /></div>
      {/if}
      {#if shownRows.has("local-model")}
        <button class="setting-row more-row" onclick={() => openDetail("local-model")}>{@render rowHead("local-model", "Local model")}
          <span class="row-more">{localModelSummary}</span></button>
      {/if}

      <!-- AI -->
      {#if shownRows.has("actions")}
        <div class="setting-row actions-row">{@render rowHead("actions", "Actions")}
          <div class="action-chips" role="group" aria-label="Actions">
            {#each builtInActions as action}
              <button class="action-chip" onclick={() => openDetail("action", action.id)}>{actionName(action, settings.action_overrides)}{#if isActionEdited(action.id, settings.action_overrides)}<span class="edited-dot" title="Edited" aria-hidden="true"></span>{/if}</button>
            {/each}
            {#each settings.custom_actions as action}
              <button class="action-chip own" onclick={() => openDetail("action", `custom:${action.id}`)}>{action.name || "Untitled"}</button>
            {/each}
            <button class="action-chip add" aria-label="Add action" onclick={addCustomAction}>+</button>
          </div>
          {#if liveMode}<p class="row-note row-message">Live uses Plain only.</p>{/if}
        </div>
      {/if}
      {#if shownRows.has("text-model")}
        <button class="setting-row more-row" title="Used for actions other than Plain" onclick={() => openDetail("text-model")}>{@render rowHead("text-model", "Text model")}
          <span class="row-more">{settings.text_model} · {effortLabel}</span></button>
      {/if}

      <!-- Output -->
      {#if shownRows.has("typing")}
        <div class="setting-row" title={typingNote}>{@render rowHead("typing", "Type at cursor")}
          <div class="segmented" role="group" aria-label="Typing method">
            <button class:active={settings.typing_method === "paste"} aria-pressed={settings.typing_method === "paste"} onclick={() => change({ typing_method: "paste" })}>Paste</button>
            <button class:active={settings.typing_method === "keystrokes"} aria-pressed={settings.typing_method === "keystrokes"} onclick={() => change({ typing_method: "keystrokes" })}>Keystrokes</button>
          </div>
          {#if typingSupport.explanation}<p class="setting-error row-message" role="status">{typingSupport.explanation}</p>{/if}
        </div>
      {/if}
      {#if shownRows.has("typing-delay") && settings.typing_method === "keystrokes"}
        <div class="setting-row" title="Milliseconds between keystrokes. Increase it if letters go missing.">{@render rowHead("typing-delay", "Keystroke delay", "typing-delay")}
          <div class="row-control"><input id="typing-delay" class="row-input narrow" type="number" min="0" max="500" step="1" value={settings.typing_delay_ms}
            oninput={(event) => change({ typing_delay_ms: Math.max(0, Math.min(500, Math.round(Number(event.currentTarget.value) || 0))) }, TYPING_SAVE_DELAY_MS)} /><span class="row-note">ms</span></div></div>
      {/if}
      {#if shownRows.has("folder")}
        <div class="setting-row">{@render rowHead("folder", "Folder")}
          <div class="row-control"><span class="row-path" title={settings.output_directory ?? ""}>{settings.output_directory ?? "None"}</span><button class="row-button" onclick={chooseOutputFolder}>Choose</button></div></div>
      {/if}
      {#if shownRows.has("history")}
        <div class="setting-row" title="The last 100 texts, stored on this device, unencrypted.">{@render rowHead("history", "History")}
          <div class="row-control">
            {#if confirmClear}<button class="row-button" onclick={() => (confirmClear = false)}>Keep</button>{/if}
            <button class="row-button danger-text" onclick={clearHistory}>{confirmClear ? "Delete all" : "Clear"}</button>
            <input type="checkbox" role="switch" class="switch" aria-label="Remember recent texts" checked={settings.history_enabled} onchange={(event) => change({ history_enabled: event.currentTarget.checked })} />
          </div>
          {#if historyMessage}<p class="row-note row-message" role="status">{historyMessage}</p>{/if}
        </div>
      {/if}

      <!-- App -->
      {#if shownRows.has("theme")}
        <div class="setting-row">{@render rowHead("theme", "Theme")}
          <div class="segmented" role="group" aria-label="Theme">
            {#each ["system", "light", "dark"] as theme}
              <button class:active={settings.theme === theme} aria-pressed={settings.theme === theme} onclick={() => chooseTheme(theme as Theme)}>{theme[0].toUpperCase() + theme.slice(1)}</button>
            {/each}
          </div></div>
      {/if}
      {#if shownRows.has("autostart")}
        <div class="setting-row" title="Starts hidden in the system tray">{@render rowHead("autostart", "Start at login")}
          <div class="row-control">{#if autostartChecking}<span class="row-note">checking…</span>{/if}
            <input type="checkbox" role="switch" class="switch" aria-label="Start Utterform when I sign in" checked={autostartEnabled} disabled={autostartChecking || autostartBusy || !autostartAvailable} onchange={async (event) => {
              // The switch shows the entry, also when a write did not move it.
              const box = event.currentTarget;
              await setAutostart(box.checked);
              box.checked = autostartEnabled;
            }} /></div>
          {#if autostartError}<p class="setting-error row-message" role="alert">{autostartError}</p>{/if}
        </div>
      {/if}
      {#if shownRows.has("diagnostics")}
        <div class="setting-row" title={`Logs stay on this device and contain no transcripts or audio. ${logPath ?? ""}`}>{@render rowHead("diagnostics", "Diagnostics")}
          <div class="row-control">
            <button class="row-button" aria-label="Copy diagnostics" aria-busy={supportBusy.copy} disabled={supportBusy.copy} onclick={() => runSupportAction("copy")}>Copy</button>
            <button class="row-button" aria-label="Open log file" aria-busy={supportBusy.file} disabled={supportBusy.file} onclick={() => runSupportAction("file")}>Log</button>
            <button class="row-button" aria-label="Open log folder" aria-busy={supportBusy.folder} disabled={supportBusy.folder} onclick={() => runSupportAction("folder")}>Folder</button>
          </div>
          {#if supportMessage}<p class="row-note row-message" role="status">{supportMessage}</p>{/if}
          {#if supportError}<p class="setting-error row-message" role="alert">{supportError}</p>{/if}
        </div>
      {/if}

      {#if settingsEmpty}<p class="row-note settings-empty">Nothing found.</p>{/if}
    {/if}
    </div>
    <p class="settings-version">Utterform {version}</p>
  </div>
{/if}
