// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/svelte";
/// Just the queries these helpers use, so they take any render result.
type Screen = {
  getByRole: (role: string, options?: Record<string, unknown>) => HTMLElement;
  queryByRole: (role: string, options?: Record<string, unknown>) => HTMLElement | null;
};
import { DEFAULT_SETTINGS } from "./lib/types";
import type { BuiltInAction, HistoryEntry } from "./lib/types";
import App from "./App.svelte";
import { api } from "./lib/api";

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
  hasOpenAiApiKey: vi.fn(async () => true), listHistory: vi.fn(), saveSettings: vi.fn(),
  globalHotkeySupport: vi.fn(async () => ({ supported: true, default: "Ctrl+Alt+D", explanation: "", failure: null })),
  applyGlobalHotkey: vi.fn(),
  autostartEnabled: vi.fn(), enableAutostart: vi.fn(), disableAutostart: vi.fn(),
  startRecording: vi.fn(), finishRecording: vi.fn(), cancelRecording: vi.fn(),
  getRecordingStatus: vi.fn(), setRecordingPaused: vi.fn(),
  copyText: vi.fn(), clearHistory: vi.fn(),
  playTestCues: vi.fn(),
  diagnosticsInfo: vi.fn(), copyDiagnostics: vi.fn(), openLogFile: vi.fn(), openLogFolder: vi.fn(),
  reportFrontendError: vi.fn(),
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
  vi.mocked(api.liveSupport).mockResolvedValue({ supported: true, explanation: "" });
  vi.mocked(api.typingSupport).mockResolvedValue({ supported: true, explanation: "" });
  vi.mocked(api.getLiveStatus).mockResolvedValue(null);
  vi.mocked(api.getSettings).mockResolvedValue(structuredClone(DEFAULT_SETTINGS));
  vi.mocked(api.listBuiltInActions).mockResolvedValue(structuredClone(SHIPPED_ACTIONS));
  vi.mocked(api.listHistory).mockResolvedValue([latest, older]);
  vi.mocked(api.listLocalModels).mockResolvedValue([]);
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

/// Settings opens on General; everything else lives one tab away.
async function showSettingsTab(view: Screen, tab: string) {
  await waitFor(() => expect(view.queryByRole("dialog")).not.toBeNull());
  await fireEvent.click(view.getByRole("tab", { name: new RegExp(tab) }));
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
    await fireEvent.click(view.getByRole("button", { name: "Clear saved history" }));
    expect(api.clearHistory).not.toHaveBeenCalled();
    await fireEvent.click(view.getByRole("button", { name: "Confirm: delete all saved texts" }));
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
    await showSettingsTab(view, "General");
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

  it("takes a combination from the keyboard and registers exactly it", async () => {
    const view = await openSettings();
    const recorder = await listen(view);
    expect(recorder.textContent).toContain("Press shortcut");

    await fireEvent.keyDown(recorder, { code: "KeyK", key: "k", ctrlKey: true, altKey: true });
    expect(recorder.textContent).toContain("Ctrl+Alt+K");
    expect(recorder.getAttribute("aria-pressed")).toBe("false");
    // The old key is back in effect until Save; nothing may be left released.
    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenLastCalledWith("Ctrl+Alt+D"));

    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenLastCalledWith("Ctrl+Alt+K"));
    expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ global_hotkey: "Ctrl+Alt+K" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
  });

  it("reads the physical key, and shows a Mac the name it uses for it", async () => {
    onAMac();
    const view = await openSettings();
    const recorder = await listen(view);
    // A German keyboard types Z here and an American one Y; the combination
    // must be the same one, and Meta is stored as Super either way.
    await fireEvent.keyDown(recorder, { code: "KeyY", key: "z", metaKey: true, shiftKey: true });
    expect(recorder.textContent).toContain("Shift+Cmd+Y");

    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenLastCalledWith("Shift+Super+Y"));
  });

  it("reads space, a digit, a function key and an arrow", async () => {
    const view = await openSettings();
    for (const [code, shown] of [["Space", "Ctrl+Shift+Space"], ["Digit4", "Ctrl+Shift+4"],
      ["F9", "Ctrl+Shift+F9"], ["ArrowUp", "Ctrl+Shift+Up"]] as const) {
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

    await fireEvent.keyDown(recorder, { code: "KeyD", key: "d", ctrlKey: true, altKey: true });
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ global_hotkey: "Ctrl+Alt+D" }));
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

    // A second Escape is the dialog's again, as it always was.
    await fireEvent.keyDown(view.getByRole("dialog"), { key: "Escape" });
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
  });

  it("keeps the stored shortcut when Settings is cancelled", async () => {
    const view = await openSettings();
    const recorder = await listen(view);
    await fireEvent.keyDown(recorder, { code: "KeyJ", key: "j", ctrlKey: true, altKey: true });
    await fireEvent.click(view.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.saveSettings).not.toHaveBeenCalled();
    // Whatever was registered is registered still.
    expect(vi.mocked(api.applyGlobalHotkey).mock.calls.at(-1)).toEqual(["Ctrl+Alt+D"]);

    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "General");
    expect(view.getByRole("button", { name: /Shortcut/ }).textContent).toContain("Ctrl+Alt+D");
  });

  it("does not let the key it is listening for start a recording", async () => {
    const view = await openSettings();
    await listen(view);
    // Windows, macOS and X11 have released the key by now; a compositor
    // binding still arrives and is what this turns away.
    await remoteIntent("toggle");
    expect(api.startRecording).not.toHaveBeenCalled();
  });

  it("keeps the dialog open and names the problem when the key is taken", async () => {
    const view = await openSettings();
    const recorder = await listen(view);
    await fireEvent.keyDown(recorder, { code: "KeyK", key: "k", ctrlKey: true, altKey: true });
    vi.mocked(api.applyGlobalHotkey).mockRejectedValue("Ctrl+Alt+K is not available");
    await showSettingsTab(view, "Output");
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));

    await waitFor(() => expect(view.queryByRole("alert")).not.toBeNull());
    expect(view.getByRole("alert").textContent).toContain("not available");
    expect(view.getByRole("tab", { name: "General" }).getAttribute("aria-selected")).toBe("true");
    // The rest of the settings are stored; only the key needs another attempt.
    expect(api.saveSettings).toHaveBeenCalled();
    expect(view.queryByRole("dialog")).not.toBeNull();
  });

  it("leaves a working key alone when other settings change", async () => {
    const view = await openSettings();
    await showSettingsTab(view, "General");
    await fireEvent.click(view.getByRole("button", { name: "Dark" }));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));

    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.applyGlobalHotkey).not.toHaveBeenCalled();
  });

  it("turning the key off unregisters it", async () => {
    const view = await openSettings();
    await fireEvent.click(view.getByRole("checkbox", { name: /Enable global dictation shortcut/ }));
    expect(view.queryByRole("button", { name: /Shortcut/ })).toBeNull();
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));

    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenLastCalledWith(null));
  });

  it("reports a key that could not be reserved at startup, when Settings is first opened", async () => {
    vi.mocked(api.globalHotkeySupport).mockResolvedValue({
      supported: true, default: "Ctrl+Alt+D", explanation: "",
      failure: "Ctrl+Alt+D is not available — another application may already hold it",
    });
    const view = await openSettings();
    expect(view.getByRole("alert").textContent).toContain("not available");
    expect(view.getByRole("tab", { name: "General" }).getAttribute("aria-selected")).toBe("true");
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
    await showSettingsTab(view, "General");
    return view;
  }

  function toggleOf(view: Screen) {
    return view.getByRole("checkbox", { name: /when I sign in/ }) as HTMLInputElement;
  }

  it("shows what the system says rather than a stored setting", async () => {
    startupEntry = true;
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).checked).toBe(true));
    expect(api.autostartEnabled).toHaveBeenCalled();
    // Settings that reach the backend carry nothing about starting up.
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalled());
    expect(Object.keys(vi.mocked(api.saveSettings).mock.calls[0][0]).join()).not.toMatch(/autostart|login|startup/);
  });

  it("is off unless the system says otherwise", async () => {
    const view = await openStartup();
    await waitFor(() => expect(api.autostartEnabled).toHaveBeenCalled());
    expect(toggleOf(view).checked).toBe(false);
  });

  it("writes only on Save, and only what changed", async () => {
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).disabled).toBe(false));
    await fireEvent.click(toggleOf(view));
    expect(api.enableAutostart).not.toHaveBeenCalled();

    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.enableAutostart).toHaveBeenCalledOnce());
    expect(api.disableAutostart).not.toHaveBeenCalled();
    // Written, then read back: the entry is the truth, not the request.
    expect(api.autostartEnabled).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());

    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "General");
    await waitFor(() => expect(toggleOf(view).checked).toBe(true));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.enableAutostart).toHaveBeenCalledOnce();
  });

  it("changes nothing on the system when Settings is cancelled", async () => {
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).disabled).toBe(false));
    await fireEvent.click(toggleOf(view));
    await fireEvent.click(view.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.enableAutostart).not.toHaveBeenCalled();
    expect(api.disableAutostart).not.toHaveBeenCalled();

    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "General");
    await waitFor(() => expect(toggleOf(view).checked).toBe(false));
  });

  it("a system that will not answer costs the switch and nothing else", async () => {
    vi.mocked(api.autostartEnabled).mockRejectedValue("no startup items on this system");
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).disabled).toBe(true));
    expect(view.getByRole("alert").textContent).toContain("could not read");

    // Everything else still saves, and the dialog still closes.
    await fireEvent.click(view.getByRole("button", { name: "Dark" }));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.saveSettings).toHaveBeenCalled();
  });

  it("keeps a failed write in front of the user", async () => {
    vi.mocked(api.enableAutostart).mockRejectedValue("the startup folder is read-only");
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).disabled).toBe(false));
    await fireEvent.click(toggleOf(view));
    await showSettingsTab(view, "Actions");
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));

    await waitFor(() => expect(view.queryByRole("alert")).not.toBeNull());
    expect(view.getByRole("alert").textContent).toContain("read-only");
    expect(view.queryByRole("dialog")).not.toBeNull();
    expect(view.getByRole("tab", { name: "General" }).getAttribute("aria-selected")).toBe("true");
    // The rest of the settings were still stored.
    expect(api.saveSettings).toHaveBeenCalled();
  });

  it("believes the entry rather than the call that claimed to write it", async () => {
    // Reports success, changes nothing: without reading back, Settings would
    // close on a switch that never moved.
    vi.mocked(api.enableAutostart).mockResolvedValue(undefined);
    const view = await openStartup();
    await waitFor(() => expect(toggleOf(view).disabled).toBe(false));
    await fireEvent.click(toggleOf(view));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));

    await waitFor(() => expect(view.queryByRole("alert")).not.toBeNull());
    expect(view.getByRole("alert").textContent).toContain("could not be added");
    expect(view.queryByRole("dialog")).not.toBeNull();
    expect(toggleOf(view).checked).toBe(false);
  });
});

describe("typing at the cursor", () => {
  it("keeps the keystroke delay out of the way until keystrokes are chosen", async () => {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Output");

    // Paste is the default because it cannot drop or reorder characters.
    expect(view.queryByRole("spinbutton", { name: /Delay between keystrokes/ })).toBeNull();
    await fireEvent.click(view.getByRole("button", { name: "Keystrokes" }));
    const delay = view.getByRole("spinbutton", { name: /Delay between keystrokes/ }) as HTMLInputElement;
    expect(delay.value).toBe("15");

    await fireEvent.input(delay, { target: { value: "40" } });
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ typing_method: "keystrokes", typing_delay_ms: 40 }),
    ));
  });
});

describe("editing the prompts Utterform ships with", () => {
  async function openPrompts() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Actions");
    return view;
  }

  it("shows the shipped instructions, and Plain as the one with none", async () => {
    const view = await openPrompts();
    await fireEvent.click(view.getByRole("button", { name: /Clean/ }));
    expect((view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement).value)
      .toBe("Correct punctuation, capitalization, and spelling.");

    await fireEvent.click(view.getByRole("button", { name: /Plain/ }));
    expect(view.queryByRole("textbox", { name: "Prompt instructions" })).toBeNull();
    expect(view.queryByText(/no rewriting or text model/)).not.toBeNull();
  });

  it("saves a rewritten prompt, and offers the original alongside it", async () => {
    const view = await openPrompts();
    await fireEvent.click(view.getByRole("button", { name: /Email/ }));
    const editor = view.getByRole("textbox", { name: "Prompt instructions" });
    await fireEvent.input(editor, { target: { value: "Answer in two sentences." } });

    await fireEvent.click(view.getByRole("button", { name: "View the original" }));
    expect(view.queryByText("Write the transcript as an email.")).not.toBeNull();

    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({
      action_overrides: { email: { prompt: "Answer in two sentences." } },
    })));
  });

  it("keeps an emptied box empty to type in, without storing an edit", async () => {
    const view = await openPrompts();
    await fireEvent.click(view.getByRole("button", { name: /Clean/ }));
    const editor = view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement;
    await fireEvent.input(editor, { target: { value: "" } });

    expect(editor.value).toBe("");
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
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
    await fireEvent.click(view.getByRole("button", { name: /Tidy/ }));
    expect((view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement).value)
      .toBe("Fix commas only.");

    await fireEvent.click(view.getByRole("button", { name: "Reset" }));
    expect((view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement).value)
      .toBe("Correct punctuation, capitalization, and spelling.");
    expect(view.queryByRole("button", { name: /Tidy/ })).toBeNull();

    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ action_overrides: {} }),
    ));
  });

  it("offers a renamed action under its new name, and Cancel puts it back", async () => {
    const view = await openPrompts();
    await fireEvent.click(view.getByRole("button", { name: /Email/ }));
    await fireEvent.input(view.getByRole("textbox", { name: "Prompt name" }), { target: { value: "Reply" } });
    await fireEvent.click(view.getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    await fireEvent.click(view.getByRole("combobox", { name: "Action" }));
    await waitFor(() => expect(view.queryByRole("option", { name: /Email/ })).not.toBeNull());
    expect(view.queryByRole("option", { name: /Reply/ })).toBeNull();
  });

  it("chooses an added prompt straight away, and drops it again on delete", async () => {
    const view = await openPrompts();
    await fireEvent.click(view.getByRole("button", { name: "Add prompt" }));
    const editor = view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement;
    expect(editor.value).toBe("");
    await fireEvent.input(view.getByRole("textbox", { name: "Prompt name" }), { target: { value: "Standup" } });
    await fireEvent.input(editor, { target: { value: "Three bullets." } });

    await fireEvent.click(view.getByRole("button", { name: "Delete prompt" }));
    expect(view.queryByRole("button", { name: /Standup/ })).toBeNull();
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ custom_actions: [] }),
    ));
  });

  it("sends a chosen reasoning level, and nothing at all on Auto", async () => {
    const view = await openPrompts();
    await showSettingsTab(view, "AI & Models");
    await fireEvent.click(view.getByRole("button", { name: "Low" }));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ text_effort: "low" }),
    ));

    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "AI & Models");
    await fireEvent.click(view.getByRole("button", { name: "Auto" }));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ text_effort: null }),
    ));
  });
});

describe("vocabulary", () => {
  async function openVoice() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Recording");
    return view;
  }

  it("keeps one term per line, and drops the blank ones", async () => {
    const view = await openVoice();
    await fireEvent.input(view.getByRole("textbox", { name: "Vocabulary" }), {
      target: { value: "Careum\n\n  Utterform  \n" },
    });
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ vocabulary: ["Careum", "Utterform"] }),
    ));
  });

  it("names the term the transcription API would refuse, rather than losing the recording to it", async () => {
    const view = await openVoice();
    await fireEvent.input(view.getByRole("textbox", { name: "Vocabulary" }), {
      target: { value: "Careum\n<tagged>" },
    });
    expect(view.getByRole("alert").textContent).toContain("<tagged>");
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
    await showSettingsTab(view, "AI & Models");
    expect(view.getByRole("combobox", { name: "Transcription mode" }).textContent).toContain("GPT Transcribe");
    await fireEvent.click(view.getByRole("combobox", { name: "Transcription mode" }));
    await fireEvent.click(view.getByRole("option", { name: /GPT Live Transcribe/ }));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ cloud_model: "gpt_live_transcribe" })));
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
    await showSettingsTab(view, "AI & Models");
    await fireEvent.click(view.getByRole("combobox", { name: "Transcription mode" }));
    await fireEvent.click(view.getByRole("option", { name: /^GPT Transcribe / }));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    expect(view.queryByRole("combobox", { name: "Action" })).not.toBeNull();
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
    await showSettingsTab(view, "AI & Models");
    expect(view.getByRole("combobox", { name: "Transcription mode" }).textContent).toContain("GPT Transcribe");
    await fireEvent.click(view.getByRole("button", { name: "Cancel" }));
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

  it("shows the same choice as Settings, in both directions, and Cancel there keeps it", async () => {
    const view = await renderReady();
    await choose(view, /^Local Whisper /);
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledOnce());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "AI & Models");
    expect(view.getByRole("combobox", { name: "Transcription mode" }).textContent).toContain("Local Whisper");
    await fireEvent.click(view.getByRole("combobox", { name: "Transcription mode" }));
    await fireEvent.click(view.getByRole("option", { name: /^GPT Transcribe / }));
    await fireEvent.click(view.getByRole("button", { name: "Cancel" }));
    expect(selector(view).textContent).toContain("Local Whisper");
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "AI & Models");
    await fireEvent.click(view.getByRole("combobox", { name: "Transcription mode" }));
    await fireEvent.click(view.getByRole("option", { name: /^GPT Transcribe / }));
    await fireEvent.click(view.getByRole("combobox", { name: "Transcription mode" }));
    await fireEvent.click(view.getByRole("option", { name: /GPT Live Transcribe/ }));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
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
    expect(api.startRecording).toHaveBeenCalledTimes(2);
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

  it("does not stop a recording that is still starting", async () => {
    let started!: () => void;
    vi.mocked(api.startRecording).mockImplementationOnce(() => new Promise<void>((done) => started = done));
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Start recording" })).not.toBeNull());
    await remoteIntent("toggle");
    await waitFor(() => expect(api.startRecording).toHaveBeenCalledOnce());
    await remoteIntent("toggle");
    await remoteIntent("stop");
    started();
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    expect(api.finishRecording).not.toHaveBeenCalled();
    expect(api.startRecording).toHaveBeenCalledOnce();
  });
});

// Support actions are immediate resource operations, like a model download:
// they neither take part in the settings draft nor close the dialog.
describe("support and diagnostics", () => {
  async function openGeneral() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "General");
    await waitFor(() => expect(view.queryByText(LOG_PATH)).not.toBeNull());
    return view;
  }

  it("ends General with the support group and the path the backend reported", async () => {
    const view = await openGeneral();
    const headings = view.getByRole("tabpanel").querySelectorAll("h3");
    expect(headings[headings.length - 1].textContent).toBe("Support & diagnostics");
    expect(view.getByText(/contain no transcripts or audio/)).toBeTruthy();
    for (const name of ["Copy diagnostics", "Open log file", "Open log folder"]) {
      expect((view.getByRole("button", { name }) as HTMLButtonElement).disabled).toBe(false);
    }
  });

  it("runs each action at once, never saves, and leaves Cancel with nothing to undo", async () => {
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
    await fireEvent.click(view.getByRole("button", { name: "Cancel" }));
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
    // The path stays readable for opening it by hand.
    expect(view.queryByText(LOG_PATH)).not.toBeNull();
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
    await showSettingsTab(view, "General");
    await waitFor(() => expect(view.queryByText("not available in this session")).not.toBeNull());
  });

  it("leaves the sound test and its report in Recording, without the log path", async () => {
    const view = await openGeneral();
    await showSettingsTab(view, "Recording");
    expect(view.queryByText(LOG_PATH)).toBeNull();
    expect(view.queryByRole("button", { name: "Copy diagnostics" })).toBeNull();
    await fireEvent.click(view.getByRole("button", { name: "Test sounds (5s delay)" }));
    expect(api.playTestCues).toHaveBeenCalledWith(5);
    await waitFor(() => expect(view.queryByRole("button", { name: "Playing in 5 seconds…" })).not.toBeNull());
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
    await fireEvent.click(view.getByRole("button", { name: "Browse" }));
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
    await showSettingsTab(view, "General");
    await waitFor(() => expect(view.getByRole("alert").textContent).toBe(
      "Utterform could not read your startup settings: no startup items on this system (ref 0a1b2c3d-9)",
    ));
    expect(api.reportFrontendError).toHaveBeenCalledWith({
      source: "autostart", message: "no startup items on this system", stack: null, phase: "idle",
    });
  });
});
