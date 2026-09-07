// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/svelte";
import { DEFAULT_SETTINGS } from "./lib/types";
import type { HistoryEntry } from "./lib/types";
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
  hasOpenAiApiKey: vi.fn(async () => true), listHistory: vi.fn(), saveSettings: vi.fn(),
  globalHotkeySupport: vi.fn(async () => ({ supported: true, default: "Ctrl+Alt+D", explanation: "" })),
  applyGlobalHotkey: vi.fn(),
  startRecording: vi.fn(), finishRecording: vi.fn(), cancelRecording: vi.fn(),
  getRecordingStatus: vi.fn(), setRecordingPaused: vi.fn(),
  copyText: vi.fn(), clearHistory: vi.fn(),
} }));

const latest: HistoryEntry = { id: "2", createdAtMs: new Date(2026, 8, 6, 14, 30).getTime(), title: "Plan for tomorrow", text: "Plan for tomorrow: write tests.", durationMs: 2400, engine: "open_ai" };
const older: HistoryEntry = { ...latest, createdAtMs: new Date(2026, 8, 4, 9, 15).getTime(), id: "1", title: "Earlier idea", text: "An earlier idea worth keeping." };

beforeEach(() => {
  vi.clearAllMocks();
  listeners.clear();
  vi.mocked(api.copyText).mockReset().mockResolvedValue(undefined);
  vi.mocked(api.getSettings).mockResolvedValue(structuredClone(DEFAULT_SETTINGS));
  vi.mocked(api.listHistory).mockResolvedValue([latest, older]);
  let paused = false;
  const status = () => ({ recording: true, paused, limitReached: false, elapsedSeconds: 12, level: paused ? 0 : .3 });
  vi.mocked(api.getRecordingStatus).mockImplementation(async () => status());
  vi.mocked(api.setRecordingPaused).mockImplementation(async (value) => { paused = value; return status(); });
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

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

describe("dictation key settings", () => {
  async function openSettings() {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await waitFor(() => expect(view.queryByRole("dialog")).not.toBeNull());
    return view;
  }

  it("registers a changed shortcut and closes", async () => {
    const view = await openSettings();
    const field = view.getByRole("textbox", { name: /Shortcut/ });
    await fireEvent.input(field, { target: { value: "Ctrl+Alt+K" } });
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));

    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenCalledWith("Ctrl+Alt+K"));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
  });

  it("keeps the dialog open and names the problem when the key is taken", async () => {
    vi.mocked(api.applyGlobalHotkey).mockRejectedValue("Ctrl+Alt+K is not available");
    const view = await openSettings();
    await fireEvent.input(view.getByRole("textbox", { name: /Shortcut/ }), { target: { value: "Ctrl+Alt+K" } });
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));

    await waitFor(() => expect(view.queryByRole("alert")).not.toBeNull());
    expect(view.getByRole("alert").textContent).toContain("not available");
    // The rest of the settings are stored; only the key needs another attempt.
    expect(api.saveSettings).toHaveBeenCalled();
    expect(view.queryByRole("dialog")).not.toBeNull();
  });

  it("leaves a working key alone when other settings change", async () => {
    const view = await openSettings();
    await fireEvent.click(view.getByRole("button", { name: "Dark" }));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));

    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(api.applyGlobalHotkey).not.toHaveBeenCalled();
  });

  it("turning the key off unregisters it", async () => {
    const view = await openSettings();
    await fireEvent.click(view.getByRole("checkbox", { name: /without raising the window/ }));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));

    await waitFor(() => expect(api.applyGlobalHotkey).toHaveBeenCalledWith(null));
  });

  it("offers a Wayland session the command line instead of a dead field", async () => {
    vi.mocked(api.globalHotkeySupport).mockResolvedValue({
      supported: false, default: "Ctrl+Alt+D",
      explanation: "Wayland gives no application a global shortcut. Bind `utterform --toggle` in your compositor instead.",
    });
    const view = await openSettings();
    await waitFor(() => expect(view.queryByText(/utterform --toggle/)).not.toBeNull());
    expect(view.queryByRole("textbox", { name: /Shortcut/ })).toBeNull();
  });
});

describe("typing at the cursor", () => {
  it("keeps the keystroke delay out of the way until keystrokes are chosen", async () => {
    const view = render(App);
    await waitFor(() => expect(view.queryByRole("button", { name: "Open settings" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));

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
