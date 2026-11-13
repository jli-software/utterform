// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/svelte";
/// Just the queries these helpers use, so they take any render result.
type Screen = {
  getByRole: (role: string, options?: Record<string, unknown>) => HTMLElement;
  queryByRole: (role: string, options?: Record<string, unknown>) => HTMLElement | null;
};
import { DEFAULT_SETTINGS } from "./lib/types";
import type { BuiltInAction, HistoryEntry } from "./lib/types";
import App from "./App.svelte";
import { api } from "./lib/api";
import { open } from "@tauri-apps/plugin-dialog";

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));
const listeners = new Map<string, (event: { payload: unknown }) => void>();
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(name, handler);
    return () => listeners.delete(name);
  }),
}));
/// Deliver what the backend emits when a compositor hotkey runs `utterform --toggle`.
async function remoteIntent(intent: string) {
  await waitFor(() => expect(listeners.has("remote-intent")).toBe(true));
  listeners.get("remote-intent")!({ payload: intent });
}
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("./lib/api", () => ({ api: {
  takeStartupIntent: vi.fn(async () => null), getSettings: vi.fn(), listInputDevices: vi.fn(async () => []), listLocalModels: vi.fn(async () => []),
  listBuiltInActions: vi.fn(),
  liveSupport: vi.fn(), getLiveStatus: vi.fn(), typingSupport: vi.fn(async () => ({ supported: true, explanation: "" })),
  contextSupport: vi.fn(async () => ({ selection: true, explanation: "" })),
  foregroundSupport: vi.fn(async () => ({ supported: true, explanation: "" })),
  foregroundApp: vi.fn(async () => null), announceAppAction: vi.fn(async () => {}),
  hasOpenAiApiKey: vi.fn(async () => true), codexStatus: vi.fn(), openCodexSetup: vi.fn(async () => {}), listHistory: vi.fn(), saveSettings: vi.fn(),
  globalHotkeySupport: vi.fn(async () => ({ supported: true, default: "Ctrl+Alt+D", explanation: "", failure: null })),
  applyGlobalHotkey: vi.fn(),
  autostartEnabled: vi.fn(), enableAutostart: vi.fn(), disableAutostart: vi.fn(),
  startRecording: vi.fn(), finishRecording: vi.fn(), cancelRecording: vi.fn(),
  retainedRecording: vi.fn(async () => null), retryRecording: vi.fn(), discardRecording: vi.fn(),
  notifyIfUnattended: vi.fn(async () => {}),
  getRecordingStatus: vi.fn(), setRecordingPaused: vi.fn(),
  copyText: vi.fn(), clearHistory: vi.fn(),
  playTestCues: vi.fn(),
  diagnosticsInfo: vi.fn(), copyDiagnostics: vi.fn(), openLogFile: vi.fn(), openLogFolder: vi.fn(),
  reportFrontendError: vi.fn(),
  transcribeFile: vi.fn(), takePendingFile: vi.fn(async () => null),
  openOpenAiApiKeys: vi.fn(), completeOnboarding: vi.fn(), setOpenAiApiKey: vi.fn(), downloadLocalModel: vi.fn(),
} }));

// What the backend answers with: the prompts live in Rust, so the interface
// only ever sees them over the wire.
const SHIPPED_ACTIONS: BuiltInAction[] = [
  { id: "plain", name: "Plain", hint: "Transcription only", prompt: "" },
  { id: "clean", name: "Clean", hint: "Fix punctuation and obvious errors", prompt: "Correct punctuation, capitalization, and spelling." },
  { id: "email", name: "Email", hint: "Turn it into a ready-to-send email", prompt: "Write the transcript as an email." },
];

const latest: HistoryEntry = { id: "2", createdAtMs: new Date(2026, 8, 6, 14, 30).getTime(), title: "Plan for tomorrow", text: "Plan for tomorrow: write tests.", durationMs: 2400, engine: "open_ai" };
const older: HistoryEntry = { ...latest, createdAtMs: new Date(2026, 8, 4, 9, 15).getTime(), id: "1", title: "Earlier idea", text: "An earlier idea worth keeping." };

beforeEach(() => {
  vi.clearAllMocks();
  // jsdom has no canvas renderer; actual pixels and motion are covered in Playwright.
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
  listeners.clear();
  vi.mocked(api.copyText).mockReset().mockResolvedValue(undefined);
  vi.mocked(api.takeStartupIntent).mockResolvedValue(null);
  vi.mocked(api.globalHotkeySupport).mockResolvedValue({ supported: true, default: "Ctrl+Alt+D", explanation: "", failure: null });
  vi.mocked(api.applyGlobalHotkey).mockReset().mockResolvedValue(undefined);
  vi.mocked(api.saveSettings).mockReset().mockResolvedValue(undefined);
  vi.mocked(api.liveSupport).mockResolvedValue({ supported: true, explanation: "" });
  vi.mocked(api.typingSupport).mockResolvedValue({ supported: true, explanation: "" });
  vi.mocked(api.getLiveStatus).mockResolvedValue(null);
  vi.mocked(api.getSettings).mockResolvedValue(structuredClone(DEFAULT_SETTINGS));
  vi.mocked(api.listBuiltInActions).mockResolvedValue(structuredClone(SHIPPED_ACTIONS));
  vi.mocked(api.listHistory).mockResolvedValue([latest, older]);
  vi.mocked(api.listLocalModels).mockResolvedValue([]);
  vi.mocked(api.codexStatus).mockResolvedValue({ signedIn: false, models: [], defaultModel: null });
  let paused = false;
  const status = () => ({ recording: true, paused, limitReached: false, elapsedSeconds: 12, level: paused ? 0 : .3 });
  vi.mocked(api.getRecordingStatus).mockImplementation(async () => status());
  vi.mocked(api.setRecordingPaused).mockImplementation(async (value) => { paused = value; return status(); });
  // The operating system's startup entry, as a thing that is written to and
  // read back — nothing in the interface may keep a second copy of it.
  startupEntry = false;
  vi.mocked(api.autostartEnabled).mockImplementation(async () => startupEntry);
  vi.mocked(api.enableAutostart).mockImplementation(async () => { startupEntry = true; });
  vi.mocked(api.disableAutostart).mockImplementation(async () => { startupEntry = false; });
  vi.mocked(api.diagnosticsInfo).mockResolvedValue({ logPath: LOG_PATH });
  vi.mocked(api.copyDiagnostics).mockResolvedValue({ bytes: 12_698, lines: 318, truncated: false });
  vi.mocked(api.openLogFile).mockResolvedValue(undefined);
  vi.mocked(api.openLogFolder).mockResolvedValue(undefined);
  vi.mocked(api.reportFrontendError).mockResolvedValue(null);
  vi.mocked(api.playTestCues).mockResolvedValue(undefined);
});

const LOG_PATH = "/home/tester/.local/share/software.jli.utterform/logs/utterform.log";

let startupEntry = false;

/// Pretend this is a Mac, where the key stored as Super is shown as Cmd.
function onAMac() {
  Object.defineProperty(navigator, "platform", { value: "MacIntel", configurable: true });
}
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(navigator, "platform");
});

/// Settings opens on Recording the first time; everything else is one tab away.
async function showSettingsTab(view: Screen, tab: string) {
  await waitFor(() => expect(view.queryByRole("dialog")).not.toBeNull());
  await fireEvent.click(view.getByRole("tab", { name: tab }));
}

/// Queries within the Settings page an element belongs to.
function screenOf(element: HTMLElement) {
  return within(element.closest<HTMLElement>("[role=dialog]")!);
}

async function renderExpanded() {
  const view = render(App);
  await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
  await fireEvent.click(view.getByRole("button", { name: /Latest text/ }));
  return view;
}

describe("recording pause", () => {
  async function start() {
    const view = await renderExpanded();
    await waitFor(() => expect(view.queryByText(latest.text)).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Pause recording" })).not.toBeNull());
    return view;
  }

  it("pauses and resumes without processing or losing the previous text", async () => {
    const view = await start();
    await fireEvent.click(view.getByRole("button", { name: "Pause recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Resume recording" })).not.toBeNull());
    expect(api.setRecordingPaused).toHaveBeenLastCalledWith(true);
    expect(api.finishRecording).not.toHaveBeenCalled();
    expect(api.cancelRecording).not.toHaveBeenCalled();
    expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(latest.text);
    expect(view.getByRole("button", { name: "Open settings" }).hasAttribute("disabled")).toBe(true);
    await fireEvent.blur(window);
    await fireEvent.keyDown(window, { code: "KeyP", key: "p" });
    await waitFor(() => expect(view.queryByRole("button", { name: "Pause recording" })).not.toBeNull());
    expect(api.setRecordingPaused).toHaveBeenLastCalledWith(false);
    expect(api.startRecording).toHaveBeenCalledOnce();
  });

  it("finishes paused audio with Space, and ignores key repeat", async () => {
    vi.mocked(api.finishRecording).mockResolvedValue({ historyEntry: latest, text: latest.text, durationMs: 12000, engine: "open_ai", savedPath: null, copiedToClipboard: true,
      typedAtCursor: false, deliveryWarnings: [] });
    const view = await start();
    await fireEvent.keyDown(window, { code: "KeyP", repeat: true });
    expect(api.setRecordingPaused).not.toHaveBeenCalled();
    await fireEvent.keyDown(window, { code: "KeyP" });
    await waitFor(() => expect(view.queryByRole("button", { name: "Resume recording" })).not.toBeNull());
    await fireEvent.keyDown(window, { code: "Space" });
    await waitFor(() => expect(api.finishRecording).toHaveBeenCalledOnce());
    expect(api.startRecording).toHaveBeenCalledOnce();
  });

  it("discards a paused recording with Escape", async () => {
    const view = await start();
    await fireEvent.keyDown(window, { code: "KeyP" });
    await waitFor(() => expect(view.queryByRole("button", { name: "Resume recording" })).not.toBeNull());
    await fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(view.queryByText("Recording discarded")).not.toBeNull());
    expect(api.cancelRecording).toHaveBeenCalledOnce();
    expect(api.finishRecording).not.toHaveBeenCalled();
  });

  it("keeps Stop usable after a pause error", async () => {
    vi.mocked(api.setRecordingPaused).mockRejectedValueOnce(new Error("Device unavailable"));
    const view = await start();
    await fireEvent.keyDown(window, { code: "KeyP" });
    await waitFor(() => expect(view.queryByText(/Could not change pause/)).not.toBeNull());
    expect(view.getByRole("button", { name: "Stop recording" }).hasAttribute("disabled")).toBe(false);
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(api.cancelRecording).toHaveBeenCalledOnce();
  });

  it("serializes repeated pause requests and handles the watchdog winning the race", async () => {
    let resolve!: (status: Awaited<ReturnType<typeof api.getRecordingStatus>>) => void;
    vi.mocked(api.setRecordingPaused).mockImplementationOnce(() => new Promise((done) => resolve = done));
    vi.mocked(api.finishRecording).mockResolvedValue({ historyEntry: latest, text: latest.text, durationMs: 600000, engine: "open_ai", savedPath: null, copiedToClipboard: true,
      typedAtCursor: false, deliveryWarnings: [] });
    const view = await start();
    await fireEvent.keyDown(window, { code: "KeyP" });
    await fireEvent.keyDown(window, { code: "KeyP" });
    await fireEvent.keyDown(window, { code: "Space" });
    expect(api.setRecordingPaused).toHaveBeenCalledOnce();
    expect(api.finishRecording).not.toHaveBeenCalled();
    resolve({ recording: false, paused: false, limitReached: true, elapsedSeconds: 600, level: 0 });
    await waitFor(() => expect(api.finishRecording).toHaveBeenCalledOnce());
    await waitFor(() => expect(view.queryByText("Copied to clipboard")).not.toBeNull());
  });
});

describe("transcript history", () => {
  it("restores the latest text on mount and copies selected history with the shortcut", async () => {
    const view = await renderExpanded();
    await waitFor(() => expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(latest.text));
    await fireEvent.click(view.getByRole("button", { name: /Copy/ }));
    expect(api.copyText).toHaveBeenLastCalledWith(latest.text);
    await fireEvent.click(view.getByRole("combobox", { name: "Recent texts" }));
    await fireEvent.click(view.getByRole("option", { name: new RegExp(older.title) }));
    await fireEvent.keyDown(window, { code: "KeyC", ctrlKey: true, shiftKey: true });
    expect(api.copyText).toHaveBeenLastCalledWith(older.text);
    view.unmount();
    const reopened = await renderExpanded();
    await waitFor(() => expect(reopened.getByRole("region", { name: "Transcript" }).textContent).toBe(latest.text));
  });

  it("keeps the previous text while recording, on focus loss and after cancellation", async () => {
    const view = await renderExpanded();
    await waitFor(() => expect(view.queryByText(latest.text)).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await fireEvent.blur(window);
    expect(api.cancelRecording).not.toHaveBeenCalled();
    expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(latest.text);
    await fireEvent.keyDown(view.getByRole("button", { name: "Stop recording" }), { key: "Escape" });
    await waitFor(() => expect(api.cancelRecording).toHaveBeenCalledOnce());
    expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(latest.text);
  });

  it("shows a new result even when history persistence failed", async () => {
    vi.mocked(api.finishRecording).mockResolvedValue({ historyEntry: null, text: "Unsaved but recoverable", durationMs: 1000, engine: "open_ai", savedPath: null, copiedToClipboard: true,
      typedAtCursor: false, deliveryWarnings: ["History was not saved"] });
    const view = await renderExpanded();
    await waitFor(() => expect(view.queryByText(latest.text)).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Stop recording" }));
    await waitFor(() => expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe("Unsaved but recoverable"));
    await fireEvent.keyDown(window, { code: "KeyC", metaKey: true, shiftKey: true });
    expect(api.copyText).toHaveBeenLastCalledWith("Unsaved but recoverable");
  });

  it("shows European history dates and updates relative minutes while open", async () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    vi.setSystemTime(new Date(2026, 8, 6, 14, 35));
    const view = await renderExpanded();
    await waitFor(() => expect(view.queryByText("Today · 14:30 · 5 min ago")).not.toBeNull());
    await vi.advanceTimersByTimeAsync(60_000);
    await waitFor(() => expect(view.queryByText("Today · 14:30 · 6 min ago")).not.toBeNull());
    await fireEvent.click(view.getByRole("combobox", { name: "Recent texts" }));
    await fireEvent.click(view.getByRole("option", { name: /Earlier idea 04.09.2026/ }));
    expect(view.container.querySelector("time")?.textContent).toBe("04.09.2026");
    expect(view.container.querySelector(".result-date")?.getAttribute("title")).toBe("04.09.2026 · 09:15");
  });

  it("requires confirmation before clearing history", async () => {
    const view = await renderExpanded();
    await waitFor(() => expect(view.queryByText(latest.text)).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Output");
    await fireEvent.click(view.getByRole("button", { name: "Clear" }));
    expect(api.clearHistory).not.toHaveBeenCalled();
    await fireEvent.click(view.getByRole("button", { name: "Delete all" }));
    expect(api.clearHistory).toHaveBeenCalledOnce();
    await waitFor(() => expect(view.queryByText(latest.text)).toBeNull());
  });
});


describe("compact result feedback", () => {
  it("starts collapsed and copies without exposing the transcript, also after remount", async () => {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    expect(view.queryByRole("region", { name: "Transcript" })).toBeNull();
    expect(view.getByRole("button", { name: /Latest text/ }).getAttribute("aria-expanded")).toBe("false");
    await fireEvent.click(view.getByRole("button", { name: /Copy/ }));
    expect(api.copyText).toHaveBeenLastCalledWith(latest.text);
    expect(view.queryByRole("region", { name: "Transcript" })).toBeNull();
    await waitFor(() => expect(view.queryByText("Copied")).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: /Latest text/ }));
    expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(latest.text);
    view.unmount();
    const reopened = render(App);
    await waitFor(() => expect(reopened.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    expect(reopened.queryByRole("region", { name: "Transcript" })).toBeNull();
  });

  it.each([true, false])("reports actual automatic clipboard outcome (%s), not the requested setting", async (copiedToClipboard) => {
    vi.mocked(api.finishRecording).mockResolvedValue({ historyEntry: latest, text: latest.text, durationMs: 1000, engine: "open_ai", savedPath: null, copiedToClipboard, typedAtCursor: false, deliveryWarnings: copiedToClipboard ? ["History was not saved"] : ["Clipboard unavailable"] });
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Stop recording" }));
    await waitFor(() => expect(view.queryByText(copiedToClipboard ? "History was not saved" : "Clipboard unavailable")).not.toBeNull());
    expect(view.queryByText("Copied") !== null).toBe(copiedToClipboard);
    expect(view.queryByRole("region", { name: "Transcript" })).toBeNull();
  });

  it("does not show success before copy resolves, after failure, or on a different history entry", async () => {
    let resolve!: () => void;
    vi.mocked(api.copyText).mockImplementationOnce(() => new Promise<void>((done) => resolve = done));
    const view = await renderExpanded();
    await fireEvent.click(view.getByRole("button", { name: /Copy/ }));
    expect(view.queryByText("Copied")).toBeNull();
    await fireEvent.click(view.getByRole("combobox", { name: "Recent texts" }));
    await fireEvent.click(view.getByRole("option", { name: new RegExp(older.title) }));
    resolve();
    await waitFor(() => expect(view.getByRole("button", { name: /Copy/ }).hasAttribute("disabled")).toBe(false));
    expect(view.queryByText("Copied")).toBeNull();
    vi.mocked(api.copyText).mockRejectedValueOnce(new Error("Clipboard unavailable"));
    await fireEvent.click(view.getByRole("button", { name: /Copy/ }));
    await waitFor(() => expect(view.queryByText(/Could not copy:.*Clipboard unavailable/)).not.toBeNull());
    expect(view.queryByText("Copied")).toBeNull();
  });
});


it("drains an outstanding manual copy before recording and blocks copies until processing ends", async () => {
  let completeCopy!: () => void;
  let completeProcessing!: (value: Awaited<ReturnType<typeof api.finishRecording>>) => void;
  vi.mocked(api.copyText).mockImplementationOnce(() => new Promise<void>((done) => completeCopy = done));
  vi.mocked(api.finishRecording).mockImplementationOnce(() => new Promise((done) => completeProcessing = done));
  const view = render(App);
  await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
  await fireEvent.click(view.getByRole("button", { name: /Copy/ }));
  await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
  expect(api.startRecording).not.toHaveBeenCalled();
  completeCopy();
  await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
  await fireEvent.click(view.getByRole("button", { name: "Stop recording" }));
  await waitFor(() => expect(api.finishRecording).toHaveBeenCalledOnce());
  expect(view.getByRole("button", { name: /Copy/ }).hasAttribute("disabled")).toBe(true);
  await fireEvent.keyDown(window, { code: "KeyC", ctrlKey: true, shiftKey: true });
  expect(api.copyText).toHaveBeenCalledOnce();
  completeProcessing({ text: "New text", historyEntry: null, savedPath: null, copiedToClipboard: true,
      typedAtCursor: false, durationMs: 1000, engine: "open_ai", deliveryWarnings: [] });
  await waitFor(() => expect(view.queryByText("Copied")).not.toBeNull());
  expect(view.getByRole("button", { name: /Copied/ }).hasAttribute("disabled")).toBe(false);
});

// Both a compositor binding running `utterform --toggle` and a system-wide
// dictation key on Windows, macOS or X11 arrive as the same `remote-intent`
// event, so these cover the recording path for every platform.
describe("dictation hotkey", () => {
  it("starts and finishes a recording without the window being touched", async () => {
    vi.mocked(api.finishRecording).mockResolvedValue({ historyEntry: null, text: "Dictated", savedPath: null,
      copiedToClipboard: true, typedAtCursor: true, deliveryWarnings: [], durationMs: 900, engine: "open_ai" });
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Start recording" })).not.toBeNull());

    await remoteIntent("toggle");
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledOnce());

    await remoteIntent("toggle");
    await waitFor(() => expect(api.finishRecording).toHaveBeenCalledOnce());
    await waitFor(() => expect(view.queryByText("Typed at the cursor · Copied to clipboard")).not.toBeNull());
  });

  it("ignores a stop or cancel when nothing is being recorded", async () => {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Start recording" })).not.toBeNull());
    await remoteIntent("stop");
    await remoteIntent("cancel");
    expect(api.finishRecording).not.toHaveBeenCalled();
    expect(api.cancelRecording).not.toHaveBeenCalled();
  });

  it("takes the action chosen for the app in front, with its prompt, and only from the key", async () => {
    vi.mocked(api.getSettings).mockResolvedValue({
      ...structuredClone(DEFAULT_SETTINGS),
      custom_actions: [{ id: "a", name: "Notes", prompt: "Bullets only" }],
      app_actions: [{ app: { id: "thunderbird", name: "Thunderbird" }, action: "custom:a" }],
    });
    vi.mocked(api.foregroundApp).mockResolvedValue({ id: "thunderbird", name: "Thunderbird" });
    vi.mocked(api.finishRecording).mockResolvedValue({ historyEntry: null, text: "- Bullet", savedPath: null,
      copiedToClipboard: true, typedAtCursor: false, deliveryWarnings: [], durationMs: 900, engine: "open_ai" });
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Start recording" })).not.toBeNull());

    await remoteIntent("toggle");
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledWith(null, "open_ai", "base", "custom"));
    expect(api.announceAppAction).toHaveBeenCalledWith("custom", "Notes", "Thunderbird");
    await remoteIntent("toggle");
    await waitFor(() => expect(api.finishRecording).toHaveBeenCalledWith(expect.objectContaining({ action: "custom", customPrompt: "Bullets only" })));
    expect(vi.mocked(api.saveSettings).mock.calls.at(-1)?.[0].recent_apps).toEqual([{ id: "thunderbird", name: "Thunderbird" }]);

    // From the window itself the window's own action applies; nothing is asked.
    await waitFor(() => expect(view.queryByRole("button", { name: "Start recording" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledTimes(2));
    expect(vi.mocked(api.startRecording).mock.calls[1][3]).toBe("plain");
    expect(api.foregroundApp).toHaveBeenCalledOnce();
    expect(api.announceAppAction).toHaveBeenCalledOnce();
  });

  it("records straight away when the hotkey had to start the app first", async () => {
    vi.mocked(api.takeStartupIntent).mockResolvedValue("toggle");
    render(App);
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledOnce());
  });
});

describe("the dictation key recorder", () => {
  async function openSettings() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Recording");
    return view;
  }

  /// Start reading a shortcut, and wait until the registered key has been let
  /// go of — until then the operating system would swallow the very key the
  /// user is trying to press.
  async function listen(view: Screen) {
    const recorder = view.getByRole("button", { name: /Shortcut/ });
    await fireEvent.click(recorder);
    expect(recorder.getAttribute("aria-pressed")).toBe("true");
    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenCalledWith(null));
    return recorder;
  }

  it("takes a combination from the keyboard, stores it and registers exactly it at once", async () => {
    const view = await openSettings();
    const recorder = await listen(view);
    expect(recorder.textContent).toContain("Press shortcut");

    await fireEvent.keyDown(recorder, { code: "KeyK", key: "k", ctrlKey: true, altKey: true });
    expect(recorder.textContent).toContain("Ctrl+Alt+K");
    expect(recorder.getAttribute("aria-pressed")).toBe("false");
    // The old key comes back first, then the new one replaces it.
    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenLastCalledWith("Ctrl+Alt+K"));
    expect(vi.mocked(api.applyGlobalHotkey).mock.calls.map(([key]) => key)).toEqual([null, "Ctrl+Alt+D", "Ctrl+Alt+K"]);
    expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ global_hotkey: "Ctrl+Alt+K" }));
    expect(view.queryByRole("dialog")).not.toBeNull();
  });

  it("reads the physical key, and shows a Mac the name it uses for it", async () => {
    onAMac();
    const view = await openSettings();
    const recorder = await listen(view);
    // A German keyboard types Z here and an American one Y; the combination
    // must be the same one, and Meta is stored as Super either way.
    await fireEvent.keyDown(recorder, { code: "KeyY", key: "z", metaKey: true, shiftKey: true });
    expect(recorder.textContent).toContain("Shift+Cmd+Y");
    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenLastCalledWith("Shift+Super+Y"));
  });

  it("reads space, a digit, a function key and an arrow", async () => {
    const view = await openSettings();
    for (const [code, shown] of [["Space", "Ctrl+Shift+Space"], ["Digit4", "Ctrl+Shift+4"],
      ["F9", "Ctrl+Shift+F9"], ["ArrowUp", "Ctrl+Shift+Up"]] as const) {
      vi.mocked(api.applyGlobalHotkey).mockClear();
      const recorder = await listen(view);
      await fireEvent.keyDown(recorder, { code, ctrlKey: true, shiftKey: true });
      expect(recorder.textContent).toContain(shown);
    }
  });

  it("waits while only modifiers are held", async () => {
    const view = await openSettings();
    const recorder = await listen(view);
    await fireEvent.keyDown(recorder, { code: "ControlLeft", key: "Control", ctrlKey: true });
    await fireEvent.keyDown(recorder, { code: "AltLeft", key: "Alt", ctrlKey: true, altKey: true });
    expect(recorder.textContent).toContain("Press shortcut");
    expect(recorder.getAttribute("aria-pressed")).toBe("true");
  });

  it("refuses a key on its own and keeps the shortcut that works", async () => {
    const view = await openSettings();
    const recorder = await listen(view);
    await fireEvent.keyDown(recorder, { code: "KeyD", key: "d" });
    // Said where the shortcut is, and announced through the live region.
    const complaint = view.getAllByText(/D needs a modifier/);
    expect(complaint.some((node) => node.classList.contains("setting-error"))).toBe(true);
    expect(complaint.some((node) => node.getAttribute("role") === "status")).toBe(true);
    expect(recorder.getAttribute("aria-pressed")).toBe("true");
    expect(api.saveSettings).not.toHaveBeenCalled();

    // The same key again is no change: nothing is stored, the key is held again.
    await fireEvent.keyDown(recorder, { code: "KeyD", key: "d", ctrlKey: true, altKey: true });
    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenLastCalledWith("Ctrl+Alt+D"));
    expect(api.saveSettings).not.toHaveBeenCalled();
  });

  it("lets Escape end the reading without closing Settings", async () => {
    const view = await openSettings();
    const recorder = await listen(view);
    await fireEvent.keyDown(recorder, { code: "Escape", key: "Escape" });
    expect(recorder.getAttribute("aria-pressed")).toBe("false");
    expect(recorder.textContent).toContain("Ctrl+Alt+D");
    expect(view.queryByRole("dialog")).not.toBeNull();
    // And the key Utterform had is held again.
    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenLastCalledWith("Ctrl+Alt+D"));

    // A second Escape is the page's again, as it always was.
    await fireEvent.keyDown(view.getByRole("dialog"), { key: "Escape" });
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.saveSettings).not.toHaveBeenCalled();
  });

  it("does not let the key it is listening for start a recording", async () => {
    const view = await openSettings();
    await listen(view);
    // Windows, macOS and X11 have released the key by now; a compositor
    // binding still arrives and is what this turns away.
    await remoteIntent("toggle");
    expect(api.startRecording).not.toHaveBeenCalled();
  });

  it("names the problem beside the field when the key is taken, and keeps the rest stored", async () => {
    const view = await openSettings();
    const recorder = await listen(view);
    vi.mocked(api.applyGlobalHotkey).mockImplementation(async (key) => {
      if (key === "Ctrl+Alt+K") throw "Ctrl+Alt+K is not available";
    });
    await fireEvent.keyDown(recorder, { code: "KeyK", key: "k", ctrlKey: true, altKey: true });

    await waitFor(() => expect(view.queryByRole("alert")).not.toBeNull());
    expect(view.getByRole("alert").textContent).toContain("not available");
    expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ global_hotkey: "Ctrl+Alt+K" }));
    expect(view.queryByRole("dialog")).not.toBeNull();
  });

  it("leaves a working key alone when other settings change", async () => {
    const view = await openSettings();
    await showSettingsTab(view, "App");
    await fireEvent.click(view.getByRole("button", { name: "Dark" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ theme: "dark" })));
    expect(api.applyGlobalHotkey).not.toHaveBeenCalled();
  });

  it("turning the key off unregisters it at once, and on again registers it", async () => {
    const view = await openSettings();
    const toggle = view.getByRole("switch", { name: "Global dictation shortcut" });
    await fireEvent.click(toggle);
    expect(view.queryByRole("button", { name: /Shortcut/ })).toBeNull();
    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenLastCalledWith(null));
    expect(api.saveSettings).toHaveBeenLastCalledWith(expect.objectContaining({ global_hotkey: null }));

    await fireEvent.click(toggle);
    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenLastCalledWith("Ctrl+Alt+D"));
  });

  it("reports a key that could not be reserved at startup, when Settings is first opened", async () => {
    vi.mocked(api.globalHotkeySupport).mockResolvedValue({
      supported: true, default: "Ctrl+Alt+D", explanation: "",
      failure: "Ctrl+Alt+D is not available — another application may already hold it",
    });
    const view = await openSettings();
    expect(view.getByRole("alert").textContent).toContain("not available");
    expect(view.getByRole("tab", { name: "Recording" }).getAttribute("aria-selected")).toBe("true");
  });

  it("offers a Wayland session the command line instead of a dead recorder", async () => {
    vi.mocked(api.globalHotkeySupport).mockResolvedValue({
      supported: false, default: "Ctrl+Alt+D", failure: null,
      explanation: "Wayland gives no application a global shortcut. Bind `utterform --toggle` in your compositor instead.",
    });
    const view = await openSettings();
    await waitFor(() => expect(view.queryByText(/utterform --toggle/)).not.toBeNull());
    expect(view.queryByRole("button", { name: /Shortcut/ })).toBeNull();
  });
});

// Autostart is the operating system's own entry. Nothing here may keep a copy
// of it: what `isEnabled` says is what the switch shows.
describe("starting when the user signs in", () => {
  async function openStartup() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "App");
    return view;
  }

  function toggleOf(view: Screen) {
    return view.getByRole("switch", { name: /when I sign in/ }) as HTMLInputElement;
  }

  it("shows what the system says rather than a stored setting", async () => {
    startupEntry = true;
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).checked).toBe(true));
    expect(api.autostartEnabled).toHaveBeenCalled();
    // Settings that reach the backend carry nothing about starting up.
    await fireEvent.click(view.getByRole("button", { name: "Dark" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalled());
    expect(Object.keys(vi.mocked(api.saveSettings).mock.calls[0][0]).join()).not.toMatch(/autostart|login|startup/);
  });

  it("is off unless the system says otherwise", async () => {
    const view = await openStartup();
    await waitFor(() => expect(api.autostartEnabled).toHaveBeenCalled());
    expect(toggleOf(view).checked).toBe(false);
  });

  it("writes at once, then reads the entry back", async () => {
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).disabled).toBe(false));
    await fireEvent.click(toggleOf(view));
    await waitFor(() => expect(api.enableAutostart).toHaveBeenCalledOnce());
    expect(api.disableAutostart).not.toHaveBeenCalled();
    // Written, then read back: the entry is the truth, not the request.
    await waitFor(() => expect(api.autostartEnabled).toHaveBeenCalledTimes(2));
    expect(toggleOf(view).checked).toBe(true);

    await fireEvent.click(toggleOf(view));
    await waitFor(() => expect(api.disableAutostart).toHaveBeenCalledOnce());
    await waitFor(() => expect(toggleOf(view).checked).toBe(false));
  });

  it("a system that will not answer costs the switch and nothing else", async () => {
    vi.mocked(api.autostartEnabled).mockRejectedValue("no startup items on this system");
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).disabled).toBe(true));
    expect(view.getByRole("alert").textContent).toContain("could not read");

    // Everything else still saves.
    await fireEvent.click(view.getByRole("button", { name: "Dark" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalled());
  });

  it("keeps a failed write in front of the user, with the switch as the system has it", async () => {
    vi.mocked(api.enableAutostart).mockRejectedValue("the startup folder is read-only");
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).disabled).toBe(false));
    await fireEvent.click(toggleOf(view));

    await waitFor(() => expect(view.queryByRole("alert")).not.toBeNull());
    expect(view.getByRole("alert").textContent).toContain("read-only");
    expect(view.queryByRole("dialog")).not.toBeNull();
    await waitFor(() => expect(toggleOf(view).checked).toBe(false));
  });

  it("believes the entry rather than the call that claimed to write it", async () => {
    // Reports success, changes nothing: without reading back, the switch
    // would show an entry that does not exist.
    vi.mocked(api.enableAutostart).mockResolvedValue(undefined);
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).disabled).toBe(false));
    await fireEvent.click(toggleOf(view));

    await waitFor(() => expect(view.queryByRole("alert")).not.toBeNull());
    expect(view.getByRole("alert").textContent).toContain("could not be added");
    expect(toggleOf(view).checked).toBe(false);
  });
});

describe("typing at the cursor", () => {
  it("keeps the keystroke delay out of the way until keystrokes are chosen", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Output");

    // Paste is the default because it cannot drop or reorder characters.
    expect(view.queryByRole("spinbutton", { name: "Keystroke delay" })).toBeNull();
    await fireEvent.click(view.getByRole("button", { name: "Keystrokes" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ typing_method: "keystrokes" })));
    const delay = view.getByRole("spinbutton", { name: "Keystroke delay" }) as HTMLInputElement;
    expect(delay.value).toBe("15");

    // Typed numbers wait for a pause before they are written.
    vi.mocked(api.saveSettings).mockClear();
    await fireEvent.input(delay, { target: { value: "4" } });
    await fireEvent.input(delay, { target: { value: "40" } });
    expect(api.saveSettings).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(600);
    expect(api.saveSettings).toHaveBeenCalledOnce();
    expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ typing_method: "keystrokes", typing_delay_ms: 40 }));
  });

  it("writes a change still waiting for its pause when Settings closes", async () => {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Transcription");
    await fireEvent.input(view.getByRole("textbox", { name: "Topic" }), { target: { value: "A standup" } });
    expect(api.saveSettings).not.toHaveBeenCalled();
    await fireEvent.click(view.getByRole("button", { name: "Close settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ transcription_context: "A standup" })));
  });

  it("says when a change could not be written", async () => {
    vi.mocked(api.saveSettings).mockRejectedValueOnce("the settings file is read-only");
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Output");
    await fireEvent.click(view.getByRole("button", { name: "Keystrokes" }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("read-only"));
  });
});

describe("editing the prompts Utterform ships with", () => {
  async function openPrompts() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "AI");
    return view;
  }

  async function openAction(view: Screen, name: string | RegExp) {
    await fireEvent.click(view.getByRole("button", { name }));
  }

  async function back(view: Screen) {
    await fireEvent.click(view.getByRole("button", { name: "‹ AI" }));
  }

  it("shows the shipped instructions one level down, and Plain as the one with none", async () => {
    const view = await openPrompts();
    expect(view.queryByRole("textbox", { name: "Prompt instructions" })).toBeNull();
    await openAction(view, "Clean");
    expect((view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement).value)
      .toBe("Correct punctuation, capitalization, and spelling.");

    await back(view);
    await openAction(view, "Plain");
    expect(view.queryByRole("textbox", { name: "Prompt instructions" })).toBeNull();
    expect(view.queryByText(/no prompt/)).not.toBeNull();
  });

  it("writes a rewritten prompt after a pause in typing, and offers the original alongside it", async () => {
    const view = await openPrompts();
    await openAction(view, "Email");
    const editor = view.getByRole("textbox", { name: "Prompt instructions" });
    await fireEvent.input(editor, { target: { value: "Answer in two sentences." } });

    await fireEvent.click(view.getByRole("button", { name: "View the original" }));
    expect(view.queryByText("Write the transcript as an email.")).not.toBeNull();

    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({
      action_overrides: { email: { prompt: "Answer in two sentences." } },
    })));
  });

  it("keeps an emptied box empty to type in, without storing an edit", async () => {
    const view = await openPrompts();
    await openAction(view, "Clean");
    const editor = view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement;
    await fireEvent.input(editor, { target: { value: "" } });

    expect(editor.value).toBe("");
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ action_overrides: {} }),
    ));
  });

  it("brings the shipped text back, and stops calling the action edited", async () => {
    vi.mocked(api.getSettings).mockResolvedValue({
      ...structuredClone(DEFAULT_SETTINGS),
      action_overrides: { clean: { name: "Tidy", prompt: "Fix commas only." } },
    });
    const view = await openPrompts();
    await openAction(view, "Tidy");
    expect((view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement).value)
      .toBe("Fix commas only.");

    await fireEvent.click(view.getByRole("button", { name: "Reset" }));
    expect((view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement).value)
      .toBe("Correct punctuation, capitalization, and spelling.");
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ action_overrides: {} }),
    ));
    await back(view);
    expect(view.queryByRole("button", { name: "Tidy" })).toBeNull();
    expect(view.queryByRole("button", { name: "Clean" })).not.toBeNull();
  });

  it("offers a renamed action under its new name in the main window", async () => {
    const view = await openPrompts();
    await openAction(view, "Email");
    await fireEvent.input(view.getByRole("textbox", { name: "Prompt name" }), { target: { value: "Reply" } });
    await fireEvent.click(view.getByRole("button", { name: "Close settings" }));

    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({
      action_overrides: { email: { name: "Reply" } },
    })));
    await fireEvent.click(view.getByRole("combobox", { name: "Action" }));
    await waitFor(() => expect(view.queryByRole("option", { name: /Reply/ })).not.toBeNull());
    expect(view.queryByRole("option", { name: /Email/ })).toBeNull();
  });

  it("opens an added action straight away, and drops it again on delete", async () => {
    const view = await openPrompts();
    await fireEvent.click(view.getByRole("button", { name: "Add action" }));
    const editor = view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement;
    expect(editor.value).toBe("");
    await fireEvent.input(view.getByRole("textbox", { name: "Prompt name" }), { target: { value: "Standup" } });
    await fireEvent.input(editor, { target: { value: "Three bullets." } });

    await fireEvent.click(view.getByRole("button", { name: "Delete" }));
    expect(view.queryByRole("button", { name: "Standup" })).toBeNull();
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ custom_actions: [] }),
    ));
  });

  it("sends a chosen reasoning level, and nothing at all on Auto", async () => {
    const view = await openPrompts();
    await fireEvent.click(view.getByRole("button", { name: /^Text model/ }));
    await fireEvent.click(view.getByRole("button", { name: "Low" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ text_effort: "low" }),
    ));
    await fireEvent.click(view.getByRole("button", { name: "Auto" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ text_effort: null }),
    ));
  });
});

describe("text actions through ChatGPT", () => {
  const CODEX = {
    signedIn: true,
    models: [{ id: "gpt-6.1-sol", name: "GPT-6.1-Sol" }, { id: "gpt-6-luna", name: "GPT-6-Luna" }],
    defaultModel: "gpt-6-luna",
  };

  async function openAi() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "AI");
    return view;
  }

  it("switches the provider at once, marked experimental, and says when the login is missing", async () => {
    const view = await openAi();
    expect(view.queryByText("Experimental")).toBeNull();
    await fireEvent.click(view.getByRole("button", { name: "ChatGPT" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ text_provider: "codex" }),
    ));
    expect(view.queryByText("Experimental")).not.toBeNull();
    await waitFor(() => expect(view.queryByRole("button", { name: "Set up" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Set up" }));
    expect(api.openCodexSetup).toHaveBeenCalled();

    await fireEvent.click(view.getByRole("button", { name: "OpenAI key" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ text_provider: "openai_key" }),
    ));
    expect(view.queryByText("Experimental")).toBeNull();
  });

  it("offers the models the Codex CLI lists, Auto first, and no Minimal effort", async () => {
    vi.mocked(api.codexStatus).mockResolvedValue(structuredClone(CODEX));
    vi.mocked(api.getSettings).mockResolvedValue({ ...structuredClone(DEFAULT_SETTINGS), text_provider: "codex", text_effort: "minimal" });
    const view = await openAi();
    await waitFor(() => expect(view.queryByRole("button", { name: /^Text model/ })?.textContent).toContain("GPT-6-Luna · Low"));
    expect(view.queryByRole("button", { name: "Set up" })).toBeNull();
    await fireEvent.click(view.getByRole("button", { name: /^Text model/ }));
    expect(view.queryByRole("button", { name: "Minimal" })).toBeNull();
    expect(view.queryByRole("textbox", { name: "Model" })).toBeNull();
    await fireEvent.click(view.getByRole("combobox", { name: "ChatGPT model" }));
    const options = [...document.querySelectorAll("[role=option]")].map((node) => node.textContent?.trim());
    expect(options[0]).toContain("Auto");
    expect(options.slice(1)).toEqual(["GPT-6.1-Sol", "GPT-6-Luna"]);
    await fireEvent.click(view.getByRole("option", { name: "GPT-6.1-Sol" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ codex_model: "gpt-6.1-sol" }),
    ));
  });
});

describe("vocabulary", () => {
  async function openVocabulary() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Transcription");
    return view.getByRole("textbox", { name: "Vocabulary" }) as HTMLInputElement;
  }

  it("adds a word on Enter, keeps a pasted list's lines, and removes a word with ×", async () => {
    const field = await openVocabulary();
    await fireEvent.input(field, { target: { value: "Careum" } });
    await fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(expect.objectContaining({ vocabulary: ["Careum"] })));

    const pasted = new Event("paste", { bubbles: true, cancelable: true }) as Event & { clipboardData: unknown };
    pasted.clipboardData = { getData: () => "Utterform\n\n  Omarchy  \nCareum\n" };
    await fireEvent(field, pasted);
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ vocabulary: ["Careum", "Utterform", "Omarchy"] }),
    ));

    await fireEvent.click(screenOf(field).getByRole("button", { name: "Remove Utterform" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ vocabulary: ["Careum", "Omarchy"] }),
    ));
  });

  it("refuses a term the transcription API would refuse, rather than losing the recording to it", async () => {
    const field = await openVocabulary();
    await fireEvent.input(field, { target: { value: "<tagged>" } });
    await fireEvent.keyDown(field, { key: "Enter" });
    const alert = screenOf(field).getByRole("alert");
    expect(alert.textContent).toContain("< and > are not allowed");
    expect(field.value).toBe("<tagged>");
    expect(api.saveSettings).not.toHaveBeenCalled();
  });

  it("marks a stored term the API would refuse, so it can be removed", async () => {
    vi.mocked(api.getSettings).mockResolvedValue({ ...structuredClone(DEFAULT_SETTINGS), vocabulary: ["Careum", "<old>"] });
    const field = await openVocabulary();
    const chip = screenOf(field).getByRole("button", { name: "Remove <old>" }).closest(".chip")!;
    expect(chip.classList.contains("flagged")).toBe(true);
  });
});

describe("finding a setting", () => {
  async function openSearch() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await waitFor(() => expect(view.queryByRole("dialog")).not.toBeNull());
    return view;
  }

  it("shows matching rows from every category, each with its category", async () => {
    const view = await openSearch();
    const search = view.getByRole("searchbox", { name: "Search settings" });
    expect(document.activeElement).toBe(search);
    await fireEvent.input(search, { target: { value: "key" } });
    await waitFor(() => expect(view.queryByRole("button", { name: /Shortcut/ })).not.toBeNull());
    expect(view.queryByText("OpenAI key")).not.toBeNull();
    expect(view.queryByRole("button", { name: "Keystrokes" })).not.toBeNull();
    expect(view.queryByRole("combobox", { name: "Microphone" })).toBeNull();
    expect([...document.querySelectorAll(".row-where")].map((node) => node.textContent)).toEqual(["Recording", "Transcription", "Output"]);
  });

  it("finds a row by a word that is not on the screen, and says when nothing matches", async () => {
    const view = await openSearch();
    const search = view.getByRole("searchbox", { name: "Search settings" });
    await fireEvent.input(search, { target: { value: "autostart" } });
    expect(view.queryByRole("switch", { name: /when I sign in/ })).not.toBeNull();
    await fireEvent.input(search, { target: { value: "xylophone" } });
    expect(view.queryByText("Nothing found.")).not.toBeNull();
  });

  it("clears the search on Escape before leaving, and a category tab ends it", async () => {
    const view = await openSearch();
    const search = view.getByRole("searchbox", { name: "Search settings" }) as HTMLInputElement;
    await fireEvent.input(search, { target: { value: "theme" } });
    await fireEvent.keyDown(view.getByRole("dialog"), { key: "Escape" });
    expect(search.value).toBe("");
    expect(view.queryByRole("dialog")).not.toBeNull();

    await fireEvent.input(search, { target: { value: "theme" } });
    await fireEvent.click(view.getByRole("tab", { name: "Output" }));
    expect(search.value).toBe("");
    expect(view.queryByRole("button", { name: "Keystrokes" })).not.toBeNull();
  });
});

describe("live dictation", () => {
  const snapshot = { text: "Hello live world", insertedText: "Hello ", deliveryPaused: true, warning: "Focus changed. Insertion is paused.", phase: "streaming" as const };

  function useLive() {
    vi.mocked(api.getSettings).mockResolvedValue({ ...structuredClone(DEFAULT_SETTINGS), cloud_model: "gpt_live_transcribe", copy_to_clipboard: false, save_to_file: false, type_at_cursor: false });
    vi.mocked(api.getLiveStatus).mockResolvedValue(snapshot);
    vi.mocked(api.finishRecording).mockResolvedValue({ historyEntry: null, text: snapshot.text, durationMs: 12000, engine: "open_ai", savedPath: null, copiedToClipboard: false, typedAtCursor: true, deliveryWarnings: [snapshot.warning] });
  }

  it("migrates old settings to batch transcription and persists the live choice", async () => {
    const { cloud_model: _oldMissingField, ...oldSettings } = DEFAULT_SETTINGS;
    vi.mocked(api.getSettings).mockResolvedValue(oldSettings as typeof DEFAULT_SETTINGS);
    const view = await renderExpanded();
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Transcription");
    const settingsPage = within(view.getByRole("dialog"));
    expect(settingsPage.getByRole("button", { name: "Cloud" }).getAttribute("aria-pressed")).toBe("true");
    await fireEvent.click(settingsPage.getByRole("button", { name: "Live" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ cloud_model: "gpt_live_transcribe" })));
    await fireEvent.click(view.getByRole("button", { name: "Close settings" }));
    expect(view.queryByRole("combobox", { name: "Action" })).toBeNull();
    expect(view.queryByText(/Place the cursor in your text field/)).not.toBeNull();
  });

  it("explains target-field startup, streams on remote start, and never batch-types on finish", async () => {
    useLive();
    const view = await renderExpanded();
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    expect(api.startRecording).not.toHaveBeenCalled();
    await fireEvent.keyDown(window, { key: "2" });
    await remoteIntent("start");
    await waitFor(() => expect(view.queryByRole("region", { name: "Live transcript" })?.textContent).toBe(snapshot.text));
    expect(api.startRecording).toHaveBeenCalledWith(null, "open_ai", "base", "plain");
    expect(view.queryByText(snapshot.warning)).not.toBeNull();
    expect(view.queryByText(/Some text may already be inserted/)).not.toBeNull();
    await remoteIntent("stop");
    await waitFor(() => expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(snapshot.text));
    expect(api.finishRecording).toHaveBeenCalledWith(expect.objectContaining({ action: "plain", customPrompt: null, typeAtCursor: false }));
    await fireEvent.click(view.getByRole("button", { name: /Copy/ }));
    expect(api.copyText).toHaveBeenLastCalledWith(snapshot.text);
  });

  it("rejects remote starts on unsupported platforms without breaking batch mode", async () => {
    useLive();
    vi.mocked(api.liveSupport).mockResolvedValue({ supported: false, explanation: "Live dictation is not supported on macOS yet." });
    const view = await renderExpanded();
    await remoteIntent("start");
    await waitFor(() => expect(view.queryAllByText(/not supported on macOS/).length).toBeGreaterThan(0));
    expect(api.startRecording).not.toHaveBeenCalled();
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Transcription");
    expect(view.getByRole("dialog").textContent).toContain("not supported on macOS");
    await fireEvent.click(within(view.getByRole("dialog")).getByRole("button", { name: "Cloud" }));
    await fireEvent.click(view.getByRole("button", { name: "Close settings" }));
    await waitFor(() => expect(view.queryByRole("combobox", { name: "Action" })).not.toBeNull());
  });

  it("tells a Mac user about the Accessibility grant where typing is configured", async () => {
    // macOS drops typed keys from a process without the grant and says
    // nothing, so Settings has to say it before the first delivery fails.
    vi.mocked(api.typingSupport).mockResolvedValue({ supported: true, explanation: "macOS lets Utterform type into other windows only with Accessibility." });
    const view = await renderExpanded();
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await fireEvent.click(view.getByRole("tab", { name: /Output/ }));
    await waitFor(() => expect(view.queryAllByText(/only with Accessibility/).length).toBeGreaterThan(0));
  });

  it("keeps cancelled live text available for recovery without pretending to remove inserted text", async () => {
    useLive();
    const view = await renderExpanded();
    await remoteIntent("start");
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await remoteIntent("cancel");
    await waitFor(() => expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(snapshot.text));
    await waitFor(() => expect(view.queryByText(/Text already inserted is unchanged/)).not.toBeNull());
    expect(view.queryByText(/Copy includes the full transcript/)).not.toBeNull();
    expect(api.finishRecording).not.toHaveBeenCalled();
  });

  it("finalizes a failed stream from the native event, including when hidden", async () => {
    useLive();
    const view = await renderExpanded();
    await remoteIntent("start");
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    listeners.get("live-failed")!({ payload: "Connection lost" });
    await waitFor(() => expect(api.finishRecording).toHaveBeenCalledOnce());
    await waitFor(() => expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(snapshot.text));
    vi.restoreAllMocks();
  });

  it("retains an early failure while start IPC is pending in a hidden WebView", async () => {
    useLive();
    let resolveStart!: () => void;
    vi.mocked(api.startRecording).mockImplementationOnce(() => new Promise<void>((resolve) => { resolveStart = resolve; }));
    const view = await renderExpanded();
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    try {
      await remoteIntent("start");
      await waitFor(() => expect(api.startRecording).toHaveBeenCalledOnce());
      listeners.get("live-failed")!({ payload: "Connection lost before start returned" });
      expect(api.finishRecording).not.toHaveBeenCalled();
      resolveStart();
      await waitFor(() => expect(api.finishRecording).toHaveBeenCalledOnce());
      await waitFor(() => expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(snapshot.text));
      expect(api.getRecordingStatus).not.toHaveBeenCalled();
    } finally {
      hidden.mockRestore();
    }
  });

  it("recovers the last live transcript when finishing fails", async () => {
    useLive();
    vi.mocked(api.finishRecording).mockRejectedValueOnce(new Error("Connection lost"));
    const view = await renderExpanded();
    await remoteIntent("start");
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await remoteIntent("stop");
    await waitFor(() => expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(snapshot.text));
    await waitFor(() => expect(view.queryByText("Connection lost")).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: /Copy/ }));
    expect(api.copyText).toHaveBeenLastCalledWith(snapshot.text);
  });
});

describe("a recording whose transcription failed", () => {
  const kept = { count: 1, durationMs: 42_000 };
  const failedText = "OpenAI transcription request failed (ref 0a1b2c3d-2) The recording is kept, so you can try again.";

  async function failOneRecording() {
    vi.mocked(api.finishRecording).mockRejectedValueOnce(failedText);
    vi.mocked(api.retainedRecording).mockResolvedValue(kept);
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Stop recording" }));
    await waitFor(() => expect(view.queryByRole("group", { name: "Kept recording" })).not.toBeNull());
    return view;
  }

  beforeEach(() => {
    vi.mocked(api.retainedRecording).mockReset().mockResolvedValue(null);
    vi.mocked(api.retryRecording).mockReset();
    vi.mocked(api.discardRecording).mockReset().mockResolvedValue(undefined);
    vi.mocked(api.notifyIfUnattended).mockClear();
  });

  it("offers to try again and tells a user who is not looking", async () => {
    const view = await failOneRecording();
    expect(view.getByRole("group", { name: "Kept recording" }).textContent).toContain("00:42");
    expect(api.notifyIfUnattended).toHaveBeenCalledWith(failedText, true);
  });

  it("delivers the text once the retry succeeds and stops offering it", async () => {
    const view = await failOneRecording();
    vi.mocked(api.retainedRecording).mockResolvedValue(null);
    vi.mocked(api.retryRecording).mockResolvedValueOnce({ historyEntry: latest, text: latest.text, savedPath: null, copiedToClipboard: true, typedAtCursor: false, deliveryWarnings: [], durationMs: 42_000, engine: "open_ai" });
    await fireEvent.click(view.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(view.queryByRole("group", { name: "Kept recording" })).toBeNull());
    expect(view.queryByText("Copied to clipboard")).not.toBeNull();
  });

  it("keeps offering it when the retry fails too", async () => {
    const view = await failOneRecording();
    vi.mocked(api.retryRecording).mockRejectedValueOnce("Still offline (ref 0a1b2c3d-3)");
    await fireEvent.click(view.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(view.queryByText("Still offline (ref 0a1b2c3d-3)")).not.toBeNull());
    expect(view.queryByRole("group", { name: "Kept recording" })).not.toBeNull();
  });

  it("clears the failure when the user gives the recording up", async () => {
    const view = await failOneRecording();
    vi.mocked(api.retainedRecording).mockResolvedValue(null);
    await fireEvent.click(view.getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(view.queryByRole("group", { name: "Kept recording" })).toBeNull());
    expect(api.discardRecording).toHaveBeenCalledOnce();
    expect(view.queryByText(failedText)).toBeNull();
  });

  it("tells a user who is not looking when delivery only partly worked", async () => {
    vi.mocked(api.finishRecording).mockResolvedValueOnce({ historyEntry: latest, text: latest.text, savedPath: null, copiedToClipboard: true, typedAtCursor: false, deliveryWarnings: ["The microphone stopped early."], durationMs: 2400, engine: "open_ai" });
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Stop recording" }));
    await waitFor(() => expect(api.notifyIfUnattended).toHaveBeenCalledWith("The microphone stopped early.", false));
  });

  it("leaves the decision about a missing key to the backend, which can actually read it", async () => {
    vi.mocked(api.hasOpenAiApiKey).mockResolvedValueOnce(false);
    const started = vi.mocked(api.startRecording).mock.calls.length;
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(vi.mocked(api.startRecording).mock.calls.length).toBe(started + 1));
  });
});

describe("the transcription selector in the main window", () => {
  const base = { id: "base", name: "Whisper Base", description: "", sizeBytes: 1, downloaded: true };
  const selector = (view: Screen) => view.getByRole("combobox", { name: "Transcription" });
  async function choose(view: Screen, label: RegExp) {
    await fireEvent.click(selector(view));
    await fireEvent.click(view.getByRole("option", { name: label }));
  }
  /// Rendered with the stored settings in place — history arrives after them.
  async function renderReady() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    return view;
  }
  const finished = { historyEntry: latest, text: latest.text, durationMs: 1000, engine: "open_ai" as const, savedPath: null, copiedToClipboard: true, typedAtCursor: false, deliveryWarnings: [] };

  it.each([
    ["GPT Live Transcribe", { engine: "open_ai", cloud_model: "gpt_live_transcribe" }, DEFAULT_SETTINGS],
    // Local Whisper keeps the cloud model that was chosen before it.
    ["Local Whisper", { engine: "local_whisper", cloud_model: "gpt_live_transcribe" }, { ...DEFAULT_SETTINGS, cloud_model: "gpt_live_transcribe" }],
    ["GPT Transcribe", { engine: "open_ai", cloud_model: "gpt_transcribe" }, { ...DEFAULT_SETTINGS, engine: "local_whisper" }],
  ] as const)("writes %s at once as engine and cloud model, and nothing else", async (label, expected, stored) => {
    vi.mocked(api.getSettings).mockResolvedValue(structuredClone(stored) as typeof DEFAULT_SETTINGS);
    const view = await renderReady();
    await choose(view, new RegExp(`^${label} `));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledOnce());
    expect(api.saveSettings).toHaveBeenCalledWith({ ...stored, ...expected });
    expect(view.queryByRole("dialog")).toBeNull();
    expect(selector(view).textContent).toContain(label);
  });

  it("names a failed save, returns to the confirmed choice, and lets the next attempt through", async () => {
    vi.mocked(api.saveSettings).mockRejectedValueOnce(new Error("Disk full"));
    const view = await renderReady();
    await choose(view, /^Local Whisper /);
    await waitFor(() => expect(view.queryByText(/could not be saved.*Disk full/)).not.toBeNull());
    expect(selector(view).textContent).toContain("GPT Transcribe");
    expect(selector(view).textContent).not.toContain("Local Whisper");
    // Settings shows what the backend confirmed, not what was asked for.
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Transcription");
    expect(within(view.getByRole("dialog")).getByRole("button", { name: "Cloud" }).getAttribute("aria-pressed")).toBe("true");
    await fireEvent.click(view.getByRole("button", { name: "Close settings" }));
    await choose(view, /^Local Whisper /);
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledTimes(2));
    expect(selector(view).textContent).toContain("Local Whisper");
    expect(view.queryByText(/could not be saved/)).toBeNull();
  });

  it("keeps an output toggled during the write when the write fails", async () => {
    let fail!: (error: Error) => void;
    vi.mocked(api.saveSettings).mockImplementationOnce(() => new Promise<void>((_, reject) => fail = reject));
    const view = await renderReady();
    await choose(view, /^Local Whisper /);
    expect((selector(view) as HTMLButtonElement).disabled).toBe(true);
    // Settings waits too: a snapshot taken now could outlive the rollback.
    expect(view.getByRole("button", { name: "Open settings" }).hasAttribute("disabled")).toBe(true);
    await fireEvent.click(view.getByRole("button", { name: /^File/ }));
    fail(new Error("Disk full"));
    await waitFor(() => expect(selector(view).textContent).toContain("GPT Transcribe"));
    expect((selector(view) as HTMLButtonElement).disabled).toBe(false);
    expect(view.getByRole("button", { name: /^File/ }).getAttribute("aria-pressed")).toBe("true");
  });

  it("shows the same choice as Settings, in both directions", async () => {
    const view = await renderReady();
    await choose(view, /^Local Whisper /);
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledOnce());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Transcription");
    const engine = (name: string) => within(view.getByRole("dialog")).getByRole("button", { name });
    expect(engine("Local").getAttribute("aria-pressed")).toBe("true");
    await fireEvent.click(engine("Cloud"));
    await fireEvent.click(engine("Live"));
    await fireEvent.click(view.getByRole("button", { name: "Close settings" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(selector(view).textContent).toContain("GPT Live Transcribe");
    expect(api.saveSettings).toHaveBeenLastCalledWith(expect.objectContaining({ engine: "open_ai", cloud_model: "gpt_live_transcribe" }));
  });

  it("names the local model, or its absence, without offering to choose it here", async () => {
    vi.mocked(api.listLocalModels).mockResolvedValue([base, { ...base, id: "small", name: "Whisper Small", downloaded: false }]);
    vi.mocked(api.getSettings).mockResolvedValue({ ...structuredClone(DEFAULT_SETTINGS), engine: "local_whisper" });
    const view = await renderReady();
    expect(selector(view).textContent?.trim()).toBe("Local Whisper");
    await fireEvent.click(selector(view));
    // Three ways to transcribe, and no model list: that stays in Settings.
    expect(view.getAllByRole("option").map((option) => option.textContent?.replace("✓", "").trim())).toEqual([
      "GPT TranscribeFinished text · Cloud",
      "GPT Live TranscribeLive at cursor · Windows and Omarchy",
      "Local WhisperOn device · Whisper Base",
    ]);
    await fireEvent.keyDown(selector(view), { key: "Escape" });
    view.unmount();

    vi.mocked(api.getSettings).mockResolvedValue({ ...structuredClone(DEFAULT_SETTINGS), engine: "local_whisper", local_model_id: "small" });
    const undownloaded = await renderReady();
    expect(selector(undownloaded).textContent).toContain("On device · Download Whisper Small in Settings");
    undownloaded.unmount();

    vi.mocked(api.getSettings).mockResolvedValue({ ...structuredClone(DEFAULT_SETTINGS), engine: "local_whisper", local_model_id: null });
    const unchosen = await renderReady();
    expect(selector(unchosen).textContent).toContain("On device · Choose a model in Settings");
  });

  it("switching to GPT Live keeps the action Plain and finishes without typing the text again", async () => {
    vi.mocked(api.getLiveStatus).mockResolvedValue({ text: "Live words", insertedText: "Live words", deliveryPaused: false, warning: null, phase: "streaming" });
    vi.mocked(api.finishRecording).mockResolvedValue({ ...finished, historyEntry: null, text: "Live words", copiedToClipboard: false, typedAtCursor: true });
    const view = await renderReady();
    await fireEvent.click(view.getByRole("combobox", { name: "Action" }));
    await fireEvent.click(view.getByRole("option", { name: /^Email/ }));
    await choose(view, /^GPT Live Transcribe /);
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledOnce());
    expect(view.queryByRole("combobox", { name: "Action" })).toBeNull();
    expect(view.container.querySelector(".engine-chip")?.textContent).toContain("Plain");
    expect(view.queryByText(/Place the cursor in your text field/)).not.toBeNull();
    // The window's own button only explains; the recording starts from the target field.
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    expect(api.startRecording).not.toHaveBeenCalled();
    await remoteIntent("start");
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledWith(null, "open_ai", "base", "plain"));
    await remoteIntent("stop");
    await waitFor(() => expect(api.finishRecording).toHaveBeenCalledOnce());
    expect(api.finishRecording).toHaveBeenCalledWith(expect.objectContaining({ action: "plain", typeAtCursor: false }));
  });

  it("says where GPT Live does not run, and turns a remote start away there", async () => {
    vi.mocked(api.liveSupport).mockResolvedValue({ supported: false, explanation: "Live Dictation in 0.6.0 supports Windows and Omarchy/Hyprland only. Use GPT Transcribe here; macOS Live is planned separately." });
    const view = await renderReady();
    await choose(view, /^GPT Live Transcribe /);
    await waitFor(() => expect(view.queryAllByText(/supports Windows and Omarchy\/Hyprland only/).length).toBeGreaterThan(0));
    await remoteIntent("start");
    expect(api.startRecording).not.toHaveBeenCalled();
    // The hint does not pretend otherwise for the platform it is shown on.
    await fireEvent.click(selector(view));
    expect(view.getByRole("option", { name: /^GPT Live Transcribe / }).textContent).toContain("Windows and Omarchy");
  });

  it("is locked while a recording is starting, running, paused and processing", async () => {
    let started!: () => void;
    let processed!: (value: typeof finished) => void;
    vi.mocked(api.startRecording).mockImplementationOnce(() => new Promise<void>((done) => started = done));
    vi.mocked(api.finishRecording).mockImplementationOnce(() => new Promise((done) => processed = done));
    const view = await renderReady();
    const locked = () => (selector(view) as HTMLButtonElement).disabled;
    expect(locked()).toBe(false);
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledOnce());
    expect(locked()).toBe(true);
    started();
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    expect(locked()).toBe(true);
    await fireEvent.click(view.getByRole("button", { name: "Pause recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Resume recording" })).not.toBeNull());
    expect(locked()).toBe(true);
    await fireEvent.click(view.getByRole("button", { name: "Stop recording" }));
    await waitFor(() => expect(api.finishRecording).toHaveBeenCalledOnce());
    expect(locked()).toBe(true);
    // A locked selector cannot be opened from the keyboard either.
    await fireEvent.keyDown(selector(view), { key: "ArrowDown" });
    expect(view.queryByRole("listbox")).toBeNull();
    processed(finished);
    await waitFor(() => expect(locked()).toBe(false));
    expect(api.saveSettings).toHaveBeenCalledOnce(); // the recording's own save, nothing from the selector
  });

  it("is driven from the keyboard without leaking keys into the window's shortcuts", async () => {
    const view = await renderReady();
    const trigger = selector(view);
    trigger.focus();
    await fireEvent.keyDown(trigger, { key: "ArrowDown" });
    expect(view.getByRole("listbox", { name: "Transcription" })).toBeTruthy();
    await fireEvent.keyDown(trigger, { key: "ArrowDown" });
    await fireEvent.keyDown(trigger, { key: "Enter" });
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ engine: "open_ai", cloud_model: "gpt_live_transcribe" })));
    expect(view.queryByRole("listbox")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    // Typeahead reaches Local Whisper; "f" here must not toggle the file output.
    await fireEvent.keyDown(trigger, { key: "l" });
    await fireEvent.keyDown(trigger, { key: "Enter" });
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(expect.objectContaining({ engine: "local_whisper" })));
    await fireEvent.keyDown(trigger, { key: "f" });
    await fireEvent.keyDown(trigger, { key: "Escape" });
    expect(view.getByRole("button", { name: /^File/ }).getAttribute("aria-pressed")).toBe("false");
    expect(api.saveSettings).toHaveBeenCalledTimes(2);
  });

  it("a recording started while the choice is being written waits for it, then runs with it", async () => {
    let saved!: () => void;
    vi.mocked(api.listLocalModels).mockResolvedValue([base]);
    vi.mocked(api.saveSettings).mockImplementationOnce(() => new Promise<void>((done) => saved = done));
    const view = await renderReady();
    await choose(view, /^Local Whisper /);
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByText("Preparing the microphone…")).not.toBeNull());
    // Turned away as long as the choice is not confirmed; a second press meanwhile is too.
    expect(api.startRecording).not.toHaveBeenCalled();
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    saved();
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledOnce());
    expect(api.startRecording).toHaveBeenCalledWith(null, "local_whisper", "base", "plain");
    expect(api.saveSettings).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
  });
});

// Once `finish_recording` has answered, Utterform takes the next dictation at
// once; the Done cue and the ribbon's release run on without holding it. What
// arrives while the transcript is still being processed is dropped, never
// queued: a key pressed then must not start a recording seconds later.
describe("the ready boundary after processing", () => {
  const finished = { historyEntry: null, text: "Dictated", durationMs: 900, engine: "open_ai" as const, savedPath: null, copiedToClipboard: true, typedAtCursor: false, deliveryWarnings: [] };

  async function recordUntilProcessing() {
    let complete!: (value: typeof finished) => void;
    let fail!: (error: Error) => void;
    vi.mocked(api.finishRecording).mockImplementationOnce(() => new Promise((done, reject) => { complete = done; fail = reject; }));
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Start recording" })).not.toBeNull());
    await remoteIntent("toggle");
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await remoteIntent("toggle");
    await waitFor(() => expect(api.finishRecording).toHaveBeenCalledOnce());
    expect(view.getByText("Processing", { selector: ".record-status" })).toBeTruthy();
    return { view, complete: () => complete(finished), fail: () => fail(new Error("Transcription failed")) };
  }

  it("drops a toggle that arrives during processing, and never replays it", async () => {
    const { view, complete } = await recordUntilProcessing();
    await remoteIntent("toggle");
    await remoteIntent("start");
    await fireEvent.keyDown(window, { code: "Space" });
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    expect(api.startRecording).toHaveBeenCalledOnce();
    complete();
    await waitFor(() => expect(view.queryByText("Copied to clipboard")).not.toBeNull());
    // Ready, and nothing pending: the dropped presses do not start anything now.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(api.startRecording).toHaveBeenCalledOnce();
    expect(api.finishRecording).toHaveBeenCalledOnce();
    expect(view.getByRole("button", { name: "Start recording" }).hasAttribute("disabled")).toBe(false);
  });

  it("starts exactly one recording on the first toggle after the answer", async () => {
    const { view, complete } = await recordUntilProcessing();
    complete();
    await waitFor(() => expect(view.queryByText("Copied to clipboard")).not.toBeNull());
    await remoteIntent("toggle");
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    expect(api.startRecording).toHaveBeenCalledTimes(2);
    expect(api.finishRecording).toHaveBeenCalledOnce();
  });

  it("takes a burst of presses at the boundary as one start, not a start and a stop", async () => {
    let started!: () => void;
    const { view, complete } = await recordUntilProcessing();
    complete();
    await waitFor(() => expect(view.queryByText("Copied to clipboard")).not.toBeNull());
    vi.mocked(api.startRecording).mockImplementationOnce(() => new Promise<void>((done) => started = done));
    // A bouncing key, a double click, and a hotkey all inside the same moment.
    listeners.get("remote-intent")!({ payload: "toggle" });
    listeners.get("remote-intent")!({ payload: "toggle" });
    listeners.get("remote-intent")!({ payload: "start" });
    await fireEvent.keyDown(window, { code: "Space" });
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    // The key press first asks which app is in front; the start follows.
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledTimes(2));
    started();
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    expect(api.startRecording).toHaveBeenCalledTimes(2);
    expect(api.finishRecording).toHaveBeenCalledOnce();
  });

  it("stays startable after a failed completion", async () => {
    const { view, fail } = await recordUntilProcessing();
    fail();
    await waitFor(() => expect(view.queryByText("Transcription failed")).not.toBeNull());
    await remoteIntent("toggle");
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    expect(api.startRecording).toHaveBeenCalledTimes(2);
  });

  it("does not let a toggle stop a recording that is still starting", async () => {
    let started!: () => void;
    vi.mocked(api.startRecording).mockImplementationOnce(() => new Promise<void>((done) => started = done));
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Start recording" })).not.toBeNull());
    await remoteIntent("toggle");
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledOnce());
    await remoteIntent("toggle");
    await remoteIntent("start");
    started();
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    expect(api.finishRecording).not.toHaveBeenCalled();
    expect(api.startRecording).toHaveBeenCalledOnce();
  });

  // Omarchy's push-to-talk sends `--stop` the moment Right Alt is let go. A
  // short press releases it while the microphone opens and the start cue
  // plays; dropping that stop would leave the recording running.
  it("keeps a push-to-talk stop that arrives while the recording is starting", async () => {
    let started!: () => void;
    vi.mocked(api.startRecording).mockImplementationOnce(() => new Promise<void>((done) => started = done));
    vi.mocked(api.finishRecording).mockResolvedValue({ historyEntry: null, text: "Short", savedPath: null,
      copiedToClipboard: true, typedAtCursor: true, deliveryWarnings: [], durationMs: 400, engine: "open_ai" });
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Start recording" })).not.toBeNull());
    await remoteIntent("start");
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledOnce());
    await remoteIntent("stop");
    expect(api.finishRecording).not.toHaveBeenCalled();
    started();
    await waitFor(() => expect(api.finishRecording).toHaveBeenCalledOnce());
    await waitFor(() => expect(view.queryByText("Typed at the cursor · Copied to clipboard")).not.toBeNull());
  });

  it("forgets a stop kept for a start that failed", async () => {
    vi.mocked(api.startRecording).mockImplementationOnce(() => new Promise<void>((_, reject) => setTimeout(() => reject("No microphone"), 0)));
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Start recording" })).not.toBeNull());
    await remoteIntent("start");
    await remoteIntent("stop");
    await waitFor(() => expect(view.queryByText(/No microphone/)).not.toBeNull());
    await remoteIntent("start");
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    expect(api.startRecording).toHaveBeenCalledTimes(2);
    expect(api.finishRecording).not.toHaveBeenCalled();
  });
});

// Support actions are immediate resource operations, like a model download:
// they neither take part in the settings draft nor close the dialog.
describe("support and diagnostics", () => {
  /// The log path and the privacy note are a tooltip on the row, not text in front.
  function diagnosticsRow(view: Screen) {
    return view.getByRole("button", { name: "Copy diagnostics" }).closest(".setting-row")!;
  }

  async function openGeneral() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "App");
    await waitFor(() => expect(diagnosticsRow(view).getAttribute("title")).toContain(LOG_PATH));
    return view;
  }

  it("ends App with the diagnostics row and the path the backend reported", async () => {
    const view = await openGeneral();
    const rows = view.getByRole("tabpanel").querySelectorAll(".setting-row");
    expect(rows[rows.length - 1]).toBe(diagnosticsRow(view));
    expect(diagnosticsRow(view).getAttribute("title")).toMatch(/contain no transcripts or audio/);
    for (const name of ["Copy diagnostics", "Open log file", "Open log folder"]) {
      expect((view.getByRole("button", { name }) as HTMLButtonElement).disabled).toBe(false);
    }
  });

  it("runs each action at once and never saves settings", async () => {
    const view = await openGeneral();
    await fireEvent.click(view.getByRole("button", { name: "Copy diagnostics" }));
    await waitFor(() => expect(view.queryByText("Diagnostics copied · 12.4 KB · 318 lines")?.getAttribute("role")).toBe("status"));
    await fireEvent.click(view.getByRole("button", { name: "Open log file" }));
    await waitFor(() => expect(view.queryByText("Log file opened")).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open log folder" }));
    await waitFor(() => expect(view.queryByText("Log folder opened")).not.toBeNull());
    expect(api.copyDiagnostics).toHaveBeenCalledOnce();
    expect(api.openLogFile).toHaveBeenCalledOnce();
    expect(api.openLogFolder).toHaveBeenCalledOnce();
    // No path ever travels from the interface to the backend.
    for (const call of [api.copyDiagnostics, api.openLogFile, api.openLogFolder]) {
      expect(vi.mocked(call).mock.calls[0]).toEqual([]);
    }
    expect(view.queryByRole("dialog")).not.toBeNull();
    await fireEvent.click(view.getByRole("button", { name: "Close settings" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.saveSettings).not.toHaveBeenCalled();
    expect(api.copyDiagnostics).toHaveBeenCalledOnce();
  });

  it("disables only the action in flight and keeps a failure in front of the user", async () => {
    let finishCopy!: (value: { bytes: number; lines: number; truncated: boolean }) => void;
    vi.mocked(api.copyDiagnostics).mockImplementationOnce(() => new Promise((done) => finishCopy = done));
    vi.mocked(api.openLogFolder).mockRejectedValueOnce("The log folder could not be opened (ref 0a1b2c3d-4)");
    const view = await openGeneral();
    const copy = view.getByRole("button", { name: "Copy diagnostics" }) as HTMLButtonElement;
    await fireEvent.click(copy);
    expect(copy.disabled).toBe(true);
    expect(copy.getAttribute("aria-busy")).toBe("true");
    expect((view.getByRole("button", { name: "Open log file" }) as HTMLButtonElement).disabled).toBe(false);
    await fireEvent.click(copy);
    expect(api.copyDiagnostics).toHaveBeenCalledOnce();
    await fireEvent.click(view.getByRole("button", { name: "Open log folder" }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toBe("The log folder could not be opened (ref 0a1b2c3d-4)"));
    // The path stays at hand for opening it by hand.
    expect(diagnosticsRow(view).getAttribute("title")).toContain(LOG_PATH);
    finishCopy({ bytes: 900, lines: 1, truncated: true });
    await waitFor(() => expect(copy.disabled).toBe(false));
    expect(view.queryByText("Diagnostics copied · 900 bytes · 1 line · newest lines only")).not.toBeNull();
    expect(view.queryByRole("dialog")).not.toBeNull();
    expect(api.saveSettings).not.toHaveBeenCalled();
  });

  it("says when no log file could be opened in this session", async () => {
    vi.mocked(api.diagnosticsInfo).mockResolvedValue({ logPath: null });
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "App");
    await waitFor(() => expect(api.diagnosticsInfo).toHaveBeenCalled());
    expect(diagnosticsRow(view).getAttribute("title")).not.toContain("/");
  });

  it("leaves the sound test and its report in Recording, without the log path", async () => {
    const view = await openGeneral();
    await showSettingsTab(view, "Recording");
    expect(view.getByRole("dialog").innerHTML).not.toContain(LOG_PATH);
    expect(view.queryByRole("button", { name: "Copy diagnostics" })).toBeNull();
    await fireEvent.click(view.getByRole("button", { name: "Test" }));
    expect(api.playTestCues).toHaveBeenCalledWith(5);
    await waitFor(() => expect(view.queryByRole("button", { name: "In 5 s…" })).not.toBeNull());
    listeners.get("test-cues-finished")!({ payload: "Start: played\nStop: played\nDone: played" });
    await waitFor(() => expect(view.container.querySelector(".cue-report")?.textContent).toContain("Done: played"));
  });
});

describe("failures only the interface sees", () => {
  function rejection(reason: unknown) {
    const event = new Event("unhandledrejection") as PromiseRejectionEvent;
    Object.defineProperty(event, "reason", { value: reason });
    return event;
  }

  it("reports a global error and an unhandled rejection once each, and stops listening when destroyed", async () => {
    // Even a log that cannot be written must not become the next rejection.
    vi.mocked(api.reportFrontendError).mockRejectedValue("logging is unavailable");
    const added = vi.spyOn(window, "addEventListener");
    const removed = vi.spyOn(window, "removeEventListener");
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    window.dispatchEvent(new ErrorEvent("error", { error: new TypeError("state is undefined"), message: "state is undefined" }));
    await waitFor(() => expect(api.reportFrontendError).toHaveBeenCalledOnce());
    expect(api.reportFrontendError).toHaveBeenLastCalledWith(expect.objectContaining({
      source: "window.error", message: "TypeError: state is undefined", phase: "idle",
    }));
    window.dispatchEvent(rejection({ settings: { vocabulary: ["CANARY"] } }));
    await waitFor(() => expect(api.reportFrontendError).toHaveBeenCalledTimes(2));
    const sent = vi.mocked(api.reportFrontendError).mock.calls[1][0];
    expect(sent).toEqual({ source: "unhandled_rejection", message: "Non-error value of type object", stack: null, phase: "idle" });
    expect(JSON.stringify(sent)).not.toContain("CANARY");
    view.unmount();
    // The very handlers that were added are the ones removed.
    for (const type of ["error", "unhandledrejection"]) {
      const handler = added.mock.calls.find(([name]) => name === type)?.[1];
      expect(handler).toBeTypeOf("function");
      expect(removed.mock.calls.some(([name, fn]) => name === type && fn === handler)).toBe(true);
    }
    window.dispatchEvent(rejection(new Error("after unmount")));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(api.reportFrontendError).toHaveBeenCalledTimes(2);
    added.mockRestore();
    removed.mockRestore();
  });

  it("gives an unexpected interface failure the reference of its log event", async () => {
    vi.mocked(api.reportFrontendError).mockResolvedValue("0a1b2c3d-7");
    vi.mocked(api.finishRecording).mockRejectedValueOnce(new TypeError("result is undefined"));
    // Ready once settings, the key and history have arrived.
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Stop recording" }));
    await waitFor(() => expect(view.queryByText("result is undefined (ref 0a1b2c3d-7)")).not.toBeNull());
    expect(api.reportFrontendError).toHaveBeenCalledWith(expect.objectContaining({ source: "state", phase: "error" }));
  });

  it("does not report a command's own failure a second time", async () => {
    vi.mocked(api.finishRecording).mockRejectedValueOnce("OpenAI transcription request failed (ref 0a1b2c3d-2)");
    // Ready once settings, the key and history have arrived.
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Stop recording" }));
    await waitFor(() => expect(view.queryByText("OpenAI transcription request failed (ref 0a1b2c3d-2)")).not.toBeNull());
    expect(api.reportFrontendError).not.toHaveBeenCalled();
  });

  it("reports a folder picker that failed to open, instead of leaving an unhandled rejection", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockRejectedValueOnce("the portal is unavailable");
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Output");
    await fireEvent.click(view.getByRole("button", { name: "Choose" }));
    await waitFor(() => expect(api.reportFrontendError).toHaveBeenCalledWith({
      source: "dialog", message: "the portal is unavailable", stack: null, phase: "idle",
    }));
    expect(view.queryByRole("dialog")).not.toBeNull();
  });

  it("reports a startup entry the system would not read, with its reference", async () => {
    vi.mocked(api.reportFrontendError).mockResolvedValue("0a1b2c3d-9");
    vi.mocked(api.autostartEnabled).mockRejectedValue("no startup items on this system");
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "App");
    await waitFor(() => expect(view.getByRole("alert").textContent).toBe(
      "Utterform could not read your startup settings: no startup items on this system (ref 0a1b2c3d-9)",
    ));
    expect(api.reportFrontendError).toHaveBeenCalledWith({
      source: "autostart", message: "no startup items on this system", stack: null, phase: "idle",
    });
  });
});

describe("an audio file", () => {
  const fromFile = { historyEntry: { ...latest, id: "3", text: "Bring the blue folder on Monday." }, text: "Bring the blue folder on Monday.", savedPath: null, copiedToClipboard: true, typedAtCursor: false, deliveryWarnings: [], durationMs: 63_000, engine: "open_ai" as const };
  const voiceMessage = "/home/tester/Downloads/PTT-20261107-WA0003.opus";

  beforeEach(() => {
    vi.mocked(api.transcribeFile).mockReset().mockResolvedValue(fromFile);
    vi.mocked(api.takePendingFile).mockReset().mockResolvedValue(null);
    vi.mocked(open).mockReset();
  });

  async function ready() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: /Latest text/ })).not.toBeNull());
    await waitFor(() => expect(listeners.has("tauri://drag-drop")).toBe(true));
    return view;
  }

  it("opens from the button and goes to history and the clipboard, never the cursor", async () => {
    vi.mocked(open).mockResolvedValueOnce(voiceMessage);
    const view = await ready();
    await fireEvent.click(view.getByRole("button", { name: "Open audio file" }));
    await waitFor(() => expect(api.transcribeFile).toHaveBeenCalledTimes(1));
    expect(vi.mocked(open).mock.calls[0][0]).toMatchObject({ multiple: false, directory: false });
    expect(vi.mocked(open).mock.calls[0][0]?.filters?.[0].extensions).toContain("opus");
    const [path, source, request] = vi.mocked(api.transcribeFile).mock.calls[0];
    expect(path).toBe(voiceMessage);
    expect(source).toBe("open");
    expect(request).toMatchObject({ action: "plain", copyToClipboard: true, typeAtCursor: false });
    await waitFor(() => expect(view.queryByText("Copied to clipboard")).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: /Latest text/ }));
    expect(view.getByRole("region", { name: "Transcript" }).textContent).toContain("blue folder");
  });

  it("opens with the O key, and a cancelled dialog changes nothing", async () => {
    vi.mocked(open).mockResolvedValueOnce(null);
    const view = await ready();
    await fireEvent.keyDown(window, { key: "o", code: "KeyO" });
    await waitFor(() => expect(open).toHaveBeenCalledTimes(1));
    expect(api.transcribeFile).not.toHaveBeenCalled();
    expect(view.queryByText("Attention")).toBeNull();
  });

  it("applies the chosen action", async () => {
    vi.mocked(open).mockResolvedValueOnce(voiceMessage);
    const view = await ready();
    await fireEvent.keyDown(window, { key: "2" });
    await fireEvent.click(view.getByRole("button", { name: "Open audio file" }));
    await waitFor(() => expect(api.transcribeFile).toHaveBeenCalledTimes(1));
    expect(vi.mocked(api.transcribeFile).mock.calls[0][2].action).toBe("clean");
  });

  it("shows where a dragged file will go and transcribes it when dropped", async () => {
    const view = await ready();
    listeners.get("tauri://drag-enter")!({ payload: { paths: [voiceMessage], position: { x: 1, y: 1 } } });
    await waitFor(() => expect(view.queryByText("Drop to transcribe")).not.toBeNull());
    expect(view.getByText(/Plain · GPT Transcribe → Clipboard/)).not.toBeNull();
    listeners.get("tauri://drag-drop")!({ payload: { paths: [voiceMessage], position: { x: 1, y: 1 } } });
    await waitFor(() => expect(api.transcribeFile).toHaveBeenCalledWith(voiceMessage, "drop", expect.anything()));
    expect(view.queryByText("Drop to transcribe")).toBeNull();
  });

  it("lets a drag that leaves again do nothing", async () => {
    const view = await ready();
    listeners.get("tauri://drag-enter")!({ payload: { paths: [voiceMessage], position: { x: 1, y: 1 } } });
    await waitFor(() => expect(view.queryByText("Drop to transcribe")).not.toBeNull());
    listeners.get("tauri://drag-leave")!({ payload: null });
    await waitFor(() => expect(view.queryByText("Drop to transcribe")).toBeNull());
    expect(api.transcribeFile).not.toHaveBeenCalled();
  });

  it("takes no file while a recording runs", async () => {
    const view = await ready();
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    expect((view.getByRole("button", { name: "Open audio file" }) as HTMLButtonElement).disabled).toBe(true);
    listeners.get("tauri://drag-enter")!({ payload: { paths: [voiceMessage], position: { x: 1, y: 1 } } });
    listeners.get("tauri://drag-drop")!({ payload: { paths: [voiceMessage], position: { x: 1, y: 1 } } });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(view.queryByText("Drop to transcribe")).toBeNull();
    expect(api.transcribeFile).not.toHaveBeenCalled();
  });

  it("works through files handed in from outside, one after another", async () => {
    const queue = ["/tmp/one.opus", "/tmp/two.m4a"];
    vi.mocked(api.takePendingFile).mockImplementation(async () => queue.shift() ?? null);
    await ready();
    await waitFor(() => expect(api.transcribeFile).toHaveBeenCalledTimes(2));
    expect(vi.mocked(api.transcribeFile).mock.calls.map((call) => [call[0], call[1]])).toEqual([
      ["/tmp/one.opus", "command_line"],
      ["/tmp/two.m4a", "command_line"],
    ]);
    queue.push("/tmp/three.ogg");
    listeners.get("file-waiting")!({ payload: null });
    await waitFor(() => expect(api.transcribeFile).toHaveBeenCalledTimes(3));
  });

  it("says why a file could not be read", async () => {
    vi.mocked(open).mockResolvedValueOnce("/tmp/notes.txt");
    vi.mocked(api.transcribeFile).mockRejectedValueOnce("Utterform cannot read this file. (ref 0a1b2c3d-9)");
    const view = await ready();
    await fireEvent.click(view.getByRole("button", { name: "Open audio file" }));
    await waitFor(() => expect(view.queryByText(/cannot read this file/)).not.toBeNull());
    expect(api.notifyIfUnattended).toHaveBeenCalledWith("Utterform cannot read this file. (ref 0a1b2c3d-9)", true);
  });
});

describe("first-run assistant", () => {
  const MODELS = [
    { id: "tiny", name: "Whisper Tiny", description: "Fastest", sizeBytes: 75 * 1024 * 1024, downloaded: false },
    { id: "base", name: "Whisper Base", description: "Balanced", sizeBytes: 142 * 1024 * 1024, downloaded: false },
  ];

  beforeEach(() => {
    vi.mocked(api.getSettings).mockResolvedValue({ ...structuredClone(DEFAULT_SETTINGS), onboarding_completed: false });
    vi.mocked(api.hasOpenAiApiKey).mockResolvedValue(false);
    vi.mocked(api.listLocalModels).mockResolvedValue(structuredClone(MODELS));
    vi.mocked(api.completeOnboarding).mockReset().mockResolvedValue(undefined);
    vi.mocked(api.setOpenAiApiKey).mockReset().mockResolvedValue(undefined);
    vi.mocked(api.openOpenAiApiKeys).mockReset().mockResolvedValue(undefined);
    vi.mocked(api.downloadLocalModel).mockReset().mockImplementation(async (id: string) => {
      vi.mocked(api.listLocalModels).mockResolvedValue(MODELS.map((model) => ({ ...model, downloaded: model.id === id })));
    });
    Object.defineProperty(navigator, "languages", { value: ["de-CH", "en"], configurable: true });
  });
  afterEach(() => { Reflect.deleteProperty(navigator, "languages"); });

  async function assistant() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("dialog", { name: "How should Utterform transcribe?" })).not.toBeNull());
    return view;
  }

  /// Everything the assistant may only do on Finish.
  function expectNoSideEffects() {
    expect(api.setOpenAiApiKey).not.toHaveBeenCalled();
    expect(api.downloadLocalModel).not.toHaveBeenCalled();
    expect(api.applyGlobalHotkey).not.toHaveBeenCalled();
    expect(api.enableAutostart).not.toHaveBeenCalled();
    expect(api.disableAutostart).not.toHaveBeenCalled();
  }

  it("never appears for someone already set up", async () => {
    vi.mocked(api.getSettings).mockResolvedValue(structuredClone(DEFAULT_SETTINGS));
    const view = render(App);
    await waitFor(() => expect(api.takeStartupIntent).toHaveBeenCalled());
    expect(view.queryByText("How should Utterform transcribe?")).toBeNull();
  });

  it("skips in one click from any step, applying nothing it was told", async () => {
    const view = await assistant();
    await fireEvent.click(view.getByRole("radio", { name: /On this device/ }));
    await fireEvent.click(view.getByRole("button", { name: "Next" }));
    await fireEvent.click(view.getByRole("radio", { name: /Tiny/ }));
    await fireEvent.click(view.getByRole("button", { name: "Next" }));
    expect(view.queryByText("You can change all of this later in Settings.")).not.toBeNull();
    await fireEvent.click(view.getByRole("button", { name: "Skip setup" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.completeOnboarding).toHaveBeenCalledTimes(1);
    const [stored, skipped] = vi.mocked(api.completeOnboarding).mock.calls[0];
    expect(skipped).toBe(true);
    expect(stored).toEqual({ ...DEFAULT_SETTINGS, onboarding_completed: true });
    expect(api.saveSettings).not.toHaveBeenCalled();
    expectNoSideEffects();
  });

  it("needs a key for the cloud, stores it only on Finish, and ends in the main window", async () => {
    const view = await assistant();
    await fireEvent.click(view.getByRole("button", { name: "Next" }));
    const next = view.getByRole("button", { name: "Next" }) as HTMLButtonElement;
    expect(next.disabled).toBe(true);
    await fireEvent.input(view.getByLabelText("OpenAI key", { selector: "input" }), { target: { value: " sk-test " } });
    expect(next.disabled).toBe(false);
    await fireEvent.click(view.getByRole("button", { name: "Create key ↗" }));
    expect(api.openOpenAiApiKeys).toHaveBeenCalledWith();
    await fireEvent.click(next);
    // The system language is suggested, and a typo holds Next back.
    const language = view.getByLabelText("Language") as HTMLInputElement;
    expect(language.value).toBe("de");
    await fireEvent.input(language, { target: { value: "deutsch!" } });
    expect((view.getByRole("button", { name: "Next" }) as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.input(language, { target: { value: "de" } });
    await fireEvent.click(view.getByRole("button", { name: "Next" }));
    expect(view.queryByRole("switch", { name: "Use my ChatGPT subscription" })).not.toBeNull();
    expectNoSideEffects();

    await fireEvent.click(view.getByRole("button", { name: "Finish" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.setOpenAiApiKey).toHaveBeenCalledWith("sk-test");
    expect(vi.mocked(api.saveSettings).mock.calls[0][0]).toMatchObject({ onboarding_completed: false, engine: "open_ai", language_hints: ["de"] });
    const [stored, skipped] = vi.mocked(api.completeOnboarding).mock.calls[0];
    expect(skipped).toBe(false);
    expect(stored).toMatchObject({ onboarding_completed: true, engine: "open_ai", language_hints: ["de"], text_provider: "openai_key" });
    // Stored before setup counts as done.
    expect(vi.mocked(api.setOpenAiApiKey).mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(api.completeOnboarding).mock.invocationCallOrder[0]);
    expect(view.queryByText("Setup complete")).not.toBeNull();
    expect(api.downloadLocalModel).not.toHaveBeenCalled();
  });

  it("downloads the local model on Finish and keeps setup open when that fails", async () => {
    vi.mocked(api.downloadLocalModel).mockRejectedValueOnce("The download was interrupted.");
    const view = await assistant();
    await fireEvent.click(view.getByRole("radio", { name: /On this device/ }));
    await fireEvent.click(view.getByRole("button", { name: "Next" }));
    expect(view.getByRole("radio", { name: /Base/ }).getAttribute("aria-checked")).toBe("true");
    await fireEvent.click(view.getByRole("button", { name: "Next" }));
    await fireEvent.click(view.getByRole("button", { name: "Next" }));
    await fireEvent.click(view.getByRole("button", { name: "Finish" }));
    // Back where the model is chosen, with the reason, and still not done.
    await waitFor(() => expect(view.queryByText("The download was interrupted.")).not.toBeNull());
    expect(view.queryByRole("heading", { name: "Which model?" })).not.toBeNull();
    expect(api.completeOnboarding).not.toHaveBeenCalled();

    await fireEvent.click(view.getByRole("button", { name: "Next" }));
    await fireEvent.click(view.getByRole("button", { name: "Next" }));
    await fireEvent.click(view.getByRole("button", { name: "Finish" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.downloadLocalModel).toHaveBeenLastCalledWith("base");
    expect(vi.mocked(api.completeOnboarding).mock.calls[0][0]).toMatchObject({ engine: "local_whisper", local_model_id: "base" });
    expect(api.setOpenAiApiKey).not.toHaveBeenCalled();
  });

  it("offers ChatGPT only with a Codex login", async () => {
    vi.mocked(api.hasOpenAiApiKey).mockResolvedValue(true);
    vi.mocked(api.codexStatus).mockResolvedValue({ signedIn: true, models: [], defaultModel: null });
    const view = await assistant();
    for (let step = 0; step < 3; step += 1) await fireEvent.click(view.getByRole("button", { name: "Next" }));
    const chatGpt = view.getByRole("switch", { name: "Use my ChatGPT subscription" }) as HTMLInputElement;
    await waitFor(() => expect(chatGpt.disabled).toBe(false));
    await fireEvent.click(chatGpt);
    await fireEvent.click(view.getByRole("button", { name: "Finish" }));
    await waitFor(() => expect(api.completeOnboarding).toHaveBeenCalled());
    expect(vi.mocked(api.completeOnboarding).mock.calls[0][0]).toMatchObject({ text_provider: "codex" });
    expect(api.setOpenAiApiKey).not.toHaveBeenCalled();
  });

  it("applies a changed dictation key and start at login on Finish only", async () => {
    vi.mocked(api.hasOpenAiApiKey).mockResolvedValue(true);
    const view = await assistant();
    for (let step = 0; step < 3; step += 1) await fireEvent.click(view.getByRole("button", { name: "Next" }));
    await fireEvent.click(view.getByRole("switch", { name: "Dictation key" }));
    await fireEvent.click(view.getByRole("switch", { name: "Start Utterform when I sign in" }));
    expectNoSideEffects();
    await fireEvent.click(view.getByRole("button", { name: "Finish" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.applyGlobalHotkey).toHaveBeenCalledWith(null);
    expect(api.enableAutostart).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.completeOnboarding).mock.calls[0][0]).toMatchObject({ global_hotkey: null });
  });

  it("lets no dictation key or keyboard shortcut record beneath it", async () => {
    const view = await assistant();
    await remoteIntent("toggle");
    await fireEvent.keyDown(window, { code: "Space", key: " " });
    expect(api.startRecording).not.toHaveBeenCalled();
    expect(view.queryByRole("dialog")).not.toBeNull();
  });
});

describe("the tray's Microphone menu", () => {
  it("records the next dictation with the microphone chosen in the tray, and never saves the old one back", async () => {
    vi.mocked(api.startRecording).mockResolvedValue(undefined);
    render(App);
    await waitFor(() => expect(listeners.has("microphone-chosen")).toBe(true));
    // The backend has already written the choice to settings.json.
    listeners.get("microphone-chosen")!({ payload: "laptop-microphone" });
    await remoteIntent("toggle");
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledOnce());
    expect(vi.mocked(api.startRecording).mock.calls[0][0]).toBe("laptop-microphone");
    for (const [saved] of vi.mocked(api.saveSettings).mock.calls) expect(saved.input_device).toBe("laptop-microphone");
  });

  it("goes back to the system default when the tray says so", async () => {
    vi.mocked(api.getSettings).mockResolvedValue({ ...structuredClone(DEFAULT_SETTINGS), input_device: "headset" });
    vi.mocked(api.startRecording).mockResolvedValue(undefined);
    render(App);
    await waitFor(() => expect(listeners.has("microphone-chosen")).toBe(true));
    listeners.get("microphone-chosen")!({ payload: null });
    await remoteIntent("toggle");
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledOnce());
    expect(vi.mocked(api.startRecording).mock.calls[0][0]).toBeNull();
  });
});
