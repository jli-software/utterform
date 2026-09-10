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
  startRecording: vi.fn(), finishRecording: vi.fn(), cancelRecording: vi.fn(),
  getRecordingStatus: vi.fn(), setRecordingPaused: vi.fn(),
  copyText: vi.fn(), clearHistory: vi.fn(),
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
  let paused = false;
  const status = () => ({ recording: true, paused, limitReached: false, elapsedSeconds: 12, level: paused ? 0 : .3 });
  vi.mocked(api.getRecordingStatus).mockImplementation(async () => status());
  vi.mocked(api.setRecordingPaused).mockImplementation(async (value) => { paused = value; return status(); });
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); });

/// Settings opens on Voice; everything else lives one tab away.
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
    await showSettingsTab(view, "General");
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
    await showSettingsTab(view, "Output");
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
    await showSettingsTab(view, "General");
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

  it("reports a key that could not be reserved at startup, when Settings is first opened", async () => {
    vi.mocked(api.globalHotkeySupport).mockResolvedValue({
      supported: true, default: "Ctrl+Alt+D", explanation: "",
      failure: "Ctrl+Alt+D is not available — another application may already hold it",
    });
    const view = await openSettings();
    expect(view.getByRole("alert").textContent).toContain("not available");
  });

  it("offers a Wayland session the command line instead of a dead field", async () => {
    vi.mocked(api.globalHotkeySupport).mockResolvedValue({
      supported: false, default: "Ctrl+Alt+D", failure: null,
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
    await showSettingsTab(view, "Prompts");
    return view;
  }

  it("shows the shipped instructions, and Plain as the one with none", async () => {
    const view = await openPrompts();
    await fireEvent.click(view.getByRole("button", { name: /Clean/ }));
    expect((view.getByRole("textbox", { name: "Prompt instructions" }) as HTMLTextAreaElement).value)
      .toBe("Correct punctuation, capitalization, and spelling.");

    await fireEvent.click(view.getByRole("button", { name: /Plain/ }));
    expect(view.queryByRole("textbox", { name: "Prompt instructions" })).toBeNull();
    expect(view.queryByText(/never reaches a text model/)).not.toBeNull();
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
    await fireEvent.click(view.getByRole("button", { name: "Low" }));
    await fireEvent.click(view.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ text_effort: "low" }),
    ));

    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await showSettingsTab(view, "Prompts");
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
    await showSettingsTab(view, "Voice");
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
    expect(view.getByRole("combobox", { name: "Cloud transcription model" }).textContent).toContain("GPT Transcribe");
    await fireEvent.click(view.getByRole("combobox", { name: "Cloud transcription model" }));
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
    await fireEvent.click(view.getByRole("combobox", { name: "Cloud transcription model" }));
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
