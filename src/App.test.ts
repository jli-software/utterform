// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/svelte";
import { DEFAULT_SETTINGS } from "./lib/types";
import type { HistoryEntry } from "./lib/types";
import App from "./App.svelte";
import { api } from "./lib/api";

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("./lib/api", () => ({ api: {
  getSettings: vi.fn(), listInputDevices: vi.fn(async () => []), listLocalModels: vi.fn(async () => []),
  hasOpenAiApiKey: vi.fn(async () => true), listHistory: vi.fn(), saveSettings: vi.fn(),
  startRecording: vi.fn(), finishRecording: vi.fn(), cancelRecording: vi.fn(),
  copyText: vi.fn(), clearHistory: vi.fn(),
} }));

const latest: HistoryEntry = { id: "2", title: "Plan for tomorrow", text: "Plan for tomorrow: write tests.", durationMs: 2400, engine: "open_ai" };
const older: HistoryEntry = { ...latest, id: "1", title: "Earlier idea", text: "An earlier idea worth keeping." };

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getSettings).mockResolvedValue(structuredClone(DEFAULT_SETTINGS));
  vi.mocked(api.listHistory).mockResolvedValue([latest, older]);
});
afterEach(cleanup);

describe("transcript history", () => {
  it("restores the latest text on mount and copies selected history with the shortcut", async () => {
    const view = render(App);
    await waitFor(() => expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(latest.text));
    await fireEvent.click(view.getByRole("button", { name: /Copy/ }));
    expect(api.copyText).toHaveBeenLastCalledWith(latest.text);
    await fireEvent.click(view.getByRole("combobox", { name: "Recent texts" }));
    await fireEvent.click(view.getByRole("option", { name: older.title }));
    await fireEvent.keyDown(window, { code: "KeyC", ctrlKey: true, shiftKey: true });
    expect(api.copyText).toHaveBeenLastCalledWith(older.text);
    view.unmount();
    const reopened = render(App);
    await waitFor(() => expect(reopened.getByRole("region", { name: "Transcript" }).textContent).toBe(latest.text));
  });

  it("keeps the previous text while recording, on focus loss and after cancellation", async () => {
    const view = render(App);
    await waitFor(() => expect(view.queryByText(latest.text)).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await fireEvent.blur(window);
    expect(api.cancelRecording).not.toHaveBeenCalled();
    expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(latest.text);
    await fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(api.cancelRecording).toHaveBeenCalledOnce());
    expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe(latest.text);
  });

  it("shows a new result even when history persistence failed", async () => {
    vi.mocked(api.finishRecording).mockResolvedValue({ historyEntry: null, text: "Unsaved but recoverable", durationMs: 1000, engine: "open_ai", savedPath: null, deliveryWarnings: ["History was not saved"] });
    const view = render(App);
    await waitFor(() => expect(view.queryByText(latest.text)).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(view.queryByRole("button", { name: "Stop recording" })).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Stop recording" }));
    await waitFor(() => expect(view.getByRole("region", { name: "Transcript" }).textContent).toBe("Unsaved but recoverable"));
    await fireEvent.keyDown(window, { code: "KeyC", metaKey: true, shiftKey: true });
    expect(api.copyText).toHaveBeenLastCalledWith("Unsaved but recoverable");
  });

  it("requires confirmation before clearing history", async () => {
    const view = render(App);
    await waitFor(() => expect(view.queryByText(latest.text)).not.toBeNull());
    await fireEvent.click(view.getByRole("button", { name: "Open settings" }));
    await fireEvent.click(view.getByRole("button", { name: "Clear saved history" }));
    expect(api.clearHistory).not.toHaveBeenCalled();
    await fireEvent.click(view.getByRole("button", { name: "Confirm: delete all saved texts" }));
    expect(api.clearHistory).toHaveBeenCalledOnce();
    await waitFor(() => expect(view.queryByText(latest.text)).toBeNull());
  });
});
