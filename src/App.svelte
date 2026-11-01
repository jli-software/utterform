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
  import Icon from "./lib/Icon.svelte";
  import SelectMenu from "./lib/SelectMenu.svelte";
  import ShortcutRecorder from "./lib/ShortcutRecorder.svelte";
  import { isApplePlatform } from "./lib/shortcut";
  import { version } from "../package.json";
  import Brand from "./lib/Brand.svelte";
  import SignalField from "./lib/SignalField.svelte";
  import { formatHistoryTime, fullHistoryDate, historyTimestamp } from "./lib/history-time";
  import { modalFocus } from "./lib/modal-focus";
  import { copiedSummary, createFailureReporter } from "./lib/diagnostics";

  type Phase = "idle" | "starting" | "recording" | "paused" | "processing" | "done" | "error";
  /// The one choice the main window offers for transcription. Not stored
  /// anywhere: it is read from `engine` and `cloud_model` and written back as
  /// exactly those two, so Settings and the main window cannot disagree.
  type TranscriptionMode = "gpt_transcribe" | "gpt_live_transcribe" | "local_whisper";
  type SettingsTab = "recording" | "transcription" | "actions" | "output" | "app";
  type SupportAction = "copy" | "file" | "folder";

  const SETTINGS_TABS: Array<{ id: SettingsTab; label: string; icon: "mic" | "wave" | "spark" | "output" | "gear" }> = [
    { id: "recording", label: "Recording", icon: "mic" },
    { id: "transcription", label: "Transcription", icon: "wave" },
    { id: "actions", label: "Text actions", icon: "spark" },
    { id: "output", label: "Output", icon: "output" },
    { id: "app", label: "App", icon: "gear" },
  ];
  // Explanations live in tooltips; the page itself stays short.
  const ENGINES: Array<{ mode: TranscriptionMode; label: string; icon: "cloud" | "bolt" | "chip"; hint: string }> = [
    { mode: "gpt_transcribe", label: "Cloud", icon: "cloud", hint: "GPT Transcribe. Audio is sent to OpenAI." },
    { mode: "gpt_live_transcribe", label: "Live", icon: "bolt", hint: "GPT Live Transcribe. Audio streams to OpenAI and text appears at your cursor. Plain only. Do not move the cursor, change fields or type while dictating. Line breaks become spaces." },
    { mode: "local_whisper", label: "Local", icon: "chip", hint: "Local Whisper. Audio stays on this device; only text actions other than Plain use OpenAI." },
  ];
  const THEMES: Array<{ value: Theme; label: string; icon: "screen" | "sun" | "moon" }> = [
    { value: "system", label: "System", icon: "screen" },
    { value: "light", label: "Light", icon: "sun" },
    { value: "dark", label: "Dark", icon: "moon" },
  ];
  const TOGGLE_COMMAND = "utterform --toggle";
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
  let settingsTab: SettingsTab = "recording";
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
  let autostartDraft = false;
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
  // Split on lines, so a term can never carry the newline the API refuses; only
  // the angle brackets have to be caught here.
  $: rejectedTerms = settings.vocabulary.filter((term) => FORBIDDEN_IN_KEYWORD.test(term));
  $: historyOptions = history.map((entry) => ({ value: entry.id, label: entry.title, hint: formatHistoryTime(historyTimestamp(entry), now) }));
  $: displayedTimestamp = selectedHistory ? historyTimestamp(selectedHistory) : resultTimestamp;
  $: recordingActive = phase === "recording" || phase === "paused";
  $: deviceOptions = [{ value: "", label: "System default" }, ...devices.map((device) => ({ value: device.id, label: device.name, hint: device.isDefault ? "Default microphone" : undefined }))];
  $: canRecord = liveMode || settings.copy_to_clipboard || settings.save_to_file || settings.type_at_cursor;
  // A key that could not be reserved at startup had nowhere to report; the
  // field it belongs to is where the user finds out. Derived rather than read
  // once, so a Settings dialog opened before the backend answered still shows it.
  $: hotkeyMessage = hotkeyError || hotkeySupport.failure || "";
  $: typingNote = settings.typing_method === "paste"
    ? "Pastes the text at once and leaves it on the clipboard."
    : "Types character by character. Increase the delay if letters go missing.";
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

  /// Let go of the registered dictation key while a new one is being read,
  /// and take it back again afterwards. Serialized, because the end of a
  /// reading and the Save that follows it must not reach the backend out of
  /// order and leave the session without a key.
  function captureHotkey(capturing: boolean) {
    hotkeyCapturing = capturing;
    hotkeyCaptureWork = hotkeyCaptureWork.then(() => suspendHotkey(capturing));
  }

  async function suspendHotkey(capturing: boolean) {
    if (!hotkeySupport.supported) return;
    if (capturing) {
      // What the operating system is actually holding: the key as it was when
      // Settings opened, not the one being edited in the dialog.
      const registered = settingsSnapshot?.global_hotkey ?? null;
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
      // had. Recording the failure is what makes Save try it again.
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
      autostartDraft = autostartEnabled;
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

  /// Write the change the user made, if they made one, and believe the entry
  /// rather than the request: `isEnabled` is asked again afterwards.
  async function saveAutostart() {
    if (!autostartAvailable || autostartDraft === autostartEnabled) return true;
    const wanted = autostartDraft;
    try {
      if (wanted) await api.enableAutostart();
      else await api.disableAutostart();
      autostartEnabled = await api.autostartEnabled();
      autostartDraft = autostartEnabled;
      if (autostartEnabled !== wanted) {
        autostartError = wanted
          ? "Utterform could not be added to your startup items."
          : "Utterform could not be removed from your startup items.";
        void withReference("autostart", `the startup entry did not change after ${wanted ? "enable" : "disable"}`, () => autostartError, (text) => (autostartError = text));
        return false;
      }
      autostartError = "";
      return true;
    } catch (error) {
      autostartError = String(error);
      void withReference("autostart", error, () => autostartError, (text) => (autostartError = text));
      return false;
    }
  }

  async function savePreferences() {
    // A reading that has not been put back yet owns the shortcut; saving must
    // not register the new key underneath it.
    await hotkeyCaptureWork;
    // A word typed but not yet added is still something the user asked for.
    addVocabulary();
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
          selectSettingsTab("recording");
          return;
        }
      }
      if (!(await saveAutostart())) {
        // The startup entry is the only thing left undone, and the switch it
        // belongs to is where the reason is waiting.
        selectSettingsTab("app");
        return;
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
    let selected: string | string[] | null;
    try {
      selected = await open({ directory: true, multiple: false, title: "Choose output folder" });
    } catch (error) {
      // The dialog plugin answers the interface alone; no command saw this.
      void failures.report("dialog", error);
      return;
    }
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
    settingsTab = "recording";
    vocabularyDraft = "";
    selectPrompt(promptExists(selectedPrompt) ? selectedPrompt : firstEditablePrompt());
    supportMessage = "";
    supportError = "";
    showSettings = true;
    if (isTauri()) {
      void refreshAutostart();
      void refreshDiagnosticsInfo();
      void refreshInputState();
    }
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

  function selectSettingsTab(tab: SettingsTab) {
    settingsTab = tab;
    const panel = document.getElementById("settings-panel");
    if (panel) panel.scrollTop = 0;
  }

  function moveSettingsTab(event: KeyboardEvent, index: number) {
    const step = event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : event.key === "ArrowUp" || event.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const next = SETTINGS_TABS[(index + step + SETTINGS_TABS.length) % SETTINGS_TABS.length];
    selectSettingsTab(next.id);
    // Follow the selection with focus, as a tablist is expected to.
    (document.getElementById(`settings-tab-${next.id}`) as HTMLElement | null)?.focus();
  }

  function addVocabulary(text = vocabularyDraft) {
    vocabularyDraft = "";
    const terms = text.split("\n").map((term) => term.trim()).filter(Boolean);
    if (!terms.length) return;
    settings = { ...settings, vocabulary: [...new Set([...settings.vocabulary, ...terms])] };
  }

  /// A pasted list keeps its lines; a single-line field would run them together.
  function pasteVocabulary(event: ClipboardEvent) {
    const text = event.clipboardData?.getData("text") ?? "";
    if (!text.includes("\n")) return;
    event.preventDefault();
    addVocabulary(`${vocabularyDraft}${text}`);
  }

  function removeVocabulary(term: string) {
    settings = { ...settings, vocabulary: settings.vocabulary.filter((other) => other !== term) };
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
    // Nothing was written to the operating system while the dialog was open,
    // so putting the switch back is all that Cancel has to do here.
    autostartDraft = autostartEnabled;
    autostartError = "";
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

{#if showSettings}
  <div class="modal-backdrop" role="presentation">
    <div class="settings-modal" use:modalFocus role="dialog" tabindex="-1" aria-modal="true" aria-labelledby="settings-title" onkeydown={(event) => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); cancelSettings(); } }}>
      <div class="settings-brand"><Brand className="settings-mark" /><h2 id="settings-title">Settings</h2></div>

      <div class="settings-rail" role="tablist" aria-label="Settings sections" aria-orientation="vertical">
        {#each SETTINGS_TABS as tab, index}
          <button id={`settings-tab-${tab.id}`} role="tab" class:active={settingsTab === tab.id}
            aria-selected={settingsTab === tab.id} aria-controls="settings-panel"
            tabindex={settingsTab === tab.id ? 0 : -1}
            onclick={() => selectSettingsTab(tab.id)} onkeydown={(event) => moveSettingsTab(event, index)}>
            <Icon name={tab.icon} /><strong>{tab.label}</strong>
          </button>
        {/each}
      </div>

      <div class="settings-scroll" id="settings-panel" role="tabpanel" aria-labelledby={`settings-tab-${settingsTab}`}>
        {#if settingsTab === "recording"}
        <div class="setting-group">
          <div class="setting-row"><span>Microphone</span><div class="setting-control wide"><SelectMenu id="microphone" label="Microphone" value={settings.input_device ?? ""} options={deviceOptions} onchange={(value) => settings = { ...settings, input_device: value || null }} /></div></div>
          <div class="setting-row" title={hotkeySupport.supported ? "Press once to record, again to finish" : hotkeySupport.explanation}><span>Shortcut</span><div class="setting-control">
            {#if hotkeySupport.supported}
              {#if settings.global_hotkey !== null}
                <ShortcutRecorder id="dictation-shortcut" label="Shortcut" {apple}
                  value={settings.global_hotkey || hotkeySupport.default}
                  onchange={(shortcut) => (settings = { ...settings, global_hotkey: shortcut })}
                  oncapture={captureHotkey} />
              {/if}
              <input class="switch" type="checkbox" aria-label="Enable global dictation shortcut" checked={settings.global_hotkey !== null} onchange={(event) => (settings = { ...settings, global_hotkey: event.currentTarget.checked ? settings.global_hotkey ?? hotkeySupport.default : null })} />
            {:else}
              <code class="setting-command">{TOGGLE_COMMAND}</code>
              <button class="icon-action" aria-label="Copy command" title="Copy command" onclick={() => api.copyText(TOGGLE_COMMAND).catch(setError)}><Icon name="copy" /></button>
            {/if}
          </div></div>
          {#if hotkeyMessage}<p class="setting-error" role="alert">{hotkeyMessage}</p>{/if}
          <div class="setting-row" title="Start/stop clicks and a chime when text is ready"><span>Sounds</span><div class="setting-control">
            <button class="icon-action" disabled={cueTestPending} aria-label={cueTestPending ? "Playing in 5 seconds…" : "Test sounds (5s delay)"} title={cueTestPending ? "Playing in 5 seconds…" : "Test sounds (5s delay)"} onclick={testCues}><Icon name="play" /></button>
            <input class="switch" type="checkbox" aria-label="Play recording sounds" bind:checked={settings.sound_enabled} />
          </div></div>
          {#if cueTestMessage}<pre class="cue-report" role="status">{cueTestMessage}</pre>{/if}
        </div>
        {/if}

        {#if settingsTab === "transcription"}
        <div class="setting-group">
          <div class="setting-row"><span>Engine</span><div class="setting-control"><div class="segmented" role="group" aria-label="Engine">
            {#each ENGINES as choice}
              <button class:active={transcriptionMode === choice.mode} aria-pressed={transcriptionMode === choice.mode} title={choice.hint} onclick={() => settings = transcriptionSettings(choice.mode)}><Icon name={choice.icon} />{choice.label}</button>
            {/each}
          </div></div></div>
          {#if liveMode && !liveSupport.supported}<p class="setting-error" role="status">{liveSupport.explanation}</p>{/if}
          <div class="setting-row" title="Stored in your system keyring"><label for="api-key">API key</label><div class="setting-control">
            {#if hasApiKey}<span class="setting-ok" title="Stored in your system keyring"><Icon name="check" /><span class="visually-hidden">Stored securely</span></span>{/if}
            <input id="api-key" class="setting-input short" type="password" autocomplete="off" bind:value={apiKeyInput} placeholder={hasApiKey ? "••••••••" : "sk-…"} />
            {#if hasApiKey}<button class="icon-action danger-text" aria-label="Remove API key" title="Remove API key" onclick={removeApiKey}><Icon name="trash" /></button>{/if}
          </div></div>
          <div class="setting-row" title="Comma-separated, optional"><label for="language-hints">Language</label><div class="setting-control"><input id="language-hints" class="setting-input short" value={settings.language_hints.join(", ")} oninput={(event) => (settings = { ...settings, language_hints: event.currentTarget.value.split(",").map((v) => v.trim()).filter(Boolean) })} placeholder="en, de, fr" /></div></div>
        </div>

        <div class="setting-group" role="radiogroup" aria-label="Local Whisper model">
          {#each models as model}
            <div class="setting-row model-row" title={model.description}>
              <label class="model-choice"><input type="radio" name="local-model" aria-label={model.name} checked={settings.local_model_id === model.id} disabled={!model.downloaded} onchange={() => settings = { ...settings, local_model_id: model.id }} /><span>{model.name}</span><span class="setting-muted">{formatBytes(model.sizeBytes)}</span></label>
              <button class="icon-action" class:danger-text={model.downloaded} disabled={busyModel !== null} aria-label={`${model.downloaded ? "Remove" : "Download"} ${model.name}`} title={`${model.downloaded ? "Remove" : "Download"} ${model.name}`} onclick={() => toggleModel(model)}><Icon name={model.downloaded ? "trash" : "download"} /></button>
              {#if busyModel === model.id && !model.downloaded}<progress aria-label={`Downloading ${model.name}`} max="1" value={downloadProgress[model.id] ?? 0}></progress>{/if}
            </div>
          {/each}
        </div>

        <div class="setting-group">
          <div class="setting-row" title="Names and terms to recognise. Works with both engines."><label for="vocabulary">Vocabulary</label><div class="chips">
            {#each settings.vocabulary as term}
              <span class="chip" class:rejected={FORBIDDEN_IN_KEYWORD.test(term)}>{term}<button aria-label={`Remove ${term}`} onclick={() => removeVocabulary(term)}>×</button></span>
            {/each}
            <input id="vocabulary" class="chip-input" placeholder="Add word" value={vocabularyDraft} oninput={(event) => (vocabularyDraft = event.currentTarget.value)} onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); addVocabulary(); } }} onblur={() => addVocabulary()} onpaste={pasteVocabulary} />
          </div></div>
          {#if rejectedTerms.length}<p class="setting-error" role="alert">Not sent, the API refuses &lt; and &gt;: {rejectedTerms.join(", ")}</p>{/if}
          <div class="setting-row" title="Cloud only"><label for="transcription-context">Context</label><div class="setting-control wide"><input id="transcription-context" class="setting-input" placeholder="Optional" value={settings.transcription_context} oninput={(event) => (settings = { ...settings, transcription_context: event.currentTarget.value })} /></div></div>
        </div>
        {/if}

        {#if settingsTab === "actions"}
        {#if liveMode}<p class="setting-hint">Live uses Plain only.</p>{/if}
        <div class="setting-group">
          <div class="setting-row"><label for="text-model">Model</label><div class="setting-control"><input id="text-model" class="setting-input short" bind:value={settings.text_model} /></div></div>
          <div class="setting-row" title="Lower is faster and cheaper"><span>Effort</span><div class="setting-control"><div class="segmented" role="group" aria-label="Effort">
            {#each EFFORTS as effort}
              <button class:active={settings.text_effort === effort.value} onclick={() => (settings = { ...settings, text_effort: effort.value })}>{effort.label}</button>
            {/each}
          </div></div></div>
        </div>

        <div class="setting-group prompt-workbench">
          <div class="prompt-list" role="group" aria-label="Prompts">
            {#each builtInActions as action}
              <button class="prompt-entry" class:active={selectedPrompt === action.id} aria-pressed={selectedPrompt === action.id} onclick={() => selectPrompt(action.id)}>
                <span class="prompt-entry-name">{actionName(action, settings.action_overrides)}</span>
                {#if isActionEdited(action.id, settings.action_overrides)}<span class="prompt-dot" aria-label="Edited"></span>{/if}
              </button>
            {/each}
            {#each settings.custom_actions as action}
              <button class="prompt-entry" class:active={selectedPrompt === `custom:${action.id}`} aria-pressed={selectedPrompt === `custom:${action.id}`} onclick={() => selectPrompt(`custom:${action.id}`)}>
                <span class="prompt-entry-name">{action.name || "Untitled prompt"}</span>
              </button>
            {/each}
            <button class="icon-action prompt-add" aria-label="Add prompt" title="Add prompt" onclick={addCustomAction}><Icon name="plus" /></button>
          </div>

          <div class="prompt-editor">
            {#if editedCustom}
              <input class="setting-input" aria-label="Prompt name" value={nameDraft} oninput={(event) => { nameDraft = event.currentTarget.value; updateCustomAction(editedCustom.id, "name", nameDraft); }} />
              <textarea class="prompt-text" rows="7" aria-label="Prompt instructions" placeholder="How should the transcript change?" value={promptDraft} oninput={(event) => { promptDraft = event.currentTarget.value; updateCustomAction(editedCustom.id, "prompt", promptDraft); }}></textarea>
              <div class="prompt-tools">
                <button class="icon-action danger-text" aria-label="Delete prompt" title="Delete prompt" onclick={() => removeCustomAction(editedCustom.id)}><Icon name="trash" /></button>
              </div>
            {:else if editedPrompt && !editedPrompt.prompt}
              <div class="prompt-empty"><strong>{actionName(editedPrompt, settings.action_overrides)}</strong><span>Transcription only</span></div>
            {:else if editedPrompt}
              <input class="setting-input" aria-label="Prompt name" value={nameDraft} oninput={(event) => { nameDraft = event.currentTarget.value; overrideAction(editedPrompt, "name", nameDraft); }} />
              <textarea class="prompt-text" rows="7" aria-label="Prompt instructions" value={promptDraft} oninput={(event) => { promptDraft = event.currentTarget.value; overrideAction(editedPrompt, "prompt", promptDraft); }}></textarea>
              <div class="prompt-tools">
                <button class="icon-action" aria-pressed={showsDefault === editedPrompt.id} aria-label={showsDefault === editedPrompt.id ? "Hide the original" : "View the original"} title={showsDefault === editedPrompt.id ? "Hide the original" : "View the original"} onclick={() => (showsDefault = showsDefault === editedPrompt.id ? "" : editedPrompt.id)}><Icon name="eye" /></button>
                <button class="icon-action danger-text" aria-label="Reset" title="Reset" disabled={!isActionEdited(editedPrompt.id, settings.action_overrides)} onclick={() => resetAction(editedPrompt.id)}><Icon name="undo" /></button>
              </div>
              {#if showsDefault === editedPrompt.id}<p class="prompt-default">{editedPrompt.prompt}</p>{/if}
            {/if}
          </div>
        </div>
        {/if}

        {#if settingsTab === "output"}
        {#if liveMode}<p class="setting-hint">Live ignores Insert.</p>{/if}
        <div class="setting-group">
          <div class="setting-row" title={typingNote}><span>Insert</span><div class="setting-control"><div class="segmented" role="group" aria-label="Insert">
            <button class:active={settings.typing_method === "paste"} onclick={() => (settings = { ...settings, typing_method: "paste" })}>Paste</button>
            <button class:active={settings.typing_method === "keystrokes"} onclick={() => (settings = { ...settings, typing_method: "keystrokes" })}>Keystrokes</button>
          </div></div></div>
          {#if settings.typing_method === "keystrokes"}
            <div class="setting-row" title="Increase the delay if letters go missing"><label for="typing-delay">Delay (ms)</label><div class="setting-control"><input id="typing-delay" class="setting-input tiny" type="number" min="0" max="500" step="1" value={settings.typing_delay_ms} oninput={(event) => (settings = { ...settings, typing_delay_ms: Math.max(0, Math.min(500, Math.round(Number(event.currentTarget.value) || 0))) })} /></div></div>
          {/if}
          {#if typingSupport.explanation}<p class="setting-error" role="status">{typingSupport.explanation}</p>{/if}
          <div class="setting-row"><label for="output-folder">Folder</label><div class="setting-control wide"><input id="output-folder" class="setting-input" readonly value={settings.output_directory ?? ""} placeholder="None" /><button class="icon-action" aria-label="Browse" title="Browse" onclick={chooseOutputFolder}><Icon name="folder" /></button></div></div>
          <div class="setting-row" title="Stored on this device, unencrypted. Turning this off keeps what is already saved."><span>Keep last 100 texts</span><div class="setting-control">
            {#if confirmClear}
              <button class="text-action danger-text" onclick={clearHistory}>Confirm: delete all saved texts</button>
              <button class="text-action" onclick={() => confirmClear = false}>Keep history</button>
            {:else}
              <button class="icon-action danger-text" aria-label="Clear saved history" title="Clear saved history" onclick={clearHistory}><Icon name="trash" /></button>
            {/if}
            <input class="switch" type="checkbox" aria-label="Keep last 100 texts" bind:checked={settings.history_enabled} />
          </div></div>
          {#if historyMessage}<p class="setting-hint" role="status">{historyMessage}</p>{/if}
        </div>
        {/if}

        {#if settingsTab === "app"}
        <div class="setting-group">
          <div class="setting-row"><span>Theme</span><div class="setting-control"><div class="segmented" role="group" aria-label="Theme">
            {#each THEMES as choice}
              <button class:active={settings.theme === choice.value} onclick={() => { settings = { ...settings, theme: choice.value }; applyTheme(settings.theme); }}><Icon name={choice.icon} />{choice.label}</button>
            {/each}
          </div></div></div>
          <div class="setting-row" title="Starts hidden in the system tray"><span>Start at sign-in</span><div class="setting-control">
            <input class="switch" type="checkbox" aria-label="Start Utterform when I sign in" aria-busy={autostartChecking} checked={autostartDraft} disabled={autostartChecking || !autostartAvailable} onchange={(event) => (autostartDraft = event.currentTarget.checked)} />
          </div></div>
          {#if autostartError}<p class="setting-error" role="alert">{autostartError}</p>{/if}
          <div class="setting-row" title="Logs stay on this device and contain no transcripts or audio"><span>Logs</span><div class="setting-control">
            <button class="text-action" aria-label="Copy diagnostics" title="Copy diagnostics" aria-busy={supportBusy.copy} disabled={supportBusy.copy} onclick={() => runSupportAction("copy")}>Copy</button>
            <button class="text-action" aria-label="Open log file" title={logPath ?? "Log file not available in this session"} aria-busy={supportBusy.file} disabled={supportBusy.file} onclick={() => runSupportAction("file")}>Open</button>
            <button class="icon-action" aria-label="Open log folder" title="Open log folder" aria-busy={supportBusy.folder} disabled={supportBusy.folder} onclick={() => runSupportAction("folder")}><Icon name="folder" /></button>
          </div></div>
          {#if supportMessage}<p class="setting-hint" role="status">{supportMessage}</p>{/if}
          {#if supportError}<p class="setting-error" role="alert">{supportError}</p>{/if}
        </div>
        {/if}
      </div>

      <div class="modal-actions"><button class="secondary" onclick={cancelSettings}>Cancel</button><button class="primary" aria-label="Save settings" onclick={savePreferences}>Save</button></div>
    </div>
  </div>
{/if}
